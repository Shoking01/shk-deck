//! Hardware gate for the heartbeat: does `Device::keepalive()` keep the deck alive?
//!
//! Measured on this device, with **zero writes** from the host: the deck stays
//! healthy for 68.7 s after power-up and then drops both HID interfaces off the
//! bus. With one `CRT|DIS` every 500 ms it survived 180 s (360 reports) with no
//! drop. The trigger is idleness, not time.
//!
//! `hold_probe` proved that with raw writes. This example proves the same thing
//! through the **driver's public API**: it paints two corners, then calls
//! [`Device::keepalive`] in a tight loop for three minutes. Three minutes
//! deliberately crosses the 68.7 s mark.
//!
//! Run this against a deck that is **already healthy** — no replug needed:
//!
//! ```text
//! cargo run -p ss550-driver --example heartbeat_gate
//! ```
//!
//! * survives 180 s -> the heartbeat is the fix; an idle app must call
//!   `keepalive()` from its main loop.
//! * dies anyway   -> the reset is on a timer the host cannot influence, and
//!   `connect()` will need transparent reconnection.

use hidapi::HidApi;
use ss550_driver::{ButtonImage, Device};
use std::io::Write;
use std::time::{Duration, Instant};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;
const TICK: Duration = Duration::from_millis(100);
const TARGET: Duration = Duration::from_secs(180);
const REPORT_EVERY: Duration = Duration::from_secs(10);

fn say(args: std::fmt::Arguments<'_>) {
    println!("{args}");
    let _ = std::io::stdout().flush();
}

macro_rules! say {
    ($($t:tt)*) => { say(format_args!($($t)*)) };
}

/// `device_list` is a cached snapshot; `refresh_devices` is required to see a
/// plug or unplug.
fn is_present() -> bool {
    let mut api = match HidApi::new() {
        Ok(a) => a,
        Err(_) => return false,
    };
    if api.refresh_devices().is_err() {
        return false;
    }
    let present = api.device_list().any(|d| {
        d.vendor_id() == VID && d.product_id() == PID && d.usage_page() == VENDOR_USAGE_PAGE
    });
    present
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if !is_present() {
        say!("the deck is not on the bus (or has no vendor collection).");
        say!("Power cycle it once, then run this while it is healthy.");
        return Ok(());
    }

    let born = Instant::now();
    let mut device = Device::connect()?;
    say!(
        "connected at t=+{}ms; heartbeat interval is {:?}",
        born.elapsed().as_millis(),
        device.heartbeat_interval()
    );

    device.initialize()?;
    device.set_key_image(ButtonImage::white(0)?)?;
    device.set_key_image(ButtonImage::red(14)?)?;
    say!(
        "painted bottom-left WHITE and top-right RED at t=+{}ms",
        born.elapsed().as_millis()
    );
    say!(
        "--- calling keepalive() every {}ms for {}s ---",
        TICK.as_millis(),
        TARGET.as_secs()
    );

    let mut next_report = REPORT_EVERY;
    while born.elapsed() < TARGET {
        std::thread::sleep(TICK);

        // The throttle lives inside the driver: this is a no-op on most ticks,
        // which is exactly how an app should call it — once per frame, always.
        if let Err(e) = device.keepalive() {
            let at = born.elapsed().as_millis();
            say!("t=+{at}ms  keepalive FAILED: {e}");
            say!("");
            say!("RESULT: the deck reset at {at}ms despite the heartbeat.");
            say!("Device present on the bus now: {}", is_present());
            return Ok(());
        }

        if born.elapsed() >= next_report {
            say!("t=+{:>6}ms  alive", born.elapsed().as_millis());
            next_report += REPORT_EVERY;
        }
    }

    say!("");
    say!(
        "RESULT: survived {}s with the driver heartbeat, past the measured 68.7 s idle death.",
        TARGET.as_secs()
    );
    say!("An idle app must call Device::keepalive() from its main loop.");
    Ok(())
}
