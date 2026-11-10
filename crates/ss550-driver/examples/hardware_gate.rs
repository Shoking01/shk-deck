//! Hardware gate: exercise the real `ss550-driver` API on a dedicated test device.
//!
//! Paints two keys through the driver's own code path and leaves them painted:
//!
//! * `OpenDeck 0`  -> firmware key 1  -> bottom-left, WHITE
//! * `OpenDeck 14` -> firmware key 15 -> top-right, RED
//!
//! Two opposite corners prove the linear `N -> N+1` key mapping end to end.
//!
//! # Why `--wait-hotplug` exists
//!
//! Measured on this device: the deck re-enumerates itself several times while
//! booting, so a handle opened at the first (or second) appearance is stale
//! before it is used, and every write through it fails with `0x48F` or `0x3E5`.
//!
//! The flag starts watching the bus **before** the deck is unplugged, so the
//! first reappearance it sees is a boot stage rather than a settled device. That
//! hands [`Device::connect`] the exact condition its settle window exists for:
//! it must wait for continuous presence and ignore the intermediate appearances.
//! This is the end-to-end check of that rule, not a workaround for it.
//!
//! See `docs/troubleshooting/deck-stuck.md` §5.3 for the measured run.
//!
//! Run: `cargo run -p ss550-driver --example hardware_gate -- --wait-hotplug`
//!      (plug the deck in when prompted)
//!
//! Without the flag it paints against whatever state the device is in.

use hidapi::HidApi;
use ss550_driver::{ButtonImage, Device};
use std::io::Write;
use std::time::{Duration, Instant};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;
const POLL: Duration = Duration::from_millis(20);
const MAX_WAIT: Duration = Duration::from_secs(300);

fn say(args: std::fmt::Arguments<'_>) {
    println!("{args}");
    let _ = std::io::stdout().flush();
}

macro_rules! say {
    ($($t:tt)*) => { say(format_args!($($t)*)) };
}

/// `HidApi::device_list` is a cached snapshot: `refresh_devices` is required for
/// a poller to see a plug or unplug.
fn is_present(api: &mut HidApi) -> bool {
    if api.refresh_devices().is_err() {
        return false;
    }
    api.device_list().any(|d| {
        d.vendor_id() == VID && d.product_id() == PID && d.usage_page() == VENDOR_USAGE_PAGE
    })
}

/// Blocks until the deck is unplugged and plugged back in; returns the instant
/// it reappeared, so the caller knows how much of the write window is left.
fn wait_for_fresh_connect() -> Result<Instant, Box<dyn std::error::Error>> {
    let mut api = HidApi::new()?;

    if is_present(&mut api) {
        say!("unplug the deck now. Measurement starts when it comes back.");
        let t = Instant::now();
        while is_present(&mut api) {
            if t.elapsed() > MAX_WAIT {
                return Err("the deck never disappeared".into());
            }
            std::thread::sleep(POLL);
        }
        say!("removed; plug it back in now");
    } else {
        say!("plug the deck in now");
    }

    loop {
        if is_present(&mut api) {
            return Ok(Instant::now());
        }
        std::thread::sleep(POLL);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wait = std::env::args().any(|a| a == "--wait-hotplug");
    let born = if wait {
        Some(wait_for_fresh_connect()?)
    } else {
        None
    };

    let mark = |label: &str, born: Option<Instant>| match born {
        Some(b) => say!("{label} at t=+{}ms", b.elapsed().as_millis()),
        None => say!("{label}"),
    };

    let mut device = Device::connect()?;
    mark("connected", born);

    device.initialize()?;
    mark("initialized (DIS)", born);

    device.set_key_image(ButtonImage::white(0)?)?;
    mark(
        "painted WHITE on OpenDeck 0 (bottom-left, firmware key 1)",
        born,
    );

    device.set_key_image(ButtonImage::red(14)?)?;
    mark(
        "painted RED on OpenDeck 14 (top-right, firmware key 15)",
        born,
    );

    // Read-only and informational: this is a static identity report, not proof
    // that the writes landed.
    say!(
        "identity_report={:?} (static, not proof)",
        device.firmware_version()?
    );

    say!(">>> OBSERVE: bottom-left WHITE, top-right RED. State left as-is.");
    Ok(())
}
