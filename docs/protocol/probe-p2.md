# Probe P2 — Enumerate + Report Lengths (Gap 2)

**Risk:** LOW — read-only. No writes. Reads descriptor/strings only.

**Purpose:** Fix the payload chunk size (64 vs 65) for every later write.

**Evidence base** — `hid-descriptors.md`, `image-upload.md`:
- `this+0x30 = hid_get_output_report_lenght(h)`, `this+0x34 = this+0x30 + 1`.
- Fallback when the query returns `< 1`: `+0x30 = 0x40` (64), `+0x34 = 0x41` (65).
- `K992` returns `-1`.

## Harness

Minimal Rust binary with the `hidapi` crate. One-shot, no writes.

```rust
use hidapi::HidApi;

fn main() {
    let api = HidApi::new().expect("hidapi init");
    let devices = api.device_list(0x0200, 0x3000);
    for d in &devices {
        println!(
            "path={:?} vid=0x{:04x} pid=0x{:04x} usage_page=0x{:04x} usage=0x{:04x} serial={:?} product={:?} manufacturer={:?}",
            d.path().to_str(), d.vid(), d.pid(), d.usage_page(), d.usage(),
            d.serial_number(), d.product_string(), d.manufacturer_string()
        );
    }
    // Open the first match and read lengths.
    let dev = api.open_path(devices[0].path())?;
    println!("input_report_len = {}", dev.input_report_length());
    println!("output_report_len = {}", dev.output_report_length());
}
```

Also run a small C helper calling `HidP_GetCaps` (via `SetupAPI`/`HidD`) and log `OutputReportByteLength` and `InputReportByteLength` (`HidD`/`HidP` availability on Windows is assumed for this helper).

## What to log

| Field | Source |
|---|---|
| device path, `vendor_id`, `product_id`, `usage_page`, `usage`, `serial_number` | `hid_enumerate(0x0200, 0x3000)` |
| `manufacturer`, `product`, `serial` strings | `hid_get_*_string` |
| `hid_get_input_report_lenght()` | DLL name (vendor spelling) |
| `hid_get_output_report_lenght()` | DLL name (vendor spelling) |
| `OutputReportByteLength`, `InputReportByteLength` | `HidP_GetCaps` |

## Decision rule

- `n_out == 64` → payload chunk 64, total 65.
- `n_out == 65` → payload chunk 65 (already includes ID per hidapi convention, assumption) → total 65; the DLL's `+0x34` calc then double-counts and the Rust driver uses 65 total.
- `n_out == -1` or `product == "K992"` → fallback 64/65.
- Cross-check: `n_out == OutputReportByteLength` = total including ID; `n_out == OutputReportByteLength - 1` = payload only.

## Run order

G1 (this probe) → G2 (P1B, needs the buffer size from here).
