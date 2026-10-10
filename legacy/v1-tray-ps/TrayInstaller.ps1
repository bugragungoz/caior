# croxz aio runtimes - tray notification installer
# github.com/bugragungoz

param([string]$RuntimesPath)

# Auto-detect paths
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if ([string]::IsNullOrEmpty($RuntimesPath)) {
    $RuntimesPath = Split-Path -Parent $scriptDir
}
$runtimesFolder = Join-Path $RuntimesPath "runtimes"

# Hide console window
Add-Type -MemberDefinition '[DllImport("kernel32.dll")]public static extern IntPtr GetConsoleWindow();[DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr h,Int32 s);' -Name Win -Namespace Native -PassThru | Out-Null
[Native.Win]::ShowWindow([Native.Win]::GetConsoleWindow(), 0) | Out-Null

# Load assemblies
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

# Create tray icon
$trayIcon = New-Object System.Windows.Forms.NotifyIcon
$trayIcon.Icon = [System.Drawing.SystemIcons]::Application
$trayIcon.Visible = $true
$trayIcon.Text = "croxz aio runtimes"

# Package definitions - UPDATED
$packages = @(
    # .NET Framework
    @{Name=".NET Framework 3.5"; File="dotnet\dotnetfx35.exe"; Args="/q /norestart"; Type="EXE"}
    @{Name=".NET Framework 4.8"; File="dotnet\NDP48-x86-x64-AllOS-ENU.exe"; Args="/q /norestart"; Type="EXE"}
    
    # Visual C++ Redistributables
    @{Name="VC++ 2005 x86"; File="vcredist\vcredist2005_x86.exe"; Args="/Q"; Type="EXE"}
    @{Name="VC++ 2005 x64"; File="vcredist\vcredist2005_x64.exe"; Args="/Q"; Type="EXE"}
    @{Name="VC++ 2008 x86"; File="vcredist\vcredist2008_x86.exe"; Args="/q"; Type="EXE"}
    @{Name="VC++ 2008 x64"; File="vcredist\vcredist2008_x64.exe"; Args="/q"; Type="EXE"}
    @{Name="VC++ 2010 x86"; File="vcredist\vcredist2010_x86.exe"; Args="/quiet /norestart"; Type="EXE"}
    @{Name="VC++ 2010 x64"; File="vcredist\vcredist2010_x64.exe"; Args="/quiet /norestart"; Type="EXE"}
    @{Name="VC++ 2012 x86"; File="vcredist\vcredist2012_x86.exe"; Args="/quiet /norestart"; Type="EXE"}
    @{Name="VC++ 2012 x64"; File="vcredist\vcredist2012_x64.exe"; Args="/quiet /norestart"; Type="EXE"}
    @{Name="VC++ 2013 x86"; File="vcredist\vcredist2013_x86.exe"; Args="/quiet /norestart"; Type="EXE"}
    @{Name="VC++ 2013 x64"; File="vcredist\vcredist2013_x64.exe"; Args="/quiet /norestart"; Type="EXE"}
    @{Name="VC++ 2015-2022 x86"; File="vcredist\vcredist2015_2017_2019_2022_x86.exe"; Args="/quiet /norestart"; Type="EXE"}
    @{Name="VC++ 2015-2022 x64"; File="vcredist\vcredist2015_2017_2019_2022_x64.exe"; Args="/quiet /norestart"; Type="EXE"}
    
    # Game Runtimes
    @{Name="DirectX June 2010"; File="game\DirectX\DXSETUP.exe"; Args="/silent"; Type="EXE"}
    @{Name="OpenAL"; File="game\OpenAL_Installer.exe"; Args="/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-"; Type="EXE"}
    @{Name="PhysX"; File="game\PhysX_Installer.exe"; Args="/s"; Type="EXE"}
    @{Name="XNA Framework"; File="game\xnafxrf.msi"; Type="MSI"}
    
    # Web Components
    @{Name="WebView2 x86"; File="web\WebView2_x86.exe"; Args="/silent /install"; Type="EXE"}
    @{Name="WebView2 x64"; File="web\WebView2_x64.exe"; Args="/silent /install"; Type="EXE"}
    
    # Java Runtime
    @{Name="Java x86"; File="java\java_x86.msi"; Type="MSI"}
    @{Name="Java x64"; File="java\java_x64.msi"; Type="MSI"}
    
    # Legacy Components
    @{Name="MSXML 4.0"; File="legacy\msxml.msi"; Type="MSI"}
    @{Name="VB/C Runtime"; File="legacy\vbcrun.msi"; Type="MSI"}
    @{Name="Visual F# 4.1"; File="legacy\Visual_F_4.1.msi"; Type="MSI"}
    @{Name="VSTOR x86"; File="legacy\vstor40_x86.msi"; Type="MSI"}
    @{Name="VSTOR x64"; File="legacy\vstor40_x64.msi"; Type="MSI"}
)

# Show initial balloon
$trayIcon.BalloonTipIcon = [System.Windows.Forms.ToolTipIcon]::Info
$trayIcon.BalloonTipTitle = "croxz aio runtimes"
$trayIcon.BalloonTipText = "Installing runtime packages..."
$trayIcon.ShowBalloonTip(3000)
Start-Sleep -Milliseconds 500

# Silent installation function
function Install-Package {
    param($Package)
    
    $filePath = Join-Path $runtimesFolder $Package.File
    if (-not (Test-Path $filePath)) { return -1 }
    
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.CreateNoWindow = $true
    $psi.UseShellExecute = $false
    $psi.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    
    if ($Package.Type -eq "MSI") {
        $psi.FileName = "msiexec.exe"
        $psi.Arguments = "/i `"$filePath`" /qn /norestart"
    } else {
        $psi.FileName = $filePath
        $psi.Arguments = $Package.Args
    }
    
    try {
        $proc = [System.Diagnostics.Process]::Start($psi)
        $proc.WaitForExit()
        return $proc.ExitCode
    } catch {
        return -999
    }
}

# Install all packages
$total = $packages.Count
$success = 0
$failed = 0

for ($i = 0; $i -lt $total; $i++) {
    $pkg = $packages[$i]
    $progress = [math]::Round((($i + 1) / $total) * 100)
    
    $trayIcon.Text = "[$progress%] $($pkg.Name)"
    [System.Windows.Forms.Application]::DoEvents()
    
    $exitCode = Install-Package -Package $pkg
    
    # Success codes: 0, 3010 (reboot required), 1641 (reboot initiated)
    if ($exitCode -eq 0 -or $exitCode -eq 3010 -or $exitCode -eq 1641) {
        $success++
    } else {
        $failed++
    }
}

# Show completion balloon
$trayIcon.BalloonTipIcon = [System.Windows.Forms.ToolTipIcon]::Info
$trayIcon.BalloonTipTitle = "croxz aio runtimes"
if ($failed -eq 0) {
    $trayIcon.BalloonTipText = "Complete! ($success packages)`ngithub.com/bugragungoz"
} else {
    $trayIcon.BalloonTipText = "Done: $success ok, $failed skipped`ngithub.com/bugragungoz"
}
$trayIcon.ShowBalloonTip(5000)

Start-Sleep -Seconds 6

$trayIcon.Visible = $false
$trayIcon.Dispose()
