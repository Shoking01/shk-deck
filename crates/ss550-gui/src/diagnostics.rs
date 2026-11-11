//! The append-only diagnostic log.
//!
//! The release build is a GUI subsystem with no console, so a panic would
//! otherwise leave no trace at all. Every run appends a `started` line, the
//! tray's Quit appends a `quit` line, and a panic hook appends the panic before
//! the process goes down.
//!
//! A `started` line with no matching `quit` means the process did not exit
//! through the tray's Quit. That is what an unexpected exit looks like, but it
//! is a narrowed search rather than a verdict: a logoff, a shutdown or an "End
//! task" kills the process without running that handler either.
//!
//! Nothing here may fail the app: a log that cannot be written is a log that
//! cannot be written, and the controller keeps running.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The log's name inside the app's config directory.
const LOG_FILE: &str = "ss550.log";

/// The previous log, kept when the current one rotates.
const PREVIOUS_FILE: &str = "ss550.log.old";

/// Rotate before a line would push the file past this size.
const MAX_BYTES: u64 = 1_000_000;

/// The log's full path, next to `bindings.json`.
fn log_path() -> PathBuf {
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
        record(&format!(
            "panic at {}: {}",
            panic_place(info.location()),
            panic_message(info.payload())
        ));
        previous(info);
    }));
}

/// The panic's message, whichever way the payload carries it.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a payload that is not a string".to_string())
}

/// Where the panic happened, or a placeholder when the place is unknown.
fn panic_place(location: Option<&std::panic::Location<'_>>) -> String {
    location
        .map(|place| format!("{}:{}:{}", place.file(), place.line(), place.column()))
        .unwrap_or_else(|| "an unknown place".to_string())
}

/// Opens the log in a text viewer.
///
/// Notepad on Windows, `open` on macOS, `xdg-open` elsewhere. Notepad is named
/// explicitly rather than through the `.log` file association: a clean Windows
/// has no association for `.log` at all, so `start` would do nothing. It is also
/// the system copy by absolute path, so the lookup never walks the process's
/// current directory, where a planted `notepad.exe` would otherwise run.
///
/// Returns the reason when the viewer would not start, so the window can say so
/// instead of failing in silence. The outcome, not the intent, is what the log
/// records.
pub fn reveal() -> Result<(), String> {
    let path = log_path();

    let mut command = if cfg!(windows) {
        std::process::Command::new(notepad_path())
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };

    match command.arg(&path).spawn() {
        Ok(_) => {
            record("the log was opened");
            Ok(())
        }
        Err(error) => {
            let reason = format!("Could not open {}: {error}", path.display());
            record(&reason);
            Err(reason)
        }
    }
}

/// The system Notepad, by absolute path.
///
/// `SystemRoot` is set for every interactive Windows process. The bare name is
/// the fallback when it is not, so the reveal still works in a stripped
/// environment, where there is no current directory to plant anything in.
fn notepad_path() -> std::ffi::OsString {
    match std::env::var_os("SystemRoot") {
        Some(root) => Path::new(&root)
            .join("System32")
            .join("notepad.exe")
            .into_os_string(),
        None => std::ffi::OsString::from("notepad"),
    }
}

/// Appends one timestamped line to `path`, rotating it first when it is full.
///
/// The whole line is built before the file is opened and written in one call,
/// so a panic on a worker thread cannot interleave with a line already being
/// written. `record_at` is the only writer.
fn record_at(path: &Path, message: &str) {
    let line = format!("{} {}\n", timestamp_at(SystemTime::now()), escape(message));
    rotate_if_full(path, line.len() as u64);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = file.write_all(line.as_bytes());
}

/// Renames a full log to `ss550.log.old`, dropping the one already there.
///
/// A viewer holding the file open blocks the rename on Windows, and so does an
/// indexer, so a failed rename truncates in place instead: the previous log is
/// lost, but the file cannot grow past its bound forever.
fn rotate_if_full(path: &Path, incoming: u64) {
    let current = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if current + incoming <= MAX_BYTES {
        return;
    }
    let previous = path.with_file_name(PREVIOUS_FILE);
    let _ = fs::remove_file(&previous);
    if fs::rename(path, &previous).is_ok() {
        return;
    }
    let _ = OpenOptions::new().write(true).truncate(true).open(path);
}

