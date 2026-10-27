// REA RD MVP host - standalone device loop test (T7)
// Connects to the SDK server, paints a test image on key 1, logs key events.
// Start the SDK server first (see host/README.md):
//   StreamDockWebsocketSDK.exe -oem 293V3:0x0200:0x3000
const fs = require("fs");
const path = require("path");
const { DeviceClient } = require("./device-client");

const device = new DeviceClient();
let keyEvents = 0;

device.on("server-connect", () => console.log(`[device] connected to SDK server at ${device.url}`));

device.on("device-connect", (payload) => {
	const p = payload || {};
	console.log(`[device] deviceDidConnect raw: ${JSON.stringify(p)}`);
	try {
		// Optional CLI arg: send a specific image instead of the default test image.
		const imgPath = process.argv[2] || path.join(__dirname, "test-image.png");
		const img = fs.readFileSync(imgPath);
		device.setKeyImage(1, "data:image/png;base64," + img.toString("base64"));
		console.log(`[device] image sent to key 1: ${imgPath} (${img.length} bytes)`);
	} catch (e) {
		console.log(`[device] could not load image: ${e.message}`);
	}
	device.startRead();
	console.log("[device] listening for key events (read started)");
});

device.on("key", (payload) => {
	keyEvents += 1;
	console.log(`[device] key #${keyEvents}: ${JSON.stringify(payload)}`);
});

device.on("device-disconnect", () => console.log("[device] deviceDidDisconnect"));
device.on("error", (e) => console.log(`[device] ws error: ${e.message}`));
device.on("server-close", () => console.log("[device] connection closed"));
