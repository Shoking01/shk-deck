//! One-shot live-device probe harness for the Redragon StreamCraft SS550.
//!
//! Fase 0 probes:
//!
//! * P2 (G1, default): enumerate matching HID interfaces and log report
//!   lengths. Read-only, no writes.
//! * P3 (`--listen <ms>`): passive listener — open every matching interface
//!   and read it for `<ms>`. NEVER writes to the device.
//! * P4 (`--descriptor`): read-only dump + scan of the HID report descriptor.
//! * P1B (`--han`), P5 (`--diff`), P6 (`--wake`), P7 (`--ver`): write probes,
//!   one per session at most.

#![deny(unsafe_code)]

use std::ffi::CStr;
use std::ffi::OsString;
use std::process::ExitCode;
use std::time::Duration;
use std::time::Instant;

use hidapi::HidApi;

/// Vendor/product IDs for the SS550 v3 (`0200:3000`).
const VID: u16 = 0x0200;
/// Vendor/product IDs for the SS550 v3 (`0200:3000`).
const PID: u16 = 0x3000;
/// Usage page of the SS550's writable vendor HID interface (MI_00), observed
/// live: `usage_page=0xffa0`, `usage=0x0001`.
const SS550_VENDOR_USAGE_PAGE: u16 = 0xffa0;
/// Read timeout (ms) for the first P1B read.
const READ_TIMEOUT_FIRST_MS: i32 = 3000;
/// Read timeout (ms) for the P1B second read after the sleep.
const READ_TIMEOUT_SECOND_MS: i32 = 500;
/// Sleep (ms) between the P1B first and second reads.
const SLEEP_BETWEEN_READS_MS: u64 = 3000;
/// Number of leading bytes logged per read buffer.
const LOGGED_BYTES: usize = 32;
/// Per-interface read timeout (ms) used by the P3 passive listener.
const LISTEN_POLL_MS: i32 = 100;
/// Read timeout (ms) after each differential probe write.
const DIFF_READ_TIMEOUT_MS: i32 = 2000;
/// Pause (ms) between differential probes so the device settles.
const DIFF_GAP_MS: u64 = 200;

/// Usage text printed on `-h`/`--help` and on invalid arguments.
const USAGE: &str = "usage: ss550-probe [--upload | --all | --key <0-14> | --profile | --init]\n\
                     \x20      [--bright <0-100>] [--ver] [--wake] [--diff] [--han]\n\
                     \x20      [--descriptor] [--listen <ms>] [-h|--help]\n\
                     \n\
                     One-shot Fase 0 live-device probe harness for the Redragon\n\
                     StreamCraft SS550 (VID 0x0200 / PID 0x3000, 15 LCD buttons).\n\
                     \n\
                     Default (no flags): P2 (G1) — enumerate matching devices and\n\
                     log report lengths. Read-only, no writes.\n\
                     \n\
                     --upload: write a solid white 100x100 JPEG to one key using the\n\
                     mirajazz sequence: CRT|DIS, CRT|LIG 0, then CRT|BAT + chunks\n\
                     per key, then one CRT|STP.\n\
                     --all: same as --upload but writes all 15 keys with a distinct\n\
                     colour each, so the physical key order is readable.\n\
                     --key <0-14>: device key index for --upload (default 0).\n\
                     --profile: wake + handshake + a full insertSettings-style push\n\
                     (ICO 0x05 + JPEG, ICO 0x0F + JSON, ULEND).\n\
                     --init: wake + HAN + listen windows around a key-0 upload.\n\
                     --bright <0-100>: single CRT|LIG brightness write.\n\
                     --ver: request the firmware identity string.\n\
                     --wake: send the wake/handshake probe table once.\n\
                     --diff: differential write/read probe over control transfer.\n\
                     --han: send ONE verified HAN pack then perform the P1B reads.\n\
                     --descriptor: decode the HID report descriptor. Read-only.\n\
                     --listen <ms>: passive interrupt-IN listener. Read-only.\n\
                     \n\
                     Exactly one write probe per session; read-only flags never\n\
                     combine with a write probe.\n";

/// Fifteen visually distinct solid colours, one per wire key in order.
///
/// A white upload proves the image path works but is symmetric, so it cannot
/// distinguish key indices. Distinct colours per key make the physical key
/// order readable in a single write probe, which is the only thing left to
/// prove for the plain-image path.
const KEY_COLORS: [u32; 15] = [
    0xFF_0000, // 1  red
    0xFF_6A00, // 2  dark orange
    0xFF_D500, // 3  yellow
    0x8C_FF00, // 4  lime
    0x00_C853, // 5  green
    0x00_BFA5, // 6  teal
    0x00_E5FF, // 7  cyan
    0x29_79FF, // 8  blue
    0x30_4FFE, // 9  indigo
    0x62_00EA, // 10 violet
    0xD5_00F9, // 11 magenta
    0xFF_4081, // 12 pink
    0x8D_6E63, // 13 brown
    0x9E_9E9E, // 14 gray
    0xFF_FFFF, // 15 white
];

/// Sends each colour to each wire key in sequence, proving key index mapping.
/// OpenDeck's `opendeck_to_device` array says: [10,11,12,13,14,5,6,7,8,9,0,1,2,3,4].
/// We send 1-based wire keys 1..15 and watch where each colour lands.
fn run_key_proof(
    dev: &hidapi::HidDevice,
    _n_in: usize,
    n_out: usize,
    keys: Vec<u8>,
) -> Result<(), String> {
    let total = if n_out > 0 { n_out } else { FALLBACK_TOTAL };
    let chunk = total - 1;
    let width: u16 = 100;
    let height: u16 = 100;

    // Single solid red image for every key.
    let mut rgb = vec![0u8; width as usize * height as usize * 3];
    for px in rgb.chunks_exact_mut(3) {
        px[0] = 255;
        px[1] = 0;
        px[2] = 0;
    }
    let mut image_jpeg = Vec::<u8>::new();
    jpeg_encoder::Encoder::new(&mut image_jpeg, 90)
        .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
        .map_err(|e| format!("error= JPEG encode failed: {e}"))?;
    let image_len = image_jpeg.len();
    let size_hi = (image_len >> 8) as u8;
    let size_lo = (image_len & 0xff) as u8;
    println!("proof jpeg_bytes={image_len}");

    // Wake + brightness 0
    let wake = build_probe_pack(
        &Probe {
            name: "wake",
            marker: *b"CRT",
            data_type: 0x00,
            tag: Some(*b"DIS"),
            fields: &[],
        },
        total,
    );
    dev.write(&wake)
        .map_err(|e| format!("error= wake write failed: {e}"))?;
    let lig0 = build_probe_pack(
        &Probe {
            name: "lig0",
            marker: *b"CRT",
            data_type: 0x00,
            tag: Some(*b"LIG"),
            fields: &[(10, 0x00u8)],
        },
        total,
    );
    dev.write(&lig0)
        .map_err(|e| format!("error= lig0 write failed: {e}"))?;

    // Send red to selected keys using OpenDeck's mapping
    let fw_keys: Vec<u8> = if keys.is_empty() {
        (0..15).collect()
    } else {
        keys.iter().map(|k| opendeck_to_wire_key(*k)).collect()
    };
    for (i, &key_idx) in fw_keys.iter().enumerate() {
        let bat = build_probe_pack(
            &Probe {
                name: "bat",
                marker: *b"CRT",
                data_type: 0x00,
                tag: Some(*b"BAT"),
                fields: &[(10, size_hi), (11, size_lo), (12, key_idx)],
            },
            total,
        );
        println!("proof slot={i} firmware_key={key_idx} color=#FF0000");
        dev.write(&bat)
            .map_err(|e| format!("error= bat write for key {i} failed: {e}"))?;
        for (c, bytes) in image_jpeg.chunks(chunk).enumerate() {
            let mut pack = vec![0u8; total];
            pack[0] = 0x00;
            pack[1..1 + bytes.len()].copy_from_slice(bytes);
            dev.write(&pack)
                .map_err(|e| format!("error= key {i} chunk {c} write failed: {e}"))?;
        }
    }
    let stp = build_probe_pack(
        &Probe {
            name: "stp",
            marker: *b"CRT",
            data_type: 0x00,
            tag: Some(*b"STP"),
            fields: &[],
        },
        total,
    );
    dev.write(&stp)
        .map_err(|e| format!("error= stp write failed: {e}"))?;

    // Restore brightness
    let lig100 = build_probe_pack(
        &Probe {
            name: "lig100",
            marker: *b"CRT",
            data_type: 0x00,
            tag: Some(*b"LIG"),
            fields: &[(10, 100u8)],
        },
        total,
    );
    dev.write(&lig100)
        .map_err(|e| format!("error= lig100 write failed: {e}"))?;

    println!("proof_result=done");
    Ok(())
}

