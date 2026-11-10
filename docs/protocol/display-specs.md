# Display Specs — StreamCraft SS550

**Target:** `SDLibrary1.dll` / `Stream Station.exe`. Evidence via Ghidra 12.1.4 REA static analysis.

> **Clean-room note:** This document records observable behavior of the official binary only. No original source code was copied.

## Device surface

| Surface | Notes |
|---|---|
| Key grid | 15 buttons (5 × 3, per product spec) |
| Side screen | Supported via the secondary-screen picture branch (`getSecondaryScreenPicInfo`, see below) |

The side screen is fed by the `secondary-screen` path with a per-image header, not by a separate command.

## Secondary-screen header

`getSecondaryScreenPicInfo` at `0x180019f00` prepends one of two headers before chunking:

| Type | Header | Fields |
|---|---|---|
| `0x12` | 0x400-byte `"M_V…"` | (not further specified) |
| other | 0x20-byte `"CRTDRA"` | `[6..9]` = BE32(size + `0x20`), `[10]` = `+0x0C`, `[11..12]` = width, `[13..14]` = height, `[15..16]` = x, `[17..18]` = y |

## Pixel formats

`DataFormatConversion::imageToBin` (`0x180004270`) honors Qt's `QImage::Format` enum:

| Format | Output |
|---|---|
| 4 (RGB32) | ARGB8888 (4 bytes/px) |
| 7 (RGB16) | **RGB565** (2 bytes/px) |
| 13 (RGB888) / 29 (RGBX8888) | RGB888 (skips every 4th byte) |

> The RGB565 branch is the "565" code that the EXE does not contain — it lives in the DLL, code-side only.

## Sizes (empirical, runtime-observed)

The Mirabox WebSocket-SDK accepts 126×126 PNGs and scales them to the background layer at runtime, and an 800×480 black PNG was accepted and scaled into the background layer. The native geometry constants for the 15 keys and the side screen are **not visible statically** — enumerate a live device (or observe one upload through the EXE → DLL queue) to recover them.

## Brightness, sleep, and lamp control

| Capability | API (DLL) |
|---|---|
| Brightness pack | `addAdjustBrightnessPack(int, bool)` |
| Screen off | `addScreenOffPack()` / `sendScreenOffPack()` |
| Wake | `sendWakeUpScreenPack()` |
| Sleep timer | `resetScreenOffTimer(qint64)` |
| State query | `isDeviceSleeping()` / `getCurrentBrightness()` / `increaseBrightness()` / `decreaseBrightness(uint)` |
| Dark/timer lamps | `addLampControlCommand(const QList<uchar>&, bool)` and `addLampControlCommand(const QString&, bool)` |

Model capability flags (`setSupportDisplayLogoAfterUpload`, `setSupportAdjustBrightness`, `setSupportGetDeviceVersion`, `setIsLampControlDevice`) are set by the host per model, not probed from the hardware.

## Known gaps

- Key-grid and side-screen native pixel geometry (no constants found statically).
- Which pixel format the EXE selects for the SS550 (RGB565 vs RGB888) is caller-side.
- `ImageStruct` field layout (no RTTI) — see `image-upload.md`.
