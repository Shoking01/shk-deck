# HID Descriptors — StreamCraft SS550

**Target:** `SDLibrary1.dll` (Qt5 C++ device layer used by Stream Station). Evidence via Ghidra 12.1.4 REA static analysis of `C:\Program Files (x86)\Stream Station\SDLibrary1.dll` (PE32+, x86_64, 879 exports, sha256 `7d65f3c9…5a7a8`) and `Stream Station.exe` (sha256 `9992d38d…e8458e`).

> **Clean-room note:** This document records observable behavior of the official binaries only. No original source code was copied. All addresses refer to the analyzed DLL at image base `0x180000000`.

## Transport

| Aspect | Observed fact |
|---|---|
| Transport library | **hidapi statically linked** inside `SDLibrary1.dll` |
| Imported USB/HID DLLs | **None** — 21 import DLLs are Qt5, MSVCRT, Kernel32/User32 only |
| APIs used | `hid_init`, `hid_enumerate`, `hid_free_enumeration`, `hid_open`, `hid_open_path`, `hid_exit`, `hid_error`, `hid_write`, `hid_read_timeout`, `hid_get_manufacturer_string`, `hid_get_product_string`, `hid_get_serial_number_string`, `hid_get_indexed_string`, `hid_get_output_report_lenght` (vendor spelling), `hid_get_input_report_lenght` |
| Enumeration function | `SDDevice::openHidDevice(vid,pid,serial,usage_page,usage,report_id)` at `0x18001b580` — calls `hid_enumerate(vid,pid)`, matches `vendor_id@+0x08`, `product_id@+0x0A`, optional `usage_page@+0x30` / `usage@+0x32` (`-1` = any), 3rd parameter is the **serial-number string** compared against `dev->serial_number@+0x10`, then `hid_open_path(dev->path@+0x00)` |
| No custom USB code | The EXE imports no `hid.dll` / `setupapi.dll` / `libusb-1.0.dll`; all 63 `SDDevice` methods arrive via 226 `SDLibrary1.dll` imports. A clean-room Rust driver therefore uses the `hidapi` crate only. |

## Production defaults (recovered constants)

Source: `SDDevice::openHidDevice` 4-argument overload at `0x18001acc0` (ordinal 433), called from `SDController::onDeviceConnected` at `0x1800137f0` (no in-DLL callers; invoked from the EXE).

| Item | Value |
|---|---|
| Report ID | **`0x00`** — `this[0x98] = 0` |
| Usage page | **`0xFFFFFFFF` (−1, "any")** — `*(undefined8*)(this+0x48) = 0xffffffffffffffff` |
| Usage | **`0xFFFFFFFF` (−1, "any")** — same instruction (covers `this+0x48` and `this+0x4C`) |
| Payload chunk size | `this+0x30 = hid_get_output_report_lenght(h)` at runtime |
| Total report size | `this+0x34 = this+0x30 + 1` at runtime |
| Fallback when length query < 1 | `this+0x30 = 0x40` (64-byte payload), `this+0x34 = 0x41` (65-byte report) |
| K992 special case | report length −1 when the product string is `"K992"` |
| Object fields | `+0x90` = `hid_device*`, `+0x30` = chunk size, `+0x34` = report size, `+0x98` = report-ID byte, `+0x99` = open flag, `+0x48` usage page, `+0x4C` usage |

Report length is **runtime-only** — it is not a constant in the DLL — so `64`/`65` is a fallback, not a recovered device value.

## VID / PID

The DLL contains **no hardcoded VID/PID**. Evidence:
1. `openHidDevice` has exactly one internal caller (`FUN_18001a5b0`, a reconnect path re-passing stored `+0x38`/`+0x3C`); the values `0x0200`/`0x3000`/`512`/`12288` never appear as literals feeding it.
2. `SDController::onDeviceConnected` has an **empty caller list** inside the DLL — it is invoked from the EXE — and calls the 4-argument overload with only register arguments (no stack parameters), so `0x0200`/`0x3000` reach the DLL only as runtime arguments supplied by the EXE.

For the Redragon StreamCraft SS550 the runtime VID:PID pair is `0200:3000` (bus-reported descriptor `HOTSPOTEKUSB HID DEMO`, per runtime enumeration). SS550V3 corresponds to the `293V3` class in the Mirabox SDK.

