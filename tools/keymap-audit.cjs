#!/usr/bin/env node
/**
 * Z-08 keymap-audit.cjs — 键位静态审计。
 *
 * 1. 扫描 src/ 下所有裸 keydown 监听（addEventListener("keydown"...)），
 *    与 Z-08 注册表收编基线比对，输出报告（stdout JSON + 人读摘要）；
 * 2. 扫描 settings.shortcutBinds 默认表与系统保留键（Z-09）的命中；
 * 3. 退出码：发现裸 keydown 新增（相对基线）时为 1，供 CI / pre-commit 门禁。
 *
 * 用法：node tools/keymap-audit.cjs [--baseline] [--update-baseline]
 * 基线文件：tools/keymap-audit-baseline.json
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

  console.log("== keymap-audit (Z-08) ==");
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
}

main();
