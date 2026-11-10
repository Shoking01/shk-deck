//! Does writing `CRT|VER` halt the endpoint?
//!
//! Hypothesis: the firmware rejects a written `VER` (the version is served by
//! `GET_REPORT` as a static identity report, no command needed), and a rejected
//! report halts the interrupt OUT endpoint for every later write.
//!
//! Sequence, in order:
//!   1. `DIS`      must succeed (proves the transport is healthy)
//!   2. `LIG 100`  must succeed
//!   3. `VER`      the suspected poison
//!   4. `DIS`      if this fails, step 3 halted the endpoint
//!   5. `LIG 100`  only runs if step 4 survived
//!   6. paint white on firmware key 1 (bottom-left) and red on key 15 (top-right)
//!
//! Run: `cargo run -p ss550-probe --example test_ver_halt`

use hidapi::HidApi;
use std::time::Instant;

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;

fn crt(tag: &[u8; 3], fields: &[(usize, u8)]) -> Vec<u8> {
    let mut buf = vec![0u8; 1025];
    buf[1..4].copy_from_slice(b"CRT");
    buf[6..9].copy_from_slice(tag);
    for (off, v) in fields {
        buf[1 + off] = *v;
    }
    buf
}

fn bat(image_len: usize, firmware_key: u8) -> Vec<u8> {
    crt(
        b"BAT",
        &[
            (10, (image_len >> 8) as u8),
            (11, (image_len & 0xff) as u8),
            (12, firmware_key),
        ],
    )
}

fn solid_jpeg(r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(100 * 100 * 3);
    for _ in 0..100 * 100 {
        rgb.extend_from_slice(&[r, g, b]);
    }
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 90)
        .encode(&rgb, 100, 100, jpeg_encoder::ColorType::Rgb)
        .expect("encode");
    jpeg
}

fn send(dev: &hidapi::HidDevice, name: &str, pack: &[u8]) -> bool {
    let start = Instant::now();
    match dev.write(pack) {
        Ok(n) => {
            println!(
                "{name:<10} ok   bytes={n} elapsed={}ms",
                start.elapsed().as_millis()
            );
            true
        }
        Err(e) => {
            println!(
                "{name:<10} ERR  elapsed={}ms err={e}",
                start.elapsed().as_millis()
            );
            false
        }
    }
}

fn paint(dev: &hidapi::HidDevice, firmware_key: u8, rgb: (u8, u8, u8), label: &str) -> bool {
    let jpeg = solid_jpeg(rgb.0, rgb.1, rgb.2);
    if !send(dev, "BAT", &bat(jpeg.len(), firmware_key)) {
        return false;
    }
    let mut chunk = vec![0u8; 1025];
    chunk[1..1 + jpeg.len()].copy_from_slice(&jpeg);
    if !send(dev, "chunk", &chunk) {
        return false;
    }
    if !send(dev, "STP", &crt(b"STP", &[])) {
        return false;
    }
    println!("painted {label} on firmware key {firmware_key}");
    true
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

    println!("--- control phase: prove the transport is healthy ---");
    if !send(&dev, "DIS", &crt(b"DIS", &[])) {
        println!("aborting: transport already down before the test");
        return Ok(());
    }
    if !send(&dev, "LIG100", &crt(b"LIG", &[(10, 100)])) {
        return Ok(());
    }

    println!("--- suspected poison: write CRT|VER ---");
    send(&dev, "VER", &crt(b"VER", &[]));

    println!("--- after VER ---");
    let survived = send(&dev, "DIS", &crt(b"DIS", &[]));
    if !survived {
        println!("RESULT: writing VER halts the endpoint. firmware_version() must not write.");
        return Ok(());
    }
    send(&dev, "LIG100", &crt(b"LIG", &[(10, 100)]));

    println!("--- render through the interrupt OUT path ---");
    if !paint(&dev, 1, (0xff, 0xff, 0xff), "WHITE bottom-left") {
        return Ok(());
    }
    if !paint(&dev, 15, (0xff, 0x00, 0x00), "RED top-right") {
        return Ok(());
    }
    println!("RESULT: VER is safe and the driver path works. Observe both keys.");
    Ok(())
}
