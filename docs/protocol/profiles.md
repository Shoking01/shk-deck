# Profiles and Stored State — StreamCraft SS550

**Target:** `Stream Station.exe` (Qt5, `QtSQLite` wrapper). Evidence via Ghidra 12.1.4 REA static analysis.

> **Clean-room note:** This document records observable behavior of the official binary only. No original source code was copied.

## Key finding

**Profile switching is host-side only.** The host rewrites all button state; there is **no per-profile HID command** on the device and no command that activates profile *N*. "Profile switching" in the official app is a host-side operation over stored state, plus image uploads (see `image-upload.md`).

## Stored state

The profile store is a **SQLite** database (Qt5 `QtSQLite`, 6 imports in the EXE) with a `Device` column keyed by **device-model string**:

```sql
INSERT INTO "%1" (Device) VALUES ("%2")   -- SQL string at 0x14226fd38
DELETE FROM "%1" WHERE Device = "%2"
```

The device-model strings observed in the same data area — `SS550V3` at `0x14226c260` and `SS-550` (display name `Stream Station SS-550` at `0x14226fe20`) — sit next to sibling models (`293V3`, `K-992`, `CX35`, `AKP815`, `KB-1`, etc.).

## What "profiles" mean for the driver

- The device itself holds **no profile selection state** the driver needs to manage.
- The SS550's 6 "profiles" (per the product spec) are a host-app concept: 6 slot matrices of button images + action assignments that the host pushes to the device on switch.
- The action executors (key combos, OBS, Twitch, system labels, etc.) live entirely in the host application. The device only renders images and reports key presses (see `input-events.md`).

## Known gaps

- The per-profile storage **format** beyond the `Device` column (how many columns, how images/actions are referenced) is not visible from static analysis.
- No profile-activation, profile-write, or profile-read command exists in the HID protocol — any future host-side profile system is unconstrained by the device protocol.
