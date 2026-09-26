/**
 * H4 运行时状态机（AI-H4 深化批次 v2 · F351-F400 界面壳层的逻辑面）。
 *
 * 职责边界（诚实声明）：
 * - 本文件是 H4Runtime.tsx 的**可测逻辑层**——开机序列（F371 徽标 + F399 彩蛋）、
 *   色彩滤镜应用计划（F387 互斥单点）、阅读模式排版变量（F386 三参数）三件事
 *   的纯函数实现；React 壳只做事件监听与 DOM 落笔，不做任何决策。
 * - 与引擎模块的关系：不复制引擎状态、不二次存储——全部读写走
 *   `src/system/h4/` 各模块的原生持久化（variable:h4:* 键域），本层只做
 *   「编排与换算」，一处一事实。
 * - 广播通道：滤镜/阅读模式变更后派发 CustomEvent（开放性扩展点，十四章；
 *   与 J1Runtime 的 vx-j1-action 同款惯例）——桌面内任何组件可订阅跟随。
 */

import * as bootBadge from "../../system/h4/f371-bootBadge";
import * as eggs from "../../system/h4/f399-easterEggs";
import * as grayscale from "../../system/h4/f387-grayscaleMode";
import * as reading from "../../system/h4/f386-readingMode";
import type { KvStore } from "../../system/h4/internal/store";

/* ------------------------------- 广播通道 ------------------------------- */

/** 滤镜位变更事件（载荷：{ now: ColorFilter }）。 */
export const H4_FILTER_EVENT = "vx-h4-filter-changed";
/** 阅读模式变更事件（载荷：{ appId, on }）。 */
export const H4_READING_EVENT = "vx-h4-reading-changed";

export function announceFilterChanged(now: grayscale.ColorFilter): void {
  window.dispatchEvent(new CustomEvent(H4_FILTER_EVENT, { detail: { now } }));
}

export function announceReadingChanged(appId: string, on: boolean): void {
  window.dispatchEvent(new CustomEvent(H4_READING_EVENT, { detail: { appId, on } }));
}

/* ------------------------------- 开机序列 ------------------------------- */

export interface BootPlan {
  /** 徽标视图模型（null = 徽标被用户关闭——F371 可关判据）。 */
  badge: bootBadge.BootRecord | null;
  badgeVm: bootBadge.BadgeViewModel | null;
  /** 彩蛋①播放计划（一次性：第 100 次开机且未放过）。 */
  eggPlay: boolean;
  eggVariant: string | null;
  /** 本次计入的开机序号（徽标与曲线同源账本）。 */
  seq: number;
}

export interface BootOnceState {
  /** 本会话是否已跑过（去重守卫：同一会话多次 mount 只计一次开机）。 */
  done: boolean;
}

/**
 * 开机序列（纯函数）：记账 → 计数 → 彩蛋判定，一次会话只执行一次。
 * - F371：把实测开机时长写入历史账（seq = 账本长度 + 1，徽标与曲线同源）；
 * - F399：开机计数 +1；第 100 次且未放过 → 播放一次（一次性判据由引擎持久化）。
 * 幂等保证：state.done 为 true 时返回零动作计划，绝不重复记账。
 */
export function runBootPlan(state: BootOnceState, store: KvStore, measuredMs: number): { state: BootOnceState; plan: BootPlan } {
  if (state.done) {
    return { state, plan: { badge: null, badgeVm: null, eggPlay: false, eggVariant: null, seq: 0 } };
  }
  const seq = bootBadge.bootHistory(store).length + 1;
  const rec: bootBadge.BootRecord = { seq, measuredMs: Math.max(0, Math.round(measuredMs)), at: Date.now() };
  bootBadge.recordBoot(rec, store);
  eggs.countBoot(store); // F399 开机计数（bootEgg 内部读同一账本判定一次性）
  const egg = eggs.bootEgg(store);
  const badgeEnabled = bootBadge.isEnabled(store);
  const next: BootOnceState = { done: true };
  return {
    state: next,
    plan: {
      badge: badgeEnabled ? rec : null,
      badgeVm: badgeEnabled ? bootBadge.badgeFromMeasured(rec) : null,
      eggPlay: egg.play,
      eggVariant: egg.variant,
      seq,
    },
  };
}

/* ------------------------------- 滤镜应用计划 ------------------------------- */

export interface FilterPlan {
  /** documentElement 上的属性（null = 移除）。 */
  attr: string | null;
  /** 单点滤镜 CSS（null = 无滤镜）。 */
  cssFilter: string | null;
}

/** 滤镜 → 应用计划（F387 单点审计的落地面：全系统只有这一个翻译点）。 */
export function filterPlan(now: grayscale.ColorFilter): FilterPlan {
  switch (now) {
    case "grayscale":
      return { attr: "grayscale", cssFilter: "grayscale(1)" };
    case "highContrast":
      return { attr: "high-contrast", cssFilter: "contrast(1.6)" };
    case "colorDeficiency":
      return { attr: "cvd", cssFilter: "saturate(0.72) hue-rotate(14deg)" };
    default:
      return { attr: null, cssFilter: null };
  }
}

/** 把滤镜计划落到元素（可注入元素——测试与 documentElement 共用一份实现）。 */
export function applyFilterPlan(el: Element, plan: FilterPlan): void {
  if (plan.attr === null) el.removeAttribute("data-h4-filter");
  else el.setAttribute("data-h4-filter", plan.attr);
  const style = el as HTMLElement & { style: CSSStyleDeclaration };
  if (plan.cssFilter === null) style.style.removeProperty("--h4-filter");
  else style.style.setProperty("--h4-filter", plan.cssFilter);
}

/* ------------------------------- 阅读模式变量 ------------------------------- */

export interface ReadingVars {
  lineHeightRatio: string;
  measurePx: string;
  serif: string;
}

/** 阅读模式三参数 → CSS 变量（F386 排版换算的唯一落地点）。 */
export function readingVarsFor(style: reading.ReadingStyle, fontSizePx: number, sampleText: string): ReadingVars {
  return {
    lineHeightRatio: String(style.lineHeight),
    measurePx: String(reading.pageWidthPx(style, fontSizePx, sampleText)),
    serif: style.serif ? "1" : "0",
  };
}

/** 把阅读变量落到元素（on=false 时移除全部变量——零残留判据）。 */
export function applyReadingVars(el: HTMLElement, vars: ReadingVars | null): void {
  const keys = ["--h4-reading-line", "--h4-reading-measure", "--h4-reading-serif"] as const;
  if (vars === null) {
    for (const k of keys) el.style.removeProperty(k);
    el.removeAttribute("data-h4-reading");
    return;
  }
  el.style.setProperty("--h4-reading-line", vars.lineHeightRatio);
  el.style.setProperty("--h4-reading-measure", vars.measurePx);
  el.style.setProperty("--h4-reading-serif", vars.serif);
  el.setAttribute("data-h4-reading", "on");
}

/** 开机徽标浮层的阶段类名（BADGE_TIMING 真实时序 → CSS 过渡类）。 */
export function badgePhaseClass(tSinceShowMs: number): string {
  switch (bootBadge.badgePhase(tSinceShowMs)) {
    case "fade-in":
      return "h4-boot-badge--in";
    case "hold":
      return "h4-boot-badge--hold";
    case "fade-out":
      return "h4-boot-badge--out";
    default:
      return "h4-boot-badge--gone";
  }
}
