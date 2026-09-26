/**
 * H4 真实浮层逻辑层（v3 · 大量深化）：
 * 拾色器/像素标尺/快捷键速查/专注芯片四个真实表面（H4Overlays.tsx 的可测逻辑面）。
 *
 * 诚实边界（生效面声明）：
 * - 拾色器采样的是 **VARIX 合成面**（webview DOM）——VARIX 桌面即本窗口，
 *   「取屏幕任意像素」在本系统内 = 取合成面像素；采样经 elementFromPoint →
 *   computed style 解析（rgb/rgba/hex 全格式 + 祖先回退链），零网络零截屏权限。
 * - 速查浮层与既有 Z-12（Ctrl+/）是**不同入口共存**：本浮层走 F374 判据的
 *   「长按 Win 600ms」触发；数据源消费 N-17 effectiveKeymap 真源（改键即反映），
 *   不复制注册表。
 */

import * as f359 from "../../system/h4/f359-colorPicker";
import * as f360 from "../../system/h4/f360-pixelRuler";
import * as f374 from "../../system/h4/f374-hotkeySheet";
import * as f363 from "../../system/h4/f363-focusTimer";

/* ------------------------------- 拾色器取样引擎 ------------------------------- */

/** 颜色字符串解析：rgb(a)/#hex 三格式 → Rgb；解析失败显式 null（不编色）。 */
export function parseColorString(raw: string): f359.Rgb | null {
  const s = raw.trim().toLowerCase();
  const rgba = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/.exec(s);
  if (rgba) return { r: f359.clampChannel(Number(rgba[1])), g: f359.clampChannel(Number(rgba[2])), b: f359.clampChannel(Number(rgba[3])) };
  const hex = /^#([0-9a-f]{6})$/.exec(s);
  if (hex) {
    const v = hex[1]!;
    return { r: parseInt(v.slice(0, 2), 16), g: parseInt(v.slice(2, 4), 16), b: parseInt(v.slice(4, 6), 16) };
  }
  return null;
}

export interface StyleReader {
  /** 读元素指定属性（getComputedStyle 的可注入面）。 */
  (el: Element, prop: string): string;
}

export interface SampleChainStep {
  el: Element | null;
  prop: string;
  value: string;
  parsed: f359.Rgb | null;
}

/**
 * 取样回退链（判据「取色准确性」的落地面）：
 * 目标元素 backgroundColor → color → 祖先逐级 backgroundColor（跳过透明），
 * 每一步留痕（诊断面）；全链透明 = null（诚实：取不到不编色）。
 */
export function sampleColorChain(target: Element | null, read: StyleReader): { color: f359.Rgb | null; chain: SampleChainStep[] } {
  const chain: SampleChainStep[] = [];
  let cur: Element | null = target;
  let depth = 0;
  while (cur && depth < 12) {
    for (const prop of depth === 0 ? ["background-color", "color"] : ["background-color"]) {
      const value = read(cur, prop);
      const parsed = value && value !== "transparent" ? parseColorString(value) : null;
      chain.push({ el: cur, prop, value, parsed });
      if (parsed) return { color: parsed, chain };
    }
    cur = cur.parentElement;
    depth++;
  }
  return { color: null, chain };
}

/** 拾色会话状态机（f359.pickOnce 的宿主面）：Esc 退、Shift 连续、取样计数。 */
export interface PickerHostState {
  active: boolean;
  picks: number;
  recent: f359.Rgb[];
  last: f359.Rgb | null;
}

export function pickerHostStart(state: PickerHostState): PickerHostState {
  return { ...state, active: true };
}

export function pickerHostEscape(state: PickerHostState): PickerHostState {
  return { ...state, active: false };
}

/** 命中取样： Shift 按住则保持（f359.pickOnce 语义），取样入最近色（F234 联动）。 */
export function pickerHostPick(state: PickerHostState, color: f359.Rgb | null, shiftHeld: boolean): PickerHostState {
  if (!color) return { ...state, active: shiftHeld }; // 取不到不编色：链路全透明时仅推进会话
  const next = f359.pickOnce({ active: state.active, shiftHeld, picks: state.picks });
  return { ...state, active: next.active, picks: next.picks, last: color, recent: f359.pushRecentColor(state.recent, color) };
}

/* ------------------------------- 像素标尺宿主面 ------------------------------- */

export interface RulerHostState {
  active: boolean;
  origin: { x: number; y: number } | null;
  current: { x: number; y: number } | null;
}

export function rulerHostStart(state: RulerHostState, at: { x: number; y: number }): RulerHostState {
  return { ...state, active: true, origin: at, current: at };
}

