//! How a title looks when its key does not say otherwise: the default style.
//!
//! A title is drawn twice — into the deck's JPEG and over the window's tile — so
//! the style has to live somewhere both sides read. Each key may carry its own
//! style in its profile ([`ss550_engine::TitleStyle`], written through
//! [`ss550_engine::Profile::set_title_style`]); this module keeps the **default**
//! that every key without one follows, as JSON in
//! `config_dir()/title-style.json`.
//!
//! Nothing here measures or draws text: the layout runs when a payload is
//! composed, never while a frame is being painted.
//!
//! ## Why the file is read by hand
//!
//! The engine keeps its JSON behind `ss550_engine::config::read_json`, which is
//! crate-private, and it has no reader for a handful of fields. The window
//! therefore writes and reads this one file itself: a BOM strip, three quoted
//! words and a number, no parser dependency, and a missing or unreadable file
//! falls back to the default instead of failing the start. Every field falls
//! back on its own, so a file that lost one line keeps the rest — which a
//! whole-file parse would not do.
//!
//! ## Version 1 files still load
//!
//! Version 1 called the size a word (`small`, `medium`, `large`) because it was
//! a cap on a bitmap's dot scale, and chose a bitmap `face`. Both are gone: the
//! size is a number of pixels now, and there is one renderer. A version 1 file is
//! read for its `size` word, mapped to the pixels that word stood for, and its
//! `face` is ignored.

use std::path::PathBuf;

pub(crate) use ss550_engine::{TitlePosition, TitleStyle, MAX_SIZE_PX, MIN_SIZE_PX};

/// The file the default style lives in, inside the app's configuration folder.
pub(crate) const TITLE_STYLE_FILE: &str = "title-style.json";

/// What a style with no family of its own is described as.
const SYSTEM_FAMILY: &str = "system default";

/// The words, sizes and font names a [`TitleStyle`] is described with.
///
/// The type itself lives in the engine, because a profile stores it; these are
/// the window's half — strings to print and the scaling between the deck's tile
/// and the window's — so they stay here, as an extension trait rather than a
/// second copy of the type.
pub(crate) trait TitleLook {
    /// The family as the settings page prints it.
    fn family_label(&self) -> &str;

    /// The style as the settings page and the status line describe it.
    fn summary(&self) -> String;

    /// The type size the window draws a title at, on a tile `side` px tall.
    fn window_font(&self, side: f32, device_side: f32) -> f32;
}

impl TitleLook for TitleStyle {
    fn family_label(&self) -> &str {
        match self.family.is_empty() {
            true => SYSTEM_FAMILY,
            false => self.family.as_str(),
        }
    }

    fn summary(&self) -> String {
        let place = match self.position {
            TitlePosition::Top => "at the top",
            TitlePosition::Middle => "in the middle",
            TitlePosition::Bottom => "at the bottom",
        };
        let case = match self.uppercase {
            true => "upper-cased",
            false => "as typed",
        };
        format!(
            "drawn at {} px, {place}, in the {} font, {case}",
            self.size_px,
            self.family_label()
        )
    }

    /// A pure function of the style and the two tiles, so no frame measures a
    /// string: the deck's tile is `device_side` pixels and the window's is
    /// `side`, so the label scales with the window while the number the user
    /// chose keeps meaning pixels on the key. With no deck, the window's own
    /// tile is the only reference there is.
    fn window_font(&self, side: f32, device_side: f32) -> f32 {
        let device = match device_side > 0.0 {
            true => device_side,
            false => side.max(1.0),
        };
        let px = f32::from(self.size_px) * (side / device);
        px.clamp(7.0, (side * 0.42).max(7.0))
    }
}

/// Where the default style is kept.
pub(crate) fn path() -> PathBuf {
    ss550_engine::config_dir().join(TITLE_STYLE_FILE)
}

/// Reads the saved default style, or the built-in one when there is nothing
/// usable.
///
/// The second half is a sentence for the status line. A file that was never
/// written is a first run, not a fault — the default is the answer and there is
/// nothing to report. A file that exists and cannot be read is a fault: the
/// user's choice was lost, and the sentence says so.
pub(crate) fn load() -> (TitleStyle, Option<String>) {
    let path = path();
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let (style, understood) = from_json(&text);
            let note = (!understood).then(|| {
                format!(
                    "The title style in {} is not readable: using the default.",
                    path.display()
                )
            });
            (style, note)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (TitleStyle::default(), None),
        Err(error) => (
            TitleStyle::default(),
            Some(format!(
                "The title style could not be read ({error}): using the default."
            )),
        ),
    }
}

