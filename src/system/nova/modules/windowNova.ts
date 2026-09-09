/**
 * NOVA-200 · S2 窗口物理路（AI-02）—— 域2 窗口与空间（W-013…W-025）。
 *
 * 边界（全景 §2）：Q-14 给拖拽加速度倾角、U-14 管吸附规则、N-01 管历史时间线、
 * Z-37 管每显示器 1:1 恢复；本模块补**质量物理、磁吸手感、结构族谱、空间折叠**。
 *
 * 纪律：
 * - 零侵入：只读订阅 vwmStore + 仅使用 vwm.ts 公开 API（move/resize/focus/unmax/topmost）；
 *   不改写任何既有组件内部逻辑，不碰 singularity 工件；
 * - 前缀：类名 `nova-`、事件 `nova://win-*`、localStorage 键 `nova.window.*`；
 * - 降级：`[data-reduce-motion="true"]` 下全部动效归零（物理/辉光/呼吸），语义保留；
 * - 诚实：环境尚不存在的维度（进程父子树需 S0 nova_pulse_ex、Alt+Tab 标签列、
 *   标题栏菜单项挂载）在 manifest.wiringHint 如实注明，不假装完成。
 */

import {
  isVwmWinVisible,
  resizeVwmWin,
  unmaxVwmTo,
  focusVwmWin,
  closeVwmApp,
  setVwmTopmost,
  vwmStore,
  vwmWindowTitle,
  type VwmRect,
  type VwmWin,
} from "../../windows/vwm";

// ---------------------------------------------------------------------------
// 通用工具（本模块自持，不依赖 S0 工件）
// ---------------------------------------------------------------------------

export function clamp(v: number, min: number, max: number): number {
  return Math.min(Math.max(v, min), max);
}

/** reduce-motion 运行时标记（App.tsx 写 root.dataset.reduceMotion）→ 动效全关。 */
export function motionOK(): boolean {
  if (typeof document === "undefined") return false;
  return document.documentElement.dataset.reduceMotion !== "true";
}

/** localStorage 安全读（node 环境无 localStorage → fallback）。 */
function lsGet<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw == null) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

/** localStorage 安全写。 */
function lsSet(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* storage full/blocked → 不持久化 */
  }
}

const REGISTRY_KEY = "nova.registry";
const NS = "nova.window";

/**
 * 功能开关：只读消费 S0 注册表（`nova.registry`），防御性解析两种形态：
 * `{ "W-013": true }` 或 `{ features: { "W-013": { enabled: true } } }`；
 * 无注册表 / 解析失败 → 用本模块 manifest 的 defaultOn（诚实降级，不报错）。
 */
export function flagOn(id: string): boolean {
  const def = WINDOW_NOVA_FEATURES.find((f) => f.id === id);
  const fallback = def?.defaultOn ?? false;
  try {
    const raw = localStorage.getItem(REGISTRY_KEY);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const direct = parsed[id];
    if (typeof direct === "boolean") return direct;
    const feats = parsed.features as Record<string, { enabled?: boolean }> | undefined;
    const fe = feats?.[id];
    if (fe && typeof fe.enabled === "boolean") return fe.enabled;
    return fallback;
  } catch {
    return fallback;
  }
}

/** 派发 `nova://win-*` 事件（SSR/测试环境安全）。 */
export function novaEvent(name: string, detail?: unknown): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  window.dispatchEvent(new CustomEvent(`nova://win-${name}`, { detail }));
}

// ---------------------------------------------------------------------------
// Hub 注册清单（S0 NovaHub 消费：功能卡 + 参数 + 降级说明）
// ---------------------------------------------------------------------------

export interface NovaFeatureCard {
  id: string;
  titleZh: string;
  titleEn: string;
  descZh: string;
  defaultOn: boolean;
  /** 需要 S17 在既有文件补接线的行为（本模块 API/事件已备好）。 */
  wiringHint?: string;
  /** reduce-motion / safeMode / static 降级说明。 */
  degrade: string;
}

export const WINDOW_NOVA_FEATURES: NovaFeatureCard[] = [
  {
    id: "W-013",
    titleZh: "窗口质量物理",
    titleEn: "Mass Physics",
    descZh: "窗口面积映射惯性：大窗（≥60% 屏幕面积）起步迟滞 60ms、停止余韵 80ms；小窗轻快如纸。",
    defaultOn: false,
    degrade: "reduce-motion 下物理关闭，仅保留面积语义",
  },
  {
    id: "W-014",
    titleZh: "磁吸边缘手感",
    titleEn: "Magnetic Edge",
    descZh: "拖至吸附线前 6px 进入磁场：吸附线亮起呼吸确认，窗缘辉光预告；不改 U-14 吸附规则。",
    defaultOn: false,
    wiringHint: "指针 3px 阻尼需在拖拽循环注入 magnetState() 的 pullPx（1 行）",
    degrade: "reduce-motion 下仅保留吸附线静态亮起，无呼吸",
  },
  {
    id: "W-015",
    titleZh: "窗口族谱",
    titleEn: "Window Genealogy",
    descZh: "以诞生关系树展示当前全部窗口：谁先诞生、谁被收编；点击聚焦、右键结束整树。",
    defaultOn: true,
    wiringHint: "进程父子树待 S0 nova_pulse_ex 提供真实 pid 链后切换数据源（当前为 VWM 结构树）",
    degrade: "无动效，纯面板",
  },
  {
    id: "W-016",
    titleZh: "Z 序阴影深度",
    titleEn: "Z-Shadow",
    descZh: "阴影深度映射 z 序高度：顶层最深、逐层变浅成空气透视；焦点切换 200ms 平滑过渡。",
    defaultOn: false,
    degrade: "reduce-motion / static 下退化为焦点双层分档（有焦/无焦）",
  },
  {
    id: "W-017",
    titleZh: "空间气味标记",
    titleEn: "Space Scent",
    descZh: "每个空间维度分配专属色相（与主题强调色错开），切换瞬间环境辉光闪现 600ms 做记忆锚点。",
    defaultOn: false,
    wiringHint: "当前环境空间维度为 VWM 标签组；虚拟桌面维落地后按 id 平移映射",
    degrade: "reduce-motion 下辉光关闭，仅保留色相分配",
  },
  {
    id: "W-018",
    titleZh: "键盘吸附格阵",
    titleEn: "Key Snap Lattice",
    descZh: "Alt+小键盘 1-9 九宫格直达吸附（1=左下、5=居中最大化、9=右上）；连按同格二次恢复原位。",
    defaultOn: true,
    degrade: "纯键盘几何摆放，无动效",
  },
  {
    id: "W-019",
    titleZh: "几何裁剪",
    titleEn: "Geometry Snap-8",
    descZh: "一键把窗口宽高取整到 8px 网格消除像素参差；可选对齐屏幕安全区（距边 8px）。",
    defaultOn: true,
    wiringHint: "标题栏菜单挂「对齐几何」一项（S17 合并 winfeelMenu）；已备 snap8Focused() 与 nova://win-snap8 事件",
    degrade: "无动效，纯几何动作",
  },
  {
    id: "W-020",
    titleZh: "焦点回声",
    titleEn: "Focus Echo",
    descZh: "上个焦点窗口失焦超 10s 后，任务栏图标 4s 一次柔和呼吸微光，点击即归位；同一时刻至多 1 枚。",
    defaultOn: false,
    degrade: "reduce-motion 下呼吸改为静态微光点",
  },
  {
    id: "W-021",
    titleZh: "马赛克顾问",
    titleEn: "Mosaic Advisor",
    descZh: "同屏 ≥3 窗且拥挤度 >60% 时按各窗宽高比推荐镶嵌布局（宽窗给列/方窗给格）；一键应用或忽略（8h 记忆）。",
    defaultOn: false,
    degrade: "无动效，纯建议卡",
  },
  {
    id: "W-022",
    titleZh: "窗口记忆仓位",
    titleEn: "Position Slots",
    descZh: "每个窗口记住最近 3 个几何仓位（真实变更事件才记录，最小化不记仓），一键回滚上一仓。",
    defaultOn: true,
    wiringHint: "标题栏右键「仓位」子菜单挂载（S17）；已备 rollbackWindowSlot() API",
    degrade: "无动效",
  },
  {
    id: "W-023",
    titleZh: "轨道浮窗",
    titleEn: "Rail Window",
    descZh: "把窗口拖到屏幕顶端松手 → 变 72px 高的顶部贴轨浮窗（横向自由、常驻置顶）；下拉脱离或双击恢复原几何。",
    defaultOn: true,
    wiringHint: "轨道窗排除 Alt+Tab 排列动画需 S17 在切换器过滤 nova-rail 类",
    degrade: "无过渡动画，几何直接生效",
  },
  {
    id: "W-024",
    titleZh: "空间折叠带",
    titleEn: "Fold Ribbon",
    descZh: "Ctrl+双击桌面空白：全部非焦点窗口折叠为右缘 36px 竖带，点一枚 350ms 展开；再按全体展开。折叠不最小化、零进程干预。",
    defaultOn: true,
    degrade: "reduce-motion 下折叠直接落位（350ms 过渡取消）",
  },
  {
    id: "W-025",
    titleZh: "窗口标签",
    titleEn: "Window Labels",
    descZh: "给窗口贴浮动文字标签（≤12 字符），随窗口移动；多窗口并行作业的人脑索引。",
    defaultOn: true,
    wiringHint: "Alt+Tab 切换器内显示标签需 S17 读取 nova.window.labels 渲染一行；已备 labelOf()/nova://win-label-set",
    degrade: "无动效",
  },
];

