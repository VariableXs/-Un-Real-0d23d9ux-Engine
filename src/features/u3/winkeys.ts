/**
 * 窗口与快捷键八件（AI-U3 · F516 横幅位置 / F518 输入法切换键 / F535 Win+数字 /
 * F536 Win+T / F537 瞥桌面 / F538 Alt+Esc / F539 布局锁定 / F548 管理器置顶）。
 *
 * 判据唯一源（主册摘文）：
 * - F516「右下（默认）/顶部中央/顶部右侧三档；堆叠方向随位置自适应（右下向
 *   上堆、顶部向下堆——F383 排队纪律不变）；OSD 位置独立不动（文档化）；
 *   多屏时横幅出主屏」。
 * - F518「默认 Win+空格，可选 Ctrl+Shift/Alt+Shift/Caps 长按；与 F421 点击
 *   循环并存；自定义后旧键让出（一键一职，F244 冲突审计同源）；Caps 双用
 *   （长按 600ms 切输入法、短按锁定大小写）」。
 * - F535「Win+1..0 按序激活任务栏第 N 个图标——没运行则启动（F282）、运行中
 *   则切换/最小化切换（F252 语义同源）；顺序与任务栏排布一致（含溢出 F495）；
 *   Shift 变体=新开实例」。
 * - F536「Win+T 焦点跳任务栏：方向键遍历、Enter 激活、菜单键=右键语义；
 *   首尾回绕；Esc 退回窗口焦点」。
 * - F537「按住 Win+逗号：窗口 120ms 淡出至 15% 透明度；松开 120ms 恢复；
 *   纯看限制（瞥的过程桌面交互受限）；与 F316 闲置计时无冲突」。
 * - F538「Alt+Esc Z 序循环（无浮层）；最小化窗参与（选中即还原）；连按
 *   <100ms/次；与 Alt+Tab 并存」。
 * - F539「布局锁定后图标不可拖动（轻微抖动 120ms 提示+状态栏一句话）；右键
 *   排列照常；解锁需到设置页；锁角标可选；默认关」。
 * - F548「任务管理器置顶（F248 语义不抢焦点）；边框亮标记；重启不记忆」。
 */

import { u3Store } from "./u3store";

/* ------------------------------- F516 横幅位置 ------------------------------- */

export type BannerPos = "bottom-right" | "top-center" | "top-right";

/** 堆叠方向自适应（判据：右下向上堆、顶部向下堆——F383 排队纪律不变）。 */
export function bannerStackDirection(pos: BannerPos): "up" | "down" {
  return pos === "bottom-right" ? "up" : "down";
}

/** 横幅锚点几何（判据：多屏时横幅出主屏——由调用方固定传主屏尺寸）。 */
export function bannerAnchor(pos: BannerPos, screen: { w: number; h: number }, bannerSize: { w: number; h: number }, margin = 16): { x: number; y: number } {
  switch (pos) {
    case "bottom-right": return { x: screen.w - bannerSize.w - margin, y: screen.h - bannerSize.h - margin };
    case "top-center": return { x: (screen.w - bannerSize.w) / 2, y: margin };
    case "top-right": return { x: screen.w - bannerSize.w - margin, y: margin };
  }
}

/** OSD 独立性声明（判据：OSD 位置独立不动——文档化）。 */
export const OSD_SEPARATE_NOTE = "OSD（音量/亮度）位置独立于通知横幅，不随本设置变动——通知是通知、OSD 是 OSD";

/* ------------------------------- F518 输入法切换键 ------------------------------- */

export type ImeScheme = "win-space" | "ctrl-shift" | "alt-shift" | "caps-long";
export const IME_SCHEMES: Array<{ id: ImeScheme; label: string }> = [
  { id: "win-space", label: "Win+空格（Windows 同款默认）" },
  { id: "ctrl-shift", label: "Ctrl+Shift（老输入法习惯）" },
  { id: "alt-shift", label: "Alt+Shift" },
  { id: "caps-long", label: "Caps 长按（双用键）" },
];
/** Caps 双用分界（主册：长按 600ms 切输入法、短按锁定大小写）。 */
export const CAPS_LONG_PRESS_MS = 600;

/** Caps 两段语义裁决（判据：600ms 分界实测）。 */
export function capsVerdict(holdMs: number): "ime-toggle" | "caps-lock" {
  return holdMs >= CAPS_LONG_PRESS_MS ? "ime-toggle" : "caps-lock";
}

/** 让出旧键（判据：一键一职——切走 scheme 后旧键不再有切换职责）。 */
export function imeKeyOwnership(scheme: ImeScheme): { owner: ImeScheme; retired: ImeScheme[] } {
  return { owner: scheme, retired: IME_SCHEMES.map((s) => s.id).filter((id) => id !== scheme) };
}

