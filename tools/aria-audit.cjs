#!/usr/bin/env node
/**
 * M-76 aria-audit.cjs — 屏幕阅读器标注静态审计（ARIA Audit）。
 *
 * 1. 扫描 src/ 下所有 .tsx 的 <button> 块：图标按钮（无可见文本）
 *    必须带 aria-label，否则记 finding（缺 label 即 fail）；
 * 2. 统计主路径 aria 覆盖率：aria-label / aria-live / aria-keyshortcuts
 *    / role 出现次数（覆盖率报告，每发版出一份）；
 * 3. 退出码：相对基线（tools/aria-audit-baseline.json）新增缺失即 1，
 *    供 CI / pre-commit 门禁。
 *
 * 用法：node tools/aria-audit.cjs [--update-baseline]
 */

const fs = require("fs");
const path = require("path");

const ROOT = path.resolve(__dirname, "..");
const SRC = path.join(ROOT, "src");
const BASELINE = path.join(__dirname, "aria-audit-baseline.json");

function walk(dir, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(p, out);
    else if (/\.tsx$/.test(entry.name)) out.push(p);
  }
  return out;
}

/** 提取文件中所有 <button ...>...</button> 块（跨行，含自闭合误报剔除）。 */
function buttonBlocks(text) {
  const blocks = [];
  const re = /<button\b[^>]*>([\s\S]*?)<\/button>/g;
  let m;
  while ((m = re.exec(text)) !== null) {
    const openTag = m[0].slice(0, m[0].indexOf(">") + 1);
    blocks.push({ openTag, inner: m[1], index: m.index });
  }
  return blocks;
}

/** 块内是否有可见文本（剔除 JSX 标签；{expr} 会被渲染为文本，保留判定）。 */
function visibleText(inner) {
  return inner
    .replace(/<[^>]*>/g, "")
    .replace(/&[a-z]+;/g, "")
    .trim();
}

function lineOf(text, index) {
  let line = 1;
  for (let i = 0; i < index && i < text.length; i++) {
    if (text[i] === "\n") line++;
  }
  return line;
}

function main() {
  const updateBaseline = process.argv.includes("--update-baseline");
  const findings = [];
  const stats = { buttons: 0, labeledButtons: 0, ariaLabel: 0, ariaLive: 0, ariaKeyshortcuts: 0, role: 0, files: 0 };

  for (const file of walk(SRC)) {
    const text = fs.readFileSync(file, "utf8");
    if (!/<button/.test(text) && !/aria-/.test(text)) continue;
    stats.files++;
    const rel = path.relative(ROOT, file).replace(/\\/g, "/");
    for (const attr of ["aria-label", "aria-live", "aria-keyshortcuts"]) {
      stats[attr === "aria-label" ? "ariaLabel" : attr === "aria-live" ? "ariaLive" : "ariaKeyshortcuts"] +=
        (text.match(new RegExp(attr, "g")) || []).length;
    }
    stats.role += (text.match(/\brole=/g) || []).length;
    for (const b of buttonBlocks(text)) {
      stats.buttons++;
      const hasLabel = /aria-label(=|\s*=)/.test(b.openTag) || /aria-label/.test(b.inner);
      const label = hasLabel || visibleText(b.inner) !== "";
      if (label) stats.labeledButtons++;
      else {
        findings.push({ file: rel, line: lineOf(text, b.index), openTag: b.openTag.slice(0, 110) });
      }
    }
  }

  let baseline = { count: 0 };
  if (fs.existsSync(BASELINE)) baseline = JSON.parse(fs.readFileSync(BASELINE, "utf8"));

  if (updateBaseline) {
    fs.writeFileSync(BASELINE, JSON.stringify({ count: findings.length, findings }, null, 2));
    console.log(`baseline updated: ${findings.length} unlabeled icon buttons`);
    return;
  }

  const coverage = stats.buttons ? Math.round((stats.labeledButtons / stats.buttons) * 100) : 100;
  console.log("== aria-audit (M-76) ==");
  console.log(`icon-only buttons without aria-label: ${findings.length} (baseline ${baseline.count})`);
  console.log(`button label coverage: ${stats.labeledButtons}/${stats.buttons} (${coverage}%)`);
  console.log(`aria-label occurrences: ${stats.ariaLabel}, aria-live: ${stats.ariaLive}, aria-keyshortcuts: ${stats.ariaKeyshortcuts}, role: ${stats.role} (files scanned: ${stats.files})`);
  for (const f of findings.slice(0, 60)) {
    console.log(`  ${f.file}:${f.line}  ${f.openTag}`);
  }
  if (findings.length > (baseline.count ?? 0)) {
    console.error(`\nFAIL: ${findings.length - (baseline.count ?? 0)} new unlabeled icon button(s) — every icon-only button MUST have aria-label (M-76 iron rule).`);
    console.error("Fix them, or acknowledge the current level with: node tools/aria-audit.cjs --update-baseline");
    process.exit(1);
  }
  console.log("\nOK: no new unlabeled icon buttons.");
}

main();
