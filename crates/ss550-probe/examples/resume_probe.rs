//! Can a control transfer revive the interrupt OUT endpoint?
//!
//! Two failures look identical from the outside and need different fixes:
//!
//! * **Endpoint halted**: the firmware stopped servicing interrupt OUT. Every
//!   write fails until a real power cycle.
//! * **Device suspended**: Windows put the HID device to sleep (selective
//!   suspend). Interrupt transfers fail instantly with `ERROR_GEN_FAILURE`,
//!   while control transfers still work because they wake the device.
//!
//! The discriminator: interleave a `GET_REPORT` (control IN) between failed
//! writes. If a write succeeds right after the control transfer, the device was
//! suspended, not halted - and the fix is to keep it awake, never to power
//! cycle it.
//!
//! Read-only in spirit: it sends `DIS` (wake) and reads `GET_REPORT`, and paints
//! one white key only if the transport comes back.
//!
//! Run: `cargo run -p ss550-probe --example resume_probe`

use hidapi::{HidApi, HidDevice};
use std::time::{Duration, Instant};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;
const ROUNDS: u32 = 5;

fn crt(tag: &[u8; 3]) -> Vec<u8> {
    let mut buf = vec![0u8; 1025];
    buf[1..4].copy_from_slice(b"CRT");
    buf[6..9].copy_from_slice(tag);
    buf
}

fn write_dis(dev: &HidDevice) -> Result<Duration, String> {
    let start = Instant::now();
    match dev.write(&crt(b"DIS")) {
        Ok(_) => Ok(start.elapsed()),
        Err(e) => Err(format!("{}ms {e}", start.elapsed().as_millis())),
    }
}

fn read_report(dev: &HidDevice) -> Result<usize, String> {
    let mut buf = vec![0u8; 1025];
    let start = Instant::now();
    match dev.get_input_report(&mut buf) {
        Ok(n) => {
            let ascii: String = buf[..n.min(64)]
                .iter()
                .map(|b| {
                    if b.is_ascii_graphic() || *b == b' ' {
                        *b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            println!(
                "   GET_REPORT ok bytes={n} elapsed={}ms ascii={ascii:?}",
                start.elapsed().as_millis()
            );
            Ok(n)
        }
        Err(e) => {
            println!(
                "   GET_REPORT ERR elapsed={}ms err={e}",
                start.elapsed().as_millis()
            );
            Err(e.to_string())
        }
    }
}

fn paint_white(dev: &HidDevice) -> Result<(), String> {
    let mut rgb = Vec::with_capacity(100 * 100 * 3);
    for _ in 0..100 * 100 {
        rgb.extend_from_slice(&[0xff, 0xff, 0xff]);
    }
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 90)
        .encode(&rgb, 100, 100, jpeg_encoder::ColorType::Rgb)
        .expect("encode");

    let mut bat = crt(b"BAT");
    bat[11] = (jpeg.len() >> 8) as u8;
    bat[12] = (jpeg.len() & 0xff) as u8;
    bat[13] = 1;
    dev.write(&bat).map_err(|e| format!("BAT: {e}"))?;

    let mut chunk = vec![0u8; 1025];
    chunk[1..1 + jpeg.len()].copy_from_slice(&jpeg);
    dev.write(&chunk).map_err(|e| format!("chunk: {e}"))?;

    dev.write(&crt(b"STP")).map_err(|e| format!("STP: {e}"))?;
    Ok(())
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
    println!(
        "opened interface={:?} usage_page=0x{:04x}",
        info.interface_number(),
        info.usage_page()
    );
    let dev = info.open_device(&api)?;

    for round in 1..=ROUNDS {
        println!("--- round {round} ---");
        match write_dis(&dev) {
            Ok(elapsed) => {
                println!(
                    "   DIS ok {}ms - interrupt OUT is alive",
                    elapsed.as_millis()
                );
                println!("RESULT: the endpoint answers again. It was not a permanent halt.");
                println!("Trying one paint...");
                match paint_white(&dev) {
                    Ok(()) => println!("painted WHITE on firmware key 1 (bottom-left)"),
                    Err(e) => println!("paint failed: {e}"),
                }
                return Ok(());
            }
            Err(e) => println!("   DIS ERR {e}"),
        }

        // Control transfer: wakes a suspended device, does not touch a halted
        // endpoint's state.
        let _ = read_report(&dev);
        std::thread::sleep(Duration::from_millis(200));
    }

    println!();
    println!("RESULT: {ROUNDS} control transfers and the interrupt OUT still refuses.");
    println!("That is a halted endpoint, not a suspended device: power cycle is required.");
    Ok(())
}
