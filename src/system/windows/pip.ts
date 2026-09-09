import type { VwmRect } from "./vwm";

/**
 * N-04 任意窗口画中画（NEXT-40 · AI-2 窗口路）：
 * 把任意 VWM 窗口一键「抽出来」变成悬浮小窗——置顶、四档透明度、
 * 角落磁吸（8px 阈值）、双击标题区全屏往返、尺寸档位按应用记忆。
 * - L4（反作弊）窗口禁入 PiP（复用 C-6 判定结果，避免输入注入误判）；
 * - 失败路径：回原位 + 提示，绝不卡中间态（本层返回 null 由调用方兜底）；
 * - PiP 中的窗口仍受 VWM 生命周期管辖——宿主关闭时随之回收；
 * - 多 PiP 管理：pipList() 供托盘聚合图标显示数量与一键回收。
 */

export interface PipState {
  winId: string;
  /** 透明度档（%）：85/70/50/30。 */
  opacity: 85 | 70 | 50 | 30;
  /** 当前悬浮几何。 */
  rect: VwmRect;
  /** 进入 PiP 前的还原几何（退出时回原位）。 */
  restore: VwmRect;
}

const MEMORY_KEY = "variable:vwm:pip-memory";
const CORNER_MAGNET_PX = 8;
const PIP_DEFAULT_SIZE = { w: 420, h: 264 };

export const PIP_OPACITY_LEVELS = [85, 70, 50, 30] as const;

/** 尺寸/透明度档位记忆（按 app 记忆；data/pip_memory.json 后端接管时替换）。 */
export interface PipMemory {
  w: number;
  h: number;
  opacity: 85 | 70 | 50 | 30;
}

export function loadPipMemory(app: string): PipMemory | null {
  try {
    const all = JSON.parse(localStorage.getItem(MEMORY_KEY) ?? "{}") as Record<string, PipMemory>;
    const m = all[app];
    return m && typeof m.w === "number" ? m : null;
  } catch {
    return null;
  }
}

export function savePipMemory(app: string, m: PipMemory): void {
  try {
    const all = JSON.parse(localStorage.getItem(MEMORY_KEY) ?? "{}") as Record<string, PipMemory>;
    all[app] = m;
    localStorage.setItem(MEMORY_KEY, JSON.stringify(all));
  } catch {
    /* storage full */
  }
}

// ---------- PiP 会话（内存态，随环境关闭即回收） ----------

const active = new Map<string, PipState>();

/** 当前 PiP 列表（托盘聚合图标数量来源）。 */
export function pipList(): PipState[] {
  return [...active.values()];
}

export function isPip(winId: string): boolean {
  return active.has(winId);
}

/** 透明度循环降档（85→70→50→30→85）。 */
export function nextOpacity(cur: PipState["opacity"]): PipState["opacity"] {
  const i = PIP_OPACITY_LEVELS.indexOf(cur);
  return PIP_OPACITY_LEVELS[(i + 1) % PIP_OPACITY_LEVELS.length]!;
}

/**
 * 角落磁吸：距工作区四角/四边 ≤8px 时吸附贴边。
 * 返回吸附后的坐标（无吸附则原坐标）。
 */
export function magnetSnap(x: number, y: number, w: number, h: number, wa: VwmRect): { x: number; y: number } {
  const M = CORNER_MAGNET_PX;
  let nx = x;
  let ny = y;
  if (Math.abs(x - wa.x) <= M) nx = wa.x;
  else if (Math.abs(x + w - (wa.x + wa.w)) <= M) nx = wa.x + wa.w - w;
  if (Math.abs(y - wa.y) <= M) ny = wa.y;
  else if (Math.abs(y + h - (wa.y + wa.h)) <= M) ny = wa.y + wa.h - h;
  return { x: nx, y: ny };
}

export interface PipEnterResult {
  ok: boolean;
  /** 失败原因（诚实提示，绝不卡中间态）。 */
  reason?: string;
  state?: PipState;
}

/**
 * 进入画中画：
 * - L4 窗口直接拒绝（reason 告知，验收 ④ 键盘可达性由 UI 层保证）；
 * - 已在 PiP 中 → 幂等成功（返回现状）；
 * - 尺寸优先取该应用记忆档，无记忆用默认小窗；位置磁吸到最近角落。
 */
export function pipEnter(
  input: { winId: string; app: string; tier: 1 | 2 | 3 | 4; rect: VwmRect },
  wa: VwmRect,
): PipEnterResult {
  if (input.tier === 4) {
    return { ok: false, reason: "反作弊（L4）窗口禁用画中画" };
  }
  const existing = active.get(input.winId);
  if (existing) return { ok: true, state: existing };
  const mem = loadPipMemory(input.app);
  const w = Math.min(mem?.w ?? PIP_DEFAULT_SIZE.w, wa.w);
  const h = Math.min(mem?.h ?? PIP_DEFAULT_SIZE.h, wa.h);
  // 默认贴右下角（视频类小窗习惯位），距边 12px 起浮
  const raw = { x: wa.x + wa.w - w - 12, y: wa.y + wa.h - h - 12 };
  const snapped = magnetSnap(raw.x, raw.y, w, h, wa);
  const state: PipState = {
    winId: input.winId,
    opacity: mem?.opacity ?? 85,
    rect: { x: snapped.x, y: snapped.y, w, h },
    restore: { ...input.rect },
  };
  active.set(input.winId, state);
  return { ok: true, state };
}

/** 移动 PiP 窗口（拖动结束时磁吸），并把尺寸/透明度记忆回写。 */
export function pipMove(winId: string, x: number, y: number, wa: VwmRect): PipState | null {
  const st = active.get(winId);
  if (!st) return null;
  const snapped = magnetSnap(x, y, st.rect.w, st.rect.h, wa);
  st.rect = { ...st.rect, x: snapped.x, y: snapped.y };
  savePipMemory(appKeyOf(winId), { w: st.rect.w, h: st.rect.h, opacity: st.opacity });
  return st;
}

/** 退出画中画：回原位（restore 几何），从列表移除。 */
export function pipExit(winId: string): VwmRect | null {
  const st = active.get(winId);
  if (!st) return null;
  active.delete(winId);
  return st.restore;
}

/** 一键回收全部（托盘聚合图标动作）。返回各窗口还原几何。 */
export function pipExitAll(): Record<string, VwmRect> {
  const out: Record<string, VwmRect> = {};
  for (const st of active.values()) out[st.winId] = st.restore;
  active.clear();
  return out;
}

/** app 键提取（winId 形如 vwm-<app>-xxx）。 */
function appKeyOf(winId: string): string {
  const m = winId.match(/^vwm-(.+)-[0-9a-z]+$/);
  return m?.[1] ?? winId;
}
