//! The applications installed on this PC, as the `RUN APP` editor lists them.
//!
//! The list is what the Start Menu holds: `%ProgramData%` (every user) and
//! `%APPDATA%` (this user), searched recursively for shortcuts and executables.
//! That is the same set Explorer's "All apps" shows, which is the set a person
//! means when they say "the applications installed on the PC". A registry
//! uninstall key is a different and much worse question — it lists components,
//! not things a key can start.
//!
//! ## What is skipped, and why
//!
//! A shortcut is resolved before it is listed, so a `.lnk` whose target has been
//! deleted is not offered: a key that runs it would open an error dialog. Two
//! shortcuts that resolve to the same program are one entry — the Start Menu
//! duplicates programs across folders, and the list is of applications, not of
//! files. A folder that cannot be read is skipped rather than reported: the scan
//! answers with what it found, and "nothing" is a valid answer.
//!
//! ## Cost
//!
//! Hundreds of shortcuts, one COM object each. The scan runs on whatever thread
//! calls it, so it is not something to do while drawing a frame: the window
//! calls it once, on the click that opens the picker, and keeps the answer.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::shortcut::{is_shortcut, resolve};

/// How deep a Start Menu tree may be before the walk stops descending.
///
/// Start Menu folders are two or three levels deep. The cap is not about them:
/// it is about a junction that points back up its own tree, which would
/// otherwise be an unbounded walk with no error to stop it.
const MAX_DEPTH: u8 = 8;

/// One application a key can be bound to.
///
/// The two paths are not the same thing, and both are kept. [`InstalledApp::shortcut`]
/// is what a key runs — the shortcut, so the shell starts it exactly as Explorer
/// would, with the working directory and arguments the user set up. [`InstalledApp::target`]
/// is what the shortcut points at, which is what the icon comes from and what
/// makes two entries the same application.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InstalledApp {
    /// What the Start Menu calls it: the shortcut's file stem, with no `.lnk`.
    name: String,
    /// The file to run: the shortcut when there is one, the executable otherwise.
    shortcut: PathBuf,
    /// The program behind it.
    target: PathBuf,
}

impl InstalledApp {
    /// The name the list shows, without the file extension.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The file a key should run.
    pub fn shortcut(&self) -> &Path {
        &self.shortcut
    }

    /// The program the shortcut points at, or the executable itself.
    pub fn target(&self) -> &Path {
        &self.target
    }
}

/// Every application the Start Menu of this machine offers.
///
/// Sorted by name, case-insensitively, with no duplicates. A machine whose Start
/// Menu is missing or unreadable answers with an empty list: there is nothing to
/// report, and a scan that failed is not an error the caller has to handle.
pub fn installed_apps() -> Vec<InstalledApp> {
    let mut found = Vec::new();
    for root in start_menu_roots() {
        collect(&root, 0, &mut found);
    }
    finish(found)
}

/// The two folders Windows keeps the Start Menu in.
///
/// Taken from the environment rather than from a known-folder call: both
/// variables are set for every interactive session, they are what the shell
/// itself reads, and a folder that is not there is simply skipped by the walk.
fn start_menu_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for variable in ["ProgramData", "APPDATA"] {
        let Some(base) = std::env::var_os(variable) else {
            continue;
        };
        roots.push(
            PathBuf::from(base)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    roots
}

/// Walks one Start Menu folder and adds every application it holds.
fn collect(dir: &Path, depth: u8, found: &mut Vec<InstalledApp>) {
    if depth > MAX_DEPTH {
        return;
    }
    // An unreadable folder is not a failure: the user's Start Menu is a
    // collection of folders Windows and installers own, and one of them refusing
    // to list must not take the other two hundred with it.
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect(&path, depth + 1, found);
            continue;
        }
        if !kind.is_file() {
            continue;
        }

        let Some(name) = application_name(&path) else {
            continue;
        };
        let Some(target) = target_of(&path) else {
            continue;
        };
        found.push(InstalledApp {
            name,
            shortcut: path,
            target,
        });
    }
}

