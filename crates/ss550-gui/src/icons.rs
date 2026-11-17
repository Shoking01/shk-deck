//! The picture a key shows for the thing it runs.
//!
//! A key bound to a program draws that program's own icon, read out of the file
//! by [`ss550_tray::appicon`]. A key bound to a URL draws the site's own icon,
//! fetched once by [`ss550_tray::favicon`] and kept in [`crate::favicons`]. A
//! program whose icon cannot be read, a URL whose site has no icon, and a
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
//! The one thing `prepare` cannot do is fetch: a request is a network call, and
//! it runs on the background executor through [`crate::favicons::fetch`], coming
//! back as state through [`IconCache::finish_site`].
//!
//! ## Why the deck gets the same picture
//!
//! The deck's tile and the window's tile are the same artwork fitted to the same
//! image spec — [`crate::render`] builds both from one entry point — because a
//! key that shows one thing and sends another is a bug the user cannot see.
//!
//! ## The title is drawn, not converted
//!
//! A key may carry a title, and the deck only ever receives a JPEG: the words
//! have to be pixels before the encoder sees them. [`compose_title`] draws them
//! with the same five-by-seven bitmap the initial uses, at a whole-pixel scale,
//! onto the composed picture — so the tile the deck receives and the tile the
//! window paints carry the same words, and nothing is rasterised inside
//! [`IconCache::frame`]. The pair (picture, title) is composed once in
//! [`IconCache::prepare`], on the change that made it necessary.
//!
//! ## Example
//!
//! ```text
//! let mut icons = IconCache::new();
//! icons.prepare(Some(spec), &actions);
//! let face = icons.artwork(&action);
//! let picture = icons.frame(&action);
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use deck_api::ImageSpec;
use gpui_kit::RenderImage;
use image::{imageops, Rgba, RgbaImage};
use ss550_engine::Action;

use crate::favicons;
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
    /// The icon of what the key runs: a program's, read out of the file, or a
    /// site's, fetched once and kept.
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

/// The picture a title is composed onto, and the untitled tile built from it.
///
/// The pixels are kept because a title can arrive after the icon was read: a
/// titled variant is composed from these, so typing a title never asks Windows
/// for the same icon a second time.
struct BaseArt {
    /// The composed pixels, at the deck's own tile size.
    picture: RgbaImage,
    /// What the tile draws and sends when the key has no title.
    art: Art,
}

/// Which picture a key's face is built from, before any title is drawn on it.
///
/// Two keys that run the same program share one base and, when they carry the
/// same words, one titled variant: a title changes the payload, not the icon.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Base {
    /// A program's own icon, keyed by its path.
    Program(PathBuf),
    /// A site's icon, keyed by its host.
    Site(String),
    /// The initial of what the key runs.
    Glyph(char),
}

/// One key's face as [`IconCache::prepare`] needs it: what it runs, and what it
/// says.
///
/// The title is per key while the icon is per program, so the two are separate
/// facts and the cache keys its work by both.
#[derive(Clone, Copy)]
pub(crate) struct KeyArt<'a> {
    /// What the key runs.
    pub action: &'a Action,
    /// The words the profile gives the key, when it gives any.
    pub title: Option<&'a str>,
}

/// Every picture the deck's bindings need, built once per change.
pub(crate) struct IconCache {
    /// The image size the pictures were built for. A different deck is a
    /// different tile, and nothing built for the old one can be reused.
    spec: Option<ImageSpec>,
    /// One entry per program path. A `None` remembers a program with no
    /// readable icon, so a rebind does not ask Windows for it again.
    icons: BTreeMap<PathBuf, Option<BaseArt>>,
    /// One entry per site, keyed by host. A `None` remembers a site that has no
    /// icon to give — offline, or nothing at any of the paths a site publishes
    /// one on — so the key draws its initial and no second request is made.
    ///
    /// The entry is the **host's**, not the URL's: two bindings to the same site
    /// share one picture, and a rebind to another host leaves this one alone.
    sites: BTreeMap<String, Option<BaseArt>>,
    /// Hosts a fetch is already in flight for, so a second pass over the
    /// bindings does not start a second request for the same site.
    pending: BTreeSet<String>,
    /// One entry per initial, so two keys that start with the same letter share
    /// a picture instead of encoding it twice.
    glyphs: BTreeMap<char, BaseArt>,
    /// One entry per (picture, title) pair: the base picture with the words
    /// drawn into it, encoded for this deck.
    ///
    /// Built by [`IconCache::prepare`] on the change that added the title, and
    /// read by [`IconCache::tile`] at paint time — never composed in a frame.
    titled: BTreeMap<(Base, String), Art>,
}

impl IconCache {
    /// An empty cache: nothing is extracted until a deck says what it needs.
    pub(crate) fn new() -> Self {
        Self {
            spec: None,
            icons: BTreeMap::new(),
            sites: BTreeMap::new(),
            pending: BTreeSet::new(),
            glyphs: BTreeMap::new(),
            titled: BTreeMap::new(),
        }
    }

