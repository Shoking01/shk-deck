//! The contract between the controller and a plugin.
//!
//! A plugin is a piece of this build that measures something and publishes
//! **readings**: numbers a key can show. It is deliberately the smallest
//! contract that can carry that, because everything here is a promise kept to
//! every later plugin:
//!
//! - **ids**, so a binding can name a cell and be checked against it;
//! - **cells**, so the window can list what a plugin offers before it measures
//!   anything;
//! - **readings**, already formatted, so two plugins cannot disagree about what
//!   `42%` looks like.
//!
//! This crate has no I/O, no platform code and no pixels. A reading is data: how
//! it reaches a deck — the JPEG the hardware wants, the bar under the number —
//! is the window's business, in `ss550-gui`. Keeping the two apart is what lets
//! a later loader (WASM, or a native library behind its own sandbox) implement
//! the same contract without the window changing.
//!
//! ```
//! use ss550_plugin_api::{CellId, PluginId, Reading};
//!
//! let plugin = PluginId::parse("system-stats").expect("a usable id");
//! let cell = CellId::parse("cpu.usage").expect("a usable id");
//! assert_eq!(plugin.as_str(), "system-stats");
//! assert_eq!(cell.as_str(), "cpu.usage");
//!
//! let reading = Reading::percent(41.6);
//! assert_eq!(reading.text(), "42%");
//! let bar = reading.fraction().expect("a percentage has a bar");
//! assert!((bar - 0.416).abs() < 1e-6);
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use thiserror::Error;

/// The longest plugin id this build accepts.
pub const MAX_PLUGIN_ID_CHARS: usize = 32;

/// The longest cell id this build accepts.
pub const MAX_CELL_ID_CHARS: usize = 48;

/// Why a string is not a usable plugin or cell id.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdError {
    /// The id was empty, or only spaces.
    #[error("an id cannot be empty")]
    Empty,

    /// The id is longer than the contract allows.
    #[error("an id is at most {max} characters; this one is {length}")]
    TooLong {
        /// The bound.
        max: usize,
        /// What the id measured.
        length: usize,
    },

    /// The id holds a character the contract does not allow.
    #[error(
        "{character:?} is not allowed in an id: use lowercase letters, digits, '-', '_' or '.'"
    )]
    BadCharacter {
        /// The character that was refused.
        character: char,
    },

    /// The id starts or ends with a separator, or repeats one.
    #[error("an id cannot start or end with {separator:?}, or repeat it")]
    BadSeparator {
        /// The separator that was refused.
        separator: char,
    },
}

/// The identifier of one plugin, e.g. `system-stats`.
///
/// A plugin id is what a binding stores, so it is a stable, lowercase, ASCII
/// name: a name with a space or a capital in it would give one plugin two
/// spellings and two bindings that never meet.
///
/// ```
/// use ss550_plugin_api::PluginId;
///
/// assert!(PluginId::parse("system-stats").is_ok());
/// assert!(PluginId::parse("System Stats").is_err());
/// assert!(PluginId::parse("").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginId(String);

/// The identifier of one cell inside a plugin, e.g. `cpu.usage`.
///
/// A cell id is namespaced by dots, which is what makes a list of them readable
/// in the editor: `cpu.usage` and `cpu.temp` are two cells of one subject.
///
/// ```
/// use ss550_plugin_api::CellId;
///
/// assert!(CellId::parse("cpu.temp").is_ok());
/// assert!(CellId::parse("disk.free").is_ok());
/// assert!(CellId::parse("cpu..temp").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellId(String);

impl PluginId {
    /// Checks a string and keeps it as a plugin id.
    pub fn parse(text: &str) -> Result<Self, IdError> {
        parse_id(text, MAX_PLUGIN_ID_CHARS, false).map(Self)
    }

    /// The id as it is written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl CellId {
    /// Checks a string and keeps it as a cell id.
    ///
    /// A cell id may also hold `.` and `_`, because a cell is addressed by the
    /// subject it measures: `cpu.package.temp`.
    pub fn parse(text: &str) -> Result<Self, IdError> {
        parse_id(text, MAX_CELL_ID_CHARS, true).map(Self)
    }

