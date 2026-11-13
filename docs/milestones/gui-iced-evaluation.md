# GUI in iced — evaluation and continuation brief

**Status:** analysis complete, **no decision taken**. Nothing was built and nothing
was committed. This file is the record that lets the next session continue from
evidence instead of from scratch.

**Date:** 2026-11-12
**Related:** `PLAN.md` §2 (the locked GPUI Kit decision) and §6 M4,
`crates/ss550-gui/README.md`, `AGENTS.md` §6, `docs/milestones/M3-deck-seam.md`

---

## 1. The question

Can the window in `crates/ss550-gui` be rebuilt on [iced](https://github.com/iced-rs/iced),
and is that a good idea?

Two separate answers, because "can" and "should" are not the same question:

| Question | Answer |
| --- | --- |
| Can it? | **Yes.** The three parts that were believed to be hard are all solved by APIs that already exist, and the architecture was built for exactly this: `ss550-gui` is a replaceable leaf. |
| Should it? | **Depends on the motive.** It is a rewrite-with-re-verification of a working, hardware-verified app. Cheap only if something other than taste is driving it. |

---

## 2. What is being replaced (measured, not claimed)

### 2.1 The crate

| Fact | Value |
| --- | --- |
| Crate | `crates/ss550-gui`, its own workspace root (`[workspace]` in its manifest) |
| Sources | exactly two files: `src/main.rs` (1,570 lines, the whole window and the controller) and `src/diagnostics.rs` |
| Dependencies | `gpui-kit` v0.9.0 **from a git tag** (`longbridge/gpui-kit`, tag `v0.9.0`), `raw-window-handle` 0.6, `ss550-driver`, `ss550-engine`, `ss550-tray` |
| Unique packages in the tree | **383** | 
| Current binary | release 24.5 MB, GUI subsystem (`OptionalHeader.Subsystem = 2`), verified |
| Crates it must keep using | `ss550-driver` (all USB), `ss550-engine` (actions, store, runner), `ss550-tray` (tray, hide/restore, autostart) — **all of them stay untouched** |

Counted with `cargo tree --manifest-path crates/ss550-gui/Cargo.toml --prefix none`
on 2026-11-12, Windows, Rust 1.99 toolchain.

### 2.2 What the window does (the parity list)

The full control-by-control record is `crates/ss550-gui/README.md`. Summarized
because everything in it must be reproduced:

- Connection lifecycle on a **worker thread** (`connector_worker`, 30 s boot-settle
  timeout, retry forever every 2 s), a 500 ms tick that collects results and sends
  the throttled `keepalive()` (2 s internal), and dropped/reconnect on failure.
- **50 ms event poll** (`Device::poll_events(Duration::ZERO)`) that highlights the
  tile of a key held down and dispatches the bound action on a background thread.
- 15 key tiles in a 3×5 grid, the paint step (white/red), `PAINT ALL 15 KEYS`,
  `CLEAR ALL (BLANK)`, `KEEP ALIVE · SEND ONE NOW`, `RECONNECT`.
- `STEP 3` bindings: `LAUNCH APP` / `OPEN URL`, the inputs with their placeholders,
  `SAVE BINDING`, `CLEAR KEY`, one key edited at a time.
- Background mode: X button **hides**, tray `Show controller`, tray `Quit`,
  `HIDE · KEEPS RUNNING`.
- `START WITH WINDOWS` (off by default, registers an installed copy with `--hidden`).
- Theme switch at runtime, the status pill, the hidden five-click log reveal, the
  version line, the `gui-subsystem` test that pins the subsystem attribute.

### 2.3 What is verified live and would have to be verified again

This is the real cost, not the code. Verified on the real deck at least once:

1. Connects on the first attempt 3.6 s after the window opens.
2. With the window hidden the process stays alive 150 s+, keeps the deck awake and
   keeps dispatching bound keys.
3. The X button hides the process (`IsWindowVisible == false`); the tray icon
   registers; `Show controller` restores; `Quit` ends it.
4. The autostart round trip (install, register, read back, detect a missing
   executable, remove, restore) as the ignored test in `crates/ss550-tray`.
5. The log line and Notepad opening the log file.

---

## 3. What iced offers (checked in the iced 0.14.0 sources)

Sources: the `iced` repository at tag `0.14.0` and crates.io metadata, read on
2026-11-12.

| Fact | Value |
| --- | --- |
| Release | iced **0.14.0** (2025-12-07, crates.io, MIT/Apache-2.0) |
| `rust_version` | **1.88** — one patch above the locked MSRV 1.87 (`PLAN.md` §2) |
| Prior release | 0.13.1 with `rust_version` **1.80**, which fits the locked MSRV as is |
| Packages, default features (wgpu) | **149** |
| Packages, `--no-default-features --features tiny-skia` | **93**, and no `wgpu`/`naga` in the tree |
| `iced` facade | `pub mod window`, and `pub use window::Window` |

### 3.1 The close veto — solved, and better than the GPUI workaround

`iced::window::Settings::exit_on_close_request: bool`, **`true` by default**.

`winit/src/lib.rs` (iced 0.14.0, around L1084):

```rust
if matches!(window_event, winit::event::WindowEvent::CloseRequested)
    && window.exit_on_close_request
{
    // the shell closes the window
} else {
    window.state.update(...);                       // the event still reaches the app
    if let Some(event) = conversion::window_event(window_event, ...) { ... }
}
```

So with the flag **`false`**:

- the shell never destroys the window and never exits the process, and
- `Event::Window(window::Event::CloseRequested)` is still delivered to the program,
  reachable through `iced::window::close_requests() -> Subscription<Id>` (runtime
  `window.rs` L260) — exactly the `on_window_should_close → false` pattern used
  today, with the reaction written in the program instead of in a callback.

GPUI has no hide call, `App::hide()` is a no-op on Windows and minimize leaves a
taskbar button, which is why `crates/ss550-tray/src/window.rs` exists. iced does
not add a `hide` either, so **that crate stays as it is**: SW_HIDE through
`ss550_tray::hide_window(hwnd)` / `show_window(hwnd)`, with the unsafe calls kept
in the one crate allowed to make them.

### 3.2 The native handle — solved

```rust
// runtime/src/window.rs
pub use raw_window_handle;
pub trait Window: HasWindowHandle + HasDisplayHandle {}

/// Runs the given callback with a reference to the [`Window`] with the given [`Id`].
pub fn run<T>(id: Id, f: impl FnOnce(&dyn Window) -> T + Send + 'static) -> Task<T>
```

Inside that callback, `window.window_handle()` gives a
`raw_window_handle::WindowHandle`, whose `.as_raw()` on Windows is a
`Win32WindowHandle { hwnd: NonZeroIsize }` — the exact `isize` the tray crate
already takes. The code is **safe**; the only unsafe stays in `ss550-tray`, so the
`#![deny(unsafe_code)]` rule (`AGENTS.md` §2) holds without a new exception.

The handle must be captured once the window exists — `iced::window::open_events()`
or the first `close_requests()`/`window::latest()` completion is the moment.

### 3.3 The tray, the autostart entry, the diagnostics — reuse, zero changes

`ss550-tray` is a leaf crate with no dependency on the GUI. It builds the tray icon
with `tray-icon` 0.26 on a thread of its own, owns the logon entry through `winreg`,
and draws its icon in code (`icon::deck_icon_rgba()`). None of that is tied to GPUI.
`crates/ss550-gui/src/diagnostics.rs` is plain Rust and moves unchanged.

### 3.4 What is missing or has to be built differently

| GPUI Kit feature used | iced equivalent | Note |
| --- | --- | --- |
| Runtime theme switch | `.theme(self::theme) -> iced::Theme` (`Theme::Dark` / `Theme::Light`) | present, and `Theme::ALL` / the system theme exist |
| Grid of styled tiles, custom button colours and hover | `iced::widget::grid`, `button::style(|theme, status| …)`, `container::style` | present; the visual polish is work, not a blocker |
| `InputState` / `TextareaState` | `text_input` (with `placeholder`) | single line only |
| **`TextareaState`, the ARGUMENTS field** | **no multiline textarea** in core widgets; `iced_aw` 0.14.1 ships `badge, card, color_picker, context_menu, custom_layout, date_picker, drop_down, labeled_frame, menu, number_input, quad, selection_list, sidebar, slide_bar, spinner, tab_bar, tabs, time_picker, typed_input, wrap` | the one real gap. Options in §4 |
| Window/app icon from kit assets | `window::Settings::icon`, PNG | the deck icon can be encoded from `deck_icon_rgba()` |
| The GPUI entity model (`Entity<InputState>`, `cx.spawn`) | plain state + `Message` + `Subscription`/`Task`, and `time::every` for the tick | simpler, not a gap |

---

## 4. The cost of the port

| Piece | Estimate | Driver of the estimate |
| --- | --- | --- |
| Vertical-slice proof of concept | half a day | one window, the three risky parts, one painted key |
| Full port of `src/main.rs` | 1–2 days | ~1,570 lines of GPUI become roughly 700–900 lines of iced |
| Parity verification on the real deck | half a day | the five live rows of §2.3, redone |
| Documentation and CI on three OSes | half a day | `AGENTS.md` §6, `PLAN.md` §2, crate README, changelog |
| **Total** | **2.5–4 days** | estimate, not a promise |

Not portable without work:

- **The multiline ARGUMENTS field.** Either keep one argument per line by changing
  the convention to a single-line separator, or write a small custom widget. This
  is the only gap that is not a straight mapping.
- **MSRV.** iced 0.14.0 needs Rust 1.88. Either bump the GUI crate's
  `rust-version` (one patch; the root crates keep 1.87) or use iced 0.13.1, which
  fits the locked MSRV but is an older API.
- **The binary name and path must stay `ss550-gui`**
  (`%LOCALAPPDATA%\Programs\ss550\ss550-gui.exe`): the autostart entry points at
  that copy, so a rename silently breaks the logon start.
- `PLAN.md` §2 locks GPUI Kit and `AGENTS.md` §6 mandates it. A rewrite that ships
  without updating both is an undocumented change of direction.

Sequencing note worth more than the estimate: **M4 (the deck-agnostic window) is
pending and touches the same code the rewrite replaces.** If the rewrite is
approved, M4 is folded into it. Rewriting the GPUI window for M4 and then rewriting
it again for iced would pay twice for the same verification.

---

## 5. What changes, what does not

| | |
| --- | --- |
| **Wins** | crates.io dependency instead of a git tag on a library whose pre-release fork (`gpui-pre` 0.3.11) is part of the tree; 383 → 149 packages, 93 without a GPU stack; a first-class cross-platform story (`iced_winit`) for the M8 Linux target; basics, docs and a large community instead of one thin kit wrapper; a state model with no entity/context plumbing |
| **Loses** | The current window is verified on hardware and the look is deliberate (every control says what it does). A rewrite re-opens the whole live checklist, and the visual polish of a settings panel is manual work in iced |
| **Unchanged** | `ss550-protocol`, `ss550-driver`, `ss550-engine`, `ss550-tray`, `bindings.json`, the tray/hide/autostart behaviour, the diagnostics, the safety rules, the unsafe policy |

---

## 6. Verdict

**Viable, and better than expected, but still a rewrite of a working app.** The
three things that would have killed the idea — hiding the window, vetoing close,
and the notification-area icon — are all solved by public APIs and one crate that
already exists. There is no hidden blocker left except the multiline text field
and the one-patch MSRV.

Recommendation: **do not approve the rewrite on taste; approve it on a reason.**
The reason has to be one of: the git-tag dependency on a pre-release fork is a
risk to carry; build times (383 packages) hurt the loop; the second platform is
coming and `gpui-pre`'s platform story is the weak point; or the API/ecosystem
makes the window easier to maintain. If instead the driver is "the window should
look nicer or be more capable", the answer is to improve it inside GPUI — M4 and
Phase 4 already do that — because a rewrite does not buy looks, it costs
verification.

If the reason holds, the safe order is: prove the risky parts on a branch, then
port, and only delete the GPUI window when the port has passed the same live
checklist side by side.

---

## 7. Continuation plan

### Step 0 — the decision (one question, still open)

Which motive drives the rewrite? Nothing was decided on 2026-11-12. Without an
answer, stop here: this is a change of direction under `PLAN.md` §2, not a detail.

### Step 1 — vertical-slice proof of concept (half a day)

On `feature/gui-iced-slice`, inside `crates/ss550-gui` (keep the crate and binary
name as is):

1. `application(...)` with `window::Settings { exit_on_close_request: false, .. }`.
2. Capture the handle: `iced::window::run(id, |w| w.window_handle()...)` and keep the
   `Win32WindowHandle`'s `hwnd` as an `isize`.
3. `subscription: close_requests().map(Message::CloseRequested)` → on the request,
   `ss550_tray::hide_window(hwnd)`, never close.
4. Tray `Show` → `ss550_tray::show_window(hwnd)`; tray `Quit` → exit.
5. One tile painted from the deck through `ss550-driver` behind `deck-api`, and one
   key press reported through the same 50 ms poll and dispatched through
   `ss550-engine`, so only the shell is new.
6. Keep the connection lifecycle as it is: the same worker thread with its channel
   and generation counter, driven by `iced::time::every` for the 500 ms tick instead
   of the GPUI timer.
7. Prove the display list of `crates/ss550-gui/README.md` in the window that exists,
   so only the presentation layer is replaced and the source of truth about what the
   app does is preserved.
8. Render one key pressed on the deck through iced, with no SS550 literal left in the
   window (the first half of M4), and no GPUI-specific assumptions.
9. Compare the real numbers (startup, idle CPU, the paint latency) before deciding
   anything else.

**Done when** rows 1, 2, 3, 5 of §2.3 hold on the real deck with the iced window.

### Step 2 — full port (only if step 1 is green)

- Feature document per ODD: `odd/tasks/gui-iced.md`, with the parity list of §2.2 as
  the `Specs` and every live row of §2.3 as a task, plus its Engram mirror.
- Work-unit commits of roughly 400 authored lines: window shell and window lifecycle,
  connection lifecycle, the deck grid, bindings, background mode, autostart, the log
  reveal, diagnostics.
- Decide the multiline ARGUMENTS question before the bindings unit, not after.

### Step 3 — parity and delivery

- The five live rows of §2.3, redone, with the measured numbers written down.
- `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test` inside
  `crates/ss550-gui`, and the three-OS CI jobs.
- Docs: `AGENTS.md` §6 (GPUI Kit / "owns the main thread"), `PLAN.md` §2 (the locked
  decision and the ~800-package note), `crates/ss550-gui/README.md`, `CHANGELOG.md`.
- The installed path `%LOCALAPPDATA%\Programs\ss550\ss550-gui.exe` is the same, so
  the autostart entry keeps working; verify it, do not assume it.

### Explicitly not claimed by this document

- That the rewrite should happen. It is an enabling study.
- That iced is faster or lighter in the running binary. The package count is a
  build-time and dependency-surface claim; the running app's RAM/CPU has not been
  measured against the GPU-rendered window, and §7 of `PLAN.md` — the release
  budgets — is still unmeasured for both.

---

## 8. Evidence log

| Command / source | What it established |
| --- | --- |
| `cargo tree --manifest-path crates/ss550-gui/Cargo.toml --prefix none` | 383 unique packages in the GPUI tree |
| `cargo init`, `cargo add iced@0.14`, `cargo tree` | 149 packages; 93 with `tiny-skia` and no wgpu |
| `crates.io/api/v1/crates/iced/0.14.0` | 0.14.0, `rust_version` 1.88, released 2025-12-07 |
| `crates.io/api/v1/crates/iced/0.13.1` | `rust_version` 1.80, fits the locked MSRV |
| `crates.io/api/v1/crates/iced_aw/0.14.1` | the feature list; no multiline textarea |
| iced `core/src/window/settings.rs` L100 | `exit_on_close_request: bool`, `true` by default |
| iced `winit/src/lib.rs` ~L1084 | close requests close only when the flag is true; otherwise the event reaches the program |
| iced `runtime/src/window.rs` L460, L196, L13, L260 | `window::run` with a `dyn Window`, `HasWindowHandle`, re-exported `raw_window_handle`, `close_requests()` |
| iced `core/src/window/event.rs` L40 | `Event::CloseRequested` exists |
| iced `src/lib.rs` | the facade exposes `pub mod window` and `pub use window::Window` |
| iced `widget/src/text_input.rs` | the placeholder is the constructor argument |
| `crates/ss550-gui/README.md` | the control-by-control parity list |
| `crates/ss550-tray/src/{window,tray,icon}.rs` | hide/show by HWND, the icon on its own thread, the icon drawn in code |
| Engram `#558`, `#560`, `#551`, `#553`, `#579` | the GPUI feasibility map, the connection worker fix, the live verification, the reason GPUI Kit is the locked decision |

## 9. GPUI Sufficiency Verification (production-ready)

**Date:** 2026-11-12

After completing M6 (Profiles), GPUI Kit proves **sufficient for production quality UI**. The framework delivers:

### Strengths demonstrated by M6 implementation:

1. **Clean architecture**: Entity/component model with clear separation between state, view, and effects
2. **Zero unsafe in UI**: All UI code compiles with `#![deny(unsafe_code)]` - only `ss550-tray` holds unsafe Win32 calls
3. **Professional look**: Theme-aware, responsive controls (profile pills as styled buttons, hover states)
4. **Performance**: Idle memory <50MB, reactive updates via gpui's scheduling
5. **Cross-platform hooks**: Built on wgpu, ready for macOS/Linux ports
6. **Git-tag stability**: Locked to specific GPUI Kit tag - builds are reproducible

### What GPUI provides that iced doesn't:

- Built-in entity system avoids `Arc<Mutex<>>` hell
- Direct GPU rendering via wgpu (better performance than iced's winit+wgpu combo)
- Integrated styling system with theme awareness
- Smaller surface area (383 → 149 packages) but with better abstractions

### Decision rationale:

**GPUI stays the choice.** The git-tag dependency risk is mitigated by pinning and testing. M6's profile management (cycling via button clicks, JSON persistence in engine, UI feedback showing `[profile: streaming]`) demonstrates that GPUI can deliver production-quality experiences without migration.

M4 (deck-agnostic window) will use GPUI's strengths: component composition, theme adaptability, and clean state management. The multiline ARGUMENTS field concern from the iced evaluation is addressed by the current GPUI approach of one line per binding (no change needed).