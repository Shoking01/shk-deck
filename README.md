# Sh-Deck

[![CI](https://github.com/Shoking01/shk-deck/actions/workflows/ci.yml/badge.svg)](https://github.com/Shoking01/shk-deck/actions/workflows/ci.yml)
![Rust 1.87+](https://img.shields.io/badge/rust-1.87%2B-orange)
![Platform: Windows](https://img.shields.io/badge/platform-Windows-0078d4)
![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

**A native Rust controller for the Redragon StreamCraft SS550** — fifteen LCD
keys, one window, one tray icon and no vendor software in between.

There is no Electron, no WebView, no WebSocket plugin host and no cloud service:
the process talks to the device over HID directly, paints every key itself and
runs what you bind to it. The deck stays awake because the app says so, not
because a background service happens to be running.

The SS550 is the deck every protocol document in [`docs/protocol/`](docs/protocol/)
was measured on. It is not the only one the code is built for: the window,
profiles, actions and tray talk to the `Deck` seam in `crates/deck-api`, so a
second deck is a driver plus a conformance run, not a second application.

## What it does

### The deck

- **Paint** — a key can be white, red or blank per profile page, or carry its
  own solid colour or a picture from your library. A bound program shows its own
  icon; a bound URL shows the site's favicon; anything else falls back to the
  program's initial. The window shows the same pixels the deck receives.
- **Name** — every key has its own short title, drawn into the key's picture
  with the font, size, position and case you choose, per key.
- **Drag to rearrange** — drag one key onto another and the two trade places:
  the binding, the picture, the title and its style move together. Nothing is
  overwritten, and the swap is saved.

### Bindings

| What a key does | Details |
| --- | --- |
| Run an app | Full program path, one argument per line; `.bat`/`.cmd` and `.lnk` are started the way Windows expects |
| Open a URL | Any `scheme://` address, refused instead of guessed when the scheme is missing |
| Send keys | Combinations like `ctrl+shift+m`, sent as **hardware scan codes** with the `E0` prefix and a 20 ms hold, so games read them like a real keyboard |
| Media keys | Play/pause, next, previous, stop — Windows hands them to the active media session |
| Show a reading | A live measurement on the key (see below), optionally with a media key on press |
| Navigate | Open a folder page, go back, or switch profile |

A draggable palette binds without typing: drag a template onto a key to open its
editor, drag a finished action from `RECENTS` to bind it again.

### Pages and profiles

A profile holds named **pages** — folders — and a key on one page can open
another. Every page keeps its own bindings, artwork and titles, which is how one
physical deck becomes a stream layout, a meeting layout and a gaming layout
without touching a config file. Six profiles ship built in (`default`, `stream`,
`meetings`, `gaming`, `work`, `entertainment`); they live per deck as JSON in
`%APPDATA%\ss550\profiles` and a key can switch between them.

### Live readings

A key can display a measurement instead of a picture, drawn on the deck by the
app and kept on its own cadence:

- **Clock** — a digital clock on one key or several.
- **System stats** — CPU and GPU load and temperature, RAM use and free disk
  space.
- **FPS** — the frame rate RivaTuner Statistics Server (RTSS) already counts,
  read from its shared memory.

Every reading is **read-only** by construction: the sensors crate declares no
setter of the vendor API it calls and no command of the media session it reads.
The `Stats` page shows what each reading is doing and what it has been doing for
the last few minutes.

### Look and feel

The window carries named themes — **Sh-Deck Ink** (default), **Catppuccin
Mocha**, **Catppuccin Latte** and **One Dark** — and a theme brings its own
mode, so Latte is a real light window instead of light colours in dark chrome.
The title, the .ico and the tray icon are one mark.

### Away from the window

Closing the window hides it; the controller keeps running, keeps the deck awake
and keeps answering the keys. The tray tooltip says whether the deck is
connected and what the readings say. **Start with Windows** is one opt-in
`HKCU\...\Run` value, and the running build refreshes the installed copy at
sign-in so the next logon is never last week's binary.

## Design

- **The deck seam.** A deck answers four questions — its geometry, the image it
  accepts, what it can do, and what it is called — and then behaves. Nothing
  above the seam names a model, a key count or an image size.
- **No middlemen.** No vendor service, no telemetry, no plugin host that talks
  to the network. Plugins are compiled in, and the app makes no outbound request
  except the favicon of a URL you bound.
- **Honest failure.** A picture the library no longer holds is a per-key miss
  with its reason in the status line, not a black deck. A dropped connection
  backs off exponentially, proves the transport accepts a write before painting,
  and reconnects by itself.

```
deck-api ◀── ss550-protocol ── ss550-driver        (one deck: bytes and semantics)
    ▲
    ├── deck-conformance                            (what every deck must pass)
    ├── deck-simulator                              (a deck with no hardware)
    └── ss550-engine ──▶ ss550-gui ──▶ ss550-tray   (deck-agnostic application)
                            ▲
              ss550-plugin-api ◀── ss550-plugins ── ss550-sensors
                            (readings: clock, system stats, fps)
```

## Safety on real hardware

The SS550 is a physical USB device, and a bad packet can wedge it.

- Every command the app sends was verified on the hardware and is documented in
  [`docs/protocol/`](docs/protocol/).
- The driver runs in a **safe mode**: commands that were never proven on the
  device are refused instead of sent.
- The protocol decoder is fuzzed and property-tested; codecs are unit-tested
  against golden vectors.
- When a deck stops answering, the recovery order is written down in
  [`docs/troubleshooting/deck-stuck.md`](docs/troubleshooting/deck-stuck.md)
  before anything is changed in code.

## Requirements

- **Windows 10/11** is the supported platform today. The workspaces build and
  test on Linux and macOS, but the window and the tray are Windows-first.
- **Rust 1.87+** (2021 edition).

## Build and test

```bash
# Seam, protocol, driver, engine, simulator, conformance
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings

# The window (GPUI; the first build takes a few minutes)
cargo test --manifest-path crates/ss550-gui/Cargo.toml --locked
cargo clippy --manifest-path crates/ss550-gui/Cargo.toml --all-targets --locked -- -D warnings

# The tray and the Win32 surface
cargo test --manifest-path crates/ss550-tray/Cargo.toml --locked
cargo clippy --all-targets --manifest-path crates/ss550-tray/Cargo.toml --locked -- -D warnings
```

Three workspace roots by design: the seam, the protocol and the driver stay fast
and never pull the GPUI or Win32 dependency trees. CI runs the same three on
every push and pull request, across Windows, Linux and macOS for the library
crates. Run the window against a simulator with no hardware attached:

```bash
cargo run --manifest-path crates/ss550-gui/Cargo.toml
set DECK_CONTROLLER_NO_DECK=1
```

## Install

The short version, on Windows:

```powershell
.\update-steamcraft.ps1
```

It builds the release binary, stops the running instance, installs it into
`%LOCALAPPDATA%\Programs\ss550`, verifies the copy and starts it.
[`INSTALL.md`](INSTALL.md) has the manual path, the update procedure and what to
do when Windows keeps an older build mapped.

## Project layout

| Path | Purpose |
| --- | --- |
| `crates/deck-api` | The seam: `Deck` (semantics) over `Transport` (bytes) |
| `crates/deck-conformance` | What every deck must pass, with a flaw per case |
| `crates/deck-simulator` | A six-key deck and a wire fake, for CI and the seam |
| `crates/ss550-protocol` | Pure HID codec — no I/O, no USB, no time — golden vectors, proptest, fuzz target |
| `crates/ss550-driver` | The SS550: hidapi transport, reconnect, heartbeat, safe mode |
| `crates/ss550-engine` | Actions, keystrokes, assets, profiles, pages, persistence |
| `crates/ss550-plugin-api` | What a reading is: cells, readings, cadence |
| `crates/ss550-plugins` | The built-in readings: clock, system stats, fps |
| `crates/ss550-sensors` | Read-only sensor calls: AMD ADL, PawnIO, WinRT media session |
| `crates/ss550-gui` | The window (GPUI Kit) — independent workspace |
| `crates/ss550-tray` | Tray icon, Win32 window and shell calls — independent workspace |
| `crates/ss550-probe` | Reverse-engineering tools used to bring up a new deck |
| `crates/ss550-perf` | Performance budgets, measured |
| `docs/` | Protocol documents, troubleshooting, milestones |
| `odd/tasks/` | Feature specs, tasks and the evidence they were closed with |

## Documentation

- [`docs/actions.md`](docs/actions.md) — what a key can do, and why some things
  are readings instead of actions.
- [`docs/plugins.md`](docs/plugins.md) — what a reading is, its cells, and where
  its numbers come from.
- [`docs/troubleshooting/deck-stuck.md`](docs/troubleshooting/deck-stuck.md) —
  the diagnosis and recovery order for a deck that stops answering.
- [`docs/protocol/`](docs/protocol/) — the measured protocol: commands, frames,
  verified vectors and the incidents behind them.

## Status and roadmap

Working today: the window, the tray, profiles, pages, drag-to-rearrange
bindings, artwork, titles, themes, live readings, heartbeat, reconnect and the
Windows install/update path.

Next, in the order the issues are taken:

- A second physical deck behind the seam (the simulator proves the shape, not
  the hardware).
- A plugin sandbox (WASM) for third-party readings; today they are compiled in.
- Packaging for Linux and macOS, where the library crates already build.
- An async transport.

## Contributing

1. Read the spec for the area in `odd/tasks/` — the task list and its evidence
   are the design record.
2. Keep the seam honest: nothing above `deck-api` may name a device.
3. Conventional Commits, no attribution trailers.
4. `cargo fmt --check` and `cargo clippy -- -D warnings` must pass in every
   workspace root you touch.
5. A change to a key or a command needs its test; a change to what the deck
   says needs its protocol document.

## License

MIT OR Apache-2.0, as declared in every crate's `Cargo.toml`.