    /// The id as it is written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Display for CellId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The one rule both ids follow.
///
/// `segments` is what a cell id adds: `.` and `_` are separators a cell may use
/// and a plugin may not, because a plugin id is a file-name-shaped name.
fn parse_id(text: &str, max: usize, segments: bool) -> Result<String, IdError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(IdError::Empty);
    }
    let length = trimmed.chars().count();
    if length > max {
        return Err(IdError::TooLong { max, length });
    }
    let separators =
        |character: char| character == '-' || (segments && (character == '.' || character == '_'));
    for (position, character) in trimmed.chars().enumerate() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            continue;
        }
        if !separators(character) {
            return Err(IdError::BadCharacter { character });
        }
        // A separator has to sit between two parts: `cpu..temp` addresses
        // nothing, and `-cpu` is not a name.
        let edge = position == 0 || position + 1 == length;
        let doubled = trimmed.chars().nth(position + 1).is_some_and(separators);
        let doubled_before =
            position > 0 && trimmed.chars().nth(position - 1).is_some_and(separators);
        if edge || doubled || doubled_before {
            return Err(IdError::BadSeparator {
                separator: character,
            });
        }
    }
    Ok(trimmed.to_string())
}

/// One thing a plugin can measure, as the window lists it.
///
/// The accent is the colour the key's bar and value are drawn in. It lives here
/// rather than in the window because a plugin knows what it measures: a
/// temperature and a disk share no palette, and the plugin is the only place
/// that can decide without the window learning every plugin's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellSpec {
    plugin: PluginId,
    cell: CellId,
    label: String,
    hint: String,
    accent: [u8; 3],
}

impl CellSpec {
    /// Builds a cell's description.
    ///
    /// The label is what a key shows above its value, so it is kept short by the
    /// plugin that states it: the window draws it at a size that fits four or
    /// five letters. `hint` is the sentence the editor lists beside it.
    pub fn new(
        plugin: PluginId,
        cell: CellId,
        label: impl Into<String>,
        hint: impl Into<String>,
        accent: [u8; 3],
    ) -> Self {
        Self {
            plugin,
            cell,
            label: label.into(),
            hint: hint.into(),
            accent,
        }
    }

    /// The plugin this cell belongs to.
    pub fn plugin(&self) -> &PluginId {
        &self.plugin
    }

    /// This cell's own id.
    pub fn cell(&self) -> &CellId {
        &self.cell
    }

    /// What a key shows above the value, e.g. `CPU`.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// What the cell measures, in a sentence.
    pub fn hint(&self) -> &str {
        &self.hint
    }

    /// The colour the value and the bar are drawn in.
    pub fn accent(&self) -> [u8; 3] {
        self.accent
    }
}

/// What one cell measured, already formatted.
///
/// Formatting happens here, once, so two plugins cannot print the same number
/// two ways and a test can hold what the deck shows. The value carries its own
/// bar: a percentage has one, a temperature does not, and a reading nothing
/// could measure says why instead of pretending.
///
/// ```
/// use ss550_plugin_api::Reading;
///
/// let load = Reading::percent(41.6).with_detail("41.6 of 100");
/// assert_eq!(load.text(), "42%");
/// assert_eq!(load.detail(), Some("41.6 of 100"));
///
/// let heat = Reading::temperature(56.4);
/// assert_eq!(heat.text(), "56°C");
/// assert_eq!(heat.fraction(), None, "a temperature has no bar");
///
/// let missing = Reading::unavailable("no sensor answered");
/// assert_eq!(missing.text(), "—");
/// assert_eq!(missing.reason(), Some("no sensor answered"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum Reading {
    /// A number was measured.
    Value {
        /// The number itself, in the unit its text states.
        ///
        /// A key draws the text, but a history graph and a threshold need the
        /// number: parsing it back out of `"59°C"` would be guessing at what the
        /// plugin already knew.
        value: f32,
        /// The number as a key shows it: `42%`, `56°C`, `12.3 GB`.
        text: String,
        /// How full the bar is, `0.0..=1.0`, when the number has a bar.
        fraction: Option<f32>,
        /// The line under the number, when there is more to say.
        detail: Option<String>,
    },
    /// Nothing could be measured, and this is why.
    Unavailable {
        /// The reason, in words a user can act on.
        why: String,
    },
}

