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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::shortcut::{is_shortcut, resolve};

#[cfg(windows)]
mod windows_shell;

/// How deep a Start Menu tree may be before the walk stops descending.
///
/// Start Menu folders are two or three levels deep. The cap is not about them:
/// it is about a junction that points back up its own tree, which would
/// otherwise be an unbounded walk with no error to stop it.
const MAX_DEPTH: u8 = 8;

/// One application a key can be bound to.
///
/// Two kinds of application reach this list, and a key can start both:
///
/// - a **Start Menu entry** — a shortcut or a bare executable, started by running
///   the file, so the shell applies the working directory and arguments the
///   installer chose. [`InstalledApp::program`] is that file.
/// - a **packaged app** — Settings, Calculator, anything from the Store — which
///   has no file to run at all. Its identity is an AppUserModelId inside the
///   shell's `AppsFolder`, and the program that starts it is `explorer.exe` with
///   `shell:AppsFolder\<id>` as its only argument.
///
/// [`InstalledApp::target`] is what the icon comes from and what makes two
/// entries one application. For a packaged app it is the shell parsing name,
/// because there is no file behind it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InstalledApp {
    /// What the list calls it.
    name: String,
    /// The file a key runs.
    program: PathBuf,
    /// The arguments that name it. Empty for a Start Menu entry.
    arguments: Vec<String>,
    /// The program behind it, or the shell name of a packaged app.
    target: PathBuf,
    /// Whether it is started through the shell rather than by running a file.
    shell: bool,
    /// Whether it is a packaged app, named by an AppUserModelId.
    packaged: bool,
}

impl InstalledApp {
    /// A Start Menu entry: the file a key runs, and the program behind it.
    pub(crate) fn file(
        name: String,
        program: PathBuf,
        target: PathBuf,
        arguments: Vec<String>,
    ) -> Self {
        Self {
            name,
            program,
            arguments,
            target,
            shell: false,
            packaged: false,
        }
    }

    /// A packaged app, named by its AppUserModelId.
    ///
    /// Used only where the shell is asked for its own list, which is Windows;
    /// the attribute keeps the tests that cover it building everywhere.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn packaged(name: String, aumid: &str) -> Self {
        Self::shell(name, &format!("shell:AppsFolder\\{aumid}"), true)
    }

    /// An entry the shell starts from an address rather than from a file: a
    /// packaged app's `shell:AppsFolder\<id>`, or a registered URI such as
    /// `steam://rungameid/993090`.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn shell(name: String, address: &str, packaged: bool) -> Self {
        Self {
            name,
            program: explorer(),
            arguments: vec![address.to_string()],
            target: PathBuf::from(address),
            shell: true,
            packaged,
        }
    }

    /// The name the list shows, without the file extension.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The program a key should run.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// The arguments that name it, one per entry.
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    /// The program behind it, or the shell name of a packaged app.
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Whether this is a packaged app, started through the shell's `AppsFolder`.
    pub fn is_packaged(&self) -> bool {
        self.packaged
    }

    /// Whether the shell starts it from an address instead of by running a file.
    pub fn is_shell(&self) -> bool {
        self.shell
    }
}

/// The program that starts a packaged app.
///
/// `explorer.exe` is the shell's own front door: handed `shell:AppsFolder\<id>`
/// it resolves the id and activates whatever registered it, which is the only
/// way to start an app that has no executable of its own.
#[cfg_attr(not(windows), allow(dead_code))]
fn explorer() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Windows"))
        .join("explorer.exe")
}

/// Every application this machine offers a key.
///
/// The Start Menu first, then the packaged apps the shell keeps in
/// `AppsFolder` — Settings, Calculator, anything from the Store — which have no
/// shortcut to walk to. Sorted by name, case-insensitively, with no duplicates.
/// A machine whose Start Menu is missing or unreadable still answers with
/// whatever the shell knows: a scan that failed is not an error the caller has
/// to handle.
pub fn installed_apps() -> Vec<InstalledApp> {
    let mut found = Vec::new();
    for root in start_menu_roots() {
        collect(&root, 0, &mut found);
    }
    merge(finish(found), packaged_apps())
}

