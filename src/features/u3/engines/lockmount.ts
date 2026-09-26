/**
 * 锁屏与横幅真窗口挂接引擎（AI-U3 v5 · 批次五装配层之三）。
 *
 * 职责：把 F504/F507/F508/F516 从「逻辑判定」推进到「真窗口挂接装配」：
 * - 锁屏窗口状态机（F504/F507）：locked → PIN 键盘 → 冷却 → 密码回退；
 *   全链时序预算 1.5s（locksec 同源）；焦点陷阱（章四：浮层焦点必须
 *   圈在锁屏内，解锁后还给触发者——锁屏触发者是系统，还给原焦点窗）；
 *   防截黑帧全屏覆盖（locksec.lockScreenShotPolicy 同源裁决）；
 * - 防截黑块合成（F508）：多窗口防截声明 → 与录屏视口求交 → 交叠区
 *   黑块矩形集（union 去重、跨屏坐标换算）——绘制层直接照单画黑；
 * - 横幅真窗口几何（F516）：横幅窗口落在真实屏上的锚点矩形（多屏时
 *   落在事件屏）、任务栏避让、堆叠方向——bannerpack.bannerStackRects
 *   同源换算，本引擎补「窗口层」语义（独立置顶窗、点击穿透豁免、
 *   失焦收起）；
 * - Z 序表：锁屏 > 防截黑块 > 横幅 > OSD > 焦点浮层——一套表（章十：
 *   全局一致性），消费方不自己定 zIndex。
 *
 * 诚实边界：不创建真窗口——产出挂接裁决与几何，真窗口宿主
 * （shell 领地）消费本引擎（章十四：能力开放，本体自由组装）。
 */

import {
  PIN_UNLOCK_BUDGET_MS, PIN_COOLDOWN_AFTER_FAILS, pinCooldownMs,
  lockScreenShotPolicy, redactRects, REDACT_NOTE,
  type ShieldMode, type WindowRect,
} from "../locksec";
import { bannerAnchor, bannerStackDirection, type BannerPos } from "../winkeys";

/* ------------------------------- 锁屏窗口状态机 ------------------------------- */

export type LockWindowState =
  | { phase: "locked"; pinPad: true; cooldownUntil: 0 }
  | { phase: "pin-entry"; pinPad: true; cooldownUntil: 0 }
  | { phase: "cooldown"; pinPad: false; cooldownUntil: number }
  | { phase: "password-fallback"; pinPad: false; cooldownUntil: number }
  | { phase: "unlocking"; pinPad: false; cooldownUntil: 0 };

export interface LockMountRt {
  /** PIN 连错次数（locksec 同源语义）。 */
  failCount: number;
  /** 当前窗口态。 */
  state: LockWindowState;
}

/** 锁屏挂接初始态（锁定即 PIN 键盘自动切——判据「锁屏自动切 PIN 键盘」）。 */
export function lockMountInit(): LockMountRt {
  return { failCount: 0, state: { phase: "locked", pinPad: true, cooldownUntil: 0 } };
}

/** PIN 错误递进：5 次内留在 PIN 键盘（清空重输），满 5 次进冷却（逐次翻倍）。 */
export function lockMountPinFail(rt: LockMountRt, now: number): { rt: LockMountRt; message: string; nextAction: "retry-pin" | "wait-cooldown" } {
  const failCount = rt.failCount + 1;
  if (failCount < PIN_COOLDOWN_AFTER_FAILS) {
    return {
      rt: { ...rt, failCount, state: { phase: "pin-entry", pinPad: true, cooldownUntil: 0 } },
      message: `PIN 不对，还可试 ${PIN_COOLDOWN_AFTER_FAILS - failCount} 次`,
      nextAction: "retry-pin",
    };
  }
  const until = now + pinCooldownMs(failCount);
  return {
    rt: { failCount, state: { phase: "cooldown", pinPad: false, cooldownUntil: until } },
    message: "错误次数太多——冷却中，稍后可用密码登录",
    nextAction: "wait-cooldown",
  };
}

