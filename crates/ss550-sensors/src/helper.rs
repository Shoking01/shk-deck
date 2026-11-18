//! Talking to the privileged sensor helper.
//!
//! The processor's temperature needs a driver that opens only to an elevated
//! process. Rather than elevate the whole application, a small helper runs with
//! the privilege and answers one question over loopback; this module is the
//! unprivileged side of that conversation.
//!
//! The protocol is one line in, one line out, on a fixed loopback port:
//!
//! ```text
//! client → GET
//! helper → ss550-sensors/1 cpu=61.5
//! ```
//!
//! **What this is not:** an authenticated channel. Any local process could
//! connect and read the number — a temperature is not a secret — and a local
//! process could bind the port first and answer with a different one. The worst
//! that buys is a wrong number on a key, which is why the range check is here
//! too, and why the helper never accepts a command, only a request for the
//! reading it already measured.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::usable_celsius;

/// The loopback port the helper listens on.
///
/// Fixed, documented and high: a port a user can type in a firewall rule, and
/// one far from anything a common service would claim.
pub const PORT: u16 = 47821;

/// The first word of every answer, with its protocol version.
pub const HEADER: &str = "ss550-sensors/1";

/// How long the connect may take before the helper is called absent.
const CONNECT: Duration = Duration::from_millis(150);

/// How long the answer may take once connected.
const READ: Duration = Duration::from_millis(300);

/// The processor's temperature, as the helper last measured it.
///
/// `None` when the helper is not running, not answering, or answered something
/// that is not a temperature — which is exactly the case where the caller should
/// fall back to another source.
pub fn cpu_temperature() -> Option<f32> {
    ask()?
        .strip_prefix(HEADER)?
        .trim()
        .strip_prefix("cpu=")
        .map(str::trim)
        .and_then(parse_celsius)
}

/// Whether a helper is answering at all.
pub fn is_running() -> bool {
    ask().is_some()
}

/// One request, one answer.
fn ask() -> Option<String> {
    let address: SocketAddr = format!("127.0.0.1:{PORT}").to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&address, CONNECT).ok()?;
    stream.set_read_timeout(Some(READ)).ok()?;
    stream.set_write_timeout(Some(CONNECT)).ok()?;
    stream.write_all(b"GET\n").ok()?;

    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).ok()?;
    let line = line.trim();
    line.starts_with(HEADER).then(|| line.to_string())
}

/// The number in one field of the answer.
fn parse_celsius(text: &str) -> Option<f32> {
    if text == "-" || text.is_empty() {
        return None;
    }
    let value: f32 = text.parse().ok()?;
    usable_celsius(value.round() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_is_read_out_of_the_answer() {
        assert_eq!(parse_celsius("61.5"), Some(62.0));
        assert_eq!(parse_celsius("61"), Some(61.0));
    }

    #[test]
    fn an_answer_without_a_reading_is_not_a_reading() {
        assert_eq!(parse_celsius("-"), None);
        assert_eq!(parse_celsius(""), None);
        assert_eq!(parse_celsius("hot"), None);
        // The same range as every other source, so a spoofed helper cannot put a
        // fantasy on a key.
        assert_eq!(parse_celsius("-5"), None);
        assert_eq!(parse_celsius("400"), None);
    }

    #[test]
    fn a_helper_that_is_not_listening_answers_nothing_quickly() {
        let started = std::time::Instant::now();
        let _ = ask();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "a missing helper must not hold the sampler"
        );
    }
}