/// Adds the packaged apps to the Start Menu's list, once each.
///
/// The two sources overlap: `Settings` has a shortcut on some machines and only
/// an `AppsFolder` entry on others. The Start Menu entry wins, because it is the
/// one whose target the shell has already answered an icon for. A name is the
/// identity here and not the target: two packages are free to present the same
/// name, and two rows saying `Clock` would be a coin toss for the user.
fn merge(mut menu: Vec<InstalledApp>, packaged: Vec<InstalledApp>) -> Vec<InstalledApp> {
    let mut names: BTreeSet<String> = menu.iter().map(|app| app.name.to_lowercase()).collect();
    for app in packaged {
        if names.insert(app.name.to_lowercase()) {
            menu.push(app);
        }
    }
    menu.sort_by_cached_key(|app| app.name.to_lowercase());
    menu
}

/// The packaged apps the shell lists in `AppsFolder`, or nothing when it cannot
/// be read.
///
/// This is the one source that is not a folder on disk: a packaged app is a
/// registration, and the shell's `AppsFolder` is its index. Reading it is a COM
/// walk over shell items, which is why it lives in this crate.
#[cfg(windows)]
fn packaged_apps() -> Vec<InstalledApp> {
    windows_shell::apps()
}

/// Nothing to list: only Windows keeps packaged apps.
#[cfg(not(windows))]
fn packaged_apps() -> Vec<InstalledApp> {
    Vec::new()
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
        let Some((target, arguments)) = target_of(&path) else {
            continue;
        };
        found.push(InstalledApp::file(name, path, target, arguments));
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

/// The program a Start Menu entry starts and the arguments it starts it with,
/// or `None` when it starts nothing.
///
/// A shortcut is resolved, and one whose target is gone is dropped: the Start
/// Menu keeps shortcuts for programs that have been uninstalled, and offering
/// them would fill the list with keys that open an error dialog.
fn target_of(path: &Path) -> Option<(PathBuf, Vec<String>)> {
    if !is_shortcut(path) {
        return path.is_file().then(|| (path.to_path_buf(), Vec::new()));
    }
    let target = resolve(path)?;
    // `is_file`, not `exists`: a shortcut to a folder is a way to open a window,
    // not an application a key can run.
    if !target.is_file() {
        return None;
    }
    // A link that states none answers an empty list; a link that cannot be read
    // at all is still a program with no arguments, which is what most are.
    let arguments = crate::shortcut::arguments(path).unwrap_or_default();
    Some((target, arguments))
}

/// Drops duplicates and sorts what the walk found.
///
/// Two shortcuts that start the same program **the same way** are one
/// application: the Start Menu duplicates programs across folders, and the list
/// is of applications, not of files. The arguments are part of "the same way" —
/// Visual Studio's six developer prompts are one `cmd.exe` with six argument
/// lines, and collapsing them would leave five of them unreachable. The shortest
/// name wins, because the Start Menu's longer name is the one with the folder in
/// it — `Brave` and `Brave Software\Brave` are the same browser, and the short one
/// is what the user calls it. Equal lengths are decided alphabetically so the
/// answer does not depend on the order the folders were read in.
fn finish(found: Vec<InstalledApp>) -> Vec<InstalledApp> {
    let mut by_identity: BTreeMap<String, InstalledApp> = BTreeMap::new();
    for app in found {
        // Windows compares paths without regard to case, and so does this: the
        // same program reached through `%ProgramFiles%` and through a shortcut's
        // stored path is one program.
        let key = format!(
            "{}\u{1}{}",
            app.target.to_string_lossy().to_lowercase(),
            app.arguments.join("\u{1}").to_lowercase()
        );
        match by_identity.get(&key) {
            Some(existing) if !is_shorter(&app.name, &existing.name) => {}
            _ => {
                by_identity.insert(key, app);
            }
        }
    }

    let mut apps: Vec<InstalledApp> = by_identity.into_values().collect();
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
    use super::{
        application_name, collect, finish, installed_apps, is_shorter, merge, InstalledApp,
    };
    use crate::scratch::ScratchDir;
    use std::path::PathBuf;

    fn app(name: &str, shortcut: &str, target: &str) -> InstalledApp {
        InstalledApp::file(
            name.to_string(),
            PathBuf::from(shortcut),
            PathBuf::from(target),
            Vec::new(),
        )
    }

    /// A Start Menu entry that starts its target with arguments.
    fn command(name: &str, shortcut: &str, target: &str, arguments: &[&str]) -> InstalledApp {
        InstalledApp::file(
            name.to_string(),
            PathBuf::from(shortcut),
            PathBuf::from(target),
            arguments
                .iter()
                .map(|argument| argument.to_string())
                .collect(),
        )
    }

    fn packaged(name: &str, aumid: &str) -> InstalledApp {
        InstalledApp::packaged(name.to_string(), aumid)
    }

    #[test]
    fn a_packaged_app_is_started_through_the_shell_namespace() {
        // A Store app has no file to run: the shell resolves its AppUserModelId
        // inside `AppsFolder`, and `explorer.exe` is the program that asks.
        let calculator = packaged(
            "Calculator",
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
        );

        assert!(
            calculator
                .program()
                .to_string_lossy()
                .ends_with("explorer.exe"),
            "a packaged app is not a file of its own: {:?}",
            calculator.program()
        );
        assert_eq!(
            calculator.arguments(),
            ["shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"]
        );
        assert!(calculator.is_packaged());
    }

    #[test]
    fn a_packaged_app_reaches_the_list_after_the_menu() {
        let menu = vec![app(
            "Brave",
            "C:\\Start Menu\\Brave.lnk",
            "C:\\Brave\\brave.exe",
        )];
        let shell = vec![packaged(
            "Calculator",
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
        )];

        let merged = merge(menu, shell);

        let names: Vec<&str> = merged.iter().map(|app| app.name()).collect();
        assert_eq!(names, ["Brave", "Calculator"]);
    }

    #[test]
    fn an_application_the_start_menu_already_offers_is_not_listed_twice() {
        // `Settings` and a few others come from both places on some machines.
        // The Start Menu entry wins: it is the one with a real shortcut, a real
        // target and an icon the shell already answered for.
        let menu = vec![app(
            "Settings",
            "C:\\Start Menu\\Settings.lnk",
            "C:\\Windows\\SystemSettings.exe",
        )];
        let shell = vec![packaged(
            "Settings",
            "windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel",
        )];

        let merged = merge(menu, shell);

        assert_eq!(merged.len(), 1, "the same application twice");
        assert!(
            !merged[0].is_packaged(),
            "the shortcut was replaced by the shell entry"
        );
    }

    #[test]
    fn two_packaged_apps_that_share_a_name_are_one_row() {
        let merged = merge(
            Vec::new(),
            vec![
                packaged("Clock", "Microsoft.WindowsAlarms_8wekyb3d8bbwe!App"),
                packaged("Clock", "Some.Other_1234!App"),
            ],
        );

        assert_eq!(merged.len(), 1);
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
        assert_eq!(tool_entry.program(), tool);
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
    fn two_prompts_for_one_program_are_two_applications() {
        // Visual Studio's six developer prompts are one `cmd.exe`. Collapsing
        // them by target leaves the user with whichever name sorts first.
        let found = vec![
            command(
                "Developer Command Prompt for VS 2022",
                r"C:\Start Menu\VS\Developer Command Prompt.lnk",
                r"C:\Windows\System32\cmd.exe",
                &["/k", "VsDevCmd.bat"],
            ),
            command(
                "x64 Native Tools Command Prompt for VS 2022",
                r"C:\Start Menu\VS\x64 Native Tools.lnk",
                r"C:\Windows\System32\cmd.exe",
                &["/k", "VsDevCmd.bat", "-arch=amd64"],
            ),
        ];

        let apps = finish(found);

        assert_eq!(apps.len(), 2, "two commands are two applications");
        assert_eq!(
            apps[1].arguments(),
            ["/k", "VsDevCmd.bat", "-arch=amd64"],
            "the arguments travel with the entry"
        );
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
        assert_eq!(apps[0].program(), PathBuf::from(r"C:\Start Menu\Brave.lnk"));
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
            // Every entry is startable one of two ways: a file that exists, or
            // an address the shell resolves (a packaged app, or a registered
            // URI). A row that is neither is a key that opens nothing.
            let by_shell = app.is_shell();
            match by_shell {
                true => assert!(
                    app.program()
                        .to_string_lossy()
                        .to_lowercase()
                        .ends_with("explorer.exe"),
                    "a shell entry without the shell to start it: {app:?}"
                ),
                false => assert!(app.target().is_file(), "the target is gone: {app:?}"),
            }
            if app.is_packaged() {
                assert!(
                    app.target()
                        .to_string_lossy()
                        .starts_with("shell:AppsFolder\\"),
                    "a packaged app without a shell address: {app:?}"
                );
            }
        }
        let mut sorted: Vec<String> = apps.iter().map(|app| app.name().to_lowercase()).collect();
        let before = sorted.clone();
        sorted.sort();
        assert_eq!(before, sorted, "the list is sorted by name");
    }
}
