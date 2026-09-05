import { desktopAppLabel } from "../desktop-icons/DesktopIcons";
import { getThirdApps } from "../launcher/thirdApps";
import { createStore } from "../../lib/store";
import type { AppMode } from "../../state/uiStore";
import type { TaskbarPos } from "../../lib/settings";

/**
 * 虚拟窗口管理器（Virtual Window Manager, VWM）：
 * 在 Variable 桌面层内托管四款软件 + 系统窗口（文件管理器/回收站）的"虚拟窗口"——
 * - 独立 Z-Index 调度 / 聚焦态（Focus）/ 拖拽移动 / 边缘贴靠分屏 /
 *   最小化到任务栏（保留挂载状态）/ 右上角 Mac 红绿灯
 * - 同一软件多开（窗口按实例 id 区分；业务数据仍走各自软件的既有存储，
 *   壳层不触碰任何业务逻辑）
 * - 几何持久化：localStorage 存各软件最近一次 normal 几何，新窗口按
 *   瀑布式级联偏移摆放，避免多开完全重叠
 *
 * 坐标全部为桌面窗口 CSS 像素（视口局部坐标）；工作区 = 视口减去任务栏
 * （停靠位置四向由 settings.taskbarPos 决定）。
 */

/** VWM 托管对象：四款官方软件 + 系统窗口（explorer / recycle）+ 第三方应用（tp:<id>）。 */
export type VwmApp = AppMode | "explorer" | "recycle" | `tp:${string}`;

/** 是否第三方应用虚拟窗口（宿主为 SetParent 嵌入的原生窗口）。 */
export function isTpApp(app: VwmApp): app is `tp:${string}` {
  return typeof app === "string" && app.startsWith("tp:");
}

/** tp:<id> → 登记名。 */
export function tpIdOf(app: VwmApp): string {
  return isTpApp(app) ? (app as `tp:${string}`).slice(3) : "";
}

export interface VwmWin {
  /** 实例 id（同软件多开各不相同），如 `vwm-write-k3x9`。 */
  id: string;
  app: VwmApp;
  /** explorer 初始定位路径（仅系统窗口使用；null = 打开默认位置）。 */
  path: string | null;
  x: number;
  y: number;
  w: number;
  h: number;
  state: "normal" | "max";
  minimized: boolean;
  /** 渲染用 z-index（单调递增，越大越靠上）。 */
  z: number;
  /** 最大化/贴靠前的还原几何（null = 无，取当前几何）。 */
  restore: { x: number; y: number; w: number; h: number } | null;
}

export interface VwmRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface VwmState {
  wins: VwmWin[];
  focusedId: string | null;
  topZ: number;
  /** 拖拽贴靠预览矩形（视口局部 CSS 像素；null = 不显示）。 */
  snapPreview: VwmRect | null;
  /** 桌面工作区（视口局部 CSS 像素，任务栏之外）。 */
  workArea: VwmRect;
  seq: number;
  /** 批次E-14 关闭动画中：窗口仍在渲染（缩小淡出），动画结束才真正移除。 */
  closing: string[];
  /** 批次E-14 最小化飞行中：窗口向任务栏飞去（transition 生效），落地后 display:none。 */
  flying: string[];
}

const GEOM_KEY = "variable:vwm:geom:v2";
const MIN_W = 820;
const MIN_H = 540;
const DEFAULT_W = 1180;
const DEFAULT_H = 760;
const CASCADE = 28;
/** 任务栏占位（与 desktop.css .taskbar 尺寸一致）。 */
const TB_MAIN = 54;
const TB_SIDE = 62;

export const vwmStore = createStore<VwmState>({
  wins: [],
  focusedId: null,
  topZ: 10,
  snapPreview: null,
  workArea: { x: 0, y: 0, w: 0, h: 0 },
  seq: 0,
  closing: [],
  flying: [],
});

// ---------- geometry persistence（按软件记忆最近一次 normal 几何） ----------

function loadGeomMap(): Record<string, VwmRect> {
  try {
    const raw = JSON.parse(localStorage.getItem(GEOM_KEY) ?? "{}") as Record<string, VwmRect>;
    return raw && typeof raw === "object" ? raw : {};
  } catch {
    return {};
  }
}

function persistGeom(app: VwmApp, r: VwmRect): void {
  try {
    const all = loadGeomMap();
    all[app] = { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.w), h: Math.round(r.h) };
    localStorage.setItem(GEOM_KEY, JSON.stringify(all));
  } catch {
    /* storage full/blocked → geometry won't persist */
  }
}

