# streamcraft-rs

[![CI](https://github.com/Shoking01/shk-deck/actions/workflows/ci.yml/badge.svg)](https://github.com/Shoking01/shk-deck/actions/workflows/ci.yml)

A native Rust controller for the Redragon StreamCraft SS550 (15 LCD keys, `VID 0x0200 / PID 0x0300`). This project provides a lightweight alternative to the vendor's Stream Station software, with no Electron, WebView2, WebSocket plugins, or vendor services. Communication with the device happens directly via HID.

## Architecture

```
protocol ──▶ driver ──▶ engine ──▶ gui (GPUI Kit)
                         │           │
                         └───────────┴──▶ tray (Win32)
```

## What Works Today

| Area | Status |
| --- | --- |
| Connect/wake/paint/clear | done, including boot re-enumeration and 68.7s idle reset |
| Keep awake | 500ms heartbeat (`keepalive`), throttled to 2s |
| Read key presses | both edges reported, host-side debounce policy |
| Bind to actions | v1 catalogue: **App** and **URL** only |
| Run actions | `.exe` by path, `.lnk` via explorer, `.bat`/`.cmd` via `cmd /C call`, URLs by scheme |
| Background mode | tray icon, window hides (deck stays awake) |
| Start with Windows | opt-in, one `HKCU\...\Run` value, off by default |
| Report dead programs | watched for 6s, shown in status line |
| Safe mode | driver refuses unproven commands |
| Profiles (M6) | 6 named profiles with JSON persistence |
| Hotplug (M7) | auto-reconnect with exponential backoff |

Every feature above has been verified on the actual hardware.

## What's Left to Do

Listed in [`CHANGELOG.md`](#known-limitations):
- Plugin sandbox
- Async transport
- Linux udev rules

## Build & Test

Requires **Rust 1.87+** and, for the GUI, a GPU-capable Windows machine.

```bash
# Workspace (protocol/driver/engine/probe)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# GUI window (pulls the GPUI tree, several minutes first time)
cargo test --workspace --manifest-path crates/ss550-gui/Cargo.toml
cargo clippy --manifest-path crates/ss550-gui/Cargo.toml --all-targets -- -D warnings

# Tray (Win32 + tray-icon)
cargo test --manifest-path crates/ss550-tray/Cargo.toml
cargo clippy --manifest-path crates/ss550-tray/Cargo.toml --all-targets -- -D warnings
```

Three separate workspaces by design: driver/protocol stays fast, never pulls 800 packages of GPUI or Win32.

## Development

```bash
# Run GUI window (development)
cargo run --manifest-path crates/ss550-gui/Cargo.toml

# Run GUI in tray-only mode (hidden)
cargo run --manifest-path crates/ss550-gui/Cargo.toml -- --hidden
```

## Installation

See [`INSTALL.md`](INSTALL.md) for detailed installation instructions and update procedures.

## Project Structure

| Path | Purpose |
| --- | --- |
| `crates/ss550-protocol` | Pure HID codec (no I/O, no USB, no time). Golden vectors + proptest |
| `crates/deck-api` | The deck seam: `Deck` (semantics) over `Transport` (bytes) |
| `crates/deck-conformance` | Conformance suite (11 tests every deck must pass) |
| `crates/deck-simulator` | 6-key deck and wire fake for CI |
| `crates/ss550-driver` | Synchronous hidapi transport, reconnect, heartbeat |
| `crates/ss550-engine` | Actions, persistence, URL/path validation |
| `crates/ss550-gui` | Window (GPUI Kit). Independent workspace |
| `crates/ss550-tray` | Tray icon, Win32 hide/restore. Only crate allowed unsafe Win32 |
| `crates/ss550-probe` | Reverse-engineering tools |
| `docs/` | Protocol docs, troubleshooting, milestones |
| `odd/tasks/` | Feature specs, tasks, implementation evidence |

## Contributing

1. Check existing work in `odd/tasks/`
2. Read the spec and implementation notes
3. Follow the conventional commit format
4. Ensure `cargo fmt --check` and `cargo clippy -- -D warnings` pass

## License

MIT OR Apache-2.0