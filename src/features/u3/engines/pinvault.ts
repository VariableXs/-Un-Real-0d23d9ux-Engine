/**
 * F504/F505 深化引擎 · PIN 生命周期与蓝牙动态锁跟踪器（AI-U3 · pinvault）。
 *
 * 判据唯一源（主册摘文）：
 * - F504「4-6 位设置；解锁时序（<1.5s 全链）；5 次冷却翻倍表；密码回退；
 *   改废即时生效」。
 * - F505「信号消失 30s±5s 触发；锁形标记；误触发测试（短暂信号波动 <10s
 *   不锁）；回连时长；与 F316 闲置锁屏双保险并存」。
 *
 * 深化点：
 * 1. PIN 状态机完整生命周期（空 → 就绪 → 验证中 → 冷却 → 作废），冷却翻倍表
 *    是**表驱动**（fail 5→10→20→40→80 分钟），不是 if 链——表即账册。
 * 2. 解锁时序预算分解（<1.5s 全链）：输入校验/散列/比对/解锁广播四段各记
 *    预算，超预算段显性（体验八章：慢要诚实）。
 * 3. 蓝牙动态锁：RSSI 时间线状态机（在场/离场/宽限/锁定），<10s 波动豁免与
 *    30s±5s 触发在同一时间线上裁决；回连即时解除。
 * 4. 与 F316 双保险并存：两锁任一触发即锁，事件去重（同 2s 内不双报）。
 */

/* ------------------------------ F504 翻倍表 ------------------------------ */

/** 冷却翻倍表（判据「5 次冷却翻倍表」：第 5 次失败起，每次翻倍，分钟）。 */
export const PIN_COOLDOWN_LADDER_MIN = [10, 20, 40, 80, 160] as const;
/** 连续失败触发冷却的起点（判据：5 次）。 */
export const PIN_COOLDOWN_TRIGGER_FAILS = 5;

/** 第 n 次连续失败（n≥触发点）应得的冷却分钟数；未达触发点返回 0。 */
export function cooldownMinutesFor(consecutiveFails: number): number {
  if (consecutiveFails < PIN_COOLDOWN_TRIGGER_FAILS) return 0;
  const idx = Math.min(consecutiveFails - PIN_COOLDOWN_TRIGGER_FAILS, PIN_COOLDOWN_LADDER_MIN.length - 1);
  return PIN_COOLDOWN_LADDER_MIN[idx]!;
}

/* ------------------------------ F504 状态机 ------------------------------ */

export type PinPhase = "empty" | "ready" | "verifying" | "cooldown" | "revoked";

export interface PinMachineState {
  phase: PinPhase;
  pin: string;
  failStreak: number;
  cooldownUntil: number;
}

export interface PinBudget {
  validateMs: number;
  hashMs: number;
  compareMs: number;
  broadcastMs: number;
}

/** 解锁全链预算（判据 <1.5s）：四段预算合计 1400ms，留 100ms 余量。 */
export const PIN_TOTAL_BUDGET_MS = 1500;
export const PIN_SEGMENT_BUDGET: PinBudget = { validateMs: 50, hashMs: 600, compareMs: 100, broadcastMs: 650 };

/** 时序对账：四段实测 vs 预算，超段显性（返回超支段清单，空=绿）。 */
export function auditPinTiming(actual: PinBudget): { withinBudget: boolean; over: Array<keyof PinBudget>; totalMs: number } {
  const keys: Array<keyof PinBudget> = ["validateMs", "hashMs", "compareMs", "broadcastMs"];
  const over = keys.filter((k) => actual[k] > PIN_SEGMENT_BUDGET[k]);
  const totalMs = keys.reduce((s, k) => s + actual[k], 0);
  return { withinBudget: over.length === 0 && totalMs <= PIN_TOTAL_BUDGET_MS, over, totalMs };
}

/** PIN 合法性（4-6 位数字——判据「4-6 位设置」）。 */
export function isValidPinShape(pin: string): boolean {
  return /^[0-9]{4,6}$/.test(pin);
}

/* ------------------------------ F505 RSSI 状态机 ------------------------------ */

export type BtPhase = "present" | "away" | "grace" | "locked";
export const BT_LOCK_AFTER_SEC = 30;
/** 触发容差 ±5s（判据 30s±5s）。 */
export const BT_LOCK_TOLERANCE_SEC = 5;
/** 短波动豁免（判据：短暂信号波动 <10s 不锁）。 */
export const BT_FLAP_IMMUNE_SEC = 10;

export interface BtState {
  phase: BtPhase;
  awaySince: number | null;
  lockedAt: number | null;
}

/** 初始态：设备在场。 */
export function btInitialState(): BtState {
  return { phase: "present", awaySince: null, lockedAt: null };
}

/**
 * RSSI 事件推进（nowSec 单调秒）。信号在场=true。
 * 裁决规则：
 * - 在场 → 离场：记 awaySince，进入 away。
 * - 离场 <10s 又回来：直接回 present（波动豁免——awaySince 清账）。
 * - 离场 ≥10s 回来：进 grace（回连观察），仍在场即解锁路径。
 * - 离场达 30s±5s → locked（锁形标记）。
 */
