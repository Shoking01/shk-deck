// REA RD MVP host - Electron main process (T8)
// Runs one real .sdPlugin (JS/HTML type) in a hidden Chromium page using the
// captured StreamDock launch contract, and bridges device <-> plugin.
//
//   StreamDockWebsocketSDK.exe -oem 293V3:0x0200:0x3000   (device server, :9002)
//   npm start                                              (this host)
const { app, BrowserWindow } = require("electron");
const { WebSocketServer } = require("ws");
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const { DeviceClient } = require("./device-client");

const PLUGINS_DIR = path.join(process.env.APPDATA, "HotSpot", "StreamDock", "plugins");
const COLUMNS = 5;
const ROWS = 3;

// MVP instance profile: one action placed at (0,0). A real profile editor comes later.
// Settings mirror what the plugin's property inspector would persist (zone/theme/select).
const INSTANCE = {
	plugin: "com.mirabox.streamdock.time",
	action: "com.mirabox.streamdock.time.action1", // World Time
	column: 0,
	row: 0,
	settings: { zone: "system", theme: "theme1", select: "0", title: "" },
	titleColor: "#000000",
};

let pluginPort = null;
let deviceId = null;
let appReady = false;
let deviceConnected = false;
const device = new DeviceClient();
const pluginSockets = new Set(); // connected plugin pages (MVP: one)
const instances = new Map(); // context -> { action, column, row, settings }

function makeContext() {
	const chars = "abcdefghijklmnopqrstuvwxyz0123456789";
	let s = "";
	for (let i = 0; i < 32; i++) s += chars[crypto.randomInt(chars.length)];
	return s;
}

function keyIndex(column, row) {
	return row * COLUMNS + column + 1; // 1-based, row-major (verified against device events)
}

function sendToPlugin(msg) {
	for (const ws of pluginSockets) {
		if (ws.readyState === ws.OPEN) ws.send(JSON.stringify(msg));
	}
}

function buildRegisterInfo(pluginUuid) {
	return {
		application: { language: "es_ES", platform: "windows", version: "2.9.0" },
		plugin: { uuid: pluginUuid },
		deviceType: 0,
		devices: [{
			id: deviceId,
			name: "SS550V3",
			size: { columns: COLUMNS, rows: ROWS },
			type: 0,
		}],
	};
}

function sendWillAppear(inst) {
	const context = makeContext();
	instances.set(context, { ...inst, settings: { ...(inst.settings || {}) } });
	sendToPlugin({
		action: inst.action,
		context,
		device: deviceId,
		event: "willAppear",
		payload: {
			coordinates: { column: inst.column, row: inst.row },
			isInMultiAction: false,
			settings: { ...(inst.settings || {}) },
			state: 0,
		},
	});
	console.log(`[host] willAppear -> ${inst.action} context=${context} @ (${inst.column},${inst.row})`);
	sendTitleParameters(inst, context);
}

function sendTitleParameters(inst, context) {
	sendToPlugin({
		action: inst.action,
		context,
		device: deviceId,
		event: "titleParametersDidChange",
		payload: {
			coordinates: { column: inst.column, row: inst.row },
			settings: { ...(inst.settings || {}) },
			state: 0,
			title: (inst.settings && inst.settings.title) || "",
			titleParameters: {
				fontFamily: "HarmonyOS Sans",
				fontSize: 8,
				fontStyle: "Regular",
				fontUnderline: false,
				showTitle: false,
				titleAlignment: "bottom",
				titleColor: inst.titleColor || "#ffffff",
			},
		},
	});
	console.log(`[host] titleParametersDidChange -> titleColor=${inst.titleColor || "#ffffff"}`);
}

