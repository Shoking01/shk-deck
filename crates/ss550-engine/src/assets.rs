//! The image library a key's appearance can name.
//!
//! The library is one directory the application owns, so a profile stores a
//! **file name** and never a path. Moving the configuration directory, or the
//! folder a picture was imported from, cannot leave a profile pointing at
//! nothing, and a hand-edited configuration file cannot make the window read a
//! file outside the library: [`AssetName`] is the only way to address one, and
//! it refuses anything that could escape.
//!
//! ## Example
//!
//! ```
//! use ss550_engine::{AssetLibrary, AssetName};
//!
//! let name = AssetName::parse("live.png")?;
//! assert_eq!(name.extension(), "png");
//!
//! let library = AssetLibrary::new("C:/config/assets");
//! assert_eq!(library.resolve(&name), library.root().join("live.png"));
//! # Ok::<(), ss550_engine::AssetError>(())
//! ```

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The directory, under the configuration directory, that holds the library.
pub const ASSETS_DIR: &str = "assets";

/// The file extensions the library accepts, lower case and without the dot.
pub const ACCEPTED_EXTENSIONS: [&str; 4] = ["bmp", "jpeg", "jpg", "png"];

/// The longest file name the library accepts, in bytes.
pub const MAX_NAME_BYTES: usize = 128;

/// The largest file the library will copy, in bytes.
pub const MAX_ASSET_BYTES: u64 = 8 * 1024 * 1024;

/// Stems Windows refuses as file names, whatever the extension.
///
/// `CON.png` is not a file on Windows: it is the console device. A library that
/// accepted the name would copy nothing and report success, so the name is
/// refused here instead.
const RESERVED_STEMS: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Everything that can go wrong around the asset library.
#[derive(Debug, Error)]
pub enum AssetError {
    /// The name is not usable as a library file name.
    #[error("{name:?} is not a usable asset name: {reason}")]
    InvalidName {
        /// The name that was refused.
        name: String,
        /// Why it was refused, in the user's words.
        reason: &'static str,
    },

    /// The library holds nothing under that name.
    #[error("the library holds no asset named {0:?}")]
    NotFound(String),

    /// The path is not a file that can be imported.
    #[error("{0} is not a file")]
    NotAFile(PathBuf),

    /// The file is larger than the library accepts.
    #[error("{path} is {bytes} bytes; the library accepts at most {max}")]
    TooLarge {
        /// The file that was refused.
        path: PathBuf,
        /// Its size.
        bytes: u64,
        /// The largest size the library accepts.
        max: u64,
    },

    /// The library already holds that name.
    #[error("the library already holds {0:?}")]
    Exists(String),

    /// The library could not be read or written.
    #[error("the asset library could not be used: {0}")]
    Io(#[from] io::Error),
}

/// A file name inside the library, guaranteed to stay inside it.
///
/// It is the only way to address an asset, so an invalid name cannot exist in
/// memory and no caller has to remember to validate one. Serde goes through
/// [`AssetName::parse`] as well, so a hand-edited configuration file is checked
/// on load rather than trusted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AssetName(String);

impl AssetName {
    /// Checks that `name` can address a file inside the library.
    ///
    /// # Errors
    ///
    /// [`AssetError::InvalidName`] names the reason: an empty name, a path
    /// separator, a directory reference, a leading dot, a control character, a
    /// Windows device stem, a missing or unaccepted extension, or a name longer
    /// than [`MAX_NAME_BYTES`].
    ///
    /// # Example
    ///
    /// ```
    /// use ss550_engine::AssetName;
    ///
    /// assert!(AssetName::parse("live.png").is_ok());
    /// assert!(AssetName::parse("../secrets.png").is_err());
    /// assert!(AssetName::parse("notes.txt").is_err());
    /// ```
    pub fn parse(name: impl Into<String>) -> Result<Self, AssetError> {
        let name = name.into();
        let reject = |reason: &'static str| AssetError::InvalidName {
            name: name.clone(),
            reason,
        };

