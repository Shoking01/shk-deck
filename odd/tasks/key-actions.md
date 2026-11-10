# Key actions — turn the SS550 into a controller

**Objective:** grow the API beyond painting buttons: read the physical key
presses, bind an action to each key, and run it.
**Branch:** `main` of the private repository `Shoking01/shk-deck` (it was not
under version control while this feature was written, so its evidence cites no
commits). The feature document and `CHANGELOG.md` are the record of this work.
**Delivery strategy:** single delivery, no chain.
**Test runner:** `cargo test --workspace` and
`cargo clippy --workspace --all-targets -- -D warnings` at the repo root;
`cargo clippy --all-targets -- -D warnings` inside `crates/ss550-gui` (its own
workspace).

## Specs

### S1 — Action catalogue

The v1 catalogue is **"App + URL"**, chosen by the user verbatim. The API
exposes exactly two action kinds:

- `Launch { program, args }` — run a program.
- `OpenUrl { url }` — open a URL in the default handler.

No other action kind exists in v1. OBS scenes and keyboard macros are
explicitly out of scope until a later version.

### S2 — A binding ties a deck key to an action

A binding maps one of the 15 deck keys to one action. Keys are addressed by
the **1-based number the user sees in the UI** (1 = bottom-left, 15 =
top-right), which is also the firmware key number.

### S3 — Bindings persist and reload

Bindings are stored as JSON in the user's configuration directory and are
reloaded when the app starts. A corrupt or missing file must not stop the app
from opening: it starts with no bindings and reports the problem in the status
line.

### S4 — Actions run without a shell

The program is launched with its arguments as **separate argv entries**, never
through a shell string, so a path or argument can never be re-interpreted as a
command. On Windows `CreateProcess` cannot execute `.bat`/`.cmd` or `.lnk`
directly, so those are routed through `cmd /C` and `explorer` respectively.
`OpenUrl` requires a URI scheme; a bare string is rejected instead of being
guessed at.

### S5 — A physical press lights the key and runs its action

Pressing a key on the deck lights its tile in the UI and runs the action bound
to it. The action runs off the UI thread, so the window never stutters.

### S6 — The user can bind and clear any key from the GUI

The GUI exposes a bindings section: pick a key, choose `LAUNCH APP` or
`OPEN URL`, fill the fields, and save or clear. Every deck tile also shows
what its key does, so the grid is readable at a glance.

### S7 — Documentation follows the behaviour

The engine crate, the GUI README and the changelog document the new
behaviour, the action catalogue and where the bindings file lives.

## Tasks

| ID | Specs | Route | Status | Evidence |
| --- | --- | --- | --- | --- |
| T1 | S1, S2 | inline | done | `Action`, `KeyNumber` (1..=15, `Default` = key 1). RED first: 23 tests failed on `todo!()`; GREEN: 31 unit + 5 doctests in `ss550-engine`. |
| T2 | S3 | inline | done | `ActionStore` JSON round-trip, version check, missing-file load (`tempfile`). Corrupt file → GUI opens with no bindings and reports it. |
| T3 | S4 | inline | done | `CommandSpec`/`command_for`/`run_action`, no shell; `.bat`/`.cmd` → `cmd /C`, `.lnk` → `explorer`, URL → platform opener. `validate_url` bug found in RED and fixed. |
| T4 | S5 | inline | done | GUI `POLL` (50 ms) non-blocking `poll_events`; `ButtonState::Pressed` → highlight + `ActionRunner::submit`; release clears. Driver reply-skipping fix (`is_key_edge` + `decode_key_edge`) needed for this to work past the first heartbeat. |
| T5 | S6 | inline | done | `STEP 3` panel: picker, `LAUNCH APP`/`OPEN URL`, `Input`/`Textarea` fields, SAVE BINDING/CLEAR KEY; tile labels show the binding; two-column 1240×880 layout. |
| T6 | S7 | inline | done | `crates/ss550-engine/README.md`; GUI README (STEP 3, press→highlight→action, bindings file); root `CHANGELOG.md`; `docs/protocol/CHANGELOG.md` (`is_key_edge`, stale deferred item corrected). |

**Route evidence.** All six units run inline. The Writer trigger does not
fire: the units are sequential (the GUI consumes the engine), T4 and T5 touch
the same file, and none of them reaches the ~100 changed lines that would
make a worker cheaper than doing it here.

