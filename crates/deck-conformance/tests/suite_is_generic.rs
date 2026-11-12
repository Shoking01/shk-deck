//! Guard: the suite names no device and assumes no key count.
//!
//! The suite exists to be run against decks that share nothing but the seam.
//! The moment it mentions one device, or bakes in one deck's shape, it stops
//! being a conformance suite and becomes that device's test suite wearing a
//! general name.
//!
//! The two things guarded against are spelled in pieces so this file, which
//! has to name them, is scanned like every other file and does not trip over
//! itself.

use std::fs;
use std::path::{Path, PathBuf};

/// The device name the suite must never contain.
fn device_name() -> String {
    ["ss", "550"].concat()
}

/// The key count the suite must never assume.
fn key_count() -> String {
    ["1", "5"].concat()
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

/// Every number written out in the text, digit run by digit run.
fn numbers_in(text: &str) -> impl Iterator<Item = &str> {
    text.split(|character: char| !character.is_ascii_digit())
        .filter(|number| !number.is_empty())
}

#[test]
fn the_suite_never_names_a_device() {
    let root = crate_root();
    let mut files = Vec::new();
    crate_files(&root, &mut files);

    assert!(!files.is_empty(), "the scan found no files at all");

    let needle = device_name();
    for file in files {
        let Some(text) = text_of(&file) else {
            continue;
        };
        assert!(
            !text.to_lowercase().contains(&needle),
            "{} names a device: the suite must stay generic",
            file.display()
        );
    }
}

#[test]
fn the_suite_never_assumes_a_key_count() {
    let root = crate_root();
    let mut files = Vec::new();
    crate_files(&root, &mut files);

    assert!(!files.is_empty(), "the scan found no files at all");

    let needle = key_count();
    for file in files {
        let Some(text) = text_of(&file) else {
            continue;
        };
        assert!(
            !numbers_in(&text).any(|number| number == needle),
            "{} assumes a key count",
            file.display()
        );
    }
}
