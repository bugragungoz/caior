//! CAIOR entry point. GUI, CLI and the elevated worker share this one exe.
//! It is a GUI-subsystem program: double-clicking it opens the window without
//! a console; command-line use attaches to the console of the calling shell.

#![cfg_attr(windows, windows_subsystem = "windows")]
//! The GUI and the install engine come in later phases; for now the CLI can
//! list the catalog, detect what is installed, and fetch installers.

mod gui;
mod install_cmd;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use caior_core::catalog::{self, Catalog};
use caior_core::detect::{self, PackageStatus, State, Status, SystemInfo};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "caior", version, about = "CAIOR - croxz all-in-one runtimes")]
struct Cli {
    /// Read the catalog from this folder instead of the one built into the exe.
    #[arg(long, global = true)]
    catalog: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// List the catalog packages.
    List,
    /// Show which packages are installed on this PC.
    Detect {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
        /// Also list packages that do not apply to this PC.
        #[arg(long)]
        all: bool,
    },
    /// Download and verify installers into the cache without installing them.
    Fetch {
        /// Package ids; a trailing * matches a prefix. Repeatable.
        #[arg(long = "id", required = true)]
        ids: Vec<String>,
        #[arg(long, default_value = "cache")]
        cache: PathBuf,
    },
    /// Install packages silently. Asks for administrator approval once.
    Install {
        #[command(flatten)]
        sel: install_cmd::SelectionArgs,
        /// Do not ask before starting.
        #[arg(long, short = 'y')]
        yes: bool,
        /// Same as --yes; for unattended use.
        #[arg(long)]
        silent: bool,
        /// Show the plan and stop.
        #[arg(long)]
        dry_run: bool,
    },
    /// Internal: the elevated half of `install`.
    #[command(hide = true)]
    Worker {
        #[arg(long)]
        pipe: String,
        /// The worker stops after the current installer once this file exists.
        #[arg(long)]
        cancel_file: Option<PathBuf>,
        #[command(flatten)]
        sel: install_cmd::SelectionArgs,
    },
}

fn load_catalog(dir: &Option<PathBuf>) -> Result<Catalog> {
    match dir {
        Some(dir) => {
            let (catalog, errors) = catalog::load_dir(dir)?;
            if let Some(e) = errors.first() {
                bail!("Error: the catalog could not be read: {e}");
            }
            Ok(catalog)
        }
        None => catalog::embedded().context("Error: the built-in catalog could not be read"),
    }
}

fn state_label(s: &PackageStatus) -> (String, String) {
    match &s.state {
        Status::Detected(State::Installed { version }) => (
            "installed".into(),
            version.as_ref().map(ToString::to_string).unwrap_or_default(),
        ),
        Status::Detected(State::Outdated { version, wanted }) => ("outdated".into(), format!("{version} < {wanted}")),
        Status::Detected(State::Missing) => ("missing".into(), String::new()),
        Status::Detected(State::Unknown { reason }) => ("unknown".into(), reason.clone()),
        Status::NotApplicable { why, .. } => ("n/a".into(), format!("{why:?}")),
    }
}

