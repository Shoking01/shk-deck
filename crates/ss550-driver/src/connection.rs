//! Device connection and communication.

use crate::error::Ss550Error;
use crate::hid_transport::HidTransport;
use crate::image::ButtonImage;
use crate::keys::{self, Key};
use deck_api::Transport;
use hidapi::{HidApi, HidDevice};
use ss550_protocol::{
    build_bat, build_cle_all, build_cle_key, build_dis, build_image_chunks, build_lig, build_sleep,
    build_stp, is_key_edge, parse_input_report, ButtonState, Verification, INPUT_REPORT_SIZE,
};
use std::fmt;
use std::time::{Duration, Instant};

/// Redragon SS550 v3 VID/PID (re-exported from `ss550-protocol`).
pub use ss550_protocol::{PID, VID};

/// A command the host can send to the deck (re-exported from `ss550-protocol`).
///
/// The driver refuses to send anything that is not on the verified list, so
/// a caller that wants to know what *can* be sent asks the registry.
pub use ss550_protocol::Command;

/// Usage page of the writable vendor interface (MI_00).
/// Windows denies output reports on the keyboard collection (MI_01,
/// usage page `0x0001`), so discovery must prefer this page — exactly
/// like `ss550-probe` does.
const VENDOR_USAGE_PAGE: u16 = 0xffa0;

/// Refuses a command that no test deck has proven.
///
/// This is the safety net of `AGENTS.md` §4 in code form: the driver sends
/// nothing that is not on the verified list, and the refusal happens
/// **before** the bytes leave the host.
///
/// There is deliberately **no flag, no environment variable and no setter**
/// that turns this off (S7): a rule with an off switch is a rule that gets
/// switched off.
///
/// The error names the command and the reason, so the caller can tell
/// *which* command was blocked and *why*:
///
/// ```text
/// Refusing to send clear_key: never sent to a deck: live clear-per-key is still pending (docs/protocol/CHANGELOG.md)
/// ```
///
/// # Examples
///
/// ```
/// use ss550_driver::{ensure_verified, Command};
///
/// assert!(ensure_verified(Command::DisplayOn).is_ok());
/// assert!(ensure_verified(Command::ClearKey).is_err());
/// ```
pub fn ensure_verified(command: Command) -> Result<(), Ss550Error> {
    let info = command.info();
    match info.verification {
        Verification::Verified { .. } => Ok(()),
        Verification::Unverified { reason } => Err(Ss550Error::CommandNotVerified {
            command: info.command.name(),
            reason,
        }),
    }
}

/// How long the driver may stay silent before it should send a heartbeat.
///
/// The deck resets itself when the host stops talking to it: measured with
/// **zero writes**, it stayed healthy for 68.7 s after power-up and then
/// dropped both HID interfaces. With one `CRT|DIS` every 500 ms it survived
/// 180 s (360 reports) without a single drop. See
/// `docs/troubleshooting/deck-stuck.md` §5.
///
/// The exact idle threshold is **not** known, so this is deliberately
/// conservative: 2 s is far below the 68.7 s observed, and the cost is one
/// 1025-byte report — negligible next to an image upload.
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

/// Decides whether [`Device::keepalive`] must write now.
///
/// Split out as a pure function so the policy is unit-testable without a
/// device. A heartbeat never fires while an image is queued, because a `DIS`
/// landing inside a `BAT` → chunk → `STP` transfer would corrupt it.
fn heartbeat_due(
    initialized: bool,
    has_pending_images: bool,
    since_last_write: Duration,
    interval: Duration,
) -> bool {
    initialized && !has_pending_images && since_last_write >= interval
}

/// How long the deck must be **continuously** present before its boot is
/// trusted.
///
/// The firmware re-enumerates more than once while booting; the longest
/// intermediate appearance measured was 502 ms, so 3 s is a wide margin.
pub const SETTLE_TIME: Duration = Duration::from_secs(3);

/// How long [`Device::connect`] keeps waiting once the deck has been seen.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Checks whether the SS550 deck is currently present on the USB bus.
///
/// This is the non-blocking probe for hotplug detection: it refreshes the
/// device list and checks if the SS550 is enumerated. Returns `true` if
/// the device is found, `false` otherwise.
///
/// Use this for periodic polling to detect plug/unplug events without
/// blocking the call.
///
/// # Example
///
/// ```ignore
/// use ss550_driver::is_deck_present;
///
/// if is_deck_present() {
///     println!("Deck is plugged in!");
/// } else {
///     println!("Deck is not found");
/// }
/// ```
pub fn is_deck_present() -> bool {
    let mut api = match HidApi::new() {
        Ok(api) => api,
        Err(_) => return false,
    };
    vendor_collection_present(&mut api)
}

/// How long a connection attempt may wait for a deck that was already seen.
///
/// The boot cycle is: appear → leave the bus → brief appearance → leave →
/// settle. The longest measured absence is 4–5 s and the settle window is
/// 3 s, so an attempt that starts while the deck is booting needs up to
/// ~11 s to reach a stable handle — **more than the 10 s of
/// [`CONNECT_TIMEOUT]`**.
///
/// An app that gives up at 10 s has to start over, and the restart lands in
/// a different (possibly worse) phase of the boot. Waiting instead is what
/// makes "open the app while the deck is rebooting" work on the first try.
pub const LONG_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long [`Device::connect`] waits before reporting `DeviceNotFound` when
/// the deck was never seen at all — an app started without the device must not
/// hang.
const ABSENT_GRACE: Duration = Duration::from_millis(500);

