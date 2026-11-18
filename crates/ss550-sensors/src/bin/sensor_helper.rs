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
//! that one register, so the elevated surface is as small as the job allows. Run
//! it with a scheduled task at logon and highest privileges; without the
//! privilege it still starts, reports that it cannot read the chip, and stays
//! useful for nothing — which is a sentence in its log, not a crash.
//!
//! ```text
//! ss550-sensor-helper            # serve on the fixed port
//! ss550-sensor-helper --once     # read once, print, exit (support tool)
//! ```

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ss550_sensors::helper::{HEADER, PORT};
use ss550_sensors::Cpu;

/// How often the processor is read.
const SAMPLE: Duration = Duration::from_secs(1);

/// How long a client may take to send its request.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);

fn main() {
    if std::env::args().any(|argument| argument == "--once") {
        match Cpu::open() {
            Some(cpu) => println!(
                "{HEADER} cpu={}",
                cpu.temperature()
                    .map_or("-".to_string(), |value| format!("{value:.1}"))
            ),
            None => println!("{HEADER} cpu=- (PawnIO's driver refused this process)"),
        }
        return;
    }

    let reading = Arc::new(Mutex::new(None));
    {
        let reading = Arc::clone(&reading);
        std::thread::Builder::new()
            .name("sensor-helper-sampler".to_string())
            .spawn(move || sample(reading))
            .expect("the sampling thread must start");
    }

    let listener = match TcpListener::bind(("127.0.0.1", PORT)) {
        Ok(listener) => listener,
        Err(error) => {
            // Another copy is already answering, which is the good outcome.
            eprintln!("ss550-sensor-helper: cannot listen on {PORT}: {error}");
            return;
        }
    };
    println!("ss550-sensor-helper: listening on 127.0.0.1:{PORT}");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let value = *reading
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Err(error) = answer(stream, value) {
                    eprintln!("ss550-sensor-helper: could not answer: {error}");
                }
            }
            Err(error) => eprintln!("ss550-sensor-helper: connection failed: {error}"),
        }
    }
}

/// Reads the processor on a loop until the process ends.
fn sample(reading: Arc<Mutex<Option<f32>>>) {
    let mut cpu = Cpu::open();
    if cpu.is_none() {
        eprintln!(
            "ss550-sensor-helper: PawnIO refused this process; run it elevated (a scheduled task with highest privileges)"
        );
    }

    loop {
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