/// Writes the default style where the next start reads it.
pub(crate) fn save(style: &TitleStyle) -> Result<(), String> {
    let path = path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    std::fs::write(&path, to_json(style))
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

/// The style as the bytes of the file.
///
/// The writer cannot represent a quote or a backslash inside a string, and no
/// installed family uses one: a name that did is written as the system's own
/// font, so the file stays readable instead of losing the fields after it.
fn to_json(style: &TitleStyle) -> String {
    let family = match style.family.contains(['"', '\\']) {
        true => "",
        false => style.family.as_str(),
    };
    format!(
        "{{\n  \"version\": 2,\n  \"font\": \"{family}\",\n  \"size_px\": {},\n  \
         \"position\": \"{}\",\n  \"uppercase\": {}\n}}\n",
        style.size_px,
        position_word(style.position),
        style.uppercase
    )
}

/// Reads a style out of `text`, and says whether any field was understood.
///
/// Every field falls back on its own, so a file that lost one line keeps the
/// others. The flag is what tells [`load`] the difference between a file this
/// build wrote and something that is not a title style at all.
fn from_json(text: &str) -> (TitleStyle, bool) {
    // A BOM is legal UTF-8 and every editor that offers "UTF-8" in older
    // Windows tooling writes one. The engine's reader is crate-private, so the
    // one comparison is repeated here rather than widening its API.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut style = TitleStyle::default();
    let mut understood = false;

    // The number first: a version 1 file has no `size_px`, and its `size` word is
    // the only thing left that says how large the words were.
    if let Some(px) = number_field(text, "size_px") {
        style.size_px = px.clamp(MIN_SIZE_PX, MAX_SIZE_PX);
        understood = true;
    } else if let Some(px) = quoted_field(text, "size").and_then(legacy_size_px) {
        style.size_px = px;
        understood = true;
    }
    if let Some(family) = quoted_field(text, "font") {
        style.family = family.to_string();
        understood = true;
    }
    if let Some(value) = quoted_field(text, "position") {
        if let Some(position) = position_from_word(value) {
            style.position = position;
            understood = true;
        }
    }
    if let Some(value) = flag_field(text, "uppercase") {
        style.uppercase = value;
        understood = true;
    }

    (style, understood)
}

/// The word a position is printed as on the settings page.
pub(crate) fn position_label(position: TitlePosition) -> &'static str {
    match position {
        TitlePosition::Top => "TOP",
        TitlePosition::Middle => "MIDDLE",
        TitlePosition::Bottom => "BOTTOM",
    }
}

/// The word a position is written as, and used in a setting's id.
pub(crate) fn position_word(position: TitlePosition) -> &'static str {
    match position {
        TitlePosition::Top => "top",
        TitlePosition::Middle => "middle",
        TitlePosition::Bottom => "bottom",
    }
}

/// Reads the word back, case-insensitively.
fn position_from_word(text: &str) -> Option<TitlePosition> {
    match text.trim().to_ascii_lowercase().as_str() {
        "top" => Some(TitlePosition::Top),
        "middle" => Some(TitlePosition::Middle),
        "bottom" => Some(TitlePosition::Bottom),
        _ => None,
    }
}

/// The pixels a version 1 size word stood for.
///
/// Version 1's `small` capped a bitmap at two dots a pixel, `medium` left it to
/// the fit, and `large` gave it more of the tile. On the deck's own tile those
/// three landed near ten, fourteen and eighteen pixels of type, which is what a
/// file written by an older build keeps. A word this build does not know is not
/// a size at all, and the field falls back on its own.
fn legacy_size_px(word: &str) -> Option<u16> {
    match word.trim().to_ascii_lowercase().as_str() {
        "small" => Some(10),
        "medium" => Some(ss550_engine::DEFAULT_SIZE_PX),
        "large" => Some(18),
        _ => None,
    }
}

/// The string value of `key` in a flat JSON object, if it is there.
///
/// Deliberately not a parser: the file has a handful of fields this build wrote,
/// and anything else is a reason to fall back to the default, not to fail. It
/// cannot panic — every step is an `Option`.
fn quoted_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let after_key = key_end(text, key)?;
    let rest = text[after_key..].strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// The whole number after `key`, if there is one.