export const windowNovaDomain = {
  id: "S2",
  nameZh: "窗口物理",
  nameEn: "Window Physics",
  route: "AI-02",
  features: WINDOW_NOVA_FEATURES,
} as const;

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

// ---- W-013 质量物理 ----

export const MASS_HEAVY_RATIO = 0.6;

/** 面积比 = 窗口面积 / 工作区面积。 */
export function areaRatioOf(win: { w: number; h: number }, wa: VwmRect): number {
  if (wa.w <= 0 || wa.h <= 0) return 0;
  return (win.w * win.h) / (wa.w * wa.h);
}

/**
 * 质量惯性曲线：面积比 0 → 轻如纸（0ms/0ms）；≥0.6 → 60ms 起步 / 80ms 余韵。
 * 线性单调（参数与面积单调相关），超过 0.6 封顶（更重不更迟）。
 */
export function massInertia(areaRatio: number): {
  startDelayMs: number;
  stopLingerMs: number;
  heavy: boolean;
} {
  const t = clamp(areaRatio / MASS_HEAVY_RATIO, 0, 1);
  return {
    startDelayMs: Math.round(60 * t),
    stopLingerMs: Math.round(80 * t),
    heavy: areaRatio >= MASS_HEAVY_RATIO,
  };
}

// ---- W-014 磁吸边缘手感 ----

export const MAGNET_FIELD_PX = 6;
export const MAGNET_DAMP_PX = 3;
export const MAGNET_CONFIRM_PX = 2;

/**
 * 磁场状态：距吸附线 d px 时的预告。
 * d > 6 → 场外（零影响，不碰非吸附路径）；d ≤ 6 → 场内，指针阻尼拉力随接近二次增强，
 * d ≤ 2 → 吸附线呼吸确认。pullPx 供 S17 拖拽循环注入（本模块视觉层不动指针）。
 */
export function magnetState(distPx: number): { inField: boolean; pullPx: number; confirm: boolean } {
  const d = Math.abs(distPx);
  if (d > MAGNET_FIELD_PX) return { inField: false, pullPx: 0, confirm: false };
  const pull = MAGNET_DAMP_PX * (1 - d / MAGNET_FIELD_PX);
  return { inField: true, pullPx: Math.round(pull * 100) / 100, confirm: d <= MAGNET_CONFIRM_PX };
}

export type SnapEdge = "left" | "right" | "top" | "bottom" | "centerX" | "centerY";

/**
 * 拖拽窗口矩形到最近吸附线的距离（U-14 的半屏/贴边线系）：
 * 垂直线 wa.x / wa.x+wa.w/2 / wa.x+wa.w，水平线 wa.y / wa.y+wa.h/2 / wa.y+wa.h。
 * 返回窗口左/上边缘到最近线的绝对距离与该线身份（供磁吸线渲染定位）。
 */
export function nearestSnapDistance(
  r: VwmRect,
  wa: VwmRect,
): { dist: number; edge: SnapEdge; linePos: number; axis: "x" | "y" } | null {
  const vLines: Array<[number, SnapEdge]> = [
    [wa.x, "left"],
    [wa.x + wa.w / 2, "centerX"],
    [wa.x + wa.w, "right"],
  ];
  const hLines: Array<[number, SnapEdge]> = [
    [wa.y, "top"],
    [wa.y + wa.h / 2, "centerY"],
    [wa.y + wa.h, "bottom"],
  ];
  let best: { dist: number; edge: SnapEdge; linePos: number; axis: "x" | "y" } | null = null;
  for (const [lx, edge] of vLines) {
    const d = Math.abs(r.x - lx);
    if (!best || d < best.dist) best = { dist: d, edge, linePos: lx, axis: "x" };
  }
  for (const [ly, edge] of hLines) {
    const d = Math.abs(r.y - ly);
    if (!best || d < best.dist) best = { dist: d, edge, linePos: ly, axis: "y" };
  }
  return best;
}

// ---- W-015 窗口族谱 ----

export interface GenealogyInput {
  id: string;
  /** 诞生父节点（null = 根）；进程树语义下为 ppid。 */
  ppid: string | null;
  label: string;
  /** 收编节点（embed 协议，如 steam 收编链）。 */
  adopted?: boolean;
}

export interface GenealogyNode extends GenealogyInput {
  depth: number;
  order: number;
  parentId: string | null;
}

/**
 * 族谱树布局：按输入顺序稳定 DFS；根 = ppid 为 null 或父不在集合中；
 * 环（a→b→a）安全——已访问节点不再下钻，防死循环。
 */
export function genealogyLayout(nodes: GenealogyInput[]): GenealogyNode[] {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const children = new Map<string | null, GenealogyInput[]>();
  for (const n of nodes) {
    const key = n.ppid != null && byId.has(n.ppid) ? n.ppid : null;
    const list = children.get(key) ?? [];
    list.push(n);
    children.set(key, list);
  }
  const out: GenealogyNode[] = [];
  const visited = new Set<string>();
  const walk = (n: GenealogyInput, depth: number, parentId: string | null): void => {
    if (visited.has(n.id)) return;
    visited.add(n.id);
    out.push({ ...n, depth, order: out.length, parentId });
    for (const c of children.get(n.id) ?? []) walk(c, depth + 1, n.id);
  };
  for (const root of children.get(null) ?? []) walk(root, 0, null);
  // 环上节点（所有父都在集合中但形成环）兜底：按输入顺序作为根补齐
  for (const n of nodes) {
    if (!visited.has(n.id)) walk(n, 0, null);
  }
  return out;
}

// ---- W-016 Z 序阴影深度 ----

export const Z_SHADOW_MAX_PX = 24;
export const Z_SHADOW_MIN_PX = 2;

