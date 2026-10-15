//! `caior install` and the hidden `caior worker` it starts elevated.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use anyhow::{Result, bail};
use caior_core::catalog::{Catalog, Profile};
use caior_core::detect::{self, State, Status, SystemInfo};
use caior_core::plan::{self, Plan, PlanOptions, SkipReason};
use caior_core::session::{self, Event, SessionOptions, Summary};
use clap::{Args, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ProfileArg {
    Recommended,
    Gaming,
    Developer,
    Legacy,
    All,
}

/// What to install. Shared by `install` and `worker`, so the worker can rebuild
/// the same plan from its own catalog instead of trusting the caller.
#[derive(Debug, Clone, Args)]
pub struct SelectionArgs {
    /// Preselect a profile. Repeatable.
    #[arg(long = "profile", value_enum)]
    pub profiles: Vec<ProfileArg>,
    /// Add packages by id; a trailing * matches a prefix. Repeatable.
    #[arg(long = "id")]
    pub ids: Vec<String>,
    /// Leave packages out by id or prefix*. Repeatable.
    #[arg(long = "exclude")]
    pub excludes: Vec<String>,
    /// Install even what is already installed.
    #[arg(long)]
    pub reinstall: bool,
    /// Download and verify only.
    #[arg(long)]
    pub download_only: bool,
    /// Folder for downloaded installers.
    #[arg(long)]
    pub cache: Option<PathBuf>,
    /// Folder for session logs.
    #[arg(long)]
    pub logs: Option<PathBuf>,
    /// Parallel downloads.
    #[arg(long, default_value_t = 3)]
    pub jobs: usize,
}

impl SelectionArgs {
    /// The same selection as command line arguments, for the worker.
    fn to_args(&self) -> Vec<String> {
        let mut a = Vec::new();
        for p in &self.profiles {
            a.push("--profile".into());
            a.push(p.to_possible_value().unwrap().get_name().to_string());
        }
        for (flag, values) in [("--id", &self.ids), ("--exclude", &self.excludes)] {
            for v in values {
                a.push(flag.into());
                a.push(v.clone());
            }
        }
        if self.reinstall {
            a.push("--reinstall".into());
        }
        if self.download_only {
            a.push("--download-only".into());
        }
        for (flag, value) in [("--cache", &self.cache), ("--logs", &self.logs)] {
            if let Some(v) = value {
                a.push(flag.into());
                a.push(v.display().to_string());
            }
        }
        a.push("--jobs".into());
        a.push(self.jobs.to_string());
        a
    }

    fn data_dir() -> PathBuf {
        PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into())).join("CAIOR")
    }

    fn session_options(&self) -> SessionOptions {
        SessionOptions {
            cache_dir: self.cache.clone().unwrap_or_else(|| Self::data_dir().join("cache")),
            log_root: self.logs.clone().unwrap_or_else(|| Self::data_dir().join("logs")),
            download_jobs: self.jobs,
            download_only: self.download_only,
        }
    }
}

/// A mistake in the command line (exit code 2), as opposed to a failed run.
#[derive(Debug)]
pub struct Usage(pub String);

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Usage {}

fn matches(patterns: &[String], id: &str) -> bool {
    patterns.iter().any(|pat| match pat.strip_suffix('*') {
        Some(prefix) => id.starts_with(prefix),
        None => pat == id,
    })
}