impl Reading {
    /// A percentage, whole, with a bar that shows the same value.
    ///
    /// The number is clamped to `0..=100` before it is rounded: a provider that
    /// answers `-1` for "no idea" would otherwise draw a bar that runs
    /// backwards.
    pub fn percent(value: f32) -> Self {
        if !value.is_finite() {
            return Self::unavailable("no answer");
        }
        let clamped = value.clamp(0.0, 100.0);
        Reading::Value {
            value: clamped,
            text: format!("{clamped:.0}%"),
            fraction: Some(clamped / 100.0),
            detail: None,
        }
    }

    /// A temperature in whole degrees Celsius.
    ///
    /// There is no bar: a bar needs a scale that means something, and `56°C` on
    /// a scale of nothing tells less than the number alone. It is [`Reading::whole`]
    /// with the degree suffix, so the formatting rule lives in one place.
    pub fn temperature(celsius: f32) -> Self {
        Self::whole(celsius, "°C")
    }

    /// A reading that has a number and no bar, rounded to whole units.
    ///
    /// A percentage has a bar and a temperature has its own constructor; this is
    /// for a number with a unit neither of them states.
    pub fn whole(value: f32, suffix: &str) -> Self {
        if !value.is_finite() {
            return Self::unavailable("no answer");
        }
        Reading::Value {
            value,
            text: format!("{value:.0}{suffix}"),
            fraction: None,
            detail: None,
        }
    }

    /// A reading whose text is its own, for something that is not a number with
    /// a unit.
    ///
    /// A clock's `07` is the case: the leading zero is the reading, and
    /// `whole(7.0, "")` would show `7` — a different thing to look at. The number
    /// travels beside the text because a history graph and a threshold still want
    /// it, exactly as in [`Reading::whole`].
    ///
    /// # Example
    ///
    /// ```
    /// let minute = ss550_plugin_api::Reading::spelled("07", 7.0);
    /// assert_eq!(minute.text(), "07");
    /// assert_eq!(minute.value(), Some(7.0));
    /// ```
    pub fn spelled(text: impl Into<String>, value: f32) -> Self {
        if !value.is_finite() {
            return Self::unavailable("no answer");
        }
        Reading::Value {
            value,
            text: text.into(),
            fraction: None,
            detail: None,
        }
    }

    /// A reading nothing measured, with the reason.
    pub fn unavailable(why: impl Into<String>) -> Self {
        Reading::Unavailable { why: why.into() }
    }

    /// Adds the line under the number.
    pub fn with_detail(self, detail: impl Into<String>) -> Self {
        match self {
            Reading::Value {
                value,
                text,
                fraction,
                detail: _,
            } => Reading::Value {
                value,
                text,
                fraction,
                detail: Some(detail.into()),
            },
            other => other,
        }
    }

    /// The number as a key shows it, or `—` when nothing was measured.
    pub fn text(&self) -> &str {
        match self {
            Reading::Value { text, .. } => text,
            Reading::Unavailable { .. } => "—",
        }
    }

    /// The number this reading measured, when it measured one.
    ///
    /// A key draws [`Reading::text`]; a graph and a threshold need the number,
    /// and it is here so nothing has to parse `"59°C"` back into what the
    /// plugin already knew.
    pub fn value(&self) -> Option<f32> {
        match self {
            Reading::Value { value, .. } => Some(*value),
            Reading::Unavailable { .. } => None,
        }
    }

    /// How full the bar is, when this reading has one.
    pub fn fraction(&self) -> Option<f32> {
        match self {
            Reading::Value { fraction, .. } => *fraction,
            Reading::Unavailable { .. } => None,
        }
    }