**Checks.** `cargo test --workspace` green (24 protocol, **45** engine + 6
doctests, 10 driver, plus probe/integration suites); `cargo fmt --all` clean;
`cargo clippy --workspace --all-targets -- -D warnings` clean. GUI:
`cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo build`
clean; the binary ran 12 s with title `StreamCraft SS550 Controller` and no
stdout/stderr.

Hardware verification (2026-11-08): a physical press lights the tile and the
bound action runs. The first attempt failed with `ERROR_INVALID_NAME` (L13)
and the second revealed the silent-death gap (L15); both are fixed. The
`.lnk` bound to key 15 was confirmed on hardware: the browser opened at once.

## Log

- **L1 (2026-11-07)** — "Eso ultimo de que corra en segundo plano es para mas adelante. Necesitamos que nuestra Api haga mas cosas que solo pintar botones"
- **L2 (2026-11-07)** — Decision: the next capability is **"Pulsaciones + acciones"**. The tray/background work is deferred, not cancelled.
- **L3 (2026-11-07)** — Decision: the v1 action catalogue is **"App + URL"**.
- **L4 (2026-11-07)** — Evidence: the read path is already live-verified.
  `Device::poll_events` decodes interrupt-IN reports on the vendor collection
  (`"ACK"` at `[0..3]`, `"OK"` at `[5..7]`, key at `[9]`, state at `[10]`),
  512 bytes, no report ID; `read_index_to_firmware_key` applies the measured
  top↔bottom row flip. The driver has never had a consumer for it.
- **L5 (2026-11-07)** — Evidence: the driver already exposes `set_brightness`
  (live-verified `LIG`), `sleep`/`initialize` (`HAN`, *not* live-verified) and
  `firmware_version` (a static identity read — writing `CRT|VER` halts the
  USB endpoint). `brightness()` returns the last value *written*, not the
  device's state; that is a known defect, out of scope here.
- **L6 (2026-11-07)** — Evidence: the side screen, the lamps and the logo are
  static-analysis only (`docs/protocol/display-specs.md`) and must not be
  written to the device without a dedicated test session. The `MOD` profile
  push is unverified for the same reason.
- **L7 (2026-11-07)** — Decision: the new code goes in a new crate
  `crates/ss550-engine` (the `engine` layer of the dependency flow
  `protocol → driver → engine → ui`), inside the root workspace so it keeps
  the root test suite fast and free of the GPUI tree.
- **L8 (2026-11-07)** — Decision: the driver is **not** modified. `Key`,
  `Key::from_firmware_key` and `poll_events` are enough, so the existing
  public API stays byte-for-byte compatible.
- **L8 amended (2026-11-08)** — The decision above was wrong and is amended:
  the driver *did* need one read-side fix. `write_pack` never reads the deck's
  ACK replies, so they queue on the interrupt-IN endpoint;
  `parse_input_report` rejects them (key `0`) and the first `poll_events` after
  a heartbeat would have dropped a healthy connection. Fixed additively:
  `ss550_protocol::is_key_edge` plus a driver-private `decode_key_edge` that
  `poll_events` uses to skip non-key replies. No public signature changed, so
  the compatibility goal still holds.
- **L9 (2026-11-08)** — Evidence: the RED run of T1–T3 caught a real defect in
  `validate_url`: it accepted `C:\Windows\notepad.exe` (a one-letter scheme is
  a drive letter) and `https://` (no host). Fixed by requiring a scheme of at
  least two characters and a non-empty host whenever `//` is present; both
  cases are now pinned by tests.
- **L10 (2026-11-08)** — Decision: the GUI is two columns in a 1240×880
  window — deck and paint controls on the left, `STEP 3 · BIND AN ACTION TO A
  KEY` on the right. A single column made binding a key require scrolling away
  from the key it belongs to. Each tile prints its binding so the grid answers
  "what does this key do" without opening the panel.
- **L11 (2026-11-08)** — Decision: the deck is polled at 50 ms with
  `poll_events(Duration::ZERO)`, in a timer separate from the 500 ms
  maintenance tick (collect, retry, heartbeat). A press has to feel instant;
  retries and heartbeats must not run 20 times a second.
- **L12 (2026-11-08)** — Open verification: the whole software path is tested
  and green, but only a physical press proves S5 end to end (tile lights, bound
  action fires). That is the one item left, and it needs the hardware.
