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
    // [a-z0-9_]+：命令名含数字（如 a11y_probe），纯 [a-z_]+ 会在数字处截断产生幽灵名。
    const mm = t.match(/^(?:[a-z0-9_]+\s*::\s*)*([a-z0-9_]+)\s*,?\s*$/);
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
  // fn 名用 [a-z0-9_]+：a11y_probe 等含数字名会被 [a-z_]+ 在数字处截断成幽灵命令。
  for (const m of src.matchAll(/#\[tauri::command(?:\([^)]*\))?\]((?:\s*#[^\r\n\]]+\]|\s*\/\/\/[^\r\n]*)*)\s*(?:pub\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)/g)) {
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
// 需求15 性能重构后词典拆分：zh/zh-TW 留在 dictionaries.ts，en 懒加载拆至 dict-en.ts
// （dictionaries.ts 中 en 位为运行期回填的空壳）——解析必须跟着布局走。
const dictSrc = fs.readFileSync("src/i18n/dictionaries.ts", "utf8");
const zhBlock = dictSrc.slice(dictSrc.indexOf("const zh"), dictSrc.indexOf("const zhTwOverrides"));
const zhKeys = new Set([...zhBlock.matchAll(/([A-Za-z0-9_]+)\s*:/gm)].map((m) => m[1]));
const enSrc = fs.readFileSync("src/i18n/dict-en.ts", "utf8");
const enBlock = enSrc.slice(enSrc.indexOf("export const en"));
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

// ---- 6. S2.01 垫片协议生成物一致性门禁（AI-3 三体定版）----
// 单源 tools/shim-protocol.source.json → 三产物（TS/后端/内核）必须与磁盘逐字节一致；
// 手改生成物、或改了单源未重跑生成器 → 门禁红（禁手改生成物戒律的机械化执法）。
(function shimProtocolGate() {
  console.log("\n== SHIM PROTOCOL GATE (S2.01) ==");
  const genPath = path.join(__dirname, "gen-shim-protocol.cjs");
  if (!fs.existsSync(genPath)) {
    console.log("MISSING: tools/gen-shim-protocol.cjs");
    failed = true;
    return;
  }
  // 生成器以 buildArtifacts 导入（require.main 守卫保证导入零副作用）。
  const { buildArtifacts } = require(genPath);
  const src = JSON.parse(fs.readFileSync(genPath.replace(/gen-shim-protocol\.cjs$/, "shim-protocol.source.json"), "utf8"));
  const arts = buildArtifacts(src);
  const targets = [
    ["src/lib/shim/protocol.ts", arts.ts],
    ["src-tauri/src/shim_protocol.rs", arts.rs],
    ["kernel/varix/src/vport/shim_protocol.rs", arts.krs],
  ];
  let drift = false;
  for (const [rel, expected] of targets) {
    const p = path.join(__dirname, "..", rel);
    if (!fs.existsSync(p)) {
      console.log("MISSING GENERATED:", rel);
      drift = true;
      continue;
    }
    const actual = fs.readFileSync(p, "utf8");
    if (actual !== expected) {
      console.log(`DRIFT: ${rel}（与单源不一致——跑 node tools/gen-shim-protocol.cjs）`);
      drift = true;
    }
  }
  if (drift) failed = true;
  else console.log("OK: 三产物与单源逐字节一致（生成物禁手改）");
})();

// ---- 7. S2.10 降级提示 i18n 门禁（AI-4 三体桌面承载）----
// 全部 ❌ 命令统一降级（禁裸错误码直达用户）：perfBaseline.ts 的 DegradeKey
// 全量键必须 zh/en 词典在场（zh-TW 走 zh 基底繁体转换，运行期保证）——
// 缺键即 fail；降级映射无遗漏分支由 perf-baseline.test.ts 单测守护。
(function shimDegradeGate() {
  console.log("\n== SHIM DEGRADE GATE (S2.10) ==");
  const pbPath = path.join(__dirname, "..", "src", "lib", "shim", "perfBaseline.ts");
  if (!fs.existsSync(pbPath)) {
    console.log("MISSING: src/lib/shim/perfBaseline.ts（降级面映射单源）");
    failed = true;
    return;
  }
  const pb = fs.readFileSync(pbPath, "utf8");
  const typeStart = pb.indexOf("export type DegradeKey");
  const typeEnd = pb.indexOf(";", typeStart);
  if (typeStart < 0 || typeEnd < 0) {
    console.log("BROKEN: DegradeKey 类型声明缺失");
    failed = true;
    return;
  }
  const degradeKeys = [...pb.slice(typeStart, typeEnd).matchAll(/"(degrade[A-Za-z]+)"/g)].map((m) => m[1]);
  if (degradeKeys.length === 0) {
    console.log("BROKEN: DegradeKey 类型零键（降级面空转）");
    failed = true;
    return;
  }
  const missZh = degradeKeys.filter((k) => !zhKeys.has(k));
  const missEn = degradeKeys.filter((k) => !enKeys.has(k));
  console.log("degrade keys:", degradeKeys.length, "| zh:", zhKeys.has(degradeKeys[0]) ? "present" : "missing");
  if (missZh.length) {
    console.log("MISSING ZH DEGRADE:", missZh);
    failed = true;
  }
  if (missEn.length) {
    console.log("MISSING EN DEGRADE:", missEn);
    failed = true;
  }
  if (!missZh.length && !missEn.length) {
    console.log("OK: 降级词条 zh/en 全量在场（缺键即 fail 门禁已执法）");
  }
})();

console.log(failed ? "\nAUDIT FAILED" : "\nAUDIT PASSED");
process.exit(failed ? 1 : 0);