/// Resolves profiles, `--id` and `--exclude` into package ids. Profiles may
/// hold both sides of a conflict ("all" does); for an install one side is
/// kept, and a package named with `--id` wins over the profile's pick.
/// Packages named with `--id` that conflict with each other stay, so the
/// plan asks the user to choose. Downloads keep everything.
fn select_ids(catalog: &Catalog, sys: &SystemInfo, sel: &SelectionArgs) -> Result<Vec<String>> {
    let mut picked: Vec<String> = Vec::new();
    for p in &sel.profiles {
        picked.extend(match p {
            ProfileArg::Recommended => plan::profile_ids(catalog, Profile::Recommended, sys),
            ProfileArg::Gaming => plan::profile_ids(catalog, Profile::Gaming, sys),
            ProfileArg::Developer => plan::profile_ids(catalog, Profile::Developer, sys),
            ProfileArg::Legacy => plan::profile_ids(catalog, Profile::Legacy, sys),
            ProfileArg::All => plan::all_ids(catalog, sys),
        });
    }
    picked.retain(|id| !matches(&sel.excludes, id));
    let named: Vec<String> = catalog.packages().filter(|p| matches(&sel.ids, &p.id)).map(|p| p.id.clone()).collect();
    if !sel.ids.is_empty() && named.is_empty() {
        return Err(Usage(format!("no package matches {:?}. Run `caior list` to see the ids.", sel.ids)).into());
    }
    if !sel.download_only {
        picked = plan::resolve_conflicts(catalog, sys, picked);
        picked.retain(|id| catalog.package(id).is_none_or(|p| !p.conflicts.iter().any(|c| named.contains(c))));
    }
    let mut seen = HashSet::new();
    let mut ids: Vec<String> = picked.into_iter().chain(named).collect();
    ids.retain(|id| !matches(&sel.excludes, id) && seen.insert(id.clone()));
    if ids.is_empty() {
        return Err(Usage("nothing selected. Use --profile or --id.".into()).into());
    }
    Ok(ids)
}

/// Detects, resolves the selection and builds the plan.
fn make_plan(catalog: &Catalog, sys: &SystemInfo, sel: &SelectionArgs) -> Result<Plan> {
    let ids = select_ids(catalog, sys, sel)?;

    let probe = detect::WindowsProbe::new();
    let states: HashMap<String, State> = detect::detect_all(catalog, sys, &probe)
        .into_iter()
        .filter_map(|s| match s.state {
            Status::Detected(st) => Some((s.id, st)),
            Status::NotApplicable { .. } => None,
        })
        .collect();
    let opts = PlanOptions { reinstall: sel.reinstall, download_only: sel.download_only };
    plan::build(catalog, sys, &states, &ids, opts).map_err(|e| Usage(e.to_string()).into())
}

fn print_plan(catalog: &Catalog, plan: &Plan, download_only: bool) {
    let name = |id: &str| catalog.package(id).map(|p| p.name.clone()).unwrap_or_else(|| id.to_string());
    let mb: u64 = plan
        .install
        .iter()
        .filter_map(|i| catalog.package(&i.id).and_then(|p| p.source.as_ref()).map(|s| s.size))
        .sum::<u64>()
        / 1_048_576;
    let verb = if download_only { "To download" } else { "To install" };
    println!("{verb} ({}, about {mb} MB to download):", plan.install.len());
    for i in &plan.install {
        let state = match &i.state {
            State::Installed { .. } => "installed".into(),
            State::Outdated { version, wanted } => format!("update {version} -> {wanted}"),
            State::Unknown { .. } => "state unknown".into(),
            State::Missing => "missing".into(),
        };
        println!("  {:<34} {:<52} {state}", i.id, name(&i.id));
    }
    let count = |f: fn(&SkipReason) -> bool| plan.skipped.iter().filter(|s| f(&s.reason)).count();
    let installed = count(|r| matches!(r, SkipReason::AlreadyInstalled { .. }));
    let blocked = count(|r| matches!(r, SkipReason::ConflictInstalled { .. }));
    let other = plan.skipped.len() - installed - blocked;
    let blocked = if blocked > 0 { format!(", {blocked} cannot sit next to an installed package") } else { String::new() };
    println!("Skipped: {installed} already installed{blocked}, {other} not for this PC.");
}

/// Prints events as plain lines. Download progress is shown in quarter steps.
struct Printer {
    names: HashMap<String, String>,
    quarters: Mutex<HashMap<String, u64>>,
}

impl Printer {
    fn new(catalog: &Catalog) -> Self {
        Printer {
            names: catalog.packages().map(|p| (p.id.clone(), p.name.clone())).collect(),
            quarters: Mutex::new(HashMap::new()),
        }
    }

    fn name<'a>(&'a self, id: &'a str) -> &'a str {
        self.names.get(id).map(String::as_str).unwrap_or(id)
    }

