# streamcraft-rs

[![CI](https://github.com/Shoking01/shk-deck/actions/workflows/ci.yml/badge.svg)](https://github.com/Shoking01/shk-deck/actions/workflows/ci.yml)

A native controller for the **Redragon StreamCraft SS550** (15 LCD keys,
`VID 0x0200 / PID 0x3000`). It replaces the vendor's *Stream Station*: no
Electron, no WebView2, no WebSocket plugin host, no vendor service. Rust talks
to the deck over HID directly.

```
protocol ──▶ driver ──▶ engine ──▶ gui (GPUI Kit)
                          │           │
                          └───────────┴──▶ tray (Win32)
```

## What works today — verified on hardware

| Area | State |
| --- | --- |
| Connect, wake, paint keys, clear | done, incl. the deck's boot re-enumeration and the 68.7 s idle reset |
| Keep the deck awake | 500 ms heartbeat (`keepalive`, throttled to 2 s) |
| Read key presses | both edges reported, host-side debounce policy |
| Bind a key to an action | v1 catalogue is exactly **App** and **URL** |
| Run the action | `.exe` by path, `.lnk` via `explorer`, `.bat`/`.cmd` via `cmd /C call`, URLs by scheme |
| Background mode | tray icon, X hides the window, deck stays awake, keys keep working |
| Start with Windows | opt-in, one `HKCU\...\Run` value, off by default |
| Report a program that dies | watched for 6 s, failure shown in the status line |
| Safe mode | the driver refuses to send any command a test deck has not proven |
| Profiles (M6) | 6 named profiles (default, stream, meetings, gaming, work, entertainment) with JSON persistence |
| Hotplug (M7) | Device detection via polling, automatic reconnect with exponential backoff (2s base, doubling, 60s cap) |

Every launch route above was exercised on the real deck. What is *not* done is
listed in [`CHANGELOG.md`](CHANGELOG.md#known-limitations--fase-1-close-out):
> plugin sandbox, async transport and the udev rules.

The commands the deck has actually proven live are listed in
[`docs/protocol/verified.md`](docs/protocol/verified.md), and the driver
enforces that list: a command nobody has exercised on a deck is refused
before its bytes leave the host.

## Layout

| Path | What it is |
| --- | --- |
| `crates/ss550-protocol` | Pure HID codec. No I/O, no clock, no USB. Golden vectors + proptest |
| `crates/deck-api` | The deck seam: `Deck` (semantics) over `Transport` (bytes). Names no device, assumes no key count |
| `crates/deck-conformance` | One suite of eleven cases every deck implementation must pass |
| `crates/deck-simulator` | A six-key deck and the SS550 wire fake, so the seam is proven without hardware |
| `crates/ss550-driver` | Synchronous `hidapi` transport, caching, reconnect, heartbeat; the SS550 as a `Deck` |
| `crates/ss550-engine` | Bindings, persistence, URL/path validation, action runner |
| `crates/ss550-gui` | The window (GPUI Kit). Its own workspace root |
| `crates/ss550-tray` | Tray icon, `ShowWindow` hide/restore, logon entry. Its own workspace root, and the **only** crate allowed to call Win32 |
| `crates/ss550-probe` | Diagnostics used to reverse-engineer the deck |
| `docs/protocol/` | Wire format, descriptors, input events, image upload, incidents |
| `docs/troubleshooting/` | The "backlit but black" runbook — read it before touching USB code |
| `docs/milestones/` | What each milestone delivered, what it left open, and the evidence |
| `odd/tasks/` | Feature records: specs, tasks, and the evidence behind every claim |

## Build and test

Requires Rust **1.87+** and, for the GUI, a GPU-capable Windows machine.

```bash
# protocol / driver / engine / probe
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# the window (pulls the GPUI tree, several minutes the first time)
cargo test --workspace --manifest-path crates/ss550-gui/Cargo.toml
cargo clippy --manifest-path crates/ss550-gui/Cargo.toml --all-targets -- -D warnings

# the tray (Win32 + tray-icon)
cargo test --manifest-path crates/ss550-tray/Cargo.toml
cargo clippy --manifest-path crates/ss550-tray/Cargo.toml --all-targets -- -D warnings
```

The three roots are separate workspaces on purpose: the driver/protocol suite
stays fast and never pays for GPUI's ~800 packages or the Win32 tree.

The protocol decoder is fuzzed from a fourth, separate workspace root, so it
never enters the shipped lock files:

```bash
cargo fuzz run decode_input_report -- -max_total_time=60
```

It needs a nightly toolchain and, on Windows, the Visual Studio sanitizer
runtime on `PATH`. [`fuzz/README.md`](fuzz/README.md) has the details.

```bash
cargo run --manifest-path crates/ss550-gui/Cargo.toml
cargo run --manifest-path crates/ss550-gui/Cargo.toml -- --hidden   # tray only
```

## Background mode

The window is a settings panel, not the app. Closing it hides it; the process,
the HID handle, the poll timer and the heartbeat all keep running, and the
tray icon is the only way back and the only way out. `START WITH WINDOWS` copies
the controller to `%LOCALAPPDATA%\Programs\ss550` and registers that copy — never
a build folder that the next `cargo clean` deletes — then removes the
registration again when switched off. Details in
[`crates/ss550-gui/README.md`](crates/ss550-gui/README.md#background-mode).

## Hardware safety

Bad packets can leave the deck backlit but black, and from that point no host
can paint it until it is fully power-cycled. Before sending anything new:

1. Document the command in `docs/protocol/`.
2. Pin it with a golden vector.
3. Try it on a **test deck**, never the daily driver.

If it already happened, follow
[`docs/troubleshooting/deck-stuck.md`](docs/troubleshooting/deck-stuck.md):
a 5-second replug is not enough, 60 seconds is.

## Previous generation

This repository used to hold an earlier architecture (Electron host, WebSocket
SDK transport, an `egui` app under `v2/`). It is preserved, unchanged, in the
tag **`archive/reard-v2`** and in this branch's history; the tree now describes
only the Rust/HID controller.

## License

MIT OR Apache-2.0.
