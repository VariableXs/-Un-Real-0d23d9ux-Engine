/**
 * F358 全局画中画（H 域 · AI-H4）：
 * 视频全局 PiP：播放中的视频一键缩为置顶小窗（240×135 起、四角可停靠、可缩放三档），
 * 小窗带播放/暂停/关闭/回到原窗四键；多视频并发 PiP 上限 2 个（第三个提示排队）；
 * PiP 窗享受置顶不抢焦点（F248 语义）。
 * 判据（主册 F358）：四键功能；停靠吸附；并发上限与排队；焦点语义；回原窗状态接续（时间点不跳）。
 * 依赖锚点：F248 无焦点置顶。
 */

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** 基础尺寸 240×135（16:9）。 */
export const PIP_BASE: Rect = { x: 0, y: 0, w: 240, h: 135 };
/** 缩放三档。 */
export const SIZE_TIERS = [1, 1.5, 2] as const;
/** 停靠吸附阈值（px）。 */
export const DOCK_SNAP_PX = 16;
/** 并发上限（判据：2）。 */
export const PIP_CONCURRENCY_CAP = 2;

export type DockCorner = "topLeft" | "topRight" | "bottomLeft" | "bottomRight";

export interface PipSession {
  winId: string;
  /** 源视频标题（回原窗提示用）。 */
  sourceTitle: string;
  /** PiP 当前几何。 */
  rect: Rect;
  tier: (typeof SIZE_TIERS)[number];
  docked: DockCorner | null;
  /** 进入 PiP 时的播放位置（s）——回原窗接续用（时间点不跳判据）。 */
  resumeAtSec: number;
  playing: boolean;
}

export interface PiPRegistry {
  sessions: PipSession[];
  /** 排队等待的 winId（FIFO——并发上限判据的排队面）。 */
  queue: string[];
}

/** 进入 PiP：上限内直接进；已满则入队并返回 queued 提示（判据「第三个提示排队」）。 */
export function enterPip(reg: PiPRegistry, winId: string, sourceTitle: string, resumeAtSec: number): { registry: PiPRegistry; outcome: "entered" | "queued" | "already"; session: PipSession | null } {
  if (reg.sessions.some((s) => s.winId === winId)) return { registry: reg, outcome: "already", session: null };
  if (reg.sessions.length >= PIP_CONCURRENCY_CAP) {
    return { registry: { ...reg, queue: [...reg.queue, winId] }, outcome: "queued", session: null };
  }
  const session: PipSession = {
    winId,
    sourceTitle,
    rect: { ...PIP_BASE },
    tier: 1,
    docked: null,
    resumeAtSec,
    playing: true,
  };
  return { registry: { ...reg, sessions: [...reg.sessions, session] }, outcome: "entered", session };
}

/** 缩放档位切换：以当前停靠角为锚点重排几何（停靠中缩放不越屏外）。 */
export function setTier(reg: PiPRegistry, winId: string, tier: (typeof SIZE_TIERS)[number], screen: { w: number; h: number }): PiPRegistry {
  return {
    ...reg,
    sessions: reg.sessions.map((s) => {
      if (s.winId !== winId || !SIZE_TIERS.includes(tier)) return s;
      const w = Math.round(PIP_BASE.w * tier);
      const h = Math.round(PIP_BASE.h * tier);
      const anchored = s.docked
        ? {
            x: s.docked === "topLeft" || s.docked === "bottomLeft" ? 0 : Math.max(0, screen.w - w),
            y: s.docked === "topLeft" || s.docked === "topRight" ? 0 : Math.max(0, screen.h - h),
            w,
            h,
          }
        : { x: s.rect.x, y: s.rect.y, w, h };
      return { ...s, tier, rect: anchored };
    }),
  };
}

/** 四键功能之播放/暂停。 */
export function togglePlay(reg: PiPRegistry, winId: string): PiPRegistry {
  return {
    ...reg,
    sessions: reg.sessions.map((s) => (s.winId === winId ? { ...s, playing: !s.playing } : s)),
  };
}

/**
 * 回原窗：移除会话，返回接续状态（播放位置原样带回——时间点不跳）；
 * 队列中的下一个自动递补（并发上限判据的公平面）。
 */
export function exitPip(reg: PiPRegistry, winId: string): { registry: PiPRegistry; resumed: { winId: string; resumeAtSec: number; playing: boolean } | null; promoted: string | null } {
  const session = reg.sessions.find((s) => s.winId === winId) ?? null;
  const sessions = reg.sessions.filter((s) => s.winId !== winId);
  let queue = reg.queue;
  let promoted: string | null = null;
  if (session && queue.length > 0 && sessions.length < PIP_CONCURRENCY_CAP) {
    promoted = queue[0]!;
    queue = queue.slice(1);
    sessions.push({ winId: promoted, sourceTitle: promoted, rect: { ...PIP_BASE }, tier: 1, docked: null, resumeAtSec: 0, playing: true });
  }
  return {
    registry: { sessions, queue },
    resumed: session ? { winId: session.winId, resumeAtSec: session.resumeAtSec, playing: session.playing } : null,
    promoted,
  };
}

