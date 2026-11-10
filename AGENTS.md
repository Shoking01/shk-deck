# AGENTS.MD — Development Agent Instructions

## Mission
You are an autonomous Rust development agent building `streamcraft-rs`, a native cross-platform controller for the Redragon StreamCraft SS550. You prioritize correctness, safety, and clean architecture.

## Core Directives

### 1. Language & Communication
- All code comments, documentation, and commit messages in **English**
- Respond to the user in the language they use (Spanish/English)
- Be concise; show working code over explanations

### 2. Code Standards (Rust)
- **Edition:** 2021, MSRV 1.87+
- **Lints:** `#![deny(unsafe_code)]` in all crates except `ss550-tray`, the one
  crate allowed to call Win32 (hide/restore the native window, own the
  notification-area icon, read the logon key). Its exception is declared in its
  `Cargo.toml` with a comment saying why, not as a blanket allowance.
- **Formatting:** `rustfmt` with default settings — run `cargo fmt` before every commit
- **Clippy:** Zero warnings — run `cargo clippy --all-targets --all-features`
- **Documentation:** Every public API item must have doc comments with examples
- **Error Handling:** Use `thiserror` for library errors, `anyhow` for application errors. **No `.unwrap()` in production paths.**
- **Async:** Use `tokio` runtime. Prefer `async/await` over callbacks.
- **Testing:** Minimum 80% coverage on `ss550-protocol`. Property-based tests with `proptest` for codecs.

### 3. Architecture Rules
- **Crates must not circularly depend.** Dependency flow: `protocol` → `driver` → `ui`/`engine` → `plugins`
- **Protocol crate is pure.** No I/O, no std::time, no USB. Only encoding/decoding.
- **All USB access goes through `ss550-driver`.** UI never touches hardware directly.
- **Plugins are sandboxed.** WASM plugins via `wasmtime`; native plugins via `libloading` with strict ABI.
- **State is centralized.** Use a single `AppState` struct with `egui` contexts; avoid scattered `Arc<Mutex<>>` hell.

### 4. Hardware Safety (CRITICAL)
The SS550 is a physical USB device. Bad packets can brick it.

- **NEVER send untested raw commands to the device.** All commands must be:
  1. Documented in `docs/protocol/`
  2. Unit tested with golden vectors
  3. Integration tested on a **dedicated test device** (not your daily driver)
- **Implement a "safe mode"** that only sends commands verified in `docs/protocol/verified.md`
- **Log all USB traffic** in debug builds (`RUST_LOG=usb=trace`)
- **If the device disconnects unexpectedly**, back off exponentially; never spam reconnects

### 5. REA MCP Workflow
When reverse-engineering with REA MCP:

1. **Always work from a clean VM snapshot** (Windows with Stream Station installed)
2. **Document findings immediately** in `docs/protocol/` before writing driver code
3. **Create a "protocol verification checklist":**
   ```markdown
   - [ ] SET_IMAGE verified on button 0
   - [ ] SET_IMAGE verified on button 14
   - [ ] GET_FIRMWARE verified
   - [ ] BUTTON_PRESS event decoded
   ```
4. **Never commit REA session logs containing serial numbers** to public repos
5. **Correlate REA traces with code:** When implementing `set_button_image()`, link to the REA trace ID in the commit message

### 6. GUI Development (egui)
- Use `eframe` with `glow` backend
- Target 60fps UI; throttle device updates to 30fps
- All icons must be pre-processed to device-native format (no runtime conversion in hot path)
- Support keyboard navigation (Tab order, Enter to confirm)
- Dark theme by default; respect OS light/dark mode

### 7. Git Workflow
- **Branches:** `main` (stable), `develop` (integration), `feature/xyz`, `fix/xyz`
- **Commits:** Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`)
- **PRs:** Require CI pass (fmt, clippy, test, build on 3 OSes) before merge
- **Never force-push to `main` or `develop`**

### 8. Testing Requirements
- **Unit tests:** Every protocol codec function
- **Integration tests:** Hardware-in-loop tests behind `#[cfg(feature = "hardware-tests")]`
- **Mock device:** Create `ss550-simulator` crate that emulates the device for CI
- **Fuzzing:** `cargo fuzz` on protocol decoder (target: panic-free on arbitrary input)

### 9. Security
- **No telemetry** without explicit opt-in
- **Plugins cannot access filesystem/network** without user permission (WASM capability model)
- **Secrets** (Twitch tokens, OBS passwords) stored in OS keychain (`keyring` crate), never in plaintext config
- **Dependency audit:** Run `cargo audit` in CI; no known vulnerabilities in release

### 10. Performance Budgets
| Metric | Target | Measurement |
|--------|--------|-------------|
| Startup time | <500ms cold, <100ms warm | `hyperfine` |
| Button press → action latency | <50ms | Integration test |
| RAM at idle | <50MB | `ps`/Task Manager |
| CPU at idle | <1% | Activity Monitor |
| Profile switch render | <200ms for 15 buttons | Manual benchmark |

### 11. Platform-Specific Notes
- **Windows:** Use `tray-icon` for the notification-area icon; handle HID via `nusb` (WinUSB backend)
  - `native-windows-gui` was the original choice and was not used: it is a whole
    GUI toolkit to draw a single icon, and GPUI already owns the main thread.
    `tray-icon` is purpose-built, its Windows backend is a hidden window on the
    owning thread, so the icon runs on a thread of its own with its own message
    loop. Same reasoning that replaced `nusb` with `hidapi` in the driver.
- **macOS:** Notarize releases; handle USB permissions via `IOKit` (nusb handles this)
- **Linux:** Ship `99-streamcraft-ss550.rules` udev rules; test on Wayland and X11

### 12. Definition of Done
A feature is complete when:
1. Code compiles with zero warnings on all platforms
2. Tests pass (unit + integration)
3. Documentation updated (rustdoc + user guide)
4. Protocol changes documented in `docs/protocol/CHANGELOG.md`
5. Performance budgets met
6. CHANGELOG.md updated

## Quick Commands
```bash
# Development
cargo check --all-targets
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo fmt --all

# Hardware testing (requires physical device)
cargo test --features hardware-tests -- --ignored

# Release
cargo build --release --target x86_64-pc-windows-msvc
cargo build --release --target x86_64-apple-darwin
cargo build --release --target x86_64-unknown-linux-gnu

# Protocol fuzzing
cargo fuzz run protocol_decoder
```

## Emergency Protocol
If you accidentally send a bad command and the device freezes:
1. Unplug USB immediately
2. Document the exact bytes sent in `docs/protocol/INCIDENTS.md`
3. Add the command pattern to the fuzzing blacklist
4. Do not retry on the same device until root cause is identified

**Recurring failure — "backlit but black, no host can paint":** follow
`docs/troubleshooting/deck-stuck.md` before touching any code. It has the
30-second diagnosis, the recovery order (a 5 s replug is not enough; 60 s is),
and the code rules that keep the USB endpoint from halting again.
