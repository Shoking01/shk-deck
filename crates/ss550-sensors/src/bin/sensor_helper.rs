//! The privileged half: reads the processor and answers one question on loopback.
//!
//! It exists because of a single Windows fact: PawnIO's driver — the only way to
//! read a Ryzen's thermal register — opens **only to an elevated process**. This
//! binary is that process, and it does nothing else:
//!
//! 1. opens the driver and reads the processor once a second;
//! 2. listens on `127.0.0.1:47821` and answers `GET` with
//!    `ss550-sensors/1 cpu=<degrees>` (see [`ss550_sensors::helper`]).
//!
//! It accepts no command, writes no file and touches no hardware beyond reading
//! that one register, so the elevated surface is as small as the job allows. It is
//! started by the controller, not at logon, and leaves when nothing asks any more;
//! without the privilege it still starts, reports that it cannot read the chip,
//! and stays useful for nothing — a sentence in its log, not a crash.
//!
//! It has **no window**. A console sitting on the desktop all day would be a
//! program asking to be closed, so the binary is built for the Windows GUI
//! subsystem: nothing appears when the task starts it. Started from a terminal it
//! still prints, because it inherits that terminal's handles — the subsystem
//! decides whether a *new* console is created, not whether output works.
//!
//! ```text
//! ss550-sensor-helper            # serve on the fixed port, silently
//! ss550-sensor-helper --once     # read once, print, exit (support tool)
//! ```
//!
//! **Everywhere else it is a stub that says so.** The driver, the register and the
//! task exist only on Windows, but the binary is still built on every platform so
//! `cargo test --workspace` keeps checking that it compiles there instead of
//! discovering it only on a Windows runner.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    serve::serve();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("ss550-sensor-helper: this program only serves on Windows");
}

#[cfg(windows)]
mod serve {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use ss550_sensors::helper::{HEADER, PORT};
    use ss550_sensors::Cpu;

    /// How often the processor is read.
    const SAMPLE: Duration = Duration::from_secs(1);

    /// How long a client may take to send its request.
    const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);

    /// How long the helper keeps existing without being asked anything.
    ///
    /// Longer than the app's own sample period by a wide margin, so a slow frame or
    /// a suspended machine does not kill the source, and short enough that nothing
    /// privileged outlives the window for long.
    const IDLE: u64 = 90_000;

    pub(crate) fn serve() {
        if std::env::args().any(|argument| argument == "--once") {
            match Cpu::open() {
                Some(cpu) => say(&format!(
                    "{HEADER} cpu={}",
                    cpu.temperature()
                        .map_or("-".to_string(), |value| format!("{value:.1}"))
                )),
                None => say(&format!(
                    "{HEADER} cpu=- (PawnIO's driver refused this process)"
                )),
            }
            return;
        }

        let reading = Arc::new(Mutex::new(None));
        let asked = Arc::new(AtomicU64::new(now_millis()));
        {
            let reading = Arc::clone(&reading);
            let asked = Arc::clone(&asked);
            std::thread::Builder::new()
                .name("sensor-helper-sampler".to_string())
                .spawn(move || sample(reading, asked))
                .expect("the sampling thread must start");
        }

        // A read per connection, so the socket can wait for ever; the watchdog is
        // what ends the process when Sh-Deck stops asking.
        let listener = match TcpListener::bind(("127.0.0.1", PORT)) {
            Ok(listener) => listener,
            Err(error) => {
                // Another copy is already answering, which is the good outcome.
                warn(&format!(
                    "ss550-sensor-helper: cannot listen on {PORT}: {error}"
                ));
                return;
            }
        };
        warn(&format!(
            "ss550-sensor-helper: listening on 127.0.0.1:{PORT}"
        ));

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    asked.store(now_millis(), Ordering::Relaxed);
                    let value = *reading
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Err(error) = answer(stream, value) {
                        warn(&format!("ss550-sensor-helper: could not answer: {error}"));
                    }
                }
                Err(error) => warn(&format!("ss550-sensor-helper: connection failed: {error}")),
            }
        }
    }

    /// Reads the processor on a loop, and leaves when nothing has asked for a while.
    ///
    /// The helper belongs to the controller, not to the machine: Sh-Deck starts it,
    /// Sh-Deck's samples keep it alive, and a minute after the window is gone it
    /// stops existing. A privileged process outliving the thing that needs it is a
    /// process nobody remembers to close.
    fn sample(reading: Arc<Mutex<Option<f32>>>, asked: Arc<AtomicU64>) {
        let mut cpu = Cpu::open();
        if cpu.is_none() {
            warn("ss550-sensor-helper: PawnIO refused this process; run it elevated (a scheduled task with highest privileges)");
        }

        loop {
            let since = now_millis().saturating_sub(asked.load(Ordering::Relaxed));
            if since > IDLE {
                warn(&format!(
                    "ss550-sensor-helper: nothing has asked for {IDLE} ms; leaving"
                ));
                std::process::exit(0);
            }
            let value = match cpu.as_mut() {
                Some(cpu) => cpu.temperature(),
                None => None,
            };
            *reading
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = value;
            std::thread::sleep(SAMPLE);
        }
    }

    /// Milliseconds since an arbitrary point, for the idle check.
    fn now_millis() -> u64 {
        use std::sync::OnceLock;
        use std::time::Instant;
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_millis() as u64
    }

    /// Says something on stdout, and does not care whether anyone is listening.
    ///
    /// The binary has no console, so the handles are null when the task starts it,
    /// and `println!` panics when the write fails — a panic here is a helper that
    /// dies silently and a temperature that stops arriving. These lines are for a
    /// redirected run (`ss550-sensor-helper --once > out.txt`), not for a desktop.
    fn say(text: &str) {
        use std::io::Write;
        let _ = writeln!(std::io::stdout(), "{text}");
    }

    /// The same, for the lines that are diagnostics.
    fn warn(text: &str) {
        use std::io::Write;
        let _ = writeln!(std::io::stderr(), "{text}");
    }

    /// Answers one client: it says `GET` (or anything), it gets the reading.
    fn answer(mut stream: TcpStream, value: Option<f32>) -> std::io::Result<()> {
        stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
        stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;

        let mut request = String::new();
        BufReader::new(stream.try_clone()?).read_line(&mut request)?;

        let number = value.map_or_else(|| "-".to_string(), |celsius| format!("{celsius:.1}"));
        stream.write_all(format!("{HEADER} cpu={number}\n").as_bytes())?;
        stream.flush()
    }
}
