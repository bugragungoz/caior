# Starts Windows Sandbox with the release build mapped read-only and runs
# inside.ps1 at logon. Results land in -Results on the host.
#   powershell -ExecutionPolicy Bypass -File tools\sandbox\start.ps1 -Profile recommended -Results C:\temp\caior-sandbox
param(
    [string]$Profile = "recommended",
    [string]$Extra = "",
    [Parameter(Mandatory = $true)][string]$Results,
    # Optional catalog folder to use instead of the one built into the exe.
    [string]$Catalog = "",
    [switch]$EnableServicing
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$build = Join-Path $root "target\release"
if (-not (Test-Path (Join-Path $build "caior.exe"))) {
    throw "Error: target\release\caior.exe is missing. Run: cargo build --release -p caior"
}
New-Item -ItemType Directory -Force -Path $Results | Out-Null
# Only clear a folder this script made before (it holds caior-test.wsb).
$existing = @(Get-ChildItem $Results -Force)
if ($existing.Count -gt 0) {
    if (-not (Test-Path (Join-Path $Results "caior-test.wsb"))) {
        throw "Error: $Results is not empty and was not made by this script. Pick an empty folder."
    }
    $existing | Remove-Item -Recurse -Force
}

# Copy only the exe so the sandbox sees nothing else from the build folder.
$stage = Join-Path $Results "_stage"
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item (Join-Path $build "caior.exe") $stage

$catalogMap = ""
if ($Catalog) {
    $catalogStage = Join-Path $Results "_catalog"
    New-Item -ItemType Directory -Force -Path $catalogStage | Out-Null
    Copy-Item (Join-Path (Resolve-Path $Catalog) "*.toml") $catalogStage
    $catalogMap = "<MappedFolder><HostFolder>$catalogStage</HostFolder><SandboxFolder>C:\caior-catalog</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>"
}

$wsb = @"
<Configuration>
  <Networking>Enable</Networking>
  <MappedFolders>
    <MappedFolder><HostFolder>$stage</HostFolder><SandboxFolder>C:\caior</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>
    <MappedFolder><HostFolder>$PSScriptRoot</HostFolder><SandboxFolder>C:\caior-tools</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>
    <MappedFolder><HostFolder>$Results</HostFolder><SandboxFolder>C:\results</SandboxFolder><ReadOnly>false</ReadOnly></MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\caior-tools\inside.ps1 -Profile $Profile -Extra "$Extra"$(if ($EnableServicing) { " -EnableServicing" })</Command>
  </LogonCommand>
</Configuration>
"@
$wsbPath = Join-Path $Results "caior-test.wsb"
Set-Content -Path $wsbPath -Value $wsb -Encoding UTF8
Start-Process $wsbPath
"Sandbox started. Results: $Results (DONE appears when finished)."