        if name.is_empty() {
            return Err(reject("it is empty"));
        }
        if name.len() > MAX_NAME_BYTES {
            return Err(reject("it is longer than 128 bytes"));
        }
        if name.contains('/') || name.contains('\\') {
            return Err(reject("it contains a path separator"));
        }
        if name.contains(':') {
            return Err(reject("it contains a colon"));
        }
        if name == "." || name == ".." {
            return Err(reject("it is a directory reference"));
        }
        if name.starts_with('.') {
            return Err(reject("it starts with a dot"));
        }
        if name.chars().any(char::is_control) {
            return Err(reject("it contains a control character"));
        }

        let Some((stem, extension)) = name.rsplit_once('.') else {
            return Err(reject("it has no extension"));
        };
        if stem.trim().is_empty() {
            return Err(reject("it has no name before the extension"));
        }
        if RESERVED_STEMS
            .iter()
            .any(|reserved| stem.eq_ignore_ascii_case(reserved))
        {
            return Err(reject("it is a name Windows reserves for a device"));
        }
        let extension = extension.to_ascii_lowercase();
        if !ACCEPTED_EXTENSIONS.contains(&extension.as_str()) {
            return Err(reject("its extension is not an accepted image type"));
        }

        Ok(Self(name))
    }

    /// The file name as it is stored on disk.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The extension, exactly as the name spells it.
    pub fn extension(&self) -> &str {
        self.0
            .rsplit_once('.')
            .map_or("", |(_, extension)| extension)
    }

    /// The name without its extension.
    pub fn file_stem(&self) -> &str {
        self.0.rsplit_once('.').map_or(&self.0, |(stem, _)| stem)
    }
}

impl fmt::Display for AssetName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<AssetName> for String {
    fn from(name: AssetName) -> Self {
        name.0
    }
}

impl TryFrom<String> for AssetName {
    type Error = AssetError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        Self::parse(name)
    }
}

/// One file in the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// Its name inside the library.
    pub name: AssetName,
    /// Its size on disk.
    pub bytes: u64,
}

/// The directory of images a key can show.
///
/// It never decodes an image: the engine is deck-agnostic and carries no image
/// codec. Whether the bytes are a readable picture is the renderer's answer, and
/// the renderer knows the deck's [`ImageSpec`] the engine cannot see.
///
/// [`ImageSpec`]: https://docs.rs/deck-api
#[derive(Debug, Clone)]
pub struct AssetLibrary {
    root: PathBuf,
    max_bytes: u64,
}

