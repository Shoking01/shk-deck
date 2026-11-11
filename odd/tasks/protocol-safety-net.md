# Protocol safety net — the verified command list, the gate and the fuzzer

**Objective:** the driver must never send a command that has not been exercised
on a real deck; the list of what has been exercised must live in one file that
cannot drift away from the code; and the protocol decoder must survive
arbitrary input.
**Branch:** `feature/protocol-safety-net` of `Shoking01/shk-deck`.
**Delivery strategy:** single delivery, no chain.
**Test runner:** `cargo test --workspace` and
`cargo clippy --workspace --all-targets -- -D warnings` at the repo root;
`cargo test --workspace` and `cargo clippy --all-targets -- -D warnings`
inside `crates/ss550-gui`; `cargo test` and
`cargo clippy --all-targets -- -D warnings` inside `crates/ss550-tray` (the GUI
and the tray are their own workspace roots, so the heavy trees stay out of the
root workspace). The fuzz target runs outside the three roots, with
`cargo fuzz run decode_input_report` on nightly.

## Specs

### S1 — The rule becomes a rule the driver enforces

`AGENTS.md` §4 says it, verbatim:

> **NEVER send untested raw commands to the device.** All commands must be:
> 1. Documented in `docs/protocol/`
> 2. Unit tested with golden vectors
> 3. Integration tested on a **dedicated test device** (not your daily driver)

The driver enforces the first half of it: it refuses to send a command that is
not on the verified list. The refusal happens **before** the bytes leave the
host, and it names the command and the reason.

### S2 — The verified list lives in one file

`docs/protocol/verified.md` lists every command the protocol crate can encode:
what it does, the tag on the wire, whether it is verified, and where the
evidence lives. It is the file a person reads before touching the deck.

### S3 — Hardware evidence is the only evidence

A command is verified only when it is documented, pinned by a golden vector
and exercised on a test deck. Static analysis, a reference implementation or a
recovered disassembly is a hypothesis, not a verification — so `CLE` per key,
`HAN` sleep, `CONNECT`, the bare `HAN`, `VER` and `MOD` stay **unverified**
until a deck proves them.

### S4 — The list and the code cannot drift

A test compares `docs/protocol/verified.md` with the registry in
`ss550-protocol` and fails when they disagree: a command in one and not the
other, a different status, or missing evidence.

### S5 — The decoder is panic-free on arbitrary input

`cargo fuzz` runs against the protocol decoder, on the bytes a hostile or
broken device could send. The target must survive arbitrary input without
panicking, and the command to run it is written down.

### S6 — The app keeps working exactly as it does today

Every command the application actually sends is verified, so the gate changes
nothing for the running app: the image path, the brightness, the heartbeat and
"clear all" behave as before. The commands that were never exercised are
refused with a clear error instead of being sent to a deck.

### S7 — The safety net is not a switch

There is no flag, no environment variable and no setter to turn the gate off.
A rule with an off switch is a rule that gets switched off.

## Tasks

| ID | Specs | Route | Status | Evidence |
| --- | --- | --- | --- | --- |
| T1 | S2, S3 | inline | done | `docs/protocol/verified.md`: ten commands, four verified, six unverified, each with its evidence |
| T2 | S1, S6, S7 | delegated (`rust-core`) | done | `crates/ss550-protocol/src/verified.rs`: ten commands, eight unit tests. `crates/ss550-driver`: `ensure_verified`, the gated `write_pack`, `Ss550Error::CommandNotVerified`, four unit tests, and a source-level test that keeps `device.write(` in one place |
| T3 | S4 | inline | done | `crates/ss550-protocol/tests/verified_list.rs`: three tests; proven to fail (2 of 3) when the document's status for `brightness` is flipped to `unverified`, and green again when it is restored |
| T4 | S5 | delegated (`rust-core`) | done | `fuzz/`: the `decode_input_report` target, its own workspace root, and a README with the run command. 142,297 executions in 61 s on nightly with AddressSanitizer, no crash, no timeout, no OOM |

## Log

- **L1 — 2026-11-10, the user's request, verbatim:** "Vamos con M1 como /goal"
- **L2 — 2026-11-10, the milestone being implemented, verbatim from `PLAN.md`:**
  > ### M1 — Safety net for the protocol (blocking for any new command)
  >
  > The deck can be left backlit and black by a bad packet, and from that point
  > no host can paint it until it is fully power-cycled. Before any *new* command
  > is sent to the device:
  >
  > 1. `docs/protocol/verified.md`: one row per command — what it does, the
  >    golden vector, the trace it came from, and whether it was exercised on a
  >    test deck.
  > 2. The driver refuses to send anything that is not on that list (safe mode).
  > 3. `cargo fuzz` on the protocol decoder: panic-free on arbitrary input.
  >
  > **Done when** the fuzz target runs clean and every command the driver can send
  > appears in `verified.md`.
- **L3 — 2026-11-10, what the code actually sends today.** The driver writes
  exactly five kinds of command: `DIS` (wake and heartbeat), `LIG`
  (brightness), `BAT` + JPEG chunks + one `STP` (the image path), `CLE` with
  `0xFF` + one `STP` (clear all) and `CLE` with a key + one `STP` (clear one
  key). `sleep` writes `CRT|HAN`. `CONNECT`, the bare `HAN`, `VER` and `MOD`
  exist in the protocol crate but the driver never sends them: the heartbeat
  is a `DIS`, the deck is waited for instead of greeted, the version is read
  from the static identity report, and `MOD` belongs to the unverified profile
  path.
- **L4 — 2026-11-10, the verification status, from the protocol changelog.**
  Verified live: the single-image path (`DIS`, `LIG 0`, `BAT` + chunks, one
  `STP`, `LIG 100`) and clearing every key. Pending: clear-per-key (`CLE`
  with a key), and the live parse of the `VER` reply. Static-only: `MOD` and
  the whole `ICO`/`ULEND` profile path. `VER` is worse than unverified: writing
  it halts the USB endpoint, which is why the driver reads the version instead
  of asking for it.
- **L5 — 2026-11-10, decision.** The gate is always on, with no switch (S7),
  and it lives in the driver, not in the protocol crate: the protocol crate
  stays pure (no I/O), so it owns the *registry* and the driver owns the
  *enforcement*. `ss550-probe` keeps its freedom on purpose — it is the tool
  for talking to a deck with bytes nobody has vouched for yet.
- **L6 — 2026-11-10, the gate exposed two bugs, both fixed.** `clear_key`
  dropped the key from the image cache **before** writing, so a refused clear
  would have left the driver claiming a key was blank while the deck still
  showed it; the cache is now cleared only after the deck has been told.
  `sleep` woke the deck with a `DIS` **before** the sleep pack, so a refused
  sleep would have written to the deck on its way out; the gate now runs
  before the wake.
- **L7 — 2026-11-10, verification.** `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` and `cargo metadata --locked --format-version 1`
  are green at the root; the GUI and the tray roots pass their own fmt, clippy,
  test and `--locked` checks. The fuzz harness ran 142,297 executions in 61 s
  on nightly with AddressSanitizer and found nothing.
- **L8 — 2026-11-10, what is deliberately left open.** The CI audit job covers
  the three workspace lock files and not `fuzz/Cargo.lock`, so the fuzz
  dependency tree is unaudited; and the fuzz target is not run by CI, because
  a permanent fuzzing job is a decision about CI, not about the protocol.
  Both are follow-ups, recorded here on purpose.
