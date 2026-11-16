//! The picture a key shows for the thing it runs.
//!
//! A key bound to a program draws that program's own icon, read out of the file
//! by [`ss550_tray::appicon`]. A program whose icon cannot be read, a URL and a
//! keystroke fall back to the initial of what they run, drawn on the accent
//! colour: a key that runs something is never a blank tile.
//!
//! ## When the work happens
//!
//! [`IconCache::prepare`] is the only place that extracts, decodes or encodes
//! anything. It runs when the action, the profile or the deck changes, and it
//! keeps what it built; [`IconCache::frame`] and [`IconCache::tile`] are plain
//! lookups over that map, so a frame that draws the grid never asks Windows for
//! anything and never touches an encoder.
//!
//! ## Why the deck gets the same picture
//!
//! The deck's tile and the window's tile are the same artwork fitted to the same
//! image spec — [`crate::render`] builds both from one entry point — because a
//! key that shows one thing and sends another is a bug the user cannot see.
//!
//! ## Example
//!
//! ```text
//! let mut icons = IconCache::new();
//! icons.prepare(Some(spec), &actions);
//! let face = icons.artwork(&action);
//! let picture = icons.frame(&action);
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use deck_api::ImageSpec;
use gpui_kit::RenderImage;
use image::{Rgba, RgbaImage};
use ss550_engine::Action;

use crate::render;

/// The side of a glyph when there is no deck to state an image size.
const GLYPH_SIDE: u32 = 32;

/// The largest square a glyph is ever drawn on.
///
/// A deck states its own image size, and a glyph is drawn at exactly that size
/// so the initial is crisp instead of a scaled-up thumbnail. The ceiling is
/// there for a deck that states something absurd: a key image is a thumbnail,
/// and a megabyte of accent colour helps nobody.
const GLYPH_MAX_SIDE: u32 = 512;

/// The accent a glyph is drawn on, as the image encoder sees it.
///
/// It mirrors the window's own accent, because the tile and the deck show the
/// same picture; the palette itself lives in [`crate::ui`], and the test there
/// pins the two together.
pub(crate) const GLYPH_ACCENT: [u8; 3] = [0x7c, 0x6c, 0xf0];

/// The ink a glyph's initial is drawn in.
const GLYPH_INK: [u8; 4] = [0xff, 0xff, 0xff, 0xff];

/// What a key shows for the thing it runs, when the profile says nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Artwork {
    /// The key runs nothing, so it has nothing of its own to show.
    None,
    /// The program's own icon, extracted from the file.
    Icon,
    /// The initial of what the key runs, on the accent colour.
    Glyph,
}

/// One picture, as the tile draws it and as the deck receives it.
struct Art {
    /// The fitted frame the window paints.
    frame: Arc<RenderImage>,
    /// The encoded payload, once a deck has stated an image size.
    tile: Option<Vec<u8>>,
}

/// Every picture the deck's bindings need, built once per change.
pub(crate) struct IconCache {
    /// The image size the pictures were built for. A different deck is a
    /// different tile, and nothing built for the old one can be reused.
    spec: Option<ImageSpec>,
    /// One entry per program path. A `None` remembers a program with no
    /// readable icon, so a rebind does not ask Windows for it again.
    icons: BTreeMap<PathBuf, Option<Art>>,
    /// One entry per initial, so two keys that start with the same letter share
    /// a picture instead of encoding it twice.
    glyphs: BTreeMap<char, Art>,
}

impl IconCache {
    /// An empty cache: nothing is extracted until a deck says what it needs.
    pub(crate) fn new() -> Self {
        Self {
            spec: None,
            icons: BTreeMap::new(),
            glyphs: BTreeMap::new(),
        }
    }

