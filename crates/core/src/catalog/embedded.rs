//! The catalog compiled into the binary (see `build.rs`).

use super::load::{LoadError, parse_category};
use super::model::Catalog;

mod files {
    include!(concat!(env!("OUT_DIR"), "/embedded_catalog.rs"));
}

/// Names and contents of the embedded catalog files.
pub fn files() -> &'static [(&'static str, &'static str)] {
    files::FILES
}

/// Parses the embedded catalog. Any file that fails to parse is an error:
/// a shipped binary must carry a complete catalog.
pub fn embedded() -> Result<Catalog, LoadError> {
    let mut catalog = Catalog::default();
    for (name, text) in files() {
        catalog.categories.push(parse_category(name, text)?);
    }
    catalog.categories.sort_by_key(|c| (c.category.order, c.category.id.clone()));
    Ok(catalog)
}
