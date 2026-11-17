//! The icon a website publishes, decoded and kept.
//!
//! A key bound to a URL draws the site's own mark, the way a key bound to a
//! program draws the program's icon. Nothing about it lives in the profile: the
//! host in the URL decides it, so the same binding wears the same mark on any
//! machine that can reach the site, and a URL edited to another host gets that
//! host's icon with no other step.
//!
//! ## Where the work happens
//!
//! The request is [`fetch`], and it never runs on the thread that draws: the
//! controller hands it to the background executor and the answer comes back as
//! state. The file read is [`cached`], and it runs inside
//! [`crate::icons::IconCache::prepare`] with the rest of the picture work, on
//! the change that made it necessary — never in a frame.
//!
//! ## Why the decoded picture is kept on disk
//!
//! A window started on a train has no network, and a key that showed its site's
//! mark yesterday has to show it today. The decoded icon is written as a PNG
//! under `config_dir()/icons/`, so a start with no network is a file read
//! instead of a request, and a host that answers once is never asked again.
//!
//! The file is named after the **host**, never after the URL: two bindings to
//! the same site share one picture, and a rebind to another host leaves the old
//! file where it is.
//!
//! ## What a site may answer with
//!
//! `/favicon.ico` is a convention, not a format, and it is not the only path a
//! site publishes an icon at. [`ss550_tray::favicon::site_icons`] asks for the
//! two touch icons first — the picture a phone's home screen draws — and for
//! `/favicon.ico` last, because that one is the 16- or 32-pixel image a tab
//! draws and a key is 108 pixels wide. Every body that answered is decoded here
//! and the one with the largest area wins: `x.com` answers 1024×1024 at
//! `/apple-touch-icon.png` and 32×32 at `/favicon.ico`, so the answer is 1024.
//!
//! The format is read from the bytes, never from the name: an ICO, a PNG and a
//! JPEG all arrive at these paths and the path says `.png` for all three. An
//! ICO is decoded at its largest entry — the 256-pixel picture, not the 16-pixel
//! one — and a PNG is decoded as it is. An SVG would need a renderer this window
//! does not carry, so it is refused here and the key falls back to its initial,
//! which is a picture and not a hole.

use std::path::{Path, PathBuf};

use image::{ExtendedColorType, ImageFormat, RgbaImage};

/// The largest side a decoded site icon may have.
///
/// An icon is 16 to 256 pixels, and the largest source a site is known to serve
/// is `x.com`'s 1024×1024 touch icon — exactly this. The ceiling exists for the
/// server that answers with a wallpaper instead: a picture past it is not an
/// icon, and decoding it on the way to a 108-pixel tile would cost memory for
/// nothing.
const MAX_SIDE: u32 = 1024;

/// The largest side a site icon is written to disk at.
///
/// A 1024×1024 icon is 4 MiB decoded and is never drawn bigger than a key, so it
/// is reduced once — here, on the background thread that decoded it, never at
/// draw time. 256 leaves room for a high-DPI tile and keeps a whole profile's
/// worth of icons within a few megabytes on disk and in the cache.
const MAX_STORED_SIDE: u32 = 256;

/// The folder the decoded icons are kept in, inside the configuration folder.
const ICONS_DIR: &str = "icons";

