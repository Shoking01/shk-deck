// REA RD config UI preload (T12/S10) - exposes a minimal IPC bridge to the
// config renderer. The window keeps Electron's default contextIsolation.
const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("reard", {
	listPlugins: () => ipcRenderer.invoke("reard:listPlugins"),
	getProfile: () => ipcRenderer.invoke("reard:getProfile"),
	assign: (payload) => ipcRenderer.invoke("reard:assign", payload),
	clear: (payload) => ipcRenderer.invoke("reard:clear", payload),
	// Main pushes { instances } after every successful profile change.
	onProfileChanged: (callback) => {
		const listener = (_event, data) => callback(data);
		ipcRenderer.on("profile-changed", listener);
		return () => ipcRenderer.removeListener("profile-changed", listener);
	},
});
