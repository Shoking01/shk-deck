//! How long the window takes to put its first frame on screen.
//!
//! `hyperfine` would time the process from outside, but it is not installed
//! here and the numbers would then depend on a binary the repository does not
//! ship. The app already knows the answer: it records `first frame after N ms`
//! in its own diagnostic log, measured from the first line of `main`. This
//! probe starts the process, waits for that line, and kills it — so the number
//! is the one the application measured about itself, and the tool that reads it
//! is in the repository.
//!
//! **The deck is never touched.** Each run is given its own config directory
//! and `DECK_CONTROLLER_NO_DECK`, which makes the window skip the connector
//! entirely: no device is opened, so a timing run cannot paint the deck on the
//! desk. That also removes the USB wait from the measurement, which is correct
//! — the first frame does not wait for a deck.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ss550_engine::config::{CONFIG_DIR_ENV, NO_DECK_ENV};

use crate::stats::Summary;

/// The log's name inside a config directory.
const LOG_FILE: &str = "ss550.log";

/// The line the app writes once the first frame is on screen.
const FIRST_FRAME: &str = "first frame after ";

/// The line the app writes for each frame it times.
const FRAME: &str = "render ";

/// How long one run may take before it is declared broken.
///
/// A window that never paints is a real failure, not a slow measurement: the
/// probe reports it instead of waiting forever.
const RUN_TIMEOUT: Duration = Duration::from_secs(30);

/// One launch of the app.
#[derive(Debug, Clone, Default)]
pub struct Run {
    /// Milliseconds from the first line of `main` to the first frame.
    pub first_frame_ms: u128,
    /// The render cost of the frames the app logged, in milliseconds.
    pub frames_ms: Vec<f64>,
}

/// What a startup measurement found.
#[derive(Debug, Default)]
pub struct Report {
    /// The first launch of a copy of the binary, at a path the OS had not seen.
    pub cold: Vec<Run>,
    /// The launches that followed on the original path.
    pub warm: Vec<Run>,
}

impl Report {
    /// The cold runs as a summary.
    pub fn cold_summary(&self) -> Option<Summary> {
        summary(&self.cold)
    }

    /// The warm runs as a summary.
    pub fn warm_summary(&self) -> Option<Summary> {
        summary(&self.warm)
    }

    /// Every frame the app logged, across every run.
    pub fn frame_summary(&self) -> Option<Summary> {
        let frames: Vec<Duration> = self
            .cold
            .iter()
            .chain(self.warm.iter())
            .flat_map(|run| run.frames_ms.iter().copied())
            .map(|ms| Duration::from_secs_f64(ms / 1000.0))
            .collect();
        Summary::of(&frames)
    }
}

/// Summarises the first-frame time of each run.
fn summary(runs: &[Run]) -> Option<Summary> {
    let samples: Vec<Duration> = runs
        .iter()
        .map(|run| Duration::from_millis(run.first_frame_ms as u64))
        .collect();
    Summary::of(&samples)
}

/// Measures cold and warm startup of `exe`.
///
/// `scratch` is a directory this tool owns; each run gets a fresh config
/// directory inside it, so no run can read the previous one's state.
pub fn measure(
    exe: &Path,
    scratch: &Path,
    cold_runs: usize,
    warm_runs: usize,
    tail: Duration,
    want_frames: usize,
) -> Result<Report, String> {
    let exe = exe
        .canonicalize()
        .map_err(|error| format!("{} is not usable: {error}", exe.display()))?;
    if !exe.is_file() {
        return Err(format!("{} is not a file", exe.display()));
    }

    let mut report = Report::default();

    // A cold run is a launch of a binary the file system has not seen at that
    // path: the copy is new, so nothing about this image is in the OS cache
    // under its own name. It is not a true cold start — Windows still holds the
    // DLLs it shares with every other process — and the milestone record says
    // so instead of pretending otherwise.
    let cold_dir = scratch.join("cold");
    std::fs::create_dir_all(&cold_dir)
        .map_err(|error| format!("{}: {error}", cold_dir.display()))?;
    let cold_exe = cold_dir.join(
        exe.file_name()
            .ok_or_else(|| format!("{} has no file name", exe.display()))?,
    );
    std::fs::copy(&exe, &cold_exe)
        .map_err(|error| format!("copying {} failed: {error}", exe.display()))?;

    for index in 0..cold_runs {
        let dir = scratch.join(format!("cold-run-{index}"));
        report
            .cold
            .push(one_run(&cold_exe, &dir, tail, want_frames)?);
    }

    for index in 0..warm_runs {
        let dir = scratch.join(format!("warm-run-{index}"));
        report.warm.push(one_run(&exe, &dir, tail, want_frames)?);
    }

    Ok(report)
}