    /// The line under the number, when there is one.
    pub fn detail(&self) -> Option<&str> {
        match self {
            Reading::Value { detail, .. } => detail.as_deref(),
            Reading::Unavailable { .. } => None,
        }
    }

    /// Why nothing was measured, when nothing was.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Reading::Unavailable { why } => Some(why),
            Reading::Value { .. } => None,
        }
    }

    /// Whether two readings would draw the same face.
    ///
    /// This is the whole of the repaint rule: the deck is sent a new JPEG only
    /// when something a person can see has changed. `detail` counts because it
    /// is drawn, and so does the reason a reading is missing.
    ///
    /// The comparison is **exact**, and deliberately so: the bar's fill is
    /// `round(span × fraction)`, so two fractions that differ can still land on
    /// different columns even when they differ by less than a pixel — a
    /// tolerance would then skip a repaint and leave a bar on the deck that a
    /// repaint would have drawn differently. Being conservative here costs one
    /// JPEG in the rare case; being tolerant costs the invariant the whole
    /// repaint rule rests on: a key that is skipped is byte for byte the key a
    /// repaint would have sent.
    pub fn same_face(&self, other: &Reading) -> bool {
        self.text() == other.text()
            && self.detail() == other.detail()
            && self.reason() == other.reason()
            && match (self.fraction(), other.fraction()) {
                (None, None) => true,
                (Some(left), Some(right)) => left == right,
                _ => false,
            }
    }
}

/// Every cell of one plugin, measured in one pass.
///
/// A plugin measures all its cells together — one walk over the machine, not one
/// per key — and the window reads whatever it needs from the answer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    cells: BTreeMap<CellId, Reading>,
}

impl Snapshot {
    /// An empty snapshot.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records what one cell measured.
    pub fn set(&mut self, cell: CellId, reading: Reading) {
        self.cells.insert(cell, reading);
    }

    /// What one cell measured, when it was measured at all.
    pub fn get(&self, cell: &CellId) -> Option<&Reading> {
        self.cells.get(cell)
    }

    /// The cells, in id order.
    pub fn iter(&self) -> impl Iterator<Item = (&CellId, &Reading)> {
        self.cells.iter()
    }
}

/// How often the sampler asks a plugin to measure.
///
/// A plugin is measured on a cadence, not by a global clock: what a machine
/// measurement costs is what decides how often it can be taken, and a reading the
/// process already has must not be held back by a provider that blocks. The
/// sampler takes the machine walk as one pass — the plugin that reads a
/// processor's temperature and the disk's free space belong together, because
/// they are the same walk — and gives a plugin that asks for its own cadence a
/// thread of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cadence {
    /// With the machine walk, at the interval the window asked for.
    Machine,
    /// On its own thread, at this interval, whatever the machine walk is doing.
    Own(Duration),
}

/// One plugin of this build.
///
/// `Send` because the sampler measures on a thread of its own: reading a
/// machine's sensors is not something a frame can wait for.
pub trait Plugin: Send {
    /// The plugin's stable name.
    fn id(&self) -> &PluginId;

    /// The name a person reads, e.g. `System stats`.
    fn name(&self) -> &str;

    /// Every cell this plugin offers, in the order the editor lists them.
    fn cells(&self) -> &[CellSpec];

    /// Measures every cell once.
    ///
    /// A provider that fails is reported as an [`Reading::Unavailable`] for the
    /// cells it owns, not as an error: one dead sensor must not stop the rest of
    /// the sample.
    fn sample(&mut self) -> Snapshot;