/// Replaces control characters so one message stays one line.
///
/// A panic payload can carry newlines, and an unescaped one would append extra
/// lines to the log, including a fabricated `quit` marker.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if other.is_control() => out.push('?'),
            other => out.push(other),
        }
    }
    out
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

    /// A directory this process owns, so parallel tests never share a log.
    ///
    /// It must be a directory, not a bare file name in the temp folder:
    /// `with_file_name` drops everything before the last separator, so two
    /// tests would otherwise share one `ss550.log.old`.
    fn scratch(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("ss550-diagnostics-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("the scratch directory");
        dir
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
        let dir = scratch("line");
        let path = dir.join(LOG_FILE);
        record_at(&path, "quit from the tray");
        let text = fs::read_to_string(&path).expect("the log was written");
        let _ = fs::remove_dir_all(&dir);
        let stamp = "1970-01-01T00:00:00Z";
        assert_eq!(text.len(), stamp.len() + " quit from the tray\n".len());
        assert_eq!(text.lines().count(), 1, "got: {text:?}");
        assert!(text.ends_with(" quit from the tray\n"), "got: {text:?}");
        assert!(text.starts_with("20"), "got: {text:?}");
    }

    #[test]
    fn a_newline_in_a_message_cannot_forge_a_second_line() {
        let dir = scratch("forge");
        let path = dir.join(LOG_FILE);
        record_at(&path, "boom\nquit from the tray");
        let text = fs::read_to_string(&path).expect("the log was written");
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(text.lines().count(), 1, "got: {text:?}");
        assert!(
            text.ends_with(" boom\\nquit from the tray\n"),
            "got: {text:?}"
        );
    }

    #[test]
    fn a_panic_payload_is_read_whatever_shape_it_has() {
        assert_eq!(panic_message(&"static text"), "static text");
        assert_eq!(panic_message(&String::from("owned text")), "owned text");
        assert_eq!(panic_message(&7u8), "a payload that is not a string");
    }

    #[test]
    fn a_panic_without_a_place_says_so() {
        assert_eq!(panic_place(None), "an unknown place");
    }

    #[test]
    fn a_full_log_rotates_before_the_next_line() {
        let dir = scratch("rotate");
        let path = dir.join(LOG_FILE);
        let previous = dir.join(PREVIOUS_FILE);
        fs::write(&path, vec![b'x'; MAX_BYTES as usize]).expect("the full log was written");

        record_at(&path, "after the rotation");

        let line = fs::read_to_string(&path).expect("a fresh log replaced the full one");
        let kept = fs::metadata(&previous).map(|meta| meta.len());
        let _ = fs::remove_dir_all(&dir);
        assert!(line.ends_with(" after the rotation\n"), "got: {line:?}");
        assert_eq!(kept.ok(), Some(MAX_BYTES), "the full log was kept as .old");
    }

    #[test]
    fn a_log_one_line_short_of_the_bound_rotates_before_overflowing() {
        let dir = scratch("bound");
        let path = dir.join(LOG_FILE);
        let previous = dir.join(PREVIOUS_FILE);
        // One byte below the bound: the current length alone says "not full",
        // but the line about to be written would push it past it.
        fs::write(&path, vec![b'x'; MAX_BYTES as usize - 1]).expect("the nearly full log");

        record_at(&path, "the line that would overflow");

        let kept = fs::metadata(&previous).map(|meta| meta.len());
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(kept.ok(), Some(MAX_BYTES - 1), "the nearly full log moved");
    }

    #[test]
    fn a_small_log_is_left_alone() {
        let dir = scratch("small");
        let path = dir.join(LOG_FILE);
        record_at(&path, "first");
        record_at(&path, "second");
        let text = fs::read_to_string(&path).expect("the log was written");
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(text.lines().count(), 2, "got: {text:?}");
    }
}
