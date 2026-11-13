# StreamCraft Controller Update Script
# Compiles the latest version and updates the installed copy

param(
    [switch]$KillOld = $true,
    [switch]$CreateShortcut = $false
)

$ErrorActionPreference = "Stop"

# Colors for output
function Write-Info { Write-Host "[INFO] " -ForegroundColor Cyan -NoNewline; Write-Host $args }
function Write-Success { Write-Host "[OK] " -ForegroundColor Green -NoNewline; Write-Host $args }
function Write-Warning { Write-Host "[WARN] " -ForegroundColor Yellow -NoNewline; Write-Host $args }
function Write-Error { Write-Host "[ERROR] " -ForegroundColor Red -NoNewline; Write-Host $args }

# Paths
$ProjectRoot = "C:\Users\quiro\Downloads\Adrian\Trabajo\Portfolio\REA RD"
$SourceFile = Join-Path $ProjectRoot "crates\ss550-gui\target\release\ss550-gui.exe"
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\StreamCraft"
$InstallFile = Join-Path $InstallDir "ss550-gui.exe"

Write-Info "Updating StreamCraft Controller..."

# Step 1: Change to project directory
Set-Location (Join-Path $ProjectRoot "crates\ss550-gui")
Write-Info "Working directory: $(Get-Location)"

# Step 2: Build release
Write-Info "Building release version..."
try {
    cargo build --release 2>&1 | Out-Null
    Write-Success "Build completed successfully"
} catch {
    Write-Error "Build failed: $_"
    exit 1
}

# Step 3: Verify binary exists
if (-not (Test-Path $SourceFile)) {
    Write-Error "Binary not found at: $SourceFile"
    exit 1
}

Write-Success "Binary found at: $SourceFile"

# Step 4: Kill old process if running
if ($KillOld -and (Get-Process -Name "ss550-gui" -ErrorAction SilentlyContinue)) {
    Write-Info "Stopping old instance..."
    try {
        Stop-Process -Name "ss550-gui" -Force
        Start-Sleep -Seconds 1
        Write-Success "Old instance stopped"
    } catch {
        Write-Warning "Could not stop old instance: $_"
    }
}

# Step 5: Create install directory
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Write-Info "Created install directory: $InstallDir"
}

# Step 6: Copy new binary
try {
    Copy-Item -Path $SourceFile -Destination $InstallFile -Force
    Write-Success "Updated: $InstallFile"
} catch {
    Write-Error "Copy failed: $_"
    exit 1
}

# Step 7: Create startup shortcut (optional)
if ($CreateShortcut -or $args.Contains("create-shortcut")) {
    $StartupDir = [Environment]::GetFolderPath("Startup")
    $ShortcutPath = Join-Path $StartupDir "StreamCraft Controller.lnk"
    
    $WshShell = New-Object -ComObject WScript.Shell
    $Shortcut = $WshShell.CreateShortcut($ShortcutPath)
    $Shortcut.TargetPath = $InstallFile
    $Shortcut.WorkingDirectory = $InstallDir
    $Shortcut.WindowStyle = 7  # Hidden window
    $Shortcut.Save()
    
    Write-Success "Created startup shortcut: $ShortcutPath"
}

# Step 8: Show version info
$VersionInfo = (Get-Item $InstallFile).VersionInfo
Write-Host ""
Write-Info "Update complete!"
Write-Host "Version: $($VersionInfo.FileVersion)"
Write-Host "Location: $InstallFile"
Write-Host ""

if (-not $CreateShortcut) {
    Write-Warning "To auto-start on login, run with -CreateShortcut"
}

Write-Host "To run now: & `"$InstallFile`""