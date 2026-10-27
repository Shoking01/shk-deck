// REA RD config UI renderer - plain DOM, no frameworks, no CDN.
// Deck replica grid with live key previews + a persistent details panel.
const COLUMNS = 5;
const ROWS = 3;
const PREVIEW_POLL_MS = 2000;

let plugins = [];
let profile = { instances: [] };
let selected = null; // { column, row } | null
let keyImages = {}; // keyIndex -> base64 PNG (as sent by plugins)
let keyTitles = {}; // keyIndex -> current title text
let editorMode = "json"; // "json" = raw settings textarea, "app" = app picker
let appsLoaded = false; // Start Menu app list fetched once, cached in the datalist

const grid = document.getElementById("grid");
const panelTitle = document.getElementById("panel-title");
const panelSubtitle = document.getElementById("panel-subtitle");
const panelEmpty = document.getElementById("panel-empty");
const assignFields = document.getElementById("assign-fields");
const panelButtons = document.getElementById("panel-buttons");
const panelError = document.getElementById("panel-error");
const pluginSelect = document.getElementById("plugin-select");
const actionSelect = document.getElementById("action-select");
const settingsInput = document.getElementById("settings-input");
const settingsField = document.getElementById("settings-field");
const appFields = document.getElementById("app-fields");
const appPathInput = document.getElementById("app-path-input");
const appArgsInput = document.getElementById("app-args-input");
const appBrowseBtn = document.getElementById("app-browse-btn");
const appsDatalist = document.getElementById("apps-datalist");
const titleColorInput = document.getElementById("titlecolor-input");
const saveBtn = document.getElementById("save-btn");
const removeBtn = document.getElementById("remove-btn");
const cancelBtn = document.getElementById("cancel-btn");
const deviceBadge = document.getElementById("device-badge");
const deviceText = document.getElementById("device-text");

function instanceAt(column, row) {
	return profile.instances.find((i) => i.column === column && i.row === row) || null;
}

function findPlugin(uuid) {
	return plugins.find((p) => p.uuid === uuid) || null;
}

function actionLabel(actionUuid) {
	for (const p of plugins) {
		const found = p.actions.find((a) => a.uuid === actionUuid);
		if (found) return found.name;
	}
	return actionUuid; // fall back to the raw uuid when not in the scanned list
}

// Plugin images arrive either as raw base64 or as full data URLs.
function asDataUrl(img) {
	if (!img) return null;
	return img.startsWith("data:") ? img : `data:image/png;base64,${img}`;
}

function renderGrid() {
	grid.textContent = "";
	for (let row = 0; row < ROWS; row++) {
		for (let column = 0; column < COLUMNS; column++) {
			const idx = row * COLUMNS + column + 1; // 1-based row-major, device order
			const inst = instanceAt(column, row);
			const isSelected = selected && selected.column === column && selected.row === row;
			const cell = document.createElement("div");
			cell.className = "cell" + (inst ? " assigned" : " empty") + (isSelected ? " selected" : "");
			cell.setAttribute("role", "option");
			cell.setAttribute("aria-selected", String(!!isSelected));
			cell.tabIndex = 0;
			const preview = asDataUrl(keyImages[idx]);
			if (preview) cell.style.backgroundImage = `url("${preview}")`;
			const coords = document.createElement("span");
			coords.className = "coords";
			coords.textContent = `${column + 1},${row + 1}`;
			cell.appendChild(coords);
			if (keyTitles[idx]) {
				const t = document.createElement("span");
				t.className = "key-title";
				t.textContent = keyTitles[idx];
				cell.appendChild(t);
			} else if (inst) {
				const label = document.createElement("span");
				label.className = "key-label";
				label.textContent = actionLabel(inst.action);
				cell.appendChild(label);
			}
			cell.title = inst
				? `${actionLabel(inst.action)} - ${findPlugin(inst.plugin)?.name || inst.plugin}`
				: "Empty key";
			const open = () => openPanel(column, row);
			cell.addEventListener("click", open);
			cell.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") open(); });
			grid.appendChild(cell);
		}
	}
}

function showPanelError(message) {
	panelError.textContent = message || "";
}