/**
 * z 序 → 阴影深度（px）：rank 0（最顶层）= maxPx，逐层线性递减到 minPx。
 * 单调递减；layers=1（单窗）恒为 maxPx。
 */
export function zElevationPx(rank: number, layers: number, maxPx = Z_SHADOW_MAX_PX): number {
  if (layers <= 1) return maxPx;
  const t = clamp(rank / (layers - 1), 0, 1);
  return Math.max(Z_SHADOW_MIN_PX, Math.round(maxPx * (1 - t)));
}

// ---- W-017 空间气味标记 ----

/** 主题强调色相（tokens.css 三主题 oklch hue 实测：262 / 75 / 95）——新色相必须错开。 */
export const NOVA_RESERVED_HUES: readonly number[] = [262, 75, 95];

/**
 * 空间色相分配：以 47° 步进 + 23° 排解扫描，返回第一个与全部保留色相
 * 距离 ≥ minSep（环面最短弧）的色相。确定性、无冲突。
 */
export function deskHue(index: number, reserved: readonly number[], minSep = 20): number {
  const sep = (a: number, b: number): number => {
    const d = Math.abs(a - b) % 360;
    return Math.min(d, 360 - d);
  };
  for (let k = 0; k < 360; k++) {
    const hue = (((index * 47 + k * 23) % 360) + 360) % 360;
    if (reserved.every((r) => sep(hue, r) >= minSep)) return hue;
  }
  return (index * 47) % 360;
}

// ---- W-018 键盘吸附格阵 ----

export type LatticeKey = "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9";

/**
 * 九宫格吸附映射（小键盘布局：7 8 9 上排 / 1 2 3 下排）。
 * 5 = 整个工作区（居中最大化）；其余 = 三分格。全整数坐标且在工作区内。
 */
export function latticeRect(key: LatticeKey, wa: VwmRect): VwmRect {
  if (key === "5") return { x: wa.x, y: wa.y, w: wa.w, h: wa.h };
  const col = ((Number(key) - 1) % 3) as 0 | 1 | 2; // 0 左 1 中 2 右
  const row = key <= "3" ? 2 : key <= "6" ? 1 : 0; // 0 上 1 中 2 下
  const cw = Math.floor(wa.w / 3);
  const ch = Math.floor(wa.h / 3);
  const x = wa.x + col * cw;
  const y = wa.y + row * ch;
  // 末列/末行吃满余量，保证严丝合缝
  const w = col === 2 ? wa.x + wa.w - x : cw;
  const h = row === 2 ? wa.y + wa.h - y : ch;
  return { x, y, w, h };
}

/** 连按同格二次 = 恢复原位：同键且距上次 ≤ windowMs。 */
export function isLatticeRepeat(
  key: LatticeKey,
  lastKey: LatticeKey | null,
  lastMs: number,
  now: number,
  windowMs = 900,
): boolean {
  if (lastKey !== key) return false;
  return now - lastMs <= windowMs;
}

// ---- W-019 几何裁剪 ----

/** 宽高/坐标一键取整到 8px 网格（裁剪后坐标 mod 8 == 0）。 */
export function snap8Rect(r: VwmRect): VwmRect {
  const g = (v: number): number => Math.round(v / 8) * 8;
  return { x: g(r.x), y: g(r.y), w: Math.max(8, g(r.w)), h: Math.max(8, g(r.h)) };
}

/** 对齐屏幕安全区：距工作区四边统一 8px 留白（窗口钳入内框）。 */
export function alignSafeRect(r: VwmRect, wa: VwmRect, margin = 8): VwmRect {
  const x = clamp(r.x, wa.x + margin, Math.max(wa.x + margin, wa.x + wa.w - margin - r.w));
  const y = clamp(r.y, wa.y + margin, Math.max(wa.y + margin, wa.y + wa.h - margin - r.h));
  const w = Math.min(r.w, wa.w - margin * 2);
  const h = Math.min(r.h, wa.h - margin * 2);
  return { x, y, w: Math.max(8, w), h: Math.max(8, h) };
}

/** 最大化/贴边窗口（触两条以上工作区边缘）自动跳过裁剪。 */
export function isSnappedLike(w: { state: string; x: number; y: number; w: number; h: number }, wa: VwmRect): boolean {
  if (w.state === "max") return true;
  const tol = 2;
  let edges = 0;
  if (Math.abs(w.x - wa.x) <= tol) edges++;
  if (Math.abs(w.y - wa.y) <= tol) edges++;
  if (Math.abs(w.x + w.w - (wa.x + wa.w)) <= tol) edges++;
  if (Math.abs(w.y + w.h - (wa.y + wa.h)) <= tol) edges++;
  return edges >= 2;
}

// ---- W-020 焦点回声 ----

export const ECHO_AFTER_MS = 10_000;

/** 失焦超过 10s 的上一个焦点窗口具备回声资格。 */
export function echoEligible(unfocusedMs: number): boolean {
  return unfocusedMs >= ECHO_AFTER_MS;
}

// ---- W-021 马赛克顾问 ----

/** 拥挤度 = Σ窗口面积 / 工作区面积（百分比 0..100）。 */
export function crowdingOf(rects: VwmRect[], wa: VwmRect): number {
  if (wa.w <= 0 || wa.h <= 0) return 0;
  const sum = rects.reduce((acc, r) => acc + r.w * r.h, 0);
  return Math.round((sum / (wa.w * wa.h)) * 100);
}

export interface MosaicWin {
  id: string;
  w: number;
  h: number;
}

/**
 * 宽高比感知的镶嵌建议：≥3 窗且拥挤度 > 60% 才出建议。
 * 平均宽高比 > 1.4（横宽窗多）→ 列；< 0.72（竖高窗多）→ 行；其余 → 格。
 */
export function mosaicSuggestion(
  wins: MosaicWin[],
  crowding: number,
  minWins = 3,
  minCrowd = 60,
): { mode: "columns" | "grid" | "rows"; avgAspect: number } | null {
  if (wins.length < minWins || crowding <= minCrowd) return null;
  const avgAspect = wins.reduce((acc, w) => acc + (w.h > 0 ? w.w / w.h : 1), 0) / wins.length;
  const mode = avgAspect > 1.4 ? "columns" : avgAspect < 0.72 ? "rows" : "grid";
  return { mode, avgAspect: Math.round(avgAspect * 100) / 100 };
}

/** 忽略记忆 8h：期间不再打扰。 */
export function adviceBlocked(ignoredAt: number | null, now: number, hours = 8): boolean {
  if (ignoredAt == null) return false;
  return now - ignoredAt < hours * 3_600_000;
}

/** 建议落地：按模式把 n 个窗口均分工作区（与 Q-11 共享动画不共享算法）。 */
export function mosaicApplyRects(mode: "columns" | "grid" | "rows", n: number, wa: VwmRect): VwmRect[] {
  if (n <= 0) return [];
  const out: VwmRect[] = [];
  if (mode === "columns") {
    const cw = Math.floor(wa.w / n);
    for (let i = 0; i < n; i++) {
      const x = wa.x + i * cw;
      out.push({ x, y: wa.y, w: i === n - 1 ? wa.x + wa.w - x : cw, h: wa.h });
    }
    return out;
  }
  if (mode === "rows") {
    const ch = Math.floor(wa.h / n);
    for (let i = 0; i < n; i++) {
      const y = wa.y + i * ch;
      out.push({ x: wa.x, y, w: wa.w, h: i === n - 1 ? wa.y + wa.h - y : ch });
    }
    return out;
  }
  const cols = Math.ceil(Math.sqrt(n));
  const rows = Math.ceil(n / cols);
  const cw = Math.floor(wa.w / cols);
  const ch = Math.floor(wa.h / rows);
  for (let i = 0; i < n; i++) {
    const c = i % cols;
    const r = Math.floor(i / cols);
    const x = wa.x + c * cw;
    const y = wa.y + r * ch;
    out.push({
      x,
      y,
      w: c === cols - 1 ? wa.x + wa.w - x : cw,
      h: r === rows - 1 ? wa.y + wa.h - y : ch,
    });
  }
  return out;
}

