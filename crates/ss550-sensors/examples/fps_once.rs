//! Prints what RTSS is counting: every tracked application and its frame counter.
//!
//! Support tool, not part of the window: when a key's FPS reads nothing, this
//! answers whether RTSS is running and what it is tracking, before the deck is
//! blamed.
//!
//! ```text
//! cargo run -p ss550-sensors --example fps_once
//! ```
//!
//! Twice, a second apart, is the whole measurement: the rate is the difference
//! between the two readings.

fn main() {
    let counters = ss550_sensors::fps::frame_counters();
    let foreground = ss550_sensors::fps::foreground_process();

    if counters.is_empty() {
        println!("no counters: RTSS is not running, or it is tracking nothing");
    }
    for counter in &counters {
        let mine = match foreground {
            Some(pid) if pid == counter.pid => "  <- foreground",
            _ => "",
        };
        println!(
            "pid {:>6}  {:>10} frames at {:>10} ms  {}{mine}",
            counter.pid, counter.frames, counter.time_ms, counter.name
        );
    }
    println!("foreground pid: {foreground:?}");
}
