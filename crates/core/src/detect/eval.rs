//! Turns a catalog `Detect` rule into an install state.

use regex::RegexBuilder;
use serde::Serialize;

use super::probe::{Found, Probe, RegData};
use crate::catalog::{Detect, DetectKind, RegValue, RegView};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum State {
    /// Present and new enough. `version` is what the system reports, if anything.
    Installed { version: Option<Version> },
    /// Present but older than the rule's `min_version`.
    Outdated { version: Version, wanted: Version },
    Missing,
    /// The rule could not be evaluated (no rule, or the query needs elevation).
    Unknown { reason: String },
}

impl State {
    pub fn is_installed(&self) -> bool {
        matches!(self, State::Installed { .. })
    }

    fn rank(&self) -> u8 {
        match self {
            State::Installed { .. } => 3,
            State::Outdated { .. } => 2,
            State::Unknown { .. } => 1,
            State::Missing => 0,
        }
    }

    fn version(&self) -> Option<&Version> {
        match self {
            State::Installed { version } => version.as_ref(),
            State::Outdated { version, .. } => Some(version),
            _ => None,
        }
    }
}

fn unknown(reason: impl Into<String>) -> State {
    State::Unknown { reason: reason.into() }
}

/// Applies `min_version` to something that was found.
fn finish(version: Option<Version>, min_version: &Option<String>) -> State {
    let wanted = min_version.as_deref().and_then(|m| m.parse::<Version>().ok());
    match (version, wanted) {
        (Some(v), Some(w)) if v < w => State::Outdated { version: v, wanted: w },
        (version, _) => State::Installed { version },
    }
}

fn best_found(found: impl IntoIterator<Item = Found>) -> Option<Option<Version>> {
    let mut any = false;
    let mut best: Option<Version> = None;
    for f in found {
        any = true;
        if let Some(v) = f.version {
            if best.as_ref().is_none_or(|b| &v > b) {
                best = Some(v);
            }
        }
    }
    any.then_some(best)
}

fn value_matches(data: &RegData, expected: &RegValue) -> bool {
    match (data, expected) {
        (RegData::Dword(d), RegValue::Int(i)) => i64::from(*d) == *i,
        (RegData::Qword(q), RegValue::Int(i)) => i64::try_from(*q).is_ok_and(|q| q == *i),
        (RegData::Str(s), RegValue::Str(e)) => s.trim().eq_ignore_ascii_case(e.trim()),
        (RegData::Str(s), RegValue::Int(i)) => s.trim().parse::<i64>().is_ok_and(|n| n == *i),
        (RegData::Dword(d), RegValue::Str(e)) => e.trim().parse::<i64>().is_ok_and(|n| n == i64::from(*d)),
        _ => false,
    }
}

fn as_dword(data: &RegData) -> Option<u64> {
    match data {
        RegData::Dword(d) => Some(u64::from(*d)),
        RegData::Qword(q) => Some(*q),
        RegData::Str(s) => s.trim().parse().ok(),
        RegData::Other => None,
    }
}

pub fn evaluate(d: &Detect, probe: &dyn Probe) -> State {
    match d.kind {
        DetectKind::Registry => registry(d, probe),
        DetectKind::Uninstall => uninstall(d, probe),
        DetectKind::MsiProduct => {
            let found = match (&d.product_code, &d.upgrade_code) {
                (Some(code), _) => best_found(probe.msi_product(code)),
                (None, Some(code)) => best_found(probe.msi_related(code)),
                (None, None) => return unknown("msi-product rule without a code"),
            };
            match found {
                Some(v) => finish(v, &d.min_version),
                None => State::Missing,
            }
        }
        DetectKind::FileVersion => match d.path.as_deref().map(|p| probe.file_version(p)) {
            Some(Some(f)) => finish(f.version, &d.min_version),
            Some(None) => State::Missing,
            None => unknown("file-version rule without a path"),
        },
        DetectKind::DismFeature => match d.feature.as_deref().map(|f| probe.optional_feature_enabled(f)) {
            Some(Ok(true)) => State::Installed { version: None },
            Some(Ok(false)) => State::Missing,
            Some(Err(e)) => unknown(e.0),
            None => unknown("dism-feature rule without a feature"),
        },
        DetectKind::DismCapability => match d.capability.as_deref().map(|c| probe.capability_installed(c)) {
            Some(Ok(true)) => State::Installed { version: None },
            Some(Ok(false)) => State::Missing,
            Some(Err(e)) => unknown(e.0),
            None => unknown("dism-capability rule without a capability"),
        },
        DetectKind::Appx => match d.family.as_deref().map(|f| probe.appx_versions(f)) {
            Some(Ok(versions)) if versions.is_empty() => State::Missing,
            Some(Ok(versions)) => finish(versions.into_iter().max(), &d.min_version),
            Some(Err(e)) => unknown(e.0),
            None => unknown("appx rule without a family"),
        },
        DetectKind::Any => combine_any(d.rules.iter().map(|r| evaluate(r, probe))),
        DetectKind::All => combine_all(d.rules.iter().map(|r| evaluate(r, probe))),
        DetectKind::None => unknown("no detection rule"),
    }
}