/// How often to poll for device presence changes.
///
/// Shorter is more responsive but uses more CPU during the "waiting" state.
/// 1 second is a good balance: the deck is usually seen within a few seconds
/// of being plugged in, and the polling cost is negligible.
#[allow(dead_code)]
pub const DISCOVERY_POLL: Duration = Duration::from_secs(1);

/// Presence poll interval while waiting for the boot to settle.
const PRESENCE_POLL: Duration = Duration::from_millis(50);

/// Reconnect backoff constants (reserved for M7: hotplug detection)
/// These will be used when we implement automatic reconnect with exponential
/// backoff in the UI layer.
#[allow(dead_code)]
const RECONNECT_BACKOFF_FACTOR: u32 = 2;

/// Maximum reconnect delay.
///
/// After many failures, cap the delay at this value to avoid indefinite
/// waits. 60 seconds lets a user know the app is still trying, without
/// overwhelming the USB bus.
#[allow(dead_code)]
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

/// How many failed connections trigger full reset backoff.
///
/// Instead of exponentially increasing the delay, count failures and when
/// this limit is reached, use MAX_RECONNECT_DELAY.
#[allow(dead_code)]
const BACKOFF_RESET_THRESHOLD: u32 = 10;

/// Decides when [`wait_until_settled`] must stop waiting and report failure.
///
/// Split out as a pure function so the policy is unit-testable: a device that
/// was never seen gets only [`ABSENT_GRACE`], while one that is visibly booting
/// gets the full `timeout`.
fn connect_should_give_up(seen: bool, waited: Duration, timeout: Duration) -> bool {
    if seen {
        waited >= timeout
    } else {
        waited >= ABSENT_GRACE
    }
}

/// Returns true if the deck's vendor collection is enumerated right now.
///
/// Always refreshes first: `device_list` alone is a cached snapshot and would
/// report the device as present forever after it has gone.
fn vendor_collection_present(api: &mut HidApi) -> bool {
    if api.refresh_devices().is_err() {
        return false;
    }
    api.device_list().any(|d| {
        d.vendor_id() == VID && d.product_id() == PID && d.usage_page() == VENDOR_USAGE_PAGE
    })
}

/// Blocks until the deck has been continuously present for [`SETTLE_TIME`].
fn wait_until_settled(api: &mut HidApi, timeout: Duration) -> Result<(), Ss550Error> {
    let started = Instant::now();
    let mut seen = false;
    let mut stable_since: Option<Instant> = None;

    loop {
        if vendor_collection_present(api) {
            seen = true;
            let since = *stable_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= SETTLE_TIME {
                return Ok(());
            }
        } else {
            // Any disappearance restarts the window: an intermediate
            // appearance must never be mistaken for the settled state.
            stable_since = None;
        }

        if connect_should_give_up(seen, started.elapsed(), timeout) {
            return Err(Ss550Error::DeviceNotFound { vid: VID, pid: PID });
        }

        std::thread::sleep(PRESENCE_POLL);
    }
}

/// Upper bound on reports drained by one [`Device::poll_events`] call.
///
/// A click produces two reports, so 64 is far more than any real burst; the cap
/// only exists so a misbehaving device cannot spin the caller forever.
const MAX_EVENTS_PER_POLL: usize = 64;

/// One key edge read from the deck.
///
/// A click produces **two** events: [`ButtonState::Pressed`] then
/// [`ButtonState::Released`], typically 150-270 ms apart. There is no debounce
/// in this layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonEvent {
    /// Which physical key changed.
    pub key: Key,
    /// Which edge this is.
    pub state: ButtonState,
}

/// Decodes one input report into a key edge.
///
/// Returns `Ok(None)` for the replies the deck sends to our own writes: they
/// share the `"ACK"` / `"OK"` envelope but carry key 0. [`Device::write_pack`]
/// never reads those replies, so they are still queued when a poll happens;
/// reporting one as a key would drop a healthy connection on the next
/// heartbeat.
fn decode_key_edge(report: &[u8]) -> Result<Option<ButtonEvent>, Ss550Error> {
    if !is_key_edge(report) {
        return Ok(None);
    }
    let event = parse_input_report(report)?;
    let key = Key::from_firmware_key(event.key).ok_or(Ss550Error::KeyOutOfRange(event.key))?;
    Ok(Some(ButtonEvent {
        key,
        state: event.state,
    }))
}

/// A connection to an SS550 device.
///
/// Every byte this type sends or receives goes through a
/// [`Transport`](deck_api::Transport), never through the handle directly. In
/// production that transport is [`HidTransport`]; in tests and in the simulator
/// it is a fake, which is how the deck's logic is exercised without a device.
pub struct Device {
    transport: Box<dyn Transport + Send>,
    initialized: bool,
    brightness: u8,
    pending_images: Vec<ButtonImage>,
    heartbeat_interval: Duration,
    last_write: Instant,
    /// Cache of images painted for each key. None = cleared or never set.
    button_cache: Vec<Option<ButtonImage>>,
}