export function rulerHostMove(state: RulerHostState, at: { x: number; y: number }): RulerHostState {
  return state.origin ? { ...state, current: at } : state;
}

/** 量测读数（复用 f360 引擎：dx/dy 绝对值 + 归一化矩形）。 */
export function rulerHostReadout(state: RulerHostState): { dx: number; dy: number; rect: { x: number; y: number; w: number; h: number } } | null {
  if (!state.origin || !state.current) return null;
  const norm = f360.measureReadout({
    x: Math.min(state.origin.x, state.current.x),
    y: Math.min(state.origin.y, state.current.y),
    w: Math.abs(state.current.x - state.origin.x),
    h: Math.abs(state.current.y - state.origin.y),
  });
  return {
    dx: Math.abs(state.current.x - state.origin.x),
    dy: Math.abs(state.current.y - state.origin.y),
    rect: { x: Math.min(state.origin.x, state.current.x), y: Math.min(state.origin.y, state.current.y), w: norm?.w ?? 0, h: norm?.h ?? 0 },
  };
}

export function rulerHostEscape(_state: RulerHostState): RulerHostState {
  return { active: false, origin: null, current: null };
}

/** 网格叠加绘制计划（8px 步进 20% 透明度——f360 gridPaintSpec 一处定义的宿主换算）。 */
export function rulerGridPlan(w: number, h: number): { step: number; opacity: number; lines: number } {
  const spec = f360.gridPaintSpec();
  return { step: spec.step, opacity: spec.opacity, lines: Math.ceil(w / spec.step) + Math.ceil(h / spec.step) };
}

/* ------------------------------- 速查浮层同源适配 ------------------------------- */

/** N-17/F244 键位表 → F374 HotkeyEntry 适配（真源消费：改键后立即反映——无缓存层）。 */
export interface ExternalKeymapRow {
  action: string;
  labelKey: string;
  accel: string;
  group: string;
}

export function entriesFromKeymap(rows: ReadonlyArray<ExternalKeymapRow>): f374.HotkeyEntry[] {
  return rows.map((r) => ({
    combo: r.accel.toUpperCase(),
    action: r.labelKey,
    group: r.group === "window" ? "window" : r.group === "system" || r.group === "launch" ? "system" : "appGeneric",
    app: r.group === "panel" ? "settings" : undefined,
  }));
}

/** 长按 Win 会话（f374 状态机宿主面）：真键盘事件的进/出。 */
export interface WinHoldHostState {
  sheet: f374.CheatSheetState;
  holdTimer: number | null;
}

export function winHoldDown(atMs: number): WinHoldHostState {
  return { sheet: f374.winDown(f374.initialSheet(), atMs), holdTimer: null };
}

/** tick 心跳：600ms 后 visible（宿主以 100ms 间隔喂入）。 */
export function winHoldTick(host: WinHoldHostState, nowMs: number): WinHoldHostState {
  return { ...host, sheet: f374.tick(host.sheet, nowMs) };
}

export function winHoldUp(host: WinHoldHostState): WinHoldHostState {
  return { ...host, holdTimer: null, sheet: f374.winUp(host.sheet) };
}

/* ------------------------------- 专注芯片宿主面 ------------------------------- */

export interface FocusChipState {
  run: f363.FocusRun | null;
  /** 每日账是否已记（到点自动记——防双通道提醒重复入账）。 */
  recorded: boolean;
}

export function focusChipTick(state: FocusChipState, now: number): { state: FocusChipState; badge: string | null; due: boolean } {
  if (!state.run || state.run.outcome !== "running") return { state, badge: null, due: false };
  if (f363.isDue(state.run, now)) {
    const done: f363.FocusRun = { ...state.run, endedAt: now, outcome: "completed" };
    const recorded = state.recorded || f363.recordRun(done, now).ok;
    return { state: { run: done, recorded }, badge: null, due: true };
  }
  return { state, badge: f363.badgeText(state.run, now), due: false };
}

/** 芯片摆放（bottom-left：任务栏 48px + 边距 16px；竖屏换算由消费面按 F388 做）。 */
export function focusChipPlacement(_screenH: number): { bottom: number; left: number } {
  return { bottom: 48 + 16, left: 16 };
}

/* ------------------------------- 召唤事件（开放性扩展点） ------------------------------- */

/** 拾色器召唤事件（设置页/快捷面板派发）。 */
export const SUMMON_PICKER = "vx-h4-pick";
/** 像素标尺召唤事件。 */
export const SUMMON_RULER = "vx-h4-ruler";
/** 专注计时事件（载荷 { type: "start"; minutes: number }）。 */
export const FOCUS_EVENT = "vx-h4-focus";
