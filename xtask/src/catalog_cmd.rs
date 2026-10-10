use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use caior_core::catalog::{self, Catalog, Package, Severity, ValidateOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Selection;

const USER_AGENT: &str = concat!("CAIOR-xtask/", env!("CARGO_PKG_VERSION"));

fn load(sel: &Selection) -> Result<Catalog> {
    let (catalog, errors) = catalog::load_dir(&sel.dir)
        .with_context(|| format!("loading catalog from {}", sel.dir.display()))?;
    for e in &errors {
        eprintln!("error: {e}");
    }
    if !errors.is_empty() {
        bail!("{} catalog file(s) failed to parse", errors.len());
    }
    Ok(catalog)
}

fn file_selected(sel: &Selection, file: &str) -> bool {
    sel.files.is_empty() || sel.files.iter().any(|f| f.eq_ignore_ascii_case(file))
}

fn id_selected(sel: &Selection, id: &str) -> bool {
    sel.ids.is_empty()
        || sel.ids.iter().any(|pat| match pat.strip_suffix('*') {
            Some(prefix) => id.starts_with(prefix),
            None => pat == id,
        })
}

/// Selected packages with the file each came from.
fn selected<'a>(catalog: &'a Catalog, sel: &Selection) -> Vec<(&'a str, &'a Package)> {
    catalog
        .categories
        .iter()
        .filter(|c| file_selected(sel, &c.file))
        .flat_map(|c| c.packages.iter().map(move |p| (c.file.as_str(), p)))
        .filter(|(_, p)| id_selected(sel, &p.id))
        .collect()
}

fn client(timeout: Duration) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .redirect(reqwest::redirect::Policy::limited(15))
        .timeout(timeout)
        .build()?)
}

/// Runs `work` over `items` on `jobs` threads, keeping input order in the output.
fn run_parallel<T: Sync, R: Send>(items: &[T], jobs: usize, work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..jobs.clamp(1, 16) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= items.len() {
                        break;
                    }
                    let r = work(&items[i]);
                    results.lock().unwrap()[i] = Some(r);
                }
            });
        }
    });
    results.into_inner().unwrap().into_iter().map(Option::unwrap).collect()
}

// ---------------------------------------------------------------- check

pub fn check(sel: &Selection, strict: bool) -> Result<()> {
    let (catalog, parse_errors) = catalog::load_dir(&sel.dir)?;
    let mut errors = 0;
    let mut warnings = 0;
    for e in &parse_errors {
        let file = match e {
            catalog::LoadError::Parse { file, .. } => file.clone(),
            catalog::LoadError::Io { path, .. } => path.display().to_string(),
        };
        if file_selected(sel, &file) {
            println!("error: {e}");
            errors += 1;
        }
    }
    for issue in catalog::validate(&catalog, ValidateOptions { strict }) {
        if !file_selected(sel, &issue.file) {
            continue;
        }
        if let Some(p) = &issue.package
            && !id_selected(sel, p) {
                continue;
            }
        println!("{issue}");
        match issue.severity {
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
        }
    }
    let packages = selected(&catalog, sel).len();
    println!("{packages} package(s) checked: {errors} error(s), {warnings} warning(s)");
    if errors > 0 {
        bail!("catalog check failed");
    }
    Ok(())
}

// ---------------------------------------------------------------- probe

#[derive(Serialize)]
struct ProbeResult {
    id: String,
    url: String,
    ok: bool,
    status: Option<u16>,
    final_url: Option<String>,
    size: Option<u64>,
    last_modified: Option<String>,
    content_type: Option<String>,
    error: Option<String>,
}

fn header(resp: &reqwest::blocking::Response, name: &str) -> Option<String> {
    resp.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
}

fn probe_url(client: &reqwest::blocking::Client, id: &str, url: &str) -> ProbeResult {
    let mut result = ProbeResult {
        id: id.to_string(),
        url: url.to_string(),
        ok: false,
        status: None,
        final_url: None,
        size: None,
        last_modified: None,
        content_type: None,
        error: None,
    };
    // HEAD first; some CDNs reject it, so fall back to a one-byte ranged GET.
    let head = client.head(url).send();
    let resp = match head {
        Ok(r) if r.status().is_success() => Ok(r),
        _ => client.get(url).header("Range", "bytes=0-0").send(),
    };
    match resp {
        Ok(r) => {
            result.status = Some(r.status().as_u16());
            result.ok = r.status().is_success();
            result.final_url = Some(r.url().to_string());
            result.last_modified = header(&r, "last-modified");
            result.content_type = header(&r, "content-type");
            result.size = header(&r, "content-range")
                .and_then(|v| v.rsplit('/').next().and_then(|n| n.parse().ok()))
                .or_else(|| {
                    // reqwest reports a HEAD body as 0 bytes, so read the header itself.
                    if r.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                        None
                    } else {
                        header(&r, "content-length").and_then(|v| v.parse().ok())
                    }
                })
                .filter(|&n| n > 0);
        }
        Err(e) => result.error = Some(e.to_string()),
    }
    result
}

