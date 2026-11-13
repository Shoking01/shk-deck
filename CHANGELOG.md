# Changelog — streamcraft-rs

All notable changes are documented here. This project follows
[Conventional Commits](https://www.conventionalcommits.org/) for commit
messages; the file below is a human summary, not a substitute for
`git log` once the workspace is under version control.

## Unreleased

### Changed — the window follows whatever deck answers
- `crates/ss550-gui` no longer asserts which deck it is talking to. The grid
  comes from `Deck::geometry`, each key image is encoded at `Deck::image_spec`'s
  own width and height, and every deck-wide control exists only because
  `Deck::capabilities` says the deck has it. A deck with no brightness never
  sees a brightness row; a deck with no single-key clear never sees that
  button, and a call that asks anyway comes back as `DeckError::Unsupported`
  and is reported, never swallowed.
- The window holds a `deck_api::DeckHandle` behind a `DeckSession` that reads
  the deck once per connection. Nothing below it carries a key count of its own:
  the tile grid, the key picker, the row labels and the "painted N keys" notes
  all come from the deck's geometry, in display order (top row first), because
  the hardware counts its rows from the bottom.
- The texts name the deck in front of them: the header, the window title and the
  tray tooltip use the connected deck's identity, or its shape (keys, columns,
  image size, dials) when it does not report one. Before a deck answers the
  window says so instead of describing a model.
- The keep-alive is a seam operation now (`Deck::keep_alive`,
  `Capabilities::keep_alive`, `DeckOperation::KeepAlive`), not a window timer:
  the trait's default is the typed refusal, so a deck that never resets says so.
  The conformance suite gained an eleventh case,
  `keep_alive_without_the_capability_is_refused`, with its falsifying stub.
- Bindings are stored per deck, in `config_dir()/bindings/<slug>-<fnv64>.json`,
  keyed by the deck's identity or by its shape when it has none. The pre-M4
  `bindings.json` is still read, and copied into the per-deck file only when
  every bound key exists on the deck in front of the window; a file that would
  need keys the deck does not have is left alone rather than repointed at other
  keys.
- `ss550-engine::KeyNumber` no longer carries a `1..=15` range. A key number is
  a positive 1-based number, and the deck's geometry decides whether the key
  exists. A window guard scans `crates/ss550-gui/src` for the device name, the
  product name, the USB id and a standalone key-count literal, so the window
  cannot quietly start naming a deck again.

### Added — a seam that lets a second deck drive the same app
- `crates/deck-api` is the contract every deck answers to: `Deck` (what a deck
  can do) above `Transport` (how its bytes travel). It is a leaf crate — its
  only dependency is `thiserror` — and it names no device and assumes no key
  count, with two guards that fail the build if either creeps in.
- An operation a deck cannot do returns a typed error (`DeckError::Unsupported`)
  and never a silent no-op: a caller that cannot tell "done" from "not
  possible" has no way to tell the user anything useful. A brightness outside
  the accepted range is refused, not clamped.
- `crates/ss550-driver` now sits on the seam as `Ss550Deck`, a `Deck` over a
  `Transport`. The bytes still leave through one gated funnel after the
  verified-command check — the gate did not move — and the crate-wide guard now
  watches every `.rs` file in the crate, `src/` and `tests/`, instead of one.
- `crates/deck-conformance` holds one suite of ten cases, run against any
  `Deck`. No case paints a key, so the same suite is safe on a desk. Each case
  is paired with a stub that breaks exactly one rule, and the pairing is
  checked in both directions: the flaw fails that case and only that case, and
  every case has a flaw that breaks it.
- `crates/deck-simulator` holds the second profile: a six-key deck, two rows of
  three, 72×72 images, no brightness — plus the SS550 wire fake, which decodes
  real packs so the driver runs in CI without a device. Four tests in
  `tests/ss550_deck_over_the_wire.rs` put `Ss550Deck` on that fake and paint a
  key, set brightness and clear it, decode a press, and watch a command the
  registry forbids write nothing, so the claim is exercised rather than
  asserted.
- `crates/ss550-engine` no longer knows what a physical key is.
  `KeyNumber::key()` and `KeyNumber::from_key()` are gone, and with them the
  engine's dependency on the driver. The conversions carried a silent
  fallback: a number that could not be converted became the bottom-left key
  instead of an error. A guard fails if the driver, the fallback or either
  conversion comes back.

### Removed — a key geometry helper with no callers
- `Key::position()` computed `(index / 5, index % 5)`, which is the SS550's
  five-wide grid written as a formula. The grid belongs to `deck_api::Geometry`
  now, the helper had no callers, and leaving it would have been a trap for the
  first deck that is not five keys wide. It is gone rather than corrected.

### Fixed — the release build opened a console window
- The GUI binary was linked as a console subsystem. Windows creates a console
  window for such an executable no matter how it is started — double-click, the
  autostart entry, or a terminal — and a tray application must not have one.
  `crates/ss550-gui/src/main.rs` now carries
  `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`.
- The attribute is release-only on purpose: debug builds keep the console, so
  `cargo run` still shows output and panics while developing.
- Measured on the release artifact: `OptionalHeader.Subsystem` went from `3`
  (CONSOLE) to `2` (GUI), and launching the installed copy creates no `conhost`
  window. A test pins the attribute and cuts the test module off before reading
  the source, so it cannot pass by matching its own literal.
- Without the console, a release-mode panic would have no visible trace. The
  diagnostic log below is what replaced it: a panic now writes itself to a file
  before the process goes down.

### Added — the controller keeps a diagnostic log
- `crates/ss550-gui/src/diagnostics.rs` appends to
  `%APPDATA%\ss550\ss550.log`: one `started` line per run, one `quit` line when
  the tray's Quit is used, and the message and location of a panic, written by a
  hook before the process goes down. A `started` line with no matching `quit`
  means the process did not leave through the tray's Quit — an unexpected exit,
  a logoff, or an "End task". It narrows the search; it does not prove a crash.
- Control characters in a message are escaped, so a panic payload cannot forge a
  second line, including a fake `quit`. Each line is built in full and written in
  one call, so a panic on a worker thread cannot interleave with a line already
  being written.
