//! Hardware gate for input: does `Device::poll_events` see real key presses?
//!
//! Paints two corners, then runs the loop a real app would run — poll without
//! blocking, keep the deck alive, sleep a little — and prints every edge.
//!
//! This exercises three things that were only ever true for `ss550-probe`:
//!
//! 1. button reports arrive on the **vendor** collection, which is the one
//!    [`Device::connect`] opens (the keyboard collection is silent);
//! 2. reads and writes can share **one** handle — the probe used two;
//! 3. polling and [`Device::keepalive`] coexist, so the deck does not reset
//!    while the app is only listening.
//!
//! Measured layout: 512-byte interrupt IN reports, `"ACK"` at `[0..3]`,
//! `"OK"` at `[5..7]`, key at `[9]`, state at `[10]`. Every click is two events,
//! pressed then released.
//!
//! Run: `cargo run -p ss550-driver --example button_gate`
//!      `cargo run -p ss550-driver --example button_gate -- 120`
//!
//! Expected: pressing the **bottom-left** key prints `BottomLeft`, because the
//! report numbering starts at the top row and `read_index_to_firmware_key`
//! flips it back to the paint numbering. If a keypress names the *mirrored* key,
//! the flip is wrong.

use hidapi::HidApi;
use ss550_driver::{ButtonImage, Device};
use std::io::Write;
use std::time::{Duration, Instant};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;
const TICK: Duration = Duration::from_millis(10);
const REPORT_EVERY: Duration = Duration::from_secs(10);
const DEFAULT_SECS: u64 = 60;

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
    let secs = std::env::args()
        .nth(1)
        .and_then(|a| a.parse::<u64>().ok())
        .unwrap_or(DEFAULT_SECS);
    let target = Duration::from_secs(secs);

    if !is_present() {
        say!("the deck is not on the bus. Power cycle it, then run again.");
        return Ok(());
    }

    let born = Instant::now();
    let mut device = Device::connect()?;
    say!(
        "connected at t=+{}ms, heartbeat interval {:?}",
        born.elapsed().as_millis(),
        device.heartbeat_interval()
    );

    device.initialize()?;
    device.set_key_image(ButtonImage::white(0)?)?;
    device.set_key_image(ButtonImage::red(14)?)?;
    say!("painted bottom-left WHITE and top-right RED");

    say!("");
    say!("=== PRESS KEYS NOW (running {secs}s) ===");
    say!("");

    let mut total = 0u32;
    let mut next_report = REPORT_EVERY;

    while born.elapsed() < target {
        // Non-blocking: the app loop must never stall on a device that is idle.
        match device.poll_events(Duration::ZERO) {
            Ok(events) => {
                for event in events {
                    total += 1;
                    say!(
                        "t=+{:>6}ms  {:?} {:?}",
                        born.elapsed().as_millis(),
                        event.key,
                        event.state
                    );
                }
            }
            Err(e) => {
                say!("t=+{:>6}ms  read FAILED: {e}", born.elapsed().as_millis());
                say!("Device present on the bus now: {}", is_present());
                return Ok(());
            }
        }

        // Same loop an idle app runs: the throttle lives inside the driver.
        device.keepalive()?;

        if born.elapsed() >= next_report {
            say!(
                "t=+{:>6}ms  alive, {total} events so far",
                born.elapsed().as_millis()
            );
            next_report += REPORT_EVERY;
        }

        std::thread::sleep(TICK);
    }

    say!("");
    if total == 0 {
        say!("RESULT: no events. Either nothing was pressed, or reports are");
        say!("not reaching this handle while it also writes.");
    } else {
        say!("RESULT: {total} key edges decoded through the public API.");
        say!("Check the names: bottom-left must print `BottomLeft`, not `TopLeft`.");
    }
    Ok(())
}
