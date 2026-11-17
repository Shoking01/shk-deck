//! The icon a website publishes, as the bytes it served.
//!
//! A key bound to a URL draws the site's own mark, the way a key bound to a
//! program draws the program's icon. The picture is not stored in the profile
//! and is not part of the action: it is derived from the host, fetched once, and
//! kept by the caller. This module answers the **bytes**; decoding them is
//! [`crate::favicon`]'s caller's job, because the window already links an image
//! decoder and this crate does not.
//!
//! ## Which paths are asked
//!
//! `/favicon.ico` is what a browser falls back to, and here it is the **last**
//! thing asked. It is almost always the 16- or 32-pixel picture the tab bar
//! draws, and a key is 108 pixels wide: the same file scaled up is a mosaic of
//! squares, not the site's mark. Modern sites also publish the picture they hand
//! a phone's home screen, at `/apple-touch-icon.png`, and that one is drawn for
//! a real tile — `x.com` answers 1024×1024 there and a 32×32 PNG at
//! `/favicon.ico`. So the candidates are asked best first:
//! `/apple-touch-icon.png`, `/apple-touch-icon-precomposed.png`, `/favicon.ico`.
//!
//! A pixel size cannot be read from these bytes without decoding, and this crate
//! has no decoder, so **every body that answered** is returned and the caller
//! keeps the largest. Asking still stops early: a first candidate of
//! [`LARGE_BODY`] bytes or more is a real icon and not a 32×32, and two more
//! requests to learn nothing would cost the background thread seconds it does
//! not have to spend.
//!
//! ## Why WinINet and not a socket
//!
//! `https://<host>/favicon.ico` is an HTTPS request, and an HTTPS client is a
//! certificate store, a TLS stack and a redirect policy — three dependencies
//! this project does not want for one file. WinINet is the HTTP client Windows
//! already ships: it follows redirects, uses the system proxy and the system
//! certificate store, and needs no crate at all.
//!
//! It also lets the **budget** be ours. Without `InternetSetOptionW` a dead host
//! is WinINet's own thirty-second default, on a thread the user is waiting on;
//! with it, a connection that has not answered in [`CONNECT_TIMEOUT`] is
//! abandoned, and so is a read that stalls. The body is capped at
//! [`MAX_BYTES`], so a server that answers with something enormous costs a
//! bounded buffer and nothing more.
//!
//! ## What is a failure
//!
//! Only a missing icon. A host that does not resolve, a site with no icon at any
//! of the candidate paths, an HTTP error, an empty body and a body over the cap
//! all answer an empty list, and the window draws the key's initial instead. A
//! site with no icon is the normal case on the open web, not an error to report.
//!
//! # Example
//!
//! ```no_run
//! // Every body that answered, best candidate first; empty when the site has
//! // no icon to give.
//! let bodies = ss550_tray::favicon::site_icons("example.com");
//! if let Some(icon) = bodies.first() {
//!     assert!(icon.len() <= ss550_tray::favicon::MAX_BYTES);
//! }
//! ```

/// The largest body this module accepts, in bytes.
///
/// A favicon is a handful of kilobytes. A few hundred kilobytes is generous for
/// a multi-resolution ICO and small enough that a server answering with
/// something else cannot fill memory: anything past it is not an icon, and the
/// key falls back to its initial.
pub const MAX_BYTES: usize = 256 * 1024;

/// How long the connection may take before the fetch is abandoned, in
/// milliseconds.
///
/// The window never waits on this — the fetch runs on a background thread — but
/// a dead host must not hold that thread for WinINet's own default.
const CONNECT_TIMEOUT: u32 = 3_000;

/// How long one send or one read may take before the fetch is abandoned, in
/// milliseconds.
///
/// A server that accepts the connection and then stops talking is the case this
/// exists for: the connect succeeded, and without this the read would wait
/// forever.
const READ_TIMEOUT: u32 = 3_000;

/// How many times WinINet may retry a refused connection.
///
/// Its default is five, with a pause between them, which turns a closed port
/// into four seconds of nothing. One attempt is enough: the icon is optional.
const CONNECT_ATTEMPTS: u32 = 1;

/// The paths a site may publish its icon at, best first.
///
/// The touch icons come before the tab icon because a key is 108 pixels and a
/// `favicon.ico` is a 16- or 32-pixel picture meant for a 16-pixel tab: the same
/// file on a key is blocks of colour. A site that publishes nothing but
/// `/favicon.ico` still works, which is why it is still asked.
const CANDIDATES: [&str; 3] = [
    "/apple-touch-icon.png",
    "/apple-touch-icon-precomposed.png",
    "/favicon.ico",
];

