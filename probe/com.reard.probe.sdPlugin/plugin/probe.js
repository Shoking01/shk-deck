// REA Protocol Probe v0.2
// Logs every host<->plugin WebSocket message to a local mirror listener at
// ws://127.0.0.1:4455 (probe/listener). Buffers messages emitted before the
// mirror socket is open (the host injects connectElgatoStreamDeckSocket very
// early), and fails silently when the listener is not running, so Stream
// Station is never disturbed.

let hostSocket = null;
let mirrorSocket = null;
const mirrorQueue = [];

function mirror(dir, msg) {
	try {
		const entry = JSON.stringify({ dir: dir, at: new Date().toISOString(), msg: msg });
		if (mirrorSocket && mirrorSocket.readyState === 1) {
			mirrorSocket.send(entry);
		} else if (mirrorQueue.length < 500) {
			mirrorQueue.push(entry);
		}
	} catch (e) { /* listener down: ignore */ }
}

function connectMirror() {
	try {
		mirrorSocket = new WebSocket("ws://127.0.0.1:4455");
		mirrorSocket.onopen = function () {
			mirror("mirror-open", {});
			while (mirrorQueue.length) {
				mirrorSocket.send(mirrorQueue.shift());
			}
		};
		mirrorSocket.onclose = function () { mirrorSocket = null; };
		mirrorSocket.onerror = function () { /* ignore */ };
	} catch (e) { /* ignore */ }
}

// The host injects a call to this global with the connection parameters
// (same convention the shipped plugins rely on).
function connectElgatoStreamDeckSocket(port, uuid, event, info) {
	mirror("connect-args", { port: port, uuid: uuid, event: event, info: info });
	hostSocket = new WebSocket("ws://127.0.0.1:" + port);
	hostSocket.onopen = function () {
		const registration = { uuid: uuid, event: event };
		hostSocket.send(JSON.stringify(registration));
		mirror("to-host", registration);
	};
	hostSocket.onmessage = function (e) {
		let parsed = e.data;
		try { parsed = JSON.parse(e.data); } catch (err) { /* keep raw */ }
		mirror("from-host", parsed);
	};
	hostSocket.onclose = function (e) { mirror("host-closed", { code: e.code, reason: e.reason }); };
	hostSocket.onerror = function () { mirror("host-error", {}); };
}

connectMirror();

// Fallback: if the host passes parameters as query strings instead of calling
// connectElgatoStreamDeckSocket, honor them too (skipped when the host already
// connected us).
setTimeout(function () {
	if (hostSocket) return;
	try {
		const qs = new URLSearchParams(window.location.search);
		const port = qs.get("port");
		if (port) {
			connectElgatoStreamDeckSocket(
				port,
				qs.get("pluginUUID") || qs.get("uuid") || "",
				qs.get("registerEvent") || "registerPlugin",
				qs.get("info") || "{}"
			);
		} else {
			mirror("no-connection-params", { href: window.location.href });
		}
	} catch (e) { /* ignore */ }
}, 1500);
