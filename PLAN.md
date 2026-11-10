# PLAN.MD — StreamCraft SS550 Native Rust Controller

## 1. Project Overview

**Goal:** Build a native, cross-platform desktop application in Rust to fully control the Redragon StreamCraft SS550 stream controller (15 LCD buttons, 6 profiles), replacing the official "Stream Station" software.

**Why:** The official Redragon software is outdated, Windows/macOS-only, and closed-source. A native Rust app gives us performance, safety, and full control.

**Hardware Target:**
- Redragon StreamCraft SS550 (USB VID:PID `0200:1000`)
- Redragon StreamCraft SS550 v3 (USB VID:PID `0200:3000`)
- 15 LCD buttons, 6 customizable profiles, USB interface

## 2. Tech Stack Decision

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Language | Rust (edition 2021, MSRV 1.87+) | Memory safety, performance, ecosystem |
| GUI Framework | **egui** (primary) | Immediate-mode, cross-platform, rapid iteration, excellent for control-panel style UIs |
| GUI Alternative | GPUI (fallback) | If native performance or Zed-style UI is needed later; note GPUI is currently macOS/Linux-focused |
| USB Communication | `nusb` or `rusb` | Modern async USB HID communication |
| Image Processing | `image`, `imageproc` | Resize/format icons for LCD (typically 72x72 or similar per button) |
| Async Runtime | `tokio` | Async USB I/O and plugin event loops |
| Serialization | `serde`, `toml`, `json` | Profile/config persistence |
| Plugin System | `libloading` (dynamic) or WASM (`wasmtime`) | Extensible actions (OBS, Twitch, etc.) |

**Decision: Use egui.** It is the most mature immediate-mode Rust GUI, works on Windows/macOS/Linux, and is ideal for a control-panel application. GPUI can be evaluated in Phase 5 if needed.

## 3. Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     UI Layer (egui)                          │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────────┐  │
│  │ Button   │ │ Profile  │ │ Icon     │ │ Plugin        │  │
│  │ Grid     │ │ Manager  │ │ Editor   │ │ Marketplace   │  │
│  └──────────┘ └──────────┘ └──────────┘ └───────────────┘  │
├─────────────────────────────────────────────────────────────┤
│                   Core Business Logic                        │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────────┐  │
│  │ Profile  │ │ Action   │ │ Icon     │ │ Event         │  │
│  │ Engine   │ │ Executor │ │ Renderer │ │ Dispatcher    │  │
│  └──────────┘ └──────────┘ └──────────┘ └───────────────┘  │
├─────────────────────────────────────────────────────────────┤
│                    Hardware Abstraction                      │
│  ┌────────────────────────────────────────────────────────┐ │
│  │              SS550 HID Protocol Driver                  │ │
│  │  ┌─────────────┐  ┌─────────────┐  ┌────────────────┐ │ │
│  │  │ REA-MCP     │  │ Protocol    │  │ Device         │ │ │
│  │  │ Analyzer    │  │ Encoder/    │  │ Discovery &    │ │ │
│  │  │ (passive)   │  │ Decoder     │  │ Hotplug        │ │ │
│  │  └─────────────┘  └─────────────┘  └────────────────┘ │ │
│  └────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                    Plugin System (WASM)                      │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────────┐  │
│  │ OBS      │ │ Twitch   │ │ System   │ │ Custom        │  │
│  │ Plugin   │ │ Plugin   │ │ Monitor  │ │ Plugins       │  │
│  └──────────┘ └──────────┘ └──────────┘ └───────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## 4. Phases & Milestones

### Phase 0: Research & Protocol Analysis (Weeks 1–2)
**Objective:** Fully understand the SS550 USB protocol using REA MCP.

| Task | Deliverable |
|------|-------------|
| Set up REA MCP environment | Working REA instance with target acquisition |
| Capture official Stream Station traffic | PCAP/REA session logs of all operations |
| Document HID report descriptors | `docs/protocol/hid-descriptors.md` |
| Document button image upload protocol | `docs/protocol/image-upload.md` |
| Document button press events | `docs/protocol/input-events.md` |
| Document profile switching | `docs/protocol/profiles.md` |
| Identify LCD resolution & color format | `docs/protocol/display-specs.md` |
| Create protocol test harness | Rust CLI tool to send raw packets |

**REA MCP Usage:**
- Use REA to perform dynamic analysis of `StreamStation.exe` (Windows)
- Hook USB HID APIs (`HidP_SendReport`, `WriteFile` on HID handles)
- Trace image encoding (likely BMP/JPEG/RAW RGB)
- Trace button press interrupt reports
- Build a "protocol dictionary" of commands → responses

