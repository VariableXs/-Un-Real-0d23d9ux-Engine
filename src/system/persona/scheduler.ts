/**
 * F153/F163 调度深化 · 多源统一时间轴（公平份额 + 错过策略 + 优先级）。
 *
 * 主册判据延伸：
 * - F153「定时切换三连测准时（±5s）」、F163「更新调度表（15s 抖动）」——
 *   二十页各有定时面，统一成一个调度模型避免每页自搞一套（十章一致性）；
 * - 「后台任务零饥饿」（F057 IO 分级同思想）：多任务竞争时按公平份额
 *   轮转，不按注册顺序饿死后者；
 * - 错过策略显性化：休眠唤醒后错过的任务三选一（skip/catchup/once），
 *   不静默堆积也不静默丢弃。
 */

// ---------- 任务登记 ----------

export type MissedPolicy = "skip" | "catchup" | "once";

export interface ScheduledTask {
  id: string;
  /** 基准间隔 ms。 */
  intervalMs: number;
  /** 优先级（同刻竞争时高者先跑——交互面任务优先）。 */
  priority: number;
  missed: MissedPolicy;
  /** 上次执行时刻（null = 从未跑过）。 */
  lastRunAt: number | null;
  enabled: boolean;
}

export const MISSED_POLICY_NOTE: Record<MissedPolicy, string> = {
  skip: "错过即跳过——等下一个周期（壁纸轮换类：过期的新图没有意义）",
  catchup: "错过全部补跑（计费/备份类：一次都不能少）",
  once: "错过合并为一次（数据刷新类：补最新一次即可）",
};

// ---------- 就绪判定与公平选取 ----------

export interface DueVerdict {
  task: ScheduledTask;
  /** 距应跑时刻（正=已迟到 ms，负=还有多久）。 */
  overdueMs: number;
}

export function dueTasks(tasks: ScheduledTask[], now: number): DueVerdict[] {
  const out: DueVerdict[] = [];
  for (const t of tasks) {
    if (!t.enabled) continue;
    const anchor = t.lastRunAt ?? now - t.intervalMs; // 从未跑过 = 立即就绪。
    const elapsed = now - anchor;
    if (elapsed >= t.intervalMs) {
      out.push({ task: t, overdueMs: elapsed - t.intervalMs });
    }
  }
  return out;
}

/** 错过次数精确版（once/catchup 的执行次数依据；lastRunAt=0 是合法时刻不是"从未"）。 */
export function missedCount(t: ScheduledTask, now: number): number {
  if (t.lastRunAt === null) return 1; // 从未跑过 = 待跑一次。
  const elapsed = now - t.lastRunAt;
  return Math.max(1, Math.floor(elapsed / t.intervalMs));
}

/**
 * 公平选取：同刻多个任务就绪时——
 * 1) 优先级降序；2) 同级按 overdue 降序（最饿的先吃）；
 * 3) 返回本轮应执行清单（全部——公平指顺序，不是裁剪）。
 */
export function fairOrder(due: DueVerdict[]): DueVerdict[] {
  return [...due].sort((a, b) => b.task.priority - a.task.priority || b.overdueMs - a.overdueMs);
}

/** 执行计划（once/catchup/skip 三策略的执行次数裁决）。 */
export function executionPlan(due: DueVerdict[], now: number): Array<{ id: string; runs: number; note: string }> {
  return fairOrder(due).map(({ task, overdueMs }) => {
    if (overdueMs < task.intervalMs) {
      return { id: task.id, runs: 1, note: "准周期执行" };
    }
    const missed = missedCount(task, now);
    switch (task.missed) {
      case "skip":
        return { id: task.id, runs: 1, note: `${missed} 个周期已错过——按 skip 策略补跑 1 次最新值` };
      case "catchup":
        return { id: task.id, runs: Math.min(8, missed), note: `错过 ${missed} 次——按 catchup 补跑 ${Math.min(8, missed)} 次（封顶 8 防风暴）` };
      case "once":
        return { id: task.id, runs: 1, note: `错过 ${missed} 次——按 once 合并为 1 次` };
    }
  });
}

// ---------- 下次时刻（抖动友好 + 唤醒重算口径） ----------

/** 下次应跑时刻：anchor + k×interval 里第一个严格大于 now 的（k = floor(elapsed/interval)+1）。 */
export function nextRunAt(t: ScheduledTask, now: number): number {
  const anchor = t.lastRunAt === null ? now : t.lastRunAt;
  const k = Math.floor((now - anchor) / t.intervalMs) + 1;
  return anchor + Math.max(1, k) * t.intervalMs;
}

/** 唤醒重算：休眠跨过的任务按各自 missed 策略给出唤醒后第一步（十三章补：唤醒恢复路径显性化）。 */
export function wakeRecoveryPlan(tasks: ScheduledTask[], sleptFrom: number, wokeAt: number): Array<{ id: string; action: string }> {
  return tasks.filter((t) => t.enabled).map((t) => {
    const anchor = t.lastRunAt ?? sleptFrom;
    if (anchor >= wokeAt - t.intervalMs) return { id: t.id, action: "睡眠期无错过——照常等下一周期" };
    const missed = missedCount(t, wokeAt);
    return { id: t.id, action: `睡眠期错过 ${missed} 个周期 → ${MISSED_POLICY_NOTE[t.missed]}` };
  });
}