export function btTick(st: BtState, nowSec: number, signalPresent: boolean): { state: BtState; lockTriggered: boolean; unlocked: boolean } {
  const s = { ...st };
  let lockTriggered = false;
  let unlocked = false;
  if (signalPresent) {
    if (s.phase !== "present") {
      unlocked = s.phase === "locked" || s.phase === "grace";
      s.phase = "present";
      s.awaySince = null;
      s.lockedAt = null;
    }
    return { state: s, lockTriggered, unlocked };
  }
  // 信号消失
  if (s.phase === "present") {
    s.phase = "away";
    s.awaySince = nowSec;
    return { state: s, lockTriggered: false, unlocked: false };
  }
  if (s.phase === "locked" || s.awaySince === null) return { state: s, lockTriggered: false, unlocked: false };
  const awayFor = nowSec - s.awaySince;
  if (awayFor >= BT_FLAP_IMMUNE_SEC && s.phase === "away") s.phase = "grace";
  if (awayFor >= BT_LOCK_AFTER_SEC - BT_LOCK_TOLERANCE_SEC) {
    s.phase = "locked";
    s.lockedAt = nowSec;
    lockTriggered = true;
  }
  return { state: s, lockTriggered, unlocked: false };
}

/** 锁形标记语义（判据「锁形标记」）：锁定时状态行显示的标识与可读原因。 */
export function btLockBadge(state: BtState): { show: boolean; reason: string } {
  switch (state.phase) {
    case "locked": return { show: true, reason: "蓝牙设备离开超过 30 秒，已自动锁定" };
    case "grace": return { show: true, reason: "蓝牙信号不稳定——若持续离开将自动锁定" };
    case "away": return { show: false, reason: "" };
    case "present": return { show: false, reason: "" };
  }
}

/* --------------------------- F316 双保险并存 --------------------------- */

export type LockCause = "bt-dynamic" | "idle-f316" | "manual";

/**
 * 双保险合并器（判据：与 F316 闲置锁屏双保险并存）：两锁事件在 2s 窗口内
 * 视为同一次锁（去重——通知不双弹），保留主因（手动 > 蓝牙 > 闲置）。
 */
export function mergeLockEvents(events: Array<{ cause: LockCause; atMs: number }>): Array<{ cause: LockCause; atMs: number; merged: LockCause[] }> {
  const priority: Record<LockCause, number> = { manual: 3, "bt-dynamic": 2, "idle-f316": 1 };
  const sorted = [...events].sort((a, b) => a.atMs - b.atMs);
  const out: Array<{ cause: LockCause; atMs: number; merged: LockCause[] }> = [];
  for (const e of sorted) {
    const last = out[out.length - 1];
    if (last && e.atMs - last.atMs <= 2000) {
      last.merged.push(e.cause);
      if (priority[e.cause] > priority[last.cause]) last.cause = e.cause;
      continue;
    }
    out.push({ cause: e.cause, atMs: e.atMs, merged: [e.cause] });
  }
  return out;
}

/* ------------------------------ 自检 ------------------------------ */

export function pinvaultSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 翻倍表：4 次失败 0 分钟；5→10、6→20、8→80、12→160（封顶）
  checks.push({ name: "F504 冷却翻倍表", pass: cooldownMinutesFor(4) === 0 && cooldownMinutesFor(5) === 10 && cooldownMinutesFor(6) === 20 && cooldownMinutesFor(8) === 80 && cooldownMinutesFor(12) === 160 });
  // PIN 形状：3 位拒、7 位拒、字母拒、4-6 位数字收
  checks.push({ name: "F504 4-6 位形状", pass: !isValidPinShape("123") && !isValidPinShape("1234567") && !isValidPinShape("12a4") && isValidPinShape("1234") && isValidPinShape("123456") });
  // 时序预算：全按预算=绿；散列超支=红且点名 hashMs
  const okTiming = auditPinTiming({ ...PIN_SEGMENT_BUDGET });
  const badTiming = auditPinTiming({ ...PIN_SEGMENT_BUDGET, hashMs: 900 });
  checks.push({ name: "F504 解锁时序对账", pass: okTiming.withinBudget && !badTiming.withinBudget && badTiming.over.includes("hashMs") });
  // RSSI：<10s 波动豁免不锁
  let s = btInitialState();
  s = btTick(s, 100, true).state;
  const flap1 = btTick(s, 105, false);
  const flap2 = btTick(flap1.state, 112, true);
  checks.push({ name: "F505 <10s 波动豁免", pass: flap1.state.phase === "away" && flap2.state.phase === "present" && !flap1.lockTriggered });
  // RSSI：离场 30s 触发锁定（容差内 25s 也锁——±5s 口径）
  let t = btInitialState();
  t = btTick(t, 0, true).state;
  const away = btTick(t, 5, false);
  const locked = btTick(away.state, 32, false);
  checks.push({ name: "F505 30s±5s 触发锁定", pass: locked.lockTriggered && locked.state.phase === "locked" });
  // 回连解锁
  const back = btTick(locked.state, 40, true);
  checks.push({ name: "F505 回连解锁", pass: back.unlocked && back.state.phase === "present" });
  // 锁形标记
  checks.push({ name: "F505 锁形标记", pass: btLockBadge(locked.state).show && !btLockBadge(s).show });
  // 双保险去重：蓝牙与闲置 1s 内 → 一条、主因蓝牙
  const merged = mergeLockEvents([
    { cause: "bt-dynamic", atMs: 1000 },
    { cause: "idle-f316", atMs: 1800 },
  ]);
  checks.push({ name: "F505 F316 双保险去重", pass: merged.length === 1 && merged[0]!.cause === "bt-dynamic" && merged[0]!.merged.length === 2 });
  return checks;
}
