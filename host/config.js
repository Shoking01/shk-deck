// REA RD config UI renderer (T12/S10) - plain DOM, no frameworks, no CDN.
const COLUMNS = 5;
const ROWS = 3;

let plugins = [];
let profile = { instances: [] };
let selected = null; // { column, row } | null

const grid = document.getElementById("grid");
const panel = document.getElementById("panel");
const panelTitle = document.getElementById("panel-title");
const assignFields = document.getElementById("assign-fields");
const panelError = document.getElementById("panel-error");
const pluginSelect = document.getElementById("plugin-select");
const actionSelect = document.getElementById("action-select");
const settingsInput = document.getElementById("settings-input");
const titleColorInput = document.getElementById("titlecolor-input");
const assignBtn = document.getElementById("assign-btn");
const removeBtn = document.getElementById("remove-btn");
const cancelBtn = document.getElementById("cancel-btn");

function instanceAt(column, row) {
	return profile.instances.find((i) => i.column === column && i.row === row) || null;
}

function actionLabel(actionUuid) {
	for (const p of plugins) {
		const found = p.actions.find((a) => a.uuid === actionUuid);
		if (found) return found.name;
	}
	return actionUuid; // fall back to the raw uuid when not in the scanned list
}

function renderGrid() {
	grid.textContent = "";
	for (let row = 0; row < ROWS; row++) {
		for (let column = 0; column < COLUMNS; column++) {
			const inst = instanceAt(column, row);
			const cell = document.createElement("div");
			cell.className = "cell" + (inst ? " assigned" : "") +
				(selected && selected.column === column && selected.row === row ? " selected" : "");
			const coords = document.createElement("span");
			coords.className = "coords";
			coords.textContent = `(${column},${row})`;
			const name = document.createElement("span");
			name.className = "action";
			name.textContent = inst ? actionLabel(inst.action) : "empty";
			cell.append(coords, name);
			cell.addEventListener("click", () => openPanel(column, row));
			grid.appendChild(cell);
		}
	}
}

// Plugin select: one option per installed plugin, labeled by display name.
function fillPluginSelect() {
	pluginSelect.textContent = "";
	for (const p of plugins) {
		if (!p.actions.length) continue; // nothing assignable in this bundle
		const opt = document.createElement("option");
		opt.value = p.uuid;
		opt.textContent = p.name;
		pluginSelect.appendChild(opt);
	}
	fillActionSelect();
}

// Action select for the chosen plugin, grouped under its display name.
function fillActionSelect() {
	actionSelect.textContent = "";
	const plugin = plugins.find((p) => p.uuid === pluginSelect.value);
	if (!plugin) return;
	const group = document.createElement("optgroup");
	group.label = plugin.name;
	for (const a of plugin.actions) {
		const opt = document.createElement("option");
		opt.value = a.uuid;
		opt.textContent = `${a.name} (${a.uuid})`;
		group.appendChild(opt);
	}
	actionSelect.appendChild(group);
}

function showPanelError(message) {
	panelError.textContent = message || "";
}

function openPanel(column, row) {
	selected = { column, row };
	const inst = instanceAt(column, row);
	panelTitle.textContent = `Key (${column},${row})` + (inst ? ` - ${actionLabel(inst.action)}` : " - empty");
	assignFields.style.display = inst ? "none" : "block";
	assignBtn.style.display = inst ? "none" : "inline-block";
	removeBtn.style.display = inst ? "inline-block" : "none";
	showPanelError("");
	settingsInput.value = JSON.stringify((inst && inst.settings) || {});
	titleColorInput.value = (inst && inst.titleColor) || "#ffffff";
	panel.classList.remove("hidden");
	renderGrid();
}

function hidePanel() {
	selected = null;
	panel.classList.add("hidden");
	renderGrid();
}

assignBtn.addEventListener("click", async () => {
	if (!selected) return;
	const pluginUuid = pluginSelect.value;
	const actionUuid = actionSelect.value;
	if (!pluginUuid || !actionUuid) {
		showPanelError("Pick a plugin and an action.");
		return;
	}
	let settings;
	try {
		settings = JSON.parse(settingsInput.value || "{}");
	} catch (e) {
		showPanelError(`Settings is not valid JSON: ${e.message}`);
		return;
	}
	if (settings === null || typeof settings !== "object" || Array.isArray(settings)) {
		showPanelError("Settings must be a JSON object.");
		return;
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
	hidePanel();
});

removeBtn.addEventListener("click", async () => {
	if (!selected) return;
	const res = await window.reard.clear({ column: selected.column, row: selected.row });
	if (!res.ok) {
		showPanelError(res.error || "Remove failed.");
		return;
	}
	hidePanel();
});

cancelBtn.addEventListener("click", hidePanel);

pluginSelect.addEventListener("change", fillActionSelect);

// Register the push listener before the initial loads so no change is missed.
window.reard.onProfileChanged((data) => {
	profile = data;
	hidePanel();
});

(async function init() {
	plugins = await window.reard.listPlugins();
	profile = await window.reard.getProfile();
	fillPluginSelect();
	renderGrid();
})();
