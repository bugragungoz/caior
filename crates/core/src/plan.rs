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
    /// Download only (offline packages): conflicts do not matter, since
    /// nothing is installed on this PC.
    pub download_only: bool,
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
    /// A package it cannot live next to is already installed (ACE x86 next to x64).
    ConflictInstalled { with: String },
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

/// Every visible package that applies to this system. Both sides of a
/// conflict are in the list; pass it through `resolve_conflicts` before
/// installing.
pub fn all_ids(catalog: &Catalog, sys: &SystemInfo) -> Vec<String> {
    catalog
        .packages()
        .filter(|p| !p.hidden && applicability(p, sys).is_ok())
        .map(|p| p.id.clone())
        .collect()
}

/// Keeps one package of each conflicting pair in `ids` (order is kept). The
/// one built for this Windows architecture wins, then the one a profile
/// picks, then the one listed first in the catalog. Used for automatic picks
/// such as "Everything"; packages the user names explicitly should still
/// fail in `build` so they choose.
pub fn resolve_conflicts(catalog: &Catalog, sys: &SystemInfo, ids: Vec<String>) -> Vec<String> {
    let position: HashMap<&str, usize> = catalog.packages().enumerate().map(|(i, p)| (p.id.as_str(), i)).collect();
    let rank = |id: &str| {
        let p = catalog.package(id);
        (
            p.is_none_or(|p| p.arch != sys.os_arch),
            p.is_none_or(|p| p.profiles.is_empty()),
            position.get(id).copied().unwrap_or(usize::MAX),
        )
    };
    let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let loses = |id: &str| {
        catalog.package(id).is_some_and(|p| {
            p.conflicts.iter().any(|other| wanted.contains(other.as_str()) && rank(other) < rank(id))
        })
    };
    let keep: Vec<bool> = ids.iter().map(|id| !loses(id)).collect();
    ids.into_iter().zip(keep).filter_map(|(id, k)| k.then_some(id)).collect()
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
        if !opts.download_only
            && let Some(other) = pkg.conflicts.iter().find(|o| states.get(*o).is_some_and(State::is_installed))
        {
            plan.skipped.push(Skipped { id: pkg.id.clone(), reason: SkipReason::ConflictInstalled { with: other.clone() } });
            continue;
        }
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

    if !opts.download_only {
        let chosen: HashSet<&str> = plan.install.iter().map(|i| i.id.as_str()).collect();
        for item in &plan.install {
            for other in &by_id[item.id.as_str()].conflicts {
                if chosen.contains(other.as_str()) {
                    return Err(PlanError::Conflict(item.id.clone(), other.clone()));
                }
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
        let again = build(&catalog, &sys, &states, &["winget".into()], PlanOptions { reinstall: true, ..Default::default() }).unwrap();
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
    fn conflicts_are_fine_when_only_downloading() {
        let (catalog, sys) = setup();
        let opts = PlanOptions { reinstall: true, download_only: true };
        let plan = build(&catalog, &sys, &HashMap::new(), &["ace-x64".into(), "ace-x86".into()], opts).unwrap();
        assert_eq!(plan.install.len(), 2);
    }

    #[test]
    fn everything_keeps_one_side_of_a_conflict() {
        let (catalog, mut sys) = setup();
        let all = all_ids(&catalog, &sys);
        assert!(all.contains(&"ace-x64".to_string()) && all.contains(&"ace-x86".to_string()));
        let picked = resolve_conflicts(&catalog, &sys, all.clone());
        assert_eq!(picked, vec!["winget", "ace-x64"], "native x64 wins on x64");
        assert!(build(&catalog, &sys, &HashMap::new(), &picked, PlanOptions::default()).is_ok());
        // On x86 Windows only the x86 build applies, so there is nothing to resolve.
        sys.os_arch = Arch::X86;
        let picked = resolve_conflicts(&catalog, &sys, all_ids(&catalog, &sys));
        assert_eq!(picked, vec!["winget", "ace-x86"]);
    }

    #[test]
    fn installed_conflict_skips_the_other_side() {
        let (catalog, sys) = setup();
        let states = HashMap::from([("ace-x64".to_string(), State::Installed { version: None })]);
        let plan = build(&catalog, &sys, &states, &["ace-x86".into()], PlanOptions::default()).unwrap();
        assert!(plan.install.is_empty());
        assert!(matches!(&plan.skipped[0].reason, SkipReason::ConflictInstalled { with } if with == "ace-x64"));
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
