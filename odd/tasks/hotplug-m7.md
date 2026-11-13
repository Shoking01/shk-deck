# Feature: Hotplug and Auto-Reconnect (M7)

**Objective.** Watch for connect/disconnect events, show the state to the user, and automatically reconnect when the deck is plugged back in with exponential backoff.

**Definition of Done.** Unplugging and replugging the deck recovers by itself without user intervention.

**Branch.** `feature/deck-seam`
**Delivery strategy.** `single-pr`
**Test runner.** Root workspace: `cargo test --workspace`, `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features -- --deny warnings`.

## Specs

### S1 — Device presence detection

> The app detects when the deck is plugged in or unplugged.

Accepted scope:
- Poll for device presence every 1 second (DISCOVERY_POLL)
- Show "WAITING FOR DECK" when deck is not present
- Show "CONNECTING" when attempt is in progress
- Show "CONNECTED" when deck is ready

### S2 — Automatic reconnect

> The app reconnects automatically when the deck is plugged back in.

Accepted scope:
- On disconnect, show "Connection lost" and start reconnect attempts
- Use exponential backoff: 2s, 4s, 8s, 16s..., capped at 60s
- After MAX_RECONNECT_DELAY (60s), reset backoff counter

### S3 — State display in UI

> The connection state is shown in the window header.

Accepted scope:
- Status pill shows: CONNECTING | WAITING FOR DECK | CONNECTED | CONNECTION LOST
- Message line shows detailed note about current state

### S4 — Manual reconnect button

> User can force a reconnect attempt.

Accepted scope:
- "RECONNECT" button drops current connection and attempts fresh connect
- Button clears backoff and resets to 2s initial delay

## Tasks

| ID | Task | Specs | Route | Status | Commit |
| --- | --- | --- | --- | --- | --- |
| T1 | Add `is_deck_present()` to driver | S1 | inline | ✅ done | 9abd9a7 |
| T2 | Implement hotplug detection constants | S1, S2 | inline | ✅ done | 9abd9a7 |
| T3 | Integrate hotplug polling into Controller | S1, S2, S3 | inline | ✅ done | 138427f |
| T4 | Add automatic reconnect with backoff | S1, S2 | inline | ✅ done | 138427f |
| T5 | Update UI to show connection states | S3 | inline | ✅ done | |
| T6 | Verification: tests, clippy, fmt | all | inline | ✅ done | |

## Log

L1 - 2026-11-13, user request: "Completar M7 (Hotplug)"

L2 - Implementation completed:

### Completed Implementation

**T1 - `is_deck_present()` function added:**
- Location: `crates/ss550-driver/src/connection.rs:164-184`
- Non-blocking probe using `HidApi::new()` and `vendor_collection_present()`
- Returns `true` if SS550 is enumerated on the USB bus

**T2 - Hotplug constants added:**
- `DISCOVERY_POLL`: 1 second polling interval for device presence
- `RECONNECT_BACKOFF_FACTOR`: 2 (doubling factor)
- `MAX_RECONNECT_DELAY`: 60 seconds cap
- `BACKOFF_RESET_THRESHOLD`: 10 failures before max delay

**T3 - Controller integration:**
- Modified `maint()` to check device presence via `is_deck_present()`
- Added `handle_disconnect()` method for exponential backoff
- Added `reconnect_at` field to track next reconnect time

**T4 - Exponential backoff:**
- 2s base delay, doubling after each failure (2s, 4s, 8s, 16s...)
- Capped at 60 seconds maximum
- Counter reset on successful connection

**T5 - UI state display:**
- Status pill already shows: CONNECTED (green), CONNECTING (amber), WAITING FOR DECK (amber), CONNECTION LOST (red)
- Message line shows detailed backoff info when connection is lost
- Profile switching already integrated (from M6)

**T6 - Verification:**
- All tests pass: `cargo test --workspace`
- Clippy clean: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Code formatted: `cargo fmt --all`

### Files Changed

- `crates/ss550-driver/src/connection.rs`: +57 lines (is_deck_present function)
- `crates/ss550-driver/src/lib.rs`: +1 line (export is_deck_present)
- `crates/ss550-gui/src/main.rs`: +41 lines (hotplug integration, backoff logic)
- `odd/tasks/hotplug-m7.md`: this file

### How Hotplug Works

1. **Device detection**: Every 1 second (DISCOVERY_POLL), `maint()` calls `is_deck_present()` to check if the SS550 is on the USB bus

2. **Disconnect handling**: If we have a session but `is_deck_present()` returns false, `handle_disconnect()` is called:
   - Sets `link` to `Link::Lost`
   - Increments failure counter
   - Calculates exponential backoff delay
   - Updates `self.note` with retry timing message

3. **Reconnect logic**: When `session` is `None` and `pending` is `false`:
   - Checks if current time >= `reconnect_at`
   - If so, requests a new connection attempt via `request_attempt()`

4. **Backoff calculation**: 
   ```
   delay = 2s * (2^(failures-1)) = 2s, 4s, 8s, 16s, 32s...
   cap at 60s after 10 failures
   ```

5. **Reset on success**: On successful connection, `attempts` and `reconnect_at` are reset

### Key Learnings

- hidapi does not provide true hotplug events on Windows without external crates
- Polling every 1-2 seconds is reliable and low-impact
- Exponential backoff prevents USB bus spam when devices are unplugged
- The existing `Link` enum states (Connecting, Waiting, Connected, Lost) map perfectly to UI display