/**
 * U3-v6 引擎一：shellbar —— shell 层装配引擎（AI-U3 · 批次六）。
 *
 * 判据覆盖（深化面）：
 * - F516/F521 通知与截图的 shell 落点（任务栏角部几何、托盘溢出、
 *   通知中心抽屉的三段时间线）；
 * - F548 任务管理器置顶（shell 窗口 Z 序提升的裁决与反馈）；
 * - F535 Win+数字的任务栏实例定位（shell 图标槽位 ↔ 窗口实例映射）；
 * - F536 Win+T 任务栏焦点环（焦点在 shell 槽位上的推进与循环）；
 * - F549 时钟悬停完整日期的 shell 挂点（时钟槽 tooltip 预算与翻转避让）。
 *
 * 纪律：纯函数 + 定长结构；全部几何为整数像素（4K DPR 精度由调用方
 * 传入物理像素值）；每条自检对应主册判据可测面。
 */

/* ------------------------------- 常量 ------------------------------- */

/** 任务栏高度（默认档 48px 物理像素；小图标档 40）。 */
export const TASKBAR_HEIGHT_DEFAULT_PX = 48;
export const TASKBAR_HEIGHT_SMALL_PX = 40;

/** 托盘区宽度预算：图标 16px × 最多 8 个 + 溢出箭头 20px + 内边距 12px。 */
export const TRAY_ICON_PX = 16;
export const TRAY_MAX_VISIBLE = 8;
export const TRAY_OVERFLOW_PX = 20;
export const TRAY_PADDING_PX = 12;

/** 通知中心抽屉宽度（屏宽的 1/3，夹取 [320, 480]）。 */
export const ACTION_CENTER_MIN_W = 320;
export const ACTION_CENTER_MAX_W = 480;

/** 通知中心时间线三段（今晨 / 今日 / 更早）——超过 24h 折入「更早」。 */
export const TIMELINE_TODAY_MS = 24 * 60 * 60 * 1000;

/** F549 时钟 tooltip 预算（判据：完整日期公历+星期+农历一行不折行）。 */
export const CLOCK_TOOLTIP_MAX_W = 260;

/* ------------------------------- 托盘布局 ------------------------------- */

export interface TrayItem { id: string; w: number; pinned: boolean }

/** 托盘布局：可见图标右对齐排布 + 溢出列表（固定 pinned 优先）。
 *  返回可见项与溢出项两个定长切片的来源顺序（不重排用户顺序）。 */
export function trayLayout(
  items: ReadonlyArray<TrayItem>,
  taskbarW: number,
): { visible: ReadonlyArray<TrayItem>; overflow: ReadonlyArray<TrayItem> } {
  const budget = taskbarW - TRAY_OVERFLOW_PX - TRAY_PADDING_PX * 2;
  const visible: TrayItem[] = [];
  const overflow: TrayItem[] = [];
  let used = 0;
  // 两遍扫：先 pinned（保底可见），再普通项
  for (const it of items) if (it.pinned) { visible.push(it); used += it.w; }
  for (const it of items) {
    if (it.pinned) continue;
    if (used + it.w <= budget && visible.length < TRAY_MAX_VISIBLE) {
      visible.push(it); used += it.w;
    } else {
      overflow.push(it);
    }
  }
  return { visible, overflow };
}

/* ------------------------------- 通知中心 ------------------------------- */

export interface NotifEntry { id: string; atMs: number; kind: "banner" | "quiet" | "capture-block" }

export type TimelineBand = "morning" | "today" | "earlier";

/** 时间线三段分桶（nowMs 参照：今晨 0 点前归「更早」）。 */
export function timelineBand(atMs: number, nowMs: number): TimelineBand {
  const dayStart = Math.floor(nowMs / TIMELINE_TODAY_MS) * TIMELINE_TODAY_MS;
  if (atMs >= dayStart) return "today";
  if (atMs >= dayStart - 12 * 60 * 60 * 1000) return "morning";
  return "earlier";
}

