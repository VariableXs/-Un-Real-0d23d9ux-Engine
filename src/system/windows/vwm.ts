import { desktopAppLabel } from "../desktop-icons/DesktopIcons";
import { getThirdApps } from "../launcher/thirdApps";
import { createStore } from "../../lib/store";
import { parseScreenDetails, screenShift } from "./winfeel";
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

/** VWM 托管对象：四款官方软件 + 系统窗口（explorer / recycle）+ 第三方应用（tp:<id>）+ 实用工具（F-2）。 */
export type VwmToolApp =
  | "calc"
  | "notes"
  | "calendar"
  | "snapshot"
  | "clipboard"
  | "rename"
  | "dupe"
  | "space"
  | "checksum";
export type VwmApp = AppMode | "explorer" | "recycle" | "taskman" | `tp:${string}` | VwmToolApp;

/** F-2：工具应用集合（窗口语义与四软件一致：贴靠/保活/多开）。AI-09 文件操作四工具并入。 */
export const VWM_TOOLS: readonly VwmToolApp[] = [
  "calc",
  "notes",
  "calendar",
  "snapshot",
  "clipboard",
  "rename",
  "dupe",
  "space",
  "checksum",
];

export function isVwmTool(app: VwmApp): app is VwmToolApp {
  return VWM_TOOLS.includes(app as VwmToolApp);
}

/** 工具窗口默认几何（计算器/便签类比文档窗口小得多，不再套 1180×760）。 */
const TOOL_DEFAULT_SIZE: Record<VwmToolApp, { w: number; h: number }> = {
  calc: { w: 660, h: 560 },
  notes: { w: 380, h: 460 },
  calendar: { w: 520, h: 600 },
  snapshot: { w: 760, h: 560 },
  clipboard: { w: 620, h: 640 },
  rename: { w: 760, h: 640 },
  dupe: { w: 720, h: 620 },
  space: { w: 720, h: 620 },
  checksum: { w: 640, h: 400 },
};

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
  /** 批次W-5 标签页化：所属标签组 id（null = 未分组）。 */
  group: string | null;
  /** 批次W-5 标签页化：是否为组内当前显示的标签。 */
  groupActive: boolean;
  /** M-02 卷帘：收起后仅剩标题栏高度（rolledFromH 记忆原高）。 */
  rolledUp: boolean;
  /** M-02 卷帘：收起前的高度（undefined = 从未收起）。 */
  rolledFromH?: number;
  /** M-03 最小化时刻（抽屉排序用；null = 不在最小化态）。 */
  minimizedAt: number | null;
  /** Z-36 不透明度（0.2..1）。 */
  opacity: number;
  /** Z-36 置顶（浮于普通窗口之上）。 */
  topmost: boolean;
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
/** M-02 标题栏高度（与 EmbedBridge 嵌入偏移的 38px 一致；卷帘收起后的窗高）。 */
export const VWM_TITLEBAR_H = 38;

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

function clampRect(r: VwmRect, wa: VwmRect, minW = MIN_W, minH = MIN_H): VwmRect {
  const w = Math.min(Math.max(r.w, minW), Math.max(minW, wa.w));
  const h = Math.min(Math.max(r.h, minH), Math.max(minH, wa.h));
  const x = Math.min(Math.max(r.x, wa.x - w + 120), Math.max(wa.x, wa.x + wa.w - 120));
  const y = Math.min(Math.max(r.y, wa.y), Math.max(wa.y, wa.y + wa.h - 48));
  return { x, y, w, h };
}

/** 工具窗口最小几何（仍满足拖拽/贴靠的可操作性）。 */
const TOOL_MIN_SIZE = { w: 300, h: 280 };

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
export function openVwmSystem(kind: "explorer" | "recycle" | "taskman", path?: string): void {
  const s = vwmStore.getState();
  const mine = s.wins.filter((w) => w.app === kind);
  if (mine.length > 0 && (kind === "recycle" || !path)) {
    const top = mine.reduce((a, b) => (a.z >= b.z ? a : b));
    focusVwmWin(top.id);
    return;
  }
  openVwmInstance(kind, path ?? null);
}

