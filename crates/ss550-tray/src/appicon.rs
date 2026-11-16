//! The icon Windows shows for a program, as pixels.
//!
//! A key bound to a program draws that program's icon, so the picture has to
//! come out of the file itself. It lives in the executable's resources — or in
//! the resources of the target a shortcut points at — and the only component
//! that can ask Windows for it is this crate, because it is the one allowed
//! `unsafe`.
//!
//! ## Why the link is resolved first
//!
//! `SHGetFileInfoW` answers about the file it is **given**, and a `.lnk` is a
//! file in its own right: ask about the shortcut and the shell hands back the
//! shortcut's icon — the little arrow in the corner, and a generic sheet of
//! paper when the link has no icon of its own. A key bound to a Start Menu
//! shortcut then wears Windows' arrow instead of the application's mark.
//!
//! So a shortcut is resolved to its target first — [`crate::shortcut`] — and the
//! shell is asked about the program. That is the difference between "the icon of
//! this file" and "the icon of the thing this file starts", and it is the second
//! one a key needs. `SHGFI_LINKOVERLAY` is never requested: it is the flag that
//! *adds* the arrow, and it is exactly what must not be drawn.
//!
//! ## Size
//!
//! The icon is **32×32**. That is what `SHGFI_LARGEICON` means, and it is the
//! picture Explorer draws in a file list. The deck's tile is larger, so the
//! renderer scales the picture up; the alternative — `SHGetImageList` with
//! `SHIL_EXTRALARGE` — is a COM image list and a shell-malloc'd pointer for
//! sixteen more pixels a side.
//!
//! ## What comes back
//!
//! Straight (not premultiplied) RGBA, row-major, top row first. The icon handle
//! is destroyed before this function returns, so no `HICON` and no `HBITMAP`
//! ever leaves this module. A path that does not exist, an icon with no pixels
//! at all, and a monochrome icon with no colour bitmap are `None`, and the
//! window draws the key's initial instead. A shortcut whose target is gone is
//! not a failure: the link itself is asked about instead, so the key shows the
//! picture the shell has for that file rather than falling back for no reason.
//!
//! # Example
//!
//! ```no_run
//! use std::path::Path;
//!
//! if let Some(icon) = ss550_tray::appicon::program_icon(Path::new(r"C:\Windows\notepad.exe")) {
//!     assert_eq!(icon.width(), 32);
//!     assert_eq!(icon.rgba().len(), 32 * 32 * 4);
//! }
//! ```

use std::path::Path;

use crate::shortcut;

/// The side of the icon this crate asks Windows for, in pixels.
///
/// `SHGFI_LARGEICON` is the shell's 32×32 icon. Stated here so the tests and the
/// window can both say what they expect instead of repeating the number.
pub const ICON_SIDE: u32 = 32;

/// An icon as straight RGBA, row-major, top row first.
///
/// "Straight" means not premultiplied: a pixel's colour is the colour the icon
/// has wherever it is opaque, and whatever the file holds where it is not. A
/// consumer that composites onto a background does the multiply itself, once,
/// with the background it actually has.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IconPixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl IconPixels {
    /// The icon's width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The icon's height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, four bytes each, row by row from the top.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// The pixel at `(x, y)`, or `None` when the point is outside the icon.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = ((y * self.width + x) * 4) as usize;
        let pixel = self.rgba.get(offset..offset + 4)?;
        Some([pixel[0], pixel[1], pixel[2], pixel[3]])
    }

    /// Whether the icon is entirely transparent, and therefore has no picture.
    ///
    /// An empty icon is worse than no icon: it paints a blank tile where the
    /// window would otherwise draw the key's initial.
    pub fn is_blank(&self) -> bool {
        self.rgba.chunks_exact(4).all(|pixel| pixel[3] == 0)
    }
}

/// The icon Windows shows for `path`, or `None` when there is none to read.
///
/// `path` may be an executable or a shortcut to one. A shortcut is resolved
/// first, and the icon read is the one of the program it starts: asking the
/// shell about the `.lnk` itself would answer with the shortcut's icon, arrow
/// overlay and all. A path that does not exist and an icon with no pixels answer
/// `None`; a shortcut whose target is gone answers with the icon the shell has
/// for the link, which is something, rather than nothing.
pub fn program_icon(path: &Path) -> Option<IconPixels> {
    // The one step that has to happen before the shell is asked: which file the
    // picture is about.
    let source = shortcut::icon_source(path);
    platform::program_icon(&source)
}

