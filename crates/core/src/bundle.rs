//! Offline packages. Two formats, both readable by the same exe:
//!
//! - Folder: `caior.exe` next to `caior-packages/`, laid out like the download
//!   cache (`<id>/<filename>`) plus `bundle.json`. The folder simply serves as
//!   the cache, so nothing is copied before installing.
//! - Single exe: the exe's own bytes, then every installer, then the JSON
//!   index, then a 24-byte footer: `CAIORPK1`, index offset (u64 LE), index
//!   length (u64 LE). Selected files are extracted into the cache on demand.
//!
//! Files are verified again after extraction (SHA256 or signature), so a
//! damaged bundle can never install a wrong file.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::catalog::Catalog;
use crate::fetch::{Cache, sha256_file};

pub const MAGIC: &[u8; 8] = b"CAIORPK1";
pub const FOLDER_NAME: &str = "caior-packages";
const FOOTER_LEN: u64 = 24;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleFile {
    pub id: String,
    pub filename: String,
    pub size: u64,
    pub sha256: String,
    /// Byte offset inside a single-exe bundle; 0 for folder bundles.
    #[serde(default)]
    pub offset: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleIndex {
    pub format: u32,
    pub app_version: String,
    /// Seconds since 1970, when the bundle was written.
    pub created: u64,
    pub files: Vec<BundleFile>,
}

#[derive(Debug, Clone)]
pub enum Bundle {
    Folder { dir: PathBuf, index: BundleIndex },
    Exe { path: PathBuf, index: BundleIndex },
}

impl Bundle {
    pub fn index(&self) -> &BundleIndex {
        match self {
            Bundle::Folder { index, .. } | Bundle::Exe { index, .. } => index,
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.index().files.iter().any(|f| f.id == id)
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Reads the index appended to an exe, if there is one.
pub fn read_exe_index(path: &Path) -> io::Result<Option<BundleIndex>> {
    Ok(read_footer(path)?.map(|(index, _)| index))
}

/// Length of the program part of an exe: everything before the appended
/// installers, or before the index when the bundle holds no files.
pub fn program_len(exe: &Path) -> io::Result<u64> {
    Ok(match read_footer(exe)? {
        Some((index, index_offset)) => index.files.iter().map(|f| f.offset).fold(index_offset, u64::min),
        None => fs::metadata(exe)?.len(),
    })
}

/// The appended index and where it starts.
fn read_footer(path: &Path) -> io::Result<Option<(BundleIndex, u64)>> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    if len < FOOTER_LEN {
        return Ok(None);
    }
    f.seek(SeekFrom::End(-(FOOTER_LEN as i64)))?;
    let mut footer = [0u8; FOOTER_LEN as usize];
    f.read_exact(&mut footer)?;
    if &footer[..8] != MAGIC {
        return Ok(None);
    }
    let offset = u64::from_le_bytes(footer[8..16].try_into().unwrap());
    let size = u64::from_le_bytes(footer[16..24].try_into().unwrap());
    if offset.checked_add(size).is_none_or(|end| end > len - FOOTER_LEN) || size > 64 * 1024 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bundle index out of range"));
    }
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; size as usize];
    f.read_exact(&mut buf)?;
    let index = serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some((index, offset)))
}

/// Finds the bundle the running exe belongs to: appended data first, then a
/// `caior-packages` folder next to it.
pub fn current() -> Option<Bundle> {
    let exe = std::env::current_exe().ok()?;
    if let Ok(Some(index)) = read_exe_index(&exe) {
        return Some(Bundle::Exe { path: exe, index });
    }
    let dir = exe.parent()?.join(FOLDER_NAME);
    let text = fs::read_to_string(dir.join("bundle.json")).ok()?;
    let index = serde_json::from_str(&text).ok()?;
    Some(Bundle::Folder { dir, index })
}

/// The cache to use with a bundle: the folder itself, or the normal cache for
/// single-exe bundles (files are extracted into it by `prepare`).
pub fn cache_dir(bundle: &Bundle, default: &Path) -> PathBuf {
    match bundle {
        Bundle::Folder { dir, .. } => dir.clone(),
        Bundle::Exe { .. } => default.to_path_buf(),
    }
}

/// What `prepare` did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Prepared {
    pub extracted: usize,
    /// Packages whose bytes in the bundle do not match their hash. They are
    /// left out; the session downloads them when it can.
    pub damaged: Vec<String>,
}

