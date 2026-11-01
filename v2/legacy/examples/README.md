# Plugin Examples

This directory contains example implementations for creating custom Stream Deck plugins that work with REA RD v2.

## Architecture

Plugins run as **native executables** that communicate with the host via WebSocket. Each plugin:

1. Receives its manifest as a command-line argument
2. Connects to the host's WebSocket server
3. Follows the Elgato SDK1 WebSocket protocol
4. Sends images, titles, and receives events

## Creating Your Own Plugin

### Step 1: Create the Plugin Directory

```
my-plugin.exe
my-plugin/
├── plugin.json          # Plugin manifest
└── ...                  # Plugin files
```

### Step 2: Create the Manifest

Create `plugin.json`:

```json
{
  "Name": "My Plugin",
  "Version": "1.0.0",
  "CodePathWin": "my-plugin.exe",
  "Events": ["willAppear", "willDisappear", "keyDown", "keyUp"]
}
```

### Step 3: Implement the Plugin

The plugin executable receives:
- `argv[1]`: JSON manifest with plugin metadata
- Reads from stdin: WebSocket messages (after connecting)
- Writes to stdout: WebSocket protocol messages

### Step 4: Register in Profile

Add to `host/profile.json`:

```json
{
  "instances": [
    {
      "plugin": "com.yourcompany.myplugin",
      "action": "my-action",
      "column": 0,
      "row": 0,
      "settings": {}
    }
  ]
}
```

## Available Examples

### plugin_time (Rust)

A minimal time plugin template showing:
- Manifest parsing
- Plugin lifecycle
- Image sending pattern

Build: `cd examples/plugin_time && cargo build --release`

## SDK1 Protocol Reference

### Host → Plugin Messages

```json
{ "event": "willAppear", "context": "...", "payload": { "settings": {} } }
{ "event": "keyDown", "context": "...", "payload": { "coordinates": { "column": 0, "row": 0 } } }
{ "event": "keyUp", "context": "...", "payload": { "coordinates": { "column": 0, "row": 0 } } }
```

### Plugin → Host Messages

```json
{ "event": "setImage", "context": "...", "payload": { "image": "data:image/png;base64,..." } }
{ "event": "setTitle", "context": "...", "payload": { "title": "My Title" } }
{ "event": "didReceiveSettings", "context": "...", "payload": { "settings": {} } }
```

## Debugging

1. Run `reard-v2.exe` and check console output
2. Look for `[plugin-uuid]` log messages
3. Verify WebSocket connection on the port shown in console

## Notes

- Plugins must be **standalone executables** - no JavaScript/HTML
- The host discovers plugins in `%APPDATA%\HotSpot\StreamDeck\plugins\*.sdPlugin`
- Each plugin gets its own process - crashes are isolated
- Images are base64-encoded PNGs (256x256 pixels)