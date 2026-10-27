// REA Protocol Probe listener
// Receives mirrored traffic from the probe plugin and appends each line to
// probe-capture.jsonl next to this file.
const fs = require("fs");
const path = require("path");
const { WebSocketServer } = require("ws");

const outFile = path.join(__dirname, "probe-capture.jsonl");
const stream = fs.createWriteStream(outFile, { flags: "a" });

const wss = new WebSocketServer({ port: 4455, host: "127.0.0.1" });
let connections = 0;
let messages = 0;

wss.on("connection", (ws, req) => {
	connections += 1;
	const entry = { dir: "listener-connection", at: new Date().toISOString(), url: req.url };
	stream.write(JSON.stringify(entry) + "\n");
	console.log(`[probe] plugin connected (#${connections})`);
	ws.on("message", (data) => {
		messages += 1;
		stream.write(data.toString() + "\n");
		if (messages % 25 === 0) console.log(`[probe] ${messages} messages captured`);
	});
});

console.log("[probe] listener on ws://127.0.0.1:4455 -> " + outFile);
