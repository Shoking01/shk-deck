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
| T1 | Add `is_deck_present()` to driver | S1 | inline | ✅ done | |
| T2 | Implement hotplug detection constants | S1, S2 | inline | ✅ done | |
| T3 | Integrate hotplug polling into Controller | S1, S2, S3 | inline | pending | |
| T4 | Add automatic reconnect with backoff | S1, S2 | inline | pending | |
| T5 | Update UI to show connection states | S3 | inline | pending | |
| T6 | Verification: tests, clippy, fmt | all | inline | pending | |

## Log

L1 - 2026-11-13, user request: "Completar M7 (Hotplug)"

L2 - Implementation notes:
- hidapi 2.6 supports device enumeration via `refresh_devices()` and `device_list()`
- We added `is_deck_present()` in `ss550-driver` as a non-blocking probe
- Connection logic in `connector_worker` will be reused/reworked
- Backoff strategy: 2s base, doubling after each failure, cap at 60s

## Implementation Plan

### Phase 1: Add hotspot detection to driver

- Add `is_deck_present()` function that checks if the SS550 is enumerated
- Already implemented in `crates/ss550-driver/src/connection.rs`

### Phase 2: Integrate into Controller

- Replace the current `request_attempt_if_due()` polling with presence-based detection
- Add connection state tracking with explicit states
- Implement exponential backoff logic

### Phase 3: UI updates

- Update status pill to show different states
- Add reconnect logic on device disappearance

## Key Learnings

- hidapi does not provide true hotplug events on Windows without external crates
- Polling every 1-2 seconds is reliable and low-impact
- Exponential backoff prevents USB bus spam when devices are unplugged