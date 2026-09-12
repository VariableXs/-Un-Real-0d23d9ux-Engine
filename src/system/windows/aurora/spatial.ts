import type { VwmRect } from "../vwm";

/**
 * AURORA-10000 · 空间层（AI-08/09）：
 * - 族0036 无限画布桌面（F00901~F00925）→ 平移缩放变换与书签
 * - 族0037 窗口物理玩具（F00926~F00950）→ 趣味物理积分器
 * - 族0038 桌面小地图（F00951~F00975）→ 视口映射与雷达模式
 * - 族0039 桌面天气与时辰（F00976~F01000）→ 节律计算
 * - 族0043 空间整理助手（F01076~F01100）→ 整理策略与整洁评分
 * 全部纯函数，可直接单测。
 */

// ---------- 族0036 无限画布桌面 ----------

export interface CanvasState {
  /** 视口原点（世界坐标）。 */
  ox: number;
  oy: number;
  /** 缩放（0.25~4）。 */
  zoom: number;
  /** 书签。 */
  bookmarks: Array<{ name: string; x: number; y: number; zoom: number }>;
  /** 网格开关。 */
  grid: boolean;
  /** 栅格步长。 */
  gridStep: number;
}

export const CANVAS_INITIAL: CanvasState = { ox: 0, oy: 0, zoom: 1, bookmarks: [], grid: false, gridStep: 64 };

/** 世界 → 屏幕。 */
export function worldToScreen(s: CanvasState, wx: number, wy: number): { x: number; y: number } {
  return { x: (wx - s.ox) * s.zoom, y: (wy - s.oy) * s.zoom };
}

/** 屏幕 → 世界。 */
export function screenToWorld(s: CanvasState, sx: number, sy: number): { x: number; y: number } {
  return { x: sx / s.zoom + s.ox, y: sy / s.zoom + s.oy };
}

/** 缩放（以屏幕点为锚）。 */
export function canvasZoom(s: CanvasState, factor: number, anchorSx: number, anchorSy: number): CanvasState {
  const zoom = Math.min(4, Math.max(0.25, s.zoom * factor));
  const w = screenToWorld(s, anchorSx, anchorSy);
  const ox = w.x - anchorSx / zoom;
  const oy = w.y - anchorSy / zoom;
  return { ...s, zoom, ox, oy };
}

/** 书签操作。 */
export function canvasBookmark(s: CanvasState, op: { t: "add" | "remove"; name?: string; index?: number }): CanvasState {
  if (op.t === "add" && op.name) {
    return { ...s, bookmarks: [...s.bookmarks, { name: op.name, x: s.ox, y: s.oy, zoom: s.zoom }] };
  }
  if (op.t === "remove" && op.index != null) {
    return { ...s, bookmarks: s.bookmarks.filter((_, i) => i !== op.index) };
  }
  return s;
}

/** 房间分区：把世界划分为 sector×sector 房间，返回点所在房间号。 */
export function canvasSector(wx: number, wy: number, sector = 2048): { col: number; row: number } {
  return { col: Math.floor(wx / sector), row: Math.floor(wy / sector) };
}

// ---------- 族0037 窗口物理玩具 ----------

export interface PhysBody {
  id: string;
  rect: VwmRect;
  vx: number;
  vy: number;
  /** 颜色键（磁铁相吸/相斥用）。 */
  hue: number;
}

export interface PhysMode {
  gravity: number;
  bounce: number;
  friction: number;
  /** 相互作用：none/attraction 同色相吸/repulsion 异色相斥/collide 推挤。 */
  interact: "none" | "attraction" | "repulsion" | "collide" | "chain";
  wind: number;
  /** 上浮（气球）。 */
  lift: number;
  /** 旋涡（龙卷风）。 */
  vortex: number;
}

