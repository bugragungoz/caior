//! "Is it installed?" for every catalog package.

mod eval;
mod probe;
mod system;
#[cfg(windows)]
mod windows;

pub use eval::{State, combine_all, combine_any, evaluate};
pub use probe::{Found, Probe, RegData, UninstallEntry, Unavailable};
pub use system::{NotApplicable, SystemInfo, applicability};
#[cfg(windows)]
pub use windows::WindowsProbe;

use serde::Serialize;

use crate::catalog::Catalog;

#[derive(Debug, Clone, Serialize)]
pub struct PackageStatus {
    pub id: String,
    pub category: String,
    #[serde(flatten)]
    pub state: Status,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Status {
    Detected(State),
    NotApplicable { state: &'static str, #[serde(flatten)] why: NotApplicable },
}

/// Detects every package of the catalog on this system.
pub fn detect_all(catalog: &Catalog, sys: &SystemInfo, probe: &dyn Probe) -> Vec<PackageStatus> {
    catalog
        .categories
        .iter()
        .flat_map(|c| c.packages.iter().map(move |p| (c, p)))
        .map(|(c, p)| PackageStatus {
            id: p.id.clone(),
            category: c.category.id.clone(),
            state: match applicability(p, sys) {
                Ok(()) => Status::Detected(evaluate(&p.detect, probe)),
                Err(why) => Status::NotApplicable { state: "not-applicable", why },
            },
        })
        .collect()
}

#[cfg(test)]
mod tests;
