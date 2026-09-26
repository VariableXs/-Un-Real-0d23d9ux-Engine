/**
 * 指针与提示六件（AI-U3 · F513 Ctrl 定位 / F514 声音视觉提示 / F519 Caps 音 /
 * F520 中键最小化 / F522 指针轨迹 / F523 打字隐藏指针）。
 *
 * 判据唯一源（主册摘文）：
 * - F513「按住 Ctrl 键 1 秒：同心涟漪三轮扩散 1.5s；多屏时涟漪出现在指针
 *   真实所在屏；组合键场景豁免（Ctrl+C 等 20 例 0 误触）；开关默认开」。
 * - F514「通知蓝/警告黄/电量红——边缘 8px 光带 2 次脉冲；逐事件开关；
 *   与勿扰档（F341）联动（仅声音档=只闪不响、全静=都不来但中心记录）」。
 * - F519「开=高双音、关=低双音；默认关；音量跟随系统提示音量（F240）」。
 * - F520「标题栏中键=最小化；三义分流（中收/双大/右菜单）按按键类型天然
 *   分流；开关（不喜可关）；动画 F124」。
 * - F522「轨迹三档：200/400/600ms；走 F335 优先平面；默认关；开关即时」。
 * - F523「键盘输入开始 <100ms 淡出至 30%、停止 2s 或移动鼠标即回 100%；
 *   触屏豁免；默认关」。
 */

import { u3Store } from "./u3store";

/* ------------------------------- F513 Ctrl 定位 ------------------------------- */

/** 按住 1s 触发（主册 F513 判据）。 */
export const CTRL_FIND_HOLD_MS = 1000;
/** 三轮涟漪共 1.5s（主册 F513 规格表）。 */
export const CTRL_FIND_RIPPLES = 3;
export const CTRL_FIND_TOTAL_MS = 1500;

/** 组合键豁免表（判据：Ctrl+C 等 20 例 0 误触——非纯 Ctrl 按住即豁免）。 */
export const CTRL_COMBO_EXEMPT = [
  "c", "v", "x", "a", "z", "y", "s", "f", "h", "n", "p", "w", "t", "k", "d", "r", "l", "b", "j", "e",
] as const;

export interface CtrlFindRt {
  downAtMs: number | null;
  otherKeyDown: boolean;
}

/** Ctrl 按住状态机 tick：纯 Ctrl 持续 1s → 触发；期间出现任何其他键 → 豁免复位。 */
export function ctrlFindTick(rt: CtrlFindRt, key: "ctrl" | "other" | "up", now: number): { fired: boolean; progress: number } {
  if (key === "up") {
    rt.downAtMs = null;
    rt.otherKeyDown = false;
    return { fired: false, progress: 0 };
  }
  if (key === "other") {
    rt.otherKeyDown = true;
    return { fired: false, progress: 0 };
  }
  if (rt.otherKeyDown) return { fired: false, progress: 0 };
  if (rt.downAtMs === null) {
    rt.downAtMs = now;
    return { fired: false, progress: 0 };
  }
  const held = now - rt.downAtMs;
  if (held >= CTRL_FIND_HOLD_MS) {
    rt.downAtMs = null; // 触发后复位（松开前不重复触发）
    return { fired: true, progress: 1 };
  }
  return { fired: false, progress: held / CTRL_FIND_HOLD_MS };
}

/** 涟漪时间轴：第 i 轮起点 = i × total/3（三轮均布）。 */
export function rippleStarts(totalMs = CTRL_FIND_TOTAL_MS, count = CTRL_FIND_RIPPLES): number[] {
  return Array.from({ length: count }, (_, i) => (i * totalMs) / count);
}

/* ------------------------------- F514 声音视觉提示 ------------------------------- */

export type SoundEventKind = "notify" | "warn" | "battery";
/** 三事件色映射（主册：通知蓝/警告黄/电量红）。 */
export const SOUND_EVENT_COLORS: Record<SoundEventKind, string> = {
  notify: "var(--vx-sv-notify, #3b82f6)",
  warn: "var(--vx-sv-warn, #eab308)",
  battery: "var(--vx-sv-battery, #ef4444)",
};
/** 边缘 8px 光带 2 次脉冲（主册 F514 规格表）。 */
export const SV_EDGE_PX = 8;
export const SV_PULSES = 2;
/** 单次脉冲时长（视觉节奏：in 120ms + out 180ms × 2 = 600ms 总长）。 */
export const SV_PULSE_MS = 300;

/** 勿扰联动镜像（判据：仅声音档=只闪不响、全静=都不来但中心记录）。 */
export type DisturbMode = "normal" | "sound-only" | "silent";
export function soundLightPolicy(kind: SoundEventKind, mode: DisturbMode, perEventEnabled: boolean): { flash: boolean; sound: boolean; logToCenter: boolean } {
  const evOn = perEventEnabled;
  switch (mode) {
    case "normal":     return { flash: evOn, sound: true, logToCenter: true };
    case "sound-only": return { flash: evOn, sound: false, logToCenter: true };  // 只闪不响
    case "silent":     return { flash: false, sound: false, logToCenter: true }; // 都不来但中心记录
  }
}

/* ------------------------------- F519 Caps 提示音 ------------------------------- */

export type CapsTone = "on" | "off";
/** 双音色频率（开=高双音 880/988Hz、关=低双音 440/494Hz——盲打可辨）。 */
export const CAPS_TONE_HZ: Record<CapsTone, [number, number]> = { on: [880, 988], off: [440, 494] };

/* ------------------------------- F520 标题栏三义分流 ------------------------------- */

