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
| T1 | `deck-api`: the seam crate — `Deck`, `Transport`, the data types, the typed errors, the guards | S1, S4, S10 | inline | done | `257cb99` + `9b89741` (review approved, authority burned) |
| T1b | Seal the seam contract before anything consumes it: the missing `dials` capability, a `sleep` operation for the capability that advertises it, a bounded brightness with a typed refusal, a geometry that cannot contradict itself, one timeout unit across the seam, a guard that covers the whole crate, and behaviour tests for the handle | S1, S4, S10 | inline | done | `70d19aa` + `727e220` + `44b5c73` (review approved, authority burned) + `93ad96c` (review findings) |
| T2 | `deck-conformance`: the shared suite - ten cases, none of which paints a key, each falsified by exactly one stub flaw and checked in both directions - and its own no-device/no-key-count guard | S3, S10 | inline | done | `5c5cdf6` (review approved, authority burned) + `425d3a5` (review findings) |
| T3 | `ss550-driver`: the SS550 as a `Deck` over a `Transport` - including the image adapter (the driver takes an encoded `ButtonImage`, the seam takes raw bytes plus a key), which must refuse the key range and then the image length before it builds or decodes anything, because the suite asserts both refusals and their numbers - and declaring `clear_key`/`sleep`/`identity` as typed refusals where the verified registry forbids them - plus the crate-wide writer guard and the hardware test behind the feature | S1, S3, S4, S6, S7, S10 | delegated | done | `f13d403` |
| T4 | `deck-simulator`: the SS550 wire fake and the 6-key deck, both passing the suite | S2, S3, S10 | delegated | done | `c4bfd6e` |
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

L6 - 2026-11-11, T1. The seam crate landed as `257cb99`. The native review
(`review-aaf606ba4eea87f0`, three lenses) refuted one CRITICAL finding:
**R3-seam-hardcodes-15** — the seam's own doc examples carried the number 15
in four places and no guard forbade it, which is exactly what S4 forbids. The
finding was correct: the commit violated the spec it was written from.

L7 - 2026-11-11, T1 correction. The guard was added first and observed
**failing** against the old examples (`the_seam_never_assumes_a_key_count`,
exit 101), then the examples were changed to a neutral eight-key shape and the
guard went green. Correction committed as `9b89741`, exactly 24 changed lines
(the declared correction plan). The targeted validator approved it and the
review authority was burned (`gentle-ai.review-acknowledged/v1`).

L8 - 2026-11-11, T1b opened. The review's advisory findings are not a reason
to re-run the review, but three of them are holes in the contract **before**
anything consumes it, and one of them (no `dials` capability, a `sleep` flag
with no `sleep` operation) is an unmet spec item — S1 names both. A seam is
the one contract every crate above it codes against; sealing it now costs one
commit, sealing it after T2, T3 and T4 costs three. T1b fixes: the capability
set against the trait's operations, the brightness range, the geometry that
can contradict itself, the two timeout units, the guard's real scope, and the
missing behaviour tests for the handle.

L9 - 2026-11-11, T1b. Two commits: `70d19aa` (the contract) and `727e220`
(the guards). Evidence: the API did not exist before the change (the test
run failed to compile — RED), then `cargo test -p deck-api` went green with
9 handle tests, 2 guard tests and 43 doctests, and
`cargo clippy -p deck-api --all-targets -- --deny warnings` is clean. Two
guards were proved, not assumed: a device name leaked into the crate
manifest on purpose made the widened guard fail (exit 101), and a
deliberately mis-forwarded `clear_key` made the handle test fail before it
was put back.

L10 - 2026-11-11, T1b size note. The contract commit is ~670 changed lines,
over the ~400 heuristic. The reason is the house rule: every public item in
a contract crate carries a doc example, and the change is one API migration
that cannot be split without leaving the crate not compiling in the middle.
The heuristic is advisory and the delivery strategy is `single-pr`, so the
branch carries it as one reviewable unit instead of being carved into
artificial pieces.

L11 - 2026-11-11, T3 read before writing. Reading the driver's real surface
changed two things about T3, and both are recorded here because the task row
no longer describes the work:

1. **The seam's `set_key_image(key, jpeg)` cannot wrap the driver's
   `set_key_image(ButtonImage)`.** The driver takes an already-encoded
   `ButtonImage { jpeg, key }` and the seam takes raw bytes plus the key. The
   adapter is not a forwarding shim: it must build the image (or refuse a
   payload the deck would not accept), and `DeckError::ImageTooLarge` /
   `ImageWrongSize` are exactly the typed refusals the seam promised. This is
   the seam's own example made real, not an extra.
2. **Three capabilities the seam can advertise are refused by the protocol
   registry today**: `ClearKey`, `Sleep` and `FirmwareVersion` are
   `Unverified`, and `ensure_verified` refuses them before the bytes leave the
   host. So the SS550 provider must declare `clear_key: false`,
   `sleep: false` and answer `identity()` with a typed refusal rather than
   pretend. That is the milestone's own rule ("no silent no-op") biting the
   driver, and it is the honest shape: the deck cannot do what was never
   proven safe.

