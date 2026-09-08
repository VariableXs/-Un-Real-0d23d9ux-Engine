#!/usr/bin/env node
/**
 * AI-13 M-52 冷启动 A/B 对照（Boot A/B Comparator）
 *
 * 用法：
 *   node tools/boot-diff.cjs docs/bench/boot-1.5.8.json docs/bench/boot-1.5.9.json
 *
 * 规则：与上一版 diff，同名阶段回归 >10% 标红并给出归因建议（按阶段名）；
 * 固定口径：同机空载 5 次取中位（数据由 Z-58/U-19 侧导出的 boot-<version>.json 提供）。
 */

const fs = require("node:fs");

function load(p) {
  const raw = JSON.parse(fs.readFileSync(p, "utf8"));
  if (!Array.isArray(raw.marks)) throw new Error(`${p}: missing "marks"`);
  return raw;
}

function durations(marks) {
  const out = new Map();
  for (let i = 0; i < marks.length - 1; i += 1) {
    out.set(marks[i].name, marks[i + 1].atMs - marks[i].atMs);
  }
  return out;
}

/** 归因建议（按阶段名映射；P0=阻断 / P1=可延迟 / P2=托盘化）。 */
function advise(name) {
  const table = [
    ["app-shell", "P0 阶段：检查 React 挂载前的同步模块（import 链）与首帧阻塞脚本"],
    ["db-open", "P0 阶段：SQLite 打开/迁移耗时——检查 migrate 是否被重复执行"],
    ["settings-load", "P1 阶段：可延迟——确认设置解析未在首帧前同步展开"],
    ["icons-warm", "P2 阶段：应托盘化/预热缓存——mtime 快照预热是否退化成全量扫描"],
    ["media-import", "P2 阶段：媒体索引应后台切片（IO 治理 U-22 分块管道）"],
  ];
  const hit = table.find(([k]) => name.includes(k));
  return hit ? hit[1] : "通用建议：用 performance.mark 细分该阶段，确认是否含同步 IO 或大 JSON 解析";
}

function main() {
  const [basePath, curPath] = process.argv.slice(2);
  if (!basePath || !curPath) {
    console.error("用法: node tools/boot-diff.cjs <base boot-<ver>.json> <current boot-<ver>.json>");
    process.exit(2);
  }
  const base = load(basePath);
  const cur = load(curPath);
  const b = durations(base.marks);
  const c = durations(cur.marks);
  let regressed = 0;
  const rows = [];
  for (const [name, baseMs] of b) {
    if (!c.has(name)) continue;
    const curMs = c.get(name);
    const pct = baseMs === 0 ? 0 : ((curMs - baseMs) / baseMs) * 100;
    const bad = pct > 10;
    if (bad) regressed += 1;
    rows.push({
      name,
      baseMs,
      curMs,
      deltaPct: Math.round(pct * 10) / 10,
      verdict: bad ? "RED" : "ok",
      advice: bad ? advise(name) : undefined,
    });
  }
  const report = {
    base: { version: base.version, file: basePath },
    current: { version: cur.version, file: curPath },
    rows,
    regressedCount: regressed,
    gate: regressed === 0 ? "PASS" : "FAIL（回归阶段 >10% 需归因后修）",
  };
  console.log(JSON.stringify(report, null, 2));
  process.exit(regressed === 0 ? 0 : 1);
}

main();