/// OpenDeck key-to-wire key mapping from the official plugin.
/// This is the same array used in `opendeck-ss550/src/inputs.rs`.
fn opendeck_to_wire_key(key: u8) -> u8 {
    // The firmware uses 1-based keys 1..15; key 0 is a placeholder.
    // Map OpenDeck's 0..14 to firmware keys 1..15 via the plugin's table.
    const PLUGIN_MAP: [u8; 15] = [10, 11, 12, 13, 14, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4];
    let fw_key = PLUGIN_MAP[key as usize];
    // Firmware keys are 1-based; the plugin uses 0 as "all keys" placeholder
    // For key 0 (plugin's placeholder), use firmware key 10 (first real key).
    if fw_key == 0 {
        10
    } else {
        fw_key
    }
}

/// Checks the key_index variable exists and defaults to 0.
///
/// On Windows, opens the device path with `CreateFileW` + `HidD_GetPreparsedData`
/// and reads `OutputReportByteLength` from `HIDP_CAPS` via `HidP_GetCaps` in
/// `hid.dll`. Returns `(n_in_total, n_out_total)` in the hidapi convention
/// (total bytes including any report-ID byte). `(0, 0)` means the caps query
/// was unavailable and is reported verbatim, never mocked. Only this function
/// carries `#[allow(unsafe_code)]`; the rest of the crate stays under `deny`.
#[cfg(windows)]
#[allow(unsafe_code)]
fn read_report_lengths_windows(path: &CStr) -> (usize, usize) {
    use std::mem::MaybeUninit;

    use windows_sys::Win32::Devices::HumanInterfaceDevice::{
        HidD_FreePreparsedData, HidD_GetPreparsedData, HidP_GetCaps, HIDP_CAPS,
        HIDP_STATUS_SUCCESS, PHIDP_PREPARSED_DATA,
    };
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let path_str = match path.to_str() {
        Ok(s) => s,
        Err(_) => {
            eprintln!("caps_error= path not representable as UTF-8");
            return (0, 0);
        }
    };

    let buf16: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();

    // Open with the same flags hidapi uses (`GENERIC_READ | GENERIC_WRITE`,
    // `FILE_FLAG_OVERLAPPED`): opening with access 0 — as the fallback for
    // system devices — yields a preparsed-data blob that `HidP_GetCaps`
    // rejects. hidapi also opens read-write first and only falls back to
    // access 0 for system devices (keyboards), so try RW first.
    let handle: HANDLE = unsafe {
        CreateFileW(
            buf16.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            core::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            core::ptr::null_mut(),
        )
    };

    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        eprintln!("caps_error= CreateFileW failed last_error={err}");
        return (0, 0);
    }

    unsafe {
        let mut pdata: PHIDP_PREPARSED_DATA = 0;
        if !HidD_GetPreparsedData(handle, &mut pdata) {
            let err = windows_sys::Win32::Foundation::GetLastError();
            eprintln!("caps_error= HidD_GetPreparsedData failed last_error={err}");
            CloseHandle(handle);
            return (0, 0);
        }

        let mut caps = MaybeUninit::<HIDP_CAPS>::zeroed();
        let status = HidP_GetCaps(pdata, caps.as_mut_ptr());
        // HidP_GetCaps returns HIDP_STATUS_SUCCESS (0x00110000) on success,
        // NOT 0. Anything else is a real failure.
        let (n_in, n_out) = if status == HIDP_STATUS_SUCCESS {
            let c = caps.assume_init();
            (
                c.InputReportByteLength as usize,
                c.OutputReportByteLength as usize,
            )
        } else {
            let err = windows_sys::Win32::Foundation::GetLastError();
            eprintln!("caps_error= HidP_GetCaps status={status} last_error={err}");
            (0, 0)
        };

        HidD_FreePreparsedData(pdata);
        CloseHandle(handle);
        (n_in, n_out)
    }
}