// ---- W-022 窗口记忆仓位 ----

/** 两仓近乎相同（各维 ≤ tol px）视为同一仓位，不重复记录。 */
export function nearSame(a: VwmRect, b: VwmRect, tol = 2): boolean {
  return (
    Math.abs(a.x - b.x) <= tol &&
    Math.abs(a.y - b.y) <= tol &&
    Math.abs(a.w - b.w) <= tol &&
    Math.abs(a.h - b.h) <= tol
  );
}

/** 记录仓位：跳过近同仓；保留最近 max 个（旧→新序，超限裁最旧）。 */
export function pushSlot(slots: VwmRect[], r: VwmRect, max = 3): VwmRect[] {
  if (slots.length > 0 && nearSame(slots[slots.length - 1] as VwmRect, r)) return slots;
  const next = [...slots, r];
  return next.slice(Math.max(0, next.length - max));
}

/** 回滚：取距离当前几何最远的最近一仓（≠ 当前位置才回滚）。 */
export function pickRollback(
  slots: VwmRect[],
  current: VwmRect,
): { rect: VwmRect; rest: VwmRect[] } | null {
  for (let i = slots.length - 1; i >= 0; i--) {
    const rect = slots[i] as VwmRect;
    if (!nearSame(rect, current)) {
      return { rect, rest: [...slots.slice(0, i), ...slots.slice(i + 1)] };
    }
  }
  return null;
}

// ---- W-023 轨道浮窗 ----

export const RAIL_HEIGHT = 72;
export const RAIL_ENTER_PX = 8;

/** 拖到顶端（窗口顶距工作区顶 ≤ 8px）松手 → 入轨。 */
export function railDecision(winY: number, waY: number): "dock" | "none" {
  return winY - waY <= RAIL_ENTER_PX ? "dock" : "none";
}

/** 贴轨几何：高锁 72px、横向原位钳制、常驻工作区顶。 */
export function railGeom(wa: VwmRect, origX: number, origW: number): VwmRect {
  const w = Math.min(origW, wa.w);
  const x = clamp(origX, wa.x, Math.max(wa.x, wa.x + wa.w - w));
  return { x, y: wa.y, w, h: Math.min(RAIL_HEIGHT, wa.h) };
}

// ---- W-024 空间折叠带 ----

export const FOLD_W = 36;
export const FOLD_GAP = 4;
export const FOLD_EXPAND_MS = 350;

/** 折叠几何：右缘竖带 36px 格，自顶向下堆叠。 */
export function foldGeometry(count: number, wa: VwmRect): { ribbonX: number; cells: VwmRect[] } {
  const cells: VwmRect[] = [];
  const ribbonX = wa.x + wa.w - FOLD_W;
  const h = Math.min(FOLD_W, wa.h);
  for (let i = 0; i < count; i++) {
    const y = wa.y + i * (FOLD_W + FOLD_GAP);
    if (y + h > wa.y + wa.h) break; // 超出工作区即止（诚实容量）
    cells.push({ x: ribbonX, y, w: FOLD_W, h });
  }
  return { ribbonX, cells };
}

// ---- W-025 窗口标签 ----

export const LABEL_MAX = 12;

/** 标签规范化：去首尾空白、压缩连续空白、按码点截 12 字符；空 → null。 */
export function normalizeLabel(raw: string): string | null {
  const s = raw.replace(/\s+/g, " ").trim();
  if (!s) return null;
  const cps = Array.from(s);
  return cps.slice(0, LABEL_MAX).join("");
}

// ---------------------------------------------------------------------------
// 行为层（零侵入 DOM 叠层 + 事件）
// ---------------------------------------------------------------------------

let active = false;
type Unsub = () => void;
let bag: Unsub[] = [];
let syncScheduled = false;

/** 上一份窗口几何（W-022 变更检测 / W-023 入轨判定共用）。 */
const prevRects = new Map<string, VwmRect>();
/** W-022 仓位存储（id → 旧→新序，≤3）。 */
let slotsMap: Record<string, VwmRect[]> = {};
const slotDebounce = new Map<string, ReturnType<typeof setTimeout>>();
/** W-020 回声。 */
let echoId: string | null = null;
let echoSince = 0;
let echoTimer: ReturnType<typeof setInterval> | null = null;
/** W-023 入轨表（id → 原几何）。 */
const railOrig = new Map<string, VwmRect>();
/** W-024 折叠态。 */
let folded = false;
const foldSaved = new Map<string, VwmRect>();
/** W-018 连按记忆。 */
let lastLatticeKey: LatticeKey | null = null;
let lastLatticeMs = 0;
const latticeBackup = new Map<string, VwmRect>();
/** W-017 空间色相分配（group id → hue）。 */
let spaceHues: Record<string, number> = {};
let prevActiveGroups: string[] = [];
/** W-021 马赛克。 */
let mosaicShownAt = 0;
/** W-013 拖拽手感。 */
let massTimer: ReturnType<typeof setTimeout> | null = null;

function loadPersisted(): void {
  slotsMap = lsGet<Record<string, VwmRect[]>>(`${NS}.slots`, {});
  spaceHues = lsGet<Record<string, number>>(`${NS}.spaces`, {});
}

// ---- CSS（唯一注入点，nova- 前缀，全部走 tokens 语义变量） ----

