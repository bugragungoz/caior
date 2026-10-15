//! Starts installer processes inside a job object, enforces the timeout, and
//! retries when Windows Installer is busy with another installation.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject};
use windows::core::PCWSTR;

use super::command::{Invocation, RunContext, invocations};
use super::{ERROR_INSTALL_ALREADY_RUNNING, Outcome, PackageResult, classify, normalize_code};
use crate::catalog::Package;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// ERROR_TIMEOUT, used as the exit code of a killed installer.
const TIMEOUT_EXIT: u32 = 1460;

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Extra attempts when the installer reports 1618 (another install running).
    pub busy_retries: u32,
    pub busy_wait: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy { busy_retries: 3, busy_wait: Duration::from_secs(30) }
    }
}

struct Job(HANDLE);

impl Job {
    fn new() -> Option<Job> {
        // SAFETY: an anonymous job with default security.
        unsafe { CreateJobObjectW(None, PCWSTR::null()).ok().map(Job) }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateJobObjectW. Closing it does not kill
        // the processes (no KILL_ON_JOB_CLOSE): an installer must never be cut
        // off because CAIOR itself exits.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

enum Exit {
    Code(i64),
    TimedOut,
}

fn run_one(inv: &Invocation, timeout: Duration) -> std::io::Result<Exit> {
    if let Some(dir) = inv.output_log.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut log = OpenOptions::new().create(true).append(true).open(&inv.output_log)?;
    writeln!(log, "> \"{}\" {}", inv.program.display(), inv.args)?;
    let mut child = Command::new(&inv.program)
        .raw_arg(&inv.args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log.try_clone()?)
        .spawn()?;

    let job = Job::new();
    if let Some(job) = &job {
        // SAFETY: both handles are valid while `child` is alive. Children the
        // installer starts from now on join the job too.
        unsafe {
            let _ = AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle()));
        }
    }

    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let code = status.code().map(normalize_code).unwrap_or(-1);
            writeln!(log, "< exit {code} after {}s", start.elapsed().as_secs())?;
            return Ok(Exit::Code(code));
        }
        // A cancel request never interrupts a running installer (see run_package);
        // the timeout is the only hard stop.
        if start.elapsed() >= timeout {
            match &job {
                // SAFETY: valid job handle; ends the installer and its children.
                Some(job) => unsafe {
                    let _ = TerminateJobObject(job.0, TIMEOUT_EXIT);
                },
                None => {
                    let _ = child.kill();
                }
            }
            let _ = child.wait();
            writeln!(log, "< killed after the {}s timeout", timeout.as_secs())?;
            return Ok(Exit::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Installs one package: runs its invocations in order and classifies the result.
/// Every invocation but the last must exit 0 (DirectX extraction, for one).
pub fn run_package(
    pkg: &Package,
    installer: Option<&Path>,
    ctx: &RunContext,
    retry: RetryPolicy,
    cancel: &AtomicBool,
) -> PackageResult {
    let started = Instant::now();
    let mut result = PackageResult {
        id: pkg.id.clone(),
        outcome: Outcome::Success,
        attempts: 0,
        seconds: 0,
        logs: Vec::new(),
    };
    let invs = match invocations(pkg, installer, ctx) {
        Ok(i) => i,
        Err(e) => {
            result.outcome = Outcome::LaunchFailed { message: e.to_string() };
            return result;
        }
    };
    let _ = fs::create_dir_all(&ctx.work_dir);
    let timeout = Duration::from_secs(u64::from(pkg.install.timeout_sec));
    let last = invs.len() - 1;

    'steps: for (n, inv) in invs.iter().enumerate() {
        result.logs.push(inv.output_log.clone());
        if let Some(l) = &inv.tool_log {
            result.logs.push(l.clone());
        }
        let mut busy_left = retry.busy_retries;
        loop {
            result.attempts += 1;
            let exit = match run_one(inv, timeout) {
                Ok(e) => e,
                Err(e) => {
                    result.outcome = Outcome::LaunchFailed { message: format!("{}: {e}", inv.program.display()) };
                    break 'steps;
                }
            };
            let code = match exit {
                Exit::TimedOut => {
                    result.outcome = Outcome::TimedOut;
                    break 'steps;
                }
                Exit::Code(c) => c,
            };
            if code == ERROR_INSTALL_ALREADY_RUNNING && busy_left > 0 && !cancel.load(Ordering::Relaxed) {
                busy_left -= 1;
                // Wait in small steps: a cancel during the wait gives up the retry.
                let until = Instant::now() + retry.busy_wait;
                while Instant::now() < until && !cancel.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(250).min(retry.busy_wait));
                }
                if !cancel.load(Ordering::Relaxed) {
                    continue;
                }
            }
            if n < last {
                if code != 0 {
                    result.outcome = Outcome::Failed { code };
                    break 'steps;
                }
                break;
            }
            result.outcome = classify(code, &pkg.install);
            break;
        }
    }

    // DirectX leaves its extracted redist behind; nothing else uses work_dir per package.
    let _ = fs::remove_dir_all(ctx.work_dir.join(format!("{}-redist", pkg.id)));
    result.seconds = started.elapsed().as_secs();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::parse_category;
    use std::path::PathBuf;

    fn exe_pkg(args: &str, timeout: u32) -> Package {
        let text = format!(
            "[category]\nid = \"t\"\nname = {{ en = \"T\" }}\norder = 1\nicon = \"x\"\n\n[[package]]\nid = \"p\"\nname = \"P\"\nversion = \"1\"\narch = \"x64\"\norder = 1\n[package.source]\nurl = \"https://example.com/f\"\nfilename = \"cmd.exe\"\n[package.install]\nkind = \"exe\"\nargs = '{args}'\nsuccess_codes = [0]\nreboot_codes = [3010]\ntimeout_sec = {timeout}\n[package.detect]\nkind = \"none\"\n"
        );
        parse_category("t.toml", &text).unwrap().packages.remove(0)
    }

    fn ctx(name: &str) -> RunContext {
        let base = std::env::temp_dir().join(format!("caior-runner-{}-{name}", std::process::id()));
        RunContext {
            log_dir: base.join("logs"),
            work_dir: base.join("work"),
            system_root: PathBuf::from(std::env::var("SystemRoot").unwrap()),
        }
    }

    fn cmd() -> PathBuf {
        PathBuf::from(std::env::var("SystemRoot").unwrap()).join(r"System32\cmd.exe")
    }

    #[test]
    fn exit_codes_map_to_outcomes() {
        let cancel = AtomicBool::new(false);
        let ok = run_package(&exe_pkg("/c exit 0", 60), Some(&cmd()), &ctx("ok"), RetryPolicy::default(), &cancel);
        assert_eq!(ok.outcome, Outcome::Success);
        let reboot = run_package(&exe_pkg("/c exit 3010", 60), Some(&cmd()), &ctx("rb"), RetryPolicy::default(), &cancel);
        assert_eq!(reboot.outcome, Outcome::Reboot);
        let fail = run_package(&exe_pkg("/c exit 1603", 60), Some(&cmd()), &ctx("f"), RetryPolicy::default(), &cancel);
        assert_eq!(fail.outcome, Outcome::Failed { code: 1603 });
        let log = fs::read_to_string(&fail.logs[0]).unwrap();
        assert!(log.contains("exit 1603"), "{log}");
    }

    #[test]
    fn busy_installer_is_retried() {
        let cancel = AtomicBool::new(false);
        let retry = RetryPolicy { busy_retries: 2, busy_wait: Duration::from_millis(10) };
        let r = run_package(&exe_pkg("/c exit 1618", 60), Some(&cmd()), &ctx("busy"), retry, &cancel);
        assert_eq!(r.attempts, 3);
        assert_eq!(r.outcome, Outcome::Failed { code: 1618 });
    }

    #[test]
    fn timeout_kills_the_process_tree() {
        let cancel = AtomicBool::new(false);
        // cmd starts ping as a child; the job must take both down.
        let started = Instant::now();
        let r = run_package(&exe_pkg("/c ping -n 30 127.0.0.1 >nul", 2), Some(&cmd()), &ctx("to"), RetryPolicy::default(), &cancel);
        assert_eq!(r.outcome, Outcome::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn missing_program_is_a_launch_failure() {
        let cancel = AtomicBool::new(false);
        let r = run_package(&exe_pkg("", 60), Some(Path::new(r"C:\does\not\exist.exe")), &ctx("nf"), RetryPolicy::default(), &cancel);
        assert!(matches!(r.outcome, Outcome::LaunchFailed { .. }));
    }
}
