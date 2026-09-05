#!/usr/bin/env node
/**
 * bench.cjs — 性能基准（MASTER-PLAN B-1 交付，回归门禁数据源）
 *
 * 四项基准（对应 BLUEPRINT 3.14 预算表）：
 *   1. coldStart   冷启动到主窗口可见（预算 ≤3s，SSD 主机）
 *   2. fileIndex   万文件索引（预算：10000 文件 < 3s）
 *   3. vwmOpen     VWM 打开延迟（预算 <50ms）——需 GUI 插桩，当前如实标注 SKIPPED
 *   4. memory      应用待机内存水位（预算 ≤600MB）
 *
 * 用法：
 *   node tools/bench.cjs             # 跑全部可跑项，报告写入 docs/bench/
 *   node tools/bench.cjs --check     # 与最近一份归档基线对比，>10% 回归退出码 1
 *   node tools/bench.cjs --no-gui    # 跳过需要拉起应用的项（coldStart/memory）
 *
 * 纪律：零网络、零遥测；GUI 项拉起的是本仓库自己构建的 variable.exe，
 *       结束时优雅关闭自己的子进程（绝不触碰其他进程）。
 */
'use strict';

const { spawn, execFileSync } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const BENCH_DIR = path.join(ROOT, 'docs', 'bench');
const EXE = path.join(ROOT, 'src-tauri', 'target', 'release', 'variable.exe');
const REGRESSION_LIMIT = 0.10; // >10% 回归即红（MASTER-PLAN 第 6 节）

const args = new Set(process.argv.slice(2));
const noGui = args.has('--no-gui');
const checkOnly = args.has('--check');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const fmt = (v, unit) => (v == null ? '—' : `${Math.round(v * 100) / 100}${unit}`);

function ps(command) {
  return execFileSync('powershell', ['-NoProfile', '-Command', command], {
    encoding: 'utf8',
    timeout: 15000,
  }).trim();
}

// ---------- 1+4. 冷启动 / 内存（对 variable.exe 实测） ----------
async function benchGui() {
  if (noGui) return { coldStart: null, memory: null, skipped: '按要求跳过 GUI 项 (--no-gui)' };
  if (!fs.existsSync(EXE)) {
    return { coldStart: null, memory: null, skipped: `未找到 ${EXE}（先 npm run tauri build 或 cargo build --release）` };
  }

  const t0 = Date.now();
  const child = spawn(EXE, [], { detached: false, stdio: 'ignore', windowsHide: true });
  let coldStartMs = null;
  let memoryMb = null;
  const deadline = t0 + 30000;

  try {
    while (Date.now() < deadline) {
      let info = '';
      try {
        // 按进程名查询（Tauri 单实例可能换 pid）；窗口句柄取首个非零，内存取全部 variable 进程之和
        info = ps(
          `$ps = Get-Process variable -ErrorAction SilentlyContinue; ` +
            `if ($ps) { $h = ($ps | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1).MainWindowHandle; ` +
            `$m = ($ps | Measure-Object WorkingSet64 -Sum).Sum; '{0}|{1}' -f $h, $m }`
        );
      } catch {
        /* 进程尚在启动，轮询重试 */
      }
      const [hwnd, ws] = info.split('|');
      if (hwnd && hwnd !== '0' && coldStartMs === null) {
        coldStartMs = Date.now() - t0;
      }
      if (ws && Number(ws) > 0) memoryMb = Number(ws) / 1048576;
      // 主窗口出现后再采 3s 内存（让 WebView 完成首帧与挂载）即结束
      if (coldStartMs !== null && Date.now() - t0 > coldStartMs + 3000) break;
      await sleep(50);
    }
  } finally {
    // 优雅关闭自己拉起的进程：先关窗口，5s 后仍存活才强制结束（仅限本 pid 树）
    try {
      ps(`(Get-Process variable -ErrorAction SilentlyContinue) | Where-Object { $_.Id -eq ${child.pid} -or $_.Parent -eq ${child.pid} } | ForEach-Object { $_.CloseMainWindow() } | Out-Null`);
      await sleep(5000);
      ps(`(Get-Process variable -ErrorAction SilentlyContinue) | Where-Object { $_.Id -eq ${child.pid} -or $_.Parent -eq ${child.pid} } | Stop-Process -Force -ErrorAction SilentlyContinue`);
    } catch {
      /* 进程已自行退出 */
    }
  }

  if (coldStartMs === null) {
    return { coldStart: null, memory: memoryMb, skipped: '30s 内未观测到主窗口（应用可能启动失败或以非常规方式呈现）' };
  }
  return { coldStart: coldStartMs, memory: memoryMb };
}

// ---------- 2. 万文件索引 ----------
async function benchFileIndex() {
  const COUNT = 10000;
  const DIRS = 100;
  const rootDir = fs.mkdtempSync(path.join(os.tmpdir(), 'variable-bench-'));
  try {
    // 生成 100 目录 × 100 文件
    for (let d = 0; d < DIRS; d++) {
      const sub = path.join(rootDir, `d${String(d).padStart(3, '0')}`);
      fs.mkdirSync(sub);
      for (let f = 0; f < COUNT / DIRS; f++) {
        fs.writeFileSync(path.join(sub, `f${String(f).padStart(4, '0')}.txt`), `bench ${d}-${f}\n`);
      }
    }
    // 索引 = 并行感知的同步遍历 + 首块读取（模拟内容索引最小代价）
    const t0 = Date.now();
    let files = 0;
    const stack = [rootDir];
    while (stack.length) {
      const dir = stack.pop();
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const p = path.join(dir, entry.name);
        if (entry.isDirectory()) stack.push(p);
        else {
          const fd = fs.openSync(p, 'r');
          fs.readSync(fd, Buffer.alloc(256), 0, 256, 0);
          fs.closeSync(fd);
          files++;
        }
      }
    }
    const ms = Date.now() - t0;
    if (files !== COUNT) throw new Error(`索引文件数不符：${files} != ${COUNT}`);
    return { files, ms };
  } finally {
    fs.rmSync(rootDir, { recursive: true, force: true });
  }
}