- The log rotates at 1 MB into `ss550.log.old`, keeping one previous file. The
  cap is soft: when a viewer or an indexer holds the file open, Windows refuses
  the rename and the log keeps growing until the lock clears, then the next run
  rotates it. Truncating instead would delete the only crash evidence a release
  build produces, so it is not done. The log sits next to `bindings.json`, so
  `SS550_CONFIG_DIR` moves both.
- Nothing in the log path can fail the app: an unwritable log is an unwritable
  log, and the controller keeps running.
- Opening it is deliberately quiet: five clicks on the version line at the
  bottom of `STEP 4 · RUN IN THE BACKGROUND` open the file in Notepad, and the
  log records that the viewer started rather than that it displayed the file.
  Notepad is named explicitly — a clean Windows has no `.log` association, so
  `start` would do nothing — and launched as `%SystemRoot%\System32\notepad.exe`,
  so the lookup cannot pick up a planted `notepad.exe` from the current directory.
  That guarantee holds whenever `SystemRoot` is set; the stripped-environment
  fallback to the bare name gives it up, and is documented as such.
- Seventeen tests cover the timestamp conversion including a leap day, line
  framing and every escaping shape, all three panic payload shapes, rotation at,
  below and exactly on the bound, a log that cannot rotate keeping its evidence,
  the small-file case, both Notepad branches, and the five-click boundary.
  Verified live: `2026-11-11T16:42:59Z started v0.1.0 (hidden=true)` in the real
  path, and Notepad opened the file from the absolute path.

### Added — the registry of commands a deck has proven
- `docs/protocol/verified.md` is the safe-mode list: one row per command the
  protocol crate can encode, with the tag on the wire, whether a test deck has
  proven it, and where the evidence lives. Ten commands, four verified, six
  not.
- `ss550-protocol` gained a registry (`Command`, `Verification`, `CommandInfo`,
  `COMMANDS`) that carries exactly the text the document prints. The protocol
  crate stays pure: it owns the registry, the driver owns the enforcement.
- A test compares the document with the registry, so the list cannot drift
  away from the code: flipping a status in the document fails the build.

### Added — the driver refuses to send commands nobody has proven
- The driver's only write path now checks the registry **before** the bytes
  leave the host. An unproven command is refused with the command's name and
  the reason, and there is no flag, no environment variable and no setter that
  turns the gate off.
- What it changes today: `clear_key` and `sleep` are refused, because neither
  was ever exercised on a deck. The app calls neither. `clear_all`, the image
  path, the brightness and the heartbeat are untouched: all four are verified.
- Two bugs the gate exposed. `clear_key` used to drop the key from the image
  cache *before* writing, so a refused clear would have left the driver
  claiming a key was blank while the deck still showed it. And `sleep` woke
  the deck with a `DIS` before the sleep pack, so a refused sleep would have
  written to the deck on its way out. Both are fixed.
- The image cache now changes only after both clear packs are accepted, in
  both clear paths. Before this change, `clear_all` had the same ordering
  hazard as `clear_key`: a failed write left the driver believing every key
  was blank while the deck still showed the old images, and `repaint_all`
  skips keys the cache has forgotten, so nothing would have repainted them.
  Now a failed write leaves the deck's state unknown, so the driver keeps
  the images: they are what the user had, and a later repaint can restore
  them. The cost is that a deck that cleared anyway gets its images back on
  the next repaint.
- `firmware_version()` keeps reading the static identity report instead of
  asking the firmware: `VER` is unverified precisely because writing it halts
  the USB endpoint.

### Added — a fuzz target for the protocol decoder
- `fuzz/` is its own workspace root with a `cargo fuzz` target that feeds
  arbitrary bytes to the decoder: raw bytes, every prefix of them, the same
  bytes inside a valid `ACK`/`OK` envelope (at every length just above the
  minimum, and at a bounded sample of the rest), and an oversized buffer. It
  asserts nothing about the result — a malformed report may be rejected; it
  may not panic.
- Which lengths the sweep visits is decided in `fuzz/src/lib.rs`, where
  ordinary unit tests pin it: the minimum and the fifteen lengths above it
  are always visited, the rest of the range is sampled at most 64 times per
  input, and no length is ever below the minimum.
  `cargo test --manifest-path fuzz/Cargo.toml --lib` runs them; the fuzz
  binary is excluded from the test build, so the command needs neither
  nightly nor the sanitizer runtime.
- Verified on nightly with AddressSanitizer. The edge counter rose with
  every harness change — `cov: 75` before the enveloped sweep existed,
  `cov: 94` with it, `cov: 115` with the lengths just above the minimum
  always visited — and no run found a crash, a timeout or an out-of-memory.
  The throughput that bought those edges fell from 2,332 executions per
  second to 653, which is what the extra shapes cost: every input now pays
  for the whole band and for the list of lengths. The session recorded for
  the milestone: 30,059 executions in 46 seconds at 653 per second, peak
  RSS 509 MB.
- How to run it is in `fuzz/README.md`. On Windows the sanitizer runtime has
  to be on `PATH`, and `--sanitizer none` cannot link there: cargo-fuzz always
  applies sanitizer-coverage instrumentation whose section symbols do not
  exist in the COFF format.
- The enveloped reports are the ones that catch an off-by-one at the state
  byte: the prefix sweep cuts random bytes, which do not spell `ACK`, and the
  framed input was always full length, so the short-but-enveloped report was
  never exercised before this.

### Documentation — `PLAN.md` describes the project that exists
- The old `PLAN.md` planned a different application: egui as the primary GUI
  with GPUI as a fallback, a `ss550-ui` / `action-engine` / `plugins/` layout,
  Phase 2 checkboxes for an egui shell that was never built, and a risk
  register written before any of it existed. It now describes the project as
  it is: the decisions that are locked (GPUI Kit, synchronous `hidapi`,
  `tray-icon`, a two-action v1 catalogue, three workspace roots), the real
  crate layout, the status of every phase, an honest list of what is **not**
  done, and the eight remaining milestones in order.
- The plan states plainly that the copy registered for autostart is still the
  debug build, which is the first thing the release milestone fixes.
- `.engram/config.json` pins this directory to the Engram project that already
  holds the repository's history (`rea rd`). Without it, Engram resolved the
  project from the git remote (`shk-deck`) and the two names disagreed.