    /// Builds every picture `actions` needs, and forgets the rest.
    ///
    /// This is the whole of the work: one icon extraction per program path and
    /// one glyph per distinct initial. Called on a rebind, a profile switch, a
    /// library refresh and a deck change — never from a frame.
    ///
    /// `draft` is the program the editor is about to bind. It is not a binding
    /// yet, but the key's face is decided by it, so its icon is read here — once,
    /// on the change that made it the draft — and the editor draws it before
    /// SAVE BINDING is pressed. It is kept in the cache for the same reason the
    /// bound ones are: the tile and the deck will want exactly this picture.
    pub(crate) fn prepare(
        &mut self,
        spec: Option<ImageSpec>,
        actions: &[Action],
        draft: Option<&Path>,
    ) {
        if spec != self.spec {
            // Every fitted frame and every encoded payload was built for the
            // old image size, so none of them can be served to this deck.
            self.icons.clear();
            self.glyphs.clear();
            self.spec = spec;
        }

        let mut programs: Vec<PathBuf> = Vec::new();
        let mut letters: Vec<char> = Vec::new();
        for action in actions {
            if let Action::Launch { program, .. } = action {
                if !programs.contains(program) {
                    programs.push(program.clone());
                }
            }
            let letter = glyph_letter(action);
            if !letters.contains(&letter) {
                letters.push(letter);
            }
        }
        // An empty field is not a program: asking the shell about "" would cache
        // a miss that nothing can ever use.
        if let Some(program) = draft {
            if !program.as_os_str().is_empty() && !programs.iter().any(|known| known == program) {
                programs.push(program.to_path_buf());
            }
        }

        // A binding that was removed takes its picture with it: the cache is
        // the set of things the deck can run now, not a history of them.
        self.icons.retain(|path, _| programs.contains(path));
        self.glyphs.retain(|letter, _| letters.contains(letter));

        for program in programs {
            if self.icons.contains_key(&program) {
                continue;
            }
            let art = build_icon(self.spec, &program);
            self.icons.insert(program, art);
        }
        for letter in letters {
            if self.glyphs.contains_key(&letter) {
                continue;
            }
            let art = build_glyph(self.spec, letter);
            self.glyphs.insert(letter, art);
        }
    }

    /// What the tile should draw for `action`, when the profile says nothing.
    ///
    /// `None` is a key with no binding: it has nothing of its own to show and
    /// the grid falls back to its flat colour.
    pub(crate) fn artwork(&self, action: Option<&Action>) -> Artwork {
        match action {
            None => Artwork::None,
            Some(Action::Launch { program, .. }) => match self.icons.get(program) {
                Some(Some(_)) => Artwork::Icon,
                _ => Artwork::Glyph,
            },
            Some(Action::OpenUrl { .. } | Action::Keys { .. }) => Artwork::Glyph,
        }
    }

    /// The picture the tile paints for `action`, when it has one.
    pub(crate) fn frame(&self, action: &Action) -> Option<&Arc<RenderImage>> {
        self.art(action).map(|art| &art.frame)
    }

    /// The picture a program would give a key, whether or not it is bound yet.
    ///
    /// The editor asks this for the path in its field, so the user sees the
    /// icon the key is about to wear before pressing SAVE BINDING. `None` means
    /// the program has no icon to read, and the editor draws the initial
    /// instead — the same fallback the tile uses.
    pub(crate) fn program_frame(&self, program: &Path) -> Option<&Arc<RenderImage>> {
        self.icons
            .get(program)
            .and_then(Option::as_ref)
            .map(|art| &art.frame)
    }

    /// The payload the deck receives for `action`, once it stated a size.
    pub(crate) fn tile(&self, action: &Action) -> Option<&[u8]> {
        self.art(action).and_then(|art| art.tile.as_deref())
    }

    /// The picture one action has: its program's icon first, its initial second.
    ///
    /// A program whose icon could not be read falls through to its initial, so a
    /// key that runs something always has a picture — and the deck receives the
    /// one the tile draws.
    fn art(&self, action: &Action) -> Option<&Art> {
        let letter = glyph_letter(action);
        match action {
            Action::Launch { program, .. } => self
                .icons
                .get(program)
                .and_then(Option::as_ref)
                .or_else(|| self.glyphs.get(&letter)),
            Action::OpenUrl { .. } | Action::Keys { .. } => self.glyphs.get(&letter),
        }
    }
}

