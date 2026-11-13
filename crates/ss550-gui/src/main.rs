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
//! Pressing a key on the deck lights its tile and runs the action bound to it,
//! on a background thread. Bindings are kept per deck, in the app's own
//! configuration folder.

// The release build is a GUI binary: Windows creates a console window for a
// console-subsystem executable no matter how it is started, and a tray
// application must not have one. Debug builds keep the console so `cargo run`
// still shows output and panics while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![deny(unsafe_code)]

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::time::{Duration, Instant};

use deck_api::{ButtonState, DeckError, KeyId};
use gpui_kit::component::{
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    form::Field,
    input::{Input, InputState, Textarea, TextareaState},
    switch::Switch,
    ActiveTheme as _, Theme, ThemeMode,
};
use gpui_kit::{
    div, px, rgb, size, AnyElement, AnyWindowHandle, AppContext as _, ClickEvent, Context, Entity,
    FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, WindowBounds,
    WindowOptions,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use ss550_driver::LONG_CONNECT_TIMEOUT;
use ss550_engine::{Action, ActionRunner, ActionStore, KeyNumber};
use ss550_tray::{autostart, install, TrayCommand, TrayHost};

mod diagnostics;
mod provider;
mod session;

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

/// Hex colour as GPUI `Hsla`.
fn color(hex: u32) -> Hsla {
    rgb(hex).into()
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

/// How often the deck is polled for key presses.
///
/// The poll is non-blocking, so this only costs a read that usually returns
/// nothing. 50 ms is fast enough that a press feels instant.
const POLL: Duration = Duration::from_millis(50);

/// How often connection results, retries and the heartbeat are handled.
const TICK: Duration = Duration::from_millis(500);

/// Minimum delay between two connection attempts.
///
/// The deck leaves the USB bus for 4–8 s while it boots and re-enumerates
/// itself more than once, so a single attempt can easily miss it. Retrying
/// forever is what makes "plug the deck in while the app is open" work.
const RETRY_INTERVAL: Duration = Duration::from_secs(2);

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
/// appear → leave → brief appearance → leave → settle, and the settle can be
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
    /// When the last attempt was requested.
    last_attempt: Option<Instant>,
    /// Attempts made since the last successful connection.
    attempts: u32,
    /// When the connection and heartbeat work last ran.
    last_tick: Instant,
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
    /// Clicks on the version line, until they add up to the reveal.
    version_clicks: u8,
}

impl Controller {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (commands, command_rx) = mpsc::sync_channel::<u64>(1);
        let (result_tx, results) = mpsc::channel::<Attempt>();

        let worker_error = match std::thread::Builder::new()
            .name("deck-connector".to_string())
            .spawn(move || connector_worker(command_rx, result_tx))
        {
            Ok(_) => None,
            Err(err) => Some(format!("Could not start the connection worker: {err}")),
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
            link: Link::Connecting,
            note: match worker_error {
                Some(err) => err.into(),
                None => "Looking for the deck…".into(),
            },
            paint: Paint::White,
            slots: Vec::new(),
            pressed: Vec::new(),
            brightness: deck_api::VALUE_MAX,
            commands,
            results,
            generation: 0,
            pending: false,
            last_attempt: None,
            attempts: 0,
            last_tick: Instant::now(),
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
            version_clicks: 0,
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
                Attempt::Connected { generation, deck } => {
                    if generation != self.generation {
                        // A newer request replaced this one; its result wins.
                        continue;
                    }
                    self.pending = false;
                    self.attempts = 0;
                    self.adopt(deck);
                }
                Attempt::Failed { generation, error } => {
                    if generation != self.generation {
                        continue;
                    }
                    self.pending = false;
                    let message = error.to_string();

                    match error {
                        provider::ConnectError::NotFound(_) => {
                            // The deck is simply not there. This is not a
                            // failure: the app keeps looking for it.
                            self.link = Link::Waiting;
                            self.note = format!(
                                "{message} — the deck is not on the bus. \
                                 Retrying every {} s (attempt {}).",
                                RETRY_INTERVAL.as_secs(),
                                self.attempts
                            )
                            .into();
                        }
                        provider::ConnectError::Other(_) => {
                            self.link = Link::Lost;
                            self.note = format!(
                                "{message} — retrying every {} s (attempt {}).",
                                RETRY_INTERVAL.as_secs(),
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

        self.store = store;
        self.store_path = Some(path);
        self.slots = vec![Slot::Off; key_count];
        self.pressed = vec![false; key_count];
        if !key_exists(&session, self.selected_key) {
            self.selected_key = KeyNumber::default();
        }
        self.session = Some(session);
        self.link = Link::Connected;

        let slot = self.paint.slot();
        self.apply_all(slot);
        if self.session.is_some() {
            self.note = match note {
                Some(note) => note.into(),
                None => format!(
                    "Connected — all {key_count} keys painted {}.",
                    slot_name(slot)
                )
                .into(),
            };
        }
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

    /// Asks for a new attempt when the deck is missing and the retry
    /// interval has elapsed.
    fn request_attempt_if_due(&mut self) {
        if self.session.is_some() || self.pending {
            return;
        }
        if let Some(last) = self.last_attempt {
            if last.elapsed() < RETRY_INTERVAL {
                return;
            }
        }
        self.request_attempt();
    }

    /// Queues one connection attempt for the worker.
    ///
    /// The generation counter makes a late result harmless: if a newer
    /// request was made in the meantime, the old result is dropped instead
    /// of replacing a fresh handle with a stale one.
    fn request_attempt(&mut self) {
        let generation = self.generation + 1;
        if self.commands.try_send(generation).is_err() {
            // A request is already queued: it will produce a fresh attempt.
            return;
        }
        self.generation = generation;
        self.pending = true;
        self.last_attempt = Some(Instant::now());
        self.attempts += 1;
        self.link = Link::Connecting;
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

    /// Paints one key with the colour picked in step 1.
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
        let Some(session) = self.session.as_mut() else {
            self.note = "Not connected. Press RECONNECT first.".into();
            return;
        };
        let key_count = session.key_count();
        let mut failure = None;
        for index in 0..key_count {
            if let Err(error) = session.paint(index, colour) {
                failure = Some(error);
                break;
            }
        }

        match failure {
            Some(error) => self.report(error),
            None => {
                self.slots = vec![slot; key_count];
                self.note = format!("All {key_count} keys painted {}.", slot_name(slot)).into();
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
        self.request_attempt_if_due();
        self.heartbeat();
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
            Ok(()) => match self.store.save(&path) {
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
            },
            Err(err) => {
                self.note = format!("Key #{} was not bound: {err}", self.selected_key.get()).into();
            }
        }
    }

    /// Removes the selected key's binding and saves the file.
    fn clear_binding(&mut self) {
        let Some(path) = self.store_path.clone() else {
            self.note = "No deck is connected, so there is no binding file to clear.".into();
            return;
        };
        match self.store.remove(self.selected_key) {
            Some(_) => match self.store.save(&path) {
                Ok(()) => {
                    self.note = format!(
                        "Key #{} cleared — it does nothing now.",
                        self.selected_key.get()
                    )
                    .into();
                }
                Err(err) => {
                    self.note = format!(
                        "Key #{} was cleared, but the file was not saved: {err}",
                        self.selected_key.get()
                    )
                    .into();
                }
            },
            None => {
                self.note =
                    format!("Key #{} had no action to clear.", self.selected_key.get()).into()
            }
        }
    }
}

fn slot_name(slot: Slot) -> &'static str {
    match slot {
        Slot::White => "white",
        Slot::Red => "red",
        Slot::Off => "off",
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

/// The label above one display row: where it sits, and the keys in it.
///
/// The row count comes from the deck, so a deck with two rows and a deck with
/// five get the right words without the window knowing either number.
fn row_label(depth: usize, total: usize, keys: &[usize]) -> String {
    let place = if depth == 0 {
        "TOP ROW"
    } else if depth + 1 == total {
        "BOTTOM ROW"
    } else {
        "MIDDLE ROW"
    };
    match (keys.first(), keys.last()) {
        (Some(first), Some(last)) => format!("{place} · keys {} to {}", first + 1, last + 1),
        _ => place.to_string(),
    }
}

/// One clickable key tile: the tile itself is the colour the key shows.
fn key_tile(
    idx: usize,
    slot: Slot,
    pressed: bool,
    binding: Option<SharedString>,
    cx: &mut Context<Controller>,
) -> impl IntoElement {
    let off_fill = cx.theme().muted;
    let off_ink = cx.theme().muted_foreground;
    let idle_border = cx.theme().muted_foreground.opacity(0.35);
    let active_border = cx.theme().primary;

    let (fill, ink, name) = match slot {
        Slot::White => (color(0xe4e4e7), color(0x18181b), "WHITE"),
        Slot::Red => (color(0xdc2626), color(0xffffff), "RED"),
        Slot::Off => (off_fill, off_ink, "OFF"),
    };

    let has_binding = binding.is_some();
    let binding_text = binding.unwrap_or_else(|| "NO ACTION".into());
    let binding_ink = if has_binding { ink } else { ink.opacity(0.45) };

    div()
        .id(("deck-key", idx))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(2.))
        .w(px(140.))
        .h(px(84.))
        .rounded(px(12.))
        .border_2()
        .border_color(if pressed { active_border } else { idle_border })
        .bg(fill)
        .text_color(ink)
        .cursor_pointer()
        .child(
            div()
                .text_size(px(20.))
                .font_weight(FontWeight::BOLD)
                .child(format!("#{}", idx + 1)),
        )
        .child(div().text_size(px(12.)).child(name))
        .child(
            div()
                .text_size(px(11.))
                .text_color(binding_ink)
                .child(binding_text),
        )
        .child(
            div()
                .text_size(px(9.))
                .font_weight(FontWeight::BOLD)
                .text_color(if pressed {
                    active_border
                } else {
                    ink.opacity(0.0)
                })
                .child("PRESSED"),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.paint_key(idx);
            cx.notify();
        }))
}

/// One small button of the key picker in the bindings panel.
fn picker_button(
    idx: usize,
    number: KeyNumber,
    selected: bool,
    bound: bool,
    cx: &mut Context<Controller>,
) -> impl IntoElement {
    let primary = cx.theme().primary;
    let foreground = cx.theme().foreground;
    let muted = cx.theme().muted;
    let muted_foreground = cx.theme().muted_foreground;

    let (fill, ink, border) = if selected {
        (primary.opacity(0.18), foreground, primary)
    } else if bound {
        (muted, foreground, primary.opacity(0.6))
    } else {
        (
            muted.opacity(0.3),
            muted_foreground,
            muted_foreground.opacity(0.25),
        )
    };

    div()
        .id(("picker-key", idx))
        .flex()
        .items_center()
        .justify_center()
        .w(px(44.))
        .h(px(30.))
        .rounded(px(8.))
        .border_2()
        .border_color(border)
        .bg(fill)
        .text_color(ink)
        .text_size(px(13.))
        .font_weight(FontWeight::BOLD)
        .cursor_pointer()
        .child(number.get().to_string())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.select_key(number, window, cx);
            cx.notify();
        }))
}

fn section_title(text: &'static str, cx: &mut Context<Controller>) -> impl IntoElement {
    div()
        .text_size(px(14.))
        .font_weight(FontWeight::BOLD)
        .text_color(cx.theme().foreground)
        .child(text)
}

fn hint(text: impl Into<SharedString>, cx: &mut Context<Controller>) -> impl IntoElement {
    div()
        .text_size(px(13.))
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

impl Render for Controller {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().background;
        let foreground = cx.theme().foreground;
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let primary = cx.theme().primary;

        let (status_fill, status_ink, status_text) = match self.link {
            Link::Connected => (color(0x16a34a), color(0xffffff), "CONNECTED"),
            Link::Connecting => (color(0xf59e0b), color(0x18181b), "CONNECTING"),
            Link::Waiting => (color(0xf59e0b), color(0x18181b), "WAITING FOR DECK"),
            Link::Lost => (color(0xdc2626), color(0xffffff), "CONNECTION LOST"),
        };

        // The header names the deck in front of the window, and nothing else.
        // Before one answers there is no deck to name, so the window says so
        // instead of describing a model it may not be talking to.
        let (deck_title, deck_subtitle) = match self.session.as_ref() {
            Some(session) => {
                let geometry = session.geometry();
                let spec = session.image_spec();
                let mut subtitle = format!(
                    "{} keys · {} columns · {}x{} images",
                    geometry.key_count(),
                    geometry.columns(),
                    spec.width(),
                    spec.height()
                );
                let dials = session.capabilities().dial_count();
                if dials > 0 {
                    subtitle.push_str(&format!(" · {dials} dials"));
                }
                (SharedString::from(session.label().to_string()), subtitle)
            }
            None => (
                SharedString::from("No deck yet"),
                "Plug a deck in; this window follows whatever answers.".to_string(),
            ),
        };

        let is_dark = cx.theme().mode.is_dark();
        let theme_toggle = Button::new("theme-toggle")
            .secondary()
            .label(if is_dark {
                "THEME: DARK · CLICK FOR LIGHT"
            } else {
                "THEME: LIGHT · CLICK FOR DARK"
            })
            .on_click(cx.listener(|_, _: &ClickEvent, window, cx| {
                let next = if cx.theme().mode.is_dark() {
                    ThemeMode::Light
                } else {
                    ThemeMode::Dark
                };
                Theme::change(next, Some(window), cx);
                cx.notify();
            }));

        let white_selected = self.paint == Paint::White;
        let red_selected = self.paint == Paint::Red;

        let paint_white = Button::new("paint-white")
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(color(0xe4e4e7))
                    .foreground(color(0x18181b))
                    .hover(color(0xd4d4d8))
                    .active(color(0xc4c4c9)),
            )
            .label(if white_selected {
                "WHITE  ✓ SELECTED"
            } else {
                "WHITE"
            })
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.paint = Paint::White;
                this.note = "Clicking a key now paints it white.".into();
                cx.notify();
            }));

        let paint_red = Button::new("paint-red")
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(color(0xdc2626))
                    .foreground(color(0xffffff))
                    .hover(color(0xc31f1f))
                    .active(color(0xab1a1a)),
            )
            .label(if red_selected {
                "RED  ✓ SELECTED"
            } else {
                "RED"
            })
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.paint = Paint::Red;
                this.note = "Clicking a key now paints it red.".into();
                cx.notify();
            }));

        // What each key does, by tile index, for the labels on the deck. The
        // range comes from the deck, not from a key count written here.
        let key_count = self
            .session
            .as_ref()
            .map_or(0, session::DeckSession::key_count);
        let binding_summaries: Vec<Option<SharedString>> = self
            .session
            .as_ref()
            .map(|session| {
                (0..key_count)
                    .map(|index| {
                        binding_number(session, index)
                            .and_then(|number| self.store.get(number))
                            .map(|action| SharedString::from(action.summary()))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut rows = Vec::new();
        if let Some(session) = self.session.as_ref() {
            for (depth, keys) in session.rows().iter().enumerate() {
                let tiles = keys
                    .iter()
                    .map(|&index| {
                        key_tile(
                            index,
                            self.slots.get(index).copied().unwrap_or(Slot::Off),
                            self.pressed.get(index).copied().unwrap_or(false),
                            binding_summaries.get(index).cloned().flatten(),
                            cx,
                        )
                    })
                    .collect::<Vec<_>>();
                rows.push(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(muted)
                                .child(row_label(depth, session.rows().len(), keys)),
                        )
                        .child(div().flex().flex_row().gap(px(10.)).children(tiles)),
                );
            }
        }

        // ── Bindings panel ────────────────────────────────────────────
        let mut picker_rows = Vec::new();
        if let Some(session) = self.session.as_ref() {
            for keys in session.rows() {
                let mut buttons = Vec::new();
                for &index in keys {
                    let Some(number) = binding_number(session, index) else {
                        continue;
                    };
                    buttons.push(picker_button(
                        index,
                        number,
                        number == self.selected_key,
                        self.store.get(number).is_some(),
                        cx,
                    ));
                }
                picker_rows.push(div().flex().flex_row().gap(px(6.)).children(buttons));
            }
        }

        let launch_selected = self.draft == DraftKind::Launch;
        let url_selected = self.draft == DraftKind::OpenUrl;
        let kind_launch = Button::new("draft-launch")
            .secondary()
            .label(if launch_selected {
                "LAUNCH APP  ✓"
            } else {
                "LAUNCH APP"
            })
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.draft = DraftKind::Launch;
                this.note = "Fill the program path, then press SAVE BINDING.".into();
                cx.notify();
            }));

        let kind_url = Button::new("draft-url")
            .secondary()
            .label(if url_selected {
                "OPEN URL  ✓"
            } else {
                "OPEN URL"
            })
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.draft = DraftKind::OpenUrl;
                this.note = "Fill the URL, then press SAVE BINDING.".into();
                cx.notify();
            }));

        let launch_fields = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                Field::new()
                    .label("PROGRAM · full path to the program to run")
                    .child(Input::new(&self.program_input)),
            )
            .child(
                Field::new()
                    .label("ARGUMENTS · one per line, each passed on its own")
                    .child(Textarea::new(&self.args_input)),
            );

        let url_fields = div().flex().flex_col().gap(px(8.)).child(
            Field::new()
                .label("URL · must start with its scheme, e.g. https://")
                .child(Input::new(&self.url_input)),
        );

        let draft_fields = if launch_selected {
            div().child(launch_fields)
        } else {
            div().child(url_fields)
        };

        let save = Button::new("save-binding")
            .primary()
            .label("SAVE BINDING")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.save_binding(cx);
                cx.notify();
            }));

        let clear = Button::new("clear-binding")
            .danger()
            .label("CLEAR KEY")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.clear_binding();
                cx.notify();
            }));

        // What the app does when it is not in front of the user.
        let tray_note = match &self.tray_error {
            Some(error) => format!("The tray icon is missing: {error}"),
            None => "The tray icon is in the notification area.".to_string(),
        };
        let background_section = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .w(px(380.))
            .child(section_title("STEP 4 · RUN IN THE BACKGROUND", cx))
            .child(hint(
                "The controller lives in the notification area. Close the window \
                 and it keeps the deck awake and still runs your keys. The tray \
                 icon brings the window back, and it is the only way to leave.",
                cx,
            ))
            .child(hint(tray_note, cx))
            .child(
                Switch::new("start-with-windows")
                    .checked(self.starts_with_windows)
                    .label("START WITH WINDOWS")
                    .on_change({
                        let controller = cx.entity();
                        move |checked: &bool, _window, cx| {
                            controller.update(cx, |this, cx| this.set_autostart(*checked, cx));
                        }
                    }),
            )
            .child(hint(
                "Turn this on and Windows starts the controller when you sign in, hidden in \
                 the tray. It copies the controller to %LOCALAPPDATA%\\Programs\\ss550, writes \
                 one entry under HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run, for \
                 this user only, and removes that entry again when you turn this off.",
                cx,
            ));
        let background_section = match &self.autostart_note {
            Some(note) => background_section.child(hint(note.clone(), cx)),
            None => background_section,
        };
        // The version line is the quiet way into the diagnostic log. It reads
        // as a stamp, not as a control, so nothing invites a click.
        let background_section = background_section.child(
            div()
                .id("version-line")
                .cursor_pointer()
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground.opacity(0.55))
                .child(format!("v{}", env!("CARGO_PKG_VERSION")))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.reveal_log();
                    cx.notify();
                })),
        );

        let bindings_panel = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .w(px(380.))
            .child(section_title("STEP 3 · BIND AN ACTION TO A KEY", cx))
            .child(hint(
                "Pick a key, choose what it does, fill the fields and press \
                 SAVE BINDING. CLEAR KEY makes the key do nothing again.",
                cx,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(muted)
                            .child(format!("EDITING KEY #{}", self.selected_key.get())),
                    )
                    .children(picker_rows),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .child(kind_launch)
                    .child(kind_url),
            )
            .child(draft_fields)
            .child(div().flex().flex_row().gap(px(8.)).child(save).child(clear))
            .child(hint(
                match &self.store_path {
                    Some(path) => format!("Bindings are saved to {}", path.display()),
                    None => "Bindings are saved once a deck answers.".to_string(),
                },
                cx,
            ));

        // ── Deck-wide controls, offered only where the deck has them ───
        //
        // The window does not decide what a deck can do: the deck's own
        // capabilities decide which buttons exist. A deck without brightness
        // never sees a brightness row, so nothing offers a control that could
        // only be refused.
        let capabilities = self
            .session
            .as_ref()
            .map_or(deck_api::Capabilities::default(), |session| {
                session.capabilities()
            });
        let paint_all_label = format!("PAINT ALL {key_count} KEYS");

        let mut controls: Vec<AnyElement> = Vec::new();
        controls.push(
            Button::new("paint-all")
                .primary()
                .label(paint_all_label)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    let slot = this.paint.slot();
                    this.apply_all(slot);
                    cx.notify();
                }))
                .into_any_element(),
        );
        if capabilities.clear_key() {
            controls.push(
                Button::new("clear-selected-key")
                    .secondary()
                    .label("CLEAR SELECTED KEY")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        let index = usize::from(this.selected_key.get().saturating_sub(1));
                        this.clear_painted_key(index);
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        if capabilities.clear_all() {
            controls.push(
                Button::new("clear-all")
                    .danger()
                    .label("CLEAR ALL (BLANK)")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.clear_all();
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        if capabilities.keep_alive() {
            controls.push(
                Button::new("keepalive")
                    .secondary()
                    .label("KEEP ALIVE · SEND ONE NOW")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.heartbeat();
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        controls.push(
            Button::new("reconnect")
                .secondary()
                .label("RECONNECT")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.drop_deck();
                    this.link = Link::Connecting;
                    this.attempts = 0;
                    this.note = "Reconnecting…".into();
                    this.request_attempt();
                    cx.notify();
                }))
                .into_any_element(),
        );
        controls.push(
            Button::new("hide")
                .secondary()
                .label("HIDE · KEEPS RUNNING")
                .on_click(cx.listener(|this, _: &ClickEvent, _, _| {
                    this.hide_window();
                }))
                .into_any_element(),
        );

        let mut wide_hint = String::from("PAINT ALL uses the colour from step 1.");
        if capabilities.clear_key() {
            wide_hint.push_str(" CLEAR SELECTED KEY blanks the key the bindings panel is editing.");
        }
        if capabilities.clear_all() {
            wide_hint.push_str(" CLEAR ALL blanks every key.");
        }
        if capabilities.keep_alive() {
            wide_hint.push_str(
                " The app keeps the deck awake by itself and sends a heartbeat on its own; \
                 KEEP ALIVE just sends one by hand.",
            );
        }
        if !capabilities.brightness() {
            wide_hint.push_str(" This deck has no brightness control, so none is offered.");
        }
        wide_hint.push_str(
            " RECONNECT looks for the deck again. HIDE takes the window off the screen and out \
             of the taskbar; the app keeps running in the notification area and still runs your \
             keys.",
        );

        let brightness_row: Vec<AnyElement> = if capabilities.brightness() {
            let mut buttons = Vec::new();
            for value in [25_u8, 50, 75, 100] {
                let label = if self.brightness == value {
                    format!("{value}%  ✓")
                } else {
                    format!("{value}%")
                };
                buttons.push(
                    Button::new(SharedString::from(format!("brightness-{value}")))
                        .secondary()
                        .label(label)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.set_brightness(value);
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
            vec![div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(muted)
                        .child("BRIGHTNESS"),
                )
                .children(buttons)
                .into_any_element()]
        } else {
            Vec::new()
        };

        // ── Left column: paint controls and the deck ──────────────────
        let left_column = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .w(px(792.))
            // Step 1: paint colour
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(section_title("STEP 1 · PICK THE COLOUR", cx))
                    .child(hint("The highlighted button is the colour used next.", cx))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(12.))
                            .child(paint_white)
                            .child(paint_red),
                    ),
            )
            // Step 2: deck
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(section_title("STEP 2 · CLICK A KEY TO PAINT IT", cx))
                    .child(hint(
                        "Each tile is the real colour of that key on the deck right now, \
                         and the line under the colour is what pressing it runs. \
                         A bright border means the key is pressed right now. \
                         Rows match the device from top to bottom.",
                        cx,
                    ))
                    .children(rows),
            )
            .child(div().h(px(1.)).w_full().bg(border))
            // Deck-wide actions
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap(px(10.))
                    .children(controls),
            )
            .child(hint(wide_hint, cx))
            .children(brightness_row)
            // Status line
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(if self.link == Link::Lost {
                        cx.theme().danger
                    } else {
                        primary
                    })
                    .child(self.note.clone()),
            );

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(background)
            .text_color(foreground)
            .p(px(24.))
            .gap(px(16.))
            // ── Header ────────────────────────────────────────────────
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(26.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(deck_title),
                            )
                            .child(hint(deck_subtitle, cx)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(10.))
                            .child(theme_toggle)
                            .child(
                                div()
                                    .px(px(14.))
                                    .py(px(8.))
                                    .rounded(px(999.))
                                    .bg(status_fill)
                                    .text_color(status_ink)
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(status_text),
                            ),
                    ),
            )
            .child(div().h(px(1.)).w_full().bg(border))
            // ── Body: deck on the left, bindings on the right ─────────
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(20.))
                    .child(left_column)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child(bindings_panel)
                            .child(background_section),
                    ),
            )
    }
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
    use super::advance_clicks;

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
}
