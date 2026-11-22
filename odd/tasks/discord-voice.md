# Discord voice: not built, and why — the record of a closed door

**Outcome: nothing ships.** Discord does not grant the voice scopes to an
application without partner approval, and the `rpc.local` commands it does grant
are an application-private sandbox. A deck key that mutes the microphone *and*
shows whether it is muted cannot be built by this project. The supported path is
a `Keys` action sending Discord's own global bind (`ctrl+shift+m`,
`ctrl+shift+d`), which mutes for real and cannot show the state.

What remains in the repository: `crates/ss550-discord`, reduced to the pipe, the
framing, the Application-ID config and three probes. It ships nothing; it exists
so the next person can answer "does this work?" in a minute instead of an
evening. The full attempt is preserved in branch `feat/discord-voice-attempt`.

Branch `feat/discord-voice` is otherwise the branch this record lives on, stacked
on `feat/clock-plugin` (18 commits ahead of `origin/main`). Runner:
`cargo test -p ss550-discord`,
`cargo test -p ss550-engine`, `cargo test -p ss550-plugins`, `cargo test` in
`crates/ss550-gui`.

## Specs

- **S1** — A built-in plugin named `Discord`, id `discord`, offers two cells:
  `mic` (label `MIC`) and `deafen` (label `DEAF`), both `Face::Number`, so a
  bound key shows the state as its value. They appear in the palette as one
  draggable row, like every other plugin.
- **S2** — Pressing a key bound to `mic` toggles the microphone mute in the
  Discord client; `deafen` toggles deafen. The key shows the new state without
  the user refreshing anything.
- **S3** — The state shown is **what Discord says**, never what we last sent.
  The plugin learns it from Discord's `VOICE_SETTINGS_UPDATE_2` event. When
  nothing has been heard, the reading is `Reading::unavailable` with the reason
  — never a guess and never a stale value presented as live.
- **S4** — The integration uses the Discord client's **local IPC pipe only**
  (`\\.\pipe\discord-ipc-{0..9}`): no network, no OAuth, no access token, no
  client secret, no keyring. The only configuration is the **Application ID**,
  a public snowflake the user creates in Discord's developer portal, stored in
  `config_dir()/discord.json`.
- **S5** — One connection per process, owned by the new `ss550-discord` crate:
  handshake, subscribe, keep the last known voice state, apply commands, and
  reconnect with a backoff that respects Discord's limit of 2 connections per
  minute. It never spams reconnects.
- **S6** — `ss550-engine` stays free of the OS and of Discord: the action runs
  through a `DiscordSender` seam exactly like the existing `KeySender`, so
  `crates/ss550-engine/tests/the_engine_knows_no_device.rs` keeps passing.
- **S7** — The Application ID is entered in the window (a card in the settings
  panel beside THRESHOLDS and TITLES), persisted, and applied to a running
  client without restarting the app.
- **S8** — `#![deny(unsafe_code)]` stays in every crate. Opening a named pipe
  needs no new exception: `std::fs::File` opens `\\.\pipe\name` through
  `CreateFileW`, so the pipe lives in a safe crate.

## Tasks

- T1 — `ss550-discord` crate: pipe discovery, 8-byte framing, handshake,
  subscribe, command channel, reconnect. Route: inline. **Done** in `7a3337a`.
  Measured: `std::fs::File` opens `\\.\pipe\discord-ipc-0` and Discord answers a
  handshake with a framed `Close {"code":4000,"message":"Invalid Client ID"}` —
  the wire, the framing and the read path are proven against the live client.
- T2 — `ss550-engine`: `Action::Discord { command }`, `DiscordCommand`,
  `DiscordSender` seam, runner job and errors, with tests. Route: inline.
  **Done** in `7329d6e`; 163 engine tests green.
- T3 — `ss550-plugins`: the `discord` plugin and its two cells. Route: inline.
  **Done** in `7a3337a`; 57 plugin tests green.
- T4 — `ss550-gui`: wire the sender, add the APPLICATION ID card, persist it.
  Route: inline. **Done** in `103048e`; 200 window tests green, clippy silent.
- T5 — Live verification against the running Discord client, then docs.
  **Done** with the user's real Application ID. Measured, in this order:
  - the handshake is accepted and `READY` arrives with the account it names;
  - `SUBSCRIBE VOICE_SETTINGS_UPDATE_2` is acknowledged **and immediately
    followed by the current flags** (`self_mute:false, self_deaf:false`), which
    settles L4: the reading is live at startup, not only after the first change;
  - `voice_once` (through the production API) toggled the microphone and read it
    back — `mute=false → true → false`, restored — which is also the proof that
    the asynchronous link writes while a read is pending.
  Docs done in `14ce6df`, corrected after the measurement.

## Log

- **L1** (2026-11-21) — User, verbatim: *"Ahora quiero integrar un plugin que se
  conecte con Discord"*. Asked which relation to Discord, offering presence,
  mute/deafen with state, and keybind-only. User chose, verbatim:
  *"Mute/Deafen con estado en la tecla"*.
- **L2** — I first told the user this needed OAuth2 plus a token in the OS
  keyring. That was wrong, and the scope table says so: `SET_VOICE_SETTINGS_2`
  and the `VOICE_SETTINGS_UPDATE_2` event are both `rpc.local`, which is granted
  to any IPC connection without an `Origin` header. Only `GET_VOICE_SETTINGS`
  needs `rpc`/`rpc.voice.read`. Corrected with the user before writing code.
- **L3** — Environment measured, not assumed: Discord is running on this machine
  (6 processes) and `\\.\pipe\discord-ipc-0` exists. Live verification is
  possible.
- **L4** — **Answered by measurement.** `SUBSCRIBE VOICE_SETTINGS_UPDATE_2` is
  answered with an ack and then a `DISPATCH` carrying the current
  `self_mute`/`self_deaf`, so a freshly started app knows the flags without any
  grant. A key is never blind after a restart.
- **L5** — **A design bug the live client found.** The first version read on one
  thread and wrote on another with `std::fs::File`. A named pipe's file object
  carries one pending operation at a time, so the write blocked behind the
  pending read and a `SUBSCRIBE` never left the process — the probe hung after
  `READY`. Fixed by making the link asynchronous with overlapped I/O
  (`tokio::net::windows::named_pipe`), which is the only way to read and write at
  once without an `unsafe` exception. The unit tests could not have caught this:
  it is a property of the pipe, not of the bytes.
- **L7** — Binding the feature to the deck exposed a gap the crate could not:
  a key could *toggle* (the `Discord mute` template) or *show* (the plugin's
  `mic` cell) but not both, because the editor's `WHEN PRESSED` list only offered
  media commands. The runner already accepted any action as a reading's press.
  Closed in the editor: a Discord cell now offers `Mute microphone` and
  `Deafen` beside the media four, so one key shows `LIVE`/`MUTE` and toggles it.
  A CPU gauge still offers neither, which a test pins.
- **L6** — The Application ID is stored at
  `%APPDATA%\ss550\discord.json` and the whole path was re-run with no argument,
  so the window's Settings card and the crate read the same file.
