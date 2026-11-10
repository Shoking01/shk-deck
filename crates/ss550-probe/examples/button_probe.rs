//! Where do button presses come from? Listen on **both** HID collections at once.
//!
//! `read_in_probe` reported "zero IN bytes ever", and that result has been
//! treated as if the deck never reports anything. It does not mean that. That
//! probe read the vendor collection while painting, and **no button was pressed
//! during it**: zero reports only proves the deck is silent when idle.
//!
//! The deck enumerates two collections under one PID:
//!
//! ```text
//! MI_00  usage_page=0xffa0  usage=0x0001   vendor  (the driver's write path)
//! MI_01  usage_page=0x0001  usage=0x0006   keyboard
//! ```
//!
//! So a press could arrive on either. If it arrives on MI_01 as a standard
//! keyboard report, Windows itself may react to it — which would explain why the
//! vendor app "just works" without owning the input path.
//!
//! This probe opens a handle on **each** collection in its own thread and dumps
//! every report it sees, with a timestamp and a hex preview. Press every one of
//! the 15 buttons while it runs.
//!
//! The main thread also sends `CRT|DIS` every 2 s: the deck resets after ~68 s of
//! host silence, and a reset in the middle of the run would look like an input
//! bug. A heartbeat never appears on the IN endpoint, so it cannot mask a report.
//!
//! Run: `cargo run -p ss550-probe --example button_probe`
//!      `cargo run -p ss550-probe --example button_probe -- 120`   (run 120 s)

use hidapi::{HidApi, HidDevice};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;
const KEYBOARD_USAGE_PAGE: u16 = 0x0001;
const KEYBOARD_USAGE: u16 = 0x0006;
const BEAT: Duration = Duration::from_secs(2);
const DEFAULT_SECS: u64 = 60;

fn say(args: std::fmt::Arguments<'_>) {
    println!("{args}");
    let _ = std::io::stdout().flush();
}

macro_rules! say {
    ($($t:tt)*) => { say(format_args!($($t)*)) };
}

/// One 1025-byte report with `CRT` + tag, the shape the driver sends.
fn crt(tag: &[u8]) -> Vec<u8> {
    let mut buf = vec![0u8; 1025];
    buf[1..4].copy_from_slice(b"CRT");
    buf[6..6 + tag.len()].copy_from_slice(tag);
    buf
}

/// Opens a handle on one specific collection.
///
/// `device_list` is a cached snapshot, so this refreshes first and then finds
/// the entry by usage page *and* usage: `0x0001` alone matches several unrelated
/// collections, and opening the wrong one silently yields a dead reader.
fn open_collection(api: &mut HidApi, usage_page: u16, usage: u16) -> Option<HidDevice> {
    api.refresh_devices().ok()?;
    let info = api
        .device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
        .find(|d| d.usage_page() == usage_page && d.usage() == usage)?;
    info.open_device(api).ok()
}

