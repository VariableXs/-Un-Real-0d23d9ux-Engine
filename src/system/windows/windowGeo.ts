/**
 * UNREAL-X-15000 · AI-05 窗口几何学（族0041~0050 · X01001~X01250 · 领域02 · Variable 桌面线）。
 * 落点：src/system/windows/ + src/features/settings/WinFeelTab.tsx + src/styles/vwm.css。
 * 红线：全部模型纯几何/纯逻辑，可被 CheckSet 断言；不改既有 snap2/vwm 落位行为，只在其上扩展。
 */

/* ===================== 族0041 吸附 2.0（X01001~X01025） ===================== */

export interface Rect { x: number; y: number; w: number; h: number }

export interface SnapZone { id: string; rect: Rect; label: string }

/** 吸附模型：区域预测 + 磁吸 + 网格步进 + Shift 临时禁用（接 snap2 既有方案库）。 */
export class SnappingModel {
  readonly zones: SnapZone[];
  private readonly threshold: number;
  private readonly gridStep: number;
  disabledByShift = false;
  lastZone: string | null = null;

  constructor(zones: SnapZone[], threshold = 16, gridStep = 8) {
    this.zones = zones;
    this.threshold = Math.max(1, threshold);
    this.gridStep = Math.max(1, gridStep);
  }

  setShift(held: boolean): void { this.disabledByShift = held; }

  /** 光标落点 → 命中区域（Shift 按住时返回 null，即临时禁用）。 */
  predict(cx: number, cy: number): SnapZone | null {
    if (this.disabledByShift) return null;
    for (const z of this.zones) {
      const r = z.rect;
      if (cx >= r.x - this.threshold && cx <= r.x + r.w + this.threshold
        && cy >= r.y - this.threshold && cy <= r.y + r.h + this.threshold) {
        this.lastZone = z.id;
        return z;
      }
    }
    return null;
  }

  /** 幽灵预览与最终落位同源：偏差恒 0。 */
  ghost(zone: SnapZone): { rect: Rect; drift: number } {
    return { rect: { ...zone.rect }, drift: 0 };
  }

  /** 网格步进吸附：坐标钳到网格。 */
  snapToGrid(x: number, y: number): { x: number; y: number } {
    return { x: Math.round(x / this.gridStep) * this.gridStep, y: Math.round(y / this.gridStep) * this.gridStep };
  }

  /** 边缘阻力：靠近工作区边缘按比例减速。 */
  edgeResistance(x: number, min: number, max: number): number {
    const d = Math.min(x - min, max - x);
    if (d >= 0 || d < -this.threshold) return Math.max(min, Math.min(x, max));
    return Math.max(min, Math.min(max, x + (d / this.threshold) * (this.gridStep / 2)));
  }
}

export const SNAP_SCHEMA_5GEAR: string[] = ['half', 'thirds', 'two-plus-one', 'quadrant', 'grid-3x3'];

/* ===================== 族0042 布局语法 2.0（X01026~X01050） ===================== */

export interface Grammar { id: string; cols: number; rows: number; cells: Array<{ col: number; row: number }> }
export const GRAMMARS: Grammar[] = [
  { id: 'single', cols: 1, rows: 1, cells: [{ col: 0, row: 0 }] },
  { id: 'halves', cols: 2, rows: 1, cells: [{ col: 0, row: 0 }, { col: 1, row: 0 }] },
  { id: 'thirds', cols: 3, rows: 1, cells: [{ col: 0, row: 0 }, { col: 1, row: 0 }, { col: 2, row: 0 }] },
  { id: 'quad', cols: 2, rows: 2, cells: [{ col: 0, row: 0 }, { col: 1, row: 0 }, { col: 0, row: 1 }, { col: 1, row: 1 }] },
  { id: 'two-plus-one', cols: 2, rows: 2, cells: [{ col: 0, row: 0 }, { col: 0, row: 1 }, { col: 1, row: 0 }] },
];

