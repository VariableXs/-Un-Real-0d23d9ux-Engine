/**
 * AI-13 U-21 渲染帧预算器（Frame Budget Governor）
 *
 * 三档策略（与 perfMode/bgTier 既有档位语义对齐）：
 * - full（16.6ms 预算）：全部动效
 * - half（33ms 预算）：减半粒子/暂停次要动效
 * - floor（50ms 预算）：只保留必要动效，长任务全部 idle 切片
 *
 * 动画成本账本：登记每站点单帧均耗，供降档决策与归因。
 */

export type FrameTier = "full" | "half" | "floor";

export const TIER_BUDGET_MS: Record<FrameTier, number> = { full: 16.6, half: 33, floor: 50 };

export interface CostRecord {
  site: string;
  frames: number;
  totalMs: number;
}

const ledger = new Map<string, CostRecord>();
let tier: FrameTier = "full";

export function setFrameTier(next: FrameTier): void {
  tier = next;
}

export function getFrameTier(): FrameTier {
  return tier;
}

export function frameBudgetMs(): number {
  return TIER_BUDGET_MS[tier];
}

/** 站点帧计时：包裹一帧的实际工作耗时并记账。 */
export function chargeFrame(site: string, costMs: number): void {
  let rec = ledger.get(site);
  if (!rec) {
    rec = { site, frames: 0, totalMs: 0 };
    ledger.set(site, rec);
  }
  rec.frames += 1;
  rec.totalMs += costMs;
}

export function avgCost(site: string): number {
  const rec = ledger.get(site);
  if (!rec || rec.frames === 0) return 0;
  return rec.totalMs / rec.frames;
}

/** 站点是否超预算（连续超帧数阈值 → 降档建议信号）。 */
export function overBudget(site: string, framesWindow = 30): boolean {
  const rec = ledger.get(site);
  if (!rec || rec.frames < framesWindow) return false;
  const recent = rec.totalMs / rec.frames;
  return recent > frameBudgetMs();
}

/** 长任务 idle 切片：把 >8ms 的工作切到 requestIdleCallback（floor 档强制启用）。 */
export function runInSlices<T>(
  items: T[],
  worker: (item: T) => void,
  opts?: { sliceMs?: number },
): Promise<void> {
  const sliceMs = opts?.sliceMs ?? 8;
  return new Promise((resolve) => {
    let i = 0;
    const step = (deadline?: IdleDeadline): void => {
      const hasBudget = deadline ? deadline.timeRemaining() > 1 : true;
      if (!hasBudget) {
        scheduleNext();
        return;
      }
      const t0 = performance.now();
      while (i < items.length && performance.now() - t0 < sliceMs) {
        worker(items[i] as T);
        i += 1;
      }
      if (i < items.length) scheduleNext();
      else resolve();
    };
    const scheduleNext = (): void => {
      if (typeof requestIdleCallback === "function") {
        requestIdleCallback((d) => step(d));
      } else {
        setTimeout(() => step(undefined), 8);
      }
    };
    scheduleNext();
  });
}

/** 三档决策（Z-60 内存压力联动降档映射的帧侧）：tier 单调只降，恢复由调用方显式升档。 */
export function downgradeTier(cur: FrameTier): FrameTier {
  return cur === "full" ? "half" : "floor";
}

export function ledgerSnapshot(): CostRecord[] {
  return [...ledger.values()];
}