/// The letter a key shows when there is no icon to show.
///
/// The first letter of what the key runs: the program's file name, the URL's
/// host, or the key a combination presses. Upper-cased, because a tile reads as
/// a label and not as prose. Anything with no letter at all — a path that is
/// only separators, a punctuation key — answers `#`, so the tile is never
/// empty.
pub(crate) fn glyph_letter(action: &Action) -> char {
    match action {
        Action::Launch { program, .. } => {
            // The text is borrowed only for the length of this arm: the answer
            // is one character, not a slice of a path that may be owned.
            let text = program.to_string_lossy();
            first_letter(last_segment(&text))
        }
        Action::OpenUrl { url } => first_letter(host_of(url)),
        // `BaseKey`'s own spelling is what the tile shows: `M`, `7`, `F24`,
        // `Enter`. The initial of that is the initial of the key.
        Action::Keys { keys } => first_letter(&keys.base_key().to_string()),
    }
}

/// The first letter of `text`, upper-cased, or `#` when it has none.
fn first_letter(text: &str) -> char {
    text.chars()
        .find(|character| character.is_alphanumeric())
        .map(|character| character.to_uppercase().next().unwrap_or('#'))
        .unwrap_or('#')
}

/// The last segment of a stored path, whichever separator it uses.
///
/// [`Path::file_name`] splits on the host's separator only, so a Windows path
/// kept in a bindings file would read as one long segment on another platform.
fn last_segment(text: &str) -> &str {
    match text.rsplit(['\\', '/']).find(|segment| !segment.is_empty()) {
        Some(segment) => segment,
        None => text,
    }
}

/// The host part of a URL, for the initial.
fn host_of(url: &str) -> &str {
    // The scheme is never the subject: `mailto:` and `https:` name a way to
    // reach something, not the something.
    let without_scheme = match url.find(':') {
        Some(index) => &url[index + 1..],
        None => url,
    };
    let authority = without_scheme.strip_prefix("//").unwrap_or(without_scheme);
    authority.split(['/', '?', '#']).next().unwrap_or(authority)
}

/// One picture, built from pixels the deck's size can be applied to.
fn art(spec: Option<ImageSpec>, picture: &RgbaImage, name: &str) -> Art {
    let frame = match spec.and_then(|spec| render::preview_frame_rgba(spec, picture).ok()) {
        Some(frame) => frame,
        // No deck, or a deck that asked for no pixels: the window still shows
        // the picture, at the size it was extracted.
        None => render::frame_from_rgba(picture),
    };
    let tile = spec.and_then(|spec| render::render_rgba(spec, name, picture).ok());
    Art {
        frame: Arc::new(RenderImage::new([frame])),
        tile,
    }
}

/// The icon of one program, or `None` when the file has none to read.
fn build_icon(spec: Option<ImageSpec>, program: &Path) -> Option<Art> {
    let pixels = ss550_tray::appicon::program_icon(program)?;
    if pixels.is_blank() {
        return None;
    }
    let icon = RgbaImage::from_raw(pixels.width(), pixels.height(), pixels.rgba().to_vec())?;
    // An icon is 32 pixels square and a tile is 108: letting the fit resample it
    // is what turns a crisp app icon into a smear. It is composed once at whole
    // pixels instead — the largest integer scale that leaves a margin — and the
    // fit then has nothing left to scale.
    let side = spec.map_or(icon.width().max(icon.height()), |spec| {
        u32::from(spec.width().max(spec.height())).max(1)
    });
    let picture = compose_icon(&icon, side);
    // The path, not the file name: a refusal names what the user can go and
    // look at, and two programs can share a file name.
    let name = program.to_string_lossy();
    Some(art(spec, &picture, &name))
}

