# REA RD v2 — Native Stream Deck Host

A lightweight, **WebView2-free** Rust application for the Redragon Stream Craft SS550 / Mirabox StreamDeck.

## Features

- **Native Rust** (egui UI, no Electron/WebView2)
- **Low memory**: ~130 MB RAM idle
- **Fast startup**: < 3 seconds
- **Plugin system**: Native `.exe` plugins via `settings.path`
- **Launcher mode**: Open apps directly from deck keys

## Project Structure

```
v2/                   # Main application (Rust)
├─ Cargo.toml         # Release build
├─ target/release/
│  └─ reard_v2.exe    # Built executable
├─ src/
│  ├─ main.rs         # UI + device loop
│  ├─ device.rs       # WebSocket SDK transport
│  ├─ profile.rs      # Profile JSON loader
│  ├─ widgets.rs      # hand-painted egui UI
│  └─ plugins/mod.rs   # Plugin runtime (native launch)
├─ examples/
│  ├─ plugin_launcher/ # Example: open apps from deck
│  └─ plugin_time/     # Template: build your own plugin
├─ electron-wrapper/   # Optional: HTML plugin support (needs Electron)
└─ odd/tasks/reard-v2.md # Development log / specs
```

## Quick Start

1. Build the app:
   ```bash
   cd v2
   cargo build --release
   ```

2. Start the SDK (required):
   ```bash
   cd third_party/websocket-sdk
   ./StreamDockWebsocketSDK.exe -oem 293V3:0x0200:0x3000
   ```

3. Run the app:
   ```bash
   v2/target/release/reard_v2.exe
   ```

4. Deck connects automatically and shows static images from `host/last-setImage-key*.png`.

## Launcher Plugin Setup

In `host/profile.json`:

```json
{
  "instances": [
    {
      "plugin": "com.inginversa.launcher",
      "action": "open-brave",
      "column": 0,
      "row": 0,
      "settings": {
        "path": "C:\\Program Files\\BraveSoftware\\Brave\\Application\\brave.exe"
      }
    },
    {
      "plugin": "com.inginversa.launcher",
      "action": "open-steam",
      "column": 4,
      "row": 0,
      "settings": {
        "path": "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\Steam\\Steam.lnk"
      }
    }
  ]
}
```

Key layout: 5×3 grid, column 0–4 left→right, row 0–2 bottom→top.

## Native Plugin Template

Create `examples/plugin_time/` with:

- `Cargo.toml` → `name = "reard-plugin-time"`
- `src/main.rs` → read manifest from `args[1]`, connect to WebSocket, handle events
- `manifest.json` → `CodePathWin: "plugin.exe"`

Build with `cargo build --release` and copy the `.exe` to `%APPDATA%\HotSpot\StreamDeck\plugins\{uuid}.sdPlugin\`.

## Electron Support (optional)

HTML plugins require Electron installed. The `electron-wrapper/` script handles lazy launching when needed.

## License

MIT