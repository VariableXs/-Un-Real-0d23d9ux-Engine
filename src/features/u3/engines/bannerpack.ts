/**
 * F516/F519/F520 深化引擎 · 通知横幅布局、大写提示音、标题栏中键（AI-U3 · bannerpack）。
 *
 * 判据唯一源（主册摘文）：
 * - F516「三档位置；堆叠方向自适应；OSD 独立性；主屏固定；切换即时与持久化」。
 * - F519「双音色区分；默认关；跟随提示音量；与 F230 视觉互补；开关持久化」。
 * - F520「中键最小化行为；三义分流矩阵；与拖拽冲突（中键拖拽无定义即无冲突）；
 *   开关（不喜可关）；动画 F124」。
 *
 * 深化点：
 * 1. 横幅布局器：三档位置 → 堆叠方向与新横幅插入点**推导**（右下=向上堆、
 *    右上=向下堆……），不是六份 if——方向是位置的函数。
 * 2. OSD 独立性：OSD 与通知横幅各自的槽位账，互不占位（同源不同池）。
 * 3. 双音色分类器：Caps On/Off 两种音色选择器 + 音量跟随曲线。
 * 4. 三义分流矩阵：标题栏中键按修饰键/区域分流（最小化/关闭组/无定义显性
 *    no-op），矩阵即表，测试逐格。
 */

/* ------------------------------ F516 横幅布局 ------------------------------ */

export type BannerPosition = "top-left" | "top-right" | "bottom-right" | "bottom-left";
/** 判据「三档位置」——三档语义档映射到四角（默认档+两自定义档）。 */
export const BANNER_PRESETS: Record<string, BannerPosition> = {
  "右下（默认）": "bottom-right",
  "右上": "top-right",
  "左下": "bottom-left",
};

export interface BannerSlot {
  id: string;
  title: string;
  body: string;
}

export interface BannerLayout {
  position: BannerPosition;
  /** 堆叠方向：新横幅从边缘向屏幕内侧生长。 */
  stackDirection: "down" | "up";
  /** 插入点：新横幅贴边。 */
  insertAtEdge: boolean;
}

/** 位置 → 布局推导（判据「堆叠方向自适应」的函数化）。 */
export function bannerLayout(position: BannerPosition): BannerLayout {
  const fromTop = position.startsWith("top");
  return {
    position,
    stackDirection: fromTop ? "down" : "up",
    insertAtEdge: true,
  };
}

/**
 * 堆叠矩形计算（判据「切换即时」的布局层）：给定槽位序与横幅尺寸，输出
 * 逐槽矩形（贴近边缘、沿堆叠方向排列）。
 */
export function bannerStackRects(
  layout: BannerLayout,
  items: Array<{ id: string; w: number; h: number }>,
  marginPx: number,
  gapPx: number,
  screenW: number,
  screenH: number,
): Array<{ id: string; x: number; y: number; w: number; h: number }> {
  const out: Array<{ id: string; x: number; y: number; w: number; h: number }> = [];
  let offset = marginPx;
  const right = layout.position.endsWith("right");
  for (const it of items) {
    const x = right ? screenW - marginPx - it.w : marginPx;
    // offset 自 marginPx 起算：down 直接作为上边距；up 从屏幕底边扣除
    const y = layout.stackDirection === "down" ? offset : screenH - offset - it.h;
    out.push({ id: it.id, x, y, w: it.w, h: it.h });
    offset += it.h + gapPx;
  }
  return out;
}

/** OSD 独立性（判据：OSD 独立性）：OSD 有自己的固定槽（底部中央），与横幅池无关。 */
export function osdSlot(screenW: number, screenH: number, osdW: number, osdH: number, marginPx: number) {
  return { x: Math.round((screenW - osdW) / 2), y: screenH - marginPx - osdH, pool: "osd" as const };
}

/* ------------------------------ F519 双音色 ------------------------------ */

export type CapsTone = "on" | "off";

/** 双音色频率对（Hz——On 上行音 / Off 下行音，听感可区分）。 */
export const CAPS_TONE_HZ: Record<CapsTone, number> = { on: 880, off: 587 };
/** 默认关（判据「默认关」）。 */
export const CAPS_SOUND_DEFAULT_ENABLED = false;