/// Straight RGBA from a top-down DIB whose pixels are B, G, R, A.
///
/// `alpha`, when given, replaces the fourth byte: an icon without an alpha
/// channel carries its shape in a separate monochrome mask.
fn rgba_from_bgra(bgra: &[u8], alpha: Option<&[u8]>) -> Vec<u8> {
    let count = bgra.len() / 4;
    let mut rgba = Vec::with_capacity(count * 4);
    for (index, pixel) in bgra.chunks_exact(4).enumerate() {
        let a = match alpha {
            Some(mask) => mask.get(index).copied().unwrap_or(0),
            None => pixel[3],
        };
        // The DIB is blue-first and the rest of the project is red-first.
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], a]);
    }
    rgba
}

/// Whether a 32-bit DIB carries an alpha channel at all.
///
/// A 24-bit icon read back as 32-bit pixels comes out with a zero fourth byte
/// everywhere: the channel exists in the buffer and means nothing. Such an icon
/// takes its shape from the mask instead.
fn has_alpha(bgra: &[u8]) -> bool {
    bgra.chunks_exact(4).any(|pixel| pixel[3] != 0)
}

/// The alpha each pixel takes from a 1-bit AND mask.
///
/// A monochrome mask is one bit per pixel, each row padded to a 32-bit
/// boundary, and a set bit means "transparent" — the mask is what the icon's
/// colour bitmap is *not*.
fn mask_alpha(mask: &[u8], width: u32, height: u32) -> Vec<u8> {
    let stride = row_bytes(width, 1);
    let mut alpha = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let offset = (y * stride + x / 8) as usize;
            // A mask that ran out means "no shape known", which is transparent:
            // an invisible icon falls back to the key's initial, while an opaque
            // one would paint a rectangle nobody asked for.
            let byte = mask.get(offset).copied().unwrap_or(0xFF);
            let bit = byte >> (7 - (x % 8));
            alpha.push(if bit & 1 == 0 { 0xFF } else { 0x00 });
        }
    }
    alpha
}

/// How many bytes one row of a DIB of this width and depth occupies.
///
/// Every row is padded to a 32-bit boundary, which is why a 5-pixel-wide mask
/// still costs four bytes a row.
fn row_bytes(width: u32, bits: u16) -> u32 {
    let bits = width.saturating_mul(u32::from(bits));
    bits.div_ceil(32).saturating_mul(4)
}

