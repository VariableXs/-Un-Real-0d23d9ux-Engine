const fs = require("fs");
const path = require("path");

// ---- 1. IPC surface cross-check ----
const ipcSrc = fs.readFileSync("src/lib/ipc.ts", "utf8");
const invoked = new Set();
for (const m of ipcSrc.matchAll(/invoke<[^>]*>\(\s*"([a-z_]+)"/g)) invoked.add(m[1]);
for (const m of ipcSrc.matchAll(/invoke<[^>]*>\(\s*'([a-z_]+)'/g)) invoked.add(m[1]);

const libSrc = fs.readFileSync("src-tauri/src/lib.rs", "utf8");
const registered = new Set();
for (const m of libSrc.matchAll(/generate_handler!\[([^\]]*)\]/gs)) {
  for (const line of m[1].split(/\r?\n/)) {
    const t = line.trim();
    if (!t || t.startsWith("//")) continue;
    const mm = t.match(/^(?:[a-z_]+\s*::\s*)*([a-z_]+)\s*,?\s*$/);
    if (mm) registered.add(mm[1]);
  }
}

const cmdAttrs = [];
function walkRs(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const fp = path.join(dir, e.name);
    if (e.isDirectory()) walkRs(fp, out);
    else if (e.name.endsWith(".rs")) out.push(fp);
  }
  return out;
}
const cmdFiles = walkRs("src-tauri/src");
for (const f of cmdFiles) {
  const src = fs.readFileSync(f, "utf8");
  // #[tauri::command] 与 fn 之间允许夹带其它属性行（#[cfg(windows)] 等）
  for (const m of src.matchAll(/#\[tauri::command\]((?:\s*#\[[^\]]+\])*)\s*(?:pub\s+)?(?:async\s+)?fn\s+([a-z_]+)/g)) {
    cmdAttrs.push({ file: path.relative("src-tauri/src", f), name: m[2] });
  }
}
const attrNames = new Set(cmdAttrs.map((c) => c.name));

const missingBackend = [...invoked].filter((n) => !registered.has(n) || !attrNames.has(n));
const notRegisteredInHandler = [...attrNames].filter((n) => !registered.has(n));
const unusedBackend = [...attrNames].filter((n) => !invoked.has(n));

console.log("== IPC AUDIT ==");
console.log("frontend invokes:", invoked.size, "| rust commands:", attrNames.size, "| registered:", registered.size);
if (missingBackend.length) console.log("MISSING BACKEND:", missingBackend);
else console.log("OK: every invoke has a backend command");
if (notRegisteredInHandler.length) console.log("NOT IN generate_handler:", notRegisteredInHandler);
else console.log("OK: every command registered");

// ---- 2. i18n keys audit ----
const dictSrc = fs.readFileSync("src/i18n/dictionaries.ts", "utf8");
const zhBlock = dictSrc.slice(dictSrc.indexOf("const zh"), dictSrc.indexOf("const en"));
const enBlock = dictSrc.slice(dictSrc.indexOf("const en"), dictSrc.indexOf("export const dictionaries"));
const zhKeys = new Set([...zhBlock.matchAll(/([A-Za-z0-9_]+)\s*:/gm)].map((m) => m[1]));
const enKeys = new Set([...enBlock.matchAll(/([A-Za-z0-9_]+)\s*:/gm)].map((m) => m[1]));

function walk(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, out);
    else if (/\.(tsx?|ts)$/.test(e.name)) out.push(p);
  }
  return out;
}
const srcFiles = walk("src").filter((f) => !f.includes("__tests__"));
const used = new Map(); // key -> [files]
for (const f of srcFiles) {
  const s = fs.readFileSync(f, "utf8");
  for (const m of s.matchAll(/\bt\(\s*"([A-Za-z0-9_]+)"/g)) {
    if (!used.has(m[1])) used.set(m[1], []);
    used.get(m[1]).push(f);
  }
}
const missingZh = [...used.keys()].filter((k) => !zhKeys.has(k));
const missingEn = [...used.keys()].filter((k) => !enKeys.has(k));
console.log("\n== I18N AUDIT ==");
console.log("used keys:", used.size, "| zh:", zhKeys.size, "| en:", enKeys.size);
if (missingZh.length) console.log("MISSING ZH:", missingZh); else console.log("OK zh complete");
if (missingEn.length) console.log("MISSING EN:", missingEn); else console.log("OK en complete");

// zh/en parity
const zhOnly = [...zhKeys].filter((k) => !enKeys.has(k));
const enOnly = [...enKeys].filter((k) => !zhKeys.has(k));
if (zhOnly.length) console.log("ZH ONLY:", zhOnly);
if (enOnly.length) console.log("EN ONLY:", enOnly);

// ---- 3. dynamic t(`kind${...}`) style keys sanity (search hits kinds)
const kindKeys = ["kindDocument", "kindFolder", "kindMindmap", "kindNode"];
console.log("\nkind* present:", kindKeys.every((k) => zhKeys.has(k) && enKeys.has(k)));

process.exit(0);
