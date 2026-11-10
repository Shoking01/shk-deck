# Probe P1B — Single HAN Write + 3000 ms Read (Gap 1)

**Risk:** LOW — one verified handshake pack only. No unknown opcode. Worst case the device ignores it.

**Purpose:** Decide the read-side report-ID stripping: must a Rust driver drop byte 0 on reads?

**Evidence base** — `hid-descriptors.md`, `input-events.md`:
- `readDataFromHidDevice` at `0x18001bfb0` sets `*_Dst = this[0x98] = 0x00` then dispatches on ASCII `'H'`/`'A'`/`'C'`/`'e'` at byte 0.
- Read timeout is 3000 ms.
- `HANDLER` = handshake ACK → `sendHandshakePack`.
- `addHandshakePack` at `0x180015e00` builds a `HAN` pack (bytes 0–2 = `"HAN"`, no `CRT` prefix) as the only verified safe first write.

## Harness

Minimal Rust binary with the `hidapi` crate. One-shot, after P2 lengths are known.

```rust
use hidapi::HidApi;

fn main() {
    let api = HidApi::new().expect("hidapi init");
    let devices = api.device_list(0x0200, 0x3000);
    let dev = api.open_path(devices[0].path())?;

    // HAN pack: buf[0] = report ID 0x00, buf[1..3] = "HAN", zero-padded to total report size.
    let total = dev.output_report_length();  // from P2
    let mut han = vec![0u8; total];
    han[0] = 0x00;
    han[1..4].copy_from_slice(b"HAN");
    let n = dev.write(&han).expect("write HAN");
    println!("write returned {n} bytes");

    // One 3000 ms read; log n, buf[0..32], and whether buf[0] is 0x00.
    let mut buf = vec![0u8; dev.input_report_length()];
    let m = dev.read_timeout(&mut buf, 3000).expect("read");
    println!("read returned {m} bytes; buf[0..32] = {:02x?}", &buf[..m.min(32)]);
    println!("buf[0] is 0x00: {}", buf[0] == 0x00);

    // Wait 3000 ms for the "HANDLER" reply; repeat once after 500 ms.
    std::thread::sleep(std::time::Duration::from_millis(3000));
    let m2 = dev.read_timeout(&mut buf, 500).expect("read 2");
    println!("second read returned {m2} bytes; buf[0..32] = {:02x?}", &buf[..m2.min(32)]);
}
```

## What to log

| Field | Source |
|---|---|
| `n_out`, `n_in` from P2 | buffer size for the write |
| write return value | `hid_write` |
| read return, `len`, `buf[0..32]` hex, `buf[0]`-is-`0x00` bool | `hid_read_timeout` (3000 ms, then 500 ms) |
| `HANDLER` presence | reply sniff |

## Decision rule

- `buf[0] == 0x00 && buf[1] == 'H'` (i.e. `\x00HANDLER…`) → **standard hidapi layout**; the Rust driver strips byte 0.
- `buf[0] == 'H'` directly (`HANDLER…` at offset 0) → **vendor build strips the ID**; the Rust driver does not strip.
- Any other `buf[0]` → log raw and stop; do not retry with a larger buffer.

## Run order

G1 (P2, buffer size) → G2 (this probe). Cap the session at 5 minutes of device time; stop on timeout or unexpected opcode echo.