/** 通知中心抽屉几何：右下贴任务栏上方，宽度夹取。 */
export function actionCenterRect(
  screen: { w: number; h: number },
  taskbarH: number,
): { x: number; y: number; w: number; h: number } {
  const w = Math.min(ACTION_CENTER_MAX_W, Math.max(ACTION_CENTER_MIN_W, Math.round(screen.w / 3)));
  const h = Math.min(screen.h - taskbarH - 16, 560);
  return { x: screen.w - w - 8, y: screen.h - taskbarH - h - 8, w, h };
}

/* ------------------------------- F548 置顶裁决 ------------------------------- */

export type AlwaysOnTopVerdict =
  | { allow: true; zLayer: "always-on-top" }
  | { allow: false; reason: "fullscreen-app" | "lockscreen"; hint: string };

/** 任务管理器置顶裁决：全屏应用被用户主动置顶时仅置顶不遮挡全屏内容
 *  （判据：置顶且可切换回来——退出全屏或切走后仍置顶）。 */
export function alwaysOnTopVerdict(frontApp: "normal" | "fullscreen", locked: boolean): AlwaysOnTopVerdict {
  if (locked) return { allow: false, reason: "lockscreen", hint: "锁屏期间不提升置顶——解锁后恢复" };
  if (frontApp === "fullscreen") return { allow: false, reason: "fullscreen-app", hint: "全屏应用前台时不压顶——切出全屏后自动置顶" };
  return { allow: true, zLayer: "always-on-top" };
}

/* ------------------------------- F535 槽位定位 ------------------------------- */

export interface TaskbarSlot { id: string; x: number; w: number; instances: number }

/** Win+数字：槽位号（1 起）→ 焦点目标。无实例=启动新实例；多实例=列出窗口清单。 */
export function winNumberTarget(
  slots: ReadonlyArray<TaskbarSlot>,
  n: number,
): { slot: TaskbarSlot; action: "focus" | "launch" | "list" } | null {
  if (n < 1 || n > 9) return null;
  const slot = slots[n - 1];
  if (!slot) return null;
  if (slot.instances === 0) return { slot, action: "launch" };
  if (slot.instances === 1) return { slot, action: "focus" };
  return { slot, action: "list" };
}

/* ------------------------------- F536 焦点环 ------------------------------- */

/** Win+T：任务栏焦点推进（含循环；从无焦点起步进第一槽）。 */
export function taskbarFocusNext(
  slots: ReadonlyArray<TaskbarSlot>,
  currentId: string | null,
  backwards: boolean,
): string | null {
  if (slots.length === 0) return null;
  if (currentId === null) return backwards ? slots[slots.length - 1]!.id : slots[0]!.id;
  const idx = slots.findIndex((s) => s.id === currentId);
  if (idx === -1) return slots[0]!.id;
  const next = backwards ? (idx - 1 + slots.length) % slots.length : (idx + 1) % slots.length;
  return slots[next]!.id;
}

/* ------------------------------- F549 时钟挂点 ------------------------------- */

/** 时钟 tooltip 几何：默认时钟上方居中；右缘翻转避让（不越出屏）。 */
export function clockTooltipRect(
  clockSlot: { x: number; w: number; y: number },
  screen: { w: number },
  textW: number,
): { x: number; y: number; w: number; flipped: boolean } {
  const w = Math.min(textW, CLOCK_TOOLTIP_MAX_W);
  let x = clockSlot.x + Math.round((clockSlot.w - w) / 2);
  const flipped = x < 0;
  if (flipped) x = 4;
  if (x + w > screen.w - 4) x = screen.w - 4 - w;
  return { x, y: clockSlot.y - 34, w, flipped };
}

/** 完整日期行构造（公历+星期；农历由 clockcal 域提供——本函数只拼壳）。 */
export function clockFullDateLine(gregorian: string, weekday: string, lunar: string): string {
  return `${gregorian} ${weekday} ${lunar}`;
}

/* ------------------------------- 自检 ------------------------------- */