## Model gate

`FUN_180034f40` is the device-model gate: **93 case-insensitive `QString` comparisons** (`QString::compare_helper(..., -1, 1)`), shared success path `MOV AL,1; RET` at `0x180036346`. Callers `onDeviceConnected` / `onDeviceDisconnected` require a nonzero result — a device **must** match the list or it is dropped.

The gate includes **both SS550 names**: `SS550V3` (entry 6) and `SS-550` (entry 10) are present and pass. Other listed names (entry inspection alphabetized): `H3`, `K992`, `LMA`, `StreamDock-300`, `StreamDock[0108D]`, `StreamDock[293]`, `StreamDock[295]`, `StreamDock[296]`, `StreamDock[321D]`, `StreamDock2961`, `XF-A3501`. `LMA` appears in the DLL in other contexts (`+0x70` marker) but was **not** found in this list.

## Feature and input reports

All feature-report helpers are **probe-only** — results are appended to a `QByteArray` that is immediately destroyed, so they are not used for identity or firmware:

| Function | Behaviour |
|---|---|
| `getHidFeatureReport` `0x180019b40` | Allocates `feat_len + 1`, sets `buf[0] = this[0x98]` (report ID), calls `hid_get_feature_report`, discards. |
| `sendHidFeatureReport` `0x18001d820` | Same allocation; payload `buf[0] = reportID`, `*(uint*)(buf+1) = 0xAA0` → wire `[00][A0 0A 00 00]`. Contains **no `"CRT"`** marker — not a protocol command; appears to be a wake/init frame. |
| `getHidInputReport` `0x180019c50` | Calls `hid_get_input_report`, appends `buf+1`. |
| `getHidReportDescriptor` `0x180019d60` | Reads into a 4096-byte buffer and discards (dead end). |
| `debugHidDeviceInformation` `0x180018d20` | Debug print helper; does not expose a fixed descriptor. |

## Known gaps (require a live device)

1. **Report-ID stripping on read** — the reader sets `*_Dst = this[0x98]` and dispatches on byte 0 = `'H'/'A'/'C'/'e'` (ASCII), which is only consistent with a hidapi build that returns reads **without** the report-ID byte. The exact buffer layout for a Rust driver needs one live `hid_write`/`hid_read` round-trip to confirm.
2. **Report length at runtime** — `hid_get_output_report_lenght` returns either payload or total bytes; confirm against `HidP_GetCaps().OutputReportByteLength` to decide chunk = 64 vs 65.
3. The actual USB HID report descriptor bytes are **not** published here — enumerate the live device to read them.

## Live-measured constants (SS550, measured)

Report lengths obtained with `HidP_GetCaps` against a real deck (`crate ss550-probe`, `--descriptor` / P2):

| Direction | Byte length | Payload (minus report ID) |
|---|---|---|
| Input | **513** | 1 B usage `0x03` page `0xFFA1` + 511 B usage `0x04` page `0xFFA0` |
| Output | **1025** | 1 B usage `0x05` page `0xFFA0` + 1023 B usage `0x06` page `0xFFA0` |

The `64`/`65` fallback described above belongs to other StreamDock models — **not** the SS550.

### Device identity (report ID 0, control path)

Reading report ID `0` with `hid_get_input_report` using a buffer at least as large as the output report returns exactly 18 bytes:

```
00 | "V3.SS_550.01.002" | 00
```

Model `SS_550`, firmware `V3`, version `01.002`. This is a fixed identity report: it is returned for report ID `0` regardless of the command that preceded it.

### The answer path is the control transfer

Across 11 probe sessions every `hid_read_timeout` (interrupt IN) returned **0 bytes**, including immediately after commands that produced a control-path reply. Answers must be fetched with `hid_get_input_report` — the same call `getHardwareFirmwareVersion` @ `0x180019800` uses.

Two practical notes from live use:

- The control buffer must be sized to the **output** report length (1025). A 513-byte input-sized buffer fails with `ERROR_GEN_FAILURE` while the 1025-byte buffer returns data.
- Control reads fail intermittently with `ERROR_GEN_FAILURE` (`0x1F`) — roughly one run in three — so the driver needs a retry with backoff.
