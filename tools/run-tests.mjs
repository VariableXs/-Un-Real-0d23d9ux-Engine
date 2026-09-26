#!/usr/bin/env node
/**
 * 大测试总闸 run-tests（Variable 明令待办收编 · 2026-09-26）。
 *
 * 三模式：
 *   node tools/run-tests.mjs fast    —— 宿主全量日常跑（分钟级：vitest + tsc 增量）
 *   node tools/run-tests.mjs full    —— 自检 + QEMU 批队列闸门跑（人工/无人值守可过夜）
 *   node tools/run-tests.mjs single <脚本名> —— 单脚本直跑
 *
 * 收编纪律：旧脚本**原样收编不重写**——本闸只做调度/并行/台账，不改脚本本体。
 * 台账：SCRIPT_MAP 记录「脚本 → 归属分队 → 模式 → 超时」，新增脚本登记即入闸。
 */
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";

const ROOT = process.cwd();

// ---------- 脚本台账（映射表——新增脚本登记即入闸，散装脚本原样收编） ----------

/** @type {Array<{ name: string; cmd: string; args: string[]; owner: string; mode: "fast"|"full"|"both"; timeoutMs: number; note: string }>} */
const SCRIPT_MAP = [
  { name: "vitest-e", cmd: "npx", args: ["vitest", "run", "src/system/persona"], owner: "AI-E1", mode: "fast", timeoutMs: 300_000, note: "E 域 405 用例（隔离验证主口径）" },
  { name: "tsc-incremental-false", cmd: "npx", args: ["tsc", "--noEmit", "--incremental", "false"], owner: "全树", mode: "fast", timeoutMs: 600_000, note: "严格模式全新口径（防陈旧缓存假阴性）" },
  { name: "walkcheck-e", cmd: "python", args: ["tools/vx-walkcheck-e.py", "--selftest"], owner: "AI-E1", mode: "both", timeoutMs: 120_000, note: "域总检 19 项 checklist 自检" },
  { name: "tally-e", cmd: "node", args: ["_attic/aie1-f151-f170/tally.mjs", "."], owner: "AI-E1", mode: "both", timeoutMs: 60_000, note: "检查项对账（二十桶非零判定）" },
];

// ---------- 执行器（并行分片 + 超时 + 结果归档） ----------

function runOne(job) {
  return new Promise((resolve) => {
    const t0 = Date.now();
    const child = spawn(job.cmd, job.args, { cwd: ROOT, shell: process.platform === "win32", stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill();
      resolve({ name: job.name, ok: false, elapsedMs: Date.now() - t0, stdout, stderr: `${stderr}\n超时 ${job.timeoutMs}ms`, timedOut: true });
    }, job.timeoutMs);
    child.stdout.on("data", (d) => { stdout += d; });
    child.stderr.on("data", (d) => { stderr += d; });
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({ name: job.name, ok: code === 0, elapsedMs: Date.now() - t0, stdout, stderr, timedOut: false });
    });
  });
}

/** 并行分片（CPU 核数上限——LPT 同思路：长任务先发）。 */
async function runAll(jobs, concurrency = Math.max(2, Math.min(4, (process.pid % 4) + 2))) {
  const results = [];
  const queue = [...jobs];
  const workers = Array.from({ length: Math.min(concurrency, queue.length) }, async () => {
    while (queue.length > 0) {
      const job = queue.shift();
      process.stdout.write(`▶ ${job.name} ... `);
      const r = await runOne(job);
      console.log(r.ok ? `PASS (${(r.elapsedMs / 1000).toFixed(1)}s)` : `FAIL (${(r.elapsedMs / 1000).toFixed(1)}s)`);
      results.push(r);
    }
  });
  await Promise.all(workers);
  return results;
}

// ---------- 模式 ----------

function pickJobs(mode, singleName) {
  if (singleName) {
    const job = SCRIPT_MAP.find((j) => j.name === singleName);
    if (!job) {
      console.error(`未登记脚本: ${singleName}——台账脚本: ${SCRIPT_MAP.map((j) => j.name).join(", ")}`);
      process.exit(2);
    }
    return [job];
  }
  const pool = SCRIPT_MAP.filter((j) => mode === "full" || j.mode === "fast" || j.mode === "both");
  return pool;
}

async function main() {
  const mode = process.argv[2] ?? "fast";
  const singleName = process.argv[3];
  if (!["fast", "full", "single"].includes(mode)) {
    console.error("用法: node tools/run-tests.mjs fast|full|single [脚本名]");
    process.exit(2);
  }
  console.log(`═══ 大测试总闸 · ${mode} 模式 · 收编脚本 ${SCRIPT_MAP.length} 个 ═══`);
  if (mode === "full") {
    console.log("full 模式：fast 全量 + full-only 脚本（QEMU 批队列可过夜无人值守）。");
  }
  const jobs = pickJobs(mode, singleName).filter((j) => {
    const missing = !existsSync(join(ROOT, j.args[0] ?? "")) && !j.cmd.startsWith("npx") && !j.cmd.startsWith("python") && !j.cmd.startsWith("node");
    if (missing) console.warn(`⚠ 跳过（文件缺失）: ${j.name}`);
    return !missing;
  });
  const results = await runAll(jobs);
  const pass = results.filter((r) => r.ok);
  const fail = results.filter((r) => !r.ok);
  console.log("═══ 总闸结果 ═══");
  console.log(`PASS ${pass.length} / FAIL ${fail.length} / 共 ${results.length}`);
  for (const f of fail) {
    console.log(`── FAIL ${f.name}: ${f.timedOut ? "超时" : "非零退出"}${f.stderr ? `\n${f.stderr.slice(0, 800)}` : ""}`);
  }
  process.exit(fail.length === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error("总闸自身异常（显性化——不静默）:", e);
  process.exit(1);
});
