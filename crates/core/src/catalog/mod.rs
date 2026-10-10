//! Package catalog: model, loading and validation.

mod embedded;
mod load;
mod model;
mod validate;

pub use embedded::{embedded, files as embedded_files};
pub use load::{LoadError, load_dir, parse_category};
pub use model::*;
pub use validate::{Issue, Severity, ValidateOptions, validate};