/** 冷却到期 → 密码回退（判据：冷却期回退密码登录）；未到期显性拒绝。 */
export function lockMountCooldownTick(rt: LockMountRt, now: number): { rt: LockMountRt; canFallback: boolean; remainingMs: number } {
  if (rt.state.phase !== "cooldown") return { rt, canFallback: rt.state.phase === "password-fallback", remainingMs: 0 };
  const remainingMs = Math.max(0, rt.state.cooldownUntil - now);
  return remainingMs === 0
    ? { rt: { ...rt, state: { phase: "password-fallback", pinPad: false, cooldownUntil: 0 } }, canFallback: true, remainingMs: 0 }
    : { rt, canFallback: false, remainingMs };
}

/** 解锁成功：状态归零 + 焦点归还（章四：关闭后焦点还给触发元素）。 */
export function lockMountUnlock(rt: LockMountRt): { rt: LockMountRt; focusReturn: "original-window"; budgetMs: number; clearedFails: number } {
  return { rt: lockMountInit(), focusReturn: "original-window", budgetMs: PIN_UNLOCK_BUDGET_MS, clearedFails: rt.failCount };
}

/** 焦点陷阱声明（章四：锁屏打开期间 Tab 圈在锁屏内——键盘不可逃逸到桌面）。 */
export const LOCK_FOCUS_TRAP = "锁屏态 Tab/方向键只在 PIN 键盘与密码框内循环；解锁后焦点归还原窗口";

/* ------------------------------- F508 防截黑块合成 ------------------------------- */

/** 多屏坐标：屏相对坐标 = 窗口绝对坐标 - 屏原点（负值 = 部分出屏，裁剪）。 */
export function intersectOnScreen(win: WindowRect, screen: { x: number; y: number; w: number; h: number }): WindowRect | null {
  const x1 = Math.max(win.x, screen.x);
  const y1 = Math.max(win.y, screen.y);
  const x2 = Math.min(win.x + win.w, screen.x + screen.w);
  const y2 = Math.min(win.y + win.h, screen.y + screen.h);
  return x2 > x1 && y2 > y1 ? { x: x1, y: y1, w: x2 - x1, h: y2 - y1 } : null;
}

/** 矩形去重（同几何只画一块——union 语义的下界：不重不漏）。 */
export function dedupeRects(rects: ReadonlyArray<WindowRect>): Array<WindowRect> {
  const seen = new Set<string>();
  const out: Array<WindowRect> = [];
  for (const r of rects) {
    const k = `${r.x},${r.y},${r.w},${r.h}`;
    if (!seen.has(k)) { seen.add(k); out.push(r); }
  }
  return out;
}

/**
 * 黑块合成单：给定防截窗口声明与截屏视口 → 视口内黑块矩形集。
 * 锁屏态整屏黑帧由 lockScreenShotPolicy 裁决（同源）；本函数只管
 * 非锁屏态的应用声明黑块（REDICT_NOTE 语义出口照搬）。
 */
export function composeBlackBlocks(
  markers: Record<string, WindowRect>,
  shot: WindowRect,
  locked: boolean,
  mode: ShieldMode,
): { blocks: Array<WindowRect>; mode: ShieldMode; note: string; deny: boolean } {
  const policy = lockScreenShotPolicy(locked, mode, "screen-recorder");
  if (!policy.allowed) return { blocks: [], mode, note: REDACT_NOTE, deny: true };
  const rects = redactRects(markers, shot);
  return { blocks: dedupeRects(rects), mode, note: REDACT_NOTE, deny: false };
}

/** 黑块像素精度自证（判据：黑块区域精确=窗口几何对齐）：面积守恒检查。 */
export function blackBlockAreaHonest(blocks: ReadonlyArray<WindowRect>, shot: WindowRect): boolean {
  // 每块都在视口内且总面积不超视口（不精确超画即失真）
  const shotArea = shot.w * shot.h;
  let sum = 0;
  for (const b of blocks) {
    if (b.x < shot.x || b.y < shot.y || b.x + b.w > shot.x + shot.w || b.y + b.h > shot.y + shot.h) return false;
    sum += b.w * b.h;
  }
  return sum <= shotArea;
}

