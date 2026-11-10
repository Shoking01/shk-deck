//! Is a 1025-byte output report *slow* or *impossible*?
//!
//! `hidapi` 2.6.7 waits a hardcoded 1000 ms for the overlapped write to
//! complete (`windows_native/mod.rs:152`) and exposes no setter, so a device
//! slower than 1 s is indistinguishable from a dead endpoint. This example
//! opens the same HID path itself and waits up to 10 s, reporting the real
//! completion time.
//!
//! Run: `cargo run -p ss550-probe --example raw_write`
//!
//! Windows only: the timeout being measured lives in `hidapi`'s Windows
//! backend, so the probe calls `CreateFileW` and `WriteFile` itself. On any
//! other platform `main` says so and exits, which keeps the example — and
//! therefore `cargo test --workspace`, which builds examples — compiling on
//! every platform CI checks.

#![cfg_attr(windows, allow(unsafe_code))]

#[cfg(windows)]
mod win32 {
    use std::mem::zeroed;
    use std::ptr::{null, null_mut};
    use std::time::Instant;

    use hidapi::HidApi;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, WriteFile};
    use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

    const VID: u16 = 0x0200;
    const PID: u16 = 0x3000;
    const VENDOR_USAGE_PAGE: u16 = 0xffa0;

    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
    const ERROR_IO_PENDING: i32 = 997;
    const INVALID_HANDLE: isize = -1;

    /// One overlapped write; returns the outcome and the elapsed milliseconds.
    fn raw_write(handle: HANDLE, pack: &[u8], timeout_ms: u32) -> (bool, u128, String) {
        let start = Instant::now();
        unsafe {
            let mut ol: OVERLAPPED = zeroed();
            ol.hEvent = CreateEventW(null(), 1, 0, null());
            if ol.hEvent as isize == INVALID_HANDLE || ol.hEvent.is_null() {
                return (false, 0, "CreateEventW failed".into());
            }

            let submitted = WriteFile(
                handle,
                pack.as_ptr(),
                pack.len() as u32,
                null_mut(),
                &mut ol,
            );

            if submitted == 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() != Some(ERROR_IO_PENDING) {
                    CloseHandle(ol.hEvent);
                    return (
                        false,
                        start.elapsed().as_millis(),
                        format!("WriteFile: {err}"),
                    );
                }
            }

            let wait = WaitForSingleObject(ol.hEvent, timeout_ms);
            let elapsed = start.elapsed().as_millis();
            let mut done: u32 = 0;
            let ok = GetOverlappedResult(handle, &ol, &mut done, 0);
            if ok == 0 {
                let err = std::io::Error::last_os_error();
                CancelIoEx(handle, &ol);
                CloseHandle(ol.hEvent);
                return (
                    false,
                    elapsed,
                    format!("wait={wait} completed={done} err={err}"),
                );
            }
            CloseHandle(ol.hEvent);
            (true, elapsed, format!("wait={wait} completed={done}"))
        }
    }

    fn crt(tag: &[u8; 3], fields: &[(usize, u8)]) -> Vec<u8> {
        let mut buf = vec![0u8; 1025];
        buf[1..4].copy_from_slice(b"CRT");
        buf[6..9].copy_from_slice(tag);
        for (off, v) in fields {
            buf[1 + off] = *v;
        }
        buf
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let api = HidApi::new()?;
        let devices: Vec<_> = api
            .device_list()
            .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
            .collect();
        let info = devices
            .iter()
            .find(|d| d.usage_page() == VENDOR_USAGE_PAGE)
            .or_else(|| devices.first())
            .ok_or("device not found")?;

        let path = info.path().to_string_lossy().to_string();
        println!("path={path}");
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                null_mut(),
            )
        };
        if handle as isize == INVALID_HANDLE || handle.is_null() {
            return Err(format!("CreateFileW failed: {}", std::io::Error::last_os_error()).into());
        }

        // DIS first: this is the wake the firmware expects before anything else.
        for (name, pack) in [
            ("DIS", crt(b"DIS", &[])),
            ("LIG100", crt(b"LIG", &[(10, 100)])),
        ] {
            let (ok, elapsed, detail) = raw_write(handle, &pack, 10_000);
            println!("{name:<7} ok={ok} elapsed={elapsed}ms {detail}");
        }

        unsafe { CloseHandle(handle) };
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    win32::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!(
        "raw_write opens the HID path through Win32 to measure hidapi's 1000 ms write timeout; \
         it only runs on Windows."
    );
}
