# StreamCraft Controller update script.
#
# Builds the release binary and replaces the copy Windows starts, so the next
# launch (and the next sign-in) runs the code in this working tree instead of
# whatever was installed the first time the switch was turned on.
#
# The app does this comparison itself on startup; this script is the one-step
# way to do it without opening the window first, and the only way to update a
# copy while the old one is still running.

param(
    [switch]$Hidden,
    [switch]$SkipLaunch,
    [switch]$CreateShortcut
)

$ErrorActionPreference = "Stop"

function Write-Info { Write-Host "[INFO] " -ForegroundColor Cyan -NoNewline; Write-Host $args }
function Write-Success { Write-Host "[OK] " -ForegroundColor Green -NoNewline; Write-Host $args }
function Write-Warning { Write-Host "[WARN] " -ForegroundColor Yellow -NoNewline; Write-Host $args }
function Write-ErrorLine { Write-Host "[ERROR] " -ForegroundColor Red -NoNewline; Write-Host $args }

$ProjectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$GuiDir = Join-Path $ProjectRoot "crates\ss550-gui"
$SourceFile = Join-Path $GuiDir "target\release\ss550-gui.exe"
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\ss550"
$InstallFile = Join-Path $InstallDir "ss550-gui.exe"

Write-Info "Updating StreamCraft Controller from $GuiDir"

# Step 1: build. `cargo` reports failure with its exit code, not an exception,
# so the exit code is what has to be checked; piping it to Out-Null silently
# swallowed every build error this script used to have.
Write-Info "Building release version..."
Push-Location $GuiDir
try {
    cargo build --release
    $buildExit = $LASTEXITCODE
} finally {
    Pop-Location
}
if ($buildExit -ne 0) {
    Write-ErrorLine "Build failed with exit code $buildExit"
    exit 1
}
if (-not (Test-Path $SourceFile)) {
    Write-ErrorLine "Binary not found at: $SourceFile"
    exit 1
}
Write-Success "Build completed"

# Step 2: stop the running instance. Windows maps a running executable, so the
# copy below fails until the old process is gone.
$running = Get-Process -Name "ss550-gui" -ErrorAction SilentlyContinue
if ($running) {
    Write-Info "Stopping the running instance..."
    $running | Stop-Process -Force
    Start-Sleep -Milliseconds 800
    Write-Success "Stopped $($running.Count) process(es)"
}

# Step 3: install. The hash is compared afterwards because a copy that silently
# did nothing looks exactly like one that worked.
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Write-Info "Created install directory: $InstallDir"
}
try {
    Copy-Item -Path $SourceFile -Destination $InstallFile -Force
} catch {
    Write-ErrorLine "Copy failed: $_"
    exit 1
}
$sourceHash = (Get-FileHash $SourceFile -Algorithm SHA256).Hash
$installHash = (Get-FileHash $InstallFile -Algorithm SHA256).Hash
if ($sourceHash -ne $installHash) {
    Write-ErrorLine "The installed copy does not match the build; nothing was installed."
    exit 1
}
Write-Success "Installed: $InstallFile"

# Step 4: optional startup shortcut, for the folder Windows reads at sign-in.
if ($CreateShortcut) {
    $StartupDir = [Environment]::GetFolderPath("Startup")
    $ShortcutPath = Join-Path $StartupDir "StreamCraft Controller.lnk"
    $Shell = New-Object -ComObject WScript.Shell
    $Shortcut = $Shell.CreateShortcut($ShortcutPath)
    $Shortcut.TargetPath = $InstallFile
    $Shortcut.WorkingDirectory = $InstallDir
    $Shortcut.WindowStyle = 7
    $Shortcut.Save()
    Write-Success "Created startup shortcut: $ShortcutPath"
}

# Step 5: start the build that was just installed, so the window on screen and
# the file at logon are the same bytes.
#
# `-ArgumentList` refuses an empty array on Windows PowerShell 5.1, so the two
# shapes are stated instead of built into a list that may be empty.
if (-not $SkipLaunch) {
    if ($Hidden) {
        Start-Process -FilePath $InstallFile -ArgumentList "--hidden" -WorkingDirectory $InstallDir
    } else {
        Start-Process -FilePath $InstallFile -WorkingDirectory $InstallDir
    }
    Write-Success "Started the installed copy"
}

Write-Info "Done. Version: $((Get-Item $InstallFile).VersionInfo.FileVersion)"
