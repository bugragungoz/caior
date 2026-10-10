//! Getting installers onto disk: cache layout, download with resume and
//! fallback URLs, zip extraction, and verification (SHA256 or Authenticode).

pub mod authenticode;

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::catalog::{Package, Source};

pub use authenticode::Signature;

const USER_AGENT: &str = concat!("CAIOR/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("package {0} has no download source")]
    NoSource(String),
    #[error("download failed: {0}")]
    Http(String),
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Verify(#[from] VerifyError),
    #[error("cannot unpack {entry:?}: {message}")]
    Extract { entry: String, message: String },
    #[error("file error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VerifyError {
    #[error("checksum does not match (expected {expected}, got {actual})")]
    HashMismatch { expected: String, actual: String },
    #[error("the digital signature is not valid (code {status:#x})")]
    BadSignature { status: i32 },
    #[error("signed by {signer:?}, which is not an accepted publisher ({allowed})")]
    SignerNotAllowed { signer: String, allowed: String },
    #[error("the source has neither a checksum nor an accepted signer")]
    NothingToCheck,
}

/// Proof that a file is what the catalog expects.
#[derive(Debug, Clone, Serialize)]
pub struct Verified {
    pub size: u64,
    pub sha256: String,
    pub signature: Option<Signature>,
}

/// A ready-to-run installer on disk.
#[derive(Debug, Clone, Serialize)]
pub struct Fetched {
    /// The file to run (the zip entry when the source is an archive).
    pub installer: PathBuf,
    /// The downloaded file as stored in the cache (the zip itself for archives).
    pub download: PathBuf,
    pub verified: Verified,
    pub from_cache: bool,
}

/// `<root>/<package id>/<filename>`. One folder per package keeps untrusted
/// downloads apart from each other.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Cache { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn download_path(&self, pkg: &Package, src: &Source) -> PathBuf {
        self.root.join(&pkg.id).join(&src.filename)
    }

    pub fn installer_path(&self, pkg: &Package, src: &Source) -> PathBuf {
        match &src.extract {
            Some(x) => self.root.join(&pkg.id).join(entry_file_name(&x.entry)),
            None => self.download_path(pkg, src),
        }
    }
}

fn entry_file_name(entry: &str) -> &str {
    entry.rsplit(['/', '\\']).next().unwrap_or(entry)
}

pub fn sha256_file(path: &Path) -> io::Result<(u64, String)> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((size, hex(&hasher.finalize())))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Checks a downloaded file (and the installer inside it, for archives).
/// Pinned sources must match their SHA256. Evergreen sources must carry a valid
/// Authenticode signature from an accepted signer. Pinned sources with a signer
/// list are signature-checked too when the file is signed, but a hash match is
/// what decides, so old installers with expired certificates still pass.
pub fn verify(src: &Source, download: &Path, installer: &Path) -> Result<Verified, FetchError> {
    let (size, sha256) = sha256_file(download)?;
    let mut signature = None;
    match (&src.sha256, src.evergreen) {
        (Some(expected), false) => {
            if !expected.eq_ignore_ascii_case(&sha256) {
                return Err(VerifyError::HashMismatch { expected: expected.clone(), actual: sha256 }.into());
            }
            if !src.signer.is_empty() {
                signature = Some(authenticode::check(installer));
            }
        }
        _ => {
            if src.signer.is_empty() {
                return Err(VerifyError::NothingToCheck.into());
            }
            let sig = authenticode::check(installer);
            if !sig.valid {
                return Err(VerifyError::BadSignature { status: sig.status }.into());
            }
            let signer = sig.signer.clone().unwrap_or_default();
            if !src.signer.iter().any(|s| s == &signer) {
                return Err(VerifyError::SignerNotAllowed { signer, allowed: src.signer.join(", ") }.into());
            }
            signature = Some(sig);
        }
    }
    Ok(Verified { size, sha256, signature })
}

pub fn http_client() -> reqwest::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .redirect(reqwest::redirect::Policy::limited(15))
        .connect_timeout(Duration::from_secs(30))
        .timeout(None)
        .build()
}

/// Download progress: bytes so far and the total when the server sends it.
pub type ProgressFn<'a> = dyn FnMut(u64, Option<u64>) + 'a;

