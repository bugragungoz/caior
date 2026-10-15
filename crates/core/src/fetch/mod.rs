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
    #[error("download failed: HTTP {status} from {url}")]
    Status { status: u16, url: String },
    #[error("cancelled")]
    Cancelled,
    #[error("{0} is not in this offline package")]
    NotInPackage(String),
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

/// How long a download may go without receiving a byte before it is
/// dropped (and resumed by the next attempt).
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// Attempts per URL for network errors and server-side (5xx) failures.
const ATTEMPTS_PER_URL: u32 = 3;

pub fn http_client() -> reqwest::Result<reqwest::blocking::Client> {
    // The blocking client applies `timeout` to sending the request and to
    // each read of the body, not to the whole download, so large files are
    // fine and a stalled connection fails instead of hanging forever.
    reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .redirect(reqwest::redirect::Policy::limited(15))
        .connect_timeout(Duration::from_secs(30))
        .timeout(STALL_TIMEOUT)
        .build()
}

/// Download progress: bytes so far and the total when the server sends it.
pub type ProgressFn<'a> = dyn FnMut(u64, Option<u64>) + 'a;

/// Downloads `url` into `dest`, resuming from `dest.part` when the server
/// supports ranges. Returns whether the download continued a partial file.
fn download(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    progress: &mut ProgressFn,
) -> Result<bool, FetchError> {
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir)?;
    }
    let part = dest.with_file_name(format!(
        "{}.part",
        dest.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    ));
    let send = |from: u64| {
        let mut req = client.get(url);
        if from > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={from}-"));
        }
        req.send().map_err(|e| FetchError::Http(e.to_string()))
    };
    let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let mut resp = send(have)?;
    if have > 0 && resp.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        // The partial file does not fit the file the server has now (it is
        // complete already, or the URL serves a newer build). Start over;
        // keeping it would fail every later attempt the same way.
        drop(resp);
        let _ = fs::remove_file(&part);
        resp = send(0)?;
    }
    let status = resp.status();
    if !status.is_success() {
        return Err(FetchError::Status { status: status.as_u16(), url: url.to_string() });
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
    Ok(resumed)
}

/// Worth another try on the same URL: the connection broke or the server had
/// a temporary problem. A 404 or 403 will not change.
fn is_transient(e: &FetchError) -> bool {
    match e {
        FetchError::Http(_) => true,
        FetchError::Status { status, .. } => *status >= 500 || *status == 408 || *status == 429,
        _ => false,
    }
}

/// Sleeps in small steps so a cancel request is not held up.
fn pause(d: Duration, cancel: &AtomicBool) {
    let until = std::time::Instant::now() + d;
    while std::time::Instant::now() < until && !cancel.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Writes the archive entry `entry` (matched case-insensitively) to `out`.
/// The zip reader checks the entry's CRC as it reads.
pub(crate) fn unzip_entry<R: Read + io::Seek>(archive: R, entry: &str, out: &mut impl Write) -> Result<(), FetchError> {
    let err = |message: String| FetchError::Extract { entry: entry.to_string(), message };
    let mut archive = zip::ZipArchive::new(archive).map_err(|e| err(e.to_string()))?;
    let wanted = entry.replace('\\', "/").to_ascii_lowercase();
    let index = (0..archive.len())
        .find(|&i| archive.by_index(i).is_ok_and(|f| f.name().to_ascii_lowercase() == wanted))
        .ok_or_else(|| err("not found in the archive".into()))?;
    let mut f = archive.by_index(index).map_err(|e| err(e.to_string()))?;
    io::copy(&mut f, out).map_err(|e| err(e.to_string()))?;
    Ok(())
}

fn extract(zip_path: &Path, entry: &str, dest: &Path) -> Result<(), FetchError> {
    let tmp = dest.with_extension("unpack");
    // Never write into a file someone else left there; start from a new one.
    let _ = fs::remove_file(&tmp);
    let mut w = File::create_new(&tmp)?;
    unzip_entry(File::open(zip_path)?, entry, &mut w)?;
    drop(w);
    fs::rename(&tmp, dest)?;
    Ok(())
}

/// Offline packages: the cached copy (from the package) or nothing. Never
/// touches the network; the file is verified like a download.
pub fn from_cache(pkg: &Package, cache: &Cache) -> Result<Fetched, FetchError> {
    let src = pkg.source.as_ref().ok_or_else(|| FetchError::NoSource(pkg.id.clone()))?;
    let download = cache.download_path(pkg, src);
    if !download.is_file() {
        return Err(FetchError::NotInPackage(pkg.id.clone()));
    }
    let installer = cache.installer_path(pkg, src);
    if let Some(x) = &src.extract {
        extract(&download, &x.entry, &installer)?;
    }
    let verified = verify(src, &download, &installer)?;
    Ok(Fetched { installer, download, verified, from_cache: true })
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
        for attempt in 1..=ATTEMPTS_PER_URL {
            if cancel.load(Ordering::Relaxed) {
                return Err(FetchError::Cancelled);
            }
            let retry = match download(client, url, &download_path, cancel, progress) {
                Ok(resumed) => match prepare(false) {
                    Ok(f) => return Ok(f),
                    Err(e) => {
                        // A bad file must not survive to be "resumed" or reused.
                        let bad_file = matches!(e, FetchError::Verify(_) | FetchError::Extract { .. });
                        if bad_file {
                            let _ = fs::remove_file(&download_path);
                        }
                        last = Some(e);
                        // The first part may have come from an older build of an
                        // evergreen file: one more try from scratch.
                        resumed && bad_file
                    }
                },
                Err(FetchError::Cancelled) => return Err(FetchError::Cancelled),
                Err(e) => {
                    let retry = is_transient(&e);
                    last = Some(e);
                    retry
                }
            };
            if !retry || attempt == ATTEMPTS_PER_URL {
                break;
            }
            pause(Duration::from_secs(2 * u64::from(attempt)), cancel);
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

    /// Serves `body` on 127.0.0.1; requests with a Range header get 416.
    fn serve_without_ranges(body: &'static [u8]) -> String {
        use std::io::{BufRead, BufReader};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut ranged = false;
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 0) && line != "\r\n" {
                    ranged |= line.to_ascii_lowercase().starts_with("range:");
                    line.clear();
                }
                let mut out = stream;
                let reply = if ranged {
                    "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                } else {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())
                };
                let _ = out.write_all(reply.as_bytes());
                if !ranged {
                    let _ = out.write_all(body);
                }
            }
        });
        format!("http://{addr}/file.bin")
    }

    #[test]
    fn stale_partial_file_is_dropped_on_416() {
        let url = serve_without_ranges(b"fresh content");
        let dir = temp_file("x", b"").parent().unwrap().join("dl416");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("file.bin");
        fs::write(dir.join("file.bin.part"), b"leftover bytes that no longer fit").unwrap();
        let client = reqwest::blocking::Client::builder().no_proxy().build().unwrap();
        let resumed = download(&client, &url, &dest, &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        assert!(!resumed);
        assert_eq!(fs::read(&dest).unwrap(), b"fresh content");
        assert!(!dir.join("file.bin.part").exists());
    }

    #[test]
    fn only_network_and_server_errors_are_retried() {
        assert!(is_transient(&FetchError::Http("connection reset".into())));
        assert!(is_transient(&FetchError::Status { status: 503, url: String::new() }));
        assert!(!is_transient(&FetchError::Status { status: 404, url: String::new() }));
        assert!(!is_transient(&FetchError::Cancelled));
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
