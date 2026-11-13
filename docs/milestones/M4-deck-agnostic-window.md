# M4 — A deck-agnostic window

**Status:** complete.
**Branch:** `feature/deck-seam`
**Date:** 2026-11-12
**Related:** `PLAN.md` §M4, `odd/tasks/deck-agnostic-window.md`, `CHANGELOG.md`

This file records the milestone as delivered and how each of its four claims is
proven. It is the reference for "what does M4 consist of, and what would break
it".

---

## 1. What the milestone asked for

The window stops asserting what deck it is talking to. The grid comes from the
profile's geometry, the image size from the profile, and a capability the deck
does not have is not offered — and if something asks for it anyway, it fails
with the typed error, never with a silent no-op. Bindings are stored per deck.
The texts name the deck in front of them.

**Done when** the same window drives the 6-key profile with no SS550 literal
left in `crates/ss550-gui`, and an operation that profile does not support
returns its typed error.

No hardware is involved: the second profile is `deck_simulator::SixKeyDeck` —
6 keys in 2 rows of 3, 72×72 images, two dials, no brightness, no sleep.

---

## 2. Where each claim lives

| Claim | Where | How it is proven |
| --- | --- | --- |
| The grid comes from the deck | `crates/ss550-gui/src/session.rs` | `DeckSession` reads `Deck::geometry` once per connection and turns it into display rows, top row first. Nothing above it has a key count: the tiles, the key picker, the row labels and the "painted N keys" notes are all built from those rows. |
| The image size comes from the deck | `session::encode_tile` | the encoder is given `ImageSpec::width()` and `height()`; a test decodes the JPEG's frame header and asserts 72×72, so a test that only checked the arguments could not pass with an encoder that ignored them. |
| A missing capability is not offered | `Controller::render` | the paint-all label, `CLEAR SELECTED KEY`, `CLEAR ALL (BLANK)`, `KEEP ALIVE · SEND ONE NOW` and the brightness row are each pushed only when `Capabilities` says the deck has them. A deck without brightness gets no brightness row at all. |
| Asking anyway fails with the typed error | `DeckSession::set_brightness`, `::keep_alive` | `SixKeyDeck` refuses both; the tests assert `DeckError::Unsupported { operation: Brightness }` and `{ operation: KeepAlive }` exactly, not just "an error". |
| Bindings are stored per deck | `ss550-engine::config::bindings_path_for` | the file is `config_dir()/bindings/<slug>-<fnv64>.json`, keyed by `identity()` when the deck has one and by its shape when it does not. |
| The texts name the deck | `Controller::render` header | the title is `DeckSession::label()` and the subtitle is `{keys} keys · {columns} columns · {W}x{H} images`, plus dials when the deck has any. Before a deck answers: "No deck yet". |

### The seam grew one operation

The SS550 resets itself after about 68 seconds of host silence, so the
keep-alive is real deck behaviour, not a window timer. M4 therefore added
`Deck::keep_alive`, `Capabilities::keep_alive` and `DeckOperation::KeepAlive`.
The trait's default is the typed refusal, `DeckHandle` forwards it, `Ss550Deck`
answers it with the driver's `keepalive()`, and the conformance suite gained an
eleventh case with its falsifying stub so a deck that claims the capability and
does nothing is caught.

The window's automatic heartbeat fires only when `capabilities().keep_alive()`
is true, so `SixKeyDeck` — which never resets — is never asked.

### Dropping the deck only on transport failure

`Controller::report` drops the session only for `DeckError::Transport(_)`. Any
other `DeckError` is the deck answering: the link is fine, so the window reports
the typed refusal and keeps the deck. Treating every error as a disconnect would
make a refusal look like a dead cable.

---

## 3. The guard

`the_window_never_names_the_deck_it_drives` in `crates/ss550-gui/src/main.rs`
walks every `.rs` file under `src/` and fails on:

- the device name (`SS` + `550`),
- the product name (`Stream` + `Craft`),
- the vendor (`Red` + `ragon`),
- the USB id (`0200` + `:3000`),
- any whole-number literal equal to the SS550's key count.

