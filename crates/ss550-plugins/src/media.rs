//! What the machine is playing, as one cell.
//!
//! A player's key shows the track and, if the binding says so, pauses it on the
//! same press. The reading is the track's title with the artist as its detail,
//! and its face is [`Face::Title`] — words, not a gauge: a track name is not a
//! number and does not fit like one.
//!
//! ## Why the read is not here
//!
//! Windows exposes the current session through WinRT
//! (`GlobalSystemMediaTransportControlsSessionManager`), and every call in it is
//! `unsafe`. This crate is not allowed `unsafe`, so the read lives in
//! `ss550-sensors` — the crate the project trusts to read the machine — and this
//! plugin only asks it a question: [`ss550_sensors::media::now_playing`].
//!
//! ## Cadence
//!
//! Its own, not the machine walk's: reading the session is three WinRT objects,
//! and a title does not change at the pace a processor's load does. Two seconds
//! is faster than a person can read a track, and it keeps the key from waiting
//! behind a sensor.

use std::time::Duration;

use ss550_plugin_api::{Cadence, CellId, CellSpec, Face, Plugin, PluginId, Reading, Snapshot};

/// The plugin's name, and the word the palette shows for it.
pub(crate) const NAME: &str = "Now playing";

/// The one cell, by name.
const CELL: &str = "now";

/// How often the track is asked about.
const EVERY: Duration = Duration::from_secs(2);

/// What a player is showing.
pub(crate) struct NowPlaying {
    id: PluginId,
    cells: Vec<CellSpec>,
    cell: CellId,
}

impl NowPlaying {
    /// Builds the plugin. Reads nothing: asking Windows is the sample's job.
    pub(crate) fn new() -> Self {
        let id = PluginId::parse("media").expect("a usable plugin id");
        Self {
            cells: Self::describe(),
            cell: CellId::parse(CELL).expect("a usable cell id"),
            id,
        }
    }

    /// What this plugin measures, without building one.
    pub(crate) fn describe() -> Vec<CellSpec> {
        let plugin = PluginId::parse("media").expect("a usable plugin id");
        vec![CellSpec::new(
            plugin,
            CellId::parse(CELL).expect("a usable cell id"),
            "PLAY",
            "what the player is showing",
            [0x1d, 0xb9, 0x54],
        )
        .with_face(Face::Title)]
    }

    /// The reading for one answer from the system.
    ///
    /// Split out from [`Plugin::sample`] so the formatting is a function over
    /// text and a test can pick the track — a test that only looks at "now"
    /// cannot tell an empty artist from a missing one.
    pub(crate) fn reading(title: &str, artist: &str) -> Reading {
        let track = title.trim();
        let artist = artist.trim();
        match artist.is_empty() {
            true => Reading::spelled(track, 0.0),
            false => Reading::spelled(track, 0.0).with_detail(artist),
        }
    }
}

impl Plugin for NowPlaying {
    fn id(&self) -> &PluginId {
        &self.id
    }

    fn name(&self) -> &str {
        NAME
    }

    fn cells(&self) -> &[CellSpec] {
        &self.cells
    }

    /// The current session's track, or the reason there is none.
    ///
    /// "Nothing is playing" is a reading like any other, and it says so: a key
    /// frozen on the last track would be a lie about what the machine is doing.
    fn sample(&mut self) -> Snapshot {
        let reading = match ss550_sensors::media::now_playing() {
            Some(playing) => Self::reading(&playing.title, &playing.artist),
            None => Reading::unavailable("nothing playing"),
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
    use super::*;

    #[test]
    fn a_track_is_its_title_and_the_artist_under_it() {
        let reading = NowPlaying::reading("  Windowlicker ", " Aphex Twin ");

        assert_eq!(reading.text(), "Windowlicker");
        assert_eq!(reading.detail(), Some("Aphex Twin"));
    }

    #[test]
    fn a_track_with_no_artist_is_the_title_alone() {
        // A podcast or a video often names no artist; the reading must not grow a
        // blank line where the artist would be.
        assert_eq!(NowPlaying::reading("A video", "  ").detail(), None);
    }

    #[test]
    fn the_cell_is_a_title_face_and_not_a_gauge() {
        // The face is the plugin's statement: words wrapped and shrunk, not a
        // number with a label and a bar.
        let cells = NowPlaying::describe();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].face(), Face::Title);
        assert_eq!(cells[0].cell().as_str(), "now");
        assert_eq!(cells[0].plugin().as_str(), "media");
    }
}