/// Extracts the given packages from a single-exe bundle into the cache. Files
/// already present with the right hash are kept. A damaged file does not stop
/// the others from being extracted.
pub fn prepare(bundle: &Bundle, ids: &[String], cache: &Path) -> io::Result<Prepared> {
    let Bundle::Exe { path, index } = bundle else { return Ok(Prepared::default()) };
    let mut src = File::open(path)?;
    let mut done = Prepared::default();
    for f in index.files.iter().filter(|f| ids.contains(&f.id)) {
        let dest = cache.join(&f.id).join(&f.filename);
        if dest.is_file() && sha256_file(&dest).map(|(_, h)| h == f.sha256).unwrap_or(false) {
            continue;
        }
        fs::create_dir_all(dest.parent().unwrap())?;
        let tmp = dest.with_extension("unpack");
        src.seek(SeekFrom::Start(f.offset))?;
        let mut out = BufWriter::new(File::create(&tmp)?);
        io::copy(&mut (&mut src).take(f.size), &mut out)?;
        out.flush()?;
        drop(out);
        let (_, hash) = sha256_file(&tmp)?;
        if hash != f.sha256 {
            let _ = fs::remove_file(&tmp);
            done.damaged.push(f.id.clone());
            continue;
        }
        fs::rename(&tmp, &dest)?;
        done.extracted += 1;
    }
    Ok(done)
}

/// The cached download of each package, ready to be bundled.
pub fn collect(catalog: &Catalog, ids: &[String], cache: &Cache) -> io::Result<Vec<(BundleFile, PathBuf)>> {
    let mut out = Vec::new();
    for id in ids {
        let Some(pkg) = catalog.package(id) else { continue };
        let Some(src) = &pkg.source else { continue };
        if src.online_only {
            continue;
        }
        let path = cache.download_path(pkg, src);
        if !path.is_file() {
            continue;
        }
        let (size, sha256) = sha256_file(&path)?;
        out.push((BundleFile { id: id.clone(), filename: src.filename.clone(), size, sha256, offset: 0 }, path));
    }
    Ok(out)
}

/// Copies the program part of `exe` (without any appended bundle) to `out`.
fn copy_program(exe: &Path, out: &Path) -> io::Result<()> {
    let len = program_len(exe)?;
    let mut w = BufWriter::new(File::create(out)?);
    io::copy(&mut File::open(exe)?.take(len), &mut w)?;
    w.flush()
}

/// Writes `caior.exe` + `caior-packages/` into `out_dir`. When `exe` is a
/// single-file bundle itself, only its program part is copied: the folder
/// carries the installers, and an appended bundle would hide it.
pub fn write_folder(exe: &Path, files: &[(BundleFile, PathBuf)], out_dir: &Path) -> io::Result<PathBuf> {
    let pkgs = out_dir.join(FOLDER_NAME);
    fs::create_dir_all(&pkgs)?;
    copy_program(exe, &out_dir.join("caior.exe"))?;
    for (f, path) in files {
        let dest = pkgs.join(&f.id).join(&f.filename);
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::copy(path, &dest)?;
    }
    let index = BundleIndex {
        format: 1,
        app_version: env!("CARGO_PKG_VERSION").into(),
        created: now_secs(),
        files: files.iter().map(|(f, _)| f.clone()).collect(),
    };
    fs::write(pkgs.join("bundle.json"), serde_json::to_vec_pretty(&index)?)?;
    Ok(out_dir.join("caior.exe"))
}

/// Writes one exe with every installer appended. `exe` must be a plain CAIOR
/// exe; if it already carries a bundle, only its program part is copied.
pub fn write_exe(exe: &Path, files: &[(BundleFile, PathBuf)], out: &Path) -> io::Result<()> {
    let program_len = program_len(exe)?;
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = out.with_extension("partial");
    let mut w = BufWriter::new(File::create(&tmp)?);
    io::copy(&mut File::open(exe)?.take(program_len), &mut w)?;
    let mut offset = program_len;
    let mut index = BundleIndex { format: 1, app_version: env!("CARGO_PKG_VERSION").into(), created: now_secs(), files: Vec::new() };
    // Identical files (several packages unpacked from one archive) are stored once.
    let mut stored: std::collections::HashMap<String, (u64, u64)> = std::collections::HashMap::new();
    for (f, path) in files {
        if let Some(&(at, size)) = (!f.sha256.is_empty()).then(|| stored.get(&f.sha256)).flatten() {
            index.files.push(BundleFile { offset: at, size, ..f.clone() });
            continue;
        }
        let mut r = BufReader::new(File::open(path)?);
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        let mut size = 0u64;
        loop {
            let n = r.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            w.write_all(&buf[..n])?;
            size += n as u64;
        }
        let sha256: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        stored.insert(sha256.clone(), (offset, size));
        index.files.push(BundleFile { offset, size, sha256, ..f.clone() });
        offset += size;
    }
    let json = serde_json::to_vec(&index)?;
    w.write_all(&json)?;
    w.write_all(MAGIC)?;
    w.write_all(&offset.to_le_bytes())?;
    w.write_all(&(json.len() as u64).to_le_bytes())?;
    w.flush()?;
    drop(w);
    fs::rename(&tmp, out)?;
    Ok(())
}

