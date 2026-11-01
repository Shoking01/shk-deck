# Deploy script for plugin examples
# Build and copy plugins to the StreamDeck plugins directory

$plugin_dir = "$env:APPDATA\HotSpot\StreamDeck\plugins"
$examples_dir = Split-Path -Parent $PSScriptRoot

# Ensure plugin directory exists
if (-not (Test-Path $plugin_dir)) {
    New-Item -ItemType Directory -Path $plugin_dir -Force | Out-Null
}

# Build and deploy launcher plugin
$launcher_dir = Join-Path $examples_dir "plugin_launcher"
Write-Host "Building launcher plugin..." -ForegroundColor Cyan

Push-Location $launcher_dir
cargo build --release 2>&1 | Select-String "Compiling|Finished|error"
Pop-Location

# Copy files
$launcher_exe = Join-Path $launcher_dir "target\release\reard-plugin-launcher.exe"
$manifest = Join-Path $launcher_dir "manifest.json"

if (Test-Path $launcher_exe) {
    $plugin_name = "com.inginversa.launcher"
    $dest_dir = Join-Path $plugin_dir "$plugin_name.sdPlugin"
    
    if (-not (Test-Path $dest_dir)) {
        New-Item -ItemType Directory -Path $dest_dir -Force | Out-Null
    }
    
    Copy-Item $launcher_exe $dest_dir -Force
    Copy-Item $manifest $dest_dir -Force
    
    Write-Host "  Deployed to: $dest_dir" -ForegroundColor Green
}

Write-Host "Done! Plugins ready." -ForegroundColor Cyan
Write-Host ""
Write-Host "To use the launcher plugin:" -ForegroundColor Yellow
Write-Host "1. Add entries to host/profile.json with app paths" -ForegroundColor Gray
Write-Host "2. Restart reard-v2.exe" -ForegroundColor Gray
Write-Host ""
Write-Host "Example profile.json entries:" -ForegroundColor Gray
Write-Host '  {"plugin":"com.inginversa.launcher","action":"open-brave","column":0,"row":0,"settings":{"appPath":"C:\\Program Files\\BraveSoftware\\Brave\\Application\\brave.exe"}}'