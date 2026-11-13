# ss550-gui

Desktop UI for the Redragon StreamCraft SS550 controller, built with
[GPUI Kit](https://github.com/longbridge/gpui-kit) `v0.9.0`.

## Why it is a separate crate

GPUI Kit pulls in the full GPUI stack (about 800 packages). Keeping it in its
own workspace root (`[workspace]` in this `Cargo.toml`) means
`cargo test`/`cargo clippy` at the repository root stay fast and keep working
without a GPU-capable dependency tree.

`crates/ss550-tray` is its own workspace root for the same reason: it carries
the Win32 and notification-area tree, and the driver/protocol workspace should
not pay for it.

## Run

```text
cargo run --manifest-path crates/ss550-gui/Cargo.toml
```

The first build compiles the whole GPUI tree and takes several minutes. Later
builds are incremental.

## What the window shows

| Element | Meaning |
| --- | --- |
| Coloured pill in the header | `CONNECTED`, `CONNECTING`, `WAITING FOR DECK` or `CONNECTION LOST` |
| `THEME: DARK · CLICK FOR LIGHT` button | Switches the theme at runtime; the app starts in dark mode |
| `STEP 1 · PICK THE COLOUR` | `WHITE` / `RED`; the one marked `✓ SELECTED` is used for the next clicks |
| `STEP 2 · CLICK A KEY TO PAINT IT` | 3x5 tiles matching the physical deck, top row first |
| A tile | Its background is the colour that key is showing; it prints the key number, the colour name and what pressing the key runs |
| A bright border on a tile | That key is **pressed right now** on the deck |
| `STEP 3 · BIND AN ACTION TO A KEY` | 5x3 key picker, `LAUNCH APP` / `OPEN URL`, the fields for the chosen kind, `SAVE BINDING` and `CLEAR KEY` |
| `PAINT ALL 15 KEYS` | Applies the step 1 colour to every key |
| `CLEAR ALL (BLANK)` | Blanks every key (`Device::clear_all`) |
| `KEEP ALIVE · SEND ONE NOW` | Sends one `keepalive()` by hand |
| `RECONNECT` | Drops the handle and asks the worker for an immediate new attempt |
| `HIDE · KEEPS RUNNING` | Hides the window; the app keeps running, connected, heartbeating and still running bound keys |
| `STEP 4 · RUN IN THE BACKGROUND` | Says where the tray icon is, whether the tray is available at all, and holds `START WITH WINDOWS` |

## Key numbering

The driver numbers keys with a bottom-left origin (`0..=14`), matching OpenDeck.
The UI shows the 1-based firmware key (`index + 1`), so the rows read:

- top row: keys 11 to 15
- middle row: keys 6 to 10
- bottom row: keys 1 to 5

That matches how the driver maps paint indices to the firmware.

## Key presses and bindings

The window is also the controller: pressing a key on the physical deck runs
whatever is bound to it.

- The deck is polled every **50 ms** with `Device::poll_events(Duration::ZERO)`.
  The poll never blocks, so it cannot stall the window even when nothing
  happens.
- A press sets the tile's bright border and dispatches the bound action on a
  background thread (`ss550_engine::ActionRunner`), so a slow program never
  freezes the UI. The release clears the border.
- A key with no binding says so in the status line instead of doing nothing
  silently. A failure the worker reports appears there too, on the next tick.
- Bindings are stored as JSON in `%APPDATA%\ss550\bindings.json` (or
  `$SS550_CONFIG_DIR/bindings.json`) by `ss550-engine`, and reloaded on
  startup. A broken file does not keep the app from opening: it starts with no
  bindings and says why.
- `STEP 3` edits one key at a time. Picking a key loads what it currently does
  into the fields, so a binding can be corrected instead of retyped.
  `ARGUMENTS` takes **one argument per line** and each line is passed as its
  own argv entry — nothing is re-split or interpreted by a shell.
- `LAUNCH APP` takes a full program path. On Windows `.bat`/`.cmd` go through
  `cmd /C` and `.lnk` through `explorer`, because `CreateProcess` cannot start
  them directly. `OPEN URL` needs a scheme (`https://…`); a bare string is
  refused instead of guessed at.
- A path pasted with its quotes — what Explorer's *Copy as path* produces —
  is accepted: the matching pair is stripped before the action is stored and
  before it runs.

## Background mode

This is what makes the app behave like ordinary Stream Deck software: it lives
in the notification area, and the window is only a settings panel you can put
away without losing the deck.

| Action | What happens |
| --- | --- |
| The window's X button | **Hides** the window. Nothing quits. |
| `HIDE · KEEPS RUNNING` | The same thing, with a label that says so |
| Tray → `Show controller` | Restores and focuses the window, without restarting |
| Tray → `Quit` | The only way to leave: stops the tray, the poll timer and the heartbeat |

- The controller entity owns the HID device, the 50 ms poll timer and the action
  runner, so it must outlive the window. Hiding keeps all of it alive; closing
  would drop it and the deck would stop being kept awake.
- Hiding is `ShowWindow(hwnd, SW_HIDE)` on the native handle: GPUI has no hide
  API, `App::hide()` is empty on Windows and `minimize_window()` leaves a
  taskbar button. The close veto is `on_window_should_close` returning `false`,
  and the app runs with `QuitMode::Explicit` so only `Quit` ends it.
- The tray icon runs on a thread of its own with its own message loop
  (`GetMessageW`/`DispatchMessageW`); `tray-icon` builds a hidden window on the
  thread that creates it. The controller's window and the icon never share
  messages.
- **Verified live**: `WM_CLOSE` leaves the process alive with
  `IsWindowVisible == false`; the tray icon registers (the process owns a
  `tray_icon_app` window); `Show controller` brings the window back; `Quit`
  ends the process.

### Start with Windows

`START WITH WINDOWS` is **off by default**. Turning it on copies the running
executable to a folder of its own and registers *that* copy:

```text
%LOCALAPPDATA%\Programs\ss550\ss550-gui.exe
```

The registration is one value, `ss550-gui`, under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, for the current user
only: no installer, no administrator rights, no service. Turning the switch off
removes the value; the copied executable stays until you delete the folder,
which is a complete uninstall.

The copy is the point. Registering `current_exe()` would pin the entry to
whatever binary happened to be running — usually `target\debug\ss550-gui.exe`,
which the next `cargo clean` deletes — and Windows would then fail at sign-in
while the app kept reporting the feature as on.

- The entry runs the installed copy with `--hidden`, so a logon start opens the
  controller with no window at all: it appears only in the tray.
- At startup the app reads what Windows would actually start and checks that
  the file is there. An entry that points at a missing file shows as **off**
  with a note naming the path, instead of a switch that lies.
- A failure while installing or registering leaves the switch off and puts the
  reason in the status line.

**Verified live**: the switch produced
`"...\AppData\Local\Programs\ss550\ss550-gui.exe" --hidden`, the copy is a
running application, and the round trip (install, register, read back, detect a
missing executable, remove, restore) passes as an ignored test:
`cargo test -p ss550-tray -- --ignored` inside `crates/ss550-tray`. It is
ignored because it writes to the real `HKCU` key and to `LOCALAPPDATA`, and it
puts back anything it found there.

## Diagnostics

The release build is a GUI subsystem with no console, so a panic or a sudden
exit would otherwise leave no trace. The app writes an append-only log instead:

```text
%APPDATA%\ss550\ss550.log
```

- Every run appends a `started` line, and the tray's `Quit` appends a `quit`
  line. A `started` line with no matching `quit` means the process did not leave
  through the tray's Quit. That is what an unexpected exit looks like, but it is
  a narrowed search rather than a verdict: a logoff, a shutdown or an "End task"
  kills the process without running that handler either.
- A panic writes its message and location before the process goes down, then
  hands the panic to the previous hook, so debug builds still print it to the
  console. Control characters in the message are escaped, so a payload cannot
  forge a second line — including a fake `quit`.
- The log rotates at 1 MB: the full file becomes `ss550.log.old` and a fresh one
  starts. One old file is kept, never a growing pile. The cap is soft on purpose:
  when a viewer or an indexer holds the file open, Windows refuses the rename and
  the log keeps growing until the lock clears, and the next run rotates it then.
  Truncating instead would delete the current log — the only crash evidence a
  release build produces — to satisfy a size limit nobody is waiting on.
- Each line is built in full and written in one call, so a panic on a worker
  thread cannot interleave with a line already being written.
- Writing the log can never fail the app. A log that cannot be written is a log
  that cannot be written, and the controller keeps running.
- `SS550_CONFIG_DIR` moves the whole directory, log included — the log sits next
  to `bindings.json`.

**Opening it.** There is no button. Click the version line at the bottom of
`STEP 4 · RUN IN THE BACKGROUND` five times and the log opens in Notepad; the
status line says whether it started, and the log records the same thing — the
viewer's launch, not that it managed to display the file. The trigger is hidden
on purpose: this is a support tool, not a feature, and a button would invite
clicks that answer nothing.

Notepad is chosen explicitly rather than through the `.log` file association. A
clean Windows has no association for `.log` at all, so `start` would do nothing.
It is also launched as `%SystemRoot%\System32\notepad.exe`, so the lookup does not
walk the process's current directory, where a planted `notepad.exe` would
otherwise run with the controller's privileges. That holds whenever `SystemRoot`
is set, which is every interactive Windows session; the stripped-environment
fallback to the bare name gives it up, and a missing `notepad.exe` at the
absolute path reports the reason instead of falling back to the weaker lookup.

**Verified live**: `2026-11-11T16:22:23Z started v0.1.0 (hidden=true)` landed in
the real path with UTC correct, and `notepad <log>` opened the file.

## Connection lifetime

The app is meant to be opened and forgotten. It connects on its own, keeps the
deck awake and reconnects by itself.

- **The first attempt starts immediately** when the window opens.
- **The attempt runs on a dedicated worker thread.** `Device::connect` blocks
  while it waits for the deck's boot re-enumeration to settle, so calling it
  on the UI thread would freeze the window.
- **The attempt waits for the deck's whole boot cycle**
  (`LONG_CONNECT_TIMEOUT`, 30 s). The deck's sequence is appear → leave the
  bus → brief appearance → leave → settle, and the settle can be ~11 s away.
  Giving up at 10 s (the driver's default) means starting over in a different
  phase of the boot, over and over.
- **The app retries forever.** If the deck is not on the USB bus, a new
  attempt starts every 2 s and the status line shows the attempt counter.
  Plugging the deck in after the app is open works.
- A 500 ms tick collects the worker results, requests retries and sends
  `keepalive()` (internally throttled to 2 s), because the deck resets itself
  after roughly 68.7 s of host silence.
- If a write or the heartbeat fails, the handle is dropped, the pill turns
  red (`CONNECTION LOST`) and the app starts reconnecting on its own.

Measured on hardware: with the deck already settled the app connects **3.6 s**
after the window opens, on the first attempt.

### When the deck stays missing

`Device not found with VID 0x0200 PID 0x3000` means Windows never enumerated
the deck. That is a hardware-side problem, not a UI one: the app keeps
retrying, but it cannot paint a device that is not on the bus. Check the
device manager for `VID_0000&PID_0002` (`Device Descriptor Request Failed`,
Code 43) and follow `docs/troubleshooting/deck-stuck.md` §3: close Stream
Station and `streamdockSwitchAudio`, unplug USB for **60 s** (a 5 s replug is
not enough), confirm the LEDs go dark, then reconnect on another port.