/// Formats `buf[..]` as a space-free lowercase hex string limited to `limit` bytes.
fn hex_prefix(buf: &[u8], limit: usize) -> String {
    let n = buf.len().min(limit);
    let mut s = String::with_capacity(n * 2);
    for b in &buf[..n] {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Checks whether any read payload contains the ASCII bytes `HANDLER`.
fn contains_handler(buf: &[u8]) -> bool {
    contains_ascii(buf, b"HANDLER")
}

/// Checks whether `needle` appears anywhere in `buf`.
fn contains_ascii(buf: &[u8], needle: &[u8]) -> bool {
    !buf.is_empty() && !needle.is_empty() && buf.windows(needle.len()).any(|w| w == needle)
}

/// Renders the first `limit` bytes of `buf` as printable ASCII (`.` for the rest).
fn ascii_prefix(buf: &[u8], limit: usize) -> String {
    buf[..buf.len().min(limit)]
        .iter()
        .map(|b| {
            if b.is_ascii_graphic() || *b == b' ' {
                *b as char
            } else {
                '.'
            }
        })
        .collect()
}

/// Builds the single verified `HAN` pack of total `total` bytes (spec P1B).
///
/// `pack[0] = 0x00` (report ID), `pack[1..4] = "HAN"`, zero-padded otherwise.
fn build_han_pack(total: usize) -> Vec<u8> {
    let mut pack = vec![0u8; total];
    if total >= 4 {
        pack[0] = 0x00;
        pack[1..4].copy_from_slice(b"HAN");
    } else if !pack.is_empty() {
        pack[0] = 0x00;
    }
    pack
}

/// On non-Windows targets, reports the caps as unavailable.
#[cfg(not(windows))]
fn read_report_lengths(path: &CStr) -> (usize, usize) {
    let _ = path;
    (0, 0)
}

/// Dispatches to the platform-specific helper.
#[cfg(windows)]
fn read_report_lengths(path: &CStr) -> (usize, usize) {
    read_report_lengths_windows(path)
}

/// One differential probe: a documented command pack from the DLL framer.
///
/// Wire layout: byte 0 is the (implicit) report ID `0x00`, then the payload.
/// Payload framing recovered from `SDLibrary1.dll` framers:
/// `payload[0..3]` = 3-char marker (`"CRT"` for data commands, `"HAN"` for the
/// handshake), `payload[4]` = variant/dataType, `payload[5..8]` = 3-char command
/// tag (CRT commands only).
struct Probe<'a> {
    name: &'static str,
    marker: [u8; 3],
    data_type: u8,
    tag: Option<[u8; 3]>,
    fields: &'a [(usize, u8)],
}

/// The differential command set. Every entry is a command observed in the
/// official framers — no invented opcodes. Each varies exactly one thing
/// against the previous probe so a response (or its absence) is attributable.
const PROBES: &[Probe] = &[
    // Control: reproduces the failed P1B exactly (bare HAN, variant 0).
    Probe {
        name: "han_bare_v0",
        marker: *b"HAN",
        data_type: 0x00,
        tag: None,
        fields: &[],
    },
    // Hypothesis 1: the variant byte carries the dataType (0x0f for HAN).
    Probe {
        name: "han_bare_v0f",
        marker: *b"HAN",
        data_type: 0x0f,
        tag: None,
        fields: &[],
    },
    // Hypothesis 2: the handshake uses CRT framing with HAN as its tag.
    Probe {
        name: "han_crt",
        marker: *b"CRT",
        data_type: 0x0f,
        tag: Some(*b"HAN"),
        fields: &[],
    },
    // Documented CLE command (addClearCommand, dataType 0x0b, [11]=key),
    // key = 0.
    Probe {
        name: "cle_key0",
        marker: *b"CRT",
        data_type: 0x0b,
        tag: Some(*b"CLE"),
        fields: &[(11, 0x00)],
    },
    // Same command, key = 1: differential on the key field position.
    Probe {
        name: "cle_key1",
        marker: *b"CRT",
        data_type: 0x0b,
        tag: Some(*b"CLE"),
        fields: &[(11, 0x01)],
    },
    // Documented MOD command (addModeChangedCommand, dataType 0x0e, [10]=mode).
    Probe {
        name: "mod_mode0",
        marker: *b"CRT",
        data_type: 0x0e,
        tag: Some(*b"MOD"),
        fields: &[(10, 0x00)],
    },
    // Same command, mode = 1: differential on the mode field position.
    Probe {
        name: "mod_mode1",
        marker: *b"CRT",
        data_type: 0x0e,
        tag: Some(*b"MOD"),
        fields: &[(10, 0x01)],
    },
];

/// Builds one probe pack of exactly `total` bytes.
fn build_probe_pack(p: &Probe, total: usize) -> Vec<u8> {
    let mut pack = vec![0u8; total];
    // Wire byte 0 is the report ID.
    pack[0] = 0x00;
    // Payload begins at wire byte 1.
    let payload_len = total.saturating_sub(1);
    if payload_len >= 12 {
        let payload = &mut pack[1..];
        payload[..3].copy_from_slice(&p.marker);
        payload[4] = p.data_type;
        if let Some(t) = &p.tag {
            payload[5..8].copy_from_slice(t);
        }
        for (off, v) in p.fields {
            if *off < payload.len() {
                payload[*off] = *v;
            }
        }
    }
    pack
}

/// Reads one reply over the **control-transfer** path.
///
/// This is how the device actually answers: `hid_get_input_report`
/// (`getHardwareFirmwareVersion` @ `0x180019800`) with `buf[0]` holding the
/// report ID. The buffer must be sized to the OUTPUT report length — a
/// 513-byte input-sized buffer fails with `ERROR_GEN_FAILURE` while the
/// 1025-byte one returns data. Returns `(bytes_read, read_ascii)`.
fn read_reply_control(dev: &hidapi::HidDevice, out_len: usize) -> (usize, String) {
    let mut buf = vec![0u8; out_len];
    buf[0] = 0x00; // the report ID, required before the call
    match dev.get_input_report(&mut buf) {
        Ok(n) => (n, ascii_prefix(&buf[..n.min(buf.len())], 24)),
        Err(_) => (0, String::new()),
    }
}

/// P9 brightness probe — the least-invasive visible write to the deck.
///
/// Payload recovered from `SDDevice::sendAdjustBrightnessPack` @ `0x18001cd30`
/// (direct twin of the queued `addAdjustBrightnessPack` @ `0x1800155e0`):
/// `[0..2]=CRT`, `[3..4]=0`, `[5..7]=LIG`, `[8..9]=0`, `[10]=clamp(value,0,100)`.
/// Delivered via `writeData` -> `hid_write`. Value is the slider's 0–100 int.
fn run_brightness(dev: &hidapi::HidDevice, n_out: usize, value: u8) -> Result<(), String> {
    let total = if n_out > 0 { n_out } else { FALLBACK_TOTAL };
    let clamped = value.clamp(0, 100);
    let pack = build_probe_pack(
        &Probe {
            name: "lig",
            marker: *b"CRT",
            data_type: 0x00,
            tag: Some(*b"LIG"),
            fields: &[(10, clamped)],
        },
        total,
    );
    println!(
        "brightness wrote={} value={clamped}",
        dev.write(&pack)
            .map_err(|e| format!("error= brightness write failed: {e}"))?
    );
    std::thread::sleep(Duration::from_millis(DIFF_GAP_MS));
    let mut buf = vec![0u8; total];
    buf[0] = 0x00;
    match dev.get_input_report(&mut buf) {
        Ok(n) => println!(
            "brightness identity_bytes={n} ascii={:?}",
            ascii_prefix(&buf[..n.min(buf.len())], 24)
        ),
        Err(e) => println!("brightness get_input_report_error={e}"),
    }
    println!("brightness_result=done value={clamped}");
    Ok(())
}

/// P10 full-session probe — replicates the app's connect-time sequence and
/// drives the ACK exchange the firmware expects.
///
/// App order (writeDataToHidDevice + readDataFromHidDevice + EXE dispatcher):
/// 1. `sendWakeUpScreenPack` 0x18001ecf0 -> `CRT|DIS`  (wakes sleep flag)
/// 2. `sendHandshakePack`    0x18001d650 -> `HAN`     (via writeData)
/// 3. 10 s timer send keepalive `CRT|00|CONNECT`      (first connect only)
/// 4. Read loop matches `HANDLER` / `ACKTF` / `ACK..CB..OK` / `CRTMOD` /
///    `V1./V2./V3.`
/// 5. On `HANDLER` while idle -> `sendHandshakePack`  (the ack-of-ack)
/// 6. On connect the EXE then pushes the profile via `insertSettings`.
///
/// This probe performs steps 1-5 and then uploads a solid white image to
/// key 0 using the documented ICO -> chunks -> ABT -> STP order, closing with
/// the 1000 ms wait the app performs for type 5 on model branches it matches.
fn run_full_session(
    dev: &hidapi::HidDevice,
    n_in: usize,
    n_out: usize,
    key: u8,
) -> Result<(), String> {
    let total = if n_out > 0 { n_out } else { FALLBACK_TOTAL };
    let buf_len = if n_in > 0 { n_in } else { FALLBACK_TOTAL };

    // Step 1 — wake.
    let wake = build_session_pack("DIS", 0x00, total, &[]);
    println!("init wake wrote={}", dev.write(&wake).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(200));

    // Step 2 — handshake (HAN). The FIRST HAN is unconditional; the app sends
    // HANDLER replies back through `sendHandshakePack`.
    let han = build_session_pack("HAN", 0x00, total, &[]);
    println!("init han wrote={}", dev.write(&han).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(300));

    // Step 3 — listen on interrupt-IN for 3 s. The app's read loop matches:
    //   replies starting with "HANDLER" (sendHandshakePack back)
    //   replies starting with "error"   (error counter)
    //   replies starting with "ACKTF"   (free-space digits -> wakeAll)
    //   replies starting with "ACK" with CB/OK      (command OK -> wakeAll)
    //   replies starting with "CRT" with MOD        (mode report, no wake)
    //   replies starting with "V1./V2./V3."        (firmware version)
    read_with_handler_table(dev, buf_len, "post_han");

    // Step 4 — upload the white image to `key`.
    let width: u16 = 144;
    let height: u16 = 144;
    let mut rgb = vec![0xffu8; width as usize * height as usize * 3];
    for px in rgb.chunks_exact_mut(3) {
        px[0] = 0xff;
        px[1] = 0xff;
        px[2] = 0xff;
    }
    let mut jpeg = Vec::<u8>::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 100)
        .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
        .map_err(|e| format!("error= JPEG encode failed: {e}"))?;
    let image_len = jpeg.len();
    let size_hi = (image_len >> 8) as u8;
    let size_lo = (image_len & 0xff) as u8;
    println!("init jpeg_bytes={image_len}");

    let ico = build_session_pack(
        "ICO",
        0x00,
        total,
        &[
            (8, 0x05),
            (9, size_hi),
            (10, size_lo),
            (11, 0x01),
            (12, 0x00),
        ],
    );
    println!("init ico wrote={}", dev.write(&ico).unwrap_or(0));

    let chunks = image_len.div_ceil(total - 1);
    for (i, bytes) in jpeg.chunks(total - 1).enumerate() {
        let mut pack = vec![0u8; total];
        pack[0] = 0x00;
        pack[1..1 + bytes.len()].copy_from_slice(bytes);
        dev.write(&pack)
            .map_err(|e| format!("error= chunk {i} write failed: {e}"))?;
    }
    println!("init chunks={chunks}");

    let abt = build_session_pack(
        "ABT",
        0x00,
        total,
        &[(10, size_hi), (11, size_lo), (12, key)],
    );
    println!("init abt wrote={}", dev.write(&abt).unwrap_or(0));

    read_with_handler_table(dev, buf_len, "post_abt");

    let stp = build_session_pack("STP", 0x00, total, &[(9, 0x00)]);
    println!("init stp wrote={}", dev.write(&stp).unwrap_or(0));

    read_with_handler_table(dev, buf_len, "post_stp");

    println!("init_result=done key={key} jpeg_bytes={image_len}");
    Ok(())
}

