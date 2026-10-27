// REA RD MVP host - Electron main process (T8/T10)
// Runs real .sdPlugin plugins in hidden Chromium pages using the captured
// StreamDock launch contract, and bridges device <-> plugins.
//
//   StreamDockWebsocketSDK.exe -oem 293V3:0x0200:0x3000   (device server, :9002)
//   npm start                                              (this host)
//
// Fase 2: N instances from a profile file (host/profile.json), one page per
// distinct plugin, targeted event routing, willDisappear on shutdown.
const { app, BrowserWindow, ipcMain, Tray, Menu, nativeImage, shell, dialog } = require("electron");
const { WebSocketServer } = require("ws");
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const { spawn } = require("child_process");
const os = require("os");
const { DeviceClient } = require("./device-client");

// Log instead of the default modal dialog: an uncaught exception in the main
// process used to freeze the host silently (the dialog blocks the event loop
// and nobody is watching). Keep running; the failing feature logs its own
// recovery path.
process.on("uncaughtException", (e) => {
	console.log(`[host] uncaught exception: ${(e && e.stack) || e}`);
});

// Mirror console.log into host/host.log: the stdout capture (shell pipe) can
// die mid-session while the host keeps running; the file always has the story.
const logFile = fs.createWriteStream(path.join(__dirname, "host.log"), { flags: "a" });
const origConsoleLog = console.log.bind(console);
console.log = (...args) => {
	origConsoleLog(...args);
	logFile.write(`${new Date().toISOString()} ${args.join(" ")}\n`);
};

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
const setImageLogCount = new Map(); // key index -> paint count, throttles setImage logging
const lastImageByKey = new Map(); // key index -> last image the owning context sent; used to repaint after a device reconnect
const titleByKey = new Map(); // key index -> active setTitle text, composited over the key image
const nativeChildren = new Map(); // plugin uuid -> spawned child process (native .exe bundles)
const nativePluginByToken = new Map(); // launch token -> plugin uuid; native exes register under the argv token

// Stream Station passes a random 32-char uppercase alnum token as -pluginUUID
// (captured: RI1UY9YF6G4XPN8A1P3O6JW6M08I0993); the exe echoes it back as its
// registerPlugin uuid, so we map token -> real plugin uuid for routing.
function makeLaunchToken() {
	const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
	let t = "";
	for (let i = 0; i < 32; i++) t += chars[crypto.randomInt(chars.length)];
	return t;
}
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

