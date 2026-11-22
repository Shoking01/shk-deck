//! The payloads that travel inside the frames.
//!
//! Every payload is a JSON object; the command, the arguments and the nonce are
//! plain fields of it. This module builds the three commands this crate sends
//! and reads the four answers that matter — it does not know what a pipe is,
//! so every rule in it is a test over a string.

use serde::Deserialize;
use serde_json::{json, Value};

/// The protocol version Discord's IPC transport speaks today.
const VERSION: u32 = 1;

/// The scope-less event that carries the client's own mute and deafen flags.
///
/// Reading it needs `rpc.local`, which every local connection has: that is why
/// this crate carries no token and no secret.
pub const VOICE_EVENT: &str = "VOICE_SETTINGS_UPDATE_2";

/// The handshake packet: the version, and which application is asking.
///
/// ```
/// use ss550_discord::protocol::handshake;
///
/// let payload: serde_json::Value =
///     serde_json::from_str(&handshake("123")).expect("valid JSON");
/// assert_eq!(payload["v"], 1);
/// assert_eq!(payload["client_id"], "123");
/// ```
pub fn handshake(application_id: &str) -> String {
    json!({ "v": VERSION, "client_id": application_id }).to_string()
}

/// A `SUBSCRIBE` for the voice settings event.
pub fn subscribe(nonce: &str) -> String {
    json!({
        "cmd": "SUBSCRIBE",
        "evt": VOICE_EVENT,
        "args": {},
        "nonce": nonce,
    })
    .to_string()
}

/// The real voice-settings command, the one a stock client uses.
///
/// It is **not** what a working feature would send first: it needs the
/// `rpc.voice.write` scope, which Discord grants only to an authorized
/// application, and `AUTHORIZE` answers `5000 invalid_scope` for it unless the
/// application has partner approval. This exists so a probe can ask Discord
/// directly, and its refusal is the record of a closed door.
pub fn raw_set_voice(nonce: &str, mute: Option<bool>, deaf: Option<bool>) -> String {
    let mut args = serde_json::Map::new();
    if let Some(mute) = mute {
        args.insert("mute".to_string(), Value::Bool(mute));
    }
    if let Some(deaf) = deaf {
        args.insert("deaf".to_string(), Value::Bool(deaf));
    }
    json!({
        "cmd": "SET_VOICE_SETTINGS",
        "args": Value::Object(args),
        "nonce": nonce,
    })
    .to_string()
}

/// What an incoming payload was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// The client accepted the handshake.
    Ready,
    /// The client's own mute and deafen, as it states them.
    Voice {
        /// The microphone's mute, when the client stated it.
        mute: Option<bool>,
        /// The speaker's deafen, when the client stated it.
        deaf: Option<bool>,
    },
    /// The client refused something, in its own words.
    Refused {
        /// Discord's error code.
        code: i64,
        /// Discord's sentence.
        message: String,
    },
    /// Anything this crate does not read: an acknowledgement, an event it did
    /// not subscribe to, a keep-alive.
    Other,
}

/// The parts of a payload this crate reads.
#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    cmd: Option<String>,
    #[serde(default)]
    evt: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    data: Option<Value>,
}

