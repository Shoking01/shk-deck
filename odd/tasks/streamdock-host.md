# StreamDock Host (REA RD)

Build a custom desktop host app for the Redragon Stream Craft SS550 (Mirabox OEM hardware,
reports as `SS550V3`, 5 columns x 3 rows, side screen) that runs `.sdPlugin` plugins using the
StreamDock/Elgato-SDK1-style WebSocket protocol, reusing the user's installed plugins, with a
fully customizable UI. Replaces Stream Station (Qt5 C++ + CEF, `C:\Program Files (x86)\Stream Station`).

Branch: none yet (workspace `REA RD` is not a git repository). Delivery strategy: ask-on-risk.
Test runner: none established yet; phase 0 verification is manual + syntax checks.

## Specs

- S1: User goal (verbatim): "Mi idea es hacerle Reverse Engineer a la app de mi StreamDeck y asi poder hacer mi propia app, hacer la correr con mis plugins y si es posible importar algunos de la misma para aprovecharlos. Copiar el funcionamiento pero hacerla personalizable a mi gusto."
- S2: Device: Redragon Stream Craft SS550 (user correction verbatim: "Como tal es una Redragon Stream Craft SS550, y si el Software es Stream Station"). Host `-info` reports device name `SS550V3`, `size: {columns: 5, rows: 3}`, `type: 0`.
- S3: Plugins live in `%APPDATA%\HotSpot\StreamDock\plugins` as `.sdPlugin` bundles. 10 installed: com.gabilan.calendario, com.hotspot.streamdock.discord, com.hotspot.streamdock.memo, com.hotspot.streamdock.system.monitor, com.hotspot.streamdock.weather, com.inginversa.shplugins, com.mirabox.streamdock.calendar, com.mirabox.streamdock.spotify, com.mirabox.streamdock.switchAudio, com.mirabox.streamdock.time.
- S4: Phase 0 authorized by user ("Puedes proceder con la Fase 0"): (a) static inventory of the plugin API surface, (b) WS probe plugin that captures the live host<->plugin protocol, (c) verify the official MiraboxSpace StreamDock-Device-SDK supports the device.
- S5: Legal constraint (clean-room): never copy Stream Station assets or code; observe protocol and reimplement. Third-party plugins: personal local use only, no redistribution.
- S6 (Fase 1, authorized "Procede a Fase 1"): MVP host controls the SS550V3 through the official MiraboxSpace WebSocket-SDK device server (`WebsocketSDK.exe`, ws://127.0.0.1:9002), registering the Redragon OEM VID/PID against the `293V3` protocol model when needed. Acceptance: host receives `deviceDidConnect` for the device and successfully sets an image on a key.
- S7: MVP host runs one real installed JS/HTML `.sdPlugin` inside an embedded Chromium page using the captured launch contract (hidden window + injected `connectElgatoStreamDeckSocket(port, uuid, event, info)`), relays `keyDown`/`keyUp` from the device to the plugin context, and answers `getSettings`. Acceptance: plugin connects, registers, receives `willAppear`, and reacts to a physical key press.
- S8: Images the plugin sends via `setImage` (PNG dataURL) are rendered on the mapped device key via `setKeyImgData`. Acceptance: the device key shows plugin-drawn pixels.

## Verified protocol facts (evidence, phase 0)

- Host launches EXE plugins with: `-port <port> -pluginUUID <uuid> -registerEvent registerPlugin -info <json>` (captured live from `streamdockSwitchAudio.exe` command line, port 18618, app version 2.9).
- JS plugins are HTML pages loaded by the host's embedded Chromium (manifest `CodePath: "plugin/index.html"`, `SDKVersion: 1`). The host calls the page-defined global `connectElgatoStreamDeckSocket(port, uuid, event, info)`; the plugin then connects to `ws://127.0.0.1:<port>` and registers with `{uuid, event}` (observed in com.gabilan.calendario plugin.js).
- Message shapes from real plugin code: `setImage` -> `{event:"setImage", context, payload:{target:0, image:<png dataURL>}}`; `setTitle` -> `{event:"setTitle", context, payload:{target:0, title}}`; `setSettings` -> `{event:"setSettings", context, payload}`; `setState` -> `{event:"setState", payload:{state}, context}`; `sendToPropertyInspector` -> `{event, action, context, payload}`; `openUrl` -> `{event:"openUrl", payload:{url}}`.
- Event vocabulary used by the 10 installed plugins (outgoing): getGlobalSettings, getSettings, logMessage, openUrl, registrationScreenSaverEvent, requestSensorsData, sendToPlugin, sendToPropertyInspector, setBackground, setGlobalSettings, setImage, setSettings, setState, setTitle, showAlert, showOk, stopBackground, switchToProfile. StreamDock-specific extensions beyond Elgato SDK: setBackground, stopBackground, requestSensorsData, switchToProfile, registrationScreenSaverEvent.
- Incoming events observed in plugin code: didReceiveGlobalSettings, didReceiveSettings, deviceDidDisconnect, willAppear, sendToPlugin, sendToPropertyInspector, registerPlugin, registerPropertyInspector. (keyDown/keyUp not seen statically; the probe captures the real stream.)
- Manifest schema (observed): SDKVersion, Author, Name, Icon, CodePath (+CodePathMac/CodePathWin for EXE), Description, Category, CategoryIcon, Version, URL, OS, Software.MinimumVersion ("2.9.172.0" seen), Actions[] with UUID/Icon/States/PropertyInspectorPath/Tooltip/SupportedInMultiActions.
- Stream Station host internals: Qt5 + CEF (`libcef.dll`, `QCefView.dll`, `CefViewWing.exe`), `Qt5WebSockets.dll`, `libusb-1.0.dll`, `SDLibrary1.dll`, `ffmpeg-6.dll`, `debug.log`. Not Electron; no asar.
- Device SDK (official, MIT): github.com/MiraboxSpace/StreamDock-Device-SDK with CPP-SDK, Python-SDK (Windows x64 supported; DeviceManager.enumerate/open/init/set_key_image/set_touchscreen_image/set_key_callback/refresh; async input on 293V3/N4/N4Pro/XL/M3/M18), WebSocket-SDK, docs at creator.key123.vip. SS550V3 corresponds to the 293V3 class (5x3 + side screen); VID/PID match to be confirmed at runtime in phase 1.

## Tasks

- T1 [S4a] Inventory plugin API surface (grep over installed plugins) — inline — done: event lists above.
- T2 [S4c] Verify Device-SDK support for the device — inline — done: Python-SDK supports Windows x64, 293V3 class listed; runtime VID/PID check deferred to phase 1.
- T3 [S4b] Build WS probe plugin + local listener — inline — done: `probe/com.reard.probe.sdPlugin/` (manifest.json, plugin/index.html, plugin/probe.js, image/ico.png borrowed placeholder from an installed plugin for local testing only) and `probe/listener/` (node + ws, writes probe-capture.jsonl).
- T4 [S4b] Capture protocol: install probe into the plugins dir, restart Stream Station, run listener, exercise UI/keys, collect log — done: 33 lines captured (30 host->plugin messages: deviceDidConnect x1, willAppear x2, willDisappear x2, keyDown x8, keyUp x8, titleParametersDidChange x9; registration `{uuid, event:"registerPlugin"}` confirmed live). Evidence: probe/listener/probe-capture.jsonl.
- T5 [S4] Write protocol specification from captured traffic — done: `docs/streamdock-protocol.md` (evidence-tagged: captured/static/inferred; gaps listed in its section 7).
- T6 [S6] Vendor WebSocket-SDK (Windows.zip), discover Redragon VID/PID, start device server with OEM alias — inline
- T7 [S6] Device loop unit: `host/device.js` connects to the SDK server, sets a key image, listens for key events — inline
- T8 [S7] Electron plugin runtime: hidden BrowserWindow + injected connect call + plugin WS server + event routing — inline
- T9 [S7,S8] End-to-end: real plugin (com.mirabox.streamdock.time) renders to the device and reacts to keys — inline

## Log

- L1 (2026-10-27): User request verbatim: "Como tal es una Redragon Stream Craft SS550, y si el Software es Stream Station. Puedes proceder con la Fase 0". Correction: device is Redragon Stream Craft SS550 (Mirabox OEM), not Elgato; software confirmed Stream Station. Phase 0 authorized.
- L1b: Prior context: REA MCP installed for OpenCode (rea-agents@6.8.0); feasibility analysis with live protocol capture saved (Engram obs-442).
- L2 (2026-10-27): T1/T2 completed. Static inventory produced the message vocabulary and connection mechanism above; Device-SDK verified (MIT, Windows x64, 293V3 class). No runnable test framework applies to phase 0 analysis artifacts; checks are structural (file reads, grep inventories, syntax checks).
- L3 (2026-10-27): T3 completed. Probe plugin built in workspace; not yet installed into the live plugins dir (explicit user approval required before touching Stream Station's plugin folder).
- L4 (2026-10-27): User approved install ("Instálala tú"). Probe copied to %APPDATA%\HotSpot\StreamDock\plugins\com.reard.probe.sdPlugin (4 files verified). Listener running in background on 127.0.0.1:4455 (TcpTestSucceeded True). Awaiting user restart of Stream Station + interaction; capture file: probe/listener/probe-capture.jsonl.
- L5 (2026-10-27): T4/T5 completed. User restarted Stream Station and exercised the probe action: 33 capture lines, 30 host->plugin messages. Wire format confirmed identical to Elgato SDK1 (envelope {action, context, device, event, payload}; keyDown/keyUp with controller:"Keypad" and userDesiredState; deviceDidConnect pushed to plugins). Registration {uuid, event:"registerPlugin"} captured live. Gap found: probe v0.1 dropped connect-args (mirror opened after host injection); probe v0.2 with buffering deployed to the plugins dir (effective on next restart). Spec written: docs/streamdock-protocol.md. Phase 0 CLOSED. Next: Fase 1 MVP host (pending user go).
