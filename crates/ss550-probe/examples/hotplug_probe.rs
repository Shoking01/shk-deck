//! Measures the window in which a freshly replugged deck accepts writes.
//!
//! Evidence that motivates this: writing within ~250 ms of the device appearing
//! succeeds (`0 ms`), writing a second later also succeeds, and writing ten
//! seconds later fails instantly with `0x1F`. So the endpoint is alive only for
//! a short time after power-up, and presence in PnP says nothing about it -
//! the device stays enumerated with a dead OUT endpoint.
//!
//! This probe writes every 100 ms from the moment the deck reappears, and stops
//! at the first failure, so the output is the size of that window.
//!
//! Run this FIRST, then replug the deck.
//!
//! Run: `cargo run -p ss550-probe --example hotplug_probe`

use hidapi::{HidApi, HidDevice};
use std::io::Write;
use std::time::{Duration, Instant};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;
const POLL: Duration = Duration::from_millis(50);
const BEAT: Duration = Duration::from_millis(100);
const MAX_RUN: Duration = Duration::from_secs(120);

fn say(args: std::fmt::Arguments<'_>) {
    println!("{args}");
    let _ = std::io::stdout().flush();
}

macro_rules! say {
    ($($t:tt)*) => { say(format_args!($($t)*)) };
}

fn dis() -> Vec<u8> {
    let mut buf = vec![0u8; 1025];
    buf[1..4].copy_from_slice(b"CRT");
    buf[6..9].copy_from_slice(b"DIS");
    buf
}

/// `device_list` is a cached snapshot: without `refresh_devices` a poller never
/// sees a plug or unplug.
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
            if t.elapsed() > MAX_RUN {
                say!("never disappeared");
                return Ok(());
            }
            std::thread::sleep(POLL);
        }
        say!("removed after {}s", t.elapsed().as_secs());
    }

    say!("waiting for the deck to reappear...");
    let dev = loop {
        if let Some(d) = open(&mut api) {
            break d;
        }
        std::thread::sleep(POLL);
    };

    let born = Instant::now();
    say!(
        "appeared. beating every {}ms until the first failure",
        BEAT.as_millis()
    );
    let pack = dis();

    let mut beats = 0u32;
    while born.elapsed() < MAX_RUN {
        beats += 1;
        match dev.write(&pack) {
            Ok(_) => say!("t=+{:>5}ms  ok  (beat {beats})", born.elapsed().as_millis()),
            Err(e) => {
                say!("t=+{:>5}ms  ERR {e}", born.elapsed().as_millis());
                say!("");
                say!(
                    "RESULT: {beats} writes accepted in the first {} ms.",
                    born.elapsed().as_millis()
                );
                say!("That is the usable window after power-up.");
                return Ok(());
            }
        }
        std::thread::sleep(BEAT);
    }

    say!("");
    say!(
        "RESULT: {beats} writes over {:?} - the deck stayed healthy.",
        MAX_RUN
    );
    Ok(())
}