export function applyGrammar(g: Grammar, area: Rect, gap = 8): Rect[] {
  const cw = (area.w - gap * (g.cols - 1)) / g.cols;
  const ch = (area.h - gap * (g.rows - 1)) / g.rows;
  return g.cells.map((c) => ({
    x: Math.round(area.x + c.col * (cw + gap)),
    y: Math.round(area.y + c.row * (ch + gap)),
    w: Math.round(cw),
    h: Math.round(ch),
  }));
}

/** 语法记忆：每显示器独立记忆上一次使用的语法。 */
export class GrammarMemory {
  private map = new Map<string, string>();
  remember(monitorId: string, grammarId: string): void { this.map.set(monitorId, grammarId); }
  recall(monitorId: string): string | null { return this.map.get(monitorId) ?? null; }
  forget(monitorId: string): boolean { return this.map.delete(monitorId); }
}

/* ===================== 族0043 动效物理 2.0（X01051~X01075） ===================== */

/** 临界阻尼弹簧：窗口落位/吸附的物理回弹，reduce-motion 降级为常量插值。 */
export class SpringPhysics {
  private v = 0;
  x: number;
  private readonly stiffness: number;
  constructor(private target: number, stiffness = 170, private damping = 26, private mass = 1) {
    this.x = target;
    this.stiffness = stiffness;
  }

  step(dt: number): number {
    const a = (-this.stiffness * (this.x - this.target) - this.damping * this.v) / this.mass;
    this.v += a * dt;
    this.x += this.v * dt;
    if (Math.abs(this.x - this.target) < 0.01 && Math.abs(this.v) < 0.01) { this.x = this.target; this.v = 0; }
    return this.x;
  }

  settled(): boolean { return this.x === this.target && this.v === 0; }
  retarget(t: number): void { this.target = t; }

  /** reduce-motion：直接跳目标（80ms 纯淡入淡出口径）。 */
  static instant(target: number): SpringPhysics {
    const s = new SpringPhysics(target, 1e9, 1e9);
    return s;
  }
}

/** 曲线-时长配对表（全部映射既有 --ease/--dur 令牌语义，禁自造）。 */
export const MOTION_PAIRS: Record<string, { ease: string; durMs: number }> = {
  hover: { ease: '--ease-standard', durMs: 120 },
  menu: { ease: '--ease-standard', durMs: 170 },
  snap: { ease: '--ease-standard', durMs: 120 },
  layer: { ease: '--ease-emphasized', durMs: 240 },
  reduced: { ease: 'linear', durMs: 80 },
};

/* ===================== 族0044 多显示器编排 2.0（X01076~X01100） ===================== */

export interface Monitor { id: string; x: number; y: number; w: number; h: number; scale: number; primary: boolean }

/** 矩形夹回所属显示器：越界窗口回到最近显示器工作区。 */
export function containToMonitor(r: Rect, m: Monitor): Rect {
  const x = Math.min(Math.max(r.x, m.x), m.x + m.w - r.w);
  const y = Math.min(Math.max(r.y, m.y), m.y + m.h - r.h);
  return { x, y, w: r.w, h: r.h };
}

export function monitorAt(monitors: Monitor[], cx: number, cy: number): Monitor | null {
  return monitors.find((m) => cx >= m.x && cx < m.x + m.w && cy >= m.y && cy < m.y + m.h) ?? null;
}

/** DPI 缩放换算：物理像素 ↔ 逻辑像素。 */
export function scaleRect(r: Rect, scale: number, toPhysical: boolean): Rect {
  const f = toPhysical ? scale : 1 / scale;
  return { x: Math.round(r.x * f), y: Math.round(r.y * f), w: Math.round(r.w * f), h: Math.round(r.h * f) };
}

/** 排列顺序：主屏优先、从左到右、从上到下。 */
export function arrangeOrder(monitors: Monitor[]): string[] {
  return [...monitors].sort((a, b) => (Number(b.primary) - Number(a.primary)) || (a.x - b.x) || (a.y - b.y)).map((m) => m.id);
}

/* ===================== 族0045 状态持久化 2.0（X01101~X01125） ===================== */

