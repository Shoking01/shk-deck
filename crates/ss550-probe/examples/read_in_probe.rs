//! Does continuously reading the interrupt IN endpoint keep the deck writable?
//!
//! Measured: the OUT endpoint closes ~3 s after power-up (1203 ms with
//! `CONNECT`, 2315 ms with 100 ms beats, 3009 ms and 3011 ms in later runs),
//! and reopening the handle does not help. The bare `HAN` handshake does not
//! change it either.
//!
//! A classic firmware mechanism produces exactly that shape: the device pushes
//! status reports to the interrupt IN endpoint, nobody drains them, the firmware
//! blocks in its send call once the buffer fills, and its OUT handling stops
//! being serviced. This project has never read the IN endpoint at all.
//!
//! This probe opens a second handle in a reader thread that drains IN
//! continuously, while the main thread sends `HAN`, paints both corners and
//! beats. If the window stays open past 3 s, that is the mechanism.
//!
//! Run this FIRST, then replug the deck.
//!
//! Run: `cargo run -p ss550-probe --example read_in_probe`

use hidapi::{HidApi, HidDevice};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
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
    let reads = Arc::new(AtomicU64::new(0));

    // Reader thread: its own HidApi and its own handle, so no cross-thread
    // handle sharing is needed.
    let reads_for_thread = Arc::clone(&reads);
    let reader = std::thread::spawn(move || {
        let mut api = match HidApi::new() {
            Ok(a) => a,
            Err(_) => return,
        };
        let dev = loop {
            if let Some(d) = open(&mut api) {
                break d;
            }
            std::thread::sleep(POLL);
        };
        let mut buf = vec![0u8; 1025];
        loop {
            match dev.read_timeout(&mut buf, 100) {
                Ok(n) if n > 0 => {
                    reads_for_thread.fetch_add(1, Ordering::Relaxed);
                    say!("   IN read {n} bytes: {:?}", &buf[..n.min(16)]);
                }
                _ => {}
            }
        }
    });

    say!("reader thread started; sending bare HAN then painting");
    match dev.write(&han_bare()) {
        Ok(_) => say!("t=+{:>5}ms  HAN ok", born.elapsed().as_millis()),
        Err(e) => {
            say!("t=+{:>5}ms  HAN ERR {e}", born.elapsed().as_millis());
            return Ok(());
        }
    }
    for (key, rgb, name) in [
        (1u8, (0xff, 0xff, 0xff), "WHITE"),
        (15, (0xff, 0x00, 0x00), "RED"),
    ] {
        match paint(&dev, key, rgb) {
            Ok(()) => say!(
                "t=+{:>5}ms  {name} painted (firmware key {key})",
                born.elapsed().as_millis()
            ),
            Err(e) => {
                say!("paint failed: {e}");
                return Ok(());
            }
        }
    }

    say!(
        "--- beating every {}ms while IN is being drained ---",
        BEAT.as_millis()
    );
    while born.elapsed() < TARGET {
        std::thread::sleep(BEAT);
        match dev.write(&crt(b"DIS")) {
            Ok(_) => say!(
                "t=+{:>5}ms  ok (IN reads so far: {})",
                born.elapsed().as_millis(),
                reads.load(Ordering::Relaxed)
            ),
            Err(e) => {
                say!("t=+{:>5}ms  ERR {e}", born.elapsed().as_millis());
                say!("");
                say!(
                    "RESULT: draining IN did not help; window closed at {}ms.",
                    born.elapsed().as_millis()
                );
                std::process::exit(0);
            }
        }
    }

    say!("");
    say!("RESULT: survived {}s with IN drained.", TARGET.as_secs());
    say!(
        "The blocked-IN-write hypothesis holds. IN reads: {}",
        reads.load(Ordering::Relaxed)
    );
    // Keep the reader alive until stdout is flushed.
    drop(reader);
    Ok(())
}