/// Launches the app once and reads the timings it logged.
///
/// The child is killed as soon as it has logged what was asked for, and always
/// before returning, so a probe never leaves a controller running.
fn one_run(exe: &Path, config: &Path, tail: Duration, want_frames: usize) -> Result<Run, String> {
    std::fs::create_dir_all(config).map_err(|error| format!("{}: {error}", config.display()))?;
    let log = config.join(LOG_FILE);
    let _ = std::fs::remove_file(&log);

    let mut child = Command::new(exe)
        .env(CONFIG_DIR_ENV, config)
        .env(NO_DECK_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("starting {} failed: {error}", exe.display()))?;

    let outcome = wait_for_frame(&log, &mut child, tail, want_frames);
    stop(&mut child);
    outcome
}

/// Polls the log until the first frame is there, then collects a few more.
fn wait_for_frame(
    log: &Path,
    child: &mut Child,
    tail: Duration,
    want_frames: usize,
) -> Result<Run, String> {
    let started = Instant::now();
    let mut first_frame_ms = None;
    let mut frames_ms = Vec::new();
    let mut painted_at = None;

    loop {
        if let Ok(text) = std::fs::read_to_string(log) {
            if first_frame_ms.is_none() {
                if let Some(ms) = first_frame(&text) {
                    first_frame_ms = Some(ms);
                    painted_at = Some(Instant::now());
                }
            }
            frames_ms = frame_costs(&text);
        }

        if let Some(ms) = first_frame_ms {
            let collected = frames_ms.len() >= want_frames;
            let waited = painted_at.is_some_and(|at| at.elapsed() >= tail);
            if collected || waited {
                return Ok(Run {
                    first_frame_ms: ms,
                    frames_ms,
                });
            }
        }

        // A process that died has no first frame coming: saying so beats
        // waiting out the timeout for a window that will never open.
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "the app left before the first frame ({status}); see {}",
                log.display()
            ));
        }
        if started.elapsed() >= RUN_TIMEOUT {
            return Err(format!(
                "no first frame within {} s; see {}",
                RUN_TIMEOUT.as_secs(),
                log.display()
            ));
        }

        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Ends the child and waits for it, so nothing is left running.
fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// The first-frame timing in `text`, if the app logged one.
fn first_frame(text: &str) -> Option<u128> {
    text.lines().find_map(|line| {
        let (_, rest) = line.split_once(FIRST_FRAME)?;
        let (ms, _) = rest.split_once(" ms")?;
        ms.trim().parse().ok()
    })
}

/// Every per-frame cost in `text`, in the order the app logged them.
///
/// The app writes one decimal (`render 3 in 0.4 ms`) because a frame is not a
/// whole number of milliseconds; the parse is of the number either way.
fn frame_costs(text: &str) -> Vec<f64> {
    text.lines()
        .filter_map(|line| {
            let (_, rest) = line.split_once(FRAME)?;
            let (_, rest) = rest.split_once(" in ")?;
            let (ms, _) = rest.split_once(" ms")?;
            ms.trim().parse().ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_frame_line_is_read_as_a_number() {
        let log = "2026-11-12T10:00:00Z started v0.1.0 (hidden=false)\n\
                   2026-11-12T10:00:00Z first frame after 412 ms\n\
                   2026-11-12T10:00:00Z render 1 in 9 ms\n";
        assert_eq!(first_frame(log), Some(412));
    }

    #[test]
    fn a_log_without_a_first_frame_has_none() {
        assert_eq!(first_frame("started v0.1.0 (hidden=false)\n"), None);
    }

    #[test]
    fn the_frame_costs_keep_their_order() {
        let log = "first frame after 400 ms\n\
                   render 1 in 9 ms\n\
                   render 2 in 12 ms\n\
                   render 3 in 7 ms\n";
        assert_eq!(frame_costs(log), vec![9.0, 12.0, 7.0]);
    }

    #[test]
    fn a_frame_shorter_than_a_millisecond_is_not_read_as_zero() {
        // The app writes one decimal, so a sub-millisecond frame is still a
        // measurement. Reading it as 0 would report a frame that never
        // happened, which is the one thing a budget table must not do.
        let log = "render 1 in 0.4 ms\nrender 2 in 0.9 ms\n";
        assert_eq!(frame_costs(log), vec![0.4, 0.9]);
    }

    #[test]
    fn a_line_that_is_not_a_frame_is_not_counted() {
        let log = "the render 1 in a hurry\nrender 2 in 5 ms\n";
        assert_eq!(frame_costs(log), vec![5.0]);
    }
}
