# REA RD v2 — native host (Rust + egui)

Objective: replace the Electron host with a native Rust host (user goal: no web apps in his own software) while keeping the StreamDock/Elgato-SDK1 WebSocket plugin contract intact (third-party plugins stay HTML by design).
Branch: feat/mvp-host. Delivery strategy: ask-on-risk. Test runner: cargo build / cargo test (no suite yet).

## Specs

S1 — Transport: v2 talks to the official Mirabox SDK server on `ws://127.0.0.1:9002`, same as the Electron host. Inbound events: `deviceDidConnect` (with `path` = BASE64 device path and `payload` = device info), `deviceDidDisconnect`, `read` (key events in `payload`). Outbound commands: `{event:"setKeyImgData", path, payload:{keyIndex, imgData}}` followed by `{event:"refresh", path, payload:{}}`; `{event:"setBackgroundImgData", path, payload:{imgData}}`; `{event:"read", path, payload:{}}`. Reconnect every 2s unless closed by us. Re-assert `read` every 5s (the SDK routes input to the last client that sent it).

S2 — Profile: v2 reads the existing profile file `host/profile.json` with the same schema `{instances:[{plugin, action, column, row, settings, titleColor}]}`; key index = `row*5 + column + 1`. Read-only in this slice (no writes).

S3 — UI: the approved look from `spikes/egui-deck` — Rust + egui chassis + hand-painted widget layer (custom buttons with animated hover/press, custom dropdown, custom-framed text fields). Deck 5x3, header with a device badge that reflects the REAL connection state, Spanish copy.

S4 — Preview: key previews come from the current host dumps (`host/last-setImage-key*.png`) until the plugin runtime exists. Cells without a dump show the empty/assigned styling from the profile.

S5 — End-to-end proof: a "Probar tecla" action sends a real key image through the SDK to the device (proves Rust -> SDK -> device works, not just the socket).

S6 — Plugins (HTML runtime, WS server for plugins) are NOT part of this slice; they get their own task.

## Tasks

- T1 [S1-S5] v2 skeleton: `v2/` cargo project — device client thread (blocking tungstenite + channels), profile loader, egui UI ported from the approved spike, real connection badge, test-paint action. Route: inline. Status: DONE. Evidence: live run connected (`[v2] SDK conectado`, `[v2] deck conectada: HOTSPOTEKUSB HID DEMO 0300D0783C46`), auto-paint on connect sent 8 key images and the SDK log accepted every `setKeyImgData` + `refresh` with no error. Build warning-free.

## Log

- L1 (user request, verbatim): "Vamos con el spike. Ya probamos la funcionalidad y que somos capaces de modificar la Deck a gusto y usarla. Ahora me gustaria buscar lo real y lo que quiero tener" — then "Vamos con la A. Me gusta la idea" (option A: egui + custom-painted widget layer), then "Estan perfectos. El de Quitar se podria modificar y darle el estilo del de guardar", then "Continua" (start the real v2 project).
- L2 (2026-10-30): Spike v2 approved (commits 1e471e3, d7a307b, c353423). Decision carried into v2: egui is the chassis only; every visible control is hand-painted (see `spikes/egui-deck/src/main.rs`, the widget layer is the seed of v2's widget library). Device transport decision for this slice: reuse the proven Mirabox SDK exe over WebSocket (zero-risk parity with the Electron host); a native hidapi transport is a later, optional improvement.
- L3 (2026-10-30): T1 built and verified live. Design facts worth keeping: one owner thread per WebSocket (100ms socket read timeout lets the same thread drain the command channel; no shared locking); commands must be dropped when no device path is known yet; `setKeyImgData` needs the follow-up `refresh`; auto-paint on `deviceDidConnect` restores the last known deck state (parity with the host). Verified through the SDK's own log: it echoes `Rev: setKeyImgData` / `Rev: refresh` for every command and prints nothing on success — an error oracle for wire shapes. Preview source is still the Electron host's dumps (`host/last-setImage-key*.png`), base64-encoded once at boot and reused for painting.
