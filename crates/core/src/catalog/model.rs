//! Catalog data model. One TOML file per category under `catalog/`.
//! The format is documented in `docs/CATALOG-FORMAT.md`; keep both in sync.

use serde::{Deserialize, Serialize};

/// UI text with an English base and an optional Turkish translation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub en: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tr: Option<String>,
}

impl Text {
    pub fn get(&self, lang: &str) -> &str {
        match (lang, &self.tr) {
            ("tr", Some(tr)) if !tr.is_empty() => tr,
            _ => &self.en,
        }
    }
}

/// Contents of one `catalog/<category>.toml` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryFile {
    pub category: Category,
    #[serde(default, rename = "package")]
    pub packages: Vec<Package>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Category {
    pub id: String,
    pub name: Text,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Text>,
    pub order: u32,
    pub icon: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    X86,
    X64,
    Arm64,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Recommended,
    Gaming,
    Developer,
    Legacy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    /// Unique across the whole catalog: `[a-z0-9][a-z0-9-]*`.
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Text>,
    /// Version this entry installs, as shown to the user. Free text for evergreen
    /// sources ("latest"); use `detect.min_version` for comparisons.
    pub version: String,
    pub arch: Arch,
    /// Minimum Windows build, e.g. "10.0.17763".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_min: Option<String>,
    /// Last Windows build the package applies to (inclusive).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_max: Option<String>,
    /// Windows architectures this package is offered on. Empty means the default:
    /// x86 packages on x86/x64/arm64, x64 on x64/arm64, arm64 on arm64.
    /// Set it for runtimes that only make sense natively, e.g. `os_arch = ["x86"]` for WebView2 x86.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub os_arch: Vec<Arch>,
    /// Offer only on Windows "N" editions (Media Feature Pack).
    #[serde(default, skip_serializing_if = "is_false")]
    pub only_n_edition: bool,
    /// Profiles that preselect this package. "all" is implicit.
    #[serde(default)]
    pub profiles: Vec<Profile>,
    /// Install order, lower first.
    pub order: u32,
    #[serde(default)]
    pub depends: Vec<String>,
    /// Packages that cannot be installed side by side with this one (e.g. Access
    /// Database Engine x86 and x64). The GUI lets the user pick one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
    /// Packages sharing a group (x86/x64 of one product) show as one row in the GUI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Out of vendor support; shown with a badge.
    #[serde(default, skip_serializing_if = "is_false")]
    pub legacy: bool,
    /// Not listed in the GUI; only installed as a dependency.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<Text>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Maintainer notes: where the facts came from, quirks. Not shown in the GUI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Absent only for `dism-feature` and `dism-capability` installs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    pub install: Install,
    pub detect: Detect,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_urls: Vec<String>,
    /// File name in the cache and in offline bundles.
    pub filename: String,
    /// Size in bytes; 0 until `xtask catalog refresh` fills it.
    #[serde(default)]
    pub size: u64,
    /// Lowercase hex SHA256. Required for non-evergreen sources in strict mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// The URL always serves the newest build, so the hash is not stable.
    #[serde(default, skip_serializing_if = "is_false")]
    pub evergreen: bool,
    /// Accepted Authenticode signer subject common names. Required when evergreen.
    #[serde(default)]
    pub signer: Vec<String>,
    /// Cannot go into offline bundles (license click-through, login).
    #[serde(default, skip_serializing_if = "is_false")]
    pub online_only: bool,
    /// The download is an archive; the installer is one entry inside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract: Option<Extract>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extract {
    pub archive: ArchiveKind,
    /// Path of the installer inside the archive, '/' separated.
    pub entry: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveKind {
    Zip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallKind {
    /// Run the file with `args`.
    Exe,
    /// `msiexec /i <file> /qn /norestart /l*v <log> <properties>`.
    Msi,
    /// `wusa.exe <file> /quiet /norestart`.
    Msu,
    /// Add-AppxPackage, or Add-AppxProvisionedPackage when `provisioned`.
    Appx,
    /// `DISM /Online /Enable-Feature /FeatureName:<feature> [/All] /NoRestart`.
    DismFeature,
    /// `DISM /Online /Add-Capability /CapabilityName:<capability> /NoRestart`.
    DismCapability,
    /// DirectX redist: extract with `/Q /T:<dir>`, then `DXSETUP.exe /silent`.
    Directx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Install {
    pub kind: InstallKind,
    /// `exe`: command line arguments (may be empty).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<String>,
    /// `msi`: extra properties, e.g. `ADDLOCAL=FeatureMain`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<String>,
    /// `appx`: install for all users.
    #[serde(default, skip_serializing_if = "is_false")]
    pub provisioned: bool,
    /// `dism-feature`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    /// `dism-feature`: enable parent features too (`/All`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
    /// `dism-feature`: how to get an offline source (shown when Windows Update is unreachable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hint: Option<Text>,
    /// `dism-capability`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
    #[serde(default = "default_success_codes")]
    pub success_codes: Vec<i64>,
    #[serde(default = "default_reboot_codes")]
    pub reboot_codes: Vec<i64>,
    #[serde(default = "default_timeout")]
    pub timeout_sec: u32,
}

fn default_success_codes() -> Vec<i64> {
    vec![0]
}

fn default_reboot_codes() -> Vec<i64> {
    vec![3010, 1641]
}

fn default_timeout() -> u32 {
    1200
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DetectKind {
    Registry,
    Uninstall,
    MsiProduct,
    FileVersion,
    DismFeature,
    DismCapability,
    Appx,
    Any,
    All,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Hive {
    HKLM,
    HKCU,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RegView {
    #[serde(rename = "32")]
    Bits32,
    #[serde(rename = "64")]
    Bits64,
    #[serde(rename = "both")]
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RegValue {
    Int(i64),
    Str(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Detect {
    pub kind: DetectKind,
    // registry
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hive: Option<Hive>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// registry, uninstall: which registry view to read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<RegView>,
    /// registry: value name that must exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// registry: the value must equal this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals: Option<RegValue>,
    /// registry: DWORD value must be at least this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_dword: Option<u32>,
    /// registry: value holding a version string, compared with `min_version`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_value: Option<String>,
    /// registry: the names of the key's values or subkeys are versions (dotnet
    /// `InstalledVersions\...\sharedfx\<framework>` lists one value per version).
    /// The highest name starting with this prefix ("8." or "" for any) must exist,
    /// and is compared with `min_version` when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_name_prefix: Option<String>,
    /// registry, uninstall, msi-product, file-version, appx.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_version: Option<String>,
    // uninstall
    /// uninstall: regex on DisplayName.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// uninstall: optional regex on Publisher.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    // msi-product
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade_code: Option<String>,
    // file-version
    /// file-version: path with environment variables, e.g. `%SystemRoot%\System32\d3dx9_43.dll`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    // dism
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
    // appx
    /// appx: package family name, e.g. `Microsoft.DesktopAppInstaller_8wekyb3d8bbwe`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    // any / all
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Detect>,
}

/// The whole catalog: every category file merged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub categories: Vec<CategoryEntry>,
}

/// A category with its packages and the file it came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryEntry {
    pub file: String,
    pub category: Category,
    pub packages: Vec<Package>,
}

impl Catalog {
    pub fn packages(&self) -> impl Iterator<Item = &Package> {
        self.categories.iter().flat_map(|c| c.packages.iter())
    }

    pub fn package(&self, id: &str) -> Option<&Package> {
        self.packages().find(|p| p.id == id)
    }
}
