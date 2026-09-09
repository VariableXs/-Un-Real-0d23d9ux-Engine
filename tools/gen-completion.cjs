#!/usr/bin/env node
/**
 * AI-20 质量门禁与收官组 — V-100 收官毕业页完成度数据生成器。
 *
 * 数据源（自动生成，绝不手工维护 —— V-100 红线）：
 *   1. docs/ENGINE-Version-01XHI9DN.1.5xw-AI分工图.md 第 3 部分任务总表
 *      → 全量 355 项（id / 名称 / 域 / 组 / 阶段）；
 *   2. docs/五路进度总览.md → 已交付 ID 集（总览表「域」列 + 各路动态段
 *      的 ID 记号，支持 A-1…A-9 区间、A-1/2/3 速记、单个 A-1）。
 * 输出：src/generated/completion.ts（GraduationWall 唯一数据源）。
 *
 * 用法：node tools/gen-completion.cjs [--check]
 *   --check：CI 门禁模式——生成结果与现存文件不一致即退出码 1
 *   （进度文档更新后未重新生成 = 毕业页数据过期，门禁拦截）。
 */

const fs = require("fs");
const path = require("path");

const ROOT = path.resolve(__dirname, "..");
const SPLIT = path.join(ROOT, "docs", "ENGINE-Version-01XHI9DN.1.5xw-AI分工图.md");
const PROGRESS = path.join(ROOT, "docs", "五路进度总览.md");
const OUT = path.join(ROOT, "src", "generated", "completion.ts");

const PREFIXES = new Set(["U", "M", "V", "N", "Z"]);

// ---------- 1. 分工图：全量任务表 ----------

const splitSrc = fs.readFileSync(SPLIT, "utf8");
// 只取第 2 部分「完整分工矩阵（355 项 × 责任组）」：从「## 2.」到「## 3.」；
// 第 5 部分是原五计划原文全量（含旧任务表），必须排除。
const sec2Start = splitSrc.indexOf("## 2.");
const sec3Start = splitSrc.indexOf("## 3.");
const matrix = splitSrc.slice(sec2Start, sec3Start);

const tasks = [];
{
  const lines = matrix.split(/\r?\n/);
  for (const line of lines) {
    const m = line.match(/^\|\s*([UMVNZ])-(\d+)\s*\|([^|]+)\|([^|]+)\|([^|]+)\|([^|]+)\|([^|]+)\|([^|]+)\|/);
    if (!m) continue;
    const id = `${m[1]}-${m[2]}`;
    const name = m[3].trim();
    const domainCell = m[4].trim();
    const groupCell = m[5].trim();
    const stageCell = m[6].trim();
    const nameTaken = m[7].trim();
    void nameTaken;
    if (!name || name.startsWith(":---")) continue;
    const domain = domainCell.includes("·") ? domainCell.split("·").slice(1).join("·") : domainCell;
    const group = groupCell.split("·")[0].trim();
    tasks.push({ id, name, domain, group, stage: stageCell });
  }
}

// ---------- 2. 进度总览：已交付 ID 集 ----------

const progressSrc = fs.readFileSync(PROGRESS, "utf8");
const delivered = new Set();
{
  // 2a. 区间：M-73…M-78 / U-19…U-24（省略号 U+2026 或 …）
  for (const m of progressSrc.matchAll(/([UMVNZ])-(\d+)\s*[…]+\s*(?:[UMVNZ]-)?(\d+)/g)) {
    const p = m[1];
    const from = Number(m[2]);
    const to = Number(m[3]);
    if (to >= from && to - from < 60) for (let i = from; i <= to; i++) delivered.add(`${p}-${i}`);
  }
  // 2b. 速记列表：U-07/08/10/55
  for (const m of progressSrc.matchAll(/\b([UMVNZ])-(\d+(?:\/\d+)+)/g)) {
    const p = m[1];
    for (const n of m[2].split("/")) {
      const num = Number(n);
      if (Number.isFinite(num)) delivered.add(`${p}-${num}`);
    }
  }
  // 2c. 单个：U-40（词边界）
  for (const m of progressSrc.matchAll(/\b([UMVNZ])-(\d{1,3})\b/g)) {
    delivered.add(`${m[1]}-${Number(m[2])}`);
  }
}

// ---------- 3. 汇总 ----------

const seen = new Set();
const uniqueTasks = tasks.filter((t) => {
  if (seen.has(t.id)) return false;
  seen.add(t.id);
  return true;
});
for (const t of uniqueTasks) t.delivered = delivered.has(t.id);

const groups = [];
{
  const gmap = new Map();
  for (const t of uniqueTasks) {
    if (!gmap.has(t.group)) gmap.set(t.group, { group: t.group, total: 0, done: 0 });
    const g = gmap.get(t.group);
    g.total++;
    if (t.delivered) g.done++;
  }
  groups.push(...[...gmap.values()].sort((a, b) => a.group.localeCompare(b.group, undefined, { numeric: true })));
}

const total = uniqueTasks.length;
const done = uniqueTasks.filter((t) => t.delivered).length;
const generatedAt = new Date().toISOString().slice(0, 10);

const body = `// ⚠️ 自动生成（tools/gen-completion.cjs）—— 请勿手工编辑；
// 更新 docs/五路进度总览.md 后运行 \`node tools/gen-completion.cjs\` 重新生成。
// 数据口径：分工图全量任务 × 进度总览已交付 ID 集（未出现在进度文档 = 如实灰色）。
export interface CompletionTask {
  id: string;
  name: string;
  domain: string;
  group: string;
  stage: string;
  delivered: boolean;
}

export interface CompletionGroup {
  group: string;
  total: number;
  done: number;
}

export const COMPLETION_GENERATED_AT = "${generatedAt}";
export const COMPLETION_TOTAL = ${total};
export const COMPLETION_DONE = ${done};
export const COMPLETION_GROUPS: CompletionGroup[] = ${JSON.stringify(groups, null, 2)};

export const COMPLETION_TASKS: CompletionTask[] = ${JSON.stringify(uniqueTasks, null, 2)};
`;

fs.mkdirSync(path.dirname(OUT), { recursive: true });
if (process.argv.includes("--check")) {
  const existing = fs.existsSync(OUT) ? fs.readFileSync(OUT, "utf8") : "";
  // --check 模式下忽略生成日期行（日期变化不算数据过期）
  const norm = (s) => s.replace(/COMPLETION_GENERATED_AT = "[^"]*"/g, "");
  if (norm(existing) !== norm(body)) {
    console.error("[gen-completion] 过期：进度文档已更新但 completion.ts 未重新生成。");
    console.error("  运行：node tools/gen-completion.cjs");
    process.exit(1);
  }
  console.log(`[gen-completion] OK (${done}/${total} delivered)`);
} else {
  fs.writeFileSync(OUT, body);
  console.log(`[gen-completion] ${OUT} 已生成：${done}/${total} 项已交付（${groups.length} 组）。`);
}
