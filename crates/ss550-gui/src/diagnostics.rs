//! The append-only diagnostic log.
//!
//! The release build is a GUI subsystem with no console, so a panic would
//! otherwise leave no trace at all. Every run appends a `started` line, the
//! tray's Quit appends a `quit` line, and a panic hook appends the panic before
//! the process goes down. A `started` line with no matching `quit` is what a
//! crash looks like in this file.
//!
//! Nothing here may fail the app: a log that cannot be written is a log that
//! cannot be written, and the controller keeps running.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The log's name inside the app's config directory.
pub const LOG_FILE: &str = "ss550.log";

/// The previous log, kept when the current one rotates.
pub const PREVIOUS_FILE: &str = "ss550.log.old";

/// Rotate before a write that would push the file past this size.
pub const MAX_BYTES: u64 = 1_000_000;

/// The log's full path, next to `bindings.json`.
pub fn log_path() -> PathBuf {
    ss550_engine::config_dir().join(LOG_FILE)
}

/// Appends one timestamped line to the log the app uses.
pub fn record(message: &str) {
    record_at(&log_path(), message);
}

/// Installs the hook that records a panic before the process goes down.
///
/// Call once, as early as possible. The hook writes the message and the place,
/// then hands the panic to the hook that was there before, so debug builds
/// still print it to the console.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|text| (*text).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a payload that is not a string".to_string());
        let place = info
            .location()
            .map(|place| format!("{}:{}:{}", place.file(), place.line(), place.column()))
            .unwrap_or_else(|| "an unknown place".to_string());
        record(&format!("panic at {place}: {message}"));
        previous(info);
    }));
}

/// Opens the log with whatever the system uses for text files.
///
/// Returns the reason when the system would not take it, so the window can say
/// so instead of failing in silence.
pub fn reveal() -> Result<(), String> {
    let path = log_path();
    record("the log was opened from the controller");

    // Notepad is the one text viewer that exists on every Windows machine.
    // `start` would go through the `.log` file association, and a clean Windows
    // has no association for `.log` at all, so it would do nothing.
    let mut command = if cfg!(windows) {
        std::process::Command::new("notepad")
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };

    match command.arg(&path).spawn() {
        Ok(_) => Ok(()),
        Err(error) => Err(format!("Could not open {}: {error}", path.display())),
    }
}

/// Appends one timestamped line to `path`, rotating it first when it is full.
fn record_at(path: &Path, message: &str) {
    rotate_if_full(path);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(file, "{} {}", timestamp_at(SystemTime::now()), message);
}

/// Renames a full log to `ss550.log.old`, dropping the one already there.
fn rotate_if_full(path: &Path) {
    let too_big = fs::metadata(path).is_ok_and(|meta| meta.len() >= MAX_BYTES);
    if !too_big {
        return;
    }
    let previous = path.with_file_name(PREVIOUS_FILE);
    let _ = fs::remove_file(&previous);
    let _ = fs::rename(path, &previous);
}

/// `YYYY-MM-DDTHH:MM:SSZ`, in UTC.
fn timestamp_at(now: SystemTime) -> String {
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (rest / 3_600, (rest % 3_600) / 60, rest % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a civil date.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_part + 2) / 5 + 1) as u32;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A path this process owns, so parallel tests never share a log.
    fn scratch(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("ss550-diagnostics-{}-{name}", std::process::id()));
        let _ = fs::remove_file(&path);
        path
    }

    fn at(seconds: u64) -> String {
        timestamp_at(UNIX_EPOCH + Duration::from_secs(seconds))
    }

    #[test]
    fn the_epoch_is_the_first_day_of_1970() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn known_instants_match_the_calendar() {
        // 2001-09-09T01:46:40Z and 2023-11-14T22:13:20Z, the two the Unix
        // clock is usually quoted with.
        assert_eq!(at(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(at(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn a_leap_day_is_a_leap_day() {
        // 2024-02-29T12:00:00Z.
        assert_eq!(at(1_709_208_000), "2024-02-29T12:00:00Z");
    }

    #[test]
    fn a_line_carries_the_timestamp_and_the_message() {
        let path = scratch("line");
        record_at(&path, "quit from the tray");
        let text = fs::read_to_string(&path).expect("the log was written");
        let _ = fs::remove_file(&path);
        assert!(text.ends_with(" quit from the tray\n"), "got: {text:?}");
        assert_eq!(text.len(), "1970-01-01T00:00:00Z".len() + 1 + 18 + 1);
        assert!(text.starts_with("20"), "got: {text:?}");
    }

    #[test]
    fn a_full_log_rotates_before_the_next_line() {
        let path = scratch("rotate");
        fs::write(&path, vec![b'x'; MAX_BYTES as usize]).expect("the full log was written");
        let previous = path.with_file_name(PREVIOUS_FILE);
        let _ = fs::remove_file(&previous);

        record_at(&path, "after the rotation");

        let line = fs::read_to_string(&path).expect("a fresh log replaced the full one");
        let kept = fs::metadata(&previous).map(|meta| meta.len());
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&previous);
        assert!(line.ends_with(" after the rotation\n"), "got: {line:?}");
        assert_eq!(kept.ok(), Some(MAX_BYTES), "the full log was kept as .old");
    }

    #[test]
    fn a_small_log_is_left_alone() {
        let path = scratch("small");
        record_at(&path, "first");
        record_at(&path, "second");
        let text = fs::read_to_string(&path).expect("the log was written");
        let previous = path.with_file_name(PREVIOUS_FILE);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&previous);
        assert_eq!(text.lines().count(), 2, "got: {text:?}");
    }
}
