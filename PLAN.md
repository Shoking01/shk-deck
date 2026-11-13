# PLAN.md — StreamCraft SS550 Native Rust Controller

**Status: 2026-11-11 — v1 works on the real deck and ships as a release build.
This document is what is left to do, not what was once intended.**

The controller connects to the SS550, keeps it awake, shows its 15 keys, and
runs one of two actions when a key is pressed. It lives in the notification
area, so it behaves like ordinary Stream Deck software: no window in the way,
the deck never sleeps, the keys never stop working. What is still missing is
the hardware-in-loop proof, the deck seam that keeps a second model from
becoming a rewrite, and the measured budgets.

`README.md` is the user-facing truth. This file is the engineering truth: the
decisions that are locked, the phases as they actually stand, and the order in
which the remaining work should be done.

---

## 1. What this is

**Goal.** A native, cross-platform desktop application in Rust that fully
controls the Redragon StreamCraft SS550 (15 LCD keys, 6 profiles), replacing
the vendor's *Stream Station*.

**Why.** The vendor software is outdated, Windows/macOS-only and closed-source.
A native app gives performance, safety and full control of the device.

**Hardware target.** Redragon StreamCraft SS550, USB `VID 0x0200 / PID 0x3000`
(v3), 15 LCD buttons, 6 profiles.

**Scope.** The SS550 is the only deck v1 supports on purpose: it is the one
that exists on the desk. The project is not meant to stay SS550-only, so no
layer may assume its key count, geometry, image size or capabilities. M3
builds that seam and proves it with a second implementation, M4 makes the
window follow it, and M9 exercises it on real hardware.

---

## 2. Decisions that are locked

These were decided with the code in front of us. Reopening one is a change of
direction, not a detail.

| Area | Decision | Why |
| --- | --- | --- |
| GUI | **GPUI Kit** (`gpui-kit` v0.9.0) | The window is a settings panel, not a game. The old egui example was a debug tool from the previous generation and was deleted (`ff37af8`). |
| Transport | **`hidapi`, synchronous** | The byte-exact path came first. Async (`nusb`/`tokio`) is deferred and must keep the same `ss550-protocol` bytes. |
| Deck seam | **`Deck` (semantics) above `Transport` (bytes)** | A second deck differs in key count, geometry, image size, capabilities and input decoding *before* it differs in transport. A transport-only seam gets recut at the first second device. |
| Deck seam evidence | **Two implementations minimum** | One implementation is a guess; two is the smallest set that can falsify a seam. The simulator is already planned (M3), so the second one is free. |
| Tray | **`tray-icon` 0.26 + Win32** | `tray-icon` is purpose-built for the notification area and builds its hidden window on its own thread, so the icon and the controller window never share a message loop. `native-windows-gui` would be a whole GUI toolkit for one icon. |
| Win32 calls | **Only in `crates/ss550-tray`** | Every other crate keeps `#![deny(unsafe_code)]`. The exception is declared in that crate's `Cargo.toml`, with the reason. |
| Action catalogue (v1) | **Exactly `Launch { program, args }` and `OpenUrl { url }`** | Every extra kind is a promise to keep. No macros, no key combos, no OBS, no Twitch. |
| Plugins | **None in v1** | A WASM sandbox only after v1 is stable and the safety net exists. |
| Workspace layout | **Three roots, three lock files** | The root suite stays fast and never pays for GPUI's ~800 packages or the Win32 tree. |
| Crate naming | **`ss550-*` means "the SS550 provider"** | Renaming seven crates across three workspace roots, the binary, `%APPDATA%\ss550` and the autostart entry buys nothing today. The seam crate is not named `ss550`, and a guard test says so. |
| Toolchain | Rust 2021, **MSRV 1.87+**, zero clippy warnings | `eframe` 0.37 needs Rust 1.95; `jpeg-encoder` 0.7, `thiserror` 2.0, `winreg` 0.56 and `windows` 0.62 all fit. |

---

## 3. Architecture as built

```text
protocol ──▶ driver ──▶ engine ──▶ gui (GPUI Kit)
                          │           │
                          └───────────┴──▶ tray (Win32)
```

| Crate | Owns | Rule |
| --- | --- | --- |
| `ss550-protocol` | Pure HID codec. Golden vectors, proptest. | No I/O, no clock, no USB. |
| `ss550-driver` | `hidapi` transport: connect, keepalive, poll, paint, clear, reconnect, sleep. | All USB access in the project lives here. |
| `ss550-engine` | `Action` (Launch / OpenUrl), validation, the JSON store, the action runner. | No UI, no USB. |
| `ss550-gui` | The window. Its own workspace root. | Owns the main thread; the controller entity outlives the window. |
| `ss550-tray` | Tray icon, `ShowWindow` hide/restore, the logon entry. Its own workspace root. | The only crate allowed to call Win32. |
| `ss550-probe` | The diagnostics used to reverse-engineer the deck. | Not shipped. |

