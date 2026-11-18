//! What a `.lnk` points at.
//!
//! Windows draws a shortcut as its own object: a little arrow in the corner, and
//! — for a shortcut with no icon of its own — a generic sheet of paper. The
//! shell answers about the file it is *given*, so asking it about a `.lnk`
//! answers about the shortcut, not about the program behind it. A key bound to a
//! Start Menu shortcut would then wear Windows' shortcut arrow instead of the
//! application's mark, which is exactly what the user sees and does not want.
//!
//! Resolving the link first is the whole fix: one COM object per link,
//! `IShellLinkW` loaded out of the file through `IPersistFile`, and
//! `IShellLinkW::GetPath` for the target. Both interfaces are released before
//! this module returns, so a caller may call [`resolve`] from any thread — the
//! UI thread, a worker with no COM at all, or one already in a different
//! apartment.
//!
//! ## Why the apartment is entered and never left
//!
//! COM is initialized on a thread the first time it resolves a link, and it is
//! **not** torn down again. That is not laziness, it is a crash that was
//! measured: reading a program's icon — `SHGetFileInfoW`, which is what
//! [`crate::appicon`] does right after this module resolves the link — hands the
//! work to shell threads that keep running after the call returns. A balanced
//! `CoUninitialize` decrements the module's reference count and unloads
//! `thumbcache.dll` while those threads are still inside it, and the process dies
//! with an access violation in `thumbcache.dll_unloaded` — at random, only when
//! two threads extracted an icon at the same time. Leaving the apartment up for
//! the life of the thread is what a long-lived Windows application does anyway,
//! and it costs one initialized thread.
//!
//! ## Why the walk is bounded
//!
//! A shortcut to a shortcut is legal, and a pair that point at each other is a
//! cycle no filesystem check would catch: both files exist. [`MAX_HOPS`] is what
//! keeps the walk finite.

use std::path::{Path, PathBuf};

/// How many links a chain may be followed before the walk gives up.
const MAX_HOPS: usize = 8;

/// Whether `path` names a shell shortcut rather than the program itself.
pub(crate) fn is_shortcut(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
}

/// The file `path` points at, when it is a shortcut that resolves.
///
/// `None` for anything that is not a shortcut, for a link the shell cannot read,
/// and on a platform that has no shortcuts at all. A link whose target has been
/// deleted still answers: the stored path is what `GetPath` returns, and whether
/// a missing target is acceptable is the caller's question, not this one's.
pub(crate) fn resolve(path: &Path) -> Option<PathBuf> {
    platform::resolve(path)
}

/// The path whose icon stands for `path`: the target of a shortcut, or `path`.
///
/// The answer is never `None`. A link that does not resolve — a stale shortcut,
/// a file that is not a link at all — falls back to itself, because the shell
/// still has a picture for it and a key that runs something should show
/// something.
pub(crate) fn icon_source(path: &Path) -> PathBuf {
    icon_source_with(path, resolve)
}

/// COM, entered on this thread the first time anything here needs it.
///
/// The icon reader calls this before it asks for one of the shell's image
/// lists, which are COM objects. Entered once per thread and never torn down:
/// the module documentation has the crash a balanced `CoUninitialize` caused.
/// `None` means this thread has no COM and the shell cannot be asked.
#[cfg(windows)]
pub(crate) fn enter_apartment() -> Option<()> {
    platform::enter()
}

/// Writes a shortcut at `link` that starts `target`.
///
/// The icon the entry shows is the program's own, so an installed copy and the
/// entry in the Start Menu are the same picture. A working directory is written
/// too: a program started from a launcher otherwise inherits one, and anything
/// it resolves relative to the working directory would land somewhere else.
///
/// # Errors
///
/// The shell's own sentence when the link cannot be written — no COM on this
/// thread, no permission in the folder, or a path the shell refuses.
#[cfg(windows)]
pub(crate) fn create(
    link: &Path,
    target: &Path,
    working_dir: &Path,
    description: &str,
) -> Result<(), String> {
    platform::create(link, target, working_dir, description)
}

