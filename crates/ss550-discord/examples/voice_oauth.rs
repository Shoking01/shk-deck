//! Asks Discord whether this application may read the real voice settings.
//!
//! The `_2` commands are an application-private sandbox: they mute nothing and
//! report nothing about the client. The commands that do — `GET_VOICE_SETTINGS`,
//! `SET_VOICE_SETTINGS` and the `VOICE_SETTINGS_UPDATE` event — need an OAuth2
//! grant. This probe walks the whole flow once, by hand, to find out whether
//! Discord gives that grant to *this* application before any of it is built into
//! the app:
//!
//! 1. `AUTHORIZE` over the pipe, which opens Discord's own consent window;
//! 2. the code is exchanged for an access token at Discord's token endpoint;
//! 3. `AUTHENTICATE` with that token;
//! 4. `GET_VOICE_SETTINGS` — the real `mute` and `deaf` of the client.
//!
//! ```text
//! $env:DISCORD_CLIENT_SECRET = "<the secret from the portal>"
//! cargo run -p ss550-discord --example voice_oauth -- <application id>
//! ```
//!
//! The secret is read from the environment and never written anywhere or
//! printed: it travels in one form-encoded request body, which is how Discord's
//! endpoint expects it.
//!
//! Nothing here mutes anything. The one command that could is not sent, so a run
//! leaves the microphone exactly as it found it.

use std::io::Read;
use std::time::{Duration, Instant};

use ss550_discord::pipe::{self, Frames, Next, Opcode, CHUNK};
use ss550_discord::protocol;

/// Where a code becomes a token. Form-encoded only: Discord rejects JSON here.
const TOKEN_URL: &str = "https://discord.com/api/oauth2/token";

/// The two scopes the feature needs, and the reason this probe exists.
const SCOPES: [&str; 2] = ["rpc.voice.read", "rpc.voice.write"];
/// How long the consent window may stay open.
const CONSENT: Duration = Duration::from_secs(180);