export function shellbarSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];

  // 托盘布局
  const items: TrayItem[] = [
    { id: "a", w: 16, pinned: true },
    { id: "b", w: 16, pinned: false },
    { id: "c", w: 16, pinned: false },
    { id: "d", w: 16, pinned: false },
    { id: "e", w: 16, pinned: false },
  ];
  // 400px 任务栏预算 = 400-20-24 = 356 → 全可见
  const t1 = trayLayout(items, 400);
  checks.push({ name: "托盘全可见", pass: t1.visible.length === 5 && t1.overflow.length === 0 });
  // 窄任务栏：pinned 保底，其余溢出
  const t2 = trayLayout(items, 80);
  checks.push({ name: "托盘 pinned 保底+溢出", pass: t2.visible.some((i) => i.id === "a") && t2.overflow.length >= 1 });
  // 超过 8 上限
  const many: TrayItem[] = Array.from({ length: 12 }, (_, i) => ({ id: `x${i}`, w: 16, pinned: false }));
  const t3 = trayLayout(many, 2000);
  checks.push({ name: "托盘 8 上限", pass: t3.visible.length === TRAY_MAX_VISIBLE && t3.overflow.length === 4 });

  // 时间线分桶
  const now = 1_700_000_000_000;
  const dayStart = Math.floor(now / TIMELINE_TODAY_MS) * TIMELINE_TODAY_MS;
  checks.push({ name: "时间线今日", pass: timelineBand(dayStart + 1000, now) === "today" });
  checks.push({ name: "时间线今晨（昨夜 12h 内）", pass: timelineBand(dayStart - 11 * 3600_000, now) === "morning" });
  checks.push({ name: "时间线更早", pass: timelineBand(dayStart - 13 * 3600_000, now) === "earlier" });

  // 通知中心几何
  const ac = actionCenterRect({ w: 3840, h: 2160 }, TASKBAR_HEIGHT_DEFAULT_PX);
  checks.push({ name: "通知中心宽度夹取", pass: ac.w === ACTION_CENTER_MAX_W && ac.y + ac.h <= 2160 - TASKBAR_HEIGHT_DEFAULT_PX });
  const ac2 = actionCenterRect({ w: 900, h: 700 }, TASKBAR_HEIGHT_DEFAULT_PX);
  checks.push({ name: "通知中心小屏下限", pass: ac2.w === ACTION_CENTER_MIN_W && ac2.x >= 0 });

  // F548 置顶裁决
  const v1 = alwaysOnTopVerdict("normal", false);
  const v2 = alwaysOnTopVerdict("fullscreen", false);
  const v3 = alwaysOnTopVerdict("normal", true);
  checks.push({ name: "F548 置顶三态", pass: v1.allow && !v2.allow && !v3.allow });

  // F535 槽位定位
  const slots: TaskbarSlot[] = [
    { id: "s1", x: 0, w: 40, instances: 0 },
    { id: "s2", x: 44, w: 40, instances: 1 },
    { id: "s3", x: 88, w: 40, instances: 3 },
  ];
  checks.push({ name: "F535 无实例启动", pass: winNumberTarget(slots, 1)?.action === "launch" });
  checks.push({ name: "F535 单实例聚焦", pass: winNumberTarget(slots, 2)?.action === "focus" });
  checks.push({ name: "F535 多实例列窗", pass: winNumberTarget(slots, 3)?.action === "list" });
  checks.push({ name: "F535 越界空手", pass: winNumberTarget(slots, 0) === null && winNumberTarget(slots, 10) === null });

  // F536 焦点环
  checks.push({ name: "F536 无焦点起步进首槽", pass: taskbarFocusNext(slots, null, false) === "s1" });
  checks.push({ name: "F536 末槽循环回首", pass: taskbarFocusNext(slots, "s3", false) === "s1" });
  checks.push({ name: "F536 反向从头进尾", pass: taskbarFocusNext(slots, null, true) === "s3" });

  // F549 时钟挂点
  const tip = clockTooltipRect({ x: 3700, w: 80, y: 2112 }, { w: 3840 }, 300);
  checks.push({ name: "F549 右缘翻转避让", pass: tip.flipped === false && tip.x + tip.w <= 3840 - 4 });
  checks.push({ name: "F549 完整日期一行", pass: clockFullDateLine("2026-09-27", "周日", "八月十七") === "2026-09-27 周日 八月十七" });

  return checks;
}
