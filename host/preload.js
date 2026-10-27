// Plugin page preload: diagnostics + the host bridge.
//
// Every plugin BrowserWindow loads this file with contextIsolation disabled, so
// `window.reardHost` lands in the plugin page's own scope. The bridge is the
// ONLY privileged surface a plugin page has: launchApp asks the main process to
// start a program. Everything else a plugin can do goes through the WebSocket
// protocol frames.
const { ipcRenderer } = require("electron");

window.addEventListener("error", (e) => {
	console.log(`[PAGE-ERROR] ${e.message} @ ${e.filename}:${e.lineno}:${e.colno}`);
});
window.addEventListener("unhandledrejection", (e) => {
	const r = e.reason;
	console.log(`[PAGE-REJECTION] ${r && (r.stack || r.message) ? (r.stack || r.message) : String(r)}`);
});

window.reardHost = {
	launchApp: (appPath, args) => ipcRenderer.invoke("reard-host:launchApp", { path: appPath, args: args || "" }),
	// Windows shell icon for an app path (.exe or .lnk) as a PNG data URL, for
	// plugins that draw the target app's icon on their key face.
	getAppIcon: (appPath) => ipcRenderer.invoke("reard-host:getAppIcon", { path: appPath }),
};