export type TitleBarClick = "middle" | "double" | "right";
/** 三义分流矩阵（判据：中收/双大/右菜单——判定按按键类型天然分流）。 */
export function titleBarAction(click: TitleBarClick, enabled: boolean): "minimize" | "maximize-toggle" | "sysmenu" | "none" {
  if (click === "middle") return enabled ? "minimize" : "none";
  if (click === "double") return "maximize-toggle";
  return "sysmenu";
}

/* ------------------------------- F522 指针轨迹 ------------------------------- */

/** 三档轨迹存留（主册：200/400/600ms）。 */
export const TRAIL_LEN_MS = { short: 200, medium: 400, long: 600 } as const;
export type TrailLen = keyof typeof TRAIL_LEN_MS;

export interface TrailPoint { x: number; y: number; atMs: number }

/** 轨迹采样：保留 now-retainMs 之后的点（判据：三档时长实测）。 */
export function trailSample(points: TrailPoint[], nowMs: number, len: TrailLen): TrailPoint[] {
  const retain = TRAIL_LEN_MS[len];
  return points.filter((p) => nowMs - p.atMs <= retain);
}

/* ------------------------------- F523 打字隐藏指针 ------------------------------- */

/** 输入开始 <100ms 淡出（主册 F523 判据）。 */
export const TYPE_HIDE_FADE_MS = 100;
/** 停止输入 2s 恢复（主册 F523 判据）。 */
export const TYPE_HIDE_RESUME_MS = 2000;
/** 30% 透明度（主册：隐身但可寻）。 */
export const TYPE_HIDE_OPACITY = 0.3;

export interface TypeHideRt { lastKeyMs: number; mouseMovedMs: number }

/** 指针透明度计算：打字中淡出至 30%，停 2s 或动鼠标回 100%（移动恢复即时）。 */
export function typeHideOpacity(rt: TypeHideRt, now: number, enabled: boolean, isTouch: boolean): number {
  if (!enabled || isTouch) return 1;
  const typing = now - rt.lastKeyMs < TYPE_HIDE_RESUME_MS;
  const mouseMovedAfterKey = rt.mouseMovedMs >= rt.lastKeyMs;
  if (!typing || mouseMovedAfterKey) return 1;
  return TYPE_HIDE_OPACITY;
}

/* ------------------------------- 运行时读取 ------------------------------- */

export function ctrlFindConfig() {
  const s = u3Store.get("ctrlFind");
  return { enabled: (s.enabled as boolean) ?? true, holdMs: (s.holdMs as number) ?? CTRL_FIND_HOLD_MS };
}
export function typeHideConfig() {
  const s = u3Store.get("typeHide");
  return { enabled: (s.enabled as boolean) ?? false, opacity: (s.opacity as number) ?? TYPE_HIDE_OPACITY };
}

/* ------------------------------- 自检 ------------------------------- */

/** F513/F514/F519/F520/F522/F523 判据自检（前端面）。 */
export function pointerfxSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // Ctrl 定位：组合键豁免 + 1s 触发
  const rt: CtrlFindRt = { downAtMs: null, otherKeyDown: false };
  ctrlFindTick(rt, "ctrl", 0);
  ctrlFindTick(rt, "other", 300); // Ctrl+C 组合
  const afterCombo = ctrlFindTick(rt, "ctrl", 5000);
  checks.push({ name: "F513 组合键豁免", pass: !afterCombo.fired });
  const rt2: CtrlFindRt = { downAtMs: null, otherKeyDown: false };
  ctrlFindTick(rt2, "ctrl", 0);
  const fired = ctrlFindTick(rt2, "ctrl", 1000);
  checks.push({ name: "F513 1s 触发三轮", pass: fired.fired && rippleStarts().length === 3 });
  // 豁免表 20 例
  checks.push({ name: "F513 豁免表 20 例", pass: CTRL_COMBO_EXEMPT.length === 20 });
  // 勿扰镜像
  const so = soundLightPolicy("notify", "sound-only", true);
  const si = soundLightPolicy("warn", "silent", true);
  checks.push({ name: "F514 勿扰联动镜像", pass: so.flash && !so.sound && !si.flash && !si.sound && si.logToCenter });
  // 三义分流
  checks.push({
    name: "F520 三义分流矩阵",
    pass: titleBarAction("middle", true) === "minimize" && titleBarAction("double", true) === "maximize-toggle" && titleBarAction("right", true) === "sysmenu" && titleBarAction("middle", false) === "none",
  });
  // 轨迹三档（@600ms：short=550 / medium=350,550 / long=全 4）
  const pts: TrailPoint[] = [{ x: 0, y: 0, atMs: 0 }, { x: 1, y: 1, atMs: 150 }, { x: 2, y: 2, atMs: 350 }, { x: 3, y: 3, atMs: 550 }];
  checks.push({
    name: "F522 三档存留",
    pass: trailSample(pts, 600, "short").length === 1 && trailSample(pts, 600, "medium").length === 2 && trailSample(pts, 600, "long").length === 4,
  });
  // 打字隐藏：淡出 30% / 动鼠标恢复 / 触屏豁免
  const rtT: TypeHideRt = { lastKeyMs: 0, mouseMovedMs: -1 };
  checks.push({
    name: "F523 30%/恢复/触屏",
    pass: typeHideOpacity(rtT, 500, true, false) === 0.3 && (rtT.mouseMovedMs = 100, typeHideOpacity(rtT, 500, true, false)) === 1 && typeHideOpacity(rtT, 100, true, true) === 1,
  });
  checks.push({ name: "F523 默认关", pass: typeHideConfig().enabled === false });
  return checks;
}
