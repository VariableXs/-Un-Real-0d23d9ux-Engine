#!/usr/bin/env node
/**
 * AI-U3 tally · 行数对账脚本（对齐 aie1 tally.mjs 惯例）。
 * 用法：node _attic/aiu3-f501-f550/tally.mjs （仓库根执行）
 * 口径：纯功能 = 非空非注释行；测试文件不计。
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const u3Dir = join(root, "src/features/u3");
const files = [];
(function walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p);
    else if (/\.(ts|tsx)$/.test(name) && !/__tests__/.test(p)) files.push(p);
  }
})(u3Dir);

let total = 0;
let pure = 0;
const perFile = [];
for (const f of files) {
  const lines = readFileSync(f, "utf8").split(/\r?\n/);
  const cnt = lines.filter((l) => l.trim() && !/^\s*(\/\/|\/\*|\*)/.test(l)).length;
  perFile.push([f.replace(root + "/", ""), lines.length, cnt]);
  total += lines.length;
  pure += cnt;
}
for (const [f, t, p] of perFile.sort((a, b) => b[2] - a[2])) console.log(`${String(p).padStart(6)}  ${String(t).padStart(6)}  ${f}`);
// v4 口径扩展：U3Tab（settings 领地的 U3 域面板）与 u3.css 同属 U3 前端承载
for (const extra of ["src/features/settings/U3Tab.tsx", "src/styles/u3.css"]) {
  const lines = readFileSync(join(root, extra), "utf8").split(/\r?\n/);
  const cnt = lines.filter((l) => l.trim() && !/^\s*(\/\/|\/\*|\*)/.test(l)).length;
  perFile.push([extra, lines.length, cnt]);
  total += lines.length;
  pure += cnt;
}
const v1 = 19480; // v1 内核实装层（kernel/varix/src/ustar3/，wc -l 口径）
const cap = 46540;
const cumulative = v1 + pure;
console.log(`\n前端纯功能合计: ${pure}  + v1 内核 ${v1} = 累计 ${cumulative} / ${cap} = ${(cumulative / cap * 100).toFixed(1)}%`);