**Success Criteria:** Can replicate every basic function (set image, read button press, switch profile) with raw USB packets from a Rust test tool.

### Phase 1: Core Hardware Driver (Weeks 3–5)
**Objective:** Production-ready Rust crate for SS550 communication.

> **Status (closed 2026-11-06):** `ss550-protocol` (pure) + `ss550-driver`
> (sync `hidapi`) + `ss550-probe` live. Wire mathes `mirajazz` byte-exact;
> protocol coverage **100% lines/functions/regions**. Items below marked
> `PARTIAL`/`DEFERRED` are explicitly out of the close-out, not silently
> dropped. See `CHANGELOG.md` and `docs/protocol/CHANGELOG.md` for the
> honest list. No live driver session on a dedicated test device yet —
> the probe path is verified, the driver bytes are not yet hardware-certified.

- [x] Create `ss550-protocol` crate: pure protocol encoding/decoding (no I/O)
- [x] Create `ss550-driver` crate: sync USB I/O via `hidapi` (async `nusb` deferred — see below)
- [~] Implement device discovery (VID:PID enumeration done; hotplug watcher deferred)
- [x] Implement firmware version query (`CRT|VER` write + `get_input_report` parse; live parse pending)
- [x] Implement button image upload (all 15 buttons; probe `--all` live-verified, driver not yet hardware-certified)
- [ ] Implement button press event stream (interrupt IN endpoint) — deferred, decode unknown
- [x] Implement brightness control (restore 100 after flush)
- [ ] Implement profile/page switching — deferred, `insertSettings`/`ICO` path is static-only and unverified
- [x] Implement sleep/wake display (wake `DIS+LIG0`, sleep `CRT|HAN`)
- [~] Cross-platform support: Windows builds verified; macOS/Linux CI + udev rules deferred

**Deviations decided at close-out (explicit, not drift):**
- Transport is synchronous `hidapi`, not async `nusb`/`tokio`. Rationale: minimal
  risk to land the byte-exact path; async is the Phase 2/deferred item, not a
  regression. A future async crate must keep the same `ss550-protocol` bytes.
- Safe-mode (`docs/protocol/verified.md`), simulator, fuzzer, and
  `INCIDENTS.md` from AGENTS.md are intentionally not created yet — they are
  gating items before sending the static-only profile path, not before closing
  the plain-image path.

**Success Criteria (original):** Integration tests pass on all 3 OSes; can
render a full 15-button profile with custom images.
**Status at close-out:** unit/doc/property tests pass (34) on Windows; probe
`--all` renders 15 keys live; driver live render + multi-OS CI remain open
and are the entry criteria for Fase 2 hardware work.

### Phase 2: GUI Application Foundation (Weeks 6–8)
**Objective:** Working egui application shell.

- [ ] Project scaffolding (`cargo new`, workspace layout)
- [ ] egui + eframe setup with custom theme (dark, streamer aesthetic)
- [ ] Device connection status indicator
- [ ] Virtual SS550 preview (render 15-button grid in UI)
- [ ] Drag-and-drop icon assignment to virtual buttons
- [ ] Profile manager (create, rename, delete, switch 6 profiles)
- [ ] Config persistence (`~/.config/streamcraft-rs/profiles.toml`)
- [ ] System tray integration (minimize to tray)

**Success Criteria:** User can visually arrange a profile in the app and see it rendered on the physical device in real-time.

### Phase 3: Action Engine & Plugins (Weeks 9–12)
**Objective:** Make buttons actually do things.

- [ ] Action type system: KeyCombo, TextInput, URL, Hotkey, LaunchApp, Multimedia, OBS, Twitch, System
- [ ] Action executor with OS-level input injection (`enigo` crate)
- [ ] OBS WebSocket plugin (scene switching, source toggle, streaming control)
- [ ] Twitch chat integration (send messages, run ads)
- [ ] System monitoring plugin (CPU/GPU/RAM to LCD as dynamic icons)
- [ ] Multi-action sequences with delays
- [ ] WASM plugin sandbox (optional advanced)

**Success Criteria:** All 15 buttons on all 6 profiles can trigger meaningful actions; OBS plugin works live.

### Phase 4: Icon & Asset Pipeline (Weeks 13–14)
**Objective:** Rich visual customization.

- [ ] Icon library browser (built-in packs)
- [ ] Custom image import (PNG, JPG, SVG → device format)
- [ ] Icon editor: crop, resize, add text/badges, GIF animation frames
- [ ] GIF → animated button support (if protocol allows frame streaming)
- [ ] Icon pack import/export (`.ss550pack` format)

