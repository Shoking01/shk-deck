//! What does the deck actually become, step by step, while it boots?
//!
//! `reboot_probe` filtered presence on `usage_page == 0xffa0` and concluded the
//! device "never settled". That filter cannot tell the difference between
//!
//! * the deck is really gone, and
//! * the deck came back with a different PID, interface or usage page,
//!
//! and the second case is entirely plausible: a device that re-enumerates three
//! times may well change identity at each step (bootloader -> mass storage ->
//! vendor HID).
//!
//! This probe does not filter on that. It watches **every device with vendor id
//! `0x0200`**, whatever its PID, and prints the full set only when it changes.
//! The output is the complete boot choreography: how many appearances, how long
//! each lasts, and what the deck is at each step.
//!
//! Run this FIRST, then replug the deck.
//!
//! Run: `cargo run -p ss550-probe --example identity_timeline`

use hidapi::HidApi;
use std::io::Write;
use std::time::{Duration, Instant};

const VENDOR_ID: u16 = 0x0200;
const SAMPLE: Duration = Duration::from_millis(30);
const WINDOW: Duration = Duration::from_secs(90);

#[derive(PartialEq, Eq, Clone, Debug)]
struct Entry {
    pid: u16,
    interface: i32,
    usage_page: u16,
    usage: u16,
    product: String,
}

fn say(args: std::fmt::Arguments<'_>) {
    println!("{args}");
    let _ = std::io::stdout().flush();
}

macro_rules! say {
    ($($t:tt)*) => { say(format_args!($($t)*)) };
}

/// Every device from the vendor, unfiltered by PID or usage.
fn snapshot(api: &mut HidApi) -> Vec<Entry> {
    if api.refresh_devices().is_err() {
        return Vec::new();
    }
    api.device_list()
        .filter(|d| d.vendor_id() == VENDOR_ID)
        .map(|d| Entry {
            pid: d.product_id(),
            interface: d.interface_number(),
            usage_page: d.usage_page(),
            usage: d.usage(),
            product: d.product_string().unwrap_or("<none>").to_string(),
        })
        .collect()
}

fn describe(entries: &[Entry]) -> String {
    if entries.is_empty() {
        return "  (none)".to_string();
    }
    entries
        .iter()
        .map(|e| {
            format!(
                "  pid=0x{:04x} iface={} usage_page=0x{:04x} usage=0x{:04x} {:?}",
                e.pid, e.interface, e.usage_page, e.usage, e.product
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut api = HidApi::new()?;

    if !snapshot(&mut api).is_empty() {
        say!("deck is plugged in: unplug it. Measurement starts on replug.");
        say!("(no timeout: this waits as long as it takes)");
        let t = Instant::now();
        let mut next_beat = Duration::from_secs(30);
        while !snapshot(&mut api).is_empty() {
            if t.elapsed() >= next_beat {
                say!("still plugged in at t=+{}s", t.elapsed().as_secs());
                next_beat += Duration::from_secs(30);
            }
            std::thread::sleep(SAMPLE);
        }
        say!("unplugged at t=+{}ms", t.elapsed().as_millis());
    }

    say!("waiting for replug...");
    // Wait indefinitely for the deck to appear. The measurement window must
    // start at the first appearance, never at program start: a human needs
    // time to read the instruction, unplug, wait 60 s and plug back in.
    let t_wait = Instant::now();
    let mut next_beat = Duration::from_secs(30);
    while snapshot(&mut api).is_empty() {
        if t_wait.elapsed() >= next_beat {
            say!(
                "still waiting for the deck at t=+{}s",
                t_wait.elapsed().as_secs()
            );
            next_beat += Duration::from_secs(30);
        }
        std::thread::sleep(SAMPLE);
    }

    say!("appeared; measuring the boot sequence now");
    let born = Instant::now();

    let mut last: Vec<Entry> = Vec::new();
    let mut transitions = 0u32;
    let mut last_change = Instant::now();

    while born.elapsed() < WINDOW {
        let now = snapshot(&mut api);
        if now != last {
            transitions += 1;
            let elapsed = born.elapsed();
            say!(
                "t=+{:>6}ms  change #{}  ({} device(s) from vendor 0x{:04x})",
                elapsed.as_millis(),
                transitions,
                now.len(),
                VENDOR_ID
            );
            say!("{}", describe(&now));
            if !last.is_empty() && now.is_empty() {
                say!(
                    "            ^ gone after {}ms of presence",
                    last_change.elapsed().as_millis()
                );
            }
            last = now;
            last_change = Instant::now();
        }
        std::thread::sleep(SAMPLE);
    }

    say!("");
    say!(
        "RESULT: {} identity change(s) in {}s.",
        transitions,
        WINDOW.as_secs()
    );
    if last.is_empty() {
        say!("The deck ended the window ABSENT.");
    } else {
        say!(
            "The deck was present at the end, unchanged for {}ms.",
            last_change.elapsed().as_millis()
        );
        say!("If that is the vendor HID collection, this is the handle to use.");
    }
    Ok(())
}
