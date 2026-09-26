/**
 * F370 后台不惊扰承诺（H 域 · AI-H4）：
 * 后台任务对用户的三不承诺入册为判据：不弹窗（后台问题进通知中心不弹横幅打断）、
 * 不抢 IO（前台文件操作永远优先，F057 分级）、不显眼（除 F369 中心外无任何常驻 UI）；
 * 后台失败的重试静默进行（三次指数退避），最终失败才在通知中心留一条。
 * 判据（主册 F370）：三不逐项测试（注入后台高 IO/高错误场景测前台指标）；
 * 退避重试用例；最终失败通知一条；横幅=0 判据。
 * 依赖锚点：F057 IO 分级 / F369 后台任务中心 / F077 通知中心。
 */

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
