//! Is the bare `HAN` handshake what keeps the deck writable?
//!
//! Measured: a fresh deck accepts reports for 1.2-3.0 s and then closes the OUT
//! endpoint for good; reopening the handle does not help, only a power cycle
//! does. The vendor app works in normal use, so the firmware must be waiting for
//! something we never send.
//!
//! `ss550-protocol` documents two forms and warns they are not interchangeable:
//!
//! * `build_han_bare()` -> `HAN` at wire `[1..4]`, no `CRT` prefix: the
//!   connect-time handshake (`sendHandshakePack` in the vendor DLL).
//! * `build_sleep()` -> `CRT|HAN`: sleep.
//!
//! Nothing in this project has ever sent the bare handshake. This probe sends it
//! as the very first report after power-up, then paints both corners and keeps
//! beating for 20 s to see whether the window stays open.
//!
//! Run this FIRST, then replug the deck.
//!
//! Run: `cargo run -p ss550-probe --example handshake_probe`

use hidapi::{HidApi, HidDevice};
use std::io::Write;
use std::time::{Duration, Instant};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;
const POLL: Duration = Duration::from_millis(20);
const BEAT: Duration = Duration::from_millis(500);
const TARGET: Duration = Duration::from_secs(20);

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

/// `HAN` at wire `[1..4]`, report ID 0, no `CRT` prefix.
fn han_bare() -> Vec<u8> {
    let mut buf = blank();
    buf[1..4].copy_from_slice(b"HAN");
    buf
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
    say!("appeared; sending bare HAN as the first report");

    match dev.write(&han_bare()) {
        Ok(n) => say!("t=+{:>5}ms  HAN ok bytes={n}", born.elapsed().as_millis()),
        Err(e) => {
            say!("t=+{:>5}ms  HAN ERR {e}", born.elapsed().as_millis());
            say!("RESULT: even the handshake was refused.");
            return Ok(());
        }
    }

    say!("painting both corners now...");
    match paint(&dev, 1, (0xff, 0xff, 0xff)) {
        Ok(()) => say!(
            "t=+{:>5}ms  WHITE painted (firmware key 1)",
            born.elapsed().as_millis()
        ),
        Err(e) => {
            say!("paint failed: {e}");
            return Ok(());
        }
    }
    match paint(&dev, 15, (0xff, 0x00, 0x00)) {
        Ok(()) => say!(
            "t=+{:>5}ms  RED painted (firmware key 15)",
            born.elapsed().as_millis()
        ),
        Err(e) => {
            say!("paint failed: {e}");
            return Ok(());
        }
    }

    say!(
        "--- beating every {}ms to see if the window stays open ---",
        BEAT.as_millis()
    );
    while born.elapsed() < TARGET {
        std::thread::sleep(BEAT);
        match dev.write(&crt(b"DIS")) {
            Ok(_) => say!("t=+{:>5}ms  ok", born.elapsed().as_millis()),
            Err(e) => {
                say!("t=+{:>5}ms  ERR {e}", born.elapsed().as_millis());
                say!("");
                say!(
                    "RESULT: HAN did not hold the window; it closed at {}ms.",
                    born.elapsed().as_millis()
                );
                return Ok(());
            }
        }
    }

    say!("");
    say!(
        "RESULT: survived {}s after the handshake.",
        TARGET.as_secs()
    );
    say!("The bare HAN handshake is what keeps the endpoint open.");
    Ok(())
}
