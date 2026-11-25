//! What the Discord client says on its pipe, printed as it says it.
//!
//! This is the measurement the feature rests on: whether `std::fs::File` can
//! open `\\.\pipe\discord-ipc-0`, whether the framing is the one Discord
//! speaks, and whether `VOICE_SETTINGS_UPDATE_2` arrives without an OAuth
//! grant.
//!
//! ```text
//! cargo run -p ss550-discord --example voice_probe -- <application id> [seconds]
//! ```
//!
//! Without an argument it uses the Application ID stored in the configuration
//! folder, so a configured machine can run it bare.
//!
//! **One thread writes until the subscription is out.** A named pipe's file
//! object cannot have a read pending and accept a write at the same time, so a
//! second thread that writes while another reads blocks forever. That was the
//! first thing this probe measured, and it is why the order below is what it is.

use std::env;
use std::io::Read;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use ss550_discord::pipe::{self, Frames, Next, Opcode, CHUNK};
use ss550_discord::protocol;

/// How long to keep listening after the subscription, by default.
const LISTEN: Duration = Duration::from_secs(12);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let application_id = env::args().nth(1).or_else(ss550_discord::application_id);
    let Some(application_id) = application_id else {
        eprintln!(
            "no Application ID: pass one as the first argument, or store one with the window"
        );
        return Ok(());
    };
    let listen = env::args()
        .nth(2)
        .and_then(|seconds| seconds.parse::<u64>().ok())
        .map_or(LISTEN, Duration::from_secs);
    println!("application id: {application_id}");

    let (index, mut wire) = match pipe::connect() {
        Ok(open) => open,
        Err(error) => {
            println!("connect failed: {error}");
            return Ok(());
        }
    };
    println!("pipe: {index} ({})", pipe::pipe_path(index).display());

    let handshake = protocol::handshake(&application_id);
    pipe::write_frame(&mut wire, Opcode::Handshake, handshake.as_bytes())?;
    println!("> HANDSHAKE {handshake}");

    let mut frames = Frames::new();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let (opcode, payload) = frame(&mut wire, &mut frames, &mut buffer)?;
        println!("< {opcode:?} {}", String::from_utf8_lossy(&payload));
        if protocol::read(&payload) == protocol::Incoming::Ready {
            break;
        }
    }

    let subscribe = protocol::subscribe("probe-1");
    pipe::write_frame(&mut wire, Opcode::Frame, subscribe.as_bytes())?;
    println!("> SUBSCRIBE {subscribe}");
    println!("--- listening {listen:?}: change the microphone in Discord or with a hotkey ---");

    // From here on this probe only reads, so the wire belongs to the reader.
    let (sender, inbox) = mpsc::channel();
    thread::spawn(move || read_frames(wire, sender));

    let deadline = Instant::now() + listen;
    let mut heard = 0;
    while Instant::now() < deadline {
        let Ok((opcode, payload)) = inbox.recv_timeout(Duration::from_millis(200)) else {
            continue;
        };
        let text = String::from_utf8_lossy(&payload);
        println!("< {opcode:?} {text}");
        if protocol::read(&payload) != protocol::Incoming::Other {
            heard += 1;
        }
    }

    println!("--- {heard} voice frames in {listen:?}");
    Ok(())
}

/// One whole frame, reading more bytes until there is one.
fn frame(
    wire: &mut std::fs::File,
    frames: &mut Frames,
    buffer: &mut [u8],
) -> Result<(Opcode, Vec<u8>), Box<dyn std::error::Error>> {
    loop {
        match frames.next_frame() {
            Next::Frame(opcode, payload) => return Ok((opcode, payload)),
            Next::Incomplete => {}
            Next::Oversized { length } => return Err(format!("oversized frame: {length}").into()),
            Next::Unknown { opcode } => return Err(format!("unknown opcode: {opcode}").into()),
        }
        let read = wire.read(buffer)?;
        if read == 0 {
            return Err("the pipe closed".into());
        }
        frames.push(&buffer[..read]);
    }
}

/// Hands every frame to the printer, and says how the wire ended.
fn read_frames(mut wire: std::fs::File, sender: mpsc::Sender<(Opcode, Vec<u8>)>) {
    let mut frames = Frames::new();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = match wire.read(&mut buffer) {
            Ok(0) => {
                let _ = sender.send((Opcode::Close, b"the pipe closed".to_vec()));
                return;
            }
            Ok(read) => read,
            Err(error) => {
                let _ = sender.send((
                    Opcode::Close,
                    format!("the pipe failed: {error}").into_bytes(),
                ));
                return;
            }
        };
        frames.push(&buffer[..read]);
        loop {
            match frames.next_frame() {
                Next::Frame(opcode, payload) => {
                    if sender.send((opcode, payload)).is_err() {
                        return;
                    }
                }
                Next::Incomplete => break,
                Next::Oversized { length } => {
                    let _ = sender.send((
                        Opcode::Close,
                        format!("oversized frame: {length} bytes").into_bytes(),
                    ));
                    return;
                }
                Next::Unknown { opcode } => {
                    let _ = sender.send((
                        Opcode::Close,
                        format!("unknown opcode: {opcode}").into_bytes(),
                    ));
                    return;
                }
            }
        }
    }
}
