// REA RD host - device client module (T7/T8)
// Connects to the official Mirabox StreamDock WebSocket SDK server and exposes
// device events + commands as an EventEmitter.
const WebSocket = require("ws");
const { EventEmitter } = require("events");

class DeviceClient extends EventEmitter {
	constructor(url = process.env.SDK_URL || "ws://127.0.0.1:9002") {
		super();
		this.url = url;
		this.devicePath = null;
		this.deviceInfo = null;
		this.ws = new WebSocket(url);
		this.ws.on("open", () => this.emit("server-connect"));
		this.ws.on("error", (e) => this.emit("error", e));
		this.ws.on("close", () => this.emit("server-close"));
		this.ws.on("message", (data) => {
			let msg;
			try { msg = JSON.parse(data.toString()); } catch { return; }
			this._onMessage(msg);
		});
	}

	_onMessage(msg) {
		switch (msg.event) {
			case "deviceDidConnect":
				this.devicePath = msg.path;
				this.deviceInfo = msg.payload || null;
				this.emit("device-connect", msg.payload);
				break;
			case "deviceDidDisconnect":
				this.devicePath = null;
				this.deviceInfo = null;
				this.emit("device-disconnect");
				break;
			case "read":
				this.emit("key", msg.payload || {});
				break;
			default:
				this.emit("event", msg);
		}
	}

	send(obj) {
		if (this.ws.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(obj));
	}

	setKeyImage(keyIndex, imgData) {
		this.send({ event: "setKeyImgData", path: this.devicePath, payload: { keyIndex, imgData } });
		this.send({ event: "refresh", path: this.devicePath, payload: {} });
	}

	startRead() {
		this.send({ event: "read", path: this.devicePath, payload: {} });
	}
}

module.exports = { DeviceClient };
