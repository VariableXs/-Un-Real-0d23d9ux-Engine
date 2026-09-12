#!/usr/bin/env node
/**
 * AURORA-10000 领域04 catalog 生成器：
 * 从 docs/AURORA-10000-功能全景图.md 的领域04 段机械生成
 * src/system/taskbar/aurora/catalog.ts（625 项 = 25 族 × 25 项）。
 * 用法：node tools/gen-aurora-catalog4.cjs
 */
'use strict';
const fs = require('fs');
const path = require('path');

const root = path.resolve(__dirname, '..');
const doc = path.join(root, 'docs', 'AURORA-10000-功能全景图.md');
const out = path.join(root, 'src', 'system', 'taskbar', 'aurora', 'catalog.ts');

const src = fs.readFileSync(doc, 'utf8');
const lines = src.split(/\r?\n/);
const start = lines.findIndex((l) => l.startsWith('## 领域04'));
const end = lines.findIndex((l) => l.startsWith('## 领域05'));
if (start < 0 || end < 0 || end <= start) {
  console.error('未找到领域04/领域05 段边界');
  process.exit(1);
}
const seg = lines.slice(start, end);
const famRe = /^#### (族\d{4}) (.+?)（(.+?) ×25.*· 归属：(.+?)）$/;
const itemRe = /^- (F\d{5}) (.+?) — (.+?)(?:\s+✅)?$/;
const fams = [];
let cur = null;
for (const l of seg) {
  const fm = l.match(famRe);
  if (fm) {
    cur = { header: fm[1], name: fm[2], kind: fm[3], owner: fm[4], items: [] };
    fams.push(cur);
    continue;
  }
  const im = l.match(itemRe);
  if (im && cur) cur.items.push({ id: im[1], name: im[2], desc: im[3] });
}
const total = fams.reduce((a, f) => a + f.items.length, 0);
let bad = 0;
for (const f of fams) {
  if (f.items.length !== 25) {
    console.error('BAD', f.header, f.items.length);
    bad++;
  }
}
if (bad) process.exit(1);
const esc = (s) => s.replace(/\\/g, '\\\\').replace(/'/g, "\\'");
const famNum = (h) => parseInt(h.slice(1), 10);

let body = `/**
 * AURORA-10000 领域04 · 任务栏与开始菜单 全量功能目录（F01876~F02500 · AI-16~AI-20 · W2）。
 * 由 docs/AURORA-10000-功能全景图.md 领域04 段机械生成（tools/gen-aurora-catalog4.cjs），
 * 每族 25 项 × 25 族 = 625 项；每项为独立可交付单元（参数档 / 开关 / 资产）。
 * 勿手改条目文本；改全景图后重新生成。
 * AURORA-10000：AI-16~AI-20 批次，勿删。
 */

/** 单项功能：AURORA-10000 规划 ID + 名称 + 说明。 */
export interface AuroraItem {
  /** 规划 ID，形如 F01876。 */
  id: string;
  /** 族号（76~100）。 */
  fam: number;
  /** 短名（如「形态·底部居中」）。 */
  name: string;
  /** 一句说明。 */
  desc: string;
}

/** 功能族：25 项一组。 */
export interface AuroraFamily {
  /** 族号。 */
  fam: number;
  /** 族名。 */
  name: string;
  /** 参数族类型（如「形态」）。 */
  kind: string;
  /** 归属（Variable 桌面）。 */
  owner: string;
  /** 负责 AI 批次标记。 */
  ai: string;
  items: readonly AuroraItem[];
}

`;
for (const f of fams) {
  const n = famNum(f.header);
  body += `/** ${f.header} ${f.name}（${f.kind} ×25 · 归属：${f.owner}）。 */\n`;
  body += `export const FAMILY_${n}: AuroraFamily = {\n  fam: ${n},\n  name: '${esc(f.name)}',\n  kind: '${esc(f.kind)}',\n  owner: '${esc(f.owner)}',\n  ai: '${f.header}',\n  items: [\n`;
  for (const it of f.items) {
    body += `    { id: '${it.id}', fam: ${n}, name: '${esc(it.name)}', desc: '${esc(it.desc)}' },\n`;
  }
  body += `  ],\n} as const;\n\n`;
}
body += `/** 全部 25 族（按族号升序）。 */\nexport const AURORA_TASKBAR_FAMILIES: readonly AuroraFamily[] = [\n`;
for (const f of fams) body += `  FAMILY_${famNum(f.header)},\n`;
body += `] as const;\n\n`;
body += `/** 625 项平铺（按 ID 升序）。 */\nexport const AURORA_TASKBAR_ITEMS: readonly AuroraItem[] =\n  AURORA_TASKBAR_FAMILIES.flatMap((f) => f.items);\n`;

fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, body);
console.log(`families=${fams.length} items=${total} -> ${path.relative(root, out)}`);
