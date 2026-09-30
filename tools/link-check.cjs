#!/usr/bin/env node
/**
 * link-check.cjs — M-86 文档链接检查（P0）。
 *
 * 扫描 README.md 与 docs 目录全树的 markdown 相对链接与锚点：
 *   - 相对文件目标（.md/.ts/.png/目录…）：目标必须存在；
 *   - `#锚点`（同页或跨文件）：目标文件按 GitHub slug 规则生成的锚点集必须包含；
 *   - 外链（http/https/mailto）不在检查范围（零网络红线，不做在线探测）；
 *   - 代码围栏内的「链接样例」跳过。
 *
 * 断链 → 逐条列出 + 退出码 1（CI 门禁 / pre-push 手跑）。
 * 用法：node tools/link-check.cjs [--quiet]
 */
'use strict';

const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const quiet = process.argv.includes('--quiet');

/** 待扫描根：README + docs 全树（其余目录的 md 属历史归档，不进 P0 范围）。 */
function collectMarkdown() {
  const out = [path.join(ROOT, 'README.md')];
  const docsDir = path.join(ROOT, 'docs');
  if (fs.existsSync(docsDir)) {
    const walk = (dir) => {
      for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
        const p = path.join(dir, e.name);
        if (e.isDirectory()) walk(p);
        else if (e.name.endsWith('.md')) out.push(p);
      }
    };
    walk(docsDir);
  }
  return out.filter((p) => fs.existsSync(p));
}

/** GitHub 风格 heading slug（github-slugger 近似：小写、去标点、空白→-、CJK 保留）。 */
function slugify(heading) {
  return heading
    .trim()
    .toLowerCase()
    .replace(/`/g, '')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1') // 链接取文字
    .replace(/[!'"#$%&()*+,./:;<=>?@[\]^`{|}~\u2014\u2018\u2019\u201c\u201d\u2026]/g, '')
    .replace(/\s+/g, '-');
}

/** 提取一个 md 文件的锚点集：ATX 标题 slug + <a id/name> 显式锚。 */
function anchorsOf(text) {
  const set = new Set();
  for (const m of text.matchAll(/^#{1,6}\s+(.+?)\s*#*\s*$/gm)) set.add(slugify(m[1]));
  for (const m of text.matchAll(/<a\s+(?:id|name)=["']([^"']+)["']/gi)) set.add(m[1]);
  return set;
}

/** 主扫描：返回断链列表 [{file, line, target, reason}]。 */
function scan() {
  const broken = [];
  const files = collectMarkdown();
  for (const file of files) {
    const rel = path.relative(ROOT, file).replace(/\\/g, '/');
    const text = fs.readFileSync(file, 'utf8');
    const lines = text.split(/\r?\n/);

    let inFence = false;
    lines.forEach((line, i) => {
      if (/^\s*(```|~~~)/.test(line)) {
        inFence = !inFence;
        return;
      }
      if (inFence) return;
      // 内联代码段（`...`）中的 []() 样例不是链接（如 `embed_windows[](hwnd/...)`）
      const prose = line.replace(/`[^`]*`/g, '');
      for (const m of prose.matchAll(/(!?)\[([^\]]*)\]\(\s*([^)\s]+)(?:\s+"[^"]*")?\s*\)/g)) {
        const rawTarget = m[3];
        if (/^(https?:|mailto:|data:)/i.test(rawTarget)) continue; // 外链不查（零网络）
        const target = rawTarget.replace(/^<|>$/g, '');
        const check = checkTarget(file, target, text);
        if (check) broken.push({ file: rel, line: i + 1, target, reason: check });
      }
    });
  }
  return broken;
}

/** 校验单个目标；返回 undefined=通过，否则断链原因。 */
function checkTarget(srcFile, rawTarget, srcText) {
  let target = rawTarget;
  // 百分号编码还原（中文文件名常见）
  try {
    target = decodeURIComponent(rawTarget);
  } catch {
    /* 保留原样 */
  }

  // 纯锚点（同页）
  if (target.startsWith('#')) {
    const anchor = target.slice(1);
    if (!anchor) return undefined; // '#' 空锚视为回到顶部，放行
    if (anchorsOf(srcText).has(anchor)) return undefined;
    return `同页锚点 #${anchor} 不存在`;
  }

  const hashIdx = target.indexOf('#');
  const filePathPart = hashIdx >= 0 ? target.slice(0, hashIdx) : target;
  const anchorPart = hashIdx >= 0 ? target.slice(hashIdx + 1) : null;
  if (!filePathPart) return undefined;

  const abs = path.resolve(path.dirname(srcFile), filePathPart);
  if (!fs.existsSync(abs)) return `目标文件不存在`;

  if (anchorPart) {
    const stat = fs.statSync(abs);
    if (stat.isDirectory()) return `目录目标带锚点无意义`;
    if (abs.endsWith('.md')) {
      const anchors = anchorsOf(fs.readFileSync(abs, 'utf8'));
      if (!anchors.has(anchorPart)) return `锚点 #${anchorPart} 在目标文件中不存在`;
    } else {
      return `非 md 目标不支持锚点`;
    }
  }
  return undefined;
}

const broken = scan();
if (broken.length === 0) {
  if (!quiet) console.log('link-check (M-86): OK — 相对链接与锚点零断链。');
  process.exit(0);
}
console.error(`link-check (M-86): FAIL — ${broken.length} 处断链：`);
for (const b of broken) console.error(`  ${b.file}:${b.line}  ${b.target}  → ${b.reason}`);
process.exit(1);
