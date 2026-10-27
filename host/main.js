// REA RD MVP host - Electron main process (T8/T10)
// Runs real .sdPlugin plugins in hidden Chromium pages using the captured
// StreamDock launch contract, and bridges device <-> plugins.
//
//   StreamDockWebsocketSDK.exe -oem 293V3:0x0200:0x3000   (device server, :9002)
//   npm start                                              (this host)
//
// Fase 2: N instances from a profile file (host/profile.json), one page per
// distinct plugin, targeted event routing, willDisappear on shutdown.
const { app, BrowserWindow, ipcMain, Tray, Menu, nativeImage } = require("electron");
const { WebSocketServer } = require("ws");
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const { DeviceClient } = require("./device-client");

const PLUGINS_DIR = path.join(process.env.APPDATA, "HotSpot", "StreamDock", "plugins");
const PROFILE_PATH = path.join(__dirname, "profile.json");
const COLUMNS = 5;
const ROWS = 3;

// ---- Profile (S9) ----------------------------------------------------------
// instances: [{ plugin, action, column, row, settings?, titleColor? }, ...]
let profileInstances = [];
try {
	profileInstances = JSON.parse(fs.readFileSync(PROFILE_PATH, "utf8")).instances || [];
	console.log(`[host] profile: ${profileInstances.length} instance(s)`);
} catch (e) {
	console.log(`[host] no profile.json (${e.message}) - starting with an empty profile`);
}

// One page per distinct plugin bundle.
const pluginsToLaunch = [...new Set(profileInstances.map((i) => i.plugin))];
const instancesByPlugin = new Map();
for (const inst of profileInstances) {
	if (!instancesByPlugin.has(inst.plugin)) instancesByPlugin.set(inst.plugin, []);
	instancesByPlugin.get(inst.plugin).push(inst);
}

// ---- State -----------------------------------------------------------------
let pluginPort = null;
let deviceId = null;
let appReady = false;
let deviceConnected = false;
const device = new DeviceClient();
const socketByPlugin = new Map(); // plugin uuid -> WebSocket
const instances = new Map(); // context -> { plugin, action, column, row, settings, titleColor }
const windows = new Map(); // plugin uuid -> BrowserWindow

function makeContext() {
	const chars = "abcdefghijklmnopqrstuvwxyz0123456789";
	let s = "";
	for (let i = 0; i < 32; i++) s += chars[crypto.randomInt(chars.length)];
	return s;
}

function keyIndex(column, row) {
	return row * COLUMNS + column + 1; // 1-based, row-major (verified against device events)
}

