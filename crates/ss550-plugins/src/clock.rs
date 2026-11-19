//! The time of day, on three keys.
//!
//! A deck over a desk is read at a glance, and the most glanced-at number of all
//! is the clock. It is three cells rather than one because a key is 112 pixels
//! wide: `14:07:32` shrinks to something nobody can read, while three keys put
//! `14`, `07` and `32` in a size that reads from across the room.
//!
//! Each cell is its own reading, so each ticks on its own: the seconds key
//! repaints once a second, the minutes key once a minute, and the hours key once
//! an hour. The repaint rule is the plugin system's own — a face is re-sent only
//! when its text changed — so nothing here has to know about frames.

use chrono::{Local, Timelike};

use ss550_plugin_api::{Cadence, CellId, CellSpec, Face, Plugin, PluginId, Reading, Snapshot};

/// The plugin's name, and the word the palette shows for it.
pub(crate) const NAME: &str = "Clock";

/// How often the clock wants to be read.
const SECOND: std::time::Duration = std::time::Duration::from_secs(1);

/// The three cells, by name, in the order the editor lists them.
const CELLS: [&str; 3] = ["hours", "minutes", "seconds"];

/// The time of day, as three readings.
pub(crate) struct Clock {
    id: PluginId,
    cells: Vec<CellSpec>,
    ids: [CellId; 3],
}

impl Clock {
    /// Builds the plugin. Reads nothing: the clock has no handle to open.
    pub(crate) fn new() -> Self {
        let id = PluginId::parse("clock").expect("a usable plugin id");
        Self {
            cells: Self::describe(),
            ids: [
                CellId::parse(CELLS[0]).expect("a usable cell id"),
                CellId::parse(CELLS[1]).expect("a usable cell id"),
                CellId::parse(CELLS[2]).expect("a usable cell id"),
            ],
            id,
        }
    }

    /// What this plugin measures, without building one.
    ///
    /// The window lists these while it is being built, so this must stay a table
    /// of names: no clock is read here.
    pub(crate) fn describe() -> Vec<CellSpec> {
        let plugin = PluginId::parse("clock").expect("a usable plugin id");
        [
            ("hours", "HOUR", "the hour, 00 to 23", [0x9a, 0x8c, 0xff]),
            ("minutes", "MIN", "the minute, 00 to 59", [0x7c, 0x6c, 0xf0]),
            ("seconds", "SEC", "the second, 00 to 59", [0x60, 0x9a, 0xf0]),
        ]
        .into_iter()
        .map(|(cell, label, hint, accent)| {
            CellSpec::new(
                plugin.clone(),
                CellId::parse(cell).expect("a usable cell id"),
                label,
                hint,
                accent,
            )
            // A clock's key is the number and nothing else: `HOUR` over `00` is
            // one word too many on a key read from across a desk.
            .with_face(Face::Number)
        })
        .collect()
    }

    /// The three readings at one moment, each `00` to `59` where that applies.
    ///
    /// Split out from [`Plugin::snapshot`] so the formatting is a function over
    /// numbers and a test can pick the moment — a clock whose test only looks at
    /// "now" cannot tell a leading zero from a coincidence.
    pub(crate) fn readings_at(hour: u32, minute: u32, second: u32) -> [Reading; 3] {
        [
            Reading::spelled(format!("{hour:02}"), hour as f32),
            Reading::spelled(format!("{minute:02}"), minute as f32),
            Reading::spelled(format!("{second:02}"), second as f32),
        ]
    }
}

impl Plugin for Clock {
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
        let now = Local::now();
        let [hours, minutes, seconds] = Self::readings_at(now.hour(), now.minute(), now.second());
        let mut snapshot = Snapshot::new();
        snapshot.set(self.ids[0].clone(), hours);
        snapshot.set(self.ids[1].clone(), minutes);
        snapshot.set(self.ids[2].clone(), seconds);
        snapshot
    }

    /// One second, on a thread of its own.
    ///
    /// The clock is the reason the contract has cadences at all: reading the time
    /// costs nothing, and a second hand that moved every two seconds — or every
    /// time a sensor answered — would not be a clock. The machine's own walk is
    /// the expensive one and keeps its interval; this reading never waits for it.
    fn cadence(&self) -> Cadence {
        Cadence::Own(SECOND)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_part_of_the_time_is_two_digits() {
        let [hours, minutes, seconds] = Clock::readings_at(9, 7, 3);
        assert_eq!(hours.text(), "09");
        assert_eq!(minutes.text(), "07");
        assert_eq!(seconds.text(), "03");

        let [hours, minutes, seconds] = Clock::readings_at(23, 59, 59);
        assert_eq!(hours.text(), "23");
        assert_eq!(minutes.text(), "59");
        assert_eq!(seconds.text(), "59");
    }

    #[test]
    fn the_number_travels_beside_the_text_so_a_graph_can_use_it() {
        let [hours, minutes, _] = Clock::readings_at(14, 7, 0);
        assert_eq!(hours.value(), Some(14.0));
        assert_eq!(
            minutes.value(),
            Some(7.0),
            "the text pads, the number does not"
        );
    }

    #[test]
    fn the_three_cells_are_the_ones_a_key_can_be_bound_to() {
        let clock = Clock::new();
        let cells = clock.cells();
        assert_eq!(cells.len(), 3);
        let ids: Vec<&str> = cells.iter().map(|cell| cell.cell().as_str()).collect();
        assert_eq!(ids, ["hours", "minutes", "seconds"]);
        assert!(
            cells.iter().all(|cell| cell.plugin().as_str() == "clock"),
            "every cell belongs to the clock"
        );
    }

    #[test]
    fn a_snapshot_answers_all_three_cells_and_no_others() {
        let mut clock = Clock::new();
        let snapshot = clock.sample();
        for name in CELLS {
            let id = CellId::parse(name).expect("a usable cell id");
            assert!(snapshot.get(&id).is_some(), "{name} must be measured");
        }
        assert_eq!(snapshot.iter().count(), 3, "three readings, not four");
    }

    #[test]
    fn a_key_wears_the_face_of_the_part_it_is_bound_to() {
        // The repaint rule the deck obeys: a second that has not changed is not a
        // repaint, and one that has is.
        let [_, _, first] = Clock::readings_at(10, 0, 5);
        let [_, _, same] = Clock::readings_at(10, 0, 5);
        let [_, _, later] = Clock::readings_at(10, 0, 6);
        assert!(first.same_face(&same));
        assert!(!first.same_face(&later));
    }
}