/// Reads on interrupt-IN once per 50 ms for `seconds` and logs any bytes,
/// reacting to the replies the read thread is documented to match.
fn read_with_handler_table(dev: &hidapi::HidDevice, buf_len: usize, label: &str) {
    let mut buf = vec![0u8; buf_len];
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match dev.read_timeout(&mut buf, 200) {
            Ok(n) if n > 0 => {
                let visible = &buf[..n.min(buf_len)];
                println!(
                    "init {label} reply_bytes={n} hex={} ascii={:?} handler={} acktf={} ackcbok={}",
                    hex_prefix(visible, LOGGED_BYTES),
                    ascii_prefix(visible, 24),
                    contains_handler(visible),
                    contains_ascii(visible, b"ACKTF"),
                    contains_ascii(visible, b"CB")
                );
                if contains_handler(visible) && contains_ascii(visible, b"OK") {
                    println!("init {label} HANDLER+OK seen");
                }
            }
            Ok(_) => {}
            Err(e) => {
                println!("init {label} read_error={e}");
                return;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
    }
}

/// P11 profile push — replicates the app's full wake+handshake+insertSettings
/// sequence to render a single image on key "0,0" (L5 = top-left; the app
/// parses "col,row" and maps it to a 1-based key index via a device table
/// that for SS550V3 yields `1+col+5*row` for columns 0-4).
fn run_profile_push(dev: &hidapi::HidDevice, n_in: usize, n_out: usize) -> Result<(), String> {
    let total = if n_out > 0 { n_out } else { FALLBACK_TOTAL };
    let buf_len = if n_in > 0 { n_in } else { FALLBACK_TOTAL };

    // 1. Wake + handshake, exactly as the app does on connect.
    let wake = build_session_pack("DIS", 0x00, total, &[]);
    println!("profile wake wrote={}", dev.write(&wake).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(250));
    let han = build_session_pack("HAN", 0x00, total, &[]);
    println!("profile han wrote={}", dev.write(&han).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(500));

    // 2. Key item: dispatch 0x07 did here as raw wire framing (ICO head + image
    //    chunks + ICO head + JSON chunks + wait), key index 1 for "0,0".
    let width: u16 = 144;
    let height: u16 = 144;
    let mut rgb = vec![0xffu8; width as usize * height as usize * 3];
    for px in rgb.chunks_exact_mut(3) {
        px[0] = 0xff;
        px[1] = 0xff;
        px[2] = 0xff;
    }
    let mut jpeg = Vec::<u8>::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 100)
        .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
        .map_err(|e| format!("error= JPEG encode failed: {e}"))?;
    let jpeg_len = jpeg.len();

    let key_json = br#"{"Position":"0,0","Name":"Test","Settings":{},"State":0,"States":[{"Image":""}],"UUID":"com.hotspot.streamdock.profile.rotate"}"#.to_vec();
    let json_len = key_json.len();
    println!("profile jpg_bytes={jpeg_len} json_bytes={json_len}");

    // 2a. ICO head (primary/image): variant 0x05, field2=1, field3=0.
    let img_hi = (jpeg_len >> 8) as u8;
    let img_lo = (jpeg_len & 0xff) as u8;
    let ico_img = build_session_pack(
        "ICO",
        0x00,
        total,
        &[(8, 0x05), (9, img_hi), (10, img_lo), (11, 0x01), (12, 0x01)],
    );
    println!("profile ico_img wrote={}", dev.write(&ico_img).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(200));

    let chunk = total - 1;
    for (i, bytes) in jpeg.chunks(chunk).enumerate() {
        let mut pack = vec![0u8; total];
        pack[0] = 0x00;
        pack[1..1 + bytes.len()].copy_from_slice(bytes);
        dev.write(&pack)
            .map_err(|e| format!("error= jpeg chunk {i} write failed: {e}"))?;
    }
    println!("profile jpeg_chunks={}", jpeg.len().div_ceil(chunk));
    std::thread::sleep(Duration::from_millis(1200));

    // 2b. ICO head (secondary/json): variant 0x0F.
    let js_hi = (json_len >> 8) as u8;
    let js_lo = (json_len & 0xff) as u8;
    let ico_js = build_session_pack(
        "ICO",
        0x00,
        total,
        &[(8, 0x0f), (9, js_hi), (10, js_lo), (11, 0x01), (12, 0x01)],
    );
    println!("profile ico_json wrote={}", dev.write(&ico_js).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(200));

    for (i, bytes) in key_json.chunks(chunk).enumerate() {
        let mut pack = vec![0u8; total];
        pack[0] = 0x00;
        pack[1..1 + bytes.len()].copy_from_slice(bytes);
        dev.write(&pack)
            .map_err(|e| format!("error= json chunk {i} write failed: {e}"))?;
    }
    println!("profile json_chunks={}", key_json.chunks(chunk).count());
    std::thread::sleep(Duration::from_millis(1200));

    // 3. ULEND — terminates the settings/profile push. Wire = CRT + ULEND.
    let ulend = build_session_pack("ULEND", 0x00, total, &[]);
    println!("profile ulend wrote={}", dev.write(&ulend).unwrap_or(0));
    std::thread::sleep(Duration::from_millis(500));

    // 4. Read the identity report to confirm the channel survived the push.
    let mut buf = vec![0u8; total];
    buf[0] = 0x00;
    match dev.get_input_report(&mut buf) {
        Ok(n) => println!(
            "profile post_push_identity_bytes={n} ascii={:?}",
            ascii_prefix(&buf[..n.min(buf.len())], 24)
        ),
        Err(e) => println!("profile post_push_get_input_report_error={e}"),
    }

    // 5. A quick read on interrupt-IN to catch any reply the push produced.
    let mut ib = vec![0u8; buf_len];
    let n = dev.read_timeout(&mut ib, 2000).unwrap_or(0);
    println!(
        "profile interrupt_bytes={n} ascii={:?}",
        ascii_prefix(&ib[..n.min(ib.len())], 32)
    );

    println!("profile_result=done");
    Ok(())
}

/// Builds a pack of exactly `total` bytes: report ID 0x00 + payload.
/// `HAN` is a bare marker (no `CRT` prefix); everything else is `CRT` framed.
fn build_session_pack(tag: &str, data_type: u8, total: usize, fields: &[(usize, u8)]) -> Vec<u8> {
    let mut pack = vec![0u8; total];
    pack[0] = 0x00;
    let payload = &mut pack[1..];
    let _ = data_type;
    if tag == "HAN" {
        if payload.len() >= 3 {
            payload[..3].copy_from_slice(b"HAN");
        }
    } else if tag.len() == 3 {
        if payload.len() >= 8 {
            payload[..3].copy_from_slice(b"CRT");
            payload[5..8].copy_from_slice(tag.as_bytes());
        }
    } else if tag.len() == 5 {
        // 5-char tags like CONNECT: payload[0..3]="CRT", tag spans [5..10].
        if payload.len() >= 10 {
            payload[..3].copy_from_slice(b"CRT");
            payload[5..10].copy_from_slice(tag.as_bytes());
        }
    }
    for (off, v) in fields {
        if *off < payload.len() {
            payload[*off] = *v;
        }
    }
    pack
}

/// P6 wake sequence — the app-exact connect order recovered from
/// `SDLibrary1.dll`: the device is woken with `CRT`/`DIS` (which clears the
/// app's awake gate that every queued command requires), THEN the handshake,
/// and only then the queued commands work. Our cold-state failures were all
/// missing step 2.
const WAKE_PROBES: &[Probe] = &[
    // 1. Wake: `sendWakeUpScreenPack` 0x18001ecf0, CRT | 00 | DIS.
    // Byte 10 is 1 only for model "LMA"; the SS550 leaves it 0.
    Probe {
        name: "wake_dis",
        marker: *b"CRT",
        data_type: 0x00,
        tag: Some(*b"DIS"),
        fields: &[],
    },
    // 2. Handshake: `sendHandshakePack` 0x18001d650, bare HAN.
    Probe {
        name: "handshake",
        marker: *b"HAN",
        data_type: 0x00,
        tag: None,
        fields: &[],
    },
    // 3. Now that the wake gate is cleared, queued commands are admissible.
    // `addClearCommand` (0x0b, key at [11]).
    Probe {
        name: "post_wake_cle0",
        marker: *b"CRT",
        data_type: 0x0b,
        tag: Some(*b"CLE"),
        fields: &[(11, 0x00)],
    },
    // 4. `addModeChangedCommand` (0x0e, mode at [10]).
    Probe {
        name: "post_wake_mod1",
        marker: *b"CRT",
        data_type: 0x0e,
        tag: Some(*b"MOD"),
        fields: &[(10, 0x01)],
    },
];