    fn print(&self, e: &Event) {
        match e {
            Event::Started { total, download_only, log_dir } => {
                println!("{} {total} package(s). Logs: {log_dir}", if *download_only { "Downloading" } else { "Installing" })
            }
            Event::Downloading { id, done, total: Some(t) } if *t > 0 => {
                let q = done * 4 / t;
                let mut seen = self.quarters.lock().unwrap();
                if seen.get(id) != Some(&q) && q < 4 {
                    seen.insert(id.clone(), q);
                    println!("  download {:<34} {:>3}%", id, q * 25);
                }
            }
            Event::Downloading { .. } => {}
            Event::Downloaded { id, size, from_cache } => println!(
                "  ready    {:<34} {:.1} MB{}",
                id,
                *size as f64 / 1_048_576.0,
                if *from_cache { " (cache)" } else { "" }
            ),
            Event::InstallStarted { id, index, total } => println!("[{index}/{total}] Installing {}...", self.name(id)),
            Event::InstallFinished { id, outcome, code, seconds, not_detected } => {
                let text = match (outcome.as_str(), code) {
                    ("success", _) => "done".to_string(),
                    ("reboot", _) => "done, restart needed".to_string(),
                    ("timed-out", _) => "Error: the installer did not finish in time".to_string(),
                    ("failed", Some(0)) if *not_detected => {
                        "Error: the installer reported success, but the package is not installed".to_string()
                    }
                    (_, Some(c)) => format!("Error: the installer failed with code {c}"),
                    _ => "Error: the installer could not start".to_string(),
                };
                let warn = if *not_detected && outcome == "reboot" { " (not detected yet; check again after the restart)" } else { "" };
                println!("       {} - {text} in {seconds}s{warn}", self.name(id));
            }
            Event::Failed { id, error } => println!("  Error: {}: {error}", self.name(id)),
            Event::Skipped { .. } => {}
            Event::Finished(s) => print_summary(s),
        }
    }
}

fn print_summary(s: &Summary) {
    println!(
        "\nFinished: {} installed, {} downloaded, {} failed, {} skipped{}.",
        s.installed,
        s.downloaded,
        s.failed,
        s.skipped,
        if s.cancelled { ", cancelled" } else { "" }
    );
    if s.reboot_required {
        println!("Restart Windows to finish the installation.");
    }
    println!("Logs: {}", s.log_dir);
}

fn confirm() -> bool {
    print!("Continue? [Y/n] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).is_ok() && !line.trim().eq_ignore_ascii_case("n")
}

fn unique_pipe_name() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("caior-{}-{nanos:08x}", std::process::id())
}

pub fn install(catalog: &Catalog, sel: &SelectionArgs, yes: bool, dry_run: bool) -> Result<i32> {
    let sys = SystemInfo::current();
    let elevating = !sel.download_only && !sys.elevated;
    if elevating && worker_paths_refused() && (sel.cache.is_some() || sel.logs.is_some()) {
        return Err(Usage(
            "--cache and --logs need --download-only or an administrator terminal. \
             The installer that runs as administrator always uses %ProgramData%\\CAIOR."
                .into(),
        )
        .into());
    }
    let plan = make_plan(catalog, &sys, sel)?;
    print_plan(catalog, &plan, sel.download_only);
    if sys.reboot_pending {
        println!("Note: Windows is waiting for a restart. Some installers may fail until you restart.");
    }
    if plan.install.is_empty() {
        println!("Nothing to do.");
        return Ok(0);
    }
    if dry_run {
        return Ok(0);
    }
    if !yes && !confirm() {
        return Ok(0);
    }

    let printer = Printer::new(catalog);
    if sel.download_only || sys.elevated {
        let cancel = AtomicBool::new(false);
        let summary = session::run(catalog, &plan, &sel.session_options(), &cancel, &|e| printer.print(&e));
        return Ok(summary.exit_code());
    }

    println!("Waiting for administrator approval...");
    let pipe = unique_pipe_name();
    let mut args = vec!["worker".to_string(), "--pipe".into(), pipe.clone()];
    args.extend(sel.to_args());
    let mut finished = false;
    let code = caior_core::elevate::run_elevated(&pipe, &args, |line| match serde_json::from_str::<Event>(line) {
        Ok(e) => {
            finished |= matches!(e, Event::Finished(_));
            printer.print(&e);
        }
        Err(_) => println!("{line}"),
    });
    match code {
        Ok(c) => {
            if !finished {
                println!("Error: the installer stopped unexpectedly (exit code {c}). See the logs in {}.", sel.session_options().log_root.display());
            }
            Ok(c)
        }
        Err(caior_core::elevate::ElevateError::Declined) => {
            println!("Error: administrator approval was declined. Nothing was installed.");
            Ok(caior_core::elevate::UAC_DECLINED)
        }
        Err(e) => bail!("{e}"),
    }
}

