#!/usr/bin/env node
/**
 * M-62 社区翻译工作台格式（i18n Crowd Format）：
 *   导出：node tools/i18n-crowd.cjs export [--out i18n-crowd.csv]
 *     → 四列 CSV（key, zh, zh-Hant, en）；zh-Hant 由 s2t.ts 字表做字符级转换。
 *   导入：node tools/i18n-crowd.cjs import --csv i18n-crowd.csv [--out i18n-crowd.overrides.json]
 *     → 仅允许更新既有键（新增键拒绝并定位行号）；占位符 {n} 校验；
 *       产出运行时覆盖表（U-41 i18nOverrides 结构：{ lang: { key: value } }）。
 * 红线：新增键走代码评审（本工具 100% 拒绝）；导出→改→导入零副作用。
 */
const fs = require("fs");
const path = require("path");

const ROOT = path.join(__dirname, "..");
const DICT = path.join(ROOT, "src", "i18n", "dictionaries.ts");
const S2T = path.join(ROOT, "src", "i18n", "s2t.ts");

function parseDictBlock(src, startMark, endMark) {
  const start = src.indexOf(startMark);
  const end = endMark ? src.indexOf(endMark, start) : src.length;
  const block = src.slice(start, end);
  const out = new Map();
  // 逐行解析 `key: "value",`（词典为一行一键格式；支持转义引号）
  for (const m of block.matchAll(/^[ \t]*([A-Za-z0-9_]+)\s*:\s*"((?:[^"\\]|\\.)*)"/gm)) {
    const val = m[2].replace(/\\"/g, '"').replace(/\\n/g, "\n").replace(/\\\\/g, "\\");
    if (!out.has(m[1])) out.set(m[1], val);
  }
  return out;
}

