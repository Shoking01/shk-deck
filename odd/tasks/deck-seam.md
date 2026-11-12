# Feature: The deck seam, a second deck, and the hardware-in-loop suite

**Objective.** Make the controller capable of driving more than one streamdeck
model: a `Deck` seam above a `Transport` seam, a second deck implementation to
prove it, and the conformance suite both must pass.
**Branch.** `feature/deck-seam`
**Delivery strategy.** `single-pr` — one branch, work-unit commits, one pull
request at the end. Precedent: M1 shipped as PR #10 with ten work-unit commits
and five native reviews. Slice boundaries: none; the whole branch is one PR.
**Test runner.** `cargo test --workspace` (root workspace: protocol, driver,
engine, probe and the new crates), `cargo fmt --all`,
`cargo clippy --all-targets -- --deny warnings`. The GUI and tray crates are
separate roots and are not touched by this feature.

## Specs

### S1 — The seam

> **The seam.** `Deck` (semantics) sits above `Transport` (bytes). A second deck
> differs in key count, geometry, image size, capabilities and input decoding
> *before* it differs in transport, so a transport-only seam would be recut at
> the first second device. The public handle stays non-generic (`Box<dyn Deck>`
> behind it), so `ss550-gui` and `ss550-engine` do not change shape.

> What `Deck` exposes: `key_count`, the row/column geometry, the image size and
> payload limit, the capability set (brightness, clear, sleep, dials), the input
> decoding, and **a typed error for every unsupported operation**. No silent
> no-op — the same rule M1 put on the command gate.

