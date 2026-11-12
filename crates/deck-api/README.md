# deck-api

The contract every deck answers to: `Deck` (what a deck can do) above
`Transport` (how its bytes travel). One trait, one set of typed errors, and no
device anywhere in the crate.

## Why it exists

A second streamdeck differs in key count, geometry, image size, capabilities
and input decoding *before* it differs in transport. A transport-only seam
would be recut at the first second device. So the seam sits above the bytes:
a caller asks a deck what it can do, and the deck answers with data or with a
typed refusal.

## The rules it holds

| Rule | Why |
| --- | --- |
| An operation a deck cannot do returns `DeckError::Unsupported` | A caller that cannot tell "done" from "not possible" cannot tell the user anything useful |
| A value outside the accepted range is refused, never clamped | A clamped request is a request that changed without telling anyone |
| A timeout with nothing waiting is an empty list, not an error | Polling is not a failure |
| The public handle is `Box<dyn Deck>`, not a generic | The window and the engine hold a deck without naming a model |

## What it holds

| Type | Role |
| --- | --- |
| `Deck` | What a deck can do: geometry, image spec, capabilities, brightness, painting, clearing, sleeping, dials, events, reconnect |
| `DeckHandle` | A deck behind one box, so code that knows "a deck" does not name a model |
| `Transport` | The bytes: read a report, write a report, read a control report |
| `Geometry`, `ImageSpec`, `Capabilities` | What the deck can do, as data |
| `KeyId`, `KeyEvent`, `ButtonState` | The keys, 0-based, and what happened to one |
| `DeckError`, `TransportError` | Every refusal, typed |

## The guards

Two tests fail the build if the seam stops being generic. Both cover the whole
crate — the manifest, the sources and the tests included:

| Guard | Forbids |
| --- | --- |
| `the_seam_never_names_a_device` | a device name anywhere in the crate |
| `the_seam_never_assumes_a_key_count` | a specific key count anywhere in the crate |

## Tests

```text
cargo test -p deck-api
cargo clippy -p deck-api --all-targets -- --deny warnings
```

The crate's own tests cover the handle in both directions (a call that arrives
is forwarded; a refusal comes back as the same typed error), the edges of the
geometry, and the two guards. The full crate documentation lives in
`src/lib.rs`.