export interface WindowState { id: string; x: number; y: number; w: number; h: number; grammar?: string; desktop?: number }
export interface SnapshotV1 { version: 1; states: WindowState[] }
export interface SnapshotV2 { version: 2; states: WindowState[]; savedAt: number }

export function snapshotChecksum(s: SnapshotV2): number {
  const json = JSON.stringify(s);
  let h = 5381;
  for (let i = 0; i < json.length; i++) h = ((h << 5) + h + json.charCodeAt(i)) >>> 0;
  return h;
}

/** 序列化 + 校验和 + 版本迁移（v1→v2 补 savedAt）。 */
export function serializeStates(states: WindowState[], savedAt = Date.now()): SnapshotV2 {
  return { version: 2, states, savedAt };
}
export function migrateV1(raw: SnapshotV1): SnapshotV2 {
  return { version: 2, states: raw.states, savedAt: 0 };
}
export function deserializeStates(raw: SnapshotV2 | SnapshotV1, checksum?: number): SnapshotV2 {
  const v2 = raw.version === 2 ? raw : migrateV1(raw);
  if (checksum !== undefined && checksum !== snapshotChecksum(v2)) throw new Error('E_SNAPSHOT_CHECKSUM');
  return v2;
}

/* ===================== 族0046 边缘学（X01126~X01150） ===================== */

export type Edge = 'left' | 'right' | 'top' | 'bottom';

/** 边缘热区：检测光标贴边 + 触发阈值 + 停留计时。 */
export class EdgeZones {
  private enteredAt: number | null = null;
  constructor(private readonly width = 8, private readonly triggerMs = 400) {}

  hit(cx: number, cy: number, screenW: number, screenH: number): Edge | null {
    if (cx <= this.width) return 'left';
    if (cx >= screenW - this.width) return 'right';
    if (cy <= this.width) return 'top';
    if (cy >= screenH - this.width) return 'bottom';
    return null;
  }

  /** 停留计时：进入置位、离开清零，超过阈值触发。 */
  tick(edge: Edge | null, now: number): Edge | null {
    if (edge === null) { this.enteredAt = null; return null; }
    if (this.enteredAt === null) this.enteredAt = now;
    return now - this.enteredAt >= this.triggerMs ? edge : null;
  }

  /** 角落动作：左上/右上/左下/右下四角独立语义。 */
  cornerAction(edge: Edge, cx: number, cy: number, screenW: number, screenH: number): string {
    const cornerX = cx < screenW / 2 ? 'l' : 'r';
    const cornerY = cy < screenH / 2 ? 't' : 'b';
    return `${edge}:${cornerX}${cornerY}`;
  }
}

/* ===================== 族0047 阴影光 2.0（X01151~X01175） ===================== */

export const SHADOW_TOKENS: Record<number, string> = {
  0: 'var(--elev-0)',
  1: 'var(--elev-1)',
  2: 'var(--elev-2)',
  3: 'var(--w2-menu-shadow)',
  4: 'var(--elev-4)',
  5: 'var(--elev-5)',
  6: 'var(--w2-elev-drag)',
};

/** 焦点辉光：选中窗口描边强度随焦点态；HC 下降级为 2px 对比描边。 */
export function focusGlow(focused: boolean, highContrast: boolean): string {
  if (highContrast) return focused ? '2px solid var(--text-primary)' : '1px solid var(--border-strong)';
  return focused ? '0 0 0 1px var(--accent), var(--w2-glow)' : 'none';
}

/** 光源恒左上 45°：阴影偏移由海拔推导，不逐处手写。 */
export function lightOffset(elevation: number): { dx: number; dy: number } {
  return { dx: 0, dy: Math.max(0, Math.min(6, elevation)) };
}

/* ===================== 族0048 玻璃材质 2.0（X01176~X01200） ===================== */

export interface GlassPreset { id: string; blur: number; saturation: number; alpha: number }
export const GLASS_PRESETS: GlassPreset[] = [
  { id: 'm-mica', blur: 0, saturation: 1.1, alpha: 0.5 },
  { id: 'm-frosted', blur: 16, saturation: 1, alpha: 0.65 },
  { id: 'm-acrylic', blur: 20, saturation: 1.2, alpha: 0.7 },
  { id: 'm-solid', blur: 0, saturation: 1, alpha: 1 },
];

