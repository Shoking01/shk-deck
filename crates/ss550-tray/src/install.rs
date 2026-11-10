//! Where the executable lives when the controller is installed.
//!
//! "Start with Windows" must not point at whatever binary happened to be
//! running: a `target\debug` build disappears on the next `cargo clean`, and
//! the logon entry would then point at nothing while still looking enabled.
//! Installing is a copy into a folder of its own, so the entry always names a
//! path the user is not expected to move:
//!
//! ```text
//! %LOCALAPPDATA%\Programs\ss550\ss550-gui.exe
//! ```
//!
//! No administrator rights, no service, no registry-wide state; deleting the
//! folder is a complete uninstall.

use std::path::{Path, PathBuf};

use crate::TrayError;

/// The folder the controller copies itself into.
pub const PROGRAM_FOLDER: &str = "ss550";

/// The name the installed copy gets.
pub const EXECUTABLE_NAME: &str = "ss550-gui.exe";

/// The folder a per-user installation lives in.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), ss550_tray::TrayError> {
/// let dir = ss550_tray::install::install_dir()?;
/// println!("installed in {}", dir.display());
/// # Ok(())
/// # }
/// ```
#[cfg(windows)]
pub fn install_dir() -> Result<PathBuf, TrayError> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
        TrayError::Install("LOCALAPPDATA is not set, so there is no per-user folder".into())
    })?;
    Ok(PathBuf::from(local).join("Programs").join(PROGRAM_FOLDER))
}

/// The executable a logon entry should point at.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), ss550_tray::TrayError> {
/// let exe = ss550_tray::install::installed_executable()?;
/// assert!(exe.ends_with("ss550-gui.exe"));
/// # Ok(())
/// # }
/// ```
pub fn installed_executable() -> Result<PathBuf, TrayError> {
    Ok(install_dir()?.join(EXECUTABLE_NAME))
}

/// Copies the running executable into the install folder and returns where it
/// landed.
///
/// When the program already runs from that folder there is nothing to copy,
/// so flipping the switch twice never fights the file that is in use.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), ss550_tray::TrayError> {
/// let running = std::env::current_exe().map_err(|error| {
///     ss550_tray::TrayError::Install(error.to_string())
/// })?;
/// let installed = ss550_tray::install::ensure_installed(&running)?;
/// println!("the controller now lives at {}", installed.display());
/// # Ok(())
/// # }
/// ```
pub fn ensure_installed(running: &Path) -> Result<PathBuf, TrayError> {
    let target = installed_executable()?;
    copy_into(&target, running)?;
    Ok(target)
}

/// Copies `running` to `target`, creating its folder.
fn copy_into(target: &Path, running: &Path) -> Result<(), TrayError> {
    if is_same_file(target, running) {
        return Ok(());
    }
    if let Some(folder) = target.parent() {
        std::fs::create_dir_all(folder).map_err(|error| {
            TrayError::Install(format!("could not create {}: {error}", folder.display()))
        })?;
    }
    std::fs::copy(running, target).map_err(|error| {
        TrayError::Install(format!(
            "could not copy {} to {}: {error}",
            running.display(),
            target.display()
        ))
    })?;
    Ok(())
}

/// Whether both paths name the same existing file.
fn is_same_file(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// A per-user install folder is a Windows idea.
#[cfg(not(windows))]
pub fn install_dir() -> Result<PathBuf, TrayError> {
    Err(TrayError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn the_install_folder_is_a_programs_folder_in_the_user_profile() {
        let dir = install_dir().expect("this test runs on a Windows machine with a profile");
        assert!(dir.ends_with(Path::new("Programs").join(PROGRAM_FOLDER)));
        let local = std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA is set");
        assert!(dir.starts_with(PathBuf::from(local)));
    }

    #[test]
    #[cfg(windows)]
    fn the_installed_executable_is_named() {
        let exe = installed_executable().expect("the install folder is known");
        assert_eq!(
            exe.file_name().and_then(|name| name.to_str()),
            Some(EXECUTABLE_NAME)
        );
    }

    #[test]
    fn installing_copies_the_bytes_and_creates_the_folder() {
        let root = std::env::temp_dir().join(format!("ss550-install-{}", std::process::id()));
        let source = root.join("source.exe");
        let target = root.join("nested").join("copy.exe");
        std::fs::create_dir_all(&root).expect("the scratch folder can be created");
        std::fs::write(&source, b"deck").expect("the source can be written");

        copy_into(&target, &source).expect("the copy succeeds");

        assert_eq!(
            std::fs::read(&target).expect("the copy is readable"),
            b"deck"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn copying_a_file_onto_itself_is_not_an_error() {
        let root = std::env::temp_dir().join(format!("ss550-inplace-{}", std::process::id()));
        let target = root.join("itself.exe");
        std::fs::create_dir_all(&root).expect("the scratch folder can be created");
        std::fs::write(&target, b"deck").expect("the file can be written");

        copy_into(&target, &target).expect("nothing has to move");
        assert_eq!(std::fs::read(&target).expect("the file is intact"), b"deck");
        std::fs::remove_dir_all(&root).ok();
    }
}
