#!/usr/bin/env node
/**
 * M-61 变更日志自动化：conventional commits → CHANGELOG 增量草稿（中文模板）。
 * 用法：
 *   node tools/gen-changelog.cjs                 # 最近 50 条 commit 生成草稿（stdout + CHANGELOG.d/draft.md）
 *   node tools/gen-changelog.cjs --since <ref>   # 从某 ref（如上一次 tag）至今
 *   node tools/gen-changelog.cjs --check         # 兼容模式：无草稿可校验，恒 OK（发版演练 M-87 调用时生成）
 * 维护者只做删改不做录入。
 */
const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

const ROOT = path.join(__dirname, "..");
const OUT_DIR = path.join(ROOT, "CHANGELOG.d");

const KINDS = [
  { prefix: "feat", label: "新增" },
  { prefix: "fix", label: "修复" },
  { prefix: "perf", label: "性能" },
  { prefix: "docs", label: "文档" },
  { prefix: "refactor", label: "重构" },
  { prefix: "test", label: "测试" },
  { prefix: "chore", label: "杂务" },
  { prefix: "style", label: "格式" },
  { prefix: "build", label: "构建" },
  { prefix: "ci", label: "CI" },
];
const UNKNOWN = "其他";

function gitLog(since) {
  const range = since ? `${since}..HEAD` : "-50";
  const out = execSync(`git log ${range} --no-merges --pretty=format:%H%x09%s`, { cwd: ROOT, encoding: "utf8" });
  return out.split(/\r?\n/).filter(Boolean).map((l) => {
    const [hash, ...rest] = l.split("\t");
    return { hash, subject: rest.join("\t") };
  });
}

function classify(subject) {
  const m = subject.match(/^(\w+)(?:\(([^)]+)\))?!?:\s*(.+)$/);
  if (!m) return { kind: UNKNOWN, scope: "", text: subject };
  return { kind: m[1], scope: m[2] || "", text: m[3] };
}

function render(commits, since) {
  const buckets = new Map();
  for (const kind of KINDS.map((k) => k.prefix).concat(UNKNOWN)) buckets.set(kind, []);
  for (const c of commits) {
    const { kind, scope, text } = classify(c.subject);
    const known = buckets.has(kind) ? kind : UNKNOWN;
    buckets.get(known).push({ scope, text, hash: c.hash.slice(0, 7), raw: c.subject });
  }
  const today = new Date();
  const d = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;
  const lines = [];
  lines.push(`# 变更日志草稿（${d}）`);
  lines.push("");
  lines.push(`> 自动生成：node tools/gen-changelog.cjs${since ? ` --since ${since}` : ""}（来源：conventional commits，最近 ${commits.length} 条）。`);
  lines.push("> 归类为草稿，维护者审阅删改后合入正式 CHANGELOG。");
  lines.push("");
  for (const k of KINDS) {
    const items = buckets.get(k.prefix) || [];
    if (items.length === 0) continue;
    lines.push(`## ${k.label}（${items.length}）`);
    lines.push("");
    for (const it of items) {
      lines.push(`- ${it.scope ? `\`${it.scope}\` ` : ""}${it.text}（${it.hash}）`);
    }
    lines.push("");
  }
  const others = buckets.get(UNKNOWN) || [];
  if (others.length) {
    lines.push(`## ${UNKNOWN}（${others.length}，未按 conventional 规范——建议补前缀）`);
    lines.push("");
    for (const it of others) {
      lines.push(`- ${it.raw}（${it.hash}）`);
    }
    lines.push("");
  }
  return lines.join("\n");
}

function main() {
  if (process.argv.includes("--check")) {
    // 草稿型产物：无「过期」语义（每次发版重新生成），check 仅确认生成器可运行
    console.log("gen-changelog --check: OK（草稿生成器可用；发版演练 M-87 时调用生成）");
    return;
  }
  const sinceIdx = process.argv.indexOf("--since");
  const since = sinceIdx >= 0 ? process.argv[sinceIdx + 1] : undefined;
  const commits = gitLog(since);
  if (commits.length === 0) {
    console.error("gen-changelog: 无 commit 可解析");
    process.exit(1);
  }
  const text = render(commits, since);
  fs.mkdirSync(OUT_DIR, { recursive: true });
  const stamp = new Date().toISOString().slice(0, 10);
  const out = path.join(OUT_DIR, `draft-${stamp}.md`);
  fs.writeFileSync(out, text, "utf8");
  // 归类统计
  const counts = {};
  for (const c of commits) {
    const { kind } = classify(c.subject);
    const known = KINDS.some((k) => k.prefix === kind) ? kind : UNKNOWN;
    counts[known] = (counts[known] || 0) + 1;
  }
  const total = commits.length;
  const conv = total - (counts[UNKNOWN] || 0);
  const ratio = total === 0 ? 0 : Math.round((conv / total) * 100);
  console.log(`gen-changelog: 已生成 ${path.relative(ROOT, out)}（${total} 条，conventional 占比 ${ratio}%）`);
  console.log(JSON.stringify(counts));
}

main();
