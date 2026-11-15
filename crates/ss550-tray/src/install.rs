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

/// What comparing the running copy with the installed one found.
///
/// The sign-in entry names a file, not a build: a newer controller can be
/// running while Windows would still start last week's copy at logon. This is
/// the answer to "do the two name the same bytes?", so the app can keep the
/// installed copy current instead of leaving the user to notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refresh {
    /// The running binary is the installed one; nothing to do.
    RunningInstalled,
    /// The installed copy was replaced with the running binary.
    Updated(PathBuf),
    /// The installed copy is a different file and could not be replaced.
    ///
    /// The usual cause is that Windows already has it mapped: the copy the
    /// logon entry started is still running, so its image cannot be written.
    Blocked {
        /// The installed copy that is still the old bytes.
        installed: PathBuf,
        /// Why it could not be replaced, as the operating system worded it.
        reason: String,
    },
}

/// Brings the installed copy up to date with the binary that is running.
///
/// `running` is the executable the user started, which is normally a build
/// straight out of `target\release`. The copy Windows starts at logon is a
/// folder of its own (see [`install_dir`]), and nothing refreshes it on its
/// own, so a user who updates the controller keeps being started on the build
/// they installed first. Calling this on startup closes that gap: the next
/// logon runs what this session ran.
///
/// It never deletes anything and never installs on its own: when the sign-in
/// entry is off there is no copy to keep current, and the caller decides
/// whether to call it at all.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), ss550_tray::TrayError> {
/// let running = std::env::current_exe().map_err(|error| {
///     ss550_tray::TrayError::Install(error.to_string())
/// })?;
/// match ss550_tray::install::refresh_installed(&running)? {
///     ss550_tray::install::Refresh::Updated(path) => {
///         println!("the sign-in copy is now {}", path.display());
///     }
///     ss550_tray::install::Refresh::RunningInstalled => {}
///     ss550_tray::install::Refresh::Blocked { installed, reason } => {
///         println!("{} is still the old build: {reason}", installed.display());
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub fn refresh_installed(running: &Path) -> Result<Refresh, TrayError> {
    let installed = installed_executable()?;
    Ok(refresh_at(&installed, running))
}

/// [`refresh_installed`], with the installed path spelled out.
///
/// Split out so a test can point it at a scratch folder instead of the copy
/// Windows really starts, which the test run would otherwise overwrite.
fn refresh_at(installed: &Path, running: &Path) -> Refresh {
    if is_same_file(installed, running) {
        return Refresh::RunningInstalled;
    }
    if same_bytes(installed, running) {
        // Different folders, same bytes: the entry already starts this build.
        return Refresh::RunningInstalled;
    }
    match copy_into(installed, running) {
        Ok(()) => Refresh::Updated(installed.to_path_buf()),
        Err(TrayError::Install(reason)) => Refresh::Blocked {
            installed: installed.to_path_buf(),
            reason,
        },
        // `copy_into` only reports install failures, so anything else is a bug
        // in this crate, not a state the caller can act on.
        Err(other) => Refresh::Blocked {
            installed: installed.to_path_buf(),
            reason: other.to_string(),
        },
    }
}

/// Whether both paths hold the same bytes, reading as little as possible.
///
/// The lengths are compared first: a rebuild almost always changes the size of
/// the image, so the usual "already current" answer costs one metadata call per
/// file instead of reading thirty megabytes at startup.
fn same_bytes(left: &Path, right: &Path) -> bool {
    let (Ok(left_meta), Ok(right_meta)) = (std::fs::metadata(left), std::fs::metadata(right))
    else {
        return false;
    };
    if left_meta.len() != right_meta.len() {
        return false;
    }
    match (std::fs::read(left), std::fs::read(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
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

    #[test]
    fn an_older_installed_copy_is_replaced_by_the_one_that_runs() {
        let root = std::env::temp_dir().join(format!("ss550-refresh-{}", std::process::id()));
        let installed = root.join("installed.exe");
        let running = root.join("build").join("ss550-gui.exe");
        std::fs::create_dir_all(running.parent().expect("a parent")).expect("the build folder");
        std::fs::write(&installed, b"last week's build").expect("the old copy");
        std::fs::write(&running, b"this week's build").expect("the running copy");

        let outcome = refresh_at(&installed, &running);

        assert_eq!(outcome, Refresh::Updated(installed.clone()));
        assert_eq!(
            std::fs::read(&installed).expect("the installed copy is readable"),
            b"this week's build",
            "the sign-in entry must start what the user is running"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_installed_copy_of_the_same_bytes_is_left_alone() {
        let root = std::env::temp_dir().join(format!("ss550-same-{}", std::process::id()));
        let installed = root.join("installed.exe");
        let running = root.join("build.exe");
        std::fs::create_dir_all(&root).expect("the scratch folder can be created");
        std::fs::write(&installed, b"the same build").expect("the installed copy");
        std::fs::write(&running, b"the same build").expect("the running copy");

        assert_eq!(refresh_at(&installed, &running), Refresh::RunningInstalled);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_missing_installed_copy_is_created_rather_than_reported() {
        let root = std::env::temp_dir().join(format!("ss550-missing-{}", std::process::id()));
        let installed = root.join("nowhere").join("ss550-gui.exe");
        let running = root.join("ss550-gui.exe");
        std::fs::create_dir_all(&root).expect("the scratch folder can be created");
        std::fs::write(&running, b"the running build").expect("the running copy");

        let outcome = refresh_at(&installed, &running);

        assert_eq!(outcome, Refresh::Updated(installed.clone()));
        assert_eq!(
            std::fs::read(&installed).expect("the created copy"),
            b"the running build"
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
