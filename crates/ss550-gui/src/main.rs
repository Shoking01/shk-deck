//! A GPUI Kit desktop window that drives a deck through the deck seam.
//!
//! Run with:
//!
//! ```text
//! cargo run --manifest-path crates/ss550-gui/Cargo.toml
//! ```
//!
//! The window shows the deck that is plugged in, whatever it is: the grid, the
//! image size and the controls all come from the deck's own `Deck` profile.
//! Every key is a coloured tile that says its own number, the colour it is
//! showing and the action bound to it, so the screen always matches what the
//! hardware is doing.
//!
//! The window is a shell of its own: an app bar, a rail of three pages, a
//! scrolling content area and a status line, all built in [`ui`]. This file owns
//! the controller — the connection, the bindings, the action runner — and the
//! shell draws whatever it says.
//!
//! Pressing a key on the deck lights its tile and runs the action bound to it,
//! on a background thread. Bindings are kept per deck, in the app's own
//! configuration folder.

// The release build is a GUI binary: Windows creates a console window for a
// console-subsystem executable no matter how it is started, and a tray
// application must not have one. Debug builds keep the console so `cargo run`
// still shows output and panics while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![deny(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use deck_api::{ButtonState, DeckError, ImageSpec, KeyId};
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{
    px, size, AnyWindowHandle, AppContext as _, Context, Entity, IntoElement, Render, SharedString,
    Subscription, Window, WindowBounds, WindowOptions,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use ss550_driver::LONG_CONNECT_TIMEOUT;
use ss550_engine::{Action, ActionRunner, ActionStore, KeyNumber, Recents};
use ss550_plugin_api::{CellSpec, Reading};
use ss550_plugins::{Catalogue, Sampler, Samples};
use ss550_tray::{autostart, install, TrayCommand, TrayHost};

use titlestyle::TitleLook as _;
use ui::Page;

mod apps;
mod diagnostics;
mod facework;
mod favicons;
mod history;
mod icons;
mod pictures;
mod plugintile;
mod provider;
mod render;
mod session;
mod thresholds;
mod titlestyle;
mod titletext;
mod ui;

/// Clicks on the version line that open the diagnostic log.
///
/// The trigger is hidden on purpose: this is a support tool, not a feature, and
/// a button for it would invite clicks that answer nothing.
const VERSION_CLICKS_TO_REVEAL: u8 = 5;

/// Why a picture cannot be put on a key with no deck in front of the window.
///
/// A picture is fitted to the deck's tile and saved into that deck's profile, so
/// without one there is nothing to fit it to. Stated once: the note the click
/// writes and the caption under the picture controls are the same sentence.
const NO_DECK_PICTURE: &str =
    "Connect a deck to put a picture on a key: the tile size comes from it.";

/// Why a title cannot be put on a key with no deck in front of the window.
///
/// A title is drawn into the deck's JPEG and saved in that deck's profile, so
/// without one there is nothing to draw it into and nowhere to keep it.
const NO_DECK_TITLE: &str =
    "Connect a deck to give a key a title: it is drawn into that deck's tile.";

/// Counts one click on the version line and says whether the reveal is due.
///
/// Split out from the window so the boundary — four clicks do nothing, the
/// fifth reveals and starts over — is testable without starting a viewer.
fn advance_clicks(clicks: u8) -> (u8, bool) {
    let next = clicks.saturating_add(1);
    if next < VERSION_CLICKS_TO_REVEAL {
        (next, false)
    } else {
        (0, true)
    }
}

/// Colour the user can paint with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Paint {
    /// No light: the key is dark. This is the default, because a key with
    /// nothing on it should look like nothing, not like a white button.
    Blank,
    White,
    Red,
}

impl Paint {
    /// The colour this paint puts on a key, as an image encoder sees it.
    fn rgb(self) -> [u8; 3] {
        match self {
            Paint::Blank => [0x00, 0x00, 0x00],
            Paint::White => [0xE4, 0xE4, 0xE7],
            Paint::Red => [0xDC, 0x26, 0x26],
        }
    }
}

/// What a key currently shows on the deck.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slot {
    White,
    Red,
    /// A picture from the asset library, rather than a flat colour.
    Image,
    Off,
}

impl Paint {
    /// The slot a key gets when it is painted with this colour.
    fn slot(self) -> Slot {
        match self {
            Paint::Blank => Slot::Off,
            Paint::White => Slot::White,
            Paint::Red => Slot::Red,
        }
    }
}

/// Why painting one key failed.
///
/// The two halves are reported differently: a session failure may mean the link
/// died and the deck must be dropped, while a picture that could not be read is
/// the user's file talking and the link is fine.
enum PaintFailure {
    Session(session::SessionError),
    Picture(String),
}

/// What one key is going to be painted with.
///
/// Decided from the profile, the bindings and the icon cache *before* the first
/// byte goes out. A plan that is resolved first cannot leave the window's idea
/// of a key and the deck's disagreeing about everything after the one that
/// failed.
enum Tile {
    /// A picture from the asset library, encoded for this deck — with the key's
    /// title drawn into it when it has one.
    Asset {
        /// The picture the profile names.
        name: ss550_engine::AssetName,
        /// The words the key says, when it says any.
        title: Option<String>,
        /// How those words are drawn: the key's own style, or the default.
        style: titlestyle::TitleStyle,
    },
    /// The encoded picture of what the key runs: the program's own icon, or the
    /// initial of what it runs, with the title already in the pixels.
    Art(Vec<u8>),
    /// One flat colour.
    Colour([u8; 3]),
    /// A flat colour with a title drawn into it, encoded as an image.
    ///
    /// A deck only ever receives a JPEG, so a key that says something has to
    /// send one: the words are pixels, not a property of the colour.
    TitledColour(Vec<u8>),
}

/// The sentence the tray icon carries, with the window closed.
///
/// One line, and short: this is what a person reads by hovering a 16-pixel icon,
/// so it names the program once and then gives the numbers in the order the
/// catalogue states them — processor, card, memory, disk.
///
/// It says what state the controller is in when it is not measuring anything: a
/// tooltip reading "Sh-Deck" and nothing else would leave "is it connected?" to
/// guesswork, and that is the one question a tray icon exists to answer.
///
/// The length is bounded because Windows truncates a taskbar tooltip at 127
/// characters, and a sentence cut in the middle of a number reads as a bug.
fn tray_text(link: Link, samples: &Samples, cells: &[CellSpec]) -> String {
    const MAX: usize = 120;

    let mut text = String::from("Sh-Deck");
    match link {
        Link::Connected => {}
        Link::Connecting => return format!("{text} · connecting"),
        Link::Waiting => return format!("{text} · waiting for the deck"),
        Link::Lost => return format!("{text} · connection lost, reconnecting"),
    }

    let measured: Vec<String> = cells
        .iter()
        .filter_map(|cell| {
            let reading = samples.get(cell.plugin(), cell.cell())?;
            Some(format!("{} {}", cell.label(), reading.text()))
        })
        .collect();

    if measured.is_empty() {
        return format!("{text} · connected");
    }
    for reading in measured {
        let candidate = format!("{text} · {reading}");
        if candidate.chars().count() > MAX {
            break;
        }
        text = candidate;
    }
    text
}

/// Decides one key's face, before anything is sent.
///
/// The order is the whole of the rule: the profile's appearance first, because
/// that is what the user chose; then what the key runs, which is the icon or the
/// initial; then the flat colour, for a key that runs nothing or whose picture
/// this build could not build. The title is drawn on top of whichever wins.
fn plan_tile(
    appearance: Option<&ss550_engine::Appearance>,
    action: Option<&Action>,
    title: Option<&str>,
    style: &titlestyle::TitleStyle,
    default_colour: [u8; 3],
    icons: &mut icons::IconCache,
    text: &mut Option<titletext::TextEngine>,
) -> Tile {
    match appearance {
        Some(ss550_engine::Appearance::Image { asset }) => Tile::Asset {
            name: asset.clone(),
            title: title.map(str::to_string),
            style: style.clone(),
        },
        Some(ss550_engine::Appearance::Solid { rgb }) => {
            match icons.titled_colour(*rgb, title, style, text) {
                Some(jpeg) => Tile::TitledColour(jpeg),
                None => Tile::Colour(*rgb),
            }
        }
        None => match action.and_then(|action| icons.tile(action, title, style)) {
            // A picture the cache already composed is served as it is; the
            // encoder ran on the change, not now.
            Some(picture) => Tile::Art(picture.to_vec()),
            None => match icons.titled_colour(default_colour, title, style, text) {
                Some(jpeg) => Tile::TitledColour(jpeg),
                None => Tile::Colour(default_colour),
            },
        },
    }
}

/// The JPEG one key's picture becomes, with the key's title drawn into it.
///
/// A key with no title is served from the tile cache: its bytes cannot change,
/// so the picture is decoded and encoded once. A key with a title is composed
/// here — fitted, drawn on, then encoded through the same path — because the
/// words are per key while the picture is per asset. Both happen on the change
/// that caused them (a bind, a profile switch, a picture, a title), never while
/// a frame is drawn.
fn asset_tile(
    assets: &ss550_engine::AssetLibrary,
    tiles: Option<&mut render::TileCache>,
    spec: Option<ImageSpec>,
    name: &ss550_engine::AssetName,
    title: Option<&str>,
    style: &titlestyle::TitleStyle,
    text: &mut Option<titletext::TextEngine>,
) -> Result<Vec<u8>, String> {
    let source = assets.read(name).map_err(|error| error.to_string())?;

    let Some(title) = title.map(str::trim).filter(|title| !title.is_empty()) else {
        return match tiles {
            Some(cache) => cache
                .get(name, &source)
                .map(<[u8]>::to_vec)
                .map_err(|error| error.to_string()),
            None => Err("the deck has not stated an image size yet".to_string()),
        };
    };

    let Some(spec) = spec else {
        return Err("the deck has not stated an image size yet".to_string());
    };
    let fitted = render::fitted_tile(spec, name, &source).map_err(|error| error.to_string())?;
    // The font is read here and not one line earlier. While the scan is still
    // running the key keeps exactly the picture it would have with no title, and
    // the repaint that follows the scan draws the words.
    let Some(text) = titletext::ready(text) else {
        return render::render_rgba(spec, &name.to_string(), &fitted)
            .map_err(|error| error.to_string());
    };
    let composed = icons::compose_title(&fitted, title, style, text);
    render::render_rgba(spec, &name.to_string(), &composed).map_err(|error| error.to_string())
}

/// Which kind of action the bindings panel is editing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DraftKind {
    /// Run a program.
    Launch,
    /// Open a URL.
    OpenUrl,
    /// Send a key combination to the focused window.
    Keys,
    /// Show a live reading from a plugin on this key.
    Plugin,
}

impl DraftKind {
    /// The word the palette and the status line use for this kind.
    ///
    /// One name for one thing: the editor's segments, the palette rows and the
    /// sentence a drop writes all read from here.
    fn label(self) -> &'static str {
        match self {
            DraftKind::Launch => "run an app",
            DraftKind::OpenUrl => "open a URL",
            DraftKind::Keys => "send a keystroke",
            DraftKind::Plugin => "show a live reading",
        }
    }
}

/// Whether composing this deck's payloads needs the font database.
///
/// Any key with words counts, **bound or not**: a key that runs nothing still
/// shows its title on a flat colour, and those words are pixels like any other.
/// Reading the profile here rather than a list of bindings is what makes that
/// true — an earlier version asked only the bound keys and left a named-but-
/// unbound key with words the deck could never draw.
///
/// A deck that has stated no image size needs no font at all: there is no
/// payload to draw them into, which is why the scan is not requested before one
/// connects.
fn needs_fonts(
    spec: Option<ImageSpec>,
    keys: Option<&ss550_engine::profiles::PageKeys>,
    key_count: usize,
) -> bool {
    if spec.is_none() {
        return false;
    }
    let Some(keys) = keys else {
        return false;
    };
    (1..=key_count as u16).any(|key| keys.title(key).is_some())
}

/// The style one key wears: its own when the page gives it one, the app's
/// default otherwise.
///
/// Split out of [`Controller::style_for`] because it is the whole of the
/// fallback rule and needs no window to answer: the page, the default, and the
/// key are all it reads.
fn resolve_style(
    keys: Option<&ss550_engine::profiles::PageKeys>,
    default: &titlestyle::TitleStyle,
    key: u16,
) -> titlestyle::TitleStyle {
    keys.and_then(|keys| keys.title_style(key))
        .cloned()
        .unwrap_or_else(|| default.clone())
}

/// What a navigation action did to the page stack.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NavOutcome {
    /// A folder was entered; the stack grew by one.
    Entered,
    /// A step back was taken; the stack lost its last entry.
    WentBack,
    /// The folder named is not in this profile; nothing moved.
    UnknownPage,
    /// The deck is on the root already; nothing moved.
    AtRoot,
    /// Not a navigation action: the caller hands it to the runner.
    NotNavigation,
}

/// Applies one action to the page stack, without touching the deck.
///
/// Split out of [`Controller::apply_navigation`] because it is the whole of S3 —
/// enter, leave, and what happens at the edges — and needs no window to answer:
/// a stack and a way to ask whether a page exists are all it reads.
fn navigate(
    stack: &mut Vec<String>,
    action: &Action,
    page_exists: impl Fn(&str) -> bool,
) -> NavOutcome {
    match action {
        Action::Folder { page } => {
            if !page_exists(page) {
                return NavOutcome::UnknownPage;
            }
            stack.push(page.clone());
            NavOutcome::Entered
        }
        Action::Back => match stack.pop() {
            Some(_) => NavOutcome::WentBack,
            None => NavOutcome::AtRoot,
        },
        _ => NavOutcome::NotNavigation,
    }
}

/// Which title style a control edits: the app's default, or one key's own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FontTarget {
    /// The style every key without one of its own follows.
    Defaults,
    /// One key's own style, kept in its profile.
    Key(u16),
}

/// Connection state shown in the header pill.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Link {
    /// An attempt is in flight right now.
    Connecting,
    /// The deck is not on the USB bus; the app keeps retrying.
    Waiting,
    /// The deck is connected and ready.
    Connected,
    /// The link died while the app was running; the app reconnects on its own.
    Lost,
}

impl Link {
    /// What the pill and the status bar print for this state.
    ///
    /// The words are the whole state, not a tooltip for the colour: a reader who
    /// cannot tell amber from red still reads "WAITING FOR DECK".
    fn label(self) -> &'static str {
        match self {
            Link::Connecting => "CONNECTING",
            Link::Waiting => "WAITING FOR DECK",
            Link::Connected => "CONNECTED",
            Link::Lost => "CONNECTION LOST",
        }
    }
}

/// The picture dialog, when one is open.
///
/// The system's dialog pumps a message loop of its own, so opening it from a
/// click hands GPUI a second, re-entrant dispatch while the app is already
/// mutably borrowed for that click — which is a panic, not a dialog. It is
/// opened on a worker thread instead, and the answer comes back over this
/// channel and is read on the poll, where a mutable borrow is the normal state
/// of affairs.
///
/// One slot, deliberately: a click that arrives while a dialog is open is
/// ignored rather than queued, so modal windows cannot stack, and the key it
/// would have painted is still the key the open dialog is choosing for.
#[derive(Default)]
struct PictureDialog {
    /// The channel of the dialog that is open, until its answer is read.
    answer: Option<Receiver<Option<PathBuf>>>,
}

impl PictureDialog {
    /// Takes the one slot a dialog can occupy, and answers the sender for it.
    ///
    /// `None` when a dialog is already open: that is the whole of the
    /// one-at-a-time rule, stated where the slot is taken so no caller can
    /// forget it.
    fn claim(&mut self) -> Option<Sender<Option<PathBuf>>> {
        if self.answer.is_some() {
            return None;
        }
        let (sender, answer) = mpsc::channel();
        self.answer = Some(answer);
        Some(sender)
    }

    /// Gives the slot back when the worker could not be started.
    ///
    /// Without this a thread that fails to spawn would hold the slot forever
    /// and the button would never answer again.
    fn release(&mut self) {
        self.answer = None;
    }

    /// Takes the answer, once the dialog has closed.
    ///
    /// `None` while the dialog is still up. `Some(None)` is a dialog that
    /// answered "cancel" — and also a worker that died before answering, which
    /// leaves the caller exactly the same nothing to do.
    fn take_answer(&mut self) -> Option<Option<PathBuf>> {
        let answer = self.answer.as_ref()?;
        match answer.try_recv() {
            Ok(answer) => {
                self.answer = None;
                Some(answer)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.answer = None;
                Some(None)
            }
        }
    }
}

/// What one answer from the picture dialog amounts to.
///
/// Split out from the window so the two rules that matter — a click with no deck
/// imports nothing, and a cancelled or refused file leaves the key alone — are
/// testable without a screen, a deck or a dialog.
#[derive(Debug)]
enum PickOutcome {
    /// The dialog was cancelled, or never opened: nothing was touched.
    Cancelled,
    /// No deck in front of the window, so there is no tile to fit a picture to.
    NoDeck,
    /// The file is in the library under this name, and the key should show it.
    Imported(ss550_engine::AssetName),
    /// The library already had a file under this name; the key should show that
    /// one, and the window says nothing was copied.
    AlreadyHeld(ss550_engine::AssetName),
    /// The library refused the file; the sentence is the engine's own.
    Refused(String),
}

/// Turns the dialog's answer into the next step, in the order the click means.
///
/// The deck is checked before the file is copied, not after: a picture imported
/// for a key that cannot wear it is a file the user now has and did not ask for.
///
/// Nothing here assigns anything. Only [`PickOutcome::Imported`] leads to an
/// assignment, which is what keeps the key, the profile and the library
/// untouched on every other answer.
fn apply_pick(
    deck_present: bool,
    library: &ss550_engine::AssetLibrary,
    answer: Option<PathBuf>,
) -> PickOutcome {
    let Some(path) = answer else {
        return PickOutcome::Cancelled;
    };
    if !deck_present {
        return PickOutcome::NoDeck;
    }
    match pictures::import(library, &path) {
        Ok(pictures::Imported::New(name)) => PickOutcome::Imported(name),
        Ok(pictures::Imported::AlreadyHeld(name)) => PickOutcome::AlreadyHeld(name),
        Err(sentence) => PickOutcome::Refused(sentence),
    }
}

/// How often the deck is polled for key presses.
///
/// The poll is non-blocking, so this only costs a read that usually returns
/// nothing. 50 ms is fast enough that a press feels instant.
const POLL: Duration = Duration::from_millis(50);

/// How often connection results, retries and the heartbeat are handled.
const TICK: Duration = Duration::from_millis(500);

/// How many frames the window records before it only records slow ones.
///
/// The first frames are what startup costs, and they are worth a line each.
/// After that the log would become a frame dump, so only a frame long enough to
/// be seen gets recorded — which is the one a support report needs.
const FRAMES_LOGGED: u32 = 20;

/// A frame long enough to be visible as a hitch, and therefore worth logging.
const SLOW_FRAME: Duration = Duration::from_millis(50);

/// A duration as a plain number with one decimal, in milliseconds.
///
/// A frame is not a whole number of milliseconds: the first frames of this
/// window cost well under 1 ms, and rounding those to `0 ms` would report a
/// frame that never happens. One decimal keeps a sub-millisecond frame
/// readable, and the log stays a log rather than a benchmark.
fn ms_text(cost: Duration) -> String {
    format!("{:.1}", cost.as_secs_f64() * 1000.0)
}

/// Minimum delay between two connection attempts.
///
/// The deck leaves the USB bus for 4–8 s while it boots and re-enumerates
/// itself more than once, so a single attempt can easily miss it. Retrying
/// forever is what makes "plug the deck in while the app is open" work.
const RETRY_INTERVAL: Duration = Duration::from_secs(2);

/// The longest a retry is ever postponed.
///
/// The delay doubles on every failed attempt and stops here, so a deck that is
/// gone for an hour costs one attempt a minute instead of one every two
/// seconds. It is also the ceiling that keeps the shift from overflowing.
const MAX_RETRY_INTERVAL: Duration = Duration::from_secs(60);

/// Outcome of one connection attempt, produced by the connector worker.
enum Attempt {
    /// The deck answered and is ready to be spoken to.
    Connected {
        generation: u64,
        deck: ss550_driver::Ss550Deck,
        /// Where it was found, so the next start can open it without a walk.
        path: Option<String>,
    },
    /// The attempt failed; the error is shown to the user.
    Failed {
        generation: u64,
        error: provider::ConnectError,
    },
}

/// Where the deck was found last time, so the next start can go straight to it.
///
/// One line in the app's own configuration folder. It is a hint, not a fact: a
/// path that no longer opens is forgotten and the deck is discovered again, and
/// an unreadable file simply means "nothing remembered".
fn deck_path_file() -> std::path::PathBuf {
    ss550_engine::config_dir().join("deck-path.txt")
}

/// Reads the remembered device path, if there is one.
fn load_deck_path() -> Option<String> {
    read_deck_path_at(&deck_path_file())
}

/// [`load_deck_path`] for a file the caller names, so the rule can be tested.
fn read_deck_path_at(file: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(file)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|path| !path.is_empty())
}