/// Everywhere but Windows there is no shell to ask.
#[cfg(not(windows))]
pub(crate) fn create(
    _link: &Path,
    _target: &Path,
    _working_dir: &Path,
    _description: &str,
) -> Result<(), String> {
    Err("shortcuts are a Windows feature".to_string())
}

/// [`icon_source`] with the resolution handed in, so the walk is testable.
///
/// The only thing this does that needs the machine is `Path::exists`: a target
/// that is gone is not a target, and following it would answer with the icon of
/// a file nobody can start.
fn icon_source_with(path: &Path, resolve: impl Fn(&Path) -> Option<PathBuf>) -> PathBuf {
    let mut current = path.to_path_buf();
    for _ in 0..MAX_HOPS {
        if !is_shortcut(&current) {
            return current;
        }
        match resolve(&current) {
            Some(target) if target.exists() => current = target,
            _ => return current,
        }
    }
    current
}

/// Everything that needs COM.
#[cfg(windows)]
mod platform {
    use std::cell::Cell;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use windows::core::{IUnknown, Interface as _, PCWSTR};
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    /// The longest path `GetPath` is asked to fill.
    ///
    /// Well past `MAX_PATH`: a target that does not fit is a target nothing can
    /// start, and the shell truncates into the buffer rather than overrunning
    /// it.
    const PATH_CHARS: usize = 1024;

    /// COM, initialized on this thread the first time it is needed.
    ///
    /// Entered once and never left: see the module documentation for the crash a
    /// balanced `CoUninitialize` caused. `RPC_E_CHANGED_MODE` is not a failure —
    /// the thread is already in another apartment, and a shell link is an
    /// in-process object that works in either.
    pub(super) fn enter() -> Option<()> {
        thread_local! {
            /// `None` until the first call, then whether this thread has COM.
            static APARTMENT: Cell<Option<bool>> = const { Cell::new(None) };
        }

        APARTMENT.with(|slot| match slot.get() {
            Some(entered) => entered.then_some(()),
            None => {
                // SAFETY: no reserved value is passed and the apartment model is
                // the one a shell object expects.
                let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
                let entered = result.is_ok() || result == RPC_E_CHANGED_MODE;
                slot.set(Some(entered));
                entered.then_some(())
            }
        })
    }

