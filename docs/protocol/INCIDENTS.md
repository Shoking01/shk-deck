# INCIDENTS

Device-damaging or device-hanging episodes. Every entry records the exact
commands sent, so the pattern can be blacklisted and never repeated blindly.

**This failure mode has recurred more than once.** The procedure to diagnose and
recover lives in
[`docs/troubleshooting/deck-stuck.md`](../troubleshooting/deck-stuck.md). This
file is only the chronological record — read the runbook first, then add an
entry here.

Short version: a report the firmware rejects halts the USB endpoint; every host
then fails (ours *and* the vendor app) until the device is power cycled for
~60 s. A 5 s replug does not fix it. Touching the deck only toggles the
backlight and never recovers the transport.

---

## 2026-11-06 — SS550 stops accepting output; interrupt OUT never completes

**Device:** Redragon StreamCraft SS550 v3, `0200:3000`, serial redacted.
Interfaces: `MI_00` vendor (`usage_page 0xffa0`), `MI_01` keyboard.

### Symptom

- Deck backlight is on, every LCD is black. Touching the device toggles the
  backlight on/off; the toggle does not change rendering.
- Interrupt OUT (`hidapi::write`, 1025-byte report) never completes. hidapi
  reports `ERROR_IO_PENDING (0xE5)` after its hardcoded 1000 ms wait.
- Own overlapped `WriteFile` on our own handle, waiting 10 s:
  `WaitForSingleObject = 258 (WAIT_TIMEOUT)`, `GetOverlappedResult completed = 0`.
  So the failure is **not** hidapi's 1 s ceiling.
- Control `HidD_SetOutputReport` (`send_output_report`) returns TRUE and
  produces no observable effect on the device.
- The vendor app (Stream Station, process alive at the time) also renders
  nothing, so a known-good host implementation fails identically.
- Windows reports every related node `Status: OK`. No device error state.
- No competing process holds the device; no stray probe processes.

### Commands sent in the window before the fault

`probe --upload` (`DIS`, `LIG 0`, `BAT` 923 B at firmware key 10, one 1024-byte
chunk, `STP`, `LIG 100`), `probe --ver`, `probe --descriptor`,
`ss550-driver` example `hardware_gate`, and the examples `test_write_path`,
`test_send_output`, `raw_write`, `probe_control`.

### Ruled out

- **Framing.** The HID report descriptor declares a real output report
  (1024 data bytes, `91 02`, implicit report ID 0 → 1025 with the prefix). The
  bytes are the same ones that rendered 15 keys in Fase 0.
- **Interface selection.** `MI_00` / `usage_page 0xffa0` is the vendor
  collection; `MI_01` is a keyboard collection.
- **Write timeout.** Proven with a 10 s wait.
- **Read channel as proof.** `get_input_report` returns `.V3.SS_550.01.002.`
  with **zero** writes sent, so it is a static identity report. It must never
  be used as evidence that a write reached the firmware.

### Status

**Partially resolved.** A full power cycle (unplugged ~60 s, different port)
restored the endpoint: `DIS`, `LIG`, `BAT` and `STP` then completed in `0 ms`
each. A plain 5 s replug did **not** restore it.

### Findings after recovery

- The interrupt OUT endpoint works again once the device is truly power
  cycled. The write path, the framing and the interface were never the problem.
- The endpoint **stalls** when the firmware rejects a report. During the
  recovery run an out-of-order flush (`STP` sent before the image chunk, a bug
  in the test example, not in `ss550-protocol`) made the next write fail with
  `ERROR_GEN_FAILURE (0x1F)`, and every later write — including from a fresh
  process — failed with `0xE5` again until the next power cycle.
- **Writing `CRT|VER` also halts it.** `Device::firmware_version()` used to
  write that pack before reading; the firmware rejects it and the endpoint
  died again (verified: after the failed call, a plain `DIS` returned `0xE5`).
  The version string comes from a **static identity report** available from
  `GET_REPORT` with no write at all, so `firmware_version()` now never writes.
- So a rejected report is not a local error: it wedges the endpoint until the
  device is power cycled. `pnputil /restart-device` cannot be used as a
  software recovery (needs elevation: `Access is denied`).

### Next steps

1. Power cycle, then run the corrected order: `DIS`, `LIG <b>`, `BAT`, chunk,
   one `STP`.
2. Never send `STP` before the payload chunk.
3. The driver must never write brightness 0 without a guaranteed restore, and
   must surface "device not accepting output" as a distinct, loud error
   instead of retrying silently.

### Prevention

The driver must never write brightness 0 without a guaranteed restore, and must
surface "device not accepting output" as a distinct, loud error instead of
retrying silently.

---

## 2026-11-06 (b) — the "dead in 2–3 s" report is the deck's own boot re-enumeration

**Not a fault episode.** This entry records a false root cause that cost an
entire day of probing, and how it was disproved.

### Symptom

Every probe that wrote to a freshly powered deck succeeded for a while and then
failed at a time that varied between runs: `1203 ms` with `CRT|CONNECT` beats,
`1510 ms` with a thread draining IN, `2315 ms` with `DIS` every 100 ms,
`3009 ms`, `3011 ms`. After the failure every later write failed with `0x1F`
then `0x3E5`, in the same process and in new ones, and only a power cycle
appeared to help. The working model was "the OUT endpoint closes ~2.3 s after
power-up".

### What disproved it

`quiet_probe` waited for a replug and then sent **nothing at all** for 10 s,
reading only `GET_REPORT` once a second to prove the device was alive:

```text
t=+ 1000ms  present=true  control_ok=true
t=+ 2063ms  present=true  control_ok=true
t=+ 3126ms  present=false            <- left the USB bus
t=+ 7366ms  present=false
t=+ 8426ms  present=true             <- returned
```

The deck re-enumerates itself 1–3 s after power-up and is absent for 4–5 s. A
second run: left at `+953 ms`, back at `+4781 ms`.

### Why every earlier conclusion was wrong

- Each probe opened its handle during **stage 1** and kept using it after the
  reboot. That handle refers to a device instance that no longer exists, so
  `0x48F` (`ERROR_DEVICE_NOT_CONNECTED`) and `0x3E5` were handle-lifetime errors,
  not firmware state.
- `reopen_probe` re-opened three times during the reboot window and failed every
  time, because there was nothing healthy to open yet.
- The apparent "2.3 s" number was the length of stage 1 in that particular run,
  not a protocol property.

### Hypotheses eliminated before the discovery

Selective suspend (`resume_probe`), wrong HID collection (`enumerate`), bare
`HAN` handshake (`handshake_probe`, closed at `3011 ms`), `CRT|CONNECT`
keep-alive (`keepalive_probe`, `1203 ms`), unread interrupt IN blocking the
firmware (`read_in_probe`, zero IN bytes ever), and reopening the handle
(`reopen_probe`).

### Open question

A first run reported `HAN`, `BAT` and the image chunk accepted and `STP` refused
with `0x1F` after the reboot — but it opened its handle at the **first**
reappearance, which a later run proved to be an intermediate appearance lasting
under 502 ms. That result is therefore contaminated by the same stale-handle
effect and is not evidence of a protocol refusal. The measurement was redone
requiring 3 s of continuous presence before opening.

### Prevention

`Device::connect()` must confirm the device has **settled** — the boot performs
more than one re-enumeration, so "it is present" is not enough. Require
continuous presence for a few seconds (3 s covers every observed transition)
before opening the handle it will use. Any handle opened before that is
permanently stale.
