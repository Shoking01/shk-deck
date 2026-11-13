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
| T3 | Integrate hotplug polling into Controller | S1, S2, S3 | inline | in progress | |
| T4 | Add automatic reconnect with backoff | S1, S2 | inline | pending | |
| T5 | Update UI to show connection states | S3 | inline | pending | |
| T6 | Verification: tests, clippy, fmt | all | inline | pending | |

## Log

L1 - 2026-11-13, user request: "Completar M7 (Hotplug)"

L2 - Implementation notes:
- hidapi 2.6 supports device enumeration via `refresh_devices()` and `device_list()`
- We added `is_deck_present()` in `ss550-driver` as a non-blocking probe
- Added reconnect backoff constants for exponential backoff
- Created M7 task document for hotplug and auto-reconnect feature

### Current state

**Implemented (ready to use):**
- T1: `is_deck_present()` function added to `ss550-driver/src/connection.rs`
- T2: Constants `DISCOVERY_POLL`, `RECONNECT_BACKOFF_FACTOR`, `MAX_RECONNECT_DELAY`, `BACKOFF_RESET_THRESHOLD` added

**What remains to implement:**
- T3: Integrate hotplug polling into Controller - need to update `maint()` to check device presence
- T4: Add automatic reconnect with backoff - need to track consecutive failures and apply exponential delay
- T5: Update UI to show connection states - need to update status pill display

**Code analysis:**
The current implementation has a worker thread that handles connection attempts. The hotplug detection will work as follows:
1. In `maint()`, after collecting attempts, check if deck is present with `is_deck_present()`
2. If deck is not present but we have a session, call `drop_deck()` and schedule reconnect
3. Use exponential backoff based on consecutive failure count
4. Reset backoff counter on successful connection

**Files modified:**
- `crates/ss550-driver/src/connection.rs`: +57 lines (is_deck_present function)
- `crates/ss550-driver/src/lib.rs`: +1 line (export is_deck_present)

## Implementation Notes

### How hotplug will work

The `Link` enum already has the right states:
- `Connecting` - deck not found, attempt in progress
- `Waiting` - deck not on bus, retries every 2s
- `Connected` - deck is ready
- `Lost` - connection died, need to reconnect

The fix is in the `maint()` function:
```rust
fn maintain(&mut self) {
    self.collect_attempts();
    
    // NEW: Check for unexpected disconnect
    if self.session.is_some() && !is_deck_present() {
        self.drop_deck();
        self.attempts = 0;  // Reset for backoff
    }
    
    // NEW: Check for reconnect opportunity
    if self.session.is_none() && !self.pending {
        // Apply backoff delay
        if let Some(last) = self.last_attempt {
            let delay = self.calculate_backoff();
            if last.elapsed() < delay {
                return;  // Not time to reconnect yet
            }
        }
        self.request_attempt();
    }
    
    self.heartbeat();
}
```

## Key Learnings

- hidapi does not provide true hotplug events on Windows without external crates
- Polling every 1-2 seconds is reliable and low-impact
- Exponential backoff prevents USB bus spam when devices are unplugged