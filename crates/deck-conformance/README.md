# deck-conformance

One suite of ten cases, run against any deck. The real device and a six-key
simulator pass the same file, so a deck that breaks the contract is caught
before it reaches a desk.

## What it is

```rust
run(&mut dyn Deck) -> Vec<CaseResult>
```

The cases, their verdicts, and the reason for each case that does not apply to
a particular deck. The order is fixed, so a diff between two decks reads as a
diff in the same list, and a failing case does not stop the run: the caller
wants the whole picture, not the first problem.

## The cases

| Case | What it pins |
| --- | --- |
| `image_spec_is_usable` | the image spec the deck declares can be used at all |
| `a_key_past_the_end_is_refused` | an out-of-range key is refused, and the refusal names the deck's own key count |
| `an_image_over_the_limit_is_refused` | an oversized payload is refused, and the refusal names the deck's own limit |
| `brightness_past_the_range_is_refused` | a brightness outside the range is refused, not clamped |
| `brightness_without_the_capability_is_refused` | an operation the deck does not advertise is refused with `DeckError::Unsupported`, not accepted and ignored |
| `clear_key_without_the_capability_is_refused` | the same, for a single-key clear |
| `clear_all_without_the_capability_is_refused` | the same, for clear-all |
| `sleep_without_the_capability_is_refused` | the same, for sleep |
| `a_dial_past_the_count_is_refused` | a dial that does not exist is refused, naming how many there are |
| `identity_is_never_an_empty_success` | an identity is never an empty success; a deck that cannot read one refuses honestly |

## What the suite deliberately does not do

| Not done | Why |
| --- | --- |
| It never paints a key | The suite runs against a device on a desk as well as against a simulator, so it only makes calls it expects to be refused. A successful paint belongs to the provider's own tests. |
| It cannot prove a provider validates before it writes | A provider that wrote first and refused second would still pass. The provider's own tests pin that. |
| It never judges geometry arithmetic | `Geometry` derives its own rows and positions, so an inconsistent grid cannot be built. What a deck can get wrong is the key count it *reports*, and that is checked. |

## The guards

| Guard | Forbids |
| --- | --- |
| `the_suite_never_names_a_device` | a device name anywhere in the crate |
| `the_suite_never_assumes_a_key_count` | a specific key count anywhere in the crate |

The suite also tests itself: every case is run against a stub with exactly one
flaw, and that stub must fail that case and only that case. A case that no
flaw can break is a case that proves nothing.

## Tests

```text
cargo test -p deck-conformance
cargo clippy -p deck-conformance --all-targets -- --deny warnings
```
