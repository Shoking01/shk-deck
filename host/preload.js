// Plugin page diagnostic preload: surface silent failures.
window.addEventListener("error", (e) => {
	console.log(`[PAGE-ERROR] ${e.message} @ ${e.filename}:${e.lineno}:${e.colno}`);
});
window.addEventListener("unhandledrejection", (e) => {
	const r = e.reason;
	console.log(`[PAGE-REJECTION] ${r && (r.stack || r.message) ? (r.stack || r.message) : String(r)}`);
});
