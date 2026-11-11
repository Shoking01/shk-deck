# Changelog — Protocol

All notable protocol decisions are documented here. This uses a
fragment-based convention: entries cite live evidence or an explicit
open-source reference; static-only items are labelled as such.
No original vendor source code was copied (clean-room note in
`image-upload.md`).

## Unreleased

### Changed
- The driver now refuses to send any command that is not on the verified list,
  and it refuses it **before** the bytes leave the host. The refusal names the
  command and the reason, and there is no switch that turns it off.
  `clear_key` and `sleep` are the two commands it refuses: neither was ever
  exercised on a deck, so neither may reach one. `clear_all`, the image path,
  the brightness and the heartbeat are verified and behave exactly as before.
- The driver's image cache is no longer updated before the deck has been told:
  `clear_key` used to drop the key from the cache first, which would have left
  the driver claiming a key was blank while the deck still showed it.
- `sleep` no longer wakes the deck before sending the sleep pack, so a
  refused sleep writes nothing at all.

### Added
- `cargo fuzz` target on the input decoder: raw bytes, every prefix, the same
  bytes inside a valid `ACK`/`OK` envelope, and an oversized buffer. 142,297
  executions in 61 s on nightly with AddressSanitizer, with no crash. The
  decoder is panic-free by construction — its `MIN_LEN` guard precedes every
  index into the report — and the fuzzer confirms it.
- `docs/protocol/verified.md`: the safe-mode list. Ten commands, four verified
  on hardware (`DIS`, `LIG`, `BAT` + chunks + `STP`, `CLE 0xFF`), six not
  (`CLE` per key, `HAN` sleep, `CONNECT`, the bare `HAN`, `VER`, `MOD`), each
  with its evidence or the reason it is still pending. The registry in
  `ss550-protocol` carries the same text, and a test fails when the document
  and the registry disagree.
- `is_key_edge(report)`: tells a real key edge apart from a reply to one of our
  own commands. Every report from the deck wears the same envelope (`"ACK"` at
  `[0..3]`, `"OK"` at `[5..7]`), so a command reply (generic ACK, `TF` space
  reply, …) and a key press look alike until the key byte is read. A command
  reply carries key `0`; an edge carries a key in `1..=15` and a state of
  `0`/`1`. Additive: `parse_input_report` is unchanged.
  Needed because the driver never reads its ACK replies after a write, so they
  are still queued on the interrupt-IN endpoint when the host starts polling;
  treating one as a key would report a bogus event and drop a healthy link.
- `ss550-protocol` crate: pure HID codec with byte-exact builders
  `DIS | LIG | BAT | STP | CLE (all/key) | VER | HAN (bare/sleep) | CONNECT | MOD`,
  JPEG chunk packs, and `build_image_chunks`. Reference: `4ndv/mirajazz`
  `src/device.rs`. Layout: `[0]=0x00`, `[1..4]="CRT"`, `[4]=0`, `[5]=0`,
  `[6..]=tag`. `REPORT_SIZE=1025`, `CHUNK_SIZE=1024`, `MAX_IMAGE_SIZE=65535`.
  Tests: 16 golden vectors + 5 proptest properties (panic-free on arbitrary input).
  Coverage: **100% lines (108/108), 100% functions (18/18), 100% regions
  (156/156)** via `cargo llvm-cov -p ss550-protocol`.

### Changed (bugfix — wire bytes)
- Driver previously framed packs one byte early: tag at `[5..8]`, BAT
  length/key at `[10..12]`, LIG value at `[12]`, CLE key at `[11]`.
  Corrected to mirajazz/probe layout: tag at `[6..9]`, BAT len at `[11..12]`
  and key at `[13]`, LIG at `[11]`, CLE at `[12]`. The systematic shift
  was caught when extracting the protocol crate; the probe layout already
  matched mirajazz and was live-verified. No live driver proof for the new
  bytes yet — a dedicated test-device session is still required (AGENTS.md §4).

### Verified live
- Single-image path: `DIS`, `LIG 0`, per key `BAT` + 1024-byte chunks,
  one `STP`, `LIG 100`. JPEG 100×100 q90, rotate 180°, mirror none.
  Firmware keys are 1-based (1–15); OpenDeck `N` → firmware `N+1` (linear,
  bottom-left origin). Brightness starts at 0 after `initialize()` and must
  be restored to 100. See `image-upload.md` for byte tables and the cleaned
  Python-side failure log.
- `CLE` with `0xFF` = all keys and per-key `CLE` (firmware key at wire `[12]`)
  are encoded in `ss550-protocol`; live clear-per-key still pending.
- `VER` request (`CRT|VER`) encoding was recovered from
  `sendGetHardwareFirmwareVersion`; the driver now writes it and reads the
  reply via `get_input_report` (1025-byte buffer). A 513-byte buffer fails
  with `ERROR_GEN_FAILURE` on Windows. A silent reply is not proof of a
  wrong pack — the mirajazz reader only accepts `V1./V2./V3.` for
  `StreamDock[293]`. Live parse on our SS550 is pending.

### Not yet implemented (intentionally deferred)
- Profile/page switching (`MOD` encoder exists, but the full
  `insertSettings` / `ICO` / `ULEND` profile push is static-only and
  unverified). Sending it without a dedicated test device would violate
  the safe-mode rule.
- Device hotplug watcher and an async `nusb`/`tokio` transport. The current
  driver uses synchronous `hidapi`; enumeration works, a hotplug stream is
  deferred.
- `CONNECT`, `LBLIG`, `SETLB`, `QUCMD`, `OLG` (logo 320×240) — encoded or
  documented only where needed; no live proof.
- Simulator (`ss550-simulator`), fuzzer (`cargo fuzz` on decoder),
  `docs/protocol/verified.md` safe-mode list, and
  `docs/protocol/INCIDENTS.md` incident log from AGENTS.md §4 — not yet created.

### References
- `4ndv/mirajazz` (MPL-2.0) `src/device.rs`, `src/images.rs` — wire source of truth.
- `MMonkeyKiller/opendeck-ss550` (GPL-3.0) — 3×5 grid, protocol v2, image format, OpenDeck key table.
- `SDLibrary1.dll` framers (static analysis only, not live-verified for the
  profile path).
