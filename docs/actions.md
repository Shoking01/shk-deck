# Actions — what a key *does*, and why some things are plugins instead

A deck key has two halves, and this file is about the first one:

- an **action** is what happens when the key is pressed;
- a **plugin** is something the window measures so a key can *show* it.

They are different on purpose, and the split is the rule that decides where a new
feature goes. `docs/plugins.md` is the other half.

## The catalogue

| action | what it does | where the work happens |
|---|---|---|
| `Launch` | runs a program, with arguments | the runner thread |
| `OpenUrl` | opens a URL with the system handler | the runner thread |
| `Keys` | sends a key combination to the focused window | the platform sender |
| `Media` | sends one of the system's media commands | the platform sender |
| `Folder`, `Back`, `Profile` | changes what the deck is showing | the window |
| `Plugin` | shows a live reading; pressing it runs nothing | the window and the face worker |

Two of them are *delivered* rather than *started*: a keystroke and a media
command. `internal::runner::command_for` — the function that turns an action into
a process — refuses both by name, because a key that is delivered has no program
to run. That refusal is a test, not a convention.

## Why Discord mute is not an action here

A deck key that silences the microphone and shows whether it is silenced was
built, measured and removed. Discord's local pipe offers two families of
commands, and neither one does both:

- `SET_VOICE_SETTINGS_2` and the `VOICE_SETTINGS_UPDATE_2` event are in the
  `rpc.local` scope, so any local connection may call them — but they are an
  **application-private sandbox**. The command changes nothing in the client and
  the event reports nothing about it: with the microphone muted and unmuted in
  Discord's own window for two minutes, the pipe carried only the initial
  snapshot.
- `SET_VOICE_SETTINGS` and `GET_VOICE_SETTINGS` are the real ones, and they need
  the `rpc.voice.read` / `rpc.voice.write` scopes. `AUTHORIZE` answers
  `5000 invalid_scope` for them — while the same call with `identify` grants
  consent and authenticates — and even authenticated, `GET_VOICE_SETTINGS`
  answers `4006 Not authenticated or invalid scope`. Discord reserves those
  scopes for applications with partner approval.

So the key that exists is `Keys` sending Discord's own global bind:

```text
ctrl+shift+m    toggle mute
ctrl+shift+d    toggle deafen
```

It mutes for real and needs no grant. It cannot show the state, because Discord
will not state it. `crates/ss550-discord` keeps the pipe and the probes that
proved this, and its crate docs say the same thing.

## Why media control is an action

`Media` sends `VK_MEDIA_PLAY_PAUSE`, `VK_MEDIA_NEXT_TRACK`, `VK_MEDIA_PREV_TRACK`
or `VK_MEDIA_STOP` — the codes a keyboard's own media keys send. Windows hands
them to the **active media session**, so the key obeys Spotify, a browser tab and
the system's own player alike, and nothing here has to know that Spotify exists.

That is the whole point of the split: *controlling* a player is a key doing
something, which is an action. *Showing* the track is a key displaying a
measurement, which is a plugin — and that is the next piece of work, with its own
problems (a title does not fit a 112-pixel key, and a reading key deliberately
runs nothing when pressed).

## The path of a press

```
key pressed on the deck
  → the window decodes it and looks the binding up in the profile
  → navigation?  applied by the window (it owns the page stack)
  → a reading?   nothing runs; the key redraws its face
  → anything else: handed to ActionRunner, on a background thread
       Launch / OpenUrl → a process is started
       Keys / Media     → the platform sender delivers it
```

The runner is a thread of its own so the window never waits for a program to
start, and every failure comes back as a sentence the status line can show —
`take_failure()` is polled by the same loop that draws.

## Why a keystroke carries a scan code

A keyboard sends **scan codes** and Windows derives the virtual key from them.
Text fields read the derived key, so a `SendInput` carrying only a virtual key
(`wScan: 0`) types in Notepad — and is invisible where it matters most. Raw
Input reports `MakeCode = 0x00` for such an event: a key that no keyboard has. A
game that binds a key and watches the device never sees it fire, in fullscreen
or in a window, while every text field on the same desktop keeps working.

Measured on this machine, with a Raw Input listener beside the sender:

```text
virtual key only   RAW MakeCode=0x00 Flags=0x00 VKey=0x87
scan code (now)    RAW MakeCode=0x76 Flags=0x00 VKey=0x87
```

So `ss550-tray` states the virtual key only long enough to translate it with
`MapVirtualKeyW(…, MAPVK_VK_TO_VSC_EX)` and sends the scan code
(`KEYEVENTF_SCANCODE`). Two details are not decoration:

- **The `E0` prefix.** Keys such as the arrows, Home/End/PageUp/PageDown,
  Insert/Delete, the Windows key and every media key are sent by a keyboard
  with a `0xE0` prefix (`KEYEVENTF_EXTENDEDKEY`). Without it the same scan code
  arrives as the keypad's twin — `0x48` with the prefix is the Up arrow, without
  it it is the numeric keypad's `8`. Windows' own translation table only reports
  the prefix for some of them, so the navigation cluster is stated explicitly in
  `keysend::is_extended`.
- **The hold.** The presses and the releases used to go out in one `SendInput`
  call, inside a single input tick. The keys are now held for
  `keysend::HOLD` (20 ms) between the two batches, because a game that samples
  the keyboard once per frame cannot see a key that is pressed and released
  between two frames.

A key the layout has no scan code for is refused with a sentence instead of
being sent as scan code zero, and the `Media` action goes out through the same
path, so it keeps the `E0` prefix its key has on a real keyboard.

## Adding one

1. A variant in `ss550_engine::Action`, with its own `serde` spelling.
2. `validate`, `summary`, `describe`, `and` the `match`es that must stay total
   (`without_wrapping_quotes`, `picture`, `glyph_letter`).
3. `command_for` must refuse it if it is delivered rather than started.
4. If the platform sends it, a method on `KeySender` — with a default that
   **refuses with a sentence**, so a platform without it says so instead of
   accepting a key that does nothing.
5. The editor: a `DraftKind`, a segment, a form, and the `load_draft` /
   `save_binding` arms.
6. Tests at each seam: the serde round trip, the refusal, the delivery.

That list is what `Media` went through, and it is the checklist the Spotify
plugin will need on the reading side.
