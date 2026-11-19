//! One controller per desktop.
//!
//! A second start — a double click on the Start Menu entry while the controller
//! already lives in the tray — must not open a second controller. Two of them
//! poll the same deck, paint over each other's tiles and fight for the tray
//! icon. The second one instead **asks the running one to show its window** and
//! leaves, which is what makes the entry behave like an application rather than
//! a launcher that sometimes works.
//!
//! Two kernel objects, both per-session: a named mutex that says "a controller
//! owns this desktop", and a named event that carries the ask. Nothing is
//! registered, nothing is left behind when the process ends — the handles are
//! the whole state, and the system closes them.

#[cfg(windows)]
pub use platform::{acquire, Launch, SingleInstance};

#[cfg(not(windows))]
pub use other::{acquire, Launch, SingleInstance};

#[cfg(windows)]
mod platform {
    use std::sync::mpsc::{channel, Receiver};

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
    };

    use crate::TrayError;

    /// The mutex that names the controller of this desktop.
    const MUTEX_NAME: &str = "Local\\ss550-controller-single";

    /// The event a second start rings to ask for the window.
    const SHOW_EVENT: &str = "Local\\ss550-controller-show";

    /// What a start found when it looked for a controller already running.
    pub enum Launch {
        /// This process is the controller; the receiver answers when another
        /// start asks for the window.
        First(SingleInstance),
        /// A controller was already running and has been asked to show itself.
        AlreadyRunning,
    }

    /// The kernel objects that make this process the one controller.
    ///
    /// Dropping it closes the handles, and with them the claim.
    pub struct SingleInstance {
        /// Held for the life of the process: the name is taken while it exists.
        _mutex: HANDLE,
        /// Closed with the process; the listener thread waits on its own copy.
        event: HANDLE,
        /// One item per ask, delivered by the listener thread.
        asks: Receiver<()>,
    }

    impl SingleInstance {
        /// Whether another start has asked for the window since the last call.
        ///
        /// Non-blocking: the window loop asks this on every tick and shows
        /// itself when the answer is yes.
        pub fn asked(&self) -> bool {
            self.asks.try_recv().is_ok()
        }
    }

    // SAFETY: the handle is a kernel object handle, which the system allows any
    // thread to close; nothing here is a pointer into this process.
    unsafe impl Send for SingleInstance {}

    impl Drop for SingleInstance {
        fn drop(&mut self) {
            // SAFETY: both handles were returned by the system and are closed
            // exactly once, here.
            unsafe {
                let _ = CloseHandle(self.event);
                let _ = CloseHandle(self._mutex);
            }
        }
    }

    /// A kernel handle that a thread can carry: the value is an opaque number,
    /// and only the thread that created the object ever passes it back.
    #[derive(Clone, Copy)]
    struct Handle(usize);

    /// Wide, NUL-terminated name for a kernel object.
    fn name(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Claims the desktop for this process, or asks the owner to show itself.
    pub fn acquire() -> Result<Launch, TrayError> {
        let mutex_name = name(MUTEX_NAME);
        // SAFETY: the name is a live wide string, and no security attributes are
        // asked for. The handle is closed by `SingleInstance`.
        let mutex = unsafe { CreateMutexW(None, true, PCWSTR(mutex_name.as_ptr())) }
            .map_err(|error| TrayError::Tray(format!("the controller mutex: {error}")))?;

        // SAFETY: reads the thread's last error, which `CreateMutexW` just set.
        let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        if already {
            // SAFETY: the handle is ours and is not used again.
            unsafe {
                let _ = CloseHandle(mutex);
            }
            ask_owner();
            return Ok(Launch::AlreadyRunning);
        }

        let event_name = name(SHOW_EVENT);
        // SAFETY: live wide name, no security attributes, auto-reset and
        // initially unsignalled.
        let event = unsafe { CreateEventW(None, false, false, PCWSTR(event_name.as_ptr())) }
            .map_err(|error| TrayError::Tray(format!("the show event: {error}")))?;

        // The listener waits on the same handle for the life of the process.
        // A kernel handle is a plain number; wrapping it is what lets the listener
        // thread hold it while the struct owns it.
        let listener = Handle(event.0 as usize);
        let (sender, asks) = channel();
        let spawned = std::thread::Builder::new()
            .name("ss550-second-start".to_string())
            .spawn(move || loop {
                // SAFETY: waiting on a handle this thread owns, with no timeout.
                let waited = unsafe { WaitForSingleObject(HANDLE(listener.0 as *mut _), u32::MAX) };
                if waited != WAIT_OBJECT_0 {
                    break;
                }
                if sender.send(()).is_err() {
                    // The window is gone: nobody is left to show.
                    break;
                }
            });
        spawned.map_err(|error| TrayError::Tray(error.to_string()))?;

        Ok(Launch::First(SingleInstance {
            _mutex: mutex,
            event,
            asks,
        }))
    }

    /// Rings the running controller's event, if there is one to ring.
    fn ask_owner() {
        let event_name = name(SHOW_EVENT);
        // SAFETY: live wide name; the handle is closed below.
        let Ok(event) =
            (unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(event_name.as_ptr())) })
        else {
            // No listener: the other process is on its way out.
            return;
        };
        // SAFETY: the handle was just opened with modify access.
        unsafe {
            let _ = SetEvent(event);
            let _ = CloseHandle(event);
        }
    }
}

#[cfg(not(windows))]
mod other {
    use crate::TrayError;

    /// Everywhere but Windows a start is the only start.
    pub struct SingleInstance;

    impl SingleInstance {
        /// There is never a second start asking.
        pub fn asked(&self) -> bool {
            false
        }
    }

    /// A desktop with no single-instance rule.
    pub enum Launch {
        /// This process is the controller.
        First(SingleInstance),
    }

    /// Everywhere but Windows the controller is always the first one.
    pub fn acquire() -> Result<Launch, TrayError> {
        Ok(Launch::First(SingleInstance))
    }
}
