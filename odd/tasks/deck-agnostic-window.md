# Feature: A deck-agnostic window

**Objective.** Make `crates/ss550-gui` stop asserting which deck it is talking
to: the grid comes from `Deck::geometry`, the image from `Deck::image_spec`, the
offered controls from `Deck::capabilities`, the texts from the deck in front of
it, an unsupported operation fails with its typed error, and bindings are stored
per deck.
**Branch.** `feature/deck-seam`
**Delivery strategy.** `single-pr` — the branch already exists and M4 continues
it. Work-unit commits, one pull request at the end.
**Test runner.** Root workspace: `cargo test --workspace`, `cargo fmt --all`,
`cargo clippy --workspace --all-targets --all-features -- --deny warnings`.
GUI (its own workspace root): `cargo test --manifest-path
crates/ss550-gui/Cargo.toml`, `cargo clippy --manifest-path
crates/ss550-gui/Cargo.toml --all-targets -- --deny warnings`.

## Specs

### S1 — The grid, the image and the capabilities come from the deck

> The window stops asserting what deck it is talking to. The grid comes from the
> profile's geometry, the image size from the profile, and a capability the deck
> does not have is not offered - and if something asks for it anyway, it fails
> with the typed error, never with a silent no-op.

Accepted scope: the window holds a `deck_api::DeckHandle`, derives key count,
rows and row order from `Deck::geometry`, encodes each key image at
`Deck::image_spec`'s width and height, checks it against `max_bytes`, and reads
`Deck::capabilities` before offering brightness, single-key clear, clear-all or
keep-alive. `ss550-gui` no longer names the driver's `Device` or `ButtonImage`.

The keep-alive is a real deck operation, not a timer the window invents: the
seam gains `Deck::keep_alive` and `Capabilities::keep_alive`, with the typed
refusal (`DeckOperation::KeepAlive`) as the trait default, so a deck that cannot
be kept awake says so and the conformance suite can catch a deck that claims the
capability and does nothing.

### S2 — Bindings are stored per deck

> Bindings are stored per deck.

Accepted scope: `ss550-engine` derives a filesystem-safe file per deck from the
deck's identity when it has one, or from its shape (key count, columns, image
size) when it does not. The pre-M4 `bindings.json` keeps loading, and it is
copied into the per-deck file only when every bound key exists on the deck in
front of the window. `KeyNumber` stops carrying `1..=15`; a key number is a
positive 1-based number, and the deck's geometry decides whether it exists.

### S3 — The texts name the deck in front of them

> The texts name the deck in front of them.

Accepted scope: the header, the deck-wide buttons, the connection notes and the
window title use the connected deck's identity, or its shape when the deck does
not report one. No model name, USB id or key count is written into the source.

### S4 — Done when

> **Done when** the same window drives the 6-key profile with no SS550 literal
> left in `crates/ss550-gui`, and an operation that profile does not support
> returns its typed error.

Evidence: a guard test scans `src/` for the device name, the product name and a
standalone `15`; a unit-tested window state drives `deck_simulator::SixKeyDeck`
and asserts 6 keys in 2 rows of 3, 72×72 images, no brightness control, and
`DeckError::Unsupported { operation: Brightness }` from `set_brightness`.

### S5 — No hardware

> This needs no hardware: the M3 simulator profile is a 6-key deck with a
> different image size and no brightness, and the window is exercised against
> it.

Accepted scope: nothing in this feature opens a device in a test. The GUI's own
workspace adds `deck-simulator` and `jpeg-encoder` as dev-dependencies only.

### S6 — House rules

Rust 2021, MSRV 1.87+, `#![deny(unsafe_code)]`, `thiserror` in libraries, no
`.unwrap()` in production paths, doc comments with examples on every public
item, `cargo fmt` and zero clippy warnings. Artifacts in English.

## Tasks

