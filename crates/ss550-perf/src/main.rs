//! Measures the performance budgets in `PLAN.md` section 7.
//!
//! `PLAN.md` promises numbers, not adjectives: a cold start under 500 ms, a
//! warm start under 100 ms, a press→action under 50 ms, a repaint of every key
//! under 200 ms, and a controller that costs under 1 % of a core and 50 MB of
//! memory while it does nothing. This tool produces them, so the table can be
//! filled again on any machine instead of being copied from one afternoon.
//!
//! Four measurements, one per subcommand:
//!
//! | Subcommand | What it times |
//! | --- | --- |
//! | `startup` | the app's own first-frame line, from a cold copy and then warm |
//! | `idle` | CPU and resident memory of a running controller |
//! | `latency` | a real input report decoded by the real driver, then dispatched |
//! | `repaint` | encoding and sending a new image for every key |
//!
//! `hyperfine` is not used: it is not part of this repository, and a budget that
//! can only be checked on a machine that happens to have it is not a budget.
//!
//! **The deck is never written to.** `startup` starts the window with
//! `DECK_CONTROLLER_NO_DECK`, so no device is opened at all; `latency` and
//! `repaint` drive `deck_simulator::Ss550WireFake`. `idle` only reads numbers
//! from a process that is already running.

#![deny(unsafe_code)]

mod idle;
mod latency;
mod repaint;
mod startup;
mod stats;

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

/// What the tool understands, printed by `--help` and on a bad command.
const USAGE: &str = "\
ss550-perf - the numbers behind PLAN.md section 7

USAGE:
  ss550-perf startup --exe <path> [--cold N] [--warm N] [--tail-ms N] [--frames N]
  ss550-perf idle --pid <pid> [--seconds N]
  ss550-perf latency [--iterations N]
  ss550-perf repaint [--iterations N]

startup   launches the app and reads the first-frame timing it logs itself.
          Each run gets a fresh config directory and never opens a deck.
idle      watches a running controller for a window and reports CPU and RSS.
latency   decodes a real input report through the real driver over the wire
          fake, then hands the bound action to the runner.
repaint   encodes one image per key at the deck's own spec and sends them all
          over the wire fake - the work of a profile switch.

Nothing here writes to a deck. Pass --help to see this again.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };

    let rest = &args[1..];
    let outcome = match command {
        "startup" => run_startup(rest),
        "idle" => run_idle(rest),
        "latency" => run_latency(rest),
        "repaint" => run_repaint(rest),
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        other => Err(format!("unknown command {other}\n\n{USAGE}")),
    };

    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

/// The value of `--name`, if it was given.
fn value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|at| args.get(at + 1))
        .map(String::as_str)
}

/// The number after `--name`, or `default` when the flag is absent.
fn number<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> Result<T, String> {
    match value(args, name) {
        Some(text) => text
            .parse()
            .map_err(|_| format!("{name} needs a number, not {text:?}")),
        None => Ok(default),
    }
}

/// A scratch directory of this tool's own, outside the repository.
///
/// Each startup run gets a config directory inside it, and the logs stay there
/// as evidence: a number with no log behind it is a claim.
fn scratch_dir() -> Result<std::path::PathBuf, String> {
    let path = std::env::temp_dir().join(format!("ss550-perf-{}", std::process::id()));
    std::fs::create_dir_all(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(path)
}

/// The whole-number count of `seconds`.
fn whole(seconds: u64) -> usize {
    usize::try_from(seconds).unwrap_or(usize::MAX)
}

fn run_startup(args: &[String]) -> Result<(), String> {
    let exe = value(args, "--exe").ok_or("startup needs --exe <path to the release build>")?;
    let cold = whole(number(args, "--cold", 1)?);
    let warm = whole(number(args, "--warm", 5)?);
    let tail_ms: u64 = number(args, "--tail-ms", 1500)?;
    let frames = whole(number(args, "--frames", 5)?);

    let scratch = scratch_dir()?;
    println!("scratch: {}", scratch.display());

    let report = startup::measure(
        Path::new(exe),
        &scratch,
        cold,
        warm,
        Duration::from_millis(tail_ms),
        frames,
    )?;

    match report.cold_summary() {
        Some(summary) => println!("{}", summary.line("cold startup")),
        None => println!("cold startup          no runs taken"),
    }
    match report.warm_summary() {
        Some(summary) => println!("{}", summary.line("warm startup")),
        None => println!("warm startup          no runs taken"),
    }
    match report.frame_summary() {
        Some(summary) => println!("{}", summary.line("window frame")),
        None => println!("window frame          no frame was logged"),
    }
    Ok(())
}

fn run_idle(args: &[String]) -> Result<(), String> {
    let pid: u32 = number(args, "--pid", 0)?;
    if pid == 0 {
        return Err("idle needs --pid <the running controller>".to_string());
    }
    let seconds: u64 = number(args, "--seconds", 10)?;
    if seconds == 0 {
        return Err("--seconds must be at least 1".to_string());
    }

    let report = idle::measure(pid, Duration::from_secs(seconds))?;
    println!(
        "pid {pid}, {} samples over {seconds} s",
        report.samples.len()
    );
    println!(
        "cpu                   mean {:>8.3} %   max {:>7.3} %   (of one core)",
        report.mean_cpu(),
        report.max_cpu()
    );
    let (over, highest) = report.above(1.0);
    if over == 0 {
        println!(
            "cpu over 1 %          none of {} samples",
            report.samples.len()
        );
    } else {
        println!(
            "cpu over 1 %          {over} of {} samples, peak {highest:.3} %",
            report.samples.len()
        );
    }
    println!(
        "rss                   min {:>8.1} MiB   max {:>7.1} MiB",
        idle::mib(report.min_rss()),
        idle::mib(report.max_rss())
    );
    Ok(())
}

fn run_latency(args: &[String]) -> Result<(), String> {
    let iterations = whole(number(args, "--iterations", 30)?);
    let report = latency::measure(iterations)?;

    println!("key {} (as the engine numbers it)", report.key.get());
    println!("{}", report.decode.line("decode"));
    println!("{}", report.dispatch.line("dispatch"));
    println!("{}", report.spawn.line("spawn"));
    println!(
        "host total            median {:>8.2} ms   (decode + dispatch)",
        report.host_ms()
    );
    Ok(())
}

fn run_repaint(args: &[String]) -> Result<(), String> {
    let iterations = whole(number(args, "--iterations", 5)?);
    let report = repaint::measure(iterations)?;

    if !repaint::is_release() {
        println!(
            "WARNING: a debug build does not optimise the encoder, so the encode\n\
             column here is roughly an order of magnitude slower than the one the\n\
             user gets. Build with --release before quoting a number."
        );
        println!();
    }
    println!(
        "{} keys, {} bytes per encoded tile",
        report.keys, report.tile_bytes
    );
    println!("{}", report.encode.line("encode all keys"));
    println!("{}", report.send.line("send all keys"));
    println!("{}", report.total.line("full repaint"));
    Ok(())
}