function buildRegisterInfo(pluginUuid, manifestVersion) {
	return {
		application: {
			font: "HarmonyOS Sans",
			language: "es", // short code: Mirabox plugins fetch ../${$lang}.json
			platform: "windows",
			platformVersion: os.release(),
			version: "2.9",
		},
		// Captured from Stream Station's live launch argv (pid 15796); native
		// exes may read these for overlay drawing.
		colors: {
			buttonMouseOverBackgroundColor: "#464646FF",
			buttonPressedBackgroundColor: "#303030FF",
			buttonPressedBorderColor: "#646464FF",
			buttonPressedTextColor: "#969696FF",
			highlightColor: "#0078FFFF",
		},
		devicePixelRatio: 1,
		plugin: { uuid: pluginUuid, version: manifestVersion || "1.0" },
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

// setTitle layer: Mirabox plugins deliver all their text (weather status,
// temperatures) through setTitle, but the device only accepts images, so the
// title is composited over the key's last image in a hidden renderer page.
const KEY_PX = 126; // SS550V3 key resolution, measured from plugin PNG dumps
let titleRendererWin = null;
let titleRendererReady = null;
function ensureTitleRenderer() {
	if (!titleRendererReady) {
		titleRendererWin = new BrowserWindow({
			show: false,
			webPreferences: { offscreen: true, backgroundThrottling: false },
		});
		titleRendererReady = titleRendererWin.loadURL("about:blank");
	}
	return titleRendererReady;
}
function renderTitle(baseImg, title) {
	ensureTitleRenderer();
	const renderFn = function (imgUrl, text, w, h) {
		return new Promise((resolve) => {
			const c = document.createElement("canvas");
			c.width = w;
			c.height = h;
			const ctx = c.getContext("2d");
			const draw = () => {
				if (imgUrl) ctx.drawImage(img, 0, 0, w, h);
				let size = Math.round(w * 0.22);
				const font = (s) => `bold ${s}px "Segoe UI", Arial, sans-serif`;
				ctx.font = font(size);
				while (ctx.measureText(text).width > w * 0.9 && size > 8) {
					size -= 2;
					ctx.font = font(size);
				}
				ctx.textAlign = "center";
				ctx.textBaseline = "middle";
				ctx.lineWidth = Math.max(2, Math.round(w * 0.04));
				ctx.strokeStyle = "rgba(0,0,0,0.85)";
				ctx.strokeText(text, w / 2, h / 2);
				ctx.fillStyle = "#ffffff";
				ctx.fillText(text, w / 2, h / 2);
				resolve(c.toDataURL("image/png").split(",")[1]);
			};
			if (imgUrl) {
				const img = new Image();
				img.onload = draw;
				img.onerror = draw;
				img.src = imgUrl;
			} else draw();
		});
	};
	const expr = `(${renderFn.toString()})(${JSON.stringify(baseImg || null)}, ${JSON.stringify(title)}, ${KEY_PX}, ${KEY_PX})`;
	return titleRendererReady.then(() => titleRendererWin.webContents.executeJavaScript(expr));
}

function applyTitleLayer(idx) {
	// Fire-and-forget compositing: draw the cached title over the cached image
	// and push the result to the device.
	renderTitle(lastImageByKey.get(idx), titleByKey.get(idx))
		.then((img) => { if (img) device.setKeyImage(idx, img); })
		.catch((e) => console.log(`[host] title render failed: ${e.message}`));
}

const blankFn = function (w, h) {
	const c = document.createElement("canvas");
	c.width = w;
	c.height = h;
	const ctx = c.getContext("2d");
	ctx.fillStyle = "#000000";
	ctx.fillRect(0, 0, w, h);
	return c.toDataURL("image/png").split(",")[1];
};

let blankKeyImagePromise = null;
function getBlankKeyImage() {
	// One shared 126x126 solid black PNG, rendered once in the hidden page.
	if (!blankKeyImagePromise) {
		blankKeyImagePromise = ensureTitleRenderer().then(() =>
			titleRendererWin.webContents.executeJavaScript(`(${blankFn.toString()})(${KEY_PX}, ${KEY_PX})`));
	}
	return blankKeyImagePromise;
}

let blackBackgroundPromise = null;
function getBlackBackground() {
	// Solid black device background layer (800x480; the SDK scales it).
	if (!blackBackgroundPromise) {
		blackBackgroundPromise = ensureTitleRenderer().then(() =>
			titleRendererWin.webContents.executeJavaScript(`(${blankFn.toString()})(800, 480)`));
	}
	return blackBackgroundPromise;
}

function paintAllKeys() {
	// Own the whole device surface: the background layer goes black (the device
	// keeps whatever background was last stored in it - Stream Station's photo
	// - forever otherwise), assigned keys repaint from cache, unassigned keys
	// go black.
	getBlackBackground()
		.then((img) => device.setBackgroundImage(img))
		.catch((e) => console.log(`[host] background paint failed: ${e.message}`));
	for (let idx = 1; idx <= COLUMNS * ROWS; idx++) {
		const cached = lastImageByKey.get(idx);
		if (cached) {
			device.setKeyImage(idx, cached);
			if (titleByKey.has(idx)) applyTitleLayer(idx);
		} else {
			getBlankKeyImage()
				.then((img) => device.setKeyImage(idx, img))
				.catch((e) => console.log(`[host] blank key paint failed: ${e.message}`));
		}
	}
	console.log(`[host] painted all ${COLUMNS * ROWS} keys (${lastImageByKey.size} from cache, rest blank)`);
}

function handlePluginMessage(ws, msg) {
	switch (msg.event) {
		case "registerPlugin": {
			// Native exes register under the -pluginUUID token we passed; resolve
			// it to the real plugin uuid so routing stays uniform.
			const realUuid = nativePluginByToken.get(msg.uuid) || msg.uuid;
			if (realUuid !== msg.uuid) console.log(`[host] native registration ${msg.uuid} -> ${realUuid}`);
			socketByPlugin.set(realUuid, ws);
			console.log(`[host] plugin registered: ${JSON.stringify(msg).slice(0, 200)}`);
			for (const inst of instancesByPlugin.get(realUuid) || []) {
				// S10: instances assigned live already have a context when their page
				// re-registers; only deliver willAppear to instances without one.
				if (!hasLiveContext(inst)) sendWillAppear(inst);
			}
			break;
		}
		case "setImage": {
			const inst = instances.get(msg.context);
			if (!inst) { console.log(`[host] setImage for unknown context ${msg.context}`); break; }
			if (!msg.payload || typeof msg.payload.image !== "string" || !msg.payload.image) {
				// Native plugins send an empty first frame before their data
				// pipeline warms up; forwarding it just noise on the HID bus.
				console.log(`[host] setImage with empty image from ${inst.plugin}, ignored`);
				break;
			}
			const idx = keyIndex(inst.column, inst.row);
			if (lastImageByKey.get(idx) === msg.payload.image) break; // identical repaint: skip the device write
			device.setKeyImage(idx, msg.payload.image);
			// Throttled logging: per-second redraws (clock) flood the log and hide
			// everything else; log the first paint per key, then every 50th.
			const n = (setImageLogCount.get(idx) || 0) + 1;
			setImageLogCount.set(idx, n);
			lastImageByKey.set(idx, msg.payload.image);
			if (titleByKey.has(idx)) applyTitleLayer(idx); // re-composite the active title over the new image
			if (n === 1 || n % 50 === 0) {
				console.log(`[host] setImage -> key ${idx} #${n} (${msg.payload.image.length} chars)`);
			}
			try { // DEBUG: dump the exact image the plugin sent, per key, for inspection
				const b64 = msg.payload.image.replace(/^data:image\/\w+;base64,/, "");
				fs.writeFileSync(path.join(__dirname, `last-setImage-key${idx}.png`), Buffer.from(b64, "base64"));
			} catch (e) {
				console.log(`[host] image dump failed: ${e.message}`);
			}
			break;
		}
		case "setTitle": {
			const inst = instances.get(msg.context);
			if (!inst) { console.log(`[host] setTitle for unknown context ${msg.context}`); break; }
			const idx = keyIndex(inst.column, inst.row);
			const title = msg.payload && msg.payload.title != null ? String(msg.payload.title) : "";
			console.log(`[host] setTitle -> key ${idx}: "${title}"`);
			if (title === "") {
				titleByKey.delete(idx);
				const base = lastImageByKey.get(idx);
				if (base) device.setKeyImage(idx, base); // empty title clears the text layer
			} else {
				titleByKey.set(idx, title);
				applyTitleLayer(idx);
			}
			break;
		}
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
	if (windows.has(pluginUuid) || nativeChildren.has(pluginUuid)) return; // one page/process per plugin bundle
	const dir = path.join(PLUGINS_DIR, `${pluginUuid}.sdPlugin`);
	const manifest = JSON.parse(fs.readFileSync(path.join(dir, "manifest.json"), "utf8"));
	const entry = manifest.CodePath || manifest.CodePathWin || "";
	if (/\.exe$/i.test(entry)) {
		// Native plugin: spawn the exe with the argv contract captured live from
		// Stream Station (pid 15796): -port -pluginUUID -registerEvent -info.
		const exePath = path.join(dir, entry);
		const token = makeLaunchToken();
		nativePluginByToken.set(token, pluginUuid);
		const info = buildRegisterInfo(pluginUuid, manifest.Version);
		const args = [
			"-port", String(pluginPort),
			"-pluginUUID", token,
			"-registerEvent", "registerPlugin",
			"-info", JSON.stringify(info),
		];
		console.log(`[host] launching native plugin ${pluginUuid} (${exePath})`);
		const child = spawn(exePath, args, { cwd: dir, windowsHide: true, stdio: "ignore" });
		child.on("error", (e) => console.log(`[host] native spawn failed for ${pluginUuid}: ${e.message}`));
		child.on("exit", (code) => {
			console.log(`[host] native plugin ${pluginUuid} exited (code ${code === null ? "signal" : code})`);
			if (nativeChildren.get(pluginUuid) === child) nativeChildren.delete(pluginUuid);
		});
		nativeChildren.set(pluginUuid, child);
		return;
	}
	if (!/\.html?$/i.test(entry)) {
		throw new Error(`plugin ${pluginUuid} has unsupported entry "${entry || "none"}" (neither HTML page nor .exe)`);
	}
	const codePath = path.join(dir, entry);
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
	for (const key of newByKey.keys()) {
		if (!oldByKey.has(key)) changedKeys.add(key); // newly assigned keys must launch too
	}
	for (const [context, inst] of [...instances]) {
		if (changedKeys.has(`${inst.column},${inst.row}`)) {
			sendWillDisappear(context, inst);
			instances.delete(context);
			const droppedIdx = keyIndex(inst.column, inst.row);
			titleByKey.delete(droppedIdx);
			lastImageByKey.delete(droppedIdx);
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
			// Only HTML-page bundles are runnable by this host; native executables
			// (CodePathWin pointing at an .exe) stay out of the picker.
			const entry = manifest.CodePath || manifest.CodePathWin || "";
			if (/\.html?$/i.test(entry)) {
				out.push({
					uuid,
					name: manifest.Name || uuid,
					actions: (Array.isArray(manifest.Actions) ? manifest.Actions : [])
						.filter((a) => a && a.UUID)
						.map((a) => ({ uuid: a.UUID, name: a.Name || a.UUID, settingsEditor: a.REARDSettingsEditor || null })),
				});
			} else if (/\.exe$/i.test(entry)) {
				// Native bundle: runnable as a child process (S11).
				out.push({
					uuid,
					name: manifest.Name || uuid,
					native: true,
					actions: (Array.isArray(manifest.Actions) ? manifest.Actions : [])
						.filter((a) => a && a.UUID)
						.map((a) => ({ uuid: a.UUID, name: a.Name || a.UUID, settingsEditor: a.REARDSettingsEditor || null })),
				});
			} else {
				console.log(`[host] skipping plugin ${ent.name}: unsupported entry "${entry || "none"}"`);
			}
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

let started = false;
function maybeStart() {
	if (appReady && deviceConnected && !started) {
		started = true;
		for (const pluginUuid of pluginsToLaunch) {
			try { launchPlugin(pluginUuid); }
			catch (e) { console.log(`[host] launch failed for ${pluginUuid}: ${e.message}`); }
		}
		try { paintAllKeys(); } // blank every key we do not own before plugins paint theirs
		catch (e) { console.log(`[host] paintAllKeys failed: ${e.message}`); }
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
	// started flips true at the first maybeStart and never resets, so on any
	// later device-connect it means the USB device re-enumerated. (deviceConnected
	// is cleared by device-disconnect, so it is always false here.)
	const isReconnect = started;
	deviceId = (payload && payload.SerialNumber) || "SS550V3";
	console.log(`[host] device connected: ${JSON.stringify(payload)}`);
	device.startRead();
	deviceConnected = true;
	maybeStart();
	if (isReconnect && lastImageByKey.size) {
		// Re-enumeration clears the device key memory; repaint every key from
		// cache and blank the rest (covers static plugins and leftovers).
		paintAllKeys();
	}
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

// Live previews for the config grid: the exact images/titles the device shows.
ipcMain.handle("reard:getKeyImages", () => {
	const images = {};
	for (const [idx, img] of lastImageByKey) images[idx] = img;
	const titles = {};
	for (const [idx, t] of titleByKey) titles[idx] = t;
	return { images, titles };
});

ipcMain.handle("reard:getStatus", () => ({
	connected: deviceConnected,
	type: "SS550V3",
	columns: COLUMNS,
	rows: ROWS,
}));

// Plugin bridge: launch an application on key press. Every plugin page gets
// host/preload.js, so any installed plugin can ask the host to spawn a program.
// Personal-use host: the user installs their own plugins, so the trust boundary
// is the plugins directory itself. No args -> shell.openPath (handles .lnk,
// folders, and protocol handlers); args -> a detached spawn that survives
// host quit (stdio ignored, unref'd).
ipcMain.handle("reard-host:launchApp", (_event, payload) => {
	const p = payload || {};
	if (typeof p.path !== "string" || !p.path.trim()) return { ok: false, error: "missing path" };
	const appPath = p.path.trim();
	const args = typeof p.args === "string" ? p.args.trim() : "";
	try {
		if (args) {
			if (!fs.existsSync(appPath)) return { ok: false, error: `file not found: ${appPath}` };
			const child = spawn(appPath, args.split(/\s+/), {
				detached: true,
				stdio: "ignore",
				cwd: path.dirname(appPath),
				windowsHide: true,
			});
			child.on("error", (e) => console.log(`[host] launchApp spawn error: ${e.message}`));
			child.unref();
			console.log(`[host] launchApp spawned: ${appPath} ${args}`);
			return { ok: true };
		}
		shell.openPath(appPath).then((err) => {
			if (err) console.log(`[host] launchApp openPath failed for ${appPath}: ${err}`);
			else console.log(`[host] launchApp opened: ${appPath}`);
		});
		return { ok: true };
	} catch (e) {
		return { ok: false, error: e.message };
	}
});

// Installed-app list for the config UI's app picker: every .lnk shortcut in
// both Start Menu "Programs" trees (system-wide + per-user). Launching a .lnk
// through shell.openPath works, so the shortcut path itself is the stored
// setting. Deduped by lowercase display name, sorted alphabetically.
const START_MENU_DIRS = [
	process.env.PROGRAMDATA ? path.join(process.env.PROGRAMDATA, "Microsoft", "Windows", "Start Menu", "Programs") : null,
	process.env.APPDATA ? path.join(process.env.APPDATA, "Microsoft", "Windows", "Start Menu", "Programs") : null,
].filter(Boolean);

function listInstalledApps() {
	const seen = new Map(); // lowercase name -> { name, path }
	const walk = (dir, depth) => {
		if (depth > 5) return;
		let entries;
		try {
			entries = fs.readdirSync(dir, { withFileTypes: true });
		} catch (e) {
			return;
		}
		for (const ent of entries) {
			const full = path.join(dir, ent.name);
			if (ent.isDirectory()) {
				walk(full, depth + 1);
			} else if (ent.name.toLowerCase().endsWith(".lnk")) {
				const name = ent.name.slice(0, -4);
				const key = name.toLowerCase();
				if (!seen.has(key)) seen.set(key, { name, path: full });
			}
		}
	};
	for (const dir of START_MENU_DIRS) walk(dir, 0);
	return Array.from(seen.values()).sort((a, b) => a.name.localeCompare(b.name));
}

ipcMain.handle("reard:listApps", () => listInstalledApps());

ipcMain.handle("reard:pickApp", async () => {
	if (!configWindow) return { ok: false, error: "config window not open" };
	const res = await dialog.showOpenDialog(configWindow, {
		title: "Choose application",
		properties: ["openFile"],
		filters: [
			{ name: "Applications", extensions: ["exe", "lnk", "bat", "cmd"] },
			{ name: "All files", extensions: ["*"] },
		],
	});
	if (res.canceled || !res.filePaths.length) return { ok: false };
	return { ok: true, path: res.filePaths[0] };
});

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

// 2D-only workload (canvas + images): hardware acceleration only adds a GPU
// process, GPU disk-cache errors, and battery drain. Disable before ready.
app.disableHardwareAcceleration();

app.whenReady().then(() => {
	appReady = true;
	createTray();
	maybeStart();
});

app.on("before-quit", () => {
	for (const [context, inst] of instances) sendWillDisappear(context, inst);
	for (const child of nativeChildren.values()) {
		try { child.kill(); } catch { /* already gone */ }
	}
});