function openVwmInstance(app: VwmApp, path: string | null): string {
  const s = vwmStore.getState();
  const mine = s.wins.filter((w) => w.app === app);
  const wa = s.workArea;
  const saved = loadGeomMap()[app];
  const toolSize = isVwmTool(app) ? TOOL_DEFAULT_SIZE[app] : null;
  const minW = toolSize ? TOOL_MIN_SIZE.w : MIN_W;
  const minH = toolSize ? TOOL_MIN_SIZE.h : MIN_H;
  const defW = toolSize ? Math.min(toolSize.w, wa.w) : DEFAULT_W;
  const defH = toolSize ? Math.min(toolSize.h, wa.h) : DEFAULT_H;
  const n = mine.length;
  const base: VwmRect = saved
    ? clampRect(saved, wa, minW, minH)
    : {
        x: wa.x + Math.max(24, Math.round((wa.w - defW) / 2)),
        y: wa.y + Math.max(16, Math.round((wa.h - defH) / 2.4)),
        w: Math.min(defW, wa.w),
        h: Math.min(defH, wa.h),
      };
  // 级联偏移：同软件多开 / 未记忆几何时错位摆放
  const off = (n % 6) * CASCADE;
  const rect = clampRect({ ...base, x: base.x + off, y: base.y + off }, wa);
  const id = `vwm-${app}-${Date.now().toString(36)}${n}`;
  const z = s.topZ + 1;
  patch((st) => ({
    wins: [
      ...st.wins,
      { id, app, path, x: rect.x, y: rect.y, w: rect.w, h: rect.h, state: "normal", minimized: false, z, restore: null, group: null, groupActive: false, rolledUp: false, minimizedAt: null, opacity: 1, topmost: false },
    ],
    topZ: z,
    focusedId: id,
    seq: st.seq + 1,
  }));
  return id;
}

/**
 * 批次W-1：第三方应用强制新开实例并返回窗口实例 id（= 嵌入注册中心的 embed_id）。
 * 第三方每次启动都是独立进程，必须一一对应新虚拟窗口（复用既有实例会把
 * 新进程的窗口错嵌到旧占位上）。
 */
export function openVwmTpNew(app: `tp:${string}`): string {
  return openVwmInstance(app, null);
}

function nextFocus(wins: VwmWin[], excludeId: string | null): string | null {
  const cands = wins.filter((w) => !w.minimized && w.id !== excludeId);
  if (cands.length === 0) return null;
  return cands.reduce((a, b) => (a.z >= b.z ? a : b)).id;
}

