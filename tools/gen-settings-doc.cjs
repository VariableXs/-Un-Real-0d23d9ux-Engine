#!/usr/bin/env node
/**
 * M-56 设置项自动文档生成器：src/lib/settings.ts → docs/SETTINGS.md
 * 解析 Settings interface（键/类型/JSDoc 描述）与 DEFAULT_SETTINGS（默认值），
 * 生成《设置手册》：键名、类型、默认值、说明（含取值范围注释）、所有者（AI 组标记）、since。
 * 与导出格式（Z-51）字段一一对应（键即 schema 键）。
 * 用法：
 *   node tools/gen-settings-doc.cjs           # 生成 docs/SETTINGS.md
 *   node tools/gen-settings-doc.cjs --check   # 校验同步（不同步退出 1 → CI 红）
 */
const fs = require("fs");
const path = require("path");

const ROOT = path.join(__dirname, "..");
const SETTINGS_TS = path.join(ROOT, "src", "lib", "settings.ts");
const OUT = path.join(ROOT, "docs", "SETTINGS.md");

function parseInterface(src) {
  const start = src.indexOf("export interface Settings {");
  if (start < 0) throw new Error("未找到 Settings interface");
  const bodyStart = src.indexOf("{", start);
  let depth = 0;
  let end = bodyStart;
  for (let i = bodyStart; i < src.length; i++) {
    if (src[i] === "{") depth++;
    else if (src[i] === "}") {
      depth--;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }
  const body = src.slice(bodyStart + 1, end);
  const out = [];
  // 按行扫描：JSDoc 块（/** ... */）+ 字段行 key: type;
  const lines = body.split(/\r?\n/);
  let doc = [];
  for (const line of lines) {
    const t = line.trim();
    if (t.startsWith("/**") || t.startsWith("*") || t.endsWith("*/")) {
      if (t.startsWith("/**")) doc = [];
      const txt = t.replace(/^\/\*\*/, "").replace(/\*\/$/, "").replace(/^\*\s?/, "").trim();
      if (txt) doc.push(txt);
      continue;
    }
    const m = t.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*(\?)?\s*:\s*(.+?);?\s*(?:\/\/\s*(.*))?$/);
    if (m) {
      out.push({ key: m[1], optional: !!m[2], type: m[3].trim(), doc: doc.join(" "), lineNote: (m[4] || "").trim() });
      doc = [];
    }
  }
  return out;
}

function parseDefaults(src) {
  const start = src.indexOf("export const DEFAULT_SETTINGS");
  const bodyStart = src.indexOf("{", start);
  let depth = 0;
  let end = bodyStart;
  for (let i = bodyStart; i < src.length; i++) {
    if (src[i] === "{") depth++;
    else if (src[i] === "}") {
      depth--;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }
  const body = src.slice(bodyStart + 1, end);
  // 顶层 key: value —— 花括号深度为 0 时切分
  const out = [];
  let depth2 = 0;
  let curKey = null;
  let curVal = [];
  const flush = () => {
    if (curKey) out.push([curKey, curVal.join("\n").trim().replace(/,$/, "")]);
    curKey = null;
    curVal = [];
  };
  for (const line of body.split(/\r?\n/)) {
    const t = line.trim();
    if (!t) continue;
    if (depth2 === 0) {
      const m = t.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(.*)$/);
      if (m) {
        flush();
        curKey = m[1];
        curVal = [m[2]];
        depth2 = (curVal[0].match(/\{/g) || []).length - (curVal[0].match(/\}/g) || []).length;
        if (depth2 <= 0 && curVal[0].includes(",")) {
          // 单行标量/数组完结
          flush();
          depth2 = 0;
        }
        continue;
      }
    } else {
      curVal.push(t);
      depth2 += (t.match(/\{/g) || []).length - (t.match(/\}/g) || []).length;
      if (depth2 <= 0) {
        flush();
        depth2 = 0;
      }
    }
  }
  flush();
  return new Map(out);
}

function ownerOf(fields) {
  const hay = fields.join(" ");
  const m = hay.match(/AI-(\d{2})/);
  return m ? `AI-${m[1]}` : "—";
}

function render(fields, defaults) {
  const lines = [];
  lines.push("# SETTINGS — 设置手册（自动生成）");
  lines.push("");
  lines.push("> 自动生成：`node tools/gen-settings-doc.cjs`（源：`src/lib/settings.ts`）。手工修改会被下次生成覆盖。");
  lines.push("> 新增设置项不改文档，下次发版自动出现在本手册；默认值与代码实测一致（生成时直接读取 DEFAULT_SETTINGS）。");
  lines.push("> 键与数据开放导出（Z-51）/ 配置分享（Z-54）字段一一对应。");
  lines.push("");
  lines.push(`共 ${fields.length} 个设置项。`);
  lines.push("");
  lines.push("| 键 | 类型 | 默认值 | 说明 | 所有者 | Since |");
  lines.push("|---|---|---|---|---|---|");
  for (const f of fields) {
    const def = defaults.get(f.key);
    const defText = def === undefined ? "—" : `\`${def.replace(/\s+/g, " ").slice(0, 60)}${def.replace(/\s+/g, " ").length > 60 ? " …" : ""}\``;
    const desc = f.doc && f.lineNote ? `${f.doc}（${f.lineNote}）` : (f.doc || f.lineNote || "");
    lines.push(`| \`${f.key}\` | \`${f.type.replace(/\|/g, "\\|")}\` | ${defText} | ${desc.replace(/\|/g, "\\|") || "—"} | ${ownerOf([f.doc, f.lineNote])} | 1.5xw |`);
  }
  lines.push("");
  return lines.join("\n");
}

function main() {
  const src = fs.readFileSync(SETTINGS_TS, "utf8");
  const fields = parseInterface(src);
  const defaults = parseDefaults(src);
  const text = render(fields, defaults);
  if (process.argv.includes("--check")) {
    const cur = fs.existsSync(OUT) ? fs.readFileSync(OUT, "utf8") : "";
    if (cur !== text) {
      console.error("gen-settings-doc --check: docs/SETTINGS.md 与 src/lib/settings.ts 不同步（请运行 node tools/gen-settings-doc.cjs 后提交）");
      process.exit(1);
    }
    console.log(`gen-settings-doc --check: OK（${fields.length} 项设置，文档与代码同步）`);
    return;
  }
  fs.writeFileSync(OUT, text, "utf8");
  console.log(`gen-settings-doc: 已生成 docs/SETTINGS.md（${fields.length} 项设置）`);
}

main();
