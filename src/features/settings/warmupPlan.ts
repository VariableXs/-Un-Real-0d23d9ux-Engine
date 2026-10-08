/**
 * UNREAL-X AI-02 · 族0018 预热编排（X00401 档 · 唤醒后按计划的预热任务队列）。
 *
 * tick 驱动的任务队列：任务带优先级与预算，唤醒后按优先级逐 tick 执行；
 * 低电量守护（暂停队列）、取消、钳制。纯逻辑模块。
 */

export const WARMUP_TASK_TYPES = ["network", "notify-sync", "app-prewarm", "index-scan", "layout-restore"] as const;
export type WarmupTaskType = (typeof WARMUP_TASK_TYPES)[number];

export function isWarmupTaskType(t: string): t is WarmupTaskType {
  return (WARMUP_TASK_TYPES as readonly string[]).includes(t);
}

export interface WarmupTask {
  id: string;
  type: WarmupTaskType;
  /** 优先级 1（最优先）~9。 */
  priority: number;
  /** 预算 ms（0~5000，钳制）。 */
  budgetMs: number;
}

export type WarmupPhase = "idle" | "running" | "held" | "done";

export const WARMUP_BUDGET_MAX = 5000;
export const WARMUP_QUEUE_MAX = 16;

/** 预热编排器：唤醒后逐 tick 执行队列。 */
export class WarmupPlanner {
  queue: WarmupTask[] = [];
  phase: WarmupPhase = "idle";
  doneIds: string[] = [];
  clamped = 0;
  /** 低电量守护：true 时 tick 挂起队列。 */
  lowBattery: boolean;

  constructor(lowBattery = false) {
    this.lowBattery = lowBattery;
  }

  /** 入队（类型白名单、优先级钳 1~9、预算钳 0~5000、队列上限 16）。 */
  enqueue(task: WarmupTask): boolean {
    if (!isWarmupTaskType(task.type)) {
      this.clamped += 1;
      return false;
    }
    if (this.queue.length >= WARMUP_QUEUE_MAX) {
      this.clamped += 1;
      return false;
    }
    const t: WarmupTask = {
      id: String(task.id ?? "").slice(0, 48),
      type: task.type,
      priority: Math.min(9, Math.max(1, Math.round(task.priority) || 5)),
      budgetMs: Math.min(WARMUP_BUDGET_MAX, Math.max(0, Math.round(task.budgetMs) || 0)),
    };
    this.queue.push(t);
    this.queue.sort((a, b) => a.priority - b.priority);
    return true;
  }

  /** 唤醒后启动队列。 */
  begin(): void {
    this.phase = this.queue.length === 0 ? "done" : "running";
  }

  /** 推进一 tick：执行当前最高优先级任务（低电量则挂起）。 */
  tick(): WarmupPhase {
    if (this.phase !== "running") return this.phase;
    if (this.lowBattery) {
      this.phase = "held";
      return this.phase;
    }
    const next = this.queue.shift();
    if (!next) {
      this.phase = "done";
      return this.phase;
    }
    this.doneIds.push(next.id);
    if (this.queue.length === 0) this.phase = "done";
    return this.phase;
  }

  /** 恢复被挂起的队列（电量回升）。 */
  release(): boolean {
    if (this.phase !== "held") return false;
    this.phase = this.queue.length === 0 ? "done" : "running";
    return true;
  }

  /** 按 id 取消任务。 */
  cancel(id: string): boolean {
    const i = this.queue.findIndex((t) => t.id === id);
    if (i < 0) return false;
    this.queue.splice(i, 1);
    if (this.queue.length === 0 && this.phase === "running") this.phase = "done";
    return true;
  }

  /** 预算合计（队列未执行部分）。 */
  pendingBudgetMs(): number {
    return this.queue.reduce((a, t) => a + t.budgetMs, 0);
  }

  /** 净身。 */
  reset(): void {
    this.queue = [];
    this.doneIds = [];
    this.phase = "idle";
    this.clamped = 0;
  }
}