The needles are spelled in pieces so the guard does not match its own source,
and the number check parses literals rather than grepping digits, so `px(15.)`
is a size and `4_096` is a payload limit, while a bare key count is caught.

It was falsified before being trusted: a planted `// probe: SS550 and 15` line
made the test fail with both reports, and removing the line returned the suite
to green.

---

## 4. Verification

Run on 2026-11-12, Windows, from the repository root.

| Check | Command | Result |
| --- | --- | --- |
| GUI suite | `cargo test --manifest-path crates/ss550-gui/Cargo.toml` | `26 passed; 0 failed` — 8 session tests, the guard, and the existing window/diagnostics tests |
| Whole workspace | `cargo test --workspace` | exit 0, 30 suites ok |
| Root lints | `cargo clippy --workspace --all-targets --all-features -- --deny warnings` | clean |
| GUI lints | `cargo clippy --manifest-path crates/ss550-gui/Cargo.toml --all-targets --all-features -- --deny warnings` | clean |
| Root formatting | `cargo fmt --all -- --check` | clean |
| GUI formatting | `cargo fmt --manifest-path crates/ss550-gui/Cargo.toml -- --check` | clean |

`crates/ss550-gui` is its own workspace root, so its checks use
`--manifest-path`; running `cargo test -p ss550-gui` from the root would not
build it.

Not run, on purpose: nothing that opens the device. M4 needs no hardware, and the
M3 hardware line stays open and blocked on a dedicated test unit.

---

## 5. What is not promised

- **A second physical deck.** The window is proven against a simulator. M9
  closes the seam against real hardware.
- **Hotplug.** Unplugging the deck still needs `RECONNECT`; automatic
  reconnection is M7.
- **Profiles.** Bindings are per deck, not per named profile; that is M6.
- **A per-deck migration UI.** The pre-M4 `bindings.json` is copied over only
  when every bound key exists on the connected deck. A file that would need
  keys the deck does not have is left alone, and the window says so. There is no
  prompt to remap it.

---

## 6. Files touched

| File | Change |
| --- | --- |
| `crates/ss550-gui/src/session.rs` | new: `DeckSession`, `SessionError`, `encode_tile`, display rows, shape label; 8 tests over `SixKeyDeck` |
| `crates/ss550-gui/src/provider.rs` | new: `ConnectError`, `connect`, `From<Ss550Error>` |
| `crates/ss550-gui/src/main.rs` | the window renders and acts from the session; per-deck store; deck-naming guard; title "Deck Controller" |
| `crates/ss550-gui/Cargo.toml` | `deck-api`, `jpeg-encoder`, `thiserror`; dev-dependency `deck-simulator` |
| `crates/deck-api/src/types.rs` | `Capabilities::keep_alive` with builder and getter |
| `crates/deck-api/src/error.rs` | `DeckOperation::KeepAlive` and its `Display` |
| `crates/deck-api/src/deck.rs` | `Deck::keep_alive` default refusal; `DeckHandle` forwarding |
| `crates/deck-conformance/src/cases.rs` | eleventh case `keep_alive_without_the_capability_is_refused` |
| `crates/deck-conformance/tests/the_suite_catches_a_bad_deck.rs` | `Flaw::IgnoresMissingKeepAlive` and its pairing |
| `crates/deck-conformance/README.md` | eleven cases, and the new case in the table |
| `crates/ss550-driver/src/deck.rs` | `Ss550Deck` declares `keep_alive` and implements it |
| `crates/ss550-engine/src/key.rs` | `KeyNumber` is 1..=255; the deck's geometry decides existence |
| `crates/ss550-engine/src/config.rs` | `bindings_path_for`, legacy `bindings_path`, slug + FNV-1a file stem |
| `CHANGELOG.md` | the M4 entry |
| `PLAN.md` | M4 status |
| `odd/tasks/deck-agnostic-window.md` | specs, tasks, and the log with the evidence |
