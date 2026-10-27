# StreamDock Host-Plugin Protocol Specification

Evidence sources (tagged per claim):
- **[captured]** Live traffic recorded by the REA protocol probe (`probe/com.reard.probe.sdPlugin`) against Stream Station 2.9 on 2026-10-27; raw trace in `probe/listener/probe-capture.jsonl`.
- **[static]** Read from shipped plugin code in `%APPDATA%\HotSpot\StreamDock\plugins` (com.gabilan.calendario plugin.js, property inspectors, and the event vocabulary of all 10 installed plugins).
- **[inferred]** Derived, not yet directly observed.

Target: reimplementing the host side for a custom StreamDock app (project `streamdock-host`).

## 1. Transport and lifecycle

1. The host scans `%APPDATA%\HotSpot\StreamDock\plugins` for `*.sdPlugin` folders containing `manifest.json`. [static]
2. Per plugin instance the host allocates a TCP port and a plugin UUID (opaque uppercase alphanumeric, e.g. `6FZ4FC0K750JAK007EX1SJ38347K4WRB`). [captured]
3. Launch by `CodePath` type:
   - Executable plugin: spawned as a process with argv `-port <port> -pluginUUID <uuid> -registerEvent registerPlugin -info <json>`. [captured: streamdockSwitchAudio.exe command line]
   - HTML/JS plugin: `CodePath` points to an `.html` file loaded in the host's embedded Chromium (CEF). The host injects a call to the page-defined global `connectElgatoStreamDeckSocket(port, uuid, event, info)`. [static: calendar plugin defines it and never calls it itself]
4. The plugin connects to `ws://127.0.0.1:<port>` and sends exactly one registration message: `{"uuid":"<pluginUUID>","event":"registerPlugin"}`. [captured]
5. `-info` JSON shape (host to plugin at launch): `{application:{font,language,platform,platformVersion,version}, colors:{...}, devicePixelRatio, devices:[{id,name,size:{columns,rows},type}], plugin:{uuid,version}}`. [captured: switchAudio argv]

## 2. Message envelope

Every post-registration message in both directions carries `event` plus routing fields. [captured + static]

Host -> plugin:
```json
{"action":"<action UUID>","context":"<instance UUID>","device":"<device id>","event":"<event>","payload":{...}}
```

Plugin -> host (observed shapes): `context`-scoped actions send `{event, context, payload}`; `openUrl` sends `{event, payload:{url}}`; registration sends `{uuid, event}`.

`context` identifies one action instance placed on one key; `action` is the manifest action UUID; `device` is the device serial-style id.

## 3. Host -> plugin events

| Event | Evidence | Payload |
| --- | --- | --- |
| `deviceDidConnect` | captured | top-level `device` + `deviceInfo:{name,size:{columns,rows},type}` (no `payload`) |
| `willAppear` | captured | `payload:{coordinates:{column,row}, isInMultiAction:false, settings:{}, state:0}` |
| `willDisappear` | captured | same shape as willAppear |
| `keyDown` / `keyUp` | captured | `payload:{controller:"Keypad", coordinates, isInMultiAction, settings, state, userDesiredState:1}` |
| `titleParametersDidChange` | captured | `payload:{coordinates, settings, state, title:"", titleParameters:{fontFamily,fontSize,fontStyle,fontUnderline,showTitle,titleAlignment,titleColor}}` |
| `didReceiveSettings` | static | settings payload after `getSettings`/`setSettings` |
| `didReceiveGlobalSettings` | static | global settings payload |
| `deviceDidDisconnect` | static | device id |
| `sendToPlugin` | static | property-inspector -> plugin data relay |
| `sendToPropertyInspector` | static | plugin -> property-inspector relay (also used host->PI) |
| `maintenanceResult` | static | seen in a shipped plugin; semantics unverified [inferred] |

`state` is the multi-state toggle index (manifest `States`); `userDesiredState` appears on key events.

## 4. Plugin -> host events

