//! How many frames per second the application in the foreground is drawing.
//!
//! The counter comes from RTSS (`ss550_sensors::fps`), which counts the *present*
//! calls of every application it tracks. RTSS publishes a **period** — frames over
//! the last second — so the rate is its own formula, `1000 · frames / (time1 −
//! time0)`, read once. Subtracting two of our samples would compare two different
//! periods: a game running at seventy-five frames per second read as one to three.
//!
//! ## Whose frames
//!
//! The window in the foreground. A deck sits next to one game: showing the rate of
//! a background process would be a number about something nobody is looking at.

use std::time::Duration;

use ss550_plugin_api::{Cadence, CellId, CellSpec, Face, Plugin, PluginId, Reading, Snapshot};

/// The plugin's name, and the word the palette shows for it.
pub(crate) const NAME: &str = "FPS";

/// The one cell, by name.
const CELL: &str = "fps";

/// How often the rate is read.
///
/// One second, because that is RTSS's own measurement period: asking faster reads
/// the same period twice and asking slower skips periods, which averages away the
/// stutter a person is looking for.
const EVERY: Duration = Duration::from_secs(1);

/// What the deck is running, and how fast, as one cell.
pub(crate) struct Fps {
    id: PluginId,
    cells: Vec<CellSpec>,
    cell: CellId,
}

impl Fps {
    /// Builds the plugin. Reads nothing: asking RTSS is the sample's job.
    pub(crate) fn new() -> Self {
        let id = PluginId::parse("fps").expect("a usable plugin id");
        Self {
            cells: Self::describe(),
            cell: CellId::parse(CELL).expect("a usable cell id"),
            id,
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

    /// The foreground application's rate, or the reason there is none.
    ///
    /// "Nothing in front", "RTSS is not counting this app" and "measuring" are
    /// three different answers, and each one is worth saying: a key frozen on the
    /// last number would be a lie about what the machine is doing.
    fn sample(&mut self) -> Snapshot {
        let reading = match ss550_sensors::fps::foreground_process() {
            None => Reading::unavailable("nothing in front"),
            Some(pid) => match ss550_sensors::fps::frame_counters()
                .into_iter()
                .find(|counter| counter.pid == pid)
            {
                None => Reading::unavailable("RTSS is not counting this app"),
                Some(counter) => counter.fps().map_or_else(
                    || Reading::unavailable("measuring"),
                    |fps| Reading::whole(fps, ""),
                ),
            },
        };
        let mut snapshot = Snapshot::new();
        snapshot.set(self.cell.clone(), reading);
        snapshot
    }

    fn cadence(&self) -> Cadence {
        Cadence::Own(EVERY)
    }
}

#[cfg(test)]
mod tests {
    use super::Fps;
    use ss550_plugin_api::Face;

    #[test]
    fn the_cell_is_a_number_and_not_a_gauge() {
        // The face is the plugin's statement: a rate is a number, so it fills the
        // tile alone, without a label band or a bar.
        let cells = Fps::describe();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].face(), Face::Number);
        assert_eq!(cells[0].plugin().as_str(), "fps");
        assert_eq!(cells[0].cell().as_str(), "fps");
    }
}