function handlePluginMessage(ws, msg) {
	switch (msg.event) {
		case "registerPlugin":
			pluginSockets.add(ws);
			console.log(`[host] plugin registered: ${JSON.stringify(msg).slice(0, 200)}`);
			sendWillAppear(INSTANCE);
			break;
		case "setImage": {
			const inst = instances.get(msg.context);
			if (!inst) { console.log(`[host] setImage for unknown context ${msg.context}`); break; }
			const idx = keyIndex(inst.column, inst.row);
			device.setKeyImage(idx, msg.payload.image);
			console.log(`[host] setImage -> key ${idx} prefix=${msg.payload.image.slice(0, 30)} (${msg.payload.image.length} chars)`);
			try { // DEBUG: dump the exact image the plugin sent, for inspection
				const b64 = msg.payload.image.replace(/^data:image\/\w+;base64,/, "");
				fs.writeFileSync(path.join(__dirname, "last-setImage.png"), Buffer.from(b64, "base64"));
			} catch (e) {
				console.log(`[host] image dump failed: ${e.message}`);
			}
			break;
		}
		case "setTitle":
			console.log(`[host] setTitle (not rendered in MVP): ${JSON.stringify(msg.payload).slice(0, 100)}`);
			break;
		case "setSettings": {
			const inst = instances.get(msg.context);
			if (inst) inst.settings = msg.payload.settings || {};
			break;
		}
		case "getSettings": {
			const inst = instances.get(msg.context);
			ws.send(JSON.stringify({
				event: "didReceiveSettings",
				context: msg.context,
				payload: { settings: inst ? inst.settings : {} },
			}));
			break;
		}
		case "logMessage":
			console.log(`[plugin] ${msg.payload.message}`);
			break;
		default:
			console.log(`[host] plugin event ${msg.event}`);
	}
}

function launchPlugin() {
	const dir = path.join(PLUGINS_DIR, `${INSTANCE.plugin}.sdPlugin`);
	const manifest = JSON.parse(fs.readFileSync(path.join(dir, "manifest.json"), "utf8"));
	// Mirabox manifests have no top-level UUID; the plugin uuid is manifest.Name.
	const pluginUuid = INSTANCE.plugin; // real bundle uuid; manifest.Name is a display name
	const codePath = path.join(dir, manifest.CodePath);
	console.log(`[host] launching plugin ${pluginUuid} (${codePath})`);

	const win = new BrowserWindow({
		show: false,
		webPreferences: {
			contextIsolation: false,
			nodeIntegration: false,
			sandbox: false,
			webSecurity: false, // file:// workers and cross-file loads, MVP only
			backgroundThrottling: false, // hidden window must keep ticking
		},
	});
	win.webContents.on("console-message", (_e, _level, message) => {
		console.log(`[plugin-console] ${message}`);
	});
	win.webContents.on("did-finish-load", () => {
		const infoLiteral = JSON.stringify(JSON.stringify(buildRegisterInfo(pluginUuid)));
		const call = `connectElgatoStreamDeckSocket(${pluginPort}, ${JSON.stringify(pluginUuid)}, "registerPlugin", ${infoLiteral});`;
		win.webContents.executeJavaScript(call);
		console.log("[host] connect call injected");
	});
	win.loadFile(codePath);
}

function maybeStart() {
	if (appReady && deviceConnected) launchPlugin();
}

// Device + plugin-server wiring lives at module level: the SDK server may
// connect before Electron fires whenReady, and we must not miss its events.
const wss = new WebSocketServer({ port: 0 }, () => {
	pluginPort = wss.address().port;
	console.log(`[host] plugin WS server on ws://127.0.0.1:${pluginPort}`);
});
wss.on("connection", (ws) => {
	ws.on("message", (data) => {
		let msg;
		try { msg = JSON.parse(data.toString()); } catch { return; }
		handlePluginMessage(ws, msg);
	});
	ws.on("close", () => pluginSockets.delete(ws));
});

device.on("server-connect", () => console.log(`[host] SDK server at ${device.url}`));
device.on("device-connect", (payload) => {
	deviceId = (payload && payload.SerialNumber) || "SS550V3";
	console.log(`[host] device connected: ${JSON.stringify(payload)}`);
	device.startRead();
	deviceConnected = true;
	maybeStart();
});
device.on("key", (p) => {
	const keyId = p.keyId;
	const column = (keyId - 1) % COLUMNS;
	const row = Math.floor((keyId - 1) / COLUMNS);
	for (const [context, inst] of instances) {
		if (inst.column === column && inst.row === row) {
			sendToPlugin({
				action: inst.action,
				context,
				device: deviceId,
				event: p.keyUpOrKeyDown, // keyDown | keyUp
				payload: {
					controller: "Keypad",
					coordinates: { column, row },
					isInMultiAction: false,
					settings: inst.settings || {},
					state: 0,
					userDesiredState: 1,
				},
			});
			console.log(`[host] ${p.keyUpOrKeyDown} -> ${inst.action} @ (${column},${row})`);
		}
	}
});
device.on("device-disconnect", () => {
	deviceConnected = false;
	console.log("[host] device disconnected");
});
device.on("error", (e) => console.log(`[host] device ws error: ${e.message}`));

app.whenReady().then(() => {
	appReady = true;
	maybeStart();
});
