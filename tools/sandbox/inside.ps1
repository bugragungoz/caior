# Runs inside Windows Sandbox (started by start.ps1). Installs a profile with the
# release build, then records detection before/after and a second dry run.
param(
    [string]$Profile = "recommended",
    [string]$Extra = "",
    # Windows Sandbox ships with the Windows Modules Installer (TrustedInstaller)
    # disabled, which breaks side-by-side (WinSxS) installs such as VC++ 2005/2008.
    [switch]$EnableServicing,
    # Drive the GUI instead of the CLI (CAIOR_AUTOTEST); screenshots land in results\gui.
    [switch]$Gui,
    [int]$GuiProfile = 0,
    [string]$GuiExclude = "",
    [switch]$ForceWorker
)

$ErrorActionPreference = "Continue"
$out = "C:\results"
New-Item -ItemType Directory -Force -Path $out | Out-Null
Start-Transcript -Path "$out\transcript.txt" -Force | Out-Null

$exe = "C:\caior\caior.exe"
# A mapped catalog folder overrides the built-in catalog (in-process install, no worker).
$catalogArgs = @()
if (Test-Path "C:\caior-catalog") { $catalogArgs = @("--catalog", "C:\caior-catalog") }

# Sandbox only: make every Windows Installer session write a verbose log to %TEMP%.
New-Item -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\Installer" -Force | Out-Null
Set-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\Installer" -Name Logging -Value "voicewarmupx"
$ti = Get-Service TrustedInstaller -ErrorAction SilentlyContinue
"trustedinstaller=$($ti.Status)/$($ti.StartType)" | Out-File "$out\servicing.txt"
if ($EnableServicing) {
    Set-Service TrustedInstaller -StartupType Manual -ErrorAction SilentlyContinue
    Start-Service TrustedInstaller -ErrorAction SilentlyContinue
    $ti = Get-Service TrustedInstaller -ErrorAction SilentlyContinue
    "after enable: $($ti.Status)/$($ti.StartType)" | Out-File "$out\servicing.txt" -Append
}
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
"elevated=$isAdmin" | Out-File "$out\environment.txt"
[Environment]::OSVersion.VersionString | Out-File "$out\environment.txt" -Append

& $exe @catalogArgs detect --json --all | Out-File -Encoding utf8 "$out\detect-before.json"

# While installing, record the permissions of the private folder CAIOR runs the
# installers from (%SystemRoot%\Temp\CAIOR-*): SYSTEM and Administrators only,
# nothing inherited. Afterwards the folder must be gone (staging-left.txt).
$stagingWatch = Start-Job -ArgumentList "$out\staging-acl.txt" -ScriptBlock {
    param($file)
    for ($i = 0; $i -lt 14400; $i++) {
        $d = Get-ChildItem "$env:SystemRoot\Temp" -Directory -Filter "CAIOR-*" -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($d) { icacls $d.FullName | Out-File -Encoding utf8 $file; return }
        Start-Sleep -Milliseconds 250
    }
}
function Complete-StagingCheck([string]$LogRoot) {
    Stop-Job $stagingWatch -ErrorAction SilentlyContinue
    Remove-Job $stagingWatch -Force -ErrorAction SilentlyContinue
    $left = @(Get-ChildItem "$env:SystemRoot\Temp" -Directory -Filter "CAIOR-*" -ErrorAction SilentlyContinue)
    "left=$($left.Count)" | Out-File "$out\staging-left.txt"
    # The session log folder: administrators write, Users read, nothing inherited.
    $session = Get-ChildItem $LogRoot -Directory -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
    if ($session) { icacls $session.FullName | Out-File -Encoding utf8 "$out\logs-acl.txt" }
}

if ($Gui) {
    $env:CAIOR_AUTOTEST = "$out\gui"
    $env:CAIOR_AUTOTEST_PROFILE = "$GuiProfile"
    $env:CAIOR_AUTOTEST_EXCLUDE = $GuiExclude
    if ($ForceWorker) { $env:CAIOR_FORCE_WORKER = "1" }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    if ($catalogArgs.Count -gt 0) { Start-Process -FilePath $exe -ArgumentList $catalogArgs -Wait } else { Start-Process -FilePath $exe -Wait }
    "gui seconds=$([int]$sw.Elapsed.TotalSeconds)" | Out-File "$out\install-exit.txt"
    Complete-StagingCheck "C:\ProgramData\CAIOR\logs"
    Copy-Item "C:\ProgramData\CAIOR\logs" "$out\logs" -Recurse -ErrorAction SilentlyContinue
    # MSI policy logs and other installer logs from the temp folders, as in CLI runs.
    $guiTempLogs = Join-Path $out "temp-logs"
    New-Item -ItemType Directory -Force -Path $guiTempLogs | Out-Null
    foreach ($dir in @($env:TEMP, "$env:SystemRoot\Temp")) {
        Get-ChildItem -Path $dir -Recurse -File -Include *.log, *.txt, *.html -ErrorAction SilentlyContinue |
            Where-Object { $_.Length -lt 20MB } |
            ForEach-Object { Copy-Item $_.FullName (Join-Path $guiTempLogs ($_.Directory.Name + "_" + $_.Name)) -ErrorAction SilentlyContinue }
    }
    & $exe @catalogArgs detect --json --all | Out-File -Encoding utf8 "$out\detect-after.json"
    Stop-Transcript | Out-Null
    "done" | Out-File "$out\DONE"
    return
}

# -Profile none installs only the ids given in -Extra (e.g. "--id vc-*").
$selection = @()
if ($Profile -ne "none") { $selection += @("--profile", $Profile) }
if ($Extra) { $selection += $Extra.Split(" ") }
$installArgs = $catalogArgs + @("install") + $selection + @("--silent", "--logs", "$out\logs", "--cache", "C:\caior-cache")
$sw = [Diagnostics.Stopwatch]::StartNew()
& $exe @installArgs 2>&1 | Tee-Object -FilePath "$out\install-output.txt"
"exit=$LASTEXITCODE seconds=$([int]$sw.Elapsed.TotalSeconds)" | Out-File "$out\install-exit.txt"
Complete-StagingCheck "$out\logs"

& $exe @catalogArgs detect --json --all | Out-File -Encoding utf8 "$out\detect-after.json"
& $exe @($catalogArgs + @("install") + $selection + @("--dry-run")) 2>&1 | Out-File -Encoding utf8 "$out\second-plan.txt"

# Installer logs that land in temp folders (MSI policy logs, VC++ dd_*.txt, Burn logs).
$tempLogs = Join-Path $out "temp-logs"
New-Item -ItemType Directory -Force -Path $tempLogs | Out-Null
foreach ($dir in @($env:TEMP, "$env:SystemRoot\Temp")) {
    Get-ChildItem -Path $dir -Recurse -File -Include *.log, *.txt, *.html -ErrorAction SilentlyContinue |
        Where-Object { $_.Length -lt 20MB } |
        ForEach-Object { Copy-Item $_.FullName (Join-Path $tempLogs ($_.Directory.Name + "_" + $_.Name)) -ErrorAction SilentlyContinue }
}

Stop-Transcript | Out-Null
"done" | Out-File "$out\DONE"