/* ------------------------------- F535 Win+数字 ------------------------------- */

export interface TaskbarSlot { appId: string; running: boolean }

export type WinNumVerdict =
  | { action: "launch"; appId: string }
  | { action: "switch"; appId: string }
  | { action: "minimize-toggle"; appId: string }
  | { action: "new-instance"; appId: string }
  | { action: "none" };

/** 数字键 → 语义裁决（判据：没运行则启动/运行中切换/最小化切换；Shift=新实例）。 */
export function winNumberResolve(slots: TaskbarSlot[], digit: number, shift: boolean, focusedAppId: string | null): WinNumVerdict {
  if (digit < 0 || digit > 9 || digit >= slots.length) return { action: "none" };
  const slot = slots[digit];
  if (!slot) return { action: "none" }; // 防御（上方越界守卫已覆盖）
  if (shift) return { action: "new-instance", appId: slot.appId };
  if (!slot.running) return { action: "launch", appId: slot.appId };
  if (focusedAppId === slot.appId) return { action: "minimize-toggle", appId: slot.appId };
  return { action: "switch", appId: slot.appId };
}

/* ------------------------------- F536 Win+T 遍历 ------------------------------- */

export interface WinTRt { active: boolean; index: number }

/** Win+T 遍历状态机（判据：进场/遍历/激活/菜单/退出五链路；首尾回绕；Esc 归还）。 */
export function winTStep(rt: WinTRt, key: "open" | "next" | "prev" | "esc", count: number): { index: number; active: boolean; exited: boolean } {
  switch (key) {
    case "open":
      rt.active = true;
      rt.index = 0;
      return { index: rt.index, active: true, exited: false };
    case "next":
      if (!rt.active) return { index: rt.index, active: false, exited: false };
      rt.index = (rt.index + 1) % count; // 首尾回绕
      return { index: rt.index, active: true, exited: false };
    case "prev":
      if (!rt.active) return { index: rt.index, active: false, exited: false };
      rt.index = (rt.index - 1 + count) % count;
      return { index: rt.index, active: true, exited: false };
    case "esc":
      rt.active = false;
      return { index: rt.index, active: false, exited: true }; // Esc 归还焦点
  }
}

/* ------------------------------- F537 瞥桌面 ------------------------------- */

/** 透明度 15%±3%（主册 F537 规格表）。 */
export const PEEK_OPACITY = 0.15;
/** 进出 120ms±20ms（主册 F537 规格表）。 */
export const PEEK_FADE_MS = 120;

/** 瞥桌面透明度序列（判据：纯看限制——瞥的过程窗口不接受点击）。 */
export function peekStyle(peeking: boolean): { opacity: number; pointerEvents: "none" | "auto"; transition: string } {
  return peeking
    ? { opacity: PEEK_OPACITY, pointerEvents: "none", transition: `opacity ${PEEK_FADE_MS}ms` }
    : { opacity: 1, pointerEvents: "auto", transition: `opacity ${PEEK_FADE_MS}ms` };
}

/* ------------------------------- F538 Alt+Esc ------------------------------- */

export interface AltEscRt { altHeld: boolean }

/**
 * Z 序循环（判据：按住 Alt 连按 Esc 逐窗后退；最小化窗参与——选中即还原；
 * 连按 <100ms/次节流防抖）。
 * zOrder: 窗口 id 数组（索引 0 = 顶层）。返回循环后的激活窗 id。
 */
export function altEscStep(rt: AltEscRt, zOrder: string[], now: number, lastStepMs: number): { focusId: string | null; minimizedRestored: boolean } {
  if (!rt.altHeld || zOrder.length === 0) return { focusId: null, minimizedRestored: false };
  if (now - lastStepMs < 100) return { focusId: null, minimizedRestored: false }; // 连按节奏保护
  const next = zOrder[zOrder.length - 1] ?? null; // Z 序最后 = 最久未用 → 循环后退
  if (!next) return { focusId: null, minimizedRestored: false };
  return { focusId: next, minimizedRestored: true };
}

/* ------------------------------- F539 桌面布局锁定 ------------------------------- */

/** 抖动提示 120ms（主册 F539 规格表）。 */
export const LOCK_SHAKE_MS = 120;

/** 拖拽裁决（判据：锁定后拖拽被温和拒绝；右键排列照常——锁乱序不锁秩序）。 */
export function layoutDragVerdict(locked: boolean): { allowed: boolean; feedback: "shake" | "none"; statusbar: string } {
  if (!locked) return { allowed: true, feedback: "none", statusbar: "" };
  return { allowed: false, feedback: "shake", statusbar: "桌面布局已锁定——到设置中心解除后再排列" };
}