/// P8 image upload — byte-exact against `mirajazz`, the Rust library that
/// OpenDeck's SS550 plugin runs in production and that already supports this
/// exact device (PID `0x3000`, protocol version 2).
///
/// Its wire sequence for one key, in this order:
/// 1. `initialize()`: `CRT|DIS`, then `CRT|LIG 00 00 00` (brightness 0).
/// 2. `send_image()`: the `BAT` header comes FIRST — wire `[6..8]` = `BAT`,
///    `[11..12]` = BE16 image length, `[13]` = 1-based key, padded to 1025.
/// 3. Payload chunks: report ID `0x00` + up to 1024 image bytes.
/// 4. `flush()`: a single `CRT|STP` after every key's image was written.
///
/// There is NO `ICO`, NO `ABT` and NO `ULEND` on this path. The image is JPEG
/// at quality 90, resized to 100x100 (Lanczos3) and rotated 180.
fn run_image_upload(
    dev: &hidapi::HidDevice,
    n_in: usize,
    n_out: usize,
    keys: &[u8],
    colors: &[u32],
) -> Result<(), String> {
    let total = if n_out > 0 { n_out } else { FALLBACK_TOTAL };
    let _ = n_in;

    let chunk = total - 1; // payload bytes per output report

    // 100x100 is the native icon size for this family. Rotation is a no-op on
    // a solid colour, so this probe isolates the key-index mapping alone.
    let width: u16 = 100;
    let height: u16 = 100;

    // Encode every key's image up front: each BAT header must carry its own
    // length, exactly like the reference library's per-key `send_image`.
    if keys.is_empty() {
        return Err("error= no keys requested".to_string());
    }
    let mut images: Vec<Vec<u8>> = Vec::with_capacity(keys.len());
    for (i, _) in keys.iter().enumerate() {
        let color = colors.get(i).copied().unwrap_or(0x00ff_ffff);
        let mut rgb = vec![0u8; width as usize * height as usize * 3];
        for px in rgb.chunks_exact_mut(3) {
            px[0] = (color >> 16) as u8;
            px[1] = (color >> 8) as u8;
            px[2] = color as u8;
        }
        // Quality 90 matches `JpegEncoder::new_with_quality(&mut buf, 90)`.
        let mut jpeg = Vec::<u8>::new();
        jpeg_encoder::Encoder::new(&mut jpeg, 90)
            .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
            .map_err(|e| format!("error= JPEG encode failed for key {i}: {e}"))?;
        images.push(jpeg);
    }
    println!(
        "upload images={} first_jpeg_bytes={} first_jpeg_head={}",
        images.len(),
        images[0].len(),
        hex_prefix(&images[0], 16)
    );

    let send = |name: &str, pack: &[u8]| -> Result<usize, String> {
        // Interrupt OUT, like the reference software. Control `Set_Report`
        // returns success but the firmware ignores it.
        dev.write(pack)
            .map_err(|e| format!("error= {name} write failed: {e}"))
    };
    let read_reply = |label: &str| {
        let mut buf = vec![0u8; total];
        buf[0] = 0x00;
        let n = dev.get_input_report(&mut buf).unwrap_or(0);
        println!(
            "upload {label}_reply_bytes={n} ascii={:?}",
            ascii_prefix(&buf[..n.min(buf.len())], 24)
        );
    };

    // 1. initialize(): wake, then the zero-brightness LIG the library sends.
    let wake = build_probe_pack(
        &Probe {
            name: "wake",
            marker: *b"CRT",
            data_type: 0x00,
            tag: Some(*b"DIS"),
            fields: &[],
        },
        total,
    );
    println!("upload wake wrote={}", send("wake", &wake)?);
    std::thread::sleep(Duration::from_millis(DIFF_GAP_MS));

    let lig = build_probe_pack(
        &Probe {
            name: "lig",
            marker: *b"CRT",
            data_type: 0x00,
            tag: Some(*b"LIG"),
            fields: &[(10, 0x00u8)],
        },
        total,
    );
    println!("upload lig wrote={}", send("lig", &lig)?);

    // 2. Per key: the BAT header FIRST, then that key's JPEG chunks.
    let mut total_bytes = 0usize;
    for (i, &key) in keys.iter().enumerate() {
        let image_jpeg = &images[i];
        let image_len = image_jpeg.len();
        total_bytes += image_len;
        let size_hi = (image_len >> 8) as u8;
        let size_lo = (image_len & 0xff) as u8;

        let bat = build_probe_pack(
            &Probe {
                name: "bat",
                marker: *b"CRT",
                data_type: 0x00,
                tag: Some(*b"BAT"),
                // Convert OpenDeck key (0-14) to firmware key (1-15) via plugin's map.
                // The plugin uses [10,11,12,13,14,5,6,7,8,9,0,1,2,3,4]; key 0 is a placeh.
                fields: &[
                    (10, size_hi),
                    (11, size_lo),
                    (12, opendeck_to_wire_key(key)),
                ],
            },
            total,
        );
        println!(
            "upload bat wrote={} slot={i} opendeck_key={key} firmware_key={} size={image_len} color=#{:06X}",
            send("bat", &bat)?,
            opendeck_to_wire_key(key),
            colors.get(i).copied().unwrap_or(0x00ff_ffff)
        );

        for (c, bytes) in image_jpeg.chunks(chunk).enumerate() {
            let mut pack = vec![0u8; total];
            pack[0] = 0x00;
            pack[1..1 + bytes.len()].copy_from_slice(bytes);
            dev.write(&pack)
                .map_err(|e| format!("error= key {key} chunk {c} write failed: {e}"))?;
        }
        println!(
            "upload key={key} chunks={} chunk_size={chunk}",
            image_jpeg.len().div_ceil(chunk)
        );
    }

    // 3. flush(): one STP commits every image written above.
    let stp = build_probe_pack(
        &Probe {
            name: "stp",
            marker: *b"CRT",
            data_type: 0x00,
            tag: Some(*b"STP"),
            fields: &[],
        },
        total,
    );
    println!("upload stp wrote={}", send("stp", &stp)?);

    // 4. Restore brightness: `initialize()` leaves it at 0, and the library's
    //    caller immediately raises it. Without this the deck stays dark even
    //    when the images were accepted.
    let lig_on = build_probe_pack(
        &Probe {
            name: "lig_on",
            marker: *b"CRT",
            data_type: 0x00,
            tag: Some(*b"LIG"),
            fields: &[(10, 100u8)],
        },
        total,
    );
    println!("upload lig_on wrote={}", send("lig_on", &lig_on)?);

    // Read the identity/status report to prove the channel is still alive.
    read_reply("post");

    println!("upload_result=done keys={keys:?} image_bytes={total_bytes}");
    Ok(())
}

/// Sends every wake/handshake/command probe and reads each reply over the
/// control-transfer path.
fn run_wake_sequence(dev: &hidapi::HidDevice, n_in: usize, n_out: usize) -> Result<(), String> {
    let total = if n_out > 0 { n_out } else { FALLBACK_TOTAL };

    for p in WAKE_PROBES {
        let pack = build_probe_pack(p, total);
        let head: String = pack[..16.min(pack.len())]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();

        match dev.write(&pack) {
            Ok(w) => {
                // Give the device a moment, then read the reply over control.
                std::thread::sleep(Duration::from_millis(DIFF_GAP_MS));
                let (n, ascii) = read_reply_control(dev, total);
                println!("wake probe={} wrote={w} pack_head={head}", p.name);
                println!(
                    "wake probe={} reply_bytes={n} reply_ascii={ascii:?}",
                    p.name
                );
                if ascii.is_empty() {
                    // Fall back to an interrupt-IN attempt, for contrast.
                    let mut ib = vec![0u8; if n_in > 0 { n_in } else { FALLBACK_TOTAL }];
                    let m = dev.read_timeout(&mut ib, DIFF_READ_TIMEOUT_MS).unwrap_or(0);
                    if m > 0 {
                        println!(
                            "wake probe={} interrupt_read_bytes={m} ascii={:?}",
                            p.name,
                            ascii_prefix(&ib[..m.min(ib.len())], 24)
                        );
                    }
                }
            }
            Err(e) => println!("wake probe={} write_error={e}", p.name),
        }
        std::thread::sleep(Duration::from_millis(DIFF_GAP_MS));
    }

    println!("wake_result=done probes={}", WAKE_PROBES.len());
    Ok(())
}

/// P5 differential runner: sends each documented probe once and reads the reply
/// over the control-transfer path.
fn run_differential(dev: &hidapi::HidDevice, _n_in: usize, n_out: usize) -> Result<(), String> {
    let total = if n_out > 0 { n_out } else { FALLBACK_TOTAL };

    for p in PROBES {
        let pack = build_probe_pack(p, total);
        let head: String = pack[..16.min(pack.len())]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();

        match dev.write(&pack) {
            Ok(w) => {
                std::thread::sleep(Duration::from_millis(DIFF_GAP_MS));
                let (n, ascii) = read_reply_control(dev, total);
                println!(
                    "diff probe={} wrote={w} pack_head={head} reply_bytes={n} reply_ascii={ascii:?}",
                    p.name
                );
            }
            Err(e) => println!("diff probe={} write_error={e}", p.name),
        }
        std::thread::sleep(Duration::from_millis(DIFF_GAP_MS));
    }

    println!("diff_result=done probes={}", PROBES.len());
    Ok(())
}