impl AssetLibrary {
    /// Opens the library rooted at `root`, which need not exist yet.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            max_bytes: MAX_ASSET_BYTES,
        }
    }

    /// Opens the library that belongs to a configuration directory.
    pub fn at_config_dir(config_dir: &Path) -> Self {
        Self::new(config_dir.join(ASSETS_DIR))
    }

    /// Refuses files larger than `max_bytes` on import.
    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    /// The directory the library lives in.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Creates the directory if it is not there.
    pub fn ensure(&self) -> Result<(), AssetError> {
        fs::create_dir_all(&self.root)?;
        Ok(())
    }

    /// Every image in the library, sorted by name.
    ///
    /// A library that does not exist yet is empty, not an error: a fresh install
    /// has no images until the user adds one. Files the library cannot address —
    /// a subdirectory, a name with an unaccepted extension, a name that is not
    /// UTF-8 — are skipped instead of failing the whole listing, so one stray
    /// file cannot hide the rest.
    pub fn list(&self) -> Result<Vec<Asset>, AssetError> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(AssetError::Io(error)),
        };

        let mut assets = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(name) = AssetName::parse(name) else {
                continue;
            };
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            assets.push(Asset {
                name,
                bytes: metadata.len(),
            });
        }

        assets.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(assets)
    }

    /// The path an asset is stored at.
    ///
    /// It is always inside [`AssetLibrary::root`] because [`AssetName`] cannot
    /// hold a separator.
    pub fn resolve(&self, name: &AssetName) -> PathBuf {
        self.root.join(name.as_str())
    }

    /// Reads one asset's bytes.
    pub fn read(&self, name: &AssetName) -> Result<Vec<u8>, AssetError> {
        let path = self.resolve(name);
        fs::read(&path).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                AssetError::NotFound(name.as_str().to_owned())
            } else {
                AssetError::Io(error)
            }
        })
    }

    /// Copies `source` into the library, keeping its file name.
    ///
    /// # Errors
    ///
    /// Refuses a source that is not a file, a name the library cannot address, a
    /// file larger than the library accepts, and a name that already exists. An
    /// import never replaces an image the user already curated.
    pub fn import(&self, source: &Path) -> Result<Asset, AssetError> {
        let file_name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| AssetError::NotAFile(source.to_path_buf()))?;
        let name = AssetName::parse(file_name)?;
        self.import_as(source, name)
    }

    /// Copies `source` into the library under `name`.
    ///
    /// # Errors
    ///
    /// The same refusals as [`AssetLibrary::import`].
    pub fn import_as(&self, source: &Path, name: AssetName) -> Result<Asset, AssetError> {
        let metadata = match fs::metadata(source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(AssetError::NotAFile(source.to_path_buf()));
            }
            Err(error) => return Err(AssetError::Io(error)),
        };
        if !metadata.is_file() {
            return Err(AssetError::NotAFile(source.to_path_buf()));
        }
        if metadata.len() > self.max_bytes {
            return Err(AssetError::TooLarge {
                path: source.to_path_buf(),
                bytes: metadata.len(),
                max: self.max_bytes,
            });
        }

        self.ensure()?;
        let target = self.resolve(&name);
        if target.exists() {
            return Err(AssetError::Exists(name.as_str().to_owned()));
        }

        fs::copy(source, &target)?;
        let bytes = fs::metadata(&target).map_or(metadata.len(), |metadata| metadata.len());
        Ok(Asset { name, bytes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> AssetName {
        AssetName::parse(text).expect("a valid asset name")
    }

    #[test]
    fn a_plain_image_name_is_accepted() {
        for accepted in [
            "live.png",
            "my icon.png",
            "cam.jpeg",
            "x.jpg",
            "y.bmp",
            "UP.PNG",
        ] {
            assert!(
                AssetName::parse(accepted).is_ok(),
                "{accepted} should be accepted"
            );
        }
    }

    #[test]
    fn a_name_that_could_escape_the_library_is_refused() {
        for refused in [
            "",
            ".",
            "..",
            "../secrets.png",
            "..\\secrets.png",
            "a/b.png",
            "a\\b.png",
            "C:evil.png",
            ".hidden.png",
            "x.png\u{7}",
        ] {
            assert!(
                AssetName::parse(refused).is_err(),
                "{refused:?} should be refused"
            );
        }
    }

    #[test]
    fn a_name_without_an_accepted_extension_is_refused() {
        for refused in ["notes.txt", "noextension", "x.gif", "x.png.exe", "x."] {
            assert!(
                AssetName::parse(refused).is_err(),
                "{refused:?} should be refused"
            );
        }
    }

    #[test]
    fn a_windows_device_name_is_refused() {
        assert!(AssetName::parse("CON.png").is_err());
        assert!(AssetName::parse("com1.jpg").is_err());
        assert!(AssetName::parse("CONSOLE.png").is_ok());
    }

    #[test]
    fn an_over_long_name_is_refused() {
        let long = format!("{}.png", "a".repeat(MAX_NAME_BYTES));
        assert!(AssetName::parse(long).is_err());
    }

    #[test]
    fn the_parts_of_a_name_come_back_unchanged() {
        let parsed = name("Live Scene.PNG");
        assert_eq!(parsed.as_str(), "Live Scene.PNG");
        assert_eq!(parsed.extension(), "PNG");
        assert_eq!(parsed.file_stem(), "Live Scene");
    }

    #[test]
    fn an_invalid_name_cannot_be_deserialised() {
        let refused = serde_json::from_str::<AssetName>("\"../escape.png\"");
        assert!(refused.is_err());

        let accepted = serde_json::from_str::<AssetName>("\"live.png\"").expect("valid");
        assert_eq!(accepted.as_str(), "live.png");
    }

    #[test]
    fn a_library_that_does_not_exist_yet_is_empty() {
        let dir = tempfile::tempdir().expect("temp dir");
        let library = AssetLibrary::at_config_dir(dir.path());
        assert_eq!(library.list().expect("a missing library lists empty"), []);
        assert_eq!(library.root(), dir.path().join(ASSETS_DIR));
    }

    #[test]
    fn listing_skips_what_the_library_cannot_address() {
        let dir = tempfile::tempdir().expect("temp dir");
        let library = AssetLibrary::at_config_dir(dir.path());
        library.ensure().expect("create the library");
        fs::write(library.root().join("b.png"), b"image").expect("write");
        fs::write(library.root().join("a.jpg"), b"image").expect("write");
        fs::write(library.root().join("notes.txt"), b"text").expect("write");
        fs::create_dir(library.root().join("nested.png")).expect("create a directory");

        let listed = library.list().expect("list");
        let names: Vec<&str> = listed.iter().map(|asset| asset.name.as_str()).collect();
        assert_eq!(names, ["a.jpg", "b.png"]);
        assert_eq!(listed[0].bytes, 5);
    }

    #[test]
    fn importing_copies_the_bytes_and_keeps_the_name() {
        let source_dir = tempfile::tempdir().expect("temp dir");
        let source = source_dir.path().join("live.png");
        fs::write(&source, b"not really a png").expect("write");

        let target_dir = tempfile::tempdir().expect("temp dir");
        let library = AssetLibrary::at_config_dir(target_dir.path());
        let asset = library.import(&source).expect("import");

        assert_eq!(asset.name.as_str(), "live.png");
        assert_eq!(asset.bytes, 16);
        assert_eq!(
            fs::read(library.resolve(&asset.name)).expect("read back"),
            b"not really a png"
        );
    }

    #[test]
    fn importing_refuses_a_name_the_library_already_holds() {
        let source_dir = tempfile::tempdir().expect("temp dir");
        let source = source_dir.path().join("live.png");
        fs::write(&source, b"first").expect("write");

        let target_dir = tempfile::tempdir().expect("temp dir");
        let library = AssetLibrary::at_config_dir(target_dir.path());
        library.import(&source).expect("first import");

        let error = library.import(&source).expect_err("second import");
        assert!(matches!(error, AssetError::Exists(_)));
        assert_eq!(
            fs::read(library.root().join("live.png")).expect("unchanged"),
            b"first"
        );
    }

    #[test]
    fn importing_refuses_a_file_over_the_limit() {
        let source_dir = tempfile::tempdir().expect("temp dir");
        let source = source_dir.path().join("big.png");
        fs::write(&source, vec![0u8; 64]).expect("write");

        let target_dir = tempfile::tempdir().expect("temp dir");
        let library = AssetLibrary::at_config_dir(target_dir.path()).with_max_bytes(32);
        let error = library.import(&source).expect_err("over the limit");
        assert!(matches!(
            error,
            AssetError::TooLarge {
                bytes: 64,
                max: 32,
                ..
            }
        ));
    }

    #[test]
    fn importing_refuses_a_source_that_is_not_a_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let library = AssetLibrary::at_config_dir(dir.path());

        let missing = dir.path().join("nothing.png");
        assert!(matches!(
            library.import(&missing).expect_err("missing"),
            AssetError::NotAFile(_)
        ));

        // A directory whose name the library could address is refused for what
        // it is, not for its name.
        let nested = dir.path().join("nested.png");
        fs::create_dir(&nested).expect("create a directory");
        assert!(matches!(
            library.import(&nested).expect_err("a directory"),
            AssetError::NotAFile(_)
        ));

        // A path the library cannot even name is refused before it is opened.
        assert!(matches!(
            library.import(dir.path()).expect_err("an unnameable path"),
            AssetError::InvalidName { .. }
        ));
    }

    #[test]
    fn reading_an_absent_asset_says_so() {
        let dir = tempfile::tempdir().expect("temp dir");
        let library = AssetLibrary::at_config_dir(dir.path());
        let error = library.read(&name("gone.png")).expect_err("absent");
        assert!(matches!(error, AssetError::NotFound(_)));
    }

    #[test]
    fn a_resolved_path_stays_inside_the_library() {
        let library = AssetLibrary::new("C:/config/assets");
        let resolved = library.resolve(&name("live.png"));
        assert!(resolved.starts_with(library.root()));
        assert_eq!(resolved, PathBuf::from("C:/config/assets/live.png"));
    }
}
