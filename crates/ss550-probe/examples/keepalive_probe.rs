//! Does the documented keep-alive keep the deck writable past ~2.3 s?
//!
//! Measured baseline: the OUT endpoint accepts reports for about 2.3 s after
//! power-up and then refuses everything until the next power cycle, regardless
//! of whether the host keeps writing (`hotplug_probe`, 24 writes then `0x1F` at
//! t=+2315ms).
//!
//! `ss550-protocol` documents `build_connect()` = `CRT|CONNECT` as a keep-alive.
//! If the firmware closes the write window because it never hears from the host,
//! sending it every 300 ms should keep the deck alive past 2.3 s and let us
//! paint at leisure.
//!
//! Run this FIRST, then replug the deck.
//!
//! Run: `cargo run -p ss550-probe --example keepalive_probe`

use hidapi::{HidApi, HidDevice};
use std::io::Write;
use std::time::{Duration, Instant};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;
const POLL: Duration = Duration::from_millis(20);
const KEEPALIVE: Duration = Duration::from_millis(300);
/// Survive this long and the window is considered extended (8x the baseline).
const TARGET: Duration = Duration::from_secs(20);

fn say(args: std::fmt::Arguments<'_>) {
    println!("{args}");
    let _ = std::io::stdout().flush();
}

macro_rules! say {
    ($($t:tt)*) => { say(format_args!($($t)*)) };
}

fn pack(tag: &[u8]) -> Vec<u8> {
    let mut buf = vec![0u8; 1025];
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
    let mut bat = pack(b"BAT");
    bat[11] = (jpeg.len() >> 8) as u8;
    bat[12] = (jpeg.len() & 0xff) as u8;
    bat[13] = firmware_key;
    dev.write(&bat).map_err(|e| format!("BAT: {e}"))?;

    let mut chunk = vec![0u8; 1025];
    chunk[1..1 + jpeg.len()].copy_from_slice(&jpeg);
    dev.write(&chunk).map_err(|e| format!("chunk: {e}"))?;

    dev.write(&pack(b"STP")).map_err(|e| format!("STP: {e}"))?;
    Ok(())
}

/// `device_list` is cached; without `refresh_devices` a poller never sees a
/// plug or unplug.
fn open(api: &mut HidApi) -> Option<HidDevice> {
    api.refresh_devices().ok()?;
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

    if open(&mut api).is_some() {
        say!("deck is plugged in: unplug it. Measurement starts on replug.");
        let t = Instant::now();
        while open(&mut api).is_some() {
            if t.elapsed() > Duration::from_secs(300) {
                say!("never disappeared");
                return Ok(());
            }
            std::thread::sleep(POLL);
        }
    }

    say!("waiting for replug...");
    let dev = loop {
        if let Some(d) = open(&mut api) {
            break d;
        }
        std::thread::sleep(POLL);
    };

    let born = Instant::now();
    say!(
        "appeared; sending CRT|CONNECT every {}ms",
        KEEPALIVE.as_millis()
    );

    while born.elapsed() < TARGET {
        match dev.write(&pack(b"CONNECT")) {
            Ok(_) => say!("t=+{:>5}ms  connect ok", born.elapsed().as_millis()),
            Err(e) => {
                say!("t=+{:>5}ms  connect ERR {e}", born.elapsed().as_millis());
                say!("");
                say!("RESULT: CONNECT did not hold the window open; it closed at");
                say!("{}ms, same as with plain DIS.", born.elapsed().as_millis());
                return Ok(());
            }
        }
        std::thread::sleep(KEEPALIVE);
    }

    say!("");
    say!(
        "RESULT: survived {}s of keep-alive - the window is not time-limited.",
        TARGET.as_secs()
    );
    say!("Painting both corners to prove a full session works...");
    match paint(&dev, 1, (0xff, 0xff, 0xff)) {
        Ok(()) => say!("painted WHITE on firmware key 1 (bottom-left)"),
        Err(e) => {
            say!("paint failed: {e}");
            return Ok(());
        }
    }
    match paint(&dev, 15, (0xff, 0x00, 0x00)) {
        Ok(()) => say!("painted RED on firmware key 15 (top-right)"),
        Err(e) => say!("paint failed: {e}"),
    }
    Ok(())
}