export function capsToneFor(capsOn: boolean): CapsTone {
  return capsOn ? "on" : "off";
}

/** 音量跟随曲线（判据「跟随提示音量」）：master 0-100 → 事件音量同比例，不低于可闻下限 8。 */
export function capsVolumeFollow(masterVolume: number): number {
  const v = Math.min(100, Math.max(0, masterVolume));
  return v === 0 ? 0 : Math.max(8, Math.round(v));
}

/* ------------------------------ F520 三义分流 ------------------------------ */

export type MidClickZone = "titlebar" | "icon" | "tab";
export type MidClickModifier = "none" | "shift" | "ctrl";

export type MidClickOutcome =
  | { action: "minimize" }
  | { action: "close-group" }
  | { action: "new-tab" }
  | { action: "noop"; reason: string };

/**
 * 三义分流矩阵（判据「三义分流矩阵」）：
 * - 标题栏+无修饰 → 最小化（本功能主体）
 * - 任务栏图标+无修饰 → 关闭组（既有三义之一）
 * - 标签条+无修饰 → 新建标签（既有三义之一）
 * - Shift/Ctrl 修饰 → 显性 no-op（修饰键留给其他语义，不许吞）
 */
export function midClickRoute(zone: MidClickZone, mod: MidClickModifier, enabled: boolean): MidClickOutcome {
  if (mod !== "none") return { action: "noop", reason: `${mod}+中键未定义动作——修饰键组合不吞事件` };
  switch (zone) {
    case "titlebar": return enabled ? { action: "minimize" } : { action: "noop", reason: "中键最小化已关闭（F520 开关）" };
    case "icon": return { action: "close-group" };
    case "tab": return { action: "new-tab" };
  }
}

/* ------------------------------ 自检 ------------------------------ */

export function bannerpackSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 三档位置预设
  checks.push({ name: "F516 三档预设", pass: Object.keys(BANNER_PRESETS).length === 3 && BANNER_PRESETS["右下（默认）"] === "bottom-right" });
  // 堆叠方向推导
  checks.push({ name: "F516 堆叠方向自适应", pass: bannerLayout("top-right").stackDirection === "down" && bannerLayout("bottom-right").stackDirection === "up" });
  // 堆叠矩形：右下两横幅向上生长
  const rects = bannerStackRects(bannerLayout("bottom-right"), [
    { id: "a", w: 320, h: 80 },
    { id: "b", w: 320, h: 80 },
  ], 12, 8, 1920, 1080);
  const a = rects[0]!;
  const b = rects[1]!;
  checks.push({ name: "F516 贴边堆叠几何", pass: a.x === 1920 - 12 - 320 && a.y === 1080 - 12 - 80 && b.y === a.y - 88 && a.x === b.x });
  // OSD 独立槽
  const osd = osdSlot(1920, 1080, 200, 60, 12);
  checks.push({ name: "F516 OSD 独立", pass: osd.pool === "osd" && osd.x === 860 && osd.y === 1008 });
  // 双音色 + 音量跟随
  checks.push({ name: "F519 双音色", pass: capsToneFor(true) === "on" && capsToneFor(false) === "off" && CAPS_TONE_HZ.on !== CAPS_TONE_HZ.off });
  checks.push({ name: "F519 音量跟随下限", pass: capsVolumeFollow(50) === 50 && capsVolumeFollow(3) === 8 && capsVolumeFollow(0) === 0 });
  checks.push({ name: "F519 默认关", pass: CAPS_SOUND_DEFAULT_ENABLED === false });
  // 三义矩阵逐格
  const r1 = midClickRoute("titlebar", "none", true);
  const r2 = midClickRoute("icon", "none", true);
  const r3 = midClickRoute("tab", "none", true);
  const r4 = midClickRoute("titlebar", "shift", true);
  const r5 = midClickRoute("titlebar", "none", false);
  checks.push({ name: "F520 三义矩阵", pass: r1.action === "minimize" && r2.action === "close-group" && r3.action === "new-tab" });
  checks.push({ name: "F520 修饰键不吞+开关", pass: r4.action === "noop" && r5.action === "noop" });
  return checks;
}