function validateSettingsInput() {
	const value = settingsInput.value.trim() || "{}";
	try {
		const parsed = JSON.parse(value);
		if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
			settingsInput.classList.add("invalid");
			return null;
		}
		settingsInput.classList.remove("invalid");
		return parsed;
	} catch {
		settingsInput.classList.add("invalid");
		return null;
	}
}

// Which settings editor the selected action wants. Actions may declare
// "REARDSettingsEditor": "app" in their manifest entry; everything else keeps
// the raw JSON textarea.
function selectedActionMeta() {
	const actionUuid = actionSelect.value;
	for (const p of plugins) {
		const found = p.actions.find((a) => a.uuid === actionUuid);
		if (found) return found;
	}
	return null;
}

function updateEditorMode(inst) {
	const meta = selectedActionMeta();
	const mode = meta && meta.settingsEditor === "app" ? "app" : "json";
	editorMode = mode;
	settingsField.classList.toggle("hidden", mode === "app");
	appFields.classList.toggle("hidden", mode !== "app");
	if (mode === "app") {
		const s = (inst && inst.settings) || {};
		appPathInput.value = typeof s.path === "string" ? s.path : "";
		appArgsInput.value = typeof s.args === "string" ? s.args : "";
		ensureAppsLoaded();
	} else {
		settingsInput.value = JSON.stringify((inst && inst.settings) || {}, null, 2);
		validateSettingsInput();
	}
}

// Fetch the installed-app list once and feed the datalist for autocomplete.
// The list shows app NAMES only - full paths made it impossible to tell which
// entry is which. The chosen name resolves back to its path on save.
const appNameToPath = new Map(); // lowercase name -> path
function ensureAppsLoaded() {
	if (appsLoaded) return;
	appsLoaded = true;
	window.reard.listApps().then((apps) => {
		appsDatalist.textContent = "";
		appNameToPath.clear();
		for (const app of apps || []) {
			appNameToPath.set(app.name.toLowerCase(), app.path);
			const opt = document.createElement("option");
			opt.value = app.name;
			appsDatalist.appendChild(opt);
		}
	}).catch(() => { appsLoaded = false; });
}

// Resolve the app picker input: a bare app name (typed or picked from the
// list) maps to its installed path; anything that already looks like a path
// stays as-is. Returns "" for empty input, null for an unresolvable name.
function resolveAppInput(text) {
	const value = text.trim();
	if (!value) return "";
	if (/[\\/:]/.test(value)) return value; // already a path
	const key = value.toLowerCase();
	if (appNameToPath.has(key)) return appNameToPath.get(key);
	const matches = [];
	for (const [name, p] of appNameToPath) {
		if (name.startsWith(key)) matches.push(p);
	}
	return matches.length === 1 ? matches[0] : null;
}

function openPanel(column, row) {
	selected = { column, row };
	const inst = instanceAt(column, row);
	panelEmpty.classList.add("hidden");
	assignFields.classList.remove("hidden");
	panelButtons.classList.remove("hidden");
	showPanelError("");
	if (inst) {
		const plugin = findPlugin(inst.plugin);
		panelTitle.textContent = `Key ${column + 1},${row + 1}`;
		panelSubtitle.textContent = `${actionLabel(inst.action)} - ${plugin ? plugin.name : inst.plugin}`;
		saveBtn.textContent = "Save changes";
		removeBtn.style.display = "inline-block";
	} else {
		panelTitle.textContent = `Key ${column + 1},${row + 1}`;
		panelSubtitle.textContent = "Empty - assign an action";
		saveBtn.textContent = "Assign";
		removeBtn.style.display = "none";
	}
	titleColorInput.value = (inst && inst.titleColor) || "#ffffff";
	if (inst && inst.plugin && pluginSelect.value !== inst.plugin) pluginSelect.value = inst.plugin;
	fillActionSelect();
	if (inst) actionSelect.value = inst.action;
	updateEditorMode(inst);
	renderGrid();
}

function closePanel() {
	selected = null;
	panelEmpty.classList.remove("hidden");
	assignFields.classList.add("hidden");
	panelButtons.classList.add("hidden");
	showPanelError("");
	renderGrid();
}