**Success Criteria:** Users can create professional-looking button layouts in under 5 minutes.

### Phase 5: Polish & Distribution (Weeks 15–16)
**Objective:** Production release.

- [ ] Auto-updater (Tauri-style or custom)
- [ ] Installer packages (MSI, DMG, AppImage/deb)
- [ ] First-run onboarding wizard
- [ ] Import profiles from official Stream Station (if format reverse-engineered)
- [ ] Performance optimization (<50MB RAM, <1% CPU idle)
- [ ] GPUI evaluation: prototype alternative UI (optional stretch goal)
- [ ] Comprehensive documentation & user guide

**Success Criteria:** v1.0 release with installers for all platforms.

## 5. REA MCP Integration Strategy

Since REA MCP (Reverse Engineer Anything) is the key to understanding the closed protocol:

### 5.1 Setup
```bash
# REA MCP server configuration (conceptual)
# Add to your MCP client (Claude Desktop, etc.):
{
  "mcpServers": {
    "rea": {
      "command": "rea-mcp-server",
      "args": ["--target", "StreamStation.exe"]
    }
  }
}
```

### 5.2 Analysis Workflow
1. **Static Analysis:** Use REA to disassemble `StreamStation.exe`, locate USB HID write functions
2. **Dynamic Tracing:** Hook `DeviceIoControl` / `HidD_SetFeature` calls with REA
3. **Packet Capture:** Log all USB traffic during:
   - Button image upload (send a solid red image → identify header)
   - Button press (press physical button → identify interrupt report)
   - Profile switch (change profile in software → identify command)
4. **Differential Analysis:** Compare packets for button 1 vs button 2 to locate index bytes
5. **Fuzzing:** Send mutated packets to device (safe ranges) to discover undocumented features

### 5.3 Protocol Documentation Template
```markdown
## Command: SET_BUTTON_IMAGE
- **Direction:** Host → Device
- **Endpoint:** 0x01 (Interrupt OUT)
- **Report ID:** 0x02
- **Payload Structure:**
  - Byte 0: Report ID (0x02)
  - Byte 1: Command (0x?? — from REA trace)
  - Byte 2: Button index (0–14)
  - Bytes 3–4: Image width (LE u16)
  - Bytes 5–6: Image height (LE u16)
  - Bytes 7+: Pixel data (BGR565 or RGB565)
```

## 6. Project Structure

```
streamcraft-rs/
├── Cargo.toml                 # Workspace root
├── crates/
│   ├── ss550-protocol/        # Pure protocol codec
│   ├── ss550-driver/          # USB I/O driver
│   ├── ss550-ui/              # egui application
│   ├── action-engine/         # Action execution
│   ├── plugin-api/            # Plugin trait definitions
│   └── plugins/
│       ├── obs/               # OBS WebSocket
│       ├── twitch/            # Twitch chat/API
│       └── system/            # System monitoring
├── docs/
│   ├── protocol/              # REA-derived protocol docs
│   ├── development/           # This plan, AGENTS.md
│   └── user/                  # End-user guide
├── assets/
│   ├── icons/                 # Built-in icon packs
│   └── fonts/
├── tests/
│   ├── integration/           # Hardware-in-loop tests
│   └── protocol/              # Codec unit tests
└── scripts/
    ├── rea-capture/           # REA MCP session helpers
    └── package/               # Release packaging
```

## 7. Risk Register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Protocol uses proprietary image compression | Medium | REA analysis will reveal; fallback to raw RGB565 |
| Device requires encrypted/signed commands | Low | REA will show; SS550 is budget device, unlikely |
| egui performance issues with 15 animated LCDs | Low | Throttle to 30fps; use texture atlases |
| GPUI not ready for Windows | High | Stick with egui; GPUI is stretch goal only |
| USB permissions on Linux | Medium | Ship udev rules; document setup |
| Official software blocks device when running | Medium | Detect and warn user to close Stream Station |

## 8. Success Metrics

- [ ] 100% protocol coverage of official software features
- [ ] <100ms latency from button press to action execution
- [ ] <50MB RAM usage at idle
- [ ] Zero crashes in 48-hour stress test
- [ ] 3+ community plugins developed

## 9. References

- OpenDeck SS550 Plugin (MMonkeyKiller) — existing Rust implementation proving protocol viability
- `elgato-streamdeck` crate — prior art for stream deck protocols in Rust
- Redragon official specs: 15 buttons, 6 profiles, USB
