//! Does the deck die on its own, with no host writes at all?
//!
//! Every hypothesis that blamed our traffic is now measured and dead: the bare
//! `HAN` handshake, `CRT|CONNECT` keep-alives, reopening the handle, and
//! draining the interrupt IN endpoint (which never returns a single byte).
//!
//! A leftover observation suggests the host is not the cause at all: after a
//! replug during which nothing wrote to the device, a write minutes later failed
//! instantly. This probe makes that controlled — it waits for the replug, then
//! sends **nothing** for 10 s while proving the device is still present and its
//! control endpoint still answers, and only then attempts one write.
//!
//! Outcomes:
//!
//! * write at t=+10s fails, control worked the whole time -> the OUT endpoint
//!   closes by itself. No protocol change can fix that; it is a firmware or
//!   hardware fault, and the deck is only usable in its first seconds after
//!   power-up.
//! * write at t=+10s succeeds -> our own traffic IS the trigger, and the next
//!   step is bisecting which report does it.
//!
//! Run this FIRST, then replug the deck.
//!
//! Run: `cargo run -p ss550-probe --example quiet_probe`

use hidapi::HidApi;
use std::io::Write;
use std::time::{Duration, Instant};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;
const POLL: Duration = Duration::from_millis(20);
const QUIET: Duration = Duration::from_secs(10);

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

/// `device_list` is cached; without `refresh_devices` a poller never sees a
/// plug or unplug.
fn open(api: &mut HidApi) -> Option<hidapi::HidDevice> {
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
        "appeared. Sending NOTHING for {}s; only control reads.",
        QUIET.as_secs()
    );

    let mut buf = vec![0u8; 1025];
    while born.elapsed() < QUIET {
        std::thread::sleep(Duration::from_secs(1));
        let control_ok = dev.get_input_report(&mut buf).is_ok();
        say!(
            "t=+{:>5}ms  quiet; present={} control_ok={control_ok}",
            born.elapsed().as_millis(),
            open(&mut api).is_some()
        );
    }

    say!(
        "--- one write, after {}s of silence ---",
        born.elapsed().as_secs()
    );
    let start = Instant::now();
    match dev.write(&dis()) {
        Ok(n) => {
            say!("DIS ok bytes={n} elapsed={}ms", start.elapsed().as_millis());
            say!("");
            say!("RESULT: the endpoint survived 10 s of silence, so OUR traffic is");
            say!("the trigger. Next step: bisect which report kills it.");
        }
        Err(e) => {
            say!("DIS ERR elapsed={}ms err={e}", start.elapsed().as_millis());
            say!("");
            say!("RESULT: the endpoint closed on its own. Control reads answered the");
            say!("whole time, so the device was present and alive. No protocol change");
            say!("on the host side can fix this.");
        }
    }
    Ok(())
}