/// Centres `icon` on a square of `side` pixels, scaled by whole pixels only.
///
/// Nearest-neighbour on purpose: a 32-pixel icon drawn at 2× is the icon the
/// user recognises, while a smoothed 3.4× is a blurred version of it. The margin
/// keeps the icon off the tile's edge, the way a key in a finished controller
/// looks.
fn compose_icon(icon: &RgbaImage, side: u32) -> RgbaImage {
    let side = side.max(1);
    let mut canvas = RgbaImage::from_pixel(side, side, Rgba([0, 0, 0, 0xff]));
    let (width, height) = icon.dimensions();
    if width == 0 || height == 0 {
        return canvas;
    }

    // 72 % of the tile is the icon, the rest is margin; the scale is the largest
    // whole number that still fits inside that budget.
    let budget = (side as f32 * 0.72).round().max(1.0) as u32;
    let scale = (budget / width.max(height)).max(1);
    let drawn_width = (width * scale).min(side);
    let drawn_height = (height * scale).min(side);
    let left = (side - drawn_width) / 2;
    let top = (side - drawn_height) / 2;

    for y in 0..drawn_height {
        for x in 0..drawn_width {
            let source = icon.get_pixel(x / scale, y / scale);
            // An icon's transparent pixels stay the canvas's black: a JPEG has
            // no alpha, and the deck flattens onto black anyway.
            if source[3] < 128 {
                continue;
            }
            canvas.put_pixel(left + x, top + y, *source);
        }
    }
    canvas
}

/// The initial of what a key runs, drawn on the accent colour.
fn build_glyph(spec: Option<ImageSpec>, letter: char) -> Art {
    let side = spec
        .map_or(GLYPH_SIDE, |spec| {
            u32::from(spec.width().max(spec.height())).max(1)
        })
        .min(GLYPH_MAX_SIDE);
    let picture = glyph_image(letter, side);
    art(spec, &picture, "glyph")
}

/// One initial, centred on a square of accent colour.
fn glyph_image(letter: char, side: u32) -> RgbaImage {
    let accent = Rgba([GLYPH_ACCENT[0], GLYPH_ACCENT[1], GLYPH_ACCENT[2], 0xff]);
    let mut picture = RgbaImage::from_pixel(side, side, accent);

    // The bitmap is five dots wide and seven tall, so the scale follows the
    // canvas: the initial fills the same share of a small tile and a large one.
    let scale = ((side as f32) / 12.0).round().max(1.0) as u32;
    let scale = scale.min((side / 5).max(1));
    let glyph_width = 5 * scale;
    let glyph_height = 7 * scale;
    let left = side.saturating_sub(glyph_width) / 2;
    let top = side.saturating_sub(glyph_height) / 2;

    for (row, bits) in glyph_rows(letter).iter().enumerate() {
        for column in 0..5 {
            if (*bits >> (4 - column)) & 1 == 0 {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    let x = left + column * scale + dx;
                    let y = top + (row as u32) * scale + dy;
                    if x < side && y < side {
                        picture.put_pixel(x, y, Rgba(GLYPH_INK));
                    }
                }
            }
        }
    }
    picture
}

/// The seven rows of one initial, five dots wide, top row first.
///
/// A bitmap table and not a font: the deck shows a key's initial at a few dots
/// across, and a text rasteriser would be a whole dependency for seven rows of
/// dots. Every letter and digit is here; anything else draws `#`.
fn glyph_rows(letter: char) -> [u8; 7] {
    let letter = letter.to_ascii_uppercase();
    for (known, rows) in GLYPHS {
        if *known == letter {
            return *rows;
        }
    }
    GLYPH_FALLBACK
}

/// What an initial with no letter of its own draws.
const GLYPH_FALLBACK: [u8; 7] = [0x0A, 0x0A, 0x1F, 0x0A, 0x1F, 0x0A, 0x0A];

