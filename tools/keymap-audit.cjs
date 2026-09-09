#!/usr/bin/env node
/**
 * Z-08 keymap-audit.cjs — 键位静态审计。
 *
 * 1. 扫描 src/ 下所有裸 keydown 监听（addEventListener("keydown"...)），
 *    与 Z-08 注册表收编基线比对，输出报告（stdout JSON + 人读摘要）；
 * 2. 扫描 settings.shortcutBinds 默认表与系统保留键（Z-09）的命中；
 * 3. 退出码：发现裸 keydown 新增（相对基线）时为 1，供 CI / pre-commit 门禁。
 *
 * AI-20 M-83 键位 CI 门禁扩展（error/warn 两级）：
 *   - error 级（默认表功能冲突：同 accel 绑定多个 action）→ 阻断，exit 1；
 *   - warn 级（默认 accel 命中 Z-09 系统保留键）→ 要求 PR 说明标签
 *     （默认仅告警；--strict 时升级为阻断，供收口验收用）。
 *
 * 用法：node tools/keymap-audit.cjs [--baseline] [--update-baseline] [--skip-binds] [--strict]
 * 基线文件：tools/keymap-audit-baseline.json
 * 数据源：tools/keymap-binds-dump.ts（vite-node 执行，读 SHORTCUT_ACTIONS + SYSTEM_RESERVED）
 */

const fs = require("fs");
const path = require("path");

const ROOT = path.resolve(__dirname, "..");
const SRC = path.join(ROOT, "src");
const BASELINE = path.join(__dirname, "keymap-audit-baseline.json");

const IGNORE_FILES = new Set(
  fs.existsSync(BASELINE)
    ? JSON.parse(fs.readFileSync(BASELINE, "utf8")).ignoreFiles ?? []
    : [],
);

function walk(dir, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(p, out);
    else if (/\.(tsx?|jsx?)$/.test(entry.name)) out.push(p);
  }
  return out;
}

/** 扫描裸 keydown 监听（未走 Z-08 useHotkey/register 的直接 addEventListener）。 */
function scanBareKeydown() {
  const findings = [];
  for (const file of walk(SRC)) {
    const rel = path.relative(ROOT, file).replace(/\\/g, "/");
    if (rel.includes("keymap/hooks.ts") || rel.includes("keymap-arbitration")) continue; // 注册表本体豁免
    const text = fs.readFileSync(file, "utf8");
    const lines = text.split(/\r?\n/);
    lines.forEach((line, i) => {
      const m = line.match(/addEventListener\(\s*["']keydown["']/);
      if (!m) return;
      // 走注册表的代理（KeymapOverlays 的 useEscOverlayStack / KeymapOverlay 自举）视为已收编
      const isEscStack = rel.includes("KeymapOverlays.tsx");
      findings.push({ file: rel, line: i + 1, collected: isEscStack, text: line.trim().slice(0, 120) });
    });
  }
  return findings;
}

function main() {
  const updateBaseline = process.argv.includes("--update-baseline");
  const skipBinds = process.argv.includes("--skip-binds");
  const strict = process.argv.includes("--strict");
  const findings = scanBareKeydown();
  const uncollected = findings.filter((f) => !f.collected && !IGNORE_FILES.has(f.file));

  let baseline = { count: 0, files: {} };
  if (fs.existsSync(BASELINE)) baseline = JSON.parse(fs.readFileSync(BASELINE, "utf8"));
  const perFile = {};
  for (const f of uncollected) perFile[f.file] = (perFile[f.file] ?? 0) + 1;

  if (updateBaseline) {
    fs.writeFileSync(BASELINE, JSON.stringify({ count: uncollected.length, files: perFile, ignoreFiles: Object.keys(IGNORE_FILES) }, null, 2));
    console.log(`baseline updated: ${uncollected.length} bare keydown listeners`);
    return;
  }

  const baseCount = baseline.count ?? 0;
  const newOnes = [];
  for (const [f, n] of Object.entries(perFile)) {
    const bn = baseline.files?.[f] ?? 0;
    if (n > bn) newOnes.push(`${f}: ${bn} -> ${n}`);
  }

  console.log("== keymap-audit (Z-08 + M-83) ==");
  console.log(`bare keydown listeners: ${uncollected.length} (baseline ${baseCount})`);
  for (const f of uncollected) {
    console.log(`  ${f.file}:${f.line} ${f.collected ? "[collected]" : "[BARE]"} ${f.text}`);
  }
  if (newOnes.length > 0) {
    console.error("\nFAIL: new bare keydown listeners not routed through the Z-08 registry:");
    for (const n of newOnes) console.error("  " + n);
    console.error("Rule: any new keybinding MUST register via src/lib/keymap/registry.ts (Z-08 iron rule).");
    process.exit(1);
  }
  console.log("\nOK: no new bare keydown listeners.");

  // ---- AI-20 M-83：默认表功能冲突（error）+ 系统保留键重叠（warn） ----
  if (skipBinds) {
    console.log("binds check skipped (--skip-binds).");
    return;
  }
  const binds = dumpBinds();
  if (!binds) process.exit(1);
  const byAccel = new Map();
  for (const a of binds.actions) {
    byAccel.set(a.accel, [...(byAccel.get(a.accel) ?? []), a.id]);
  }
  const conflicts = [...byAccel.entries()].filter(([, ids]) => ids.length > 1);
  const reservedHits = binds.actions.filter((a) => binds.reserved.includes(a.accel));

  console.log(`\n== M-83 binds gate (${binds.actions.length} actions, ${binds.reserved.length} reserved combos) ==`);
  if (conflicts.length > 0) {
    console.error("ERROR 功能冲突（同组合键绑定多个 action，阻断）:");
    for (const [accel, ids] of conflicts) console.error(`  ${accel} -> ${ids.join(", ")}`);
  }
  if (reservedHits.length > 0) {
    console.error("WARN 系统键重叠（默认表命中 Z-09 保留键；须在 PR 说明标签，--strict 下阻断）:");
    for (const a of reservedHits) console.error(`  ${a.id} = ${a.accel}`);
  }
  if (conflicts.length > 0) {
    console.error("\nFAIL: default binds have functional conflicts.");
    process.exit(1);
  }
  if (reservedHits.length > 0 && strict) {
    console.error("\nFAIL: reserved-key overlaps present in strict mode.");
    process.exit(1);
  }
  if (conflicts.length === 0 && reservedHits.length === 0) console.log("OK: no conflicts, no reserved-key overlaps.");
  else if (reservedHits.length > 0) console.log("PASS with warnings (see above; PR must carry explanation label).");
}

/** vite-node 转储默认键位表（失败 = 门禁失败，诚实退出）。 */
function dumpBinds() {
  const { execFileSync } = require("child_process");
  // Windows 下 spawnSync .cmd 会 EINVAL（Node 安全变更）：直接以 node 起 vite-node.mjs
  const viteNode = path.join(ROOT, "node_modules", "vite-node", "vite-node.mjs");
  try {
    const out = execFileSync(
      process.execPath,
      [viteNode, path.join(__dirname, "keymap-binds-dump.ts")],
      { cwd: ROOT, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: 120_000 },
    );
    const line = out.split(/\r?\n/).find((l) => l.trim().startsWith("{"));
    return JSON.parse(line);
  } catch (e) {
    console.error("ERROR 无法转储默认键位表（vite-node keymap-binds-dump.ts 失败）:");
    console.error(String(e.stderr || e.message).split(/\r?\n/).slice(0, 5).join("\n"));
    return null;
  }
}

main();