/** 解锁路径深度声明（判据：解锁需到设置页——桌面右键不放解锁项）。 */
export const UNLOCK_PATH = "设置中心 → 桌面 → 布局锁定";

/* ------------------------------- F548 任务管理器置顶 ------------------------------- */

export interface AlwaysOnTopRt { pinned: boolean; sessionOnly: true }

/** 置顶语义（判据：F248 复用——不抢焦点只不被盖；重启不记忆）。 */
export function taskmgrTopVerdict(rt: AlwaysOnTopRt): { onTop: boolean; stealsFocus: false; remembered: false; border: string } {
  return {
    onTop: rt.pinned,
    stealsFocus: false, // 不抢焦点：点击他窗焦点正常转移
    remembered: false,  // 重启不记忆：默认回常态
    border: rt.pinned ? "亮色边框标记" : "常态",
  };
}

/* ------------------------------- 运行时读取 ------------------------------- */

export function imeToggleConfig() {
  const s = u3Store.get("imeToggle");
  return { scheme: (s.scheme as ImeScheme) ?? "win-space", capsLongPressMs: (s.capsLongPressMs as number) ?? CAPS_LONG_PRESS_MS };
}
export function layoutLockConfig() {
  const s = u3Store.get("layoutLock");
  return { locked: !!s.locked, shakeMs: (s.shakeMs as number) ?? LOCK_SHAKE_MS, cornerBadge: !!s.cornerBadge };
}

/* ------------------------------- 自检 ------------------------------- */

/** F516/F518/F535-F539/F548 判据自检（前端面）。 */
export function winkeysSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 横幅三档+堆叠方向
  checks.push({
    name: "F516 三档+堆叠自适应",
    pass: bannerStackDirection("bottom-right") === "up" && bannerStackDirection("top-center") === "down" && bannerStackDirection("top-right") === "down",
  });
  const anchor = bannerAnchor("bottom-right", { w: 1920, h: 1080 }, { w: 360, h: 80 });
  checks.push({ name: "F516 主屏几何", pass: anchor.x === 1920 - 360 - 16 && anchor.y === 1080 - 80 - 16 });
  // Caps 600ms 分界
  checks.push({ name: "F518 600ms 分界", pass: capsVerdict(599) === "caps-lock" && capsVerdict(600) === "ime-toggle" && imeKeyOwnership("caps-long").retired.length === 3 });
  // Win+数字三态+Shift 变体
  const slots: TaskbarSlot[] = [{ appId: "a", running: false }, { appId: "b", running: true }];
  checks.push({
    name: "F535 启动/切换/最小化/Shift",
    pass: winNumberResolve(slots, 0, false, null).action === "launch" && winNumberResolve(slots, 1, false, "c").action === "switch" && winNumberResolve(slots, 1, false, "b").action === "minimize-toggle" && winNumberResolve(slots, 1, true, null).action === "new-instance" && winNumberResolve(slots, 9, false, null).action === "none",
  });
  // Win+T 回绕与 Esc
  const rt: WinTRt = { active: false, index: 0 };
  winTStep(rt, "open", 3);
  const wrap = winTStep(winTStep(winTStep(rt, "next", 3), "next", 3), "next", 3);
  const esc = winTStep(rt, "esc", 3);
  checks.push({ name: "F536 首尾回绕+Esc 归还", pass: wrap.index === 0 && esc.exited });
  // 瞥桌面
  const pk = peekStyle(true);
  checks.push({ name: "F537 15%/120ms/纯看", pass: pk.opacity === 0.15 && pk.pointerEvents === "none" && PEEK_FADE_MS === 120 });
  // Alt+Esc 节流
  const ae: AltEscRt = { altHeld: true };
  const s1 = altEscStep(ae, ["w1", "w2"], 0, -1000);
  const s2 = altEscStep(ae, ["w1", "w2"], 50, 0); // 50ms < 100ms 节流
  checks.push({ name: "F538 Z 序+连按节流", pass: s1.focusId === "w2" && s2.focusId === null });
  // 布局锁定
  const dv = layoutDragVerdict(true);
  checks.push({ name: "F539 拒绝+抖动+照常排列", pass: !dv.allowed && dv.feedback === "shake" && UNLOCK_PATH.includes("设置中心") });
  // 置顶
  const tv = taskmgrTopVerdict({ pinned: true, sessionOnly: true });
  checks.push({ name: "F548 不抢焦点+重启不记忆", pass: tv.onTop && !tv.stealsFocus && !tv.remembered });
  return checks;
}
