//! Does the deck work normally once its boot re-enumeration finishes?
//!
//! Measured with `quiet_probe`, writing nothing at all:
//!
//! ```text
//! t=+ 1000ms  present=true  control_ok=true
//! t=+ 2063ms  present=true  control_ok=true
//! t=+ 3126ms  present=false            <- leaves the bus
//! t=+ 7366ms  present=false
//! t=+ 8426ms  present=true             <- comes back
//! ```
//!
//! The deck re-enumerates about 3 s after power-up and is gone for ~5 s. Every
//! earlier failure kept using a handle from before that reset, which explains
//! both the "2.3 s window" and `ERROR_DEVICE_NOT_CONNECTED`.
//!
//! This probe waits for the replug, waits for the device to leave, waits for it
//! to come back, and only then opens a **fresh** handle, paints both corners and
//! beats for 20 s.
//!
//! Run this FIRST, then replug the deck.
//!
//! Run: `cargo run -p ss550-probe --example reboot_probe`

use hidapi::{HidApi, HidDevice};
use std::io::Write;
use std::time::{Duration, Instant};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;
const POLL: Duration = Duration::from_millis(20);
const BEAT: Duration = Duration::from_millis(500);
const TARGET: Duration = Duration::from_secs(20);
/// How long the device must stay continuously present before we trust that the
/// boot sequence finished. One intermediate appearance was measured lasting
/// under 502 ms, so 3 s gives a wide margin.
const STABLE_FOR: Duration = Duration::from_secs(3);
/// Abort if the device never stabilizes (it would mean an endless reboot loop).
const SETTLE_GIVE_UP: Duration = Duration::from_secs(60);

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

    if is_present(&mut api) {
        say!("deck is plugged in: unplug it. Measurement starts on replug.");
        say!("(no timeout: this waits as long as it takes)");
        let t = Instant::now();
        let mut next_beat = Duration::from_secs(30);
        while is_present(&mut api) {
            if t.elapsed() >= next_beat {
                say!("still plugged in at t=+{}s", t.elapsed().as_secs());
                next_beat += Duration::from_secs(30);
            }
            std::thread::sleep(POLL);
        }
        say!("unplugged at t=+{}ms", t.elapsed().as_millis());
    }

    say!("waiting for replug...");
    let born = Instant::now();

    // The deck may re-enumerate more than once while booting. Measured: it
    // appears, leaves, comes back, and can leave *again* seconds later — a
    // handle opened at that intermediate appearance dies with 0x48F almost
    // immediately. So wait for the device to be continuously present for
    // STABLE_FOR before opening anything, and log every transition.
    let mut present = false;
    let mut stable_since: Option<Instant> = None;
    loop {
        let now_present = is_present(&mut api);
        if now_present != present {
            present = now_present;
            if present {
                say!("t=+{}ms  appeared", born.elapsed().as_millis());
            } else {
                say!("t=+{}ms  left the bus", born.elapsed().as_millis());
                stable_since = None;
            }
        }
        if present {
            let start = *stable_since.get_or_insert_with(Instant::now);
            if start.elapsed() >= STABLE_FOR {
                break;
            }
        }
        if born.elapsed() > SETTLE_GIVE_UP {
            say!(
                "it never stayed present for {}s within {}s; giving up",
                STABLE_FOR.as_secs(),
                SETTLE_GIVE_UP.as_secs()
            );
            say!("RESULT: the deck keeps re-enumerating and never settles.");
            return Ok(());
        }
        std::thread::sleep(POLL);
    }
    say!(
        "stable for {}s at t=+{}ms",
        STABLE_FOR.as_secs(),
        born.elapsed().as_millis()
    );

    // Fresh handle, opened only after the deck stopped re-enumerating.
    let dev = loop {
        if let Some(d) = open(&mut api) {
            break d;
        }
        std::thread::sleep(POLL);
    };
    let ready = Instant::now();
    say!("fresh handle opened");

    match dev.write(&han_bare()) {
        Ok(_) => say!("HAN ok"),
        Err(e) => {
            say!("HAN ERR {e}");
            say!("RESULT: even a fresh handle fails after the reboot.");
            return Ok(());
        }
    }
    // Is STP the only casualty, or is there still a window? Beat a plain DIS
    // first: it needs no transfer state.
    say!(
        "--- after reboot: beating DIS every {}ms for {}s ---",
        BEAT.as_millis(),
        TARGET.as_secs()
    );
    while ready.elapsed() < TARGET {
        std::thread::sleep(BEAT);
        match dev.write(&crt(b"DIS")) {
            Ok(_) => say!("t=+{:>5}ms  DIS ok", ready.elapsed().as_millis()),
            Err(e) => {
                say!("t=+{:>5}ms  DIS ERR {e}", ready.elapsed().as_millis());
                say!("");
                say!(
                    "RESULT: the endpoint still closes after the reboot, at {}ms.",
                    ready.elapsed().as_millis()
                );
                return Ok(());
            }
        }
    }

    say!("survived {}s of DIS; painting now", TARGET.as_secs());
    for (key, rgb, name) in [
        (1u8, (0xff, 0xff, 0xff), "WHITE"),
        (15, (0xff, 0x00, 0x00), "RED"),
    ] {
        match paint(&dev, key, rgb) {
            Ok(()) => say!("{name} painted (firmware key {key})"),
            Err(e) => {
                say!("paint failed: {e}");
                return Ok(());
            }
        }
    }

    say!("");
    say!(
        "RESULT: survived {}s of DIS and painted both corners after the reboot.",
        TARGET.as_secs()
    );
    say!("The deck is fully usable once the boot re-enumeration finishes.");
    Ok(())
}