export const PHYS_MODES: Readonly<Record<string, PhysMode>> = Object.freeze({
  F00926: { gravity: 0.5, bounce: 0, friction: 0.02, interact: "none", wind: 0, lift: 0, vortex: 0 },
  F00927: { gravity: 0.5, bounce: 0.7, friction: 0.01, interact: "none", wind: 0, lift: 0, vortex: 0 },
  F00928: { gravity: 0.2, bounce: 0.2, friction: 0.05, interact: "collide", wind: 0, lift: 0, vortex: 0 },
  F00929: { gravity: 0, bounce: 0.4, friction: 0.03, interact: "none", wind: 0, lift: 0, vortex: 0 }, // 果冻抖动（释放时给 vy 扰动由调用方）
  F00930: { gravity: 0.1, bounce: 0.1, friction: 0.01, interact: "none", wind: 0.4, lift: 0, vortex: 0 }, // 布料旗
  F00931: { gravity: 0, bounce: 0.3, friction: 0.01, interact: "none", wind: 0.2, lift: -0.5, vortex: 0 }, // 气球
  F00932: { gravity: 0.05, bounce: 0.05, friction: 0.2, interact: "none", wind: 0, lift: 0, vortex: 0 }, // 水中阻力
  F00933: { gravity: 0.2, bounce: 0.05, friction: 0.002, interact: "none", wind: 0, lift: 0, vortex: 0 }, // 冰面滑行
  F00934: { gravity: 0, bounce: 0.2, friction: 0.02, interact: "attraction", wind: 0, lift: 0, vortex: 0 },
  F00935: { gravity: 0, bounce: 0.2, friction: 0.02, interact: "repulsion", wind: 0, lift: 0, vortex: 0 },
  F00936: { gravity: 0.3, bounce: 0.3, friction: 0.02, interact: "chain", wind: 0, lift: 0, vortex: 0 }, // 绳链
  F00937: { gravity: 0.3, bounce: 0.5, friction: 0.02, interact: "chain", wind: 0, lift: 0, vortex: 0 }, // 弹簧链
  F00938: { gravity: 0.3, bounce: 0.3, friction: 0.02, interact: "chain", wind: 0, lift: 0, vortex: 0 }, // 链式牵引
  F00939: { gravity: 0.6, bounce: 0.4, friction: 0.01, interact: "none", wind: 0, lift: 0, vortex: 0 }, // 溜溜球
  F00940: { gravity: 0.05, bounce: 0.6, friction: 0.005, interact: "none", wind: 0, lift: 0, vortex: 0 }, // 火箭推进
  F00941: { gravity: 0.08, bounce: 0, friction: 0.1, interact: "none", wind: 0.1, lift: 0, vortex: 0 }, // 降落伞
  F00942: { gravity: 0.5, bounce: 0.1, friction: 0.03, interact: "collide", wind: 0, lift: 0, vortex: 0 }, // 多米诺
  F00943: { gravity: 0.5, bounce: 0.5, friction: 0.02, interact: "collide", wind: 0, lift: 0, vortex: 0 }, // 保龄球
  F00944: { gravity: 0.4, bounce: 0.8, friction: 0.005, interact: "collide", wind: 0, lift: 0, vortex: 0 }, // 弹珠台
  F00945: { gravity: 0.2, bounce: 0.6, friction: 0.01, interact: "none", wind: 0, lift: 0, vortex: 0 }, // 回旋镖
  F00946: { gravity: 0.3, bounce: 0, friction: 0.15, interact: "collide", wind: 0, lift: 0, vortex: 0 }, // 黏性碰撞
  F00947: { gravity: 0.3, bounce: 0.3, friction: 0.02, interact: "none", wind: 0, lift: 0, vortex: 0 }, // 软体窗（形变由渲染层）
  F00948: { gravity: 0, bounce: 0.2, friction: 0.02, interact: "none", wind: 0.8, lift: 0, vortex: 0 }, // 风场
  F00949: { gravity: 0.1, bounce: 0.2, friction: 0.01, interact: "none", wind: 0, lift: 0, vortex: 0.6 }, // 龙卷风
  F00950: { gravity: -0.5, bounce: 0.6, friction: 0.01, interact: "none", wind: 0, lift: 0, vortex: 0 }, // 重力反转
});

