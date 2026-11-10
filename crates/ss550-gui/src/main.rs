//! SS550 StreamCraft Controller — a GPUI Kit desktop UI.
//!
//! Run with:
//!
//! ```text
//! cargo run --manifest-path crates/ss550-gui/Cargo.toml
//! ```
//!
//! The window shows the physical 3x5 deck. Every key is a coloured tile that
//! says its own number, the colour it is showing and the action bound to it,
//! so the screen always matches what the hardware is doing.
//!
//! Pressing a key on the deck lights its tile and runs the action bound to it,
//! on a background thread. Bindings are kept in
//! `%APPDATA%\ss550\bindings.json` (or `$SS550_CONFIG_DIR/bindings.json`).
#![deny(unsafe_code)]

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::time::{Duration, Instant};

use gpui_kit::component::{
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    form::Field,
    input::{Input, InputState, Textarea, TextareaState},
    switch::Switch,
    ActiveTheme as _, Theme, ThemeMode,
};
use gpui_kit::{
    div, px, rgb, size, AnyWindowHandle, AppContext as _, ClickEvent, Context, Entity, FontWeight,
    Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, WindowBounds, WindowOptions,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use ss550_driver::{ButtonImage, ButtonState, Device, LONG_CONNECT_TIMEOUT};
use ss550_engine::{Action, ActionRunner, ActionStore, KeyNumber};
use ss550_tray::{autostart, TrayCommand, TrayHost};

/// Hex colour as GPUI `Hsla`.
fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// Number of keys on the SS550 v3.
const KEY_COUNT: usize = 15;

/// Colour the user can paint with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Paint {
    White,
    Red,
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

/// Rows as they appear on the physical deck, top row first.
///
/// The driver numbers keys bottom-left origin (`0..14`), so the display order
/// is reversed vertically: the top row of the device is `10..=14`.
const DECK_ROWS: [(&str, [usize; 5]); 3] = [
    ("TOP ROW  ·  keys 11 to 15", [10, 11, 12, 13, 14]),
    ("MIDDLE ROW  ·  keys 6 to 10", [5, 6, 7, 8, 9]),
    ("BOTTOM ROW  ·  keys 1 to 5", [0, 1, 2, 3, 4]),
];

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
    /// The deck answered and was initialized.
    Connected { generation: u64, device: Device },
    /// The attempt failed; the error is shown to the user.
    Failed {
        generation: u64,
        error: ss550_driver::Ss550Error,
    },
}

/// Runs connection attempts on a dedicated thread.
///
/// [`Device::connect`] blocks for seconds while it waits for the deck's boot
/// re-enumeration to settle, so it must never run on the UI thread.
///
/// The attempt waits [`LONG_CONNECT_TIMEOUT`] on purpose: the deck's boot is
/// appear → leave → brief appearance → leave → settle, and the settle can be
/// ~11 s away. Giving up earlier would mean starting over in a different
/// phase of the boot, over and over.
fn connector_worker(commands: Receiver<u64>, results: Sender<Attempt>) {
    while let Ok(generation) = commands.recv() {
        let attempt = match Device::connect_with_timeout(LONG_CONNECT_TIMEOUT) {
            Ok(mut device) => match device.initialize() {
                Ok(()) => Attempt::Connected { generation, device },
                Err(err) => Attempt::Failed {
                    generation,
                    error: err,
                },
            },
            Err(err) => Attempt::Failed {
                generation,
                error: err,
            },
        };

        if results.send(attempt).is_err() {
            // The window is gone: nobody is waiting for this result.
            break;
        }
    }
}

struct Controller {
    device: Option<Device>,
    link: Link,
    note: SharedString,
    paint: Paint,
    slots: [Slot; KEY_COUNT],
    /// Keys held down right now, for the tile highlight.
    pressed: [bool; KEY_COUNT],
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
    store_path: PathBuf,
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
}

impl Controller {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (commands, command_rx) = mpsc::sync_channel::<u64>(1);
        let (result_tx, results) = mpsc::channel::<Attempt>();

        let worker_error = match std::thread::Builder::new()
            .name("ss550-connector".to_string())
            .spawn(move || connector_worker(command_rx, result_tx))
        {
            Ok(_) => None,
            Err(err) => Some(format!("Could not start the connection worker: {err}")),
        };