pub fn probe(sel: &Selection, json: bool, jobs: usize) -> Result<()> {
    let catalog = load(sel)?;
    let client = client(Duration::from_secs(60))?;
    let targets: Vec<(String, String)> = selected(&catalog, sel)
        .into_iter()
        .filter_map(|(_, p)| p.source.as_ref().map(|s| (p, s)))
        .flat_map(|(p, s)| {
            std::iter::once(s.url.clone())
                .chain(s.fallback_urls.iter().cloned())
                .map(move |u| (p.id.clone(), u))
        })
        .collect();
    let results = run_parallel(&targets, jobs, |(id, url)| probe_url(&client, id, url));
    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        for r in &results {
            let status = r.status.map(|s| s.to_string()).unwrap_or_else(|| "---".into());
            let size = r.size.map(|s| format!("{:.1} MB", s as f64 / 1_048_576.0)).unwrap_or_default();
            println!(
                "{} {status} {:<32} {size:>10}  {}",
                if r.ok { "ok  " } else { "FAIL" },
                r.id,
                r.error.as_deref().or(r.final_url.as_deref()).unwrap_or("")
            );
        }
    }
    let failed = results.iter().filter(|r| !r.ok).count();
    println!("{} URL(s) probed, {failed} failed", results.len());
    if failed > 0 {
        bail!("{failed} URL(s) did not answer with success");
    }
    Ok(())
}

// ---------------------------------------------------------------- refresh

/// What PowerShell reads from a downloaded installer without running it:
/// the Authenticode signature, PE version info, and MSI properties.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(default)]
struct Signature {
    status: String,
    message: String,
    subject: String,
    file_version: String,
    product_version: String,
    product_name: String,
    msi_product_code: String,
    msi_upgrade_code: String,
    msi_product_version: String,
    msi_product_name: String,
    msi_manufacturer: String,
}

impl Signature {
    fn common_name(&self) -> Option<String> {
        common_name(&self.subject)
    }
}

/// Extracts CN from an X.500 subject such as `CN=Microsoft Corporation, O=..., C=US`.
fn common_name(subject: &str) -> Option<String> {
    let start = subject.find("CN=")? + 3;
    let rest = &subject[start..];
    if let Some(quoted) = rest.strip_prefix('"') {
        quoted.split('"').next().map(str::to_string)
    } else {
        Some(rest.split(',').next().unwrap_or(rest).trim().to_string())
    }
}