fn detect_cmd(catalog: &Catalog, json: bool, all: bool) -> Result<()> {
    let sys = SystemInfo::current();
    let probe = detect::WindowsProbe::new();
    let mut results = detect::detect_all(catalog, &sys, &probe);
    if !all {
        results.retain(|r| !matches!(r.state, Status::NotApplicable { .. }));
    }
    if json {
        let out = serde_json::json!({ "system": sys, "packages": results });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    println!(
        "Windows {} ({}), build {}, {:?}, edition {}{}",
        sys.display_version,
        sys.product_name,
        sys.os_version,
        sys.os_arch,
        sys.edition_id,
        if sys.elevated { ", administrator" } else { "" }
    );
    let mut category = String::new();
    for r in &results {
        if r.category != category {
            category = r.category.clone();
            println!("\n[{category}]");
        }
        let (state, detail) = state_label(r);
        println!("  {:<10} {:<34} {detail}", state, r.id);
    }
    let count = |label: &str| results.iter().filter(|r| state_label(r).0 == label).count();
    println!(
        "\n{} installed, {} outdated, {} missing, {} unknown",
        count("installed"),
        count("outdated"),
        count("missing"),
        count("unknown")
    );
    Ok(())
}

fn id_matches(patterns: &[String], id: &str) -> bool {
    patterns.iter().any(|pat| match pat.strip_suffix('*') {
        Some(prefix) => id.starts_with(prefix),
        None => pat == id,
    })
}

fn fetch_cmd(catalog: &Catalog, ids: &[String], cache: &Path) -> Result<()> {
    use caior_core::fetch;
    use std::sync::atomic::AtomicBool;

    let client = fetch::http_client()?;
    let cache = fetch::Cache::new(cache);
    let cancel = AtomicBool::new(false);
    let mut failed = 0;
    for pkg in catalog.packages().filter(|p| id_matches(ids, &p.id) && p.source.is_some()) {
        let mut last_step = u64::MAX;
        let mut progress = |done: u64, total: Option<u64>| {
            if let Some(t) = total.filter(|t| *t > 0) {
                let step = done * 10 / t;
                if step != last_step {
                    last_step = step;
                    eprintln!("  {:<34} {:>3}%", pkg.id, step * 10);
                }
            }
        };
        match fetch::fetch(&client, pkg, &cache, &cancel, &mut progress) {
            Ok(f) => {
                let signer = match &f.verified.signature {
                    Some(s) => format!(
                        "{} ({})",
                        s.signer.as_deref().unwrap_or("?"),
                        if s.valid { "valid" } else { "not valid" }
                    ),
                    None => "hash only".into(),
                };
                println!(
                    "ok    {:<34} {}{:.1} MB, sha256 {}..., signer {signer}",
                    pkg.id,
                    if f.from_cache { "(cache) " } else { "" },
                    f.verified.size as f64 / 1_048_576.0,
                    &f.verified.sha256[..12]
                );
            }
            Err(e) => {
                failed += 1;
                println!("FAIL  {:<34} Error: {e}", pkg.id);
            }
        }
    }
    if failed > 0 {
        bail!("{failed} package(s) could not be fetched");
    }
    Ok(())
}

fn main() -> Result<()> {
    // The worker runs hidden; every other command line use prints to the caller's console.
    let args: Vec<String> = std::env::args().collect();
    let is_worker = args.get(1).is_some_and(|a| a == "worker");
    if args.len() > 1 && !is_worker {
        caior_core::desktop::attach_parent_console();
    }
    let cli = Cli::parse();
    let catalog = load_catalog(&cli.catalog)?;
    match cli.command {
        None => gui::run(catalog),
        Some(Command::Detect { json, all }) => detect_cmd(&catalog, json, all),
        Some(Command::Fetch { ids, cache }) => fetch_cmd(&catalog, &ids, &cache),
        Some(Command::Install { sel, yes, silent, dry_run }) => {
            let code = install_cmd::install(&catalog, &sel, yes || silent, dry_run)?;
            std::process::exit(code)
        }
        Some(Command::Worker { pipe, cancel_file, sel }) => {
            // The elevated worker only trusts the catalog compiled into it.
            if cli.catalog.is_some() && !cfg!(debug_assertions) {
                bail!("Error: the worker does not accept --catalog");
            }
            let code = install_cmd::worker(&catalog, &pipe, cancel_file.as_deref(), &sel)?;
            std::process::exit(code)
        }
        Some(Command::List) => {
            for c in &catalog.categories {
                println!("[{}] {}", c.category.id, c.category.name.en);
                for p in &c.packages {
                    println!("  {:<34} {} ({:?})", p.id, p.name, p.arch);
                }
            }
            Ok(())
        }
    }
}