L12 - 2026-11-11, delegation topology corrected. The plan said T3 and T4 run
in parallel as two delegated writers, and the refuter would run in the same
child context. Probed: a subagent cannot see the `review-*` agents at all (a
direct `rust-core` dispatch reports the only visible subagent is a generic
one), and the orchestrator's `task` allowlist names `rust-core` and
`rust-ui` but no review agent for a child to call. So the refuter stays a
top-level dispatch owned by this orchestrator. T3 and T4 remain two units
with disjoint edit surfaces and can still run together; only the review-role
part of the plan was wrong.

L13 - 2026-11-11, T1b review closed and what it left behind. The review ran
three lenses (risk, readability, reliability) plus a refuter. The refuter
refuted both CRITICAL findings, the review came back approved, and the
authority is burned on target `sha256:b6c2862a...`. Two of the WARNING
findings were real and are fixed in `93ad96c` rather than deferred:

1. `deck_handle_forwards.rs` was named "forwards every call" and skipped
   `image_spec` and `capabilities`, and its stub could never refuse, so the
   error path through the handle was asserted nowhere. The stub now refuses
   every fallible method with its own named operation, and both directions
   have a test.
2. `Geometry::new` had no edge coverage: zero keys, zero columns, and a key
   count that does not divide evenly. `geometry_edges.rs` pins all three,
   plus the invariant that every declared key lands inside the declared grid.

Fixing them required `PartialEq`/`Eq` on `DeckError` and `TransportError`, so
the tests compare values instead of strings. Additive, no existing shape
changed. The remaining WARNING finding (the value bound is not enforced in the
trait's default methods) stays informational: enforcement belongs to each
provider, and the conformance suite in T2 is where a provider that silently
clamps gets caught.

L14 - 2026-11-11, T2 landed and the second review closed. `deck-conformance`
is in the workspace with ten cases and, next to it, the scaffolding the two
delegated writers need (`deck-simulator` joins the workspace with a
placeholder body; the driver declares `deck-api`, `deck-conformance` and a
`hardware-tests` feature that is off by default). The placeholder is
deliberate and transient: T4 replaces it in the same milestone.

Two things about the suite are worth recording because they constrain T3/T4:

1. **No case paints a key.** The same suite runs against a device on a desk,
   so it only makes calls it expects to be refused. `deck-simulator` and the
   driver's own tests are where a successful paint is exercised.
2. **The cheap check comes first.** A deck must refuse an out-of-range key
   before it looks at the payload, and must refuse an over-limit image
   before it decodes one. Both cases assert the exact error including the
   numbers in it (`key_count`, `max_bytes`), so a deck carrying another
   shape's constants inside its refusals is caught rather than trusted.

The review of the T2 candidate (`review-43c1668c5eee6bdc`) approved with no
blocker and no refuter; `425d3a5` answers its three real findings. The rest
were accepted as they stand, and both suggestions are recorded here so they
are not rediscovered later:

- The filesystem scan in each crate's genericity guard is duplicated, and
  sharing it would mean a test-only crate for one function. Left as it is;
  two copies that must both stay correct is cheaper than a crate that exists
  to hold forty lines.
- The guards read files best-effort as UTF-8 and skip anything else, so a
  non-UTF-8 file could hide a device name. No file in either crate is
  non-UTF-8, and the guard is a tripwire against a careless edit, not an
  adversarial control.

L15 - 2026-11-12, T3 and T4 landed, in parallel, on disjoint surfaces. T3 put
the device behind `deck_api::Transport` and added `Ss550Deck`; the gate did
not move — every byte still leaves through `write_pack` after `ensure_verified`,
and the crate-wide guard now scans every source file instead of one, asserting
exactly one physical write (in the transport) with exactly one call site into
it (in the funnel, after the check). T4 added the 6-key deck and the SS550 wire
fake at the transport level.

Two facts about the run are worth keeping:

1. **The T3 writer died at the end, not at the beginning.** Its session hit a
   provider usage limit after the code was already written and passing, so it
   never filed its report. Its work was verified by the parent instead: the
   whole crate compiles, `cargo test -p ss550-driver` is green, clippy is clean
   on a fresh build, the hardware-gated target compiles, and all three
   workspace roots (repo, gui, tray) build. The lesson is the procedure, not
   the code: a writer that dies leaves work in the tree, and that work has to
   be read and checked, not assumed.
2. **`crates/ss550-gui/Cargo.lock` changed**, and it is not a gui change. The
   gui is a separate workspace root that depends on the driver by path, so
   recording the driver's new path dependency is cargo's job, not an edit to
   the window. M3 touches no gui code; the lock moved because the graph under
   it moved.

T3's risk line is **item 5 (dependencies)**: the driver's manifest gained
`deck-api` and, as a dev-dependency, `deck-conformance`. Both are path
dependencies inside this workspace — no external crate and no version change —
but the manifest changed, so the line is declared and an independent verifier
runs over the diff before the native review.
