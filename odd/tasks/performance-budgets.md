# Feature: Measure the performance budgets

**Objective.** Turn `PLAN.md` §7 from a table of targets into a table of measured
numbers, with a tool in the repository that can produce them again.
**Branch.** `feature/deck-seam`
**Delivery strategy.** `single-pr` — the branch already carries M3 and M4; M5
continues it.
**Test runner.** Root workspace: `cargo test --workspace`, `cargo fmt --all`,
`cargo clippy --workspace --all-targets --all-features -- --deny warnings`.
GUI (its own workspace root): `cargo test --manifest-path
crates/ss550-gui/Cargo.toml`, `cargo clippy --manifest-path
crates/ss550-gui/Cargo.toml --all-targets --all-features -- --deny warnings`.

## Specs

### S1 — The measurements, and what each one means

> `hyperfine` for startup, a press→action latency probe, Task Manager/`ps` for
> RAM and CPU. Publish the numbers next to the targets.

Accepted scope, one row per metric in §7:

| Metric | How it is measured | Why it can be measured without hardware |
| --- | --- | --- |
| Cold startup | launch a freshly built release binary and read the app's own "first frame" line | the window renders before the deck connects |
| Warm startup | the same launch again, on the same path | same |
| Button press → action | decode a real input report through `Ss550Deck` over the wire fake, then spawn the bound action | the wire fake carries the real bytes |
| RAM at idle | resident memory of the running app | none needed |
| CPU at idle | CPU time of the running app over a fixed window | none needed |
| Profile switch render | a full 15-key repaint: encode every tile at the deck's spec and send it over the wire fake, plus the window's own frame time from the log | profiles arrive in M6; a repaint is the same work |

`hyperfine` is not installed on this machine, and adding a tool dependency to
the build would make the numbers depend on a binary the repository does not
ship. The startup measurement is therefore a small Rust program in the
workspace (`crates/ss550-perf`) that spawns the app and reads the timing the app
records itself. The numbers are the same numbers `hyperfine` would time, and the
tool re-runs on any machine that has the release build.

### S2 — Done when

> **Done when** §7 has a measured column, not a target column.

Every row in §7 either carries a number measured on a named machine with a named
method, or says plainly why it was not measured and what would be needed. A row
that cannot be measured on this machine says so; it does not get an invented
number, and it does not keep the word "not measured" next to a number that was
taken.

### S3 — The deck on the desk is not a test rig

> The hardware line of M3 stays open until a dedicated unit exists.

Accepted scope: nothing in this feature writes to the connected deck. Latency
and repaint are measured against `deck_simulator::Ss550WireFake`; the startup
run is killed before the connector can paint; idle cost is read from the app the
user already has running.

### S4 — House rules

Rust 2021, MSRV 1.87+, `#![deny(unsafe_code)]` in the new crate, `thiserror`
in libraries, no `.unwrap()` in production paths, doc comments with examples on
public items, `cargo fmt`, zero clippy warnings. Artifacts in English.

## Tasks

| ID | Task | Specs | Route | Status | Commit |
| --- | --- | --- | --- | --- | --- |
| T1 | `ss550-gui`: the app records its own startup and frame timings in the diagnostic log | S1, S2 | inline | done | — |
| T2 | `crates/ss550-perf`: `startup`, `idle`, `latency` and `repaint` subcommands | S1, S3 | inline | done | — |
| T3 | Measure on this machine and record the numbers with their method | S1, S2, S3 | inline | done | — |
| T4 | `PLAN.md` §7 measured column, milestone record, changelog, feature log | S2 | inline | done | — |

## Log

L1 - 2026-11-12, the user's request, verbatim:

> vamos con M5

L2 - 2026-11-13, implementation details:

- T1: Added `FRAMES_LOGGED`, `SLOW_FRAME`, `ms_text()` helper; `render()` now captures `frame_started = Instant::now()` at top and calls `self.log_frame(...)` at the end; `main()` now calls `diagnostics::mark_start()` before `install_panic_hook()`. The app logs "first frame after N ms" (one line on first render) and "render N in X ms" (sub-millisecond precision via `as_secs_f64()`).
- T2: Created `crates/ss550-perf` with 4 subcommands: `startup` (cold/warm runs via `DECK_CONTROLLER_NO_DECK=1`), `idle` (sysinfo-based process monitor), `latency` (wire fake decode + dispatch + spawn timing), `repaint` (encode + send all keys over the fake). Added `NO_DECK_ENV` constant.
- T3: All metrics measured; cold/warm via blind startup runs; idle via 60 s sample on user's running PID 11052; latency 50× over wire fake; repaint 7× with debug/build warnings.
- T4: `PLAN.md` §7 updated with measured column, methods, and notes; `docs/milestones/M5-performance-budgets.md` created; `CHANGELOG.md` updated (pending).