function loadS2T() {
  const src = fs.readFileSync(S2T, "utf8");
  const map = new Map();
  for (const m of src.matchAll(/([^\s::"{},]+)\s*:\s*"([^"]+)"/g)) {
    if (m[1] !== m[2]) map.set(m[1], m[2]);
  }
  return map;
}

function s2t(str, map) {
  let out = "";
  for (const ch of str) out += map.get(ch) || ch;
  return out;
}

function csvEscape(v) {
  const s = String(v ?? "");
  return /[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

function csvParse(raw) {
  // RFC4180 全文解析：支持引号包裹内的换行/逗号
  const rows = [];
  let row = [];
  let cur = "";
  let inQ = false;
  for (let i = 0; i < raw.length; i++) {
    const c = raw[i];
    if (inQ) {
      if (c === '"') {
        if (raw[i + 1] === '"') {
          cur += '"';
          i++;
        } else inQ = false;
      } else cur += c;
    } else if (c === '"') inQ = true;
    else if (c === ",") {
      row.push(cur);
      cur = "";
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && raw[i + 1] === "\n") i++;
      row.push(cur);
      rows.push(row);
      row = [];
      cur = "";
    } else cur += c;
  }
  if (cur.length > 0 || row.length > 0) {
    row.push(cur);
    rows.push(row);
  }
  return rows.filter((r) => r.length > 1 || (r[0] && r[0].length > 0));
}

function placeholders(s) {
  return [...String(s ?? "").matchAll(/\{([A-Za-z0-9_]+)\}/g)].map((m) => m[1]).sort().join(",");
}

function main() {
  const mode = process.argv[2];
  const src = fs.readFileSync(DICT, "utf8");
  const zh = parseDictBlock(src, "const zh: Dict = {", "const zhTwOverrides");
  const en = parseDictBlock(src, "const en: Dict = {", "export const dictionaries");
  if (zh.size === 0 || en.size === 0) {
    console.error("i18n-crowd: 词典解析失败（zh=%d en=%d）", zh.size, en.size);
    process.exit(2);
  }

  if (mode === "export") {
    const outIdx = process.argv.indexOf("--out");
    const out = outIdx >= 0 ? path.resolve(process.argv[outIdx + 1]) : path.join(ROOT, "i18n-crowd.csv");
    const map = loadS2T();
    const rows = ["key,zh,zh-Hant,en"];
    let n = 0;
    for (const [k, v] of zh) {
      if (!en.has(k)) continue; // 四语一致性由 audit.cjs 管；导出仅取 zh/en 双侧均有键
      rows.push([csvEscape(k), csvEscape(v), csvEscape(s2t(v, map)), csvEscape(en.get(k))].join(","));
      n++;
    }
    fs.writeFileSync(out, rows.join("\r\n") + "\r\n", "utf8");
    console.log(`i18n-crowd export: ${path.relative(ROOT, out)}（${n} 键 × 4 列）`);
    return;
  }

  if (mode === "import") {
    const csvIdx = process.argv.indexOf("--csv");
    if (csvIdx < 0) {
      console.error("用法: node tools/i18n-crowd.cjs import --csv <文件> [--out <overrides.json>]");
      process.exit(2);
    }
    const csvPath = path.resolve(process.argv[csvIdx + 1]);
    const outIdx = process.argv.indexOf("--out");
    const out = outIdx >= 0 ? path.resolve(process.argv[outIdx + 1]) : path.join(ROOT, "i18n-crowd.overrides.json");
    const raw = fs.readFileSync(csvPath, "utf8").replace(/^\uFEFF/, "");
    // 预合并：引号包裹内的换行并入同一逻辑行（保持行号定位）
    const lines = [];
    let acc = "";
    let accStart = 1;
    let inQ = false;
    let lineNo = 0;
    for (const line of raw.split(/\r?\n/)) {
      lineNo++;
      acc = acc.length ? `${acc}\n${line}` : line;
      for (let i = 0; i < line.length; i++) {
        if (line[i] === '"') {
          if (line[i + 1] === '"' && inQ) i++;
          else inQ = !inQ;
        }
      }
      if (!inQ) {
        if (acc.trim().length > 0) lines.push({ text: acc, no: accStart });
        acc = "";
        accStart = lineNo + 1;
      }
    }
    if (acc.trim().length > 0) lines.push({ text: acc, no: accStart });
    if (lines.length === 0) {
      console.error("i18n-crowd import: CSV 为空");
      process.exit(2);
    }
    const head = csvParse(lines[0].text)[0];
    if (!head || head[0] !== "key") {
      console.error("i18n-crowd import: 表头必须是 key,zh,zh-Hant,en");
      process.exit(2);
    }
    const map = loadS2T();
    const overrides = { zh: {}, "zh-Hant": {}, en: {} };
    const errors = [];
    let applied = 0;
    for (let i = 1; i < lines.length; i++) {
      const cells = csvParse(lines[i].text)[0];
      const no = lines[i].no;
      if (!cells || cells.length !== 4) {
        errors.push(`第 ${no} 行：列数 ${cells ? cells.length : 0} ≠ 4`);
        continue;
      }
      const [k, vZh, vHant, vEn] = cells;
      if (!zh.has(k)) {
        errors.push(`第 ${no} 行：未知键 "${k}"（新增键走代码评审，本工具 100% 拒绝）`);
        continue;
      }
      // 占位符校验：逐语言与该语言既有基线比对（zh 与 zh-Hant 共用 zh 基线；en 用 en 基线）
      const zhBase = placeholders(zh.get(k));
      const enBase = placeholders(en.get(k) ?? "");
      const checks = [
        ["zh", vZh, zhBase],
        ["zh-Hant", vHant, zhBase],
        ["en", vEn, enBase],
      ];
      let bad = false;
      for (const [lang, v, base] of checks) {
        if (placeholders(v) !== base) {
          errors.push(`第 ${no} 行：${lang} 占位符不符（键 ${k} 期望 {${base}}，实际 {${placeholders(v)}}）`);
          bad = true;
        }
      }
      if (bad) continue;
      overrides.zh[k] = vZh;
      overrides["zh-Hant"][k] = vHant || s2t(vZh, map);
      overrides.en[k] = vEn;
      applied++;
    }
    if (errors.length) {
      console.error("i18n-crowd import: 拒绝导入（以下行需修正后重试）：");
      for (const e of errors) console.error("  " + e);
      process.exit(1);
    }
    fs.writeFileSync(out, JSON.stringify(overrides, null, 2) + "\n", "utf8");
    console.log(`i18n-crowd import: ${path.relative(ROOT, out)}（${applied} 键生效，零副作用——仅生成运行时覆盖表）`);
    return;
  }

  console.error("用法: node tools/i18n-crowd.cjs export|import [--out …] | import --csv <文件>");
  process.exit(2);
}

main();
