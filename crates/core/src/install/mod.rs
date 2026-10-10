//! Running installers: the command line for each install kind, exit code
//! meaning, and a runner with timeout and retry.

mod command;
#[cfg(windows)]
mod runner;

pub use command::{Invocation, RunContext, invocations};
#[cfg(windows)]
pub use runner::{RetryPolicy, run_package};

use serde::Serialize;

use crate::catalog::Install;

/// Windows Installer: another installation is already in progress.
pub const ERROR_INSTALL_ALREADY_RUNNING: i64 = 1618;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Outcome {
    Success,
    /// Installed; Windows has to restart to finish.
    Reboot,
    Failed { code: i64 },
    TimedOut,
    LaunchFailed { message: String },
}

impl Outcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Outcome::Success | Outcome::Reboot)
    }
}

/// Exit codes arrive as u32 on Windows; keep them positive (0x80070005 = 2147942405).
pub fn normalize_code(code: i32) -> i64 {
    i64::from(code as u32)
}

pub fn classify(code: i64, install: &Install) -> Outcome {
    if install.reboot_codes.contains(&code) {
        Outcome::Reboot
    } else if install.success_codes.contains(&code) {
        Outcome::Success
    } else {
        Outcome::Failed { code }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageResult {
    pub id: String,
    pub outcome: Outcome,
    /// How many times the installer was started (retries after 1618 included).
    pub attempts: u32,
    pub seconds: u64,
    /// Log files written for this package (installer output, msi/dism logs).
    pub logs: Vec<std::path::PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(success: &[i64], reboot: &[i64]) -> Install {
        toml::from_str(&format!(
            "kind = \"exe\"\nargs = \"\"\nsuccess_codes = {success:?}\nreboot_codes = {reboot:?}"
        ))
        .unwrap()
    }

    #[test]
    fn classify_codes() {
        let i = install(&[0, 1638], &[3010, 1641]);
        assert_eq!(classify(0, &i), Outcome::Success);
        assert_eq!(classify(1638, &i), Outcome::Success);
        assert_eq!(classify(3010, &i), Outcome::Reboot);
        assert_eq!(classify(1603, &i), Outcome::Failed { code: 1603 });
    }

    #[test]
    fn negative_codes_become_unsigned() {
        assert_eq!(normalize_code(-2147024891), 0x80070005);
        assert_eq!(normalize_code(3010), 3010);
    }
}