/// P7 firmware-version probe.
///
/// `sendGetHardwareFirmwareVersion` @ `0x18001d570` builds
/// `CRT | 00 | 00 | VER` (the previously unmapped tag) and writes it directly.
/// The answer is read two ways by the library: interrupt IN
/// (`readDataFromHidDevice` @ `0x18001bfb0`, 3000 ms) and a control
/// `hid_get_input_report` (`getHardwareFirmwareVersion` @ `0x180019800`), where
/// `buf[0]` must hold the report ID first.
/// The reader only *accepts* a `V1./V2./V3.` reply when the product string is
/// `StreamDock[293]`, which our SS550 is not — but the device may still answer,
/// and any bytes at all prove the read channel.
fn run_version_probe(dev: &hidapi::HidDevice, n_in: usize, n_out: usize) -> Result<(), String> {
    let total = if n_out > 0 { n_out } else { FALLBACK_TOTAL };
    let in_len = if n_in > 0 { n_in } else { FALLBACK_TOTAL };

    // 1. The documented VER request: report ID + "CRT\0\0VER".
    let ver = build_probe_pack(
        &Probe {
            name: "version",
            marker: *b"CRT",
            data_type: 0x00,
            tag: Some(*b"VER"),
            fields: &[],
        },
        total,
    );
    let head: String = ver[..16.min(ver.len())]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    match dev.write(&ver) {
        Ok(_) => {
            println!("ver wrote={} pack_head={head}", ver.len());
            let mut buf = vec![0u8; in_len];
            let n = dev
                .read_timeout(&mut buf, DIFF_READ_TIMEOUT_MS)
                .unwrap_or(0);
            let visible = &buf[..n.min(buf.len())];
            println!(
                "ver interrupt_read_bytes={n} hex={} ascii={:?}",
                hex_prefix(visible, LOGGED_BYTES),
                ascii_prefix(visible, 24)
            );
            let n2 = dev.read_timeout(&mut buf, 500).unwrap_or(0);
            if n2 > 0 {
                let v2 = &buf[..n2.min(buf.len())];
                println!(
                    "ver interrupt_read2_bytes={n2} ascii={:?}",
                    ascii_prefix(v2, 24)
                );
            }
        }
        Err(e) => println!("ver write_error={e}"),
    }

    // 2. Control GET_REPORT (path B): buf[0] carries the report ID.
    let mut fb = vec![0u8; in_len];
    match dev.get_input_report(&mut fb) {
        Ok(n) => {
            let visible = &fb[..n.min(fb.len())];
            println!(
                "ver get_input_report_bytes={n} first=0x{:02x} hex={} ascii={:?}",
                fb.first().copied().unwrap_or(0),
                hex_prefix(visible, LOGGED_BYTES),
                ascii_prefix(visible, 24)
            );
        }
        Err(e) => println!("ver get_input_report_error={e}"),
    }

    // 3. A wider control read, in case the reply is longer than the caps size.
    let mut wide = vec![0u8; total];
    match dev.get_input_report(&mut wide) {
        Ok(n) => println!(
            "ver get_input_report_wide_bytes={n} ascii={:?}",
            ascii_prefix(&wide[..n.min(wide.len())], 24)
        ),
        Err(e) => println!("ver get_input_report_wide_error={e}"),
    }

    println!("ver_result=done");
    Ok(())
}

/// P4: read-only HID report descriptor dump + targeted scan.
///
/// Reads the descriptor from the vendor interface and prints its length and a
/// hex dump, then scans it for the long items that reveal the report-ID space
/// (0x85/0x84 = Report ID), usage pages (0x04/0x05 = Usage Page) and report
/// sizes (0x74/0x75 = Report Size / Report Count). This is read-only: no report
/// is ever written.
fn run_descriptor(api: &HidApi) -> Result<(), String> {
    let devices: Vec<_> = api
        .device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
        .collect();

    let first = match devices.iter().find(|d| {
        d.vendor_id() == VID && d.product_id() == PID && d.usage_page() == SS550_VENDOR_USAGE_PAGE
    }) {
        Some(d) => d,
        None => match devices
            .iter()
            .find(|d| d.vendor_id() == VID && d.product_id() == PID)
        {
            Some(d) => d,
            None => return Ok(()),
        },
    };

    let dev = first
        .open_device(api)
        .map_err(|e| format!("error= open_path failed: {e}"))?;

    let mut buf = vec![0u8; 4096];
    let n = dev
        .get_report_descriptor(&mut buf)
        .map_err(|e| format!("error= get_report_descriptor failed: {e}"))?;

    println!(
        "descriptor_bytes={n} usage_page=0x{:04x} usage=0x{:04x}",
        first.usage_page(),
        first.usage()
    );

    // Full hex dump of every byte.
    let mut dump = String::new();
    for (i, b) in buf[..n].iter().enumerate() {
        if i % 16 == 0 {
            dump.push_str(&format!("\n{:04x}: ", i));
        }
        dump.push_str(&format!("{b:02x} "));
    }
    println!("descriptor_hex={dump}");

    // Targeted scan: walk the short items and log the interesting ones.
    // Short item layout: byte0 = (size & 0x03) | ((type & 0x03) << 2) | ((tag & 0x0f) << 4)
    let mut report_ids: Vec<u8> = Vec::new();
    let mut usage_pages: Vec<u32> = Vec::new();
    let mut report_sizes: Vec<u32> = Vec::new();
    let mut report_counts: Vec<u32> = Vec::new();
    let mut usages: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < n {
        let b0 = buf[i];
        let size = (b0 & 0x03) as usize;
        if i + 1 + size > n {
            break;
        }
        let ty = (b0 >> 2) & 0x03;
        let tag = b0 >> 4;
        let mut val: u32 = 0;
        for (k, b) in buf[i + 1..i + 1 + size].iter().enumerate() {
            val |= u32::from(*b) << (8 * k);
        }
        match (ty, tag) {
            (0x01, 0x08) => report_ids.push(val as u8), // Global / Report ID
            (0x01, 0x00) => usage_pages.push(val),      // Global / Usage Page
            (0x01, 0x07) => report_sizes.push(val),     // Global / Report Size
            (0x01, 0x09) => report_counts.push(val),    // Global / Report Count
            (0x02, 0x00) => usages.push(val),           // Local / Usage
            _ => {}
        }
        i += 1 + size;
    }

    println!("report_ids={:?} (0x{:02x?})", report_ids, report_ids);
    println!("usage_pages={usage_pages:?}");
    println!("report_sizes={report_sizes:?}");
    println!("report_counts={report_counts:?}");
    println!("usages={usages:?}");

    if report_ids.is_empty() {
        println!("descriptor_note=no Report ID long items; reports use implicit ID 0x00");
    }

    println!("descriptor_result=done");
    Ok(())
}

/// P3 passive listener: opens every matching HID interface and reads it for
/// `duration_ms`. NEVER writes to the device.
fn run_passive_listen(api: &HidApi, duration_ms: u64) -> Result<(), String> {
    let devices: Vec<_> = api
        .device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
        .collect();

    if devices.is_empty() {
        println!("listen_result=no_device");
        return Ok(());
    }

    // Open every matching interface. Both the vendor collection and the
    // keyboard collection are opened; the OS may deny write on the latter
    // but reads are permitted, which is all this probe needs.
    let mut opened: Vec<(u16, hidapi::HidDevice)> = Vec::new();
    for d in &devices {
        match d.open_device(api) {
            Ok(dev) => {
                println!(
                    "listen_opened usage_page=0x{:04x} usage=0x{:04x} path={:?}",
                    d.usage_page(),
                    d.usage(),
                    d.path().to_str().unwrap_or("<non-utf8 path>")
                );
                opened.push((d.usage_page(), dev));
            }
            Err(e) => println!(
                "listen_open_failed usage_page=0x{:04x} error={e}",
                d.usage_page()
            ),
        }
    }

    if opened.is_empty() {
        return Err("error= no matching interface could be opened for listening".to_string());
    }

    let deadline = Instant::now() + Duration::from_millis(duration_ms);
    let mut buf = vec![0u8; FALLBACK_TOTAL];
    let mut reads: u64 = 0;
    let mut nonempty: u64 = 0;

    while Instant::now() < deadline {
        for (usage_page, dev) in &opened {
            match dev.read_timeout(&mut buf, LISTEN_POLL_MS) {
                Ok(n) => {
                    reads += 1;
                    if n > 0 {
                        nonempty += 1;
                        println!(
                            "listen_read usage_page=0x{usage_page:04x} bytes={n} hex={}",
                            hex_prefix(&buf[..n], LOGGED_BYTES)
                        );
                    }
                }
                Err(e) => {
                    println!("listen_read_error usage_page=0x{usage_page:04x} error={e}");
                }
            }
        }
    }

    println!("listen_result=done polls={reads} nonempty={nonempty}");
    Ok(())
}