const STYLE_ID = "nova-window-style";
const STYLE_TEXT = `
.nova-magnet-line{position:fixed;z-index:940;pointer-events:none;background:var(--accent);opacity:.55;transition:opacity var(--dur-2) var(--ease-standard)}
.nova-magnet-line.nova-confirm{animation:nova-breath 1.1s var(--ease-standard) infinite}
.nova-magnet-near{outline:1px solid var(--accent-soft);outline-offset:-1px}
@keyframes nova-breath{0%,100%{opacity:.25}50%{opacity:.85}}
[data-reduce-motion="true"] .nova-magnet-line.nova-confirm{animation:none;opacity:.55}
.nova-mass-heavy{box-shadow:0 16px 44px oklch(0 0 0 / 0.24)!important}
.nova-mass-drag{transition:left 60ms var(--ease-standard),top 60ms var(--ease-standard)}
.nova-mass-smooth{transition:left 80ms var(--ease-standard),top 80ms var(--ease-standard)}
.nova-echo{animation:nova-echo-glow 4s var(--ease-standard) infinite}
@keyframes nova-echo-glow{0%,100%{box-shadow:0 0 0 0 oklch(0 0 0 / 0)}45%{box-shadow:0 0 0 5px var(--accent-soft)}}
[data-reduce-motion="true"] .nova-echo{animation:none;box-shadow:0 0 0 2px var(--accent-soft)}
.nova-space-glow{position:fixed;inset:0;z-index:939;pointer-events:none;background:radial-gradient(ellipse at center,oklch(0.7 0.12 var(--nova-hue) / 0.14),transparent 62%);opacity:0;transition:opacity 600ms var(--ease-standard)}
.nova-space-glow.nova-on{opacity:1}
[data-reduce-motion="true"] .nova-space-glow{display:none}
.nova-fold-anim{transition:left 350ms var(--ease-emphasized),top 350ms var(--ease-emphasized),width 350ms var(--ease-emphasized),height 350ms var(--ease-emphasized)}
[data-reduce-motion="true"] .nova-fold-anim{transition:none}
.nova-folded{cursor:pointer}
.nova-winlabel{display:inline-block;max-width:140px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;margin-left:6px;padding:0 6px;border-radius:4px;background:var(--accent-soft);color:var(--ink,var(--fg,#inherit));font-size:11px;line-height:16px;vertical-align:middle;pointer-events:none}
.nova-genealogy-panel{position:fixed;left:12px;top:64px;z-index:945;min-width:230px;max-width:330px;max-height:60vh;overflow:auto;padding:10px 12px;border-radius:10px;background:var(--panel,var(--bg-raised,#111));box-shadow:0 12px 36px oklch(0 0 0 / 0.3);font-size:12px;color:var(--ink,var(--fg,#ddd))}
.nova-genealogy-panel h3{margin:0 0 8px;font-size:12px;font-weight:600;letter-spacing:.04em;opacity:.85}
.nova-genealogy-row{display:flex;align-items:center;gap:6px;padding:3px 4px;border-radius:6px;cursor:pointer;white-space:nowrap}
.nova-genealogy-row:hover{background:var(--accent-soft)}
.nova-genealogy-row .nova-glyph{opacity:.4;flex:none}
.nova-genealogy-row .nova-adopt{font-size:10px;padding:0 4px;border-radius:4px;background:var(--accent-soft);opacity:.9}
.nova-mosaic-card{position:fixed;right:16px;bottom:70px;z-index:945;padding:12px 14px;border-radius:12px;background:var(--panel,var(--bg-raised,#111));box-shadow:0 12px 36px oklch(0 0 0 / 0.3);font-size:12px;max-width:280px;color:var(--ink,var(--fg,#ddd))}
.nova-mosaic-card .nova-mosaic-actions{display:flex;gap:8px;margin-top:8px}
.nova-mosaic-card button{font:inherit;padding:3px 10px;border-radius:6px;border:1px solid var(--accent-soft);background:transparent;color:inherit;cursor:pointer}
.nova-mosaic-card button:hover{background:var(--accent-soft)}
`;

function ensureStyle(): void {
  if (typeof document === "undefined") return;
  if (document.getElementById(STYLE_ID)) return;
  const style = document.createElement("style");
  style.id = STYLE_ID;
  style.textContent = STYLE_TEXT;
  document.head.appendChild(style);
  bag.push(() => document.getElementById(STYLE_ID)?.remove());
}

function frameEl(id: string): HTMLElement | null {
  if (typeof document === "undefined") return null;
  return document.querySelector<HTMLElement>(`.vwm-window[data-winid="${CSS.escape(id)}"]`);
}

function wins(): VwmWin[] {
  return vwmStore.getState().wins;
}

function wa(): VwmRect {
  return vwmStore.getState().workArea;
}

// ---- W-013 拖拽质量手感 ----

function onDragStart(e: PointerEvent): void {
  const bar = (e.target as HTMLElement | null)?.closest?.(".vwm-titlebar") as HTMLElement | null;
  const id = bar?.dataset?.winid;
  if (!id || !flagOn("W-013") || !motionOK()) return;
  const w = wins().find((x) => x.id === id);
  if (!w || w.state !== "normal") return;
  const m = massInertia(areaRatioOf(w, wa()));
  if (!m.heavy) return;
  const el = frameEl(id);
  if (!el) return;
  el.classList.add("nova-mass-heavy", "nova-mass-drag");
  massTimer = setTimeout(() => {
    el.classList.remove("nova-mass-drag");
    el.classList.add("nova-mass-smooth");
  }, 140);
  const clear = (): void => {
    setTimeout(() => el.classList.remove("nova-mass-heavy", "nova-mass-smooth"), m.stopLingerMs);
    offUp();
  };
  const offUp = (): void => {
    document.removeEventListener("pointerup", clear, true);
    bag = bag.filter((fn) => fn !== offUp);
  };
  document.addEventListener("pointerup", clear, true);
  bag.push(offUp);
}

// ---- W-014 磁吸预告层 ----

let magnetLine: HTMLElement | null = null;
let magnetRaf = 0;
let dragId: string | null = null;

function magnetEnsureEl(): HTMLElement {
  if (!magnetLine || !magnetLine.isConnected) {
    magnetLine = document.createElement("div");
    magnetLine.className = "nova-magnet-line";
    magnetLine.setAttribute("aria-hidden", "true");
    document.body.appendChild(magnetLine);
    bag.push(() => magnetLine?.remove());
  }
  return magnetLine;
}

function onDragMove(): void {
  if (!dragId || !flagOn("W-014")) return;
  if (magnetRaf) return;
  magnetRaf = requestAnimationFrame(() => {
    magnetRaf = 0;
    const w = wins().find((x) => x.id === dragId);
    if (!w) return;
    const near = nearestSnapDistance({ x: w.x, y: w.y, w: w.w, h: w.h }, wa());
    const el = frameEl(w.id);
    if (!near || !el) return;
    const st = magnetState(near.dist);
    el.classList.toggle("nova-magnet-near", st.inField);
    const line = magnetEnsureEl();
    if (!st.inField) {
      line.style.display = "none";
      return;
    }
    line.style.display = "";
    line.classList.toggle("nova-confirm", st.confirm && motionOK());
    if (near.axis === "x") {
      line.style.left = `${near.linePos - 1}px`;
      line.style.top = `${wa().y}px`;
      line.style.width = "2px";
      line.style.height = `${wa().h}px`;
    } else {
      line.style.left = `${wa().x}px`;
      line.style.top = `${near.linePos - 1}px`;
      line.style.width = `${wa().w}px`;
      line.style.height = "2px";
    }
  });
}

function magnetClear(): void {
  dragId = null;
  if (magnetLine) magnetLine.style.display = "none";
  document.querySelectorAll(".vwm-window.nova-magnet-near").forEach((e) => e.classList.remove("nova-magnet-near"));
}

// ---- W-023 轨道浮窗 ----

function onPointerDownCapture(e: PointerEvent): void {
  const bar = (e.target as HTMLElement | null)?.closest?.(".vwm-titlebar") as HTMLElement | null;
  const id = bar?.dataset?.winid ?? null;
  dragId = id;
  if (id) onDragStart(e);
}

function onPointerUpCapture(): void {
  // 入轨判定：松手时窗口顶贴工作区顶
  if (dragId && flagOn("W-023")) {
    const w = wins().find((x) => x.id === dragId);
    if (w && w.state === "normal" && !railOrig.has(w.id) && railDecision(w.y, wa().y) === "dock") {
      dockRail(w.id);
    }
  }
  magnetClear();
}

function dockRail(id: string): void {
  const w = wins().find((x) => x.id === id);
  if (!w || w.state !== "normal") return;
  const g = railGeom(wa(), w.x, w.w);
  railOrig.set(id, { x: w.x, y: w.y, w: w.w, h: w.h });
  resizeVwmWin(id, g);
  setVwmTopmost(id, true);
  frameEl(id)?.classList.add("nova-rail");
  novaEvent("rail", { id, docked: true });
}

function undockRail(id: string): void {
  const orig = railOrig.get(id);
  railOrig.delete(id);
  const el = frameEl(id);
  el?.classList.remove("nova-rail");
  if (!orig) return;
  const waR = wa();
  const clamped = {
    x: clamp(orig.x, waR.x, Math.max(waR.x, waR.x + waR.w - orig.w)),
    y: clamp(orig.y, waR.y, Math.max(waR.y, waR.y + waR.h - orig.h)),
    w: orig.w,
    h: orig.h,
  };
  resizeVwmWin(id, clamped);
  setVwmTopmost(id, false);
  novaEvent("rail", { id, docked: false });
}

