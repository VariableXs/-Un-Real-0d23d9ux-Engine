/**
 * 八章 性能感知 · E 域性能预算台账（P95/帧耗时/内存 + 回归对比 + 告警）。
 *
 * 主册判据延伸：
 * - 「响应 P95 无卡顿感：点击 100ms 内反馈」「动画稳定 60fps」——预算
 *   逐页登记（主册硬线），实测采样录入，超线三要素告警（F062 联动）；
 * - F061「回归门捕获率：注入 5 处回退全部检出」——同款口径：两轮采样
 *   对比，劣化 >10% 且超预算 = 回归红。
 */

export type PerfMetric = "interaction-p95-ms" | "frame-ms" | "heap-mb" | "startup-ms";

export interface PerfBudget {
  page: string;
  metric: PerfMetric;
  /** 预算硬线（主册判据数值——不许拍脑袋）。 */
  limit: number;
}

/** E 域预算表（主册数值一处一事实——新页接入必须在此登记，否则采样拒绝）。 */
export const PERF_BUDGETS: PerfBudget[] = [
  { page: "tokens", metric: "interaction-p95-ms", limit: 100 },
  { page: "preview", metric: "interaction-p95-ms", limit: 100 },
  { page: "wallpaper", metric: "frame-ms", limit: 12.5 },
  { page: "icons", metric: "interaction-p95-ms", limit: 2000 }, // 热更换 <2s 线。
  { page: "pointer", metric: "interaction-p95-ms", limit: 100 },
  { page: "sound", metric: "interaction-p95-ms", limit: 200 }, // 试听 <200ms 线。
  { page: "widgets", metric: "frame-ms", limit: 16.6 },
  { page: "lock", metric: "startup-ms", limit: 2000 }, // 唤醒到可见 ≤2s。
  { page: "ime", metric: "interaction-p95-ms", limit: 16 }, // 16ms 红线。
  { page: "ctxmenu", metric: "interaction-p95-ms", limit: 100 }, // 弹出预算。
];

export interface PerfSample {
  page: string;
  metric: PerfMetric;
  value: number;
  at: number;
}

export interface BudgetVerdict {
  page: string;
  metric: PerfMetric;
  limit: number;
  p95: number;
  within: boolean;
  /** 超线比例（诚实告警的分度：≤10% 黄、>10% 红）。 */
  overrunRatio: number | null;
}

/** 预算判定：未登记页 = 显性拒绝（预算先于采样——F061 同源纪律）。 */
export function verdictFor(samples: PerfSample[]): { verdicts: BudgetVerdict[]; unregistered: string[] } {
  const verdicts: BudgetVerdict[] = [];
  const unregistered: string[] = [];
  const byKey = new Map<string, PerfSample[]>();
  for (const s of samples) {
    const key = `${s.page}:${s.metric}`;
    if (!PERF_BUDGETS.some((b) => b.page === s.page && b.metric === s.metric)) {
      if (!unregistered.includes(key)) unregistered.push(key);
      continue;
    }
    byKey.set(key, [...(byKey.get(key) ?? []), s]);
  }
  for (const b of PERF_BUDGETS) {
    const list = byKey.get(`${b.page}:${b.metric}`);
    if (!list || list.length === 0) continue;
    const sorted = list.map((s) => s.value).sort((a, b2) => a - b2);
    const p95 = sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * 0.95) - 1)]!;
    const within = p95 <= b.limit;
    verdicts.push({
      page: b.page,
      metric: b.metric,
      limit: b.limit,
      p95: Math.round(p95 * 1000) / 1000,
      within,
      overrunRatio: within ? null : Math.round(((p95 - b.limit) / b.limit) * 1000) / 1000,
    });
  }
  return { verdicts, unregistered };
}

/** 三要素告警（超预算 → 人话三件套——十三章补口径）。 */
export function budgetAlert(v: BudgetVerdict): { tone: "warn" | "danger"; what: string; why: string; next: string } {
  const danger = (v.overrunRatio ?? 0) > 0.1;
  return {
    tone: danger ? "danger" : "warn",
    what: `${v.page} 的 ${v.metric} 实测 P95=${v.p95}，超预算 ${v.limit}（${Math.round((v.overrunRatio ?? 0) * 100)}%）`,
    why: danger ? "超线 10% 以上——多为新增逻辑在热路径上（回归嫌疑最高）" : "轻微超线——可能在预算边缘的正常抖动",
    next: danger ? "按 F061 口径跑回归对比定位新增开销，超线项回炉" : "连续三轮复测，稳定超线再立案",
  };
}

/** 回归对比（两轮采样——劣化 >10% 且超预算 = 红检出）。 */
export function regressionCheck(prev: PerfSample[], curr: PerfSample[]): Array<{ page: string; metric: PerfMetric; prevP95: number; currP95: number; regressed: boolean }> {
  const p95Of = (list: PerfSample[]): Map<string, number> => {
    const m = new Map<string, PerfSample[]>();
    for (const s of list) m.set(`${s.page}:${s.metric}`, [...(m.get(`${s.page}:${s.metric}`) ?? []), s]);
    const out = new Map<string, number>();
    for (const [k, arr] of m) {
      const sorted = arr.map((s) => s.value).sort((a, b) => a - b);
      out.set(k, sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * 0.95) - 1)]!);
    }
    return out;
  };
  const p = p95Of(prev);
  const c = p95Of(curr);
  const out: Array<{ page: string; metric: PerfMetric; prevP95: number; currP95: number; regressed: boolean }> = [];
  for (const [key, currP95] of c) {
    const prevP95 = p.get(key);
    if (prevP95 === undefined) continue;
    const [page, metric] = key.split(":") as [string, PerfMetric];
    const budget = PERF_BUDGETS.find((b) => b.page === page && b.metric === metric);
    const regressed = currP95 > prevP95 * 1.1 && (!budget || currP95 > budget.limit);
    out.push({ page, metric, prevP95: Math.round(prevP95 * 1000) / 1000, currP95: Math.round(currP95 * 1000) / 1000, regressed });
  }
  return out;
}