/// Performs the P2 (G1) enumeration and length logging (read-only).
///
/// Returns the opened device (if any) together with its reported report
/// lengths `(n_in, n_out)`. An empty enumeration is handled by the caller.
fn run_p2(api: &HidApi) -> Result<Option<(hidapi::HidDevice, usize, usize)>, String> {
    let devices: Vec<_> = api.device_list().collect();
    for d in &devices {
        println!(
            "path={:?} vid=0x{:04x} pid=0x{:04x} usage_page=0x{:04x} usage=0x{:04x} serial={:?} product={:?} manufacturer={:?}",
            d.path().to_str().unwrap_or("<non-utf8 path>"),
            d.vendor_id(),
            d.product_id(),
            d.usage_page(),
            d.usage(),
            d.serial_number(),
            d.product_string(),
            d.manufacturer_string()
        );
    }

    // The SS550 exposes two HID interfaces: MI_00 (vendor desk interface,
    // usage_page 0xFFA0) and MI_01 (keyboard, usage_page 0x0001). Windows denies
    // output reports on top-level keyboard collections, so the vendor interface
    // is the only writable one. Prefer it; fall back to any match only when the
    // vendor interface is absent.
    let first = match devices.iter().find(|d| {
        d.vendor_id() == VID && d.product_id() == PID && d.usage_page() == SS550_VENDOR_USAGE_PAGE
    }) {
        Some(d) => d,
        None => match devices
            .iter()
            .find(|d| d.vendor_id() == VID && d.product_id() == PID)
        {
            Some(d) => d,
            None => return Ok(None),
        },
    };

    println!(
        "selected_interface usage_page=0x{:04x} usage=0x{:04x} path={:?}",
        first.usage_page(),
        first.usage(),
        first.path().to_str().unwrap_or("<non-utf8 path>")
    );

    let dev = first
        .open_device(api)
        .map_err(|e| format!("error= open_path failed: {e}"))?;
    let (n_in, n_out) = read_report_lengths(first.path());

    println!("input_report_length={n_in}");
    println!("output_report_length={n_out}");
    println!("caps_report_length={n_in}/{n_out}");
    if n_in == 0 && n_out == 0 {
        println!(
            "caps_fallback= available_fallback total={} chunk={}",
            FALLBACK_TOTAL, FALLBACK_CHUNK
        );
    }

    Ok(Some((dev, n_in, n_out)))
}

/// Fallback when neither report length is available (GetCaps status != 0).
/// Based on the DLL-observed fallback: payload 0x40 (64), total 0x41 (65).
const FALLBACK_CHUNK: usize = 64;
const FALLBACK_TOTAL: usize = 65;

/// Performs the P1B (G2) single HAN write + two timeout reads.
fn run_p1b_reads(dev: &hidapi::HidDevice, n_in: usize, n_out: usize) -> Result<(), String> {
    // Total pack size: prefer the reported output length, else fall back to
    // the input length, else to the DLL-observed fallback (64/65). The
    // minimal viable pack is the fixed 4-byte header (`0x00` + `"HAN"`).
    let total = if n_out > 0 {
        n_out
    } else if n_in > 0 {
        n_in
    } else {
        eprintln!(
            "caps_fallback= using FALLBACK_TOTAL ({FALLBACK_TOTAL}) after unavailable lengths"
        );
        FALLBACK_TOTAL
    };
    if total < 4 {
        return Err(format!(
            "error= cannot build HAN pack: no usable report length (input={n_in}, output={n_out})"
        ));
    }

    let han = build_han_pack(total);
    let written = dev
        .write(&han)
        .map_err(|e| format!("error= HAN write failed: {e}"))?;
    println!("han_write_bytes={written}");

    // First read: 3000 ms timeout.
    let mut buf = vec![0u8; if n_in > 0 { n_in } else { FALLBACK_TOTAL }];
    let n_read = dev
        .read_timeout(&mut buf, READ_TIMEOUT_FIRST_MS)
        .map_err(|e| format!("error= read_timeout(3000ms) failed: {e}"))?;
    println!("read1_bytes={n_read}");
    let read1 = &buf[..n_read.min(buf.len())];
    println!("read1_hex={}", hex_prefix(read1, LOGGED_BYTES));
    println!("read1_buf0_is_00={}", !read1.is_empty() && read1[0] == 0x00);
    println!("read1_handler_present={}", contains_handler(read1));

    // Sleep 3000 ms, then one more 500 ms read.
    std::thread::sleep(Duration::from_millis(SLEEP_BETWEEN_READS_MS));
    let n_read2 = dev
        .read_timeout(&mut buf, READ_TIMEOUT_SECOND_MS)
        .map_err(|e| format!("error= read_timeout(500ms) failed: {e}"))?;
    println!("read2_bytes={n_read2}");
    let read2 = &buf[..n_read2.min(buf.len())];
    println!("read2_hex={}", hex_prefix(read2, LOGGED_BYTES));
    println!("read2_buf0_is_00={}", !read2.is_empty() && read2[0] == 0x00);
    println!("read2_handler_present={}", contains_handler(read2));

    Ok(())
}