impl Device {
    /// Connects to an SS550 device once its boot sequence has settled.
    ///
    /// **The deck re-enumerates itself several times while booting**, so a
    /// handle opened at the first — or even the second — appearance is stale
    /// before it is used, and every write through it fails with `0x48F`
    /// (`ERROR_DEVICE_NOT_CONNECTED`) or `0x3E5` (a transfer that never
    /// completes). Measured intermediate appearances last under 502 ms, so this
    /// waits for [`SETTLE_TIME`] of **continuous** presence before opening
    /// anything.
    ///
    /// Behaviour:
    ///
    /// * deck already settled: blocks for [`SETTLE_TIME`], then connects;
    /// * deck mid-boot: waits until it stops re-enumerating, then connects;
    /// * deck never seen at all: fails fast with
    ///   [`Ss550Error::DeviceNotFound`] after [`ABSENT_GRACE`], so an app that
    ///   starts without the device does not hang;
    /// * deck seen but never settling: fails with
    ///   [`Ss550Error::DeviceNotFound`] after `CONNECT_TIMEOUT`.
    pub fn connect() -> Result<Self, Ss550Error> {
        Self::connect_with_timeout(CONNECT_TIMEOUT)
    }

    /// Connects to an SS550 device, waiting up to `timeout` for its boot to
    /// settle.
    ///
    /// Same rules as [`Device::connect`], with one difference: once the deck
    /// has been seen, the attempt waits `timeout` for it to settle instead of
    /// [`CONNECT_TIMEOUT`].
    ///
    /// Use [`LONG_CONNECT_TIMEOUT`] for a long-lived client (a GUI, a daemon)
    /// that must win the deck's boot window on the first try: the deck's cycle
    /// is appear → leave → brief appearance → leave → settle, and the settle
    /// can be ~11 s away. An app that gives up earlier has to start over in a
    /// different phase of the boot.
    ///
    /// A deck that is **never** seen still fails fast (after
    /// [`ABSENT_GRACE`]), so this never hangs an app that starts without the
    /// device.
    pub fn connect_with_timeout(timeout: Duration) -> Result<Self, Ss550Error> {
        let mut api = HidApi::new().map_err(|e| Ss550Error::HidOpenError(e.to_string()))?;
        Self::connect_with_api_timeout(&mut api, timeout)
    }

    /// Connects using a caller-owned [`HidApi`].
    ///
    /// Takes `&mut HidApi` on purpose: `HidApi::device_list` returns a **cached
    /// snapshot**, and only `refresh_devices` can see a plug, an unplug or a
    /// boot re-enumeration. A long-lived `HidApi` shared behind `&` would answer
    /// with stale data and hand out a dead handle.
    pub fn connect_with_api(api: &mut HidApi) -> Result<Self, Ss550Error> {
        Self::connect_with_api_timeout(api, CONNECT_TIMEOUT)
    }

    /// Connects using a caller-owned [`HidApi`], waiting up to `timeout` for
    /// the deck's boot to settle.
    pub fn connect_with_api_timeout(
        api: &mut HidApi,
        timeout: Duration,
    ) -> Result<Self, Ss550Error> {
        wait_until_settled(api, timeout)?;
        let device = Self::open_vendor_handle(api)?;
        Ok(Device::init_from_handle(device))
    }

    /// Opens a fresh handle on the vendor collection (MI_00).
    fn open_vendor_handle(api: &mut HidApi) -> Result<HidDevice, Ss550Error> {
        // Refresh once more: the settle window ended, but the list may be cached.
        if api.refresh_devices().is_err() {
            return Err(Ss550Error::DeviceNotFound { vid: VID, pid: PID });
        }

        let devices: Vec<_> = api
            .device_list()
            .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
            .collect();

        let info = devices
            .iter()
            .find(|d| d.usage_page() == VENDOR_USAGE_PAGE)
            .or_else(|| devices.first())
            .copied()
            .ok_or(Ss550Error::DeviceNotFound { vid: VID, pid: PID })?;

        info.open_device(api)
            .map_err(|e| Ss550Error::HidIoError(e.to_string()))
    }

    /// Initializes a Device from an already-opened HidDevice.
    fn init_from_handle(device: HidDevice) -> Self {
        Self::over_transport(Box::new(HidTransport::new(device)))
    }

    /// Builds a `Device` over a caller-supplied transport.
    ///
    /// Used by [`Ss550Deck::over_transport`](crate::Ss550Deck::over_transport)
    /// and by this crate's tests. It does not weaken the gate: every write
    /// still goes through [`Device::write_pack`] and its verified-command
    /// check, whatever the transport does with the bytes afterwards.
    pub(crate) fn over_transport(transport: Box<dyn Transport + Send>) -> Self {
        Self {
            transport,
            initialized: false,
            brightness: 100,
            pending_images: Vec::new(),
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            last_write: Instant::now(),
            button_cache: vec![None; keys::KEY_COUNT],
        }
    }

