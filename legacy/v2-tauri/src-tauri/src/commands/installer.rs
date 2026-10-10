use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

#[derive(Deserialize, Clone)]
pub struct PackageRequest {
    pub id: String,
    pub name: String,
    pub url: String,
    pub drive_url: Option<String>,
    pub installer_type: String,
    pub args: String,
}

#[derive(Serialize, Clone)]
pub struct ProgressEvent {
    pub package_id: String,
    pub stage: String,
    pub progress: u8,
    pub message: String,
}

#[derive(Serialize, Clone)]
pub struct PackageError {
    pub package_id: String,
    pub message: String,
}

#[derive(Serialize, Clone)]
pub struct InstallResult {
    pub stage: String,
    pub success_count: u32,
    pub error_count: u32,
    pub errors: Vec<PackageError>,
}

pub struct InstallState {
    pub cancelled: Arc<Mutex<bool>>,
}

impl Default for InstallState {
    fn default() -> Self {
        InstallState { cancelled: Arc::new(Mutex::new(false)) }
    }
}

fn emit_progress(app: &AppHandle, pkg_id: &str, stage: &str, progress: u8, message: &str) {
    let _ = app.emit(
        "install_progress",
        ProgressEvent {
            package_id: pkg_id.to_string(),
            stage: stage.to_string(),
            progress,
            message: message.to_string(),
        },
    );
}

async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    on_progress: impl Fn(u8),
) -> Result<(), String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let total = resp.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();

    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("Cannot create temp file: {e}"))?;

    let mut last_pct: u8 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error: {e}"))?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|e| format!("Write error: {e}"))?;
        downloaded += chunk.len() as u64;
        if total > 0 {
            let pct = ((downloaded * 100) / total) as u8;
            if pct != last_pct {
                last_pct = pct;
                on_progress(pct);
            }
        }
    }

    Ok(())
}

async fn download_with_fallback(
    pkg: &PackageRequest,
    dest: &Path,
    app: &AppHandle,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;

    let app_id = pkg.id.clone();
    let app_ref = app.clone();

    let primary_result = if !pkg.url.is_empty() {
        let app2 = app_ref.clone();
        let id2 = app_id.clone();
        download_file(&client, &pkg.url, dest, move |p| {
            emit_progress(&app2, &id2, "downloading", p, "");
        })
        .await
    } else {
        Err("No primary URL".to_string())
    };

    if primary_result.is_ok() {
        return Ok(());
    }

    let drive_url = pkg.drive_url.as_deref().unwrap_or("").trim().to_string();
    if drive_url.is_empty() {
        return primary_result;
    }

    emit_progress(app, &app_id, "downloading", 0, "Retrying via fallback...");
    download_file(&client, &drive_url, dest, move |p| {
        emit_progress(&app_ref, &app_id, "downloading", p, "(fallback)");
    })
    .await
    .map_err(|e| format!("Fallback also failed: {e}"))
}

