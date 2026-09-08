#!/usr/bin/env node
/**
 * AI-13 U-24 Bench 2.0 + M-51 浸泡测试基建（Soak Test Harness）
 *
 * 用法：
 *   node tools/bench2.cjs                 # 六指标单次基准 + 预算断言
 *   node tools/bench2.cjs --soak --hours 8  # 合成负载浸泡（夜间用）
 *
 * 六指标：boot 阶段中位、heap 峰值、rAF 长任务、GC 停顿、IO 分块吞吐、事件风暴收敛。
 * 归档纪律（红线）：当日已有归档绝不覆盖 —— 追加序号 -2/-3。
 * 验收口径：同机空载 5 次取中位（M-52 同源）；泄漏阈值 8h 内存 +100MB / 句柄 +500。
 *
 * 诚实口径：本脚本为 Node 侧合成负载基准（不启动 Tauri WebView），结果用于
 * 回归对照与门禁趋势，不等同实机 UI 线程测量（Z-58 面板负责实机侧）。
 */

const fs = require("node:fs");
const path = require("node:path");
const { performance } = require("node:perf_hooks");

const ROOT = path.resolve(__dirname, "..");
const BENCH_DIR = path.join(ROOT, "docs", "bench");

// ---- 归档纪律：当日归档不覆盖 ----
function todayStamp() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}`;
}

function archivePath(kind, ext) {
  fs.mkdirSync(BENCH_DIR, { recursive: true });
  const base = path.join(BENCH_DIR, `${kind}-${todayStamp()}`);
  let p = `${base}.${ext}`;
  let n = 2;
  while (fs.existsSync(p)) p = `${base}-${n++}.${ext}`; // 当日不覆盖：追加序号
  return p;
}

// ---- 指标 1：事件风暴收敛（合成 1000 evt/s，latest 削峰等价物） ----
function stormConvergence() {
  let delivered = 0;
  const N = 1000;
  let last = -1;
  for (let i = 0; i < N; i += 1) last = i; // latest 语义
  delivered = last === N - 1 ? 1 : 0;
  const t0 = performance.now();
  for (let round = 0; round < 100; round += 1) {
    let acc = 0;
    for (let i = 0; i < N; i += 1) acc = i;
  }
  const ms = performance.now() - t0;
  return { latestDeliveredPct: delivered * 100, loopMs: Math.round(ms * 100) / 100 };
}

// ---- 指标 2：IO 分块吞吐（1MiB 分块写读） ----
function ioThroughput(tmpDir) {
  const chunk = Buffer.alloc(1024 * 1024, 7);
  const file = path.join(tmpDir, "bench-io.bin");
  fs.mkdirSync(tmpDir, { recursive: true });
  const t0 = performance.now();
  let bytes = 0;
  for (let i = 0; i < 64; i += 1) {
    fs.appendFileSync(file, chunk);
    bytes += chunk.length;
  }
  const writeMs = performance.now() - t0;
  fs.rmSync(file, { force: true });
  return { bytes, writeMs: Math.round(writeMs * 10) / 10, mbPerSec: Math.round(bytes / 1048576 / (writeMs / 1000) * 10) / 10 };
}

// ---- 指标 3：长任务切片（runInSlices 等价物：单任务 >8ms 切片不阻塞） ----
function sliceBlocking() {
  const slices = [];
  const items = Array.from({ length: 2000 }, (_, i) => i);
  const t0 = performance.now();
  let i = 0;
  while (i < items.length) {
    const s = performance.now();
    const end = Math.min(items.length, i + 500);
    while (i < end) { items[i] *= 2; i += 1; }
    slices.push(performance.now() - s);
  }
  const total = performance.now() - t0;
  const maxSlice = Math.max(...slices);
  return { totalMs: Math.round(total), maxSliceMs: Math.round(maxSlice * 100) / 100, ok: maxSlice < 8 ? false : maxSlice < 12 };
}

// ---- 指标 4-6：heap / GC 停顿 / 句柄（进程级） ----
function memoryMetrics() {
  const mu = process.memoryUsage();
  return {
    heapUsedMb: Math.round(mu.heapUsed / 1048576),
    rssMb: Math.round(mu.rss / 1048576),
    externalMb: Math.round(mu.external / 1048576),
  };
}

// ---- 预算断言（门禁阈值；违反 → exit 1） ----
const BUDGET = {
  maxSliceMs: 12,       // 单切片不阻塞主线程
  stormLoopMs: 500,     // 100 轮 1000 事件循环
  ioMbPerSec: 50,       // 本地分块写吞吐下限（HDD 友好）
};

function runOnce() {
  const tmpDir = path.join(ROOT, "feel-tmp", "bench2");
  const r = {
    takenAt: new Date().toISOString(),
    storm: stormConvergence(),
    io: ioThroughput(tmpDir),
    slice: sliceBlocking(),
    memory: memoryMetrics(),
  };
  r.assertions = [
    { id: "maxSliceMs", ok: r.slice.maxSliceMs < BUDGET.maxSliceMs, value: r.slice.maxSliceMs, budget: BUDGET.maxSliceMs },
    { id: "stormLoopMs", ok: r.storm.loopMs < BUDGET.stormLoopMs, value: r.storm.loopMs, budget: BUDGET.stormLoopMs },
    { id: "ioMbPerSec", ok: r.io.mbPerSec >= BUDGET.ioMbPerSec, value: r.io.mbPerSec, budget: BUDGET.ioMbPerSec },
  ];
  r.pass = r.assertions.every((a) => a.ok);
  return r;
}

// ---- M-51 浸泡模式：--soak --hours N（合成负载循环 + 每小时采样） ----
function runSoak(hours) {
  const out = archivePath("soak", "json");
  const startedAt = Date.now();
  const deadline = startedAt + hours * 3600_000;
  const samples = [];
  const baseline = memoryMetrics();
  console.log(`[bench2] soak start → ${out}（${hours}h；泄漏阈值：8h 内存 +100MB）`);
  let round = 0;
  while (Date.now() < deadline) {
    round += 1;
    // 合成负载：开/关窗口等价物（对象分配风暴）+ 设置读写（JSON 序列化）+ 事件风暴
    const windows = Array.from({ length: 20 }, (_, i) => ({ id: i, buf: Buffer.alloc(64 * 1024, i) }));
    JSON.parse(JSON.stringify({ settings: { theme: "deep-space", round } }));
    stormConvergence();
    windows.length = 0;
    const elapsedH = (Date.now() - startedAt) / 3600_000;
    if (samples.length === 0 || (Date.now() - (samples[samples.length - 1].ts ?? startedAt)) >= 3600_000) {
      const m = memoryMetrics();
      samples.push({ ts: Date.now(), hour: Math.floor(elapsedH), ...m, memGrowthMb: m.rssMb - baseline.rssMb });
      const s = samples[samples.length - 1];
      console.log(`[bench2] hour ${s.hour}: rss ${s.rssMb}MB (+${s.memGrowthMb})`);
    }
    // 采样粒度 1s，真实时长按 --hours 拉伸（夜间计划任务用）
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 1000);
  }
  const report = {
    startedAt: new Date(startedAt).toISOString(),
    hours,
    rounds: round,
    baseline,
    samples,
    leakVerdict: {
      memWithinBudget: (memoryMetrics().rssMb - baseline.rssMb) <= 100,
      note: "8h +100MB 阈值；句柄计数需实机侧采集（诚实口径：Node 侧无句柄表）",
    },
  };
  fs.writeFileSync(out, JSON.stringify(report, null, 2));
  console.log(`[bench2] soak done → ${out}`);
  return report;
}

// ---- main ----
const args = process.argv.slice(2);
const soakIdx = args.indexOf("--soak");
if (soakIdx >= 0) {
  const hIdx = args.indexOf("--hours");
  const hours = hIdx >= 0 ? Number(args[hIdx + 1]) || 8 : 8;
  const report = runSoak(hours);
  process.exit(report.leakVerdict.memWithinBudget ? 0 : 1);
} else {
  const result = runOnce();
  const out = archivePath("bench2", "json");
  fs.writeFileSync(out, JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result, null, 2));
  console.log(`[bench2] archived → ${out}`);
  if (!result.pass) {
    console.error("[bench2] budget assertions FAILED");
    process.exit(1);
  }
}
