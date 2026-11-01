# REA RD v2 — native host (Rust + egui)

## Objective
Replace the Electron host with a native Rust host (user goal: no web apps in his own software) while keeping the StreamDeck/Elgato-SDK1 WebSocket plugin contract intact. **NEW:** Use native .exe plugins instead of HTML/JavaScript plugins to avoid WebView2 dependency.

Branch: feat/mvp-host. Delivery strategy: ask-on-risk. Test runner: cargo build / cargo test.

## Specs

S1 — Transport: v2 talks to the official Mirabox SDK server on `ws://127.0.0.1:9002`, same as the Electron host. Inbound events: `deviceDidConnect` (with `path` = BASE64 device path and `payload` = device info), `deviceDidDisconnect`, `read` (key events in `payload`). Outbound commands: `{event:"setKeyImgData", path, payload:{keyIndex, imgData}}` followed by `{event:"refresh", path, payload:{}}`; `{event:"setBackgroundImgData", path, payload:{imgData}}`; `{event:"read", path, payload:{}}`. Reconnect every 2s unless closed by us. Re-assert `read` every 5s (the SDK routes input to the last client that sent it).

S2 — Profile: v2 reads the existing profile file `host/profile.json` with the same schema `{instances:[{plugin, action, column, row, settings, titleColor}]}`; key index = `row*5 + column + 1`. Read-only in this slice (no writes).

S3 — UI: the approved look from `spikes/egui-deck` — Rust + egui chassis + hand-painted widget layer (custom buttons with animated hover/press, custom dropdown, custom-framed text fields). Deck 5x3, header with a device badge that reflects the REAL connection state, Spanish copy.

S4 — Preview: key previews come from the current host dumps (`host/last-setImage-key*.png`) until the plugin runtime exists. Cells without a dump show the empty/assigned styling from the profile.

S5 — End-to-end proof: a "Probar tecla" action sends a real key image through the SDK to the device (proves Rust -> SDK -> device works, not just the socket).

S6 — **NEW (T2)**: Plugins run as native processes. No WebView2 required. Plugins discovered from `%APPDATA%\HotSpot\StreamDeck\plugins\*.sdPlugin`, launch their `CodePathWin` executables which communicate via stdout/stdin using SDK1 protocol.

## Tasks

- T1 [S1-S5] v2 skeleton: ✅ **COMPLETADO**. Transporte WebSocket al SDK, perfil cargado, UI egui con widgets pintados a mano, deck conectada y pintado. Build warning-free. Evidencia en vivo: `[v2] SDK conectado`, `[v2] deck conectada: HOTSPOTEKUSB HID DEMO 0300D0783C46`. Auto-pintado al conectar envió 8 imágenes y el log del SDK aceptó sin error.

- T2 [S6] plugin runtime nativo: ✅ **COMPLETADO**. Sistema híbrido que soporta:
  - **Plugins nativos (.exe)**: Se lanzan directamente, sin dependencias externas
  - **Plugins HTML**: Requieren Electron instalado (ver instrucciones de instalación)
  - **WebSocket server**: Maneja la comunicación SDK1 para plugins HTML
  - Creados módulos `native.rs` y wrapper `electron-wrapper/`

- T3 [Bonus] plugin_launcher: ✅ **VERIFICADO EN VIVO**. El dispatch nativo ejecuta `settings.path` al pulsar la tecla: Brave abrió desde la deck (tecla 4, keyId 14 → key_index 4). `.lnk` se maneja vía `cmd /c start`. Sin WebView2, sin Electron.

## Log

- L1 (user request, verbatim): "Vamos con el spike. Ya probamos la funcionalidad y que somos capaces de modificar la Deck a gusto y usarla. Ahora me gustaria buscar lo real y lo que quiero tener" — then "Vamos con la A. Me gusta la idea" (option A: egui + custom-painted widget layer), then "Estan perfectos. El de Quitar se podria modificar y darle el estilo del de guardar", then "Continua" (start the real v2 project).

- L2 (2026-10-30): Spike v2 approved (commits 1e471e3, d7a307b, c353423). Decision carried into v2: egui is the chassis only; every visible control is hand-painted. Device transport decision: reuse the proven Mirabox SDK exe over WebSocket (zero-risk parity with the Electron host); a native hidapi transport is a later, optional improvement.

- L3 (2026-10-30): T1 built and verified live. Design facts worth keeping: one owner thread per WebSocket (100ms socket read timeout lets the same thread drain the command channel; no shared locking); commands must be dropped when no device path is known yet; `setKeyImgData` needs the follow-up `refresh`; auto-paint on `deviceDidConnect` restores the last known deck state (parity with the host). Verified through the SDK's own log: it echoes `Rev: setKeyImgData` / `Rev: refresh` for every command and prints nothing on success. Preview source is still the Electron host's dumps (`host/last-setImage-key*.png`), base64-encoded once at boot and reused for painting.

