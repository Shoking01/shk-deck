# streamcraft-rs

[![CI](https://github.com/Shoking01/shk-deck/actions/workflows/ci.yml/badge.svg)](https://github.com/Shoking01/shk-deck/actions/workflows/ci.yml)

A native Rust controller for HID stream decks, with no Electron, no WebView, no
WebSocket plugins and no vendor services: the host talks to the device over HID
directly.

The **Redragon StreamCraft SS550** (15 LCD keys) is the first deck it drives, and
the one every protocol document in `docs/protocol/` was measured on. It is not
the only one it is built for: the window, the profiles, the actions, the tray and
the installer all talk to the `Deck` seam in `crates/deck-api`, so a second deck
is a new driver plus a conformance run, not a second application.

## The deck seam

```
deck-api ◀── ss550-protocol ── ss550-driver        (one deck: bytes and semantics)
    ▲
    ├── deck-conformance                            (what every deck must pass)
    ├── deck-simulator                              (a deck with no hardware)
    └── ss550-engine ──▶ ss550-gui ──▶ ss550-tray   (deck-agnostic application)
```

A deck answers four questions — its geometry, the image it accepts, what it can
do, and what it is called — and then behaves: paint a key, clear, brightness,
sleep, dials, events, reconnect. Nothing above the seam names a model, a key
count or an image size; the window reads the grid, the tile size and the offered
controls from whatever deck answered. Two guards keep it that way: the seam never
names a device, and the window's sources may not contain a deck's name.

Adding a deck means implementing `Deck` (and `Transport` for its bytes) and
passing `deck-conformance`, which pairs every case with a flaw it must catch.
`deck-simulator` shows the shape of a deck that is nothing like the one on the
desk.

## What Works Today

| Area | Status |
| --- | --- |
| Deck-agnostic window | grid, tile size, controls and texts come from the deck |
| Connect/wake/paint/clear | including boot re-enumeration and the idle reset |
| Keep awake | throttled heartbeat, never inside an upload |
| Read key presses | both edges, with a host-side debounce policy |
| Actions | **run an app**, **open a URL**, **send a key combination** |
| Action palette | drag an entry onto a key; a template opens its editor and writes nothing |
| Key artwork | the application's own icon, the site's favicon, or a picture you pick from anywhere |
| Per-key colour | white, red or blank; blank is the default |
| Profiles | six named profiles, per deck, persisted as JSON |
| Hotplug | auto-reconnect with exponential backoff |
| Background mode | tray icon; the window hides and the deck stays awake |
| Start with Windows | opt-in, one `HKCU\...\Run` value; the installed copy is refreshed on every start |
| Safe mode | the driver refuses commands that were never proven on hardware |
| Failure reporting | a bound program that dies is watched and named in the status line |

Every deck-specific behaviour above was verified on the actual SS550.

## What's Left

- A second physical deck behind the seam (the simulator proves the shape, not
  the hardware).
- Plugin sandbox (WASM).
- Async transport.
- Linux udev rules and macOS packaging.

## Build & Test

Requires **Rust 1.87+** and, for the GUI, a GPU-capable Windows machine.

```bash
# Workspace: seam, protocol, driver, engine, simulator, conformance
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# GUI window (pulls the GPUI tree, several minutes the first time)
cargo test --manifest-path crates/ss550-gui/Cargo.toml
cargo clippy --manifest-path crates/ss550-gui/Cargo.toml --all-targets -- -D warnings

# Tray and Win32 surface
cargo test --manifest-path crates/ss550-tray/Cargo.toml
cargo clippy --manifest-path crates/ss550-tray/Cargo.toml --all-targets -- -D warnings
```

Three workspaces by design: the seam, the protocol and the driver stay fast and
never pull the GPUI or Win32 trees.

## Development

```bash
# Window
cargo run --manifest-path crates/ss550-gui/Cargo.toml

# Window, straight to the tray
cargo run --manifest-path crates/ss550-gui/Cargo.toml -- --hidden

# Nothing plugged in: the window still runs, and says so
set DECK_CONTROLLER_NO_DECK=1
```

## Installation

See [`INSTALL.md`](INSTALL.md) for installation and the update procedure.

## Project Structure

| Path | Purpose |
| --- | --- |
| `crates/deck-api` | The seam: `Deck` (semantics) over `Transport` (bytes) |
| `crates/deck-conformance` | What every deck must pass, with a flaw per case |
| `crates/deck-simulator` | A six-key deck and a wire fake, for CI and for the seam |
| `crates/ss550-protocol` | Pure HID codec (no I/O, no USB, no time), golden vectors, proptest |
| `crates/ss550-driver` | The first deck: hidapi transport, reconnect, heartbeat |
| `crates/ss550-engine` | Actions, keystrokes, assets, profiles, persistence |
| `crates/ss550-gui` | The window (GPUI Kit). Independent workspace |
| `crates/ss550-tray` | Tray icon, Win32 window and shell calls. The only crate allowed `unsafe` |
| `crates/ss550-probe` | Reverse-engineering tools for a new deck |
| `docs/` | Protocol documents, troubleshooting, milestones |
| `odd/tasks/` | Feature specs, tasks and implementation evidence |

## Contributing

1. Check existing work in `odd/tasks/`
2. Read the spec and its evidence
3. Conventional commits, no attribution trailers
4. `cargo fmt --check` and `cargo clippy -- -D warnings` must pass in every root
5. A new deck: implement the seam, pass the conformance suite, and add nothing
   device-specific above it

## License

MIT OR Apache-2.0