// Plugin select: one option per installed plugin, labeled by display name.
function fillPluginSelect() {
	pluginSelect.textContent = "";
	for (const p of plugins) {
		if (!p.actions.length) continue; // nothing assignable in this bundle
		const opt = document.createElement("option");
		opt.value = p.uuid;
		opt.textContent = p.name + (p.native ? "  [native]" : "");
		pluginSelect.appendChild(opt);
	}
	fillActionSelect();
}

// Action select for the chosen plugin; raw uuid only in the tooltip.
function fillActionSelect() {
	actionSelect.textContent = "";
	const plugin = plugins.find((p) => p.uuid === pluginSelect.value);
	if (!plugin) return;
	const group = document.createElement("optgroup");
	group.label = plugin.name;
	for (const a of plugin.actions) {
		const opt = document.createElement("option");
		opt.value = a.uuid;
		opt.textContent = a.name;
		opt.title = a.uuid;
		group.appendChild(opt);
	}
	actionSelect.appendChild(group);
}

saveBtn.addEventListener("click", async () => {
	if (!selected) return;
	const pluginUuid = pluginSelect.value;
	const actionUuid = actionSelect.value;
	if (!pluginUuid || !actionUuid) {
		showPanelError("Pick a plugin and an action.");
		return;
	}
	let settings;
	if (editorMode === "app") {
		const rawApp = appPathInput.value.trim();
		const appPath = resolveAppInput(rawApp);
		if (!appPath) {
			showPanelError(rawApp ? "Unknown application - pick one from the list or use Browse." : "Pick an application (search or Browse).");
			appPathInput.focus();
			return;
		}
		appPathInput.value = appPath; // show the resolved path after saving
		settings = { path: appPath };
		const args = appArgsInput.value.trim();
		if (args) settings.args = args;
	} else {
		settings = validateSettingsInput();
		if (settings === null) {
			showPanelError("Settings is not a valid JSON object.");
			return;
		}
	}
	const titleColor = titleColorInput.value.trim() || "#ffffff";
	const res = await window.reard.assign({
		column: selected.column,
		row: selected.row,
		plugin: pluginUuid,
		action: actionUuid,
		settings,
		titleColor,
	});
	if (!res.ok) {
		showPanelError(res.error || "Assign failed.");
		return;
	}
	showPanelError("");
});

appBrowseBtn.addEventListener("click", async () => {
	const res = await window.reard.pickApp();
	if (res && res.ok && res.path) appPathInput.value = res.path;
});

removeBtn.addEventListener("click", async () => {
	if (!selected) return;
	const res = await window.reard.clear({ column: selected.column, row: selected.row });
	if (!res.ok) {
		showPanelError(res.error || "Remove failed.");
		return;
	}
	closePanel();
});

cancelBtn.addEventListener("click", closePanel);
pluginSelect.addEventListener("change", () => {
	fillActionSelect();
	updateEditorMode(null);
});
actionSelect.addEventListener("change", () => {
	const inst = selected ? instanceAt(selected.column, selected.row) : null;
	updateEditorMode(inst && inst.action === actionSelect.value ? inst : null);
});
settingsInput.addEventListener("input", validateSettingsInput);

function refreshStatus() {
	window.reard.getStatus().then((s) => {
		deviceBadge.classList.toggle("connected", !!s.connected);
		deviceText.textContent = s.connected ? `${s.type} - connected` : `${s.type} - disconnected`;
	}).catch(() => {});
}

function refreshPreviews() {
	window.reard.getKeyImages().then((data) => {
		keyImages = data.images || {};
		keyTitles = data.titles || {};
		renderGrid();
	}).catch(() => {});
}

// Register the push listener before the initial loads so no change is missed.
window.reard.onProfileChanged((data) => {
	profile = data;
	if (selected && !instanceAt(selected.column, selected.row)) {
		// The selected key was cleared elsewhere; keep it selected as empty.
	}
	renderGrid();
	if (selected) openPanel(selected.column, selected.row);
	refreshPreviews();
});

(async function init() {
	plugins = await window.reard.listPlugins();
	profile = await window.reard.getProfile();
	fillPluginSelect();
	refreshStatus();
	refreshPreviews();
	renderGrid();
	setInterval(() => { refreshPreviews(); refreshStatus(); }, PREVIEW_POLL_MS);
})();