| ID | Task | Specs | Route | Status | Commit |
| --- | --- | --- | --- | --- | --- |
| T1 | `deck-api`: `keep_alive` operation, capability, typed default and handle forwarding; one conformance case with its falsifying stub | S1, S6 | inline | done — 30 suites green | |
| T2 | `ss550-engine`: `KeyNumber` without the fixed 15-key range, and one bindings file per deck with the legacy file readable | S2, S6 | inline | done — 30 suites green | |
| T3 | `ss550-gui`: a unit-tested deck session (geometry, image spec, capabilities, typed refusals) driven by `SixKeyDeck` | S1, S4, S5 | inline | done — 8 session tests | |
| T4 | `ss550-gui`: the window renders and controls from the session; texts from the deck; per-deck store | S1, S2, S3 | inline | done — compiles, clippy clean | |
| T5 | Guard: no device, product or key-count literal in `crates/ss550-gui/src` | S4 | inline | done — proven to fail on a planted literal | |
| T6 | Verification: both workspaces, fmt, clippy, and the M4 milestone record | S4, S6 | inline | done — evidence below | |

## Log

L1 - 2026-11-12, the user's request, verbatim:

> Completar M4

> Que hay que hacer en M4?

The milestone definition is `PLAN.md` §6 "M4 - A deck-agnostic window". M3 is
closed except its hardware line, which stays open and is not part of this
feature.

L2 - 2026-11-12, decisions taken while implementing (all inline, no writer
trigger held: one understood change per task, no parallel independent units).

- The keep-alive is behaviour the deck really has (the SS550 resets itself after
  ~68 s of host silence), so it is a seam operation, not a window timer: `Deck::keep_alive`,
  `Capabilities::keep_alive`, `DeckOperation::KeepAlive`. The trait default is the
  typed refusal, and the conformance suite gained an eleventh case so a deck that
  claims the capability and does nothing is caught.
- "No SS550 literal" is read as: no model name, product name, vendor, USB id or
  standalone key-count literal in `crates/ss550-gui/src`. Lowercase
  `ss550_driver`/`ss550_engine` crate paths stay, because those are dependency
  names, not claims about the deck.
- `KeyNumber` lost its `1..=15` range. A key number is 1-based and positive; the
  deck's geometry is the only thing that decides whether the key exists.
- Per-deck bindings: the deck key is `identity()` when the deck has one, else a
  shape fingerprint (key count, columns, image width/height), and the file is
  `config_dir()/bindings/<slug>-<fnv64>.json`. The pre-M4 `bindings.json` is read
  once and copied only when every bound key exists on the connected deck; a file
  that would need keys the deck does not have is left alone, not repointed.
- The window drops the deck only on `DeckError::Transport(_)`. Any other
  `DeckError` is the deck answering, so it is reported as a typed refusal and the
  session stays.
- The heartbeat only fires when `capabilities().keep_alive()` is true, so a deck
  that never resets is never asked.

L3 - 2026-11-12, verification evidence.

- `cargo test --manifest-path crates/ss550-gui/Cargo.toml` → `26 passed; 0 failed`.
  That is 8 session tests (geometry, rows, encoded size, refused key, refused
  brightness, refused keep-alive, polled event, shape label) plus the existing
  window and diagnostics tests, plus the new literal guard.
- Guard falsification: a planted `// probe: SS550 and 15` line made
  `the_window_never_names_the_deck_it_drives` fail with both reports
  (`names the deck (SS550)` and `uses the key count as a literal (15)`); the line
  was removed and the suite returned to 26 passed.
- `cargo test --workspace` → exit 0, 30 suites ok.
- `cargo clippy --workspace --all-targets --all-features -- --deny warnings` →
  exit 0. `cargo clippy --manifest-path crates/ss550-gui/Cargo.toml
  --all-targets --all-features -- --deny warnings` → exit 0.
- `cargo fmt --all -- --check` → exit 0. `cargo fmt --manifest-path
  crates/ss550-gui/Cargo.toml -- --check` → exit 0.
- Not verified: the window against the real SS550 on hardware. M4 needs none
  (S5), and the M3 hardware line stays open.