/** 聚焦窗口（置顶 + 取消最小化）。Z-36：置顶窗口始终浮在焦点窗口之上。 */
export function focusVwmWin(id: string): void {
  const s = vwmStore.getState();
  const w = s.wins.find((x) => x.id === id);
  if (!w) return;
  const z = s.topZ + 1;
  const tops = s.wins.filter((x) => x.topmost && x.id !== id);
  const topZs = new Map(tops.map((t, i) => [t.id, z + 1 + i]));
  patch((st) => ({
    wins: st.wins.map((x) =>
      x.id === id
        ? { ...x, z, minimized: false, minimizedAt: null }
        : topZs.has(x.id)
          ? { ...x, z: topZs.get(x.id)! }
          : x,
    ),
    topZ: z + tops.length,
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
  // Z-37 几何记忆增强：normal 态关闭也持久化（卷帘中按记忆原高）
  if (w.state === "normal") {
    persistGeom(w.app, { x: w.x, y: w.y, w: w.w, h: w.rolledUp ? (w.rolledFromH ?? w.h) : w.h });
  }
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
    wins: st.wins.map((x) => (x.id === id ? { ...x, minimized: true, minimizedAt: Date.now() } : x)),
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

/** M-03 抽屉排序：最小化窗口按 minimizedAt 降序（最近的最先）。 */
export function minimizedOrder(wins: VwmWin[]): VwmWin[] {
  return wins
    .filter((w) => w.minimizedAt !== null)
    .sort((a, b) => (b.minimizedAt ?? 0) - (a.minimizedAt ?? 0));
}

/** 最大化 / 还原（记录还原几何）。 */
export function toggleMaxVwmWin(id: string): void {
  const s = vwmStore.getState();
  let w = s.wins.find((x) => x.id === id);
  if (!w) return;
  if (w.rolledUp) {
    // M-02：卷帘中先还原高度再最大化/还原
    patch((st) => ({ wins: st.wins.map((x) => (x.id === id ? { ...x, ...unrollPatch(x) } : x)) }));
    w = vwmStore.getState().wins.find((x) => x.id === id);
    if (!w) return;
  }
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

/** Z-37 关闭记忆的恢复入口：返回钳制后的记忆几何；完全出屏 → null（调用方居中）。 */
export function restoreGeomFor(app: VwmApp, wa: VwmRect): VwmRect | null {
  const saved = loadGeomMap()[app];
  if (!saved) return null;
  const tool = isVwmTool(app);
  const minW = tool ? TOOL_MIN_SIZE.w : MIN_W;
  const minH = tool ? TOOL_MIN_SIZE.h : MIN_H;
  const intersects =
    saved.x < wa.x + wa.w && saved.x + saved.w > wa.x && saved.y < wa.y + wa.h && saved.y + saved.h > wa.y;
  if (!intersects) return null;
  return clampRect(saved, wa, minW, minH);
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
  let w = s.wins.find((x) => x.id === id);
  if (!w) return;
  if (w.rolledUp) {
    // M-02：卷帘中先还原高度再贴靠
    patch((st) => ({ wins: st.wins.map((x) => (x.id === id ? { ...x, ...unrollPatch(x) } : x)) }));
    w = vwmStore.getState().wins.find((x) => x.id === id);
    if (!w) return;
  }
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
  let w = s.wins.find((x) => x.id === id);
  if (!w) return;
  if (w.rolledUp) {
    // M-02：卷帘中先还原高度再贴靠
    patch((st) => ({ wins: st.wins.map((x) => (x.id === id ? { ...x, ...unrollPatch(x) } : x)) }));
    w = vwmStore.getState().wins.find((x) => x.id === id);
    if (!w) return;
  }
  const restore = w.state === "max" ? w.restore : { x: w.x, y: w.y, w: w.w, h: w.h };
  patch((st) => ({
    wins: st.wins.map((x) => (x.id === id ? { ...x, state: "normal", ...rect, restore } : x)),
  }));
  persistGeom(w.app, rect);
}

/** 拖拽期间更新贴靠预览。 */
/** M-02 卷帘还原字段（未收起 → 空补丁）。 */
function unrollPatch(w: VwmWin): Partial<VwmWin> {
  return w.rolledUp ? { rolledUp: false, h: w.rolledFromH ?? w.h, rolledFromH: undefined } : {};
}

/** M-02 卷帘：收起仅剩标题栏高度；再展开还原原高。 */
export function rollVwmWin(id: string, rolled: boolean): void {
  const s = vwmStore.getState();
  const w = s.wins.find((x) => x.id === id);
  if (!w || w.state !== "normal" || w.rolledUp === rolled) return;
  patch((st) => ({
    wins: st.wins.map((x) =>
      x.id === id
        ? rolled
          ? { ...x, rolledUp: true, rolledFromH: x.h, h: Math.min(x.h, VWM_TITLEBAR_H) }
          : { ...x, rolledUp: false, h: x.rolledFromH ?? x.h, rolledFromH: undefined }
        : x,
    ),
  }));
}

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

/** M-08 过滤式焦点轮转（如按应用切换）：过滤集为空 → false（调用方提示并兜底）。 */
export function cycleVwmFocusFiltered(opts?: { byApp?: VwmApp; backward?: boolean }): boolean {
  const s = vwmStore.getState();
  let cands = s.wins.filter((w) => !w.minimized);
  if (opts?.byApp !== undefined) cands = cands.filter((w) => w.app === opts.byApp);
  if (cands.length === 0) return false;
  const sorted = [...cands].sort((a, b) => b.z - a.z);
  const cur = sorted.findIndex((w) => w.id === s.focusedId);
  const next = cur < 0 ? 0 : (cur + (opts?.backward ? -1 : 1) + sorted.length) % sorted.length;
  focusVwmWin(sorted[cur < 0 ? 0 : next]!.id);
  return true;
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
  if (app === "taskman") return "任务管理器";
  if (isTpApp(app)) {
    const id = tpIdOf(app);
    return (
      getThirdApps().find((a) => a.id === id)?.name ?? `应用 ${id}`
    );
  }
  // F-2 实用工具窗口标题
  if (isVwmTool(app)) {
    const labels: Record<VwmToolApp, string> = {
      calc: "计算器",
      notes: "便签",
      calendar: "日历与时钟",
      snapshot: "截图工具",
      clipboard: "剪贴板历史",
      rename: "批量重命名",
      dupe: "重复文件报告",
      space: "空间分析",
      checksum: "校验和",
    };
    return labels[app];
  }
  return desktopAppLabel(app);
}

// ---------- 批次W-5：标签页化（可选开启） ----------

const TABS_KEY = "variable:vwm:tabs";

/** 标签页化是否开启（设置→外观；默认关，localStorage 持久）。 */
export function tabsEnabled(): boolean {
  try {
    return localStorage.getItem(TABS_KEY) === "1";
  } catch {
    return false;
  }
}

export function setTabsEnabled(v: boolean): void {
  try {
    localStorage.setItem(TABS_KEY, v ? "1" : "0");
  } catch {
    /* storage blocked → 本次会话内开关不持久 */
  }
}

/** 同应用 ≥ 2 窗口：把 dragId 拖到 targetId 标题栏上 → 合并为一个标签组。 */
export function groupVwmWins(dragId: string, targetId: string): void {
  const s = vwmStore.getState();
  const a = s.wins.find((w) => w.id === dragId);
  const b = s.wins.find((w) => w.id === targetId);
  if (!a || !b || a.id === b.id || a.app !== b.app) return;
  const gid = b.group ?? `vwm-g-${Date.now().toString(36)}`;
  patch((st) => ({
    wins: st.wins.map((w) => {
      if (w.id === dragId || w.id === targetId) return { ...w, group: gid, groupActive: w.id === targetId };
      if (w.group === gid) return { ...w, groupActive: false };
      return w;
    }),
    focusedId: targetId,
  }));
}

/** 拖出标签 = 拆分：该窗口脱离标签组（几何保留原窗口位置）。 */
export function ungroupVwmWin(id: string): void {
  patch((st) => ({
    wins: st.wins.map((w) => (w.id === id ? { ...w, group: null, groupActive: true } : w)),
  }));
}

/** 点击标签：切换组内显示（保活语义——非显示成员仅隐藏不卸载业务数据）。 */
export function activateVwmTab(id: string): void {
  const s = vwmStore.getState();
  const w = s.wins.find((x) => x.id === id);
  if (!w?.group) return;
  patch((st) => ({
    wins: st.wins.map((x) => (x.group === w.group ? { ...x, groupActive: x.id === id } : x)),
    focusedId: id,
  }));
}

/** 关闭组内某个标签（红绿灯只关当前标签；其它成员保活语义不变）。 */
export function closeVwmTab(id: string): void {
  const s = vwmStore.getState();
  const w = s.wins.find((x) => x.id === id);
  if (!w?.group) {
    closeVwmWin(id);
    return;
  }
  const members = s.wins.filter((x) => x.group === w.group);
  if (members.length <= 2) {
    // 组只剩两个：关闭一个后另一个自动拆组
    closeVwmWin(id);
    patch((st) => ({
      wins: st.wins.map((x) => (x.group === w.group ? { ...x, group: null, groupActive: true } : x)),
    }));
    return;
  }
  closeVwmWin(id);
  // 关闭的是显示中的标签 → 让给相邻成员
  if (w.groupActive) {
    const others = members.filter((x) => x.id !== id);
    const next = others[others.length - 1];
    if (next) activateVwmTab(next.id);
  }
}

/** 组内成员（按 z 序，稳定显示顺序）。 */
export function groupMembersOf(wins: VwmWin[], group: string): VwmWin[] {
  return wins.filter((w) => w.group === group).sort((a, b) => a.z - b.z);
}

/** 窗口是否可见渲染（未分组 / 组内激活成员）。 */
export function isVwmWinVisible(w: VwmWin): boolean {
  return !w.group || w.groupActive;
}


// ---------- Z-36 不透明度 / 置顶 ----------

/** Z-36 设置窗口不透明度（钳制 0.2..1，保留两位小数）。 */
export function setVwmOpacity(id: string, v: number): void {
  const c = Math.round(Math.min(1, Math.max(0.2, v)) * 100) / 100;
  patch((st) => ({ wins: st.wins.map((w) => (w.id === id ? { ...w, opacity: c } : w)) }));
}

/** Z-36 置顶开关：开启即浮到最上；此后每次聚焦，其余置顶窗仍被抬到焦点之上。 */
export function setVwmTopmost(id: string, on: boolean): void {
  const s = vwmStore.getState();
  const w = s.wins.find((x) => x.id === id);
  if (!w || w.topmost === on) return;
  if (!on) {
    patch((st) => ({ wins: st.wins.map((x) => (x.id === id ? { ...x, topmost: false } : x)) }));
    return;
  }
  const z = s.topZ + 1;
  patch((st) => ({
    wins: st.wins.map((x) => (x.id === id ? { ...x, topmost: true, z } : x)),
    topZ: z,
  }));
}

// ---------- Z-40 布局快照（轻量：名字 → 全部窗口几何） ----------

const LAYOUTS_KEY = "variable:vwm:layouts";
const LAYOUTS_CAP = 20;

export interface LayoutSnapshotWin {
  app: VwmApp;
  x: number;
  y: number;
  w: number;
  h: number;
  state: "normal" | "max";
}

interface LayoutEntry {
  name: string;
  wins: LayoutSnapshotWin[];
}

function loadLayouts(): LayoutEntry[] {
  try {
    const raw = JSON.parse(localStorage.getItem(LAYOUTS_KEY) ?? "[]") as LayoutEntry[];
    return Array.isArray(raw)
      ? raw.filter((e) => !!e && typeof e.name === "string" && Array.isArray(e.wins))
      : [];
  } catch {
    return [];
  }
}

function saveLayouts(all: LayoutEntry[]): void {
  try {
    localStorage.setItem(LAYOUTS_KEY, JSON.stringify(all));
  } catch {
    /* storage full/blocked → 布局不持久化 */
  }
}

/** 保存当前布局（同名原地覆盖；上限 20 份，超出按插入序淘汰最旧）。 */
export function saveLayoutSnapshot(name: string): void {
  if (!name) return;
  const s = vwmStore.getState();
  const entry: LayoutEntry = {
    name,
    wins: s.wins
      .filter((w) => !s.closing.includes(w.id))
      .map((w) => ({
        app: w.app,
        x: Math.round(w.x),
        y: Math.round(w.y),
        w: Math.round(w.w),
        h: Math.round(w.h),
        state: w.state,
      })),
  };
  const all = loadLayouts();
  const idx = all.findIndex((e) => e.name === name);
  if (idx >= 0) all[idx] = entry;
  else all.push(entry);
  while (all.length > LAYOUTS_CAP) all.shift();
  saveLayouts(all);
}

export function listLayoutSnapshots(): Array<{ name: string; count: number }> {
  return loadLayouts().map((e) => ({ name: e.name, count: e.wins.length }));
}

/**
 * 应用布局：每个保存项找同 app 的最顶层存活窗口搬进保存矩形（保存为 max → 还原进矩形）；
 * 缺失的 app 用 openVwmInstance 新开后再贴到保存矩形。全部矩形按当前工作区钳制。
 */
export function applyLayoutSnapshot(name: string): boolean {
  const entry = loadLayouts().find((e) => e.name === name);
  if (!entry) return false;
  const consumed = new Set<string>();
  for (const sw of entry.wins) {
    const st = vwmStore.getState();
    const rect = clampRect({ x: sw.x, y: sw.y, w: sw.w, h: sw.h }, st.workArea);
    const live = st.wins
      .filter((w) => w.app === sw.app && !st.closing.includes(w.id) && !consumed.has(w.id))
      .sort((a, b) => b.z - a.z)[0];
    if (live) consumed.add(live.id);
    const targetId = live?.id ?? openVwmInstance(sw.app, null);
    patch((cur) => ({
      wins: cur.wins.map((w) => (w.id === targetId ? { ...w, state: "normal", ...rect, restore: null } : w)),
    }));
  }
  return true;
}

export function deleteLayoutSnapshot(name: string): void {
  saveLayouts(loadLayouts().filter((e) => e.name !== name));
}

// ---------- Z-42/M-05 跨屏摆渡 ----------

/** Z-42/M-05 摆渡：把窗口水平搬到相邻显示器。屏幕信息不可用或无邻居 → false。 */
export function ferryVwmWin(id: string, dir: "left" | "right"): boolean {
  const s = vwmStore.getState();
  const w = s.wins.find((x) => x.id === id);
  if (!w) return false;
  let screens: ReturnType<typeof parseScreenDetails> = null;
  try {
    if (typeof window !== "undefined") {
      const get = (window as { getScreenDetails?: () => unknown }).getScreenDetails;
      screens = typeof get === "function" ? parseScreenDetails(get.call(window)) : null;
    }
  } catch {
    screens = null;
  }
  if (!screens) return false;
  const next = screenShift({ x: w.x, y: w.y, w: w.w, h: w.h }, screens, dir);
  if (!next) return false;
  patch((st) => ({ wins: st.wins.map((x) => (x.id === id ? { ...x, ...next } : x)) }));
  return true;
}
