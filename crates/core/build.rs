//! Embeds every `catalog/*.toml` into the library so the exe works without the
//! repository. Files are only concatenated here; parsing and validation happen
//! at run time and in tests, so one bad file gives a clear error instead of a
//! failed build.

use std::fmt::Write as _;
use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../catalog");
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "toml"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();

    let mut code = String::from("pub static FILES: &[(&str, &str)] = &[\n");
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
        let name = f.file_name().unwrap().to_string_lossy();
        let path = f.canonicalize().unwrap_or_else(|_| f.clone());
        writeln!(code, "    ({name:?}, include_str!({:?})),", path.display().to_string()).unwrap();
    }
    code.push_str("];\n");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("embedded_catalog.rs");
    std::fs::write(out, code).unwrap();
}