/** 物理积分一步：bodies 原地演化并返回新数组（纯函数）。 */
export function physStep(mode: PhysMode, bodies: PhysBody[], bounds: VwmRect, dt = 1): PhysBody[] {
  const cx = bounds.x + bounds.w / 2;
  const cy = bounds.y + bounds.h / 2;
  return bodies.map((b, i) => {
    let vx = b.vx;
    let vy = b.vy + (mode.gravity + mode.lift) * dt;
    vx += mode.wind * dt;
    if (mode.vortex) {
      const dx = cx - (b.rect.x + b.rect.w / 2);
      const dy = cy - (b.rect.y + b.rect.h / 2);
      vx += (dx * 0.01 - dy * 0.02) * mode.vortex;
      vy += (dy * 0.01 + dx * 0.02) * mode.vortex;
    }
    if (mode.interact === "attraction" || mode.interact === "repulsion" || mode.interact === "chain" || mode.interact === "collide") {
      for (let j = 0; j < bodies.length; j++) {
        if (j === i) continue;
        const o = bodies[j]!;
        const dx = o.rect.x - b.rect.x;
        const dy = o.rect.y - b.rect.y;
        const d = Math.hypot(dx, dy) || 1;
        if (mode.interact === "attraction" && b.hue === o.hue) {
          vx += (dx / d) * 0.05;
          vy += (dy / d) * 0.05;
        } else if (mode.interact === "repulsion" && d < 300) {
          vx -= (dx / d) * 0.08;
          vy -= (dy / d) * 0.08;
        } else if (mode.interact === "chain" && j === i + 1) {
          // 只与相邻窗成链：距离大于 260 时牵引
          if (d > 260) {
            vx += (dx / d) * 0.1;
            vy += (dy / d) * 0.1;
          }
        } else if (mode.interact === "collide" && d < 60) {
          vx -= (dx / d) * 0.2;
          vy -= (dy / d) * 0.2;
        }
      }
    }
    vx *= 1 - mode.friction;
    vy *= 1 - mode.friction;
    let x = b.rect.x + vx * dt;
    let y = b.rect.y + vy * dt;
    const maxX = bounds.x + bounds.w - b.rect.w;
    const maxY = bounds.y + bounds.h - b.rect.h;
    if (x < bounds.x) {
      x = bounds.x;
      vx = -vx * mode.bounce;
    } else if (x > maxX) {
      x = maxX;
      vx = -vx * mode.bounce;
    }
    if (y < bounds.y) {
      y = bounds.y;
      vy = -vy * mode.bounce;
    } else if (y > maxY) {
      y = maxY;
      vy = -vy * mode.bounce;
    }
    return { ...b, rect: { ...b.rect, x: Math.round(x), y: Math.round(y) }, vx, vy };
  });
}

// ---------- 族0038 桌面小地图 ----------

/** 世界（画布/全空间）到小地图的映射。 */
export interface MiniMapSpec {
  /** 地图尺寸（px）。 */
  size: number;
  /** 世界跨度（px）。 */
  span: number;
  mode: "rect" | "radar";
  opacity: number;
  autoHide: boolean;
}

/** 屏幕 → 地图坐标（矩形模式；保留小数，落盘/渲染时由调用方取整）。 */
export function mapProject(world: VwmRect, spec: MiniMapSpec, px: number, py: number): { x: number; y: number } {
  const k = spec.size / spec.span;
  return { x: (px - world.x) * k, y: (py - world.y) * k };
}

/** 地图坐标 → 屏幕（拖视口/点击跳转，F00952/F00953）。 */
export function mapUnproject(world: VwmRect, spec: MiniMapSpec, mx: number, my: number): { x: number; y: number } {
  const k = spec.span / spec.size;
  return { x: Math.round(world.x + mx * k), y: Math.round(world.y + my * k) };
}

/** 雷达模式：直角坐标 → 极坐标（F00969）。 */
export function radarPolar(spec: MiniMapSpec, dx: number, dy: number): { r: number; deg: number } {
  const r = Math.min(1, Math.hypot(dx, dy) / (spec.span / 2));
  const deg = (Math.atan2(dy, dx) * 180) / Math.PI;
  return { r, deg: Math.round((deg + 360) % 360) };
}

// ---------- 族0039 桌面天气与时辰 ----------

/** 太阳时事件（小时数 0~24）。 */
export interface SunTimes {
  sunrise: number;
  sunset: number;
}

/**
 * NOAA 近似日出日落（简化版，精度 ±3 分钟级，纯函数可测）。
 * lat 纬度（北正），dayOfYear 一年中第几天；
 * meridianCorrection = (时区中央经线 - 当地经度)/15（小时），如上海 (120-121.47)/15。
 */
export function sunTimes(lat: number, dayOfYear: number, meridianCorrection: number): SunTimes {
  const gamma = ((2 * Math.PI) / 365) * (dayOfYear - 1);
  const eqtime = 229.18 * (0.000075 + 0.001868 * Math.cos(gamma) - 0.032077 * Math.sin(gamma) - 0.014615 * Math.cos(2 * gamma) - 0.040849 * Math.sin(2 * gamma));
  const decl = 0.006918 - 0.399912 * Math.cos(gamma) + 0.070257 * Math.sin(gamma) - 0.006758 * Math.cos(2 * gamma) + 0.000907 * Math.sin(2 * gamma) - 0.002697 * Math.cos(3 * gamma) + 0.00148 * Math.sin(3 * gamma);
  const latRad = (lat * Math.PI) / 180;
  const cosH = (Math.cos((90.833 * Math.PI) / 180) - Math.sin(latRad) * Math.sin(decl)) / (Math.cos(latRad) * Math.cos(decl));
  const H = Math.acos(Math.min(1, Math.max(-1, cosH))) * (180 / Math.PI) / 15;
  const noon = 12 + meridianCorrection - eqtime / 60;
  return { sunrise: noon - H, sunset: noon + H };
}

