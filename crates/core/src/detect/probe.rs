//! What detection needs from the system, behind a trait so rules can be tested
//! with a fake machine. The Windows implementation lives in `windows.rs`.

use crate::catalog::{Hive, RegView};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegData {
    Dword(u32),
    Qword(u64),
    Str(String),
    /// Present, but a type detection does not read (binary, multi-string).
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallEntry {
    pub display_name: String,
    pub display_version: Option<String>,
    pub publisher: Option<String>,
}

/// The probe could not answer, e.g. the query needs administrator rights.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Unavailable(pub String);

/// A product that was found, with the version the system reports when there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub version: Option<Version>,
}

pub trait Probe {
    /// `view` is Bits32 or Bits64 here; `Both` is resolved by the caller.
    fn reg_key_exists(&self, hive: Hive, view: RegView, key: &str) -> bool;
    fn reg_value(&self, hive: Hive, view: RegView, key: &str, name: &str) -> Option<RegData>;
    /// Names of the key's values and subkeys.
    fn reg_names(&self, hive: Hive, view: RegView, key: &str) -> Vec<String>;
    /// Entries of the Uninstall keys: HKLM in the given view (Both = 32 and 64) plus HKCU.
    fn uninstall_entries(&self, view: RegView) -> Vec<UninstallEntry>;
    fn msi_product(&self, product_code: &str) -> Option<Found>;
    /// Installed products that share an upgrade code.
    fn msi_related(&self, upgrade_code: &str) -> Vec<Found>;
    /// `None` when the file does not exist. Environment variables are expanded.
    fn file_version(&self, path: &str) -> Option<Found>;
    fn optional_feature_enabled(&self, name: &str) -> Result<bool, Unavailable>;
    fn capability_installed(&self, name: &str) -> Result<bool, Unavailable>;
    /// Versions of the installed packages of an appx family, for the current user.
    fn appx_versions(&self, family: &str) -> Result<Vec<Version>, Unavailable>;
}
