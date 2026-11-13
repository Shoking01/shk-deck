# M3 — The deck seam, a second deck, and the hardware-in-loop suite

**Status:** complete, except the hardware line.
**Branch:** `feature/deck-seam`
**Date:** 2026-11-12
**Related:** `PLAN.md` §M3, `odd/tasks/deck-seam.md`, `CHANGELOG.md`

This file records the milestone as delivered, the audit that closed it, and the
one item that stays open. It is the reference for "what does M3 consist of and
what is still missing".

---

## 1. What the milestone asked for

The application is meant to drive more than one streamdeck model, so the driver
stops being *the SS550 driver* and becomes *a deck driver*. The proof is a
second implementation with a different shape: fewer keys, a different image
size, no brightness.

The seam has two levels:

| Level | Trait | Owns |
| --- | --- | --- |
| Semantics | `deck_api::Deck` | key count, geometry, image spec and payload limit, capabilities, input decoding, typed refusals |
| Bytes | `deck_api::Transport` | one report out, an input read with a timeout, a control read |

A second deck differs in key count, geometry, image size, capabilities and input
decoding **before** it differs in transport, so a transport-only seam would have
to be recut at the first second device. The public handle stays non-generic
(`Box<dyn Deck>` behind `DeckHandle`), so the window and the engine do not change
shape.

No silent no-ops: an operation a deck cannot do returns
`DeckError::Unsupported`, and a value outside the accepted range is refused
with `DeckError::ValueOutOfRange`, never clamped.

### Done when

1. The conformance suite is green over two profiles: the SS550 and one with a
   different key count and image size.
2. The three guards are green.
3. `cargo test --features hardware-tests -- --ignored` has run against a real
   deck on a dedicated test unit.

---

## 2. What was already in place

| Piece | Where | What it does |
| --- | --- | --- |
| The contract | `crates/deck-api` | `Deck`, `Transport`, `DeckHandle`, `Geometry`, `ImageSpec`, `Capabilities`, `KeyId`, `KeyEvent`, `ButtonState`, `DeckError`, `TransportError`. Leaf crate: `thiserror` is its only dependency, and it names no device and assumes no key count. |
| The suite | `crates/deck-conformance` | Ten deterministic cases, none of which paints a key, so the same suite is safe on a desk. Each case is falsified by exactly one stub flaw in `tests/the_suite_catches_a_bad_deck.rs`, checked in both directions. |
| The SS550 provider | `crates/ss550-driver/src/deck.rs` | `Ss550Deck`, a `Deck` over a `Transport`. 15 keys, 3×5, 100×100 JPEG up to 9,000 bytes, brightness, clear-all; `clear_key`, `sleep` and `identity` are typed refusals because the protocol registry marks them unverified. |
| The second deck | `crates/deck-simulator/src/six_key_deck.rs` | `SixKeyDeck`: 6 keys in 2 rows of 3, 72×72 JPEG up to 4,096 bytes, two dials, clear-key and clear-all, **no brightness and no sleep**. |
| The wire fake | `crates/deck-simulator/src/wire_fake.rs` | `Ss550WireFake`, a `Transport` that decodes real packs built with `ss550-protocol`, records a transcript, and can fail or stall on demand. |
| The engine | `crates/ss550-engine` | No dependency on the driver; `KeyNumber::key()` / `from_key()` and the silent `BottomLeft` fallback are gone. |

The three guards, all green:

1. **The seam names no device.** `crates/deck-api/tests/seam_is_generic.rs`
   scans every file in the crate for the device name.
2. **The seam and the suite assume no key count.** The same file plus
   `crates/deck-conformance/tests/suite_is_generic.rs` scan for the number 15;
   the second profile's geometry (6 keys, 3 columns, 72×72) differs from the
   SS550's.
3. **Exactly one physical write.** `every_write_goes_through_the_gated_funnel`
   in `crates/ss550-driver/src/connection.rs` asserts one `.write(` in the crate
   (in `hid_transport.rs`) and exactly one call site into it (the funnel in
   `connection.rs`, after `ensure_verified`).

---

## 3. What was missing, found by the audit

### 3.1 The wire fake was orphaned — the one real code gap

S2's accepted scope says: *"the SS550 wire emulated at the `Transport` level
(so the real driver runs in CI)"*. That was documented in three places —
`crates/deck-simulator/README.md`, `crates/ss550-driver/src/lib.rs` and the
changelog — but no test did it:

- `Ss550WireFake` was only ever fed by packs a test built by hand, in
  `crates/deck-simulator/tests/wire_fake.rs`;
- `deck-simulator` had no dependency on `ss550-driver`, so it *could not* put the
  driver on the fake;
- the driver's own conformance test used a second, local `FakeTransport`
  (`crates/ss550-driver/tests/deck_conformance.rs`), so the fake and the driver
  never met.

The consequence is not cosmetic: the fake decoded bytes the test wrote, which
proves the test and the fake agree. Only the real driver's upload, its single
wake, its `CLE 0xFF` and its input numbering can prove the fake speaks the wire
the application will speak.