function onDblClickCapture(e: MouseEvent): void {
  const winEl = (e.target as HTMLElement | null)?.closest?.(".vwm-window") as HTMLElement | null;
  // W-024：Ctrl+双击桌面空白 = 折叠/展开全体
  if (e.ctrlKey && !winEl && (e.target as HTMLElement | null)?.closest?.('[data-testid="desktop-shell"]')) {
    if (flagOn("W-024")) toggleFold();
    return;
  }
  const id = winEl?.dataset?.winid;
  if (!id) return;
  // W-023：双击轨道窗恢复原几何
  if (railOrig.has(id) && flagOn("W-023")) {
    e.stopPropagation();
    undockRail(id);
  }
}

// ---- W-024 折叠带 ----

function visibleNormalIds(): string[] {
  const f = vwmStore.getState().focusedId;
  return wins()
    .filter((w) => w.state === "normal" && !w.minimized && isVwmWinVisible(w) && w.id !== f && !railOrig.has(w.id))
    .map((w) => w.id);
}

function toggleFold(): void {
  if (folded) expandAllFolded();
  else foldAll();
}

function withFoldAnim(fn: () => void): void {
  const els = document.querySelectorAll<HTMLElement>(".vwm-window[data-winid]");
  els.forEach((e) => e.classList.add("nova-fold-anim"));
  fn();
  setTimeout(() => els.forEach((e) => e.classList.remove("nova-fold-anim")), motionOK() ? FOLD_EXPAND_MS + 60 : 0);
}

function foldAll(): void {
  const ids = visibleNormalIds();
  if (ids.length === 0) return;
  const { cells } = foldGeometry(ids.length, wa());
  withFoldAnim(() => {
    ids.forEach((id, i) => {
      const w = wins().find((x) => x.id === id);
      const cell = cells[i];
      if (!w || !cell) return;
      foldSaved.set(id, { x: w.x, y: w.y, w: w.w, h: w.h });
      resizeVwmWin(id, cell);
      frameEl(id)?.classList.add("nova-folded");
    });
  });
  folded = true;
  novaEvent("fold", { folded: true, count: Math.min(ids.length, foldSaved.size) });
}

function expandFolded(id: string): void {
  const orig = foldSaved.get(id);
  if (!orig) return;
  foldSaved.delete(id);
  const waR = wa();
  withFoldAnim(() => {
    resizeVwmWin(id, {
      x: clamp(orig.x, waR.x, Math.max(waR.x, waR.x + waR.w - orig.w)),
      y: clamp(orig.y, waR.y, Math.max(waR.y, waR.y + waR.h - orig.h)),
      w: orig.w,
      h: orig.h,
    });
  });
  frameEl(id)?.classList.remove("nova-folded");
  if (foldSaved.size === 0) folded = false;
  novaEvent("fold", { folded: folded, expanded: id });
}

function expandAllFolded(): void {
  for (const id of Array.from(foldSaved.keys())) expandFolded(id);
  folded = false;
}

function onClickCapture(e: MouseEvent): void {
  if (!folded) return;
  const winEl = (e.target as HTMLElement | null)?.closest?.(".vwm-window") as HTMLElement | null;
  const id = winEl?.dataset?.winid;
  if (id && foldSaved.has(id)) expandFolded(id);
}

// ---- W-018 键盘格阵 ----

function onKeyDownCapture(e: KeyboardEvent): void {
  if (!e.altKey || !flagOn("W-018")) return;
  const m = /^Numpad([1-9])$/.exec(e.code);
  if (!m) return;
  const key = m[1] as LatticeKey;
  const id = vwmStore.getState().focusedId;
  const w = wins().find((x) => x.id === id);
  if (!id || !w) return;
  e.preventDefault();
  const now = Date.now();
  if (isLatticeRepeat(key, lastLatticeKey, lastLatticeMs, now)) {
    // 二次同格：恢复进格前的真实几何（内存备份优先，兜底 Z-37 每应用记忆）
    const back = latticeBackup.get(id);
    if (back) {
      latticeBackup.delete(id);
      if (w.state === "max") unmaxVwmTo(id, back);
      else resizeVwmWin(id, back);
    }
    lastLatticeKey = null;
    return;
  }
  const rect = latticeRect(key, wa());
  latticeBackup.set(id, { x: w.x, y: w.y, w: w.w, h: w.h });
  if (w.state === "max") unmaxVwmTo(id, rect);
  else resizeVwmWin(id, rect);
  lastLatticeKey = key;
  lastLatticeMs = now;
}

// ---- W-019 几何裁剪（导出动作 + 事件双入口，菜单挂载归 S17） ----

export function snap8Focused(mode: "grid" | "safe" = "grid"): boolean {
  if (!flagOn("W-019")) return false;
  const id = vwmStore.getState().focusedId;
  const w = wins().find((x) => x.id === id);
  if (!id || !w || w.state !== "normal") return false;
  const waR = wa();
  if (isSnappedLike(w, waR)) return false; // 最大化/贴边自动跳过
  const next = mode === "safe" ? alignSafeRect({ x: w.x, y: w.y, w: w.w, h: w.h }, waR) : snap8Rect({ x: w.x, y: w.y, w: w.w, h: w.h });
  resizeVwmWin(id, next);
  return true;
}

// ---- W-022 仓位 API（菜单挂载归 S17） ----

export function windowSlots(id: string): VwmRect[] {
  return [...(slotsMap[id] ?? [])];
}

export function rollbackWindowSlot(id: string): boolean {
  if (!flagOn("W-022")) return false;
  const w = wins().find((x) => x.id === id);
  if (!w || w.state !== "normal") return false;
  const pick = pickRollback(slotsMap[id] ?? [], { x: w.x, y: w.y, w: w.w, h: w.h });
  if (!pick) return false;
  slotsMap[id] = pick.rest;
  lsSet(`${NS}.slots`, slotsMap);
  resizeVwmWin(id, pick.rect);
  return true;
}

// ---- W-025 标签 API（Alt+Tab 呈现归 S17） ----

export function labelOf(id: string): string | null {
  return lsGet<Record<string, string>>(`${NS}.labels`, {})[id] ?? null;
}

export function setLabel(id: string, raw: string): string | null {
  const label = normalizeLabel(raw);
  const all = lsGet<Record<string, string>>(`${NS}.labels`, {});
  if (label) all[id] = label;
  else delete all[id];
  lsSet(`${NS}.labels`, all);
  novaEvent("labels-changed", { id, label });
  scheduleSync();
  return label;
}

// ---- W-015 族谱面板 ----

let genealogyPanel: HTMLElement | null = null;

export function toggleGenealogyPanel(force?: boolean): void {
  if (!flagOn("W-015")) return;
  const want = force ?? genealogyPanel == null;
  if (!want) {
    genealogyPanel?.remove();
    genealogyPanel = null;
    return;
  }
  if (genealogyPanel?.isConnected) return;
  const panel = document.createElement("div");
  panel.className = "nova-genealogy-panel";
  panel.setAttribute("role", "tree");
  panel.setAttribute("aria-label", "窗口族谱 Window Genealogy");
  document.body.appendChild(panel);
  genealogyPanel = panel;
  bag.push(() => {
    panel.remove();
    if (genealogyPanel === panel) genealogyPanel = null;
  });
  renderGenealogy();
}

