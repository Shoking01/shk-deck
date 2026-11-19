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
/// A name the library already holds is **not** a failure: the file the user
/// picked is, by its name, the picture the library already has, and refusing it
/// would tell them "no" for choosing something they can see in their own folder.
/// It answers [`Imported::AlreadyHeld`] so the window can say what happened and
/// show it anyway; the bytes already there are left alone.
///
/// # Errors
///
/// The engine's own sentence, unchanged, for every other refusal — the name is
/// unusable, the file is not a file, or it is over the size limit. It is what the
/// status line shows, and it already names the problem in the user's words.
pub(crate) fn import(library: &AssetLibrary, path: &Path) -> Result<Imported, String> {
    match library.import(path) {
        Ok(asset) => Ok(Imported::New(asset.name)),
        Err(ss550_engine::AssetError::Exists(name)) => {
            let name = AssetName::parse(name).map_err(|error| error.to_string())?;
            Ok(Imported::AlreadyHeld(name))
        }
        Err(error) => Err(error.to_string()),
    }
}

/// What an import did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Imported {
    /// The file was copied in under this name.
    New(AssetName),
    /// The library already had a file under this name; nothing was copied.
    AlreadyHeld(AssetName),
}

#[cfg(test)]
mod tests {
    use super::{import, Imported};
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

        let imported = import(&library, &source).expect("the file is imported");

        let Imported::New(name) = imported else {
            panic!("a new file is a new name");
        };
        assert_eq!(name.as_str(), "live.png");
        assert!(
            library.resolve(&name).is_file(),
            "the picture the key will name has to be in the library"
        );
    }

    #[test]
    fn a_name_the_library_already_holds_is_shown_not_refused() {
        // The library here holds thousands of pictures the user dropped in, so
        // picking one of them again is the normal case, not an error: the key
        // should show the picture they chose. Nothing is overwritten.
        let scratch = scratch("exists");
        let source = scratch.join("live.png");
        fs::write(&source, b"first").expect("a file to import");
        let library = AssetLibrary::new(scratch.join("assets"));
        let Imported::New(first) = import(&library, &source).expect("the first import") else {
            panic!("the first import is a new name");
        };

        let imported = import(&library, &source).expect("the second import is not an error");

        assert_eq!(imported, Imported::AlreadyHeld(first.clone()));
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
