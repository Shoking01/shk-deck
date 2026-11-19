//! The lines a reading is judged by, as the user sets them.
//!
//! The Stats page paints a card amber past a warning and red past a limit. Those
//! two numbers are a property of the machine, not of this program: a desktop with
//! a tower cooler is comfortable at 80 °C and a laptop is not, and a card at 98 %
//! load is a normal frame while a card at 98 °C is a fault. So the lines are the
//! user's, kept in `thresholds.json` beside the rest of the window's settings, and
//! this module is the whole of the rule:
//!
//! - a **share** (a reading that shows a percentage) is judged 0–100;
//! - a **temperature** is judged 0–150, which is past any running chip;
//! - the warning line is always below the limit, because a card that is red before
//!   it is amber says nothing at all.
//!
//! A file that cannot be understood is not a fault to fail on: the defaults are
//! the answer and the status line says what happened, the same courtesy the title
//! style already extends.

use std::path::PathBuf;

use ss550_plugin_api::Reading;

use crate::history::Severity;

/// Where the lines are kept.
const THRESHOLDS_FILE: &str = "thresholds.json";

/// The highest a share can be, and the highest a temperature sensibly is.
const SHARE_CEILING: f32 = 100.0;
const DEGREES_CEILING: f32 = 150.0;

/// One kind of reading's two lines.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Lines {
    /// At or above this, a card is marked.
    pub(crate) warn: f32,
    /// At or above this, it is marked hard.
    pub(crate) alarm: f32,
}

impl Lines {
    /// Two lines, put in order and inside the kind's own range.
    ///
    /// This is the only way a pair is built, so a file, a click and a default all
    /// end up valid: `warn` below `alarm`, `alarm` at or under the ceiling, and
    /// neither of them below zero.
    pub(crate) fn new(warn: f32, alarm: f32, ceiling: f32) -> Self {
        let alarm = alarm.clamp(1.0, ceiling);
        let warn = warn.clamp(0.0, alarm - 1.0);
        Self { warn, alarm }
    }
}

/// The lines for both kinds of reading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Thresholds {
    /// What a percentage is judged by.
    pub(crate) share: Lines,
    /// What a temperature is judged by.
    pub(crate) degrees: Lines,
}

impl Default for Thresholds {
    /// The lines a machine over a desk is comfortable with.
    fn default() -> Self {
        Self {
            share: Lines::new(80.0, 95.0, SHARE_CEILING),
            degrees: Lines::new(80.0, 95.0, DEGREES_CEILING),
        }
    }
}

/// Which of the two kinds a reading is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A reading with a bar: a percentage of something.
    Share,
    /// A reading in degrees.
    Degrees,
}

impl Thresholds {
    /// The lines that judge `kind`.
    pub(crate) fn lines(&self, kind: Kind) -> Lines {
        match kind {
            Kind::Share => self.share,
            Kind::Degrees => self.degrees,
        }
    }

    /// The same settings with one kind's lines replaced, kept valid.
    pub(crate) fn with(&self, kind: Kind, lines: Lines) -> Self {
        match kind {
            Kind::Share => Self {
                share: Lines::new(lines.warn, lines.alarm, SHARE_CEILING),
                degrees: self.degrees,
            },
            Kind::Degrees => Self {
                share: self.share,
                degrees: Lines::new(lines.warn, lines.alarm, DEGREES_CEILING),
            },
        }
    }

    /// The ceiling of one kind.
    pub(crate) fn ceiling(kind: Kind) -> f32 {
        match kind {
            Kind::Share => SHARE_CEILING,
            Kind::Degrees => DEGREES_CEILING,
        }
    }

    /// The word the Settings page uses for a kind.
    pub(crate) fn label(kind: Kind) -> &'static str {
        match kind {
            Kind::Share => "Shares",
            Kind::Degrees => "Temperatures",
        }
    }
}

/// Which kind a reading belongs to, from what it says about itself.
///
/// The text is the reading's own statement of its unit — `42%`, `59°C` — and it is
/// the same signal the face uses to decide whether it has a bar.
pub(crate) fn kind_of(reading: &Reading) -> Option<Kind> {
    let text = reading.text();
    if text.ends_with('%') {
        return Some(Kind::Share);
    }
    if text.contains('°') {
        return Some(Kind::Degrees);
    }
    None
}

/// How bad one reading is, by the lines in force.
pub(crate) fn severity(reading: &Reading, limits: &Thresholds) -> Severity {
    let (Some(value), Some(kind)) = (reading.value(), kind_of(reading)) else {
        return Severity::Quiet;
    };
    let lines = limits.lines(kind);
    if value >= lines.alarm {
        Severity::Alarm
    } else if value >= lines.warn {
        Severity::Warn
    } else {
        Severity::Quiet
    }
}

/// Where the lines are kept.
pub(crate) fn path() -> PathBuf {
    ss550_engine::config_dir().join(THRESHOLDS_FILE)
}

/// Reads the saved lines, or the defaults when there is nothing usable.
///
/// The second half is a sentence for the status line: a file that was never
/// written is a first run, and a file that cannot be understood is the user's
/// choice lost, which is worth saying.
pub(crate) fn load() -> (Thresholds, Option<String>) {
    let path = path();
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let (thresholds, understood) = from_json(&text);
            let note = (!understood).then(|| {
                format!(
                    "The thresholds in {} are not readable: using the defaults.",
                    path.display()
                )
            });
            (thresholds, note)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Thresholds::default(), None),
        Err(error) => (
            Thresholds::default(),
            Some(format!(
                "The thresholds could not be read ({error}): using the defaults."
            )),
        ),
    }
}