/// Entry point: parses args and runs the selected probe.
fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    let mut han = false;
    let mut listen_ms: Option<u64> = None;
    let mut descriptor = false;
    let mut diff = false;
    let mut wake = false;
    let mut ver = false;
    let mut upload = false;
    let mut bright: Option<u8> = None;
    let mut init = false;
    let mut profile = false;
    let mut all_keys = false;
    let mut key_index: u8 = 0;
    let mut proof = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].to_str() {
            Some("--han") => han = true,
            Some("--descriptor") => descriptor = true,
            Some("--diff") => diff = true,
            Some("--wake") => wake = true,
            Some("--ver") => ver = true,
            Some("--upload") => upload = true,
            Some("--init") => init = true,
            Some("--profile") => profile = true,
            Some("--bright") => {
                match args
                    .get(i + 1)
                    .and_then(|v| v.to_str())
                    .and_then(|v| v.parse::<u8>().ok())
                {
                    Some(v) => bright = Some(v),
                    None => {
                        eprintln!("error= --bright requires a value 0-100");
                        eprint!("{USAGE}");
                        return ExitCode::from(2);
                    }
                }
                i += 1;
            }
            Some("--listen") => {
                match args
                    .get(i + 1)
                    .and_then(|v| v.to_str())
                    .and_then(|v| v.parse().ok())
                {
                    Some(ms) => listen_ms = Some(ms),
                    None => {
                        eprintln!("error= --listen requires a duration in milliseconds");
                        eprint!("{USAGE}");
                        return ExitCode::from(2);
                    }
                }
                i += 1;
            }
            Some("--key") => {
                match args
                    .get(i + 1)
                    .and_then(|v| v.to_str())
                    .and_then(|v| v.parse::<u8>().ok())
                {
                    Some(v) if v < 15 => key_index = v,
                    _ => {
                        eprintln!("error= --key requires a device key index 0-14");
                        eprint!("{USAGE}");
                        return ExitCode::from(2);
                    }
                }
                i += 1;
            }
            Some("--all") => {
                all_keys = true;
                upload = true;
            }
            Some("--proof") => {
                proof = true;
            }
            Some("-h" | "--help") => {
                print!("{USAGE}");
                return ExitCode::from(0);
            }
            Some(_) => {
                eprintln!("error= unrecognized argument: {:?}", args[i]);
                eprint!("{USAGE}");
                return ExitCode::from(2);
            }
            None => {
                eprintln!("error= non-UTF-8 argument");
                eprint!("{USAGE}");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    // The passive listener is strictly read-only and must never be combined
    // with a write probe; refuse the combination rather than risk a write.
    if listen_ms.is_some() && han {
        eprintln!("error= --listen (read-only) cannot be combined with --han (write)");
        return ExitCode::from(2);
    }

    // Read-only probes must never be combined with a write probe.
    if (listen_ms.is_some() || descriptor)
        && (han || diff || wake || ver || upload || bright.is_some() || init || profile)
    {
        eprintln!("error= read-only probes cannot be combined with a write probe");
        return ExitCode::from(2);
    }
    // Exactly one write probe per session, so each result is attributable.
    {
        let writes = [
            han,
            diff,
            wake,
            ver,
            upload,
            bright.is_some(),
            init,
            profile,
            proof,
        ]
        .iter()
        .filter(|b| **b)
        .count();
        if writes > 1 {
            eprintln!(
                "error= only one write probe per session (--han | --diff | --wake | --ver | --upload | --bright | --init | --profile | --proof)"
            );
            return ExitCode::from(2);
        }
    }

    println!("probe ss550-probe vid=0x{VID:04x} pid=0x{PID:04x}");

    let api = match HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error= HidApi::new failed: {e}");
            return ExitCode::from(1);
        }
    };

    if descriptor {
        return match run_descriptor(&api) {
            Ok(()) => ExitCode::from(0),
            Err(msg) => {
                eprintln!("{msg}");
                ExitCode::from(1)
            }
        };
    }

    if let Some(ms) = listen_ms {
        return match run_passive_listen(&api, ms) {
            Ok(()) => ExitCode::from(0),
            Err(msg) => {
                eprintln!("{msg}");
                ExitCode::from(1)
            }
        };
    }

    let matched: Vec<_> = api
        .device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
        .collect();
    println!("matched={}", matched.len());

    if matched.is_empty() {
        println!("result=no_device");
        return ExitCode::from(0);
    }

    match run_p2(&api) {
        Ok(Some((dev, n_in, n_out))) => {
            if upload {
                let keys: Vec<u8> = if all_keys {
                    (0..15).collect()
                } else {
                    vec![key_index]
                };
                let colors: Vec<u32> = if all_keys {
                    KEY_COLORS.to_vec()
                } else {
                    vec![0x00ff_ffff]
                };
                match run_image_upload(&dev, n_in, n_out, &keys, &colors) {
                    Ok(()) => println!("result=ok mode=upload"),
                    Err(msg) => {
                        eprintln!("{msg}");
                        println!("result=error mode=upload");
                        return ExitCode::from(1);
                    }
                }
            } else if let Some(v) = bright {
                match run_brightness(&dev, n_out, v) {
                    Ok(()) => println!("result=ok mode=bright"),
                    Err(msg) => {
                        eprintln!("{msg}");
                        println!("result=error mode=bright");
                        return ExitCode::from(1);
                    }
                }
            } else if init {
                match run_full_session(&dev, n_in, n_out, 0) {
                    Ok(()) => println!("result=ok mode=init"),
                    Err(msg) => {
                        eprintln!("{msg}");
                        println!("result=error mode=init");
                        return ExitCode::from(1);
                    }
                }
            } else if profile {
                match run_profile_push(&dev, n_in, n_out) {
                    Ok(()) => println!("result=ok mode=profile"),
                    Err(msg) => {
                        eprintln!("{msg}");
                        println!("result=error mode=profile");
                        return ExitCode::from(1);
                    }
                }
            } else if proof {
                let keys: Vec<u8> = if all_keys {
                    (0..15).collect()
                } else {
                    vec![key_index]
                };
                match run_key_proof(&dev, n_in, n_out, keys) {
                    Ok(()) => println!("result=ok mode=proof"),
                    Err(msg) => {
                        eprintln!("{msg}");
                        println!("result=error mode=proof");
                        return ExitCode::from(1);
                    }
                }
            } else if ver {
                match run_version_probe(&dev, n_in, n_out) {
                    Ok(()) => println!("result=ok mode=ver"),
                    Err(msg) => {
                        eprintln!("{msg}");
                        println!("result=error mode=ver");
                        return ExitCode::from(1);
                    }
                }
            } else if wake {
                match run_wake_sequence(&dev, n_in, n_out) {
                    Ok(()) => println!("result=ok mode=wake"),
                    Err(msg) => {
                        eprintln!("{msg}");
                        println!("result=error mode=wake");
                        return ExitCode::from(1);
                    }
                }
            } else if diff {
                match run_differential(&dev, n_in, n_out) {
                    Ok(()) => println!("result=ok mode=diff"),
                    Err(msg) => {
                        eprintln!("{msg}");
                        println!("result=error mode=diff");
                        return ExitCode::from(1);
                    }
                }
            } else if han {
                match run_p1b_reads(&dev, n_in, n_out) {
                    Ok(()) => println!("result=ok mode=han"),
                    Err(msg) => {
                        eprintln!("{msg}");
                        println!("result=error mode=han");
                        return ExitCode::from(1);
                    }
                }
            } else {
                println!("result=ok mode=p2");
            }
            ExitCode::from(0)
        }
        Ok(None) => {
            println!("result=no_device");
            ExitCode::from(0)
        }
        Err(msg) => {
            eprintln!("{msg}");
            println!("result=error mode=p2");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_control_pack_matches_the_failed_p1b() {
        let pack = build_probe_pack(&PROBES[0], 1025);
        assert_eq!(pack.len(), 1025);
        assert_eq!(pack[0], 0x00, "wire byte 0 is the report ID");
        assert_eq!(&pack[1..4], b"HAN");
        assert_eq!(pack[5], 0x00, "variant byte is 0 in the control probe");
        assert!(pack[8..].iter().all(|b| *b == 0), "rest of payload is zero");
    }

    #[test]
    fn command_packs_carry_marker_tag_and_fields() {
        let cle = build_probe_pack(&PROBES[3], 1025);
        assert_eq!(&cle[1..4], b"CRT", "CRT marker at payload 0..3");
        assert_eq!(cle[5], 0x0b, "payload byte 4 = dataType");
        assert_eq!(&cle[6..9], b"CLE", "command tag at payload 5..8");
        assert_eq!(cle[12], 0x00, "payload byte 11 = key index");

        let cle1 = build_probe_pack(&PROBES[4], 1025);
        assert_eq!(cle1[12], 0x01, "only the key field differs between probes");

        let m = build_probe_pack(&PROBES[5], 1025);
        assert_eq!(&m[1..4], b"CRT");
        assert_eq!(m[5], 0x0e, "MOD uses dataType 0x0e");
        assert_eq!(&m[6..9], b"MOD");
        assert_eq!(m[11], 0x00, "payload byte 10 = mode");
    }

    #[test]
    fn all_probes_are_backed_by_documented_commands_only() {
        // Every probe must be one of the framers observed in SDLibrary1.dll.
        for p in PROBES.iter().chain(WAKE_PROBES.iter()) {
            let tags: Vec<_> = match p.tag {
                Some(t) => t.to_vec(),
                None => p.marker.to_vec(),
            };
            assert!(
                tags == b"HAN" || tags == b"CLE" || tags == b"MOD" || tags == b"DIS",
                "unexpected command tag {:?} in probe {}",
                String::from_utf8_lossy(&tags),
                p.name
            );
        }
    }

    #[test]
    fn version_pack_is_the_documented_crt_00_00_ver() {
        // sendGetHardwareFirmwareVersion 0x18001d570:
        // report ID 0x00, then payload "CRT" | 0x00 | 0x00 | "VER".
        let ver = build_probe_pack(
            &Probe {
                name: "version",
                marker: *b"CRT",
                data_type: 0x00,
                tag: Some(*b"VER"),
                fields: &[],
            },
            1025,
        );
        assert_eq!(ver[0], 0x00);
        assert_eq!(&ver[1..4], b"CRT");
        assert_eq!(ver[4], 0x00);
        assert_eq!(ver[5], 0x00, "variant/dataType is 0 for VER");
        assert_eq!(&ver[6..9], b"VER");
        assert!(ver[9..].iter().all(|b| *b == 0), "rest is zero-filled");
    }

    #[test]
    fn wake_sequence_carries_the_documented_crt_dis_pack() {
        // sendWakeUpScreenPack: `[report id] CRT | 00 | DIS | ...` (byte 10 is
        // set to 1 only for model "LMA", so the SS550 leaves it 0).
        let wake = build_probe_pack(&WAKE_PROBES[0], 1025);
        assert_eq!(wake[0], 0x00);
        assert_eq!(&wake[1..4], b"CRT");
        assert_eq!(wake[5], 0x00, "variant byte is 0 for DIS");
        assert_eq!(&wake[6..9], b"DIS");
        assert_eq!(wake[11], 0x00, "byte 10 stays 0 for non-LMA models");
        assert_eq!(&WAKE_PROBES[1].marker, b"HAN", "handshake follows the wake");
    }
}