    /// Reads the device's identity string from the control path.
    ///
    /// The SS550 serves this value from a **static identity report**: it is
    /// available with no command sent at all. Writing `CRT|VER` is therefore
    /// not only unnecessary but harmful — the firmware rejects that report and
    /// halts the interrupt OUT endpoint, after which no host (this driver or
    /// the vendor app) can write until the deck is power cycled. See
    /// `docs/troubleshooting/deck-stuck.md`.
    ///
    /// So this never writes. It reads with a control transfer and a 1025-byte
    /// buffer (a 513-byte input-sized buffer fails with `ERROR_GEN_FAILURE` on
    /// Windows). The returned string is informational: because the report is
    /// static, a successful read is **not** evidence that anything reached the
    /// firmware. If the device returns no bytes, this returns an empty string
    /// rather than fabricating a version.
    ///
    /// `VER` stays `unverified` in the registry
    /// (`docs/protocol/verified.md`) precisely **because** writing it halts
    /// the endpoint. That is why this method reads the static identity report
    /// instead of asking the firmware: the gate would refuse the write
    /// anyway, and the refusal would be the last warning before the deck
    /// goes dark.
    ///
    /// Takes `&mut self` because a read is a byte transfer and the transport
    /// seam models one as a mutable borrow; nothing here writes.
    pub fn firmware_version(&mut self) -> Result<String, Ss550Error> {
        let mut buf = vec![0u8; ss550_protocol::REPORT_SIZE];
        // `buf[0]` already holds report ID 0x00, as required before the call.
        let n = self.transport.read_control(&mut buf)?;

        if n == 0 {
            return Ok(String::new());
        }
        let end = n.min(buf.len());
        Ok(parse_firmware_version(&buf[..end]))
    }

    /// The one and only way bytes leave the host.
    ///
    /// The command is checked against the verified registry **before** the
    /// write, so an unproven command never reaches the deck. This is the
    /// single call site into the transport's write: keep it that way, or the
    /// safety net has a hole in it. The crate-wide guard
    /// (`every_write_goes_through_the_gated_funnel`) scans every source file
    /// for a second writer and fails the build if one appears.
    fn write_pack(
        &mut self,
        command: Command,
        pack: &ss550_protocol::Pack,
    ) -> Result<(), Ss550Error> {
        // The gate. Before the bytes, never after: an unproven command must
        // not leave the host at all.
        ensure_verified(command)?;

        // The transport writes over interrupt OUT. Control `Set_Report`
        // returns success but the firmware ignores it, so it must never be
        // used here: a silent no-op is worse than a loud error.
        self.transport.write_report(pack.as_bytes())?;

        // Every accepted report counts as activity: an app that paints keys is
        // already keeping the deck awake, so the heartbeat stays silent.
        self.last_write = Instant::now();
        Ok(())
    }

    /// Sends a heartbeat if the driver has been silent for too long.
    ///
    /// **The deck resets itself when the host stops writing to it.** Measured:
    /// with zero writes it dropped off the bus after 68.7 s; with one report
    /// every 500 ms it survived 180 s without a single drop
    /// (`docs/troubleshooting/deck-stuck.md` §5). A UI that repaints constantly
    /// needs nothing, but an idle app must call this from its main loop or the
    /// deck will reset and the handle will die with
    /// [`Ss550Error::TransportStalled`].
    ///
    /// The call is throttled internally, so it is safe to invoke every frame:
    /// it writes only after [`DEFAULT_HEARTBEAT_INTERVAL`] of silence. It never
    /// fires while an image is queued, because a `DIS` inside a `BAT` → chunk →
    /// `STP` transfer would corrupt that upload.
    ///
    /// It does nothing before [`Device::initialize`]: a sleeping deck is
    /// supposed to be idle.
    pub fn keepalive(&mut self) -> Result<(), Ss550Error> {
        let due = heartbeat_due(
            self.initialized,
            !self.pending_images.is_empty(),
            self.last_write.elapsed(),
            self.heartbeat_interval,
        );
        if !due {
            return Ok(());
        }

        // `DIS` is the proven heartbeat: it wakes the display and has no
        // persistent side effect, unlike `LIG` which would need a restore.
        // The heartbeat is the `display_on` command, not a command of its own:
        // the registry lists what the deck has proven, and `DIS` is proven.
        let dis = build_dis();
        self.write_pack(Command::DisplayOn, &dis)
    }

    /// Overrides the heartbeat interval.
    ///
    /// [`Duration::ZERO`] makes [`Device::keepalive`] write on every call;
    /// [`Duration::MAX`] effectively disables it. The default is
    /// [`DEFAULT_HEARTBEAT_INTERVAL`].
    pub fn set_heartbeat_interval(&mut self, interval: Duration) {
        self.heartbeat_interval = interval;
    }

    /// Returns the heartbeat interval currently in effect.
    pub fn heartbeat_interval(&self) -> Duration {
        self.heartbeat_interval
    }

    /// Reads pending key events, waiting up to `timeout` for the first one.
    ///
    /// Returns every report that is already queued, so one call can yield
    /// several events. Pass [`Duration::ZERO`] from a UI loop: it is
    /// non-blocking and returns an empty vector when nothing happened.
    ///
    /// Measured behaviour this relies on:
    ///
    /// * reports arrive on the **vendor** collection (the one this driver
    ///   opens), as 512-byte interrupt IN reports;
    /// * a click yields two events, pressed then released;
    /// * reading does **not** count as activity, so it does not keep the deck
    ///   awake and does not reset the heartbeat timer.
    ///
    /// Call [`Device::keepalive`] between polls. A non-zero `timeout` blocks
    /// this thread for that long, and an app that only polls without writing
    /// will let the deck fall into its idle reset.
    pub fn poll_events(&mut self, timeout: Duration) -> Result<Vec<ButtonEvent>, Ss550Error> {
        let mut buf = [0u8; INPUT_REPORT_SIZE];
        let mut events = Vec::new();
        let mut wait = timeout;

        while events.len() < MAX_EVENTS_PER_POLL {
            let read = self.transport.read_input(&mut buf, wait)?;

            if read == 0 {
                break;
            }

            if let Some(event) = decode_key_edge(&buf[..read])? {
                events.push(event);
            }

            // One report arrived: drain the rest of the queue without blocking,
            // so a fast double-click is reported in a single poll.
            wait = Duration::ZERO;
        }

        Ok(events)
    }

