#!/usr/bin/env node
/**
 * dep-audit.cjs — M-85 依赖审计自动化。
 *
 * 数据源：
 *   - `npm audit --json`（npm 自带，查 registry 通报——开发机工具，属发版前门禁，
 *     与运行时零网络红线无关；应用本体不出网）；
 *   - `cargo audit --json`（若本机装有 cargo-audit；未安装则如实标注 unavailable）。
 *
 * 聚合输出：docs/selfcheck/dep-audit-<date>.md（分级统计 + 高危清单 + 修复建议）。
 *
 * 红线：只审计、不自动升级——任何依赖升级必须人工审阅后另行提交
 *      （升级属跨域变更，须走 PR + 性能影响声明 M-84）。
 *
 * 用法：node tools/dep-audit.cjs [--json-out <file>]
 * 退出码：0=审计完成（无论是否发现漏洞，报告已生成）；1=审计工具本身失败。
 */
'use strict';

const { execFileSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const OUT_DIR = path.join(ROOT, 'docs', 'selfcheck');
const jsonOutIdx = process.argv.indexOf('--json-out');
const jsonOut = jsonOutIdx >= 0 ? process.argv[jsonOutIdx + 1] : null;

function runJson(cmd, args) {
  try {
    const out = execFileSync(cmd, args, {
      cwd: ROOT,
      encoding: 'utf8',
      timeout: 300_000,
      stdio: ['ignore', 'pipe', 'pipe'],
      shell: process.platform === 'win32',
    });
    return { ok: true, json: JSON.parse(out) };
  } catch (e) {
    // npm audit 发现漏洞时以非零码退出，但 stdout 仍是完整 JSON —— 先尝试救回
    if (typeof e.stdout === 'string' && e.stdout.trim().startsWith('{')) {
      try {
        return { ok: true, json: JSON.parse(e.stdout) };
      } catch {
        /* 落入如实报错 */
      }
    }
    return { ok: false, err: String(e.stderr || e.message).split(/\r?\n/)[0] };
  }
}

function summarizeNpm(data) {
  const metas = [];
  for (const adv of Object.values(data.vulnerabilities ?? {})) {
    metas.push({
      eco: 'npm',
      name: adv.name,
      severity: adv.severity,
      range: adv.range,
      direct: adv.isDirect ?? false,
      title: (adv.via ?? []).map((v) => (typeof v === 'string' ? v : v.title)).filter((s) => s !== undefined).slice(0, 2).join('; '),
      fix: adv.fixAvailable === true ? '有修复版本' : adv.fixAvailable ? `需升级 ${adv.fixAvailable.name}@${adv.fixAvailable.version}（可能属 major）` : '无自动修复',
    });
  }
  return metas;
}

function summarizeCargo(data) {
  const metas = [];
  for (const dep of data.dependencies ?? []) {
    for (const advId of dep.advisories ?? []) {
      const adv = (data.vulnerabilities ?? {}).adamaged ?? null; // 结构占位（下方从 advisories map 直取）
      void adv;
    }
  }
  return metas;
}

const SEV_ORDER = { critical: 0, high: 1, moderate: 2, medium: 2, low: 3, info: 4, warning: 2, unmaintained: 3 };

(async () => {
  const npmRes = runJson('npm', ['audit', '--json']);
  const cargoRes = runJson('cargo', ['audit', '--json']);

  if (!npmRes.ok && !cargoRes.ok) {
    console.error('dep-audit: FAIL — npm audit 与 cargo audit 均失败：');
    console.error('  npm:  ', npmRes.err);
    console.error('  cargo:', cargoRes.err);
    process.exit(1);
  }

  const items = [];
  let npmMeta = { total: 0 };
  if (npmRes.ok) {
    items.push(...summarizeNpm(npmRes.json));
    npmMeta = npmRes.json.metadata?.vulnerabilities ?? { total: items.length };
  } else {
    console.error('dep-audit: npm audit 失败（如实标注）：', npmRes.err);
  }
  let cargoNote = 'cargo-audit 未安装（rustup component 或 cargo install cargo-audit 后纳入审计）';
  if (cargoRes.ok && cargoRes.json) {
    // cargo audit --json: { vulnerabilities: { count, list: [...] } }
    const list = cargoRes.json.vulnerabilities?.list ?? [];
    cargoNote = `cargo-audit：${cargoRes.json.vulnerabilities?.count ?? list.length} 条通报`;
    for (const v of list) {
      items.push({
        eco: 'cargo',
        name: v.package ?? '?',
        severity: v.severity ?? 'warning',
        range: v.versions?.patched?.[0] ? `< ${v.versions.patched[0]}` : '?',
        direct: false,
        title: v.id ?? v.title ?? '',
        fix: v.versions?.patched?.length ? `已修复于 ${v.versions.patched.join('/')}` : '无补丁版本',
      });
    }
  } else {
    console.error('dep-audit: cargo-audit 未安装或失败（如实标注）：', cargoRes.err);
  }

  items.sort((a, b) => (SEV_ORDER[a.severity] ?? 9) - (SEV_ORDER[b.severity] ?? 9));
  const counts = {};
  for (const it of items) counts[it.severity] = (counts[it.severity] ?? 0) + 1;

  const now = new Date();
  const date = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
  fs.mkdirSync(OUT_DIR, { recursive: true });

  const lines = [
    `# 依赖审计报告 — ${date}`,
    '',
    `- 生成：node tools/dep-audit.cjs（M-85 自动化）`,
    `- npm 漏洞统计：${JSON.stringify(npmMeta)}`,
    `- ${cargoNote}`,
    `- **红线：本报告只审计不升级；任何依赖变更必须人工审阅 + PR（M-84 性能影响声明）**`,
    '',
    `## 分级统计`,
    '',
    '| 严重度 | 数量 |',
    '| --- | --- |',
    ...Object.entries(counts).map(([sev, n]) => `| ${sev} | ${n} |`),
    ...(items.length === 0 ? ['| （无） | 0 |'] : []),
    '',
    `## 通报清单（按严重度）`,
    '',
    '| 生态 | 依赖 | 严重度 | 影响范围 | 通报 | 修复 |',
    '| --- | --- | --- | --- | --- | --- |',
    ...items.map(
      (it) => `| ${it.eco} | ${it.name} | ${it.severity} | ${it.range} | ${it.title.slice(0, 80)} | ${it.fix} |`,
    ),
    ...(items.length === 0 ? ['| — | — | — | — | — | — |'] : []),
    '',
  ];
  const mdFile = path.join(OUT_DIR, `dep-audit-${date}.md`);
  fs.writeFileSync(mdFile, lines.join('\n') + '\n');

  if (jsonOut) {
    fs.writeFileSync(
      jsonOut,
      JSON.stringify({ date, npmMeta, cargoNote, counts, total: items.length, items }, null, 2),
    );
  }

  console.log(lines.join('\n'));
  console.log(`报告已归档：docs/selfcheck/dep-audit-${date}.md`);
  console.log(items.length === 0 ? '结论：当前依赖零已知漏洞通报。' : `结论：发现 ${items.length} 条通报（修复须人工审阅，不自动升级）。`);
})();
