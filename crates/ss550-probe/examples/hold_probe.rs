//! Does continuous traffic keep the deck alive, or does it reset anyway?
//!
//! `identity_timeline` measured, with **zero writes**, on a different USB port:
//!
//! ```text
//! t=+  4921ms  both collections present
//! t=+ 73600ms  vendor interface (iface 0) disappears
//! t=+ 73696ms  everything disappears
//! t=+ 77925ms  both come back
//! ```
//!
//! 68.7 s of stability is far longer than the 1-3 s seen on the previous port,
//! and the reset happened with no host traffic at all. So the open question is
//! no longer "how long is the window" but "what triggers the reset".
//!
//! This probe connects **now** — no replug needed while the deck is healthy —
//! paints both corners, then sends one harmless `DIS` every 500 ms for three
//! minutes. Three minutes deliberately crosses the 68.7 s mark.
//!
//! * survives all 180 s  -> traffic suppresses the reset; the driver needs a
//!   heartbeat, and idleness is the trigger.
//! * dies near 68 s anyway -> the reset is on a timer we do not control, and
//!   `connect()` must be able to reconnect transparently.
//!
//! Run: `cargo run -p ss550-probe --example hold_probe`

use hidapi::{HidApi, HidDevice};
use std::io::Write;
use std::time::{Duration, Instant};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;
const BEAT: Duration = Duration::from_millis(500);
const TARGET: Duration = Duration::from_secs(180);

fn say(args: std::fmt::Arguments<'_>) {
    println!("{args}");
    let _ = std::io::stdout().flush();
}

macro_rules! say {
    ($($t:tt)*) => { say(format_args!($($t)*)) };
}

fn blank() -> Vec<u8> {
    vec![0u8; 1025]
}

fn crt(tag: &[u8]) -> Vec<u8> {
    let mut buf = blank();
    buf[1..4].copy_from_slice(b"CRT");
    buf[6..6 + tag.len()].copy_from_slice(tag);
    buf
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

fn paint(dev: &HidDevice, firmware_key: u8, rgb: (u8, u8, u8)) -> Result<(), String> {
    let jpeg = solid_jpeg(rgb.0, rgb.1, rgb.2);
    let mut bat = crt(b"BAT");
    bat[11] = (jpeg.len() >> 8) as u8;
    bat[12] = (jpeg.len() & 0xff) as u8;
    bat[13] = firmware_key;
    dev.write(&bat).map_err(|e| format!("BAT: {e}"))?;

    let mut chunk = blank();
    chunk[1..1 + jpeg.len()].copy_from_slice(&jpeg);
    dev.write(&chunk).map_err(|e| format!("chunk: {e}"))?;

    dev.write(&crt(b"STP")).map_err(|e| format!("STP: {e}"))?;
    Ok(())
}

fn is_present(api: &mut HidApi) -> bool {
    if api.refresh_devices().is_err() {
        return false;
    }
    api.device_list().any(|d| {
        d.vendor_id() == VID && d.product_id() == PID && d.usage_page() == VENDOR_USAGE_PAGE
    })
}

fn open(api: &mut HidApi) -> Option<HidDevice> {
    if api.refresh_devices().is_err() {
        return None;
    }
    let devices: Vec<_> = api
        .device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
        .collect();
    let info = devices
        .iter()
        .find(|d| d.usage_page() == VENDOR_USAGE_PAGE)
        .or_else(|| devices.first())?;
    info.open_device(api).ok()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut api = HidApi::new()?;

    if !is_present(&mut api) {
        say!("deck is NOT present (or without its vendor collection).");
        say!("Replug it once, then run this again while it is healthy.");
        return Ok(());
    }

    let dev = match open(&mut api) {
        Some(d) => d,
        None => {
            say!("present but could not be opened.");
            return Ok(());
        }
    };

    let born = Instant::now();
    say!("opened the existing connection (no replug)");

    for (key, rgb, name) in [
        (1u8, (0xff, 0xff, 0xff), "WHITE"),
        (15, (0xff, 0x00, 0x00), "RED"),
    ] {
        match paint(&dev, key, rgb) {
            Ok(()) => say!(
                "t=+{:>5}ms  {name} painted (key {key})",
                born.elapsed().as_millis()
            ),
            Err(e) => {
                say!("t=+{:>5}ms  paint failed: {e}", born.elapsed().as_millis());
                say!("RESULT: writes are refused right now, before any beating.");
                return Ok(());
            }
        }
    }
    say!(
        "--- beating DIS every {}ms for {}s ---",
        BEAT.as_millis(),
        TARGET.as_secs()
    );

    let mut beats = 0u64;
    while born.elapsed() < TARGET {
        std::thread::sleep(BEAT);
        beats += 1;
        match dev.write(&crt(b"DIS")) {
            Ok(_) => {
                // Print once every 10 beats (~5 s) to keep the log readable.
                if beats.is_multiple_of(10) {
                    say!(
                        "t=+{:>6}ms  ok ({} beats)",
                        born.elapsed().as_millis(),
                        beats
                    );
                }
            }
            Err(e) => {
                let at = born.elapsed().as_millis();
                say!("t=+{:>6}ms  ERR {e}", at);
                say!("");
                say!("RESULT: died at {at}ms after {beats} beats.");
                say!("Checking whether the device left the bus...");
                for _ in 0..10 {
                    std::thread::sleep(Duration::from_secs(1));
                    let present = is_present(&mut api);
                    say!(
                        "  after {}s: present={present}",
                        (born.elapsed().as_millis() - at) / 1000
                    );
                    if present {
                        say!("");
                        say!("The device came back but the handle is dead (expected).");
                        break;
                    }
                }
                return Ok(());
            }
        }
    }

    say!("");
    say!(
        "RESULT: survived {}s of continuous traffic ({} beats).",
        TARGET.as_secs(),
        beats
    );
    say!("Traffic suppresses the reset. The driver needs a heartbeat.");
    Ok(())
}
