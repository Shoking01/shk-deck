//! Guard 1: the seam names no device.
//!
//! The whole point of `deck-api` is to be the part of the code that does not
//! know which deck it is talking to. The day a device's name appears here,
//! the seam has leaked and a second model would have to work around it.

use std::fs;
use std::path::{Path, PathBuf};

fn source_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).expect("the crate's source directory is readable");
    for entry in entries {
        let path = entry.expect("a readable directory entry").path();
        if path.is_dir() {
            source_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn the_seam_never_names_a_device() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    source_files(&root, &mut files);

    assert!(!files.is_empty(), "the scan found no source files at all");

    for file in files {
        let text = fs::read_to_string(&file).expect("a readable source file");
        let lowered = text.to_lowercase();
        assert!(
            !lowered.contains("ss550"),
            "{} names a device: the seam must stay generic",
            file.display()
        );
    }
}
