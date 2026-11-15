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
//! The window is a shell of its own: an app bar, a rail of four pages, a
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

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use deck_api::{ButtonState, DeckError, KeyId};
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{
    px, size, AnyWindowHandle, AppContext as _, Context, Entity, IntoElement, Render, SharedString,
    Window, WindowBounds, WindowOptions,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use ss550_driver::{is_deck_present, LONG_CONNECT_TIMEOUT};
use ss550_engine::{Action, ActionRunner, ActionStore, KeyNumber};
use ss550_tray::{autostart, install, TrayCommand, TrayHost};

use ui::Page;

mod diagnostics;
mod provider;
mod render;
mod session;
mod ui;

/// Clicks on the version line that open the diagnostic log.
///
/// The trigger is hidden on purpose: this is a support tool, not a feature, and
/// a button for it would invite clicks that answer nothing.
const VERSION_CLICKS_TO_REVEAL: u8 = 5;

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
    White,
    Red,
}

impl Paint {
    /// The colour this paint puts on a key, as an image encoder sees it.
    fn rgb(self) -> [u8; 3] {
        match self {
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

/// Which kind of action the bindings panel is editing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DraftKind {
    /// Run a program.
    Launch,
    /// Open a URL.
    OpenUrl,
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
    },
    /// The attempt failed; the error is shown to the user.
    Failed {
        generation: u64,
        error: provider::ConnectError,
    },
}

/// Runs connection attempts on a dedicated thread.
///
/// Opening a deck blocks for seconds while it waits for the boot
/// re-enumeration to settle, so it must never run on the UI thread.
///
/// The attempt waits [`LONG_CONNECT_TIMEOUT`] on purpose: the deck's boot is
/// appear â†’ leave â†’ brief appearance â†’ leave â†’ settle, and the settle can be
/// ~11 s away. Giving up earlier would mean starting over in a different
/// phase of the boot, over and over.
fn connector_worker(commands: Receiver<u64>, results: Sender<Attempt>) {
    while let Ok(generation) = commands.recv() {
        let attempt = match provider::connect(LONG_CONNECT_TIMEOUT) {
            Ok(deck) => Attempt::Connected { generation, deck },
            Err(error) => Attempt::Failed { generation, error },
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
    /// Runs bound actions off the UI thread.
    runner: ActionRunner,
    /// The key the bindings panel is editing.
    selected_key: KeyNumber,
    /// Which kind of action the panel is editing.
    draft: DraftKind,
    program_input: Entity<InputState>,
    args_input: Entity<TextareaState>,
    url_input: Entity<InputState>,
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
    /// The library as the picker lists it, re-read when the user asks.
    library: Vec<ss550_engine::Asset>,
    /// The picture the picker has highlighted, if any.
    selected_asset: Option<ss550_engine::AssetName>,
    /// The highlighted picture, already fitted and swapped for the renderer.
    /// Built when the selection changes, never while drawing: decoding a photo
    /// on every frame would make the window crawl.
    preview: Option<(ss550_engine::AssetName, Arc<gpui_kit::RenderImage>)>,
    /// The pictures the tiles and the gallery show, already fitted to the deck's
    /// tile size and swapped for the renderer.
    ///
    /// Built when the deck, the profile or the library changes — never while
    /// drawing. A tile that finds its picture here paints it; a tile that does
    /// not falls back to its flat colour, so a picture deleted from the library
    /// while a profile still names it cannot blank a key or panic the window.
    previews: BTreeMap<ss550_engine::AssetName, Arc<gpui_kit::RenderImage>>,
    /// Tiles already rendered for this deck's image size, so switching profile
    /// back and forth does not re-decode every picture.
    tiles: Option<render::TileCache>,
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
                .spawn(move || connector_worker(command_rx, result_tx))
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

        // The tray icon is what keeps the controller alive with no window on
        // screen. Without it the app still runs, but only as a window.
        let (tray, tray_error) = match TrayHost::spawn("Deck Controller") {
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
                if this.last_tick.elapsed() >= TICK {
                    this.last_tick = Instant::now();
                    this.maintain();
                    changed = true;
                }
                changed |= this.drain_action_failures();
                changed |= this.handle_tray(cx);
                if changed {
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();

        let mut controller = Self {
            session: None,
            link: if blind {
                Link::Waiting
            } else {
                Link::Connecting
            },
            note: match worker_error {
                Some(err) => err.into(),
                None if blind => "Started without looking for a deck.".into(),
                None => "Looking for the deck…".into(),
            },
            page: Page::Deck,
            paint: Paint::White,
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
            runner: ActionRunner::new(),
            selected_key: KeyNumber::default(),
            draft: DraftKind::Launch,
            program_input,
            args_input,
            url_input,
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
            library: Vec::new(),
            selected_asset: None,
            preview: None,
            previews: BTreeMap::new(),
            tiles: None,
        };

        // Look for the deck right away: its settle window starts the moment
        // it appears, so every second counts.
        let _ = controller.refresh_library();
        controller.request_attempt();
        controller
    }

    /// Applies every result the worker produced since the last tick.
    fn collect_attempts(&mut self) {
        let attempts: Vec<Attempt> = self.results.try_iter().collect();
        for attempt in attempts {
            match attempt {
                Attempt::Connected { generation, deck } => {
                    if generation != self.generation {
                        // A newer request replaced this one; its result wins.
                        continue;
                    }
                    self.pending = false;
                    self.attempts = 0;
                    // Reset backoff on successful connection
                    self.reconnect_at = Instant::now();
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

        let path = ss550_engine::bindings_path_for(session.key());
        let (store, note) = self.load_store(&path, &session);

        // Load profiles for this deck
        let deck_key = session.key().to_string();
        let config_dir = ss550_engine::config_dir();
        let loaded_store = ss550_engine::ProfileStore::load(&deck_key, &config_dir)
            .unwrap_or_else(|_| ss550_engine::ProfileStore::new());
        self.profile_store = loaded_store;

        self.store = store;
        self.store_path = Some(path);
        self.slots = vec![Slot::Off; key_count];
        self.pressed = vec![false; key_count];
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
        // A preview built for the previous deck's tile size is wrong for this
        // one. The picker keeps the files; the picture is rebuilt on the next
        // selection.
        self.preview = None;

        // Apply the active profile on connect
        if let Some(profile) = self.profile_store.active_profile() {
            self.store = profile.bindings.clone();
            let slot = self.paint_from_profile(&profile, key_count);
            let default_colour = self.paint.rgb();
            self.apply_all_appearances(slot, &profile.appearance, default_colour, key_count);
        } else {
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
    /// runs on a deck change, a profile switch or a library refresh, and never
    /// while a frame is being drawn.
    fn rebuild_previews(&mut self) {
        self.previews.clear();
        let names: Vec<ss550_engine::AssetName> = match self.profile_store.active() {
            Some(profile) => profile
                .appearance
                .values()
                .filter_map(|appearance| appearance.asset().cloned())
                .collect(),
            None => Vec::new(),
        };
        self.ensure_previews(names);

        // An open gallery lists every picture, not only the ones the profile
        // names, so its thumbnails come back with the same clear.
        if self.page == Page::Artwork {
            self.ensure_library_previews();
        }
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
        let Some(spec) = self.session.as_ref().map(session::DeckSession::image_spec) else {
            return;
        };

        for name in names {
            if self.previews.contains_key(&name) {
                continue;
            }
            let Ok(source) = self.assets.read(&name) else {
                continue;
            };
            let Ok(frame) = render::preview_frame(spec, &name, &source) else {
                continue;
            };
            self.previews
                .insert(name, Arc::new(gpui_kit::RenderImage::new([frame])));
        }
    }

    /// Fits every picture in the library to the deck's tile.
    ///
    /// Called when the gallery opens and when the folder is re-read, so the
    /// gallery lists thumbnails without decoding anything during a frame.
    fn ensure_library_previews(&mut self) {
        let names: Vec<ss550_engine::AssetName> = self
            .library
            .iter()
            .map(|asset| asset.name.clone())
            .collect();
        self.ensure_previews(names);
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
    }

    /// Moves the window to a page, building what that page needs before its
    /// first frame.
    ///
    /// The gallery needs a thumbnail per picture, and a thumbnail is a decode.
    /// It happens here — on the click that opens the page — and never inside
    /// `render`.
    fn show_page(&mut self, page: Page) {
        self.page = page;
        if page == Page::Artwork {
            self.ensure_library_previews();
        }
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
        if let Err(e) = self.profile_store.activate(name) {
            self.note = format!("Failed to switch profile: {e}").into();
            return;
        }

        // If deck is connected, apply the profile
        if let Some(session) = self.session.as_mut() {
            // Get profile data before mutating self.store
            let profile_data = self.profile_store.active_profile();

            if let Some(profile) = profile_data {
                // Apply bindings
                self.store = profile.bindings.clone();

                // Paint all keys according to profile colors or slot
                let key_count = session.key_count();
                let slot = self.paint_from_profile(&profile, key_count);
                let default_colour = self.paint.rgb();
                self.apply_all_appearances(slot, &profile.appearance, default_colour, key_count);

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

    /// Determines the paint slot from a profile (uses per-key color override if available).
    fn paint_from_profile(&self, profile: &ss550_engine::Profile, _key_count: usize) -> Slot {
        // Default slot based on profile's slot name
        let slot_name = profile.slot_name();
        if slot_name == "red" {
            Slot::Red
        } else {
            Slot::White
        }
    }

    /// Reports what went wrong, and drops the deck only when the bytes failed.
    ///
    /// A typed refusal is the deck answering: the link is fine, and the window
    /// must not claim it lost a deck that just told it exactly what it cannot
    /// do. Transport failure is the only thing that means the link is gone.
    fn report(&mut self, error: session::SessionError) {
        let lost = matches!(error, session::SessionError::Deck(DeckError::Transport(_)));
        if lost {
            self.drop_deck();
            self.note = format!("Connection lost: {error} — reconnecting automatically.").into();
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
    /// A key the profile says nothing about gets `default_colour`. A key with an
    /// appearance gets exactly that: a flat colour, or a picture read from the
    /// library, fitted to the deck's image size and encoded within its byte
    /// limit. The picture path never touches the wire with bytes the deck has
    /// not already accepted at this size — a refusal here names the file.
    fn apply_all_appearances(
        &mut self,
        slot: Slot,
        appearances: &BTreeMap<u16, ss550_engine::Appearance>,
        default_colour: [u8; 3],
        key_count: usize,
    ) {
        let Some(session) = self.session.as_mut() else {
            self.note = "Not connected. Press RECONNECT first.".into();
            return;
        };

        let mut failure = None;
        let mut painted = vec![slot; key_count];
        for (index, entry) in painted.iter_mut().enumerate() {
            let key = u16::try_from(index + 1).unwrap_or(u16::MAX);
            let result = match appearances.get(&key) {
                Some(ss550_engine::Appearance::Image { asset }) => {
                    let tile = self
                        .assets
                        .read(asset)
                        .map_err(|error| error.to_string())
                        .and_then(|source| match self.tiles.as_mut() {
                            Some(cache) => cache
                                .get(asset, &source)
                                .map(<[u8]>::to_vec)
                                .map_err(|error| error.to_string()),
                            None => Err("the deck has not stated an image size yet".to_string()),
                        });
                    match tile {
                        Ok(jpeg) => {
                            *entry = Slot::Image;
                            session
                                .paint_encoded(index, &jpeg)
                                .map_err(PaintFailure::Session)
                        }
                        Err(message) => Err(PaintFailure::Picture(message)),
                    }
                }
                Some(ss550_engine::Appearance::Solid { rgb }) => {
                    session.paint(index, *rgb).map_err(PaintFailure::Session)
                }
                None => session
                    .paint(index, default_colour)
                    .map_err(PaintFailure::Session),
            };

            if let Err(error) = result {
                failure = Some(error);
                break;
            }
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
                    format!("{shown} of {key_count} keys show a picture.").into()
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
            self.note = format!("Key #{} pressed — nothing is bound to it yet.", idx + 1).into();
            return;
        };

        let summary = action.summary();
        match self.runner.submit(action) {
            Ok(()) => self.note = format!("Key #{} pressed — {summary}.", idx + 1).into(),
            Err(err) => {
                self.note = format!(
                    "Key #{} pressed, but the action was refused: {err}",
                    idx + 1
                )
                .into();
            }
        }
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

        // Hotplug detection: check if deck was unexpectedly disconnected
        if self.session.is_some() && !is_deck_present() {
            self.handle_disconnect();
        }

        // Try to reconnect if disconnected and backoff allows
        if self.session.is_none() && !self.pending && Instant::now() >= self.reconnect_at {
            self.request_attempt();
        }

        self.heartbeat();
    }

    /// Handles an unexpected disconnect with exponential backoff.
    fn handle_disconnect(&mut self) {
        self.drop_deck();
        self.schedule_retry();

        self.note = format!(
            "Connection lost: deck disconnected. Retrying in {}s (attempt {}).",
            self.retry_delay().as_secs(),
            self.attempts
        )
        .into();
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

        match self.store.get(number).cloned() {
            Some(Action::Launch { program, args }) => {
                self.draft = DraftKind::Launch;
                self.program_input.update(cx, |state, cx| {
                    state.set_value(program.display().to_string(), window, cx);
                });
                self.args_input
                    .update(cx, |state, cx| state.set_value(args.join("\n"), window, cx));
                self.url_input
                    .update(cx, |state, cx| state.set_value(String::new(), window, cx));
            }
            Some(Action::OpenUrl { url }) => {
                self.draft = DraftKind::OpenUrl;
                self.url_input
                    .update(cx, |state, cx| state.set_value(url, window, cx));
                self.program_input
                    .update(cx, |state, cx| state.set_value(String::new(), window, cx));
                self.args_input
                    .update(cx, |state, cx| state.set_value(String::new(), window, cx));
            }
            None => {
                self.program_input
                    .update(cx, |state, cx| state.set_value(String::new(), window, cx));
                self.args_input
                    .update(cx, |state, cx| state.set_value(String::new(), window, cx));
                self.url_input
                    .update(cx, |state, cx| state.set_value(String::new(), window, cx));
            }
        }

        self.note = match self.store.get(number) {
            Some(action) => format!("Key #{} is bound to {}.", number.get(), action.describe()),
            None => format!(
                "Key #{} has no action yet. Fill a field below and press SAVE BINDING.",
                number.get()
            ),
        }
        .into();
    }

    /// Binds the selected key to what the panel holds, and saves it.
    fn save_binding(&mut self, cx: &mut Context<Self>) {
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
        };

        let Some(path) = self.store_path.clone() else {
            self.note = "No deck is connected, so a binding has no deck to belong to yet.".into();
            return;
        };

        match self.store.set(self.selected_key, action) {
            Ok(()) => {
                // Save the profile with updated bindings
                if let Some(session) = self.session.as_ref() {
                    let deck_key = session.key().to_string();
                    let config_dir = ss550_engine::config_dir();
                    let _ = self
                        .profile_store
                        .update_bindings(&self.store, &deck_key, &config_dir);
                }
                match self.store.save(&path) {
                    Ok(()) => {
                        let summary = self
                            .store
                            .get(self.selected_key)
                            .map(Action::summary)
                            .unwrap_or_default();
                        self.note =
                            format!("Key #{} saved — {summary}.", self.selected_key.get()).into();
                    }
                    Err(err) => {
                        self.note = format!(
                            "Key #{} was bound, but the file was not saved: {err}",
                            self.selected_key.get()
                        )
                        .into();
                    }
                }
            }
            Err(err) => {
                self.note = format!("Key #{} was not bound: {err}", self.selected_key.get()).into();
            }
        }
    }

    /// Removes the selected key's binding and saves the file.
    fn clear_binding(&mut self) {
        self.remove_binding(self.selected_key);
    }

    /// Removes one key's binding and saves the file.
    ///
    /// Takes the key as an argument so the bound list can unbind a row without
    /// moving the selection away from the key the editor is showing.
    fn remove_binding(&mut self, number: KeyNumber) {
        let Some(path) = self.store_path.clone() else {
            self.note = "No deck is connected, so there is no binding file to clear.".into();
            return;
        };
        match self.store.remove(number) {
            Some(_) => match self.store.save(&path) {
                Ok(()) => {
                    self.note =
                        format!("Key #{} cleared — it does nothing now.", number.get()).into();
                }
                Err(err) => {
                    self.note = format!(
                        "Key #{} was cleared, but the file was not saved: {err}",
                        number.get()
                    )
                    .into();
                }
            },
            None => {
                self.note = format!("Key #{} had no action to clear.", number.get()).into();
            }
        }
    }

    /// Re-reads the picture library, so a file the user just dropped in shows up.
    ///
    /// Returns how many pictures the library holds. The caller decides what to
    /// say about it: this runs at startup, where the connection note matters
    /// more than the library count, and from a button, where the count is the
    /// answer the user asked for.
    fn refresh_library(&mut self) -> Result<usize, String> {
        let assets = self.assets.list().map_err(|error| error.to_string())?;
        let count = assets.len();
        self.library = assets;

        // A file the user replaced by hand has the same name the cache already
        // holds. Refreshing is the moment they asked to look again, so both
        // caches start over.
        if let Some(tiles) = self.tiles.as_mut() {
            tiles.clear();
        }
        self.previews.clear();

        // A picture that is no longer there must not stay highlighted.
        let still_there = self
            .selected_asset
            .as_ref()
            .is_some_and(|name| self.library.iter().any(|asset| &asset.name == name));
        if !still_there {
            self.selected_asset = None;
            self.preview = None;
        }

        // The pictures the keys show are rebuilt whether or not the gallery is
        // open; the gallery's own thumbnails with them, when it is.
        self.rebuild_previews();

        Ok(count)
    }

    /// Highlights one picture and builds the preview the picker draws.
    ///
    /// The preview is built here, once per selection, and not while drawing:
    /// decoding a photograph on every frame would make the window crawl.
    fn select_asset(&mut self, name: ss550_engine::AssetName) {
        self.selected_asset = Some(name.clone());

        let Some(spec) = self.session.as_ref().map(session::DeckSession::image_spec) else {
            self.preview = None;
            self.note = "Connect a deck to see a picture at the size it will use.".into();
            return;
        };

        let source = match self.assets.read(&name) {
            Ok(source) => source,
            Err(error) => {
                self.preview = None;
                self.note = format!("Could not read {name}: {error}").into();
                return;
            }
        };

        match render::preview_frame(spec, &name, &source) {
            Ok(frame) => {
                self.preview = Some((name.clone(), Arc::new(gpui_kit::RenderImage::new([frame]))));
                self.note = format!("{name} is ready for a key.").into();
            }
            Err(error) => {
                self.preview = None;
                self.note = format!("{error}").into();
            }
        }
    }

    /// Makes the selected key show the highlighted picture, and saves it.
    fn assign_asset(&mut self) {
        let Some(name) = self.selected_asset.clone() else {
            self.note = "Pick a picture first.".into();
            return;
        };

        let key = u16::from(self.selected_key.get());
        if let Err(error) = self
            .profile_store
            .set_appearance(key, Some(ss550_engine::Appearance::image(name.clone())))
        {
            self.note = format!("Key #{key} was not changed: {error}").into();
            return;
        }

        self.save_profile();
        // The key now shows a picture, so the tile needs it fitted before the
        // next frame rather than during it.
        self.rebuild_previews();
        if self.repaint_key(key) {
            self.note = format!("Key #{key} now shows {name}.").into();
        }
    }

    /// Takes the artwork off the selected key, so it shows the profile colour.
    fn remove_artwork(&mut self) {
        let key = u16::from(self.selected_key.get());
        if let Err(error) = self.profile_store.set_appearance(key, None) {
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
        let Some(deck_key) = self
            .session
            .as_ref()
            .map(|session| session.key().to_string())
        else {
            self.note = "Connect a deck to save a profile.".into();
            return;
        };
        if let Err(error) = self
            .profile_store
            .save(&deck_key, &ss550_engine::config_dir())
        {
            self.note = format!("The profile changed but was not saved: {error}").into();
        }
    }

    /// Repaints one key from what the profile says it shows now.
    ///
    /// Returns `false` when the picture path failed; the note already says why.
    /// A colour or a removal cannot fail this way, so the caller's own message
    /// survives.
    fn repaint_key(&mut self, key: u16) -> bool {
        let appearance = self
            .profile_store
            .active()
            .and_then(|profile| profile.appearance_for(key))
            .cloned();
        let index = usize::from(key.saturating_sub(1));

        let Some(session) = self.session.as_mut() else {
            return true;
        };

        let result = match &appearance {
            Some(ss550_engine::Appearance::Image { asset }) => {
                let tile = self
                    .assets
                    .read(asset)
                    .map_err(|error| error.to_string())
                    .and_then(|source| match self.tiles.as_mut() {
                        Some(cache) => cache
                            .get(asset, &source)
                            .map(<[u8]>::to_vec)
                            .map_err(|error| error.to_string()),
                        None => Err("the deck has not stated an image size yet".to_string()),
                    });
                match tile {
                    Ok(jpeg) => session
                        .paint_encoded(index, &jpeg)
                        .map_err(PaintFailure::Session),
                    Err(message) => Err(PaintFailure::Picture(message)),
                }
            }
            Some(ss550_engine::Appearance::Solid { rgb }) => {
                session.paint(index, *rgb).map_err(PaintFailure::Session)
            }
            None => session
                .paint(index, self.paint.rgb())
                .map_err(PaintFailure::Session),
        };

        match result {
            Ok(()) => {
                let slot = match &appearance {
                    Some(ss550_engine::Appearance::Image { .. }) => Slot::Image,
                    Some(ss550_engine::Appearance::Solid { rgb }) => slot_for(*rgb),
                    None => self.paint.slot(),
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
    /// There is no file dialog in this delivery: the user drops files in the
    /// folder the app owns and presses REFRESH. This button exists so "the
    /// folder" is a place they can reach without knowing where the app keeps
    /// its configuration.
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

impl Render for Controller {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                window.set_window_title("Deck Controller");
                Theme::change(ThemeMode::Dark, Some(window), cx);
                cx.new(|cx| Controller::new(window, cx))
            })
            .expect("failed to open the controller window");
        });
}

#[cfg(test)]
mod tests {
    use super::{advance_clicks, slot_for, Slot};

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
