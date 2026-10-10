//! One install (or download-only) run: parallel downloads, sequential installs,
//! a re-check after each install, and a stream of events for the CLI and GUI.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, Package};
use crate::fetch::{self, Cache, Fetched};
use crate::plan::Plan;

#[derive(Debug, Clone)]
pub struct SessionOptions {
    pub cache_dir: PathBuf,
    /// A `<timestamp>` folder is created inside for this run's logs.
    pub log_root: PathBuf,
    pub download_jobs: usize,
    /// Fetch and verify only; nothing is installed.
    pub download_only: bool,
}

/// What happened, in order. Serialized as one JSON object per line between the
/// elevated worker and the process that shows progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    Started { total: usize, download_only: bool, log_dir: String },
    Downloading { id: String, done: u64, total: Option<u64> },
    Downloaded { id: String, size: u64, from_cache: bool },
    InstallStarted { id: String, index: usize, total: usize },
    /// `outcome` is "success", "reboot", "failed", "timed-out" or "launch-failed".
    InstallFinished {
        id: String,
        outcome: String,
        code: Option<i64>,
        seconds: u64,
        /// Detection after the install still says it is missing.
        not_detected: bool,
    },
    Failed { id: String, error: String },
    Skipped { id: String, reason: String },
    Finished(Summary),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub installed: usize,
    pub downloaded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub reboot_required: bool,
    pub cancelled: bool,
    pub log_dir: String,
}

impl Summary {
    /// Process exit code: 0 ok, 3010 ok + restart needed, 1 something failed.
    pub fn exit_code(&self) -> i32 {
        match (self.failed > 0 || self.cancelled, self.reboot_required) {
            (true, _) => 1,
            (false, true) => 3010,
            (false, false) => 0,
        }
    }
}

/// `YYYYMMDD-HHMMSS` in UTC, for folder names.
pub fn timestamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}-{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Opens a file so nobody can change, rename or delete it until the handle is
/// dropped. Held from verification until the installer has finished, so a
/// verified file cannot be swapped before it runs.
#[cfg(windows)]
fn lock_for_run(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(path)
}

type FetchResult = Result<Fetched, String>;

/// Downloads every package that has a source, `jobs` at a time, in plan order.
fn start_downloads<'a>(
    scope: &'a std::thread::Scope<'a, '_>,
    packages: Vec<&'a Package>,
    cache: &'a Cache,
    jobs: usize,
    cancel: &'a AtomicBool,
    emit: &'a (dyn Fn(Event) + Sync),
) -> mpsc::Receiver<(String, FetchResult)> {
    let (tx, rx) = mpsc::channel();
    let next = std::sync::Arc::new(AtomicUsize::new(0));
    let packages = std::sync::Arc::new(packages);
    for _ in 0..jobs.clamp(1, 8) {
        let tx = tx.clone();
        let next = next.clone();
        let packages = packages.clone();
        scope.spawn(move || {
            let client = match fetch::http_client() {
                Ok(c) => c,
                Err(e) => {
                    for p in packages.iter() {
                        let _ = tx.send((p.id.clone(), Err(format!("cannot start the downloader: {e}"))));
                    }
                    return;
                }
            };
            loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(pkg) = packages.get(i) else { break };
                if cancel.load(Ordering::Relaxed) {
                    let _ = tx.send((pkg.id.clone(), Err("cancelled".into())));
                    continue;
                }
                let mut last = Instant::now() - Duration::from_secs(1);
                let mut progress = |done: u64, total: Option<u64>| {
                    if last.elapsed() >= Duration::from_millis(300) || total == Some(done) {
                        last = Instant::now();
                        emit(Event::Downloading { id: pkg.id.clone(), done, total });
                    }
                };
                let r = fetch::fetch(&client, pkg, cache, cancel, &mut progress).map_err(|e| e.to_string());
                if let Ok(f) = &r {
                    emit(Event::Downloaded { id: pkg.id.clone(), size: f.verified.size, from_cache: f.from_cache });
                }
                let _ = tx.send((pkg.id.clone(), r));
            }
        });
    }
    rx
}

fn outcome_fields(o: &crate::install::Outcome) -> (String, Option<i64>) {
    use crate::install::Outcome;
    match o {
        Outcome::Success => ("success".into(), None),
        Outcome::Reboot => ("reboot".into(), None),
        Outcome::Failed { code } => ("failed".into(), Some(*code)),
        Outcome::TimedOut => ("timed-out".into(), None),
        Outcome::LaunchFailed { .. } => ("launch-failed".into(), None),
    }
}