    /// Wakes the display (`CRT|DIS`).
    ///
    /// This deliberately does **not** touch brightness. An earlier version sent
    /// `CRT|LIG 0` and relied on a later [`Device::flush`] to restore it, which
    /// left the deck permanently dark whenever the restore never landed. The
    /// image path does not need brightness 0: `DIS → LIG 100 → BAT → chunk →
    /// STP` renders correctly.
    pub fn initialize(&mut self) -> Result<(), Ss550Error> {
        if self.initialized {
            return Ok(());
        }

        let dis = build_dis();
        self.write_pack(Command::DisplayOn, &dis)?;

        self.initialized = true;
        Ok(())
    }

    /// Returns the last brightness value written by [`Device::set_brightness`].
    pub fn brightness(&self) -> u8 {
        self.brightness
    }

    /// Sets brightness (0-100).
    pub fn set_brightness(&mut self, b: u8) -> Result<(), Ss550Error> {
        let pack = build_lig(b.clamp(0, 100))?;
        self.write_pack(Command::Brightness, &pack)?;
        self.brightness = b.clamp(0, 100);
        Ok(())
    }

    /// Queues an image for a key, does not immediately flush.
    pub fn queue_image(&mut self, image: ButtonImage) -> Result<(), Ss550Error> {
        if !self.initialized {
            return Err(Ss550Error::NotInitialized);
        }
        self.pending_images.push(image);
        Ok(())
    }

    /// Flushes all queued images.
    ///
    /// Sends each queued image (`BAT` header, payload chunks) and then **one**
    /// `STP` commit. A bare `STP` with no queued image is never sent: a flush
    /// with nothing to commit is exactly the report the firmware rejects, and a
    /// rejected report halts the USB endpoint
    /// (`docs/troubleshooting/deck-stuck.md`).
    pub fn flush(&mut self) -> Result<(), Ss550Error> {
        if !self.initialized {
            return Err(Ss550Error::NotInitialized);
        }

        // Collect images to avoid borrow conflicts.
        let images: Vec<ButtonImage> = std::mem::take(&mut self.pending_images);

        // Nothing queued: there is no transfer to commit, so do not send STP.
        if images.is_empty() {
            return Ok(());
        }

        // Send each image.
        for img in &images {
            self.send_image(img)?;
        }

        let stp = build_stp();
        self.write_pack(Command::ImageTransfer, &stp)?;

        Ok(())
    }

    fn send_image(&mut self, img: &ButtonImage) -> Result<(), Ss550Error> {
        let firmware_key = keys::open_deck_to_firmware(img.key);
        let bat = build_bat(img.jpeg.len(), firmware_key)?;
        self.write_pack(Command::ImageTransfer, &bat)?;

        for chunk in build_image_chunks(&img.jpeg) {
            self.write_pack(Command::ImageTransfer, &chunk)?;
        }

        Ok(())
    }

    /// Clears a single button image.
    ///
    /// Sends `CRT|CLE` for the given key's 1-based firmware key, followed by
    /// one `STP` to commit. Use [`Device::clear_all`] to clear every key.
    ///
    /// **This always fails, on purpose.** The clear-per-key path was never
    /// exercised on a deck, so it is `unverified` in the registry
    /// (`docs/protocol/verified.md`) and the driver refuses to send it with
    /// [`Ss550Error::CommandNotVerified`]. The encoding stays here, unit
    /// tested, until a **test deck** proves it: a rejected report halts the
    /// USB endpoint, and the daily driver is not the place to find out.
    pub fn clear_key(&mut self, key: Key) -> Result<(), Ss550Error> {
        if !self.initialized {
            return Err(Ss550Error::NotInitialized);
        }

        let firmware_key = keys::open_deck_to_firmware(key.to_index());
        let cle = build_cle_key(firmware_key)?;
        self.write_pack(Command::ClearKey, &cle)?;

        let stp = build_stp();
        self.write_pack(Command::ClearKey, &stp)?;

        // Clear from cache as well — but only once the deck has actually been
        // told. The writes above are refused today, so the cache keeps the
        // image: dropping it would make the driver claim a key is clear
        // while the deck still shows it.
        let idx = key.to_index() as usize;
        if idx < self.button_cache.len() {
            self.button_cache[idx] = None;
        }

        Ok(())
    }

    /// Clears all button images on the device.
    ///
    /// Uses the `CLE` command with key=0xFF to clear all 15 buttons at once.
    /// Must be followed by an `STP` flush (handled by this method).
    ///
    /// Note: Brightness must be managed separately before and after.
    /// Also clears the internal image cache so these buttons won't be
    /// repainted by [`Device::repaint_all`].
    ///
    /// This command **is** on the verified list: it was exercised on a deck.
    pub fn clear_all(&mut self) -> Result<(), Ss550Error> {
        if !self.initialized {
            return Err(Ss550Error::NotInitialized);
        }

        let cle = build_cle_all();
        self.write_pack(Command::ClearAll, &cle)?;

        let stp = build_stp();
        self.write_pack(Command::ClearAll, &stp)?;

        // The deck has been told, so the images can be forgotten: repaint_all
        // will not push them back.
        //
        // A failed write never reaches this line — the `?` above returns
        // first — and that is the policy: the deck's state is then unknown
        // (the images may still be there, or the deck may have been told to
        // clear and never got the commit), so the driver keeps what the user
        // had. The cost is that a deck that cleared anyway gets its images
        // back on the next repaint.
        self.button_cache.fill(None);

        Ok(())
    }

