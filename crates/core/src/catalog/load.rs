use std::path::{Path, PathBuf};

use super::model::{Catalog, CategoryEntry, CategoryFile};

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{file}: {message}")]
    Parse { file: String, message: String },
}

/// Parses one category file. `file` is only used in error messages.
pub fn parse_category(file: &str, text: &str) -> Result<CategoryEntry, LoadError> {
    let parsed: CategoryFile = toml::from_str(text).map_err(|e| LoadError::Parse {
        file: file.to_string(),
        message: e.to_string(),
    })?;
    Ok(CategoryEntry {
        file: file.to_string(),
        category: parsed.category,
        packages: parsed.packages,
    })
}

/// Loads every `*.toml` in `dir`, sorted by category order. A file that fails to
/// parse is reported in the error list and skipped, so one broken file does not
/// hide problems in the others.
pub fn load_dir(dir: &Path) -> Result<(Catalog, Vec<LoadError>), LoadError> {
    let read_dir = std::fs::read_dir(dir).map_err(|source| LoadError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut paths: Vec<PathBuf> = read_dir
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    paths.sort();

    let mut catalog = Catalog::default();
    let mut errors = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(source) => {
                errors.push(LoadError::Io { path, source });
                continue;
            }
        };
        match parse_category(&name, &text) {
            Ok(entry) => catalog.categories.push(entry),
            Err(e) => errors.push(e),
        }
    }
    catalog
        .categories
        .sort_by_key(|c| (c.category.order, c.category.id.clone()));
    Ok((catalog, errors))
}
