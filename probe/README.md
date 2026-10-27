# REA Protocol Probe

Phase 0 tooling for the StreamDock host project. Two pieces:

1. `com.reard.probe.sdPlugin/` — a minimal StreamDock plugin (HTML/JS, same
   format as the shipped plugins). When Stream Station loads it, it connects to
   the host WebSocket and mirrors every message (both directions, plus the
   injected connection arguments) to a local listener.

2. `listener/` — a tiny Node.js WebSocket server that records the mirrored
   traffic to `probe-capture.jsonl`.

## Run

```powershell
cd probe\listener
npm install
node listener.js
```

Then copy `com.reard.probe.sdPlugin` into
`%APPDATA%\HotSpot\StreamDock\plugins\` and restart Stream Station. The
listener console prints every capture batch; the JSONL file holds the full
protocol trace. Delete the plugin folder from the plugins directory to
uninstall the probe.
