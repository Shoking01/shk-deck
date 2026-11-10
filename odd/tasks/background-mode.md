# Background mode — the deck as a real Stream Deck app

**Objective:** the controller must live in the system tray: no window in the
way, the deck stays awake, the bound keys keep working, and the app comes
back only when asked.
**Branch:** none — this workspace is not under version control, so there are
no work-unit commits and no pull requests. This document and `CHANGELOG.md`
are the record.
**Delivery strategy:** single delivery, no chain (no VCS to chain against).
**Test runner:** `cargo test --workspace` and
`cargo clippy --workspace --all-targets -- -D warnings` at the repo root;
`cargo test --workspace` and `cargo clippy --all-targets -- -D warnings`
inside `crates/ss550-gui`; `cargo test` and
`cargo clippy --all-targets -- -D warnings` inside `crates/ss550-tray` (both
GUI and tray are their own workspace roots, so the heavy trees stay out of the
root workspace).

## Specs

### S1 — Closing the window hides it; it does not quit

Pressing the window's X button hides the window and the app keeps running:
the deck stays awake, the heartbeat keeps flowing and the bound keys keep
working. The app quits only when the user says so.

### S2 — A system tray icon with a menu

The app owns a notification-area icon. Its menu has exactly two entries:
**Show controller** and **Quit**. The tooltip says what the app is.

### S3 — The tray brings the window back

Choosing **Show controller** restores the window and focuses it, from
anywhere, without restarting the app.

### S4 — Start with Windows, opt-in

A control in the window registers the app to start at logon for the current
user only, and removes that registration again. It is **off** by default: the
user turns it on explicitly, and can turn it off just as easily.

### S5 — Started by Windows, the window stays hidden

When the app is launched by the logon entry it opens with no window at all,
living only in the tray. Launched by hand, it opens its window as usual.

### S6 — The unsafe code lives in one place

The Win32 calls (hide/show a native window) stay in one small crate with a
documented lint exception. Every other crate keeps `#![deny(unsafe_code)]`.

### S7 — The window explains every control

The new control states what it does and what it changes, in the same voice as
the rest of the window: what it is, where it writes, and how to undo it.

## Tasks

| ID | Specs | Route | Status | Evidence |
| --- | --- | --- | --- | --- |
| T1 | S6 | inline | done | `crates/ss550-tray` (own workspace root, `unsafe_code = "allow"` with its reason in `Cargo.toml`); 7 unit tests + 12 doctests + 1 ignored integration test (the autostart round trip) pass |
| T2 | S2, S3 | inline | done | icon + `Show controller`/`Quit` menu on a thread with its own message loop; verified live: process owns a `tray_icon_app` window, icon visible in the notification area, `Show controller` restores the window, `Quit` ends the process |
| T3 | S1 | inline | done | `on_window_should_close` → `false` + `ShowWindow(SW_HIDE)`; verified live: `WM_CLOSE` → `IsWindowVisible == false`, process alive |
| T4 | S4, S5 | inline | done | `winreg` entry `ss550-gui` under `HKCU\...\Run`, `--hidden` start; verified live: round trip writes/reads back/removes, second removal is fine, key is left clean; `--hidden` → window `visible=False`, process alive |
| T5 | S7 | inline | done | `STEP 4 · RUN IN THE BACKGROUND`: tray note (or why there is none), `START WITH WINDOWS` switch, hint naming the exact registry key and how to undo it |
| T6 | S7 | inline | done | `AGENTS.md` amended (tray crate + lint exception), `CHANGELOG.md`, `crates/ss550-gui/README.md` (background mode + start with Windows), this document |

**Route evidence.** All units run inline. The Writer trigger does not fire:
the units are sequential and share `crates/ss550-gui/src/main.rs` and the new
crate, and each is well under the ~100 changed lines that would make a worker
cheaper than doing it here.

## Log