fn authenticode(path: &Path) -> Result<Signature> {
    // The MSI database is opened read-only (mode 0); nothing is executed.
    let script = r#"
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$p = $env:CAIOR_SIG_PATH
$s = Get-AuthenticodeSignature -LiteralPath $p
$vi = (Get-Item -LiteralPath $p).VersionInfo
$msi = @{}
if ($p -like '*.msi') {
  try {
    $wi = New-Object -ComObject WindowsInstaller.Installer
    $db = $wi.GetType().InvokeMember('OpenDatabase', 'InvokeMethod', $null, $wi, @($p, 0))
    foreach ($prop in 'ProductCode', 'UpgradeCode', 'ProductVersion', 'ProductName', 'Manufacturer') {
      $v = $db.GetType().InvokeMember('OpenView', 'InvokeMethod', $null, $db, @("SELECT Value FROM Property WHERE Property='$prop'"))
      [void]$v.GetType().InvokeMember('Execute', 'InvokeMethod', $null, $v, $null)
      $r = $v.GetType().InvokeMember('Fetch', 'InvokeMethod', $null, $v, $null)
      if ($r) { $msi[$prop] = $r.GetType().InvokeMember('StringData', 'GetProperty', $null, $r, 1) }
      [void]$v.GetType().InvokeMember('Close', 'InvokeMethod', $null, $v, $null)
    }
  } catch {}
}
[pscustomobject]@{
  status = $s.Status.ToString(); message = [string]$s.StatusMessage
  subject = if ($s.SignerCertificate) { $s.SignerCertificate.Subject } else { '' }
  file_version = [string]$vi.FileVersion; product_version = [string]$vi.ProductVersion; product_name = [string]$vi.ProductName
  msi_product_code = [string]$msi['ProductCode']; msi_upgrade_code = [string]$msi['UpgradeCode']
  msi_product_version = [string]$msi['ProductVersion']; msi_product_name = [string]$msi['ProductName']
  msi_manufacturer = [string]$msi['Manufacturer']
} | ConvertTo-Json -Compress
"#;
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("CAIOR_SIG_PATH", path)
        .output()
        .context("running powershell Get-AuthenticodeSignature")?;
    if !out.status.success() {
        bail!("Get-AuthenticodeSignature failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    serde_json::from_slice(&out.stdout).context("parsing signature json")
}

#[derive(Serialize, Clone)]
struct RefreshResult {
    id: String,
    file: String,
    skipped: bool,
    path: Option<PathBuf>,
    final_url: Option<String>,
    size: Option<u64>,
    sha256: Option<String>,
    signature: Option<Signature>,
    signer_ok: Option<bool>,
    error: Option<String>,
}

fn download(client: &reqwest::blocking::Client, url: &str, dest: &Path) -> Result<(u64, String, String)> {
    let mut resp = client.get(url).send()?.error_for_status()?;
    let final_url = resp.url().to_string();
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = dest.with_extension("part");
    let mut file = File::create(&part)?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        size += n as u64;
    }
    file.flush()?;
    drop(file);
    std::fs::rename(&part, dest)?;
    let hash = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok((size, hash, final_url))
}

fn extract_zip_entry(zip_path: &Path, entry: &str, dest_dir: &Path) -> Result<PathBuf> {
    let mut archive = zip::ZipArchive::new(File::open(zip_path)?)?;
    let wanted = entry.replace('\\', "/").to_ascii_lowercase();
    let index = (0..archive.len())
        .find(|&i| archive.by_index(i).is_ok_and(|f| f.name().to_ascii_lowercase() == wanted))
        .with_context(|| format!("{entry:?} not found in {}", zip_path.display()))?;
    let mut f = archive.by_index(index)?;
    let name = entry.rsplit('/').next().unwrap_or(entry);
    let out = dest_dir.join(name);
    let mut w = File::create(&out)?;
    std::io::copy(&mut f, &mut w)?;
    Ok(out)
}

fn refresh_one(client: &reqwest::blocking::Client, file: &str, pkg: &Package, cache: &Path, force: bool) -> RefreshResult {
    let mut r = RefreshResult {
        id: pkg.id.clone(),
        file: file.to_string(),
        skipped: false,
        path: None,
        final_url: None,
        size: None,
        sha256: None,
        signature: None,
        signer_ok: None,
        error: None,
    };
    let Some(src) = &pkg.source else {
        r.skipped = true;
        return r;
    };
    let complete = src.size > 0 && (src.evergreen || src.sha256.is_some()) && !src.signer.is_empty();
    if complete && !force {
        r.skipped = true;
        return r;
    }
    let dir = cache.join(&pkg.id);
    let dest = dir.join(&src.filename);
    let urls = std::iter::once(&src.url).chain(src.fallback_urls.iter());
    let mut last_err = None;
    for url in urls {
        match download(client, url, &dest) {
            Ok((size, hash, final_url)) => {
                r.size = Some(size);
                r.sha256 = Some(hash);
                r.final_url = Some(final_url);
                r.path = Some(dest.clone());
                last_err = None;
                break;
            }
            Err(e) => last_err = Some(format!("{url}: {e:#}")),
        }
    }
    if let Some(e) = last_err {
        r.error = Some(e);
        return r;
    }
    let installer = match &src.extract {
        Some(x) => match extract_zip_entry(&dest, &x.entry, &dir) {
            Ok(p) => p,
            Err(e) => {
                r.error = Some(format!("extract: {e:#}"));
                return r;
            }
        },
        None => dest,
    };
    match authenticode(&installer) {
        Ok(sig) => {
            let cn = sig.common_name();
            r.signer_ok = Some(
                sig.status == "Valid"
                    && cn.as_ref().is_some_and(|cn| src.signer.is_empty() || src.signer.iter().any(|s| s == cn)),
            );
            r.signature = Some(sig);
        }
        Err(e) => r.error = Some(format!("signature: {e:#}")),
    }
    r
}

/// Writes size, sha256 (non-evergreen only) and a missing signer into the TOML file.
fn write_back(dir: &Path, file: &str, results: &[&RefreshResult], catalog: &Catalog) -> Result<usize> {
    let path = dir.join(file);
    let text = std::fs::read_to_string(&path)?;
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    let mut changed = 0;
    let Some(packages) = doc.get_mut("package").and_then(|p| p.as_array_of_tables_mut()) else {
        return Ok(0);
    };
    for table in packages.iter_mut() {
        let Some(id) = table.get("id").and_then(|v| v.as_str()).map(str::to_string) else { continue };
        let Some(r) = results.iter().find(|r| r.id == id) else { continue };
        let Some(pkg) = catalog.package(&id) else { continue };
        let Some(src) = &pkg.source else { continue };
        let (Some(size), Some(hash)) = (r.size, &r.sha256) else { continue };
        let Some(source) = table.get_mut("source").and_then(|s| s.as_table_like_mut()) else { continue };
        source.insert("size", toml_edit::value(size as i64));
        if !src.evergreen {
            source.insert("sha256", toml_edit::value(hash.as_str()));
        }
        if src.signer.is_empty()
            && let Some(sig) = &r.signature
                && sig.status == "Valid"
                    && let Some(cn) = sig.common_name() {
                        let mut arr = toml_edit::Array::new();
                        arr.push(cn);
                        source.insert("signer", toml_edit::value(arr));
                    }
        changed += 1;
    }
    if changed > 0 {
        std::fs::write(&path, doc.to_string())?;
    }
    Ok(changed)
}

pub fn refresh(sel: &Selection, force: bool, jobs: usize, cache: &Path, json: bool) -> Result<()> {
    let catalog = load(sel)?;
    let client = client(Duration::from_secs(1800))?;
    let targets = selected(&catalog, sel);
    let results = run_parallel(&targets, jobs, |(file, pkg)| {
        let r = refresh_one(&client, file, pkg, cache, force);
        if !r.skipped {
            let state = match (&r.error, r.signer_ok) {
                (Some(e), _) => format!("FAIL {e}"),
                (None, Some(true)) => "ok".to_string(),
                (None, _) => format!(
                    "SIGNATURE {}",
                    r.signature.as_ref().map(|s| format!("{} {}", s.status, s.subject)).unwrap_or_default()
                ),
            };
            eprintln!("{:<32} {state}", r.id);
        }
        r
    });

    let mut by_file: BTreeMap<&str, Vec<&RefreshResult>> = BTreeMap::new();
    for r in results.iter().filter(|r| !r.skipped && r.error.is_none()) {
        by_file.entry(r.file.as_str()).or_default().push(r);
    }
    let mut written = 0;
    for (file, rs) in &by_file {
        written += write_back(&sel.dir, file, rs, &catalog)?;
    }

    std::fs::create_dir_all(cache)?;
    // One report per file selection so parallel runs on different files do not collide.
    let suffix: Vec<&str> = sel.files.iter().map(|f| f.trim_end_matches(".toml")).collect();
    let report = match suffix.is_empty() {
        true => cache.join("refresh-report.json"),
        false => cache.join(format!("refresh-report-{}.json", suffix.join("-"))),
    };
    let report_json = serde_json::to_string_pretty(&results)?;
    std::fs::write(&report, &report_json)?;
    if json {
        println!("{report_json}");
    }

    let skipped = results.iter().filter(|r| r.skipped).count();
    let failed = results.iter().filter(|r| r.error.is_some()).count();
    let bad_sig = results.iter().filter(|r| r.error.is_none() && r.signer_ok == Some(false)).count();
    println!(
        "{} package(s): {written} updated, {skipped} skipped, {failed} failed, {bad_sig} signature problem(s). Report: {}",
        results.len(),
        report.display()
    );
    if failed > 0 || bad_sig > 0 {
        bail!("refresh finished with problems");
    }
    Ok(())
}

// ---------------------------------------------------------------- summary

pub fn summary(sel: &Selection) -> Result<()> {
    let catalog = load(sel)?;
    println!("| id | name | arch | install | profiles | source | size |");
    println!("| --- | --- | --- | --- | --- | --- | --- |");
    for (_, p) in selected(&catalog, sel) {
        let profiles: Vec<String> = p.profiles.iter().map(|x| format!("{x:?}").to_lowercase()).collect();
        let (source, size) = match &p.source {
            Some(s) if s.evergreen => ("evergreen", s.size),
            Some(s) if s.sha256.is_some() => ("pinned", s.size),
            Some(s) => ("unpinned", s.size),
            None => ("windows", 0),
        };
        let size = if size > 0 { format!("{:.1} MB", size as f64 / 1_048_576.0) } else { String::new() };
        println!(
            "| {} | {} | {:?} | {:?} | {} | {source} | {size} |",
            p.id,
            p.name,
            p.arch,
            p.install.kind,
            profiles.join(", ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::common_name;

    #[test]
    fn cn_plain_and_quoted() {
        assert_eq!(
            common_name("CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, C=US").as_deref(),
            Some("Microsoft Corporation")
        );
        assert_eq!(common_name("CN=\"Acme, Inc.\", O=Acme").as_deref(), Some("Acme, Inc."));
        assert_eq!(common_name("O=No CN"), None);
    }
}