/// Downloads `url` into `dest`, resuming from `dest.part` when the server supports ranges.
fn download(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    progress: &mut ProgressFn,
) -> Result<(), FetchError> {
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir)?;
    }
    let part = dest.with_file_name(format!(
        "{}.part",
        dest.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    ));
    let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let mut req = client.get(url);
    if have > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let mut resp = req.send().map_err(|e| FetchError::Http(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(FetchError::Http(format!("HTTP {status} from {url}")));
    }
    let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let mut done = if resumed { have } else { 0 };
    let total = resp.content_length().map(|n| n + done);
    let mut out = if resumed {
        OpenOptions::new().append(true).open(&part)?
    } else {
        File::create(&part)?
    };
    let mut buf = vec![0u8; 1 << 16];
    progress(done, total);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(FetchError::Cancelled);
        }
        let n = resp.read(&mut buf).map_err(|e| FetchError::Http(e.to_string()))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, total);
    }
    out.flush()?;
    drop(out);
    if let Some(t) = total
        && done != t {
            return Err(FetchError::Http(format!("connection closed after {done} of {t} bytes")));
        }
    fs::rename(&part, dest)?;
    Ok(())
}

fn extract(zip_path: &Path, entry: &str, dest: &Path) -> Result<(), FetchError> {
    let err = |message: String| FetchError::Extract { entry: entry.to_string(), message };
    let mut archive = zip::ZipArchive::new(File::open(zip_path)?).map_err(|e| err(e.to_string()))?;
    let wanted = entry.replace('\\', "/").to_ascii_lowercase();
    let index = (0..archive.len())
        .find(|&i| archive.by_index(i).is_ok_and(|f| f.name().to_ascii_lowercase() == wanted))
        .ok_or_else(|| err("not found in the archive".into()))?;
    let mut f = archive.by_index(index).map_err(|e| err(e.to_string()))?;
    let tmp = dest.with_extension("unpack");
    let mut w = File::create(&tmp)?;
    io::copy(&mut f, &mut w)?;
    drop(w);
    fs::rename(&tmp, dest)?;
    Ok(())
}

/// Makes sure the package's installer is in the cache and verified. A cached
/// copy that still verifies is used as is; otherwise each URL is tried in turn.
pub fn fetch(
    client: &reqwest::blocking::Client,
    pkg: &Package,
    cache: &Cache,
    cancel: &AtomicBool,
    progress: &mut ProgressFn,
) -> Result<Fetched, FetchError> {
    let src = pkg.source.as_ref().ok_or_else(|| FetchError::NoSource(pkg.id.clone()))?;
    let download_path = cache.download_path(pkg, src);
    let installer = cache.installer_path(pkg, src);

    let prepare = |from_cache: bool| -> Result<Fetched, FetchError> {
        if let Some(x) = &src.extract {
            extract(&download_path, &x.entry, &installer)?;
        }
        let verified = verify(src, &download_path, &installer)?;
        Ok(Fetched { installer: installer.clone(), download: download_path.clone(), verified, from_cache })
    };

    if download_path.is_file() {
        if let Ok(f) = prepare(true) {
            return Ok(f);
        }
        let _ = fs::remove_file(&download_path);
    }

    let mut last = None;
    for url in std::iter::once(&src.url).chain(&src.fallback_urls) {
        match download(client, url, &download_path, cancel, progress).and_then(|()| prepare(false)) {
            Ok(f) => return Ok(f),
            Err(FetchError::Cancelled) => return Err(FetchError::Cancelled),
            Err(e) => {
                // A bad file must not survive to be "resumed" or reused.
                if matches!(e, FetchError::Verify(_)) {
                    let _ = fs::remove_file(&download_path);
                }
                last = Some(e);
            }
        }
    }
    Err(last.unwrap_or_else(|| FetchError::NoSource(pkg.id.clone())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(sha: Option<&str>, evergreen: bool, signer: &[&str]) -> Source {
        Source {
            url: "https://example.com/x.exe".into(),
            fallback_urls: vec![],
            filename: "x.exe".into(),
            size: 0,
            sha256: sha.map(str::to_string),
            evergreen,
            signer: signer.iter().map(|s| s.to_string()).collect(),
            online_only: false,
            extract: None,
        }
    }

    fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("caior-test-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn pinned_hash_must_match() {
        let p = temp_file("a.bin", b"hello");
        let good = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert!(verify(&source(Some(good), false, &[]), &p, &p).is_ok());
        let bad = "0".repeat(64);
        assert!(matches!(
            verify(&source(Some(&bad), false, &[]), &p, &p),
            Err(FetchError::Verify(VerifyError::HashMismatch { .. }))
        ));
    }

    #[test]
    fn evergreen_unsigned_file_is_rejected() {
        let p = temp_file("b.exe", b"not really an exe");
        assert!(matches!(
            verify(&source(None, true, &["Microsoft Corporation"]), &p, &p),
            Err(FetchError::Verify(VerifyError::BadSignature { .. }))
        ));
        assert!(matches!(
            verify(&source(None, true, &[]), &p, &p),
            Err(FetchError::Verify(VerifyError::NothingToCheck))
        ));
    }
}
