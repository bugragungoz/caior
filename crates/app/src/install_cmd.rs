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

fn matches(patterns: &[String], id: &str) -> bool {
    patterns.iter().any(|pat| match pat.strip_suffix('*') {
        Some(prefix) => id.starts_with(prefix),
        None => pat == id,
    })
}

/// Detects, resolves the selection and builds the plan.
fn make_plan(catalog: &Catalog, sys: &SystemInfo, sel: &SelectionArgs) -> Result<Plan> {
    let mut ids: Vec<String> = Vec::new();
    for p in &sel.profiles {
        let picked = match p {
            ProfileArg::Recommended => plan::profile_ids(catalog, Profile::Recommended, sys),
            ProfileArg::Gaming => plan::profile_ids(catalog, Profile::Gaming, sys),
            ProfileArg::Developer => plan::profile_ids(catalog, Profile::Developer, sys),
            ProfileArg::Legacy => plan::profile_ids(catalog, Profile::Legacy, sys),
            ProfileArg::All => plan::all_ids(catalog, sys),
        };
        ids.extend(picked);
    }
    if !sel.ids.is_empty() {
        let before = ids.len();
        ids.extend(catalog.packages().filter(|p| matches(&sel.ids, &p.id)).map(|p| p.id.clone()));
        if ids.len() == before {
            bail!("Error: no package matches {:?}. Run `caior list` to see the ids.", sel.ids);
        }
    }
    let mut seen = HashSet::new();
    ids.retain(|id| !matches(&sel.excludes, id) && seen.insert(id.clone()));
    if ids.is_empty() {
        bail!("Error: nothing selected. Use --profile or --id.");
    }

    let probe = detect::WindowsProbe::new();
    let states: HashMap<String, State> = detect::detect_all(catalog, sys, &probe)
        .into_iter()
        .filter_map(|s| match s.state {
            Status::Detected(st) => Some((s.id, st)),
            Status::NotApplicable { .. } => None,
        })
        .collect();
    Ok(plan::build(catalog, sys, &states, &ids, PlanOptions { reinstall: sel.reinstall })?)
}

fn print_plan(catalog: &Catalog, plan: &Plan) {
    let name = |id: &str| catalog.package(id).map(|p| p.name.clone()).unwrap_or_else(|| id.to_string());
    let mb: u64 = plan
        .install
        .iter()
        .filter_map(|i| catalog.package(&i.id).and_then(|p| p.source.as_ref()).map(|s| s.size))
        .sum::<u64>()
        / 1_048_576;
    println!("To install ({}, about {mb} MB to download):", plan.install.len());
    for i in &plan.install {
        let state = match &i.state {
            State::Outdated { version, wanted } => format!("update {version} -> {wanted}"),
            State::Unknown { .. } => "state unknown".into(),
            _ => "missing".into(),
        };
        println!("  {:<34} {:<52} {state}", i.id, name(&i.id));
    }
    let installed = plan.skipped.iter().filter(|s| matches!(s.reason, SkipReason::AlreadyInstalled { .. })).count();
    let other = plan.skipped.len() - installed;
    println!("Skipped: {installed} already installed, {other} not for this PC.");
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
    let plan = make_plan(catalog, &sys, sel)?;
    print_plan(catalog, &plan);
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
        Err(e) => bail!("Error: {e}"),
    }
}

/// The elevated side. Rebuilds the plan from the built-in catalog and streams
/// events into the caller's pipe.
pub fn worker(catalog: &Catalog, pipe: &str, sel: &SelectionArgs) -> Result<i32> {
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
    let cancel = AtomicBool::new(false);
    let summary = session::run(catalog, &plan, &sel.session_options(), &cancel, &|e| send(&e));
    Ok(summary.exit_code())
}