/// The name a Start Menu entry is listed under, or `None` when it is not an
/// application at all.
///
/// The stem, not the file name: `Brave`, not `Brave.lnk`. A file called `.lnk`
/// has no stem and is not an application.
fn application_name(path: &Path) -> Option<String> {
    let extension = path.extension()?;
    let is_application =
        extension.eq_ignore_ascii_case("lnk") || extension.eq_ignore_ascii_case("exe");
    if !is_application {
        return None;
    }
    let stem = path.file_stem()?.to_string_lossy().trim().to_string();
    (!stem.is_empty()).then_some(stem)
}

/// The program a Start Menu entry starts, or `None` when it starts nothing.
///
/// A shortcut is resolved, and one whose target is gone is dropped: the Start
/// Menu keeps shortcuts for programs that have been uninstalled, and offering
/// them would fill the list with keys that open an error dialog.
fn target_of(path: &Path) -> Option<PathBuf> {
    let target = match is_shortcut(path) {
        true => resolve(path)?,
        false => path.to_path_buf(),
    };
    // `is_file`, not `exists`: a shortcut to a folder is a way to open a window,
    // not an application a key can run.
    target.is_file().then_some(target)
}

/// Drops duplicates and sorts what the walk found.
///
/// Two shortcuts that point at the same program are one application. The
/// shortest name wins, because the Start Menu's longer name is the one with the
/// folder in it — `Brave` and `Brave Software\Brave` are the same browser, and
/// the short one is what the user calls it. Equal lengths are decided
/// alphabetically so the answer does not depend on the order the folders were
/// read in.
fn finish(found: Vec<InstalledApp>) -> Vec<InstalledApp> {
    let mut by_target: BTreeMap<String, InstalledApp> = BTreeMap::new();
    for app in found {
        // Windows compares paths without regard to case, and so does this: the
        // same program reached through `%ProgramFiles%` and through a shortcut's
        // stored path is one program.
        let key = app.target.to_string_lossy().to_lowercase();
        match by_target.get(&key) {
            Some(existing) if !is_shorter(&app.name, &existing.name) => {}
            _ => {
                by_target.insert(key, app);
            }
        }
    }

    let mut apps: Vec<InstalledApp> = by_target.into_values().collect();
    apps.sort_by_cached_key(|app| app.name.to_lowercase());
    apps
}

/// Whether `candidate` should displace `existing` for the same target.
fn is_shorter(candidate: &str, existing: &str) -> bool {
    match candidate.chars().count().cmp(&existing.chars().count()) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Greater => false,
        std::cmp::Ordering::Equal => candidate < existing,
    }
}

#[cfg(test)]
mod tests {
    use super::{application_name, collect, finish, installed_apps, is_shorter, InstalledApp};
    use crate::scratch::ScratchDir;
    use std::path::PathBuf;

    fn app(name: &str, shortcut: &str, target: &str) -> InstalledApp {
        InstalledApp {
            name: name.to_string(),
            shortcut: PathBuf::from(shortcut),
            target: PathBuf::from(target),
        }
    }

    /// A path inside a Start Menu folder, built the way the host spells it.
    ///
    /// Not typed as `C:\Start Menu\...`: a backslash is a separator only on
    /// Windows, so on Linux and macOS those literals are one long file name and
    /// these tests would be asking a different question from the one they name.
    fn start_menu(name: &str) -> PathBuf {
        PathBuf::from("Start Menu").join(name)
    }

    #[test]
    fn the_name_is_the_stem_and_not_the_file_name() {
        assert_eq!(
            application_name(&start_menu("Brave.lnk")).as_deref(),
            Some("Brave")
        );
        assert_eq!(
            application_name(&start_menu("Notepad.EXE")).as_deref(),
            Some("Notepad"),
            "the extension's case does not change what it is"
        );
    }

    #[test]
    fn a_file_that_is_not_an_application_has_no_name() {
        assert_eq!(application_name(&start_menu("readme.txt")), None);
        assert_eq!(application_name(&start_menu(".lnk")), None);
        assert_eq!(application_name(&start_menu("Brave")), None);
    }

    #[test]
    fn a_folder_that_is_not_there_finds_nothing_and_does_not_panic() {
        let scratch = ScratchDir::new("missing-root");
        let missing = scratch.path().join("this-folder-was-never-there");
        let mut found = Vec::new();

        collect(&missing, 0, &mut found);

        assert!(found.is_empty());
    }