/** 工作区（视口局部坐标）——由任务栏停靠位置推导。 */
export function computeWorkArea(pos: TaskbarPos, vw: number, vh: number): VwmRect {
  if (pos === "top") return { x: 0, y: TB_MAIN, w: vw, h: Math.max(200, vh - TB_MAIN) };
  if (pos === "left") return { x: TB_SIDE, y: 0, w: Math.max(200, vw - TB_SIDE), h: vh };
  if (pos === "right") return { x: 0, y: 0, w: Math.max(200, vw - TB_SIDE), h: vh };
  return { x: 0, y: 0, w: vw, h: Math.max(200, vh - TB_MAIN) };
}

function clampRect(r: VwmRect, wa: VwmRect): VwmRect {
  const w = Math.min(Math.max(r.w, MIN_W), Math.max(MIN_W, wa.w));
  const h = Math.min(Math.max(r.h, MIN_H), Math.max(MIN_H, wa.h));
  const x = Math.min(Math.max(r.x, wa.x - w + 120), Math.max(wa.x, wa.x + wa.w - 120));
  const y = Math.min(Math.max(r.y, wa.y), Math.max(wa.y, wa.y + wa.h - 48));
  return { x, y, w, h };
}

// ---------- actions ----------

function patch(p: Partial<VwmState> | ((s: VwmState) => Partial<VwmState>)): void {
  vwmStore.setState(p);
}

/** 打开一款软件的虚拟窗口：已有未最小化实例 → 聚焦；否则新建实例（多开）。 */
export function openVwmApp(app: VwmApp, opts?: { forceNew?: boolean }): void {
  const s = vwmStore.getState();
  const mine = s.wins.filter((w) => w.app === app);
  if (!opts?.forceNew && mine.length > 0) {
    const top = mine.reduce((a, b) => (a.z >= b.z ? a : b));
    focusVwmWin(top.id);
    return;
  }
  openVwmInstance(app, null);
}

/**
 * 打开系统窗口（文件管理器 / 回收站）的虚拟窗口：
 * - 回收站：单实例（已存在 → 聚焦）
 * - 文件管理器：无 path → 已有实例聚焦（Windows 习惯）；带 path → 新开实例定位
 */
export function openVwmSystem(kind: "explorer" | "recycle", path?: string): void {
  const s = vwmStore.getState();
  const mine = s.wins.filter((w) => w.app === kind);
  if (mine.length > 0 && (kind === "recycle" || !path)) {
    const top = mine.reduce((a, b) => (a.z >= b.z ? a : b));
    focusVwmWin(top.id);
    return;
  }
  openVwmInstance(kind, path ?? null);
}

function openVwmInstance(app: VwmApp, path: string | null): void {
  const s = vwmStore.getState();
  const mine = s.wins.filter((w) => w.app === app);
  const wa = s.workArea;
  const saved = loadGeomMap()[app];
  const n = mine.length;
  const base: VwmRect = saved
    ? clampRect(saved, wa)
    : {
        x: wa.x + Math.max(24, Math.round((wa.w - DEFAULT_W) / 2)),
        y: wa.y + Math.max(16, Math.round((wa.h - DEFAULT_H) / 2.4)),
        w: Math.min(DEFAULT_W, wa.w),
        h: Math.min(DEFAULT_H, wa.h),
      };
  // 级联偏移：同软件多开 / 未记忆几何时错位摆放
  const off = (n % 6) * CASCADE;
  const rect = clampRect({ ...base, x: base.x + off, y: base.y + off }, wa);
  const id = `vwm-${app}-${Date.now().toString(36)}${n}`;
  const z = s.topZ + 1;
  patch((st) => ({
    wins: [
      ...st.wins,
      { id, app, path, x: rect.x, y: rect.y, w: rect.w, h: rect.h, state: "normal", minimized: false, z, restore: null },
    ],
    topZ: z,
    focusedId: id,
    seq: st.seq + 1,
  }));
}

function nextFocus(wins: VwmWin[], excludeId: string | null): string | null {
  const cands = wins.filter((w) => !w.minimized && w.id !== excludeId);
  if (cands.length === 0) return null;
  return cands.reduce((a, b) => (a.z >= b.z ? a : b)).id;
}

/** 聚焦窗口（置顶 + 取消最小化）。 */
export function focusVwmWin(id: string): void {
  const s = vwmStore.getState();
  const w = s.wins.find((x) => x.id === id);
  if (!w) return;
  const z = s.topZ + 1;
  patch((st) => ({
    wins: st.wins.map((x) => (x.id === id ? { ...x, z, minimized: false } : x)),
    topZ: z,
    focusedId: id,
  }));
}

/** 指针按下时的聚焦：已聚焦则不改动（避免无谓重排）。 */
export function pointerFocusVwm(id: string): void {
  const s = vwmStore.getState();
  if (s.focusedId === id) return;
  focusVwmWin(id);
}

