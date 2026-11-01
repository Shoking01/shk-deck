# REA RD v2 — Native Stream Deck Host

A lightweight, **WebView2-free** Rust application for the Redragon Stream Craft SS550 / Mirabox StreamDeck.

## Features

- **Pure Rust** (egui UI, no Electron/WebView2)
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
│  └─ plugins/mod.rs  # Plugin runtime (native launch)
├─ legacy/
│  ├─ electron-wrapper/  # Optional HTML support (needs Electron)
│  └─ examples/            # Plugin templates + docs
├─ host/last-setImage-key*.png  # Key preview images
└─ ood/tasks/reard-v2.md # Development log / specs
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
    }
  ]
}
```

Key layout: 5×3 grid, column 0–4 left→right, row 0–2 bottom→top.

## License

MIT