/// A five-by-seven bitmap per letter and digit, most significant bit leftmost.
const GLYPHS: &[(char, [u8; 7])] = &[
    ('A', [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]),
    ('B', [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E]),
    ('C', [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E]),
    ('D', [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E]),
    ('E', [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F]),
    ('F', [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10]),
    ('G', [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F]),
    ('H', [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]),
    ('I', [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E]),
    ('J', [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C]),
    ('K', [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11]),
    ('L', [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F]),
    ('M', [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11]),
    ('N', [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11]),
    ('O', [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
    ('P', [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10]),
    ('Q', [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D]),
    ('R', [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11]),
    ('S', [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E]),
    ('T', [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04]),
    ('U', [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
    ('V', [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04]),
    ('W', [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11]),
    ('X', [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11]),
    ('Y', [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04]),
    ('Z', [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F]),
    ('0', [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E]),
    ('1', [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E]),
    ('2', [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F]),
    ('3', [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E]),
    ('4', [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02]),
    ('5', [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E]),
    ('6', [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E]),
    ('7', [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08]),
    ('8', [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E]),
    ('9', [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C]),
    ('#', GLYPH_FALLBACK),
];

#[cfg(test)]
mod tests {
    use super::{
        compose_icon, first_letter, glyph_image, glyph_letter, glyph_rows, host_of, last_segment,
        Artwork, GLYPHS, GLYPH_ACCENT,
    };
    use ss550_engine::Action;
    use std::path::PathBuf;

    fn launch(program: &str) -> Action {
        Action::Launch {
            program: PathBuf::from(program),
            args: Vec::new(),
        }
    }

    #[test]
    fn a_program_shows_the_first_letter_of_its_file_name() {
        assert_eq!(
            glyph_letter(&launch(r"C:\Program Files\obs-studio\obs64.exe")),
            'O'
        );
        assert_eq!(glyph_letter(&launch("C:/Apps/brave.exe")), 'B');
        // The file name decides, not the folder: every program lives in a
        // folder, and the folder is not what the key runs.
        assert_eq!(glyph_letter(&launch(r"C:\Apps\zoom.exe")), 'Z');
    }

    #[test]
    fn a_url_shows_the_first_letter_of_its_host() {
        let action = Action::OpenUrl {
            url: "https://twitch.tv/dashboard".to_string(),
        };
        assert_eq!(glyph_letter(&action), 'T');

        let mail = Action::OpenUrl {
            url: "mailto:someone@example.com".to_string(),
        };
        assert_eq!(glyph_letter(&mail), 'S', "the scheme is not the subject");
    }

    #[test]
    fn a_keystroke_shows_the_key_it_presses() {
        let combo = Action::keys("ctrl+shift+m").expect("a valid combination");
        assert_eq!(glyph_letter(&combo), 'M');

        let function = Action::keys("f24").expect("a valid combination");
        assert_eq!(glyph_letter(&function), 'F');

        let digit = Action::keys("ctrl+7").expect("a valid combination");
        assert_eq!(glyph_letter(&digit), '7');
    }

    #[test]
    fn a_name_with_no_letter_falls_back_to_a_mark() {
        // Nothing here is a letter, and a tile with no glyph at all would be a
        // hole in the grid.
        assert_eq!(first_letter(""), '#');
        assert_eq!(first_letter("\\\\"), '#');
        assert_eq!(glyph_letter(&launch(r"\\")), '#');
        // A punctuation key still has a name in the engine's vocabulary.
        let comma = Action::keys(",").expect("a valid combination");
        assert_eq!(glyph_letter(&comma), '#');
    }

    #[test]
    fn a_path_is_read_by_its_last_segment_whatever_the_separator() {
        for path in [
            r"C:\Apps\brave.exe",
            "C:/Apps/brave.exe",
            "/usr/local/bin/brave.exe",
            "brave.exe",
        ] {
            assert_eq!(last_segment(path), "brave.exe", "for {path}");
        }
        assert_eq!(host_of("https://twitch.tv/x?y=1"), "twitch.tv");
    }

    #[test]
    fn every_glyph_is_a_five_by_seven_bitmap() {
        // A row with a bit above the fifth column would be clipped, and a row
        // that is entirely empty would leave a hole in the letter.
        for (letter, rows) in GLYPHS {
            for row in rows {
                assert_eq!(
                    *row & 0b1110_0000,
                    0,
                    "{letter} has a bit outside its width"
                );
            }
            assert!(
                rows.iter().any(|row| *row != 0),
                "{letter} is an empty glyph"
            );
        }
        // The table is what a key can show: every letter and every digit.
        for letter in 'A'..='Z' {
            assert!(
                GLYPHS.iter().any(|(known, _)| *known == letter),
                "{letter} has no glyph"
            );
        }
        for digit in '0'..='9' {
            assert!(
                GLYPHS.iter().any(|(known, _)| *known == digit),
                "{digit} has no glyph"
            );
        }
    }

    #[test]
    fn a_glyph_is_drawn_on_the_accent_and_centred() {
        let side = 32;
        let picture = glyph_image('M', side);

        assert_eq!(picture.dimensions(), (side, side));
        // The corners are the accent the deck receives.
        let corner = picture.get_pixel(0, 0).0;
        assert_eq!([corner[0], corner[1], corner[2]], GLYPH_ACCENT);
        assert_eq!(corner[3], 0xFF, "the accent is opaque");

        // The letter itself is white, and it sits in the middle: the centre of
        // the canvas is a lit dot of the M.
        let centre = picture.get_pixel(side / 2, side / 2).0;
        assert_eq!(centre, [0xFF, 0xFF, 0xFF, 0xFF]);

        // Nothing was drawn outside the bitmap's own square.
        let mut lit = 0;
        for (_, _, pixel) in picture.enumerate_pixels() {
            if pixel[0] == 0xFF && pixel[1] == 0xFF && pixel[2] == 0xFF {
                lit += 1;
            }
        }
        assert!(lit > 0, "the letter must reach the canvas");
        assert!(
            lit < (side * side) / 2,
            "a glyph is a letter, not a wash of white: {lit} lit dots"
        );
    }

    #[test]
    fn a_glyph_keeps_its_shape_on_a_decks_own_tile() {
        // The deck states a larger tile; the initial is drawn at that size so
        // it is not a scaled-up thumbnail.
        let picture = glyph_image('A', 96);
        assert_eq!(picture.dimensions(), (96, 96));
        let corner = picture.get_pixel(0, 0).0;
        assert_eq!([corner[0], corner[1], corner[2]], GLYPH_ACCENT);
    }

    #[test]
    fn a_glyph_with_no_letter_of_its_own_still_draws_something() {
        assert_eq!(glyph_rows('?'), glyph_rows('#'));
        // Lower case is the same letter: the tile reads as a label.
        assert_eq!(glyph_rows('m'), glyph_rows('M'));
    }

    #[test]
    fn a_program_with_no_icon_to_read_is_a_glyph_and_never_a_blank() {
        // The cache is asked about a program that was never prepared: the
        // answer is the initial, which is a picture, not a hole.
        let cache = super::IconCache::new();
        let action = launch(r"C:\this\program\was\never\installed.exe");

        assert_eq!(cache.artwork(Some(&action)), Artwork::Glyph);
        assert!(cache.frame(&action).is_none());
        assert!(cache.tile(&action).is_none());
    }

    #[test]
    fn an_icon_is_drawn_at_whole_pixels_with_a_margin() {
        // A 2x2 icon on a 10-pixel tile: 72 % of ten is seven, so the largest
        // whole scale is three, the icon covers 6x6 and sits two pixels in.
        let mut icon = image::RgbaImage::new(2, 2);
        for (x, y, pixel) in icon.enumerate_pixels_mut() {
            *pixel = image::Rgba([(x as u8 + 1) * 10, (y as u8 + 1) * 10, 0, 0xff]);
        }

        let canvas = compose_icon(&icon, 10);

        assert_eq!(canvas.dimensions(), (10, 10));
        assert_eq!(
            canvas.get_pixel(0, 0).0,
            [0, 0, 0, 0xff],
            "the margin is black"
        );
        // The top-left icon pixel is a solid 3x3 block, not a blend.
        for y in 2..5 {
            for x in 2..5 {
                assert_eq!(canvas.get_pixel(x, y).0, [10, 10, 0, 0xff], "at ({x},{y})");
            }
        }
        assert_eq!(
            canvas.get_pixel(5, 2).0,
            [20, 10, 0, 0xff],
            "the next block"
        );
        assert_eq!(
            canvas.get_pixel(8, 8).0,
            [0, 0, 0, 0xff],
            "and the far margin"
        );
    }

    #[test]
    fn a_prepared_program_with_no_icon_falls_through_to_its_initial() {
        // A path that is not there has no icon, so the initial is what the key
        // shows — and it is a real picture, at the deck's own size, for both the
        // tile and the deck.
        let spec = deck_api::ImageSpec::new(72, 72, 4_096, deck_api::ImageFormat::Jpeg);
        let action = launch(r"C:\this\program\was\never\installed.exe");
        let mut cache = super::IconCache::new();
        cache.prepare(Some(spec), std::slice::from_ref(&action), None);

        assert_eq!(cache.artwork(Some(&action)), Artwork::Glyph);
        assert!(cache.frame(&action).is_some(), "the initial is a picture");
        let tile = cache.tile(&action).expect("the deck receives the initial");
        assert!(tile.len() <= spec.max_bytes(), "{} bytes", tile.len());
    }

    #[test]
    fn a_binding_that_is_gone_takes_its_picture_with_it() {
        // The cache is the set of things the deck can run now, not a history of
        // everything it has ever run.
        let first = launch(r"C:\Apps\one.exe");
        let second = Action::keys("ctrl+m").expect("a valid combination");
        let mut cache = super::IconCache::new();

        cache.prepare(None, std::slice::from_ref(&first), None);
        assert!(
            cache.frame(&first).is_some(),
            "a bound program has a picture"
        );

        cache.prepare(None, std::slice::from_ref(&second), None);
        assert!(
            cache.frame(&first).is_none(),
            "the program that was unbound is forgotten"
        );
        assert!(cache.frame(&second).is_some(), "the new binding has one");
    }

    #[test]
    fn a_key_that_runs_nothing_has_nothing_of_its_own_to_show() {
        // A key with no binding never reaches the icon cache with an action,
        // and the grid asks for its flat colour instead of a picture.
        let cache = super::IconCache::new();
        assert_eq!(cache.artwork(None), Artwork::None);
    }

    #[test]
    fn the_editor_has_the_picture_of_its_program_before_it_is_bound() {
        // Choosing an application fills the editor's field, and the key it is
        // about to become shows that program's icon. Nothing is bound yet, so
        // the cache only knows the program because the draft named it.
        let program = std::env::current_exe().expect("the test binary has a path");
        let mut cache = super::IconCache::new();

        cache.prepare(None, &[], Some(&program));

        #[cfg(windows)]
        assert!(
            cache.program_frame(&program).is_some(),
            "the draft's icon is read before SAVE BINDING, not after"
        );
        #[cfg(not(windows))]
        assert!(
            cache.program_frame(&program).is_none(),
            "there is no shell to ask outside Windows"
        );
    }

    #[test]
    fn an_empty_program_field_is_not_a_program() {
        // The field is empty until the user types or picks, and asking the shell
        // about "" would be a call for an answer nothing can use.
        let mut cache = super::IconCache::new();
        cache.prepare(None, &[], Some(std::path::Path::new("")));

        assert!(cache.program_frame(std::path::Path::new("")).is_none());
    }
}