fn build_wrapper_script(
    packages: &[(PackageRequest, PathBuf)],
    ctx: &crate::context::ExecutionContext,
) -> String {
    let log_path = ctx.log_path.to_string_lossy().replace('\\', "\\\\");
    let state_path = ctx.state_path.to_string_lossy().replace('\\', "\\\\");
    let exit_path = ctx.exit_code_path.to_string_lossy().replace('\\', "\\\\");

    let pkg_entries: String = packages
        .iter()
        .map(|(pkg, file)| {
            let file_str = file.to_string_lossy().replace('\\', "\\\\");
            let safe_args = pkg.args.replace('"', r#"`""#);
            let installer_type = pkg.installer_type.to_uppercase();
            format!(
                "    @{{Id=\"{}\"; Name=\"{}\"; File=\"{}\"; Args=\"{}\"; Type=\"{}\"}}",
                pkg.id, pkg.name, file_str, safe_args, installer_type
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$ProgressPreference = 'SilentlyContinue'

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {{ "1" | Set-Content -Path "{exit_path}" -Encoding ASCII; exit 1 }}

$logPath = "{log_path}"
$statePath = "{state_path}"
$exitPath = "{exit_path}"

function Write-Log($msg) {{
    try {{ "$(Get-Date -Format 'HH:mm:ss') $msg" | Add-Content -Path $logPath -Encoding UTF8 }} catch {{}}
}}
function Set-State($id, $stage, $progress) {{
    try {{ "$id|$stage|$progress" | Set-Content -Path $statePath -Encoding UTF8 }} catch {{}}
}}

$packages = @(
{pkg_entries}
)

$successCount = 0
$errorCount = 0

foreach ($pkg in $packages) {{
    Write-Log "Starting: $($pkg.Name)"
    Set-State $pkg.Id "installing" 10

    try {{
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.UseShellExecute = $false
        $psi.CreateNoWindow = $true
        $psi.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden

        if ($pkg.Type -eq "MSI") {{
            $psi.FileName = "msiexec.exe"
            $psi.Arguments = "/i `"$($pkg.File)`" /qn /norestart"
        }} elseif ($pkg.Type -eq "MSIXBUNDLE") {{
            Add-AppxPackage -Path $pkg.File -ErrorAction Stop
            Write-Log "OK: $($pkg.Name)"
            Set-State $pkg.Id "done" 100
            $successCount++
            Remove-Item -Path $pkg.File -Force -ErrorAction SilentlyContinue
            continue
        }} elseif ($pkg.Type -eq "DIRECTX_REDIST") {{
            $extractDir = [System.IO.Path]::GetTempPath() + "croxz_dx_" + [System.Guid]::NewGuid().ToString()
            New-Item -ItemType Directory -Path $extractDir -Force | Out-Null
            Start-Process $pkg.File -ArgumentList "/Q /T:`"$extractDir`"" -Wait
            $dxSetup = Join-Path $extractDir "DXSETUP.exe"
            if (Test-Path $dxSetup) {{
                $psi.FileName = $dxSetup
                $psi.Arguments = "/silent"
            }} else {{
                throw "DXSETUP.exe not found after extraction"
            }}
        }} else {{
            $psi.FileName = $pkg.File
            $psi.Arguments = $pkg.Args
        }}

        $proc = [System.Diagnostics.Process]::Start($psi)
        $proc.WaitForExit()
        $exitCode = $proc.ExitCode

        if ($exitCode -in @(0, 3010, 1641)) {{
            Write-Log "OK: $($pkg.Name) (exit=$exitCode)"
            Set-State $pkg.Id "done" 100
            $successCount++
        }} else {{
            Write-Log "FAIL: $($pkg.Name) (exit=$exitCode)"
            Set-State $pkg.Id "error" 0
            $errorCount++
        }}

        if ($pkg.Type -eq "DIRECTX_REDIST") {{
            Remove-Item $extractDir -Recurse -Force -ErrorAction SilentlyContinue
        }}
    }} catch {{
        Write-Log "ERROR: $($pkg.Name) - $($_.Exception.Message)"
        Set-State $pkg.Id "error" 0
        $errorCount++
    }}

    Remove-Item -Path $pkg.File -Force -ErrorAction SilentlyContinue
}}

Write-Log "Completed. Success=$successCount Errors=$errorCount"
"0" | Set-Content -Path $exitPath -Encoding ASCII
"#,
        exit_path = exit_path,
        log_path = log_path,
        state_path = state_path,
        pkg_entries = pkg_entries
    )
}

async fn tail_state_file(state_path: PathBuf, app: AppHandle) {
    let mut last = String::new();
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
        if let Ok(content) = tokio::fs::read_to_string(&state_path).await {
            let trimmed = content.trim().to_string();
            if trimmed != last && !trimmed.is_empty() {
                last = trimmed.clone();
                let parts: Vec<&str> = trimmed.splitn(3, '|').collect();
                if parts.len() == 3 {
                    emit_progress(
                        &app,
                        parts[0],
                        parts[1],
                        parts[2].parse().unwrap_or(0),
                        "",
                    );
                }
            }
        }
    }
}

#[tauri::command]
pub async fn start_installation(
    app: AppHandle,
    state: tauri::State<'_, InstallState>,
    packages: Vec<PackageRequest>,
) -> Result<(), String> {
    *state.cancelled.lock().unwrap() = false;

    let ctx = crate::context::ExecutionContext::new();
    let mut downloaded: Vec<(PackageRequest, PathBuf)> = Vec::new();
    let mut errors: Vec<PackageError> = Vec::new();

    // Phase 1: download all packages
    for pkg in &packages {
        if *state.cancelled.lock().unwrap() {
            break;
        }

        let url_empty = pkg.url.is_empty()
            && pkg.drive_url.as_deref().unwrap_or("").trim().is_empty();

        if url_empty {
            emit_progress(&app, &pkg.id, "skipped", 100, "No URL configured");
            continue;
        }

        let ext = if pkg.installer_type == "msi" { "msi" } else if pkg.installer_type == "msixbundle" { "msixbundle" } else { "exe" };
        let dest = std::env::temp_dir().join(format!("croxz_{}_{}.{}", ctx.id, pkg.id, ext));

        emit_progress(&app, &pkg.id, "downloading", 0, &format!("Downloading {}...", pkg.name));

        match download_with_fallback(pkg, &dest, &app).await {
            Ok(_) => {
                emit_progress(&app, &pkg.id, "downloaded", 100, "Download complete");
                downloaded.push((pkg.clone(), dest));
            }
            Err(e) => {
                errors.push(PackageError { package_id: pkg.id.clone(), message: e.clone() });
                emit_progress(&app, &pkg.id, "error", 0, &e);
            }
        }
    }

    if *state.cancelled.lock().unwrap() {
        ctx.cleanup();
        let _ = app.emit("install_result", InstallResult {
            stage: "cancelled".to_string(),
            success_count: 0,
            error_count: errors.len() as u32,
            errors,
        });
        return Ok(());
    }

    if downloaded.is_empty() {
        ctx.cleanup();
        let _ = app.emit("install_result", InstallResult {
            stage: "error".to_string(),
            success_count: 0,
            error_count: errors.len() as u32,
            errors,
        });
        return Ok(());
    }

    // Phase 2: write elevated PS wrapper
    let script = build_wrapper_script(&downloaded, &ctx);
    std::fs::write(&ctx.script_path, &script)
        .map_err(|e| format!("Failed to write installer script: {e}"))?;

    for (pkg, _) in &downloaded {
        emit_progress(&app, &pkg.id, "queued", 0, "Waiting for UAC approval...");
    }

    // Phase 3: run elevated PS + tail state in parallel
    let tail_app = app.clone();
    let state_path = ctx.state_path.clone();
    let tail_task = tokio::spawn(async move {
        tail_state_file(state_path, tail_app).await;
    });

    let script_path_str = ctx.script_path.to_string_lossy().to_string();
    let ps_cmd = format!(
        "Start-Process powershell -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList \
        '-NoProfile -ExecutionPolicy Bypass -File \\\"{script_path_str}\\\"'"
    );

    let output = tokio::process::Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_cmd])
        .output()
        .await
        .map_err(|e| format!("Failed to launch installer: {e}"))?;

    tail_task.abort();

    // Check if UAC was cancelled
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    let exit_status = output.status.code().unwrap_or(-1);
    if exit_status == 1223 || stderr.contains("cancel") || stderr.contains("1223") {
        ctx.cleanup();
        let _ = app.emit("install_result", InstallResult {
            stage: "cancelled".to_string(),
            success_count: 0,
            error_count: 0,
            errors: vec![],
        });
        return Ok(());
    }

    // Read exit code written by PS script
    let script_exit = std::fs::read_to_string(&ctx.exit_code_path)
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .unwrap_or(-1);

    // Count results from log
    let log_content = std::fs::read_to_string(&ctx.log_path).unwrap_or_default();
    let success_count = log_content.matches("OK:").count() as u32;
    let error_count = (log_content.matches("FAIL:").count() + log_content.matches("ERROR:").count()) as u32;

    ctx.cleanup();

    let stage = if script_exit == 0 { "completed" } else { "error" };
    let _ = app.emit("install_result", InstallResult {
        stage: stage.to_string(),
        success_count,
        error_count: error_count + errors.len() as u32,
        errors,
    });

    Ok(())
}

#[tauri::command]
pub async fn cancel_installation(state: tauri::State<'_, InstallState>) -> Result<(), String> {
    *state.cancelled.lock().unwrap() = true;
    Ok(())
}