Dependencies flow one way: `protocol → driver → engine → gui/tray`. No cycles.

**Runtime shape.** The window is a settings panel. Closing it hides it
(`ShowWindow(SW_HIDE)`, close veto, `QuitMode::Explicit`); the process, the HID
handle, the 50 ms poll timer and the heartbeat all keep running. The tray icon
is the only way back and the only way out. `START WITH WINDOWS` is off by
default and registers an installed copy — never a build folder.

---

## 4. Phase status

| Phase | Scope | Status |
| --- | --- | --- |
| 0 | Research & protocol analysis (REA, descriptors, image upload, input events, display specs) | **done** |
| 1 | Core driver (`ss550-protocol`, `ss550-driver`, `ss550-probe`) | **done**, with the deferred list in §5 |
| 2 | Application shell (window, tray, background mode, autostart) | **done** |
| 3 | Action engine (key press → action) | **done for v1** (`Launch`, `OpenUrl`) |
| 4 | Icon & asset pipeline | **partial** (solid colour in the UI, JPEG accepted by the driver; no editor, no packs) |
| 5 | Polish & distribution (release build, installer, updater, onboarding) | **partial** (release build installed and registered; no installer, updater or onboarding) |

### Phase 0 — Research & protocol analysis: done

`docs/protocol/` holds the wire format, HID descriptors, image upload, input
events, display specs, the profile path and the incident log. The byte math is
byte-exact against the `mirajazz` reference implementation, and every documented
command is covered by unit, doc and property tests.

### Phase 1 — Core driver: done

`ss550-protocol` (pure) + `ss550-driver` (sync `hidapi`) + `ss550-probe` are
live. Verified on the deck: connect, wake, paint, clear, the boot
re-enumeration, the 68.7 s idle reset, and the 500 ms heartbeat (throttled to
2 s) that keeps the device awake.

Deferred on purpose, not dropped: async transport, hotplug watcher, profile
push (`ICO`/`ULEND` is static-only), and the safety net of §5.

### Phase 2 — Application shell: done

The window is GPUI Kit and behaves like ordinary Stream Deck software: the X
button hides it, the tray brings it back, `Quit` ends the process. Every
control says what it does. The deck keeps working while the window is hidden.

### Phase 3 — Action engine: v1 done

The catalogue is exactly two actions:

- `Launch { program, args }` — `.exe` by path, `.lnk` through `explorer`,
  `.bat`/`.cmd` through `cmd /C call`; each argument is its own argv entry, so
  an argument can never turn into a second command.
- `OpenUrl { url }` — the scheme is required; a bare string is refused instead
  of guessed at.

Bindings live in `%APPDATA%\ss550\bindings.json` (or `$SS550_CONFIG_DIR`), a
broken file starts the app with no bindings and says why. A program that dies
is watched for 6 s and reported in the status line.

### Phase 4 — Icon & asset pipeline: partial

The driver takes solid colours and JPEG images (`ButtonImage::from_jpeg`). The
UI paints the two colours it offers and clears keys. What does not exist yet:
an icon browser, an editor, packs, import/export, and per-profile artwork.

### Phase 5 — Distribution: release build done, packaging pending

`cargo build --release` builds all three roots, and the GUI is a 24.5 MB GUI
subsystem binary that opens no console. The copy registered for autostart is
that release build, at `%LOCALAPPDATA%\Programs\ss550`. What does not exist
yet: an installer, an updater and a first-run wizard.

---

## 5. What is **not** done

Honest list. Items marked *out of scope for v1* are decisions, not debts.

- **The gate trusts the caller's label, not the bytes**: the driver checks the
  command the caller declares, never the pack it is about to send. No call
  site mislabels anything today, so no unproven byte is reachable; the
  hardening is for `Pack` to carry the command it belongs to.
- **The registry rows are on the author**: the compiler forces an entry for
  every command variant, but nothing forces a row in the registry or in the
  verified list.
- **The fuzz target does not run in CI**, and `fuzz/Cargo.lock` is not covered
  by the dependency audit. Both are decisions about CI, not about the
  protocol.
- **The second deck is a simulator, not hardware**: `deck-simulator` ships a
  six-key profile and the SS550 wire fake, so the seam is proven and the suite
  runs without a device — but no real second deck has been driven yet (M9).
