//! Throwaway directories for the tests that need real files on disk.
//!
//! Only compiled into the test binary: nothing here is part of the crate's
//! contract, and a test that leaves a folder behind in the user's temporary
//! directory is a test that accumulates rubbish.

#![cfg(test)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A directory under the system's temporary folder, removed when it is dropped.
pub(crate) struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    /// Creates an empty directory whose name no other test shares.
    ///
    /// The process id keeps two `cargo test` runs apart, and the counter keeps
    /// two tests in the same run apart.
    pub(crate) fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ss550-tray-{label}-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self { path }
    }

    /// The directory itself.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Creates a file in the directory and answers its path.
    pub(crate) fn file(&self, name: &str, contents: &[u8]) -> PathBuf {
        let path = self.path.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a scratch subdirectory");
        }
        std::fs::write(&path, contents).expect("a scratch file");
        path
    }

    /// Creates a folder in the directory and answers its path.
    pub(crate) fn dir(&self, name: &str) -> PathBuf {
        let path = self.path.join(name);
        std::fs::create_dir_all(&path).expect("a scratch subdirectory");
        path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