    /// The target of one shortcut, or `None` when it cannot be read.
    pub(super) fn resolve(path: &Path) -> Option<PathBuf> {
        // The apartment outlives this call on purpose: the shell keeps working
        // after `GetPath` returns, and tearing COM down under it is the crash
        // this module exists to avoid.
        enter()?;

        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // SAFETY: the class id is a constant, there is no outer object, and the
        // interface is released by its own `Drop` before this scope ends.
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None::<&IUnknown>, CLSCTX_INPROC_SERVER) }
                .ok()?;
        // `cast` is safe in this version of the crate: it asks the object for one
        // of its own interfaces and answers an error rather than a bad pointer.
        let file: IPersistFile = link.cast().ok()?;

        // SAFETY: `wide` is a live, nul-terminated string, the mode is read-only,
        // and the object is not written back to disk.
        unsafe { file.Load(PCWSTR(wide.as_ptr()), STGM_READ) }.ok()?;

        let mut buffer = [0u16; PATH_CHARS];
        let mut data = WIN32_FIND_DATAW::default();
        // SAFETY: the buffer is a live array whose length is passed with it, and
        // the find-data record is writable and never read back.
        unsafe { link.GetPath(&mut buffer, &mut data, 0) }.ok()?;

        // Both interfaces are released before this function answers, on every
        // path: a `?` above drops them on the way out.
        drop(file);
        drop(link);

        let end = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        if end == 0 {
            // A link to a shell folder or a URL stores no path at all. There is
            // nothing to start, so there is nothing to answer with.
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(&buffer[..end])))
    }

    /// Writes a new link that starts `target`.
    ///
    /// The wide buffers live in this frame and outlive every call that is handed
    /// their pointers, which is the whole safety argument here.
    pub(super) fn create(
        link: &Path,
        target: &Path,
        working_dir: &Path,
        description: &str,
    ) -> Result<(), String> {
        // The apartment outlives this call, as it does for `resolve`.
        enter().ok_or_else(|| "COM could not be entered on this thread".to_string())?;

        let wide = |text: &std::ffi::OsStr| -> Vec<u16> {
            text.encode_wide().chain(std::iter::once(0)).collect()
        };
        let target_wide = wide(target.as_os_str());
        let dir_wide = wide(working_dir.as_os_str());
        let description_wide = wide(std::ffi::OsStr::new(description));
        let link_wide = wide(link.as_os_str());

        // SAFETY: every pointer passed below comes from a live wide buffer in
        // this frame, and the shell writes the file it is told to write. Both
        // interfaces are released when the function returns.
        unsafe {
            let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|error| error.to_string())?;
            shell
                .SetPath(PCWSTR(target_wide.as_ptr()))
                .map_err(|error| error.to_string())?;
            shell
                .SetWorkingDirectory(PCWSTR(dir_wide.as_ptr()))
                .map_err(|error| error.to_string())?;
            shell
                .SetDescription(PCWSTR(description_wide.as_ptr()))
                .map_err(|error| error.to_string())?;
            // The entry wears the program's own icon.
            shell
                .SetIconLocation(PCWSTR(target_wide.as_ptr()), 0)
                .map_err(|error| error.to_string())?;
            let file: IPersistFile = shell.cast().map_err(|error| error.to_string())?;
            file.Save(PCWSTR(link_wide.as_ptr()), true)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

/// Everywhere but Windows there are no shortcuts to follow.
#[cfg(not(windows))]
mod platform {
    use std::path::{Path, PathBuf};

    pub(super) fn resolve(_path: &Path) -> Option<PathBuf> {
        None
    }
}

/// A real `.lnk`, written with the same object that reads one back.
///
/// Both the icon and the scan tests need a shortcut whose target is a program
/// this build can name, and there is no such file in the repository: it is
/// written on demand into a scratch directory.
#[cfg(all(test, windows))]
pub(crate) mod fixture {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::{IUnknown, Interface as _, PCWSTR};
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    /// Writes a shortcut at `link` pointing at `target`.
    ///
    /// Answers whether the shell wrote it. A test that cannot build its fixture
    /// has nothing to assert, so the answer is checked rather than unwrapped.
    pub(crate) fn write_shortcut(link: &Path, target: &Path) -> bool {
        // The same apartment the reader uses, entered the same way and left up
        // for the same reason.
        if super::platform::enter().is_none() {
            return false;
        }

        (|| {
            // SAFETY: the class id is a constant and the interface is released
            // by its own `Drop` at the end of this closure.
            let link_object: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None::<&IUnknown>, CLSCTX_INPROC_SERVER) }
                    .ok()?;
            let wide_target: Vec<u16> = target
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            // SAFETY: `wide_target` is a live, nul-terminated string.
            unsafe { link_object.SetPath(PCWSTR(wide_target.as_ptr())) }.ok()?;

            let file: IPersistFile = link_object.cast().ok()?;
            let wide_link: Vec<u16> = link
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            // SAFETY: `wide_link` is a live, nul-terminated path to a file in a
            // directory this test owns.
            unsafe { file.Save(PCWSTR(wide_link.as_ptr()), true) }.ok()?;
            Some(())
        })()
        .is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::Path;

    use super::{icon_source_with, is_shortcut, MAX_HOPS};
    use crate::scratch::ScratchDir;

    /// The writer and the reader agree: a link this build writes starts the
    /// program it was given.
    ///
    /// This is the one part of "install it as an application" that can be
    /// proven without touching the user's Start Menu: the same COM call, into a
    /// scratch folder, read back by the resolver the icon code already uses.
    #[cfg(windows)]
    #[test]
    fn a_written_shortcut_starts_the_program_it_names() {
        let scratch = ScratchDir::new("written-link");
        let target = scratch.path().join("Deck Controller.exe");
        std::fs::write(&target, b"a path is all this test needs").expect("the target");
        let link = scratch.path().join("Deck Controller.lnk");

        super::create(&link, &target, scratch.path(), "a test entry")
            .expect("the shell writes the link");

        assert!(link.exists(), "the entry is on disk");
        assert_eq!(
            super::resolve(&link),
            Some(target),
            "and it starts the program it was given"
        );
    }

    #[test]
    fn only_a_link_extension_is_a_shortcut() {
        assert!(is_shortcut(Path::new(r"C:\Start Menu\Brave.lnk")));
        // Windows does not care about the case of an extension, and neither
        // does this: a `.LNK` copied from an archive is still a shortcut.
        assert!(is_shortcut(Path::new("Brave.LNK")));
        assert!(!is_shortcut(Path::new(r"C:\Windows\notepad.exe")));
        assert!(!is_shortcut(Path::new(r"C:\Start Menu\Brave")));
    }

    #[test]
    fn a_program_is_its_own_icon_source_and_is_never_resolved() {
        // The resolver is never asked about a file that is not a link: a second
        // lookup would be a second COM object for an answer that cannot change.
        let asked = Cell::new(0);
        let program = Path::new(r"C:\Windows\notepad.exe");

        let source = icon_source_with(program, |_| {
            asked.set(asked.get() + 1);
            None
        });

        assert_eq!(source, program);
        assert_eq!(asked.get(), 0);
    }

    #[test]
    fn a_shortcut_answers_with_the_icon_of_its_target() {
        let scratch = ScratchDir::new("icon-source");
        let link = scratch.file("Brave.lnk", b"");
        let target = std::env::current_exe().expect("the test binary has a path");

        let source = icon_source_with(&link, |_| Some(target.clone()));

        assert_eq!(
            source, target,
            "a key bound to the shortcut must show the program, not the arrow"
        );
    }

    #[test]
    fn a_shortcut_whose_target_is_gone_still_answers_for_itself() {
        let scratch = ScratchDir::new("stale-link");
        let link = scratch.file("Gone.lnk", b"");
        let missing = scratch.path().join("never-installed.exe");

        let source = icon_source_with(&link, |_| Some(missing.clone()));

        assert_eq!(
            source, link,
            "a stale shortcut is not a reason to answer with nothing: the shell \
             still has the link's own picture"
        );
    }

    #[test]
    fn a_link_that_does_not_resolve_answers_for_itself() {
        let scratch = ScratchDir::new("unreadable-link");
        let link = scratch.file("Broken.lnk", b"not a shortcut");

        let source = icon_source_with(&link, |_| None);

        assert_eq!(source, link);
    }

    #[test]
    fn a_chain_of_shortcuts_is_followed_to_the_program() {
        let scratch = ScratchDir::new("chain");
        let first = scratch.file("First.lnk", b"");
        let second = scratch.file("Second.lnk", b"");
        let program = std::env::current_exe().expect("the test binary has a path");

        let source = icon_source_with(&first, |path| {
            if path == first {
                Some(second.clone())
            } else if path == second {
                Some(program.clone())
            } else {
                None
            }
        });

        assert_eq!(source, program);
    }

    #[test]
    fn a_cycle_of_shortcuts_stops_instead_of_walking_forever() {
        // Two links that point at each other both exist, so nothing but the hop
        // cap ends the walk.
        let scratch = ScratchDir::new("cycle");
        let first = scratch.file("Loop.lnk", b"");
        let second = scratch.file("Other.lnk", b"");
        let asked = Cell::new(0usize);

        let source = icon_source_with(&first, |path| {
            asked.set(asked.get() + 1);
            match path == first {
                true => Some(second.clone()),
                false => Some(first.clone()),
            }
        });

        assert_eq!(asked.get(), MAX_HOPS, "the cap is what ends the walk");
        assert!(source == first || source == second);
    }
}