**Closed by** `crates/deck-simulator/tests/ss550_deck_over_the_wire.rs` plus two
dev-dependencies (`ss550-driver`, `jpeg-encoder`). Four cases:

| Case | What it pins |
| --- | --- |
| paint | `Ss550Deck` wakes the deck once (`DIS`) and uploads a real 100×100 JPEG to firmware key 1; the fake records one `ImageTransfer { firmware_key: 1, bytes }` at the flush |
| brightness + clear | `Brightness { value: 40 }` and `ClearAll` reach the wire, and the clear reuses the wake — no second `DIS` |
| press | a real input report (`ACK` … `OK`, read key 1) decodes to `KeyId::new(10)`, so the driver's key numbering survives the seam |
| refusal | `sleep()` returns `DeckError::Unsupported` and the fake's transcript stays empty: the gate refuses before the bytes leave |

The test owns the fake through `Arc<Mutex<Ss550WireFake>>` (wrapped in a tiny
local `SharedWire` transport), because `Ss550Deck` takes ownership of its
transport and the transcript still has to be readable afterwards.

### 3.2 The second hardcoded geometry was still there

`Key::position()` in `crates/ss550-driver/src/keys.rs` computed
`(index / 5, index % 5)` — the SS550's five-wide grid written as a formula. It
had **no callers**, and the grid is `deck_api::Geometry`'s now, so it was
deleted rather than corrected. That closes the second bullet of the plan's
"What is hardcoded today" list. (The first, `KeyNumber::key()`, was already
removed by `317e0e7`; the third, the window's `15`, belongs to M4.)

### 3.3 Guard 3 claimed more scope than it had

The plan says the writer guard was "extended to the whole crate", but the scan
started at `src/`. It now starts at the manifest directory and skips only
`target/`, so `src/` and `tests/` are both covered and the claim is literal.

### 3.4 The execution record was stale

In `odd/tasks/deck-seam.md`, T5 (`317e0e7`) and T6 (`9fcd517`) were still marked
`pending`, and T7 (`seam check`) with them. The table now records T4b, T5, T6
and T7 as done, and log entry L16 records this closure with its evidence.

### 3.5 The hardware line

Not a code gap. `cargo test --features hardware-tests -- --ignored` has never
run against a real deck on a unit that is not the daily driver. It is blocked on
hardware, not on code.

---

## 4. Verification

Run from the repository root on 2026-11-12, Windows, Rust 1.99 toolchain.

| Check | Command | Result |
| --- | --- | --- |
| Whole workspace | `cargo test --workspace` | green; includes the 4 new cases, the suite over both profiles, and all guards |
| Lints | `cargo clippy --workspace --all-targets --all-features -- --deny warnings` | clean |
| Formatting | `cargo fmt --all --check` | clean |
| Hardware tests compile | `cargo test --features hardware-tests --no-run` | compiles; nothing was executed |

Not run on purpose: `cargo test --features hardware-tests -- --ignored`. It
opens the device, and the only deck available is the daily driver, which S6
forbids.

---

## 5. The open line

| Item | Status |
| --- | --- |
| Suite green over two profiles | **done** — SS550 on a fake transport, 6-key deck on itself |
| Three guards green | **done** — device name, key count, single writer |
| Hardware-in-the-loop run on a dedicated unit | **open** — blocked on a second deck |

To close it, on the dedicated unit:

```text
cargo test --features hardware-tests -- --ignored --nocapture
```

The two ignored tests are in `crates/ss550-driver/tests/hardware.rs`: the deck
on the desk must pass the shared suite, and a human must press a key within 15
seconds and see it reported through the seam. The feature is off by default and
every hardware test is `#[ignore]`d, so a plain `cargo test` never opens a
device.

---

## 6. Not promised

- **A virtual HID device in CI.** Windows has no such thing without a signed
  kernel driver; Linux has `uhid`. The suite runs at the transport seam, which
  proves the driver's logic, not Microsoft's HID stack.
- **A deck-agnostic window.** `crates/ss550-gui` still asserts 15 keys, a 5-wide
  grid and 100×100 images, and `ss550-engine::KeyNumber` still carries `1..=15`.
  Both belong to M4 and M3 deliberately did not touch them.
- **A real second deck.** The seam is proven against a simulator; M9 closes it
  against hardware.

---

## 7. Files touched by the closure

| File | Change |
| --- | --- |
| `crates/deck-simulator/tests/ss550_deck_over_the_wire.rs` | new: the real driver over the wire fake, four cases |
| `crates/deck-simulator/Cargo.toml` | dev-dependencies on `ss550-driver` and `jpeg-encoder` |
| `crates/deck-simulator/README.md` | points at the join test the CI claim rests on |
| `crates/ss550-driver/src/keys.rs` | `Key::position()` removed |
| `crates/ss550-driver/src/connection.rs` | writer guard scans `src/` and `tests/`, skipping `target/` |
| `CHANGELOG.md` | the fake's four cases and the removed helper |
| `PLAN.md` | M3 status and the "hardcoded today" list |
| `odd/tasks/deck-seam.md` | T4b, T5, T6, T7 statuses and log L16 |