/// The catalog as an offline package sees it: only the packages it carries.
/// Dependencies and conflicts on packages it does not carry are dropped (they
/// cannot be installed from it anyway); categories left empty disappear.
pub fn trim_catalog(catalog: &Catalog, bundle: &Bundle) -> Catalog {
    let carried: std::collections::HashSet<&str> = bundle.index().files.iter().map(|f| f.id.as_str()).collect();
    let mut out = catalog.clone();
    for c in &mut out.categories {
        c.packages.retain(|p| carried.contains(p.id.as_str()));
        for p in &mut c.packages {
            p.depends.retain(|d| carried.contains(d.as_str()));
            p.conflicts.retain(|d| carried.contains(d.as_str()));
        }
    }
    out.categories.retain(|c| !c.packages.is_empty());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimmed_catalog_holds_only_carried_packages() {
        let text = r#"
[category]
id = "t"
name = { en = "T" }
order = 1
icon = "x"

[[package]]
id = "netfx"
name = "NetFx3"
version = "3.5"
arch = "neutral"
order = 1
[package.install]
kind = "dism-feature"
feature = "NetFx3"
[package.detect]
kind = "none"

[[package]]
id = "xna"
name = "XNA"
version = "3.1"
arch = "x86"
order = 2
depends = ["netfx"]
[package.source]
url = "https://example.com/x.msi"
filename = "x.msi"
[package.install]
kind = "msi"
[package.detect]
kind = "none"

[[package]]
id = "other"
name = "Other"
version = "1"
arch = "x86"
order = 3
[package.source]
url = "https://example.com/o.exe"
filename = "o.exe"
[package.install]
kind = "exe"
args = ""
[package.detect]
kind = "none"

[category]
"#;
        let text = text.trim_end().trim_end_matches("[category]");
        let catalog = Catalog { categories: vec![crate::catalog::parse_category("t.toml", text).unwrap()] };
        let index = BundleIndex {
            format: 1,
            app_version: String::new(),
            created: 0,
            files: vec![BundleFile { id: "xna".into(), filename: "x.msi".into(), size: 1, sha256: String::new(), offset: 0 }],
        };
        let trimmed = trim_catalog(&catalog, &Bundle::Folder { dir: PathBuf::new(), index });
        let ids: Vec<&str> = trimmed.packages().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["xna"]);
        assert!(trimmed.package("xna").unwrap().depends.is_empty(), "dependency outside the package dropped");
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("caior-bundle-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn exe_bundle_round_trip() {
        let d = tmp("rt");
        let exe = d.join("caior.exe");
        fs::write(&exe, b"MZ fake program").unwrap();
        let a = d.join("a.exe");
        let b = d.join("b.msi");
        fs::write(&a, b"first installer").unwrap();
        fs::write(&b, vec![7u8; 100_000]).unwrap();
        let files = vec![
            (BundleFile { id: "a".into(), filename: "a.exe".into(), size: 0, sha256: String::new(), offset: 0 }, a),
            (BundleFile { id: "b".into(), filename: "b.msi".into(), size: 0, sha256: String::new(), offset: 0 }, b),
        ];
        let out = d.join("out").join("bundle.exe");
        write_exe(&exe, &files, &out).unwrap();

        let index = read_exe_index(&out).unwrap().unwrap();
        assert_eq!(index.files.len(), 2);
        assert_eq!(index.files[0].offset, 15);
        assert!(read_exe_index(&exe).unwrap().is_none(), "a plain exe has no bundle");

        let bundle = Bundle::Exe { path: out.clone(), index };
        let cache = d.join("cache");
        assert_eq!(prepare(&bundle, &["b".into()], &cache).unwrap().extracted, 1);
        assert_eq!(fs::read(cache.join("b").join("b.msi")).unwrap(), vec![7u8; 100_000]);
        assert_eq!(prepare(&bundle, &["b".into()], &cache).unwrap().extracted, 0, "already extracted");

        // Re-bundling from a bundled exe keeps only the program part.
        let again = d.join("again.exe");
        write_exe(&out, &files[..1], &again).unwrap();
        let idx = read_exe_index(&again).unwrap().unwrap();
        assert_eq!(idx.files.len(), 1);
        assert_eq!(idx.files[0].offset, 15);
    }

    #[test]
    fn identical_files_are_stored_once() {
        let d = tmp("dedupe");
        let exe = d.join("caior.exe");
        fs::write(&exe, b"MZ").unwrap();
        let z = d.join("deps.zip");
        fs::write(&z, vec![1u8; 50_000]).unwrap();
        let (_, h) = sha256_file(&z).unwrap();
        let entry = |id: &str| (BundleFile { id: id.into(), filename: "deps.zip".into(), size: 50_000, sha256: h.clone(), offset: 0 }, z.clone());
        let out = d.join("b.exe");
        write_exe(&exe, &[entry("a"), entry("b"), entry("c")], &out).unwrap();
        assert!(fs::metadata(&out).unwrap().len() < 60_000, "stored once");
        let index = read_exe_index(&out).unwrap().unwrap();
        assert_eq!(index.files.len(), 3);
        let bundle = Bundle::Exe { path: out, index };
        assert_eq!(prepare(&bundle, &["a".into(), "c".into()], &d.join("cache")).unwrap().extracted, 2);
    }

    #[test]
    fn empty_bundle_keeps_its_program_when_rebundled() {
        let d = tmp("empty");
        let exe = d.join("caior.exe");
        fs::write(&exe, b"MZ program").unwrap();
        let empty = d.join("empty.exe");
        write_exe(&exe, &[], &empty).unwrap();
        assert_eq!(program_len(&empty).unwrap(), 10);
        let a = d.join("a.exe");
        fs::write(&a, b"installer").unwrap();
        let again = d.join("again.exe");
        let file = BundleFile { id: "a".into(), filename: "a.exe".into(), size: 0, sha256: String::new(), offset: 0 };
        write_exe(&empty, &[(file, a)], &again).unwrap();
        assert!(fs::read(&again).unwrap().starts_with(b"MZ program"));
        assert_eq!(read_exe_index(&again).unwrap().unwrap().files[0].offset, 10);
    }

    #[test]
    fn folder_bundle_gets_the_plain_program() {
        let d = tmp("folder");
        let exe = d.join("caior.exe");
        fs::write(&exe, b"MZ program").unwrap();
        let a = d.join("a.exe");
        fs::write(&a, b"installer").unwrap();
        let file = BundleFile { id: "a".into(), filename: "a.exe".into(), size: 0, sha256: String::new(), offset: 0 };
        let single = d.join("single.exe");
        write_exe(&exe, &[(file.clone(), a.clone())], &single).unwrap();
        let out = write_folder(&single, &[(file, a)], &d.join("out")).unwrap();
        assert_eq!(fs::read(&out).unwrap(), b"MZ program");
        assert!(d.join("out").join(FOLDER_NAME).join("a").join("a.exe").is_file());
    }

    #[test]
    fn damaged_payload_is_rejected() {
        let d = tmp("dmg");
        let exe = d.join("caior.exe");
        fs::write(&exe, b"MZ").unwrap();
        let a = d.join("a.exe");
        fs::write(&a, b"installer bytes").unwrap();
        let out = d.join("b.exe");
        write_exe(&exe, &[(BundleFile { id: "a".into(), filename: "a.exe".into(), size: 0, sha256: String::new(), offset: 0 }, a)], &out).unwrap();
        let mut bytes = fs::read(&out).unwrap();
        bytes[3] ^= 0xff; // flip a payload byte
        fs::write(&out, bytes).unwrap();
        let index = read_exe_index(&out).unwrap().unwrap();
        let done = prepare(&Bundle::Exe { path: out, index }, &["a".into()], &d.join("cache")).unwrap();
        assert_eq!(done, Prepared { extracted: 0, damaged: vec!["a".into()] });
        assert!(!d.join("cache").join("a").join("a.exe").exists());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// One portable exe with everything inside.
    Exe,
    /// `caior.exe` plus a `caior-packages` folder.
    Folder,
}

/// Bundles the already downloaded packages. `out` is the exe path for
/// `Format::Exe` and the target folder for `Format::Folder`. Returns how many
/// packages went in and the path of the exe to run.
pub fn pack(catalog: &Catalog, ids: &[String], cache: &Cache, format: Format, out: &Path) -> io::Result<(usize, PathBuf)> {
    let exe = std::env::current_exe()?;
    let files = collect(catalog, ids, cache)?;
    let path = match format {
        Format::Exe => {
            write_exe(&exe, &files, out)?;
            out.to_path_buf()
        }
        Format::Folder => write_folder(&exe, &files, out)?,
    };
    Ok((files.len(), path))
}

/// The cache a session should use: the offline package this exe belongs to,
/// or `default` when it is a plain exe.
pub fn effective_cache(default: &Path) -> PathBuf {
    current().map(|b| cache_dir(&b, default)).unwrap_or_else(|| default.to_path_buf())
}