/** 十二时辰：小时 → 时辰名。 */
export function shichen(hour: number): string {
  const names = ["子", "丑", "寅", "卯", "辰", "巳", "午", "未", "申", "酉", "戌", "亥"];
  const idx = Math.floor(((hour + 1) % 24) / 2);
  return names[idx] ?? "子";
}

/** 月相：以已知新月为基准的周期近似（0=朔 0.5=望）。 */
export function moonPhase(date: Date): number {
  const knownNewMoon = Date.UTC(2000, 0, 6, 18, 14) / 86400000;
  const days = date.getTime() / 86400000 - knownNewMoon;
  const phase = (days % 29.530588853) / 29.530588853;
  return phase < 0 ? phase + 1 : phase;
}

/** 倒计时格式化（F00998）：毫秒 → 「x天 hh:mm:ss」。 */
export function formatCountdown(ms: number): string {
  if (ms <= 0) return "已到达";
  const s = Math.floor(ms / 1000);
  const d = Math.floor(s / 86400);
  const h = String(Math.floor((s % 86400) / 3600)).padStart(2, "0");
  const m = String(Math.floor((s % 3600) / 60)).padStart(2, "0");
  const ss = String(s % 60).padStart(2, "0");
  return d > 0 ? `${d}天 ${h}:${m}:${ss}` : `${h}:${m}:${ss}`;
}

// ---------- 族0043 空间整理助手 ----------

export interface OrganizableWin {
  id: string;
  app: string;
  rect: VwmRect;
  /** 最近使用时刻。 */
  lastUsed: number;
  /** 使用频率计数。 */
  freq: number;
}

export type OrganizeStrategy = "auto" | "byApp" | "byProject" | "byFreq" | "byRecent" | "sector";

/** 族0043 主入口：返回每个窗的目标矩形（同输入顺序）。 */
export function organize(strategy: OrganizeStrategy, wins: OrganizableWin[], work: VwmRect): VwmRect[] {
  const n = wins.length;
  if (n === 0) return [];
  const order = wins.map((w, i) => ({ w, i }));
  let ordered = order;
  if (strategy === "byApp") ordered = [...order].sort((a, b) => a.w.app.localeCompare(b.w.app) || a.i - b.i);
  else if (strategy === "byProject") ordered = [...order].sort((a, b) => a.w.id.localeCompare(b.w.id) || a.i - b.i);
  else if (strategy === "byFreq") ordered = [...order].sort((a, b) => b.w.freq - a.w.freq || a.i - b.i);
  else if (strategy === "byRecent") ordered = [...order].sort((a, b) => b.w.lastUsed - a.w.lastUsed || a.i - b.i);
  const cols = strategy === "sector" ? 2 : Math.min(3, Math.max(1, Math.ceil(Math.sqrt(n))));
  const rows = Math.ceil(n / cols);
  const cell = { w: work.w / cols, h: work.h / rows };
  const slotOf = new Map<number, number>();
  ordered.forEach((entry, k) => slotOf.set(entry.i, k));
  return wins.map((_, i) => {
    const k = slotOf.get(i) ?? i;
    const c = k % cols;
    const ro = Math.floor(k / cols);
    return {
      x: Math.round(work.x + cell.w * c),
      y: Math.round(work.y + cell.h * ro),
      w: Math.round(cell.w),
      h: Math.round(cell.h),
    };
  });
}

/** 整洁度评分 0~100：重叠越多越乱。 */
export function clutterScore(wins: OrganizableWin[]): number {
  if (wins.length <= 1) return 100;
  let overlapPairs = 0;
  for (let i = 0; i < wins.length; i++) {
    for (let j = i + 1; j < wins.length; j++) {
      const a = wins[i]!.rect;
      const b = wins[j]!.rect;
      const ox = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
      const oy = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
      if (ox > 8 && oy > 8) overlapPairs++;
    }
  }
  const penalty = (overlapPairs / ((wins.length * (wins.length - 1)) / 2)) * 100;
  return Math.max(0, Math.round(100 - penalty));
}