/// A body this large is already a real icon, so the rest are not asked for.
///
/// A pixel size is not readable from these bytes without decoding them, but a
/// 32×32 ICO is a few hundred bytes and an apple-touch-icon is a picture for a
/// home screen. Four kilobytes is far above the first and far below the second,
/// and being wrong costs one more request — `twitch.tv` answers a 524-byte
/// 180×180 PNG, so its next candidate is asked for even though the first was
/// already good.
const LARGE_BODY: usize = 4 * 1024;

/// Every icon body the site at `host` publishes, best candidate first.
///
/// `host` is a bare authority — `example.com`, `localhost:8080` — and the paths
/// asked are [`CANDIDATES`], in that order. Redirects are followed, so a site
/// that answers with a `301` to its real icon works.
///
/// The bodies are **as served**: an ICO, a PNG, or anything else the server
/// chose to put at that path — `twitch.tv` answers an HTML page at
/// `/apple-touch-icon-precomposed.png`. Deciding which of them is a picture, and
/// which is the biggest, is the caller's job, because the caller is the one with
/// a decoder. A caller that cannot decode any of them falls back to the key's
/// initial.
///
/// The list is empty when nothing answered: no DNS, no icon at any of the paths,
/// an HTTP error, an empty body, a body over [`MAX_BYTES`].
///
/// A host that cannot be a host — empty, with a slash in it, with a space in it
/// — is refused before any request is made.
pub fn site_icons(host: &str) -> Vec<Vec<u8>> {
    let mut bodies = Vec::new();
    for url in urls_for(host) {
        let Some(body) = platform::fetch(&url) else {
            // A path the site does not publish is the ordinary case: the next
            // candidate is the one that might answer.
            continue;
        };
        let large = body.len() >= LARGE_BODY;
        bodies.push(body);
        if large {
            break;
        }
    }
    bodies
}

/// The URLs an icon is fetched from, best candidate first, or none when `host`
/// is not a host.
///
/// Split out from the request so the rule is testable without a network: the
/// only thing that reaches WinINet is a string this function built.
fn urls_for(host: &str) -> Vec<String> {
    let host = host.trim();
    if !is_host(host) {
        return Vec::new();
    }
    CANDIDATES
        .iter()
        .map(|path| format!("https://{host}{path}"))
        .collect()
}

/// Whether `text` can be the authority of an HTTPS URL.
///
/// Deliberately narrow: letters, digits, dots, hyphens, underscores and one
/// colon for the port. A slash, a space, a query or a fragment would change the
/// request into one about something else, and a host made only of separators
/// names nothing.
fn is_host(text: &str) -> bool {
    if text.is_empty() || text.len() > 253 {
        return false;
    }
    if !text
        .chars()
        .any(|character| character.is_ascii_alphanumeric())
    {
        return false;
    }
    // One colon at most: it separates the port, and a second one would make the
    // authority ambiguous rather than specific.
    if text.chars().filter(|character| *character == ':').count() > 1 {
        return false;
    }
    text.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | ':' | '_')
    })
}