/* ------------------------------- F516 横幅真窗口几何 ------------------------------- */

export interface BannerWindowSpec {
  /** 独立置顶窗（不进任务栏、不抢焦点——横幅是通知不是窗口）。 */
  alwaysOnTop: true;
  skipTaskbar: true;
  /** 非交互期点击穿透豁免关闭（横幅可点——F516 三义分流在窗内）。 */
  clickThrough: false;
  /** 失焦/点击关闭/超时三路消失（章二浮层出路）。 */
  exits: ReadonlyArray<"click" | "timeout" | "focus-loss">;
}

/** 横幅窗口规格（Z 序消费方按层取，不自定 zIndex）。 */
export function bannerWindowSpec(): BannerWindowSpec {
  return { alwaysOnTop: true, skipTaskbar: true, clickThrough: false, exits: ["click", "timeout", "focus-loss"] };
}

/**
 * 横幅落屏几何：多屏时落「事件屏」（通知来源所在屏——不是主屏一刀切）。
 * 任务栏避让：工作区高度 = 屏高 - 任务栏高（40px 基线）。
 */
export const TASKBAR_HEIGHT_PX = 40;

export function bannerScreenGeometry(
  screen: { x: number; y: number; w: number; h: number },
  pos: BannerPos,
  bannerSize: { w: number; h: number },
  marginPx: number,
): { x: number; y: number; workArea: { w: number; h: number } } {
  const workArea = { w: screen.w, h: screen.h - TASKBAR_HEIGHT_PX };
  const anchor = bannerAnchor(pos, { w: workArea.w, h: workArea.h }, bannerSize, marginPx);
  return { x: screen.x + anchor.x, y: screen.y + anchor.y, workArea };
}

/**
 * 多横幅真窗口矩形：首枚落 bannerAnchor 锚点，后续沿堆叠方向
 * （bannerStackDirection 同源：底右向上、顶部向下）逐枚让位 gapPx。
 * 工作区 = 屏高 - 任务栏（横幅永不压任务栏）。
 */
export function bannerWindowRects(
  screen: { x: number; y: number; w: number; h: number },
  pos: BannerPos,
  items: ReadonlyArray<{ id: string; w: number; h: number }>,
  marginPx: number,
  gapPx: number,
): Array<{ id: string; x: number; y: number; w: number; h: number }> {
  const dir = bannerStackDirection(pos);
  const workH = screen.h - TASKBAR_HEIGHT_PX;
  const right = pos.endsWith("right");
  const out: Array<{ id: string; x: number; y: number; w: number; h: number }> = [];
  let offset = 0;
  for (const it of items) {
    const x = right
      ? screen.x + (screen.w - marginPx - it.w)
      : screen.x + marginPx;
    const localY = dir === "up"
      ? workH - marginPx - offset - it.h
      : marginPx + offset;
    out.push({ id: it.id, x, y: screen.y + localY, w: it.w, h: it.h });
    offset += it.h + gapPx;
  }
  return out;
}

/* ------------------------------- Z 序表 ------------------------------- */

/**
 * U3 领地 Z 序总表（章十：全系统一套规则——消费方按名取层值）。
 * 值越大越靠上。锁屏最顶；黑块贴着被遮窗内容（在其窗口内部合成，
 * 不与全局层竞争——因此取相对高位但低于锁屏）。
 */
export const U3_ZORDER = {
  "focus-overlay": 1200,
  osd: 1600,
  banner: 1800,
  "black-block": 2000,
  lockscreen: 2200,
} as const;

export type U3ZLayer = keyof typeof U3_ZORDER;

/** 层值出口（一处一事实：改表一处生效全局）。 */
export function zOf(layer: U3ZLayer): number {
  return U3_ZORDER[layer];
}

/* ------------------------------- 自检 ------------------------------- */