### Fixed — "start with Windows" pointed at a build folder
- The switch registered `std::env::current_exe()`, so turning it on from a
  development build wrote
  `"…\crates\ss550-gui\target\debug\ss550-gui.exe" --hidden` into
  `HKCU\...\Run`. The next `cargo clean`, or moving the folder, broke the logon
  entry permanently while the switch kept showing **on** and the status line
  kept saying the controller would start with Windows. A promise the app could
  not keep, failing silently.
- Turning the switch on now copies the running executable to
  `%LOCALAPPDATA%\Programs\ss550\ss550-gui.exe` (new `ss550_tray::install`) and
  registers that path. No administrator rights, no service; deleting the folder
  is a complete uninstall. Turning the switch off removes the registration and
  leaves the copy alone, because toggling a setting is not uninstalling.
- `autostart::sign_in()` answers what Windows would *actually* start:
  `Off`, `On(path)`, or `Broken { command, executable }`. `is_enabled()` only
  says whether a value exists, which is why the old switch could lie.
- The window reads that state at startup: an entry naming a file that is gone
  shows as **off**, with a note naming the missing path and telling the user to
  turn the switch on again to point it at this copy. A failure while installing
  also leaves the switch off instead of claiming success.
- `autostart::command_line_executable()` reads the executable out of the command
  line the app writes, quoted or not, with tests for the quoted path, the
  unquoted one, an empty value and an unterminated quote.
- Verified live: the switch produced
  `"...\AppData\Local\Programs\ss550\ss550-gui.exe" --hidden`, the copied
  executable starts and responds, and the ignored
  `installed_autostart_round_trip` test covers install → register → read back →
  detect a missing executable → remove → restore, cleaning both the registry and
  `LOCALAPPDATA` afterwards.

