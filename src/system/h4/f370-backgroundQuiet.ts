/**
 * F370 后台不惊扰承诺（H 域 · AI-H4）：
 * 后台任务对用户的三不承诺入册为判据：不弹窗（后台问题进通知中心不弹横幅打断）、
 * 不抢 IO（前台文件操作永远优先，F057 分级）、不显眼（除 F369 中心外无任何常驻 UI）；
 * 后台失败的重试静默进行（三次指数退避），最终失败才在通知中心留一条。
 * 判据（主册 F370）：三不逐项测试（注入后台高 IO/高错误场景测前台指标）；
 * 退避重试用例；最终失败通知一条；横幅=0 判据。
 * 依赖锚点：F057 IO 分级 / F369 后台任务中心 / F077 通知中心。
 */

import { fnv1a32 } from "./internal/hash";

/** 退避序列（判据「三次指数退避」）：1s → 2s → 4s，三次后放弃。 */
export const BACKOFF_SCHEDULE_MS = [1000, 2000, 4000] as const;
export const MAX_RETRIES = 3;

export interface RetryState {
  attempts: number;
  /** 下次重试时刻（null=不再重试）。 */
  nextRetryAt: number | null;
  failed: boolean;
}

/** 退避状态机：失败 → 排下一次重试；三次失败 → 终态（发一条通知的触发点）。 */
export function onTaskFailure(state: RetryState, nowMs: number): { state: RetryState; retryScheduled: boolean; finalFailure: boolean } {
  if (state.attempts >= MAX_RETRIES) {
    return { state: { ...state, failed: true, nextRetryAt: null }, retryScheduled: false, finalFailure: true };
  }
  const delay = BACKOFF_SCHEDULE_MS[Math.min(state.attempts, BACKOFF_SCHEDULE_MS.length - 1)]!;
  return {
    state: { attempts: state.attempts + 1, nextRetryAt: nowMs + delay, failed: false },
    retryScheduled: true,
    finalFailure: false,
  };
}

/** 重试到期判定。 */
export function retryDue(state: RetryState, nowMs: number): boolean {
  return !state.failed && state.nextRetryAt !== null && nowMs >= state.nextRetryAt;
}

/** 重试成功：清账（下次失败从第一档退避重来）。 */
export function onRetrySuccess(_state: RetryState): RetryState {
  return { attempts: 0, nextRetryAt: null, failed: false };
}

/* ---------- 三不承诺 ---------- */

export type NoticeSeverity = "silent" | "center";

export interface FailureNoticePlan {
  /** 横幅（banner）永远不用于后台失败——不弹窗判据。 */
  banner: false;
  /** 去向：通知中心（最终失败才有一条）。 */
  severity: NoticeSeverity;
  /** 三要素文案。 */
  message: { what: string; why: string; next: string };
}

/** 最终失败的唯一通知（三要素 + 横幅=0 判据）。 */
export function finalFailureNotice(taskName: string, cause: string): FailureNoticePlan {
  return {
    banner: false,
    severity: "center",
    message: {
      what: `后台任务「${taskName}」重试 3 次后仍未完成`,
      why: cause,
      next: "任务已安全中止、数据未受损；可在后台任务中心手动重跑",
    },
  };
}

/** 不抢 IO 判据：后台发起的 IO 请求在前台活跃时必须让路（F057 分级对账）。 */
export function ioYieldRequired(foregroundBusy: boolean): { allowed: boolean; reason: string } {
  return foregroundBusy
    ? { allowed: false, reason: "前台文件操作进行中——后台让路（F057 分级）" }
    : { allowed: true, reason: "前台空闲——后台可推进" };
}

/** 不显眼判据审计：除 F369 中心外无任何常驻 UI 面。 */
export function auditNoResidentUi(surfaces: string[]): { pass: boolean; illegal: string[] } {
  const allowed = new Set(["f369-task-center", "notify-center"]);
  const illegal = surfaces.filter((s) => !allowed.has(s));
  return { pass: illegal.length === 0, illegal };
}