/// Everything that needs Win32.
#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use std::mem::size_of;

    use windows::core::PCWSTR;
    use windows::Win32::Networking::WinInet::{
        HttpQueryInfoW, InternetCloseHandle, InternetOpenUrlW, InternetOpenW, InternetReadFile,
        InternetSetOptionW, HTTP_QUERY_FLAG_NUMBER, HTTP_QUERY_STATUS_CODE,
        INTERNET_FLAG_NO_CACHE_WRITE, INTERNET_FLAG_RELOAD, INTERNET_OPEN_TYPE_PRECONFIG,
        INTERNET_OPTION_CONNECT_RETRIES, INTERNET_OPTION_CONNECT_TIMEOUT,
        INTERNET_OPTION_RECEIVE_TIMEOUT, INTERNET_OPTION_SEND_TIMEOUT,
    };

    use super::{CONNECT_ATTEMPTS, CONNECT_TIMEOUT, MAX_BYTES, READ_TIMEOUT};

    /// What WinINet is told the client is.
    ///
    /// A normal user agent, because a site that sees a bare `wininet` or an empty
    /// string is entitled to answer `403` — and a `403` is indistinguishable
    /// from "no icon here".
    const AGENT: &str = "Mozilla/5.0 (compatible; StreamCraft/0.1)";

    /// The body of `url`, or `None` for every answer that is not a body: a
    /// failed connection, an HTTP error, an empty body, a body over the cap.
    ///
    /// Every handle opened here is closed before the function returns, on every
    /// path: [`Handle`] closes on drop, and the request is declared after the
    /// session so it is closed first.
    pub(super) fn fetch(url: &str) -> Option<Vec<u8>> {
        let agent = wide(AGENT);
        let target = wide(url);

        // SAFETY: both strings are live, nul-terminated UTF-16 buffers, and the
        // access type is the system default (the user's proxy settings). The
        // returned handle is null on failure, which is checked before use and
        // closed by its guard on every path out.
        let session = Handle(unsafe {
            InternetOpenW(
                PCWSTR(agent.as_ptr()),
                INTERNET_OPEN_TYPE_PRECONFIG.0,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            )
        });
        if session.0.is_null() {
            return None;
        }

        // The budget, set on the session so every request made with it inherits
        // it. Without these calls a dead host is WinINet's own default timeout,
        // which is far longer than a key's face is worth waiting for.
        set_option(session.0, INTERNET_OPTION_CONNECT_TIMEOUT, CONNECT_TIMEOUT);
        set_option(session.0, INTERNET_OPTION_SEND_TIMEOUT, READ_TIMEOUT);
        set_option(session.0, INTERNET_OPTION_RECEIVE_TIMEOUT, READ_TIMEOUT);
        // One attempt, not WinINet's five: a refused port must not cost seconds.
        set_option(session.0, INTERNET_OPTION_CONNECT_RETRIES, CONNECT_ATTEMPTS);

        // SAFETY: `session` is a live handle, `target` is a live nul-terminated
        // URL, and no extra headers are sent — which also means no
        // `Accept-Encoding`, so the body arrives in the encoding the server
        // stores it in. Redirects are followed: `INTERNET_FLAG_NO_AUTO_REDIRECT`
        // is deliberately not among the flags. The handle is null on failure and
        // is closed by its guard.
        let request = Handle(unsafe {
            InternetOpenUrlW(
                session.0,
                PCWSTR(target.as_ptr()),
                None,
                INTERNET_FLAG_RELOAD | INTERNET_FLAG_NO_CACHE_WRITE,
                None,
            )
        });
        if request.0.is_null() {
            return None;
        }

        // An HTTP error is a page, not an icon: WinINet hands over the error
        // body as if it were content, so the status is what separates the two.
        if status_of(request.0) != Some(200) {
            return None;
        }

        read_body(request.0)
    }

    /// The HTTP status of an open request, when WinINet reports one.
    fn status_of(request: *mut c_void) -> Option<u32> {
        let mut status: u32 = 0;
        let mut length = size_of::<u32>() as u32;

        // SAFETY: `request` is a live handle, the buffer is a live `u32` and its
        // length is that type's own size, and the level asks for the status code
        // as a number rather than as text.
        let queried = unsafe {
            HttpQueryInfoW(
                request,
                HTTP_QUERY_STATUS_CODE | HTTP_QUERY_FLAG_NUMBER,
                Some((&mut status as *mut u32).cast()),
                &mut length,
                None,
            )
        };
        queried.ok().map(|()| status)
    }

    /// The body of an open request, up to [`MAX_BYTES`].
    ///
    /// A body over the cap is refused rather than truncated: half an icon is not
    /// an icon, and a decoder handed a prefix would spend time failing.
    fn read_body(request: *mut c_void) -> Option<Vec<u8>> {
        let mut body = Vec::new();
        let mut chunk = [0u8; 8 * 1024];

        loop {
            let mut read: u32 = 0;
            // SAFETY: `request` is a live handle and `chunk` is a live buffer of
            // the length passed with it; `read` receives how many bytes were
            // written into it.
            let filled = unsafe {
                InternetReadFile(
                    request,
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    &mut read,
                )
            };
            if filled.is_err() {
                return None;
            }
            // A successful read of nothing is the end of the body.
            let read = read as usize;
            if read == 0 {
                break;
            }
            if body.len() + read > MAX_BYTES {
                return None;
            }
            body.extend_from_slice(&chunk[..read]);
        }

        // An empty body is a site with no icon, not an icon of no pixels.
        (!body.is_empty()).then_some(body)
    }

    /// Sets one numeric WinINet option on `handle`.
    ///
    /// A refused option is not fatal: the fetch then runs on WinINet's own
    /// budget, which is slow but correct, and the caller never sees the
    /// difference.
    fn set_option(handle: *mut c_void, option: u32, value: u32) {
        // SAFETY: `handle` is a live session handle, and the buffer is a live
        // `u32` whose length is passed with it.
        let _ = unsafe {
            InternetSetOptionW(
                Some(handle),
                option,
                Some((&value as *const u32).cast()),
                size_of::<u32>() as u32,
            )
        };
    }

    /// A nul-terminated UTF-16 copy of `text`, for a Win32 call.
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A WinINet handle that is closed when it goes out of scope.
    ///
    /// Every early return in [`fetch`] is a path that must not leak the session
    /// or the request, and a guard is the only shape that cannot forget one.
    struct Handle(*mut c_void);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the handle was opened by this module, is non-null, and
                // is not used again after this drop.
                let _ = unsafe { InternetCloseHandle(self.0) };
            }
        }
    }
}

