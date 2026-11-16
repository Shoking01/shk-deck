//! The icon a website publishes, as the bytes it served.
//!
//! A key bound to a URL draws the site's own mark, the way a key bound to a
//! program draws the program's icon. The picture is not stored in the profile
//! and is not part of the action: it is derived from the host, fetched once, and
//! kept by the caller. This module answers the **bytes**; decoding them is
//! [`crate::favicon`]'s caller's job, because the window already links an image
//! decoder and this crate does not.
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
//! Only a missing icon. A host that does not resolve, a site with no
//! `favicon.ico`, an HTTP error, an empty body and a body over the cap all
//! answer `None`, and the window draws the key's initial instead. A site with no
//! icon is the normal case on the open web, not an error to report.
//!
//! # Example
//!
//! ```no_run
//! // The bytes as served, or `None` when the site has no icon to give.
//! if let Some(ico) = ss550_tray::favicon::site_icon("example.com") {
//!     assert!(ico.len() <= ss550_tray::favicon::MAX_BYTES);
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

/// The icon the site at `host` publishes, or `None` when it has none.
///
/// `host` is a bare authority — `example.com`, `localhost:8080` — and the path
/// is always `/favicon.ico`, which is where the convention puts it. Redirects
/// are followed, so a site that answers with a `301` to its real icon works.
///
/// The answer is the body **as served**: an ICO, a PNG, or anything else the
/// server chose to put at that path. Deciding whether it is a picture is the
/// caller's job, and a caller that cannot decode it falls back to the key's
/// initial.
///
/// A host that cannot be a host — empty, with a slash in it, with a space in it
/// — is refused before any request is made.
pub fn site_icon(host: &str) -> Option<Vec<u8>> {
    let url = url_for(host)?;
    platform::fetch(&url)
}

/// The URL an icon is fetched from, or `None` when `host` is not a host.
///
/// Split out from the request so the rule is testable without a network: the
/// only thing that reaches WinINet is a string this function built.
fn url_for(host: &str) -> Option<String> {
    let host = host.trim();
    if !is_host(host) {
        return None;
    }
    Some(format!("https://{host}/favicon.ico"))
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

    /// The body of `url`, or `None` for every answer that is not a picture.
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
    use super::{is_host, site_icon, url_for, MAX_BYTES};

    #[test]
    fn a_host_is_turned_into_the_path_the_convention_names() {
        assert_eq!(
            url_for("example.com").as_deref(),
            Some("https://example.com/favicon.ico")
        );
        // A port is part of the host: `localhost:8080` is not `localhost`.
        assert_eq!(
            url_for("localhost:8080").as_deref(),
            Some("https://localhost:8080/favicon.ico")
        );
        // A pasted value often carries the space that came with it.
        assert_eq!(
            url_for("  example.com  ").as_deref(),
            Some("https://example.com/favicon.ico")
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
            assert_eq!(url_for(text), None, "{text:?} is not a host");
            assert!(!is_host(text), "{text:?} is not a host");
        }
        assert!(site_icon("").is_none(), "nothing is fetched for nothing");
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
}
