//! After the deck's USB reset, does reopening the handle restore writes?
//!
//! Measured: a freshly powered deck accepts reports for 1-2.5 s and then the
//! link drops - `0x48F` (`ERROR_DEVICE_NOT_CONNECTED`) with `CONNECT`, `0x1F`
//! (`ERROR_GEN_FAILURE`) with `DIS`. Both look like the firmware resetting its
//! USB stack shortly after boot.
//!
//! If that is what it is, the handle we hold becomes stale while the device
//! itself is fine: dropping it and reopening should give a working endpoint, and
//! the "2.3 s window" would be a host-side artefact of using one handle forever.
//!
//! This probe beats until the first failure, then drops the handle, waits for
//! the device to come back, reopens and writes again. Up to 3 recoveries.
//!
//! Run this FIRST, then replug the deck.
//!
//! Run: `cargo run -p ss550-probe --example reopen_probe`

use hidapi::{HidApi, HidDevice};
use std::io::Write;
use std::time::{Duration, Instant};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;
const POLL: Duration = Duration::from_millis(20);
const BEAT: Duration = Duration::from_millis(200);
const MAX_RECOVERIES: u32 = 3;
const MAX_RUN: Duration = Duration::from_secs(30);

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
    let mut dev = loop {
        if let Some(d) = open(&mut api) {
            break d;
        }
        std::thread::sleep(POLL);
    };

    let born = Instant::now();
    say!(
        "appeared; beating every {}ms, reopening after each failure",
        BEAT.as_millis()
    );

    let mut recoveries = 0u32;
    let pack = pack(b"DIS");

    while born.elapsed() < MAX_RUN {
        match dev.write(&pack) {
            Ok(_) => say!("t=+{:>6}ms  ok", born.elapsed().as_millis()),
            Err(e) => {
                say!("t=+{:>6}ms  ERR {e}", born.elapsed().as_millis());
                if recoveries >= MAX_RECOVERIES {
                    say!("");
                    say!("RESULT: {MAX_RECOVERIES} reopenings did not restore writes.");
                    return Ok(());
                }
                recoveries += 1;
                drop(dev);

                // Let the device finish whatever it is doing, then reopen.
                std::thread::sleep(Duration::from_millis(1000));
                let t = Instant::now();
                dev = loop {
                    if let Some(d) = open(&mut api) {
                        break d;
                    }
                    if t.elapsed() > Duration::from_secs(10) {
                        say!("device did not come back within 10 s");
                        return Ok(());
                    }
                    std::thread::sleep(POLL);
                };
                say!(
                    "t=+{:>6}ms  reopened after {}ms",
                    born.elapsed().as_millis(),
                    t.elapsed().as_millis()
                );
                match dev.write(&pack) {
                    Ok(_) => say!(
                        "t=+{:>6}ms  write after reopen OK",
                        born.elapsed().as_millis()
                    ),
                    Err(e) => {
                        say!(
                            "t=+{:>6}ms  write after reopen ERR {}",
                            born.elapsed().as_millis(),
                            e
                        );
                    }
                }
            }
        }
        std::thread::sleep(BEAT);
    }

    say!("");
    say!("RESULT: ran {:?} with {recoveries} reopenings.", MAX_RUN);
    Ok(())
}