    /// Builds every picture `actions` needs, and forgets the rest.
    ///
    /// This is the whole of the work: one icon extraction per program path, one
    /// cached file read per site, and one glyph per distinct initial. Called on a
    /// rebind, a profile switch and a deck change — never from a frame.
    ///
    /// Nothing here reaches the network: a site with no file on disk stays out of
    /// the map, and [`IconCache::begin_site`] is what turns that absence into a
    /// request on the background executor.
    ///
    /// `draft` is the program the editor is about to bind. It is not a binding
    /// yet, but the key's face is decided by it, so its icon is read here — once,
    /// on the change that made it the draft — and the editor draws it before
    /// SAVE BINDING is pressed. It is kept in the cache for the same reason the
    /// bound ones are: the tile and the deck will want exactly this picture.
    pub(crate) fn prepare(
        &mut self,
        spec: Option<ImageSpec>,
        faces: &[KeyArt<'_>],
        draft: Option<&Path>,
    ) {
        if spec != self.spec {
            // Every fitted frame and every encoded payload was built for the
            // old image size, so none of them can be served to this deck.
            self.icons.clear();
            self.sites.clear();
            self.glyphs.clear();
            self.titled.clear();
            self.spec = spec;
        }

        let mut programs: Vec<PathBuf> = Vec::new();
        let mut hosts: Vec<String> = Vec::new();
        let mut letters: Vec<char> = Vec::new();
        for face in faces {
            let action = face.action;
            if let Action::Launch { program, .. } = action {
                if !programs.contains(program) {
                    programs.push(program.clone());
                }
            }
            if let Action::OpenUrl { url } = action {
                if let Some(host) = favicons::host_of(url) {
                    if !hosts.contains(&host) {
                        hosts.push(host);
                    }
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
        self.sites.retain(|host, _| hosts.contains(host));
        self.glyphs.retain(|letter, _| letters.contains(letter));

        for program in programs {
            if self.icons.contains_key(&program) {
                continue;
            }
            let art = build_icon(self.spec, &program);
            self.icons.insert(program, art);
        }
        // A site's icon is read from the file an earlier run kept, and only for
        // a host this cache has never answered for. A host with no file stays
        // absent, which is what tells the controller to fetch it.
        let config = (!hosts.is_empty()).then(ss550_engine::config_dir);
        for host in hosts {
            if self.sites.contains_key(&host) {
                continue;
            }
            let picture = config
                .as_deref()
                .and_then(|config| favicons::cached(config, &host));
            if let Some(art) = picture
                .as_ref()
                .and_then(|picture| build_site(self.spec, &host, picture))
            {
                self.sites.insert(host, Some(art));
            }
        }
        for letter in letters {
            if self.glyphs.contains_key(&letter) {
                continue;
            }
            let art = build_glyph(self.spec, letter);
            self.glyphs.insert(letter, art);
        }

        // The title is part of the payload, so every (picture, title) pair the
        // profile asks for is composed once, here. The pixels come from the
        // base art built above: a title arriving later never reads the icon or
        // the site's file again, and the same pair is never composed twice.
        let mut wanted: BTreeSet<(Base, String)> = BTreeSet::new();
        for face in faces {
            let Some(title) = face.title.map(str::trim).filter(|title| !title.is_empty()) else {
                continue;
            };
            if let Some(base) = self.base_key(face.action) {
                wanted.insert((base, title.to_string()));
            }
        }
        // A title that was cleared takes its variant with it, exactly as an
        // unbound program takes its picture.
        self.titled.retain(|key, _| wanted.contains(key));
        for key in wanted {
            if self.titled.contains_key(&key) {
                continue;
            }
            let Some(picture) = self.base_picture(&key.0) else {
                continue;
            };
            let composed = compose_title(picture, &key.1, Placement::Band);
            self.titled.insert(key, art(self.spec, &composed, "title"));
        }
    }

    /// Whether `host` has no answer yet, marking it as being fetched.
    ///
    /// `false` means one of three ordinary things: the site's icon is already in
    /// the cache, a fetch is already in flight, or the site was asked and had
    /// none to give. None of them is worth a second request.
    pub(crate) fn begin_site(&mut self, host: &str) -> bool {
        if self.sites.contains_key(host) || self.pending.contains(host) {
            return false;
        }
        self.pending.insert(host.to_string());
        true
    }

    /// Records what a fetch found for `host`.
    ///
    /// `None` is a site with no icon — offline, a `404`, an SVG — and it is
    /// remembered as a miss, the way a program with no readable icon is: the key
    /// draws its initial, and the site is not asked again in this session.
    pub(crate) fn finish_site(&mut self, host: &str, picture: Option<&RgbaImage>) {
        self.pending.remove(host);
        let art = picture.and_then(|picture| build_site(self.spec, host, picture));
        self.sites.insert(host.to_string(), art);
    }

    /// Whether the cache holds a picture for `host`.
    ///
    /// The grid asks this through [`IconCache::artwork`]; this is the same fact
    /// without the picture, for the tests that pin the cache's bookkeeping.
    #[cfg(test)]
    pub(crate) fn has_site(&self, host: &str) -> bool {
        matches!(self.sites.get(host), Some(Some(_)))
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
            Some(Action::OpenUrl { url }) => match self.site_art(url) {
                Some(_) => Artwork::Icon,
                None => Artwork::Glyph,
            },
            Some(Action::Keys { .. }) => Artwork::Glyph,
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
            .map(|base| &base.art.frame)
    }

    /// The picture the deck receives for `action`, once it stated a size.
    ///
    /// With a title, this is the variant the title was composed into; without
    /// one, the untitled base. A title the cache was never told about falls back
    /// to the base rather than leaving the key blank — the paint that needed it
    /// ran before the cache was rebuilt.
    pub(crate) fn tile(&self, action: &Action, title: Option<&str>) -> Option<&[u8]> {
        let titled = title
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .and_then(|title| {
                let base = self.base_key(action)?;
                self.titled.get(&(base, title.to_string()))
            });
        titled
            .or_else(|| self.art(action))
            .and_then(|art| art.tile.as_deref())
    }

    /// The deck's payload for a flat colour that carries a title.
    ///
    /// A key with no picture has no base art to draw on, and the deck only ever
    /// receives a JPEG: the words are drawn into a block on the colour itself.
    /// `None` when there is no title, or no deck has stated an image size.
    pub(crate) fn titled_colour(&self, rgb: [u8; 3], title: Option<&str>) -> Option<Vec<u8>> {
        let title = title.map(str::trim).filter(|title| !title.is_empty())?;
        let spec = self.spec?;
        let width = u32::from(spec.width()).max(1);
        let height = u32::from(spec.height()).max(1);
        let picture = RgbaImage::from_pixel(width, height, Rgba([rgb[0], rgb[1], rgb[2], 0xff]));
        let composed = compose_title(&picture, title, Placement::Block);
        render::render_rgba(spec, "title", &composed).ok()
    }

    /// The picture one action has: the icon of what it runs first, its initial
    /// second.
    ///
    /// A program whose icon could not be read, and a site with no icon to give,
    /// fall through to the initial, so a key that runs something always has a
    /// picture — and the deck receives the one the tile draws.
    fn art(&self, action: &Action) -> Option<&Art> {
        self.base_art(&self.base_key(action)?)
    }

    /// Which base picture `action` resolves to, or `None` when none was built.
    ///
    /// This is the one statement of the order — icon, then initial — that both
    /// the frame and the titled variant read, so the two cannot disagree about
    /// which picture a key wears.
    fn base_key(&self, action: &Action) -> Option<Base> {
        let letter = glyph_letter(action);
        match action {
            Action::Launch { program, .. } => match self.icons.get(program) {
                Some(Some(_)) => Some(Base::Program(program.clone())),
                _ => self
                    .glyphs
                    .contains_key(&letter)
                    .then_some(Base::Glyph(letter)),
            },
            Action::OpenUrl { url } => {
                if let Some(host) = favicons::host_of(url) {
                    if matches!(self.sites.get(&host), Some(Some(_))) {
                        return Some(Base::Site(host));
                    }
                }
                self.glyphs
                    .contains_key(&letter)
                    .then_some(Base::Glyph(letter))
            }
            Action::Keys { .. } => self
                .glyphs
                .contains_key(&letter)
                .then_some(Base::Glyph(letter)),
        }
    }

    /// The untitled art one base holds.
    fn base_art(&self, base: &Base) -> Option<&Art> {
        match base {
            Base::Program(path) => self
                .icons
                .get(path)
                .and_then(Option::as_ref)
                .map(|base| &base.art),
            Base::Site(host) => self
                .sites
                .get(host)
                .and_then(Option::as_ref)
                .map(|base| &base.art),
            Base::Glyph(letter) => self.glyphs.get(letter).map(|base| &base.art),
        }
    }

    /// The pixels one base was composed at, which a title is drawn onto.
    fn base_picture(&self, base: &Base) -> Option<&RgbaImage> {
        match base {
            Base::Program(path) => self
                .icons
                .get(path)
                .and_then(Option::as_ref)
                .map(|base| &base.picture),
            Base::Site(host) => self
                .sites
                .get(host)
                .and_then(Option::as_ref)
                .map(|base| &base.picture),
            Base::Glyph(letter) => self.glyphs.get(letter).map(|base| &base.picture),
        }
    }

    /// The site's icon for `url`, when the cache holds one.
    fn site_art(&self, url: &str) -> Option<&Art> {
        let host = favicons::host_of(url)?;
        self.sites
            .get(&host)
            .and_then(Option::as_ref)
            .map(|base| &base.art)
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

/// [`art`], keeping the pixels a title will be drawn onto later.
fn base_art(spec: Option<ImageSpec>, picture: RgbaImage, name: &str) -> BaseArt {
    let art = art(spec, &picture, name);
    BaseArt { picture, art }
}

/// The icon of one program, or `None` when the file has none to read.
fn build_icon(spec: Option<ImageSpec>, program: &Path) -> Option<BaseArt> {
    let pixels = ss550_tray::appicon::program_icon(program)?;
    if pixels.is_blank() {
        return None;
    }
    let icon = RgbaImage::from_raw(pixels.width(), pixels.height(), pixels.rgba().to_vec())?;
    // The shell answers with the largest icon it has — up to 256 pixels — and a
    // tile is 108: letting the fit resample a 256-pixel icon would be a second
    // resample of a picture that has already been through one. It is composed
    // once, at the tile's own size, so the fit has nothing left to scale: shrunk
    // with a real filter when the icon is larger, grown at whole pixels when it
    // is smaller.
    let side = spec.map_or(icon.width().max(icon.height()), |spec| {
        u32::from(spec.width().max(spec.height())).max(1)
    });
    let picture = compose_icon(&icon, side);
    // The path, not the file name: a refusal names what the user can go and
    // look at, and two programs can share a file name.
    let name = program.to_string_lossy();
    Some(base_art(spec, picture, &name))
}

/// The icon of one site, composed for the tile the way a program's is.
///
/// The picture is already decoded — [`crate::favicons`] did that off the UI
/// thread — so this is the compose and the encode, and nothing else. A site
/// icon is drawn on the same margin budget as a program icon, because the two
/// sit side by side in the same grid and a key is not a browser tab.
fn build_site(spec: Option<ImageSpec>, host: &str, picture: &RgbaImage) -> Option<BaseArt> {
    let (width, height) = picture.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    if picture.pixels().all(|pixel| pixel[3] == 0) {
        return None;
    }
    let side = spec.map_or(width.max(height), |spec| {
        u32::from(spec.width().max(spec.height())).max(1)
    });
    let composed = compose_icon(picture, side);
    // The host, so a refusal names the site the user can go and look at.
    Some(base_art(spec, composed, host))
}

/// The share of the tile the icon covers; the rest is the margin.
const ICON_BUDGET: f32 = 0.72;

/// Centres `icon` on a square of `side` pixels, at the largest size that fits
/// the margin budget.
///
/// Which rule applies is decided by the source's size, and the two are not
/// interchangeable:
///
/// * **Larger than the budget — shrink it.** A 256-pixel jumbo icon on a
///   108-pixel tile is resampled once, here, with a real filter. This is the
///   case the picture exists for: a downscale is where a smooth filter turns a
///   stepped edge into a drawn one, and it costs one pass over 78×78 pixels on
///   the thread that built the icon, never on a frame.
/// * **Smaller than the budget — grow it at whole pixels.** A 32-pixel icon on a
///   108-pixel tile is the icon the user recognises; a smoothed 2.4× of it is a
///   blurred guess at it. Nearest-neighbour at 2× keeps the shape exact, and the
///   deck's own JPEG never has to hide a resample.
///
/// The margin keeps the icon off the tile's edge, the way a key in a finished
/// controller looks.
fn compose_icon(icon: &RgbaImage, side: u32) -> RgbaImage {
    let side = side.max(1);
    let mut canvas = RgbaImage::from_pixel(side, side, Rgba([0, 0, 0, 0xff]));
    let (width, height) = icon.dimensions();
    if width == 0 || height == 0 {
        return canvas;
    }

    // 72 % of the tile is the icon, the rest is margin.
    let budget = (side as f32 * ICON_BUDGET).round().max(1.0) as u32;
    let longest = width.max(height);

    if longest > budget {
        // More pixels than the tile can use: one resample, down.
        let shrunk = shrink_icon(icon, budget);
        let (drawn_width, drawn_height) = shrunk.dimensions();
        let left = (side - drawn_width) / 2;
        let top = (side - drawn_height) / 2;
        imageops::overlay(&mut canvas, &shrunk, i64::from(left), i64::from(top));
        return canvas;
    }

    // Fewer pixels than the tile can use: the largest whole scale that still
    // fits inside the budget, so a source pixel stays a square block.
    let scale = (budget / longest).max(1);
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

/// `icon` resampled down so its longest side fits inside `longest`, on black.
///
/// The one icon downscale in this crate: the deck's tile and the picker's row
/// both shrink a jumbo icon with it, so they cannot disagree about what a
/// shrunken icon looks like.
///
/// The icon is flattened onto black **before** the filter runs, because
/// [`imageops::resize`] weights colour and alpha independently: an edge pixel
/// that is half transparent would contribute half its colour and then be
/// darkened a second time when the deck flattens it. Black is where the picture
/// is going anyway, so the multiply happens once, at the size with the most
/// information.
pub(crate) fn shrink_icon(icon: &RgbaImage, longest: u32) -> RgbaImage {
    let (width, height) = icon.dimensions();
    let longest = longest.max(1);
    if width == 0 || height == 0 {
        return RgbaImage::new(1, 1);
    }

    // Down only. An icon that already fits is flattened and kept: enlarging it
    // here would be a smooth upscale, which is the blur this whole path exists
    // to avoid. A caller that wants a bigger picture composes it itself.
    if width.max(height) <= longest {
        return opaque(&render::on_black(icon));
    }

    let scale = f64::from(longest) / f64::from(width.max(height));
    let drawn_width = (f64::from(width) * scale)
        .round()
        .clamp(1.0, f64::from(longest)) as u32;
    let drawn_height = (f64::from(height) * scale)
        .round()
        .clamp(1.0, f64::from(longest)) as u32;

    let flat = render::on_black(icon);
    let scaled = imageops::resize(
        &flat,
        drawn_width,
        drawn_height,
        imageops::FilterType::Lanczos3,
    );
    opaque(&scaled)
}

/// An opaque RGBA copy of a picture that has already been flattened onto black.
fn opaque(picture: &image::RgbImage) -> RgbaImage {
    let mut out = RgbaImage::new(picture.width(), picture.height());
    for (x, y, pixel) in picture.enumerate_pixels() {
        // Already on black, so the alpha is the canvas's: opaque.
        out.put_pixel(x, y, Rgba([pixel[0], pixel[1], pixel[2], 0xff]));
    }
    out
}

/// The initial of what a key runs, drawn on the accent colour.
fn build_glyph(spec: Option<ImageSpec>, letter: char) -> BaseArt {
    let side = spec
        .map_or(GLYPH_SIDE, |spec| {
            u32::from(spec.width().max(spec.height())).max(1)
        })
        .min(GLYPH_MAX_SIDE);
    let picture = glyph_image(letter, side);
    base_art(spec, picture, "glyph")
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

/// The ink a title is drawn in.
const TITLE_INK: [u8; 4] = [0xff, 0xff, 0xff, 0xff];

/// How far one character advances: five dots and one of gap.
const TITLE_ADVANCE: u32 = 6;

/// The gap between two lines of a title, in dots.
const TITLE_LINE_GAP: u32 = 2;

/// The most lines a title is ever drawn on.
///
/// Three is the ceiling the cap was chosen for: thirty characters at ten per
/// line is three lines, and a fourth would make the type smaller than the key
/// can show.
const TITLE_MAX_LINES: usize = 3;

/// The share of a tile's height the bottom band takes, in percent.
const BAND_PERCENT: u32 = 38;

/// The share of a tile's height a block on a flat colour may use, in percent.
const BLOCK_PERCENT: u32 = 80;

/// Where a title sits on the tile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Placement {
    /// A dark band along the bottom, over a picture or an initial.
    Band,
    /// A dark block in the middle, on a key that has no picture of its own.
    Block,
}

/// A title broken into lines, with the whole-pixel scale it is drawn at.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct TitleLayout {
    /// One entry per line, already upper-cased and trimmed.
    pub(crate) lines: Vec<Vec<char>>,
    /// The pixels each dot of the bitmap is drawn at.
    pub(crate) scale: u32,
}

/// Draws `title` onto a copy of `picture`.
///
/// The words go into the pixels the encoder receives, not into the window's
/// layout: the deck has no text, only a JPEG. The scale is whole pixels and the
/// layout is resolved first, so the same string always lands on the same dots.
pub(crate) fn compose_title(picture: &RgbaImage, title: &str, placement: Placement) -> RgbaImage {
    let mut picture = picture.clone();
    let (width, height) = picture.dimensions();
    let Some(layout) = layout_title(title, width, height, placement) else {
        return picture;
    };

    let scale = layout.scale;
    let line_height = 7 * scale;
    let gap = TITLE_LINE_GAP * scale;
    let count = layout.lines.len() as u32;
    let block_height = count * line_height + count.saturating_sub(1) * gap;
    let block_width = layout
        .lines
        .iter()
        .map(|line| line_width(line, scale))
        .max()
        .unwrap_or(0);

    let pad = (height / 32).max(1);
    let top = match placement {
        // A band is bottom-aligned, the way a caption sits on a picture.
        Placement::Band => height.saturating_sub(block_height + pad),
        // A block is centred, because there is no picture to sit under.
        Placement::Block => height.saturating_sub(block_height) / 2,
    };
    let left = width.saturating_sub(block_width) / 2;

    // The scrim: a band across the bottom over a picture, a block just behind
    // the words on a flat colour. White on a white key is unreadable, which is
    // why a colour tile gets one too.
    let (x0, y0, x1, y1) = match placement {
        Placement::Band => (0, top.saturating_sub(pad), width, height),
        Placement::Block => (
            left.saturating_sub(pad),
            top.saturating_sub(pad),
            (left + block_width + pad).min(width),
            (top + block_height + pad).min(height),
        ),
    };
    darken(&mut picture, x0, y0, x1, y1);

    let mut y = top;
    for line in &layout.lines {
        let x = width.saturating_sub(line_width(line, scale)) / 2;
        draw_line(&mut picture, line, x, y, scale);
        y += line_height + gap;
    }
    picture
}

/// Breaks `title` into the lines a tile of `width` by `height` can hold.
///
/// The largest whole scale at which the whole string fits — at most three
/// lines — wins, so a short title gets big type and the cap lands on three
/// small lines. A title that fits at no scale keeps what fits at one dot a
/// pixel and drops the rest: shrinking below that is a blur, not a title.
fn layout_title(title: &str, width: u32, height: u32, placement: Placement) -> Option<TitleLayout> {
    if width == 0 || height == 0 {
        return None;
    }
    // The bitmap has no lower case: a title reads as a label, not as prose.
    let text: Vec<char> = title
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .flat_map(char::to_uppercase)
        .collect();
    if text.is_empty() {
        return None;
    }

    let margin = (width / 16).max(1);
    let box_width = width.saturating_sub(margin * 2).max(1);
    let box_height = match placement {
        Placement::Band => (height * BAND_PERCENT / 100).max(1),
        Placement::Block => (height * BLOCK_PERCENT / 100).max(1),
    };

    let ceiling = (height / 8).max(1);
    for scale in (1..=ceiling).rev() {
        let line_height = (7 + TITLE_LINE_GAP) * scale;
        if line_height > box_height {
            continue;
        }
        let per_line = (box_width / (TITLE_ADVANCE * scale)).max(1) as usize;
        let capacity = (box_height / line_height).max(1) as usize;
        let (lines, dropped) = wrap_title(&text, per_line, capacity.min(TITLE_MAX_LINES));
        if !dropped && !lines.is_empty() {
            return Some(TitleLayout { lines, scale });
        }
    }

    // One dot a pixel is the floor. What does not fit is dropped rather than
    // drawn smaller, and the height is still respected: a band that holds one
    // line gets one line.
    let line_height = 7 + TITLE_LINE_GAP;
    let per_line = (box_width / TITLE_ADVANCE).max(1) as usize;
    let capacity = (box_height / line_height).max(1) as usize;
    let (lines, _) = wrap_title(&text, per_line, capacity.min(TITLE_MAX_LINES));
    (!lines.is_empty()).then_some(TitleLayout { lines, scale: 1 })
}

/// Greedy wrap at `per_line` characters, breaking on a space when it can.
///
/// The second half of the answer says whether anything was left out, so the
/// caller can try a smaller scale instead of silently cutting a title in half.
fn wrap_title(text: &[char], per_line: usize, max_lines: usize) -> (Vec<Vec<char>>, bool) {
    let per_line = per_line.max(1);
    let max_lines = max_lines.max(1);
    let mut lines: Vec<Vec<char>> = Vec::new();
    let mut current: Vec<char> = Vec::new();
    let mut dropped = false;

    for &character in text {
        if character == ' ' && current.is_empty() {
            // A line never starts with a gap, whatever the spacing was.
            continue;
        }
        if current.len() == per_line {
            // The line is full: break after its last space so a word stays
            // whole when it can, otherwise break the word itself.
            let line = match current.iter().rposition(|known| *known == ' ') {
                Some(at) => {
                    let rest = current.split_off(at + 1);
                    current.pop();
                    let line = std::mem::take(&mut current);
                    current = rest;
                    line
                }
                None => std::mem::take(&mut current),
            };
            lines.push(line);
            if lines.len() == max_lines {
                dropped = true;
                break;
            }
        }
        current.push(character);
    }

    if !current.is_empty() {
        while current.last() == Some(&' ') {
            current.pop();
        }
        if current.is_empty() {
            // The rest of the title was spacing; nothing to draw.
        } else if lines.len() < max_lines {
            lines.push(current);
        } else {
            dropped = true;
        }
    }

    (lines, dropped)
}

/// The dots one character of a title draws, five columns by seven rows.
///
/// The letters and digits are [`glyph_rows`] — one table for the initial and the
/// title, so a key's letter and its words are drawn by the same face. A space is
/// blank and the punctuation a title actually uses is here; anything else falls
/// back to the `#` an unknown initial draws, so a title is never a row of holes.
fn text_rows(character: char) -> [u8; 7] {
    if character == ' ' {
        return [0; 7];
    }
    for (known, rows) in TITLE_PUNCTUATION {
        if *known == character {
            return *rows;
        }
    }
    glyph_rows(character)
}

/// The punctuation a title is likely to use, five dots wide.
///
/// A title is a name, not a paragraph: this is the set that appears in one —
/// separators, a dot, brackets, an ampersand — and not a font.
const TITLE_PUNCTUATION: &[(char, [u8; 7])] = &[
    ('.', [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C]),
    (',', [0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x18]),
    ('-', [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00]),
    ('_', [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F]),
    (':', [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00]),
    (';', [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x18]),
    ('!', [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04]),
    ('?', [0x0E, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04]),
    ('/', [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10]),
    ('\\', [0x10, 0x08, 0x08, 0x04, 0x02, 0x02, 0x01]),
    ('\'', [0x04, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00]),
    ('"', [0x0A, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00]),
    ('(', [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02]),
    (')', [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08]),
    ('[', [0x0E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0E]),
    (']', [0x0E, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0E]),
    ('+', [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00]),
    ('=', [0x00, 0x00, 0x1F, 0x00, 0x1F, 0x00, 0x00]),
    ('&', [0x0C, 0x12, 0x14, 0x08, 0x15, 0x12, 0x0D]),
    ('*', [0x00, 0x04, 0x15, 0x0E, 0x15, 0x04, 0x00]),
    ('%', [0x18, 0x19, 0x02, 0x04, 0x08, 0x13, 0x03]),
    ('·', [0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00]),
    ('•', [0x00, 0x00, 0x00, 0x0E, 0x0E, 0x00, 0x00]),
];

/// The width of one laid-out line, in pixels: the dots, less the last gap.
fn line_width(line: &[char], scale: u32) -> u32 {
    (line.len() as u32 * TITLE_ADVANCE * scale).saturating_sub(scale)
}

/// Draws one line of dots at `scale`, starting at `left`.
fn draw_line(picture: &mut RgbaImage, line: &[char], left: u32, top: u32, scale: u32) {
    let (width, height) = picture.dimensions();
    let mut x = left;
    for &character in line {
        for (row, bits) in text_rows(character).iter().enumerate() {
            for column in 0..5 {
                if (bits >> (4 - column)) & 1 == 0 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let pixel_x = x + column * scale + dx;
                        let pixel_y = top + row as u32 * scale + dy;
                        if pixel_x < width && pixel_y < height {
                            picture.put_pixel(pixel_x, pixel_y, Rgba(TITLE_INK));
                        }
                    }
                }
            }
        }
        x += TITLE_ADVANCE * scale;
    }
}

/// Darkens a rectangle of `picture`, so white words stay readable on it.
///
/// A multiply rather than an alpha blend: the pixels are already opaque and a
/// JPEG has no alpha, so what is under the band keeps its shape and loses most
/// of its brightness — the same band the window draws over a picture.
fn darken(picture: &mut RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32) {
    /// What is left of a channel under the band: 0.38, in 255ths.
    const KEEP: u16 = 97;
    let (width, height) = picture.dimensions();
    for y in y0.min(height)..y1.min(height) {
        for x in x0.min(width)..x1.min(width) {
            let pixel = picture.get_pixel_mut(x, y);
            for channel in 0..3 {
                pixel.0[channel] = ((u16::from(pixel.0[channel]) * KEEP) / 255) as u8;
            }
        }
    }
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
        compose_icon, compose_title, first_letter, glyph_image, glyph_letter, glyph_rows, host_of,
        last_segment, layout_title, text_rows, Artwork, KeyArt, Placement, TitleLayout, GLYPHS,
        GLYPH_ACCENT, TITLE_MAX_LINES, TITLE_PUNCTUATION,
    };
    use ss550_engine::Action;
    use std::path::PathBuf;

    fn launch(program: &str) -> Action {
        Action::Launch {
            program: PathBuf::from(program),
            args: Vec::new(),
        }
    }

    /// One key's face with no title, which is what every untitled key asks for.
    fn face(action: &Action) -> KeyArt<'_> {
        KeyArt {
            action,
            title: None,
        }
    }

    /// One key's face carrying `title`.
    fn titled<'a>(action: &'a Action, title: &'a str) -> KeyArt<'a> {
        KeyArt {
            action,
            title: Some(title),
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

    /// The lines of a layout joined back into one string, gaps removed.
    fn joined(layout: &TitleLayout) -> String {
        layout
            .lines
            .iter()
            .map(|line| line.iter().collect::<String>())
            .collect::<Vec<String>>()
            .concat()
    }

    #[test]
    fn a_title_wraps_at_the_tiles_width() {
        // A 72-pixel tile leaves ten characters a line at the floor scale, so
        // two words that would make eleven break at the space between them.
        let layout = layout_title("stream deck", 72, 72, Placement::Band).expect("a title");

        assert_eq!(layout.lines.len(), 2, "{:?}", layout.lines);
        assert_eq!(layout.lines[0].iter().collect::<String>(), "STREAM");
        assert_eq!(layout.lines[1].iter().collect::<String>(), "DECK");
        assert_eq!(joined(&layout), "STREAMDECK");
        for line in &layout.lines {
            assert!(line.len() <= 10, "a line overflows the tile: {line:?}");
        }
    }

    #[test]
    fn a_title_that_fits_nowhere_drops_what_does_not_fit() {
        // A 32-pixel tile holds one line of four characters in its band. The
        // rest is dropped, never shrunk: a half-dot letter is a blur, and the
        // key still has to read as words.
        let layout = layout_title("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123", 32, 32, Placement::Band)
            .expect("a title");

        assert_eq!(layout.lines.len(), 1, "{:?}", layout.lines);
        assert_eq!(layout.lines[0].len(), 4);
        assert_eq!(layout.lines[0].iter().collect::<String>(), "ABCD");
        assert_eq!(layout.scale, 1, "the floor is one dot a pixel");
    }

    #[test]
    fn a_single_character_is_one_line_at_a_legible_scale() {
        // One letter needs no wrapping, so the largest whole scale that fits
        // the band is chosen instead of the floor.
        let layout = layout_title("X", 72, 72, Placement::Band).expect("a title");

        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0], vec!['X']);
        assert!(
            layout.scale >= 2,
            "one letter is drawn big, got {}",
            layout.scale
        );
    }

    #[test]
    fn a_title_of_exactly_the_cap_fits_in_three_lines() {
        // The engine's cap is thirty characters; the tile must be able to show
        // all of them at the floor scale, in the three lines the cap was chosen
        // for.
        let title: String = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123".to_string();
        assert_eq!(title.chars().count(), ss550_engine::MAX_TITLE_CHARS);

        let layout = layout_title(&title, 72, 72, Placement::Band).expect("a title");

        assert_eq!(layout.lines.len(), TITLE_MAX_LINES);
        assert_eq!(joined(&layout), title, "the whole cap is drawn");
        assert_eq!(layout.scale, 1);
    }

    #[test]
    fn a_title_is_drawn_into_the_pixels_the_deck_receives() {
        // The words are pixels, not a layout: the JPEG the deck receives is the
        // only place a title can live.
        let base = glyph_image('O', 72);
        let composed = compose_title(&base, "live", Placement::Band);

        assert_eq!(composed.dimensions(), base.dimensions());
        // The scrim is the bottom band; the picture above it is untouched.
        assert_eq!(
            composed.get_pixel(0, 0).0,
            base.get_pixel(0, 0).0,
            "the top of the tile is not under the band"
        );
        let white = composed
            .pixels()
            .filter(|pixel| pixel.0 == [0xFF, 0xFF, 0xFF, 0xFF])
            .count();
        let before = base
            .pixels()
            .filter(|pixel| pixel.0 == [0xFF, 0xFF, 0xFF, 0xFF])
            .count();
        assert!(
            white > before,
            "the title added white dots: {white} > {before}"
        );
        // And the scrim darkened the band it covers.
        let band = composed.get_pixel(0, 71).0;
        let base_band = base.get_pixel(0, 71).0;
        assert!(
            u16::from(band[0]) + u16::from(band[1]) + u16::from(band[2])
                < u16::from(base_band[0]) + u16::from(base_band[1]) + u16::from(base_band[2]),
            "the band is darker than the accent it covers"
        );
    }

    #[test]
    fn a_colour_key_gets_a_block_because_white_on_white_is_unreadable() {
        // A flat colour has no picture to darken, so the words stand on their
        // own block in the middle — and a white key is still readable.
        let base = image::RgbaImage::from_pixel(72, 72, image::Rgba([0xE4, 0xE4, 0xE7, 0xFF]));
        let composed = compose_title(&base, "MUTE", Placement::Block);

        // The corners are the colour, untouched: the block is centred.
        assert_eq!(composed.get_pixel(0, 0).0, [0xE4, 0xE4, 0xE7, 0xFF]);
        let dark = composed
            .pixels()
            .filter(|pixel| pixel[0] < 0x80 && pixel[1] < 0x80 && pixel[2] < 0x80)
            .count();
        assert!(dark > 0, "the block behind the words is dark");
        let white = composed
            .pixels()
            .filter(|pixel| pixel.0 == [0xFF, 0xFF, 0xFF, 0xFF])
            .count();
        assert!(white > 0, "the words are white on it");
    }

    #[test]
    fn a_title_that_is_only_spaces_is_no_title_at_all() {
        assert!(layout_title("   ", 72, 72, Placement::Band).is_none());
        assert!(layout_title("", 72, 72, Placement::Band).is_none());
        // A control character is not drawn, and does not become a row of `#`.
        let layout = layout_title("a\tb", 72, 72, Placement::Band).expect("a title");
        assert_eq!(joined(&layout), "AB");
    }

    #[test]
    fn a_character_the_bitmap_does_not_know_is_a_mark_and_a_space_is_blank() {
        assert_eq!(text_rows(' '), [0; 7]);
        assert_eq!(text_rows('.'), [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C]);
        // Lower case is the same letter, exactly as the initial is.
        assert_eq!(text_rows('q'), text_rows('Q'));
        // Anything else is the same mark an unknown initial draws.
        assert_eq!(text_rows('~'), glyph_rows('#'));
    }

    #[test]
    fn every_punctuation_glyph_is_a_five_by_seven_bitmap() {
        // The same bounds the letters are held to: a row with a bit above the
        // fifth column would be clipped.
        for (character, rows) in TITLE_PUNCTUATION {
            for row in rows {
                assert_eq!(
                    *row & 0b1110_0000,
                    0,
                    "{character} has a bit outside its width"
                );
            }
            assert!(
                rows.iter().any(|row| *row != 0),
                "{character} is an empty glyph"
            );
        }
    }

    #[test]
    fn a_titled_key_sends_a_different_payload_from_an_untitled_one() {
        // The title is part of the picture: the bytes the deck receives change
        // when the words do, and the untitled key keeps exactly what it had.
        let spec = deck_api::ImageSpec::new(72, 72, 4_096, deck_api::ImageFormat::Jpeg);
        let action = Action::keys("ctrl+m").expect("a valid combination");
        let mut cache = super::IconCache::new();

        cache.prepare(Some(spec), &[face(&action)], None);
        let plain = cache
            .tile(&action, None)
            .expect("the initial is a picture")
            .to_vec();

        cache.prepare(Some(spec), &[titled(&action, "mute")], None);
        let with_title = cache
            .tile(&action, Some("mute"))
            .expect("the titled initial is a picture")
            .to_vec();

        assert_ne!(plain, with_title, "the words reach the encoder");
        assert!(with_title.len() <= spec.max_bytes());
        assert_eq!(plain, cache.tile(&action, None).expect("still the base"));

        // Clearing the title takes the variant with it, and the base is back.
        cache.prepare(Some(spec), &[face(&action)], None);
        assert_eq!(cache.tile(&action, None).expect("the base"), plain);
        assert_eq!(
            cache
                .tile(&action, Some("mute"))
                .expect("a title the cache was never told about falls back to the base"),
            plain,
        );
    }

    #[test]
    fn a_program_with_no_icon_to_read_is_a_glyph_and_never_a_blank() {
        // The cache is asked about a program that was never prepared: the
        // answer is the initial, which is a picture, not a hole.
        let cache = super::IconCache::new();
        let action = launch(r"C:\this\program\was\never\installed.exe");

        assert_eq!(cache.artwork(Some(&action)), Artwork::Glyph);
        assert!(cache.frame(&action).is_none());
        assert!(cache.tile(&action, None).is_none());
    }

    #[test]
    fn an_icon_that_must_grow_is_drawn_at_whole_pixels_with_a_margin() {
        // The upscale case, and the only one where whole pixels apply: a 2x2
        // icon on a 10-pixel tile. 72 % of ten is seven, so the largest whole
        // scale is three, the icon covers 6x6 and sits two pixels in.
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
    fn a_32_pixel_icon_on_a_108_pixel_tile_grows_by_two_and_stays_sharp() {
        // The case the whole-pixel rule exists for: a small icon, a big tile.
        // 72 % of 108 is 78, two whole pixels is 64, and three would be 96 and
        // overrun the budget — so the icon covers 64 pixels and sits 22 in.
        let side = 108;
        let mut icon = image::RgbaImage::new(32, 32);
        for (x, y, pixel) in icon.enumerate_pixels_mut() {
            *pixel = image::Rgba([x as u8 * 8, y as u8 * 8, 0, 0xff]);
        }

        let canvas = compose_icon(&icon, side);

        assert_eq!(canvas.dimensions(), (side, side));
        let margin = 22;
        // Every source pixel is a solid 2x2 block at exactly its own value: no
        // blending, which a smooth upscale could not manage.
        for (x, y, source) in icon.enumerate_pixels() {
            for dy in 0..2 {
                for dx in 0..2 {
                    let drawn = canvas.get_pixel(margin + x * 2 + dx, margin + y * 2 + dy).0;
                    assert_eq!(drawn, source.0, "at source ({x},{y}) + ({dx},{dy})");
                }
            }
        }
        // Nothing was drawn into the margin.
        assert_eq!(canvas.get_pixel(margin - 1, margin).0, [0, 0, 0, 0xff]);
        assert_eq!(canvas.get_pixel(margin + 64, margin).0, [0, 0, 0, 0xff]);
    }

    #[test]
    fn a_jumbo_icon_is_scaled_down_into_the_tiles_budget_and_not_blown_up() {
        // 256 pixels on a 108-pixel tile: 72 % of 108 is 78, so the icon covers
        // 78x78 and sits fifteen pixels in. This is what the jumbo image list is
        // for — the picture is shrunk, never magnified.
        let side = 108;
        let source = 256;
        let mut icon = image::RgbaImage::new(source, source);
        for (x, y, pixel) in icon.enumerate_pixels_mut() {
            // A diagonal edge: white above the line, black below it.
            let lit = x + y < source;
            let value = if lit { 0xff } else { 0x00 };
            *pixel = image::Rgba([value, value, value, 0xff]);
        }

        let canvas = compose_icon(&icon, side);
        assert_eq!(canvas.dimensions(), (side, side));

        // The icon area is the budget, centred, and the margin is untouched.
        let budget = 78;
        let margin = (side - budget) / 2;
        let mut left = side;
        let mut top = side;
        let mut right = 0;
        let mut bottom = 0;
        for (x, y, pixel) in canvas.enumerate_pixels() {
            if pixel[0] == 0 && pixel[1] == 0 && pixel[2] == 0 {
                continue;
            }
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
        assert_eq!(
            (left, top, right, bottom),
            (margin, margin, margin + budget - 1, margin + budget - 1),
            "the icon covers exactly the 72 % budget, centred"
        );

        // And it is not a block copy. A nearest-neighbour resample samples one
        // source pixel at a time, so a black-and-white source can only ever
        // produce black and white. A real filter averages across the diagonal
        // and leaves intermediate values along it: this one leaves a blended
        // pixel in every row the diagonal crosses — 78 of them here, one per
        // pixel of the icon area's side — and a nearest-neighbour resample
        // leaves none.
        let blended = canvas
            .pixels()
            .filter(|pixel| pixel[0] > 8 && pixel[0] < 247)
            .count();
        assert!(
            blended >= (budget / 2) as usize,
            "a diagonal edge must come out blended, not stepped: {blended} intermediate pixels"
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
        cache.prepare(Some(spec), &[face(&action)], None);

        assert_eq!(cache.artwork(Some(&action)), Artwork::Glyph);
        assert!(cache.frame(&action).is_some(), "the initial is a picture");
        let tile = cache
            .tile(&action, None)
            .expect("the deck receives the initial");
        assert!(tile.len() <= spec.max_bytes(), "{} bytes", tile.len());
    }

    #[test]
    fn a_binding_that_is_gone_takes_its_picture_with_it() {
        // The cache is the set of things the deck can run now, not a history of
        // everything it has ever run.
        let first = launch(r"C:\Apps\one.exe");
        let second = Action::keys("ctrl+m").expect("a valid combination");
        let mut cache = super::IconCache::new();

        cache.prepare(None, &[face(&first)], None);
        assert!(
            cache.frame(&first).is_some(),
            "a bound program has a picture"
        );

        cache.prepare(None, &[face(&second)], None);
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
    fn a_url_whose_site_has_no_icon_falls_back_to_its_initial() {
        // The ordinary case on the open web: a site that answers nothing at any
        // of the paths an icon is published on. The key is an initial, never a
        // blank tile.
        let cache = super::IconCache::new();
        let action = Action::OpenUrl {
            url: "https://example.com/".to_string(),
        };

        assert_eq!(cache.artwork(Some(&action)), Artwork::Glyph);
        assert!(cache.frame(&action).is_none());
        assert!(!cache.has_site("example.com"));
    }

    #[test]
    fn a_site_icon_the_cache_holds_is_what_the_key_wears() {
        // What a completed fetch does: the picture goes in once, and the key
        // then shows it — in the window and on the deck — instead of the letter.
        let spec = deck_api::ImageSpec::new(72, 72, 4_096, deck_api::ImageFormat::Jpeg);
        let mut cache = super::IconCache::new();
        cache.prepare(Some(spec), &[], None);

        let picture = image::RgbaImage::from_pixel(32, 32, image::Rgba([0x91, 0x45, 0xFF, 0xFF]));
        cache.finish_site("twitch.tv", Some(&picture));

        let action = Action::OpenUrl {
            url: "https://twitch.tv/dashboard".to_string(),
        };
        assert_eq!(cache.artwork(Some(&action)), Artwork::Icon);
        assert!(cache.frame(&action).is_some(), "the tile has a picture");
        let tile = cache
            .tile(&action, None)
            .expect("the deck gets the same picture");
        assert!(tile.len() <= spec.max_bytes(), "{} bytes", tile.len());
        assert!(cache.has_site("twitch.tv"));
    }

    #[test]
    fn a_site_with_no_icon_is_asked_once_and_remembered_as_a_miss() {
        // Offline, or a site with nothing at that path: the answer is the
        // initial, and the next pass over the bindings must not ask again.
        let mut cache = super::IconCache::new();

        assert!(cache.begin_site("example.com"), "the first pass asks");
        assert!(
            !cache.begin_site("example.com"),
            "the request is in flight: no second one"
        );
        cache.finish_site("example.com", None);

        assert!(!cache.begin_site("example.com"), "a miss is an answer");
        assert!(!cache.has_site("example.com"));
    }

    #[test]
    fn each_host_keeps_its_own_picture() {
        // A rebind to another site must not take the first site's icon with it,
        // and must not ask for a picture that is already in the cache.
        let mut cache = super::IconCache::new();
        let picture = image::RgbaImage::from_pixel(16, 16, image::Rgba([0x00, 0x80, 0x00, 0xFF]));
        cache.finish_site("twitch.tv", Some(&picture));
        cache.finish_site("github.com", None);

        let first = Action::OpenUrl {
            url: "https://twitch.tv/".to_string(),
        };
        let second = Action::OpenUrl {
            url: "https://github.com/".to_string(),
        };
        assert_eq!(cache.artwork(Some(&first)), Artwork::Icon);
        assert_eq!(cache.artwork(Some(&second)), Artwork::Glyph);
        assert!(!cache.begin_site("github.com"), "asked and answered");
        assert!(cache.begin_site("wikipedia.org"), "a new host is fetched");
    }

    #[test]
    fn a_site_icon_that_is_gone_takes_its_picture_with_it() {
        // The cache is the set of sites the deck can reach now: a key rebound
        // to a program does not keep the site's picture alive.
        let mut cache = super::IconCache::new();
        let picture = image::RgbaImage::from_pixel(16, 16, image::Rgba([0x11, 0x22, 0x33, 0xFF]));
        cache.finish_site("twitch.tv", Some(&picture));

        cache.prepare(None, &[face(&launch(r"C:\Apps\brave.exe"))], None);

        assert!(!cache.has_site("twitch.tv"));
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
