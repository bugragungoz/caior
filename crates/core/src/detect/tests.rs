use std::collections::HashMap;

use super::*;
use crate::catalog::{Arch, Detect, Hive, RegView};
use crate::version::Version;

/// A fake machine: registry keys with values, uninstall entries, files, MSI products.
#[derive(Default)]
struct Fake {
    keys: HashMap<(Hive, RegView, String), Vec<(String, RegData)>>,
    uninstall: Vec<(RegView, UninstallEntry)>,
    msi: HashMap<String, Option<Version>>,
    related: HashMap<String, Vec<String>>,
    files: HashMap<String, Option<Version>>,
    features: HashMap<String, bool>,
}

impl Fake {
    fn key(mut self, hive: Hive, view: RegView, key: &str, values: &[(&str, RegData)]) -> Self {
        self.keys.insert(
            (hive, view, key.to_ascii_lowercase()),
            values.iter().map(|(n, d)| (n.to_string(), d.clone())).collect(),
        );
        self
    }
}

impl Probe for Fake {
    fn reg_key_exists(&self, hive: Hive, view: RegView, key: &str) -> bool {
        self.keys.contains_key(&(hive, view, key.to_ascii_lowercase()))
    }
    fn reg_value(&self, hive: Hive, view: RegView, key: &str, name: &str) -> Option<RegData> {
        self.keys
            .get(&(hive, view, key.to_ascii_lowercase()))?
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, d)| d.clone())
    }
    fn reg_names(&self, hive: Hive, view: RegView, key: &str) -> Vec<String> {
        self.keys
            .get(&(hive, view, key.to_ascii_lowercase()))
            .map(|v| v.iter().map(|(n, _)| n.clone()).collect())
            .unwrap_or_default()
    }
    fn uninstall_entries(&self, view: RegView) -> Vec<UninstallEntry> {
        self.uninstall
            .iter()
            .filter(|(v, _)| view == RegView::Both || *v == view)
            .map(|(_, e)| e.clone())
            .collect()
    }
    fn msi_product(&self, code: &str) -> Option<Found> {
        self.msi.get(code).map(|v| Found { version: v.clone() })
    }
    fn msi_related(&self, upgrade: &str) -> Vec<Found> {
        self.related
            .get(upgrade)
            .map(|codes| codes.iter().filter_map(|c| self.msi_product(c)).collect())
            .unwrap_or_default()
    }
    fn file_version(&self, path: &str) -> Option<Found> {
        self.files.get(path).map(|v| Found { version: v.clone() })
    }
    fn optional_feature_enabled(&self, name: &str) -> Result<bool, Unavailable> {
        Ok(*self.features.get(name).unwrap_or(&false))
    }
    fn capability_installed(&self, _name: &str) -> Result<bool, Unavailable> {
        Err(Unavailable("needs admin".into()))
    }
    fn appx_versions(&self, _family: &str) -> Result<Vec<Version>, Unavailable> {
        Ok(vec![])
    }
}

fn rule(toml_text: &str) -> Detect {
    toml::from_str(toml_text).unwrap()
}

fn v(s: &str) -> Version {
    s.parse().unwrap()
}

const VC14_X64: &str = r#"
kind = "registry"
hive = "HKLM"
key = 'SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\X64'
view = "64"
value = "Installed"
equals = 1
version_value = "Version"
min_version = "14.44.35211"
"#;

#[test]
fn registry_installed_outdated_missing() {
    let key = r"SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\X64";
    let current = Fake::default().key(
        Hive::HKLM,
        RegView::Bits64,
        key,
        &[("Installed", RegData::Dword(1)), ("Version", RegData::Str("v14.44.35211.00".into()))],
    );
    assert_eq!(evaluate(&rule(VC14_X64), &current), State::Installed { version: Some(v("14.44.35211")) });

    let old = Fake::default().key(
        Hive::HKLM,
        RegView::Bits64,
        key,
        &[("Installed", RegData::Dword(1)), ("Version", RegData::Str("v14.29.30133.00".into()))],
    );
    assert!(matches!(evaluate(&rule(VC14_X64), &old), State::Outdated { .. }));

    // The key in the wrong view does not count.
    let wrong_view = Fake::default().key(Hive::HKLM, RegView::Bits32, key, &[("Installed", RegData::Dword(1))]);
    assert_eq!(evaluate(&rule(VC14_X64), &wrong_view), State::Missing);

    let not_installed = Fake::default().key(Hive::HKLM, RegView::Bits64, key, &[("Installed", RegData::Dword(0))]);
    assert_eq!(evaluate(&rule(VC14_X64), &not_installed), State::Missing);
}

#[test]
fn version_names_pick_the_highest_matching_prefix() {
    let key = r"SOFTWARE\dotnet\Setup\InstalledVersions\x64\sharedfx\Microsoft.WindowsDesktop.App";
    let fake = Fake::default().key(
        Hive::HKLM,
        RegView::Bits32,
        key,
        &[("6.0.36", RegData::Dword(1)), ("8.0.11", RegData::Dword(1)), ("8.0.20", RegData::Dword(1))],
    );
    let r = rule(&format!(
        "kind = \"registry\"\nhive = \"HKLM\"\nkey = '{key}'\nview = \"32\"\nversion_name_prefix = \"8.\"\nmin_version = \"8.0.20\""
    ));
    assert_eq!(evaluate(&r, &fake), State::Installed { version: Some(v("8.0.20")) });
    let r9 = rule(&format!("kind = \"registry\"\nhive = \"HKLM\"\nkey = '{key}'\nview = \"32\"\nversion_name_prefix = \"9.\""));
    assert_eq!(evaluate(&r9, &fake), State::Missing);
}

