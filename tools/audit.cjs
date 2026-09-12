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
  // #[tauri::command]（可带参数如 (async)）与 fn 之间允许夹带其它属性行（#[cfg(windows)] 等）与 /// 文档注释
  for (const m of src.matchAll(/#\[tauri::command(?:\([^)]*\))?\]((?:\s*#[^\r\n\]]+\]|\s*\/\/\/[^\r\n]*)*)\s*(?:pub\s+)?(?:async\s+)?fn\s+([a-z_]+)/g)) {
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
// 双向 diff 的反向冗余（M4 顺手项）：后端已定义但前端零 invoke 的命令。
// 多数是历史遗留/仅 CLI 使用，报出来人工确认后删除，防止 surface 腐化。
if (unusedBackend.length) console.log("UNUSED BACKEND (no frontend invoke):", unusedBackend);
else console.log("OK: no unused backend commands");

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
// 本地词典车道（lockT/sceneT/wpT 等，见各 labels.ts 头注）：t 绑定到本地工厂，
// 其键不走 dictionaries.ts —— 全局审计跳过这些文件，否则必然整片误报。
const localDictRe = /const\s+t\s*=\s*useMemo\(\s*\(\)\s*=>\s*\w+T\(\s*\)\s*,\s*\[\s*\]\s*\)/;
const used = new Map(); // key -> [files]
for (const f of srcFiles) {
  const s = fs.readFileSync(f, "utf8");
  if (localDictRe.test(s)) continue;
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

// ---- 3.5 A-1 裸色值检测（信息输出；迁移完成后可升级为门禁）----
// 扫描 src/styles 与 src/design 之外的所有 CSS：裸 hex/rgb() 不计 token 迁移率。
(function bareColorAudit() {
  const fs = require("fs");
  const path = require("path");
  let bare = 0;
  const walk = (dir) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) { if (!p.includes("design")) walk(p); continue; }
      if (!/\.(css|tsx?)$/.test(e.name)) continue;
      const text = fs.readFileSync(p, "utf8");
      const hex = text.match(/#[0-9a-fA-F]{3,8}\b/g);
      bare += hex ? hex.filter((h) => !/^#(fff|000)$/i.test(h)).length : 0;
    }
  };
  if (fs.existsSync(path.join(__dirname, "..", "src", "styles"))) walk(path.join(__dirname, "..", "src", "styles"));
  console.log("\nA-1 bare color values (info, migration in progress):", bare);
})();

// ---- 5. M-60 测试钩子：data-testid 静态检查 ----
// 同一文件内重复 testid = 门禁失败（同文档树重复 = 选择器歧义）；
// 跨文件重名仅提示（互斥渲染的组件允许，人工确认）。规范见 docs/TEST-HOOKS.md。
let failed = false;
(function testidAudit() {
  const testids = new Map(); // file -> Set<string>
  for (const f of srcFiles) {
    const s = fs.readFileSync(f, "utf8");
    const ids = [...s.matchAll(/data-testid="([^"]+)"/g)].map((m) => m[1]);
    if (ids.length) testids.set(f, new Set(ids));
  }
  const globalCount = new Map();
  for (const ids of testids.values()) for (const id of ids) globalCount.set(id, (globalCount.get(id) || 0) + 1);
  let dupInFile = false;
  for (const [f, ids] of testids) {
    const s = fs.readFileSync(f, "utf8");
    const seen = new Map();
    for (const m of s.matchAll(/data-testid="([^"]+)"/g)) {
      seen.set(m[1], (seen.get(m[1]) || 0) + 1);
    }
    const dups = [...seen.entries()].filter(([, n]) => n > 1).map(([id, n]) => `${id} ×${n}`);
    if (dups.length) {
      dupInFile = true;
      console.log(`DUP TESTID IN FILE ${f}: ${dups.join(", ")}`);
    }
  }
  const crossDup = [...globalCount.entries()].filter(([, n]) => n > 1).map(([id, n]) => `${id} (${n} files)`);
  console.log("\n== TESTID AUDIT (M-60) ==");
  console.log("files with testids:", testids.size, "| unique ids:", globalCount.size);
  if (dupInFile) {
    console.log("DUPLICATE TESTID IN SAME FILE — 命名规范见 docs/TEST-HOOKS.md");
    failed = true;
  } else console.log("OK: no in-file duplicate testids");
  if (crossDup.length) console.log("CROSS-FILE (info, 互斥渲染人工确认):", crossDup);
  else console.log("OK: no cross-file testid names");
})();

// ---- 4. CI 门禁（F-7.5）：i18n 键完整性不达标时以非零码退出 ----
// 仅对「t() 用到但词典缺失」与 IPC 缺后端判定失败（真实缺陷）；
// zh/en 全量 parity 与 UNUSED 报告保留为信息输出（历史遗留键人工确认）。
// （failed 变量声明已上移至 M-60 testid 段，各段共用同一个门禁标志。）
if (missingBackend.length) failed = true;
if (missingZh.length || missingEn.length) failed = true;

// ---- 5. AI-17 视觉门禁（U-60 品质关卡）----
// ① 令牌契约：tokens.css 必须含 --ctl-* / --focus-ring / --overlay-* / --motion-speed / --texture-grain；
// ② AI-17 视觉样式文件裸色值 = 0（新文件不允许裸 hex，历史存量迁移另行处理）；
// ③ 图标语义字典存在（U-10 唯一映射由单测守护，此处守护文件在场）。
(function visualGate() {
  console.log("\n== VISUAL GATE (AI-17 U-60) ==");
  const tokensPath = path.join(__dirname, "..", "src", "design", "tokens.css");
  const tokens = fs.readFileSync(tokensPath, "utf8");
  const requiredTokens = ["--ctl-btn:", "--ctl-input:", "--ctl-touch:", "--focus-ring:", "--overlay-1:", "--overlay-2:", "--overlay-3:", "--motion-speed:", "--texture-grain:"];
  const missingTokens = requiredTokens.filter((t) => !tokens.includes(t));
  if (missingTokens.length) {
    console.log("MISSING TOKENS:", missingTokens);
    failed = true;
  } else {
    console.log("OK: token contract complete (--ctl/--focus/--overlay/--motion-speed/--texture-grain)");
  }

  const visionStyles = ["material.css", "interactions.css", "cjk.css", "cursor.css", "animated-icons.css"];
  let visionBare = 0;
  for (const name of visionStyles) {
    const p = path.join(__dirname, "..", "src", "styles", name);
    if (!fs.existsSync(p)) {
      console.log(`MISSING STYLE FILE: ${name}`);
      failed = true;
      continue;
    }
    const text = fs.readFileSync(p, "utf8");
    const hex = text.match(/#[0-9a-fA-F]{3,8}\b/g);
    visionBare += hex ? hex.filter((h) => !/^#(fff|000)$/i.test(h)).length : 0;
  }
  if (visionBare > 0) {
    console.log("AI-17 VISION BARE COLOR VALUES:", visionBare, "(must be 0)");
    failed = true;
  } else {
    console.log("OK: vision style files bare color values = 0");
  }

  const registryPath = path.join(__dirname, "..", "src", "lib", "iconRegistry.ts");
  if (!fs.existsSync(registryPath)) {
    console.log("MISSING: src/lib/iconRegistry.ts (U-10 icon semantic registry)");
    failed = true;
  } else {
    console.log("OK: iconRegistry present");
  }
})();

console.log(failed ? "\nAUDIT FAILED" : "\nAUDIT PASSED");
process.exit(failed ? 1 : 0);
