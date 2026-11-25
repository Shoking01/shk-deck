//! Prints what RTSS is counting and the rate it reports for each application.
//!
//! Support tool, not part of the window: when an FPS key reads nothing — or a
//! number too low to be true — this answers whether RTSS is hooking that game and
//! what its own counter says, without involving the deck.
//!
//! ```text
//! cargo run -p ss550-sensors --example fps_once
//! ```
//!
//! One read is enough: RTSS publishes a **period** (frames over the last second)
//! and the rate is its own formula, `1000 · frames / (time1 - time0)`. This prints
//! the same number both ways — from the frames and from the frame time — so a
//! disagreement is visible.

use ss550_sensors::fps::{foreground_process, frame_counters};

fn main() {
    let counters = frame_counters();
    let foreground = foreground_process();

    if counters.is_empty() {
        println!("no counters: RTSS is not running, or it is tracking nothing");
        println!("foreground pid: {foreground:?}");
        return;
    }

    println!(
        "{:>7}  {:>8}  {:>8}  {:>8}  {:>8}  application",
        "pid", "fps", "by time", "frames", "period"
    );
    for counter in &counters {
        let fps = counter
            .fps()
            .map(|fps| format!("{fps:.1}"))
            .unwrap_or_else(|| "-".to_string());
        // The header's second formula, as a cross-check.
        let by_time = match counter.frame_time_us {
            0 => "-".to_string(),
            micros => format!("{:.1}", 1_000_000.0 / f64::from(micros)),
        };
        let period = counter.time1.saturating_sub(counter.time0);
        let mine = match foreground {
            Some(pid) if pid == counter.pid => "  <- foreground",
            _ => "",
        };
        println!(
            "{:>7}  {:>8}  {:>8}  {:>8}  {:>8}  {}{mine}",
            counter.pid, fps, by_time, counter.frames, period, counter.name
        );
    }
    println!();
    println!("foreground pid: {foreground:?}");
    println!(
        "fps over 60 with a \"-\" in the fps column means RTSS has not measured a period yet."
    );
    println!("A game missing from the list means RTSS is not hooking it:");
    println!("RTSS → Application detection level → High, and start the game after RTSS.");
}
