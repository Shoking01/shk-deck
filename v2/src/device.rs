//! Device transport: blocking WebSocket client to the official Mirabox SDK
//! server (the same server the Electron host used).
//!
//! Runs on its own thread. Inbound events reach the UI through a channel;
//! outbound commands travel back through another channel. The socket carries a
//! short read timeout so this single thread can drain pending commands between
//! reads — one WebSocket handle, one owner, no shared locking.
//!
//! Wire shapes are the verified ones from host/device-client.js:
//!   in:  deviceDidConnect {path: BASE64, payload: {...}}, deviceDidDisconnect,
//!        read {payload: key event}
//!   out: setKeyImgData {path, payload:{keyIndex, imgData}} + refresh
//!        setBackgroundImgData {path, payload:{imgData}}
//!        read {path, payload:{}}   (re-asserted every 5s: the SDK routes input
//!                                   to the last client that sent it)

use serde_json::{json, Value};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::Message;
use tungstenite::stream::MaybeTlsStream;

pub const SDK_URL: &str = "ws://127.0.0.1:9002";

const READ_TIMEOUT: Duration = Duration::from_millis(100);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
const READ_REASSERT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum DeviceEvent {
    ServerConnected,
    ServerDisconnected,
    DeviceConnected { #[allow(dead_code)] path: String, info: Value },
    DeviceDisconnected,
    Key(Value),
    Error(String),
}

pub enum DeviceCommand {
    SetKey { key_index: u32, img_data: String },
    SetBackground { img_data: String },
}

pub struct DeviceHandle {
    pub events: Receiver<DeviceEvent>,
    commands: Sender<DeviceCommand>,
}

impl DeviceHandle {
    pub fn spawn(url: &str) -> Self {
        let (event_tx, event_rx) = channel();
        let (command_tx, command_rx) = channel();
        let url = url.to_string();
        thread::spawn(move || run(url, event_tx, command_rx));
        Self { events: event_rx, commands: command_tx }
    }

    pub fn set_key(&self, key_index: u32, img_data: String) {
        let _ = self.commands.send(DeviceCommand::SetKey { key_index, img_data });
    }

    #[allow(dead_code)] // used once v2 paints its own background
    pub fn set_background(&self, img_data: String) {
        let _ = self.commands.send(DeviceCommand::SetBackground { img_data });
    }
}

fn run(url: String, events: Sender<DeviceEvent>, commands: Receiver<DeviceCommand>) {
    loop {
        match tungstenite::connect(url.as_str()) {
            Ok((mut ws, _response)) => {
                if let MaybeTlsStream::Plain(stream) = ws.get_ref() {
                    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
                }
                let _ = events.send(DeviceEvent::ServerConnected);
                let mut device_path: Option<String> = None;
                let mut last_read_assert = Instant::now();

                loop {
                    // Outbound first: they are the reason this read timeout exists.
                    while let Ok(command) = commands.try_recv() {
                        let Some(path) = device_path.clone() else { continue };
                        match command {
                            DeviceCommand::SetKey { key_index, img_data } => {
                                let message = json!({
                                    "event": "setKeyImgData",
                                    "path": path,
                                    "payload": { "keyIndex": key_index, "imgData": img_data },
                                });
                                let _ = ws.send(Message::Text(message.to_string().into()));
                                // The device only shows the new image after a refresh.
                                let refresh = json!({ "event": "refresh", "path": path, "payload": {} });
                                let _ = ws.send(Message::Text(refresh.to_string().into()));
                            }
                            DeviceCommand::SetBackground { img_data } => {
                                let message = json!({
                                    "event": "setBackgroundImgData",
                                    "path": path,
                                    "payload": { "imgData": img_data },
                                });
                                let _ = ws.send(Message::Text(message.to_string().into()));
                            }
                        }
                    }

                    match ws.read() {
                        Ok(message) => {
                            if let Message::Text(text) = &message {
                                if let Ok(value) = serde_json::from_str::<Value>(text.as_str()) {
                                    match value.get("event").and_then(Value::as_str) {
                                        Some("deviceDidConnect") => {
                                            let path = value
                                                .get("path")
                                                .and_then(Value::as_str)
                                                .unwrap_or_default()
                                                .to_string();
                                            device_path = Some(path.clone());
                                            let _ = events.send(DeviceEvent::DeviceConnected {
                                                path,
                                                info: value.get("payload").cloned().unwrap_or(Value::Null),
                                            });
                                        }
                                        Some("deviceDidDisconnect") => {
                                            device_path = None;
                                            let _ = events.send(DeviceEvent::DeviceDisconnected);
                                        }
                                        Some("read") => {
                                            let _ = events.send(DeviceEvent::Key(
                                                value.get("payload").cloned().unwrap_or(Value::Null),
                                            ));
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            if matches!(message, Message::Close(_)) {
                                break;
                            }
                            let _ = ws.flush();
                        }
                        Err(tungstenite::Error::Io(error))
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                || error.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(_) => break,
                    }

                    // Keep the input subscription ours (see module docs).
                    if let Some(path) = device_path.as_deref() {
                        if last_read_assert.elapsed() >= READ_REASSERT {
                            let message = json!({ "event": "read", "path": path, "payload": {} });
                            let _ = ws.send(Message::Text(message.to_string().into()));
                            last_read_assert = Instant::now();
                        }
                    }
                }
                let _ = events.send(DeviceEvent::ServerDisconnected);
            }
            Err(error) => {
                let _ = events.send(DeviceEvent::Error(error.to_string()));
            }
        }
        thread::sleep(RECONNECT_DELAY);
    }
}
