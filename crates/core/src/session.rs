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

/// Puts the installer of a verified download into `<dir>\\<id>\\` (a folder
/// only administrators can change) and locks it there. The bytes come from
/// the locked download itself, copied or unzipped, never from a file in the
/// shared cache that someone could have changed after verification.
#[cfg(windows)]
fn stage(src: &crate::catalog::Source, locked: &File, installer: &Path, dir: &Path, id: &str) -> std::io::Result<(PathBuf, File)> {
    use sha2::{Digest, Sha256};
    use std::io::{Seek, SeekFrom, Write};

    /// Hashes what passes through, to compare the copy on disk afterwards.
    struct Hashing<W: Write> {
        inner: W,
        hasher: Sha256,
    }
    impl<W: Write> Write for Hashing<W> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let n = self.inner.write(buf)?;
            self.hasher.update(&buf[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.inner.flush()
        }
    }

    let pkg_dir = dir.join(id);
    std::fs::create_dir(&pkg_dir)?;
    let dest = pkg_dir.join(installer.file_name().unwrap_or(std::ffi::OsStr::new("installer.exe")));
    let mut reader = locked;
    reader.seek(SeekFrom::Start(0))?;
    let mut out = Hashing { inner: std::io::BufWriter::new(File::create_new(&dest)?), hasher: Sha256::new() };
    match &src.extract {
        None => {
            std::io::copy(&mut reader, &mut out)?;
        }
        Some(x) => fetch::unzip_entry(reader, &x.entry, &mut out).map_err(std::io::Error::other)?,
    }
    // Close the write handle before locking: the lock allows readers only,
    // so an open writer (even our own) makes it fail with a sharing violation.
    let Hashing { inner, hasher } = out;
    drop(inner.into_inner().map_err(std::io::IntoInnerError::into_error)?);
    let expected: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    let lock = lock_for_run(&dest)?;
    let (_, actual) = fetch::sha256_file(&dest)?;
    if actual != expected {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "the copied installer does not match"));
    }
    Ok((dest, lock))
}

/// The session's log folder. Installing runs elevated, so the folder is
/// created new with fixed permissions (administrators write, users read)
/// instead of reusing a folder that may already exist; any user can create
/// folders under %ProgramData%, including one with the next timestamp.
#[cfg(windows)]
fn create_log_dir(root: &Path, elevated: bool) -> PathBuf {
    use crate::secure_dir::{self, Readers};
    let _ = std::fs::create_dir_all(root);
    let name = timestamp();
    let plain = root.join(&name);
    if !elevated {
        let _ = std::fs::create_dir_all(&plain);
        return plain;
    }
    match secure_dir::create(&plain, Readers::Users) {
        Ok(()) => plain,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            secure_dir::create_unique(root, &format!("{name}-"), Readers::Users).unwrap_or(plain)
        }
        Err(_) => {
            let _ = std::fs::create_dir_all(&plain);
            plain
        }
    }
}

type FetchResult = Result<Fetched, String>;