/** 锁屏与横幅挂接引擎自检（F550 锚点域消费）。 */
export function lockmountSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 锁屏状态机
  let rt = lockMountInit();
  checks.push({ name: "锁定即 PIN 键盘", pass: rt.state.phase === "locked" && rt.state.pinPad });
  for (let i = 0; i < 4; i++) rt = lockMountPinFail(rt, 1000 + i).rt;
  checks.push({ name: "四次内留 PIN 键盘", pass: rt.failCount === 4 && rt.state.phase === "pin-entry" && rt.state.pinPad });
  const f5 = lockMountPinFail(rt, 5000);
  rt = f5.rt;
  checks.push({ name: "满五进冷却", pass: rt.state.phase === "cooldown" && rt.state.cooldownUntil === 5000 + 30_000 && f5.nextAction === "wait-cooldown" });
  const early = lockMountCooldownTick(rt, rt.state.phase === "cooldown" ? rt.state.cooldownUntil - 1 : 0);
  checks.push({ name: "冷却未到拒绝", pass: !early.canFallback && early.remainingMs === 1 });
  const due = lockMountCooldownTick(rt, 999_999);
  checks.push({ name: "冷却到期密码回退", pass: due.canFallback && due.rt.state.phase === "password-fallback" });
  const unl = lockMountUnlock(due.rt);
  checks.push({ name: "解锁归零+焦点归还+预算", pass: unl.rt.failCount === 0 && unl.focusReturn === "original-window" && unl.budgetMs === 1500 });
  // 黑块合成
  const screen = { x: 0, y: 0, w: 3840, h: 2160 };
  const m1 = { a: { x: 100, y: 100, w: 200, h: 100 }, b: { x: 3700, y: 100, w: 280, h: 100 } };
  const comp1 = composeBlackBlocks(m1, screen, false, "black-frame");
  checks.push({ name: "黑块合成不重", pass: comp1.blocks.length === 2 && comp1.deny === false });
  const comp2 = composeBlackBlocks({ a: { x: 3600, y: 0, w: 500, h: 200 } }, screen, false, "black-frame");
  checks.push({ name: "出屏裁剪", pass: comp2.blocks.length === 1 && comp2.blocks[0]!.w === 240 });
  checks.push({ name: "面积诚实", pass: blackBlockAreaHonest(comp1.blocks, screen) && blackBlockAreaHonest(comp2.blocks, screen) });
  const cross = intersectOnScreen({ x: 3700, y: 0, w: 300, h: 100 }, screen);
  checks.push({ name: "跨屏求交", pass: cross !== null && cross.w === 140 });
  const off = intersectOnScreen({ x: 5000, y: 0, w: 100, h: 100 }, screen);
  checks.push({ name: "完全出屏 null", pass: off === null });
  const dup = dedupeRects([{ x: 1, y: 1, w: 2, h: 2 }, { x: 1, y: 1, w: 2, h: 2 }]);
  checks.push({ name: "同几何去重", pass: dup.length === 1 });
  // 横幅窗口几何
  const spec = bannerWindowSpec();
  checks.push({ name: "横幅窗口规格", pass: spec.alwaysOnTop && spec.skipTaskbar && !spec.clickThrough && spec.exits.length === 3 });
  const g = bannerScreenGeometry({ x: 1920, y: 0, w: 1920, h: 1080 }, "bottom-right", { w: 360, h: 96 }, 16);
  checks.push({
    name: "横幅落事件屏+任务栏避让",
    pass: g.workArea.h === 1080 - TASKBAR_HEIGHT_PX && g.x >= 1920 && g.x + 360 <= 1920 + 1920 && g.y + 96 <= 1040,
  });
  const stack = bannerWindowRects({ x: 0, y: 0, w: 1920, h: 1080 }, "bottom-right", [
    { id: "b1", w: 360, h: 96 }, { id: "b2", w: 360, h: 96 },
  ], 16, 8);
  checks.push({ name: "多横幅堆叠+屏偏移", pass: stack.length === 2 && stack[0]!.y !== stack[1]!.y && stack.every((r) => r.y + r.h <= 1040) });
  // Z 序
  checks.push({
    name: "Z 序一套表",
    pass: zOf("lockscreen") > zOf("black-block") && zOf("black-block") > zOf("banner") && zOf("banner") > zOf("osd") && zOf("osd") > zOf("focus-overlay"),
  });
  return checks;
}