### Repository — the workspace is under version control
- The controller is now `main` of the private repository
  [`Shoking01/shk-deck`](https://github.com/Shoking01/shk-deck). The repository
  previously held the older generation — Electron host, WebSocket SDK
  transport, the `egui` app under `v2/`, probes — and shares no file with this
  tree.
- That generation is preserved unchanged in the tag **`archive/reard-v2`** and
  in the branch history; the imported tree contains only the Rust/HID
  controller, so `README.md` and the layout now describe what the project
  actually is.
- `.gitignore` covers the three workspace roots' `target/` directories: about
  **9.5 GB** of build output in the working copy (3 GB root + 6.2 GB
  `crates/ss550-gui` + 262 MB `crates/ss550-tray`) stays out of the repository.
  85 source files and **875 KB** are tracked.
- `crates/ss550-driver` and `crates/ss550-protocol` each carried an accidental
  `.git` directory: zero commits, no remotes, an unborn `master`. They were
  moved out of the tree (not deleted) so the workspace is one repository
  instead of three.
- Added a root `README.md`: what the app does, what is verified on hardware,
  the crate layout, the three test/clippy commands (one per workspace root) and
  the hardware-safety rules.

### Dependencies — the bumps Dependabot wanted, applied one at a time
- Every pull request Dependabot opened was applied by hand instead, one
  commit per dependency, because they arrived in three overlapping pairs
  (`winreg`, `windows`, `thiserror`) that contradicted each other, and all of
  them were built against a `main` that had since changed.
- `jpeg-encoder` 0.6 → 0.7, `thiserror` 1.0.69 → 2.0.21, `winreg` 0.55 → 0.56
  and `windows` 0.61 → 0.62. Each one was verified before it was committed:
  the root workspace's tests and clippy, the tray's tests and clippy for the
  Windows, Linux and macOS targets. The error messages the user reads do not
  change, because every format string stays the same.
- **Both lock files move with every bump.** The workspace has three roots and
  three lock files, and `ss550-gui` depends on the driver, the engine and the
  tray by path, so its lock records their dependencies too. Dependabot's pull
  requests only ever touched one lock file each; merging one of them alone
  would have left the GUI job failing its `--locked` check.
- `actions/checkout` 4 → 7 in the workflow. That removes the Node.js 20
  deprecation warning every run printed; the other actions are already on
  their newest major version.

### Removed — the legacy egui example
- Deleted `crates/ss550-driver/examples/gui.rs` and the `eframe` dev-dependency
  that only it used. The window is GPUI Kit; the egui example was a debug tool
  from the generation before it, and the driver no longer needs a second UI
  stack.
- The root `Cargo.lock` loses the whole egui/winit tree with it (about 800
  lines), and `cargo test --workspace` no longer compiles a second GUI stack to
  run the deck's unit tests.
- `AGENTS.md` §6 said "GUI Development (egui) — use `eframe` with `glow`
  backend", which had stopped being true when the window moved to GPUI Kit. It
  says so now.
- This also settles Dependabot's `eframe 0.29 → 0.37` pull request, which
  could not have been merged: eframe 0.37 replaced `eframe::App::update` with
  `eframe::App::ui`, so the example no longer compiled, and eframe 0.37
  requires Rust 1.95 against the project's declared MSRV of 1.87.

### Fixed — a key tile's label depended on the host OS
- `Action::summary()` took the program name with `Path::file_name`, which
  splits on the host's separator only. The Windows build showed
  `LAUNCH obs64.exe` for `C:\Program Files\obs-studio\bin\64bit\obs64.exe`; the
  same binding, read on Linux or macOS, showed the whole path crammed onto the
  tile. The label is display data and now splits on both `\` and `/` in the
  engine, so a stored binding reads the same on every machine. Found by CI on
  its second run: `action::tests::the_summary_fits_on_a_key_tile` failed on
  Ubuntu and macOS and passed on Windows.
- The same label is used in the "… exited with code 1" message, so the action
  runner now shares that function instead of repeating the `file_name()` call.
- `runner::exit_code_means_something` — the one program whose exit code is not
  a failure, Explorer — read its argument only inside a `#[cfg(windows)]`
  block, so the Linux and macOS builds reported an unused variable and CI's
  `-D warnings` turned it into a build error. The Windows and non-Windows
  answers are now separate functions, and the non-Windows one says in its
  comment why it ignores the argument.

### Repository — every push is built, linted and audited
- Added `.github/workflows/ci.yml`: `cargo fmt --check` across the three
  workspace roots, a `windows-latest` / `ubuntu-latest` / `macos-latest` matrix
  running the protocol/driver/engine/probe suite with `clippy -D warnings`, the
  window built and linted on Windows, and `cargo audit` over all three
  `Cargo.lock` files.
- The tray is built and tested on all three platforms. Its real behaviour is
  Win32, and that is what the Windows job exercises; `tray-icon`, `winreg` and
  `windows` are now target-gated in `crates/ss550-tray/Cargo.toml`, so the
  Linux and macOS jobs build the crate's `TrayError::Unsupported` stubs without
  pulling a GTK tree, and those stubs keep compiling instead of rotting.
- Fixed: `winreg` was declared unconditionally, and the crate refuses to build
  anywhere but Windows (`compile_error!`), so the macOS job died before it
  reached a single test. Declaring a Windows-only dependency for every platform
  was the bug; `tray-icon` had the same problem with a GTK price tag on Linux,
  where only the stub is used.
- Two `install` unit tests read `LOCALAPPDATA` and therefore only mean anything
  on Windows; they are `#[cfg(windows)]` now, and the copy tests — which run
  anywhere — are not.
- Fixed: `TrayMessage::SetTooltip` carries a tooltip that only the Windows
  icon can show, so the Linux and macOS builds reported a never-read field and
  `-D warnings` turned it into a build error. The variant now says why the text
  has no reader there.
- Added `.github/dependabot.yml` for the three lock files and the actions
  themselves.
- `ss550-driver` gained `#![deny(unsafe_code)]`. It calls no `unsafe` — `hidapi`
  keeps the FFI inside its own crate — so it was a gap against `AGENTS.md` §2,
  not a working allowance.
- Fixed: `crates/ss550-probe/examples/raw_write.rs` called `CreateFileW`,
  `WriteFile` and friends unconditionally, so examples — which
  `cargo test --workspace` builds — failed to link on Linux and macOS with
  `undefined symbol: CreateEventW`. The Win32 body now lives under
  `#[cfg(windows)]` and other platforms get a `main` that says the probe only
  runs on Windows. The timeout it measures belongs to `hidapi`'s Windows
  backend, so there is nothing to port.

### GUI — background mode: tray, hidden window, start with Windows
- Added `crates/ss550-tray`, the only crate in the project allowed to call
  Win32, with the lint exception declared and explained in its `Cargo.toml`.
  Every other crate keeps `#![deny(unsafe_code)]`. It is its own workspace
  root, like the GUI crate, so its Win32 tree never slows the driver/protocol
  workspace.
- **The window's X button now hides the window instead of quitting.** The
  `Controller` entity owns the HID device, the 50 ms poll timer and the action
  runner, so it must outlive the window: hiding keeps the deck awake and the
  bound keys working, and the app stays alive with `QuitMode::Explicit`.
- Hiding is `ShowWindow(hwnd, SW_HIDE)` on the native handle. GPUI has no hide
  API (`App::hide()` is empty on Windows, `minimize_window()` leaves a taskbar
  button), and the handle comes from `HasWindowHandle::window_handle` — the
  inherent `window_handle()` shadows the trait method and returns an
  `AnyWindowHandle` instead.
- A notification-area icon with exactly two entries, `Show controller` and
  `Quit`, and a tooltip naming the app. It runs on a thread of its own with its
  own message loop, because `tray-icon` builds a hidden window on the thread
  that creates it and only `WM_QUIT` stops that loop.
- `Show controller` restores and focuses the window from anywhere;
  `Quit` stops the tray and the process. It is the only way to leave.
- New `STEP 4 · RUN IN THE BACKGROUND` panel: says where the tray icon is (or
  why there is none) and holds `START WITH WINDOWS`, off by default. Turning it
  on writes the value `ss550-gui` under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` for the current user
  only, and removes it again when turned off. No installer, no admin rights, no
  service.
- `--hidden` keeps the window off the screen at start, so the logon entry opens
  the controller straight into the tray. The window is created and then hidden,
  because a Win32 window cannot be created invisible through this path without
  a registered `WNDCLASSW.cbSize` and a zero-`HRESULT` `CreateWindowExW`
  failure.
- `tray-icon` 0.26 replaces the `native-windows-gui` named in `AGENTS.md`: NWG
  is a whole GUI toolkit for one icon, and GPUI owns the main thread. `AGENTS.md`
  is amended with the reason, the same way it was amended for `hidapi`.
- Verified live: `WM_CLOSE` leaves the process alive with
  `IsWindowVisible == false`; the process owns a `tray_icon_app` window, so the
  icon registered; the icon is visible in the notification area; `Show
  controller` brings the window back; `Quit` ends the process with no leftover
  icon. `--hidden` opens the window titled `StreamCraft SS550 Controller` with
  `visible=False` and the process alive.
- Verified live: the autostart round trip writes the entry, reads it back,
  removes it, and tolerates a second removal. It is covered by an ignored
  integration test (`cargo test -p ss550-tray -- --ignored autostart`) that
  writes to the real `HKCU` key and puts back a pre-existing entry.

### New engine crate — `ss550-engine` (key presses become actions)
- Added `crates/ss550-engine`, the `engine` layer of
  `protocol → driver → engine → ui`. It binds the deck's 15 keys to actions,
  persists those bindings and runs them. It lives in the root workspace, so
  the root test suite stays fast and free of the GPUI tree.
- The v1 action catalogue is exactly **App + URL**: `Action::Launch { program,
  args }` and `Action::OpenUrl { url }`. OBS scenes, keyboard macros and
  profile switching stay out of scope.
- `KeyNumber` addresses a key the way the UI shows it (**1 to 15**, 1 =
  bottom-left), which is also the firmware key number. Values outside the deck
  are rejected; `Default` is key 1 rather than the invalid derived `0`.
- `ActionStore` saves to JSON at `%APPDATA%\ss550\bindings.json`
  (`$XDG_CONFIG_HOME/ss550` elsewhere, `SS550_CONFIG_DIR` overrides).
  A missing file means "nothing bound yet"; a corrupt or newer-versioned file
  is reported and the app opens with no bindings.
- Actions run **without a shell**: every argument stays its own argv entry. On
  Windows `.bat`/`.cmd` route through `cmd /C` and `.lnk` through `explorer`;
  URLs use `explorer` (`open` on macOS, `xdg-open` on Linux).
- `OpenUrl` requires a real scheme. The RED run caught `validate_url`
  accepting `C:\Windows\notepad.exe` (a one-letter "scheme") and `https://`
  (no host); both are now rejected.
- `ActionRunner` runs actions on a background thread and reports failures
  through `take_failure()`, so the UI never waits for a program to start.

### Fixed — a batch file could not run at all
- `cmd /C "<script>" <args>` never worked when the script's path carried a
  space. `cmd` does not keep a quoted command line: with more than two quotes
  it strips the first and the last one and then runs the **first word**, so
  `C:\Users\…\AppData\Local\Temp\ss550 bat probe\run.bat` was executed as
  `C:\Users\…\Temp\ss550` and died with "is not recognized".
- The route is now `cmd /C call <script> <args>`, which keeps the path whole
  and hands the script its arguments the way a terminal would.
- The script's exit code survives `call`, so a failing batch is reported in
  the status line instead of looking successful.
- Covered by two tests that run a real batch: one with a space in its path
  and arguments that must arrive, one that fails with code 7 and must be
  reported.

### Added — a program that dies is reported
- Starting a program is not the same as it working. A program that cannot run
  — a missing runtime, a missing dependency — **starts fine and then leaves**,
  and the child's output is discarded on purpose, so nothing was visible.
- `ActionRunner` now watches every started program for 6 s on a thread of its
  own. A program that leaves with a failure is reported to the status line:
  `"DS4Windows.exe exited with code -2147450749 (0x80008083)"`. A program that
  leaves successfully, or is still alive when the window closes, is not.
- The window is measured, not guessed: the .NET host takes ~4 s to print
  "You must install .NET" and leave, so a "did it die right away?" check would
  have missed the very case it was meant to catch.
- `explorer.exe` is excluded: it is a shell dispatcher that returns a non-zero
  code even when it opened the file. That covers `OPEN URL` and `.lnk`.
- `ActionRunner::with_exit_watch(window)` sets the window, which is what the
  tests use to stay fast.

### Fixed — quotes in a pasted program path
- Explorer's *Copy as path* wraps the path in double quotes, and the quotes
  were stored and passed to `CreateProcess` verbatim: every key bound to a
  pasted path failed with `ERROR_INVALID_NAME` (os error 123), which reads
  like a broken path even though the path is perfect. A `.lnk` was worse: the
  quote hid the extension, so it was not routed through `explorer` at all.
- `Action::without_wrapping_quotes` removes one matching pair, and
  `ActionStore::set`, `command_for` and `run_action` all apply it, so existing
  bindings files are cleaned on load with no migration. A lone quote is left
  alone, so a genuinely malformed path still reports its real error.
- Verified against the real bindings file: key 1 now resolves to the unquoted
  `.exe` and key 15 to `explorer <Brave.lnk>`.

### Fixed — unread command replies broke key polling
- `write_pack` never consumed the deck's ACK replies, so they queued on the
  interrupt-IN endpoint. `parse_input_report` rejects them (key `0`) and the
  first `poll_events` after a heartbeat would have dropped a healthy
  connection.
- Added `ss550_protocol::is_key_edge` (additive; no public signature changed)
  and a driver-private `decode_key_edge`. `poll_events` now skips non-key
  replies instead of erroring. Command replies and key edges share the same
  `ACK`/`OK` envelope, so the key byte is what tells them apart.

### GUI — key presses and bindings
- The deck is polled every 50 ms with a non-blocking `poll_events`, separately
  from the 500 ms maintenance tick (collect, retry, heartbeat).
- A physical press sets a bright border on its tile and runs the bound action;
  the release clears the border. Tiles now print what the key does, so the
  grid is readable at a glance.
- Added `STEP 3 · BIND AN ACTION TO A KEY`: a 5x3 key picker,
  `LAUNCH APP` / `OPEN URL`, program path with one-argument-per-line
  `ARGUMENTS`, the URL field, `SAVE BINDING` and `CLEAR KEY`. Picking a key
  loads its current binding into the fields.
- The window is now two columns in a 1240x880 frame: deck on the left,
  bindings on the right.
- **Verified on hardware**: a physical press lights its tile and runs the
  bound action. Every launch route was exercised on the real deck: an `.exe`
  by path, a `.lnk` shortcut (Brave and Obsidian, the second one pasted with
  its quotes), and `OPEN URL` (`https://x.com/home`).

### New GUI crate — `ss550-gui` (GPUI Kit)
- Added `crates/ss550-gui`, a desktop UI built on
  [GPUI Kit](https://github.com/longbridge/gpui-kit) `v0.9.0`.
- It is its own workspace root, so the 800-package GPUI dependency tree never
  slows down the driver/protocol workspace.
- The window mirrors the physical deck: three rows of five colour tiles, each
  tile painted the colour the key is actually showing (`WHITE`, `RED`, `OFF`),
  with the key number and colour name rendered inside it.
- Numbered steps (`STEP 1 · PICK THE COLOUR`, `STEP 2 · CLICK A KEY TO PAINT IT`)
  plus explicit action buttons: `PAINT ALL 15 KEYS`, `CLEAR ALL (BLANK)`,
  `KEEP ALIVE`, `RECONNECT`.
- Connection state is a coloured pill (`CONNECTED` / `CONNECTING` /
  `NOT CONNECTED`), and a status line reports the last action or the exact error.
- Starts in **dark mode**; the `LIGHT MODE` / `DARK MODE` button in the header
  switches themes at runtime (`Theme::change`). Key tiles keep the literal
  white/red of the deck and only the blank (`OFF`) tile follows the theme, so the
  grid stays readable in either mode.
- The device is connected **immediately** when the window opens, and the
  connection attempt runs on a **dedicated worker thread** so the window never
  freezes while `Device::connect` waits for the deck's boot re-enumeration to
  settle.
- **The app now retries forever.** A new attempt starts every 2 s while the
  deck is missing, and the status line shows the attempt counter. The deck
  leaves the USB bus for 4–8 s while it boots, so the old single attempt could
  never win that window; plugging the deck in after the app is open now works.
- The attempt waits `LONG_CONNECT_TIMEOUT` (30 s), long enough to cover the
  deck's whole boot cycle (appear → leave → brief appearance → leave →
  settle). Measured on hardware: **the app connects 3.6 s after opening, on
  the first attempt**, with no `RECONNECT` needed.
- The header pill now distinguishes `CONNECTING`, `WAITING FOR DECK` (amber,
  the app is still looking for it) and `CONNECTION LOST` (red), so a missing
  deck no longer looks like a dead app.
- New `MINIMIZE · KEEPS RUNNING` button: the window can be hidden while the
  app stays connected, keeps the deck awake and reconnects on its own.
  Verified on hardware: with the window minimized the process was still alive
  after 150 s and the deck was still enumerated (without the heartbeat the
  deck resets itself after far less).

### Added — driver connect timeout
- `Device::connect_with_timeout(timeout)` and `Device::connect_with_api_timeout(api, timeout)`
  let a long-lived client wait for the deck's whole boot cycle.
  `Device::connect` keeps its previous 10 s behaviour, so no existing caller
  changes.
- `LONG_CONNECT_TIMEOUT` (30 s) is the value a GUI or daemon should use: the
  deck's settle can be ~11 s away, and the old 10 s ceiling made an attempt
  give up just before the deck was ready. A deck that is **never** seen still
  fails fast, so an app started without the device never hangs.

### Fixed — eframe text was invisible
- `eframe` was declared with `default-features = false` and only `glow`, which
  also removed the `default_fonts` feature. egui had **no fonts at all**, so
  every label, button caption and status text rendered as nothing — the
  "grey boxes with nothing inside" symptom. The dev-dependency now enables
  `default_fonts` explicitly.

### GUI example — visual redesign
- Completely redesigned `gui.rs` with a cleaner, more usable layout:
  - Clear section headers with labels
  - Color-coded status indicator (green/red/yellow text)
  - Visual color selector with ○/● indicators for selected state
  - Control buttons with descriptive text: "CLEAR ALL\n(Reset to white)"
  - LCD buttons show number + current color: `#11\nWhite` or `#11\nRed`
  - Row labels explain button positions: "TOP ROW (keys 11-15) - Top of device"
  - Painting indicator shows current color selection
- Each section now has clear, explanatory text
- Button labels are larger and unambiguous

### Code quality
- Fixed clippy warning: `button_cache.fill(None)` instead of manual loop.
- Fixed clippy warning: `#[derive(Default)]` on `Color` enum with `#[default]` attribute.
- `ss550-driver` painted two keys through its own public API on the test
  device: `Device::connect` → `initialize` → `set_key_image(WHITE, OpenDeck 0)`
  → `set_key_image(RED, OpenDeck 14)`, completed 68 ms after the device
  reappeared. Visually confirmed: bottom-left white, top-right red. This proves
  transport, `BAT`/chunk/`STP` framing, and the `N → N+1` key mapping end to end.
- **New measured constraint: the deck reboots itself ~1–3 s after power-up.**
  With **zero host writes** (`quiet_probe`), it is present at `+1000 ms` and
  `+2063 ms`, **leaves the USB bus at `+3126 ms`**, and returns at `+8426 ms`. A
  second run left at `+953 ms` and returned at `+4781 ms`. Measured absence:
  4–5 s.
- **Consequence: a handle opened in the first stage is permanently stale.**
  Using it after the reboot returns `0x48F` (`ERROR_DEVICE_NOT_CONNECTED`) or
  `0x3E5` (a transfer that never completes). Re-opening during the reboot does
  not help (`reopen_probe`: three attempts, all writes failed). This, not a
  protocol mistake, explains every "the window closed" measurement above —
  including the earlier "~2.3 s" number, which was the end of stage 1.
- **There is more than one re-enumeration.** A run showed the deck return at
  `+1859 ms` (absent 828 ms), accept `HAN` on a fresh handle, then vanish again
  **502 ms later** with `0x48F`. The boot is: appear → leave → brief appearance →
  leave → settle. **Presence is only trustworthy after the device has been
  continuously present for ~3 s.**
- **Root trigger found: idleness.** Two runs with the same deck and port: with
  **zero writes** from the host it stayed healthy 68.7 s and then dropped the
  vendor interface and the whole device; with one `CRT|DIS` every 500 ms it
  survived **180 s / 360 reports** with no drop. The deck does not reset on a
  timer, it resets when the host goes quiet. Verified visually: bottom-left
  WHITE and top-right RED painted on the live device during the run.
- **Product string is `HOTSPOTEKUSB HID DEMO`.** The deck exposes two HID
  collections under `pid 0x3000`: keyboard on interface 1
  (`usage_page 0x0001/0x0006`) and vendor on interface 0
  (`usage_page 0xffa0`). The driver's choice of interface 0 is correct.
- **A failed enumeration is invisible to a `VID_0200` filter.** The node becomes
  `USB\VID_0000&PID_0002` with `CM_PROB_FAILED_POST_START` (Code 43); probes that
  filter on the vendor id report "absent" when the deck is electrically present
  but cannot answer a descriptor request.
- **`connect()` verified against a real boot.** The process was already watching
  the bus before the deck was unplugged, so the first reappearance it saw was a
  boot stage. `Device::connect` returned to `SETTLE_TIME + ~170 ms` and the first
  write landed 3 ms later, with no `0x48F`/`0x3E5`. The extra ~170 ms is
  consistent with one absorbed disappearance; the driver keeps no transition
  telemetry, so that part is inference, not observation.
- **Still open:** the exact idle threshold (below 68.7 s), and automatic
  reconnection when a handle dies *after* connecting — the new `reconnect()`
  plus `repaint_all()` APIs provide the raw materials; an app can build its
  own recovery loop, but the driver does not implement it automatically yet.
- **Input verified.** Button presses arrive on the **vendor** collection as
  512-byte interrupt IN reports with `"ACK"`/`"OK"` header, key at `[9]`,
  state at `[10]`. The keyboard collection (MI_01) emits nothing. This is now
  documented in `docs/protocol/input-events.md`.
- Ruled out by measurement: unread interrupt IN blocking the firmware
  (`read_in_probe`: zero IN bytes ever), the bare `HAN` handshake
  (`handshake_probe`: closed at `3011 ms`), `CRT|CONNECT` keep-alives
  (`keepalive_probe`: `1203 ms`), and reopening the handle (`reopen_probe`).
- `hardware_gate` gained `--wait-hotplug`: it waits for the deck to disappear
  and reappear, then paints within ~70 ms, because any human-timed run lands
  outside the window.
- `ss550-probe` gained `enumerate`, `idle_halt_probe`, `resume_probe`,
  `single_write_probe`, `hotplug_probe`, `keepalive_probe`, `handshake_probe`,
  `read_in_probe`, `reopen_probe`, `quiet_probe` and `reboot_probe`.


### Added
- `Device::reconnect()`: recovery when a handle dies (e.g., after a transport
  stall). Waits for the deck to settle, opens a fresh handle, and resets
  `initialized`, `brightness`, and `last_write`. Call `initialize()` and
  `repaint_all()` after. See the inline docs for an example recovery pattern.
- `Device::repaint_all()`: repaints every cached button image from the internal
  cache. Required after `reconnect()` or `initialize()`, since neither restores
  painted images. The cache is updated on every `set_key_image()` call and
  cleared by `clear_key()`/`clear_all()`.
- `button_cache: Vec<Option<ButtonImage>>` field on `Device`: tracks which
  buttons have been painted so they can be restored after recovery.
- **Button input, decoded and verified on hardware.** `ss550-protocol` gains
  `parse_input_report`, `read_index_to_firmware_key`, `InputEvent`,
  `ButtonState` and `INPUT_REPORT_SIZE`; `ss550-driver` gains
  `Device::poll_events` and `ButtonEvent`. Measured: reports arrive on the
  **vendor** collection (MI_00) as **512-byte** interrupt IN reports with **no
  report ID byte**; layout is `"ACK"` at `[0..3]`, `"OK"` at `[5..7]`, key at
  `[9]`, state at `[10]` (`0x01` pressed, `0x00` released). The keyboard
  collection (MI_01) emits nothing, so presses never reach the OS as keypresses.
- **The read and paint numberings differ.** Painting is row-major from the
  bottom-left; reports are row-major from the top-left. Confirmed one press at a
  time: top-left reads `1` and paints as `11`, top-right reads `5` and paints as
  `15`, bottom-left reads `11` and paints as `1`. `read_index_to_firmware_key`
  applies the row flip so callers only ever see the paint numbering.
- **The firmware does not debounce.** A 150 ms click produced two separate
  reports, below the 199 ms the vendor's Qt layer applies. Both edges are
  reported; debouncing is host-side policy.
- `ss550-driver` example `button_gate`: paints two corners, then runs the loop an
  idle app would run (non-blocking poll + heartbeat) and prints every key edge.
  Verified live: `BottomLeft`, `TopRight`, `Center`, correct and not mirrored.
- `ss550-probe` example `button_probe`: listens on **both** HID collections
  simultaneously and dumps every report. `read_in_probe` had been misread — it
  never pressed a button, so its "zero IN bytes" only proved the deck is silent
  when idle.
- `Device::keepalive()` in `ss550-driver`: the deck resets itself after ~68 s of
  host silence, and the reset kills the handle. `keepalive` sends a throttled
  `CRT|DIS` (default every 2 s) and never fires while an image is queued, so it
  cannot corrupt a `BAT` → chunk → `STP` transfer. `set_heartbeat_interval()` and
  `heartbeat_interval()` expose the policy; `DEFAULT_HEARTBEAT_INTERVAL` is
  public.
- `ss550-driver` example `heartbeat_gate`: paints two corners and calls
  `keepalive()` every 100 ms for three minutes, crossing the measured 68.7 s
  idle-death mark.
- `ss550-probe` examples `identity_timeline` (full, unfiltered device identity
  over time), `hold_probe` (does continuous traffic prevent the reset),
  `handshake_probe`, `read_in_probe`, `reopen_probe`, `quiet_probe` and
  `reboot_probe`.
- `ss550-driver` crate: synchronous HID driver for the Redragon StreamCraft
  SS550 v3 (`VID 0x0200 / PID 0x3000`, 15 LCD buttons). Public API:
  `Device::connect`, `initialize`, `set_brightness`, `queue_image`,
  `flush`, `set_key_image`, `clear_all`, `clear_key`, `sleep`,
  `firmware_version`.
- `ss550-protocol` crate: pure HID codec (no I/O, no USB, no `std::time`).
  Byte-exact to `mirajazz`, covered at **100% lines/functions/regions**
  (16 golden + 5 proptest, `cargo llvm-cov`).
- `ss550-probe` crate: one-shot Fase 0 live-device probe harness
  (enumeration, descriptor dump, `--upload`/`--all`/`--key`/`--profile`,
  `--ver`, `--wake`, `--diff`, passive listener).
- Physical key model: bottom-left origin, linear mapping
  OpenDeck `N` → firmware `N+1` (1-based on the wire).
- Probe and protocol docs under `docs/protocol/` (`image-upload.md`,
  `input-events.md`, `profiles.md`, `display-specs.md`,
  `hid-descriptors.md`, `probe-p1b.md`, `probe-p2.md`).

### Changed
- `Device::connect_with_api` now takes `&mut HidApi`. `device_list` returns a
  cached snapshot, so a caller-owned `HidApi` shared behind `&` cannot see a
  plug, an unplug or a boot re-enumeration and would hand out a dead handle.
- `Device::connect()` waits for `SETTLE_TIME` (3 s) of continuous presence before
  opening the handle, and fails fast after `ABSENT_GRACE` (500 ms) when the deck
  was never seen. Previously it opened on the first appearance, which the boot
  sequence guarantees to be stale.
- Fixed systematic 1-byte framing shift in the early driver (tag/length/key
  fields). All packs are now built by `ss550-protocol` and pinned by golden
  vectors. This is a bugfix against the wire contract; the old bytes were
  wrong relative to mirajazz/probe, not a supported API.
- `firmware_version()` now actually sends `CRT|VER` and parses the control
  reply (`get_input_report`, 1025-byte buffer). Previously it built the pack
  and returned an empty string.
- `clear_key(Key)` and `sleep()` added (both `STP`-committed / stateful
  wake semantics: `sleep` marks the driver uninitialized so the next
  `initialize()` wakes it).

### Fixed
- `Key::position()` documented "Row 0 = top" while its implementation returned
  `(0, 0)` for `BottomLeft`, which is the bottom row. The implementation was
  right (row 0 = bottom, matching the enum order and the OpenDeck numbering) and
  the doc was wrong; the module's mapping table carried the same error. No
  behaviour changed — the helper had no callers.
- `firmware_version()` no longer writes `CRT|VER`. That write is rejected by
  the firmware and **halts the USB endpoint**, which then breaks every host
  (this driver and the vendor app) until a power cycle. The version comes from
  a static identity report readable with `GET_REPORT` alone, so the method now
  reads only, and its docs say the value is not evidence of a working write path.
- `initialize()` no longer sends `CRT|LIG 0` and no longer leaves the deck at
  brightness 0 waiting for a `flush()` to restore it. It wakes the display and
  touches nothing else; the image path renders correctly at full brightness.
- `flush()` no longer sends a bare `STP` when nothing is queued. A commit with
  no transfer is one of the reports the firmware rejects.
- New `Ss550Error::TransportStalled { detail }`, mapped from hidapi's
  `0x1F`/`0x3E5` messages, so a halted endpoint reports the power-cycle
  recovery instead of a generic HID I/O error. It keeps the raw hidapi message:
  `0x1F` (refused outright) and `0x3E5` (never completes) are different
  symptoms. Added `Device::brightness()`.
- `CLE` field placed at wire `[12]` (not `[11]`); `0xFF` = all keys.
- Device discovery now always prefers the writable vendor collection
  (`usage_page 0xffa0`, `MI_00`) instead of the first VID/PID match, which
  could open the keyboard collection where Windows denies writes.
- Write path restored to interrupt OUT (`hidapi::write`). Control
  `HidD_SetOutputReport` (`send_output_report`) returns success but the
  firmware ignores it, so it silently did nothing and must never be used for
  this path.

### Added (diagnostics and documentation)
- `docs/troubleshooting/deck-stuck.md`: runbook for the recurring "backlit but
  black, no host can paint" failure — diagnosis table, recovery order, root
  cause model, and the code rules that prevent a repeat.
- `docs/protocol/INCIDENTS.md`: chronological incident record.
- `ss550-probe` diagnostic examples: `test_write_path` (per-report timing over
  interrupt OUT), `raw_write` (own overlapped `WriteFile`, 10 s wait),
  `probe_control` (baseline-vs-post-write `GET_REPORT`), `test_send_output`
  (control vs interrupt path), `enumerate` (dumps every collection and which one
  the probe opens), `idle_halt_probe` (paints, then measures whether the OUT
  endpoint dies with idle time; `--settle <secs>` guards against writing during
  re-enumeration), `resume_probe` (interleaves control transfers to prove a
  failure is a halted endpoint and not USB selective suspend), `hotplug_probe`
  (writes within milliseconds of the device appearing, then beats once per
  second to find how long it survives).
- Verified that `get_input_report` returns a **static** identity report
  (`.V3.SS_550.01.002.`) with zero writes sent, so a VER reply is not evidence
  that a write reached the firmware.

### Tests
- 42 tests passing: 17 driver unit + 5 probe unit + 18 protocol (16 golden + 2
  doctests) + 5 proptest. `cargo fmt`, `cargo clippy --all-targets
  --all-features -- -D warnings`, and release builds pass on Windows
  (rustc 1.99.0).

### Added — the seam between a deck and its bytes
- `crates/deck-api` is the new contract between **what a deck can do** and
  **how its bytes travel**: a `Deck` trait (geometry, image spec, capabilities,
  identity, painting, clearing, key events) above a `Transport` trait (write a
  report, read an input report, read a control report).
- A deck that cannot do something refuses it with a typed
  `DeckError::Unsupported`. Accepting a call it cannot honour is worse than
  refusing it: the user gets a key that does nothing and no way to find out
  why.
- `KeyId` is 0-based and `Geometry::position` derives the row and the column
  from the deck's own geometry, so no layer has to assume five columns.
- The crate names no device, and a test keeps it that way: it fails when the
  string `ss550` appears anywhere in the seam's source. The guard was proved
  by leaking the name on purpose and watching it fail.
- The contract was sealed before anything consumed it, so the three crates
  above it code against one shape and not three:
  - `Geometry::new(key_count, columns)` derives the rows, so a deck cannot
    publish a grid that contradicts its own key count.
  - `Capabilities` is built by naming each capability, and every capability
    has an operation behind it — `sleep` and `set_dial` are now operations of
    the trait, and `dials` is a capability the trait can act on.
  - A brightness or a dial position outside `0..=100` is refused with
    `DeckError::ValueOutOfRange`. It is never clamped: a clamped request is a
    request that changed without telling anyone.
  - `DeckError::Unsupported` names the refused operation with the
    `DeckOperation` type instead of a free-form string, so a typo cannot
    change the message the user reads.
  - `Transport::read_input` takes a `Duration`, the same unit the deck layer
    uses, and "nothing arrived" has one answer everywhere: zero bytes read,
    not an error.
- The seam's guards now cover **the whole crate**, manifest and tests
  included, and the seam's own handle is covered by behaviour tests: a stub
  deck records what it was asked to do, and the test fails if the handle
  forwards anything to the wrong call.

### Known limitations — Fase 1 close-out
- **Live verification of input events.** Button presses were verified one-at-a-time
  on hardware, but we have not exercised many presses in quick succession nor
  verified the 150-270 ms reporting delay matches reality for all keys.
- **No event stream (button presses) exposed as a stream type.** Consumers must
  call `poll_events(Duration::ZERO)` in a loop; an async variant could be added
  later with `tokio::fs::File` wrappers.
- **No automatic reconnection on handle death.** Apps must catch
  `Ss550Error::TransportStalled`, call `reconnect()`, `initialize()`, then
  `repaint_all()`. This is a conscious design: the caller decides when to pay
  the ~3 s settle wait. A transparent reconnection mode could be added behind
  a flag if needed.
- **No profile push (`ICO`/`ULEND` is static-only).** The deck accepts image
  data but the host does not yet manage "profiles" (named sets of key images).
- **No hotplug watcher.** Apps must poll `Device::connect()` or use external
  device-notifier crates.
- **No async transport (`nusb`/`tokio` deferred; `hidapi` sync is the Phase 1
  baseline**, but `poll_events` with non-zero timeout could unblock on a thread
  pool, enabling a simple async façade).
- **No udev rules shipped.** Linux users need to create
  `/etc/udev/rules.d/99-streamcraft-ss550.rules` themselves.
- **No `cargo fuzz` target, no `docs/protocol/verified.md` safe-mode list.**
- The workspace is not yet under version control at the root; when it is,
  squash commit messages must cite the REA/mirajazz trace and must not
  contain serial numbers (AGENTS.md §5/§7).