/// Everything that needs Win32.
#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, RGBQUAD,
    };
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

    use super::{has_alpha, mask_alpha, rgba_from_bgra, IconPixels};

    /// Asks the shell for the icon of `path` and reads it out.
    pub(super) fn program_icon(path: &Path) -> Option<IconPixels> {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut info = SHFILEINFOW::default();
        // SAFETY: `wide` is a live, nul-terminated UTF-16 string, `info` is a
        // writable record of the size passed with it, and the flags ask for an
        // icon the caller owns. A failed call answers zero and leaves `info`
        // untouched, which is why the handle is checked before it is used.
        //
        // `SHGFI_LINKOVERLAY` is never among the flags: it is what draws the
        // shortcut arrow, and the caller resolved the link precisely so that
        // arrow would not be in the picture.
        let found = unsafe {
            SHGetFileInfoW(
                PCWSTR(wide.as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES::default(),
                Some(&mut info),
                size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            )
        };
        if found == 0 || info.hIcon.is_invalid() {
            return None;
        }

        // The handle belongs to this call: `SHGFI_ICON` hands over an icon the
        // caller must destroy, and every path out of `read` runs before the
        // destroy below.
        let icon = info.hIcon;
        let pixels = read(icon);
        // SAFETY: the handle came from the call above, has not been destroyed,
        // and is not used again.
        let _ = unsafe { DestroyIcon(icon) };
        pixels
    }

    /// The pixels of one icon handle.
    fn read(icon: HICON) -> Option<IconPixels> {
        let mut info = ICONINFO::default();
        // SAFETY: `icon` is a live icon handle and `info` is a writable record.
        unsafe { GetIconInfo(icon, &mut info) }.ok()?;

        let colour = info.hbmColor;
        let mask = info.hbmMask;
        let pixels = if colour.is_invalid() {
            // A monochrome icon has no colour bitmap. Its mask is a picture in
            // its own right, and reading it as one is a different job; the key
            // falls back to its initial instead of guessing.
            None
        } else {
            read_colour(colour, mask)
        };

        // Both bitmaps were created for this call and belong to it, whichever
        // path above produced the answer.
        if !colour.is_invalid() {
            // SAFETY: the bitmap came from `GetIconInfo`, is not selected into
            // any device context, and is not used again.
            let _ = unsafe { DeleteObject(colour.into()) };
        }
        if !mask.is_invalid() {
            // SAFETY: as above, for the mask.
            let _ = unsafe { DeleteObject(mask.into()) };
        }
        pixels
    }

    /// The pixels of an icon's colour bitmap.
    fn read_colour(colour: HBITMAP, mask: HBITMAP) -> Option<IconPixels> {
        let mut bitmap = BITMAP::default();
        // SAFETY: `colour` is a live bitmap and `bitmap` is a `BITMAP` of the
        // size passed with it.
        let written = unsafe {
            GetObjectW(
                colour.into(),
                size_of::<BITMAP>() as i32,
                Some(&mut bitmap as *mut BITMAP as *mut c_void),
            )
        };
        if written == 0 {
            return None;
        }

        let width = u32::try_from(bitmap.bmWidth).ok()?;
        let height = u32::try_from(bitmap.bmHeight).ok()?;
        if width == 0 || height == 0 {
            return None;
        }

        let bgra = dib_bits(colour, width, height, 32)?;
        // An icon without an alpha channel — an old 24-bit one, or one stored
        // as a DIB with an empty fourth byte — carries its shape in the mask.
        let alpha = if has_alpha(&bgra) {
            None
        } else {
            let bits = dib_bits(mask, width, height, 1)?;
            Some(mask_alpha(&bits, width, height))
        };

        Some(IconPixels {
            width,
            height,
            rgba: rgba_from_bgra(&bgra, alpha.as_deref()),
        })
    }

    /// One bitmap as a top-down DIB of `bits` bits per pixel.
    ///
    /// The negative height is what asks GDI for the rows top-down, which is the
    /// order every pixel buffer in this project speaks; without it the picture
    /// would arrive upside down and nothing downstream would know.
    fn dib_bits(bitmap: HBITMAP, width: u32, height: u32, bits: u16) -> Option<Vec<u8>> {
        if bitmap.is_invalid() {
            return None;
        }

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: i32::try_from(width).ok()?,
                biHeight: -i32::try_from(height).ok()?,
                biPlanes: 1,
                biBitCount: bits,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            bmiColors: [RGBQUAD::default()],
        };
        let mut pixels = vec![0u8; super::row_bytes(width, bits) as usize * height as usize];

        // SAFETY: a device context is only a scratch surface for `GetDIBits`,
        // which reads the bitmap and writes the buffer whose length matches the
        // header above.
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.is_invalid() {
            return None;
        }
        // SAFETY: the buffer is a live, correctly sized allocation and the
        // header describes it exactly.
        let lines = unsafe {
            GetDIBits(
                dc,
                bitmap,
                0,
                height,
                Some(pixels.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            )
        };
        // SAFETY: the context was created above and is not used again.
        let _ = unsafe { DeleteDC(dc) };

        if lines == 0 {
            return None;
        }
        Some(pixels)
    }
}

/// Everywhere but Windows there is no shell icon to read.
#[cfg(not(windows))]
mod platform {
    use std::path::Path;

    use super::IconPixels;

    pub(super) fn program_icon(_path: &Path) -> Option<IconPixels> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{has_alpha, mask_alpha, rgba_from_bgra, row_bytes};

    #[test]
    fn a_dib_row_is_padded_to_a_whole_number_of_words() {
        // One bit per pixel: five pixels still cost four bytes, because the
        // row is padded to a 32-bit boundary.
        assert_eq!(row_bytes(5, 1), 4);
        assert_eq!(row_bytes(33, 1), 8);
        // 32-bit pixels are already word-sized.
        assert_eq!(row_bytes(32, 32), 128);
        assert_eq!(row_bytes(0, 32), 0);
    }

    #[test]
    fn a_dib_pixel_comes_out_red_first() {
        // One pixel, blue-first with a half-transparent alpha: (B=0x03, G=0x02,
        // R=0x01, A=0x80). The rest of the project is red-first.
        let rgba = rgba_from_bgra(&[0x03, 0x02, 0x01, 0x80], None);
        assert_eq!(rgba, [0x01, 0x02, 0x03, 0x80]);
    }