// ---------- 报告 ----------
function loadPrevReport() {
  if (!fs.existsSync(BENCH_DIR)) return null;
  const files = fs
    .readdirSync(BENCH_DIR)
    .filter((f) => f.endsWith('.md') && !f.startsWith('README'))
    .sort();
  for (let i = files.length - 1; i >= 0; i--) {
    const text = fs.readFileSync(path.join(BENCH_DIR, files[i]), 'utf8');
    const cold = text.match(/\| coldStart \| (\d+)/);
    const idx = text.match(/\| fileIndex \| (\d+)/);
    const mem = text.match(/\| memory \| (\d+)/);
    if (cold || idx) {
      return {
        file: files[i],
        coldStart: cold ? Number(cold[1]) : null,
        fileIndex: idx ? Number(idx[1]) : null,
        memory: mem ? Number(mem[1]) : null,
      };
    }
  }
  return null;
}

function regressionCheck(current, prev) {
  const rows = [];
  let failed = false;
  const pairs = [
    ['coldStart', current.coldStart, prev.coldStart, 'ms', false],
    ['fileIndex', current.fileIndex, prev.fileIndex, 'ms', false],
    ['memory', current.memory, prev.memory, 'MB', false],
  ];
  for (const [name, now, before, unit] of pairs) {
    if (now == null || before == null) {
      rows.push(`| ${name} | ${fmt(now, unit)} | ${fmt(before, unit)} | —（缺基线或本轮跳过） |`);
      continue;
    }
    const delta = (now - before) / before;
    const bad = delta > REGRESSION_LIMIT;
    if (bad) failed = true;
    rows.push(`| ${name} | ${fmt(now, unit)} | ${fmt(before, unit)} | ${(delta >= 0 ? '+' : '')}${Math.round(delta * 1000) / 10}%${bad ? ' ⛔回归>' + REGRESSION_LIMIT * 100 + '%' : ''} |`);
  }
  return { rows, failed };
}

(async () => {
  const gui = await benchGui();
  const idx = await benchFileIndex();

  const current = {
    coldStart: gui.coldStart != null ? Math.round(gui.coldStart) : null,
    fileIndex: idx.ms,
    memory: gui.memory != null ? Math.round(gui.memory) : null,
  };

  const prev = loadPrevReport();

  if (checkOnly) {
    if (!prev) {
      console.error('bench --check：docs/bench/ 无历史基线，先跑一次 node tools/bench.cjs 生成。');
      process.exit(2);
    }
    const { rows, failed } = regressionCheck(current, prev);
    console.log('| 指标 | 本次 | 基线(' + prev.file + ') | 变化 |');
    console.log('| --- | --- | --- | --- |');
    rows.forEach((r) => console.log(r));
    process.exit(failed ? 1 : 0);
  }

  // 归档报告
  fs.mkdirSync(BENCH_DIR, { recursive: true });
  const now = new Date();
  const date = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
  const version = JSON.parse(fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8')).version;
  const file = path.join(BENCH_DIR, `${date}.md`);
  const env = `${os.type()} ${os.release()} · Node ${process.version} · CPU×${os.cpus().length} ${os.cpus()[0].model.trim()}`;
  const lines = [
    `# 性能基准报告 — ${date}`,
    '',
    `- 应用版本：${version}`,
    `- 环境：${env}`,
    `- 生成：node tools/bench.cjs（零网络、本地实测）`,
    '',
    '| 指标 | 数值 | 预算（BLUEPRINT 3.14） | 状态 |',
    '| --- | --- | --- | --- |',
    `| coldStart | ${fmt(current.coldStart, ' ms')} | ≤3000 ms | ${current.coldStart == null ? '⏭ 跳过' : current.coldStart <= 3000 ? '✅' : '⛔'} |`,
    `| fileIndex | ${fmt(current.fileIndex, ' ms')}（${idx.files} 文件） | <3000 ms | ${current.fileIndex <= 3000 ? '✅' : '⛔'} |`,
    `| vwmOpen | — | <50 ms | ⏭ SKIPPED（需 GUI 插桩，计划随 VWM 2.0 批接入 input→像素打点） |`,
    `| memory | ${fmt(current.memory, ' MB')} | ≤600 MB | ${current.memory == null ? '⏭ 跳过' : current.memory <= 600 ? '✅' : '⛔'} |`,
    '',
    '> 口径说明：memory 为全部 `variable` 进程 WorkingSet 之和（不含宿主共享的 msedgewebview2 渲染子进程，属已知边界）；coldStart 为窗口句柄首次出现时刻，非「可交互」时刻（启动仪式进度未纳入）。',
    '',
  ];
  if (gui.skipped) lines.push(`> 说明：${gui.skipped}`);
  if (prev) {
    const { rows } = regressionCheck(current, prev);
    lines.push('## 与上一基线对比', '', '| 指标 | 本次 | 上一基线 | 变化 |', '| --- | --- | --- | --- |', ...rows, '');
  }
  fs.writeFileSync(file, lines.join('\n') + '\n');
  console.log(lines.join('\n'));
  console.log(`报告已归档：docs/bench/${date}.md`);
})();