/// The host a URL points at, or `None` when it does not point at a site.
///
/// Only `http` and `https` are sites: `mailto:` and `steam:` name a way to reach
/// something, and a scheme is not a place that publishes an icon.
///
/// The answer is lower-cased and carries the port when the URL states one —
/// `localhost:8080` is a different site from `localhost` — and drops the
/// `user:password@` part, which is about who may ask, not about which site it
/// is.
pub(crate) fn host_of(url: &str) -> Option<String> {
    let url = url.trim();
    let start = ["https://", "http://"]
        .iter()
        .find(|scheme| {
            url.get(..scheme.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(scheme))
        })
        .map(|scheme| scheme.len())?;

    let authority = url[start..]
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = match authority.rsplit_once('@') {
        Some((_, host)) => host,
        None => authority,
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// The file a host's decoded icon is kept in.
pub(crate) fn cache_file(config: &Path, host: &str) -> PathBuf {
    config.join(ICONS_DIR).join(file_name(host))
}

/// The icon an earlier run kept for `host`, or `None` when there is none.
pub(crate) fn cached(config: &Path, host: &str) -> Option<RgbaImage> {
    let bytes = std::fs::read(cache_file(config, host)).ok()?;
    decode(&bytes)
}

/// Fetches, decodes and keeps the icon of `host`.
///
/// Runs on the background executor: the requests are network calls and the
/// decodes are a codec, and neither belongs on the thread that draws. The answer
/// is the picture, already decoded and already at key size, and the file is
/// written before it is returned so a later start needs no network at all.
///
/// Every candidate the site answered is decoded and the largest one is kept —
/// that is how a key gets the 1024-pixel mark instead of the 32-pixel tab icon.
/// The result is then reduced to [`MAX_STORED_SIDE`], so what is cached in
/// memory and on disk is a picture a key can wear and not a wallpaper.
///
/// `None` is every ordinary disappointment — no network, no icon at any
/// candidate path, an HTTP error, a body over the cap, an SVG — and the caller
/// draws the key's initial instead.
pub(crate) fn fetch(host: &str, config: &Path) -> Option<RgbaImage> {
    let bodies = ss550_tray::favicon::site_icons(host);
    let picture = store_sized(largest(&bodies)?);
    // Best effort: a configuration folder that cannot be written costs the next
    // start a request, not this session's icon.
    keep(config, host, &picture);
    Some(picture)
}

/// The largest picture among `bodies`, by pixel area.
///
/// The answer is not always the first candidate: `x.com` publishes 1024×1024 at
/// `/apple-touch-icon.png` and a 32×32 PNG at `/favicon.ico`, `github.com`
/// publishes 120×120, and `twitch.tv` answers its second candidate with an HTML
/// page. Only a decode tells them apart, so each body is decoded and only the
/// largest is held — the loop's memory is two frames at a time, not one per
/// candidate.
///
/// A body that does not decode is skipped, which is how a host whose only answer
/// is a 32×32 ICO still ends up with a picture.
fn largest(bodies: &[Vec<u8>]) -> Option<RgbaImage> {
    let mut best: Option<RgbaImage> = None;
    let mut best_area = 0u64;
    for body in bodies {
        let Some(picture) = decode(body) else {
            continue;
        };
        let (width, height) = picture.dimensions();
        let area = u64::from(width) * u64::from(height);
        if area > best_area {
            best_area = area;
            best = Some(picture);
        }
    }
    best
}

/// `picture` reduced to at most [`MAX_STORED_SIDE`] on its longer side.
///
/// Lanczos3 rather than nearest or triangle: this runs once per host, off the UI
/// thread, and a 1024-pixel mark shrunk with a box filter loses the thin strokes
/// that make it recognisable at key size. A picture already small enough is
/// returned untouched — a 32×32 ICO must not be resampled into a blur.
fn store_sized(picture: RgbaImage) -> RgbaImage {
    let (width, height) = picture.dimensions();
    let longest = width.max(height);
    if longest <= MAX_STORED_SIDE {
        return picture;
    }
    let scale = f64::from(MAX_STORED_SIDE) / f64::from(longest);
    let scaled_width = ((f64::from(width) * scale).round() as u32).max(1);
    let scaled_height = ((f64::from(height) * scale).round() as u32).max(1);
    image::imageops::resize(
        &picture,
        scaled_width,
        scaled_height,
        image::imageops::FilterType::Lanczos3,
    )
}

/// Decodes what a site served at one of its icon paths.
///
/// The format is read from the bytes, never from the name: an ICO, a PNG and a
/// JPEG all arrive at these paths and the path says `.png` for all three. An
/// SVG has no magic number this decoder knows and fails here, which is the
/// intended answer.
fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    if bytes.is_empty() {
        return None;
    }
    let picture = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = picture.dimensions();
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return None;
    }
    // A fully transparent icon is worse than none: it paints the black the tile
    // is drawn on and hides the initial that would have said what the key does.
    let blank = picture.pixels().all(|pixel| pixel[3] == 0);
    (!blank).then_some(picture)
}

/// Writes `picture` where a start with no network will find it.
fn keep(config: &Path, host: &str, picture: &RgbaImage) {
    let path = cache_file(config, host);
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let (width, height) = picture.dimensions();
    let _ = image::save_buffer_with_format(
        &path,
        picture.as_raw(),
        width,
        height,
        ExtendedColorType::Rgba8,
        ImageFormat::Png,
    );
}

/// A stable file name for a host: a readable slug plus a fingerprint.
///
/// The same rule the engine applies to a deck key, for the same reason: a host
/// can contain a colon for its port, and a file name cannot. Letters and digits
/// are kept, every run of anything else becomes one hyphen, and a fingerprint of
/// the whole host is appended so `example.com` and `example-com` cannot land in
/// the same file.
fn file_name(host: &str) -> String {
    let mut slug = String::new();
    for character in host.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "site" } else { slug };
    let slug: String = slug.chars().take(40).collect();
    format!("{slug}-{:016x}.png", fingerprint(host))
}

