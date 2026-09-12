import type { VwmRect } from "../vwm";

/**
 * AURORA-10000 · 会话层（AI-06/07）：
 * - 族0030 多显示器编排（F00726~F00750）→ 显示器画像与排布方案
 * - 族0034 窗口状态持久化（F00826~F00850）→ 会话快照 schema 与恢复
 * - 族0035 窗口性能与降级（F00851~F00875）→ 预算档与降级阶梯
 * 纯函数/纯数据，可直接单测；落盘由调用方（localStorage/Tauri store）承担。
 */

// ---------- 族0030 多显示器编排 ----------

export interface MonitorProfile {
  id: string;
  /** 身份命名。 */
  name: string;
  /** 角色：main/extend/board。 */
  role: "main" | "extend" | "board";
  /** 任务栏策略。 */
  taskbar: "main" | "all" | "none";
  /** 独立壁纸。 */
  wallpaper: string | null;
  /** 独立 DPI。 */
  scale: number;
  /** 夜灯单屏。 */
  nightLight: boolean;
  /** HDR 单屏。 */
  hdr: boolean;
  /** 竖屏。 */
  portrait: boolean;
}

/** 族0030：25 种多屏编排档（ID 升序）。 */
export const MONITOR_PRESETS: readonly (MonitorProfile & { id: string })[] = Object.freeze([
  { id: "F00726", name: "主屏", role: "main", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00727", name: "壁纸独立", role: "extend", taskbar: "none", wallpaper: "custom", scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00728", name: "任务栏三档", role: "extend", taskbar: "all", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00729", name: "拖拽曲线", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00730", name: "热角互通", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00731", name: "光标对齐", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00732", name: "热适配", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00733", name: "混合刷新", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00734", name: "HDR 单屏", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: true, portrait: false },
  { id: "F00735", name: "夜灯单屏", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: true, hdr: false, portrait: false },
  { id: "F00736", name: "演示一键", role: "board", taskbar: "none", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00737", name: "副屏看板", role: "board", taskbar: "none", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00738", name: "竖屏优化", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: true },
  { id: "F00739", name: "带鱼三分", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00740", name: "排列编辑", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00741", name: "克隆扩展", role: "main", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00742", name: "断开恢复", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00743", name: "独立休眠", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00744", name: "逐屏校色", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00745", name: "逐屏缩放", role: "extend", taskbar: "main", wallpaper: null, scale: 1.25, nightLight: false, hdr: false, portrait: false },
  { id: "F00746", name: "窗口追踪", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00747", name: "切换动画", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00748", name: "视频墙", role: "board", taskbar: "none", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00749", name: "健康监控", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
  { id: "F00750", name: "方案快照", role: "extend", taskbar: "main", wallpaper: null, scale: 1, nightLight: false, hdr: false, portrait: false },
]);

/** 跨屏光标对齐：把 y 从一块屏映射到另一块（F00731）。 */
export function mapCursorY(y: number, from: VwmRect, to: VwmRect): number {
  const t = from.h <= 1 ? 0 : (y - from.y) / from.h;
  return Math.round(to.y + Math.min(1, Math.max(0, t)) * to.h);
}

/** 带鱼屏三分（F00739）。 */
export function ultrawideThirds(work: VwmRect): VwmRect[] {
  const w = Math.round(work.w / 3);
  return [0, 1, 2].map((i) => ({ x: work.x + w * i, y: work.y, w: i === 2 ? work.w - w * 2 : w, h: work.h }));
}

// ---------- 族0034 窗口状态持久化 ----------

/** 单窗快照。 */
export interface WinSnapshot {
  win: string;
  rect: VwmRect;
  minimized: boolean;
  maximized: boolean;
  vdesk: number;
  group: number | null;
}

export interface SessionSnapshot {
  version: 1;
  savedAt: number;
  vdesks: number;
  activeVdesk: number;
  wins: WinSnapshot[];
}

/** 序列化：会话 → 快照（F00826）。 */
export function saveSession(wins: WinSnapshot[], vdesks: number, activeVdesk: number, now: number): SessionSnapshot {
  return { version: 1, savedAt: now, vdesks, activeVdesk, wins };
}

/** 恢复：快照 → 窗列表；屏幕拓扑变化时按 F00839/F00840/F00841 自愈重排。 */
export function restoreSession(
  snap: SessionSnapshot,
  work: VwmRect,
  opts?: { scale?: number; currentWork?: VwmRect },
): WinSnapshot[] {
  const base = opts?.currentWork ?? work;
  const scale = opts?.scale ?? 1;
  return snap.wins.map((w) => {
    const x = Math.min(base.x + base.w - 80, Math.max(base.x, Math.round((w.rect.x * scale) + (base.x - work.x))));
    const y = Math.min(base.y + base.h - 40, Math.max(base.y, Math.round((w.rect.y * scale) + (base.y - work.y))));
    return {
      ...w,
      rect: {
        x,
        y,
        w: Math.min(w.rect.w, base.w),
        h: Math.min(w.rect.h, base.h),
      },
    };
  });
}

/** 幽灵清理（F00842）：返回完全离屏的窗 id 列表。 */
export function ghostWins(wins: WinSnapshot[], work: VwmRect): string[] {
  return wins
    .filter((w) => w.rect.x + w.rect.w < work.x || w.rect.x > work.x + work.w || w.rect.y + w.rect.h < work.y || w.rect.y > work.y + work.h)
    .map((w) => w.win);
}

/** 离屏召回（F00843）：把幽灵窗拉回工作区右上瀑布位。 */
export function recallOffscreen(wins: WinSnapshot[], work: VwmRect): WinSnapshot[] {
  const ghosts = new Set(ghostWins(wins, work));
  let k = 0;
  return wins.map((w) => {
    if (!ghosts.has(w.win)) return w;
    const rect = { x: work.x + 24 * (k % 5), y: work.y + 24 * (k % 5), w: Math.min(w.rect.w, work.w - 48), h: Math.min(w.rect.h, work.h - 48) };
    k++;
    return { ...w, rect };
  });
}

// ---------- 族0035 窗口性能与降级 ----------

export interface PerfBudget {
  /** 后台限帧（fps，0=不限）。 */
  bgFps: number;
  /** 模糊半径上限（px，0=关模糊）。 */
  maxBlur: number;
  /** 不可见冻结。 */
  freezeOccluded: boolean;
  /** 拖动降级。 */
  dragDowngrade: boolean;
  /** 帧预算（ms）。 */
  frameMs: number;
}

/** 族0035：25 种性能档（ID 升序，F00873 低端模式为最保守档）。 */
export const PERF_PRESETS: readonly (PerfBudget & { id: string })[] = Object.freeze([
  { id: "F00851", bgFps: 0, maxBlur: 24, freezeOccluded: true, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00852", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: true, frameMs: 16.6 },
  { id: "F00853", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00854", bgFps: 10, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00855", bgFps: 0, maxBlur: 12, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00856", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00857", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00858", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00859", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00860", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00861", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00862", bgFps: 0, maxBlur: 24, freezeOccluded: true, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00863", bgFps: 0, maxBlur: 24, freezeOccluded: true, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00864", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00865", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00866", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00867", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00868", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: true, frameMs: 16.6 },
  { id: "F00869", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00870", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00871", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00872", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00873", bgFps: 5, maxBlur: 0, freezeOccluded: true, dragDowngrade: true, frameMs: 33.3 },
  { id: "F00874", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
  { id: "F00875", bgFps: 0, maxBlur: 24, freezeOccluded: false, dragDowngrade: false, frameMs: 16.6 },
]);

/** 掉帧自救降级阶梯（F00858）：按掉帧率返回应关闭的效果层（由重到轻）。 */
export function degradeLadder(dropRate: number, currentBlur: number): string[] {
  const steps: string[] = [];
  if (dropRate > 0.02) steps.push("blur");
  if (dropRate > 0.05) steps.push("transparency");
  if (dropRate > 0.1) steps.push("shadow");
  if (dropRate > 0.15) steps.push("animation");
  if (currentBlur <= 0 && steps.includes("blur")) steps.splice(steps.indexOf("blur"), 1);
  return steps;
}
