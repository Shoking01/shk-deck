# REA RD host — MVP

Custom host for the Redragon Stream Craft SS550 (Mirabox 293V3 hardware).
Replaces Stream Station; runs existing `.sdPlugin` plugins.

## Architecture (Fase 1 MVP)

```
StreamDockWebsocketSDK.exe (official Mirabox device server, ws://127.0.0.1:9002)
        ^                    |
        | JSON over WS       | key events / image commands
        v                    v
   host/device.js  -----> device keys (SS550V3, 5x3)
   host/main.js    -----> plugin WS server -> .sdPlugin pages (Electron)
```

## Setup

1. Download the official WebSocket-SDK (gitignored):
   `https://github.com/MiraboxSpace/StreamDock-Device-SDK/raw/main/WebSocket-SDK/Windows.zip`
   Extract so that `third_party/websocket-sdk/StreamDockWebsocketSDK.exe` exists.
2. `cd host && npm install`

## Run device loop test (T7)

```powershell
# terminal 1 - device server (Redragon SS550 = VID 0x0200 PID 0x3000, 293V3 protocol)
third_party\websocket-sdk\StreamDockWebsocketSDK.exe -oem 293V3:0x0200:0x3000

# terminal 2 - host device client
cd host
node device.js
```

Expected: `deviceDidConnect` with `Type: "293V3"`, test image sent to key 1,
key events logged as `{"keyId":N,"keyUpOrKeyDown":"keyDown|keyUp"}` (keyId is 1-based, row-major on the 5x3 grid).