/// Writes the lines where the next start reads them.
pub(crate) fn save(thresholds: &Thresholds) -> Result<(), String> {
    let path = path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    std::fs::write(&path, to_json(thresholds))
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

/// The lines as the bytes of the file.
///
/// Hand-written JSON, like the title style's: one object with two pairs, no
/// numbers that depend on a locale, and nothing this build cannot read back.
fn to_json(thresholds: &Thresholds) -> String {
    format!(
        "{{\n  \"share\": {{ \"warn\": {:.0}, \"alarm\": {:.0} }},\n  \"degrees\": {{ \"warn\": {:.0}, \"alarm\": {:.0} }}\n}}\n",
        thresholds.share.warn,
        thresholds.share.alarm,
        thresholds.degrees.warn,
        thresholds.degrees.alarm
    )
}

/// The lines in a file, and whether every field was understood.
///
/// A missing or unusable pair falls back to its default and marks the file as not
/// understood, so the status line can say so. Values are clamped on the way in:
/// a file that says `alarm: 0` is an alarm everyone would see.
fn from_json(text: &str) -> (Thresholds, bool) {
    let mut thresholds = Thresholds::default();
    let mut understood = true;

    for (kind, key, ceiling) in [
        (Kind::Share, "share", SHARE_CEILING),
        (Kind::Degrees, "degrees", DEGREES_CEILING),
    ] {
        match pair(text, key) {
            Some((warn, alarm)) => {
                thresholds = thresholds.with(kind, Lines::new(warn, alarm, ceiling));
            }
            None => understood = false,
        }
    }
    (thresholds, understood)
}

/// The two numbers of one pair, when both are there.
fn pair(text: &str, key: &str) -> Option<(f32, f32)> {
    let start = text.find(&format!("\"{key}\""))?;
    let body = &text[start..];
    let open = body.find('{')?;
    let close = body.find('}')?;
    let fields = &body[open + 1..close];
    Some((number(fields, "warn")?, number(fields, "alarm")?))
}

/// One number of a pair, when it is there and is a number.
fn number(fields: &str, key: &str) -> Option<f32> {
    let start = fields.find(&format!("\"{key}\""))?;
    let rest = &fields[start + key.len() + 2..];
    let colon = rest.find(':')?;
    let rest = &rest[colon + 1..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    rest[..end].trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_a_comfortable_machine_and_are_always_in_order() {
        let defaults = Thresholds::default();
        assert_eq!(defaults.share, Lines::new(80.0, 95.0, SHARE_CEILING));
        assert_eq!(defaults.degrees, Lines::new(80.0, 95.0, DEGREES_CEILING));
        for lines in [defaults.share, defaults.degrees] {
            assert!(lines.warn < lines.alarm, "a card cannot be red first");
        }
    }

    #[test]
    fn a_pair_is_clamped_into_its_kind_and_kept_in_order() {
        // A limit above a share is a share of a hundred.
        let lines = Lines::new(70.0, 300.0, SHARE_CEILING);
        assert_eq!((lines.warn, lines.alarm), (70.0, 100.0));
        // A warning at or above the limit is pulled below it.
        let lines = Lines::new(95.0, 90.0, SHARE_CEILING);
        assert!(lines.warn < lines.alarm);
        assert_eq!(lines.alarm, 90.0);
        // And a negative warning is a zero.
        assert_eq!(Lines::new(-5.0, 90.0, SHARE_CEILING).warn, 0.0);
    }

    #[test]
    fn a_reading_is_judged_by_the_lines_in_force_and_not_by_a_constant() {
        let strict = Thresholds {
            share: Lines::new(50.0, 70.0, SHARE_CEILING),
            degrees: Lines::new(60.0, 80.0, DEGREES_CEILING),
        };
        assert_eq!(
            severity(&Reading::percent(55.0), &strict),
            Severity::Warn,
            "the same reading under the defaults is quiet"
        );
        assert_eq!(severity(&Reading::percent(75.0), &strict), Severity::Alarm);
        assert_eq!(
            severity(&Reading::percent(55.0), &Thresholds::default()),
            Severity::Quiet
        );
        // A size has no line to cross.
        assert_eq!(
            severity(&Reading::whole(12.0, " GB"), &strict),
            Severity::Quiet
        );
        assert_eq!(kind_of(&Reading::whole(12.0, " GB")), None);
    }

    #[test]
    fn the_file_reads_back_exactly_what_was_written() {
        let thresholds = Thresholds {
            share: Lines::new(70.0, 90.0, SHARE_CEILING),
            degrees: Lines::new(75.0, 88.0, DEGREES_CEILING),
        };
        let json = to_json(&thresholds);
        let (read, understood) = from_json(&json);
        assert!(understood, "{json}");
        assert_eq!(read, thresholds);
    }

    #[test]
    fn a_file_that_is_not_the_shape_this_build_writes_is_not_understood() {
        assert_eq!(from_json(""), (Thresholds::default(), false));
        assert_eq!(from_json("{\"share\": 5}"), (Thresholds::default(), false));
        // One pair missing: the other is still read, and the file is reported.
        let (read, understood) = from_json("{\"share\": {\"warn\": 60, \"alarm\": 70}}");
        assert!(!understood);
        assert_eq!(read.share, Lines::new(60.0, 70.0, SHARE_CEILING));
        assert_eq!(read.degrees, Thresholds::default().degrees);
    }
}
