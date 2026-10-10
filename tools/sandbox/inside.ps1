# Runs inside Windows Sandbox (started by start.ps1). Installs a profile with the
# release build, then records detection before/after and a second dry run.
param(
    [string]$Profile = "recommended",
    [string]$Extra = "",
    # Windows Sandbox ships with the Windows Modules Installer (TrustedInstaller)
    # disabled, which breaks side-by-side (WinSxS) installs such as VC++ 2005/2008.
    [switch]$EnableServicing
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

# -Profile none installs only the ids given in -Extra (e.g. "--id vc-*").
$selection = @()
if ($Profile -ne "none") { $selection += @("--profile", $Profile) }
if ($Extra) { $selection += $Extra.Split(" ") }
$installArgs = $catalogArgs + @("install") + $selection + @("--silent", "--logs", "$out\logs", "--cache", "C:\caior-cache")
$sw = [Diagnostics.Stopwatch]::StartNew()
& $exe @installArgs 2>&1 | Tee-Object -FilePath "$out\install-output.txt"
"exit=$LASTEXITCODE seconds=$([int]$sw.Elapsed.TotalSeconds)" | Out-File "$out\install-exit.txt"

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