/// Runs the plan. Installing needs administrator rights; download-only does not.
#[cfg(windows)]
pub fn run(catalog: &Catalog, plan: &Plan, opts: &SessionOptions, cancel: &AtomicBool, emit: &(dyn Fn(Event) + Sync)) -> Summary {
    use crate::detect::{State, WindowsProbe, evaluate};
    use crate::install::{RetryPolicy, RunContext, run_package};

    let log_dir = opts.log_root.join(timestamp());
    let _ = std::fs::create_dir_all(&log_dir);
    let mut summary = Summary { log_dir: log_dir.display().to_string(), ..Summary::default() };
    let by_id: HashMap<&str, &Package> = catalog.packages().map(|p| (p.id.as_str(), p)).collect();
    let items: Vec<&Package> = plan.install.iter().filter_map(|i| by_id.get(i.id.as_str()).copied()).collect();

    emit(Event::Started { total: items.len(), download_only: opts.download_only, log_dir: summary.log_dir.clone() });
    for s in &plan.skipped {
        emit(Event::Skipped { id: s.id.clone(), reason: serde_json::to_string(&s.reason).unwrap_or_default() });
    }

    let cache = Cache::new(&opts.cache_dir);
    let ctx = RunContext {
        log_dir: log_dir.clone(),
        work_dir: opts.cache_dir.join("_work"),
        system_root: PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())),
    };

    std::thread::scope(|scope| {
        let downloadable: Vec<&Package> = items.iter().copied().filter(|p| p.source.is_some()).collect();
        let rx = start_downloads(scope, downloadable, &cache, opts.download_jobs, cancel, emit);
        let mut ready: HashMap<String, FetchResult> = HashMap::new();
        let mut wait_for = |id: &str| -> FetchResult {
            while !ready.contains_key(id) {
                match rx.recv() {
                    Ok((k, v)) => {
                        ready.insert(k, v);
                    }
                    Err(_) => return Err("download worker stopped".into()),
                }
            }
            ready.remove(id).unwrap()
        };

        let mut failed: HashSet<String> = HashSet::new();
        let total = items.len();
        for (index, pkg) in items.iter().enumerate() {
            let fetched = if pkg.source.is_some() {
                match wait_for(&pkg.id) {
                    Ok(f) => Some(f),
                    Err(e) => {
                        if e != "cancelled" {
                            emit(Event::Failed { id: pkg.id.clone(), error: e });
                            summary.failed += 1;
                        }
                        failed.insert(pkg.id.clone());
                        continue;
                    }
                }
            } else {
                None
            };
            if opts.download_only {
                if fetched.is_some() {
                    summary.downloaded += 1;
                }
                continue;
            }
            if cancel.load(Ordering::Relaxed) {
                summary.cancelled = true;
                emit(Event::Skipped { id: pkg.id.clone(), reason: "cancelled".into() });
                summary.skipped += 1;
                continue;
            }
            if let Some(dep) = pkg.depends.iter().find(|d| failed.contains(*d)) {
                emit(Event::Skipped { id: pkg.id.clone(), reason: format!("dependency {dep} failed") });
                summary.skipped += 1;
                failed.insert(pkg.id.clone());
                continue;
            }

            // Lock, then verify again: the file that passed is the file that runs.
            let mut locks = Vec::new();
            if let Some(f) = &fetched {
                let src = pkg.source.as_ref().unwrap();
                let locked = [&f.download, &f.installer]
                    .into_iter()
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .map(|p| lock_for_run(p).map_err(|e| format!("cannot lock {}: {e}", p.display())))
                    .collect::<Result<Vec<_>, _>>()
                    .and_then(|l| {
                        locks = l;
                        fetch::verify(src, &f.download, &f.installer).map_err(|e| e.to_string())
                    });
                if let Err(e) = locked {
                    emit(Event::Failed { id: pkg.id.clone(), error: e });
                    summary.failed += 1;
                    failed.insert(pkg.id.clone());
                    continue;
                }
            }

            emit(Event::InstallStarted { id: pkg.id.clone(), index: index + 1, total });
            let result = run_package(pkg, fetched.as_ref().map(|f| f.installer.as_path()), &ctx, RetryPolicy::default(), cancel);
            drop(locks);

            let not_detected = result.outcome.is_success()
                && matches!(evaluate(&pkg.detect, &WindowsProbe::new()), State::Missing);
            // Some wrappers (IExpress around msiexec, for one) exit 0 even when
            // the inner install rolled back. A clean "success" with the product
            // still missing is a failure; after a reboot code the product may
            // legitimately appear only once Windows restarts, so that stays a warning.
            let outcome_value = match (&result.outcome, not_detected) {
                (crate::install::Outcome::Success, true) => crate::install::Outcome::Failed { code: 0 },
                (o, _) => o.clone(),
            };
            let (outcome, code) = outcome_fields(&outcome_value);
            let result = crate::install::PackageResult { outcome: outcome_value, ..result };
            if result.outcome.is_success() {
                summary.installed += 1;
                summary.reboot_required |= result.outcome == crate::install::Outcome::Reboot;
            } else {
                summary.failed += 1;
                failed.insert(pkg.id.clone());
            }
            emit(Event::InstallFinished { id: pkg.id.clone(), outcome, code, seconds: result.seconds, not_detected });
        }
    });

    summary.skipped += plan.skipped.len();
    if cancel.load(Ordering::Relaxed) {
        summary.cancelled = true;
    }
    emit(Event::Finished(summary.clone()));
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_shape() {
        let t = timestamp();
        assert_eq!(t.len(), 15, "{t}");
        assert_eq!(&t[8..9], "-");
        assert!(t.starts_with("20"));
    }

    #[test]
    fn events_round_trip_as_json_lines() {
        let e = Event::InstallFinished {
            id: "vc-2013-x64".into(),
            outcome: "reboot".into(),
            code: None,
            seconds: 12,
            not_detected: false,
        };
        let line = serde_json::to_string(&e).unwrap();
        assert!(line.starts_with("{\"event\":\"install-finished\""), "{line}");
        assert_eq!(serde_json::from_str::<Event>(&line).unwrap(), e);
        let f = Event::Finished(Summary { installed: 3, ..Default::default() });
        assert_eq!(serde_json::from_str::<Event>(&serde_json::to_string(&f).unwrap()).unwrap(), f);
    }

    #[test]
    fn exit_codes() {
        assert_eq!(Summary::default().exit_code(), 0);
        assert_eq!(Summary { reboot_required: true, ..Default::default() }.exit_code(), 3010);
        assert_eq!(Summary { failed: 1, reboot_required: true, ..Default::default() }.exit_code(), 1);
    }
}