        // A broken bindings file must never keep the app from opening: the app
        // starts with no bindings and says why.
        let store_path = ss550_engine::bindings_path();
        let (store, store_error) = match ActionStore::load(&store_path) {
            Ok(store) => (store, None),
            Err(error) => (
                ActionStore::new(),
                Some(format!("Bindings not loaded: {error}")),
            ),
        };

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
        let (tray, tray_error) = match TrayHost::spawn("StreamCraft SS550 Controller") {
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

        let starts_with_windows = autostart::is_enabled().unwrap_or(false);

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
            device: None,
            link: Link::Connecting,
            note: match worker_error {
                Some(err) => err.into(),
                None => match store_error {
                    Some(err) => err.into(),
                    None => "Looking for the deck…".into(),
                },
            },
            paint: Paint::White,
            slots: [Slot::Off; KEY_COUNT],
            pressed: [false; KEY_COUNT],
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
                Attempt::Connected { generation, device } => {
                    if generation != self.generation {
                        // A newer request replaced this one; its result wins.
                        continue;
                    }
                    self.pending = false;
                    self.attempts = 0;
                    self.device = Some(device);
                    self.link = Link::Connected;

                    let slot = self.paint.slot();
                    self.apply_all(slot);
                    if self.device.is_some() {
                        self.note =
                            format!("Connected — all 15 keys painted {}.", slot_name(slot)).into();
                    }
                }
                Attempt::Failed { generation, error } => {
                    if generation != self.generation {
                        continue;
                    }
                    self.pending = false;
                    let message = error.to_string();

                    match error {
                        ss550_driver::Ss550Error::DeviceNotFound { .. } => {
                            // The deck is simply not there. This is not a
                            // failure: the app keeps looking for it.
                            self.link = Link::Waiting;
                            self.note = format!(
                                "{message} — the deck is not on the USB bus. \
                                 Retrying every {} s (attempt {}).",
                                RETRY_INTERVAL.as_secs(),
                                self.attempts
                            )
                            .into();
                        }
                        _ => {
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

    /// Asks for a new attempt when the deck is missing and the retry
    /// interval has elapsed.
    fn request_attempt_if_due(&mut self) {
        if self.device.is_some() || self.pending {
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

    fn disconnect(&mut self, err: ss550_driver::Ss550Error) {
        self.device = None;
        self.link = Link::Lost;
        self.note = format!("Connection lost: {err} — reconnecting automatically.").into();
    }

    /// Sends one key image to the deck.
    fn send(&mut self, idx: usize, slot: Slot) {
        if self.device.is_none() {
            self.note = "Not connected. Press RECONNECT first.".into();
            return;
        }

        let key = idx as u8;
        let image = match slot {
            Slot::White => ButtonImage::white(key),
            Slot::Red => ButtonImage::red(key),
            Slot::Off => return,
        };
        let image = match image {
            Ok(image) => image,
            Err(err) => {
                self.note = format!("Could not build the image: {err}").into();
                return;
            }
        };

        let result = match self.device.as_mut() {
            Some(device) => device.set_key_image(image),
            None => return,
        };
        match result {
            Ok(()) => self.slots[idx] = slot,
            Err(err) => self.disconnect(err),
        }
    }

    fn paint_key(&mut self, idx: usize) {
        let slot = self.paint.slot();
        self.send(idx, slot);
        self.note = format!("Key {} painted {}.", idx + 1, slot_name(slot)).into();
    }

    fn apply_all(&mut self, slot: Slot) {
        for idx in 0..KEY_COUNT {
            self.send(idx, slot);
            if self.device.is_none() {
                return;
            }
        }
        self.note = format!("All 15 keys painted {}.", slot_name(slot)).into();
    }

    fn clear_all(&mut self) {
        let result = match self.device.as_mut() {
            Some(device) => device.clear_all(),
            None => {
                self.note = "Not connected. Press RECONNECT first.".into();
                return;
            }
        };
        match result {
            Ok(()) => {
                self.slots = [Slot::Off; KEY_COUNT];
                self.note = "All keys cleared (blank).".into();
            }
            Err(err) => self.disconnect(err),
        }
    }

    fn heartbeat(&mut self) {
        let result = match self.device.as_mut() {
            Some(device) => device.keepalive(),
            None => return,
        };
        if let Err(err) = result {
            self.disconnect(err);
        }
    }

    /// Reads every key event waiting on the deck.
    ///
    /// Returns whether anything changed and the UI should repaint. The poll is
    /// non-blocking, so this runs on every tick without ever stalling the
    /// window.
    fn poll_deck(&mut self) -> bool {
        let Some(device) = self.device.as_mut() else {
            return false;
        };
        let events = match device.poll_events(Duration::ZERO) {
            Ok(events) => events,
            Err(err) => {
                self.disconnect(err);
                return true;
            }
        };
        if events.is_empty() {
            return false;
        }

        for event in events {
            let idx = event.key.to_index() as usize;
            match event.state {
                ButtonState::Pressed => {
                    self.pressed[idx] = true;
                    self.run_bound_action(idx);
                }
                ButtonState::Released => self.pressed[idx] = false,
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
                TrayCommand::Quit => cx.quit(),
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

    /// Turns the Windows sign-in entry on or off.
    fn set_autostart(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let outcome = if enabled {
            std::env::current_exe()
                .map_err(|error| error.to_string())
                .and_then(|path| {
                    autostart::enable(&autostart::command_line(&path, true))
                        .map_err(|error| error.to_string())
                })
        } else {
            autostart::disable().map_err(|error| error.to_string())
        };

        match outcome {
            Ok(()) => {
                self.starts_with_windows = enabled;
                self.note = if enabled {
                    "This controller now starts with Windows, hidden in the tray.".into()
                } else {
                    "This controller will no longer start with Windows.".into()
                };
            }
            Err(error) => {
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

        match self.store.set(self.selected_key, action) {
            Ok(()) => match self.store.save(&self.store_path) {
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
        match self.store.remove(self.selected_key) {
            Some(_) => match self.store.save(&self.store_path) {
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
        .text_size(px(15.))
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

        // What each key does, by tile index, for the labels on the deck.
        let binding_summaries: Vec<Option<SharedString>> = (0..KEY_COUNT)
            .map(|idx| {
                KeyNumber::new(idx as u8 + 1)
                    .ok()
                    .and_then(|number| self.store.get(number))
                    .map(|action| SharedString::from(action.summary()))
            })
            .collect();

        let mut rows = Vec::with_capacity(DECK_ROWS.len());
        for (row_label, keys) in DECK_ROWS {
            let tiles = keys
                .iter()
                .map(|&idx| {
                    key_tile(
                        idx,
                        self.slots[idx],
                        self.pressed[idx],
                        binding_summaries[idx].clone(),
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
                            .child(row_label),
                    )
                    .child(div().flex().flex_row().gap(px(10.)).children(tiles)),
            );
        }

        // ── Bindings panel ────────────────────────────────────────────
        let mut picker_rows = Vec::new();
        for (_, keys) in DECK_ROWS {
            let mut buttons = Vec::new();
            for &idx in keys.iter() {
                let Ok(number) = KeyNumber::new(idx as u8 + 1) else {
                    continue;
                };
                buttons.push(picker_button(
                    idx,
                    number,
                    number == self.selected_key,
                    self.store.get(number).is_some(),
                    cx,
                ));
            }
            picker_rows.push(div().flex().flex_row().gap(px(6.)).children(buttons));
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
                "Turn this on and Windows starts the controller when you sign in, \
                 hidden in the tray. It writes one entry under \
                 HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run, for this \
                 user only, and removes it again when you turn this off.",
                cx,
            ));

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
                format!("Bindings are saved to {}", self.store_path.display()),
                cx,
            ));

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
                    .child(
                        Button::new("paint-all")
                            .primary()
                            .label("PAINT ALL 15 KEYS")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                let slot = this.paint.slot();
                                this.apply_all(slot);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("clear-all")
                            .danger()
                            .label("CLEAR ALL (BLANK)")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.clear_all();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("keepalive")
                            .secondary()
                            .label("KEEP ALIVE · SEND ONE NOW")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.heartbeat();
                                this.note = "Heartbeat sent.".into();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("reconnect")
                            .secondary()
                            .label("RECONNECT")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.device = None;
                                this.link = Link::Connecting;
                                this.attempts = 0;
                                this.note = "Reconnecting…".into();
                                this.request_attempt();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("hide")
                            .secondary()
                            .label("HIDE · KEEPS RUNNING")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, _| {
                                this.hide_window();
                            })),
                    ),
            )
            .child(hint(
                "PAINT ALL uses the colour from step 1. CLEAR ALL blanks every key. \
                 KEEP ALIVE sends one heartbeat by hand. The app keeps the deck \
                 awake by itself and reconnects when the deck comes back. \
                 HIDE takes the window off the screen and out of the taskbar; the \
                 app keeps running in the notification area, keeps the deck awake \
                 and still runs your keys.",
                cx,
            ))
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
                                    .child("StreamCraft SS550"),
                            )
                            .child(hint(
                                "Redragon StreamCraft · 15 LCD keys · USB 0200:3000",
                                cx,
                            )),
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
    // Started by Windows: the controller lives in the tray with no window.
    let hidden = std::env::args().any(|argument| argument == autostart::HIDDEN_FLAG);

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
                window.set_window_title("StreamCraft SS550 Controller");
                Theme::change(ThemeMode::Dark, Some(window), cx);
                cx.new(|cx| Controller::new(window, cx))
            })
            .expect("failed to open the controller window");
        });
}