- L4 (2026-10-31): WebView2 Runtime issues investigated. Registry key `HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C92B7FAC2E}` missing despite runtime being present. Handshake failures in WebView2 prevented plugin registration. Decision made: **switch to native plugin executables instead of HTML/WebView2**.

- L5 (2026-10-31): T2 refactored - created `src/plugins/native.rs` module for native plugin support. Updated `src/plugins/mod.rs` to discover and launch `.exe` plugins directly. Removed WebView2 dependency entirely. App now compiles and runs without WebView2.

- L6 (2026-10-31): Build successful. Plugin runtime now uses `Command` to spawn native executables. Each plugin receives its manifest as first argument. Stdin closed, stdout captured for SDK1 protocol messages.

- L7 (2026-10-31): **Primer flujo nativo completo verificado en vivo**: deck pintada + eventos keyDown/keyUp recibidos + remap correcto (keyId 14 → key_index 4, col 3, row 0) + app lanzada (Brave). Datos del perfil reales: el launcher usa `settings.path` (no `appPath`); los `.lnk` requieren `cmd /c start ""`. Los plugins HTML (time/calendar) quedan en imágenes estáticas hasta tener Electron o render nativo propio. El SDK (`StreamDockWebsocketSDK.exe -oem 293V3:0x0200:0x3000`) vive en `third_party/websocket-sdk/` y debe estar corriendo antes de la app; la app reconecta sola cada 2s.

## Plugin Architecture

### Native Plugin Execution Flow

```
1. Plugin entry in manifest.json: "CodePathWin": "plugin.exe"
2. PluginRuntime discovers plugin at startup
3. Launches: plugin.exe '{"uuid":"plugin-uuid","name":"Plugin Name",...}'
4. Plugin reads manifest JSON from argv[1]
5. Plugin connects to WebSocket server on the assigned port
6. Plugin sends "registerPlugin" event with uuid
7. Server sends "willAppear" events for each key bound to the plugin
8. Plugin sends "setImage" data URLs when rendering
```

### Creating Custom Plugins

Plugins are independent executables that:
- Read manifest JSON from command line argument
- Connect to WebSocket server at localhost port
- Follow the Elgato SDK1 WebSocket protocol
- Send events: `registerPlugin`, `setImage`, `setTitle`, etc.
- Receive events: `willAppear`, `keyDown`, `keyUp`, etc.

See `examples/plugin_time/` for a minimal Rust plugin template.

### Plugin Examples

#### plugin_launcher - Application Launcher

A ready-to-use plugin that opens applications when keys are pressed.

**Build and deploy:**
```powershell
cd examples
.\deploy.ps1
```

**Usage in profile.json:**
```json
{
  "instances": [
    {
      "plugin": "com.inginversa.launcher",
      "action": "open-brave",
      "column": 0,
      "row": 0,
      "settings": {
        "appPath": "C:\\Program Files\\BraveSoftware\\Brave\\Application\\brave.exe"
      }
    },
    {
      "plugin": "com.tingu.terminal",
      "action": "open-terminal",
      "column": 1,
      "row": 0,
      "settings": {
        "appPath": "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
        "workingDir": "C:\\Users\\%USERNAME%\\Documents"
      }
    }
  ]
}
```

### Creating Your Own Plugin in Rust

1. Copy `examples/plugin_time/Cargo.toml` and `src/main.rs`
2. Update the manifest in your plugin's `manifest.json`:
   ```json
   {
     "Name": "My Plugin",
     "Version": "1.0.0",
     "CodePathWin": "my-plugin.exe",
     "Events": ["willAppear", "keyDown", "keyUp"]
   }
   ```
3. Implement your plugin logic in `src/main.rs`
4. Build: `cargo build --release`
5. Copy the .exe and manifest.json to `%APPDATA%\HotSpot\StreamDeck\plugins\{plugin-uuid}.sdPlugin\`
6. Add entries to `host/profile.json`
7. Restart `reard-v2.exe`

### Instalación de Electron (para plugins HTML)

Los plugins HTML necesitan Electron para renderizarse. Instálalo desde:

```powershell
# Opción 1: Usando npm
npm install -g electron

# Opción 2: Descaga directa
# https://www.electronjs.org/docs/latest/tutorial/quick-start
```

Electron debe estar en:
- `C:\Program Files (x86)\electron\electron.exe`
- `C:\Program Files\electron\electron.exe`
- O en tu PATH como `electron.exe`

**Los plugins nativos (.exe) funcionan SIN necesidad de Electron**.