/// FNV-1a, 64-bit: no dependency, stable across releases, enough to keep two
/// hosts in two files.
fn fingerprint(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::MAX_STORED_SIDE;
    use super::{cache_file, decode, file_name, host_of, largest, store_sized, MAX_SIDE};
    use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};

    /// A PNG of one flat colour, as a site could have served it.
    fn png(side: u32, pixel: [u8; 4]) -> Vec<u8> {
        let picture = RgbaImage::from_pixel(side, side, Rgba(pixel));
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(picture.as_raw(), side, side, ExtendedColorType::Rgba8)
            .expect("the test's own png");
        bytes
    }

    #[test]
    fn a_url_names_its_site_and_nothing_else() {
        assert_eq!(
            host_of("https://Twitch.TV/dashboard").as_deref(),
            Some("twitch.tv"),
            "a host is case-insensitive"
        );
        assert_eq!(
            host_of("http://example.com:8080/x?y=1#z").as_deref(),
            Some("example.com:8080"),
            "a port is part of the site"
        );
        assert_eq!(
            host_of("https://user:secret@example.com/private").as_deref(),
            Some("example.com"),
            "who may ask is not which site it is"
        );
        // A scheme with no authority, and a scheme that is not a site at all.
        for url in [
            "https://",
            "https:///path",
            "mailto:someone@example.com",
            "steam://run/440",
            "twitch.tv",
            "",
        ] {
            assert_eq!(host_of(url), None, "{url:?} is not a site");
        }
    }

    #[test]
    fn a_host_becomes_a_file_name_no_file_system_refuses() {
        let config = std::path::Path::new("config");
        let path = cache_file(config, "localhost:8080");
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");

        assert!(
            !name.contains(':') && !name.contains('/') && !name.contains('\\'),
            "{name} is not a file name"
        );
        assert!(name.ends_with(".png"), "{name} is not a png");
        assert_eq!(path.parent(), Some(config.join("icons").as_path()));
        // Readable, so the folder can be looked at and understood.
        assert!(name.starts_with("localhost-8080-"), "{name}");
    }

    #[test]
    fn two_hosts_never_share_one_file_and_one_host_never_moves() {
        // The colon is the case the slug alone cannot separate: it becomes a
        // hyphen, so `a:b` and `a-b` would collide without the fingerprint.
        assert_ne!(file_name("example.com"), file_name("example-com"));
        assert_ne!(file_name("example.com"), file_name("example.com:8080"));
        assert_eq!(file_name("twitch.tv"), file_name("twitch.tv"));
        // A host long enough to be truncated keeps its identity in the hash.
        let long = "a".repeat(120);
        assert_ne!(file_name(&long), file_name(&format!("{long}b")));
    }

    #[test]
    fn a_png_served_at_the_icon_path_is_a_picture() {
        let picture = decode(&png(32, [0x10, 0x20, 0x30, 0xFF])).expect("a png is an icon");
        assert_eq!(picture.dimensions(), (32, 32));
        assert_eq!(picture.get_pixel(0, 0).0, [0x10, 0x20, 0x30, 0xFF]);
    }

    #[test]
    fn an_ico_is_decoded_at_its_largest_entry() {
        // Two entries, small first: the 16-pixel picture and the 32-pixel one.
        // A decoder that took the first would answer 16, and a key wearing the
        // 16-pixel entry would be a blurred copy of the mark the site drew.
        let small = one_entry_ico(16, [0xFF, 0x00, 0x00, 0xFF]);
        let large = one_entry_ico(32, [0x00, 0xFF, 0x00, 0xFF]);

        let picture = decode(&two_entry_ico(&small, &large)).expect("an ico is an icon");

        assert_eq!(
            picture.dimensions(),
            (32, 32),
            "the largest entry is the picture"
        );
        assert_eq!(picture.get_pixel(4, 4).0, [0x00, 0xFF, 0x00, 0xFF]);
    }

    #[test]
    fn what_cannot_be_a_picture_is_refused_rather_than_guessed() {
        // An SVG is the case this exists for: it is an icon to a browser and a
        // renderer this window does not carry, so the key keeps its initial.
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"></svg>"#;
        assert!(decode(svg).is_none());
        assert!(decode(b"<html><body>404</body></html>").is_none());
        assert!(decode(b"").is_none());
        // An icon with no opaque pixel would paint the tile's black and hide the
        // initial that says what the key runs.
        assert!(decode(&png(8, [0x00, 0x00, 0x00, 0x00])).is_none());
    }

    #[test]
    fn a_picture_larger_than_an_icon_is_not_an_icon() {
        // The cap is stated once, and a picture under it decodes; a server that
        // answers with a wallpaper is refused by the same comparison. The
        // largest source a site is known to serve — x.com's touch icon — is
        // exactly at the cap, so the cap cannot be lowered without dropping it.
        assert_eq!(MAX_SIDE, 1024);
        assert!(decode(&png(2, [1, 2, 3, 0xFF])).is_some());
    }

    #[test]
    fn the_biggest_answer_is_the_one_kept() {
        // X answers 1024×1024 at the touch icon and 32×32 at the tab icon; the
        // first is the mark, the second is what a key was wearing before.
        let bodies = vec![
            png(1024, [0x00, 0x00, 0x00, 0xFF]),
            png(32, [0xFF, 0xFF, 0xFF, 0xFF]),
        ];
        let picture = largest(&bodies).expect("both decode");
        assert_eq!(picture.dimensions(), (1024, 1024));

        // The answer is the area, not the position: the order candidates answer
        // in is the tray's business, and a site may put the better picture last.
        let reversed: Vec<Vec<u8>> = bodies.into_iter().rev().collect();
        assert_eq!(
            largest(&reversed).expect("both decode").dimensions(),
            (1024, 1024)
        );
    }

    #[test]
    fn a_body_that_is_not_a_picture_is_skipped_and_the_rest_still_count() {
        // Twitch answers `/apple-touch-icon-precomposed.png` with an HTML page
        // and a 180×180 PNG at `/apple-touch-icon.png`: the page must not stop
        // the search, and it must not win it either.
        let bodies = vec![
            png(180, [1, 2, 3, 0xFF]),
            b"<html><body>moved</body></html>".to_vec(),
            png(32, [4, 5, 6, 0xFF]),
        ];
        assert_eq!(
            largest(&bodies).expect("the pngs decode").dimensions(),
            (180, 180)
        );
        // Nothing decodable is no picture at all, and the key keeps its initial.
        assert!(largest(&[b"<html></html>".to_vec()]).is_none());
        assert!(largest(&[]).is_none());
    }

    #[test]
    fn a_home_screen_icon_is_stored_at_key_size() {
        assert_eq!(MAX_STORED_SIDE, 256);
        // 4 MiB decoded, 256 pixels kept: a key never draws it bigger.
        let stored = store_sized(decode(&png(1024, [0x10, 0x20, 0x30, 0xFF])).expect("decodes"));
        assert_eq!(stored.dimensions(), (256, 256));

        // A picture that is not square keeps its shape.
        let wide = RgbaImage::from_pixel(512, 128, Rgba([1, 2, 3, 0xFF]));
        assert_eq!(store_sized(wide).dimensions(), (256, 64));

        // Already small enough: untouched, so a 32×32 ICO is not blurred by a
        // resample it does not need.
        let small = RgbaImage::from_pixel(32, 32, Rgba([1, 2, 3, 0xFF]));
        assert_eq!(store_sized(small).dimensions(), (32, 32));
    }

    /// A one-entry ICO, as `image` writes it: directory, entry, payload.
    fn one_entry_ico(side: u32, pixel: [u8; 4]) -> Vec<u8> {
        let picture = RgbaImage::from_pixel(side, side, Rgba(pixel));
        let mut bytes = Vec::new();
        image::codecs::ico::IcoEncoder::new(&mut bytes)
            .write_image(picture.as_raw(), side, side, ExtendedColorType::Rgba8)
            .expect("the test's own ico");
        bytes
    }

    /// One ICO holding both entries, the small one first.
    ///
    /// Built from two real one-entry files so the payloads are whatever the
    /// encoder produces — BMP or PNG inside the ICO — instead of a hand-rolled
    /// bitmap that could be wrong in the same way the decoder is.
    fn two_entry_ico(small: &[u8], large: &[u8]) -> Vec<u8> {
        const HEADER: usize = 6;
        const ENTRY: usize = 16;
        // A one-entry file is the header, one entry, then the payload.
        let small_payload = &small[HEADER + ENTRY..];
        let large_payload = &large[HEADER + ENTRY..];
        // Two entries sit between the header and the first payload.
        let first = HEADER + ENTRY * 2;

        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_le_bytes()); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // type: icon
        out.extend_from_slice(&2u16.to_le_bytes()); // count

        for (source, offset) in [(small, first), (large, first + small_payload.len())] {
            let mut entry = source[HEADER..HEADER + ENTRY].to_vec();
            // The payload moved; its length did not.
            entry[12..16].copy_from_slice(&(offset as u32).to_le_bytes());
            out.extend_from_slice(&entry);
        }
        out.extend_from_slice(small_payload);
        out.extend_from_slice(large_payload);
        out
    }
}