/// Everywhere but Windows there is no WinINet to ask.
#[cfg(not(windows))]
mod platform {
    pub(super) fn fetch(_url: &str) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{is_host, site_icons, urls_for, LARGE_BODY, MAX_BYTES};

    #[test]
    fn a_host_is_turned_into_the_candidates_the_convention_names() {
        // The touch icons are the mark a phone's home screen draws, and the tab
        // icon is what a 16-pixel tab draws: the order is the whole point.
        assert_eq!(
            urls_for("example.com"),
            [
                "https://example.com/apple-touch-icon.png",
                "https://example.com/apple-touch-icon-precomposed.png",
                "https://example.com/favicon.ico",
            ]
        );
        // A port is part of the host: `localhost:8080` is not `localhost`.
        assert_eq!(
            urls_for("localhost:8080"),
            [
                "https://localhost:8080/apple-touch-icon.png",
                "https://localhost:8080/apple-touch-icon-precomposed.png",
                "https://localhost:8080/favicon.ico",
            ]
        );
        // A pasted value often carries the space that came with it.
        assert_eq!(
            urls_for("  example.com  ").first().map(String::as_str),
            Some("https://example.com/apple-touch-icon.png")
        );
    }

    #[test]
    fn a_value_that_is_not_a_host_never_becomes_a_request() {
        // A slash would move the request to another path, a space would end the
        // URL, and a host of separators names nothing at all.
        for text in [
            "",
            "   ",
            "/",
            "..",
            "example.com/other",
            "exa mple.com",
            "?q=1",
            "a:b:c",
        ] {
            assert!(urls_for(text).is_empty(), "{text:?} is not a host");
            assert!(!is_host(text), "{text:?} is not a host");
        }
        assert!(site_icons("").is_empty(), "nothing is fetched for nothing");
    }

    #[test]
    fn a_body_over_the_cap_is_refused_rather_than_kept() {
        // The number itself is the contract: a caller that persists what comes
        // back can size its own buffer from it. It is far above a real icon and
        // far below anything a page could use to exhaust memory.
        let cap = MAX_BYTES as u64;
        assert_eq!(cap, 262_144);
        assert!(cap > 65_536, "a multi-resolution icon must fit");
        assert!(cap < 1_048_576, "an icon is not a download");
    }

    #[test]
    fn the_early_stop_sits_above_a_tab_icon_and_below_a_home_screen_mark() {
        // Measured answers: `/favicon.ico` at x.com is 549 bytes of 32×32 PNG,
        // GitHub's is 6518 bytes of ICO whose largest entry is 32×32, and the
        // apple-touch-icon at x.com is 13160 bytes of 1024×1024. The threshold
        // must sit between the tab icons and the touch icons, or the first
        // candidate would be abandoned for a worse one.
        assert_eq!(LARGE_BODY, 4096);
        // Bound through a cast, the way the cap test does, so the comparison is
        // a value clippy does not fold into a constant assertion.
        let stop = LARGE_BODY as u64;
        assert!(stop > 549, "a 32×32 tab icon is smaller than this");
        assert!(stop < 13_160, "a 1024×1024 touch icon is larger than this");
        assert!(stop < MAX_BYTES as u64, "the stop is not the cap");
    }
}
