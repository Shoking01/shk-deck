//! How many frames per second the application in the foreground is drawing.
//!
//! The counter comes from RTSS (`ss550_sensors::fps`), which counts the *present*
//! calls of every application it tracks. This module does the only arithmetic
//! that turns a counter into a rate: two readings, the difference in frames over
//! the difference in time.
//!
//! ## Why the subtraction lives here
//!
//! A single read cannot know how long it has been since the last frame — the
//! number is a counter, not a speed. Keeping the previous reading in the plugin
//! makes the rate a pure function of two samples, which is testable without RTSS,
//! without a game and without a mapping.
//!
//! ## Whose frames
//!
//! The window in the foreground. A deck sits next to one game: showing the rate
//! of a background process would be a number about something nobody is looking
//! at.

use std::time::Duration;

use ss550_plugin_api::{Cadence, CellId, CellSpec, Face, Plugin, PluginId, Reading, Snapshot};

/// The plugin's name, and the word the palette shows for it.
pub(crate) const NAME: &str = "FPS";

/// The one cell, by name.
const CELL: &str = "fps";

/// How often the rate is worked out.
///
/// One second, because the difference between two reads *is* the measurement: a
/// shorter window divides by almost nothing and a longer one averages away the
/// stutter a person is looking for.
const EVERY: Duration = Duration::from_secs(1);

/// What the deck is running, and how fast, as one cell.
pub(crate) struct Fps {
    id: PluginId,
    cells: Vec<CellSpec>,
    cell: CellId,
    /// The last counter read, and which process it belonged to.
    ///
    /// A rate needs two moments; this is the first one, carried to the next
    /// sample. It is cleared when the foreground application changes, because a
    /// difference across two programs is a number about neither.
    last: Option<Sample>,
}

/// One reading of one application's frame counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sample {
    /// The application the counter belongs to.
    pub(crate) pid: u32,
    /// Frames counted so far.
    pub(crate) frames: u32,
    /// When the last frame was counted, in milliseconds.
    pub(crate) time_ms: u32,
}

impl Fps {
    /// Builds the plugin. Reads nothing: asking RTSS is the sample's job.
    pub(crate) fn new() -> Self {
        let id = PluginId::parse("fps").expect("a usable plugin id");
        Self {
            cells: Self::describe(),
            cell: CellId::parse(CELL).expect("a usable cell id"),
            id,
            last: None,
        }
    }

    /// What this plugin measures, without building one.
    pub(crate) fn describe() -> Vec<CellSpec> {
        let plugin = PluginId::parse("fps").expect("a usable plugin id");
        vec![CellSpec::new(
            plugin,
            CellId::parse(CELL).expect("a usable cell id"),
            "FPS",
            "frames per second of the app in front",
            [0x8a, 0xd6, 0x4f],
        )
        .with_face(Face::Number)]
    }

    /// The rate between two readings of one counter, or why there is none.
    ///
    /// Split out from [`Plugin::sample`] so the arithmetic is a function of two
    /// samples and a test can pick them — a test that waits for a real game
    /// cannot tell a wrong divisor from a wrong clock.
    pub(crate) fn rate(previous: Sample, current: Sample) -> Option<Reading> {
        if previous.pid != current.pid {
            // Another program took the foreground: its counter has nothing to do
            // with the one that came before.
            return None;
        }
        let frames = current.frames.checked_sub(previous.frames)?;
        let elapsed = current.time_ms.checked_sub(previous.time_ms)?;
        if elapsed == 0 {
            return None;
        }
        let fps = (f64::from(frames) * 1000.0 / f64::from(elapsed)) as f32;
        Some(Reading::whole(fps, ""))
    }
}

impl Plugin for Fps {
    fn id(&self) -> &PluginId {
        &self.id
    }

    fn name(&self) -> &str {
        NAME
    }

    fn cells(&self) -> &[CellSpec] {
        &self.cells
    }