///
/// A number, not a quoted word: `size_px` is written bare, and a value with a
/// decimal point or an exponent in it is not one this build wrote.
fn number_field(text: &str, key: &str) -> Option<u16> {
    let after_key = key_end(text, key)?;
    let digits: String = text[after_key..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// The `true` or `false` after `key`, if there is one.
fn flag_field(text: &str, key: &str) -> Option<bool> {
    let after_key = key_end(text, key)?;
    let rest = text[after_key..].trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Where the value of `key` starts: past the quoted name and its colon.
fn key_end(text: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    let after_key = text.find(&needle)? + needle.len();
    let rest = text[after_key..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    Some(text.len() - rest.len())
}

#[cfg(test)]
mod tests {
    use super::{from_json, legacy_size_px, path, to_json, TitleLook, TITLE_STYLE_FILE};
    use ss550_engine::{TitlePosition, TitleStyle, DEFAULT_SIZE_PX, MAX_SIZE_PX, MIN_SIZE_PX};

    #[test]
    fn the_built_in_default_is_a_readable_caption_with_no_font_of_its_own() {
        let style = TitleStyle::default();
        assert_eq!(style.size_px, DEFAULT_SIZE_PX);
        assert!(style.family.is_empty(), "the system's sans, not a name");
        assert_eq!(style.family_label(), "system default");
        assert_eq!(style.position, TitlePosition::Bottom);
        assert!(!style.uppercase);
    }

    #[test]
    fn a_written_style_reads_back_field_for_field() {
        let style = TitleStyle {
            family: "Segoe UI".to_string(),
            size_px: 22,
            position: TitlePosition::Top,
            uppercase: true,
        };
        let (read, understood) = from_json(&to_json(&style));
        assert!(understood);
        assert_eq!(read, style);
    }

    #[test]
    fn a_bom_and_unknown_fields_do_not_hide_the_style() {
        let text = "\u{feff}{ \"version\": 2, \"font\": \"Georgia\", \"size_px\": 9, \
                    \"position\": \"top\", \"extra\": \"ignored\" }";
        let (style, understood) = from_json(text);
        assert!(understood);
        assert_eq!(style.family, "Georgia");
        assert_eq!(style.size_px, 9);
        assert_eq!(style.position, TitlePosition::Top);
    }

    #[test]
    fn one_lost_field_keeps_the_other_three() {
        let text = r#"{ "font": "Georgia", "size_px": 20, "uppercase": true }"#;
        let (style, understood) = from_json(text);
        assert!(understood);
        assert_eq!(style.family, "Georgia");
        assert_eq!(style.size_px, 20);
        assert!(style.uppercase);
        assert_eq!(
            style.position,
            TitlePosition::Bottom,
            "the default fills in"
        );
    }

    #[test]
    fn a_file_that_is_not_a_title_style_is_the_default_and_nothing_understood() {
        for text in ["", "not json", "{}", "{\"size\": \"enormous\"}"] {
            let (style, understood) = from_json(text);
            assert!(!understood, "{text:?} should not read as a style");
            assert_eq!(style, TitleStyle::default(), "{text:?}");
        }
    }

    #[test]
    fn a_version_one_file_still_names_a_size() {
        // The file an older build wrote: a word for the size and a bitmap face.
        // The face is gone, and the word still says how large the type was.
        let text = r#"{ "version": 1, "size": "large", "position": "top", "face": "compact" }"#;
        let (style, understood) = from_json(text);
        assert!(understood);
        assert_eq!(style.size_px, 18, "large stood for eighteen pixels");
        assert_eq!(style.position, TitlePosition::Top);
        assert!(style.family.is_empty(), "the old file named no font");
    }

    #[test]
    fn the_three_old_size_words_land_on_three_different_pixels() {
        assert_eq!(legacy_size_px("small"), Some(10));
        assert_eq!(legacy_size_px("medium"), Some(DEFAULT_SIZE_PX));
        assert_eq!(legacy_size_px("large"), Some(18));
        assert_eq!(legacy_size_px("enormous"), None, "not a version 1 size");
    }

    #[test]
    fn a_number_outside_the_range_is_clamped_by_the_reader() {
        let (huge, _) = from_json(r#"{ "size_px": 400 }"#);
        assert_eq!(huge.size_px, MAX_SIZE_PX);
        let (tiny, _) = from_json(r#"{ "size_px": 1 }"#);
        assert_eq!(tiny.size_px, MIN_SIZE_PX);
    }

    #[test]
    fn the_window_labels_carry_the_pixel_size_the_deck_was_given() {
        let style = TitleStyle {
            size_px: 16,
            ..TitleStyle::default()
        };
        // A window tile the same size as the deck's: the same number.
        assert!((style.window_font(108.0, 108.0) - 16.0).abs() < 0.01);
        // A window tile twice the deck's: the label scales with the tile.
        assert!((style.window_font(144.0, 72.0) - 32.0).abs() < 0.01);
        // With no deck there is no other reference, and the label is the number.
        assert!((style.window_font(72.0, 0.0) - 16.0).abs() < 0.01);
        // A tile too small to hold the type draws a legible label, not a speck.
        assert!(style.window_font(1.0, 108.0) >= 7.0);
    }

    #[test]
    fn the_summary_reads_as_a_sentence() {
        let style = TitleStyle {
            family: "Georgia".to_string(),
            size_px: 12,
            position: TitlePosition::Top,
            uppercase: true,
        };
        assert_eq!(
            style.summary(),
            "drawn at 12 px, at the top, in the Georgia font, upper-cased"
        );
    }

    #[test]
    fn the_file_has_a_name_of_its_own_in_the_configuration_folder() {
        assert_eq!(TITLE_STYLE_FILE, "title-style.json");
        assert!(path().ends_with(TITLE_STYLE_FILE));
    }
}
