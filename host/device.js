// REA RD MVP host - device client (T7)
// Connects to the official Mirabox StreamDock WebSocket SDK server, waits for
// deviceDidConnect, paints a test image on key 1, and logs key events.
//
// Start the SDK server first (see host/README.md):
//   StreamDockWebsocketSDK.exe -oem 293V3:0x0200:0x3000
const WebSocket = require("ws");
const fs = require("fs");
const path = require("path");

const SDK_URL = process.env.SDK_URL || "ws://127.0.0.1:9002";
let devicePath = null;
let keyEvents = 0;

const ws = new WebSocket(SDK_URL);

function send(obj) {
	ws.send(JSON.stringify(obj));
}

ws.on("open", () => {
	console.log(`[device] connected to SDK server at ${SDK_URL}`);
});

ws.on("message", (data) => {
	let msg;
	try { msg = JSON.parse(data.toString()); } catch (e) { return; }
	switch (msg.event) {
		case "deviceDidConnect": {
			devicePath = msg.path;
			const p = msg.payload || {};
			console.log(`[device] deviceDidConnect type=${p.type} vid=0x${(p.vendorID || 0).toString(16)} pid=0x${(p.productID || 0).toString(16)} product="${p.product}" serial=${p.serialNumber}`);
			try {
				const img = fs.readFileSync(path.join(__dirname, "test-image.png"));
				const imgData = "data:image/png;base64," + img.toString("base64");
				send({ event: "setKeyImgData", path: devicePath, payload: { keyIndex: 1, imgData: imgData } });
				send({ event: "refresh", path: devicePath, payload: {} });
				console.log("[device] test image sent to key 1");
			} catch (e) {
				console.log(`[device] could not load test-image.png: ${e.message}`);
			}
			send({ event: "read", path: devicePath, payload: {} });
			console.log("[device] listening for key events (read started)");
			break;
		}
		case "read": {
			keyEvents += 1;
			console.log(`[device] key #${keyEvents}: ${JSON.stringify(msg.payload)}`);
			break;
		}
		case "deviceDidDisconnect":
			console.log("[device] deviceDidDisconnect");
			devicePath = null;
			break;
		default:
			console.log(`[device] ${msg.event} ${JSON.stringify(msg.payload || {}).slice(0, 120)}`);
	}
});

ws.on("error", (e) => console.log(`[device] ws error: ${e.message}`));
ws.on("close", () => console.log("[device] connection closed"));