export function closeVwmWin(id: string): void {
  const s = vwmStore.getState();
  const w = s.wins.find((x) => x.id === id);
  if (!w || s.closing.includes(id)) return;
  // 批次E-14 关闭仪式：先播放缩小淡出动画，170ms 后才真正卸载
  patch((st) => ({ closing: [...st.closing, id] }));
  window.setTimeout(() => {
    const st = vwmStore.getState();
    const wins = st.wins.filter((x) => x.id !== id);
    patch((cur) => ({
      wins,
      closing: cur.closing.filter((c) => c !== id),
      focusedId: cur.focusedId === id ? nextFocus(wins, null) : cur.focusedId,
    }));
  }, 170);
}

/** 关闭某软件的全部虚拟窗口（任务栏悬停关闭/卸载联动）。 */
export function closeVwmApp(app: VwmApp): void {
  const s = vwmStore.getState();
  const wins = s.wins.filter((x) => x.app !== app);
  patch((st) => ({
    wins,
    focusedId: st.focusedId && !wins.some((w) => w.id === st.focusedId) ? nextFocus(wins, null) : st.focusedId,
  }));
}

/** 最小化（挂载状态保留，任务栏图标可恢复）。批次E-14：先播放飞向任务栏的动画。 */
export function minimizeVwmWin(id: string): void {
  const s = vwmStore.getState();
  const w = s.wins.find((x) => x.id === id);
  if (!w || w.minimized || s.flying.includes(id)) return;
  patch((st) => ({
    wins: st.wins.map((x) => (x.id === id ? { ...x, minimized: true } : x)),
    flying: [...st.flying, id],
    focusedId: st.focusedId === id ? nextFocus(st.wins, id) : st.focusedId,
  }));
  // 飞行动画期间保持渲染（.minimizing 覆盖 .minimized 的 display:none），落地后隐藏
  window.setTimeout(() => {
    patch((st) => ({ flying: st.flying.filter((f) => f !== id) }));
  }, 200);
}

export function minimizeAllVwm(): void {
  const s = vwmStore.getState();
  if (s.wins.length === 0) return;
  patch({ wins: s.wins.map((w) => ({ ...w, minimized: true })), focusedId: null });
}

/** 最大化 / 还原（记录还原几何）。 */
export function toggleMaxVwmWin(id: string): void {
  const s = vwmStore.getState();
  const w = s.wins.find((x) => x.id === id);
  if (!w) return;
  if (w.state === "max") {
    const r = w.restore ?? { x: w.x, y: w.y, w: w.w, h: w.h };
    patch((st) => ({
      wins: st.wins.map((x) => (x.id === id ? { ...x, state: "normal", ...r, restore: null } : x)),
    }));
    persistGeom(w.app, r);
  } else {
    const restore = { x: w.x, y: w.y, w: w.w, h: w.h };
    const wa = s.workArea;
    patch((st) => ({
      wins: st.wins.map((x) =>
        x.id === id ? { ...x, state: "max", restore, x: wa.x, y: wa.y, w: wa.w, h: wa.h } : x,
      ),
    }));
  }
}

/** 最大化态被拖动时：还原到指定几何并继续拖拽（Windows 习惯）。 */
export function unmaxVwmTo(id: string, r: VwmRect): void {
  patch((st) => ({
    wins: st.wins.map((w) => (w.id === id ? { ...w, state: "normal", ...r, restore: null } : w)),
  }));
}

/** 移动（仅 normal 态；max 态由拖拽还原逻辑处理）。 */
export function moveVwmWin(id: string, x: number, y: number): void {
  patch((st) => ({
    wins: st.wins.map((w) => (w.id === id && w.state === "normal" ? { ...w, x: Math.round(x), y: Math.round(y) } : w)),
  }));
}

/** 调整大小（仅 normal 态）。 */
export function resizeVwmWin(id: string, r: VwmRect): void {
  patch((st) => ({
    wins: st.wins.map((w) => (w.id === id && w.state === "normal" ? { ...w, ...r, restore: w.restore } : w)),
  }));
}

/** 拖拽/缩放结束：持久化最近 normal 几何。 */
export function settleVwmWin(id: string): void {
  const s = vwmStore.getState();
  const w = s.wins.find((x) => x.id === id);
  if (w && w.state === "normal") persistGeom(w.app, { x: w.x, y: w.y, w: w.w, h: w.h });
}