#[test]
fn min_dword_for_netfx_release() {
    let key = r"SOFTWARE\Microsoft\NET Framework Setup\NDP\v4\Full";
    let fake = Fake::default().key(Hive::HKLM, RegView::Bits64, key, &[("Release", RegData::Dword(533325))]);
    let r = |min: u32| {
        rule(&format!(
            "kind = \"registry\"\nhive = \"HKLM\"\nkey = '{key}'\nview = \"64\"\nvalue = \"Release\"\nmin_dword = {min}"
        ))
    };
    assert!(evaluate(&r(533320), &fake).is_installed());
    assert_eq!(evaluate(&r(600000), &fake), State::Missing);
}

#[test]
fn uninstall_regex_and_view() {
    let mut fake = Fake::default();
    fake.uninstall.push((
        RegView::Bits32,
        UninstallEntry {
            display_name: "NVIDIA PhysX System Software 9.23.1019".into(),
            display_version: Some("9.23.1019".into()),
            publisher: Some("NVIDIA Corporation".into()),
        },
    ));
    let both = rule("kind = \"uninstall\"\ndisplay_name = '^NVIDIA PhysX System Software'\nview = \"both\"\npublisher = 'NVIDIA'");
    assert_eq!(evaluate(&both, &fake), State::Installed { version: Some(v("9.23.1019")) });
    let only64 = rule("kind = \"uninstall\"\ndisplay_name = '^NVIDIA PhysX'\nview = \"64\"");
    assert_eq!(evaluate(&only64, &fake), State::Missing);
}

#[test]
fn msi_upgrade_code_takes_newest_related_product() {
    let mut fake = Fake::default();
    fake.msi.insert("{A}".into(), Some(v("4.0.1")));
    fake.msi.insert("{B}".into(), Some(v("4.0.5")));
    fake.related.insert("{U}".into(), vec!["{A}".into(), "{B}".into()]);
    let r = rule("kind = \"msi-product\"\nupgrade_code = '{U}'\nmin_version = \"4.0.5\"");
    assert_eq!(evaluate(&r, &fake), State::Installed { version: Some(v("4.0.5")) });
}

#[test]
fn any_and_all_combinations() {
    let mut fake = Fake::default();
    fake.files.insert(r"%SystemRoot%\System32\d3dx9_43.dll".into(), Some(v("9.29.952.3111")));
    let all = rule(
        r#"
kind = "all"
[[rules]]
kind = "file-version"
path = '%SystemRoot%\System32\d3dx9_43.dll'
[[rules]]
kind = "file-version"
path = '%SystemRoot%\SysWOW64\d3dx9_43.dll'
"#,
    );
    assert_eq!(evaluate(&all, &fake), State::Missing, "one missing file fails 'all'");
    fake.files.insert(r"%SystemRoot%\SysWOW64\d3dx9_43.dll".into(), Some(v("9.29.952.3111")));
    assert!(evaluate(&all, &fake).is_installed());

    let any = combine_any([State::Missing, State::Unknown { reason: "x".into() }, State::Installed { version: None }]);
    assert!(any.is_installed());
    let all = combine_all([State::Installed { version: None }, State::Unknown { reason: "x".into() }]);
    assert!(matches!(all, State::Unknown { .. }));
}

#[test]
fn capability_without_admin_is_unknown() {
    let r = rule("kind = \"dism-capability\"\ncapability = \"Media.MediaFeaturePack~~~~0.0.1.0\"");
    assert!(matches!(evaluate(&r, &Fake::default()), State::Unknown { .. }));
}

#[test]
fn applicability_rules() {
    use crate::catalog::parse_category;
    let text = r#"
[category]
id = "t"
name = { en = "T" }
order = 1
icon = "x"

[[package]]
id = "x64-only"
name = "X"
version = "1"
arch = "x64"
order = 1
os_min = "10.0.17763"
os_max = "10.0.18363"
[package.source]
url = "https://example.com/x.exe"
filename = "x.exe"
[package.install]
kind = "exe"
args = ""
[package.detect]
kind = "none"

[[package]]
id = "webview-x86"
name = "W"
version = "1"
arch = "x86"
os_arch = ["x86"]
order = 2
[package.source]
url = "https://example.com/w.exe"
filename = "w.exe"
[package.install]
kind = "exe"
args = ""
[package.detect]
kind = "none"
"#;
    let cat = parse_category("t.toml", text).unwrap();
    let sys = |ver: &str, arch: Arch| SystemInfo {
        os_version: v(ver),
        os_arch: arch,
        edition_id: "Professional".into(),
        product_name: String::new(),
        display_version: String::new(),
        elevated: false,
        reboot_pending: false,
    };
    let x64 = &cat.packages[0];
    let wv86 = &cat.packages[1];
    assert!(applicability(x64, &sys("10.0.18363.1556", Arch::X64)).is_ok(), "os_max is inclusive of UBR");
    assert!(matches!(applicability(x64, &sys("10.0.19045.1", Arch::X64)), Err(NotApplicable::OsTooNew { .. })));
    assert!(matches!(applicability(x64, &sys("10.0.17134.1", Arch::X64)), Err(NotApplicable::OsTooOld { .. })));
    assert!(matches!(applicability(x64, &sys("10.0.18363.1", Arch::X86)), Err(NotApplicable::Arch { .. })));
    // x64 on ARM64 needs Windows 11, which also fails os_max here; check arch alone with a wide range.
    assert!(applicability(wv86, &sys("10.0.22631.1", Arch::X86)).is_ok());
    assert!(matches!(applicability(wv86, &sys("10.0.22631.1", Arch::X64)), Err(NotApplicable::Arch { .. })));
}