    #[test]
    fn only_applications_are_collected_from_a_folder() {
        let scratch = ScratchDir::new("collect");
        let root = scratch.dir("Programs");
        let tool = root.join("Tool.exe");
        std::fs::write(&tool, b"").expect("a file");
        std::fs::write(root.join("notes.txt"), b"").expect("a file");
        let nested = root.join("Vendor");
        std::fs::create_dir_all(&nested).expect("a folder");
        let nested_tool = nested.join("Nested.exe");
        std::fs::write(&nested_tool, b"").expect("a file");

        let mut found = Vec::new();
        collect(&root, 0, &mut found);

        let mut names: Vec<&str> = found.iter().map(|app| app.name()).collect();
        names.sort_unstable();
        assert_eq!(names, ["Nested", "Tool"], "the walk reaches into folders");
        // A plain executable is its own target: there is no link to follow.
        let tool_entry = found.iter().find(|app| app.name() == "Tool").expect("Tool");
        assert_eq!(tool_entry.shortcut(), tool);
        assert_eq!(tool_entry.target(), tool);
    }

    #[test]
    fn a_shortcut_that_cannot_be_resolved_is_not_listed() {
        // The bytes are not a shortcut at all, so the shell cannot read a target
        // out of them and the entry starts nothing.
        let scratch = ScratchDir::new("broken-lnk");
        let root = scratch.dir("Programs");
        std::fs::write(root.join("Broken.lnk"), b"not a shortcut").expect("a file");

        let mut found = Vec::new();
        collect(&root, 0, &mut found);

        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn two_shortcuts_for_one_program_are_one_application() {
        let found = vec![
            app(
                "Brave Software",
                r"C:\Start Menu\Vendor\Brave.lnk",
                r"C:\Program Files\Brave\brave.exe",
            ),
            app(
                "Brave",
                r"C:\Start Menu\Brave.lnk",
                r"C:\PROGRAM FILES\BRAVE\BRAVE.EXE",
            ),
        ];

        let apps = finish(found);

        assert_eq!(apps.len(), 1, "one program is one application");
        assert_eq!(apps[0].name(), "Brave", "the shortest name is the name");
        assert_eq!(
            apps[0].shortcut(),
            PathBuf::from(r"C:\Start Menu\Brave.lnk")
        );
    }

    #[test]
    fn the_list_is_sorted_without_regard_to_case() {
        let found = vec![
            app("zoom", "a.lnk", r"C:\zoom.exe"),
            app("Alpha", "b.lnk", r"C:\alpha.exe"),
            app("brave", "c.lnk", r"C:\brave.exe"),
        ];

        let names: Vec<String> = finish(found)
            .into_iter()
            .map(|app| app.name().to_string())
            .collect();

        assert_eq!(names, ["Alpha", "brave", "zoom"]);
    }

    #[test]
    fn a_shorter_name_wins_and_an_equal_one_is_decided_by_name() {
        assert!(is_shorter("Brave", "Brave Software"));
        assert!(!is_shorter("Brave Software", "Brave"));
        assert!(
            !is_shorter("Brave", "Brave"),
            "the same name keeps the entry that was found first"
        );
        assert!(is_shorter("Alpha", "Bravo"), "a tie is decided by the name");
        assert!(!is_shorter("Bravo", "Alpha"));
    }

    #[test]
    fn a_scan_never_panics_and_answers_with_a_list() {
        // The real scan of this machine. It is allowed to find nothing — a
        // machine with an empty Start Menu is not a failure — but every entry it
        // does find has to be an application that can be started.
        let apps = installed_apps();

        for app in &apps {
            assert!(!app.name().is_empty(), "{app:?}");
            assert!(
                !app.name().to_lowercase().ends_with(".lnk"),
                "the name is the stem, not the file name: {app:?}"
            );
            assert!(app.target().is_file(), "the target is gone: {app:?}");
        }
        let mut sorted: Vec<String> = apps.iter().map(|app| app.name().to_lowercase()).collect();
        let before = sorted.clone();
        sorted.sort();
        assert_eq!(before, sorted, "the list is sorted by name");
    }
}
