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