function genealogyNodes(): ReturnType<typeof genealogyLayout> {
  const list: GenealogyInput[] = [];
  const byApp = new Map<string, number>();
  for (const w of wins()) {
    const n = (byApp.get(w.app) ?? 0) + 1;
    byApp.set(w.app, n);
    const adopted = w.app.startsWith("tp:");
    list.push({
      id: w.id,
      // 结构树：同应用首实例为根，后续实例为其子；收编（tp:）单节点标记
      ppid: n === 1 ? null : `family:${w.app}`,
      label: adopted ? `${w.app.slice(3)}（收编）` : vwmWindowTitle(w.app),
      adopted,
    });
  }
  return genealogyLayout(list);
}

function renderGenealogy(): void {
  const panel = genealogyPanel;
  if (!panel?.isConnected) return;
  panel.textContent = "";
  const h = document.createElement("h3");
  h.textContent = "窗口族谱 · GENEALOGY";
  panel.appendChild(h);
  for (const n of genealogyNodes()) {
    const row = document.createElement("div");
    row.className = "nova-genealogy-row";
    row.setAttribute("role", "treeitem");
    row.style.paddingLeft = `${4 + n.depth * 14}px`;
    const glyph = document.createElement("span");
    glyph.className = "nova-glyph";
    glyph.textContent = n.depth === 0 ? "▾" : "└";
    const name = document.createElement("span");
    name.textContent = n.label;
    row.append(glyph, name);
    if (n.adopted) {
      const badge = document.createElement("span");
      badge.className = "nova-adopt";
      badge.textContent = "EMBED";
      row.appendChild(badge);
    }
    row.addEventListener("click", () => focusVwmWin(n.id));
    row.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      const w = wins().find((x) => x.id === n.id);
      if (w) closeVwmApp(w.app); // 结束整树
    });
    panel.appendChild(row);
  }
}

// ---- W-017 空间辉光 ----

let glowEl: HTMLElement | null = null;
let glowTimer: ReturnType<typeof setTimeout> | null = null;

function flashSpaceGlow(groupKey: string): void {
  if (!flagOn("W-017") || !motionOK()) return;
  const idx = Object.keys(spaceHues).indexOf(groupKey);
  const hue = spaceHues[groupKey] ?? deskHue(Math.max(0, idx), NOVA_RESERVED_HUES);
  if (!glowEl || !glowEl.isConnected) {
    glowEl = document.createElement("div");
    glowEl.className = "nova-space-glow";
    glowEl.setAttribute("aria-hidden", "true");
    document.body.appendChild(glowEl);
    bag.push(() => glowEl?.remove());
  }
  glowEl.style.setProperty("--nova-hue", String(hue));
  glowEl.classList.add("nova-on");
  if (glowTimer) clearTimeout(glowTimer);
  glowTimer = setTimeout(() => glowEl?.classList.remove("nova-on"), 600);
}

// ---- W-021 马赛克建议卡 ----

let mosaicCard: HTMLElement | null = null;

function dismissMosaic(applyMode: "columns" | "grid" | "rows" | null): void {
  mosaicCard?.remove();
  mosaicCard = null;
  mosaicShownAt = Date.now();
  if (applyMode == null) {
    lsSet(`${NS}.mosaicIgnore`, Date.now());
    return;
  }
  const f = vwmStore.getState().focusedId;
  const targets = wins().filter((w) => w.state === "normal" && !w.minimized && isVwmWinVisible(w) && w.id !== f);
  const rects = mosaicApplyRects(applyMode, targets.length, wa());
  withFoldAnim(() => {
    targets.forEach((w, i) => {
      const r = rects[i];
      if (r) resizeVwmWin(w.id, r);
    });
  });
}

function maybeShowMosaic(): void {
  if (!flagOn("W-021") || mosaicCard?.isConnected) return;
  const waR = wa();
  const visible = wins().filter((w) => w.state === "normal" && !w.minimized && isVwmWinVisible(w));
  const crowd = crowdingOf(visible.map((w) => ({ x: w.x, y: w.y, w: w.w, h: w.h })), waR);
  const sug = mosaicSuggestion(visible.map((w) => ({ id: w.id, w: w.w, h: w.h })), crowd);
  if (!sug) return;
  const ignoredAt = lsGet<number | null>(`${NS}.mosaicIgnore`, null);
  if (adviceBlocked(ignoredAt, Date.now())) return;
  if (Date.now() - mosaicShownAt < 5 * 60_000) return; // 提示后 5min 冷却
  const card = document.createElement("div");
  card.className = "nova-mosaic-card";
  card.setAttribute("role", "status");
  const modeZh = sug.mode === "columns" ? "纵向列" : sug.mode === "rows" ? "横向行" : "方格";
  const msg = document.createElement("div");
  msg.textContent = `同屏 ${visible.length} 窗拥挤（${crowd}%）· 建议按宽高比排「${modeZh}」`;
  const actions = document.createElement("div");
  actions.className = "nova-mosaic-actions";
  const apply = document.createElement("button");
  apply.type = "button";
  apply.textContent = "应用";
  apply.addEventListener("click", () => dismissMosaic(sug.mode));
  const ignore = document.createElement("button");
  ignore.type = "button";
  ignore.textContent = "忽略";
  ignore.addEventListener("click", () => dismissMosaic(null));
  actions.append(apply, ignore);
  card.append(msg, actions);
  document.body.appendChild(card);
  mosaicCard = card;
  bag.push(() => {
    card.remove();
    if (mosaicCard === card) mosaicCard = null;
  });
  novaEvent("mosaic", { mode: sug.mode, crowding: crowd });
}

// ---- W-020 回声 ----

function echoTick(): void {
  if (!flagOn("W-020")) {
    setEchoClass(null);
    return;
  }
  const s = vwmStore.getState();
  if (echoId && (s.focusedId === echoId || !wins().some((w) => w.id === echoId))) {
    echoId = null; // 回归焦点或窗口已关 → 回声即止
  }
  const target = echoId ?? null;
  setEchoClass(target);
  if (target) {
    const w = wins().find((x) => x.id === target);
    if (w && echoEligible(Date.now() - echoSince)) {
      const btn = document.querySelector<HTMLElement>(
        `.taskbar .tb-btn[aria-label="${CSS.escape(vwmWindowTitle(w.app))}"]`,
      );
      btn?.classList.add("nova-echo");
    }
  }
}

function setEchoClass(id: string | null): void {
  document.querySelectorAll(".tb-btn.nova-echo").forEach((e) => {
    if (!id) e.classList.remove("nova-echo");
  });
  if (!id) return;
  const w = wins().find((x) => x.id === id);
  if (!w) return;
  const btn = document.querySelector<HTMLElement>(
    `.taskbar .tb-btn[aria-label="${CSS.escape(vwmWindowTitle(w.app))}"]`,
  );
  btn?.classList.add("nova-echo");
}

// ---- 同步主循环（store 订阅驱动，rAF 削峰） ----

function scheduleSync(): void {
  if (syncScheduled || typeof requestAnimationFrame === "undefined") return;
  syncScheduled = true;
  requestAnimationFrame(() => {
    syncScheduled = false;
    syncPass();
  });
}