/// What an incoming payload says, for the parts this crate acts on.
///
/// ```
/// use ss550_discord::protocol::{read, Incoming};
///
/// let event = br#"{"cmd":"DISPATCH","evt":"VOICE_SETTINGS_UPDATE_2",
///                  "data":{"self_mute":true,"self_deaf":false}}"#;
/// assert_eq!(
///     read(event),
///     Incoming::Voice { mute: Some(true), deaf: Some(false) }
/// );
/// ```
pub fn read(payload: &[u8]) -> Incoming {
    let Ok(envelope) = serde_json::from_slice::<Envelope>(payload) else {
        // A payload this crate cannot parse is not a reason to drop a working
        // connection: the frames are independent, and the next one may be the
        // one that matters.
        return Incoming::Other;
    };

    if envelope.evt.as_deref() == Some("READY") {
        return Incoming::Ready;
    }
    if envelope.evt.as_deref() == Some("ERROR") {
        let code = envelope
            .data
            .as_ref()
            .and_then(|data| data.get("code"))
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let message = envelope
            .data
            .as_ref()
            .and_then(|data| data.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("Discord refused the command")
            .to_string();
        return Incoming::Refused { code, message };
    }
    if envelope.evt.as_deref() == Some(VOICE_EVENT) {
        let data = envelope.data.unwrap_or(Value::Null);
        return Incoming::Voice {
            mute: data.get("self_mute").and_then(Value::as_bool),
            deaf: data.get("self_deaf").and_then(Value::as_bool),
        };
    }
    Incoming::Other
}

/// Whether a payload is the answer to the command with this nonce.
///
/// ```
/// use ss550_discord::protocol::answers;
///
/// assert!(answers(br#"{"cmd":"SUBSCRIBE","nonce":"n7"}"#, "n7"));
/// assert!(!answers(br#"{"cmd":"SUBSCRIBE","nonce":"n8"}"#, "n7"));
/// assert!(!answers(br#"{"cmd":"DISPATCH","evt":"READY"}"#, "n7"));
/// ```
pub fn answers(payload: &[u8], nonce: &str) -> bool {
    serde_json::from_slice::<Envelope>(payload)
        .ok()
        .is_some_and(|envelope| envelope.cmd.is_some() && envelope.nonce.as_deref() == Some(nonce))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handshake_carries_the_version_and_the_application() {
        let payload: serde_json::Value =
            serde_json::from_str(&handshake("42")).expect("valid JSON");
        assert_eq!(payload["v"], 1);
        assert_eq!(payload["client_id"], "42");
    }

    #[test]
    fn a_subscription_names_the_event_and_a_nonce() {
        let payload = subscribe("n1");
        assert!(payload.contains(r#""cmd":"SUBSCRIBE""#));
        assert!(payload.contains(r#""evt":"VOICE_SETTINGS_UPDATE_2""#));
        assert!(payload.contains(r#""nonce":"n1""#));
    }

    #[test]
    fn the_real_command_names_mute_and_deaf_without_the_self_prefix() {
        // The difference that matters: `_2` takes `self_mute`, the real command
        // takes `mute`. Sending the wrong one is how this looked like it worked.
        let payload = raw_set_voice("n1", Some(true), None);
        assert!(payload.contains(r#""cmd":"SET_VOICE_SETTINGS""#));
        assert!(payload.contains(r#""mute":true"#));
        assert!(
            !payload.contains("self_mute"),
            "the real command has no self_ prefix: {payload}"
        );
    }

    #[test]
    fn the_event_carries_both_flags() {
        let event = br#"{"cmd":"DISPATCH","evt":"VOICE_SETTINGS_UPDATE_2","data":{"self_mute":false,"self_deaf":true}}"#;
        assert_eq!(
            read(event),
            Incoming::Voice {
                mute: Some(false),
                deaf: Some(true)
            }
        );
    }

    #[test]
    fn an_event_that_states_one_flag_leaves_the_other_alone() {
        // Discord has shipped payloads that omit a field; a missing flag is not
        // a `false`, and treating it as one would paint a lie on the key.
        let event = br#"{"evt":"VOICE_SETTINGS_UPDATE_2","data":{"self_mute":true}}"#;
        assert_eq!(
            read(event),
            Incoming::Voice {
                mute: Some(true),
                deaf: None
            }
        );
    }

    #[test]
    fn ready_is_ready() {
        assert_eq!(
            read(br#"{"cmd":"DISPATCH","evt":"READY","data":{"v":1}}"#),
            Incoming::Ready
        );
    }

    #[test]
    fn a_refusal_keeps_discords_own_sentence() {
        assert_eq!(
            read(br#"{"evt":"ERROR","data":{"code":4006,"message":"You are not authorized"}}"#),
            Incoming::Refused {
                code: 4006,
                message: "You are not authorized".to_string()
            }
        );
    }

    #[test]
    fn an_unreadable_payload_is_not_a_disconnection() {
        assert_eq!(read(b"not json at all"), Incoming::Other);
        assert_eq!(read(b"{}"), Incoming::Other);
    }

    #[test]
    fn a_nonce_is_matched_only_against_the_one_that_asked() {
        assert!(answers(br#"{"cmd":"SUBSCRIBE","nonce":"n7"}"#, "n7"));
        assert!(!answers(br#"{"cmd":"SUBSCRIBE","nonce":"n8"}"#, "n7"));
        assert!(
            !answers(br#"{"evt":"READY"}"#, "n7"),
            "an event without a nonce answers nothing"
        );
    }
}
