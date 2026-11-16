//! Bringing a picture into the library, from the file the user picked.
//!
//! The dialog hands back a path anywhere on the machine — `Downloads`, a folder
//! on a stick, an icon pack with six thousand files in it — and a profile stores
//! a **name inside the library**, never a path. So a chosen file is copied in
//! first, and the key is then told the name the copy was given.
//!
//! ## Why the order is this order
//!
//! Import, then assign. The assignment writes a name into the profile, and the
//! profile has to name something the library holds: a key pointed at a file that
//! was never copied is a key that shows nothing the next time the window opens.
//! An import that fails — the file is not an image this build reads, the name is
//! already in the library, the file is larger than the library takes — therefore
//! ends the click, and the key keeps whatever it had.
//!
//! ## What is not here
//!
//! No dialog and no decoding. The dialog is [`ss550_tray::dialog`] and blocks
//! until the user answers; the decode is [`crate::render`], run by the
//! controller once the key names the picture. This module is the one step in
//! between, which is why it can be tested without a screen.

use std::path::Path;

use ss550_engine::{AssetLibrary, AssetName};

/// Copies `path` into `library` and answers the name the key should show.
///
/// # Errors
///
/// The engine's own sentence, unchanged. It is what the status line shows, and
/// it already says which of the refusals happened — the name is unusable, the
/// file is not a file, it is over the size limit, or the library already holds
/// that name — in the user's words rather than this crate's. Restating it here
/// would be a second copy of the rules that could drift from the first.
pub(crate) fn import(library: &AssetLibrary, path: &Path) -> Result<AssetName, String> {
    library
        .import(path)
        .map(|asset| asset.name)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::import;
    use ss550_engine::AssetLibrary;
    use std::fs;
    use std::path::PathBuf;

    /// A folder of this test's own, emptied before it is handed out.
    ///
    /// Under the process's temporary folder and named after the test, so two
    /// tests running at once cannot share one — the crate has no test-only
    /// dependency for this, and one does not need a library to be written for it.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ss550-gui-import-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a folder for the test");
        dir
    }

    #[test]
    fn a_picture_is_copied_in_and_the_key_is_told_its_name() {
        let scratch = scratch("copies");
        let source = scratch.join("live.png");
        fs::write(&source, b"pretend this is a png").expect("a file to import");
        let library = AssetLibrary::new(scratch.join("assets"));

        let name = import(&library, &source).expect("the file is imported");

        assert_eq!(name.as_str(), "live.png");
        assert!(
            library.resolve(&name).is_file(),
            "the picture the key will name has to be in the library"
        );
    }

    #[test]
    fn a_name_the_library_already_holds_is_refused_and_nothing_is_replaced() {
        // The dialog can hand back a file the library already has, and an import
        // never overwrites a picture the user curated. The engine's sentence is
        // the one the status line shows, so it is the one asserted here.
        let scratch = scratch("exists");
        let source = scratch.join("live.png");
        fs::write(&source, b"first").expect("a file to import");
        let library = AssetLibrary::new(scratch.join("assets"));
        let first = import(&library, &source).expect("the first import");

        let error = import(&library, &source).expect_err("the second import is refused");

        assert!(error.contains("already holds"), "{error}");
        assert_eq!(
            fs::read(library.resolve(&first)).expect("the library copy"),
            b"first",
            "the picture that was already there is untouched"
        );
    }

    #[test]
    fn a_file_the_library_cannot_address_never_reaches_it() {
        let scratch = scratch("name");
        let source = scratch.join("notes.txt");
        fs::write(&source, b"not a picture").expect("a file to import");
        let library = AssetLibrary::new(scratch.join("assets"));

        let error = import(&library, &source).expect_err("a text file is not a picture");

        assert!(error.contains("notes.txt"), "{error}");
        assert!(
            !library.root().join("notes.txt").exists(),
            "a refused import copies nothing"
        );
    }

    #[test]
    fn a_path_that_is_gone_is_refused_rather_than_created() {
        // The dialog makes the answer a file that exists, but the file can be
        // deleted between the answer and the import.
        let scratch = scratch("gone");
        let library = AssetLibrary::new(scratch.join("assets"));

        let error = import(&library, &scratch.join("never-was.png"))
            .expect_err("a missing file is not importable");

        assert!(error.contains("is not a file"), "{error}");
    }
}
