# Automated checks for a local Windows session (docs/TEST-PLAN.md, part 1):
# tests, lint, catalog, release build, CLI smoke tests, the offline package
# format and UI screenshots. Nothing is installed on this PC.
#   powershell -ExecutionPolicy Bypass -File tools\test\check.ps1
#   powershell -ExecutionPolicy Bypass -File tools\test\check.ps1 -Results C:\temp\caior-check -SkipNetwork
# Run it from an administrator terminal to include the end-to-end install test
# (a copy of cmd.exe as the installer); without it that one test is skipped.
param(
    [string]$Results = "C:\temp\caior-check",
    # Leave out the step that downloads from a vendor (offline package test).
    [switch]$SkipNetwork,
    # Reuse target\release\caior.exe instead of building it again.
    [switch]$NoBuild
)

$ErrorActionPreference = "Continue"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
Set-Location $root
if (Test-Path $Results) { Remove-Item $Results -Recurse -Force -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Results | Out-Null
$exe = Join-Path $root "target\release\caior.exe"
$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)

$summary = [System.Collections.Generic.List[string]]::new()
$script:failed = 0

# Runs one check. The block returns $true when it passed; its other output
# goes to the console only.
function Step([string]$Name, [scriptblock]$Body) {
    Write-Host ""
    Write-Host "== $Name" -ForegroundColor Cyan
    $ok = $false
    try { $ok = [bool](& $Body | Select-Object -Last 1) } catch { Write-Host "Error: $_" -ForegroundColor Red }
    if ($ok) {
        $summary.Add("ok    $Name")
    } else {
        $summary.Add("FAIL  $Name")
        $script:failed++
    }
}

# Runs a cargo command, keeps its output in the results folder. (Function
# names are case-insensitive: a function called "Cargo" would call itself.)
function Invoke-Cargo([string]$Log, [string[]]$CargoArgs) {
    & cargo @CargoArgs 2>&1 | ForEach-Object { "$_" } | Tee-Object -FilePath (Join-Path $Results $Log) | Out-Host
    return ($LASTEXITCODE -eq 0)
}

