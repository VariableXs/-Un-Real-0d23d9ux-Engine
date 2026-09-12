#!/usr/bin/env node
/**
 * AURORA-10000：AI-01~AI-05 批次，勿删。
 * gen-boot-theater.cjs — 从 docs/AURORA-10000-功能全景图.md 提取
 * 领域01（族0001~族0025，F00001~F00625）生成 src/system/boot/theater/registry.ts。
 * 保证代码注册表与全景图逐字一致（ID/名称/描述三对齐）。
 * 用法：node tools/gen-boot-theater.cjs
 */
const fs = require("fs");
const path = require("path");

const ROOT = path.resolve(__dirname, "..");
const DOC = path.join(ROOT, "docs", "AURORA-10000-功能全景图.md");
const OUT = path.join(ROOT, "src", "system", "boot", "theater", "registry.ts");

const KINDS = [
  "arc", "breath", "narrative", "soundscape", "color",
  "diagnostic", "gauge", "trust", "recovery", "firmware",
  "splash", "transition", "countdown", "selector", "log",
  "wizard", "workshop", "mood", "shutdown", "wake",
  "pacing", "a11y", "soundId", "egg", "report",
];

const doc = fs.readFileSync(DOC, "utf8");
const lines = doc.split(/\r?\n/);

const families = [];
let cur = null;
let inDomain = false;
for (const line of lines) {
  if (line.startsWith("## 领域01")) inDomain = true;
  if (line.startsWith("## 领域02")) break;
  if (!inDomain) continue;
  const fam = line.match(/^#### 族(\d{4}) (.+?)（(.*)·\s*归属：(.+?)）\s*$/);
  if (fam) {
    const num = parseInt(fam[1], 10);
    if (num < 1 || num > 25) { cur = null; continue; }
    cur = { num, name: fam[2], attr: fam[4].trim(), items: [] };
    families.push(cur);
    continue;
  }
  const item = cur && line.match(/^- F(\d{5}) (.+?) — (.+?)\s*$/);
  if (item) {
    cur.items.push({ id: `F${item[1]}`, name: item[2].trim(), desc: item[3].trim() });
  }
}

if (families.length !== 25) {
  console.error(`FAIL: expected 25 families, got ${families.length}`);
  process.exit(1);
}
for (const f of families) {
  if (f.items.length !== 25) {
    console.error(`FAIL: family 族${String(f.num).padStart(4, "0")} has ${f.items.length} items`);
    process.exit(1);
  }
}
const total = families.reduce((n, f) => n + f.items.length, 0);
if (total !== 625) { console.error(`FAIL: total ${total}`); process.exit(1); }

const esc = (s) => s.replace(/\\/g, "\\\\").replace(/"/g, '\\"');

const body = families.map((f) => {
  const items = f.items
    .map((it) => `    { id: "${it.id}", name: "${esc(it.name)}", desc: "${esc(it.desc)}" },`)
    .join("\n");
  return `  {\n    num: ${f.num},\n    name: "${esc(f.name)}",\n    attr: "${esc(f.attr)}",\n    kind: "${KINDS[f.num - 1]}",\n    items: [\n${items}\n    ]\n  }`;
}).join(",\n");

const ts = `// AURORA-10000：AI-01~AI-05 批次，勿删。
// 本文件由 tools/gen-boot-theater.cjs 从 docs/AURORA-10000-功能全景图.md 生成，
// 覆盖领域01 启动与品牌剧场 族0001~族0025（F00001~F00625）共 625 项；
// 每一项都是独立可交付单元（独立参数档），ID/名称/描述与全景图逐字对齐。
// 禁止手改本文件；改动请改生成脚本或全景图后重新生成。

export type TheaterKind =
  | "arc" | "breath" | "narrative" | "soundscape" | "color"
  | "diagnostic" | "gauge" | "trust" | "recovery" | "firmware"
  | "splash" | "transition" | "countdown" | "selector" | "log"
  | "wizard" | "workshop" | "mood" | "shutdown" | "wake"
  | "pacing" | "a11y" | "soundId" | "egg" | "report";

export interface TheaterItem {
  /** 全景图唯一编号，如 F00001。 */
  id: string;
  /** 全景图条目名（如「光弧·极简细线」）。 */
  name: string;
  /** 全景图条目描述。 */
  desc: string;
}

export interface TheaterFamily {
  num: number;
  name: string;
  attr: string;
  kind: TheaterKind;
  items: TheaterItem[];
}

export const THEATER_FAMILIES: readonly TheaterFamily[] = [
${body}
];

export const THEATER_ALL_ITEMS: readonly TheaterItem[] = THEATER_FAMILIES.flatMap((f) => f.items);

const BY_ID = new Map(THEATER_ALL_ITEMS.map((it) => [it.id, it]));

export function findTheaterItem(id: string): TheaterItem | undefined {
  return BY_ID.get(id);
}

export function findTheaterFamily(kind: TheaterKind): TheaterFamily | undefined {
  return THEATER_FAMILIES.find((f) => f.kind === kind);
}
`;

fs.mkdirSync(path.dirname(OUT), { recursive: true });
fs.writeFileSync(OUT, ts, "utf8");
console.log(`OK: registry.ts written (${families.length} families, ${total} items)`);