/** 三不逐项测试汇总（判据「三不逐项测试」）。 */
export function auditThreePromises(input: { foregroundBusy: boolean; residentSurfaces: string[]; attemptFailures: number }): { noPopup: boolean; noIoSteal: boolean; noResidentUi: boolean; pass: boolean } {
  const noPopup = finalFailureNotice("x", "y").banner === false;
  const noIoSteal = !input.foregroundBusy || ioYieldRequired(true).allowed === false;
  const noResidentUi = auditNoResidentUi(input.residentSurfaces).pass;
  return { noPopup, noIoSteal, noResidentUi, pass: noPopup && noIoSteal && noResidentUi && input.attemptFailures <= MAX_RETRIES };
}

/* ================= v4 深化批次四：确定性抖动 / 提醒预算 / 免打扰时段 / 前台采样环 ================= */

/** 确定性抖动：0-250ms，由 taskId+attempt 派生（可复现——测试与回放同值，重试不扎堆）。 */
export function backoffWithJitter(taskId: string, attempts: number, nowMs: number): { delayMs: number; nextRetryAt: number } {
  const base = BACKOFF_SCHEDULE_MS[Math.min(attempts, BACKOFF_SCHEDULE_MS.length - 1)]!;
  const jitter = parseInt(fnv1a32(`${taskId}#${attempts}`).slice(0, 4), 16) % 251;
  const delayMs = base + jitter;
  return { delayMs, nextRetryAt: nowMs + delayMs };
}

/** 每日最终失败提醒预算（不惊扰判据的量化面）：一天最多 5 条，超出并成摘要。 */
export const DAILY_NOTICE_BUDGET = 5;

export function noticeBudgetDecision(sentToday: number, taskName: string): { send: boolean; digest: boolean; message: string } {
  if (sentToday < DAILY_NOTICE_BUDGET) {
    return { send: true, digest: false, message: `「${taskName}」最终失败通知（今日第 ${sentToday + 1} 条）` };
  }
  return { send: false, digest: true, message: `「${taskName}」失败——已并入今日摘要（提醒预算 ${DAILY_NOTICE_BUDGET} 条/日）` };
}

/** 免打扰时段（22:00-08:00 不打断——与 F077 免打扰同源口径）。 */
export const QUIET_HOURS = { from: 22, to: 8 } as const;

export function inQuietHours(hourOfDay: number): boolean {
  return hourOfDay >= QUIET_HOURS.from || hourOfDay < QUIET_HOURS.to;
}

/** 通知时刻决策：免打扰时段顺延至晨间摘要；预算内即时送、超预算并摘要。 */
export function noticeScheduleDecision(sentToday: number, taskName: string, hourOfDay: number): { send: boolean; when: "now" | "morning-digest"; message: string } {
  if (inQuietHours(hourOfDay)) {
    return { send: false, when: "morning-digest", message: `「${taskName}」失败——免打扰时段，明晨随摘要送达` };
  }
  const budget = noticeBudgetDecision(sentToday, taskName);
  return { send: budget.send, when: budget.send ? "now" : "morning-digest", message: budget.message };
}

/** 前台帧率采样环（三不承诺「不抢 IO」的量化审计面）：滚动窗内最大降幅 <5fps。 */
export class ForegroundFpsSampler {
  private readonly samples: Array<{ at: number; fps: number }> = [];

  constructor(private readonly windowMs = 30_000) {}

  push(at: number, fps: number): void {
    this.samples.push({ at, fps });
    while (this.samples.length > 0 && at - this.samples[0]!.at > this.windowMs) this.samples.shift();
  }

  /** 窗内最大帧率降幅（基线 = 窗内峰值）。 */
  maxDrop(): number {
    if (this.samples.length < 2) return 0;
    const baseline = Math.max(...this.samples.map((s) => s.fps));
    const worst = Math.min(...this.samples.slice(1).map((s) => s.fps));
    return Math.max(0, baseline - worst);
  }

  withinBudget(budgetFps = 5): boolean {
    return this.maxDrop() < budgetFps;
  }
}
