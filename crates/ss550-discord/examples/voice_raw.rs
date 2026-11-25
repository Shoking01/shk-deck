//! Asks Discord what the **real** voice command does without an authorization.
//!
//! `SET_VOICE_SETTINGS` is the command a stock client uses, and it needs the
//! `rpc.voice.write` scope. This crate sends `SET_VOICE_SETTINGS_2` instead,
//! because that one is `rpc.local`. If a deck key changes the state we read but
//! Discord itself does nothing, the difference between those two commands is
//! the first thing to measure.
//!
//! ```text
//! cargo run -p ss550-discord --example voice_raw -- <application id>
//! ```
//!
//! It mutes, reads the answer, unmutes, and reads the answer again. The state is
//! left as it was found whether Discord cooperates or refuses.

use std::io::Read;
use std::time::{Duration, Instant};

use ss550_discord::pipe::{self, Frames, Next, Opcode, CHUNK};
use ss550_discord::protocol;

/// How long to wait for one answer.
const PATIENCE: Duration = Duration::from_secs(5);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(application_id) = std::env::args()
        .nth(1)
        .or_else(ss550_discord::application_id)
    else {
        eprintln!("no Application ID: pass one as the first argument");
        return Ok(());
    };

    let (_index, mut wire) = pipe::connect()?;
    pipe::write_frame(
        &mut wire,
        Opcode::Handshake,
        protocol::handshake(&application_id).as_bytes(),
    )?;

    let mut frames = Frames::new();
    let mut buffer = vec![0u8; CHUNK];
    // One thread, one direction at a time: this probe never reads while it
    // writes, which is the only way a blocking handle can do both.
    loop {
        let (opcode, payload) = frame(&mut wire, &mut frames, &mut buffer)?;
        let _ = opcode;
        if protocol::read(&payload) == protocol::Incoming::Ready {
            break;
        }
    }
    println!("READY; sending the command that needs rpc.voice.write");

    for (label, mute) in [("mute", true), ("restore", false)] {
        let payload = protocol::raw_set_voice(&format!("raw-{label}"), Some(mute), None);
        pipe::write_frame(&mut wire, Opcode::Frame, payload.as_bytes())?;
        println!("> {payload}");

        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            let (_, answer) = frame(&mut wire, &mut frames, &mut buffer)?;
            let text = String::from_utf8_lossy(&answer);
            if protocol::answers(&answer, &format!("raw-{label}")) {
                println!("< {text}");
                println!("  -> {}", verdict(&answer));
                break;
            }
        }
    }

    println!("--- done; the microphone is as it was found ---");
    Ok(())
}

/// What an answer means in a sentence.
fn verdict(answer: &[u8]) -> String {
    match protocol::read(answer) {
        protocol::Incoming::Refused { code, message } => {
            format!("Discord refused it: {message} (code {code})")
        }
        _ => {
            // The answer to `SET_VOICE_SETTINGS` is the resolved settings, so a
            // payload with `mute` in it is a success even though the state did
            // not have to change for the request to be accepted.
            match serde_json::from_slice::<serde_json::Value>(answer) {
                Ok(value)
                    if value
                        .get("data")
                        .and_then(|data| data.get("mute"))
                        .is_some() =>
                {
                    "accepted: Discord sent the resolved settings back".to_string()
                }
                _ => "accepted, with no refusal in the answer".to_string(),
            }
        }
    }
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