/// Records where the deck was found, or forgets the path when there is none.
///
/// A write that fails is not worth a sentence: the only cost is one walk on the
/// next start, which is what the app does anyway when it has never seen a deck.
fn save_deck_path(path: Option<&str>) {
    write_deck_path_at(&deck_path_file(), path);
}

/// [`save_deck_path`] for a file the caller names.
fn write_deck_path_at(file: &std::path::Path, path: Option<&str>) {
    match path.filter(|path| !path.trim().is_empty()) {
        Some(path) => {
            if let Some(parent) = file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(file, path);
        }
        None => {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// Runs connection attempts on a dedicated thread.
///
/// Opening a deck blocks for seconds while it waits for the boot
/// re-enumeration to settle, so it must never run on the UI thread.
///
/// The attempt waits [`LONG_CONNECT_TIMEOUT`] on purpose: the deck's boot is
/// appear → leave → brief appearance → leave → settle, and the settle can be
/// ~11 s away. Giving up earlier would mean starting over in a different
/// phase of the boot, over and over.
///
/// `remembered` is where the deck was found on a previous run. It is passed to
/// the one [`ss550_driver::Locator`] this thread builds, which is what turns a
/// reconnect into a single `CreateFile` instead of a walk of every HID device on
/// the machine.
fn connector_worker(commands: Receiver<u64>, results: Sender<Attempt>, remembered: Option<String>) {
    let mut locator: Option<Result<ss550_driver::Locator, ss550_driver::Ss550Error>> = None;
    while let Ok(generation) = commands.recv() {
        // The locator owns this process's one `HidApi`, and with it the one walk
        // of the machine's HID devices. Every later attempt asks for the
        // remembered path instead — see `ss550_driver::Locator`.
        let locator = locator.get_or_insert_with(|| ss550_driver::Locator::new(remembered.clone()));
        let attempt = match locator {
            Err(error) => Attempt::Failed {
                generation,
                error: provider::ConnectError::Other(error.to_string()),
            },
            Ok(locator) => match provider::connect(locator, LONG_CONNECT_TIMEOUT) {
                Ok(deck) => Attempt::Connected {
                    generation,
                    deck,
                    path: locator.path().map(str::to_string),
                },
                Err(error) => Attempt::Failed { generation, error },
            },
        };

        if results.send(attempt).is_err() {
            // The window is gone: nobody is waiting for this result.
            break;
        }
    }
}

struct Controller {
    /// The deck in front of the window, once one answered.
    session: Option<session::DeckSession>,
    link: Link,
    note: SharedString,
    /// Which page the rail has selected.
    page: Page,
    paint: Paint,
    /// What each key shows, in the deck's own key order.
    slots: Vec<Slot>,
    /// Keys held down right now, for the tile highlight.
    pressed: Vec<bool>,
    /// The brightness the window last asked for.
    brightness: u8,
    /// Commands for the connector worker. Bounded to one pending request.
    commands: SyncSender<u64>,
    /// Results coming back from the connector worker.
    results: Receiver<Attempt>,
    /// Generation of the newest requested attempt; older results are dropped.
    generation: u64,
    /// True while an attempt is in flight.
    pending: bool,
    /// Attempts made since the last successful connection.
    attempts: u32,
    /// When the connection and heartbeat work last ran.
    last_tick: Instant,
    /// When it's safe to try reconnecting again (for backoff).
    reconnect_at: Instant,
    /// What every key does, and where it is saved.
    store: ActionStore,
    /// Where this deck's bindings live, once a deck answered.
    store_path: Option<PathBuf>,
    /// The actions this deck was last given, newest first.
    ///
    /// A convenience for the rail, not a second source of truth: the bindings
    /// file still says what each key does. It is loaded with the bindings and
    /// saved on the same click that adds an action to a key.
    recents: Recents,
    /// Runs bound actions off the UI thread.
    runner: ActionRunner,
    /// The key the bindings panel is editing.
    selected_key: KeyNumber,
    /// Which kind of action the panel is editing.
    draft: DraftKind,
    /// The plugin cell the editor has chosen, while the draft is a reading.
    ///
    /// Held apart from the inputs because a cell is two ids, not a string: the
    /// list under the form sets it, and SAVE BINDING turns it into the action.
    draft_plugin: Option<(String, String)>,
    /// The plugin whose cells the reading editor lists, when a plugin row is
    /// what opened it.
    ///
    /// Set by dropping a plugin row on a key and cleared by every other way into
    /// the editor. It narrows a list; it never restricts what may be bound, so a
    /// key already bound to another plugin still shows every cell.
    draft_plugin_filter: Option<String>,
    program_input: Entity<InputState>,
    args_input: Entity<TextareaState>,
    url_input: Entity<InputState>,
    /// The applications the `RUN APP` editor offers, and whether it has looked.
    ///
    /// Scanned once, on the click that opens the list, and kept: the scan is the
    /// only slow step and it must never happen in a frame.
    app_list: apps::Apps,
    /// The picker's search field. Its own change event redraws the list, so
    /// typing filters without the editor holding the text.
    app_query: Entity<InputState>,
    /// Whether the picker is showing under the `PROGRAM` field.
    app_picker_open: bool,
    /// The program in the `PROGRAM` field, when it is a program.
    ///
    /// Held apart from the input's text because the icon cache is keyed by path
    /// and the field holds whatever has been typed. It is what makes the icon of
    /// a chosen application visible before SAVE BINDING.
    draft_program: Option<PathBuf>,
    /// Kept alive so the picker's search field redraws the list while typing.
    _app_query_changes: Subscription,
    /// The keystroke the editor is editing, as the user typed it.
    ///
    /// It holds text, not a `KeyCombo`: a half-typed combination has to stay in
    /// the field, and the engine's refusal is what the note shows until it
    /// parses.
    keys_input: Entity<InputState>,
    /// The words the selected key shows, as the user typed them.
    ///
    /// The profile is the authority, and the engine trims and bounds what it
    /// stores; this field only holds the draft, so a refusal leaves the text
    /// where the user can fix it.
    title_input: Entity<InputState>,
    /// Kept alive so Enter in the title field saves it.
    _title_enter: Subscription,
    /// The notification-area icon, when the platform has one.
    tray: Option<TrayHost>,
    /// Why the notification-area icon is not there, when it is missing.
    tray_error: Option<String>,
    /// The native handle of the controller window, to hide it and bring it back.
    native: Option<isize>,
    /// The handle of the controller window, to bring it back from the tray.
    window_handle: AnyWindowHandle,
    /// Whether the controller starts with Windows.
    starts_with_windows: bool,
    /// Why the sign-in entry does not work, when it is there but broken.
    autostart_note: Option<String>,
    /// What comparing this binary with the copy Windows starts found.
    sign_in_copy: Option<SignInCopy>,
    /// Clicks on the version line, until they add up to the reveal.
    version_clicks: u8,
    /// Frames the window has built, for the startup and hitch timings.
    frames: u32,
    /// What the most recent frame cost.
    ///
    /// Recorded by the same measurement that writes the diagnostic log, so the
    /// status bar reports the window's own timing and not a second one.
    last_frame: Duration,
    /// True when the window was told not to look for a deck at all.
    blind: bool,
    /// Profile management for this deck.
    profile_store: ss550_engine::ProfileStore,
    /// The pictures a key can show.
    assets: ss550_engine::AssetLibrary,
    /// The picture dialog, while one is open.
    ///
    /// The dialog runs on a worker thread — see [`PictureDialog`] for why — and
    /// this is the only place its answer can arrive. Its presence is also the
    /// one-dialog-at-a-time rule.
    picture: PictureDialog,
    /// The pictures the keys show, already fitted to the deck's tile size and
    /// swapped for the renderer.
    ///
    /// Built when the deck or the profile changes — never while drawing. A tile
    /// that finds its picture here paints it; a tile that does not falls back to
    /// its flat colour, so a picture deleted from the library while a profile
    /// still names it cannot blank a key or panic the window.
    previews: BTreeMap<ss550_engine::AssetName, Arc<gpui_kit::RenderImage>>,
    /// The icon of every program the deck can run, and the initial of every
    /// action that has no icon.
    ///
    /// Filled by [`Controller::rebuild_icons`] on a rebind, a profile switch or a
    /// deck change, and only read while drawing: extracting an icon is a shell
    /// call, and a frame must never make one.
    icons: icons::IconCache,
    /// Tiles already rendered for this deck's image size, so switching profile
    /// back and forth does not re-decode every picture.
    tiles: Option<render::TileCache>,
    /// The window's height in pixels, as the last frame saw it.
    ///
    /// The side column is bounded by it, so the menu scrolls inside itself and the
    /// deck's grid stays on screen while a key is being edited. It is read during
    /// a frame and never written by one.
    pub(crate) viewport_h: f32,
    /// The folders the deck has entered, outermost first. Empty is the root.
    ///
    /// A stack, not a name: VOLVER goes back where the deck came from, which is
    /// the last entry. It is runtime state and is not saved: activating a profile
    /// starts at its root, so a page that no longer exists can never strand the
    /// deck on an empty screen.
    page_path: Vec<String>,
    /// How every title looks, in the window and in the deck's picture.
    ///
    /// Loaded once at start and changed only from the settings page, where the
    /// change repaints every titled key through the same path a title edit uses.
    /// The icon cache is told the same style, so the two cannot disagree.
    title_style: titlestyle::TitleStyle,
    /// The fonts a title is drawn with.
    ///
    /// `None` until the background scan answers: reading the system's fonts is
    /// the one slow step in composing a title, and a start whose profile has no
    /// titles never asks for it at all.
    text: Option<titletext::TextEngine>,
    /// Whether that scan is already in flight, so one is asked for, not five.
    text_scan: bool,
    /// Set by a rebuild that found titled keys and no engine yet.
    fonts_wanted: bool,
    /// The families the settings page offers, read once from the engine.
    font_families: Vec<String>,
    /// Which style the family list is open for, when it is open at all.
    font_picker: Option<FontTarget>,
    /// The family list's filter field.
    font_query: Entity<InputState>,
    /// The name a new page takes, and a page's new name.
    page_input: Entity<InputState>,
    /// What the active profile would be renamed to.
    profile_input: Entity<InputState>,
    /// Redraws the list while it is being filtered, so the rows follow the field.
    _font_query_changes: Subscription,
    /// The claim that this process is the controller for this desktop.
    ///
    /// `None` only when the claim could not be taken at all; the app still runs,
    /// it just cannot promise a second start will not open a second window.
    /// Polled every tick: a start that found this one asks for the window by
    /// ringing the event this holds.
    singleton: Option<ss550_tray::instance::SingleInstance>,
    /// What the plugins of this build can measure, by name.
    ///
    /// Read-only after construction: the list is baked into the build, and the
    /// editor lists it exactly as the plugin published it.
    plugins: Catalogue,
    /// The thread that walks the machine, when it is running.
    ///
    /// `None` when the thread could not start: the editor still lists the cells
    /// and a bound key keeps the face it had, which is a window that has lost a
    /// gauge and not a window that lies.
    sampler: Option<Sampler>,
    /// The newest readings, as the sampler last handed them over.
    samples: Samples,
    /// The sentence that says the sampler thread stopped, once it has.
    ///
    /// Kept so the note is written once: a dead sampler stays dead, and a window
    /// that says so on every tick would bury every other message.
    sampler_gone: Option<String>,
    /// What each key bound to a plugin is currently showing.
    ///
    /// The repaint rule's memory: a reading that draws the same face as this one
    /// costs no bytes on the link, so a percentage that has not moved does not
    /// send a JPEG.
    drawn_readings: Vec<Option<Reading>>,
    /// The thread that composes a face, and the faces it hands back.
    ///
    /// Shaping three lines and encoding a JPEG per changed key is not work for
    /// the thread that draws, so it happens there and arrives here as bytes.
    faces: facework::FaceWorker,
    /// The keys whose face is being composed right now.
    ///
    /// A key with a job in flight is not queued again: the sample that arrives
    /// while it is being drawn is one the next sample will pick up, and queueing
    /// it twice would paint the older face second.
    faces_in_flight: BTreeSet<u16>,
    /// The window's own tile for a reading, by the key and the binding it shows.
    ///
    /// Keyed by the binding as well as the key so a page switch cannot draw one
    /// cell's face on another cell's key: a stale entry simply never matches.
    pub(crate) face_tiles: BTreeMap<(u16, String, String), Arc<gpui_kit::RenderImage>>,
    /// The sentence that says the face worker stopped, once it has.
    faces_gone: Option<String>,
    /// What every measured cell has been doing, for the Stats page.
    ///
    /// Written from the same sample the keys are painted from, so the graph and
    /// the key can never disagree about the last number.
    history: history::History,
    /// The lines the Stats page paints a card by, and the Settings page edits.
    ///
    /// Loaded once at start and changed only from that page, where a click saves
    /// it and the page is redrawn with the new value.
    thresholds: thresholds::Thresholds,
    /// What went wrong the last time the lines were read or written, said inside
    /// the card that owns them.
    ///
    /// The status line is not the place for it: the deck connecting overwrites that
    /// line within a second, and a message about a saved setting that vanishes
    /// before it is read has not been said.
    thresholds_note: Option<String>,
    /// The tooltip the tray icon is showing right now.
    ///
    /// The tray is renamed only when this changes: a message to another thread
    /// every half second for a string that says nothing new is a waste, and it is
    /// the kind of waste that shows up as a stutter.
    tray_tooltip: String,
}

/// The keys the newest sample would change on the deck.
///
/// S6, stated as a function over plain data so it is a test and not a
/// convention: a key is due only when it is bound to a plugin, that cell was
/// measured, and the reading draws a different face from the one the key last
/// drew. A key bound to anything else is never due, and a cell the sample does
/// not carry is left alone rather than blanked.
fn readings_to_repaint(
    actions: &[Option<Action>],
    drawn: &[Option<Reading>],
    samples: &Samples,
) -> Vec<usize> {
    let mut due = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        let Some(action) = action.as_ref() else {
            continue;
        };
        let Some((plugin, cell)) = action.plugin_cell() else {
            continue;
        };
        let Some(reading) = samples.get_named(plugin, cell) else {
            continue;
        };
        let unchanged = drawn
            .get(index)
            .and_then(Option::as_ref)
            .is_some_and(|drawn| drawn.same_face(reading));
        if !unchanged {
            due.push(index);
        }
    }
    due
}

impl Controller {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (commands, command_rx) = mpsc::sync_channel::<u64>(1);
        let (result_tx, results) = mpsc::channel::<Attempt>();

        // A measurement or support run can ask the window not to look for a
        // deck. Then no device is opened at all — not even enumerated — which
        // is the only way to time the window while the deck on the desk is
        // already in use, and the only way to paint nothing by accident.
        let blind = std::env::var_os(ss550_engine::config::NO_DECK_ENV).is_some();
        let worker_error = if blind {
            drop((command_rx, result_tx));
            None
        } else {
            match std::thread::Builder::new()
                .name("deck-connector".to_string())
                .spawn(move || connector_worker(command_rx, result_tx, load_deck_path()))
            {
                Ok(_) => None,
                Err(err) => Some(format!("Could not start the connection worker: {err}")),
            }
        };

        // Bindings belong to a deck, and the deck is only known once it
        // answers, so the store starts empty and is loaded on connect.
        let store = ActionStore::new();
        let store_path = None;

        let program_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("C:\\Windows\\notepad.exe"));
        let args_input = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx);
            state.set_auto_grow(2, 5, cx);
            state
        });
        let url_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://twitch.tv/dashboard"));
        // The placeholder is a combination the engine accepts, so the shape of
        // the vocabulary is visible before the first keystroke.
        let keys_input = cx.new(|cx| InputState::new(window, cx).placeholder("ctrl+shift+m"));
        // The title is a word or two, not a path: the placeholder says what the
        // field is for, and the engine's cap is stated under it.
        let title_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("A word for this key"));
        // Enter is the keyboard's way to save the title, the same way the button
        // is the mouse's. Both end in `save_title`, so the two cannot disagree
        // about what "save" means.
        let title_enter = cx.subscribe(&title_input, |this, _input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.save_title(cx);
                cx.notify();
            }
        });
        // The picker's filter. Its placeholder is a program name, not a path:
        // this field searches the list, it does not hold the answer.
        let app_query = cx.new(|cx| InputState::new(window, cx).placeholder("Search applications"));
        // Typing in the picker does not touch the controller, so the field's own
        // change event is what redraws the list around it. Without this the rows
        // would keep showing whatever the last controller change left behind.
        let app_query_changes =
            cx.subscribe(&app_query, |_this, _input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            });
        // The family list's own filter, and the same arrangement: the field
        // redraws the rows around it without touching the style.
        let font_query = cx.new(|cx| InputState::new(window, cx).placeholder("Search fonts"));
        // The name a new page takes. Nothing else is needed to make one: a page
        // is a name and an empty set of keys.
        let page_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("A name for the page"));
        // What the active profile would be called after RENAME ACTIVE.
        let profile_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("A new name for the profile"));
        let font_query_changes =
            cx.subscribe(&font_query, |_this, _input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            });

        // The runner delivers keystrokes through the platform sender. It is
        // built here, before the runner, because the runner owns the only
        // handle to it: without one, a `Keys` action is refused rather than
        // silently accepted.
        let key_sender: Arc<dyn ss550_engine::KeySender> = Arc::new(ss550_tray::SystemKeySender);

        // The tray icon is what keeps the controller alive with no window on
        // screen. Without it the app still runs, but only as a window.
        let (tray, tray_error) = match TrayHost::spawn("Sh-Deck") {
            Ok(host) => (Some(host), None),
            Err(error) => (None, Some(format!("Tray icon not available: {error}"))),
        };

        // The native handle is what lets the window leave the screen without
        // being destroyed: the controller owns the deck, the poll timer and
        // the action runner, and all three have to outlive the window.
        let native = native_handle(window);
        let window_handle = Window::window_handle(window);

        // The X button hides the window instead of closing it. The app only
        // leaves when the user says so, from the tray.
        window.on_window_should_close(cx, move |_window, _cx| {
            if let Some(hwnd) = native {
                let _ = ss550_tray::hide_window(hwnd);
            }
            false
        });

        // A registration is not the same as a working one: the switch must
        // never say "on" while Windows would try to start a file that is gone.
        let (starts_with_windows, autostart_note) = match autostart::sign_in() {
            Ok(autostart::SignIn::On(_)) => (true, None),
            Ok(autostart::SignIn::Off) => (false, None),
            Ok(autostart::SignIn::Broken { executable, .. }) => {
                let missing = executable
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "a file it cannot name".to_string());
                (
                    false,
                    Some(format!(
                        "Windows still has a sign-in entry that points at {missing}, which is \
                         not there. Turn START WITH WINDOWS on again to point it at this copy."
                    )),
                )
            }
            Err(error) => (false, Some(format!("Windows sign-in entry: {error}"))),
        };

        // The sign-in entry names a copy of this program, and that copy is only
        // written when the switch is turned on. Refreshing it here is what makes
        // a new build the one Windows opens at the next sign-in.
        let sign_in_copy = if starts_with_windows {
            sync_sign_in_copy()
        } else {
            None
        };

        // Two timers in one loop: key presses are polled fast, everything else
        // runs at the slower maintenance rate.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL).await;
            let alive = this.update(cx, |this, cx| {
                let mut changed = this.poll_deck();
                // A composed face is painted as soon as it exists. It used to
                // wait for the maintenance tick, which left the key marked as in
                // flight for up to half a second — long enough that the next
                // sample found it busy and the key skipped a second of a clock.
                changed |= this.collect_faces();
                // A rebuild that finds titled keys and no font asks for the scan
                // here, where there is a context to spawn it with.
                this.pump_fonts(cx);
                if this.last_tick.elapsed() >= TICK {
                    this.last_tick = Instant::now();
                    this.maintain();
                    changed = true;
                }
                changed |= this.drain_action_failures();
                // The picture dialog answered on its own thread; its answer is
                // state like any other, so it is applied here, where a mutable
                // borrow is normal. Importing and assigning happen on this
                // line, never while the dialog runs.
                changed |= this.collect_picture_pick();
                changed |= this.handle_tray(cx);
                // A second start asked for the window; showing it is the whole
                // answer, and it must not be lost while the window is hidden.
                if this.singleton.as_ref().is_some_and(|claim| claim.asked()) {
                    this.show_window(cx);
                    changed = true;
                }
                // A site's icon is a request, so it is started here and its
                // answer arrives as state: the frame never waits for a host.
                this.fetch_site_icons(cx);
                if changed {
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();

        // The title style is read once, before the first key is painted. A file
        // that exists and cannot be read replaces the startup note: the user's
        // choice was lost, and that matters more than "looking for the deck".
        let (title_style, title_style_note) = titlestyle::load();
        // The lines the Stats page paints by. A file that cannot be read is a
        // sentence in the status line, not a refusal to start.
        let (thresholds, thresholds_note) = thresholds::load();
        let startup_note = thresholds_note.clone().or(title_style_note);

        let mut controller = Self {
            session: None,
            link: if blind {
                Link::Waiting
            } else {
                Link::Connecting
            },
            note: startup_note
                .unwrap_or_else(|| match worker_error {
                    Some(err) => err,
                    None if blind => "Started without looking for a deck.".into(),
                    None => "Looking for the deck…".into(),
                })
                .into(),
            page: Page::Deck,
            paint: Paint::Blank,
            slots: Vec::new(),
            pressed: Vec::new(),
            brightness: deck_api::VALUE_MAX,
            commands,
            results,
            generation: 0,
            pending: false,
            attempts: 0,
            last_tick: Instant::now(),
            reconnect_at: Instant::now(),
            store,
            store_path,
            recents: Recents::new(),
            runner: ActionRunner::with_key_sender(key_sender),
            selected_key: KeyNumber::default(),
            draft: DraftKind::Launch,
            draft_plugin: None,
            draft_plugin_filter: None,
            program_input,
            args_input,
            url_input,
            app_list: apps::Apps::Unscanned,
            app_query,
            app_picker_open: false,
            draft_program: None,
            _app_query_changes: app_query_changes,
            keys_input,
            title_input,
            _title_enter: title_enter,
            tray,
            tray_error,
            native,
            window_handle,
            starts_with_windows,
            autostart_note,
            sign_in_copy,
            version_clicks: 0,
            frames: 0,
            last_frame: Duration::ZERO,
            blind,
            profile_store: ss550_engine::ProfileStore::new(),
            assets: ss550_engine::AssetLibrary::at_config_dir(&ss550_engine::config_dir()),
            picture: PictureDialog::default(),
            previews: BTreeMap::new(),
            icons: icons::IconCache::new(),
            tiles: None,
            title_style: title_style.clone(),
            page_path: Vec::new(),
            viewport_h: 880.,
            text: None,
            text_scan: false,
            fonts_wanted: false,
            font_families: Vec::new(),
            font_picker: None,
            font_query,
            page_input,
            profile_input,
            _font_query_changes: font_query_changes,
            singleton: None,
            // The catalogue is a list this build carries; the sampler is a
            // thread that starts here and is joined when the window is dropped,
            // so a machine is walked for exactly as long as the app runs.
            plugins: Catalogue::built_in(),
            sampler: Some(Sampler::start(ss550_plugins::SAMPLE_INTERVAL)),
            samples: Samples::new(),
            sampler_gone: None,
            drawn_readings: Vec::new(),
            faces: facework::FaceWorker::start(),
            faces_in_flight: BTreeSet::new(),
            face_tiles: BTreeMap::new(),
            faces_gone: None,
            history: history::History::new(),
            thresholds,
            thresholds_note,
            tray_tooltip: String::new(),
        };

        // Look for the deck right away: its settle window starts the moment
        // it appears, so every second counts.
        controller.request_attempt();
        controller
    }

    /// Applies every result the worker produced since the last tick.
    fn collect_attempts(&mut self) {
        let attempts: Vec<Attempt> = self.results.try_iter().collect();
        for attempt in attempts {
            match attempt {
                Attempt::Connected {
                    generation,
                    deck,
                    path,
                } => {
                    if generation != self.generation {
                        // A newer request replaced this one; its result wins.
                        continue;
                    }
                    self.pending = false;
                    self.attempts = 0;
                    // Reset backoff on successful connection
                    self.reconnect_at = Instant::now();
                    // Remember where it was found: the next start opens that
                    // path instead of walking the machine's HID devices.
                    save_deck_path(path.as_deref());
                    self.adopt(deck);
                }
                Attempt::Failed { generation, error } => {
                    if generation != self.generation {
                        continue;
                    }
                    self.pending = false;
                    let message = error.to_string();

                    // Every failed attempt backs off, so a deck that stays
                    // unplugged is polled once a minute, not twice a second.
                    self.schedule_retry();
                    let delay = self.retry_delay().as_secs();
                    // The sentence on screen is replaced by the next attempt;
                    // this line is what a start that never connected leaves
                    // behind.
                    diagnostics::record(&format!(
                        "connect failed: {message} — retrying in {delay} s (attempt {})",
                        self.attempts
                    ));

                    match error {
                        provider::ConnectError::NotFound(_) => {
                            // The deck is simply not there. This is not a
                            // failure: the app keeps looking for it.
                            self.link = Link::Waiting;
                            self.note = format!(
                                "{message} — the deck is not on the bus. \
                                 Retrying in {delay} s (attempt {}).",
                                self.attempts
                            )
                            .into();
                        }
                        provider::ConnectError::Other(_) => {
                            self.link = Link::Lost;
                            self.note = format!(
                                "{message} — retrying in {delay} s (attempt {}).",
                                self.attempts
                            )
                            .into();
                        }
                    }
                }
            }
        }
    }

    /// Takes the deck that just answered and loads the bindings that belong to
    /// it.
    ///
    /// Everything sized by the deck — the tile slots, the pressed flags, the
    /// selected key — is built here from the deck's own geometry, once. Nothing
    /// below has a key count of its own.
    fn adopt(&mut self, deck: ss550_driver::Ss550Deck) {
        let session = session::DeckSession::new(Box::new(deck));
        let key_count = session.key_count();
        // What the app found, and what shape it is: the one line a start that
        // "does not connect" needs in order to say why.
        diagnostics::record(&format!("connected: {}", session.label()));

        let path = ss550_engine::bindings_path_for(session.key());
        let (store, mut note) = self.load_store(&path, &session);
        // Recents live beside the bindings and are read at the same moment:
        // both belong to the deck that just answered, and a file that cannot be
        // read starts empty with its own sentence, never a half-filled list.
        let (recents, recents_note) =
            load_recents_at(&ss550_engine::recents_path_for(session.key()));
        note = join_notes(note, recents_note);

        // Load profiles for this deck
        let deck_key = session.key().to_string();
        let config_dir = ss550_engine::config_dir();
        let loaded_store = ss550_engine::ProfileStore::load(&deck_key, &config_dir)
            .unwrap_or_else(|_| ss550_engine::ProfileStore::new());
        self.profile_store = loaded_store;

        self.store = store;
        self.store_path = Some(path);
        self.recents = recents;
        self.slots = vec![Slot::Off; key_count];
        self.pressed = vec![false; key_count];
        self.drawn_readings = vec![None; key_count];
        // A deck change is a new tile size, so every face composed for the old
        // one is worthless, and a job in flight belongs to a deck that is gone.
        self.face_tiles.clear();
        self.faces_in_flight.clear();
        if !key_exists(&session, self.selected_key) {
            self.selected_key = KeyNumber::default();
        }
        self.session = Some(session);
        self.link = Link::Connected;

        // The tile cache is per deck: it holds pictures already encoded at this
        // deck's image size and byte limit, and a different deck would accept
        // neither.
        let spec = self
            .session
            .as_ref()
            .map(session::DeckSession::image_spec)
            .expect("the session was just stored");
        self.tiles = Some(render::TileCache::new(spec));

        // The profile is where this deck's bindings live now, and the per-deck
        // file is where they lived before profiles existed. A profile that has
        // never been saved carries none, so taking it as the truth would hide
        // every binding the file still holds — which is exactly what happened
        // the first time the deck key changed. The file is read once into the
        // active profile, and written back through the normal save path.
        if let Some(profile) = self.profile_store.active_profile() {
            if profile.root.bindings.is_empty() && !self.store.is_empty() {
                let bindings = self.store.clone();
                if let Err(error) = self.profile_store.update_bindings(
                    &bindings,
                    None,
                    &deck_key,
                    &ss550_engine::config_dir(),
                ) {
                    // Joined, not replaced: a recents file that also failed to
                    // read has its own sentence, and the status line is the only
                    // place either one can be seen.
                    note = join_notes(
                        note,
                        Some(format!(
                            "The bindings were read but not saved into the profile: {error}"
                        )),
                    );
                }
            }
        }

        // Apply the active profile on connect
        if let Some(profile) = self.profile_store.active_profile() {
            self.store = profile.root.bindings.clone();
            // Every key's face is decided from what it runs, so the icons of
            // those programs have to be read before the keys are painted. This
            // is the deck change, and it is the only reason the cache is
            // rebuilt here.
            self.rebuild_icons();
            let slot = self.paint_from_profile(&profile, key_count);
            let default_colour = self.paint.rgb();
            self.apply_all_appearances(slot, &profile.root.appearance, default_colour, key_count);
        } else {
            self.rebuild_icons();
            let slot = self.paint.slot();
            self.apply_all(slot);
        }

        if self.session.is_some() {
            self.note = match note {
                Some(note) => note.into(),
                None => format!(
                    "Connected — profile '{}' active.",
                    self.profile_store.active_name().unwrap_or("default")
                )
                .into(),
            };
        }

        // Last, because it reads the profile and the deck's tile size: the grid
        // can now paint every picture the profile says a key shows.
        self.rebuild_previews();
    }

    /// Fits every picture the active profile shows to the deck's tile, once.
    ///
    /// This is the only place the grid's pictures are decoded and encoded: it
    /// runs on a deck change, a profile switch or an assignment, and never while
    /// a frame is being drawn.
    fn rebuild_previews(&mut self) {
        // The icons are part of the same promise — a tile that draws a program
        // draws the picture the deck received — so they are rebuilt with them.
        self.rebuild_icons();

        // Only the pictures the active profile names are kept: one that is no
        // longer on any key is dropped, and one that still is is never decoded a
        // second time. An assignment therefore decodes the picture it added and
        // nothing else.
        let names: BTreeSet<ss550_engine::AssetName> = match self.current_keys() {
            Some(keys) => keys
                .appearance
                .values()
                .filter_map(|appearance| appearance.asset().cloned())
                .collect(),
            None => BTreeSet::new(),
        };
        self.previews.retain(|name, _| names.contains(name));
        self.ensure_previews(names);
    }

    /// Fits one picture to the deck's tile, ready for the renderer.
    ///
    /// `None` when there is no deck to state the tile size, the file cannot be
    /// read, or the bytes are not a picture this build decodes.
    fn fit_preview(&self, name: &ss550_engine::AssetName) -> Option<Arc<gpui_kit::RenderImage>> {
        let spec = self
            .session
            .as_ref()
            .map(session::DeckSession::image_spec)?;
        let source = self.assets.read(name).ok()?;
        let frame = render::preview_frame(spec, name, &source).ok()?;
        Some(Arc::new(gpui_kit::RenderImage::new([frame])))
    }

    /// Builds the fitted preview of every named picture that is not cached yet.
    ///
    /// A name with no file behind it — a picture deleted from the folder while a
    /// profile still references it — is skipped, not reported: the tile falls
    /// back to its flat colour and the note keeps saying what the user last did.
    fn ensure_previews<I>(&mut self, names: I)
    where
        I: IntoIterator<Item = ss550_engine::AssetName>,
    {
        for name in names {
            if self.previews.contains_key(&name) {
                continue;
            }
            if let Some(image) = self.fit_preview(&name) {
                self.previews.insert(name, image);
            }
        }
    }

    /// Reads the picture of every program the deck can run.
    ///
    /// One extraction per program path, on the change that made it necessary — a
    /// rebind, a profile switch or a deck change — and never while a frame is
    /// being drawn. The cache keeps what it built, so a rebind that touches one
    /// key asks Windows about one program.
    ///
    /// The program in the editor's field comes with them: it is not a binding
    /// yet, but the key it is about to become will show exactly this picture.
    fn rebuild_icons(&mut self) {
        let spec = self.session.as_ref().map(session::DeckSession::image_spec);
        // The title is per key and the icon is per program, so the cache is
        // told both: a key that says something gets a picture with the words in
        // it. A key with no binding has no action to wear a title, and its
        // titled colour is composed on demand instead.
        // The words and the styles of the page the deck is showing, taken as
        // owned values first: the cache and the font need `self` mutably, and a
        // borrow of the page cannot outlive that.
        let words: Vec<(Option<String>, titlestyle::TitleStyle)> = {
            let keys = self.current_keys();
            let default = &self.title_style;
            self.store
                .bindings()
                .iter()
                .map(|binding| {
                    let key = u16::from(binding.key.get());
                    let title = keys.and_then(|keys| keys.title(key)).map(str::to_string);
                    let style = keys
                        .and_then(|keys| keys.title_style(key))
                        .cloned()
                        .unwrap_or_else(|| default.clone());
                    (title, style)
                })
                .collect()
        };
        let faces: Vec<icons::KeyArt<'_>> = self
            .store
            .bindings()
            .iter()
            .zip(&words)
            .map(|(binding, (title, style))| icons::KeyArt {
                action: &binding.action,
                title: title.as_deref(),
                style: style.clone(),
            })
            .collect();
        // Words need a font, and a font needs a deck: with no image size there is
        // no payload to draw them into, so the scan would be spent for nothing.
        // The scan itself is started by [`Controller::pump_fonts`], from the one
        // loop that runs on the UI thread and has the context to spawn with: a
        // rebuild is called from paths that have no context of their own.
        let key_count = self
            .session
            .as_ref()
            .map_or(0, session::DeckSession::key_count);
        let wants_words = needs_fonts(spec, self.current_keys(), key_count)
            || (spec.is_some() && !self.reading_keys().is_empty());
        if wants_words && self.text.is_none() && !self.text_scan {
            self.fonts_wanted = true;
        }
        let text = match wants_words {
            true => self.text.as_mut(),
            false => None,
        };
        self.icons
            .prepare(spec, &faces, self.draft_program.as_deref(), text);
    }

    /// Starts the font scan if a rebuild asked for one.
    ///
    /// Called from the window's own loop, which is the one place that both runs
    /// on the UI thread and has a context to spawn with; the rebuild path runs
    /// from places that have neither.
    fn pump_fonts(&mut self, cx: &mut Context<Self>) {
        if self.fonts_wanted {
            self.fonts_wanted = false;
            self.request_fonts(cx);
        }
    }

    /// How one key's title is drawn: its own style, or the app's default.
    ///
    /// One place resolves the fallback, so a key, a paint and a preview cannot
    /// disagree about which font it wears.
    fn style_for(&self, key: u16) -> titlestyle::TitleStyle {
        resolve_style(self.current_keys(), &self.title_style, key)
    }

    /// Asks the background executor for the system's fonts, once.
    ///
    /// Reading them is the one slow step in drawing a title — `cosmic-text`
    /// documents it as up to a second on a release build — so it never runs on
    /// the UI thread. Until it answers, a titled key shows its picture without
    /// words: a key a moment behind is better than a window that stops
    /// answering, and it is the same rule the site icons already follow.
    fn request_fonts(&mut self, cx: &mut Context<Self>) {
        if self.text.is_some() || self.text_scan {
            return;
        }
        self.text_scan = true;
        cx.spawn(async move |this, cx| {
            let engine = cx
                .background_spawn(async { titletext::TextEngine::new() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.text = Some(engine);
                this.text_scan = false;
                // The list the settings page offers is read from the same
                // database, so opening it later is a lookup.
                this.font_families = this
                    .text
                    .as_mut()
                    .map(titletext::TextEngine::families)
                    .unwrap_or_default();
                // The words can be drawn now, so the keys that were painted
                // without them are recomposed and repainted — once. Readings are
                // not here: their faces have a font of their own, on the worker.
                this.rebuild_icons();
                for key in this.titled_keys() {
                    this.repaint_key(key);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The keys whose profile gives them words.
    fn titled_keys(&self) -> Vec<u16> {
        let count = self
            .session
            .as_ref()
            .map_or(0, session::DeckSession::key_count);
        match self.current_keys() {
            Some(keys) => (1..=count as u16)
                .filter(|key| keys.title(*key).is_some())
                .collect(),
            None => Vec::new(),
        }
    }

    /// The titled keys that follow the app's default style.
    ///
    /// The others say so themselves, in their profile, and a change of the
    /// default must not repaint them.
    fn inherited_title_keys(&self) -> Vec<u16> {
        let Some(keys) = self.current_keys() else {
            return Vec::new();
        };
        self.titled_keys()
            .into_iter()
            .filter(|key| keys.title_style(*key).is_none())
            .collect()
    }

    /// Fetches the icon of every site a bound URL points at, off the UI thread.
    ///
    /// Only a key that actually shows what it runs asks for one. A key the user
    /// painted, or gave a picture, has an appearance, and the appearance wins
    /// over the action's own icon — so no request is made for it. That is the
    /// same ordering the tile obeys, stated once in [`ui::deck::face_for`].
    ///
    /// Each host is asked once: the cache answers whether it already holds a
    /// picture, whether a request is in flight, or whether the site was already
    /// asked and had none. The request and the decode both run on the background
    /// executor, so a site that answers in twenty milliseconds and a site that
    /// never answers cost this frame the same: nothing.
    fn fetch_site_icons(&mut self, cx: &mut Context<Self>) {
        if self.session.is_none() {
            return;
        }

        // Every key that points at a site is grouped under its host: two keys on
        // the same site share one request, and both are repainted when it
        // answers, because the deck must not keep a letter the window dropped.
        let mut wanted: Vec<(Vec<KeyNumber>, String)> = Vec::new();
        for binding in self.store.bindings() {
            let Action::OpenUrl { url } = &binding.action else {
                continue;
            };
            let Some(host) = favicons::host_of(url) else {
                continue;
            };
            // The colour or the picture the user chose is what the key shows,
            // so the site's icon is neither needed nor requested.
            let key = u16::from(binding.key.get());
            let chosen = self
                .current_keys()
                .and_then(|keys| keys.appearance_for(key));
            if chosen.is_some() {
                continue;
            }
            // The deck is showing a flat colour on this key, and the tile shows
            // the same, so there is no face to fill.
            let index = usize::from(binding.key.get().saturating_sub(1));
            if self.slots.get(index) != Some(&Slot::Image) {
                continue;
            }
            match wanted.iter_mut().find(|(_, known)| *known == host) {
                Some((keys, _)) => keys.push(binding.key),
                None => wanted.push((vec![binding.key], host)),
            }
        }

        for (keys, host) in wanted {
            if !self.icons.begin_site(&host) {
                continue;
            }
            let config = ss550_engine::config_dir();
            let site = host.clone();
            cx.spawn(async move |this, cx| {
                let picture = cx
                    .background_spawn(async move { favicons::fetch(&site, &config) })
                    .await;
                let found = picture.is_some();
                let _ = this.update(cx, |this, cx| {
                    this.icons.finish_site(&host, picture.as_ref());
                    if found {
                        // The picture arrived after the last rebuild, so the
                        // titled variant composed from it does not exist yet:
                        // recomposing here is what keeps the words on a key
                        // whose icon was fetched. Without it the key loses its
                        // title until the next profile or style change.
                        this.rebuild_icons();
                        // Every key on this site now wears its mark, and the
                        // deck receives the same picture on its next paint.
                        for number in &keys {
                            this.repaint_key(u16::from(number.get()));
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// The action bound to a key, if any.
    ///
    /// The key is the deck's 1-based number, which is what a binding and a
    /// profile appearance are keyed by.
    fn action_for(&self, key: u16) -> Option<&Action> {
        let number = KeyNumber::new(u8::try_from(key).ok()?).ok()?;
        self.store.get(number)
    }

    /// Loads the bindings for one deck.
    ///
    /// The per-deck file is the one this build writes. The pre-M4 file — one
    /// file for every deck — is read once and copied over only when every key
    /// it binds exists on the deck in front of the window: a binding that would
    /// have nowhere to go is not silently repointed at another key, and it is
    /// not thrown away either.
    fn load_store(
        &self,
        path: &std::path::Path,
        session: &session::DeckSession,
    ) -> (ActionStore, Option<String>) {
        if path.exists() {
            return match ActionStore::load(path) {
                Ok(store) => (store, None),
                Err(error) => (
                    ActionStore::new(),
                    Some(format!("Bindings not loaded: {error}")),
                ),
            };
        }

        let legacy = ss550_engine::bindings_path();
        if legacy == path || !legacy.exists() {
            return (ActionStore::new(), None);
        }

        match ActionStore::load(&legacy) {
            Ok(store)
                if store
                    .bindings()
                    .iter()
                    .all(|binding| key_exists(session, binding.key)) =>
            {
                let note = store
                    .save(path)
                    .err()
                    .map(|error| format!("Bindings were read but not copied: {error}"));
                (store, note)
            }
            Ok(_) => (
                ActionStore::new(),
                Some(
                    "The saved bindings belong to a different deck, so they were left alone."
                        .to_string(),
                ),
            ),
            Err(error) => (
                ActionStore::new(),
                Some(format!("Bindings not loaded: {error}")),
            ),
        }
    }

    /// How long to wait before the next attempt.
    ///
    /// The first failure waits [`RETRY_INTERVAL`], and every failure after it
    /// doubles the wait, up to [`MAX_RETRY_INTERVAL`]. The shift is clamped
    /// before it is applied so a long absence cannot overflow it.
    fn retry_delay(&self) -> Duration {
        let doublings = self.attempts.saturating_sub(1).min(5);
        RETRY_INTERVAL
            .saturating_mul(1u32 << doublings)
            .min(MAX_RETRY_INTERVAL)
    }

    /// Counts one failure and postpones the next attempt by the backoff.
    ///
    /// Every failed attempt goes through here, not only the disconnect the
    /// window noticed: a deck that stays away must keep backing off, or the
    /// app spams the USB bus for as long as it is unplugged.
    fn schedule_retry(&mut self) {
        self.attempts = self.attempts.saturating_add(1);
        self.reconnect_at = Instant::now() + self.retry_delay();
    }

    /// Queues one connection attempt for the worker.
    ///
    /// The generation counter makes a late result harmless: if a newer
    /// request was made in the meantime, the old result is dropped instead
    /// of replacing a fresh handle with a stale one.
    fn request_attempt(&mut self) {
        if self.blind {
            // The window was told not to look for a deck, and that includes
            // the retries: nothing is opened until it is started without it.
            return;
        }
        let generation = self.generation + 1;
        if self.commands.try_send(generation).is_err() {
            // A request is already queued: it will produce a fresh attempt.
            return;
        }
        self.generation = generation;
        self.pending = true;
        self.link = Link::Connecting;
        // The start of the cadence, in the same log the failures land in: a
        // boot that reads "connecting" every few seconds and never "connected"
        // is a different bug from one that never logs a second line.
        diagnostics::record(&format!(
            "connecting (attempt {}, waiting up to {} s)",
            self.attempts.saturating_add(1),
            LONG_CONNECT_TIMEOUT.as_secs()
        ));
    }

    /// Moves the window to a page.
    ///
    /// No page needs anything built before its first frame: the only thing that
    /// ever did was the picture gallery, and that is now a list inside the key
    /// editor, opened by its own button.
    fn show_page(&mut self, page: Page) {
        self.page = page;
    }

    /// Drops whatever link there is and asks the worker for a fresh attempt.
    ///
    /// The backoff starts over so the click answers immediately, and the note
    /// says so, because a button that appears to do nothing is worse than no
    /// button.
    fn reconnect(&mut self) {
        self.drop_deck();
        self.link = Link::Connecting;
        self.attempts = 0;
        self.note = "Reconnecting…".into();
        self.request_attempt();
    }

    /// Drops the deck after its bytes stopped moving.
    ///
    /// Everything sized by the deck goes with it, so nothing in the window can
    /// index a grid that is no longer there.
    fn drop_deck(&mut self) {
        self.session = None;
        self.slots.clear();
        self.pressed.clear();
        self.link = Link::Lost;
    }

    /// Switches to a new profile and applies it to the deck.
    fn switch_profile(&mut self, name: &str) {
        // A profile starts at its root: a folder of the profile we came from is
        // not a place in this one.
        self.page_path.clear();
        if let Err(e) = self.profile_store.activate(name) {
            self.note = format!("Failed to switch profile: {e}").into();
            return;
        }

        // If deck is connected, apply the profile
        if let Some(key_count) = self.session.as_ref().map(session::DeckSession::key_count) {
            // Get profile data before mutating self.store
            let profile_data = self.profile_store.active_profile();

            if let Some(profile) = profile_data {
                // Apply bindings
                self.store = profile.root.bindings.clone();

                // The keys of this profile run different programs, so their
                // icons are read before anything is painted.
                self.rebuild_icons();

                // Paint all keys according to profile colors or slot
                let slot = self.paint_from_profile(&profile, key_count);
                let default_colour = self.paint.rgb();
                self.apply_all_appearances(
                    slot,
                    &profile.root.appearance,
                    default_colour,
                    key_count,
                );

                self.note = format!("Switched to profile: {name}").into();
            }
        } else {
            self.note =
                format!("Switched to profile: {name} (bindings will apply on connect)").into();
        }

        // Every profile names its own pictures, and the tiles show the active
        // one. Building them here keeps the decode out of the frame.
        self.rebuild_previews();
    }

    /// The fill a profile gives a key that names no appearance of its own.
    ///
    /// A profile with no slot, or one this build does not know, is **blank**:
    /// the old fallback was white, which is how every untouched key ended up
    /// glowing on a desk where nothing was bound to it.
    fn paint_from_profile(&self, profile: &ss550_engine::Profile, _key_count: usize) -> Slot {
        match profile.root.slot_name() {
            "white" => Slot::White,
            "red" => Slot::Red,
            _ => Slot::Off,
        }
    }

    /// Reports what went wrong, and drops the deck only when the bytes failed.
    ///
    /// A typed refusal is the deck answering: the link is fine, and the window
    /// must not claim it lost a deck that just told it exactly what it cannot
    /// do. Transport failure is the only thing that means the link is gone.
    fn report(&mut self, error: session::SessionError) {
        // Both of these mean the deck is no longer there to talk to: a transport
        // failure on a read or a write, and the device not being enumerable at
        // all. Anything else — a refusal, a wrong size — leaves the session
        // alone and is only a sentence.
        let lost = matches!(
            error,
            session::SessionError::Deck(DeckError::Transport(_) | DeckError::DeviceNotFound)
        );
        if lost {
            // One path for a deck that went away, whichever call noticed it: the
            // poll, a paint or the heartbeat. The reason is kept in the sentence
            // because the retry alone does not say what failed.
            let reason = error.to_string();
            self.handle_disconnect(Some(&reason));
        } else {
            self.note = error.to_string().into();
        }
    }

    /// [`Controller::report`] for the calls that already speak `DeckError`.
    fn report_deck(&mut self, error: DeckError) {
        self.report(session::SessionError::Deck(error));
    }

    /// Paints one key with the colour picked in the toolbar.
    fn paint_key(&mut self, index: usize) {
        let slot = self.paint.slot();
        let colour = self.paint.rgb();
        let result = match self.session.as_mut() {
            Some(session) => session.paint(index, colour),
            None => {
                self.note = "Not connected. Press RECONNECT first.".into();
                return;
            }
        };
        match result {
            Ok(()) => {
                if let Some(entry) = self.slots.get_mut(index) {
                    *entry = slot;
                }
                self.note = format!("Key {} painted {}.", index + 1, slot_name(slot)).into();
            }
            Err(error) => self.report(error),
        }
    }

    /// Blanks the key the bindings panel is editing.
    ///
    /// Only offered when the deck advertises single-key clear; a deck that
    /// cannot do it would refuse the call, and the refusal is what the window
    /// reports rather than a key that keeps showing its old image.
    fn clear_painted_key(&mut self, index: usize) {
        let result = match self.session.as_mut() {
            Some(session) => session.clear_key(index),
            None => {
                self.note = "Not connected. Press RECONNECT first.".into();
                return;
            }
        };
        match result {
            Ok(()) => {
                if let Some(entry) = self.slots.get_mut(index) {
                    *entry = Slot::Off;
                }
                self.note = format!("Key {} cleared (blank).", index + 1).into();
            }
            Err(error) => self.report(error),
        }
    }

    /// Paints every key with one colour.
    fn apply_all(&mut self, slot: Slot) {
        let colour = self.paint.rgb();
        let key_count = self.session.as_ref().map_or(0, |s| s.key_count());
        self.apply_all_appearances(slot, &BTreeMap::new(), colour, key_count)
    }

    /// Paints every key, honouring what the profile says each one shows.
    ///
    /// A key the profile says nothing about shows what it runs: its program's
    /// icon, or the initial of what it runs, encoded once at the deck's image
    /// size. A key with no action gets `default_colour`, and a key the profile
    /// gives an appearance gets exactly that — a flat colour, or a picture read
    /// from the library, fitted and encoded within the deck's byte limit.
    ///
    /// Every face is decided before the first write, so a refusal halfway
    /// through cannot leave the window and the deck disagreeing about the keys
    /// that came after it.
    fn apply_all_appearances(
        &mut self,
        slot: Slot,
        appearances: &BTreeMap<u16, ss550_engine::Appearance>,
        default_colour: [u8; 3],
        key_count: usize,
    ) {
        if self.session.is_none() {
            self.note = "Not connected. Press RECONNECT first.".into();
            return;
        }

        let spec = self.session.as_ref().map(session::DeckSession::image_spec);
        // The title is drawn on top of whichever face wins, so it is read from
        // the profile once, before the plans are made — as an owned value, since
        // the plan needs the cache and the font mutably.
        let titles: Vec<Option<String>> = (0..key_count)
            .map(|index| {
                let key = key_number(index);
                self.current_keys()
                    .and_then(|keys| keys.title(key))
                    .map(str::to_string)
            })
            .collect();
        let actions: Vec<Option<Action>> = (0..key_count)
            .map(|index| {
                let key = key_number(index);
                self.action_for(key).cloned()
            })
            .collect();
        // The style each key wears: its own when the profile gives one, the
        // app's default otherwise.
        let styles: Vec<titlestyle::TitleStyle> = (0..key_count)
            .map(|index| {
                let key = key_number(index);
                self.style_for(key)
            })
            .collect();
        let mut plans: Vec<Tile> = Vec::with_capacity(key_count);
        for index in 0..key_count {
            let key = key_number(index);
            plans.push(plan_tile(
                appearances.get(&key),
                actions[index].as_ref(),
                titles[index].as_deref(),
                &styles[index],
                default_colour,
                &mut self.icons,
                &mut self.text,
            ));
        }

        // A key bound to a plugin shows the reading instead of the face its
        // appearance would give it. The face is composed off this thread, so the
        // key is queued here and painted by `collect_faces` when its bytes
        // arrive; a sentence about a missing cell is taken now, before the first
        // byte of the page goes out.
        let mut painted = vec![slot; key_count];
        for (index, entry) in painted.iter_mut().enumerate() {
            let Some(action) = actions.get(index).and_then(Option::as_ref) else {
                continue;
            };
            if !action.is_reading() {
                continue;
            }
            let key = key_number(index);
            self.queue_face(key, action);
            *entry = self.slots.get(index).copied().unwrap_or(slot);
        }

        let Some(session) = self.session.as_mut() else {
            return;
        };

        let mut failure = None;
        for (index, plan) in plans.iter().enumerate() {
            // A key bound to a reading was queued above and is painted when its
            // face arrives, so its old face stays as it is until then. Nothing
            // here can fail for it, and nothing here may stop the page for it.
            if actions
                .get(index)
                .and_then(Option::as_ref)
                .is_some_and(Action::is_reading)
            {
                painted[index] = self.slots.get(index).copied().unwrap_or(slot);
                continue;
            }
            let result = match plan {
                Tile::Asset { name, title, style } => {
                    let tile = asset_tile(
                        &self.assets,
                        self.tiles.as_mut(),
                        spec,
                        name,
                        title.as_deref(),
                        style,
                        &mut self.text,
                    );
                    match tile {
                        Ok(jpeg) => Self::paint_over_black(session, index, &jpeg),
                        Err(message) => Err(PaintFailure::Picture(message)),
                    }
                }
                Tile::Art(picture) => Self::paint_over_black(session, index, picture),
                Tile::TitledColour(picture) => Self::paint_over_black(session, index, picture),
                Tile::Colour(rgb) => session.paint(index, *rgb).map_err(PaintFailure::Session),
            };

            if let Err(error) = result {
                failure = Some(error);
                break;
            }
            painted[index] = match plan {
                Tile::Asset { .. } | Tile::Art(_) | Tile::TitledColour(_) => Slot::Image,
                Tile::Colour(_) => slot,
            };
        }

        match failure {
            Some(PaintFailure::Session(error)) => self.report(error),
            Some(PaintFailure::Picture(message)) => self.note = message.into(),
            None => {
                let shown = painted.iter().filter(|slot| **slot == Slot::Image).count();
                self.slots = painted;
                self.note = if shown == 0 {
                    format!("All {key_count} keys painted {}.", slot_name(slot)).into()
                } else {
                    format!("{shown} of {key_count} keys show what they run.").into()
                };
            }
        }
    }

    /// Blanks every key at once.
    fn clear_all(&mut self) {
        let result = match self.session.as_mut() {
            Some(session) => session.clear_all(),
            None => {
                self.note = "Not connected. Press RECONNECT first.".into();
                return;
            }
        };
        match result {
            Ok(()) => {
                self.slots.fill(Slot::Off);
                self.note = "All keys cleared (blank).".into();
            }
            Err(error) => self.report_deck(error),
        }
    }

    /// Sets the display brightness, when the deck has one.
    fn set_brightness(&mut self, value: u8) {
        let result = match self.session.as_mut() {
            Some(session) => session.set_brightness(value),
            None => {
                self.note = "Not connected. Press RECONNECT first.".into();
                return;
            }
        };
        match result {
            Ok(()) => {
                self.brightness = value;
                self.note = format!("Brightness set to {value}%.").into();
            }
            Err(error) => self.report_deck(error),
        }
    }

    /// Keeps a deck that resets itself from resetting.
    ///
    /// A deck that never resets is not asked: the capability decides, so the
    /// heartbeat is not a call the window makes and then has to explain away.
    fn heartbeat(&mut self) {
        let result = match self.session.as_mut() {
            Some(session) if session.capabilities().keep_alive() => session.keep_alive(),
            _ => return,
        };
        if let Err(error) = result {
            self.report_deck(error);
        }
    }

    /// Reads every key event waiting on the deck.
    ///
    /// Returns whether anything changed and the UI should repaint. The poll is
    /// non-blocking, so this runs on every tick without ever stalling the
    /// window.
    fn poll_deck(&mut self) -> bool {
        let result = match self.session.as_mut() {
            Some(session) => session.poll_events(Duration::ZERO),
            None => return false,
        };
        let events = match result {
            Ok(events) => events,
            Err(error) => {
                self.report_deck(error);
                return true;
            }
        };
        if events.is_empty() {
            return false;
        }

        for event in events {
            let index = usize::from(event.key().index());
            match event.state() {
                ButtonState::Pressed => {
                    if let Some(held) = self.pressed.get_mut(index) {
                        *held = true;
                    }
                    self.run_bound_action(index);
                }
                ButtonState::Released => {
                    if let Some(held) = self.pressed.get_mut(index) {
                        *held = false;
                    }
                }
            }
        }
        true
    }

    /// Runs the action bound to a pressed key, off the UI thread.
    fn run_bound_action(&mut self, idx: usize) {
        let Some(number) = KeyNumber::new(idx as u8 + 1).ok() else {
            return;
        };
        let Some(action) = self.store.get(number).cloned() else {
            self.note = format!("Key #{} pressed - nothing is bound to it yet.", idx + 1).into();
            return;
        };

        // Navigation changes what the deck is showing, so it is applied here,
        // where the pages and the keys drawn from them live. The runner never
        // sees it: a worker with no window cannot swap a page.
        if self.apply_navigation(&action, idx) {
            return;
        }

        // A reading is a gauge, not a button: pressing it refreshes what it
        // shows from the newest sample and runs nothing. Saying which
        // measurement it is keeps the press from feeling like nothing happened.
        if action.is_reading() {
            let key = u16::try_from(idx + 1).unwrap_or(u16::MAX);
            let refreshed = self.repaint_key(key);
            self.note = if refreshed {
                // `describe` starts with the verb, so the template does not add
                // one: "Key #3: Show cpu.usage, measured by the plugin …".
                format!("Key #{}: {}.", idx + 1, action.describe()).into()
            } else {
                self.note.clone()
            };
            return;
        }

        let summary = action.summary();
        match self.runner.submit(action) {
            Ok(()) => self.note = format!("Key #{} pressed - {summary}.", idx + 1).into(),
            Err(err) => {
                self.note = format!(
                    "Key #{} pressed, but the action was refused: {err}",
                    idx + 1
                )
                .into();
            }
        }
    }

    /// The page the deck is showing, or `None` for the root.
    fn current_page(&self) -> Option<&str> {
        self.page_path.last().map(String::as_str)
    }

    /// The keys the deck is showing: the page it is on, or the root.
    fn current_keys(&self) -> Option<&ss550_engine::profiles::PageKeys> {
        self.profile_store.active()?.page(self.current_page())
    }

    /// Applies a navigation action, answering whether it was one.
    ///
    /// A folder that is no longer there, and a step back from the root, are
    /// facts worth a sentence: silently doing nothing reads as a broken key.
    fn apply_navigation(&mut self, action: &Action, idx: usize) -> bool {
        let exists = |page: &str| {
            self.profile_store
                .active()
                .and_then(|profile| profile.page(Some(page)))
                .is_some()
        };
        let outcome = navigate(&mut self.page_path, action, exists);
        match outcome {
            NavOutcome::Entered | NavOutcome::WentBack => {
                self.show_current_page();
                true
            }
            NavOutcome::UnknownPage => {
                let page = match action {
                    Action::Folder { page } => page.as_str(),
                    _ => "",
                };
                self.note = format!(
                    "Key #{} opens \"{page}\", which this profile no longer has.",
                    idx + 1
                )
                .into();
                true
            }
            NavOutcome::AtRoot => {
                self.note = "This is the top page: there is nothing to go back to.".into();
                true
            }
            NavOutcome::NotNavigation => match action {
                // A profile switch is navigation the stack does not hold: it
                // replaces the whole context, so it starts that profile at its
                // root instead of entering it.
                Action::Profile { name } => {
                    let name = name.clone();
                    self.switch_profile(&name);
                    true
                }
                _ => false,
            },
        }
    }

    /// Shows one page of the active profile, so its keys can be edited.
    ///
    /// Opening a page here is the same act as pressing a key that opens it: the
    /// stack grows, and the deck follows. `None` returns to the top page.
    fn show_deck_page(&mut self, page: Option<&str>, cx: &mut Context<Self>) {
        match page {
            Some(page) => self.page_path.push(page.to_string()),
            None => self.page_path.clear(),
        }
        self.show_current_page();
        cx.notify();
    }

    /// Creates a page in the active profile, named by the field on the card.
    fn add_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.page_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.note = "A page needs a name.".into();
            return;
        }
        let added = self
            .profile_store
            .active_mut()
            .is_some_and(|profile| profile.add_page(&name));
        if !added {
            self.note = format!(
                "\"{name}\" is already a page here, or not a name a page can have (up to {} \
                 characters, no control characters).",
                ss550_engine::MAX_PAGE_NAME_CHARS
            )
            .into();
            return;
        }
        self.page_input
            .update(cx, |state, cx| state.set_value(String::new(), window, cx));
        self.save_profile();
        self.note = format!("Page \"{name}\" created. BIND puts the selected key on it.").into();
    }

    /// Renames the active profile to whatever the card's field says.
    ///
    /// A profile is addressed by its name — it is what a `profile` binding holds
    /// and what the store remembers as active — so the engine moves both, and
    /// the deck is painted again because the keys that switch profiles now say a
    /// different name.
    fn rename_active_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(from) = self.profile_store.active_name().map(str::to_string) else {
            self.note = "There is no active profile to rename.".into();
            return;
        };
        let to = self.profile_input.read(cx).value().trim().to_string();
        if to.is_empty() || to == from {
            self.note =
                format!("Write the new name for \"{from}\" in the field, then RENAME ACTIVE.")
                    .into();
            return;
        }
        let Some(deck_key) = self
            .session
            .as_ref()
            .map(|session| session.key().to_string())
        else {
            self.note = "Connect a deck to rename a profile: it is saved per deck.".into();
            return;
        };
        if let Err(error) =
            self.profile_store
                .rename_profile(&from, &to, &deck_key, &ss550_engine::config_dir())
        {
            self.note = error.into();
            return;
        }
        self.profile_input
            .update(cx, |state, cx| state.set_value(String::new(), window, cx));
        // The store the deck runs is a copy of the page's bindings: the rename
        // rewrote the profile, so the copy is read again before the keys are
        // painted, or a key would keep saying the old profile's name.
        self.show_current_page();
        self.note = format!("Profile \"{from}\" is now \"{to}\".").into();
        cx.notify();
    }

    /// Renames a page to whatever the card's field says.
    ///
    /// A page is addressed by its name, so the keys that open it move with it —
    /// the profile does that, on every page. The stack is renamed as well: an
    /// entry still holding the old name would silently show the top page instead
    /// of where the deck was.
    fn rename_page(&mut self, from: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.page_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.note =
                format!("Write the new name for \"{from}\" in the field, then RENAME.").into();
            return;
        }
        let renamed = self
            .profile_store
            .active_mut()
            .is_some_and(|profile| profile.rename_page(from, &name));
        if !renamed {
            self.note = format!(
                "\"{name}\" is already a page here, or not a name a page can have (up to {} \
                 characters, no control characters).",
                ss550_engine::MAX_PAGE_NAME_CHARS
            )
            .into();
            return;
        }
        for open in self.page_path.iter_mut() {
            if open == from {
                *open = name.clone();
            }
        }
        self.page_input
            .update(cx, |state, cx| state.set_value(String::new(), window, cx));
        self.save_profile();
        // The words on the keys that open the page just changed, so the page the
        // deck is showing is painted again from its own keys.
        self.show_current_page();
        self.note = format!("Page \"{from}\" is now \"{name}\".").into();
    }

    /// Removes a page, and every key on it.
    ///
    /// A deck left on a page that is gone would show keys nobody can reach
    /// again, so the stack goes back to the top.
    fn remove_page(&mut self, name: &str, _cx: &mut Context<Self>) {
        let removed = self
            .profile_store
            .active_mut()
            .is_some_and(|profile| profile.remove_page(name));
        if !removed {
            self.note = format!("\"{name}\" is not a page of this profile.").into();
            return;
        }
        if self.page_path.iter().any(|open| open == name) {
            self.page_path.clear();
            self.show_current_page();
        }
        self.save_profile();
        self.note = format!("Page \"{name}\" removed, with its keys.").into();
    }

    /// Binds the selected key to open a page.
    fn bind_selected_to_page(&mut self, page: &str, cx: &mut Context<Self>) {
        self.bind_and_repaint(
            self.selected_key,
            Action::Folder {
                page: page.to_string(),
            },
            cx,
        );
    }

    /// Binds the selected key to step back out of a page.
    fn bind_selected_back(&mut self, cx: &mut Context<Self>) {
        self.bind_and_repaint(self.selected_key, Action::Back, cx);
    }

    /// Binds the selected key to activate another profile.
    fn bind_selected_profile(&mut self, name: &str, cx: &mut Context<Self>) {
        self.bind_and_repaint(
            self.selected_key,
            Action::Profile {
                name: name.to_string(),
            },
            cx,
        );
    }

    /// Puts the deck and the window on the page the stack says.
    ///
    /// The store the deck runs is replaced by the page's own, the icons are read
    /// again because a page binds different programs, and every key is repainted:
    /// a page switch changes all of them by definition.
    fn show_current_page(&mut self) {
        let page = self.current_page().map(str::to_string);
        let keys = match self.current_keys() {
            Some(keys) => keys.clone(),
            None => {
                // The page vanished under the stack: fall back to the root
                // rather than run a set of keys nobody can see.
                self.page_path.clear();
                match self.profile_store.active() {
                    Some(profile) => profile.root.clone(),
                    None => return,
                }
            }
        };
        self.store = keys.bindings.clone();
        self.selected_key = KeyNumber::default();
        // Selection is per page: the key that was selected may run something
        // else here, and the editor is about to show it.
        let _ = page;

        self.rebuild_icons();
        let key_count = self
            .session
            .as_ref()
            .map_or(0, session::DeckSession::key_count);
        let slot = match keys.slot_name() {
            "white" => Slot::White,
            "red" => Slot::Red,
            _ => Slot::Off,
        };
        let default_colour = self.paint.rgb();
        self.apply_all_appearances(slot, &keys.appearance, default_colour, key_count);
        self.note = match self.current_page() {
            Some(page) => format!("Showing \"{page}\".").into(),
            None => "Showing the top page.".into(),
        };
    }

    /// Surfaces any failure the action worker reported.
    fn drain_action_failures(&mut self) -> bool {
        let mut changed = false;
        while let Some(failure) = self.runner.take_failure() {
            self.note = format!("Action failed: {failure}").into();
            changed = true;
        }
        changed
    }

    /// Connection results, retries and the heartbeat, at the slower rate.
    fn maintain(&mut self) {
        self.collect_attempts();

        // Try to reconnect if disconnected and backoff allows
        if self.session.is_none() && !self.pending && Instant::now() >= self.reconnect_at {
            self.request_attempt();
        }

        self.heartbeat();
        // The newest walk over the machine, and the keys it moved. The faces it
        // queues are collected by the fast loop instead: a face that sat here
        // until the next half-second tick would keep its key marked as in flight,
        // and the sample after it would be refused for that key.
        self.pump_samples();
        self.refresh_tray_tooltip();
    }

    /// Handles an unexpected disconnect with exponential backoff.
    ///
    /// The only way in is [`Controller::report`], from a read or a write that
    /// failed on the session the app already holds — the deck going away is a
    /// transport error, not a fact to be discovered by looking. This loop used
    /// to ask Windows for the whole HID inventory instead, twice a second, on
    /// the UI thread: `HidApi::new()` plus a full `refresh_devices()`, which
    /// walks and opens every HID device on the machine. That is the one thing
    /// the app did that touched the pointer's own hardware, and a device that
    /// stalls on that walk takes the rest of the bus down with it.
    fn handle_disconnect(&mut self, reason: Option<&str>) {
        self.drop_deck();
        self.schedule_retry();

        self.note = format!(
            "Connection lost: {}. Retrying in {}s (attempt {}).",
            reason.unwrap_or("the deck is no longer answering"),
            self.retry_delay().as_secs(),
            self.attempts
        )
        .into();
        diagnostics::record(&format!(
            "disconnected: {} — retrying in {} s (attempt {})",
            reason.unwrap_or("no reason given"),
            self.retry_delay().as_secs(),
            self.attempts
        ));
    }

    /// Installs the controller where this user can find and start it.
    ///
    /// Two steps and no installer: the running binary is copied into the
    /// folder the logon entry already names, and a Start Menu entry is written
    /// that starts that copy. Both are per-user; nothing needs administrator
    /// rights and nothing is registered anywhere.
    fn install_app(&mut self) {
        let running = match std::env::current_exe() {
            Ok(path) => path,
            Err(error) => {
                self.note = format!("The running program could not name itself: {error}").into();
                return;
            }
        };
        self.note = match ss550_tray::install::install(&running) {
            Ok(entry) => format!(
                "Installed. \"Sh-Deck\" is now in the Start Menu and in your launcher; \
                 the entry is {}.",
                entry.display()
            )
            .into(),
            Err(error) => format!("The controller could not be installed: {error}").into(),
        };
    }

    /// Does what the user asked for in the tray menu.
    fn handle_tray(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(tray) = &self.tray else {
            return false;
        };
        let mut asked = Vec::new();
        while let Some(command) = tray.try_recv() {
            asked.push(command);
        }
        let mut changed = false;
        for command in asked {
            match command {
                TrayCommand::Show => {
                    self.show_window(cx);
                    changed = true;
                }
                TrayCommand::Quit => {
                    // This line, and the `started` line without it, are how the
                    // log tells a deliberate exit from a crash.
                    diagnostics::record("quit from the tray");
                    // The helper belongs to this window: it would notice within a
                    // minute and leave on its own, and saying so now is the
                    // difference between a minute of a privileged process with
                    // no owner and none.
                    ss550_sensors::helper::stop();
                    cx.quit()
                }
            }
        }
        changed
    }

    /// Brings the controller window back and puts it in front.
    fn show_window(&mut self, cx: &mut Context<Self>) {
        if let Some(hwnd) = self.native {
            let _ = ss550_tray::show_window(hwnd);
        }
        let handle = self.window_handle;
        let _ = cx.update_window(handle, |_, window, _| {
            window.activate_window();
        });
    }

    /// Hides the controller window; the app keeps running in the tray.
    fn hide_window(&mut self) {
        if let Some(hwnd) = self.native {
            let _ = ss550_tray::hide_window(hwnd);
        }
    }

    /// Opens the diagnostic log once the version line has been clicked enough.
    ///
    /// The app has no console in release, so the log is the only place a panic
    /// or a strange exit leaves a trace. Reaching it is deliberately quiet.
    fn reveal_log(&mut self) {
        let (clicks, due) = advance_clicks(self.version_clicks);
        self.version_clicks = clicks;
        if !due {
            return;
        }
        self.note = match diagnostics::reveal() {
            Ok(()) => "Opened the log file.".into(),
            Err(error) => error.into(),
        };
    }

    /// Turns the Windows sign-in entry on or off.
    ///
    /// Turning it on copies the running executable into the per-user install
    /// folder first, so the entry never names a `target` folder that the next
    /// build will delete.
    fn set_autostart(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let outcome: Result<Option<PathBuf>, String> = if enabled {
            std::env::current_exe()
                .map_err(|error| error.to_string())
                .and_then(|running| {
                    let installed =
                        install::ensure_installed(&running).map_err(|error| error.to_string())?;
                    autostart::enable(&autostart::command_line(&installed, true))
                        .map_err(|error| error.to_string())?;
                    Ok(Some(installed))
                })
        } else {
            autostart::disable()
                .map(|()| None)
                .map_err(|error| error.to_string())
        };

        match outcome {
            Ok(installed) => {
                self.starts_with_windows = enabled;
                self.autostart_note = None;
                // Turning the switch on installed this very binary, and turning
                // it off removed the entry, so the comparison note is spent.
                self.sign_in_copy = None;
                self.note = match installed {
                    Some(path) => format!(
                        "This controller now starts with Windows from {}.",
                        path.display()
                    )
                    .into(),
                    None => "This controller will no longer start with Windows.".into(),
                };
            }
            Err(error) => {
                // The switch must never claim a state Windows does not have.
                self.starts_with_windows =
                    matches!(autostart::sign_in(), Ok(autostart::SignIn::On(_)));
                self.autostart_note = None;
                self.note = format!("Windows sign-in entry: {error}").into();
            }
        }
        cx.notify();
    }

    /// Points the bindings panel at a key and loads what that key does.
    fn select_key(&mut self, number: KeyNumber, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_key = number;
        self.load_draft(number, window, cx);

        self.note = match self.store.get(number) {
            Some(action) => format!("Key #{} is bound to {}.", number.get(), action.describe()),
            None => format!(
                "Key #{} has no action yet. Fill a field below and press SAVE BINDING.",
                number.get()
            ),
        }
        .into();
    }

    /// Loads one key's binding into the editor fields.
    ///
    /// Split from [`Controller::select_key`] so a binding that was just changed
    /// can refresh the editor without the note being overwritten: the sentence
    /// that says what was saved is the one worth reading.
    fn load_draft(&mut self, number: KeyNumber, window: &mut Window, cx: &mut Context<Self>) {
        let binding = self.store.get(number).cloned();

        // Every field is written on every load, empty included: a field left
        // holding the previous key's value is how the wrong program gets saved
        // onto the wrong key.
        let (program, args, url, keys, kind, chosen) = match binding {
            Some(Action::Launch { program, args }) => (
                program.display().to_string(),
                args.join("\n"),
                String::new(),
                String::new(),
                DraftKind::Launch,
                None,
            ),
            Some(Action::OpenUrl { url }) => (
                String::new(),
                String::new(),
                url,
                String::new(),
                DraftKind::OpenUrl,
                None,
            ),
            Some(Action::Keys { keys }) => (
                String::new(),
                String::new(),
                String::new(),
                keys.to_string(),
                DraftKind::Keys,
                None,
            ),
            // A reading is edited by the list under the form, which is the only
            // place that knows which cells this build has.
            Some(Action::Plugin { plugin, cell }) => (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                DraftKind::Plugin,
                Some((plugin, cell)),
            ),
            // A navigation key has no field in this editor yet: it is selected
            // like any other, and the editor shows nothing to fill. Its own
            // controls come with the pages card, which is where a folder is
            // chosen.
            Some(Action::Folder { .. }) | Some(Action::Back) | Some(Action::Profile { .. }) => (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                self.draft,
                None,
            ),
            None => (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                self.draft,
                None,
            ),
        };

        self.draft = kind;
        self.draft_plugin = chosen;
        // A key loaded into the editor is looked at whole: the list of readings
        // is every cell this build has, with the bound one picked inside it. A
        // filter left over from a drag would hide the siblings of the very cell
        // the key already shows.
        self.draft_plugin_filter = None;
        // The key's face is decided by the program in the field, so the icon
        // cache is told which one it is before the editor draws. A field that is
        // only text has no picture, and the editor draws the initial instead.
        self.draft_program = match program.trim().is_empty() {
            true => None,
            false => Some(PathBuf::from(program.trim())),
        };
        // The title is the profile's, not the binding's: a key can say something
        // without running anything, and the field is written on every load so a
        // title never follows the selection to the next key.
        let title = self
            .current_keys()
            .and_then(|keys| keys.title(u16::from(number.get())))
            .unwrap_or_default()
            .to_string();
        self.program_input
            .update(cx, |state, cx| state.set_value(program, window, cx));
        self.args_input
            .update(cx, |state, cx| state.set_value(args, window, cx));
        self.url_input
            .update(cx, |state, cx| state.set_value(url, window, cx));
        self.keys_input
            .update(cx, |state, cx| state.set_value(keys, window, cx));
        self.title_input
            .update(cx, |state, cx| state.set_value(title, window, cx));

        // The picture is read here, on the change that made it the draft, and
        // never while the editor is being drawn.
        self.rebuild_icons();
    }

    /// Points the editor at one kind of action and puts the caret in its first
    /// field.
    ///
    /// Nothing is written: this is what a palette template asks for, and the
    /// key keeps whatever it had until SAVE BINDING is pressed.
    fn open_editor(&mut self, kind: DraftKind, window: &mut Window, cx: &mut Context<Self>) {
        self.draft = kind;
        match kind {
            DraftKind::Launch => self
                .program_input
                .update(cx, |state, cx| state.focus(window, cx)),
            DraftKind::OpenUrl => self
                .url_input
                .update(cx, |state, cx| state.focus(window, cx)),
            DraftKind::Keys => self
                .keys_input
                .update(cx, |state, cx| state.focus(window, cx)),
            // A reading is chosen by clicking a row, not by typing: the caret
            // has nowhere to go, and the list is already on screen.
            DraftKind::Plugin => {}
        }
    }

    /// Shows or hides the list of applications installed on this PC.
    ///
    /// Opening it is the click that asks for the scan: walking the Start Menu and
    /// reading one icon per shortcut is the only slow step in this editor, and it
    /// happens here or not at all. A second open lists what the first one found.
    fn toggle_app_picker(&mut self, cx: &mut Context<Self>) {
        self.app_picker_open = !self.app_picker_open;
        if self.app_picker_open && !self.app_list.is_scanned() {
            self.scan_apps(cx);
        }
    }

    /// Looks through the Start Menu for the applications a key can run.
    ///
    /// The walk and every icon are read on the background executor: hundreds of
    /// shortcuts, one COM object and one shell icon each, is far too much to do
    /// on the thread that draws. The answer comes back as state, and the picker
    /// lists it without touching the disk again.
    fn scan_apps(&mut self, cx: &mut Context<Self>) {
        if matches!(self.app_list, apps::Apps::Scanning) {
            return;
        }
        self.app_list = apps::Apps::Scanning;
        self.note = "Looking through the Start Menu for installed applications…".into();

        cx.spawn(async move |this, cx| {
            // The whole point of the background executor: this future runs off
            // the UI thread, and only its result crosses back.
            let found = cx.background_spawn(async { apps::scan() }).await;
            let count = found.len();
            let _ = this.update(cx, |this, cx| {
                this.app_list = apps::Apps::Ready(found);
                this.note = match count {
                    0 => "No application was found in the Start Menu — a path can still be typed."
                        .into(),
                    _ => format!(
                        "{count} application{} found. Pick one, or type a path.",
                        if count == 1 { "" } else { "s" }
                    )
                    .into(),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Puts one application from the list into the `PROGRAM` field.
    ///
    /// The *shortcut* goes in the field, not the target: the runner hands a
    /// `.lnk` to the shell, so the program starts with the working directory and
    /// arguments the user set up. The icon is read here — once, on the click —
    /// so the key shows the real picture before SAVE BINDING is pressed.
    fn choose_app(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(choice) = self.app_list.list().and_then(|list| list.get(index)) else {
            return;
        };
        let name = choice.name.clone();
        let program = choice.program.clone();
        let arguments = choice.arguments.join("\n");
        // The picture is the application's, not the launcher's: a packaged app is
        // started by `explorer.exe`, so asking about the program would put
        // Explorer's icon on a key that opens Settings.
        let icon_source = choice.target.clone();
        let text = program.display().to_string();

        self.program_input
            .update(cx, |state, cx| state.set_value(text, window, cx));
        self.args_input
            .update(cx, |state, cx| state.set_value(arguments, window, cx));
        self.draft_program = Some(icon_source);
        self.rebuild_icons();
        self.app_picker_open = false;
        // The next time the list opens it opens whole: the search was for the
        // application that was just picked.
        self.app_query
            .update(cx, |state, cx| state.set_value(String::new(), window, cx));
        self.note = format!(
            "{name} chosen. Press SAVE BINDING to put it on key #{}.",
            self.selected_key.get()
        )
        .into();
    }

    /// Acts on a palette entry dropped on a key.
    ///
    /// A template is an intent: it selects the key and aims the editor at that
    /// kind, in place — the drop happens on the `Deck` page, where the key is,
    /// so the window must not move to another page to show a form. The key is
    /// not touched until SAVE BINDING. A plugin row is the same intent one step
    /// further in: the editor opens on the reading list narrowed to that
    /// plugin, and which cell is bound is still the user's click. An action that
    /// is already bound has nothing left to ask for, so it goes straight to the
    /// save path — the same one the editor uses, file and deck included.
    fn drop_palette_entry(
        &mut self,
        number: KeyNumber,
        drag: &ui::palette::PaletteDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ui::palette::drop_plan(drag) {
            ui::palette::DropPlan::OpenEditor { kind, plugin } => {
                self.select_key(number, window, cx);
                // Read after `select_key`, which hands the editor a whole list
                // again: the drop is what narrows it.
                let name = plugin
                    .as_deref()
                    .and_then(|id| self.plugins.groups().iter().find(|group| group.id() == id))
                    .map(|group| group.name().to_string());
                self.draft_plugin_filter = plugin;
                self.open_editor(kind, window, cx);
                self.note = match name {
                    Some(name) => format!(
                        "Key #{} — pick which part of {name} it shows, then press SAVE BINDING.",
                        number.get()
                    ),
                    None => format!(
                        "Key #{} — set it to {} and press SAVE BINDING.",
                        number.get(),
                        kind.label()
                    ),
                }
                .into();
            }
            ui::palette::DropPlan::Assign(action) => {
                self.selected_key = number;
                self.bind_and_repaint(number, action, cx);
                // The editor follows the key that just changed, so the form and
                // the tile never disagree about what the key runs.
                self.load_draft(number, window, cx);
            }
        }
    }

    /// Binds the selected key to what the panel holds, and saves it.
    fn save_binding(&mut self, cx: &mut Context<Self>) {
        // A key that navigates is not edited by this form: its fields are empty
        // because they do not apply, and saving an empty program over it would
        // destroy the page it opens. It is rebound from the pages card, which is
        // the only place that knows the pages this profile has.
        if let Some(action) = self.store.get(self.selected_key) {
            if action.is_navigation() {
                self.note = format!(
                    "Key #{} is {}: change what it opens from the pages card, not here.",
                    self.selected_key.get(),
                    action.summary()
                )
                .into();
                return;
            }
        }

        let action = match self.draft {
            DraftKind::Launch => {
                let program = self.program_input.read(cx).value().trim().to_string();
                let args_text = self.args_input.read(cx).value();
                let args = args_text
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_string)
                    .collect();

                Action::Launch {
                    program: PathBuf::from(program),
                    args,
                }
            }
            DraftKind::OpenUrl => Action::OpenUrl {
                url: self.url_input.read(cx).value().trim().to_string(),
            },
            DraftKind::Keys => {
                let text = self.keys_input.read(cx).value().trim().to_string();
                match Action::keys(&text) {
                    Ok(action) => action,
                    Err(refusal) => {
                        // The engine's own sentence, verbatim: it names the part
                        // that is wrong, and the user is the one who can fix it.
                        // Nothing is bound, so the key keeps what it had.
                        self.note = refusal.to_string().into();
                        return;
                    }
                }
            }
            DraftKind::Plugin => match self.draft_plugin.clone() {
                // The list only offers cells this build has, but a form that was
                // loaded from a binding and then saved again must not write back
                // a cell that has since gone: the engine checks the spelling,
                // and only the catalogue knows what exists.
                Some((plugin, cell)) if !self.plugins.knows(&plugin, &cell) => {
                    self.note =
                        format!("\"{cell}\" is not a cell this build has, so nothing was bound.")
                            .into();
                    return;
                }
                Some((plugin, cell)) => Action::Plugin { plugin, cell },
                None => {
                    self.note =
                        "Choose a measurement from the list, then press SAVE BINDING.".into();
                    return;
                }
            },
        };

        self.bind_and_repaint(self.selected_key, action, cx);
    }

    /// Writes the selected key's title into the profile and repaints the key.
    ///
    /// The title is what the key *says*, so it is saved on its own button — or
    /// Enter — rather than with SAVE BINDING: a key can be named without being
    /// rebound, and a key that runs nothing can be named too. The engine is the
    /// only thing that trims, bounds and refuses, and its sentence is what the
    /// note shows; a refusal leaves the profile exactly as it was.
    fn save_title(&mut self, cx: &mut Context<Self>) {
        if self.session.is_none() {
            self.note = NO_DECK_TITLE.into();
            return;
        }

        let key = u16::from(self.selected_key.get());
        let text = self.title_input.read(cx).value().trim().to_string();
        let title = match text.is_empty() {
            // An emptied field is how a title is taken off a key: the engine
            // stores "no title" one way, and this is the way the user asks for
            // it.
            true => None,
            false => Some(text.as_str()),
        };
        let page = self.current_page().map(str::to_string);
        if let Err(sentence) = self.profile_store.set_title(key, title, page.as_deref()) {
            self.note = sentence.into();
            return;
        }

        // The title is part of the picture the deck receives, so the composed
        // variant is rebuilt before the key is painted — never during a frame.
        self.rebuild_icons();
        if self.repaint_key(key) {
            self.note = match title {
                Some(title) => format!("Key #{key} now says \"{title}\".").into(),
                None => format!("Key #{key} has no title now.").into(),
            };
        }
        // Last, so a profile that could not be written is the sentence that
        // survives: the title is gone at the next start, and that matters more
        // than the paint that already happened.
        self.save_profile();
    }

    /// Applies the default title style, and repaints the keys that follow it.
    ///
    /// A key with its own style is left alone: it does not follow the default,
    /// which is the whole point of giving it one. A key with no title is not
    /// touched either — its payload cannot change.
    fn set_title_style(&mut self, style: titlestyle::TitleStyle, cx: &mut Context<Self>) {
        if self.title_style == style {
            return;
        }
        self.title_style = style.clone();

        // The file is written first, so the sentence the status line carries is
        // about the save and not about the paint. A write that failed leaves the
        // style in effect for this run: the user asked for it, and losing the
        // choice at the next start is the lesser harm.
        let saved = titlestyle::save(&style);
        self.note = match &saved {
            Ok(()) => format!("Titles are {} now.", style.summary()).into(),
            Err(error) => {
                format!("Titles are {} now, but not saved: {error}", style.summary()).into()
            }
        };

        // Recompose every variant whose style came from the default, then
        // repaint those keys. A key that shows a reading is drawn in the same
        // family, and its face lives on the worker, so it is asked for again from
        // there rather than composed here.
        self.rebuild_icons();
        for key in self.inherited_title_keys() {
            self.repaint_key(key);
        }
        for key in self.reading_keys() {
            self.repaint_key(key);
        }
        cx.notify();
    }

    /// Gives one key its own title style, or puts it back on the default.
    ///
    /// The style lives in the profile, beside the words, so switching profile
    /// switches the look as well. `None` removes the override and leaves the
    /// words alone: a key that goes back to the default still says what it said.
    fn set_key_title_style(
        &mut self,
        key: u16,
        style: Option<&titlestyle::TitleStyle>,
        cx: &mut Context<Self>,
    ) {
        let Some(deck_key) = self
            .session
            .as_ref()
            .map(|session| session.key().to_string())
        else {
            self.note = "Connect a deck to give a key its own title style.".into();
            return;
        };
        let page = self.current_page().map(str::to_string);
        if let Err(error) = self.profile_store.set_title_style(
            key,
            style.cloned(),
            page.as_deref(),
            &deck_key,
            &ss550_engine::config_dir(),
        ) {
            self.note = error.into();
            return;
        }

        self.note = match style {
            Some(style) => format!("Key #{key}'s title is {}.", style.summary()).into(),
            None => format!("Key #{key}'s title follows the default now.").into(),
        };
        self.rebuild_icons();
        self.repaint_key(key);
        cx.notify();
    }

    /// The style one target edits: the app's default, or a key's own.
    fn title_style_of(&self, target: FontTarget) -> titlestyle::TitleStyle {
        match target {
            FontTarget::Defaults => self.title_style.clone(),
            FontTarget::Key(key) => self.style_for(key),
        }
    }

    /// Writes a style for `target` and repaints what it changed.
    fn set_title_style_at(
        &mut self,
        target: FontTarget,
        style: titlestyle::TitleStyle,
        cx: &mut Context<Self>,
    ) {
        match target {
            FontTarget::Defaults => self.set_title_style(style, cx),
            FontTarget::Key(key) => self.set_key_title_style(key, Some(&style), cx),
        }
    }

    /// A title size in pixels, from whichever stepper was clicked.
    ///
    /// The clamp is [`ss550_engine::TitleStyle::with_size`]'s, not a second copy
    /// of the rule, so a typed number and a clicked one land on the same range.
    fn set_title_size(&mut self, target: FontTarget, px: u16, cx: &mut Context<Self>) {
        let style = self.title_style_of(target).with_size(px);
        self.set_title_style_at(target, style, cx);
    }

    /// Chooses the family the open list was asking about.
    ///
    /// An empty family is the system's own sans, which is what a machine that
    /// has no font of the chosen name falls back to as well.
    fn choose_font(&mut self, family: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.font_picker.take() else {
            return;
        };
        let style = self.title_style_of(target).with_family(family);
        // The search was for the font that was just picked; the next open starts
        // whole, exactly like the application picker.
        self.font_query
            .update(cx, |state, cx| state.set_value(String::new(), window, cx));
        self.set_title_style_at(target, style, cx);
    }

    /// Opens or closes the family list, on the style that asked for it.
    ///
    /// One list serves both places, so a click on a key's FONT chip and a click
    /// on the default's cannot disagree about which style they are editing.
    /// The scan is the one slow step in drawing a title, so it runs on the
    /// background executor: the list fills itself when it answers.
    fn toggle_font_picker(&mut self, target: FontTarget, cx: &mut Context<Self>) {
        self.font_picker = match self.font_picker {
            Some(open) if open == target => None,
            _ => Some(target),
        };
        if self.font_picker.is_some() {
            self.request_fonts(cx);
        }
        cx.notify();
    }

    /// Writes one binding and puts the key in step with it.
    ///
    /// This is the only path a binding takes: the editor and a drop both end
    /// here, so a dropped action cannot skip the file or the profile. The order
    /// matters — a deck that refused the repaint keeps its own sentence, which
    /// names the picture that would not fit, instead of being overwritten by
    /// "saved".
    fn bind_and_repaint(&mut self, number: KeyNumber, action: Action, cx: &mut Context<Self>) {
        // Recording belongs to the moment an action is *added to a key*, not to
        // the moment it runs. Every successful save below — the editor's SAVE
        // BINDING and a finished action dropped from the palette — passes
        // through here exactly once, so a template drop records only after the
        // editor it opened is saved.
        let recent = action.clone();
        match self.save_action(number, action) {
            Ok(saved) => {
                // A page is not something that runs, so opening one is not a recent
                // action: the list would fill with places instead of programs.
                let recents_error = match recent.is_navigation() {
                    true => None,
                    false => self.record_recent(&recent),
                };
                // The key's face comes from what it now runs, so the icon of the
                // new program is read before the repaint and never during it.
                self.rebuild_icons();
                if self.repaint_key(u16::from(number.get())) {
                    self.note = saved.into();
                }
                // A temperature that has no source answers a dash for ever, and
                // the sentence that explains it belongs at the moment the key is
                // bound, not in a document nobody opens.
                if let Some((plugin, cell)) = recent.plugin_cell() {
                    let unavailable = self
                        .samples
                        .get_named(plugin, cell)
                        .is_some_and(|reading| reading.reason().is_some());
                    if unavailable {
                        self.note = format!(
                            "{} No temperature source answered yet: {}.",
                            self.note,
                            ss550_plugins::temperature_hint()
                        )
                        .into();
                    }
                }
                // A recents write that failed is a status line of its own: the
                // binding was made, but the rail must not pretend it remembered
                // something it did not.
                if let Some(error) = recents_error {
                    self.note = format!("{} {error}", self.note).into();
                }
                // A URL's site icon is fetched on the save that bound it: the
                // key has just been painted with its initial, and the site's
                // picture replaces it when the request answers.
                self.fetch_site_icons(cx);
            }
            Err(message) => self.note = message.into(),
        }
    }

    /// Records an action that was just added to a key, and saves the list.
    ///
    /// Returns the sentence for the status line when the file could not be
    /// written. The in-memory list keeps the action regardless: the binding it
    /// belongs to was already made, and losing the reminder must not undo it.
    fn record_recent(&mut self, action: &Action) -> Option<String> {
        let path = {
            let session = self.session.as_ref()?;
            ss550_engine::recents_path_for(session.key())
        };
        record_recent_at(&mut self.recents, action, &path)
    }

    /// Stores, profiles and saves one binding, returning the sentence that says
    /// so — or the one that says why not.
    fn save_action(&mut self, number: KeyNumber, action: Action) -> Result<String, String> {
        let Some(path) = self.store_path.clone() else {
            return Err(
                "No deck is connected, so a binding has no deck to belong to yet.".to_string(),
            );
        };

        self.store
            .set(number, action)
            .map_err(|error| format!("Key #{} was not bound: {error}", number.get()))?;

        // Save the profile with updated bindings
        if let Some(session) = self.session.as_ref() {
            let deck_key = session.key().to_string();
            let config_dir = ss550_engine::config_dir();
            let page = self.current_page().map(str::to_string);
            let _ = self.profile_store.update_bindings(
                &self.store,
                page.as_deref(),
                &deck_key,
                &config_dir,
            );
        }

        match self.store.save(&path) {
            Ok(()) => {
                let summary = self
                    .store
                    .get(number)
                    .map(Action::summary)
                    .unwrap_or_default();
                Ok(format!("Key #{} saved — {summary}.", number.get()))
            }
            Err(error) => Err(format!(
                "Key #{} was bound, but the file was not saved: {error}",
                number.get()
            )),
        }
    }

    /// Removes the selected key's binding and saves the file.
    fn clear_binding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.remove_binding(self.selected_key, window, cx);
    }

    /// Removes one key's binding and saves the file.
    ///
    /// Takes the key as an argument so the bound list can unbind a row without
    /// moving the selection away from the key the editor is showing.
    ///
    /// What the key *said* goes with what it did: a title names the action, and a
    /// title left behind would sit on an empty key promising something that no
    /// longer happens. The style is cleared in the same call — the engine keeps
    /// "no title" as one representation.
    fn remove_binding(&mut self, number: KeyNumber, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.store_path.clone() else {
            self.note = "No deck is connected, so there is no binding file to clear.".into();
            return;
        };
        let key = u16::from(number.get());
        let had_action = self.store.remove(number).is_some();
        if had_action {
            if let Err(error) = self.store.save(&path) {
                self.note = format!(
                    "Key #{} was cleared, but the file was not saved: {error}",
                    number.get()
                )
                .into();
                return;
            }
            // The profile holds the same bindings, and *it* is what the deck is
            // handed on the next start — a key removed from one file and left in
            // the other comes back when the app restarts. Binding already writes
            // both; clearing has to as well.
            if let Some(session) = self.session.as_ref() {
                let deck_key = session.key().to_string();
                let page = self.current_page().map(str::to_string);
                let _ = self.profile_store.update_bindings(
                    &self.store,
                    page.as_deref(),
                    &deck_key,
                    &ss550_engine::config_dir(),
                );
            }
        }

        // The words go too, even when the action was already gone: CLEAR KEY is
        // about the key, and a title left behind would sit on an empty key
        // promising something that no longer happens. The style is cleared in the
        // same call — the engine keeps "no title" as one representation.
        let had_title = self
            .current_keys()
            .and_then(|keys| keys.title(key))
            .is_some();
        let page = self.current_page().map(str::to_string);
        let title = self.profile_store.set_title(key, None, page.as_deref());
        let saved = match title {
            Ok(()) => self.save_profile_result(),
            Err(error) => Err(format!("its title could not be cleared: {error}")),
        };

        // A key that runs nothing shows the flat colour again, and the icon it no
        // longer needs leaves the cache with it.
        self.rebuild_icons();
        self.repaint_key(key);
        // The form is the key too: with no title left in the profile, the field
        // would otherwise keep the words that were just removed and save them
        // back on the next SAVE TITLE.
        if number == self.selected_key {
            self.load_draft(number, window, cx);
        }

        self.note = match (saved, had_action, had_title) {
            (Err(error), _, _) => format!("Key #{} was cleared, but {error}", number.get()).into(),
            (Ok(()), true, true) => format!(
                "Key #{} cleared — it does nothing now, and its title is gone.",
                number.get()
            )
            .into(),
            (Ok(()), true, false) => {
                format!("Key #{} cleared — it does nothing now.", number.get()).into()
            }
            (Ok(()), false, true) => format!(
                "Key #{} says nothing now — its title is gone.",
                number.get()
            )
            .into(),
            (Ok(()), false, false) => format!("Key #{} had nothing to clear.", number.get()).into(),
        };
    }

    /// Asks the user for a picture and puts it on the selected key.
    ///
    /// The three steps are one click, in this order, and each one decides whether
    /// the next happens:
    ///
    /// 1. the system's own dialog, filtered to the formats the library accepts —
    ///    a cancel ends here, with nothing said, because the user just said it;
    /// 2. the file is **imported** into the library, which copies the bytes and
    ///    gives the picture the name the profile will store. An import that fails
    ///    — not an image this build reads, a name the library already holds, a
    ///    file over the size limit — ends here too, and the key keeps what it
    ///    had: a profile must never name a picture the library does not hold;
    /// 3. the key is told that name, which is [`Controller::assign_asset`]: the
    ///    profile is saved, the one picture the key now shows is fitted to the
    ///    deck's tile, and the key is repainted on the deck.
    ///
    /// Only step 1 happens here, and only the request for it: the dialog is
    /// opened on a worker thread, because it pumps a message loop of its own and
    /// doing that inside a click re-enters GPUI while the app is already borrowed
    /// for the click. Steps 2 and 3 run in [`Controller::collect_picture_pick`]
    /// once the answer arrives, on the poll.
    fn choose_picture(&mut self) {
        // Checked before the dialog opens, not after: a file copied into the
        // library for a key that cannot wear it is a picture the user now has
        // and did not ask for.
        if self.session.is_none() {
            self.note = NO_DECK_PICTURE.into();
            return;
        }

        // One dialog at a time. A second click while one is open is ignored
        // rather than queued: two modal windows over the same key is a question
        // asked twice, and the second answer would overwrite the first.
        let Some(answer) = self.picture.claim() else {
            return;
        };

        // The owner handle is the only thing that crosses the thread boundary:
        // no app, no window, no entity. The dialog is still modal to this
        // window — Windows disables it while the dialog is up — even though the
        // dialog runs on another thread.
        let owner = self.native;
        let spawned = std::thread::Builder::new()
            .name("picture-dialog".to_string())
            .spawn(move || {
                // Blocks here, on a thread with no borrow of the app, until the
                // user answers. Sending the answer is the last thing this
                // thread does.
                let _ = answer.send(ss550_tray::dialog::open_image_owned_by(owner));
            });

        if let Err(error) = spawned {
            // The slot goes back, or the button would never answer again.
            self.picture.release();
            self.note = format!("The picture dialog could not start: {error}").into();
        }
    }

    /// Applies the picture dialog's answer, once the dialog has closed.
    ///
    /// Called on the poll, beside the connection results and the site icons:
    /// the dialog answered on another thread, so its answer is state, and state
    /// is applied where a mutable borrow is the normal state of affairs. This is
    /// the step that imports the file and assigns it, and it only runs when
    /// there is an answer to apply.
    fn collect_picture_pick(&mut self) -> bool {
        let Some(answer) = self.picture.take_answer() else {
            return false;
        };

        // The dialog disabled this window while it was up; Windows enables it
        // again when the dialog ends, but the dialog ran on another thread, so
        // the window does it here as well. A disabled window ignores the mouse
        // and says nothing about it.
        ss550_tray::dialog::release_owner(self.native);

        match apply_pick(self.session.is_some(), &self.assets, answer) {
            // The user said no, or the dialog never opened. Nothing is said
            // about it and nothing is touched: they already know.
            PickOutcome::Cancelled => false,
            // The deck left while the dialog was up, so there is nothing to fit
            // the picture to. The file is not imported at all.
            PickOutcome::NoDeck => {
                self.note = NO_DECK_PICTURE.into();
                true
            }
            PickOutcome::Imported(name) => {
                self.assign_asset(name);
                true
            }
            // The picture was already in the library, which is the normal case
            // once the folder holds thousands of files: show it and say that
            // nothing was copied, so the message is not read as a refusal.
            PickOutcome::AlreadyHeld(name) => {
                self.assign_asset(name);
                self.note = "That picture was already in the library; the key now shows it.".into();
                true
            }
            // The engine's own sentence, unchanged: it names the refusal and the
            // file, and restating it here would be a second copy of the rules.
            PickOutcome::Refused(sentence) => {
                self.note = sentence.into();
                true
            }
        }
    }

    /// Makes the selected key show a picture from the library, and saves it.
    ///
    /// The picture is the profile's; the key and the deck are then repainted
    /// from the same plan every other write uses, so what the window shows and
    /// what the deck received are one picture.
    fn assign_asset(&mut self, name: ss550_engine::AssetName) {
        // A picture is fitted to the deck's tile and written to the profile of
        // that deck. With none in front of the window there is nothing to fit it
        // to, so the click says why instead of doing nothing.
        if self.session.is_none() {
            self.note = NO_DECK_PICTURE.into();
            return;
        }

        let key = u16::from(self.selected_key.get());
        if let Err(error) = self.profile_store.set_appearance(
            key,
            Some(ss550_engine::Appearance::image(name.clone())),
            self.current_page().map(str::to_string).as_deref(),
        ) {
            self.note = format!("Key #{key} was not changed: {error}").into();
            return;
        }

        self.save_profile();
        // The key now shows a picture, so the tile needs it fitted before the
        // next frame rather than during it. Only the pictures the profile names
        // are rebuilt: the one that was just chosen is decoded here, on the poll
        // that read the dialog's answer, and nothing else is.
        self.rebuild_previews();
        if self.repaint_key(key) {
            self.note = format!("Key #{key} now shows {name}.").into();
        }
    }

    /// Takes the artwork off the selected key, so it shows the profile colour.
    fn remove_artwork(&mut self) {
        if self.session.is_none() {
            self.note = NO_DECK_PICTURE.into();
            return;
        }

        let key = u16::from(self.selected_key.get());
        let page = self.current_page().map(str::to_string);
        if let Err(error) = self
            .profile_store
            .set_appearance(key, None, page.as_deref())
        {
            self.note = format!("Key #{key} was not changed: {error}").into();
            return;
        }

        self.save_profile();
        self.rebuild_previews();
        if self.repaint_key(key) {
            self.note = format!("Key #{key} shows the profile colour again.").into();
        }
    }

    /// Writes the profile file, so artwork survives a restart.
    ///
    /// The file is what makes an assignment real; the deck showing the picture
    /// until the app closes is not the same thing.
    fn save_profile(&mut self) {
        if let Err(error) = self.save_profile_result() {
            self.note = error.into();
        }
    }

    /// Writes the profile file and hands the failure back instead of reporting it.
    ///
    /// The caller that has something more specific to say about the same click — a
    /// key cleared, a title removed with it — composes its own sentence and needs
    /// to know whether the file was written.
    fn save_profile_result(&mut self) -> Result<(), String> {
        let deck_key = self
            .session
            .as_ref()
            .map(|session| session.key().to_string())
            .ok_or_else(|| "Connect a deck to save a profile.".to_string())?;
        self.profile_store
            .save(&deck_key, &ss550_engine::config_dir())
            .map_err(|error| format!("The profile changed but was not saved: {error}"))
    }

    /// Paints a key black, then the image, and answers the failure either way.
    ///
    /// The deck's panel is a little larger than the area it fills with an image:
    /// the strip left over keeps whatever the key showed before. On a photo that
    /// strip reads as a bright frame around the picture, which is what the user
    /// saw, and the colour it held was the key's previous fill. Blacking the key
    /// first makes the strip the picture's own dark border instead — one extra
    /// solid tile per image key, which is a few kilobytes on a link that already
    /// carries the picture.
    fn paint_over_black(
        session: &mut session::DeckSession,
        index: usize,
        jpeg: &[u8],
    ) -> Result<(), PaintFailure> {
        session
            .paint(index, [0, 0, 0])
            .map_err(PaintFailure::Session)?;
        session
            .paint_encoded(index, jpeg)
            .map_err(PaintFailure::Session)
    }

    /// Asks the face worker for one key's face, from the newest sample.
    ///
    /// Answers whether a job was queued. Nothing is composed here: the worker
    /// shapes the words and encodes the JPEG, and `collect_faces` paints what
    /// comes back. A key whose cell this build no longer has, or a window with
    /// no deck to state a tile size, is a sentence instead of a job.
    fn queue_face(&mut self, key: u16, action: &Action) -> bool {
        let Some((plugin, cell)) = action.plugin_cell() else {
            return false;
        };
        let Some(image) = self.session.as_ref().map(session::DeckSession::image_spec) else {
            self.note = "Connect a deck to draw a reading: the tile size comes from it.".into();
            return false;
        };
        // A binding can outlive the build that wrote it: a cell that is no longer
        // here is a sentence, not a key that quietly stops moving.
        let Some(spec) = self.plugins.find(plugin, cell).cloned() else {
            self.note = format!(
                "\"{cell}\" is not a cell this build has. Bind the key to a plugin it lists."
            )
            .into();
            return false;
        };
        if !self.faces.is_alive() {
            self.report_faces_gone();
            return false;
        }
        if self.faces_in_flight.contains(&key) {
            return false;
        }
        let Some(plugin) = ss550_plugin_api::PluginId::parse(plugin).ok() else {
            return false;
        };
        let Some(cell) = ss550_plugin_api::CellId::parse(cell).ok() else {
            return false;
        };
        let reading = self
            .samples
            .get_named(plugin.as_str(), cell.as_str())
            .cloned()
            // The first sample of a session has not arrived yet: the key shows
            // the dash it will show only until the machine answers.
            .unwrap_or_else(|| Reading::unavailable("no answer"));

        let job = facework::FaceJob {
            key,
            plugin,
            cell,
            spec,
            reading,
            // The key's own family, not the app's: a reading is drawn like the
            // words on that key, so the FONT control under TITLE works on a gauge
            // and on a clock's number the same way it works on a title.
            family: self.style_for(key).family,
            image,
        };
        if self.faces.submit(job) {
            self.faces_in_flight.insert(key);
            true
        } else {
            // The queue is full: the key keeps its old face and stays due, and
            // the next sample queues it again.
            false
        }
    }

    /// Paints every face the worker has finished.
    ///
    /// Called from the maintenance tick. A face that arrives for a binding the
    /// key no longer wears is dropped: a page switch while the worker was drawing
    /// must not paint one cell's number on another cell's key.
    fn collect_faces(&mut self) -> bool {
        let finished: Vec<facework::FaceDone> =
            std::iter::from_fn(|| self.faces.try_recv()).collect();
        let mut changed = false;
        for done in finished {
            self.faces_in_flight.remove(&done.key);
            let index = key_index(done.key);
            let bound = self
                .action_for(done.key)
                .and_then(Action::plugin_cell)
                .map(|(plugin, cell)| (plugin.to_string(), cell.to_string()));
            let expected = Some((done.plugin.to_string(), done.cell.to_string()));
            if bound != expected {
                continue;
            }

            // The window's tile draws the same pixels the deck receives, so the
            // two cannot disagree about what the key is.
            if let Some(image) = self.session.as_ref().map(session::DeckSession::image_spec) {
                if let Ok(frame) = render::preview_frame_rgba(image, &done.picture) {
                    self.face_tiles.insert(
                        (done.key, done.plugin.to_string(), done.cell.to_string()),
                        Arc::new(gpui_kit::RenderImage::new([frame])),
                    );
                }
            }

            match done.jpeg {
                Ok(jpeg) => {
                    let Some(session) = self.session.as_mut() else {
                        continue;
                    };
                    if let Err(failure) = Self::paint_over_black(session, index, &jpeg) {
                        match failure {
                            PaintFailure::Session(error) => self.report(error),
                            PaintFailure::Picture(message) => self.note = message.into(),
                        }
                        continue;
                    }
                    if let Some(entry) = self.slots.get_mut(index) {
                        *entry = Slot::Image;
                    }
                    self.mark_drawn(index, done.reading);
                }
                Err(message) => self.note = message.into(),
            }
            changed = true;
        }
        changed
    }

    /// Says once that the face worker stopped.
    fn report_faces_gone(&mut self) {
        if self.faces_gone.is_some() {
            return;
        }
        self.faces_gone = Some(
            "The face worker stopped: the readings on the keys are the last ones it drew."
                .to_string(),
        );
        self.note = self.faces_gone.clone().unwrap_or_default().into();
    }

    /// Remembers what one key's plugin face drew, so the next sample is compared
    /// against what is actually on the deck.
    fn mark_drawn(&mut self, index: usize, reading: Reading) {
        if self.drawn_readings.len() <= index {
            self.drawn_readings.resize(index + 1, None);
        }
        self.drawn_readings[index] = Some(reading);
    }

    /// Takes the newest walk over the machine, and repaints the keys it moved.
    ///
    /// Called from the maintenance tick, never from a frame: the sample was
    /// measured on another thread, and everything here happens on the window's
    /// own.
    fn pump_samples(&mut self) -> bool {
        // A sampler that is gone leaves every gauge showing its last number. The
        // window cannot refresh them, so it says so once instead of letting a
        // stale percentage pass for a live one.
        if let Some(sampler) = &self.sampler {
            if !sampler.is_alive() && self.sampler_gone.is_none() {
                self.sampler_gone = Some(
                    "The plugin sampler stopped: the readings on the keys are the last ones it took."
                        .to_string(),
                );
                self.note = self.sampler_gone.clone().unwrap_or_default().into();
            }
        }
        let Some(samples) = self.sampler.as_ref().and_then(Sampler::try_recv) else {
            return false;
        };
        // Each cadence hands over its own cells, so a sample is folded in rather
        // than replaced: the clock's second must not wipe the machine's numbers,
        // and the machine's walk must not blank the seconds. What was measured is
        // recorded before the merge, one history point per cell measured.
        self.record_history(&samples);
        self.samples.merge(samples);
        self.repaint_changed_readings()
    }

    /// Applies new lines for the Stats page and saves them.
    ///
    /// The file is written first, so the sentence the status line carries is about
    /// the save and not about a redraw: a write that failed leaves the value in
    /// force for this run, and the user is told it will not survive a restart.
    fn set_thresholds(&mut self, thresholds: thresholds::Thresholds, cx: &mut Context<Self>) {
        if self.thresholds == thresholds {
            return;
        }
        self.thresholds = thresholds;
        let saved = thresholds::save(&self.thresholds);
        self.note = match &saved {
            Ok(()) => "Thresholds saved.".into(),
            Err(error) => format!("Thresholds changed but not saved: {error}").into(),
        };
        // The card keeps the bad news as well: the status line is overwritten the
        // next time the deck reports in, and this one has to survive that.
        self.thresholds_note = saved.err();
        cx.notify();
    }

    /// Keeps the tray icon's tooltip saying what the machine is doing.
    ///
    /// The icon is what a user sees with the window closed, so the tooltip is the
    /// one place the numbers are readable without opening anything. It is written
    /// only when the sentence changes: a rename is a message to another thread,
    /// and waking it every half second to say the same thing is a stutter with no
    /// information in it.
    fn refresh_tray_tooltip(&mut self) {
        let Some(tray) = &self.tray else {
            return;
        };
        let text = tray_text(self.link, &self.samples, self.plugins.cells());
        if text == self.tray_tooltip {
            return;
        }
        let failed = tray.set_tooltip(&text).is_err();
        // The sentence is remembered whether or not the icon took it: a tray that
        // has stopped answering is not something to retry twice a second.
        self.tray_tooltip = text;
        if failed {
            self.tray = None;
            self.tray_error = Some(
                "The tray icon stopped answering. The controller keeps running, but it has no \
                 icon to come back from."
                    .to_string(),
            );
        }
    }

    /// Remembers what each measured cell reported, for the Stats page.
    ///
    /// Called once per received sample, before the keys are repainted, so the
    /// graph and the key are always describing the same walk over the machine.
    /// Only the cells the message carries are recorded: cadences differ, and a
    /// sensor would otherwise get a second point for the same number every time
    /// the clock's faster message carried it.
    fn record_history(&mut self, samples: &ss550_plugins::Samples) {
        for (plugin, cell, reading) in samples.iter() {
            self.history.record(plugin, cell, reading);
        }
    }

    /// Repaints only the keys whose face the newest sample changed.
    ///
    /// This is S6 in one function: the reading is compared with what that key
    /// last drew, and a key whose number, caption and bar are the same costs
    /// nothing. A key off the visible page is not repainted at all.
    fn repaint_changed_readings(&mut self) -> bool {
        let key_count = self
            .session
            .as_ref()
            .map_or(0, session::DeckSession::key_count);
        let actions: Vec<Option<Action>> = (0..key_count)
            .map(|index| {
                let key = key_number(index);
                self.action_for(key).cloned()
            })
            .collect();
        let due = readings_to_repaint(&actions, &self.drawn_readings, &self.samples);
        let mut changed = false;
        for index in due {
            let key = key_number(index);
            changed |= self.repaint_key(key);
        }
        changed
    }

    /// The keys on this page that show a plugin reading.
    ///
    /// Used by the one action that changes how a face is drawn without the
    /// reading moving: a new title family is a new face for every gauge.
    fn reading_keys(&self) -> Vec<u16> {
        let count = self
            .session
            .as_ref()
            .map_or(0, session::DeckSession::key_count);
        (1..=count as u16)
            .filter(|key| self.action_for(*key).is_some_and(Action::is_reading))
            .collect()
    }

    /// Repaints one key from what the profile says it shows now.
    ///
    /// Returns `false` when the picture path failed; the note already says why.
    /// A colour or a removal cannot fail this way, so the caller's own message
    /// survives.
    fn repaint_key(&mut self, key: u16) -> bool {
        let appearance = self
            .current_keys()
            .and_then(|keys| keys.appearance_for(key))
            .cloned();
        // The title and the action are taken as owned values: the plan needs the
        // cache and the font mutably, and a borrow of the store cannot outlive
        // that.
        let title = self
            .current_keys()
            .and_then(|keys| keys.title(key))
            .map(str::to_string);
        let action = self.action_for(key).cloned();
        // A key bound to a plugin shows the reading and nothing else: its
        // appearance and its title are not drawn over it. The face is composed on
        // the worker, so this asks for it and returns — `collect_faces` paints it.
        if let Some(action) = action.as_ref() {
            if action.is_reading() {
                return self.queue_face(key, action);
            }
        }
        // The key's own style, or the app's default: one resolution, used by the
        // plan and by the picture the deck receives.
        let style = self.style_for(key);
        // The plan is resolved from the profile, the binding and the icon cache
        // before the session is borrowed: nothing here reads the wire, and
        // nothing here can fail after the first byte.
        let plan = plan_tile(
            appearance.as_ref(),
            action.as_ref(),
            title.as_deref(),
            &style,
            self.paint.rgb(),
            &mut self.icons,
            &mut self.text,
        );
        let index = key_index(key);
        let spec = self.session.as_ref().map(session::DeckSession::image_spec);

        let Some(session) = self.session.as_mut() else {
            return true;
        };

        let result = match &plan {
            Tile::Asset { name, title, style } => {
                let tile = asset_tile(
                    &self.assets,
                    self.tiles.as_mut(),
                    spec,
                    name,
                    title.as_deref(),
                    style,
                    &mut self.text,
                );
                match tile {
                    Ok(jpeg) => Self::paint_over_black(session, index, &jpeg),
                    Err(message) => Err(PaintFailure::Picture(message)),
                }
            }
            Tile::Art(picture) => Self::paint_over_black(session, index, picture),
            Tile::TitledColour(picture) => Self::paint_over_black(session, index, picture),
            Tile::Colour(rgb) => session.paint(index, *rgb).map_err(PaintFailure::Session),
        };

        match result {
            Ok(()) => {
                let slot = match &plan {
                    Tile::Asset { .. } | Tile::Art(_) | Tile::TitledColour(_) => Slot::Image,
                    Tile::Colour(rgb) => slot_for(*rgb),
                };
                if let Some(entry) = self.slots.get_mut(index) {
                    *entry = slot;
                }
                true
            }
            Err(PaintFailure::Session(error)) => {
                self.report(error);
                false
            }
            Err(PaintFailure::Picture(message)) => {
                self.note = message.into();
                false
            }
        }
    }

    /// Opens the picture folder in the file manager.
    ///
    /// A picture reaches a key through the system's own dialog, which copies it
    /// in — so the folder is where they end up, not where they come from. This
    /// button exists so "the folder" is a place the user can reach without
    /// knowing where the app keeps its configuration: to look at what has been
    /// imported, or to delete one.
    fn open_assets_folder(&mut self) {
        if let Err(error) = self.assets.ensure() {
            self.note = format!("Could not create the picture folder: {error}").into();
            return;
        }

        let opener = if cfg!(windows) {
            "explorer"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let action = Action::Launch {
            program: PathBuf::from(opener),
            args: vec![self.assets.root().display().to_string()],
        };

        match ss550_engine::run_action(&action) {
            Ok(()) => {
                self.note = format!("Opened {}", self.assets.root().display()).into();
            }
            Err(error) => {
                self.note =
                    format!("Could not open {}: {error}", self.assets.root().display()).into();
            }
        }
    }

    /// Records what a frame cost, while that is worth a line.
    ///
    /// The first frames are the startup number; after them only a frame long
    /// enough to be seen as a hitch is recorded. Logging every frame forever
    /// would bury a crash report under a frame dump nobody reads.
    fn log_frame(&mut self, cost: Duration) {
        self.last_frame = cost;
        self.frames = self.frames.saturating_add(1);
        if self.frames == 1 {
            diagnostics::record(&format!(
                "first frame after {} ms",
                diagnostics::since_start().as_millis()
            ));
        }
        if self.frames <= FRAMES_LOGGED || cost >= SLOW_FRAME {
            diagnostics::record(&format!("render {} in {} ms", self.frames, ms_text(cost)));
        }
    }
}

/// The key number a zero-based position addresses.
///
/// One spelling for the conversion, because the window mixes the two spaces:
/// the deck and the bindings speak key numbers from one, and every loop over the
/// grid speaks positions from zero. A number past the deck's own range is
/// clamped rather than wrapped, so a wrong index can never address a different
/// key.
pub(crate) fn key_number(index: usize) -> u16 {
    u16::try_from(index.saturating_add(1)).unwrap_or(u16::MAX)
}

/// The zero-based position a key number addresses.
pub(crate) fn key_index(key: u16) -> usize {
    usize::from(key.saturating_sub(1))
}

fn slot_name(slot: Slot) -> &'static str {
    match slot {
        Slot::White => "white",
        Slot::Red => "red",
        Slot::Image => "an image",
        Slot::Off => "off",
    }
}

/// The tile swatch that best matches a colour a profile asked for.
///
/// A tile can draw two colours and "off", so a solid shows as red when it is
/// red-dominant and white otherwise. The deck itself still receives the exact
/// colour; this only decides what the window's swatch claims about it.
fn slot_for(rgb: [u8; 3]) -> Slot {
    if rgb[0] > rgb[1].max(rgb[2]) {
        Slot::Red
    } else {
        Slot::White
    }
}

/// Whether a binding's key exists on the deck in front of the window.
///
/// The engine keeps no upper bound of its own: a key number is valid as long as
/// it is at least 1, and whether the deck has it is the geometry's answer.
fn key_exists(session: &session::DeckSession, number: KeyNumber) -> bool {
    let index = u16::from(number.get().saturating_sub(1));
    session.geometry().contains(KeyId::new(index))
}

/// The 1-based binding number for a deck key, when the deck has that key.
fn binding_number(session: &session::DeckSession, index: usize) -> Option<KeyNumber> {
    let key = u16::try_from(index).ok()?;
    if !session.geometry().contains(KeyId::new(key)) {
        return None;
    }
    KeyNumber::new(u8::try_from(key + 1).ok()?).ok()
}

/// Reads one deck's recent actions, starting empty when the file cannot be read.
///
/// The engine refuses an unreadable, corrupt, newer or unvalidatable file whole,
/// so there is no partial list to reconcile here: the window takes the sentence
/// it is given, puts it on the status line, and shows an empty `RECENTS`.
fn load_recents_at(path: &std::path::Path) -> (Recents, Option<String>) {
    match Recents::load(path) {
        Ok(recents) => (recents, None),
        Err(error) => (Recents::new(), Some(error)),
    }
}

/// Records one action and writes the list, returning the sentence on failure.
///
/// Split from the controller so the round trip — record, save, read back — is a
/// test over a scratch file, with no window and no deck in the loop.
fn record_recent_at(
    recents: &mut Recents,
    action: &Action,
    path: &std::path::Path,
) -> Option<String> {
    recents.record(action);
    recents.save(path).err()
}

/// Joins two status sentences, keeping whichever ones exist.
///
/// The bindings and the recents are read together and either can fail on its
/// own. The window has one status line, so both failures survive it instead of
/// one silently replacing the other.
fn join_notes(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(first), Some(second)) => Some(format!("{first} {second}")),
        (Some(note), None) | (None, Some(note)) => Some(note),
        (None, None) => None,
    }
}

impl Render for Controller {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The height the side column is bounded by, read before the tree is
        // built: the layout needs a number, and the window is the only place one
        // exists.
        self.viewport_h = f32::from(window.viewport_size().height);
        // Measured from here to the finished tree: that is what the window
        // spends to put a frame on screen, and what startup is made of.
        let frame_started = Instant::now();
        let root = ui::shell::app_shell(self, cx);
        self.log_frame(frame_started.elapsed());
        root
    }
}

/// What comparing this build with the installed copy ended up saying.
///
/// The line is shown in the settings page either way; `problem` is what decides
/// whether it reads as a reassuring tick or as something the user has to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SignInCopy {
    /// The sentence the settings page shows.
    text: SharedString,
    /// True when the sign-in copy is *not* what this build needs it to be.
    problem: bool,
}

/// Keeps the copy Windows starts at sign-in in step with the binary in use.
///
/// The sign-in entry names a file in a folder of its own, so a newly built
/// controller runs only when the user starts it by hand: at the next logon,
/// Windows still opens the copy it installed the day the switch was turned on.
/// Comparing the two on startup and replacing the older one closes that gap,
/// which is the difference between "the update worked" and "the app still opens
/// the old build".
///
/// A development build never overwrites the installed copy: it is slower, it
/// has a console attached, and it disappears on the next `cargo clean`, so
/// pointing the sign-in entry at it would trade one stale copy for another.
///
/// Returns `None` when the two copies already agree and there is nothing worth
/// saying.
fn sync_sign_in_copy() -> Option<SignInCopy> {
    if cfg!(debug_assertions) {
        return Some(SignInCopy {
            text: "This is a development build, so the copy Windows starts at sign-in was \
                   left alone. Build with --release to update it."
                .into(),
            problem: false,
        });
    }
    let running = std::env::current_exe().ok()?;
    let (text, problem) = match install::refresh_installed(&running) {
        Ok(install::Refresh::RunningInstalled) => return None,
        Ok(install::Refresh::Updated(installed)) => (
            format!(
                "The copy Windows starts at sign-in now matches this build: {}.",
                installed.display()
            ),
            false,
        ),
        Ok(install::Refresh::Blocked { installed, reason }) => (
            format!(
                "Windows still starts an older copy from {}, because it could not be replaced: \
                 {reason}. Close that instance, then turn START WITH WINDOWS off and on again.",
                installed.display()
            ),
            true,
        ),
        Err(error) => (
            format!("The sign-in copy could not be checked: {error}"),
            true,
        ),
    };
    Some(SignInCopy {
        text: SharedString::from(text),
        problem,
    })
}

/// The native handle of a GPUI window.
///
/// `Window::window_handle` is an inherent method that shadows the trait
/// method of the same name, so the trait has to be called explicitly to get
/// the platform handle.
fn native_handle(window: &Window) -> Option<isize> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}

fn main() {
    // The clock starts before anything else our own code does, so the startup
    // line measures the whole process, not just the part after the log opened.
    diagnostics::mark_start();

    // The log and the panic hook come first: a failure while the window is
    // being built is exactly the kind that used to leave no trace.
    diagnostics::install_panic_hook();

    // Started by Windows: the controller lives in the tray with no window.
    let hidden = std::env::args().any(|argument| argument == autostart::HIDDEN_FLAG);

    // One controller per desktop. A second start — a double click on the Start
    // Menu entry while this one lives in the tray — asks the running window to
    // show itself and leaves, instead of opening a second controller that
    // would fight for the deck, the tray icon and the tiles.
    let mut singleton = match ss550_tray::instance::acquire() {
        Ok(ss550_tray::instance::Launch::AlreadyRunning) => {
            diagnostics::record("another controller is running: asked it to show");
            return;
        }
        Ok(ss550_tray::instance::Launch::First(guard)) => Some(guard),
        Err(error) => {
            // Without the claim the app still runs; it just cannot be the only
            // one. Losing that is worth a line, not a refusal to start.
            diagnostics::record(&format!("single instance unavailable: {error}"));
            None
        }
    };

    diagnostics::record(&format!(
        "started v{} (hidden={hidden})",
        env!("CARGO_PKG_VERSION")
    ));

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        // The app leaves only when the user says so, from the tray.
        .with_quit_mode(gpui_kit::QuitMode::Explicit)
        .run(move |cx| {
            gpui_kit::init(cx);

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1240.), px(880.)), cx)),
                // A floor for the shell: the rail, a card of tiles and the
                // column beside it need this much before anything has to be
                // cut, and a window that clips its own content is worse than
                // one that will not be dragged narrower.
                window_min_size: Some(size(px(ui::WINDOW_MIN_W), px(ui::WINDOW_MIN_H))),
                ..Default::default()
            };

            gpui_kit::open_window(options, cx, |window, cx| {
                if hidden {
                    // The window goes straight to the tray: nothing on the
                    // desktop, and the deck is already awake.
                    if let Some(hwnd) = native_handle(window) {
                        let _ = ss550_tray::hide_window(hwnd);
                    }
                } else {
                    window.activate_window();
                }
                window.set_window_title("Sh-Deck");
                Theme::change(ThemeMode::Dark, Some(window), cx);
                let singleton = singleton.take();
                cx.new(move |cx| {
                    let mut controller = Controller::new(window, cx);
                    controller.singleton = singleton;
                    controller
                })
            })
            .expect("failed to open the controller window");
        });
}

#[cfg(test)]
mod tests {
    use super::{
        advance_clicks, apply_pick, join_notes, load_recents_at, navigate, needs_fonts, plan_tile,
        read_deck_path_at, readings_to_repaint, record_recent_at, resolve_style, slot_for,
        tray_text, write_deck_path_at, Link, NavOutcome, PickOutcome, PictureDialog, Slot, Tile,
    };
    use ss550_engine::{Action, AssetLibrary, Recents};
    use ss550_plugin_api::{CellId, PluginId, Reading};
    use ss550_plugins::Samples;
    use std::path::PathBuf;

    fn cell(id: &str) -> CellId {
        CellId::parse(id).expect("a usable cell id")
    }

    #[test]
    fn only_a_key_whose_reading_changed_is_repainted() {
        let plugin = |id: &str| Action::Plugin {
            plugin: "system-stats".to_string(),
            cell: id.to_string(),
        };
        let actions = vec![
            Some(plugin("cpu.usage")),
            Some(plugin("gpu.usage")),
            Some(Action::Back),
            None,
            Some(plugin("cpu.temp")),
        ];
        let drawn = vec![
            Some(Reading::percent(41.0)),
            Some(Reading::percent(50.0)),
            None,
            None,
            None,
        ];
        let mut samples = Samples::new();
        let owner = PluginId::parse("system-stats").expect("a usable plugin id");
        samples.set(owner.clone(), cell("cpu.usage"), Reading::percent(42.0));
        samples.set(owner, cell("gpu.usage"), Reading::percent(50.0));

        // 0 moved, 1 draws the same face, 2 is not a reading, 3 is unbound, and
        // 4 was not measured — so 4 keeps what it shows instead of blanking.
        assert_eq!(readings_to_repaint(&actions, &drawn, &samples), vec![0]);
    }

    #[test]
    fn a_key_that_has_never_been_painted_is_due_even_when_the_reading_is_unchanged() {
        // The first sample of a session: nothing has been drawn yet, so a key
        // bound to a plugin has to be painted even though there is no "previous"
        // face to compare against.
        let action = Action::Plugin {
            plugin: "system-stats".to_string(),
            cell: "cpu.usage".to_string(),
        };
        let mut samples = Samples::new();
        samples.set(
            PluginId::parse("system-stats").expect("a usable plugin id"),
            cell("cpu.usage"),
            Reading::percent(42.0),
        );
        let due = readings_to_repaint(&[Some(action)], &[None], &samples);
        assert_eq!(due, vec![0]);
    }

    #[test]
    fn the_tray_says_what_the_machine_is_doing_and_what_the_link_is_doing() {
        let cells = ss550_plugins::Catalogue::built_in().cells().to_vec();
        let mut samples = Samples::new();
        let owner = PluginId::parse("system-stats").expect("a usable plugin id");
        samples.set(owner.clone(), cell("cpu.usage"), Reading::percent(20.0));
        samples.set(owner.clone(), cell("cpu.temp"), Reading::temperature(55.0));
        samples.set(owner, cell("gpu.temp"), Reading::unavailable("no sensor"));

        let connected = tray_text(Link::Connected, &samples, &cells);
        assert!(connected.starts_with("Sh-Deck · CPU 20%"), "{connected}");
        assert!(connected.contains("CPU °C 55°C"), "{connected}");
        assert!(
            connected.contains("GPU °C —"),
            "a dash is said, not hidden: {connected}"
        );
        // Short enough for a Windows taskbar tooltip, which cuts at 127.
        assert!(connected.chars().count() <= 120, "{connected}");

        // With no deck there are no numbers, and the state is the whole answer.
        assert_eq!(
            tray_text(Link::Waiting, &samples, &cells),
            "Sh-Deck · waiting for the deck"
        );
        assert_eq!(
            tray_text(Link::Connected, &Samples::new(), &cells),
            "Sh-Deck · connected"
        );
    }

    #[test]
    fn a_title_on_a_key_that_runs_nothing_still_asks_for_a_font() {
        // The regression this pins: the scan used to be demanded only by the
        // *bound* keys, so naming a key that runs nothing wrote words the deck
        // could never draw. The profile below has a title and **no binding at
        // all**, which is the case that used to be invisible.
        let spec = deck_api::ImageSpec::new(72, 72, 4_096, deck_api::ImageFormat::Jpeg);
        let mut profile = ss550_engine::Profile::new("default");
        profile
            .root
            .set_title(4, Some("Music"))
            .expect("set the title");

        assert!(
            needs_fonts(Some(spec), Some(&profile.root), 6),
            "an unbound key with words still needs a font"
        );
        assert!(
            !needs_fonts(
                Some(spec),
                Some(&ss550_engine::Profile::new("empty").root),
                6
            ),
            "a profile with no titles has nothing to set in a font"
        );
        assert!(
            !needs_fonts(None, Some(&profile.root), 6),
            "with no deck there is no payload to draw into"
        );
        assert!(
            !needs_fonts(Some(spec), None, 6),
            "with no profile there is nothing to read"
        );
        assert!(
            !needs_fonts(Some(spec), Some(&profile.root), 3),
            "a title on a key the deck does not have is not drawn anywhere"
        );
    }

    #[test]
    fn the_deck_path_survives_a_restart_and_forgets_when_there_is_none() {
        // What the next start reads to open the deck without walking the bus:
        // a hint on disk, which has to come back exactly, and to disappear when
        // the deck is not there any more.
        let dir = std::env::temp_dir().join(format!("ss550-path-{}", std::process::id()));
        let file = dir.join("deck-path.txt");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(read_deck_path_at(&file), None, "nothing remembered yet");

        let path =
            r"\\?\hid#vid_1b80&pid_a139#9&2f7c1f3e&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        write_deck_path_at(&file, Some(path));
        assert_eq!(read_deck_path_at(&file), Some(path.to_string()));

        // A trailing newline from an editor is not part of the path.
        std::fs::write(&file, format!("{path}\r\n")).expect("write");
        assert_eq!(read_deck_path_at(&file), Some(path.to_string()));

        write_deck_path_at(&file, Some("   "));
        assert_eq!(
            read_deck_path_at(&file),
            None,
            "a blank path is no path at all"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_page_stack_enters_leaves_and_stops_at_the_root() {
        // S3, without a deck: entering grows the stack, VOLVER takes the last
        // entry, and on the root there is nothing to take.
        let mut stack: Vec<String> = Vec::new();
        let known = |page: &str| page == "Media" || page == "Live";

        assert_eq!(
            navigate(&mut stack, &Action::Back, known),
            NavOutcome::AtRoot,
            "the root has nowhere to go back to"
        );
        assert!(stack.is_empty(), "a refused step back changes nothing");

        assert_eq!(
            navigate(
                &mut stack,
                &Action::Folder {
                    page: "Media".to_string()
                },
                known
            ),
            NavOutcome::Entered
        );
        assert_eq!(
            navigate(
                &mut stack,
                &Action::Folder {
                    page: "Live".to_string()
                },
                known
            ),
            NavOutcome::Entered
        );
        assert_eq!(stack, vec!["Media".to_string(), "Live".to_string()]);

        assert_eq!(
            navigate(
                &mut stack,
                &Action::Folder {
                    page: "Gone".to_string()
                },
                known
            ),
            NavOutcome::UnknownPage,
            "a folder this profile no longer has is refused"
        );
        assert_eq!(stack.len(), 2, "and it does not grow the stack");

        assert_eq!(
            navigate(&mut stack, &Action::Back, known),
            NavOutcome::WentBack
        );
        assert_eq!(
            stack,
            vec!["Media".to_string()],
            "back to where we came from"
        );
        assert_eq!(
            navigate(&mut stack, &Action::Back, known),
            NavOutcome::WentBack
        );
        assert!(stack.is_empty(), "and then to the root");

        assert_eq!(
            navigate(
                &mut stack,
                &Action::Profile {
                    name: "stream".to_string()
                },
                known
            ),
            NavOutcome::NotNavigation,
            "a profile switch is the window's, not the stack's"
        );
    }

    #[test]
    fn a_key_wears_its_own_title_style_and_the_others_follow_the_default() {
        // The fallback in one sentence: an override wins, an absent one does not
        // borrow the neighbour's.
        let default = crate::titlestyle::TitleStyle::default().with_size(14);
        let own = crate::titlestyle::TitleStyle {
            family: "Georgia".to_string(),
            size_px: 26,
            position: crate::titlestyle::TitlePosition::Top,
            uppercase: true,
        };
        let mut profile = ss550_engine::Profile::new("stream");
        profile
            .root
            .set_title(2, Some("Cam"))
            .expect("set the title");
        profile.root.set_title_style(2, Some(own.clone()));

        assert_eq!(resolve_style(Some(&profile.root), &default, 2), own);
        assert_eq!(
            resolve_style(Some(&profile.root), &default, 3),
            default,
            "a key with no style of its own follows the default"
        );
        assert_eq!(
            resolve_style(None, &default, 2),
            default,
            "with no page there is nothing but the default"
        );
    }

    #[test]
    fn a_title_change_repaints_the_keys_payload() {
        // The title is drawn into the picture the deck receives, so the bytes a
        // key sends change when its words change — and the plan for a key with
        // no title is exactly the one it was before titles existed.
        let spec = deck_api::ImageSpec::new(72, 72, 4_096, deck_api::ImageFormat::Jpeg);
        let action = Action::keys("ctrl+m").expect("a valid combination");
        let mut icons = crate::icons::IconCache::new();
        let mut text = Some(crate::titletext::TextEngine::new());
        let style = crate::titlestyle::TitleStyle::default();

        icons.prepare(
            Some(spec),
            &[crate::icons::KeyArt {
                action: &action,
                title: None,
                style: style.clone(),
            }],
            None,
            text.as_mut(),
        );
        let before = match plan_tile(
            None,
            Some(&action),
            None,
            &style,
            [0, 0, 0],
            &mut icons,
            &mut text,
        ) {
            Tile::Art(picture) => picture,
            _ => panic!("an initial is a picture"),
        };

        icons.prepare(
            Some(spec),
            &[crate::icons::KeyArt {
                action: &action,
                title: Some("mute"),
                style: style.clone(),
            }],
            None,
            text.as_mut(),
        );
        let after = match plan_tile(
            None,
            Some(&action),
            Some("mute"),
            &style,
            [0, 0, 0],
            &mut icons,
            &mut text,
        ) {
            Tile::Art(picture) => picture,
            _ => panic!("a titled initial is still a picture"),
        };

        assert_ne!(before, after, "the words are part of the payload");
        assert!(after.len() <= spec.max_bytes(), "{} bytes", after.len());

        // A key that runs nothing is a flat colour — and a titled colour has to
        // be an image, because the deck has no text, only a JPEG.
        let solid = ss550_engine::Appearance::solid([0xDC, 0x26, 0x26]);
        assert!(matches!(
            plan_tile(
                Some(&solid),
                None,
                None,
                &style,
                [0, 0, 0],
                &mut icons,
                &mut text
            ),
            Tile::Colour([0xDC, 0x26, 0x26])
        ));
        assert!(matches!(
            plan_tile(
                Some(&solid),
                None,
                Some("mute"),
                &style,
                [0, 0, 0],
                &mut icons,
                &mut text
            ),
            Tile::TitledColour(_)
        ));
        assert!(
            matches!(
                plan_tile(None, None, None, &style, [0, 0, 0], &mut icons, &mut text),
                Tile::Colour([0, 0, 0])
            ),
            "a key with nothing on it and no title keeps its colour"
        );
    }

    #[test]
    fn the_version_line_reveals_on_the_fifth_click() {
        let mut clicks = 0;
        for _ in 0..4 {
            let (next, due) = advance_clicks(clicks);
            clicks = next;
            assert!(!due, "the reveal must wait for the fifth click");
        }
        let (next, due) = advance_clicks(clicks);
        assert!(due, "the fifth click must reveal");
        assert_eq!(next, 0, "the counter starts over after a reveal");
    }

    #[test]
    fn one_picture_dialog_at_a_time() {
        let mut dialog = PictureDialog::default();
        let first = dialog.claim().expect("the first click opens a dialog");

        // A second click while the dialog is up is ignored, not queued: two
        // modal windows over one key is the same question asked twice.
        assert!(
            dialog.claim().is_none(),
            "a second click must not open a second dialog"
        );

        // The dialog answers; the slot frees and the next click may open one.
        let _ = first.send(None);
        assert_eq!(dialog.take_answer(), Some(None), "a cancel is an answer");
        assert!(
            dialog.claim().is_some(),
            "once the dialog closed, the button answers again"
        );
    }

    #[test]
    fn a_worker_that_dies_frees_the_slot() {
        // A dialog thread that never answers — it panicked, or the dialog could
        // not open at all — must not leave the button dead forever.
        let mut dialog = PictureDialog::default();
        let worker = dialog.claim().expect("a dialog slot");
        drop(worker);

        assert_eq!(
            dialog.take_answer(),
            Some(None),
            "a worker that went away is a cancel, not a dialog that is still open"
        );
        assert!(dialog.claim().is_some(), "and the slot is free again");
    }

    #[test]
    fn a_cancelled_dialog_imports_nothing_and_leaves_the_key_alone() {
        let dir = scratch("pick-cancel");
        let source = dir.join("live.png");
        std::fs::write(&source, b"pretend this is a png").expect("a file the dialog could answer");
        let library = AssetLibrary::new(dir.join("assets"));

        // Cancel: the dialog answered with nothing, so there is nothing to
        // import and nothing to assign. The key keeps the face it had.
        let outcome = apply_pick(true, &library, None);

        assert!(
            matches!(outcome, PickOutcome::Cancelled),
            "a cancel is not an import"
        );
        assert!(
            !library.root().join("live.png").exists(),
            "a cancel copies nothing into the library"
        );

        // The same file, named, does import — so the check above is about the
        // cancel, not about a file the library would have refused anyway.
        assert!(
            matches!(
                apply_pick(true, &library, Some(source)),
                PickOutcome::Imported(_)
            ),
            "the same file is importable when it is actually chosen"
        );

        cleanup(dir);
    }

    #[test]
    fn an_import_error_keeps_the_key_and_says_why() {
        let dir = scratch("pick-refused");
        let source = dir.join("notes.txt");
        std::fs::write(&source, b"not a picture").expect("a file to refuse");
        let library = AssetLibrary::new(dir.join("assets"));

        match apply_pick(true, &library, Some(source)) {
            PickOutcome::Refused(sentence) => {
                assert!(sentence.contains("notes.txt"), "{sentence}");
            }
            other => panic!("a text file must be refused, not {other:?}"),
        }
        assert!(
            !library.root().join("notes.txt").exists(),
            "a refusal copies nothing"
        );

        cleanup(dir);
    }

    #[test]
    fn a_click_with_no_deck_copies_nothing() {
        // The dialog can outlive the deck: the answer is applied after the
        // click, so the deck is checked again before the file is copied.
        let dir = scratch("pick-no-deck");
        let source = dir.join("live.png");
        std::fs::write(&source, b"pretend this is a png").expect("a file the dialog could answer");
        let library = AssetLibrary::new(dir.join("assets"));

        let outcome = apply_pick(false, &library, Some(source));

        assert!(
            matches!(outcome, PickOutcome::NoDeck),
            "a picture is fitted to the deck's tile, so no deck means no import"
        );
        assert!(
            !library.root().join("live.png").exists(),
            "a click with no deck copies nothing into the library"
        );

        cleanup(dir);
    }

    #[test]
    fn a_missing_recents_file_is_empty_and_says_nothing() {
        let dir = scratch("missing-recents");
        let (recents, note) = load_recents_at(&dir.join("recents.json"));

        assert!(recents.entries().is_empty());
        assert!(
            note.is_none(),
            "a deck that has never been used is not an error"
        );

        cleanup(dir);
    }

    #[test]
    fn a_recents_file_that_cannot_be_read_starts_empty_and_says_why() {
        let dir = scratch("corrupt-recents");
        let path = dir.join("recents.json");
        std::fs::write(&path, "{ not json").expect("write the test's own file");

        let (recents, note) = load_recents_at(&path);

        assert!(
            recents.entries().is_empty(),
            "the list must not be half-read"
        );
        let note = note.expect("the file's refusal must reach the status line");
        assert!(note.contains("is not valid"), "{note}");

        cleanup(dir);
    }

    #[test]
    fn an_action_the_build_cannot_use_starts_empty_and_says_why() {
        let dir = scratch("invalid-action-recents");
        let path = dir.join("recents.json");
        std::fs::write(
            &path,
            r#"{"version": 1, "actions": [{"type": "open_url", "url": "example.com"}]}"#,
        )
        .expect("write the test's own file");

        let (recents, note) = load_recents_at(&path);

        assert!(
            recents.entries().is_empty(),
            "an entry this build cannot validate must not reach the rail"
        );
        let note = note.expect("the refusal must reach the status line");
        assert!(note.contains("cannot use"), "{note}");

        cleanup(dir);
    }

    #[test]
    fn recording_an_action_saves_it_newest_first() {
        let dir = scratch("recording-recents");
        let path = dir.join("recents.json");
        let mut recents = Recents::new();
        let launch = Action::Launch {
            program: PathBuf::from("C:\\Apps\\brave.exe"),
            args: Vec::new(),
        };
        let keys = Action::keys("ctrl+shift+m").expect("a valid combination");

        assert_eq!(record_recent_at(&mut recents, &launch, &path), None);
        assert_eq!(record_recent_at(&mut recents, &keys, &path), None);

        let (read_back, note) = load_recents_at(&path);
        assert!(note.is_none(), "{note:?}");
        assert_eq!(read_back.entries(), &[keys, launch]);

        cleanup(dir);
    }

    #[test]
    fn a_recents_write_that_fails_returns_a_sentence() {
        let dir = scratch("unwritable-recents");
        // A file where the folder should be: `save` cannot create the parent.
        let blocker = dir.join("blocked");
        std::fs::write(&blocker, "not a folder").expect("write the test's own file");
        let path = blocker.join("recents.json");
        let mut recents = Recents::new();
        let action = Action::keys("ctrl+shift+m").expect("a valid combination");

        let error = record_recent_at(&mut recents, &action, &path)
            .expect("a write that cannot land must be reported");

        assert!(error.contains("could not"), "{error}");
        assert_eq!(
            recents.entries(),
            &[action],
            "the in-memory list keeps what the binding added"
        );

        cleanup(dir);
    }

    #[test]
    fn two_failures_share_the_one_status_line() {
        assert_eq!(join_notes(None, None), None);
        assert_eq!(
            join_notes(Some("bindings".into()), None).as_deref(),
            Some("bindings")
        );
        assert_eq!(
            join_notes(None, Some("recents".into())).as_deref(),
            Some("recents")
        );
        assert_eq!(
            join_notes(Some("bindings".into()), Some("recents".into())).as_deref(),
            Some("bindings recents")
        );
    }

    /// A scratch folder under the system temp directory, unique per test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ss550-gui-{}-{name}", std::process::id()));
        // A previous run that panicked can leave a file behind; the test's own
        // folder starts empty either way.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the test's scratch folder");
        dir
    }

    /// Best-effort removal: a leftover folder is not a test failure.
    fn cleanup(dir: PathBuf) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_release_binary_is_a_windows_gui() {
        // A console-subsystem binary gets a console window from Windows no
        // matter how it is started: double-click, the autostart entry, or a
        // terminal. This app is a tray application, so the release build must
        // be the GUI subsystem. Debug builds keep the console on purpose, so
        // `cargo run` still shows output and panics while developing.
        //
        // The test module is cut off first, so this assertion cannot match the
        // literal it contains and pass while the attribute is gone.
        let source = include_str!("main.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production
                .contains(r#"#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]"#),
            "the release binary must be a GUI subsystem, not a console one"
        );
    }

    #[test]
    fn the_window_never_names_the_deck_it_drives() {
        // The window follows whatever deck answers: a model name, a vendor, a
        // USB id or a key count written here is a claim about hardware the
        // window has no right to make. The needles are spelled in pieces so
        // this file does not match itself.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        sources(&root, &mut files);
        assert!(!files.is_empty(), "no sources under {}", root.display());

        let forbidden = [
            ["SS", "550"].concat(),
            ["Stream", "Craft"].concat(),
            ["Red", "ragon"].concat(),
            ["0200", ":3000"].concat(),
        ];
        let key_count: String = ['1', '5'].iter().collect();
        let mut problems = Vec::new();

        for path in files {
            let name = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .display()
                .to_string();
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(error) => {
                    problems.push(format!("{name}: unreadable: {error}"));
                    continue;
                }
            };

            for (line, body) in text.lines().enumerate() {
                for needle in &forbidden {
                    if body.contains(needle.as_str()) {
                        problems.push(format!("{name}:{}: names the deck ({needle})", line + 1));
                    }
                }
            }

            for number in integer_literals(&text) {
                if number == key_count {
                    problems.push(format!(
                        "{name}: uses the key count as a literal ({number})"
                    ));
                }
            }
        }

        assert!(
            problems.is_empty(),
            "the window must follow the deck, not name it:\n{}",
            problems.join("\n")
        );
    }

    /// Every `.rs` file under `dir`, recursively.
    fn sources(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                sources(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
    }

    /// The whole-number literals in `text`, so a key count can be spotted
    /// without flagging a pixel size that happens to share the digits.
    fn integer_literals(text: &str) -> Vec<String> {
        let bytes = text.as_bytes();
        let mut numbers = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            if !bytes[at].is_ascii_digit() {
                at += 1;
                continue;
            }
            let start = at;
            let mut fractional = false;
            while at < bytes.len() {
                match bytes[at] {
                    b'0'..=b'9' => at += 1,
                    b'.' if at + 1 < bytes.len() && bytes[at + 1].is_ascii_digit() => {
                        fractional = true;
                        at += 1;
                    }
                    _ => break,
                }
            }
            let previous = if start == 0 { b' ' } else { bytes[start - 1] };
            let attached = previous.is_ascii_alphanumeric() || previous == b'_' || previous == b'.';
            // `px(15.)` is a size, not a key count: a trailing dot is a float.
            let trailing_dot = at < bytes.len() && bytes[at] == b'.';
            if !attached && !fractional && !trailing_dot {
                numbers.push(text[start..at].to_string());
            }
        }
        numbers
    }

    #[test]
    fn slot_for_reddish_goes_red_else_white() {
        // Red-dominant colour goes to Slot::Red, everything else to Slot::White
        assert!(matches!(slot_for([255, 0, 0]), Slot::Red));
        assert!(matches!(slot_for([255, 1, 1]), Slot::Red));

        assert!(matches!(slot_for([0, 255, 0]), Slot::White));
        assert!(matches!(slot_for([0, 0, 255]), Slot::White));
        assert!(matches!(slot_for([10, 20, 30]), Slot::White));
        assert!(matches!(slot_for([128, 128, 128]), Slot::White));
    }
}
