// Extract context around terms in a (minified) file. Usage: node grep-context.js <file> <term> [maxHits]
const fs = require("fs");
const [file, term, maxHitsArg] = process.argv.slice(2);
const maxHits = Number(maxHitsArg) || 4;
const s = fs.readFileSync(file, "utf8");
let i = -1, n = 0;
while ((i = s.indexOf(term, i + 1)) >= 0 && n < maxHits) {
	n++;
	console.log(`[${term} #${n}] ...` + s.slice(Math.max(0, i - 220), i + 220).replace(/\s+/g, " ") + "...\n");
}
if (n === 0) console.log(`[${term}] no matches`);
