//! Guard: the engine names no device.
//!
//! The engine is the action layer: what a key does, and where the bindings
//! live. Which device the keys belong to is not its business — a deck with a
//! different shape must not need a different engine, and the only way to keep
//! that true is to make the coupling impossible instead of leaving it to
//! discipline.
//!
//! The guard covers the **whole crate**, the manifest, the sources and the
//! tests, because a device name in any of them is the same coupling. It also
//! covers the two conversions that made the driver part of the engine's
//! public shape, and the silent fallback that came with them: a key number
//! that cannot be turned into a physical key must be an error, never a guess
//! at the bottom-left one.
//!
//! The needles are spelled in pieces, so this file — which has to name what it
//! looks for — is scanned like every other file and does not trip over itself.

use std::fs;
use std::path::{Path, PathBuf};

/// The driver crate the engine must not depend on.
fn driver_crate() -> String {
    ["ss", "550", "-driver"].concat()
}

/// The driver's module path, which the engine must not name either.
fn driver_module() -> String {
    ["ss", "550", "_driver"].concat()
}

/// The key the engine must never fall back to.
fn fallback_key() -> String {
    ["Bottom", "Left"].concat()
}

/// The conversion from a key number to a physical key.
fn key_conversion() -> String {
    ["pub fn ", "key("].concat()
}

/// The conversion from a physical key to a key number.
fn reverse_key_conversion() -> String {
    ["pub fn ", "from_key("].concat()
}

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn crate_files(root: &Path, found: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(root).expect("the crate directory is readable");
    for entry in entries {
        let path = entry.expect("a readable directory entry").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            crate_files(&path, found);
        } else {
            found.push(path);
        }
    }
}

fn text_of(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    String::from_utf8(bytes).ok()
}

#[test]
fn the_engine_depends_on_no_device() {
    let root = crate_root();
    let mut files = Vec::new();
    crate_files(&root, &mut files);

    assert!(!files.is_empty(), "the scan found no files at all");

    let crate_name = driver_crate();
    let module_path = driver_module();
    let fallback = fallback_key();

    for file in files {
        let Some(text) = text_of(&file) else {
            continue;
        };
        assert!(
            !text.contains(&crate_name),
            "{} names the driver crate: the engine must stay device-free",
            file.display()
        );
        assert!(
            !text.contains(&module_path),
            "{} names the driver's module: the engine must stay device-free",
            file.display()
        );
        assert!(
            !text.contains(&fallback),
            "{} falls back to a physical key: a key number that cannot be \
             turned into one must be an error, never a guess",
            file.display()
        );
    }
}

#[test]
fn the_engine_exposes_no_key_conversion() {
    let root = crate_root();
    let mut files = Vec::new();
    crate_files(&root, &mut files);

    assert!(!files.is_empty(), "the scan found no files at all");

    let conversion = key_conversion();
    let reverse = reverse_key_conversion();

    for file in files {
        let Some(text) = text_of(&file) else {
            continue;
        };
        assert!(
            !text.contains(&conversion),
            "{} converts a key number into a physical key: the physical key \
             belongs to the device, and the engine does not know the device",
            file.display()
        );
        assert!(
            !text.contains(&reverse),
            "{} converts a physical key into a key number: the physical key \
             belongs to the device, and the engine does not know the device",
            file.display()
        );
    }
}