Accepted scope: a new pure crate `deck-api` (no `ss550` string anywhere in
it), holding `Deck`, `Transport`, `Geometry`, `ImageSpec`, `Capabilities`,
`KeyId`, `KeyEvent`, `ButtonState`, `DeckError` and `TransportError`.
`KeyId` is **0-based** (the index of a key in the deck's own order), because
that is what the driver's `Key::to_index()` already is; the UI keeps showing
1-based numbers and converts at the boundary.

### S2 — A second deck, not SS550 over memory

> The proof of that seam is
> a second implementation, and the simulator is it: not SS550-over-memory, but a
> profile with a different shape — fewer keys, a different image size, no
> brightness.

Accepted scope: a new crate `deck-simulator` with two implementations: the
SS550 wire emulated at the `Transport` level (so the real driver runs in CI),
and a second deck with **6 keys in 2 rows of 3**, **72×72** images and **no
brightness**, implementing `Deck` directly. An operation the second deck does
not support must return `DeckError::Unsupported`, never a silent no-op.

### S3 — One suite, two profiles, no accidental hardware

> **The suite.** One file, parameterized by profile, so the same cases run
> against the real deck and against the simulator. The `hardware-tests` feature
> gates every test that touches hardware and each one is `#[ignore]`d; a plain
> `cargo test` must never open the device.

Accepted scope: a new crate `deck-conformance` with `run(&mut dyn Deck) ->
Vec<CaseResult>`, deterministic cases only (no human in the loop): geometry
self-consistency, image spec sanity, out-of-range key rejected, valid image
accepted, oversized image rejected, `clear_all` / `clear_key` / `brightness`
honouring the declared capabilities, identity never panicking, and
`poll_events` never panicking. A hardware-only case (press a key, see it
reported) lives in the driver's hardware test, outside the shared suite.

### S4 — Three guards

> **The guards.** Three, and the last one already exists but goes blind:
>
> 1. The seam crate does not contain the string `ss550`.
> 2. The number 15 does not appear outside the SS550 provider.
> 3. The single-writer guard of M1 (`every_write_goes_through_the_gated_funnel`)
>    counts `.write(` in `connection.rs` alone. Moving the transport out of that
>    file would leave the guard blind while a second writer exists, so it is
>    extended to the whole crate.

Accepted scope for guard 2, narrowed to what this milestone can enforce: the
seam crate and the conformance crate contain no `15`, and the second profile's
geometry differs from the SS550's. The engine's `KeyNumber` still carries
`1..=15` and the window still says 15; both belong to M4, and the plan's
wording is amended to say so.

### S5 — What is not promised

> **Not promised.** A virtual HID device in CI. Windows has no such thing
> without a signed kernel driver; Linux has `uhid`. The suite runs at the
> transport seam, which proves the driver's logic, not Microsoft's HID stack.

### S6 — Done when

> **Done when** the conformance suite is green over two profiles — the SS550 and
> one with a different key count and image size — the three guards are green,
> and `cargo test --features hardware-tests -- --ignored` has run against a real
> deck on a dedicated test unit.

### S7 — Blocked on hardware

> **Blocked on hardware.** Everything above except the run against the real deck
> needs no device. That last line needs the deck on a unit that is not the daily
> driver.

The hardware run is the one item this milestone cannot close on its own. It
stays open, with the exact command recorded, until a dedicated unit exists.

### S8 — The engine stops knowing what a key is

> - `KeyNumber::key()` falls back to `BottomLeft` on an out-of-range index
>   (`crates/ss550-engine/src/key.rs`). It is unreachable while the range is
>   1..=15; with a 6-key deck it becomes reachable and paints the wrong key,
>   silently.

Accepted scope: `KeyNumber::key()` and `KeyNumber::from_key()` are removed, the
engine's dependency on `ss550-driver` goes with them, and the silent
`BottomLeft` fallback dies with them. `KeyNumber` keeps its 1-based,
1..=15 range for now; making it per-deck is M4's work.

### S9 — The window does not move in this milestone

The window keeps `KEY_COUNT = 15`, its 5-wide grid and its visible strings.
M4 owns the deck-agnostic window. This milestone must not touch
`crates/ss550-gui/` or `crates/ss550-tray/`.

### S10 — House rules

Rust 2021, MSRV 1.87+, `#![deny(unsafe_code)]`, `thiserror` in libraries,
`no .unwrap()` in production paths, doc comments with examples on every public
item, `cargo fmt` and zero clippy warnings. Artifacts in English.

## Tasks

| ID | Task | Specs | Route | Status | Commit |
| --- | --- | --- | --- | --- | --- |
| T1 | `deck-api`: the seam crate — `Deck`, `Transport`, the data types, the typed errors, the guards | S1, S4, S10 | inline | pending | — |
| T2 | `deck-conformance`: the shared suite and its own stub-deck tests | S3, S10 | inline | pending | — |
| T3 | `ss550-driver`: the SS550 as a `Deck` over a `Transport`, the crate-wide writer guard, the hardware test behind the feature | S1, S3, S4, S6, S7, S10 | delegated | pending | — |
| T4 | `deck-simulator`: the SS550 wire fake and the 6-key deck, both passing the suite | S2, S3, S10 | delegated | pending | — |
| T5 | `ss550-engine`: drop the driver's `Key` and the silent fallback | S8, S10 | inline | pending | — |
| T6 | Docs: the plan's guard 2 amendment, the changelog, the crate READMEs | S4, S10 | inline | pending | — |
| T7 | Seam check: the whole suite, fmt, clippy, and the per-commit assessment | S6 | inline | pending | — |

Route note: T3 and T4 are two independent units of behaviour with disjoint edit
surfaces (`crates/ss550-driver/**` and `crates/deck-simulator/**`), each of
several files, launched together — the parallelism reason of the writer
trigger. T1 and T2 are the contract both of them code against, so they are
written by the parent first.

## Log

L1 - 2026-11-11, the user's request, verbatim:

> La idea es que sirva para varias streamdecks y no solo la SS550. Como afecta esto a M3?

> Si, procede

> Continua

> Vamos con M3

L2 - 2026-11-11. The plan was rewritten first (commit `8f7fb19`): M3 now
builds the seam, M4 is the deck-agnostic window and M9 is a real second deck.
That rewrite is the authority for S1-S7; this document turns it into tasks.

L3 - 2026-11-11. Guard 2 is narrowed to what M3 can enforce. The engine's
`1..=15` and the window's 15 are M4's; the plan's wording is amended in T6 so
the document does not promise a guard this milestone cannot deliver.

L4 - 2026-11-11. Delivery strategy cached as `single-pr`, on the precedent of
M1 (PR #10, ten work-unit commits). No chain question is asked.

L5 - 2026-11-11. The engine's `KeyNumber::key()` fallback is the only live
bug in this list, and `from_key` / `key()` are used by the engine's own tests
only — the window never calls them. Removing them is contained and kills the
engine's dependency on the driver.
