//! Dumps every HID collection the SS550 exposes and shows which one the probe
//! would open, then attempts one `DIS` on it.
//!
//! Why this exists: `usage_page == 0xffa0` is the writable vendor collection
//! (MI_00). The probe falls back to `devices.first()` when that page is absent,
//! and the fallback may be MI_01 - the keyboard collection, which has no
//! interrupt OUT endpoint. Writing there fails immediately with `0x1F`, which
//! is a completely different symptom from a stalled endpoint (`0xE5` after
//! ~1000 ms) and must not be diagnosed as one.
//!
//! Run: `cargo run -p ss550-probe --example enumerate`

use hidapi::HidApi;
use std::time::Instant;

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = HidApi::new()?;
    let matches: Vec<&hidapi::DeviceInfo> = api
        .device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
        .collect();

    println!("VID/PID matches: {}", matches.len());
    for (i, d) in matches.iter().enumerate() {
        println!(
            "  [{i}] interface={:?} usage_page=0x{:04x} usage=0x{:04x} product={:?}",
            d.interface_number(),
            d.usage_page(),
            d.usage(),
            d.product_string()
        );
        println!("      path={}", d.path().to_string_lossy());
    }

    let chosen = matches
        .iter()
        .copied()
        .find(|d| d.usage_page() == VENDOR_USAGE_PAGE)
        .or_else(|| matches.first().copied());

    let Some(info) = chosen else {
        println!("no device found");
        return Ok(());
    };

    let is_vendor = info.usage_page() == VENDOR_USAGE_PAGE;
    println!();
    println!(
        "chosen: interface={:?} usage_page=0x{:04x} vendor_collection={is_vendor}",
        info.interface_number(),
        info.usage_page()
    );
    if !is_vendor {
        println!("WARNING: the vendor collection 0xffa0 is ABSENT. This fallback has no");
        println!("interrupt OUT endpoint; a write here fails instantly with 0x1F and is");
        println!("NOT evidence that the deck is halted.");
    }

    let dev = info.open_device(&api)?;
    let mut pack = vec![0u8; 1025];
    pack[1..4].copy_from_slice(b"CRT");
    pack[6..9].copy_from_slice(b"DIS");

    let start = Instant::now();
    match dev.write(&pack) {
        Ok(n) => println!("DIS ok bytes={n} elapsed={}ms", start.elapsed().as_millis()),
        Err(e) => println!("DIS ERR elapsed={}ms err={e}", start.elapsed().as_millis()),
    }
    Ok(())
}
