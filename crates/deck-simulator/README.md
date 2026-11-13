# deck-simulator

The second deck profile, and the SS550's wire in memory. Two implementations,
one purpose: prove that the seam is real, without a device on the desk.

## What it holds

| Implementation | Role |
| --- | --- |
| `SixKeyDeck` | A `Deck` with a deliberately **different shape**: six keys in two rows of three, 72×72 JPEGs at a 4 KB limit, two dials, single-key clear and clear-all, and **no brightness and no sleep** |
| `Ss550WireFake` | A `Transport` that decodes **real packs** with `ss550-protocol`, so the actual driver runs in CI against a wire that behaves like the SS550's |

The difference in shape is the point. A second implementation that is the
SS550 over memory would prove nothing; a deck with fewer keys, a different
image size and no brightness is a different device from the first byte.

## Why the refusals matter

The six-key deck cannot do what the SS550 can, and it says so:

| Operation | Answer |
| --- | --- |
| `set_brightness` | `DeckError::Unsupported` — it has no backlight |
| `sleep` | `DeckError::Unsupported` — it has no sleep command |
| a key past the sixth | `DeckError::KeyOutOfRange`, naming the key count |
| a dial past the second | `DeckError::DialOutOfRange`, naming the dial count |
| an image over the limit | `DeckError::ImageTooLarge`, naming the limit |

A refusal is never a success that changed nothing. The conformance suite
checks both directions: a deck that honours the seam passes every case, and a
deck with one flaw fails exactly one.

## The fake's transcript

`Ss550WireFake` records every command it decodes, so a test can assert what
the driver actually put on the wire — not just what it returned. A failed write
or read is reported and never reaches the transcript, and an image transfer is
one entry however many packs it takes.

`tests/ss550_deck_over_the_wire.rs` is the join that matters: the **real**
`Ss550Deck` over this fake, painting, clearing, reading a press and being
refused, with no `hidapi` in the graph. That is what the "runs in CI" claim in
the table above rests on.

## Tests

```text
cargo test -p deck-simulator
cargo clippy -p deck-simulator --all-targets -- --deny warnings
```