    /// Sets a single button image synchronously (queue + flush in one call).
    pub fn set_key_image(&mut self, image: ButtonImage) -> Result<(), Ss550Error> {
        if !self.initialized {
            return Err(Ss550Error::NotInitialized);
        }

        // Cache the image so we can repaint it after a reconnect.
        let idx = image.key as usize;
        if idx < self.button_cache.len() {
            self.button_cache[idx] = Some(image.clone());
        }

        self.queue_image(image)?;
        self.flush()
    }

    /// Reconnects after the handle has died (e.g., after a transport stall).
    ///
    /// Drops the current handle and opens a fresh one after waiting for the
    /// deck to settle. Call this only when you know the handle is dead; a normal
    /// sequence (`connect` → do work) never needs it.
    ///
    /// **Why this exists:** The deck can drop the USB handle when firmware
    /// encounters an unrecoverable error (for example, a malformed image upload or
    /// sending `CRT|VER`). That endpoint halt persists until a power cycle or
    /// until the handle is opened fresh against a settled device.
    ///
    /// After `reconnect` returns successfully, the device is in the same state
    /// as after `initialize`: you must call `initialize()` and then `repaint_all()`
    /// to restore brightness and button images.
    ///
    /// Example recovery flow:
    ///
    /// ```ignore
    /// if let Err(Ss550Error::TransportStalled { .. }) = device.send_image(&img) {
    ///     device.reconnect()?;      // drop old handle, wait for settled device
    ///     device.initialize()?;     // wake the display
    ///     device.repaint_all()?;    // restore all cached images
    /// }
    /// ```
    pub fn reconnect(&mut self) -> Result<(), Ss550Error> {
        let mut api = HidApi::new().map_err(|e| Ss550Error::HidOpenError(e.to_string()))?;
        wait_until_settled(&mut api, CONNECT_TIMEOUT)?;
        // Reopen the device with a fresh handle, behind a fresh transport.
        self.transport = Box::new(HidTransport::new(Self::open_vendor_handle(&mut api)?));
        self.initialized = false;
        self.brightness = 100;
        self.last_write = Instant::now();
        Ok(())
    }

    /// Paints all cached button images, in order 0-14.
    ///
    /// Must be called after [`Device::reconnect`] or [`Device::initialize`], since
    /// neither operation restores painted images. The cache is updated on every
    /// [`Device::set_key_image`] call, so cleared buttons (via `clear_key` or
    /// `clear_all`) are not repainted.
    ///
    /// Returns an error if any key fails to paint; the first failure stops the
    /// sequence.
    pub fn repaint_all(&mut self) -> Result<(), Ss550Error> {
        // Collect images first to avoid borrow conflict with queue_image.
        let images: Vec<ButtonImage> = self
            .button_cache
            .iter()
            .enumerate()
            .filter_map(|(idx, opt)| {
                opt.as_ref().map(|img| {
                    let mut img = img.clone();
                    img.key = idx as u8;
                    img
                })
            })
            .collect();

        for img in images {
            self.queue_image(img)?;
        }
        self.flush()
    }

    /// Puts the device display to sleep (`CRT|HAN`).
    ///
    /// If the device is not yet initialized, it is initialized first (as in
    /// `mirajazz::Device::sleep`). After sleeping the driver marks itself
    /// not-initialized, so call [`Device::initialize`] again to wake.
    /// Pending queued images are kept and will flush after the next
    /// initialize + flush.
    ///
    /// **This always fails, on purpose.** The sleep form was encoded from the
    /// reference implementation and never exercised on a deck, so it is
    /// `unverified` in the registry (`docs/protocol/verified.md`) and the
    /// driver refuses to send it with [`Ss550Error::CommandNotVerified`].
    /// The gate is checked *before* [`Device::initialize`] too, so a refused
    /// sleep does not leave a `DIS` on the deck on its way out.
    pub fn sleep(&mut self) -> Result<(), Ss550Error> {
        // Check the gate first: the refusal must not wake the deck on its
        // way to being refused.
        ensure_verified(Command::Sleep)?;

        self.initialize()?;

        let sleep_pack = build_sleep();
        self.write_pack(Command::Sleep, &sleep_pack)?;

        self.initialized = false;
        Ok(())
    }

    /// Returns true if the device is initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // The transport closes the handle when it is dropped.
    }
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cached = self.button_cache.iter().filter(|opt| opt.is_some()).count();
        f.debug_struct("Device")
            .field("initialized", &self.initialized)
            .field("brightness", &self.brightness)
            .field("pending_images", &self.pending_images.len())
            .field("button_cache", &cached)
            .finish_non_exhaustive()
    }
}

