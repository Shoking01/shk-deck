# Button Input Events and Device Notifications — StreamCraft SS550

**Target:** `SDLibrary1.dll`. Evidence via Ghidra 12.1.4 REA static analysis (PE32+, image base `0x180000000`).

> **Clean-room note:** This document records observable behavior of the official binary only. No original source code was copied.

## Reading from the device

`readDataFromHidDevice` at `0x18001bfb0` reads with `hid_get_input_report_lenght(h)` (fallback `0x40`), then `hid_read_timeout(h, buf, len, 3000)` (3000 ms timeout).

### Reply sniffing (ASCII at fixed offsets)

| Condition | Meaning |
|---|---|
| `[0..6]` == `"HANDLER"` | Handshake ACK → `sendHandshakePack` |
| `[0..4]` == `"error"` | Error |
| `[0..2]` = `ACK`, `[5..6]` = `OK`, `[9]` == `0` | Generic ACK |
| `[0..2]` = `ACK`, `[5..6]` = `TF` | `[9..13]` decimal ASCII digits = MB free → `freeSpace = (((((d9*100+d10)*100+d11)*100+d12)*100+d13) << 20` |
| `[0..2]` = `ACK`, `[5..6]` = `CB`, `[9..10]` = `"OK"` | Logo upload complete |
| `[0..2]` = `CRT`, `[5..7]` = `MOD` | `[10]` = mode → `sigGetModeCmd` |
| — | Firmware replies carry a `V1.` / `V2.` / `V3.` prefix (`StreamDock[293]` and serial `≠ 3FFFEFD81FFF`) |

## Key events

Key events surface as Qt signal `sigHandleKeyEvents(int, int, uchar)` at `0x180127750`:

| Scenario | `keyId` | `keyState` |
|---|---|---|
| `StreamDock-300` | byte `[0]` | byte `[2]` |
| All other models (incl. `XF-A3501`, `StreamDock[293]`, `StreamDock[295]`, and by extension the SS550) | **`byte [9]`** | **`byte [10]`** |

### Debounce

The generic path is debounced to **> 199 ms** per key (observed in the `StreamDock[293]`/`XF-A3501`/`StreamDock[295]` path via a `TimerTool`).

### keyId → (row, col) mapping

**Confirmed EXE-side.** `readDataFromHidDevice` emits `sigHandleKeyEvents(this, uVar14, uVar4)` with

```
uVar14 = (uint)(byte)_Dst[9];
uVar4  = (uint)(byte)_Dst[10];
```

Neither `SDDevice` nor `SDController` exposes a key/grid API (verified by symbol inventory), and no division, modulo, or row/col arithmetic exists in the DLL — the raw device keyId passes through untouched. Any grid mapping must be implemented in the host application.

## Device presence (hotplug)

`Stream Station.exe` registers for device notifications at `FUN_1401957b0` (`0x1401957b0–0x140195a09`), calling `USER32::RegisterDeviceNotificationW` (`EXTERNAL:0x1b`) with `QWidget::winId` (`0x36f`); failures log via `KERNEL32::GetLastError` (`0xaea`), `QMessageLogger::critical` / `QDebug`. The filtered GUIDs are delivered at:

| GUID | Address |
|---|---|
| `\\?\USB.{a5dcbf10-6530-11d2-901f-00c04fb951ed}` | `0x142282338` |
| `\\?\HID.{4d1e55b2-f16f-11cf-88cb-001111000030}` | `0x142282368` |

Delivery dispatches via `bool __cdecl StreamDock::nativeEvent(const QByteArray&, void*, long*)` (string at `0x142282330`, class RTTI `StreamDock` at `0x1423420c0`). In the DLL, `onDeviceDisconnected` re-enters the read loop independently of the EXE path.

## Live confirmation (2026-11-06)

Measured on hardware through `ss550-driver`, which closes the gaps that static
analysis could not. Probes: `ss550-probe` `button_probe`, then
`ss550-driver` `button_gate`.

**Collection.** Button reports arrive on **MI_00** (`usage_page 0xffa0`) — the
same collection the driver writes to. **MI_01** (`usage_page 0x0001`,
`usage 0x0006`, the keyboard collection) emitted **zero bytes** across every run,
including runs with 15 presses. Presses are therefore *not* delivered to the OS
as keypresses.

**Report ID on read: none.** This resolves the open question in
`hid-descriptors.md`. Input reports are **512 bytes** with `"ACK"` at
`[0..3]` — there is no leading report ID byte. Output reports are 1025 bytes and
*do* start with `0x00`. The directions are asymmetric; an output buffer must
never be reused for a read.

**Layout**, matching the static analysis byte for byte:

```text
[0..3]  "ACK"
[5..7]  "OK"
[9]     keyId
[10]    keyState: 0x01 pressed, 0x00 released
```

Measured prefix of a top-left press (`41 43 4b` = `ACK`, `4f 4b` = `OK`):

```text
41 43 4b 00 00 4f 4b 00 00 01 01
```

**No firmware debounce.** A click of **150 ms** produced both edges as two
separate reports, below the 199 ms the Qt layer applies. The firmware reports
every edge; any debounce is host-side policy, not a protocol constraint.

**keyId → (row, col), now known.** The raw keyId is neither the paint numbering
nor a simple inverse of it. Three corners were confirmed one press at a time:

| Corner | keyId (read) | paint key (`build_bat`) |
|---|---|---|
| top-left | 1 | 11 |
| top-right | 5 | 15 |
| bottom-left | 11 | 1 |

Both are row-major over the same **3 rows x 5 columns**; reading starts at the
top row, painting starts at the bottom row. The middle row is unchanged. That
flip is implemented and unit-tested as
`ss550_protocol::read_index_to_firmware_key`, so callers only ever see the paint
numbering.

Note the arithmetic is forced, not fitted: top-right = 5 fixes the row width at
5, and bottom-left = 11 fixes the bottom row as the third, so a 3x5 row-major
grid is the only reading consistent with all three points. Column-major is ruled
out — it would give top-right = 13 and bottom-left = 3.

## Known gaps

- Grid orientation is confirmed at three corners plus a full 15-key sweep in
  ascending read order; the interior is derived, not individually pressed.