    /// How often this plugin wants to be measured.
    ///
    /// The default is the machine walk: a plugin that measures the machine is
    /// priced by what that costs, and pretending otherwise is how a window ends
    /// up walking the machine every second. A plugin that only reads a value the
    /// process already holds says [`Cadence::Own`] and is asked on its own, so a
    /// blocking provider cannot make its readings late.
    fn cadence(&self) -> Cadence {
        Cadence::Machine
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(text: &str) -> CellId {
        CellId::parse(text).expect("a usable cell id")
    }

    /// A plugin that offers one cell and measures nothing, for the contract's
    /// own defaults.
    struct Silent {
        id: PluginId,
        cells: Vec<CellSpec>,
    }

    impl Plugin for Silent {
        fn id(&self) -> &PluginId {
            &self.id
        }
        fn name(&self) -> &str {
            "Silent"
        }
        fn cells(&self) -> &[CellSpec] {
            &self.cells
        }
        fn sample(&mut self) -> Snapshot {
            Snapshot::new()
        }
    }

    #[test]
    fn a_plugin_that_says_nothing_is_measured_with_the_machine_walk() {
        // The default is the expensive cadence on purpose: a plugin that reads a
        // sensor must say so before the window pays for it every second, and one
        // that says nothing is priced like the walk it joins.
        let silent = Silent {
            id: PluginId::parse("silent").expect("a usable id"),
            cells: Vec::new(),
        };

        assert_eq!(silent.cadence(), Cadence::Machine);
    }

    #[test]
    fn a_reading_can_spell_its_own_text_and_keep_the_number_beside_it() {
        // A clock shows `07`, and `whole` would show `7`: the leading zero is the
        // reading, so the text is the plugin's to write.
        let minute = Reading::spelled("07", 7.0);
        assert_eq!(minute.text(), "07");
        assert_eq!(minute.value(), Some(7.0));
        assert!(
            minute.same_face(&Reading::spelled("07", 7.0)),
            "the same text is the same face"
        );
        assert!(
            !minute.same_face(&Reading::spelled("08", 8.0)),
            "a different minute is a repaint"
        );
        // A reading with no number at all is still the unavailable kind.
        assert!(matches!(
            Reading::spelled("--", f32::NAN),
            Reading::Unavailable { .. }
        ));
    }

    #[test]
    fn an_id_is_a_lowercase_name_that_survives_a_round_trip() {
        let id = PluginId::parse("  system-stats ").expect("trimmed");
        assert_eq!(id.as_str(), "system-stats");
        assert_eq!(id.to_string(), "system-stats");
        assert_eq!(cell("cpu.usage").to_string(), "cpu.usage");
    }

    #[test]
    fn an_id_with_a_capital_a_space_or_a_slash_is_refused() {
        assert!(matches!(
            PluginId::parse("System"),
            Err(IdError::BadCharacter { .. })
        ));
        assert!(matches!(
            PluginId::parse("system stats"),
            Err(IdError::BadCharacter { .. })
        ));
        assert!(matches!(
            PluginId::parse("../etc"),
            Err(IdError::BadCharacter { .. })
        ));
        assert!(matches!(PluginId::parse(""), Err(IdError::Empty)));
        assert!(matches!(PluginId::parse("   "), Err(IdError::Empty)));
    }

    #[test]
    fn a_dot_is_a_cell_separator_and_not_a_plugin_name_character() {
        assert!(CellId::parse("cpu.temp").is_ok());
        assert!(matches!(
            PluginId::parse("system.stats"),
            Err(IdError::BadCharacter { character: '.' })
        ));
    }

    #[test]
    fn a_separator_has_to_sit_between_two_parts() {
        assert!(matches!(
            CellId::parse("cpu..temp"),
            Err(IdError::BadSeparator { separator: '.' })
        ));
        assert!(matches!(
            CellId::parse(".cpu"),
            Err(IdError::BadSeparator { separator: '.' })
        ));
        assert!(matches!(
            CellId::parse("cpu-"),
            Err(IdError::BadSeparator { separator: '-' })
        ));
        // Adjacent different separators are still a separator with nothing
        // between them.
        assert!(matches!(
            CellId::parse("cpu.-temp"),
            Err(IdError::BadSeparator { .. })
        ));
    }

    #[test]
    fn an_id_past_its_bound_is_refused_and_says_the_measurement() {
        let long = "a".repeat(MAX_PLUGIN_ID_CHARS + 1);
        let error = PluginId::parse(&long).expect_err("too long");
        assert_eq!(
            error,
            IdError::TooLong {
                max: MAX_PLUGIN_ID_CHARS,
                length: MAX_PLUGIN_ID_CHARS + 1
            }
        );
        assert!(PluginId::parse(&"a".repeat(MAX_PLUGIN_ID_CHARS)).is_ok());
    }

    #[test]
    fn a_percentage_is_whole_and_clamped_both_ways() {
        assert_eq!(Reading::percent(41.6).text(), "42%");
        assert_eq!(Reading::percent(-4.0).text(), "0%");
        assert_eq!(Reading::percent(140.0).text(), "100%");
        assert_eq!(Reading::percent(-4.0).fraction(), Some(0.0));
        assert_eq!(Reading::percent(140.0).fraction(), Some(1.0));
    }

    #[test]
    fn a_temperature_has_no_bar_and_keeps_its_degrees() {
        let reading = Reading::temperature(56.4);
        assert_eq!(reading.text(), "56°C");
        assert_eq!(reading.fraction(), None);
        assert_eq!(reading.detail(), None);
        assert_eq!(Reading::whole(12.0, " GB").text(), "12 GB");
    }

    #[test]
    fn a_number_that_is_not_a_number_says_so_instead_of_drawing_nan() {
        for unusable in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(Reading::percent(unusable).text(), "—");
            assert_eq!(Reading::temperature(unusable).text(), "—");
            assert!(Reading::percent(unusable).reason().is_some());
        }
    }

