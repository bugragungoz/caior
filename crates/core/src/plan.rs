//! From a selection to an ordered install list: dependencies first, packages
//! that do not apply or are already installed skipped, conflicts rejected.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::catalog::{Catalog, Package, Profile};
use crate::detect::{NotApplicable, State, SystemInfo, applicability};

#[derive(Debug, Clone, Copy, Default)]
pub struct PlanOptions {
    /// Install even when detection says the package is already there.
    pub reinstall: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Reason {
    Selected,
    /// Pulled in because another package depends on it.
    Dependency { of: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanItem {
    pub id: String,
    pub reason: Reason,
    /// Detection result at planning time.
    pub state: State,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SkipReason {
    AlreadyInstalled { version: Option<String> },
    NotApplicable { why: NotApplicable },
}

#[derive(Debug, Clone, Serialize)]
pub struct Skipped {
    pub id: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Plan {
    pub install: Vec<PlanItem>,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("unknown package {0:?}")]
    UnknownPackage(String),
    #[error("{0} and {1} cannot be installed together; choose one")]
    Conflict(String, String),
}

/// Packages a profile preselects on this system (visible, applicable ones only).
pub fn profile_ids(catalog: &Catalog, profile: Profile, sys: &SystemInfo) -> Vec<String> {
    catalog
        .packages()
        .filter(|p| !p.hidden && p.profiles.contains(&profile) && applicability(p, sys).is_ok())
        .map(|p| p.id.clone())
        .collect()
}

/// Every visible package that applies to this system.
pub fn all_ids(catalog: &Catalog, sys: &SystemInfo) -> Vec<String> {
    catalog
        .packages()
        .filter(|p| !p.hidden && applicability(p, sys).is_ok())
        .map(|p| p.id.clone())
        .collect()
}

pub fn build(
    catalog: &Catalog,
    sys: &SystemInfo,
    states: &HashMap<String, State>,
    selected: &[String],
    opts: PlanOptions,
) -> Result<Plan, PlanError> {
    let by_id: HashMap<&str, &Package> = catalog.packages().map(|p| (p.id.as_str(), p)).collect();
    for id in selected {
        if !by_id.contains_key(id.as_str()) {
            return Err(PlanError::UnknownPackage(id.clone()));
        }
    }

    // Post-order walk: dependencies land before the packages that need them.
    fn visit<'a>(
        id: &'a str,
        reason: Reason,
        by_id: &HashMap<&'a str, &'a Package>,
        seen: &mut HashSet<&'a str>,
        out: &mut Vec<(&'a Package, Reason)>,
    ) -> Result<(), PlanError> {
        let Some(pkg) = by_id.get(id).copied() else {
            return Err(PlanError::UnknownPackage(id.to_string()));
        };
        if !seen.insert(pkg.id.as_str()) {
            return Ok(());
        }
        for dep in &pkg.depends {
            visit(dep, Reason::Dependency { of: pkg.id.clone() }, by_id, seen, out)?;
        }
        out.push((pkg, reason));
        Ok(())
    }

    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for id in selected {
        visit(id, Reason::Selected, &by_id, &mut seen, &mut ordered)?;
    }

    let mut plan = Plan::default();
    for (pkg, reason) in ordered {
        if let Err(why) = applicability(pkg, sys) {
            plan.skipped.push(Skipped { id: pkg.id.clone(), reason: SkipReason::NotApplicable { why } });
            continue;
        }
        let state = states.get(&pkg.id).cloned().unwrap_or(State::Unknown { reason: "not detected".into() });
        if let State::Installed { version } = &state
            && !opts.reinstall {
                plan.skipped.push(Skipped {
                    id: pkg.id.clone(),
                    reason: SkipReason::AlreadyInstalled { version: version.as_ref().map(ToString::to_string) },
                });
                continue;
            }
        plan.install.push(PlanItem { id: pkg.id.clone(), reason, state });
    }

    let chosen: HashSet<&str> = plan.install.iter().map(|i| i.id.as_str()).collect();
    for item in &plan.install {
        for other in &by_id[item.id.as_str()].conflicts {
            if chosen.contains(other.as_str()) {
                return Err(PlanError::Conflict(item.id.clone(), other.clone()));
            }
        }
    }

    // Catalog validation guarantees a dependency has a lower order than its
    // dependents, so a stable sort keeps them in front.
    plan.install.sort_by_key(|i| by_id[i.id.as_str()].order);
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Arch, parse_category};

    const CAT: &str = r#"
[category]
id = "t"
name = { en = "T" }
order = 1
icon = "x"

[[package]]
id = "vclibs-x64"
name = "VCLibs"
version = "14"
arch = "x64"
order = 400
hidden = true
[package.source]
url = "https://example.com/v.appx"
filename = "v.appx"
[package.install]
kind = "appx"
[package.detect]
kind = "none"

[[package]]
id = "winget"
name = "App Installer"
version = "1"
arch = "neutral"
order = 410
profiles = ["recommended"]
depends = ["vclibs-x64"]
[package.source]
url = "https://example.com/w.msixbundle"
filename = "w.msixbundle"
[package.install]
kind = "appx"
[package.detect]
kind = "none"

[[package]]
id = "ace-x64"
name = "ACE x64"
version = "16"
arch = "x64"
order = 600
conflicts = ["ace-x86"]
[package.source]
url = "https://example.com/a.exe"
filename = "a.exe"
[package.install]
kind = "exe"
args = "/quiet"
[package.detect]
kind = "none"

[[package]]
id = "ace-x86"
name = "ACE x86"
version = "16"
arch = "x86"
order = 601
conflicts = ["ace-x64"]
[package.source]
url = "https://example.com/b.exe"
filename = "b.exe"
[package.install]
kind = "exe"
args = "/quiet"
[package.detect]
kind = "none"
"#;

    fn setup() -> (Catalog, SystemInfo) {
        let catalog = Catalog { categories: vec![parse_category("t.toml", CAT).unwrap()] };
        let sys = SystemInfo {
            os_version: "10.0.22631.1".parse().unwrap(),
            os_arch: Arch::X64,
            edition_id: "Professional".into(),
            product_name: String::new(),
            display_version: String::new(),
            elevated: false,
            reboot_pending: false,
        };
        (catalog, sys)
    }

    #[test]
    fn dependencies_come_first_and_profiles_skip_hidden() {
        let (catalog, sys) = setup();
        assert_eq!(profile_ids(&catalog, Profile::Recommended, &sys), vec!["winget"]);
        let plan = build(&catalog, &sys, &HashMap::new(), &["winget".into()], PlanOptions::default()).unwrap();
        let ids: Vec<&str> = plan.install.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["vclibs-x64", "winget"]);
        assert_eq!(plan.install[0].reason, Reason::Dependency { of: "winget".into() });
    }

    #[test]
    fn installed_packages_are_skipped_unless_reinstall() {
        let (catalog, sys) = setup();
        let states = HashMap::from([("vclibs-x64".to_string(), State::Installed { version: None })]);
        let plan = build(&catalog, &sys, &states, &["winget".into()], PlanOptions::default()).unwrap();
        assert_eq!(plan.install.len(), 1);
        assert!(matches!(plan.skipped[0].reason, SkipReason::AlreadyInstalled { .. }));
        let again = build(&catalog, &sys, &states, &["winget".into()], PlanOptions { reinstall: true }).unwrap();
        assert_eq!(again.install.len(), 2);
    }

    #[test]
    fn conflicts_and_unknown_ids_are_errors() {
        let (catalog, sys) = setup();
        let err = build(&catalog, &sys, &HashMap::new(), &["ace-x64".into(), "ace-x86".into()], PlanOptions::default());
        assert!(matches!(err, Err(PlanError::Conflict(..))));
        let err = build(&catalog, &sys, &HashMap::new(), &["nope".into()], PlanOptions::default());
        assert_eq!(err.unwrap_err(), PlanError::UnknownPackage("nope".into()));
    }

    #[test]
    fn not_applicable_packages_are_skipped() {
        let (catalog, mut sys) = setup();
        sys.os_arch = Arch::X86;
        let plan = build(&catalog, &sys, &HashMap::new(), &["ace-x64".into()], PlanOptions::default()).unwrap();
        assert!(plan.install.is_empty());
        assert!(matches!(plan.skipped[0].reason, SkipReason::NotApplicable { .. }));
    }
}