    #[test]
    fn a_mask_replaces_an_alpha_channel_that_means_nothing() {
        let bgra = [0x10, 0x20, 0x30, 0x00, 0x40, 0x50, 0x60, 0x00];
        let alpha = [0xFF, 0x00];
        let rgba = rgba_from_bgra(&bgra, Some(&alpha));
        assert_eq!(rgba, [0x30, 0x20, 0x10, 0xFF, 0x60, 0x50, 0x40, 0x00]);
    }

    #[test]
    fn an_all_zero_fourth_byte_is_not_an_alpha_channel() {
        assert!(!has_alpha(&[0, 0, 0, 0, 1, 2, 3, 0]));
        assert!(has_alpha(&[0, 0, 0, 0, 1, 2, 3, 0xFF]));
    }

    #[test]
    fn a_mask_bit_set_means_the_pixel_is_transparent() {
        // A three-by-three mask, one bit per pixel, each row padded to a whole
        // word: 101 (transparent, opaque, transparent) is 0xA0, 010 is 0x40 and
        // 111 is 0xE0.
        let mask = [
            0xA0, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0xE0, 0x00, 0x00, 0x00,
        ];
        assert_eq!(
            mask_alpha(&mask, 3, 3),
            [0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn a_short_mask_leaves_the_rest_of_the_icon_transparent() {
        // A mask that ran out is not a licence to paint: the pixels it does not
        // cover are transparent, so the tile never gains a colour nobody chose.
        assert_eq!(mask_alpha(&[], 2, 1), [0x00, 0x00]);
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use std::path::Path;

    use super::{program_icon, ICON_SIDE};
    use crate::scratch::ScratchDir;
    use crate::shortcut::fixture::write_shortcut;

    #[test]
    fn a_program_answers_with_the_shell_icon_at_its_documented_size() {
        // The test binary is a real executable on disk, so the shell has an
        // icon for it even when the file carries none of its own: Windows falls
        // back to the generic application icon.
        let running = std::env::current_exe().expect("the test binary has a path");
        let icon = program_icon(&running).expect("an executable has an icon");

        assert_eq!(icon.width(), ICON_SIDE);
        assert_eq!(icon.height(), ICON_SIDE);
        assert_eq!(
            icon.rgba().len(),
            (ICON_SIDE * ICON_SIDE * 4) as usize,
            "four bytes per pixel, and no stride padding left in"
        );
        assert!(!icon.is_blank(), "an icon with no pixels is not an icon");
    }

    #[test]
    fn a_path_that_is_not_there_has_no_icon() {
        let missing = Path::new(r"C:\this\program\was\never\installed.exe");
        assert!(program_icon(missing).is_none());
    }

    #[test]
    fn a_shortcut_shows_the_icon_of_its_target_and_not_the_arrow() {
        // The bug this pins down: the shell answers about the file it is given,
        // and for a `.lnk` that picture is the shortcut — arrow overlay and all.
        // Resolving the link first makes the two paths answer with one picture,
        // pixel for pixel.
        let running = std::env::current_exe().expect("the test binary has a path");
        let scratch = ScratchDir::new("shortcut-icon");
        let link = scratch.path().join("Shortcut.lnk");
        if !write_shortcut(&link, &running) {
            // No shell, no fixture, nothing to compare.
            return;
        }

        let from_link = program_icon(&link).expect("a shortcut answers with its target's icon");
        let from_target = program_icon(&running).expect("the program has an icon");

        assert_eq!(
            from_link, from_target,
            "a key bound to the shortcut must wear the program's mark, not Windows' arrow"
        );
    }

    #[test]
    fn a_shortcut_whose_target_is_gone_still_answers_with_a_picture() {
        // A Start Menu shortcut outlives the program it names. That is not a
        // reason for the key to fall back to an initial: the shell still has a
        // picture for the link.
        let scratch = ScratchDir::new("stale-shortcut-icon");
        let link = scratch.path().join("Gone.lnk");
        let missing = scratch.path().join("never-installed.exe");
        if !write_shortcut(&link, &missing) {
            return;
        }

        let icon = program_icon(&link).expect("the shell has a picture for the link itself");
        assert!(!icon.is_blank());
    }
}
