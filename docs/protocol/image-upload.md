# Button Image Upload — StreamCraft SS550

**Target:** Redragon StreamCraft SS550 v3 (`VID 0x0200` / `PID 0x3000`, product
string `HOTSPOTEKUSB HID DEMO`, firmware `V3.SS_550.01.002`).

**Status of this document:** the single-image path below is **verified live**
(2026-11-05, all 15 keys render solid white with the correct sequence). Everything
else is recorded as static analysis or open-source reference, and is labelled as
such.

> **Clean-room note:** This document records observable behaviour and constants
> recovered from public sources only. No original vendor source code was copied.
> The protocol reference used is **`4ndv/mirajazz`** (MPL-2.0) and
> **`MMonkeyKiller/opendeck-ss550`** (GPL-3.0): independent community
> reimplementations of this device family, not the vendor's own code.

## Reference implementation

| Repo | Role | Relevance |
|---|---|---|
| [`4ndv/mirajazz`](https://github.com/4ndv/mirajazz) | Rust HID library for Mirabox/Ajazz devices | **Drives `0x0200:0x3000`** in production via OpenDeck |
| `4ndv/mirajazz` `src/device.rs`, `src/images.rs` | Wire protocol v1/v2/v3 | Byte-exact source of truth for this document |
| [`MMonkeyKiller/opendeck-ss550`](https://github.com/MMonkeyKiller/opendeck-ss550) | OpenDeck plugin for SS550 / SS550v3 | Device constants: 3×5 grid, protocol version 2, image format |

Our device maps to `Kind::SS550V3` → **protocol version 2** → `packet_size = 1024`
(protocol v1 devices use 512).

## Transport

Measured on the physical deck, matching the reference values:

| Property | Value |
|---|---|
| Interface | MI_00, usage page `0xFFA0` (65440), usage `1` |
| Output report length | **1025 B** (report ID + 1024 payload) |
| Input report length | 513 B |
| Report ID | implicit `0x00` (byte 0 of every report) |
| Writable/readable | Only MI_00. Windows denies **both** on MI_01 (keyboard, usage page 0x0001) |

MI_01 being denied by Windows is the reason all probing happens on MI_00.

## The single-image path (verified live)

Order matters: the **`BAT` header precedes the image data**.

```
1. [0x00] "CRT" 00 00 "DIS"                    wake
2. [0x00] "CRT" 00 00 "LIG" 00 00  00          brightness 0   (initialize step 2)
   -- repeat 3..4 for each key -----------------------------
3. [0x00] "CRT" 00 00 "BAT" 00 00 <len_hi> <len_lo> <key+1>
4. [0x00] <up to 1024 bytes of JPEG>            chunk, zero-padded to 1025
   --------------------------------------------------------
5. [0x00] "CRT" 00 00 "STP"                    flush — ONE, after every key
6. [0x00] "CRT" 00 00 "LIG" 00 00  64          restore brightness
```

Every report is padded with zeros to 1025 bytes. There is **no per-chunk length
field** — the device relies on the fixed report size.

### Details

- `BAT` = `0x42 0x41 0x54`, placed at wire bytes `[6..8]` (payload bytes `[5..8]`).
- `len` is a **BE16** of the JPEG byte length at wire bytes `[11..12]`.
- The key index is **1-based on the wire** (`key + 1`); device key 0 → wire 1.
- Image payload is split into 1024-byte chunks, each prefixed with the report
  ID `0x00` and zero-padded to 1025.
- `STP` is emitted **once**, after every image has been written. It is the
  `flush()` commit — without it nothing appears.
- No reply is expected between any of these steps. The reference library never
  reads the input report on this path, so an ACK handshake is **not** part of
  it.

### Image encoding

From `mirajazz::images::convert_image_with_format`:

1. `resize_exact(100, 100, Lanczos3)`
2. rotate **180°**
3. mirror: **none**
4. `into_rgb8()`
5. **JPEG, quality 90** (not RGB565, not BMP)

The native button icon size for this family is **100×100**, quality 90. The
`opendeck-ss550` plugin confirms it as `ImageFormat { mode: JPEG, size: (100,
100), rotation: Rot180, mirror: None }`.

> Note: rotation is a no-op for a solid colour. Rendering a *non*-symmetric
> image is how the rotation and mirror flags get proven, not a white square.

### Physical key index mapping (confirmed live)

El firmware SS550 v3 usa coordenadas con el origen en la esquina **inferior
izquierda**:

| Opencade key (0-14) | (row, col) | Firmware key | Posición física |
|---|---|---|---|
| 0 | (2,0) | 1 | abajo-izq |
| 1 | (2,1) | 2 | abajo-2º |
| 2 | (2,2) | 3 | abajo-centro |
| 3 | (2,3) | 4 | abajo-4º |
| 4 | (2,4) | 5 | abajo-der |
| 5 | (1,0) | 6 | medio-izq |
| 6 | (1,1) | 7 | medio-2º |
| 7 | (1,2) | 8 | medio-centro |
| 8 | (1,3) | 9 | medio-4º |
| 9 | (1,4) | 10 | medio-der |
| 10 | (0,0) | 11 | arriba-izq |
| 11 | (0,1) | 12 | arriba-2º |
| 12 | (0,2) | 13 | arriba-centro |
| 13 | (0,3) | 14 | arriba-4º |
| 14 | (0,4) | 15 | arriba-der |

La tabla del plugin `opendeck-ss550`:

```rust
[10, 11, 12, 13, 14, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4]
```

Este array convierte OpenDeck key → firmware key. El valor `0` es un placeholder
el firmware ignora key 0.

## Commands (payload bytes, after the report ID)

Byte 0–2 = `"CRT"` (the `HAN` handshake is the exception: no prefix), byte 4 =
variant, bytes 5–7 = the 3-character command tag. Offsets below are **payload**
offsets, so wire offsets are +1.

| Tag | Payload | Meaning | Source |
|---|---|---|---|
| `DIS` | — | wake / screen on | live + reference |
| `HAN` | — | handshake (no `CRT` prefix) | static |
| `LIG` | `[10]` = 0–100 | backlight brightness | **live: `--bright 30` visibly dims the deck** |
| `BAT` | `[10..11]` = BE16 len, `[12]` = key+1 | plain image header, **before** the data | **live: renders** |
| `STP` | — | flush / commit | **live** |
| `CLE` | `[11]` = key+1 (`0xFF` = all) | clear button image | reference |
| `MOD` | `[10]` = `'0'+mode` | device mode | reference |
| `QUCMD` | `0x1F 0x11 0x00 <vib> 0x00 0x11` | N4 Pro vibration | reference |
| `LBLIG` | `[11]` = percent | knob-LED brightness | reference |
| `SETLB` | `[r,g,b]` per LED | knob-LED colour | reference |
| `CONNECT` | — (`CONNECT` spans payload 5–11) | keep-alive | static |
| `OLG` | `[4..7]` = BE32 size, `[12]` = key | logo (still/320×240 image) | static, unverified live |

### Static-only paths (recovered from `SDLibrary1.dll`, not reproduced live)

`insertSettings` / `appendSendData` pack a whole profile into one queue that the
write thread drains:

- key item (dispatch `0x07`), optional `DEV` settings item (`0x0A`), one
  `ULEND` item (`0x0D`) closing the batch.
- `sendSettingsPackHead` `0x18001e410` emits **`ICO`** with variant `0x05`
  (primary) or `0x0F` (secondary), BE16 size at `[9..10]`, `ImageStruct+0x04`
  at `[11]`, `ImageStruct+0x08` at `[12]`.
- Per item: `ICO` head → primary chunks → secondary chunks → a second `ICO`
  with variant `0x0F` for the secondary payload.
- The write thread waits on `QWaitCondition` between steps with timeouts of
  500/1000/3000/5000/10000/20000/30000 ms per payload type; a timeout logs
  `"Timeout"` and continues.
- `writeDataToHidDevice` `0x18001f8b0` drains queue `this+0x110` (images) and
  `this+0x118` (settings); `hid_write` in `this+0x30`-sized chunks with up to 3
  retries and 10 ms backoff.

**`ICO` and `ULEND` belong to this profile path, not to a plain image.** We
sent them to a single image and the deck stayed black.

## What we got wrong, and why

Recordado para evitar repetir estos errores.

1. **El tag es `BAT` (0x42 0x41 0x54), no `ABT`.** Transcribimos el disassembly al revés.
2. **El header `BAT` va primero, no después.** Nuestro primer probe envió ICO → datos → ABT.
3. **El índice de key es 1-based en el wire** — el firmware usa keys 1..15.
4. **Formato de imagen: JPEG 100×100 calidad 90, rotación 180°, mirror none** — no 144×144, no RGB565.
5. **Ningún ACK entre chunks** — el path de imagen simple no espera replies.
6. **El probe de `initialize()` deja brillo en 0** — hay que subirlo después.
7. **El mapeo de key no es lineal** — hay que aplicar la tabla del plugin OpenDeck.
8. **El origen del grid está en la esquina inferior izquierda** — el firmware
   cuenta desde abajo, no desde arriba como el grid típico.

## Failure modes (read before writing to hardware)

**A rejected report halts the USB endpoint.** The firmware does not return a
protocol error; the transport simply stops accepting output. After that every
write fails — first `0x1F` (`ERROR_GEN_FAILURE`), then `0xE5` — for every host,
including the vendor app, until the device is power cycled (unplug 60 s; a 5 s
replug is not enough).

Observed trigger: sending `STP` **before** the payload chunk. The firmware sees
a commit for an open, empty transfer, rejects it, and the endpoint halts.

Rules:

- Always `BAT` → payload chunks → exactly **one** `STP`. Never a bare `STP`.
- On any write error, stop. Do not retry: every rejected report keeps the
  endpoint halted and extends the outage.
- A "VER works" check proves nothing. `get_input_report` returns the static
  identity report `.V3.SS_550.01.002.` with zero writes sent.
- Full recovery procedure: [`docs/troubleshooting/deck-stuck.md`](../troubleshooting/deck-stuck.md).

## Open questions

- **Secondary screen / 320×240 path** (`OLG`, dispatch 2/3/4/20) sigue siendo un path
  `insertSettings` sin test en vida.
- **Input events**: el deck nunca devuelve bytes en interrupt-IN; el SDK oficial
  usa WebSocket para eventos.
- **`StreamDockDeviceAddress.json`**: ausente en esta máquina, el `DEV` item del
  profile push es desconocido.