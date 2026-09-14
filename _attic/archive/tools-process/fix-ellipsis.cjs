const fs = require("fs");
const f = "src/library.rs";
let s = fs.readFileSync(f, "utf8");
const lines = s.split(/\r?\n/);
lines[628] = '        s.insert_str(0, "\u2026");';
lines[631] = "        s.push('\u2026');";
fs.writeFileSync(f, lines.join("\n"), "utf8");
console.log(lines.slice(626, 633).join("\n"));
