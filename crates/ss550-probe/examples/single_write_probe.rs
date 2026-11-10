//! Does a single output report kill the USB link?
//!
//! Facts so far, all measured:
//!
//! * the deck accepts the first `DIS` after a real replug (`0 ms`), and a second
//!   one a second later;
//! * roughly two seconds in, writes fail with `ERROR_DEVICE_NOT_CONNECTED`
//!   (`0x48F`) or immediately with `ERROR_GEN_FAILURE` (`0x1F`);
//! * control transfers keep working, and the device sits stable in PnP while
//!   idle - so idleness alone does not kill it.
//!
//! This probe separates "one report kills it" from "repeated reports kill it":
//! it sends exactly **one** `DIS`, then stops writing and only watches the link
//! for 30 s using control reads (which never disturb the endpoint).
//!
//! Run it FIRST, then replug the deck.
//!
//! Run: `cargo run -p ss550-probe --example single_write_probe`

use hidapi::HidApi;
use std::io::Write;
use std::time::{Duration, Instant};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;
const POLL: Duration = Duration::from_millis(100);
const WATCH: Duration = Duration::from_secs(30);

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

/// `HidApi::device_list` returns a **cached** snapshot; without
/// `refresh_devices()` a poller keeps seeing a list that never changes. That is
/// why the first version of this probe never noticed the deck being unplugged.
fn present(api: &mut HidApi) -> Option<hidapi::DeviceInfo> {
    api.refresh_devices().ok()?;
    let devices: Vec<_> = api
        .device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
        .collect();
    let info = devices
        .iter()
        .find(|d| d.usage_page() == VENDOR_USAGE_PAGE)
        .or_else(|| devices.first())?;
    Some((*info).clone())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut api = HidApi::new()?;
    // `--now` measures the current state instead of waiting for a fresh replug;
    // use it when the deck was just plugged in and nothing has written since.
    let now = std::env::args().any(|a| a == "--now");

    if !now && present(&mut api).is_some() {
        say!("deck is plugged in: unplug it. The measurement starts on replug.");
        let t = Instant::now();
        while present(&mut api).is_some() {
            if t.elapsed() > Duration::from_secs(300) {
                say!("never disappeared");
                return Ok(());
            }
            std::thread::sleep(POLL);
        }
        say!("removed after {}s", t.elapsed().as_secs());
    }

    say!("waiting for replug...");
    let info = loop {
        if let Some(i) = present(&mut api) {
            break i;
        }
        std::thread::sleep(POLL);
    };
    let appeared = Instant::now();
    say!("appeared; sending exactly ONE DIS and then nothing else");

    let dev = info.open_device(&api)?;
    let t = Instant::now();
    match dev.write(&dis()) {
        Ok(n) => say!(
            "DIS ok bytes={n} at t=+{}ms elapsed={}ms",
            appeared.elapsed().as_millis(),
            t.elapsed().as_millis()
        ),
        Err(e) => {
            say!(
                "DIS ERR at t=+{}ms elapsed={}ms err={e}",
                appeared.elapsed().as_millis(),
                t.elapsed().as_millis()
            );
            say!("RESULT: dead before this process wrote anything meaningful.");
            return Ok(());
        }
    }

    say!(
        "--- no more writes; watching the link for {}s ---",
        WATCH.as_secs()
    );
    let mut last_present = true;
    let mut last_control = true;
    let start = Instant::now();
    while start.elapsed() < WATCH {
        std::thread::sleep(POLL);
        let is_present = present(&mut api).is_some();
        if is_present != last_present {
            say!(
                "t=+{}ms  present={is_present}",
                appeared.elapsed().as_millis()
            );
            last_present = is_present;
        }
        if is_present {
            let mut buf = vec![0u8; 1025];
            let control_ok = dev.get_input_report(&mut buf).is_ok();
            if control_ok != last_control {
                say!(
                    "t=+{}ms  control_read={control_ok}",
                    appeared.elapsed().as_millis()
                );
                last_control = control_ok;
            }
        }
    }

    say!(
        "RESULT: {} after ONE write.",
        if last_present && last_control {
            "link survived 30 s"
        } else {
            "link died"
        }
    );
    Ok(())
}
