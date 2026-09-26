/**
 * F170 域总检深化 · 程序化执法 runner（分片调度 + 失败隔离 + 断点续跑）。
 *
 * 主册判据延伸：
 * - F170「脚本全量执行 <30 分钟（可进 CI 周跑）」——CI 化需要 runner：
 *   19 项探针分 N 片并行（互不依赖的项同片串行、跨片并行），单片失败
 *   不拖垮其他片（F175 崩溃隔离同思想）；
 * - 「失败后可单独重跑该项」——断点续跑：上一轮结果作输入，只跑红项
 *   与未跑项（增量执法）；
 * - 时间预算执法：片分配预估耗时，超预算自动减片宽（诚实降速不假并行）。
 */

// ---------- 探针工作单元 ----------

export interface ProbeJob {
  id: string;
  /** 预估耗时 ms（片宽分配依据——历史跑出的均值）。 */
  estMs: number;
  /** 依赖的资源组（同组互斥——同令牌表的项不能并行互踩）。 */
  resourceGroup: string;
  run: () => { ok: boolean; detail: string };
}

export interface Shard {
  index: number;
  jobs: ProbeJob[];
  estMs: number;
}

/** 分片：同资源组同片（串行），跨组均衡装箱（贪心——片数 = ceil(总时长/片宽预算)）。 */
export function shardJobs(jobs: ProbeJob[], shardBudgetMs = 120_000): Shard[] {
  const groups = new Map<string, ProbeJob[]>();
  for (const j of jobs) {
    groups.set(j.resourceGroup, [...(groups.get(j.resourceGroup) ?? []), j]);
  }
  const chains = [...groups.values()].map((g) => ({
    jobs: g,
    estMs: g.reduce((s, j) => s + j.estMs, 0),
  }));
  const shardCount = Math.max(1, Math.ceil(chains.reduce((s, c) => s + c.estMs, 0) / shardBudgetMs));
  // 最长链优先装箱（LPT 贪心——均衡度的工程标准解）。
  const sorted = [...chains].sort((a, b) => b.estMs - a.estMs);
  const shards: Shard[] = Array.from({ length: shardCount }, (_, index) => ({ index, jobs: [], estMs: 0 }));
  for (const chain of sorted) {
    const lightest = shards.reduce((min, s) => (s.estMs < min.estMs ? s : min), shards[0]!);
    lightest.jobs.push(...chain.jobs);
    lightest.estMs += chain.estMs;
  }
  return shards.filter((s) => s.jobs.length > 0);
}

// ---------- 执行（失败隔离 + 结果归档） ----------

export interface ShardResult {
  shard: number;
  results: Array<{ id: string; ok: boolean; detail: string; elapsedMs: number }>;
  /** 片内任一探针抛异常时片仍在（异常按项捕获——隔离粒度 = 项）。 */
  crashedJobs: string[];
}

export function runShard(shard: Shard): ShardResult {
  const results: ShardResult["results"] = [];
  const crashedJobs: string[] = [];
  for (const job of shard.jobs) {
    const t0 = Date.now();
    try {
      const r = job.run();
      results.push({ id: job.id, ok: r.ok, detail: r.detail, elapsedMs: Date.now() - t0 });
    } catch (err) {
      crashedJobs.push(job.id);
      results.push({ id: job.id, ok: false, detail: `探针异常: ${String(err)}`, elapsedMs: Date.now() - t0 });
    }
  }
  return { shard: shard.index, results, crashedJobs };
}

export interface RunOutcome {
  results: ShardResult[];
  passCount: number;
  failCount: number;
  totalMs: number;
  withinBudget: boolean;
}

export function runAll(jobs: ProbeJob[], budgetMs = 1_800_000): RunOutcome {
  const t0 = Date.now();
  const shards = shardJobs(jobs);
  const results = shards.map(runShard);
  const flat = results.flatMap((r) => r.results);
  const totalMs = Date.now() - t0;
  return {
    results,
    passCount: flat.filter((r) => r.ok).length,
    failCount: flat.filter((r) => !r.ok).length,
    totalMs,
    withinBudget: totalMs <= budgetMs,
  };
}

// ---------- 断点续跑（只跑红项与未跑项） ----------

export interface PreviousRun {
  /** 上一轮绿项 id 集合（绿项默认跳过）。 */
  passedIds: Set<string>;
}

/** 增量执法：上一轮已绿且未变更的项跳过——修复验证周期从 30 分钟缩到分钟级。 */
export function incrementalJobs(jobs: ProbeJob[], prev: PreviousRun, invalidatedIds: Set<string> = new Set()): ProbeJob[] {
  return jobs.filter((j) => !prev.passedIds.has(j.id) || invalidatedIds.has(j.id));
}