- **L13 (2026-11-08)** — Evidence from the first hardware test: the tile lit,
  but the status line said `Action failed: could not run
  "C:\Users\quiro\Downloads\Adrian\DS4Windows\DS4Windows.exe": The filename,
  directory name, or volume label syntax is incorrect. (os error 123)`. The
  path exists and `run_action` with the identical string succeeds, so the
  stored value differed. `%APPDATA%\ss550\bindings.json` showed why: the
  program began and ended with code 34 (`"`). The path had been pasted with
  Explorer's *Copy as path*, which adds quotes; `CreateProcess` rejects them
  with `ERROR_INVALID_NAME`. The `.lnk` on key 15 had the same defect, and the
  quote also hid the `lnk` extension so it never reached the `explorer` route.
  Fix: `Action::without_wrapping_quotes`, applied by `ActionStore::set`,
  `command_for` and `run_action`; a lone quote is left visible. Verified
  against the real file: key 1 resolves to the unquoted `.exe`, key 15 to
  `explorer <Brave.lnk>`, and the re-serialised JSON holds no quotes.
- **L14 (2026-11-08)** — Separate finding, not our defect: this machine has no
  .NET runtime (`dotnet` is not installed), and `DS4Windows.exe` is a .NET 8
  app, so it exits immediately printing "You must install .NET". The runner
  nulls the child's stdio, so that message is invisible from the GUI. Launches
  are correct; the target program needs its runtime. Detecting a quick
  non-zero exit and reporting it in the status line is a candidate follow-up,
  not implemented here.
- **L15 (2026-11-08)** — L14's follow-up is now implemented, because a silent
  failure is the worst kind: the runner watches every started program for 6 s
  (`EXIT_WATCH`) on a thread of its own and reports a non-zero exit in the
  status line. The window is measured: the .NET apphost takes ~4 s to print
  its complaint and leave, so a "did it die right away" check would have
  missed it. `explorer.exe` is excluded because it returns non-zero even when
  it opens the file (it covers `OPEN URL` and `.lnk`).
  `ActionRunner::with_exit_watch(window)` makes the window injectable, which
  is what the tests use. Verified against the real target:
  `DS4Windows.exe exited with code -2147450749 (0x80008083)`.
- **L16 (2026-11-08)** — Measured, not assumed: the `.NET` apphost exits with
  `0x80008083` and takes **4073 ms** to do it. The child's stderr is discarded
  by design (no console windows on a stream deck), so the exit code is the
  only signal the UI can show. Capturing the child's stderr was considered and
  rejected: a pipe fills at 64 KB and would block the child, and a thread per
  action would leak for every long-lived program.
- **L17 (2026-11-08)** — Hardware verification passed: the user confirmed that
  pressing the key bound to the Brave shortcut opens the browser immediately,
  and later the same for an Obsidian shortcut whose path had been copied with
  its quotes. S5 is therefore verified end to end, not just in software. The
  only thing still standing is environmental: `DS4Windows.exe` needs the .NET
  8 Desktop Runtime, which this machine does not have — and now, thanks to
  L15, the app says so instead of staying silent.
- **L18 (2026-11-08)** — The user's standard: "apps that have everything
  should work". Auditing the routes that no one had ever executed found a
  real defect: **the `.bat`/`.cmd` route never worked at all** when the
  script's path carried a space. `cmd /C` does not keep a quoted command
  line — with more than two quotes it strips the first and the last one and
  runs the first word, so the command was executed as `C:\Users\…\Temp\ss550`
  and died with "is not recognized". The route is now `cmd /C call <script>
  <args>`, which keeps the path whole, hands the script its arguments the way
  a terminal would, and preserves its exit code (so a failing batch is
  reported, not silently accepted). Two tests run a real batch to prove it:
  one with a space in the path and arguments that must arrive, one that fails
  with code 7 and must reach the status line.
- **L19 (2026-11-08)** — `OPEN URL` verified on hardware: the user bound
  `https://x.com/home` and it opened at once. With the `.lnk` routes (Brave,
  Obsidian) and the `.exe` route, **every launch route of the v1 catalogue is
  now proven on the real deck**, not only in software. The v1 scope is closed:
  the remaining known gap is environmental (.NET 8 for DS4Windows, which the
  user said does not matter) and the deferred tray/background mode.
