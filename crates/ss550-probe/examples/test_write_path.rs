//! Transport check: the documented image path over `write` (interrupt OUT),
//! with per-report timing.
//!
//! The reference software uses `hid_write` for this path, not control
//! `Set_Report`. hidapi's Windows `hid_write` has a fixed 1000 ms timeout, so
//! a report that takes longer fails with ERROR_IO_PENDING (0xE5).
//!
//! Run: `cargo run -p ss550-probe --example test_write_path`

use hidapi::HidApi;
use std::time::Instant;

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;

/// Builds a `CRT` pack exactly as `ss550-protocol` does.
fn crt(tag: &[u8; 3], fields: &[(usize, u8)]) -> Vec<u8> {
    let mut buf = vec![0u8; 1025];
    buf[1..4].copy_from_slice(b"CRT");
    buf[6..9].copy_from_slice(tag);
    for (off, v) in fields {
        buf[1 + off] = *v;
    }
    buf
}

fn white_jpeg() -> Vec<u8> {
    let rgb = vec![0xffu8; 100 * 100 * 3];
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 90)
        .encode(&rgb, 100, 100, jpeg_encoder::ColorType::Rgb)
        .expect("encode");
    jpeg
}

fn send(dev: &hidapi::HidDevice, name: &str, pack: &[u8]) -> Result<(), String> {
    let start = Instant::now();
    match dev.write(pack) {
        Ok(n) => {
            println!(
                "{name:<10} ok   bytes={n} elapsed={}ms",
                start.elapsed().as_millis()
            );
            Ok(())
        }
        Err(e) => {
            println!(
                "{name:<10} ERR  elapsed={}ms err={e}",
                start.elapsed().as_millis()
            );
            Err(e.to_string())
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = HidApi::new()?;
    let devices: Vec<_> = api
        .device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
        .collect();
    let info = devices
        .iter()
        .find(|d| d.usage_page() == VENDOR_USAGE_PAGE)
        .or_else(|| devices.first())
        .ok_or("device not found")?;
    println!("usage_page=0x{:04x}", info.usage_page());
    let dev = info.open_device(&api)?;

    let jpeg = white_jpeg();
    println!("jpeg_bytes={}", jpeg.len());

    // firmware key 1 == OpenDeck key 0 == bottom-left.
    let steps: Vec<(&str, Vec<u8>)> = vec![
        ("DIS", crt(b"DIS", &[])),
        ("LIG100", crt(b"LIG", &[(10, 100)])),
        (
            "BAT",
            crt(
                b"BAT",
                &[
                    (10, (jpeg.len() >> 8) as u8),
                    (11, (jpeg.len() & 0xff) as u8),
                    (12, 1),
                ],
            ),
        ),
    ];

    let total = Instant::now();
    for (name, pack) in &steps {
        if send(&dev, name, pack).is_err() {
            println!("aborting: transport refused the report");
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let mut chunk = vec![0u8; 1025];
    chunk[1..1 + jpeg.len()].copy_from_slice(&jpeg);
    if send(&dev, "chunk", &chunk).is_err() {
        println!("aborting: transport refused the payload");
        return Ok(());
    }
    if send(&dev, "STP", &crt(b"STP", &[])).is_err() {
        return Ok(());
    }
    println!("sequence_total={}ms", total.elapsed().as_millis());

    println!(">>> OBSERVE: bottom-left key should show WHITE. Deck left as-is on purpose.");
    std::thread::sleep(std::time::Duration::from_secs(5));
    println!(">>> done, state left painted for inspection");
    Ok(())
}
