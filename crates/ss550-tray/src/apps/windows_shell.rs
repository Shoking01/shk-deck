//! The shell's `AppsFolder`, walked as shell items.
//!
//! A packaged app — Settings, Calculator, anything from the Microsoft Store —
//! has no executable and no shortcut: it exists as a registration, and the shell
//! indexes those in a virtual folder called `AppsFolder`. That folder is the only
//! place that lists them, and it is reachable through COM only: an `IShellItem`
//! for the folder, an `IEnumShellItems` bound from it, and two display names per
//! item.
//!
//! Two names, because they answer different questions. The **normal** display
//! name is what a person calls it (`Calculator`); the **desktop absolute parsing**
//! name is the shell's own address for it, `::{CLSID}\<AppUserModelId>`, and the
//! id after the last separator is what `shell:AppsFolder\<id>` starts.
//!
//! ## What is left out
//!
//! The folder also holds things that are not packaged apps: folders (`Windows
//! Tools`), web links (`Node.js website`), and the odd shell extension. A real
//! AppUserModelId always carries a `!` between the package family and the
//! application id, and those are the entries kept: everything else already
//! reaches the list through the Start Menu, where its shortcut lives.
//!
//! ## Failure
//!
//! Every step answers `Option` or an empty list. A machine where the shell will
//! not hand the folder over is a machine whose picker lists the Start Menu only,
//! which is what it did before this module existed.

use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    BHID_EnumItems, IEnumShellItems, IShellItem, SHCreateItemFromParsingName, SIGDN,
    SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY,
};

use super::InstalledApp;

/// The shell's address for the virtual folder every packaged app is listed in.
const APPS_FOLDER: &str = "shell:AppsFolder";

/// Every packaged app the shell lists, sorted the way the shell lists them.
pub(super) fn apps() -> Vec<InstalledApp> {
    // The apartment outlives this call: the shell keeps the objects alive after
    // the enumeration, and tearing COM down under them is the crash the shortcut
    // module exists to avoid.
    if crate::shortcut::enter_apartment().is_none() {
        return Vec::new();
    }

    let Some(folder) = folder() else {
        return Vec::new();
    };
    // SAFETY: the folder is a shell item and `BHID_EnumItems` is the handler it
    // documents for listing its children.
    let Ok(items) = (unsafe { folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) })
    else {
        return Vec::new();
    };

    let mut found = Vec::new();
    loop {
        let mut batch: [Option<IShellItem>; 1] = [None];
        // A finished enumeration answers an error, which is how this ends.
        // SAFETY: `batch` is one slot long, which is the count handed over.
        if unsafe { items.Next(&mut batch, None) }.is_err() {
            break;
        }
        let Some(item) = batch.into_iter().flatten().next() else {
            break;
        };
        if let Some(app) = one(&item) {
            found.push(app);
        }
    }
    found
}

/// The `AppsFolder` itself.
fn folder() -> Option<IShellItem> {
    let wide = wide(APPS_FOLDER);
    // SAFETY: the string is NUL-terminated and lives for the call.
    unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }.ok()
}

/// One shell item, when it is a packaged app.
fn one(item: &IShellItem) -> Option<InstalledApp> {
    let name = display(item, SIGDN_NORMALDISPLAY)?;
    let parsing = display(item, SIGDN_DESKTOPABSOLUTEPARSING)?;

    // A packaged app: `::{CLSID}\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App`.
    // The `!` is the package family's seam, and it is what separates an app from
    // a folder or a web link the same folder also lists.
    if let Some(aumid) = parsing.rsplit('\\').next() {
        let aumid = aumid.trim();
        if aumid.contains('!') {
            return Some(InstalledApp::packaged(name, aumid));
        }
    }

    // A file the folder publishes: `C:\Riot Games\Riot Client\RiotClientServices.exe`
    // is one, and it is an application like any other — there is simply no
    // shortcut for it in the Start Menu. A registered URI (`steam://rungameid/…`)
    // is started through the shell too, and it is how the folder lists a game
    // that has no executable of its own. A **web page** (`https://…`) is left out
    // on purpose: that is what `OPEN URL` is for, and a browser tab is not an
    // application a key runs. A shell id without a package
    // (`Microsoft.Windows.Explorer`, `::{CLSID}\…`) is left out as well: those
    // are shell folders and commands, not applications.
    let path = Path::new(&parsing);
    if path.is_absolute() && path.is_file() {
        return Some(InstalledApp::file(
            name,
            path.to_path_buf(),
            path.to_path_buf(),
            Vec::new(),
        ));
    }
    if parsing.contains("://") && !parsing.starts_with("http") {
        return Some(InstalledApp::shell(name, &parsing, false));
    }
    None
}

/// One of an item's display names, copied out of the shell's memory.
fn display(item: &IShellItem, kind: SIGDN) -> Option<String> {
    // SAFETY: the item answers an owned string, which is freed below.
    let wide = unsafe { item.GetDisplayName(kind) }.ok()?;
    // SAFETY: the string the shell handed over is NUL-terminated UTF-16.
    let text = unsafe { wide.to_string() }
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    // SAFETY: the pointer came from COM and is freed exactly once, whatever the
    // conversion above answered.
    unsafe { CoTaskMemFree(Some(wide.0 as *const _)) };
    text
}

/// A string as the wide, NUL-terminated form the Win32 calls take.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