    #[test]
    fn an_unavailable_reading_says_why_and_has_no_bar() {
        let reading = Reading::unavailable("no sensor answered");
        assert_eq!(reading.text(), "—");
        assert_eq!(reading.reason(), Some("no sensor answered"));
        assert_eq!(reading.fraction(), None);
        assert_eq!(reading.detail(), None);
        assert!(reading.with_detail("ignored").detail().is_none());
    }

    #[test]
    fn the_same_face_is_what_a_repaint_is_for() {
        let left = Reading::percent(42.0).with_detail("6.9 / 15.9 GB");
        let same = Reading::percent(42.0).with_detail("6.9 / 15.9 GB");
        let other_detail = Reading::percent(42.0).with_detail("7.0 / 15.9 GB");
        let other_number = Reading::percent(43.0).with_detail("6.9 / 15.9 GB");
        let unavailable = Reading::unavailable("gone");

        assert!(left.same_face(&same));
        assert!(!left.same_face(&other_detail), "the detail line is drawn");
        assert!(!left.same_face(&other_number));
        assert!(
            !left.same_face(&unavailable),
            "a reason is a face of its own"
        );
        assert!(
            !Reading::unavailable("no sensor")
                .same_face(&Reading::unavailable("thermal zone closed")),
            "a new reason is drawn, so it repaints"
        );
        assert!(Reading::temperature(56.0).same_face(&Reading::temperature(56.2)));
        assert!(!Reading::temperature(56.0).same_face(&Reading::temperature(57.0)));

        // Two percentages that print the same number but carry different
        // fractions are a different face: the bar rounds `span × fraction`, so a
        // tolerance could skip a repaint and leave a bar one column off.
        let close = Reading::percent(41.5);
        let nearer = Reading::percent(41.9);
        assert_eq!(close.text(), nearer.text());
        assert!(!close.same_face(&nearer), "the bar can move a column");
    }

    #[test]
    fn a_snapshot_answers_by_cell_in_id_order() {
        let mut snapshot = Snapshot::new();
        snapshot.set(cell("cpu.usage"), Reading::percent(12.0));
        snapshot.set(cell("cpu.temp"), Reading::unavailable("none"));

        assert_eq!(
            snapshot.get(&cell("cpu.usage")).map(Reading::text),
            Some("12%")
        );
        assert!(snapshot.get(&cell("gpu.usage")).is_none());
        assert_eq!(snapshot.iter().count(), 2);
        let order: Vec<&str> = snapshot.iter().map(|(cell, _)| cell.as_str()).collect();
        assert_eq!(order, ["cpu.temp", "cpu.usage"], "the map is ordered by id");
    }
}
