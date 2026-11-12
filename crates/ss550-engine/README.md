# ss550-engine

The action layer of the Redragon StreamCraft SS550 controller: it binds the
deck's 15 physical keys to actions, keeps those bindings on disk, and runs
them without ever going through a shell.

Dependency position: `engine → ui`. This crate knows nothing about HID,
painting or windows — and nothing about which device the keys belong to. The
GUI maps a decoded key edge to a [`KeyNumber`] and calls the store and the
runner.

## What it does

| Type | Role |
| --- | --- |
| [`Action`] | What a key can do: `Launch { program, args }` or `OpenUrl { url }` |
| [`KeyNumber`] | A deck key addressed the way the UI shows it, **1 to 15** |
| [`ActionStore`] | The bindings, saved as JSON and loaded on startup |
| [`ActionRunner`] | Runs actions on a background thread so the UI never waits |

The v1 catalogue is exactly **App + URL**. OBS scenes, keyboard macros and
profile switching are deliberately out of scope.

## Key numbering

Keys are 1-based, `1` = bottom-left, `15` = top-right — the number the GUI
shows and the firmware key number. `KeyNumber::new` rejects `0` and anything
above `15`, and `KeyNumber::default()` is key `1` (there is no derived
`Default`, which would produce the invalid key `0`).

## The bindings file

`ActionStore::save`/`load` use JSON at the path returned by
[`bindings_path()`]:

- Windows: `%APPDATA%\ss550\bindings.json`
- other platforms: `$XDG_CONFIG_HOME/ss550/bindings.json`, else
  `~/.config/ss550/bindings.json`
- `SS550_CONFIG_DIR` overrides the directory — used by the tests.

A missing file means "nothing bound yet" and is not an error. A corrupt or
newer-versioned file *is* an error, and the GUI reports it and starts with no
bindings rather than refusing to open.

The file is versioned (`STORE_VERSION`); a higher version is rejected instead
of being silently misread.

```json
{
  "version": 1,
  "bindings": [
    { "key": 1, "action": { "type": "open_url", "url": "https://twitch.tv/dashboard" } },
    { "key": 2, "action": { "type": "launch", "program": "C:\\Windows\\notepad.exe", "args": [] } }
  ]
}
```

## How an action runs

`command_for` turns an `Action` into a `CommandSpec` and `run_action` spawns
it. No shell string is ever built: every argument stays its own argv entry, so
a path or argument can never be re-interpreted as a second command.

Platform rules:

| Action | Windows | macOS | Linux |
| --- | --- | --- | --- |
| `Launch` of an `.exe` | direct | direct | direct |
| `Launch` of `.bat` / `.cmd` | `cmd /C call <script> <args…>` | — | — |
| `Launch` of `.lnk` | `explorer <shortcut>` | — | — |
| `OpenUrl` | `explorer <url>` | `open <url>` | `xdg-open <url>` |

The `call` in the batch line is not decoration. `cmd /C` does not keep a
quoted command line: with more than two quotes it strips the first and the
last one and then runs the **first word**, so
`cmd /C "C:\Program Files\Apps\run.bat" arg` tries to run
`C:\Program` and fails. `cmd /C call "C:\Program Files\Apps\run.bat" arg`
keeps the path whole and hands the script its arguments the way a terminal
would — which also means the script's exit code survives, so a failing script
is reported instead of looking successful.

A batch file is a string-based format, not an argv one: it receives its
arguments the way `cmd` hands them over, so `%1` keeps the quotes a terminal
would add. `%~1` strips them.

### What is verified

| Route | How it was proven |
| --- | --- |
| `.exe` by path | a real process was started and watched (test) |
| `.exe` by path, pasted with quotes | the quotes are stripped before the launch (test) |
| `.lnk` shortcut | **verified on hardware**: the Brave and Obsidian shortcuts opened at once |
| `.bat` / `.cmd` with a space in the path and arguments | the script ran and wrote its arguments (test) |
| `.bat` / `.cmd` that fails | its exit code reaches the status line (test) |
| `OpenUrl` | **verified on hardware**: `https://x.com/home` opened at once |

`OpenUrl` requires a real URI scheme: at least two characters (so a Windows
drive letter like `C:\…` is not mistaken for one), and when the URL has an
authority (`//`) it must have a host. `https://` alone and `not a url` are
rejected instead of guessed at.

## Pasted paths keep their quotes

Explorer's *Copy as path* wraps the path in double quotes, so a pasted program
usually arrives as `"C:\…\app.exe"`. Those quotes are not part of the name:
`CreateProcess` rejects them with `ERROR_INVALID_NAME` (os error 123), which
reads as a broken path even though the path is perfect.

`Action::without_wrapping_quotes` removes one **matching** pair, and
`ActionStore::set`, `command_for` and `run_action` all apply it — so a file
saved before the fix is cleaned on load, and no caller has to remember. A lone
quote is left alone: that is a real defect and must stay visible.

Actions are validated before they are stored *and* before they are queued, so
a broken action never reaches disk and never fails later out of sight.

## Background execution

`ActionRunner` owns one worker thread fed by a channel. `submit` validates on
the caller's thread and returns at once; failures inside `run_action` are
pushed into a queue that `take_failure` drains, so the UI can show them in its
status line on the next tick.

```rust
use ss550_engine::{Action, ActionRunner};

let runner = ActionRunner::new();
runner.submit(Action::OpenUrl { url: "https://example.com".to_string() })?;
// later, from the UI loop:
if let Some(failure) = runner.take_failure() {
    eprintln!("action failed: {failure}");
}
# Ok::<(), ss550_engine::ActionError>(())
```

### A program that dies is a failure too

Starting a program is not the same as it working. A program that cannot run —
a missing runtime, a missing dependency — **starts fine and then leaves**,
which without a second look is completely invisible: the child's output is
discarded on purpose (no console windows flashing on a stream deck), so
nothing is printed anywhere.

So the runner watches every started program for a few seconds
(`EXIT_WATCH`, 6 s) on a thread of its own:

- the program leaves with a failure → `"DS4Windows.exe exited with code
  -2147450749 (0x80008083)"` reaches the status line;
- the program leaves successfully → nothing to report;
- the program is still alive when the window closes → it worked, and the watch
  ends without touching it.

The window is not arbitrary: the .NET host takes about **four seconds** to
print its complaint and leave, so a "did it die right away?" check would miss
exactly the case it was meant to catch.

One exception: `explorer.exe` is a shell dispatcher that returns a non-zero
code even when it opened the file, so its exit code is never read as a
failure. That covers both `OPEN URL` and a `.lnk` shortcut.

`ActionRunner::with_exit_watch(window)` sets the window yourself, which is
what the tests use to stay fast.

## Tests

```text
cargo test -p ss550-engine
cargo clippy -p ss550-engine --all-targets -- -D warnings
```

They cover the numbering, URL validation (including the `C:\…` and
`https://` cases), the JSON round-trip and version check, the missing-file
load, and the platform launch rules.