function sendToPlugin(pluginUuid, msg) {
	const ws = socketByPlugin.get(pluginUuid);
	if (ws && ws.readyState === ws.OPEN) ws.send(JSON.stringify(msg));
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

function willAppearPayload(inst) {
	return {
		coordinates: { column: inst.column, row: inst.row },
		isInMultiAction: false,
		settings: { ...(inst.settings || {}) },
		state: 0,
	};
}

function sendWillAppear(inst) {
	const context = makeContext();
	instances.set(context, { ...inst, settings: { ...(inst.settings || {}) } });
	sendToPlugin(inst.plugin, {
		action: inst.action,
		context,
		device: deviceId,
		event: "willAppear",
		payload: willAppearPayload(inst),
	});
	console.log(`[host] willAppear -> ${inst.action} context=${context} @ (${inst.column},${inst.row})`);
	sendTitleParameters(inst, context);
}

function sendTitleParameters(inst, context) {
	sendToPlugin(inst.plugin, {
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
	console.log(`[host] titleParametersDidChange -> ${inst.action} titleColor=${inst.titleColor || "#ffffff"}`);
}

function sendWillDisappear(context, inst) {
	sendToPlugin(inst.plugin, {
		action: inst.action,
		context,
		device: deviceId,
		event: "willDisappear",
		payload: willAppearPayload(inst),
	});
}

function handlePluginMessage(ws, msg) {
	switch (msg.event) {
		case "registerPlugin":
			socketByPlugin.set(msg.uuid, ws);
			console.log(`[host] plugin registered: ${JSON.stringify(msg).slice(0, 200)}`);
			for (const inst of instancesByPlugin.get(msg.uuid) || []) {
				// S10: instances assigned live already have a context when their page
				// re-registers; only deliver willAppear to instances without one.
				if (!hasLiveContext(inst)) sendWillAppear(inst);
			}
			break;
		case "setImage": {
			const inst = instances.get(msg.context);
			if (!inst) { console.log(`[host] setImage for unknown context ${msg.context}`); break; }
			const idx = keyIndex(inst.column, inst.row);
			device.setKeyImage(idx, msg.payload.image);
			console.log(`[host] setImage -> key ${idx} prefix=${msg.payload.image.slice(0, 30)} (${msg.payload.image.length} chars)`);
			try { // DEBUG: dump the exact image the plugin sent, per key, for inspection
				const b64 = msg.payload.image.replace(/^data:image\/\w+;base64,/, "");
				fs.writeFileSync(path.join(__dirname, `last-setImage-key${idx}.png`), Buffer.from(b64, "base64"));
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

function launchPlugin(pluginUuid) {
	if (windows.has(pluginUuid)) return; // one page per plugin bundle
	const dir = path.join(PLUGINS_DIR, `${pluginUuid}.sdPlugin`);
	const manifest = JSON.parse(fs.readFileSync(path.join(dir, "manifest.json"), "utf8"));
	const codePath = path.join(dir, manifest.CodePath);
	console.log(`[host] launching plugin ${pluginUuid} (${codePath})`);

	const win = new BrowserWindow({
		show: false,
		webPreferences: {
			contextIsolation: false,
			nodeIntegration: false,
			sandbox: false,
			preload: path.join(__dirname, "preload.js"),
			webSecurity: false, // file:// workers and cross-file loads, MVP only
			backgroundThrottling: false, // hidden window must keep ticking
		},
	});
	windows.set(pluginUuid, win);
	win.webContents.on("console-message", (_e, _level, message) => {
		console.log(`[plugin-console] ${message}`);
	});
	win.webContents.on("did-finish-load", () => {
		const infoLiteral = JSON.stringify(JSON.stringify(buildRegisterInfo(pluginUuid)));
		const call = `connectElgatoStreamDeckSocket(${pluginPort}, ${JSON.stringify(pluginUuid)}, "registerPlugin", ${infoLiteral});`;
		win.webContents.executeJavaScript(call);
		console.log(`[host] connect call injected -> ${pluginUuid}`);
	});
	win.loadFile(codePath);
}

// ---- Config UI (S10) -------------------------------------------------------
let configWindow = null;

// True when any live context in `instances` occupies the same plugin/key as inst.
function hasLiveContext(inst) {
	for (const live of instances.values()) {
		if (live.plugin === inst.plugin && live.column === inst.column && live.row === inst.row) return true;
	}
	return false;
}

function rebuildInstancesByPlugin() {
	instancesByPlugin.clear();
	for (const inst of profileInstances) {
		if (!instancesByPlugin.has(inst.plugin)) instancesByPlugin.set(inst.plugin, []);
		instancesByPlugin.get(inst.plugin).push(inst);
	}
}

function persistProfile() {
	fs.writeFileSync(PROFILE_PATH, JSON.stringify({ instances: profileInstances }, null, 2));
}

function notifyConfigWindow() {
	if (configWindow && !configWindow.isDestroyed()) {
		configWindow.webContents.send("profile-changed", { instances: profileInstances });
	}
}

// Diff old vs new profile by (column,row) and apply it live: willDisappear for
// removed/changed instances (their existing contexts), willAppear for added or
// changed ones (fresh context via sendWillAppear). Reuses the existing helpers;
// sends stay targeted through socketByPlugin and no-op while a socket is closed.
function applyProfileChange(newInstances) {
	const oldByKey = new Map(profileInstances.map((i) => [`${i.column},${i.row}`, i]));
	const newByKey = new Map(newInstances.map((i) => [`${i.column},${i.row}`, i]));
	const sameInstance = (a, b) =>
		a.plugin === b.plugin &&
		a.action === b.action &&
		JSON.stringify(a.settings || {}) === JSON.stringify(b.settings || {}) &&
		(a.titleColor || "") === (b.titleColor || "");

	// 1) removed or changed -> willDisappear for their live contexts, then drop
	const changedKeys = new Set();
	for (const [key, oldInst] of oldByKey) {
		const newInst = newByKey.get(key);
		if (!newInst || !sameInstance(oldInst, newInst)) changedKeys.add(key);
	}
	for (const [context, inst] of [...instances]) {
		if (changedKeys.has(`${inst.column},${inst.row}`)) {
			sendWillDisappear(context, inst);
			instances.delete(context);
			console.log(`[host] willDisappear + dropped context ${context} @ (${inst.column},${inst.row})`);
		}
	}

	// 2) added or changed -> launch the page if needed, then willAppear
	for (const [key, inst] of newByKey) {
		if (!changedKeys.has(key)) continue;
		launchPlugin(inst.plugin); // no-op when the page already exists (one page per plugin)
		const ws = socketByPlugin.get(inst.plugin);
		if (ws && ws.readyState === ws.OPEN) {
			sendWillAppear(inst);
		}
		// else: the page is still loading or reconnecting; registerPlugin delivers
		// willAppear via instancesByPlugin (hasLiveContext keeps it single).
	}

	// 3) commit, persist, notify the config window
	profileInstances = newInstances;
	rebuildInstancesByPlugin();
	persistProfile();
	notifyConfigWindow();
}

function openConfigWindow() {
	if (configWindow && !configWindow.isDestroyed()) return;
	configWindow = new BrowserWindow({
		width: 700,
		height: 520,
		title: "REA RD - Config",
		show: true,
		webPreferences: {
			preload: path.join(__dirname, "config-preload.js"),
			contextIsolation: true, // default; the renderer only talks via the bridge
		},
	});
	configWindow.on("closed", () => { configWindow = null; });
	configWindow.webContents.on("console-message", (_e, _level, message) => {
		console.log(`[config-console] ${message}`);
	});
	configWindow.loadFile(path.join(__dirname, "config.html"));
}

// ---- Tray (reopen path for the config window) -------------------------------
// Self-generated 16x16 icon (no third-party assets). Left click reopens the
// config window; the context menu offers the same plus Quit.
const TRAY_ICON = "iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAAIklEQVR4nGPQ0An6TwlmoJoBNW7fSMKjBowaMFwNGLDcCABMAT8/H2LMPwAAAABJRU5ErkJggg==";
let tray = null;

function createTray() {
	if (tray) return;
	tray = new Tray(nativeImage.createFromDataURL(`data:image/png;base64,${TRAY_ICON}`));
	tray.setToolTip("REA RD - StreamDock host");
	tray.on("click", () => openConfigWindow());
	tray.setContextMenu(Menu.buildFromTemplate([
		{ label: "Open config", click: () => openConfigWindow() },
		{ type: "separator" },
		{ label: "Quit", click: () => app.quit() },
	]));
	console.log("[host] tray created (click = open config)");
}

// Scan PLUGINS_DIR for *.sdPlugin bundles: uuid = directory name minus suffix,
// display name = manifest.Name, actions = manifest.Actions[] (UUID + Name).
function scanInstalledPlugins() {
	const out = [];
	let entries;
	try {
		entries = fs.readdirSync(PLUGINS_DIR, { withFileTypes: true });
	} catch (e) {
		console.log(`[host] cannot read plugins dir: ${e.message}`);
		return out;
	}
	for (const ent of entries) {
		if (!ent.isDirectory() || !ent.name.endsWith(".sdPlugin")) continue;
		const uuid = ent.name.slice(0, -".sdPlugin".length);
		try {
			const manifest = JSON.parse(fs.readFileSync(path.join(PLUGINS_DIR, ent.name, "manifest.json"), "utf8"));
			out.push({
				uuid,
				name: manifest.Name || uuid,
				actions: (Array.isArray(manifest.Actions) ? manifest.Actions : [])
					.filter((a) => a && a.UUID)
					.map((a) => ({ uuid: a.UUID, name: a.Name || a.UUID })),
			});
		} catch (e) {
			console.log(`[host] skipping plugin ${ent.name}: ${e.message}`);
		}
	}
	out.sort((a, b) => a.name.localeCompare(b.name));
	return out;
}

function validCoordinates(p) {
	const column = Number(p.column);
	const row = Number(p.row);
	if (Number.isInteger(column) && column >= 0 && column < COLUMNS &&
		Number.isInteger(row) && row >= 0 && row < ROWS) {
		return { column, row };
	}
	return null;
}

function maybeStart() {
	if (appReady && deviceConnected) {
		for (const pluginUuid of pluginsToLaunch) launchPlugin(pluginUuid);
		openConfigWindow();
	}
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
	ws.on("close", () => {
		for (const [uuid, s] of socketByPlugin) if (s === ws) socketByPlugin.delete(uuid);
	});
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
			sendToPlugin(inst.plugin, {
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

// ---- Config UI IPC (S10) ---------------------------------------------------
ipcMain.handle("reard:listPlugins", () => scanInstalledPlugins());

ipcMain.handle("reard:getProfile", () => ({ instances: profileInstances }));

ipcMain.handle("reard:assign", (_event, payload) => {
	try {
		const p = payload || {};
		const coords = validCoordinates(p);
		if (!coords) {
			return { ok: false, error: `coordinates out of range (columns 0..${COLUMNS - 1}, rows 0..${ROWS - 1})` };
		}
		if (typeof p.plugin !== "string" || !p.plugin.trim()) return { ok: false, error: "missing plugin uuid" };
		if (typeof p.action !== "string" || !p.action.trim()) return { ok: false, error: "missing action uuid" };
		const settings = p.settings === undefined || p.settings === null ? {} : p.settings;
		if (typeof settings !== "object" || Array.isArray(settings)) {
			return { ok: false, error: "settings must be a JSON object" };
		}
		const titleColor = typeof p.titleColor === "string" && p.titleColor.trim() ? p.titleColor.trim() : "#ffffff";
		const inst = {
			plugin: p.plugin.trim(),
			action: p.action.trim(),
			column: coords.column,
			row: coords.row,
			settings,
			titleColor,
		};
		const next = profileInstances.filter((i) => !(i.column === coords.column && i.row === coords.row));
		next.push(inst);
		next.sort((a, b) => (a.row - b.row) || (a.column - b.column));
		applyProfileChange(next);
		return { ok: true };
	} catch (e) {
		return { ok: false, error: e.message };
	}
});

ipcMain.handle("reard:clear", (_event, payload) => {
	try {
		const p = payload || {};
		const coords = validCoordinates(p);
		if (!coords) {
			return { ok: false, error: `coordinates out of range (columns 0..${COLUMNS - 1}, rows 0..${ROWS - 1})` };
		}
		const next = profileInstances.filter((i) => !(i.column === coords.column && i.row === coords.row));
		if (next.length === profileInstances.length) return { ok: true }; // nothing assigned there
		applyProfileChange(next);
		return { ok: true };
	} catch (e) {
		return { ok: false, error: e.message };
	}
});

app.whenReady().then(() => {
	appReady = true;
	createTray();
	maybeStart();
});

app.on("before-quit", () => {
	for (const [context, inst] of instances) sendWillDisappear(context, inst);
});