fn registry(d: &Detect, probe: &dyn Probe) -> State {
    let (Some(hive), Some(key)) = (d.hive, d.key.as_deref()) else {
        return unknown("registry rule without hive or key");
    };
    let view = match d.view {
        Some(RegView::Both) | None => RegView::Bits64,
        Some(v) => v,
    };
    if !probe.reg_key_exists(hive, view, key) {
        return State::Missing;
    }
    if let Some(name) = d.value.as_deref() {
        let Some(data) = probe.reg_value(hive, view, key, name) else {
            return State::Missing;
        };
        if let Some(expected) = &d.equals {
            if !value_matches(&data, expected) {
                return State::Missing;
            }
        }
        if let Some(min) = d.min_dword {
            if as_dword(&data).is_none_or(|n| n < u64::from(min)) {
                return State::Missing;
            }
        }
    }
    let mut version = None;
    if let Some(name) = d.version_value.as_deref() {
        version = match probe.reg_value(hive, view, key, name) {
            Some(RegData::Str(s)) => Version::parse_lenient(&s),
            _ => None,
        };
    }
    if let Some(prefix) = d.version_name_prefix.as_deref() {
        version = probe
            .reg_names(hive, view, key)
            .iter()
            .filter(|n| n.starts_with(prefix))
            .filter_map(|n| Version::parse_lenient(n))
            .max();
        if version.is_none() {
            return State::Missing;
        }
    }
    finish(version, &d.min_version)
}

fn uninstall(d: &Detect, probe: &dyn Probe) -> State {
    let Some(pattern) = d.display_name.as_deref() else {
        return unknown("uninstall rule without display_name");
    };
    let build = |p: &str| RegexBuilder::new(p).case_insensitive(true).build();
    let Ok(name_re) = build(pattern) else {
        return unknown(format!("invalid display_name regex {pattern:?}"));
    };
    let publisher_re = match d.publisher.as_deref().map(build) {
        Some(Ok(re)) => Some(re),
        Some(Err(_)) => return unknown("invalid publisher regex"),
        None => None,
    };
    let entries = probe.uninstall_entries(d.view.unwrap_or(RegView::Both));
    let found = entries
        .into_iter()
        .filter(|e| name_re.is_match(&e.display_name))
        .filter(|e| match (&publisher_re, &e.publisher) {
            (Some(re), Some(p)) => re.is_match(p),
            (Some(_), None) => false,
            (None, _) => true,
        })
        .map(|e| Found { version: e.display_version.as_deref().and_then(Version::parse_lenient) });
    match best_found(found) {
        Some(v) => finish(v, &d.min_version),
        None => State::Missing,
    }
}

/// Best state wins: one installed rule is enough.
pub fn combine_any(states: impl IntoIterator<Item = State>) -> State {
    let mut best = State::Missing;
    for s in states {
        let better = s.rank() > best.rank()
            || (s.rank() == best.rank() && s.version().is_some_and(|v| best.version().is_none_or(|b| v > b)));
        if better {
            best = s;
        }
    }
    best
}

/// Worst state wins: every rule has to be satisfied.
pub fn combine_all(states: impl IntoIterator<Item = State>) -> State {
    let mut worst: Option<State> = None;
    for s in states {
        let worse = match &worst {
            None => true,
            Some(w) => {
                // Missing beats Unknown here: one missing file means the product is not complete.
                let rank = |s: &State| match s {
                    State::Missing => 0,
                    State::Unknown { .. } => 1,
                    State::Outdated { .. } => 2,
                    State::Installed { .. } => 3,
                };
                rank(&s) < rank(w)
                    || (rank(&s) == rank(w) && s.version().is_some_and(|v| w.version().is_none_or(|b| v < b)))
            }
        };
        if worse {
            worst = Some(s);
        }
    }
    worst.unwrap_or_else(|| unknown("empty rule list"))
}
