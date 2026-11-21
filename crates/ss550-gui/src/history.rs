//! What each measurement did over the last few minutes.
//!
//! The sampler hands the window one walk over the machine every second, and
//! a number that only ever shows "now" cannot answer the question a person
//! actually asks — *is it rising?* — so the window keeps the last few minutes of
//! each cell.
//!
//! The model is deliberately small and pure: a ring of numbers per cell, the
//! shape of a series, and nothing about pixels. The page that draws it is
//! `ui::stats`, and it can be tested without a window because the rules are
//! here. What counts as *bad* is not decided here: those lines belong to the
//! user and live in `thresholds`.

use std::collections::BTreeMap;

use ss550_plugin_api::{CellId, PluginId, Reading};

/// How many samples one cell keeps.
///
/// At the machine walk's two-second interval this is four minutes: long enough to
/// see a compile start and finish, short enough that a value from another session
/// never influences the graph. The cadence and this number are one decision —
/// `the_graph_still_holds_four_minutes` holds them together.
pub(crate) const DEPTH: usize = 120;

/// How bad a reading is, in the only three states a threshold needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Severity {
    /// Nothing to see.
    Quiet,
    /// Worth a second look: above the warning line.
    Warn,
    /// At or past the limit.
    Alarm,
}

/// The history of one cell: the numbers it reported, oldest first.
#[derive(Debug, Default, Clone)]
pub(crate) struct History {
    cells: BTreeMap<(PluginId, CellId), Vec<f32>>,
}

impl History {
    /// An empty history.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Records one walk, ignoring cells the sample did not carry.
    ///
    /// A cell that answered nothing is not recorded as zero: a gap is a gap, and
    /// a graph that drew a dash as the bottom of the scale would invent a
    /// reading that never happened.
    pub(crate) fn record(&mut self, plugin: &PluginId, cell: &CellId, reading: &Reading) {
        let Some(value) = reading.value() else {
            return;
        };
        let series = self
            .cells
            .entry((plugin.clone(), cell.clone()))
            .or_default();
        series.push(value);
        if series.len() > DEPTH {
            let excess = series.len() - DEPTH;
            series.drain(..excess);
        }
    }

    /// The numbers one cell reported, oldest first.
    pub(crate) fn series(&self, plugin: &PluginId, cell: &CellId) -> &[f32] {
        self.cells
            .get(&(plugin.clone(), cell.clone()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

/// The lowest and highest of a series, with a floor under a flat one.
///
/// The scale of a graph is not always its own range: a share is drawn 0–100 so a
/// processor between 41 and 43 % looks calm instead of like a crisis. This is the
/// range for everything else. A series that never moved is **centred** in a
/// one-degree window rather than pushed to the bottom, so a steady 62 °C reads as
/// a level line and not as a row of dots on the axis.
pub(crate) fn span_of(series: &[f32]) -> (f32, f32) {
    let low = series.iter().copied().fold(f32::INFINITY, f32::min);
    let high = series.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if !low.is_finite() || !high.is_finite() {
        return (0.0, 1.0);
    }
    if (high - low).abs() < 0.5 {
        return (low - 0.5, low + 0.5);
    }
    (low, high)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> (PluginId, CellId) {
        (
            PluginId::parse("system-stats").expect("a usable id"),
            CellId::parse("cpu.usage").expect("a usable id"),
        )
    }

    #[test]
    fn the_graph_still_holds_four_minutes() {
        // The depth and the sampler's cadence are one decision: the Stats page
        // says "the last four minutes", so a sampler that walks faster must not
        // quietly shorten the graph it draws.
        let seconds = DEPTH as u32 * ss550_plugins::SAMPLE_INTERVAL.as_secs() as u32;

        assert_eq!(
            seconds,
            4 * 60,
            "the sparkline no longer holds four minutes"
        );
    }

    #[test]
    fn a_cell_keeps_its_numbers_oldest_first_and_forgets_the_oldest() {
        let (plugin, cell) = ids();
        let mut history = History::new();
        for value in 0..(DEPTH as i32 + 10) {
            history.record(&plugin, &cell, &Reading::percent(value as f32));
        }
        let series = history.series(&plugin, &cell);
        assert_eq!(series.len(), DEPTH, "the ring is bounded");
        assert_eq!(series[series.len() - 1], (DEPTH as f32 + 9.0).min(100.0));
        assert_eq!(
            series[0], 10.0,
            "the ten oldest samples were dropped, not the newest"
        );
    }

    #[test]
    fn a_reading_that_measured_nothing_is_a_gap_and_not_a_zero() {
        let (plugin, cell) = ids();
        let mut history = History::new();
        history.record(&plugin, &cell, &Reading::unavailable("no sensor"));
        assert!(history.series(&plugin, &cell).is_empty());
    }

    #[test]
    fn the_span_of_a_series_is_its_own_range_and_a_flat_one_still_has_height() {
        let (plugin, cell) = ids();
        let mut history = History::new();
        for value in [40.0, 44.0, 41.0] {
            history.record(&plugin, &cell, &Reading::temperature(value));
        }
        assert_eq!(span_of(history.series(&plugin, &cell)), (40.0, 44.0));

        let mut flat = History::new();
        flat.record(&plugin, &cell, &Reading::temperature(55.0));
        let (low, high) = span_of(flat.series(&plugin, &cell));
        assert_eq!(
            (low, high),
            (54.5, 55.5),
            "a steady value is centred, not at the floor"
        );
    }

    #[test]
    fn a_series_that_was_never_recorded_has_an_empty_span_and_not_a_panic() {
        let (plugin, cell) = ids();
        let history = History::new();
        assert_eq!(span_of(history.series(&plugin, &cell)), (0.0, 1.0));
        assert!(history.series(&plugin, &cell).is_empty());
    }
}
