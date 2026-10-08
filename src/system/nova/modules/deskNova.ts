/**
 * NOVA-200 · 域3 桌面生态路（AI-03）—— W-026…W-038 行为层与纯逻辑。
 *
 * 域界（全景 §3）：存量已覆盖排列/密度/锁定/模板/堆叠/情绪微动/缩放舞台/网格微调；
 * 本域补齐**养成性、季节性、社交化的桌面生态**。十三项：
 *   W-026 图标星座排列  W-027 图标养成    W-028 季节地影
 *   W-029 图标排行榜    W-030 桌面午憩    W-031 图标老化公约
 *   W-032 桌面地平线    W-033 布局合影    W-034 图标层光雪
 *   W-035 声音地形      W-036 图标结组    W-037 桌面标尺模式
 *   W-038 原点坐标
 *
 * 纪律（实施总纲 §1/§3）：
 * - 零侵入：只读观察 `.desktop-icons` / `.desktop-icon`，浮层一律挂 `#nova-desk-layer`
 *   （pointer-events:none，面板局部 auto）；不改任何既有组件内部逻辑；
 * - 前缀：类名 `nova-desk-`、事件 `nova://desk/...`、存储 `nova.desk.*`；
 * - 降级链：reduce-motion / safeMode / static 三态下一切动效归零（motionOK=false）；
 *   非 DOM 环境（vitest node）行为层安全 no-op；
 * - 默认档：氛围/动效类（W-026/027/028/030/031/032/034/035）默认关；工具类
 *   （W-029/033/036/037/038）仅用户主动触发，默认开。
 *
 * 诚实边界：
 * - W-029/W-033 的入口为 nova 中枢（S0）派发 `nova://desk/open` 或本模块 API；
 *   右键菜单属 DesktopIcons 内部，零侵入不改写；
 * - W-036 结组为会话级视觉组织（环抱边框随成员位移）；整体移动经由既有
 *   「框选多选 → 集群拖放」完成（组员拖出即自动解散，Enter 展开为视觉语义）；
 * - W-038 双击空白回原点仅在 V-04 动作槽为 none 且用户开启 `dblHome` 时生效
 *   （V-04 已配置动作时完全让位）；个体「回原点」走 API/中枢事件；
 * - W-035 静音态由运行时经 `nova://desk/audio-state` 推送或激活参数给入；
 *   本模块不自行轮询设置后端。
 */

import {
  allShelfMembers,
  loadDesktopLayout,
  saveDesktopLayout,
  type Cell,
  type DesktopLayout,
} from "../../desktop-icons/layout";
import { loadIconPx, tierForPx, type IconTier } from "../../desktop-icons/density";
import { loadDoubleClickAction } from "../../desktop-icons/dblclick";
import { RELOAD_LAYOUT_EVENT } from "../../desktop/profiles";

// ---------------------------------------------------------------------------
// 契约（供 S0 新星运行时消费的最小接口；不依赖 S0 工件，运行时可缺省）
// ---------------------------------------------------------------------------

export interface NovaFeatureMeta {
  id: string;
  title: string;
  titleEn: string;
  desc: string;
  /** 默认开关（氛围类默认关；工具类默认开）。 */
  defaultOn: boolean;
  /** 参数 schema（key → 取值说明）。 */
  params?: Record<string, { def: number | string | boolean; note: string }>;
}

export interface DeskNovaCtx {
  /** 功能开关（实时读取，切换即时生效）。 */
  on: (id: string) => boolean;
  /** 数值参数（缺省回落注册表默认）。 */
  num: (id: string, key: string) => number;
  str: (id: string, key: string) => string;
  bool: (id: string, key: string) => boolean;
  /** reduce-motion / safeMode / static 统一判定（false = 一切动效归零）。 */
  motionOK: () => boolean;
}

export interface DeskNovaHandle {
  activate(ctx: DeskNovaCtx): void;
  deactivate(): void;
  /** 中枢/上层可直接调用的领域动作（均为幂等安全入口）。 */
  api: {
    openLeaderboard(): void;
    openPhotos(): void;
    applyConstellation(id: string): boolean;
    restoreGrid(): boolean;
    returnAllHomes(): number;
    returnHome(id: string): boolean;
    dissolveCluster(): void;
  };
}

type Unsub = () => void;

// 事件名（nova:// 前缀纪律）
export const DESK_OPEN_EVENT = "nova://desk/open"; // detail { panel: "leaderboard" | "photos" }
export const DESK_AUDIO_EVENT = "nova://desk/audio-state"; // detail { muted?: boolean; volume?: number }
export const DESK_HOME_EVENT = "nova://desk/home-all"; // 全体回原点
export const DESK_ARCHIVE_HINT_EVENT = "nova://desk/archive-hint"; // detail { id }（接 V-96 归档语义）

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-026 图标星座排列
// ---------------------------------------------------------------------------

export interface Constellation {
  id: string;
  zh: string;
  en: string;
  /** 归一化星点（0..1，y 向下）。 */
  points: [number, number][];
  /** 连线（点索引对，星图细线用）。 */
  lines: [number, number][];
}

/** 六座真实星座曲线（每座 12–20 位；形状取真实星官主星连线）。 */
export const CONSTELLATIONS: Constellation[] = [
  {
    id: "sagittarius", zh: "人马座", en: "Sagittarius",
    points: [
      [0.06, 0.30], [0.14, 0.22], [0.24, 0.18], [0.34, 0.26], [0.30, 0.40],
      [0.20, 0.46], [0.42, 0.38], [0.52, 0.48], [0.48, 0.62], [0.36, 0.66],
      [0.60, 0.30], [0.70, 0.22], [0.80, 0.28], [0.74, 0.44],
    ],
    lines: [[0, 1], [1, 2], [2, 3], [3, 4], [4, 5], [3, 6], [6, 7], [7, 8], [8, 9], [6, 10], [10, 11], [11, 12], [12, 13]],
  },
  {
    id: "orion", zh: "猎户座", en: "Orion",
    points: [
      [0.50, 0.04], [0.30, 0.10], [0.70, 0.12], [0.24, 0.34], [0.76, 0.36],
      [0.38, 0.50], [0.50, 0.52], [0.62, 0.54], [0.30, 0.74], [0.68, 0.76],
      [0.20, 0.90], [0.44, 0.94], [0.82, 0.92],
    ],
    lines: [[0, 2], [1, 3], [0, 1], [3, 4], [2, 4], [3, 5], [4, 8], [5, 6], [6, 7], [7, 9], [8, 10], [8, 11], [9, 12], [5, 8]],
  },
  {
    id: "ursa", zh: "大熊座", en: "Ursa Major",
    points: [
      [0.06, 0.20], [0.16, 0.16], [0.26, 0.22], [0.34, 0.18],
      [0.42, 0.28], [0.52, 0.30], [0.58, 0.22],
      [0.68, 0.34], [0.76, 0.30], [0.84, 0.40], [0.90, 0.34],
      [0.70, 0.52], [0.78, 0.62],
    ],
    lines: [[0, 1], [1, 2], [2, 3], [3, 4], [4, 5], [5, 6], [6, 0], [6, 7], [7, 8], [8, 9], [9, 10], [7, 11], [11, 12]],
  },
  {
    id: "cassiopeia", zh: "仙后座", en: "Cassiopeia",
    points: [
      [0.08, 0.36], [0.22, 0.24], [0.38, 0.40], [0.54, 0.20], [0.70, 0.38],
      [0.82, 0.26], [0.92, 0.44],
      [0.30, 0.60], [0.46, 0.66], [0.62, 0.58], [0.40, 0.82], [0.58, 0.88],
    ],
    lines: [[0, 1], [1, 2], [2, 3], [3, 4], [4, 5], [5, 6], [2, 7], [7, 8], [8, 9], [8, 10], [10, 11]],
  },
  {
    id: "cygnus", zh: "天鹅座", en: "Cygnus",
    points: [
      [0.50, 0.06], [0.48, 0.20], [0.46, 0.34], [0.44, 0.48],
      [0.30, 0.40], [0.16, 0.34], [0.58, 0.40], [0.72, 0.34], [0.86, 0.28],
      [0.42, 0.64], [0.38, 0.80], [0.34, 0.94],
    ],
    lines: [[0, 1], [1, 2], [2, 3], [2, 4], [4, 5], [2, 6], [6, 7], [7, 8], [3, 9], [9, 10], [10, 11]],
  },
  {
    id: "lyra", zh: "天琴座", en: "Lyra",
    points: [
      [0.52, 0.10], [0.40, 0.24], [0.64, 0.26], [0.38, 0.40], [0.62, 0.42],
      [0.46, 0.34], [0.56, 0.36],
      [0.30, 0.58], [0.24, 0.72], [0.38, 0.78], [0.72, 0.60], [0.80, 0.74], [0.66, 0.82],
    ],
    lines: [[0, 1], [0, 2], [1, 3], [2, 4], [3, 5], [4, 6], [5, 6], [3, 4], [3, 7], [7, 8], [8, 9], [4, 10], [10, 11], [11, 12]],
  },
];

export function constellationById(id: string): Constellation | null {
  return CONSTELLATIONS.find((c) => c.id === id) ?? null;
}

/**
 * W-026：星座曲线 → 网格格子。前 count 枚图标沿曲线排布；
 * 冲突时向右下螺旋找最近空格；越界钳制；点数不足时循环复用曲线尾段收拢。
 */
export function constellationCells(c: Constellation, count: number, cols: number, rows: number): Cell[] {
  const used = new Set<string>();
  const out: Cell[] = [];
  const clampC = (v: number): number => Math.min(cols - 1, Math.max(0, Math.round(v)));
  const clampR = (v: number): number => Math.min(rows - 1, Math.max(0, Math.round(v)));
  const take = (cc: number, rr: number): void => {
    let c0 = clampC(cc);
    let r0 = clampR(rr);
    // 螺旋找空格（半径步进）
    for (let ring = 0; ring < Math.max(cols, rows); ring++) {
      let placed = false;
      for (let dr = -ring; dr <= ring && !placed; dr++) {
        for (let dc = -ring; dc <= ring && !placed; dc++) {
          if (Math.max(Math.abs(dc), Math.abs(dr)) !== ring) continue;
          const cc2 = clampC(c0 + dc);
          const rr2 = clampR(r0 + dr);
          const key = `${cc2},${rr2}`;
          if (!used.has(key)) {
            used.add(key);
            out.push({ c: cc2, r: rr2 });
            placed = true;
          }
        }
      }
      if (placed) return;
      c0 = clampC(c0 + ring + 1);
      r0 = clampR(r0 + ring + 1);
    }
  };
  const n = Math.max(0, Math.floor(count));
  for (let i = 0; i < n; i++) {
    const p = c.points[i % c.points.length]!;
    // 后半圈图标沿曲线尾段轻微错位（星密带），保持星座轮廓可读
    const wrap = Math.floor(i / c.points.length);
    const jit = wrap === 0 ? 0 : 0.018 * wrap;
    take(p[0] * (cols - 1) + jit * (wrap % 2 === 0 ? 1 : -1) * cols * 0.5, p[1] * (rows - 1) + jit * rows);
  }
  return out;
}