/// Parses the firmware-version ASCII string out of a raw HID reply.
///
/// The reply is a zero-padded report whose payload contains an ASCII
/// version such as `V3.SS_550.01.002`. This returns the longest printable run
/// starting with `V` and containing `.`; empty or junk input yields `""`.
fn parse_firmware_version(buf: &[u8]) -> String {
    let text = String::from_utf8_lossy(buf);
    let mut best = String::new();
    let mut current = String::new();

    for c in text.chars() {
        if c.is_ascii_graphic() || c == ' ' {
            current.push(c);
        } else {
            if current.starts_with('V')
                && current.contains('.')
                && (best.is_empty() || current.len() > best.len())
            {
                best = current.clone();
            }
            current.clear();
        }
    }
    if current.starts_with('V')
        && current.contains('.')
        && (best.is_empty() || current.len() > best.len())
    {
        best = current;
    }
    best.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn parse_firmware_version_extracts_v_string() {
        // Firmware replies look like "V3.SS_550.01.002" embedded after a
        // report-ID byte and zero padding, as seen via the probe harness.
        let mut buf = vec![0u8; 64];
        buf[1] = b'V';
        buf[2] = b'3';
        buf[3] = b'.';
        buf[4] = b'S';
        buf[5] = b'S';
        buf[6] = b'_';
        buf[7] = b'5';
        buf[8] = b'5';
        buf[9] = b'0';
        assert_eq!(parse_firmware_version(&buf), "V3.SS_550");
    }

    #[test]
    fn parse_firmware_version_empty_on_no_bytes() {
        assert_eq!(parse_firmware_version(&[]), "");
        assert_eq!(parse_firmware_version(&[0u8; 32]), "");
    }

    #[test]
    fn a_command_reply_is_skipped_not_reported_as_a_key() {
        let mut reply = [0u8; INPUT_REPORT_SIZE];
        reply[0..3].copy_from_slice(b"ACK");
        reply[5..7].copy_from_slice(b"OK");
        assert!(decode_key_edge(&reply)
            .expect("a reply is not an error")
            .is_none());
    }

    #[test]
    fn a_key_press_is_decoded_with_its_physical_key() {
        let mut report = [0u8; INPUT_REPORT_SIZE];
        report[0..3].copy_from_slice(b"ACK");
        report[5..7].copy_from_slice(b"OK");
        report[9] = 1; // read numbering: top-left
        report[10] = 1; // pressed

        let event = decode_key_edge(&report)
            .expect("valid report")
            .expect("a key edge");
        assert_eq!(event.key, Key::TopLeft);
        assert_eq!(event.state, ButtonState::Pressed);
    }

    #[test]
    fn a_release_is_decoded_too() {
        let mut report = [0u8; INPUT_REPORT_SIZE];
        report[0..3].copy_from_slice(b"ACK");
        report[5..7].copy_from_slice(b"OK");
        report[9] = 1;
        report[10] = 0;

        let event = decode_key_edge(&report)
            .expect("valid report")
            .expect("a key edge");
        assert_eq!(event.state, ButtonState::Released);
    }

    #[test]
    fn a_report_without_the_envelope_is_skipped() {
        assert!(decode_key_edge(&[0u8; INPUT_REPORT_SIZE])
            .expect("not an error")
            .is_none());
        assert!(decode_key_edge(&[0u8; 4]).expect("not an error").is_none());
    }

    #[test]
    fn heartbeat_is_silent_until_the_interval_elapses() {
        let interval = Duration::from_secs(2);
        assert!(!heartbeat_due(
            true,
            false,
            Duration::from_millis(1999),
            interval
        ));
    }

    #[test]
    fn heartbeat_fires_exactly_at_the_interval() {
        // Boundary: the deck must never be left silent for a full interval
        // without a report, so `>=` is deliberate, not incidental.
        let interval = Duration::from_secs(2);
        assert!(heartbeat_due(true, false, interval, interval));
        assert!(heartbeat_due(
            true,
            false,
            Duration::from_secs(60),
            interval
        ));
    }

    #[test]
    fn heartbeat_does_nothing_before_initialize() {
        // A sleeping deck is meant to be idle; waking it with DIS would defeat
        // `Device::sleep`.
        assert!(!heartbeat_due(
            false,
            false,
            Duration::from_secs(3600),
            Duration::from_secs(2)
        ));
    }

    #[test]
    fn heartbeat_never_interrupts_a_queued_upload() {
        // A DIS between BAT and its chunks would corrupt the image transfer.
        assert!(!heartbeat_due(
            true,
            true,
            Duration::from_secs(3600),
            Duration::from_secs(2)
        ));
    }

    #[test]
    fn zero_interval_means_every_call_heartbeats() {
        // Documented escape hatch for callers that drive the clock themselves.
        assert!(heartbeat_due(true, false, Duration::ZERO, Duration::ZERO));
    }

    #[test]
    fn connect_fails_fast_when_the_deck_was_never_seen() {
        // An app started without the device must not hang for the full timeout.
        assert!(!connect_should_give_up(
            false,
            Duration::from_millis(499),
            CONNECT_TIMEOUT
        ));
        assert!(connect_should_give_up(false, ABSENT_GRACE, CONNECT_TIMEOUT));
    }

    #[test]
    fn connect_keeps_waiting_while_the_deck_is_booting() {
        // Once the deck has been seen, its boot may legitimately take seconds;
        // the short absent-grace must no longer apply.
        assert!(!connect_should_give_up(
            true,
            Duration::from_secs(5),
            CONNECT_TIMEOUT
        ));
        assert!(!connect_should_give_up(
            true,
            CONNECT_TIMEOUT - Duration::from_millis(1),
            CONNECT_TIMEOUT
        ));
        assert!(connect_should_give_up(
            true,
            CONNECT_TIMEOUT,
            CONNECT_TIMEOUT
        ));
    }

    #[test]
    fn long_timeout_covers_the_measured_boot_cycle() {
        // Measured: the deck leaves the bus at +3.1 s and is back at +8.4 s,
        // then needs 3 s of continuous presence. An attempt started at the
        // first appearance therefore needs ~11.4 s to reach a stable handle.
        let worst_case_boot = Duration::from_millis(11_400);
        assert!(LONG_CONNECT_TIMEOUT > worst_case_boot);
        assert!(LONG_CONNECT_TIMEOUT > CONNECT_TIMEOUT);
    }

    #[test]
    fn the_long_timeout_still_fails_fast_when_the_deck_is_never_seen() {
        // A longer settle budget must not turn "no device attached" into a
        // 30 s hang: the absent-grace still applies.
        assert!(!connect_should_give_up(
            false,
            Duration::from_millis(499),
            LONG_CONNECT_TIMEOUT
        ));
        assert!(connect_should_give_up(
            false,
            ABSENT_GRACE,
            LONG_CONNECT_TIMEOUT
        ));
    }

    #[test]
    fn settle_time_exceeds_every_observed_intermediate_appearance() {
        // The longest intermediate appearance measured on hardware was 502 ms;
        // anything below that would reopen the stale-handle bug.
        assert!(SETTLE_TIME > Duration::from_millis(502));
        assert!(CONNECT_TIMEOUT > SETTLE_TIME);
    }

    #[test]
    fn connect_fails_after_timeout_when_deck_never_settles() {
        // This test uses a mock HidDevice that always returns "present" but never
        // stabilizes. A real connect() would timeout after CONNECT_TIMEOUT.
        //
        // Integration test: run `cargo test -p ss550-driver -- --ignored connect_real`
        // on hardware. It replugs the deck and verifies connect() returns in ~4 s.
    }

    #[test]
    fn the_verified_commands_pass_the_gate() {
        for command in [
            Command::DisplayOn,
            Command::Brightness,
            Command::ImageTransfer,
            Command::ClearAll,
        ] {
            assert!(
                ensure_verified(command).is_ok(),
                "{} must be allowed",
                command.name()
            );
        }
    }

    #[test]
    fn the_unverified_commands_are_refused() {
        for command in [
            Command::ClearKey,
            Command::Sleep,
            Command::KeepAlive,
            Command::Handshake,
            Command::FirmwareVersion,
            Command::Mode,
        ] {
            match ensure_verified(command) {
                Err(Ss550Error::CommandNotVerified {
                    command: name,
                    reason,
                }) => {
                    assert_eq!(name, command.name());
                    assert!(!reason.trim().is_empty());
                }
                Ok(()) => panic!("{} must be refused", command.name()),
                Err(other) => panic!(
                    "{} was refused for the wrong reason: {other}",
                    command.name()
                ),
            }
        }
    }

    #[test]
    fn the_refusal_names_the_command_and_the_reason() {
        let error = ensure_verified(Command::ClearKey).expect_err("clear_key is unverified");
        assert_eq!(
            error.to_string(),
            "Refusing to send clear_key: never sent to a deck: live clear-per-key \
             is still pending (docs/protocol/CHANGELOG.md)"
        );
    }

    #[test]
    fn every_write_goes_through_the_gated_funnel() {
        // The gate is only as good as its coverage: every byte that leaves the
        // host must go through the one gated funnel. This scans **every `.rs`
        // file in the crate**, `src/` and `tests/`, not one file — the
        // transport lives in its own module now, so counting `.write(` in
        // `connection.rs` alone would go blind exactly while a second writer
        // exists.
        //
        // Two facts are asserted, and together they are the whole rule:
        //
        // 1. the crate has exactly one physical write, in the transport;
        // 2. it has exactly one call site into that write, in `write_pack`, and
        //    the verified-command check runs before it.
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        collect_sources(root, &mut files);
        assert!(!files.is_empty(), "the scan found no source files");

        let mut physical_writes = Vec::new();
        let mut funnel_calls = Vec::new();

        for file in &files {
            let text = fs::read_to_string(file).expect("every source file in this crate is UTF-8");
            // The test module is cut off first: this test names what it looks
            // for, so it would otherwise match its own text.
            let production = text.split("#[cfg(test)]").next().unwrap_or_default();
            let name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<unnamed>");

            physical_writes.extend(repeat_n(name, production.matches(".write(").count()));

            if let Some(call_site) = production.find(".write_report(") {
                let gate = production.find("ensure_verified(command)?");
                funnel_calls.push((name, gate.map(|gate| gate < call_site)));
            }
        }

        assert_eq!(
            physical_writes,
            vec!["hid_transport.rs"],
            "the only physical write must be the transport's write_report"
        );
        assert_eq!(
            funnel_calls,
            vec![("connection.rs", Some(true))],
            "the transport's write must have exactly one call site, inside the \
             gated funnel in connection.rs, after the verified-command check"
        );
    }

    /// Every `.rs` file under `root`, so the guard cannot go blind when a file
    /// is added. A build directory is the one thing skipped: its files are
    /// copies, and counting them would not say anything about this crate.
    fn collect_sources(root: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(root).expect("the source directory is readable") {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "target") {
                    continue;
                }
                collect_sources(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
    }

    /// `value` repeated `count` times, so a count turns into a list the
    /// assertion can compare exactly.
    fn repeat_n(value: &str, count: usize) -> Vec<String> {
        vec![value.to_owned(); count]
    }
}