/** 贴靠矩形（视口局部坐标）：左右半屏 / 四角 1/4 / 上=最大化。 */
export function snapZoneForVwm(
  dir: "left" | "right" | "up" | "down" | "tl" | "tr" | "bl" | "br",
  wa: VwmRect,
): VwmRect {
  const halfW = Math.round(wa.w / 2);
  const halfH = Math.round(wa.h / 2);
  switch (dir) {
    case "left":
      return { x: wa.x, y: wa.y, w: halfW, h: wa.h };
    case "right":
      return { x: wa.x + wa.w - halfW, y: wa.y, w: halfW, h: wa.h };
    case "tl":
      return { x: wa.x, y: wa.y, w: halfW, h: halfH };
    case "tr":
      return { x: wa.x + wa.w - halfW, y: wa.y, w: halfW, h: halfH };
    case "bl":
      return { x: wa.x, y: wa.y + wa.h - halfH, w: halfW, h: halfH };
    case "br":
      return { x: wa.x + wa.w - halfW, y: wa.y + wa.h - halfH, w: halfW, h: halfH };
    default:
      return wa; // up
  }
}

/** 应用贴靠（up = 最大化；down = 还原，无还原几何则最小化）。 */
export function snapVwmWin(id: string, dir: "left" | "right" | "up" | "down"): void {
  const s = vwmStore.getState();
  const w = s.wins.find((x) => x.id === id);
  if (!w) return;
  if (dir === "down") {
    if (w.state === "max" || w.restore) toggleMaxVwmWin(id);
    else minimizeVwmWin(id);
    return;
  }
  if (dir === "up") {
    if (w.state !== "max") toggleMaxVwmWin(id);
    return;
  }
  const zone = snapZoneForVwm(dir, s.workArea);
  const restore = w.state === "max" ? w.restore : { x: w.x, y: w.y, w: w.w, h: w.h };
  patch((st) => ({
    wins: st.wins.map((x) => (x.id === id ? { ...x, state: "normal", ...zone, restore } : x)),
  }));
  persistGeom(w.app, zone);
}

/** 按矩形贴靠（四角 1/4 等；保留还原几何）。 */
export function snapVwmRect(id: string, rect: VwmRect): void {
  const s = vwmStore.getState();
  const w = s.wins.find((x) => x.id === id);
  if (!w) return;
  const restore = w.state === "max" ? w.restore : { x: w.x, y: w.y, w: w.w, h: w.h };
  patch((st) => ({
    wins: st.wins.map((x) => (x.id === id ? { ...x, state: "normal", ...rect, restore } : x)),
  }));
  persistGeom(w.app, rect);
}

/** 拖拽期间更新贴靠预览。 */
export function setVwmSnapPreview(r: VwmRect | null): void {
  patch({ snapPreview: r });
}

/** 任务栏图标点击（Windows 习惯）：无窗口→打开；全最小化→恢复最上层；
 *  聚焦中→最小化；否则→聚焦最上层。 */
export function taskbarClickVwm(app: VwmApp): void {
  const s = vwmStore.getState();
  const mine = s.wins.filter((w) => w.app === app);
  if (mine.length === 0) {
    openVwmApp(app);
    return;
  }
  const top = mine.reduce((a, b) => (a.z >= b.z ? a : b));
  const nonMin = mine.filter((w) => !w.minimized);
  if (nonMin.length === 0) {
    focusVwmWin(top.id);
    return;
  }
  if (s.focusedId === top.id && !top.minimized) {
    minimizeVwmWin(top.id);
    return;
  }
  focusVwmWin(top.id);
}

/** Alt+Tab / 切换器：把焦点让给 Z 序中紧邻其下的未最小化窗口。 */
export function cycleVwmFocus(backward = false): void {
  const s = vwmStore.getState();
  const cands = s.wins.filter((w) => !w.minimized);
  if (cands.length < 2) return;
  const sorted = [...cands].sort((a, b) => b.z - a.z); // z 大 → 小
  const cur = sorted.findIndex((w) => w.id === s.focusedId);
  const next = cur < 0 ? 1 : (cur + (backward ? -1 : 1) + sorted.length) % sorted.length;
  const target = sorted[cur < 0 ? 1 : next];
  if (target) focusVwmWin(target.id);
}

/** 更新工作区（窗口 resize / 任务栏位置变化时由管理器调用）。 */
export function setVwmWorkArea(wa: VwmRect): void {
  const s = vwmStore.getState();
  if (s.workArea.w === wa.w && s.workArea.h === wa.h && s.workArea.x === wa.x && s.workArea.y === wa.y) return;
  patch({
    workArea: wa,
    // 最大化窗口跟随新工作区
    wins: s.wins.map((w) =>
      w.state === "max" ? { ...w, x: wa.x, y: wa.y, w: wa.w, h: wa.h } : w,
    ),
  });
}

export function vwmWindowTitle(app: VwmApp): string {
  if (app === "explorer") return "Variable 文件管理器";
  if (app === "recycle") return "Variable 回收站";
  if (isTpApp(app)) {
    const id = tpIdOf(app);
    return (
      getThirdApps().find((a) => a.id === id)?.name ?? `应用 ${id}`
    );
  }
  return desktopAppLabel(app);
}