function syncPass(): void {
  const s = vwmStore.getState();
  const waR = s.workArea;
  const alive = new Set(s.wins.map((w) => w.id));

  // W-022 仓位记录：真实几何变更（拖拽中/折叠中/最小化不记仓）
  if (flagOn("W-022")) {
    for (const w of s.wins) {
      const prev = prevRects.get(w.id);
      const cur = { x: w.x, y: w.y, w: w.w, h: w.h };
      prevRects.set(w.id, cur);
      const dragging = frameEl(w.id)?.classList.contains("dragging") ?? false;
      const changing = prev && !nearSame(prev, cur, 0.5);
      if (changing && w.state === "normal" && !w.minimized && !dragging && !foldSaved.has(w.id)) {
        const tid = w.id;
        if (!slotDebounce.has(tid)) {
          const t = setTimeout(() => {
            slotDebounce.delete(tid);
            const win = wins().find((x) => x.id === tid);
            if (!win || win.state !== "normal" || win.minimized) return;
            slotsMap[tid] = pushSlot(slotsMap[tid] ?? [], {
              x: win.x,
              y: win.y,
              w: win.w,
              h: win.h,
            });
            lsSet(`${NS}.slots`, slotsMap);
          }, 350);
          slotDebounce.set(tid, t);
        }
      }
    }
  }
  for (const id of Array.from(prevRects.keys())) if (!alive.has(id)) prevRects.delete(id);
  for (const [tid, t] of slotDebounce) if (!alive.has(tid)) clearTimeout(t), slotDebounce.delete(tid);

  // W-016 z 序阴影
  const vis = s.wins.filter((w) => !w.minimized && isVwmWinVisible(w));
  const sorted = [...vis].sort((a, b) => b.z - a.z);
  for (const [i, w] of sorted.entries()) {
    const el = frameEl(w.id);
    if (!el) continue;
    if (flagOn("W-016")) {
      const px = motionOK() ? zElevationPx(i, Math.max(2, Math.min(10, sorted.length))) : s.focusedId === w.id ? 12 : Z_SHADOW_MIN_PX;
      el.dataset.novaZ = "1";
      el.style.setProperty("--nova-zel", `${px}px`);
    } else {
      delete el.dataset.novaZ;
      el.style.removeProperty("--nova-zel");
    }
  }

  // W-020 回声追踪
  if (s.focusedId !== echoId && s.focusedId) {
    echoId = s.focusedId; // 刚获得焦点者记为「当前」；失焦瞬间起算
    echoSince = Date.now();
  }
  echoTick();

  // W-017 空间辉光：标签组激活差分
  if (flagOn("W-017")) {
    const groups = Array.from(new Set(s.wins.filter((w) => w.group).map((w) => w.group as string)));
    for (const g of groups) if (!(g in spaceHues)) spaceHues[g] = deskHue(Object.keys(spaceHues).length, NOVA_RESERVED_HUES);
    if (groups.length !== Object.keys(spaceHues).length) lsSet(`${NS}.spaces`, spaceHues);
    const activeGroups = Array.from(
      new Set(s.wins.filter((w) => w.group && w.groupActive).map((w) => w.group as string)),
    ).sort();
    const appeared = activeGroups.filter((g) => !prevActiveGroups.includes(g));
    if (prevActiveGroups.length > 0 && appeared.length > 0) flashSpaceGlow(appeared[0] as string);
    prevActiveGroups = activeGroups;
  }

  // W-023 轨道：入轨窗口被拖离顶端 → 脱轨还原
  for (const id of Array.from(railOrig.keys())) {
    if (!alive.has(id)) {
      railOrig.delete(id);
      continue;
    }
    const w = s.wins.find((x) => x.id === id);
    if (w && w.y > waR.y + RAIL_HEIGHT * 0.6 && w.state === "normal") undockRail(id);
  }

  // W-025 标签徽章跟随
  if (flagOn("W-025")) {
    const labels = lsGet<Record<string, string>>(`${NS}.labels`, {});
    for (const w of s.wins) {
      const el = frameEl(w.id);
      if (!el) continue;
      const bar = el.querySelector<HTMLElement>(".vwm-titlebar");
      if (!bar) continue;
      const label = labels[w.id];
      let badge = bar.querySelector<HTMLElement>(".nova-winlabel");
      if (label) {
        if (!badge) {
          badge = document.createElement("span");
          badge.className = "nova-winlabel";
          bar.appendChild(badge);
        }
        badge.textContent = label;
        badge.title = label;
      } else badge?.remove();
    }
  }
  document.querySelectorAll<HTMLElement>(".vwm-window[data-winid]").forEach((el) => {
    const id = el.dataset.winid;
    if (!id || !alive.has(id)) return;
    if (!flagOn("W-025") || !lsGet<Record<string, string>>(`${NS}.labels`, {})[id]) {
      el.querySelector(".nova-winlabel")?.remove();
    }
  });

  // W-021 马赛克建议
  maybeShowMosaic();

  // W-015 族谱刷新（≤300ms：O(n) 重渲染）
  renderGenealogy();
}

// ---- 激活 / 卸载（幂等） ----

export function activateWindowNova(): void {
  if (active || typeof document === "undefined") return;
  active = true;
  loadPersisted();
  ensureStyle();

  document.addEventListener("pointerdown", onPointerDownCapture, true);
  bag.push(() => document.removeEventListener("pointerdown", onPointerDownCapture, true));
  document.addEventListener("pointermove", onDragMove, true);
  bag.push(() => document.removeEventListener("pointermove", onDragMove, true));
  document.addEventListener("pointerup", onPointerUpCapture, true);
  bag.push(() => document.removeEventListener("pointerup", onPointerUpCapture, true));
  document.addEventListener("dblclick", onDblClickCapture, true);
  bag.push(() => document.removeEventListener("dblclick", onDblClickCapture, true));
  document.addEventListener("click", onClickCapture, true);
  bag.push(() => document.removeEventListener("click", onClickCapture, true));
  document.addEventListener("keydown", onKeyDownCapture, true);
  bag.push(() => document.removeEventListener("keydown", onKeyDownCapture, true));

  const offStore = vwmStore.subscribe(() => scheduleSync());
  bag.push(offStore);

  // S0/面板事件入口（napkin：命令面板/菜单经 CustomEvent 触发，零 import 依赖）
  const onSnap8 = (e: Event): void => {
    const mode = (e as CustomEvent).detail?.mode as "grid" | "safe" | undefined;
    snap8Focused(mode ?? "grid");
  };
  window.addEventListener("nova://win-snap8", onSnap8);
  bag.push(() => window.removeEventListener("nova://win-snap8", onSnap8));
  const onGenealogy = (): void => toggleGenealogyPanel();
  window.addEventListener("nova://win-genealogy-toggle", onGenealogy);
  bag.push(() => window.removeEventListener("nova://win-genealogy-toggle", onGenealogy));
  const onLabel = (e: Event): void => {
    const d = (e as CustomEvent).detail as { id?: string; label?: string } | undefined;
    if (d?.id && typeof d.label === "string") setLabel(d.id, d.label);
  };
  window.addEventListener("nova://win-label-set", onLabel);
  bag.push(() => window.removeEventListener("nova://win-label-set", onLabel));

  echoTimer = setInterval(echoTick, 1000);
  bag.push(() => {
    if (echoTimer) clearInterval(echoTimer);
    echoTimer = null;
  });

  syncPass();
}

export function deactivateWindowNova(): void {
  if (!active) return;
  active = false;
  for (const fn of bag) {
    try {
      fn();
    } catch {
      /* 卸载容错 */
    }
  }
  bag = [];
  magnetLine = null;
  glowEl = null;
  genealogyPanel = null;
  mosaicCard = null;
  if (massTimer) clearTimeout(massTimer);
  massTimer = null;
  for (const t of slotDebounce.values()) clearTimeout(t);
  slotDebounce.clear();
  prevRects.clear();
  railOrig.clear();
  foldSaved.clear();
  folded = false;
  lastLatticeKey = null;
  prevActiveGroups = [];
  document.querySelectorAll(".nova-echo").forEach((e) => e.classList.remove("nova-echo"));
}

export function isWindowNovaActive(): boolean {
  return active;
}
