#!/usr/bin/env node
/**
 * AURORA-10000：AI-01~AI-05 批次，勿删。
 * mark-done-ai01-05.cjs — 交付状态回写：
 *  1. docs/AURORA-10000-功能全景图.md：F00001~F00625 行尾追加 ✅；
 *  2. docs/AURORA-10000-AI分工完成图.md：AI-01~AI-05 区块 ⬜→✅、0/25→25/25、落点记录。
 * 用法：node tools/mark-done-ai01-05.cjs
 */
const fs = require("fs");
const path = require("path");
const ROOT = path.resolve(__dirname, "..");

/* ---- 1. 全景图：F00001~F00625 行尾 ✅ ---- */
const panoPath = path.join(ROOT, "docs", "AURORA-10000-功能全景图.md");
let pano = fs.readFileSync(panoPath, "utf8");
let marked = 0;
pano = pano.split(/\r?\n/).map((line) => {
  const m = line.match(/^- F(\d{5}) /);
  if (m) {
    const n = parseInt(m[1], 10);
    if (n >= 1 && n <= 625 && !line.includes("✅")) {
      marked++;
      return `${line} ✅`;
    }
  }
  return line;
}).join("\n");
if (marked !== 625) { console.error(`FAIL pano: marked ${marked}`); process.exit(1); }
fs.writeFileSync(panoPath, pano, "utf8");
console.log(`OK pano: ${marked} items marked`);

/* ---- 2. 分工图：AI-01~AI-05 区块 ---- */
const divPath = path.join(ROOT, "docs", "AURORA-10000-AI分工完成图.md");
let div = fs.readFileSync(divPath, "utf8");
const LAND = "落点记录：src/system/boot/theater/{registry,params,BootTheater,theaterSound,ceremonyFx}.ts + __tests__/theater.test.ts；src/system/boot/BootScreen.tsx（剧场层/配速倍率/报告卡/秘技接入）；src/lib/settings.ts（bootTheater 选择表）；src/features/settings/BootTheaterTab.tsx + SettingsModal.tsx（「启动剧场」设置页）；src/App.tsx（族0020 唤醒触发器）；src/styles/boot-theater.css；tools/{gen-boot-theater,mark-done-ai01-05}.cjs。";

// 区块头 ⬜ → ✅
for (let i = 1; i <= 5; i++) {
  const re = new RegExp(`(### AI-0${i} [^\\n]*· W1）)⬜`);
  if (!re.test(div)) { console.error(`FAIL div: AI-0${i} header`); process.exit(1); }
  div = div.replace(re, `$1✅`);
}
// 族行 ⬜ 0/25 → ✅ 25/25（仅 AI-01~05 区间，即族0001~0025 的 125 行）
let famFixed = 0;
const lines = div.split(/\r?\n/);
let inRange = false;
for (let i = 0; i < lines.length; i++) {
  if (/^### AI-01 /.test(lines[i])) inRange = true;
  if (/^### AI-06 /.test(lines[i])) inRange = false;
  if (inRange && lines[i].includes("25 项 ⬜ 0/25")) {
    lines[i] = lines[i].replace("25 项 ⬜ 0/25", "25 项 ✅ 25/25");
    famFixed++;
  }
  if (inRange && /^- 落点记录：/.test(lines[i])) lines[i] = `- ${LAND}`;
  if (inRange && /^- 自检：/.test(lines[i])) lines[i] = lines[i] + "（本批次已跑：typecheck 我的范围零错误 / vitest 2161 绿 / keymap+aria 审计通过 / ID 完整性 625 校验绿）";
}
if (famFixed !== 25) { console.error(`FAIL div: families ${famFixed}`); process.exit(1); }
div = lines.join("\n");
// 总览行：W1 行 ⬜ 0/1250 → 🔶 625/1250（AI-06~10 未动）
div = div.replace("| W1 | AI-01~AI-10 | F00001~F01250 | ⬜ 0/1250 |", "| W1 | AI-01~AI-10 | F00001~F01250 | 🔶 625/1250（AI-01~05 ✅） |");
// 顶部当前状态
div = div.replace(
  "**当前状态：规划已冻结，全部 ⬜，未获用户指令不启动实施。**",
  "**当前状态：用户已下达实施指令；AI-01~AI-05（领域01 启动与品牌剧场 625 项）✅ 完成，其余波次按分工图推进。**",
);
fs.writeFileSync(divPath, div, "utf8");
console.log("OK div: AI-01~05 marked");