/// The elevated worker writes only below %ProgramData%\CAIOR: in release
/// builds it refuses folders chosen by the unelevated caller.
fn worker_paths_refused() -> bool {
    !cfg!(debug_assertions)
}

/// The elevated side. Rebuilds the plan from the built-in catalog and streams
/// events into the caller's pipe.
pub fn worker(catalog: &Catalog, pipe: &str, cancel_file: Option<&std::path::Path>, sel: &SelectionArgs) -> Result<i32> {
    if worker_paths_refused() && (sel.cache.is_some() || sel.logs.is_some()) {
        bail!("the worker does not accept --cache or --logs");
    }
    let sys = SystemInfo::current();
    let out = Mutex::new(caior_core::elevate::connect(pipe)?);
    let send = |e: &Event| {
        if let Ok(mut f) = out.lock() {
            let _ = serde_json::to_writer(&mut *f, e);
            let _ = f.write_all(b"\n");
            let _ = f.flush();
        }
    };
    if !sys.elevated && !sel.download_only {
        send(&Event::Failed { id: "worker".into(), error: "the worker is not running as administrator".into() });
        return Ok(5);
    }
    let plan = match make_plan(catalog, &sys, sel) {
        Ok(p) => p,
        Err(e) => {
            send(&Event::Failed { id: "worker".into(), error: e.to_string() });
            return Ok(1);
        }
    };
    // The unelevated caller asks to cancel by creating this file; the pipe only goes one way.
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    if let Some(path) = cancel_file.map(std::path::Path::to_path_buf) {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                if path.exists() {
                    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        });
    }
    let summary = session::run(catalog, &plan, &sel.session_options(), &cancel, &|e| send(&e));
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(summary.exit_code())
}

/// `caior pack`: download and verify the selection, then write an offline
/// package. Packages already installed on this PC are included too: the
/// package is meant for other PCs.
pub fn pack(catalog: &Catalog, sel: &SelectionArgs, out: &std::path::Path, folder: bool) -> Result<i32> {
    let sys = SystemInfo::current();
    let mut sel = sel.clone();
    sel.reinstall = true;
    sel.download_only = true;
    let plan = make_plan(catalog, &sys, &sel)?;
    if plan.install.is_empty() {
        println!("Nothing to pack.");
        return Ok(0);
    }
    print_plan(catalog, &plan, true);
    let printer = Printer::new(catalog);
    let opts = sel.session_options();
    let cancel = AtomicBool::new(false);
    let summary = session::run(catalog, &plan, &opts, &cancel, &|e| printer.print(&e));
    if summary.failed > 0 {
        println!("Error: {} package(s) could not be downloaded; the offline package was not written.", summary.failed);
        return Ok(1);
    }
    let ids: Vec<String> = plan.install.iter().map(|i| i.id.clone()).collect();
    let cache = caior_core::fetch::Cache::new(caior_core::bundle::effective_cache(&opts.cache_dir));
    let format = if folder { caior_core::bundle::Format::Folder } else { caior_core::bundle::Format::Exe };
    let (count, exe) = caior_core::bundle::pack(catalog, &ids, &cache, format, out)?;
    let size = if folder { String::new() } else { format!(", {} MB", std::fs::metadata(&exe).map(|m| m.len()).unwrap_or(0) / 1_048_576) };
    println!("Offline package written: {} ({count} packages{size})", exe.display());
    Ok(0)
}