| Event | Evidence | Shape |
| --- | --- | --- |
| `registerPlugin` | captured | `{uuid, event}` — first message after connect |
| `setTitle` | static | `{event, context, payload:{target:0, title}}` |
| `setImage` | static | `{event, context, payload:{target:0, image:"data:image/png;base64,..."}}` (canvas dataURL) |
| `setSettings` | static | `{event, context, payload}` |
| `getSettings` | static | `{event, context}` (or with payload) |
| `setState` | static | `{event, payload:{state}, context}` |
| `getGlobalSettings` / `setGlobalSettings` | static | `{event, context?[, payload]}` |
| `showOk` / `showAlert` | static | `{event, context}` |
| `openUrl` | static | `{event, payload:{url}}` |
| `logMessage` | static | `{event, payload}` |
| `sendToPropertyInspector` | static | `{event, action, context, payload}` |
| `switchToProfile` | static | profile switch request; payload shape unverified [inferred] |
| `setBackgroundImgData` | static | VERIFIED live (exe strings + probe, one variant per connection): `{event, path: <base64 device path as delivered by deviceDidConnect>, payload: { imgData: <base64 PNG> }}`; SDK scales to the background layer (800x480 accepted); raw base64 and `data:` URL both accepted. Payload key `image` is REJECTED with the misleading 'invalid imgData' (the SDK looks up `payload.imgData`, finds nothing, and blames the image - corrected 2026-10-27, the L20 note had the keys swapped). Raw (non-base64) path -> 'device not found'. `setTemporaryBackgroundImgData` exists but is M18/N4Pro-only per binary strings. |
| `requestSensorsData` | static | StreamDock extension (hardware sensors); shape unverified [inferred] |
| `registrationScreenSaverEvent` | static | StreamDock extension; shape unverified [inferred] |
| `read` | VERIFIED live 2026-10-27 | Client -> SDK: INPUT SUBSCRIPTION. `{event, path, payload:{}}`. The SDK routes device input events (key presses arrive back as `read` messages with `payload:{keyId, keyUpOrKeyDown}`) to the client that sent `read` LAST. A client that sends `read` and disconnects keeps the binding: the host then receives NO key events and the device falls into a press-toggles-screen (black/keys) state. Mitigation in host: re-send `read` every 5s while connected. **INPUT keyIds are row-major counted from the BOTTOM-left** (physical top-right reports 15, bottom-left 1, bottom-right 5), while `setKeyImgData` `keyIndex` renders top-down (keyIndex 1 = top-left, VERIFIED 2026-10-27 with corner presses). The host remaps input keyIds to display coordinates before routing to plugins; verified end-to-end the same day (raw 14 -> launcher key opened Brave, raw 13 -> HI key). Garbled keyIds (e.g. 12/15 for adjacent top-row keys) occurred while the deck's USB link was failing - treat inconsistent input as a hardware-link symptom, not a mapping change. The SDK detects device hot-plug and re-offers it without a restart (observed after a replug). Empty payload from other clients (e.g. `setDeviceConfig`) errors 'operation not supported' on 293V3; `changeMode`/`setKeyImagesBulk`/`getConfig`/`getScreenConfig`/`getDeviceConfig` do not exist ('unknown event'). |

## 5. Manifest schema (`.sdPlugin`)

Observed top-level keys [static]: `SDKVersion:1`, `Author`, `Name`, `Icon`, `CodePath` (`.html` for JS plugins; `.exe` plus optional `CodePathMac`/`CodePathWin` for native), `Description`, `Category`, `CategoryIcon`, `Version`, `URL`, `OS` (object or array of `{Platform, MinimumVersion}`), `Software:{MinimumVersion}` (`"2.9.172.0"` seen), `Actions[]`.

`Actions[]` keys: `Name`, `UUID` (reverse-DNS), `Icon`, `States:[{Image, ShowTitle, FontSize?}]`, `Tooltip`, `SupportedInMultiActions`, `UserTitleEnabled`, `PropertyInspectorPath` (optional HTML in `propertyInspector/`). Localization: `<lang>.json` at bundle root (en, es, de, fr, it, ja, ru, zh_CN, pt).

## 6. Property inspector

PI pages are HTML loaded by the host (they read connection params and connect to the same `ws://127.0.0.1:<port>`), bridged to the plugin via `sendToPropertyInspector`/`sendToPlugin` and settings via `setSettings`/`getSettings`. [static] Exact PI registration handshake not yet captured.

## 7. Known gaps (to close during Fase 1)

1. `connect-args` were dropped by probe v0.1 (mirror socket opened after host injection). Fixed in probe v0.2 (message buffering); re-capture on next Stream Station restart confirms the injected-call timing.
2. Payload shapes for the StreamDock extensions (`requestSensorsData`, `switchToProfile`, `registrationScreenSaverEvent`) - extract from plugin source or capture a plugin using them. `setBackgroundImgData` is now VERIFIED (see vocabulary table).
3. Property inspector registration handshake.
4. Key image size/format constraints per device (SS550V3: 5x3; side screen resolution).
5. Multi-action (`isInMultiAction:true`) semantics.