/// Downloads every package that has a source, `jobs` at a time, in plan order.
fn start_downloads<'a>(
    scope: &'a std::thread::Scope<'a, '_>,
    packages: Vec<&'a Package>,
    cache: &'a Cache,
    jobs: usize,
    // Running from an offline package: take files from it only, never the network.
    offline: bool,
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
                let r = if offline {
                    fetch::from_cache(pkg, cache)
                } else {
                    fetch::fetch(&client, pkg, cache, cancel, &mut progress)
                }
                .map_err(|e| e.to_string());
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

    let log_dir = create_log_dir(&opts.log_root, !opts.download_only);
    let mut summary = Summary { log_dir: log_dir.display().to_string(), ..Summary::default() };
    let by_id: HashMap<&str, &Package> = catalog.packages().map(|p| (p.id.as_str(), p)).collect();
    let items: Vec<&Package> = plan.install.iter().filter_map(|i| by_id.get(i.id.as_str()).copied()).collect();

    emit(Event::Started { total: items.len(), download_only: opts.download_only, log_dir: summary.log_dir.clone() });
    for s in &plan.skipped {
        emit(Event::Skipped { id: s.id.clone(), reason: serde_json::to_string(&s.reason).unwrap_or_default() });
    }

    // An offline package next to (or inside) this exe serves as the cache, and
    // it is the only source: such an exe never downloads anything.
    let bundle = crate::bundle::current();
    let offline = bundle.is_some();
    let cache_dir = match bundle {
        Some(b) => {
            let dir = crate::bundle::cache_dir(&b, &opts.cache_dir);
            let ids: Vec<String> = items.iter().map(|p| p.id.clone()).collect();
            match crate::bundle::prepare(&b, &ids, &dir) {
                Ok(p) if !p.damaged.is_empty() => emit(Event::Failed {
                    id: "offline-package".into(),
                    error: format!("damaged in the offline package: {}", p.damaged.join(", ")),
                }),
                Ok(_) => {}
                Err(e) => emit(Event::Failed { id: "offline-package".into(), error: e.to_string() }),
            }
            dir
        }
        None => opts.cache_dir.clone(),
    };
    let cache = Cache::new(&cache_dir);
    // Installers run from a fresh folder only administrators can change; the
    // DirectX redist unpacks there too. Without it nothing with a file is
    // installed: running from the shared cache is what it protects against.
    let staging: Result<PathBuf, String> = if opts.download_only {
        Err("download only".into())
    } else {
        crate::secure_dir::create_unique(&crate::secure_dir::system_temp(), "CAIOR-", crate::secure_dir::Readers::AdminsOnly)
            .map_err(|e| format!("cannot create a private folder for the installers: {e}"))
    };
    let ctx = RunContext {
        log_dir: log_dir.clone(),
        work_dir: staging.clone().unwrap_or_else(|_| opts.cache_dir.join("_work")),
        system_root: PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())),
    };

    std::thread::scope(|scope| {
        let downloadable: Vec<&Package> = items.iter().copied().filter(|p| p.source.is_some()).collect();
        let rx = start_downloads(scope, downloadable, &cache, opts.download_jobs, offline, cancel, emit);
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
                } else {
                    // Windows features come from Windows Update; there is nothing to download.
                    emit(Event::Skipped { id: pkg.id.clone(), reason: "windows-feature".into() });
                    summary.skipped += 1;
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

            // Lock the download, put its installer into the private folder
            // through that locked handle, then verify again with the copy as
            // the installer: the file that passed is the file that runs.
            let mut staged: Option<(PathBuf, File)> = None;
            if let Some(f) = &fetched {
                let src = pkg.source.as_ref().unwrap();
                let ready = lock_for_run(&f.download)
                    .map_err(|e| format!("cannot lock {}: {e}", f.download.display()))
                    .and_then(|download_lock| {
                        let dir = staging.as_ref().map_err(Clone::clone)?;
                        let (path, lock) = stage(src, &download_lock, &f.installer, dir, &pkg.id)
                            .map_err(|e| format!("cannot copy the installer to {}: {e}", dir.display()))?;
                        fetch::verify(src, &f.download, &path).map_err(|e| e.to_string())?;
                        Ok((path, lock))
                    });
                match ready {
                    Ok(s) => staged = Some(s),
                    Err(e) => {
                        emit(Event::Failed { id: pkg.id.clone(), error: e });
                        summary.failed += 1;
                        failed.insert(pkg.id.clone());
                        continue;
                    }
                }
            }

            emit(Event::InstallStarted { id: pkg.id.clone(), index: index + 1, total });
            let result = run_package(pkg, staged.as_ref().map(|(p, _)| p.as_path()), &ctx, RetryPolicy::default(), cancel);
            if let (Some((_, lock)), Ok(dir)) = (staged.take(), &staging) {
                drop(lock);
                let _ = std::fs::remove_dir_all(dir.join(&pkg.id));
            }

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

    if let Ok(dir) = &staging {
        let _ = std::fs::remove_dir_all(dir);
    }
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

    #[cfg(windows)]
    #[test]
    fn staging_copies_or_unzips_the_locked_download() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("caior-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut src: crate::catalog::Source =
            toml::from_str("url = \"https://example.com/a.exe\"\nfilename = \"a.exe\"").unwrap();

        let run = dir.join("run");
        std::fs::create_dir_all(&run).unwrap();
        let plain = dir.join("a.exe");
        std::fs::write(&plain, b"plain installer").unwrap();
        let lock = lock_for_run(&plain).unwrap();
        let (staged, _staged_lock) = stage(&src, &lock, &plain, &run, "p").unwrap();
        assert_eq!(std::fs::read(&staged).unwrap(), b"plain installer");
        assert!(std::fs::write(&plain, b"swapped").is_err(), "the download stays locked");
        drop(lock);

        let zip_path = dir.join("deps.zip");
        let mut zip = zip::ZipWriter::new(File::create(&zip_path).unwrap());
        zip.start_file("x64/Inner.appx", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"inner package").unwrap();
        zip.finish().unwrap();
        src.extract = toml::from_str("archive = \"zip\"\nentry = \"x64/inner.appx\"").ok();
        let lock = lock_for_run(&zip_path).unwrap();
        let (staged, _l) = stage(&src, &lock, &dir.join("Inner.appx"), &run, "q").unwrap();
        assert_eq!(std::fs::read(&staged).unwrap(), b"inner package");
        assert_eq!(staged, run.join("q").join("Inner.appx"));
        drop(lock);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole install path on a real Windows: cached download, lock, copy
    /// into the private folder, verify again, run, clean up. The "installer"
    /// is a copy of cmd.exe with a pinned hash. Needs administrator rights
    /// (GitHub's Windows runners have them); skipped otherwise.
    #[cfg(windows)]
    #[test]
    fn install_runs_a_verified_copy_and_cleans_up() {
        use crate::catalog::parse_category;
        use crate::plan::{PlanOptions, build};

        let sys = crate::detect::SystemInfo::current();
        if !sys.elevated {
            eprintln!("skipped: needs administrator rights");
            return;
        }
        let base = std::env::temp_dir().join(format!("caior-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let cmd = PathBuf::from(std::env::var("SystemRoot").unwrap()).join(r"System32\cmd.exe");
        let cached = base.join("cache").join("fake").join("fake.exe");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::copy(&cmd, &cached).unwrap();
        let (size, sha) = fetch::sha256_file(&cached).unwrap();
        let toml = format!(
            "[category]\nid = \"t\"\nname = {{ en = \"T\" }}\norder = 1\nicon = \"x\"\n\n[[package]]\nid = \"fake\"\nname = \"Fake\"\nversion = \"1\"\narch = \"neutral\"\norder = 1\n[package.source]\nurl = \"https://example.invalid/fake.exe\"\nfilename = \"fake.exe\"\nsize = {size}\nsha256 = \"{sha}\"\n[package.install]\nkind = \"exe\"\nargs = '/c exit 3010'\ntimeout_sec = 60\n[package.detect]\nkind = \"none\"\n"
        );
        let catalog = Catalog { categories: vec![parse_category("t.toml", &toml).unwrap()] };
        let plan = build(&catalog, &sys, &HashMap::new(), &["fake".into()], PlanOptions::default()).unwrap();
        let opts = SessionOptions {
            cache_dir: base.join("cache"),
            log_root: base.join("logs"),
            download_jobs: 1,
            download_only: false,
        };
        let events = std::sync::Mutex::new(Vec::new());
        let summary = run(&catalog, &plan, &opts, &AtomicBool::new(false), &|e| events.lock().unwrap().push(e));
        let events = events.into_inner().unwrap();
        assert_eq!((summary.installed, summary.failed, summary.reboot_required), (1, 0, true), "{events:?}");
        assert!(events.iter().any(|e| matches!(e, Event::InstallFinished { outcome, .. } if outcome == "reboot")));
        let log = std::fs::read_to_string(Path::new(&summary.log_dir).join("fake.log")).unwrap();
        // The program that ran is the copy in %SystemRoot%\Temp\CAIOR-*, not the cache.
        let ran = log.lines().next().unwrap_or_default().to_ascii_lowercase();
        let temp = crate::secure_dir::system_temp().join("CAIOR-").display().to_string().to_ascii_lowercase();
        assert!(ran.contains(&temp), "{log}");
        let staged = ran.split('"').nth(1).map(PathBuf::from).unwrap();
        assert!(!staged.exists() && !staged.parent().unwrap().parent().unwrap().exists(), "private folder removed");
        assert!(cached.is_file(), "the cache keeps its copy");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn exit_codes() {
        assert_eq!(Summary::default().exit_code(), 0);
        assert_eq!(Summary { reboot_required: true, ..Default::default() }.exit_code(), 3010);
        assert_eq!(Summary { failed: 1, reboot_required: true, ..Default::default() }.exit_code(), 1);
    }
}