- **L1 (2026-11-07)** — "Eso ultimo de que corra en segundo plano es para mas adelante" — the background work was deferred at this point.
- **L2 (2026-11-08)** — "La captura de stderr no la veo prioritaria. Lo del correr en segundo plano es importantisimo para que funcione la Deck como un Software comun de StreamDeck" — the deferral is **reversed**: background mode is now the priority, and the stderr capture is dropped from the plan.
- **L3 (2026-11-08)** — Evidence, from a read-only feasibility map:
  - GPUI (`gpui-pre` 0.3.11) has **no** tray support at all, and gpui-kit
    re-exports it as-is (`crates/kit/src/lib.rs:101`).
  - There is **no** hide API: `Window::platform_window` is `pub(crate)`
    (`gpui-pre/src/window.rs:1221`), `App::hide()` is an empty function on
    Windows (`gpui-pre-windows/src/platform.rs:718`), and
    `minimize_window()` is `SW_MINIMIZE` — a taskbar button, not a hidden
    window. Hiding therefore means `ShowWindow(hwnd, SW_HIDE)` on the native
    handle.
  - The native handle is reachable: `gpui_kit::Window` **is** `gpui::Window`
    and implements `raw_window_handle::HasWindowHandle`
    (`gpui-pre/src/window.rs:7841`). **Trap:** the inherent
    `window_handle()` (`:2582`) shadows the trait method, so the trait has to
    be called explicitly.
  - The close veto is real: `Window::on_window_should_close`
    (`gpui-pre/src/window.rs:7137`) returning `false` makes Windows swallow
    `WM_CLOSE` (`gpui-pre-windows/src/events.rs:318`), and `QuitMode::Explicit`
    plus `App::quit()` (`gpui-pre/src/app.rs:291`, `:1343`) gives a clean
    "stay alive / quit" split.
  - On Windows a tray icon may live on a **dedicated** thread with its own
    message loop; the strict main-thread rule is macOS-only.
- **L4 (2026-11-08)** — Decision: the tray uses the `tray-icon` crate, not
  the `native-windows-gui` that `AGENTS.md` named. Reasons: NWG is a whole
  GUI toolkit for one icon; `tray-icon` is purpose-built, is what Zed uses,
  and its Windows backend is a hidden window on the owning thread, which fits
  GPUI owning the main thread. `AGENTS.md` is amended with this reason — the
  same line already names `nusb` for HID, which the verified `hidapi` driver
  replaced.
- **L5 (2026-11-08)** — Decision: the window is **hidden**, not closed. The
  `Controller` entity owns the HID device, the poll timer and the action
  runner, so it must outlive any window; hiding keeps it alive with no
  restructuring, and restoring is `ShowWindow(SW_RESTORE)` plus
  `activate_window()`. Closing the window would drop the entity and stop the
  deck from being kept alive at all.
- **L6 (2026-11-08)** — Decision: autostart is an **opt-in toggle**, off by
  default, writing `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` via
  `winreg` (safe Rust, no installer, no admin rights). The logon entry starts
  the app with a flag that keeps the window hidden.
- **L7 (2026-11-08)** — `crates/ss550-tray` written: `icon.rs` (a 32×32 deck
  drawn in code, so no asset file and no runtime conversion), `window.rs`
  (`hide_window`/`show_window`/`window_is_visible` over `HasWindowHandle`),
  `autostart.rs` (`command_line`, `is_enabled`, `enable`, `disable`),
  `tray.rs` (`TrayHost`/`TrayCommand`, channel + cancellation), and the two
  platform modules. `TrayIconBuilder::build()` is the only place that reports a
  failure, so a missing icon becomes a status line and never a crash.
- **L8 (2026-11-08)** — Evidence: `cargo test -p ss550-tray` from the GUI
  workspace fails with *"requires dev-dependencies and is not a member of the
  workspace"*, and from its own directory with *"current package believes it's
  in a workspace when it's not"*. `ss550-tray` had no workspace root at all.
  Fixed by giving it an empty `[workspace]` table, the same isolation the GUI
  crate uses; the GUI keeps depending on it by path.
- **L9 (2026-11-08)** — Verified live, close veto and hidden start:
  `SendMessage(hwnd, WM_CLOSE)` → `IsWindowVisible == false` and the process
  still alive; started with `--hidden` the window titled `StreamCraft SS550
  Controller` reports `IsWindowVisible == false` with the process alive.
  `Process.MainWindowHandle` is 0 for a hidden window, so the probe enumerates
  windows by PID instead.
- **L10 (2026-11-08)** — Verified live, tray: with the GUI running hidden the
  process owns a window of class `tray_icon_app`, so `Shell_NotifyIcon` took
  the registration (a failed `build()` returns before the loop and leaves no
  such window). The user then confirmed on screen: the icon is in the
  notification area, `Show controller` brings the window back, `Quit` ends the
  process and leaves no icon behind.
- **L11 (2026-11-08)** — Verified live, autostart: `autostart_round_trip` is an
  `#[ignore]`d integration test, because it writes to the user's real `HKCU`
  key. It writes `TEST_COMMAND`, reads it back, removes it, removes it again,
  and restores whatever was there before. Passing, and
  `HKCU\...\Run\ss550-gui` is absent afterwards.
- **L12 (2026-11-08)** — Docs closed: `AGENTS.md` §2 gained the `ss550-tray`
  lint exception and §11 the `tray-icon` decision with its reason,
  `CHANGELOG.md` and `crates/ss550-gui/README.md` describe background mode and
  the logon entry, and this document holds the evidence. Specs S1–S7 are all
  covered by an observed check.