/** W-026：星图连线像素坐标（12% 透明度细线；reduce-motion 下由调用方隐藏）。 */
export function constellationLinesPx(
  c: Constellation,
  cells: Cell[],
  tier: IconTier,
  pad: number,
): { x1: number; y1: number; x2: number; y2: number }[] {
  const cx = (cell: Cell): number => pad + cell.c * tier.w + tier.w / 2;
  const cy = (cell: Cell): number => pad + cell.r * tier.h + tier.h / 2;
  const out: { x1: number; y1: number; x2: number; y2: number }[] = [];
  for (const [a, b] of c.lines) {
    const ca = cells[a];
    const cb = cells[b];
    if (!ca || !cb) continue;
    out.push({ x1: cx(ca), y1: cy(ca), x2: cx(cb), y2: cy(cb) });
  }
  return out;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-027 图标养成 / W-031 图标老化公约
// ---------------------------------------------------------------------------

/** W-027：使用频率 → 阴影加深（0..0.08；单调）。 */
export function growthShadow(clicks30: number, maxRef: number): number {
  if (maxRef <= 0 || clicks30 <= 0) return 0;
  return Math.min(0.08, (clicks30 / maxRef) * 0.08);
}

/** W-027：使用频率 → 底光饱和增益（0..1，用于 --nova-glow）。 */
export function growthGlow(clicks30: number, maxRef: number): number {
  if (maxRef <= 0 || clicks30 <= 0) return 0;
  return Math.min(1, Math.sqrt(clicks30 / maxRef));
}

/** W-027：半休眠透明度 —— 30 天未点渐入 85% 透明（21 天起渐变）；任何交互唤醒由账本 lastUse 表达。 */
export function dormancyOpacity(daysIdle: number, dormancyDays = 30): number {
  const start = Math.max(1, dormancyDays - 10);
  if (daysIdle <= start) return 1;
  if (daysIdle >= dormancyDays) return 0.15;
  const k = (daysIdle - start) / (dormancyDays - start);
  return 1 - k * 0.85;
}

/** W-031：老化梯度 —— 7 天起每周 -3%，下限 60%（返回 opacity 0.60..1）。 */
export function agingOpacity(daysIdle: number, fadeStartDays = 7, ratePerWeek = 3, floorPct = 60): number {
  if (daysIdle <= fadeStartDays) return 1;
  const weeks = Math.floor((daysIdle - fadeStartDays) / 7);
  const pct = Math.max(floorPct, 100 - weeks * ratePerWeek);
  return pct / 100;
}

/** W-031：退休提案到期（30 天未用一次性触发；免疫由调用方账本判定）。 */
export function retireDue(daysIdle: number, retireDays = 30): boolean {
  return daysIdle >= retireDays;
}

/** W-031：拒绝后免疫截止时刻（90 天）。 */
export function immuneUntil(rejectedAtMs: number, immunityDays = 90): number {
  return rejectedAtMs + immunityDays * 86_400_000;
}

/** 有效观感透明度：养成与老化叠加取较小者（互不掩盖）。 */
export function effectiveOpacity(daysIdle: number, dormancyDays = 30): number {
  return Math.min(dormancyOpacity(daysIdle, dormancyDays), agingOpacity(daysIdle));
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-028 季节地影
// ---------------------------------------------------------------------------

export type Season = "spring" | "summer" | "autumn" | "winter";
export type Hemisphere = "north" | "south";

/** 季节判定（month0 = 0 基月份；南半球反转）。 */
export function seasonOf(month0: number, hemi: Hemisphere = "north"): Season {
  const m = ((month0 % 12) + 12) % 12;
  const north: Season = m <= 1 || m === 11 ? "winter" : m <= 4 ? "spring" : m <= 7 ? "summer" : "autumn";
  if (hemi === "north") return north;
  return north === "winter" ? "summer" : north === "summer" ? "winter" : north === "spring" ? "autumn" : "spring";
}

/** 四季阴影色温（仅阴影层；spring 柔粉 / summer 锐白 / autumn 暖棕 / winter 冷蓝）。 */
export const SEASON_SHADOW: Record<Season, { tint: string; alpha: number; blur: number; offY: number }> = {
  spring: { tint: "#f2c9d6", alpha: 0.34, blur: 5, offY: 2 },
  summer: { tint: "#eef4ff", alpha: 0.30, blur: 3, offY: 1 },
  autumn: { tint: "#d8b08c", alpha: 0.38, blur: 6, offY: 3 },
  winter: { tint: "#bcd2f2", alpha: 0.36, blur: 5, offY: 2 },
};

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-029 图标排行榜
// ---------------------------------------------------------------------------

export interface LeaderRow {
  id: string;
  count: number;
  medal: "gold" | "silver" | "bronze" | "none";
}

/** 月度点击 Top10 + 领奖台金属档。 */
export function leaderboard(counts: Record<string, number>, n = 10): LeaderRow[] {
  return Object.entries(counts)
    .filter(([, v]) => v > 0)
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, n)
    .map(([id, count], i) => ({
      id,
      count,
      medal: i === 0 ? "gold" : i === 1 ? "silver" : i === 2 ? "bronze" : "none",
    }));
}

export interface SleepKing {
  id: string;
  daysIdle: number;
}

/** 沉睡王：30 天零点击中「最大」的图标（体积权重由调用方给 tile px；同权取最久未用）。 */
export function sleepKing(
  lastUse: Record<string, number>,
  nowMs: number,
  weight: Record<string, number> = {},
  idleDays = 30,
): SleepKing | null {
  const cut = nowMs - idleDays * 86_400_000;
  let best: SleepKing | null = null;
  let bestW = -1;
  for (const [id, ts] of Object.entries(lastUse)) {
    if (ts > cut) continue;
    const w = (weight[id] ?? 0) * 1e9 + (cut - ts);
    if (w > bestW) {
      bestW = w;
      best = { id, daysIdle: Math.floor((nowMs - ts) / 86_400_000) };
    }
  }
  return best;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-030 桌面午憩
// ---------------------------------------------------------------------------

export type SiestaState = "awake" | "siesta";

/** 10 分钟无输入进入午憩（阈值毫秒可注入便于单测）。 */
export function siestaState(nowMs: number, lastInputMs: number, thresholdMs = 600_000): SiestaState {
  return nowMs - lastInputMs >= thresholdMs ? "siesta" : "awake";
}

/** 唤醒过渡时长（ms）。 */
export const SIESTA_WAKE_MS = 400;

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-032 桌面地平线
// ---------------------------------------------------------------------------

/** 地平线 y（px）：图标群基线下一格下缘；空桌面回落 78% 视口高。 */
export function horizonY(maxRow: number, tier: IconTier, pad: number, vh: number): number {
  if (maxRow < 0) return Math.round(vh * 0.78);
  return Math.round(pad + (maxRow + 1) * tier.h + 2);
}

/** 纯色壁纸取对比色：亮底取深蓝灰、暗底取微光白；luma 未知回落 null（微光白）。 */
export function horizonColor(bgLuma: number | null): { color: string; alpha: number } {
  if (bgLuma === null) return { color: "#dfe8ff", alpha: 0.08 };
  return bgLuma > 0.55 ? { color: "#1b2432", alpha: 0.14 } : { color: "#dfe8ff", alpha: 0.08 };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-033 布局合影
// ---------------------------------------------------------------------------

export interface LayoutPhoto {
  id: string;
  ts: number;
  count: number;
  positions: Record<string, Cell>;
  /** 缩略图（SVG data URL，非全屏截图）。 */
  thumb: string;
}

/** 缩略图：格子点阵 SVG（体积恒小，远低于 60KB 上限）。 */
export function makeThumb(positions: Record<string, Cell>, cols = 12, rows = 8): string {
  const w = 120;
  const h = 80;
  const cw = w / cols;
  const ch = h / rows;
  let dots = "";
  for (const cell of Object.values(positions)) {
    const x = Math.min(w - 3, Math.max(1.5, cell.c * cw + cw / 2));
    const y = Math.min(h - 3, Math.max(1.5, cell.r * ch + ch / 2));
    dots += `<circle cx="${x.toFixed(1)}" cy="${y.toFixed(1)}" r="2.1"/>`;
  }
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">` +
    `<rect width="${w}" height="${h}" rx="6" fill="#0b0f18"/>` +
    `<g fill="#9db4e8">${dots}</g></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

/** 快照体积上限（60KB）。 */
export function snapshotSizeOk(photo: LayoutPhoto): boolean {
  try {
    return JSON.stringify(photo).length <= 60 * 1024;
  } catch {
    return false;
  }
}

export interface LayoutDiff {
  added: string[];
  removed: string[];
  moved: { id: string; from: Cell; to: Cell }[];
}

/** 合影差异：新增 / 离开 / 移动（三分类互斥，还原预览用）。 */
export function layoutDiff(cur: Record<string, Cell>, snap: Record<string, Cell>): LayoutDiff {
  const added: string[] = [];
  const removed: string[] = [];
  const moved: { id: string; from: Cell; to: Cell }[] = [];
  for (const [id, cell] of Object.entries(cur)) {
    const prev = snap[id];
    if (!prev) {
      added.push(id);
    } else if (prev.c !== cell.c || prev.r !== cell.r) {
      moved.push({ id, from: prev, to: cell });
    }
  }
  for (const id of Object.keys(snap)) if (!cur[id]) removed.push(id);
  return { added, removed, moved };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-034 图标层光雪
// ---------------------------------------------------------------------------

export interface SnowP {
  x: number;
  y: number;
  /** 下落速度 px/s（8–20）。 */
  vy: number;
  /** 摆幅与相位。 */
  sway: number;
  phase: number;
}

/** 晚间 18:00–06:00 自动开启（hour 24 制）。 */
export function snowActive(hour: number): boolean {
  return hour >= 18 || hour < 6;
}

export function snowSpawn(w: number, count = 6): SnowP[] {
  const out: SnowP[] = [];
  for (let i = 0; i < count; i++) {
    out.push({
      x: Math.random() * w,
      y: -2 - Math.random() * 8,
      vy: 8 + Math.random() * 12,
      sway: 4 + Math.random() * 8,
      phase: Math.random() * Math.PI * 2,
    });
  }
  return out;
}

/** 一步积分：下落 + 正弦摆动；落底回顶（x 重散）。 */
export function snowStep(p: SnowP, dtMs: number, w: number, h: number, nowMs: number): SnowP {
  const dt = Math.max(0, dtMs) / 1000;
  const y = p.y + p.vy * dt;
  const x = p.x + Math.sin(nowMs / 900 + p.phase) * p.sway * dt;
  if (y > h) {
    return { ...p, x: Math.random() * w, y: -2 };
  }
  return { ...p, x: ((x % w) + w) % w, y };
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** 绕流：光点进入图标矩形时横向推出（y 不变，视觉为「绕开图标」）。 */
export function snowAvoid(p: SnowP, rects: Rect[]): SnowP {
  for (const r of rects) {
    if (p.x > r.x - 3 && p.x < r.x + r.w + 3 && p.y > r.y && p.y < r.y + r.h) {
      const mid = r.x + r.w / 2;
      return { ...p, x: p.x < mid ? r.x - 3 : r.x + r.w + 3 };
    }
  }
  return p;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-035 声音地形
// ---------------------------------------------------------------------------

/** 声像角：X 坐标线性映射（-1 左 .. 1 右；音量恒定由调用方保证）。 */
export function panForX(x: number, width: number): number {
  if (width <= 0) return 0;
  return Math.min(1, Math.max(-1, (x / width) * 2 - 1));
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-036 图标结组（会话级）
// ---------------------------------------------------------------------------

export interface IconCluster {
  id: string;
  members: string[];
  /** 视觉展开态（Enter 开合）。 */
  open: boolean;
}

export function makeCluster(members: string[]): IconCluster {
  return { id: `ncluster-${Date.now().toString(36)}-${Math.floor(Math.random() * 1e6).toString(36)}`, members: [...members], open: false };
}

/** 组包围格（环抱边框几何）。 */
export function clusterBounds(cells: Record<string, Cell>, members: string[]): { minC: number; minR: number; maxC: number; maxR: number } | null {
  let minC = Infinity;
  let minR = Infinity;
  let maxC = -Infinity;
  let maxR = -Infinity;
  for (const m of members) {
    const cell = cells[m];
    if (!cell) continue;
    minC = Math.min(minC, cell.c);
    minR = Math.min(minR, cell.r);
    maxC = Math.max(maxC, cell.c);
    maxR = Math.max(maxR, cell.r);
  }
  return Number.isFinite(minC) ? { minC, minR, maxC, maxR } : null;
}

/** 组员被拖出即解散语义：拖拽的 id 属于组 → 自动解散。 */
export function clusterTouched(cluster: IconCluster, draggedId: string): boolean {
  return cluster.members.includes(draggedId);
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-037 桌面标尺模式
// ---------------------------------------------------------------------------

export interface RulerRead {
  px: number;
  neighborId: string | null;
  /** 拖拽点是否吸附在网格（SNAP 徽标）。 */
  snapped: boolean;
}

/** 拖拽点 → 最近格（钳制在网格内）。 */
export function nearestCell(px: number, py: number, tier: IconTier, pad: number, cols: number, rows: number): Cell {
  const c = Math.min(cols - 1, Math.max(0, Math.round((px - pad - tier.w / 2) / tier.w)));
  const r = Math.min(rows - 1, Math.max(0, Math.round((py - pad - tier.h / 2) / tier.h)));
  return { c, r };
}

/** 与最近邻图标的像素距离（格中心距；最近邻取欧氏最小）。
 * 孤立（唯一图标即拖拽者）→ 距离 0 / 无邻居；selfCell 用于排除拖拽图标自身家格。 */
export function neighborDistance(
  cells: Record<string, Cell>,
  activeCell: Cell,
  tier: IconTier,
  selfCell?: Cell,
): RulerRead {
  const entries = Object.entries(cells);
  if (entries.length <= 1) return { px: 0, neighborId: null, snapped: true }; // 孤立：无邻居
  let bestId: string | null = null;
  let bestPx = Infinity;
  for (const [id, cell] of entries) {
    if (cell.c === activeCell.c && cell.r === activeCell.r) continue; // 活动格即自身
    if (selfCell && cell.c === selfCell.c && cell.r === selfCell.r) continue; // 拖拽图标家格
    const dx = (cell.c - activeCell.c) * tier.w;
    const dy = (cell.r - activeCell.r) * tier.h;
    const d = Math.hypot(dx, dy);
    if (d < bestPx) {
      bestPx = d;
      bestId = id;
    }
  }
  return { px: Number.isFinite(bestPx) ? Math.round(bestPx) : 0, neighborId: bestId, snapped: true };
}

/** SNAP 徽标：拖拽视觉位置落在任一格中心 ±8px 内。 */
export function snapBadge(px: number, py: number, tier: IconTier, pad: number): boolean {
  const dx = ((px - pad - tier.w / 2) % tier.w + tier.w) % tier.w;
  const dy = ((py - pad - tier.h / 2) % tier.h + tier.h) % tier.h;
  const cd = Math.min(dx, tier.w - dx);
  const cdy = Math.min(dy, tier.h - dy);
  return cd <= 8 && cdy <= 8;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-038 原点坐标
// ---------------------------------------------------------------------------

/** 首次落点记录：仅补录未见过的图标（原点 = 首次放置，非安装默认）。 */
export function firstSeen(existing: Record<string, number>, ids: string[], nowMs: number): Record<string, number> {
  const out = { ...existing };
  for (const id of ids) if (out[id] === undefined) out[id] = nowMs;
  return out;
}

/** 首次落点格子账本：仅记录首次出现的 (id → cell)。 */
export function firstCell(existing: Record<string, Cell>, positions: Record<string, Cell>): Record<string, Cell> {
  const out = { ...existing };
  for (const [id, cell] of Object.entries(positions)) if (!out[id]) out[id] = { ...cell };
  return out;
}

/** 回原点计划：有原点记录且当前位置 ≠ 原点的图标。 */
export function homeRestorePlan(positions: Record<string, Cell>, homes: Record<string, Cell>): { id: string; to: Cell }[] {
  const out: { id: string; to: Cell }[] = [];
  for (const [id, home] of Object.entries(homes)) {
    const cur = positions[id];
    if (!cur) continue;
    if (cur.c !== home.c || cur.r !== home.r) out.push({ id, to: { ...home } });
  }
  return out;
}

// ---------------------------------------------------------------------------
// 本地账本（nova.desk.* 键空间；损坏即重置）
// ---------------------------------------------------------------------------

const LS_FEAT = "nova.desk.features";
const LS_USAGE = "nova.desk.usage";
const LS_HOMES = "nova.desk.homes";
const LS_PHOTOS = "nova.desk.photos";
const LS_IMMUNE = "nova.desk.retireImmune";
const LS_CONST_BAK = "nova.desk.constBackup";

interface UsageLedger {
  /** id → 最近使用时刻（ms）。 */
  lastUse: Record<string, number>;
  /** 月度点击：YYYY-MM → { id → 次数 }（排行榜月度滚动）。 */
  months: Record<string, Record<string, number>>;
}

function readJSON<T>(key: string, guard: (v: unknown) => v is T): T | null {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return null;
    const v = JSON.parse(raw) as unknown;
    return guard(v) ? v : null;
  } catch {
    return null;
  }
}

function writeJSON(key: string, v: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    /* 配额满/被禁 → 不持久化 */
  }
}

const isRec = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const isUsage = (v: unknown): v is UsageLedger => isRec(v) && isRec((v as unknown as UsageLedger).lastUse) && isRec((v as unknown as UsageLedger).months);
const isCell = (v: unknown): v is Cell => isRec(v) && typeof (v as unknown as Cell).c === "number" && typeof (v as unknown as Cell).r === "number";
const isCellMap = (v: unknown): v is Record<string, Cell> => isRec(v) && Object.values(v).every(isCell);
const isHomes = (v: unknown): v is Record<string, { cell: Cell; ts: number }> =>
  isRec(v) && Object.values(v).every((x) => isRec(x) && isCell((x as { cell: Cell }).cell) && typeof (x as { ts: number }).ts === "number");
const isPhotos = (v: unknown): v is LayoutPhoto[] => Array.isArray(v) && v.every((p) => isRec(p) && typeof (p as unknown as LayoutPhoto).id === "string" && isCellMap((p as unknown as LayoutPhoto).positions));

export function monthKey(d = new Date()): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
}

export function loadUsage(): UsageLedger {
  return readJSON<UsageLedger>(LS_USAGE, isUsage) ?? { lastUse: {}, months: {} };
}

export function saveUsage(u: UsageLedger): void {
  writeJSON(LS_USAGE, u);
}

/** 记一次图标使用（点击/打开/任何交互即唤醒）。 */
export function recordIconUse(id: string, nowMs = Date.now()): UsageLedger {
  const u = loadUsage();
  u.lastUse[id] = nowMs;
  const mk = monthKey(new Date(nowMs));
  const m = u.months[mk] ?? {};
  m[id] = (m[id] ?? 0) + 1;
  // 月度滚动：仅保留最近 3 个月
  const keys = Object.keys(u.months).sort();
  while (keys.length > 3) {
    const drop = keys.shift();
    if (drop) delete u.months[drop];
  }
  u.months[mk] = m;
  saveUsage(u);
  return u;
}

export function currentMonthCounts(u: UsageLedger = loadUsage()): Record<string, number> {
  return u.months[monthKey()] ?? {};
}

export function loadHomes(): Record<string, { cell: Cell; ts: number }> {
  return readJSON(LS_HOMES, isHomes) ?? {};
}

export function saveHomes(h: Record<string, { cell: Cell; ts: number }>): void {
  writeJSON(LS_HOMES, h);
}

const PHOTO_MAX = 24;

export function loadPhotos(): LayoutPhoto[] {
  return readJSON<LayoutPhoto[]>(LS_PHOTOS, isPhotos) ?? [];
}

export function savePhoto(p: LayoutPhoto): LayoutPhoto[] {
  const list = [...loadPhotos(), p].slice(-PHOTO_MAX);
  writeJSON(LS_PHOTOS, list);
  return list;
}

export function deletePhoto(id: string): LayoutPhoto[] {
  const list = loadPhotos().filter((p) => p.id !== id);
  writeJSON(LS_PHOTOS, list);
  return list;
}

export function loadImmunity(): Record<string, number> {
  return readJSON<Record<string, number>>(LS_IMMUNE, (v): v is Record<string, number> => isRec(v) && Object.values(v).every((x) => typeof x === "number")) ?? {};
}

export function setImmunity(id: string, untilMs: number): void {
  const m = loadImmunity();
  m[id] = untilMs;
  writeJSON(LS_IMMUNE, m);
}

// ---------------------------------------------------------------------------
// 功能注册表（S0 中枢可直接消费的元数据；nova.desk.features 存覆盖档）
// ---------------------------------------------------------------------------

export const DESK_FEATURES: NovaFeatureMeta[] = [
  { id: "W-026", title: "图标星座排列", titleEn: "Constellation Layout", desc: "沿六座真实星座曲线排布图标，星图细线相连；双击空白切回网格。", defaultOn: false, params: { constellation: { def: "orion", note: "星座 id" } } },
  { id: "W-027", title: "图标养成", titleEn: "Icon Growth", desc: "高频图标阴影加深、底光更饱和；30 天未点渐入半休眠，交互即唤醒。", defaultOn: false, params: { dormancyDays: { def: 30, note: "半休眠天数" } } },
  { id: "W-028", title: "季节地影", titleEn: "Seasonal Ground", desc: "图标接触阴影随真实季节微调色温（南半球自动反转）。", defaultOn: false, params: { hemisphere: { def: "north", note: "north|south" } } },
  { id: "W-029", title: "图标排行榜", titleEn: "Icon Leaderboard", desc: "本月点击 Top10 领奖台 + 沉睡王一键归档建议（接 V-96 语义）。", defaultOn: true },
  { id: "W-030", title: "桌面午憩", titleEn: "Desk Siesta", desc: "10 分钟无输入进入午憩态（柔化降亮停微动），任一输入 400ms 全醒。", defaultOn: false, params: { idleMinutes: { def: 10, note: "空闲分钟数" } } },
  { id: "W-031", title: "图标老化公约", titleEn: "Icon Aging", desc: "7 天未用每周 -3% 透明度（下限 60%）；30 天触发一次性退休提案，拒绝后 90 天免疫。", defaultOn: false },
  { id: "W-032", title: "桌面地平线", titleEn: "Desktop Floor", desc: "图标基线下 2px 微光地平线（8% 透明），不拦截鼠标；纯色壁纸取对比色。", defaultOn: false },
  { id: "W-033", title: "布局合影", titleEn: "Layout Photo", desc: "视觉快照相册（缩略图 + 日期 + 图标数）；历史合影一键差异对比还原。", defaultOn: true },
  { id: "W-034", title: "图标层光雪", titleEn: "Icon Snowfall", desc: "图标层独立微光飘雪，穿过图标轻微绕流；晚间 18:00–06:00 自动开启。", defaultOn: false },
  { id: "W-035", title: "声音地形", titleEn: "Panned Clicks", desc: "图标确认微音带声像定位（左半屏偏左、右半屏偏右，音量恒定）。", defaultOn: false, params: { volume: { def: 0.5, note: "0..1" } } },
  { id: "W-036", title: "图标结组", titleEn: "Icon Clusters", desc: "框选 ≥2 图标后按 G 临时结组（环抱边框随组位移）；Enter 开合、Esc 解散、拖出即散。", defaultOn: true },
  { id: "W-037", title: "桌面标尺模式", titleEn: "Spacing Ruler", desc: "拖动图标时按住 R 显示与最近邻的像素距离标尺；吸附时显示 SNAP 徽标。", defaultOn: true },
  { id: "W-038", title: "原点坐标", titleEn: "Home Position", desc: "记住每枚图标首次落点；全体/个体回原点；V-04 动作槽让位仲裁。", defaultOn: true, params: { dblHome: { def: false, note: "双击空白回原点（仅 V-04=none 时生效）" } } },
];

/** 本模块侧开关账本（S0 缺席时的独立真源；S0 在场时以 ctx 为准）。 */
export function featureDefaults(): Record<string, boolean> {
  const out: Record<string, boolean> = {};
  for (const f of DESK_FEATURES) out[f.id] = f.defaultOn;
  return out;
}

export function loadFeatureOverrides(): Record<string, boolean> {
  return readJSON<Record<string, boolean>>(LS_FEAT, (v): v is Record<string, boolean> => isRec(v) && Object.values(v).every((x) => typeof x === "boolean")) ?? {};
}

export function saveFeatureOverride(id: string, on: boolean): void {
  const m = loadFeatureOverrides();
  m[id] = on;
  writeJSON(LS_FEAT, m);
}

// ---------------------------------------------------------------------------
// 行为层（非 DOM 环境安全 no-op）
// ---------------------------------------------------------------------------

const GRID_PAD = 14; // 与 DesktopIcons.tsx 的 GRID_PAD 一致（只读几何事实）

function iconRoot(): HTMLElement | null {
  if (typeof document === "undefined") return null;
  return document.querySelector<HTMLElement>(".desktop-icons");
}

function icons(): HTMLElement[] {
  const root = iconRoot();
  if (!root) return [];
  return Array.from(root.querySelectorAll<HTMLElement>(".desktop-icon"));
}

function currentTier(): IconTier {
  return tierForPx(loadIconPx() ?? 48);
}

/** DOM 内联位置 → 格（与 DesktopIcons 的 left/top 公式互逆）。 */
function cellOfEl(el: HTMLElement, tier: IconTier): Cell | null {
  const l = Number.parseFloat(el.style.left);
  const t = Number.parseFloat(el.style.top);
  if (!Number.isFinite(l) || !Number.isFinite(t)) return null;
  return { c: Math.round((l - GRID_PAD) / tier.w), r: Math.round((t - GRID_PAD) / tier.h) };
}

function visiblePositions(): { positions: Record<string, Cell>; ids: string[] } {
  const l = loadDesktopLayout();
  const shelved = allShelfMembers(l);
  const positions: Record<string, Cell> = {};
  for (const [id, cell] of Object.entries(l.positions)) if (!shelved.has(id)) positions[id] = cell;
  return { positions, ids: Object.keys(positions) };
}

function notifyLayoutReload(): void {
  if (typeof window === "undefined" || typeof window.dispatchEvent !== "function") return;
  window.dispatchEvent(new CustomEvent(RELOAD_LAYOUT_EVENT));
}

/** 延迟视觉后续（仅真实 DOM 环境；数据路径在无 DOM 环境也完整可测）。 */
function later(fn: () => void, ms: number): void {
  if (!state || typeof document === "undefined") return;
  const h = setTimeout(() => {
    fn();
  }, ms);
  state.timers.push(h);
}

/** 模块内建样式（nova-desk- 前缀；一次注入，随停随卸）。 */
function injectStyles(): Unsub {
  if (typeof document === "undefined") return () => {};
  const id = "nova-desk-style";
  document.getElementById(id)?.remove();
  const el = document.createElement("style");
  el.id = id;
  el.textContent = `
#nova-desk-layer { position: fixed; inset: 0; pointer-events: none; z-index: 30; }
#nova-desk-layer .nova-desk-panel { pointer-events: auto; }
.nova-desk-lines { position: absolute; inset: 0; width: 100%; height: 100%; }
.nova-desk-lines line { stroke: #9db4e8; stroke-opacity: .12; stroke-width: 1; }
.nova-desk-horizon { position: absolute; left: 0; right: 0; height: 2px; pointer-events: none; }
.nova-desk-snow { position: absolute; inset: 0; width: 100%; height: 100%; }
.nova-desk-ruler { position: absolute; pointer-events: none; font: 11px/1.4 Consolas, monospace; letter-spacing: .08em; color: #cfe0ff; text-transform: uppercase; }
.nova-desk-ruler .nova-desk-ruler-line { position: absolute; height: 1px; background: #9db4e8; transform-origin: 0 50%; }
.nova-desk-ruler .nova-desk-ruler-badge { position: absolute; padding: 2px 6px; border: 1px solid #9db4e8; border-radius: 4px; background: oklch(0.15 0.02 262 / 0.8); transform: translate(-50%, -140%); }
.nova-desk-ring { position: absolute; border: 1px solid oklch(0.72 0.09 260 / 0.55); border-radius: 10px; box-shadow: 0 0 12px oklch(0.72 0.09 260 / 0.18) inset; transition: all var(--dur-4, 200ms) var(--ease-standard, ease); }
.nova-desk-ring.open { border-style: dashed; }
.nova-desk-siesta .desktop-icon { filter: brightness(.88) saturate(.82); transition: filter 400ms ease; }
.nova-desk-veil { position: absolute; inset: 0 0 auto 0; height: 0; background: transparent; }
.nova-desk-panel { position: absolute; right: 18px; top: 64px; width: 340px; max-height: min(70vh, 560px); overflow: auto; padding: 14px; border-radius: var(--r-card, 12px); border: 1px solid oklch(0.7 0.02 262 / 0.2); background: var(--tooltip-bg, oklch(0.22 0.02 262 / 0.94)); color: var(--tooltip-fg, #e8eef8); box-shadow: var(--elev-3, 0 6px 18px oklch(0.15 0.02 260 / 0.28)); font-size: var(--fs-13, 13px); }
.nova-desk-panel h3 { margin: 0 0 8px; font-size: var(--fs-15, 15px); letter-spacing: .04em; }
.nova-desk-panel .nova-desk-close { position: absolute; right: 10px; top: 10px; border: 0; background: transparent; color: inherit; cursor: pointer; font-size: 14px; }
.nova-desk-rows { display: grid; gap: 6px; }
.nova-desk-row { display: flex; align-items: center; gap: 8px; padding: 6px 8px; border-radius: var(--r-chip, 4px); background: oklch(1 0 0 / 0.04); }
.nova-desk-row .nova-desk-medal { width: 18px; text-align: center; }
.nova-desk-row.gold .nova-desk-medal { color: #f2cf7a; }
.nova-desk-row.silver .nova-desk-medal { color: #c9d4e8; }
.nova-desk-row.bronze .nova-desk-medal { color: #d8a878; }
.nova-desk-row .nova-desk-cnt { margin-left: auto; font: 11px Consolas, monospace; opacity: .8; }
.nova-desk-empty { opacity: .7; padding: 18px 6px; text-align: center; }
.nova-desk-foot { margin-top: 10px; display: flex; gap: 8px; flex-wrap: wrap; }
.nova-desk-btn { border: 1px solid oklch(0.7 0.02 262 / 0.25); background: transparent; color: inherit; border-radius: var(--r-control, 8px); padding: 5px 10px; cursor: pointer; font-size: 12px; }
.nova-desk-btn:hover { background: oklch(1 0 0 / 0.06); }
.nova-desk-photo { display: grid; grid-template-columns: 96px 1fr; gap: 10px; align-items: center; padding: 8px; border-radius: var(--r-control, 8px); background: oklch(1 0 0 / 0.04); }
.nova-desk-photo img { width: 96px; height: 64px; border-radius: 6px; }
.nova-desk-dim { opacity: .72; font-size: 12px; }
.nova-desk-diff-added { color: #9fe0b0; }
.nova-desk-diff-removed { color: #f0a3a8; }
.nova-desk-diff-moved { color: #ffd9a0; }
.nova-desk-retire { margin-top: 10px; padding: 10px; border: 1px solid oklch(0.75 0.06 80 / 0.35); border-radius: var(--r-control, 8px); }
`;
  document.head.appendChild(el);
  return () => el.remove();
}

function deskLayer(): HTMLElement {
  if (typeof document === "undefined") throw new Error("no document");
  let el = document.getElementById("nova-desk-layer");
  if (!el) {
    el = document.createElement("div");
    el.id = "nova-desk-layer";
    document.body.appendChild(el);
  }
  return el;
}

function makeEl<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  if (className) el.className = className;
  return el;
}

/** 模块内双语文案（lang = "zh" | "en"）。 */
type Lang = "zh" | "en";
function t(lang: Lang, key: string): string {
  const D: Record<string, [string, string]> = {
    leaderboard: ["图标排行榜", "Icon Leaderboard"],
    photos: ["布局合影", "Layout Photos"],
    shot: ["给布局拍张合影", "Snapshot layout"],
    restore: ["对比还原", "Compare & restore"],
    empty: ["桌面还没有图标", "No icons on desktop"],
    sleepKing: ["沉睡王", "Sleep King"],
    archive: ["建议归档", "Suggest archive"],
    close: ["关闭", "Close"],
    diffAdded: ["新增", "Added"],
    diffRemoved: ["离开", "Removed"],
    diffMoved: ["移动", "Moved"],
    diffNone: ["与合影完全一致", "Identical to photo"],
    apply: ["确认还原", "Restore now"],
    cancel: ["取消", "Cancel"],
    retireTitle: ["退休提案", "Retirement proposal"],
    retireBody: ["已 30 天未用，建议移入归档文件架（可逆）", "Unused for 30 days — archive it (reversible)"],
    retireYes: ["建议归档", "Archive"],
    retireNo: ["留下", "Keep"],
    retireNever: ["永不再提", "Never ask"],
    icons: ["枚图标", "icons"],
    homeDone: ["已回原点", "Back to home positions"],
  };
  const e = D[key];
  if (!e) return key;
  return lang === "en" ? e[1] : e[0];
}

function htmlToEl(html: string): HTMLElement {
  const tpl = makeEl("template");
  tpl.innerHTML = html.trim();
  return (tpl.content.firstElementChild as HTMLElement) ?? makeEl("div");
}

function panelShell(title: string, lang: Lang): { card: HTMLElement; body: HTMLElement } {
  const card = htmlToEl(
    `<div class="nova-desk-panel" role="dialog" aria-label="${title}">` +
      `<button class="nova-desk-close" aria-label="${t(lang, "close")}">✕</button><h3></h3><div class="nova-desk-body"></div></div>`,
  );
  (card.querySelector("h3") as HTMLElement).textContent = title;
  const close = card.querySelector(".nova-desk-close") as HTMLElement;
  close.addEventListener("click", () => card.remove());
  const body = card.querySelector(".nova-desk-body") as HTMLElement;
  return { card, body };
}

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c] as string));
}

/** DOM 图标显示名（读 .desktop-icon-label 文本；只读）。 */
function labelOfEl(el: HTMLElement): string {
  const s = el.querySelector<HTMLElement>(".desktop-icon-label");
  return s?.textContent?.trim() || "";
}

// ---- 模块态 ----

interface DeskState {
  subs: Unsub[];
  raf: number | null;
  followRaf: number | null;
  timers: ReturnType<typeof setTimeout>[];
  styleUn: Unsub | null;
  cluster: IconCluster | null;
  lastInputMs: number;
  siesta: boolean;
  rDown: boolean;
  audio: { muted: boolean; volume: number };
  lang: Lang;
  ctx: DeskNovaCtx | null;
  constActive: string | null;
  retireShownAt: Record<string, number>;
}

function freshState(): DeskState {
  return {
    subs: [], raf: null, followRaf: null, timers: [], styleUn: null, cluster: null,
    lastInputMs: Date.now(), siesta: false, rDown: false,
    audio: { muted: false, volume: 0.5 }, lang: "zh", ctx: null, constActive: null,
    retireShownAt: {},
  };
}

let state: DeskState | null = null;

function on(target: EventTarget, type: string, fn: EventListener, opts?: AddEventListenerOptions): void {
  if (!state) return;
  target.addEventListener(type, fn, opts);
  state.subs.push(() => target.removeEventListener(type, fn, opts));
}

// ---- W-026 行为：星座排布 / 星图连线 / 双击回网格 ----

function clearConstellationVisual(): void {
  if (typeof document === "undefined") return;
  document.querySelector("#nova-desk-layer .nova-desk-lines")?.remove();
}

function drawConstellationLines(constId: string): void {
  const c = constellationById(constId);
  if (!c) return;
  const { positions } = visiblePositions();
  const tier = currentTier();
  if (typeof window !== "undefined") {
    const vh = window.innerHeight;
    const cols = Math.max(1, Math.floor((window.innerWidth - GRID_PAD * 2) / tier.w));
    const rows = Math.max(1, Math.floor((vh - GRID_PAD * 2) / tier.h));
    void cols;
    void rows;
  }
  const cells = Object.values(positions);
  const lines = constellationLinesPx(c, cells.slice(0, c.points.length * 2), tier, GRID_PAD);
  const layer = deskLayer();
  clearConstellationVisual();
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", "nova-desk-lines");
  for (const l of lines) {
    const ln = document.createElementNS("http://www.w3.org/2000/svg", "line");
    ln.setAttribute("x1", String(l.x1));
    ln.setAttribute("y1", String(l.y1));
    ln.setAttribute("x2", String(l.x2));
    ln.setAttribute("y2", String(l.y2));
    svg.appendChild(ln);
  }
  layer.appendChild(svg);
}

function backupGridOnce(): void {
  try {
    if (!localStorage.getItem(LS_CONST_BAK)) {
      const l = loadDesktopLayout();
      localStorage.setItem(LS_CONST_BAK, JSON.stringify({ positions: l.positions }));
    }
  } catch {
    /* 忽略 */
  }
}

/** 应用星座布局（写布局存储 + 通知重载；不触碰组件代码）。 */
function applyConstellation(id: string): boolean {
  const c = constellationById(id);
  if (!c || typeof window === "undefined") return false;
  const vw = Number(window.innerWidth);
  const vh = Number(window.innerHeight);
  if (!Number.isFinite(vw) || !Number.isFinite(vh) || vw <= 0 || vh <= 0) return false;
  const tier = currentTier();
  const cols = Math.max(1, Math.floor((vw - GRID_PAD * 2) / tier.w));
  const rows = Math.max(1, Math.floor((vh - GRID_PAD * 2) / tier.h));
  const layout = loadDesktopLayout();
  const shelved = allShelfMembers(layout);
  const ids = Object.keys(layout.positions).filter((k) => !shelved.has(k));
  if (ids.length === 0) return false;
  backupGridOnce();
  const cells = constellationCells(c, ids.length, cols, rows);
  const next: DesktopLayout = { ...layout, autoArrange: false, positions: { ...layout.positions } };
  ids.forEach((bid, i) => {
    const cell = cells[i];
    if (cell) next.positions[bid] = cell;
  });
  saveDesktopLayout(next);
  if (state) state.constActive = id;
  notifyLayoutReload();
  later(() => drawConstellationLines(id), 60);
  return true;
}

/** 切回网格（恢复备份或直接交还 autoArrange）。 */
function restoreGrid(): boolean {
  if (typeof window === "undefined") return false;
  clearConstellationVisual();
  if (state) state.constActive = null;
  try {
    const raw = localStorage.getItem(LS_CONST_BAK);
    if (raw) {
      const bak = JSON.parse(raw) as { positions?: Record<string, Cell> };
      if (bak.positions) {
        const layout = loadDesktopLayout();
        saveDesktopLayout({ ...layout, positions: { ...layout.positions, ...bak.positions } });
        localStorage.removeItem(LS_CONST_BAK);
        notifyLayoutReload();
        return true;
      }
    }
  } catch {
    /* 损坏备份 → 忽略 */
  }
  const layout = loadDesktopLayout();
  if (layout.autoArrange) return false; // 无备份且已是网格 → 无可恢复
  saveDesktopLayout({ ...layout, autoArrange: true });
  notifyLayoutReload();
  return true;
}

function mountConstellation(): void {
  // 双击空白切回网格（只读监听；V-04 动作由 DesktopIcons 自行处理，互不影响）
  on(window, "dblclick", (ev: Event) => {
    const e = ev as MouseEvent;
    const root = iconRoot();
    if (!root || e.target !== root) return;
    if (!state?.ctx?.on("W-026")) return;
    if (state.constActive) restoreGrid();
  });
}

// ---- W-027 / W-031 行为：养成 + 老化观感（逐图标自定属性） ----

function daysIdleOf(ts: number | undefined, nowMs: number): number {
  if (ts === undefined) return 0; // 从未使用 → 视为新鲜（不惩罚新图标）
  return Math.max(0, Math.floor((nowMs - ts) / 86_400_000));
}

function iconFilter(glow: number, season: Season | null): string {
  const parts: string[] = [];
  if (glow > 0) parts.push(`drop-shadow(0 0 ${(glow * 6).toFixed(1)}px oklch(0.78 0.1 260 / ${(glow * 0.5).toFixed(2)}))`);
  if (season) {
    const s = SEASON_SHADOW[season];
    parts.push(`drop-shadow(0 ${s.offY}px ${s.blur}px ${s.tint}${Math.round(s.alpha * 255).toString(16).padStart(2, "0")})`);
  }
  return parts.length ? parts.join(" ") : "";
}

function stylePass(nowMs = Date.now()): void {
  const ctx = state?.ctx;
  if (!ctx) return;
  const tier = currentTier();
  const usage = loadUsage();
  const immune = loadImmunity();
  const hemi = (ctx.str("W-028", "hemisphere") === "south" ? "south" : "north") as Hemisphere;
  const season = ctx.on("W-028") ? seasonOf(new Date(nowMs).getMonth(), hemi) : null;
  const dormancyDays = Math.max(7, Math.round(ctx.num("W-027", "dormancyDays") || 30));
  const root = iconRoot();
  if (root) root.classList.toggle("nova-desk-season", season !== null);
  // 沉睡王退休提案（W-031；一次性 + 免疫）
  if (ctx.on("W-031")) maybeRetireProposal(usage, immune, nowMs);
  for (const el of icons()) {
    const cell = cellOfEl(el, tier);
    if (!cell) continue;
    const layout = loadDesktopLayout();
    const shelved = allShelfMembers(layout);
    // 反查 id（cell → id）
    let id: string | null = null;
    for (const [bid, bc] of Object.entries(layout.positions)) {
      if (shelved.has(bid)) continue;
      if (bc.c === cell.c && bc.r === cell.r) {
        id = bid;
        break;
      }
    }
    if (!id) continue;
    let opacity = 1;
    if (ctx.on("W-027")) {
      opacity = Math.min(opacity, dormancyOpacity(daysIdleOf(usage.lastUse[id], nowMs), dormancyDays));
    }
    if (ctx.on("W-031")) {
      const until = immune[id] ?? 0;
      const immuneNow = nowMs < until;
      if (!immuneNow) opacity = Math.min(opacity, agingOpacity(daysIdleOf(usage.lastUse[id], nowMs)));
    }
    el.style.opacity = opacity < 1 ? opacity.toFixed(3) : "";
    let filter = "";
    if (ctx.on("W-027")) {
      const month = currentMonthCounts(usage);
      const maxRef = Math.max(1, ...Object.values(month));
      filter = iconFilter(growthGlow(month[id] ?? 0, maxRef), null);
    }
    if (season) filter = [filter, iconFilter(0, season)].filter(Boolean).join(" ");
    el.style.filter = filter || "";
  }
  // 原点补录（W-038）
  if (ctx.on("W-038")) recordHomesPass();
}

function maybeRetireProposal(usage: UsageLedger, immune: Record<string, number>, nowMs: number): void {
  if (state?.retireShownAt == null) return;
  let candidate: { id: string; days: number } | null = null;
  for (const [id, ts] of Object.entries(usage.lastUse)) {
    if (id.startsWith("sys-")) continue;
    const until = immune[id] ?? 0;
    if (nowMs < until) continue;
    const days = Math.floor((nowMs - ts) / 86_400_000);
    if (retireDue(days) && (state.retireShownAt[id] ?? 0) === 0) {
      if (!candidate || days > candidate.days) candidate = { id, days };
    }
  }
  if (!candidate) return;
  state.retireShownAt[candidate.id] = nowMs;
  showRetireCard(candidate.id, candidate.days);
}

/** 退休提案卡（W-031；建议接 V-96 归档语义，拒绝 90 天免疫）。 */
function showRetireCard(id: string, days: number): void {
  const lang = state?.lang ?? "zh";
  const { card, body } = panelShell(t(lang, "retireTitle"), lang);
  card.classList.add("nova-desk-retire-card");
  body.innerHTML =
    `<div class="nova-desk-retire"><div>${esc(t(lang, "retireBody"))} · ${days}d</div>` +
    `<div class="nova-desk-foot">` +
    `<button class="nova-desk-btn" data-act="yes">${t(lang, "retireYes")}</button>` +
    `<button class="nova-desk-btn" data-act="no">${t(lang, "retireNo")}</button>` +
    `<button class="nova-desk-btn" data-act="never">${t(lang, "retireNever")}</button></div></div>`;
  body.querySelector('[data-act="yes"]')?.addEventListener("click", () => {
    window.dispatchEvent(new CustomEvent(DESK_ARCHIVE_HINT_EVENT, { detail: { id } }));
    card.remove();
  });
  body.querySelector('[data-act="no"]')?.addEventListener("click", () => card.remove());
  body.querySelector('[data-act="never"]')?.addEventListener("click", () => {
    setImmunity(id, immuneUntil(Date.now()));
    card.remove();
  });
  deskLayer().appendChild(card);
}

function recordHomesPass(): void {
  const homes = loadHomes();
  const { positions } = visiblePositions();
  const now = Date.now();
  let changed = false;
  for (const [id, cell] of Object.entries(positions)) {
    if (!homes[id]) {
      homes[id] = { cell: { ...cell }, ts: now };
      changed = true;
    }
  }
  if (changed) saveHomes(homes);
}

// ---- W-030 行为：午憩 ----

function mountSiesta(): void {
  const wake = (): void => {
    if (!state) return;
    state.lastInputMs = Date.now();
    if (state.siesta) {
      state.siesta = false;
      iconRoot()?.classList.remove("nova-desk-siesta");
    }
  };
  for (const type of ["pointermove", "pointerdown", "keydown", "wheel"] as const) {
    on(window, type, wake, { passive: true });
  }
  const tick = (): void => {
    if (!state?.ctx?.on("W-030")) return;
    if (!state.ctx.motionOK()) return; // safeMode / reduce-motion 不进入（保持静止）
    const minutes = Math.max(1, Math.round(state.ctx.num("W-030", "idleMinutes") || 10));
    if (siestaState(Date.now(), state.lastInputMs, minutes * 60_000) === "siesta" && !state.siesta) {
      state.siesta = true;
      iconRoot()?.classList.add("nova-desk-siesta");
    }
  };
  const h = setInterval(tick, 15_000);
  state?.subs.push(() => clearInterval(h));
}

// ---- W-032 行为：地平线 ----

function mountHorizon(): void {
  const render = (): void => {
    const ctx = state?.ctx;
    if (!ctx?.on("W-032")) return;
    const layer = deskLayer();
    layer.querySelector(".nova-desk-horizon")?.remove();
    const tier = currentTier();
    const { positions } = visiblePositions();
    const maxRow = Object.values(positions).reduce((a, c) => Math.max(a, c.r), -1);
    const y = horizonY(maxRow, tier, GRID_PAD, window.innerHeight);
    // 纯色壁纸取对比色；其余回落微光白
    let luma: number | null = null;
    const solid = document.querySelector<HTMLElement>(".wallpaper-solid");
    if (solid) {
      const m = /rgba?\((\d+)[,\s]+(\d+)[,\s]+(\d+)/.exec(getComputedStyle(solid).backgroundColor);
      if (m) luma = (Number(m[1]) * 0.299 + Number(m[2]) * 0.587 + Number(m[3]) * 0.114) / 255;
    }
    const col = horizonColor(luma);
    const el = makeEl("div", "nova-desk-horizon");
    el.style.top = `${y}px`;
    el.style.background = col.color;
    el.style.opacity = String(col.alpha);
    layer.appendChild(el);
  };
  on(window, "resize", render);
  on(window, RELOAD_LAYOUT_EVENT, render);
  later(render, 120);
  later(render, 900);
}

// ---- W-034 行为：光雪 ----

function mountSnow(): void {
  let canvas: HTMLCanvasElement | null = null;
  let particles: SnowP[] = [];
  let lastT = 0;
  let acc = 0;
  let iconRects: Rect[] = [];
  const collectRects = (): void => {
    iconRects = icons().slice(0, 200).map((el) => {
      const r = el.getBoundingClientRect();
      return { x: r.left, y: r.top, w: r.width, h: r.height };
    });
  };
  const frame = (ts: number): void => {
    const ctx = state?.ctx;
    if (!state || !canvas) return;
    // 循环保活：门禁不满足时清屏待命（开关切换即时生效，无需重启）
    const allowed = !!ctx?.on("W-034") && !!ctx?.motionOK() && (ctx.bool("W-034", "manual") || snowActive(new Date().getHours()));
    if (!allowed) {
      canvas.getContext("2d")?.clearRect(0, 0, canvas.width, canvas.height);
      lastT = ts;
      state.raf = requestAnimationFrame(frame);
      return;
    }
    const w = canvas.width = window.innerWidth;
    const h = canvas.height = window.innerHeight;
    const dt = lastT ? ts - lastT : 16;
    lastT = ts;
    acc += dt;
    while (acc >= 1000 / 6) {
      particles.push(...snowSpawn(w, 1));
      acc -= 1000 / 6;
    }
    if (particles.length > 120) particles = particles.slice(-120);
    const g = canvas.getContext("2d");
    if (!g) return;
    g.clearRect(0, 0, w, h);
    g.fillStyle = "oklch(0.95 0.02 260 / 0.8)";
    particles = particles.map((p) => snowAvoid(snowStep(p, dt, w, h, ts), iconRects));
    for (const p of particles) {
      g.fillRect(p.x, p.y, 2, 2);
    }
    state.raf = requestAnimationFrame(frame);
  };
  on(window, RELOAD_LAYOUT_EVENT, () => later(collectRects, 120));
  on(window, "resize", collectRects);
  const start = (): void => {
    if (typeof document === "undefined") return;
    collectRects();
    canvas = makeEl("canvas", "nova-desk-snow") as HTMLCanvasElement;
    deskLayer().appendChild(canvas);
    lastT = 0;
    state!.raf = requestAnimationFrame(frame);
  };
  later(start, 200);
  state?.subs.push(() => {
    if (state?.raf) cancelAnimationFrame(state.raf);
    canvas?.remove();
    canvas = null;
    particles = [];
  });
}

// ---- W-035 行为：声像微音 ----

function mountPannedClicks(): void {
  let ac: AudioContext | null = null;
  on(window, "pointerdown", (ev: Event) => {
    const ctx = state?.ctx;
    if (!ctx?.on("W-035")) return;
    const e = ev as PointerEvent;
    const icon = (e.target as HTMLElement).closest<HTMLElement>(".desktop-icon");
    if (!icon) return;
    if (state?.audio.muted) return;
    const root = iconRoot();
    if (!root) return;
    const w = root.getBoundingClientRect().width || window.innerWidth;
    const pan = panForX(e.clientX, w);
    try {
      const AC = window.AudioContext || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!AC) return;
      if (!ac) ac = new AC();
      if (ac.state === "suspended") void ac.resume().catch(() => {});
      const t0 = ac.currentTime;
      const osc = ac.createOscillator();
      const env = ac.createGain();
      const panner = ac.createStereoPanner();
      panner.pan.value = pan;
      osc.type = "sine";
      osc.frequency.setValueAtTime(660, t0);
      osc.frequency.exponentialRampToValueAtTime(880, t0 + 0.06);
      env.gain.setValueAtTime(0.0001, t0);
      env.gain.exponentialRampToValueAtTime(Math.max(0.0002, (state?.audio.volume ?? 0.5) * 0.06), t0 + 0.01);
      env.gain.exponentialRampToValueAtTime(0.0001, t0 + 0.09);
      osc.connect(env).connect(panner).connect(ac.destination);
      osc.start(t0);
      osc.stop(t0 + 0.11);
      window.setTimeout(() => env.disconnect(), 140);
    } catch {
      /* 声音是增益不是依赖 */
    }
  }, { capture: true });
  state?.subs.push(() => {
    void ac?.close().catch(() => {});
    ac = null;
  });
}

// ---- 使用账本（W-027/029/031 数据源；点击即唤醒） ----

function mountUsageLedger(): void {
  on(window, "pointerdown", (ev: Event) => {
    const ctx = state?.ctx;
    if (!ctx) return;
    const e = ev as PointerEvent;
    const icon = (e.target as HTMLElement).closest<HTMLElement>(".desktop-icon");
    if (!icon) return;
    const tier = currentTier();
    const cell = cellOfEl(icon, tier);
    if (!cell) return;
    const layout = loadDesktopLayout();
    const shelved = allShelfMembers(layout);
    for (const [id, bc] of Object.entries(layout.positions)) {
      if (shelved.has(id)) continue;
      if (bc.c === cell.c && bc.r === cell.r) {
        recordIconUse(id);
        break;
      }
    }
  }, { capture: true });
}

// ---- W-036 行为：结组（框选后 G；Esc 解散；Enter 开合；拖出即散） ----

function mountClusters(): void {
  const ring = (): HTMLElement | null => (typeof document === "undefined" ? null : document.querySelector<HTMLElement>("#nova-desk-layer .nova-desk-ring"));
  const dissolve = (): void => {
    ring()?.remove();
    if (state) state.cluster = null;
  };
  on(window, "keydown", (ev: Event) => {
    const e = ev as KeyboardEvent;
    const ctx = state?.ctx;
    if (!ctx?.on("W-036") || !ctx.motionOK()) return;
    const target = e.target as HTMLElement | null;
    if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)) return;
    if (e.key.toLowerCase() === "g" && !e.ctrlKey && !e.metaKey && !e.altKey) {
      const sel = Array.from(document.querySelectorAll<HTMLElement>(".desktop-icon.selected"));
      if (sel.length < 2) return;
      const tier = currentTier();
      const cells: Record<string, Cell> = {};
      const layout = loadDesktopLayout();
      const shelved = allShelfMembers(layout);
      const members: string[] = [];
      for (const el of sel) {
        const cell = cellOfEl(el, tier);
        if (!cell) continue;
        for (const [id, bc] of Object.entries(layout.positions)) {
          if (shelved.has(id)) continue;
          if (bc.c === cell.c && bc.r === cell.r) {
            cells[id] = cell;
            members.push(id);
            break;
          }
        }
      }
      if (members.length < 2) return;
      dissolve();
      state!.cluster = makeCluster(members);
      const b = clusterBounds(cells, members);
      if (b) {
        const el = makeEl("div", "nova-desk-ring");
        el.style.left = `${GRID_PAD + b.minC * tier.w - 6}px`;
        el.style.top = `${GRID_PAD + b.minR * tier.h - 6}px`;
        el.style.width = `${(b.maxC - b.minC + 1) * tier.w + 12}px`;
        el.style.height = `${(b.maxR - b.minR + 1) * tier.h + 12}px`;
        deskLayer().appendChild(el);
      }
    } else if (e.key === "Escape") {
      dissolve();
    } else if (e.key === "Enter" && state?.cluster) {
      state.cluster.open = !state.cluster.open;
      ring()?.classList.toggle("open", state.cluster.open);
    }
  });
  // 拖出即散：pointerdown 落在组员上即触发解散判定（组员集合实时核对）
  on(window, "pointerdown", (ev: Event) => {
    const e = ev as PointerEvent;
    const cl = state?.cluster;
    if (!cl) return;
    const icon = (e.target as HTMLElement).closest<HTMLElement>(".desktop-icon");
    if (!icon) return;
    const tier = currentTier();
    const cell = cellOfEl(icon, tier);
    if (!cell) return;
    const layout = loadDesktopLayout();
    const shelved = allShelfMembers(layout);
    for (const [id, bc] of Object.entries(layout.positions)) {
      if (shelved.has(id)) continue;
      if (bc.c === cell.c && bc.r === cell.r) {
        if (clusterTouched(cl, id)) dissolve();
        break;
      }
    }
  }, { capture: true });
  // 环随组位移（拖拽期间跟随 .desktop-icon 的包围盒）
  const follow = (): void => {
    const cl = state?.cluster;
    const r = ring();
    if (!cl || !r) return;
    const els = icons().filter((el) => {
      const tier = currentTier();
      const cell = cellOfEl(el, tier);
      if (!cell) return false;
      const layout = loadDesktopLayout();
      const shelved = allShelfMembers(layout);
      return cl.members.some((mid) => {
        const bc = layout.positions[mid];
        return bc && !shelved.has(mid) && bc.c === cell.c && bc.r === cell.r;
      });
    });
    if (els.length === 0) return;
    let x0 = Infinity;
    let y0 = Infinity;
    let x1 = -Infinity;
    let y1 = -Infinity;
    for (const el of els) {
      const b = el.getBoundingClientRect();
      x0 = Math.min(x0, b.left);
      y0 = Math.min(y0, b.top);
      x1 = Math.max(x1, b.right);
      y1 = Math.max(y1, b.bottom);
    }
    r.style.left = `${x0 - 6}px`;
    r.style.top = `${y0 - 6}px`;
    r.style.width = `${x1 - x0 + 12}px`;
    r.style.height = `${y1 - y0 + 12}px`;
    state!.followRaf = requestAnimationFrame(follow);
  };
  on(window, "pointerdown", () => {
    if (state?.cluster && state.followRaf == null) state.followRaf = requestAnimationFrame(follow);
  }, { capture: true });
  on(window, "pointerup", () => {
    if (state?.followRaf != null) {
      cancelAnimationFrame(state.followRaf);
      state.followRaf = null;
    }
  });
  state?.subs.push(dissolve);
}

// ---- W-037 行为：标尺 ----

function mountRuler(): void {
  let el: HTMLElement | null = null;
  const remove = (): void => {
    el?.remove();
    el = null;
  };
  on(window, "keydown", (ev: Event) => {
    if ((ev as KeyboardEvent).key.toLowerCase() === "r" && state) state.rDown = true;
  });
  on(window, "keyup", (ev: Event) => {
    if ((ev as KeyboardEvent).key.toLowerCase() === "r") {
      if (state) state.rDown = false;
      remove();
    }
  });
  on(window, "pointerup", remove);
  on(window, "pointermove", (ev: Event) => {
    const ctx = state?.ctx;
    if (!ctx?.on("W-037") || !state?.rDown) return;
    if (!ctx.motionOK()) return;
    const dragging = document.querySelector<HTMLElement>(".desktop-icon.dragging");
    if (!dragging) {
      remove();
      return;
    }
    const e = ev as PointerEvent;
    const tier = currentTier();
    const root = iconRoot();
    if (!root) return;
    const rr = root.getBoundingClientRect();
    const cols = Math.max(1, Math.floor((rr.width - GRID_PAD * 2) / tier.w));
    const rows = Math.max(1, Math.floor((rr.height - GRID_PAD * 2) / tier.h));
    const active = nearestCell(e.clientX - rr.left, e.clientY - rr.top, tier, GRID_PAD, cols, rows);
    const { positions } = visiblePositions();
    const read = neighborDistance(positions, active, tier, cellOfEl(dragging, tier) ?? undefined);
    const ax = rr.left + GRID_PAD + active.c * tier.w + tier.w / 2;
    const ay = rr.top + GRID_PAD + active.r * tier.h + tier.h / 2;
    const nb = read.neighborId ? positions[read.neighborId] : null;
    const bx = nb ? rr.left + GRID_PAD + nb.c * tier.w + tier.w / 2 : ax;
    const by = nb ? rr.top + GRID_PAD + nb.r * tier.h + tier.h / 2 : ay;
    if (!el) {
      el = makeEl("div", "nova-desk-ruler");
      el.innerHTML = `<div class="nova-desk-ruler-line"></div><span class="nova-desk-ruler-badge"></span>`;
      deskLayer().appendChild(el);
    }
    const line = el.querySelector<HTMLElement>(".nova-desk-ruler-line") as HTMLElement;
    const len = Math.hypot(bx - ax, by - ay);
    line.style.left = `${ax}px`;
    line.style.top = `${ay}px`;
    line.style.width = `${len}px`;
    line.style.transform = `rotate(${Math.atan2(by - ay, bx - ax)}rad)`;
    const badge = el.querySelector<HTMLElement>(".nova-desk-ruler-badge") as HTMLElement;
    badge.style.left = `${(ax + bx) / 2}px`;
    badge.style.top = `${(ay + by) / 2}px`;
    badge.textContent = snapBadge(e.clientX - rr.left, e.clientY - rr.top, tier, GRID_PAD) ? `SNAP ${read.px}px` : `${read.px}px`;
  }, { passive: true });
  state?.subs.push(remove);
}

// ---- W-038 行为：回原点（API + 事件 + 双击让位仲裁） ----

function returnAllHomes(): number {
  const homes = loadHomes();
  const layout = loadDesktopLayout();
  const plan = homeRestorePlan(layout.positions, Object.fromEntries(Object.entries(homes).map(([k, v]) => [k, v.cell])));
  if (plan.length === 0) return 0;
  const next: DesktopLayout = { ...layout, positions: { ...layout.positions } };
  for (const p of plan) next.positions[p.id] = p.to;
  saveDesktopLayout(next);
  notifyLayoutReload();
  return plan.length;
}

function returnHome(id: string): boolean {
  const homes = loadHomes();
  const home = homes[id];
  if (!home) return false;
  const layout = loadDesktopLayout();
  const cur = layout.positions[id];
  if (!cur) return false;
  saveDesktopLayout({ ...layout, positions: { ...layout.positions, [id]: { ...home.cell } } });
  notifyLayoutReload();
  return true;
}

function mountHomes(): void {
  on(window, DESK_HOME_EVENT, () => {
    returnAllHomes();
  });
  // 双击空白回原点：仅当用户开启 dblHome 且 V-04 动作槽为 none（V-04 已配置动作时完全让位）
  on(window, "dblclick", (ev: Event) => {
    const e = ev as MouseEvent;
    const root = iconRoot();
    const ctx = state?.ctx;
    if (!root || !ctx?.on("W-038") || e.target !== root) return;
    if (!ctx.bool("W-038", "dblHome")) return;
    if (loadDoubleClickAction() !== "none") return;
    returnAllHomes();
  });
}

// ---- W-029 行为：排行榜面板 ----

function openLeaderboard(): void {
  const ctx = state?.ctx;
  if (!ctx?.on("W-029")) return;
  const lang = state?.lang ?? "zh";
  document.querySelector("#nova-desk-layer .nova-desk-panel")?.remove();
  const { card, body } = panelShell(t(lang, "leaderboard"), lang);
  const usage = loadUsage();
  const month = currentMonthCounts(usage);
  const rows = leaderboard(month);
  const tier = currentTier();
  const weight: Record<string, number> = {};
  for (const id of Object.keys(usage.lastUse)) weight[id] = tier.tile;
  const king = sleepKing(usage.lastUse, Date.now(), weight);
  if (rows.length === 0 && !king) {
    body.innerHTML = `<div class="nova-desk-empty">${esc(t(lang, "empty"))}</div>`;
  } else {
    const nameOf = (id: string): string => {
      // DOM 反查显示名（cell 匹配）；找不到时用 id 尾段
      const cell = loadDesktopLayout().positions[id];
      if (cell) {
        for (const el of icons()) {
          const c = cellOfEl(el, tier);
          if (c && c.c === cell.c && c.r === cell.r) return labelOfEl(el) || id;
        }
      }
      return id;
    };
    const medal = (m: string): string => (m === "gold" ? "①" : m === "silver" ? "②" : m === "bronze" ? "③" : "·");
    body.innerHTML =
      `<div class="nova-desk-rows">` +
      rows
        .map(
          (r) =>
            `<div class="nova-desk-row ${r.medal}"><span class="nova-desk-medal">${medal(r.medal)}</span>` +
            `<span>${esc(nameOf(r.id))}</span><span class="nova-desk-cnt">${r.count}</span></div>`,
        )
        .join("") +
      `</div>` +
      (king
        ? `<div class="nova-desk-foot"><span class="nova-desk-dim">${esc(t(lang, "sleepKing"))}: ${esc(nameOf(king.id))} · ${king.daysIdle}d</span>` +
          `<button class="nova-desk-btn" data-act="archive">${esc(t(lang, "archive"))}</button></div>`
        : "");
    body.querySelector('[data-act="archive"]')?.addEventListener("click", () => {
      if (king) window.dispatchEvent(new CustomEvent(DESK_ARCHIVE_HINT_EVENT, { detail: { id: king.id } }));
    });
  }
  deskLayer().appendChild(card);
}

// ---- W-033 行为：布局合影 ----

function openPhotos(): void {
  const ctx = state?.ctx;
  if (!ctx?.on("W-033")) return;
  const lang = state?.lang ?? "zh";
  document.querySelector("#nova-desk-layer .nova-desk-panel")?.remove();
  const { card, body } = panelShell(t(lang, "photos"), lang);
  const render = (): void => {
    const photos = loadPhotos().slice().reverse();
    body.innerHTML =
      `<div class="nova-desk-foot"><button class="nova-desk-btn" data-act="shot">${esc(t(lang, "shot"))}</button></div>` +
      (photos.length === 0
        ? `<div class="nova-desk-empty">${esc(lang === "en" ? "No photos yet" : "还没有合影")}</div>`
        : `<div class="nova-desk-rows">` +
          photos
            .map(
              (p) =>
                `<div class="nova-desk-photo" data-id="${esc(p.id)}"><img alt="" src="${p.thumb}">` +
                `<div><div>${new Date(p.ts).toLocaleString()}</div><div class="nova-desk-dim">${p.count} ${esc(t(lang, "icons"))}</div>` +
                `<div class="nova-desk-foot"><button class="nova-desk-btn" data-act="cmp">${esc(t(lang, "restore"))}</button>` +
                `<button class="nova-desk-btn" data-act="del">${esc(lang === "en" ? "Delete" : "删除")}</button></div></div></div>`,
            )
            .join("") +
          `</div>`);
    body.querySelector('[data-act="shot"]')?.addEventListener("click", () => {
      const { positions: pos } = visiblePositions();
      const photo: LayoutPhoto = {
        id: `nphoto-${Date.now().toString(36)}`,
        ts: Date.now(),
        count: Object.keys(pos).length,
        positions: JSON.parse(JSON.stringify(pos)) as Record<string, Cell>,
        thumb: makeThumb(pos),
      };
      if (snapshotSizeOk(photo)) {
        savePhoto(photo);
        render();
      }
    });
    body.querySelectorAll<HTMLButtonElement>('[data-act="del"]').forEach((b) => {
      b.addEventListener("click", () => {
        const id = b.closest<HTMLElement>(".nova-desk-photo")?.dataset.id;
        if (id) {
          deletePhoto(id);
          render();
        }
      });
    });
    body.querySelectorAll<HTMLButtonElement>('[data-act="cmp"]').forEach((b) => {
      b.addEventListener("click", () => {
        const id = b.closest<HTMLElement>(".nova-desk-photo")?.dataset.id;
        const photo = loadPhotos().find((p) => p.id === id);
        if (!photo) return;
        const { positions: cur } = visiblePositions();
        const diff = layoutDiff(cur, photo.positions);
        const total = diff.added.length + diff.removed.length + diff.moved.length;
        body.innerHTML =
          `<div class="nova-desk-foot"><button class="nova-desk-btn" data-act="back">${esc(t(lang, "cancel"))}</button>` +
          `<button class="nova-desk-btn" data-act="apply">${esc(t(lang, "apply"))}</button></div>` +
          `<div class="nova-desk-rows">` +
          `<div class="nova-desk-row"><span class="nova-desk-diff-added">${esc(t(lang, "diffAdded"))}</span><span class="nova-desk-cnt">${diff.added.length}</span></div>` +
          `<div class="nova-desk-row"><span class="nova-desk-diff-removed">${esc(t(lang, "diffRemoved"))}</span><span class="nova-desk-cnt">${diff.removed.length}</span></div>` +
          `<div class="nova-desk-row"><span class="nova-desk-diff-moved">${esc(t(lang, "diffMoved"))}</span><span class="nova-desk-cnt">${diff.moved.length}</span></div>` +
          (total === 0 ? `<div class="nova-desk-empty">${esc(t(lang, "diffNone"))}</div>` : "") +
          `</div>`;
        body.querySelector('[data-act="back"]')?.addEventListener("click", render);
        body.querySelector('[data-act="apply"]')?.addEventListener("click", () => {
          const layout = loadDesktopLayout();
          saveDesktopLayout({ ...layout, positions: JSON.parse(JSON.stringify(photo.positions)) as Record<string, Cell> });
          notifyLayoutReload();
          card.remove();
        });
      });
    });
  };
  render();
  deskLayer().appendChild(card);
}

// ---- 中枢入口事件 ----

function mountOpenEvents(): void {
  on(window, DESK_OPEN_EVENT, (ev: Event) => {
    const panel = (ev as CustomEvent<{ panel?: string }>).detail?.panel;
    if (panel === "photos") openPhotos();
    else if (panel === "leaderboard") openLeaderboard();
  });
  on(window, DESK_AUDIO_EVENT, (ev: Event) => {
    const d = (ev as CustomEvent<{ muted?: boolean; volume?: number }>).detail ?? {};
    if (!state) return;
    if (typeof d.muted === "boolean") state.audio.muted = d.muted;
    if (typeof d.volume === "number") state.audio.volume = Math.min(1, Math.max(0, d.volume));
  });
}

// ---- 观感刷新节拍（60s；养成/老化随时间演化） ----

function mountStyleTicker(): void {
  const h = setInterval(() => stylePass(), 60_000);
  state?.subs.push(() => clearInterval(h));
  later(() => stylePass(), 300);
  later(() => stylePass(), 1500);
}

// ---------------------------------------------------------------------------
// 模块装配（activate/deactivate 幂等；非 DOM 环境 no-op）
// ---------------------------------------------------------------------------

export function createDeskNova(): DeskNovaHandle {
  const ensureState = (): DeskState => {
    if (!state) state = freshState();
    return state;
  };
  return {
    activate(ctx: DeskNovaCtx): void {
      if (typeof document === "undefined") return; // 行为层安全 no-op（纯逻辑/单测环境）
      const s = ensureState();
      if (s.ctx) this.deactivate(); // 幂等：重复激活先卸载
      s.ctx = ctx;
      s.lang = typeof navigator !== "undefined" && navigator.language?.toLowerCase().startsWith("en") ? "en" : "zh";
      s.styleUn = injectStyles();
      on(window, "resize", () => stylePass());
      mountUsageLedger();
      mountConstellation();
      mountSiesta();
      mountHorizon();
      mountSnow();
      mountPannedClicks();
      mountClusters();
      mountRuler();
      mountHomes();
      mountOpenEvents();
      mountStyleTicker();
    },
    deactivate(): void {
      const s = state;
      if (!s) return;
      for (const un of s.subs) {
        try {
          un();
        } catch {
          /* 卸载互不阻塞 */
        }
      }
      s.subs = [];
      for (const h of s.timers) clearTimeout(h);
      s.timers = [];
      if (s.raf != null) cancelAnimationFrame(s.raf);
      if (s.followRaf != null) cancelAnimationFrame(s.followRaf);
      if (typeof document !== "undefined") {
        document.getElementById("nova-desk-layer")?.remove();
        iconRoot()?.classList.remove("nova-desk-siesta", "nova-desk-season");
        for (const el of icons()) {
          el.style.opacity = "";
          el.style.filter = "";
        }
      }
      s.styleUn?.();
      s.styleUn = null;
      s.ctx = null;
      s.cluster = null;
      s.siesta = false;
      s.constActive = null;
      s.raf = null;
    },
    api: {
      openLeaderboard: () => openLeaderboard(),
      openPhotos: () => openPhotos(),
      applyConstellation: (id: string) => applyConstellation(id),
      restoreGrid: () => restoreGrid(),
      returnAllHomes: () => returnAllHomes(),
      returnHome: (id: string) => returnHome(id),
      dissolveCluster: () => {
        if (typeof document !== "undefined") document.querySelector("#nova-desk-layer .nova-desk-ring")?.remove();
        if (state) state.cluster = null;
      },
    },
  };
}

/** 单例（S0 运行时可直接 import 挂载）。 */
export const deskNova = createDeskNova();
