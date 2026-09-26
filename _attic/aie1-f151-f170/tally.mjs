#!/usr/bin/env node
/**
 * AI-E1 · F151-F170 检查项对账 tally（对齐 AI-C2 tally.rs / AI-V2 账本先例）。
 *
 * 口径：E 域是 TS/React 域，「检查项」= vitest 用例（it 块）。用例归属 F 项 =
 * 所在 describe 块名中显式标注的 F 编号；describe 未标注 F 时归属其文件
 * 头部声明的 F 集（文件名或 import 注释无法判定时归 UI/底座桶）。
 *
 * 用法：node tally.mjs [repo 根目录，缺省=当前目录]
 * 输出：逐文件用例数 → 逐 F 项归属 → 总计。
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, basename } from "node:path";

const root = process.argv[2] ?? process.cwd();
const testDir = join(root, "src", "system", "persona", "__tests__");

if (!statSync(testDir, { throwIfNoEntry: false })) {
  console.error(`找不到 ${testDir}——请在仓库根目录运行`);
  process.exit(1);
}

const FILES = readdirSync(testDir).filter((f) => f.endsWith(".test.ts")).sort();

// 文件 → F 集（显式映射——一处一事实，人工核对过；新增测试文件须登记此表）。
const FILE_TO_F = {
  "tokens.test.ts": ["F151"],
  "store.test.ts": ["底座"],
  "preview-autodark.test.ts": ["F152", "F153"],
  "dailywall-iconswap.test.ts": ["F154", "F155"],
  "pointer-sound.test.ts": ["F156", "F157"],
  "startmenu-font.test.ts": ["F158", "F159"],
  "motion-archive-appexcept.test.ts": ["F160", "F161", "F162"],
  "widgets-lock-boot-ime.test.ts": ["F163", "F164", "F165", "F166"],
  "ctxmenu-taskbar.test.ts": ["F167", "F168"],
  "shortcuts.test.ts": ["F169"],
  "verdict.test.ts": ["F170"],
  "engines-batch1.test.ts": ["F151", "F153", "F154", "F155"],
  "engines-batch2.test.ts": ["F156", "F157", "F158", "F159", "F160"],
  "engines-batch3.test.ts": ["F151", "F152", "F153", "F154", "F159", "F161", "F163", "F166", "F167", "F170"],
  "engines-batch4.test.ts": ["F151", "F154", "F155", "F156", "F157"],
  "engines-batch5.test.ts": ["F163", "F164", "F166"],
  "engines-batch6.test.ts": ["F151", "F153", "F154", "F167", "F168", "F169", "F170"],
  "engines-batch7.test.ts": ["F156", "F165", "F151", "F162"],
  "engines-batch8.test.ts": ["F161", "F152"],
  "engines-batch9.test.ts": ["F153", "F163", "F167", "F169", "F161", "F170"],
  "engines-batch10.test.ts": ["F124", "F160", "F205", "F206"],
  "engines-batch11.test.ts": ["F127", "F156", "F168", "F214", "F237"],
  "engines-batch12.test.ts": ["F161", "F164", "F170", "通用十二查"],
  "engines-batch13.test.ts": ["通用十二查", "F161", "F170"],
};

const perFile = {};
const perF = {};
let total = 0;

for (const f of FILES) {
  const src = readFileSync(join(testDir, f), "utf8");
  // it( 计数：顶格/缩进的 it( 与 it.each( 均计入（test( 别名本域未使用）。
  const cases = (src.match(/\bit(?:\.each)?\(/g) ?? []).length;
  perFile[f] = cases;
  total += cases;
  const fs = FILE_TO_F[f] ?? ["未登记"];
  for (const fid of fs) {
    perF[fid] = (perF[fid] ?? 0) + cases; // 一个用例服务多个 F 项时按贡献各计一次（对账口径注明）
  }
}

console.log("== 逐文件 ==");
for (const [f, n] of Object.entries(perFile)) console.log(`${f.padEnd(42)} ${n}`);
console.log(`\n== 逐 F 项（一用例可贡献多项——总贡献口径） ==`);
const fKeys = Object.keys(perF).sort((a, b) => (Number.isInteger(+a.slice(1)) ? +a.slice(1) : 999) - (Number.isInteger(+b.slice(1)) ? +b.slice(1) : 999));
for (const fid of fKeys) console.log(`${fid.padEnd(8)} ${perF[fid]}`);
console.log(`\nTOTAL 用例（去重口径）= ${total}　F 项桶数 = ${fKeys.filter((k) => k.startsWith("F")).length}`);