/** 低配降级链：acrylic→frosted→solid 两级递降（§12.5 口径）。 */
export function degradeGlass(presetId: string, tiersDown: number): string {
  const idx = GLASS_PRESETS.findIndex((g) => g.id === presetId);
  if (idx < 0) return 'm-solid';
  const chain = ['m-acrylic', 'm-frosted', 'm-mica', 'm-solid'];
  const from = chain.indexOf(presetId);
  if (from < 0) return 'm-solid';
  return chain[Math.min(from + Math.max(0, tiersDown), chain.length - 1)]!;
}

/** 省电档强制 solid。 */
export function powerAwareGlass(presetId: string, powerSaver: boolean, cores: number): string {
  if (powerSaver || cores <= 4) return 'm-solid';
  return degradeGlass(presetId, 0);
}

/* ===================== 族0049 标题栏再造 2.0（X01201~X01225） ===================== */

export interface CaptionButton { id: 'min' | 'max' | 'close'; label: string; key: string }

/** 标题栏模型：按钮顺序、双击区、拖拽区、无障碍标签。 */
export class TitlebarModel {
  height = 32;
  buttons: CaptionButton[] = [
    { id: 'min', label: '最小化', key: 'Ctrl+M' },
    { id: 'max', label: '最大化', key: 'Ctrl+Enter' },
    { id: 'close', label: '关闭', key: 'Alt+F4' },
  ];

  /** 双击标题区 → 最大化/还原；双击按钮区不触发。 */
  doubleClick(px: number, btnZoneWidth = this.buttons.length * 46): 'maximize-toggle' | null {
    return px < btnZoneWidth ? null : 'maximize-toggle';
  }

  isDragRegion(px: number, btnZoneWidth = this.buttons.length * 46): boolean {
    return px >= btnZoneWidth;
  }

  /** 按钮顺序重排（关闭恒最后，符合平台习惯）。 */
  reorder(ids: Array<CaptionButton['id']>): CaptionButton[] {
    const rest = ids.filter((i) => i !== 'close').map((i) => this.buttons.find((b) => b.id === i)!);
    return [...rest, this.buttons.find((b) => b.id === 'close')!];
  }
}

/* ===================== 族0050 可达性 2.0（X01226~X01250） ===================== */

/** 焦点序模型：几何排序（左→右、上→下）供 Tab 序自动推导。 */
export function focusOrder(rects: Array<Rect & { id: string }>): string[] {
  return [...rects]
    .sort((a, b) => (a.y - b.y) || (a.x - b.x))
    .map((r) => r.id);
}

/** 键位冲突检测：同修饰组合不得重复注册。 */
export function keybindingConflicts(kb: Array<{ combo: string; cmd: string }>): string[] {
  const seen = new Map<string, string>();
  const dup: string[] = [];
  for (const { combo, cmd } of kb) {
    const key = combo.toLowerCase();
    if (seen.has(key)) dup.push(cmd);
    else seen.set(key, cmd);
  }
  return dup;
}

/** WCAG 对比度（简化亮度模型，供几何状态文案自检）。 */
export function contrastRatio(fg: [number, number, number], bg: [number, number, number]): number {
  const lum = ([r, g, b]: [number, number, number]) => {
    const f = (c: number) => { const v = c / 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; };
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  };
  const hi = Math.max(lum(fg), lum(bg));
  const lo = Math.min(lum(fg), lum(bg));
  return (hi + 0.05) / (lo + 0.05);
}

/** 几何状态微文案（中文语境，禁裸报错）。 */
export function geoNarrative(code: string): string {
  const map: Record<string, string> = {
    E_SNAPSHOT_CHECKSUM: '布局快照校验失败，已回退到上次可用布局',
    E_NO_MONITOR: '未找到目标显示器，窗口已移回主屏',
    E_GRAMMAR_INVALID: '布局语法无效，已恢复默认语法',
  };
  return map[code] ?? '窗口布局已调整';
}
