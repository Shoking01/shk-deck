# Installation Guide for streamcraft-rs

## Quick Start (Windows)

### Option 1: Using the update script (recommended)

The repository includes scripts for easy building and installation:

- **`update-steamcraft.bat`** - Batch script for Windows
- **`.\update-steamcraft.ps1`** - PowerShell script

Run from the repository root:

```batch
cd C:\path\to\streamcraft-rs
.\update-steamcraft.bat
```

The script will:
1. Build the release version
2. Copy it to `%LOCALAPPDATA%\Programs\ss550\`
3. (Optional) Create a startup shortcut with `-CreateShortcut`

### Option 2: Manual installation

```bash
cargo build --manifest-path crates/ss550-gui/Cargo.toml --release
mkdir "%LOCALAPPDATA%\Programs\ss550"
copy crates\ss550-gui\target\release\ss550-gui.exe "%LOCALAPPDATA%\Programs\ss550\"
```

### Starting with Windows (optional)

To auto-start the controller on login:

1. Open Run dialog (Win+R)
2. Type `shell:startup` and press Enter
3. Right-click → New → Shortcut
4. Target: `%LOCALAPPDATA%\Programs\ss550\ss550-gui.exe --hidden`
5. Name: "streamcraft-rs Controller"

The `--hidden` flag starts the app without showing the window; the tray icon remains accessible.

## Updating

Always use the update script or copy the newly built binary. Old versions will be overwritten automatically. The script kills any running instances before updating to avoid duplicate processes.

**Important:** If you experience issues after an update, try:
1. Fully close any running instances (`taskkill /F /IM ss550-gui.exe`)
2. Run the update script again
3. Restart your computer if needed

## Running Without Installation

During development, you can run directly from the build directory:

```bash
cargo run --manifest-path crates/ss550-gui/Cargo.toml
```

For a release build:

```bash
cargo build --manifest-path crates/ss550-gui/Cargo.toml --release
.\crates\ss550-gui\target\release\ss550-gui.exe
```

## Linux

For Linux users:

1. Build the release version:
   ```bash
   cargo build --release -p ss550-gui
   ```

2. Create udev rules for the device:
   ```bash
   sudo tee /etc/udev/rules.d/99-streamcraft-ss550.rules << 'EOF'
   SUBSYSTEM=="usb", ATTR{idVendor}=="0200", ATTR{idProduct}=="0300", MODE="0666"
   EOF
   ```

3. Reload udev rules:
   ```bash
   sudo udevadm control --reload-rules
   sudo udevadm trigger
   ```

4. Run the application:
   ```bash
   ./target/release/ss550-gui
   ```

Note: Linux support is in early stages. Some features (like tray integration) may not work correctly.

## macOS

macOS support follows similar steps to Linux:

```bash
cargo build --release -p ss550-gui
./target/release/ss550-gui
```

You may need to grant accessibility permissions in System Preferences → Security & Privacy → Privacy → Accessibility for the app to control the device.

## Troubleshooting

### Device not detected

1. Ensure the device is plugged in and powered on
2. Check the device appears in system device manager
3. Try a different USB port/cable
4. Restart the application

### Multiple instances

If you see duplicate processes or the application doesn't respond:
```bash
taskkill /F /IM ss550-gui.exe  # Windows
# or
pkill -9 ss550-gui              # Linux/macOS
```

Then restart the application.