/// How long Discord may take to answer one command.
const PATIENCE: Duration = Duration::from_secs(20);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(application_id) = std::env::args()
        .nth(1)
        .or_else(ss550_discord::application_id)
    else {
        eprintln!("no Application ID: pass one as the first argument");
        return Ok(());
    };
    let Ok(secret) = std::env::var("DISCORD_CLIENT_SECRET") else {
        eprintln!("set DISCORD_CLIENT_SECRET first: the token exchange needs it");
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
    loop {
        let (_, payload) = frame(&mut wire, &mut frames, &mut buffer)?;
        if protocol::read(&payload) == protocol::Incoming::Ready {
            break;
        }
    }
    println!("READY");

    // 1. The consent window. This is the step that can say "no".
    //
    // `DISCORD_SCOPES` overrides the pair, which is how a run tells apart "this
    // application may not authorize anything" from "these two scopes are not
    // available to it": `identify` is a scope every application may ask for.
    let scopes: Vec<String> = std::env::var("DISCORD_SCOPES")
        .ok()
        .filter(|text| !text.trim().is_empty())
        .map(|text| {
            text.split(',')
                .map(|scope| scope.trim().to_string())
                .collect()
        })
        .unwrap_or_else(|| SCOPES.iter().map(|scope| scope.to_string()).collect());
    let listed: Vec<String> = scopes.iter().map(|scope| format!("\"{scope}\"")).collect();
    let payload = format!(
        r#"{{"cmd":"AUTHORIZE","args":{{"client_id":"{application_id}","scopes":[{}]}},"nonce":"oauth-authorize"}}"#,
        listed.join(",")
    );
    pipe::write_frame(&mut wire, Opcode::Frame, payload.as_bytes())?;
    println!(
        "> AUTHORIZE [{}] (a consent window should open in Discord — approve it)",
        scopes.join(", ")
    );

    let answer = await_nonce(
        &mut wire,
        &mut frames,
        &mut buffer,
        "oauth-authorize",
        CONSENT,
    )?;
    if let protocol::Incoming::Refused { code, message } = protocol::read(&answer) {
        println!("REFUSED by Discord: {message} (code {code})");
        println!("--- the grant is not available to this application: stop here ---");
        return Ok(());
    }
    let Some(authorization_code) = field(&answer, "code") else {
        println!(
            "no code in the answer: {}",
            String::from_utf8_lossy(&answer)
        );
        return Ok(());
    };
    println!("< consent granted, code received");

    // 2. The exchange. The only request that leaves this machine.
    let body = format!(
        "client_id={}&client_secret={}&grant_type=authorization_code&code={}",
        percent(&application_id),
        percent(&secret),
        percent(&authorization_code)
    );
    let access_token = match ureq::post(TOKEN_URL)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&body)
    {
        Ok(response) => match response.into_string() {
            Ok(text) => match json_field(&text, "access_token") {
                Some(token) => {
                    println!("< token received ({} characters)", token.len());
                    token
                }
                None => {
                    println!("the token answer has no access_token: {text}");
                    return Ok(());
                }
            },
            Err(error) => {
                println!("the token answer could not be read: {error}");
                return Ok(());
            }
        },
        Err(ureq::Error::Status(status, response)) => {
            let text = response.into_string().unwrap_or_default();
            println!("the token exchange failed with HTTP {status}: {text}");
            println!("--- the grant is not available to this application: stop here ---");
            return Ok(());
        }
        Err(error) => {
            println!("the token exchange could not be sent: {error}");
            return Ok(());
        }
    };

    // 3. Prove the token to the same connection.
    let payload = format!(
        r#"{{"cmd":"AUTHENTICATE","args":{{"access_token":"{}"}},"nonce":"oauth-auth"}}"#,
        access_token.replace('"', "")
    );
    pipe::write_frame(&mut wire, Opcode::Frame, payload.as_bytes())?;
    let answer = await_nonce(&mut wire, &mut frames, &mut buffer, "oauth-auth", PATIENCE)?;
    match protocol::read(&answer) {
        protocol::Incoming::Refused { code, message } => {
            println!("AUTHENTICATE refused: {message} (code {code})");
            return Ok(());
        }
        _ => println!("< authenticated"),
    }

    // 4. The question the feature actually asks.
    let payload = r#"{"cmd":"GET_VOICE_SETTINGS","args":{},"nonce":"oauth-get"}"#;
    pipe::write_frame(&mut wire, Opcode::Frame, payload.as_bytes())?;
    let answer = await_nonce(&mut wire, &mut frames, &mut buffer, "oauth-get", PATIENCE)?;
    if let protocol::Incoming::Refused { code, message } = protocol::read(&answer) {
        println!("GET_VOICE_SETTINGS refused: {message} (code {code})");
        return Ok(());
    }
    let text = String::from_utf8_lossy(&answer);
    match (json_field(&text, "mute"), json_field(&text, "deaf")) {
        (Some(mute), Some(deaf)) => {
            println!("--- REAL CLIENT STATE: mute={mute} deaf={deaf} ---");
            println!("--- this is what the deck key must show ---");
        }
        _ => println!("the settings answer had no mute/deaf: {text}"),
    }
    Ok(())
}

/// Waits for the answer carrying one nonce.
fn await_nonce(
    wire: &mut std::fs::File,
    frames: &mut Frames,
    buffer: &mut [u8],
    nonce: &str,
    patience: Duration,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + patience;
    while Instant::now() < deadline {
        let (_, payload) = frame(wire, frames, buffer)?;
        if protocol::answers(&payload, nonce) {
            return Ok(payload);
        }
    }
    Err(format!("no answer to {nonce} within {patience:?}").into())
}

/// A field of the answer's `data`, as text.
fn field(answer: &[u8], key: &str) -> Option<String> {
    json_field(&String::from_utf8_lossy(answer), key)
}

/// A string or number field anywhere in a JSON document, as text.
fn json_field(text: &str, key: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let found = value
        .get("data")
        .and_then(|data| data.get(key))
        .or_else(|| value.get(key))?;
    match found {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// Percent-encodes a form value: everything but the unreserved set.
fn percent(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
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