/// Prints the collection inventory once, so the run is self-documenting.
fn describe(api: &mut HidApi) {
    let _ = api.refresh_devices();
    say!("collections visible under VID_{VID:04X}&PID_{PID:04X}:");
    let mut any = false;
    for d in api
        .device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
    {
        any = true;
        say!(
            "  iface={} usage_page=0x{:04x} usage=0x{:04x} path={}",
            d.interface_number(),
            d.usage_page(),
            d.usage(),
            d.path().to_string_lossy()
        );
    }
    if !any {
        say!("  (none — the deck is not enumerated)");
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let secs = std::env::args()
        .nth(1)
        .and_then(|a| a.parse::<u64>().ok())
        .unwrap_or(DEFAULT_SECS);
    let target = Duration::from_secs(secs);

    let mut api = HidApi::new()?;
    describe(&mut api);

    // Fail early and loudly: a missing collection is a finding, not a hiccup.
    let vendor = open_collection(&mut api, VENDOR_USAGE_PAGE, 0x0001);
    let keyboard = open_collection(&mut api, KEYBOARD_USAGE_PAGE, KEYBOARD_USAGE);

    say!(
        "vendor collection (MI_00):   {}",
        if vendor.is_some() {
            "opened"
        } else {
            "NOT AVAILABLE"
        }
    );
    say!(
        "keyboard collection (MI_01): {}",
        if keyboard.is_some() {
            "opened"
        } else {
            "NOT AVAILABLE — Windows may hold it exclusively"
        }
    );

    if vendor.is_none() && keyboard.is_none() {
        say!("nothing to listen on. Unplug and replug the deck, then retry.");
        return Ok(());
    }

    let born = Instant::now();
    let vendor_hits = Arc::new(AtomicU64::new(0));
    let keyboard_hits = Arc::new(AtomicU64::new(0));

    // One thread per collection: `HidApi` is not `Send`, so each thread owns its
    // own instance and its own handle.
    let spawn_reader = |device: Option<HidDevice>, hits: Arc<AtomicU64>, page: u16, usage: u16| {
        std::thread::spawn(move || {
            let Some(dev) = device else { return };
            let mut buf = vec![0u8; 1025];
            loop {
                match dev.read_timeout(&mut buf, 100) {
                    Ok(n) if n > 0 => {
                        let count = hits.fetch_add(1, Ordering::Relaxed) + 1;
                        let preview = n.min(16);
                        say!(
                            "t=+{:>6}ms  IN page=0x{page:04x} usage=0x{usage:04x} n={n} #{count}  {:02x?}",
                            born.elapsed().as_millis(),
                            &buf[..preview]
                        );
                    }
                    // Timeout and error are both "nothing yet": a keyboard
                    // collection Windows owns returns errors forever, and that
                    // must not kill the thread.
                    _ => {}
                }
            }
        })
    };

    let _v = spawn_reader(vendor, Arc::clone(&vendor_hits), VENDOR_USAGE_PAGE, 0x0001);
    let _k = spawn_reader(
        keyboard,
        Arc::clone(&keyboard_hits),
        KEYBOARD_USAGE_PAGE,
        KEYBOARD_USAGE,
    );

    say!("");
    say!("=== PRESS EVERY ONE OF THE 15 BUTTONS NOW ===");
    say!("(one press each, a second apart; running {secs}s)");
    say!("");

    // Keep the deck awake: it resets after ~68 s of host silence, and a reset
    // mid-run would look exactly like an input bug.
    let mut beat_api = HidApi::new()?;
    let mut writer = open_collection(&mut beat_api, VENDOR_USAGE_PAGE, 0x0001);
    let mut next_beat = BEAT;

    while born.elapsed() < target {
        std::thread::sleep(Duration::from_millis(200));

        if born.elapsed() >= next_beat {
            next_beat += BEAT;
            if let Some(dev) = writer.as_ref() {
                match dev.write(&crt(b"DIS")) {
                    Ok(_) => {}
                    Err(e) => {
                        say!(
                            "t=+{:>6}ms  heartbeat FAILED: {e}",
                            born.elapsed().as_millis()
                        );
                        // Drop the dead handle; the read threads keep running so
                        // we still learn whether input survives a write death.
                        writer = None;
                    }
                }
            }
        }
    }

    say!("");
    say!("RESULT after {}s:", secs);
    say!(
        "  vendor   (MI_00, 0xffa0): {} reports",
        vendor_hits.load(Ordering::Relaxed)
    );
    say!(
        "  keyboard (MI_01, 0x0001/0x0006): {} reports",
        keyboard_hits.load(Ordering::Relaxed)
    );
    say!("");
    if vendor_hits.load(Ordering::Relaxed) + keyboard_hits.load(Ordering::Relaxed) == 0 {
        say!("No input report on either collection. Next suspects: the press is");
        say!("delivered over the control endpoint (polled with GET_REPORT), or the");
        say!("firmware only reports while a session is open (CRT|CONNECT).");
    } else {
        say!("Input is a live HID interrupt IN stream. That is the driver's event");
        say!("source; record the report layout before writing any decoder.");
    }
    Ok(())
}