- **The hardware-in-loop run is still open**: the `hardware-tests` feature
  exists and the test compiles, but it has never run against a deck on a
  dedicated test unit (M3's last open line).
- **The window follows the deck; the rest of the app still knows it is an
  SS550**: the grid, the image size, the offered controls and the texts come
  from the `Deck` in front of the window (M4), and bindings are stored per deck
  — but the provider that opens the deck is still the SS550 one, and a real
  second deck is M9.
- **No profile push**: the host does not manage named sets of key images; the
  `ICO`/`ULEND` path is static-only and unverified.
- **No hotplug watcher and no automatic reconnect**: unplugging the deck needs
  the `RECONNECT` button. The driver exposes `reconnect()`, `initialize()` and
  `repaint_all()`; the app does not yet chain them on its own.
- **No async transport**: `hidapi` is synchronous.
- **No Linux packaging**: no udev rules, no `.desktop`, no AppImage/deb.
- **No release build**: no installer, no updater, no first-run wizard.
- **Performance budgets are unmeasured** (§7).
- **Out of scope for v1**: OBS/Twitch plugins, a WASM sandbox, macros, key
  combos, text injection, multi-action sequences, an icon editor.

---

## 6. Next milestones, in order

Each one is small enough to close on its own and leaves the project in a
shippable state.

### M1 — Safety net for the protocol: done (2026-11-11)

The deck can be left backlit and black by a bad packet, and from that point no
host can paint it until it is fully power-cycled. Before any *new* command is
sent to the device:

1. `docs/protocol/verified.md`: one row per command — what it does, the golden
   vector, the trace it came from, and whether it was exercised on a test deck.
2. The driver refuses to send anything that is not on that list (safe mode).
3. `cargo fuzz` on the protocol decoder: panic-free on arbitrary input.

**Done when** the fuzz target runs clean and every command the driver can send
appears in `verified.md`.

**Status: done.** Merged into `main` as PR #10 — ten work-unit commits and
five native reviews, all approved. Ten commands on the list, four verified on
hardware and six not, each with its evidence or the reason it is pending. The
driver refuses an unproven command **before its bytes leave the host**, and
there is no switch that turns that off. A test fails when the document and the
registry disagree, so the list cannot drift away from the code. The fuzz
target ran 30,059 executions with no crash, and eight unit tests pin which
inputs it feeds.

Two commands changed behaviour, on purpose: `clear_key` and `sleep` are now
refused, because neither was ever exercised on a deck. The application calls
neither.

### M2 — A real release build: done (2026-11-11)

`cargo build --release` for the three roots, the GUI binary installed to
`%LOCALAPPDATA%\Programs\ss550`, the autostart entry pointing at that copy,
and the debug build out of the picture.

**Done when** the registered binary is the release build, it survives a
sign-in cycle, and the installed copy is no longer the 53.7 MB debug binary
that sits there today.

**Status: done.** Measured, not claimed:

- `cargo build --release` builds all three roots. The GUI binary is 24.5 MB
  against the 53.7 MB debug build, and its `OptionalHeader.Subsystem` is `2`
  (GUI) instead of `3` (CONSOLE), so a launch creates no console window and no
  `conhost` child.
- The installed copy at `%LOCALAPPDATA%\Programs\ss550\ss550-gui.exe` **is** the
  release build; the debug binary is out of the picture.
- `ss550-gui = "...\Programs\ss550\ss550-gui.exe" --hidden` is the only entry
  under `HKCU\...\Run` for this app; no startup-folder shortcut, no scheduled
  task. The old `StreamStation` entry is gone and Stream Station no longer
  starts.
- Survived a sign-in cycle: after signing back in, the process runs from the
  installed path with no visible window (`MainWindowHandle = 0`), and the log
  gained a `started` line for that session. The log shows two `started` lines
  and no `quit`: the session that ended was killed by the sign-out, which is
  exactly the shape the log documents as "did not exit through the tray".
- A release panic used to leave no trace at all, because the console it printed
  to was gone. `crates/ss550-gui/src/diagnostics.rs` now writes
  `%APPDATA%\ss550\ss550.log`: `started` per run, `quit` from the tray, and a
  panic's message and place before the process goes down. It opens from a hidden
  five-click trigger on the version line.

### M3 — Hardware-in-loop tests, a simulator, and the deck seam

The app is meant to drive more than one deck model, so the driver stops being
*the SS550 driver* and starts being *a* deck driver. The proof of that seam is
a second implementation, and the simulator is it: not SS550-over-memory, but a
profile with a different shape — fewer keys, a different image size, no
brightness.

**The seam.** `Deck` (semantics) sits above `Transport` (bytes). A second deck
differs in key count, geometry, image size, capabilities and input decoding
*before* it differs in transport, so a transport-only seam would be recut at
the first second device. The public handle stays non-generic (`Box<dyn Deck>`
behind it), so `ss550-gui` and `ss550-engine` do not change shape.

What `Deck` exposes: `key_count`, the row/column geometry, the image size and
payload limit, the capability set (brightness, clear, sleep, dials), the input
decoding, and **a typed error for every unsupported operation**. No silent
no-op — the same rule M1 put on the command gate.

**What is hardcoded today, and is a latent bug for a second deck:**

- `KeyNumber::key()` falls back to `BottomLeft` on an out-of-range index
  (`crates/ss550-engine/src/key.rs`). It is unreachable while the range is
  1..=15; with a 6-key deck it becomes reachable and paints the wrong key,
  silently.
- `Key::position()` computes the geometry as `index / 5` and `index % 5`
  (`crates/ss550-driver/src/keys.rs`), which splits the rows wherever a deck
  is not 5 wide. **Removed in M3**: the helper had no callers, and
  `deck_api::Geometry` owns a deck's grid now.
- The window asserts 15 keys, a 5-wide grid and 100×100 images in about ten
  places, including the strings `PAINT ALL 15 KEYS` and
  `Redragon StreamCraft · 15 LCD keys · USB 0200:3000`.

**The suite.** One file, parameterized by profile, so the same cases run
against the real deck and against the simulator. The `hardware-tests` feature
gates every test that touches hardware and each one is `#[ignore]`d; a plain
`cargo test` must never open the device.

**The guards.** Three, and the last one already exists but goes blind:

1. The seam crate does not contain the string `ss550`.
2. The number 15 does not appear outside the SS550 provider. **As M3 can
   enforce it**: the seam and the conformance crate contain no `15`, and the
   second profile's geometry differs from the SS550's. The engine's
   `KeyNumber` still carries `1..=15` and the window still says 15; both
   belong to M4.
3. The single-writer guard of M1 (`every_write_goes_through_the_gated_funnel`)
   counts `.write(` in `connection.rs` alone. Moving the transport out of that
   file would leave the guard blind while a second writer exists, so it is
   extended to the whole crate.

**Not promised.** A virtual HID device in CI. Windows has no such thing
without a signed kernel driver; Linux has `uhid`. The suite runs at the
transport seam, which proves the driver's logic, not Microsoft's HID stack.

**Done when** the conformance suite is green over two profiles — the SS550 and
one with a different key count and image size — the three guards are green,
and `cargo test --features hardware-tests -- --ignored` has run against a real
deck on a dedicated test unit.

**Blocked on hardware.** Everything above except the run against the real deck
needs no device. That last line needs the deck on a unit that is not the daily
driver.

**Status: done, except the hardware line.** The seam shipped as three crates:
`deck-api` holds the contract, `deck-conformance` holds one suite of ten
cases, and `deck-simulator` holds a six-key deck plus the SS550 wire fake. The
driver now sits on the seam as `Ss550Deck`, a `Deck` over a `Transport`, and
the suite is green over two profiles. The three guards are green: the seam
names no device, the seam assumes no key count, and the driver writes through
exactly one gated funnel — the gate did not move, and the crate-wide guard now
watches every source file instead of one.The engine no longer knows what a physical key is: the two conversions are gone
and so is its dependency on the driver.

The wire fake carries the real driver, which is what makes it a second
implementation rather than a copy of the tests: four cases in
`crates/deck-simulator/tests/ss550_deck_over_the_wire.rs` put `Ss550Deck` on
`Ss550WireFake` and paint a key, set brightness and clear it, decode a press,
and watch a forbidden command write nothing. The writer guard scans every
`.rs` file in the driver crate, `src/` and `tests/`, so the "whole crate" is
literal, and the second hardcoded geometry — `Key::position()` — is gone. The
audit, what it found missing and how each piece was closed are recorded in
`docs/milestones/M3-deck-seam.md`.

Two lines of the "done when" stay open, and both are honest:

1. `cargo test --features hardware-tests -- --ignored` has never run against a
   real deck on a dedicated test unit. The feature is off by default and every
   hardware test is `#[ignore]`d, so a plain `cargo test` never opens the
   device.
2. The suite runs at the transport seam. It proves the driver's logic, not
   Microsoft's HID stack — a virtual HID device in CI is not promised.

### M4 — A deck-agnostic window

The window stops asserting what deck it is talking to. The grid comes from the
profile's geometry, the image size from the profile, and a capability the deck
does not have is not offered — and if something asks for it anyway, it fails
with the typed error, never with a silent no-op. Bindings are stored per
deck. The texts name the deck in front of them.

This needs no hardware: the M3 simulator profile is a 6-key deck with a
different image size and no brightness, and the window is exercised against
it.

**Done when** the same window drives the 6-key profile with no SS550 literal
left in `crates/ss550-gui`, and an operation that profile does not support
returns its typed error.

**Status: done** (2026-11-12, `feature/deck-seam`). See
`docs/milestones/M4-deck-agnostic-window.md`. The window holds a `DeckHandle`
behind a `DeckSession`; the grid, the image size, the controls and the texts
come from the deck, bindings live in one file per deck, and a guard scans
`crates/ss550-gui/src` for the device name, the product name, the USB id and a
standalone key-count literal. The keep-alive became a seam operation, so the
conformance suite grew an eleventh case.

### M5 — Measure the performance budgets

`hyperfine` for startup, a press→action latency probe, Task Manager/`ps` for
RAM and CPU. Publish the numbers next to the targets.

**Done when** §7 has a measured column, not a target column.

### M6 — Profiles

Host-side profile management: named sets of bindings and images, switching
them on the deck, showing the active one in the window. The new commands only
go out after M1 puts them on the verified list.

**Done when** six named profiles exist and survive a restart.

### M7 — Hotplug and auto-reconnect

Watch for connect/disconnect, show the state, repaint on reconnect, and back
off exponentially so a missing deck is never spammed.

**Done when** unplugging and replugging the deck recovers by itself.

### M8 — Linux packaging

`99-streamcraft-ss550.rules`, a `.desktop` entry, an AppImage or deb.

### M9 — A real second deck

The seam is proven against a simulator; this closes it against hardware. The
model is chosen by evidence, not by taste: the SS550 already speaks an
OpenDeck-style numbering (`open_deck_to_firmware` in
`crates/ss550-driver/src/keys.rs`), so a serial OpenDeck board is the cheaper
second family than an Elgato, whose HID feature reports and 72×72 images are a
different protocol entirely.

The new model arrives as a new provider crate. The application must drive it
without a single change in the seam, the engine or the window; if it needs
one, the seam was wrong and M3 was not finished.

**Gate:** a real second deck in hand. This milestone waits for hardware, not
for the milestones above it — it starts the day a second deck is on the desk.

**Done when** a second physical deck is driven by the same binary, with its
own key count, image size and capabilities, and the app needed no change
outside its provider crate.

### M10 — Plugins (only after v1 is stable)

A WASM sandbox first, native `libloading` second. Nothing before M1–M5 are
closed.

---

## 7. Success metrics

Targets come from `AGENTS.md` §10 and §12.

| Metric | Target | Current |
| --- | --- | --- |
| CI on Windows, Linux, macOS | green | **green** (run `42763968848`, six jobs) |
| Clippy | zero warnings, `-D warnings` | **green**, including the Linux and macOS targets |
| Protocol coverage | every documented command | **green** (unit, doc, property tests) |
| Cold startup | <500 ms | not measured |
| Warm startup | <100 ms | not measured |
| Button press → action | <50 ms | not measured (the deck itself reports in 150–270 ms) |
| RAM at idle | <50 MB | not a v1 gate (declared out of scope) |
| CPU at idle | <1 % | not measured |
| Profile switch render | <200 ms for 15 keys | not measured |
| 48 h stress run | zero crashes | not run |

---

## 8. Safety rules (non-negotiable)

1. **Never send an untested raw command to the device.** A command is
   documented in `docs/protocol/`, pinned by a golden vector, and tried on a
   **test deck**, never on the daily driver.
2. **When the deck goes backlit but black**, follow
   `docs/troubleshooting/deck-stuck.md`. A 5-second replug is not enough;
   60 seconds is.
3. **Log USB traffic** in debug builds (`RUST_LOG=usb=trace`).
4. **Back off exponentially** on an unexpected disconnect. Never spam reconnects.
5. **Never commit REA session logs** that contain serial numbers.

---

## 9. Where the truth lives

| Document | What it answers |
| --- | --- |
| `README.md` | What works today, and how to build and run it |
| `CHANGELOG.md` | What changed, with the evidence; known limitations at the end |
| `crates/ss550-gui/README.md` | Every control in the window, and the background mode |
| `docs/protocol/` | The wire format, the evidence behind it, the incident log |
| `docs/troubleshooting/deck-stuck.md` | The recovery runbook for a stuck deck |
| `odd/tasks/` | Feature records: the specs, the tasks, the proof behind every claim |
| `AGENTS.md` | The rules this project is built under |