function Quote([string]$s) { if ($s -match '\s') { "`"$s`"" } else { $s } }

# Runs caior.exe (a window app) and waits for it; returns exit code and output.
function Invoke-Caior([string]$Name, [string[]]$CliArgs, [string]$Program = $exe) {
    $out = Join-Path $Results "$Name.txt"
    $p = Start-Process -FilePath $Program -ArgumentList (($CliArgs | ForEach-Object { Quote $_ }) -join " ") `
        -NoNewWindow -PassThru -RedirectStandardOutput $out -RedirectStandardError "$out.err"
    # Without touching the handle first, ExitCode can come back empty.
    $null = $p.Handle
    if (-not $p.WaitForExit(600000)) { $p.Kill(); return @{ Code = -1; Text = "timed out" } }
    $text = ((Get-Content $out -Raw -ErrorAction SilentlyContinue) + (Get-Content "$out.err" -Raw -ErrorAction SilentlyContinue))
    Write-Host "exit $($p.ExitCode)"
    Write-Host $text
    return @{ Code = $p.ExitCode; Text = "$text" }
}

Step "cargo test --workspace" { Invoke-Cargo "test.txt" @("test", "--workspace") }
Step "cargo clippy (warnings are errors)" { Invoke-Cargo "clippy.txt" @("clippy", "--workspace", "--all-targets", "--", "-D", "warnings") }
Step "catalog check --strict" { Invoke-Cargo "catalog.txt" @("xtask", "catalog", "check", "--strict") }
if (-not $NoBuild) {
    Step "release build" { Invoke-Cargo "build.txt" @("build", "--release", "-p", "caior") }
}

Step "caior list" {
    $r = Invoke-Caior "cli-list" @("list")
    $r.Code -eq 0 -and $r.Text -match "ace-2016-x64"
}
Step "caior detect --json gives valid JSON" {
    $r = Invoke-Caior "cli-detect" @("detect", "--json")
    $r.Code -eq 0 -and $null -ne ($r.Text | ConvertFrom-Json).packages
}
Step "install --profile all --dry-run plans without a conflict" {
    $r = Invoke-Caior "cli-all-dry-run" @("install", "--profile", "all", "--dry-run")
    $r.Code -eq 0 -and $r.Text -notmatch "cannot be installed together"
}
Step "two conflicting ids: one clear error, exit 2" {
    $r = Invoke-Caior "cli-conflict" @("install", "--id", "ace-2016-x86", "--id", "ace-2016-x64", "--dry-run")
    # With 64-bit Office (or ACE x64) installed, x86 is skipped instead: also right.
    ($r.Code -eq 2 -and $r.Text -match "cannot be installed together" -and ([regex]::Matches($r.Text, "Error:").Count -eq 1)) -or
    ($r.Code -eq 0 -and $r.Text -match "cannot sit next to an installed package")
}
Step "unknown id: one Error: line, exit 2" {
    $r = Invoke-Caior "cli-unknown" @("install", "--id", "no-such-package", "--dry-run")
    $r.Code -eq 2 -and ([regex]::Matches($r.Text, "Error:").Count -eq 1)
}
if (-not $admin) {
    Step "install --cache without admin is refused (exit 2)" {
        $r = Invoke-Caior "cli-cache" @("install", "--profile", "recommended", "--cache", "C:\temp\x", "--dry-run")
        $r.Code -eq 2 -and $r.Text -match "--download-only"
    }
}

if (-not $SkipNetwork) {
    $single = Join-Path $Results "pack\openal.exe"
    $folder = Join-Path $Results "pack\folder"
    Step "pack a single exe (OpenAL, 0.6 MB from openal.org)" {
        $r = Invoke-Caior "pack-exe" @("pack", "--id", "openal", "--out", $single, "--cache", (Join-Path $Results "pack\cache"))
        $r.Code -eq 0 -and (Get-Item $single).Length -gt (Get-Item $exe).Length
    }
    Step "the packed exe still runs and lists the catalog" {
        $r = Invoke-Caior "pack-exe-list" @("list") $single
        $r.Code -eq 0 -and $r.Text -match "openal"
    }
    Step "folder package made from the single exe holds the plain program" {
        $r = Invoke-Caior "pack-folder" @("pack", "--id", "openal", "--folder", "--out", $folder, "--cache", (Join-Path $Results "pack\cache")) $single
        $r.Code -eq 0 -and
            (Get-Item (Join-Path $folder "caior.exe")).Length -eq (Get-Item $exe).Length -and
            (Test-Path (Join-Path $folder "caior-packages\bundle.json"))
    }
}

# Screenshots of each page in English and Turkish (CAIOR_SNAPSHOT renders the
# window once detection is done, saves a PNG and quits).
$shots = Join-Path $Results "screenshots"
New-Item -ItemType Directory -Force -Path $shots | Out-Null
foreach ($lang in @("en", "tr")) {
    foreach ($page in @(@{ n = 0; name = "install" }, @{ n = 2; name = "offline" }, @{ n = 3; name = "logs" }, @{ n = 4; name = "settings" })) {
        $file = Join-Path $shots "$($page.name)-$lang.png"
        Step "screenshot $($page.name) ($lang)" {
            $env:CAIOR_SNAPSHOT = $file
            $env:CAIOR_PAGE = "$($page.n)"
            $env:CAIOR_LANG = $lang
            $p = Start-Process -FilePath $exe -PassThru
            $null = $p.Handle
            $done = $p.WaitForExit(120000)
            if (-not $done) { $p.Kill() }
            Remove-Item Env:CAIOR_SNAPSHOT, Env:CAIOR_PAGE, Env:CAIOR_LANG -ErrorAction SilentlyContinue
            $done -and (Test-Path $file) -and (Get-Item $file).Length -gt 10000
        }
    }
}

$header = "CAIOR checks, $(Get-Date -Format 'yyyy-MM-dd HH:mm'), commit $(git rev-parse --short HEAD), admin: $admin"
$lines = @($header, "") + $summary + @("", "$($summary.Count - $script:failed) passed, $script:failed failed. Logs and screenshots: $Results")
if (-not $admin) { $lines += "Not an administrator terminal: the end-to-end install test in cargo test was skipped." }
$lines | Set-Content -Encoding UTF8 (Join-Path $Results "summary.txt")
Write-Host ""
$lines | ForEach-Object { Write-Host $_ }
exit ([int]($script:failed -gt 0))