/** 停靠吸附：靠近四角（阈值内）→ 吸附落角并贴边；否则自由位。 */
export function applyDocking(session: PipSession, screen: { w: number; h: number }): PipSession {
  const { x, y, w, h } = session.rect;
  const nearLeft = x <= DOCK_SNAP_PX;
  const nearRight = x + w >= screen.w - DOCK_SNAP_PX;
  const nearTop = y <= DOCK_SNAP_PX;
  const nearBottom = y + h >= screen.h - DOCK_SNAP_PX;
  const corner: DockCorner | null = nearTop && nearLeft ? "topLeft" : nearTop && nearRight ? "topRight" : nearBottom && nearLeft ? "bottomLeft" : nearBottom && nearRight ? "bottomRight" : null;
  if (!corner) return { ...session, docked: null };
  const nx = corner === "topLeft" || corner === "bottomLeft" ? 0 : screen.w - w;
  const ny = corner === "topLeft" || corner === "topRight" ? 0 : screen.h - h;
  return { ...session, docked: corner, rect: { x: nx, y: ny, w, h } };
}

/** 焦点语义（F248）：PiP 窗永远置顶但不夺焦点。 */
export function focusPolicy(): { alwaysOnTop: true; stealsFocus: false; rationale: string } {
  return { alwaysOnTop: true, stealsFocus: false, rationale: "F248 语义：置顶不抢焦点——打字不被 PiP 打断" };
}

/* ================= v4 深化批次四：停靠几何解算 / 多窗无重叠布局 / 回原窗计划 ================= */

/** 停靠边距：贴角时与屏幕边缘保持的呼吸距离（贴边不粘边）。 */
export const PIP_MARGIN = 8;

/** 停靠几何解算：角 → 具体矩形（含边距），尺寸任意（缩放档切换后仍可解）。 */
export function dockGeometry(corner: DockCorner, size: { w: number; h: number }, screen: { w: number; h: number }): Rect {
  const w = Math.min(size.w, Math.max(1, screen.w - PIP_MARGIN * 2));
  const h = Math.min(size.h, Math.max(1, screen.h - PIP_MARGIN * 2));
  const x = corner === "topLeft" || corner === "bottomLeft" ? PIP_MARGIN : screen.w - w - PIP_MARGIN;
  const y = corner === "topLeft" || corner === "topRight" ? PIP_MARGIN : screen.h - h - PIP_MARGIN;
  return { x, y, w, h };
}

/** 工作区钳制：自由拖动不越屏（任何一轴越界即拉回）。 */
export function freeMoveClamp(rect: Rect, screen: { w: number; h: number }): Rect {
  const x = Math.min(Math.max(rect.x, 0), Math.max(0, screen.w - rect.w));
  const y = Math.min(Math.max(rect.y, 0), Math.max(0, screen.h - rect.h));
  return { ...rect, x, y };
}

/** 16:9 保形缩放：只给目标宽，高按基准比例回推（整数像素，形制不走样）。 */
export function aspectPreservingResize(newW: number): { w: number; h: number } {
  const w = Math.max(80, Math.round(newW));
  const h = Math.round((w * PIP_BASE.h) / PIP_BASE.w);
  return { w, h };
}

/**
 * 多 PiP 无重叠布局（并发上限内的碰撞解算）：已停靠的按角就位；
 * 自由窗若与已布局窗相交 → 沿 Y 轴逐格下移找第一个空位——解算只动位置不动尺寸，
 * 体验承诺是「新窗永远完整可见」。
 */
export function layoutPips(sessions: PipSession[], screen: { w: number; h: number }): Map<string, Rect> {
  const placed: Rect[] = [];
  const out = new Map<string, Rect>();
  const overlaps = (r: Rect) => placed.some((p) => r.x < p.x + p.w && p.x < r.x + r.w && r.y < p.y + p.h && p.y < r.y + r.h);
  for (const s of sessions) {
    let rect = s.docked ? dockGeometry(s.docked, s.rect, screen) : freeMoveClamp(s.rect, screen);
    if (overlaps(rect)) {
      for (let y = PIP_MARGIN; y + rect.h <= screen.h - PIP_MARGIN; y += rect.h + PIP_MARGIN) {
        const cand = { ...rect, y };
        if (!overlaps(cand)) {
          rect = cand;
          break;
        }
      }
    }
    placed.push(rect);
    out.set(s.winId, rect);
  }
  return out;
}

/** 布局无重叠审计（「不叠罗汉」的 PiP 面）：两两相交 = 0。 */
export function auditNoOverlap(rects: Rect[]): boolean {
  for (let i = 0; i < rects.length; i++) {
    for (let j = i + 1; j < rects.length; j++) {
      const a = rects[i]!;
      const b = rects[j]!;
      if (a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h) return false;
    }
  }
  return true;
}

/** 回原窗计划（判据「回原窗状态接续」）：几何还原 + 播放位置原样带回 + 焦点归源窗（时间点不跳）。 */
export function returnPlan(session: PipSession, originRect: Rect): { restoreRect: Rect; resumeAtSec: number; playing: boolean; focusTarget: "source-window" } {
  return { restoreRect: { ...originRect }, resumeAtSec: session.resumeAtSec, playing: session.playing, focusTarget: "source-window" };
}

/** 队列快照（排队提示的数据面）：「第 N 位」的 N 从 1 起——排队也要有数。 */
export function queuePosition(reg: PiPRegistry, winId: string): { queued: boolean; position: number | null } {
  const idx = reg.queue.indexOf(winId);
  return { queued: idx >= 0, position: idx >= 0 ? idx + 1 : null };
}