    fn sample(&mut self) -> Snapshot {
        let reading = self.next_reading();
        let mut snapshot = Snapshot::new();
        snapshot.set(self.cell.clone(), reading);
        snapshot
    }

    fn cadence(&self) -> Cadence {
        Cadence::Own(EVERY)
    }
}

impl Fps {
    /// The reading for this moment, keeping the counter it was taken from.
    fn next_reading(&mut self) -> Reading {
        let Some(pid) = ss550_sensors::fps::foreground_process() else {
            self.last = None;
            return Reading::unavailable("nothing in front");
        };
        let Some(counter) = ss550_sensors::fps::frame_counters()
            .into_iter()
            .find(|counter| counter.pid == pid)
        else {
            self.last = None;
            return Reading::unavailable("RTSS is not counting this app");
        };

        let current = Sample {
            pid,
            frames: counter.frames,
            time_ms: counter.time_ms,
        };
        let reading = match self.last {
            Some(previous) => Self::rate(previous, current),
            None => None,
        };
        self.last = Some(current);
        // The first sample has nothing to compare against, and a counter that has
        // not moved yet cannot answer either: both are "measuring", not zero.
        reading.unwrap_or_else(|| Reading::unavailable("measuring"))
    }
}

#[cfg(test)]
mod tests {
    use super::{Fps, Sample};
    use ss550_plugin_api::Face;

    #[test]
    fn the_rate_is_the_difference_over_the_time_it_took() {
        // Sixty frames in one second is sixty per second; thirty in a second is
        // thirty. The divisor is milliseconds, which is the classic mistake.
        let start = Sample {
            pid: 7,
            frames: 1_000,
            time_ms: 50_000,
        };
        let sixty = Fps::rate(
            start,
            Sample {
                frames: 1_060,
                time_ms: 51_000,
                ..start
            },
        )
        .expect("a rate");
        assert_eq!(sixty.text(), "60");

        let thirty = Fps::rate(
            start,
            Sample {
                frames: 1_030,
                time_ms: 51_000,
                ..start
            },
        )
        .expect("a rate");
        assert_eq!(thirty.text(), "30");
    }

    #[test]
    fn a_counter_that_did_not_move_is_zero_and_not_a_division() {
        let start = Sample {
            pid: 7,
            frames: 1_000,
            time_ms: 50_000,
        };
        let still = Fps::rate(
            start,
            Sample {
                frames: 1_000,
                time_ms: 51_000,
                ..start
            },
        )
        .expect("zero is a measurement");
        assert_eq!(still.text(), "0");
    }

    #[test]
    fn two_readings_of_one_clock_are_no_measurement_at_all() {
        // The same millisecond twice: nothing was measured, so nothing is shown.
        let start = Sample {
            pid: 7,
            frames: 1_000,
            time_ms: 50_000,
        };
        assert_eq!(Fps::rate(start, start), None);
    }

    #[test]
    fn another_application_is_another_measurement() {
        // A game closing and a browser taking the foreground must not average
        // their counters into a number about neither.
        let game = Sample {
            pid: 7,
            frames: 90_000,
            time_ms: 50_000,
        };
        let browser = Sample {
            pid: 9,
            frames: 10,
            time_ms: 50_100,
        };
        assert_eq!(Fps::rate(game, browser), None);
    }

    #[test]
    fn a_counter_that_went_backwards_is_not_a_rate() {
        // RTSS restarts its counter when an application restarts; a subtraction
        // that underflows would report four billion frames per second.
        let before = Sample {
            pid: 7,
            frames: 500,
            time_ms: 50_000,
        };
        let after = Sample {
            pid: 7,
            frames: 5,
            time_ms: 51_000,
        };
        assert_eq!(Fps::rate(before, after), None);
    }

    #[test]
    fn the_cell_is_a_number_and_not_a_gauge() {
        let cells = Fps::describe();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].face(), Face::Number);
        assert_eq!(cells[0].plugin().as_str(), "fps");
        assert_eq!(cells[0].cell().as_str(), "fps");
    }
}
