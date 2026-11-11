# Verified commands — StreamCraft SS550

**This file is the safe-mode list.** The driver refuses to send a command that
is not on it, so a command becomes sendable only by being written down here
**and** in the registry inside `ss550-protocol`
(`crates/ss550-protocol/src/verified.rs`). A test fails if the two disagree, so
this file cannot drift away from the code.

The rule comes from `AGENTS.md` §4:

> **NEVER send untested raw commands to the device.** All commands must be:
> 1. Documented in `docs/protocol/`
> 2. Unit tested with golden vectors
> 3. Integration tested on a **dedicated test device** (not your daily driver)

A command is **verified** only when all three hold. Static analysis, a reference
implementation or a recovered disassembly is *not* evidence: it is a hypothesis
waiting for a test deck.

## The list

The **Evidence or reason** column is the registry's own text, character for
character. That is what lets a test compare this file with the code.

| Command | Tag | Status | Evidence or reason |
| --- | --- | --- | --- |
| `display_on` | `DIS` | verified | docs/protocol/image-upload.md — the single-image path, step 1, verified live |
| `brightness` | `LIG` | verified | docs/protocol/image-upload.md — --bright 30 visibly dims the deck |
| `image_transfer` | `BAT` | verified | docs/protocol/image-upload.md — the single-image path renders all 15 keys |
| `clear_all` | `CLE` | verified | docs/protocol/CHANGELOG.md — clearing every key was exercised on the deck |
| `clear_key` | `CLE` | unverified | never sent to a deck: live clear-per-key is still pending (docs/protocol/CHANGELOG.md) |
| `sleep` | `HAN` | unverified | encoded from the reference implementation, never exercised on a deck (docs/protocol/image-upload.md) |
| `keep_alive` | `CONNECT` | unverified | the driver heartbeats with DIS instead, so this one was never needed (docs/protocol/CHANGELOG.md) |
| `handshake` | `HAN` | unverified | the driver never handshakes: the deck is waited for, not greeted (docs/protocol/image-upload.md) |
| `firmware_version` | `VER` | unverified | writing it halts the USB endpoint, so it must never be sent (docs/troubleshooting/deck-stuck.md) |
| `mode` | `MOD` | unverified | static-only, part of the unverified profile path (docs/protocol/CHANGELOG.md) |

**Read-only operations are not on this list, on purpose.** Polling for key
events and reading the identity report write nothing to the deck, so they
cannot brick it and they need no entry here.

## What each command does

### `display_on` — `DIS`

Wakes the display. It has no persistent side effect, which is exactly why the
driver uses it as the heartbeat: a write that keeps the deck alive without
changing what it shows.

### `brightness` — `LIG`

Sets the backlight, 0 to 100. The value lives at wire `[11]`. Verified live by
setting 30 and watching the deck dim.

### `image_transfer` — `BAT`, the JPEG chunks, then exactly one `STP`

The only path that puts pixels on the deck. The order is not negotiable:

1. `BAT` with the BE16 length and the 1-based firmware key,
2. the JPEG in 1024-byte chunks,
3. **one** `STP` after every key.

A `STP` before the payload is the one mistake that halts the endpoint: the
firmware sees a commit for an empty transfer, refuses it, and no host can
write to the deck again until it is power-cycled. See
[`docs/troubleshooting/deck-stuck.md`](../troubleshooting/deck-stuck.md).

### `clear_all` — `CLE` with `0xFF`, then one `STP`

Blanks every key. `0xFF` is the "all keys" selector, not a key number.

### `clear_key` — `CLE` with the firmware key, then one `STP`

Blanks one key. Encoded and unit-tested, **never sent to a deck**: the live
clear-per-key check is still pending. Until it is done on a test deck, the
driver refuses to send it.

### `sleep` — `CRT|HAN`

Puts the display to sleep. Encoded from the reference implementation, never
exercised on a deck.

### `keep_alive` — `CONNECT`

The reference implementation's keep-alive. **This driver does not use it**: the
heartbeat is a `DIS`, which is proven and has no side effect. `CONNECT` stays
unverified because nothing here ever needed it.

### `handshake` — bare `HAN`, no `CRT` prefix

The connect-time greeting. The driver does not greet: it waits for the deck to
stop re-enumerating and then opens the handle. The bare `HAN` and the
`CRT|HAN` sleep form are **not** interchangeable.

### `firmware_version` — `VER`

**Never send this.** The firmware rejects the report and halts the interrupt
OUT endpoint, after which no host — this driver or the vendor app — can write
anything until the deck is power-cycled. The version is read from the static
identity report instead, with no command sent at all.

### `mode` — `MOD`

Switches the device mode. Static-only, and it belongs to the profile path that
is still unverified end to end.

## How a command becomes verified

1. **Document it** in `docs/protocol/`, with the byte layout and where the
   layout came from.
2. **Pin it** with a golden vector in `crates/ss550-protocol/tests/golden.rs`.
3. **Try it on a test deck**, never on the daily driver, and record what
   happened: what was sent, what the deck did, what it looked like.
4. **Write it down here**, with the evidence, and in the registry in
   `crates/ss550-protocol/src/verified.rs`. The drift test fails until both
   agree.

Until then the command is `unverified` and the driver refuses to send it. The
refusal is not a bug to work around: it is the rule.

## When the driver refuses

The error says which command was blocked and why:

```text
Refusing to send clear_key: never sent to a deck: live clear-per-key is still pending (docs/protocol/CHANGELOG.md)
```

If you hit that, the command has not been proven on hardware. Finish the
verification steps above on a **test deck** first — or, if you only need to
experiment, use `ss550-probe`, which is the tool for talking to a deck with
bytes nobody has vouched for yet.
