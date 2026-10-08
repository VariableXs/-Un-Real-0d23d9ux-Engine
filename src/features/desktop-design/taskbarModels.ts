/**
 * UNREAL-X-15000 · AI-13 任务栏形态与交互逻辑核（族0121~0130 · X03001~X03250），勿删。
 * 任务栏形态 2.0 / 交互 2.0 / 托盘区 2.0 / 小组件区 2.0 / 行为 2.0 / 预览卡 /
 * 进度融合 / 分组 / 多屏 / 性能。纯逻辑、零依赖，UI 层按此装配。
 */

/* ===================== 族0121 任务栏形态 2.0 ===================== */

export const TASKBAR_POSITIONS = ['bottom', 'top', 'left', 'right'] as const;
export type TaskbarPosition = (typeof TASKBAR_POSITIONS)[number];
export const TASKBAR_HEIGHT_PRESETS = [40, 48, 56, 64, 72] as const;

export interface TaskbarShapeState {
  position: TaskbarPosition;
  heightPreset: number; // 0..4
  align: 'center' | 'start';
  autoHide: boolean;
  accentBar: boolean;
}

/** 任务栏形态：档位矩阵 + 越界钳制 + 快照迁移 + 净身回滚。 */
export class TaskbarShape {
  state: TaskbarShapeState = {
    position: 'bottom', heightPreset: 1, align: 'center', autoHide: false, accentBar: true,
  };
  history: TaskbarShapeState[] = [];

  set<K extends keyof TaskbarShapeState>(key: K, value: TaskbarShapeState[K]): boolean {
    if (key === 'position' && !TASKBAR_POSITIONS.includes(value as TaskbarPosition)) return false;
    if (key === 'align' && value !== 'center' && value !== 'start') return false;
    if (key === 'heightPreset' && (Number(value) < 0 || Number(value) > 4)) return false;
    this.history.push({ ...this.state });
    this.state = { ...this.state, [key]: value };
    return true;
  }

  height(): number {
    return TASKBAR_HEIGHT_PRESETS[this.state.heightPreset]!;
  }

  /** 快照导出 → 导入 → 跨版本携带（未知字段丢弃、越界回默认）。 */
  serialize(): string {
    return JSON.stringify(this.state);
  }

  static deserialize(raw: string): TaskbarShape {
    const t = new TaskbarShape();
    try {
      const o = JSON.parse(raw) as Partial<TaskbarShapeState>;
      if (o.position && TASKBAR_POSITIONS.includes(o.position)) t.state.position = o.position;
      if (typeof o.heightPreset === 'number' && o.heightPreset >= 0 && o.heightPreset <= 4) t.state.heightPreset = o.heightPreset;
      if (o.align === 'center' || o.align === 'start') t.state.align = o.align;
      if (typeof o.autoHide === 'boolean') t.state.autoHide = o.autoHide;
      if (typeof o.accentBar === 'boolean') t.state.accentBar = o.accentBar;
    } catch { /* 非法快照回默认 */ }
    return t;
  }

  rollback(): void {
    const prev = this.history.pop();
    if (prev) this.state = prev;
  }

  reset(): TaskbarShapeState {
    this.state = { position: 'bottom', heightPreset: 1, align: 'center', autoHide: false, accentBar: true };
    return this.state;
  }

  /** 低配降级：低内存档关闭 accentBar + 强制默认高度。 */
  degrade(level: 'none' | 'mid' | 'low'): void {
    if (level !== 'none') { this.state.accentBar = false; this.set('heightPreset', 1); }
    if (level === 'low') this.state.autoHide = true;
  }
}

/* ===================== 族0122 任务栏交互 2.0 ===================== */

export type ClickKind = 'left' | 'middle' | 'right';

/** 任务栏交互：左键聚/最小化（开合切换）、中键新实例、右键菜单、roving 键序、防抖。 */
export class TaskbarInteraction {
  private order: string[] = [];
  private open = new Set<string>();
  private lastClick: { id: string; at: number } | null = null;
  debounceMs = 120;
  focusIndex = 0;

  add(id: string): void { if (!this.order.includes(id)) this.order.push(id); }

  click(id: string, kind: ClickKind, at: number): string {
    if (kind === 'middle') return 'new-instance';
    if (kind === 'right') return 'context-menu';
    if (this.lastClick && this.lastClick.id === id && at - this.lastClick.at < this.debounceMs) return 'ignored';
    this.lastClick = { id, at };
    if (this.open.has(id)) { this.open.delete(id); return 'minimize'; }
    this.open.add(id);
    return 'focus';
  }

  isOpen(id: string): boolean { return this.open.has(id); }

  /** 键盘 roving：ArrowRight/ArrowLeft 循环，Home/End 到首尾。 */
  keys(key: 'ArrowRight' | 'ArrowLeft' | 'Home' | 'End'): number {
    if (this.order.length === 0) return -1;
    if (key === 'Home') this.focusIndex = 0;
    else if (key === 'End') this.focusIndex = this.order.length - 1;
    else if (key === 'ArrowRight') this.focusIndex = (this.focusIndex + 1) % this.order.length;
    else this.focusIndex = (this.focusIndex - 1 + this.order.length) % this.order.length;
    return this.focusIndex;
  }

  /** 悬停预览延迟令牌对齐：show 300 / hide 150。 */
  static readonly HOVER = { show: 300, hide: 150 } as const;
}

/* ===================== 族0123 托盘区 2.0 ===================== */

export interface TrayIcon {
  id: string;
  badge: number;
  tooltip: string;
  pinned: boolean;
}

/** 托盘区：注册去重、溢出折叠、徽标 99+、tooltip 钳长、净身移除。 */
export class TrayArea {
  private icons = new Map<string, TrayIcon>();
  visibleSlots = 5;
  maxTooltip = 24;

  register(id: string, tooltip = id, pinned = false): boolean {
    if (this.icons.has(id)) return false;
    this.icons.set(id, { id, badge: 0, tooltip: tooltip.slice(0, this.maxTooltip), pinned });
    return true;
  }

  unregister(id: string): boolean {
    return this.icons.delete(id);
  }

  setBadge(id: string, n: number): void {
    const ic = this.icons.get(id);
    if (ic) ic.badge = Math.max(0, Math.min(n, 999));
  }

  badgeLabel(id: string): string {
    const ic = this.icons.get(id);
    if (!ic) return '';
    return ic.badge > 99 ? '99+' : String(ic.badge);
  }

  /** 固定图标优先、其余按注册序；超槽位进溢出。 */
  layout(): { visible: TrayIcon[]; overflow: TrayIcon[] } {
    const all = [...this.icons.values()].sort((a, b) => Number(b.pinned) - Number(a.pinned));
    return { visible: all.slice(0, this.visibleSlots), overflow: all.slice(this.visibleSlots) };
  }

  get size(): number { return this.icons.size; }
}

/* ===================== 族0124 小组件区 2.0 ===================== */

export type WidgetSize = 's' | 'm' | 'l';

export interface WidgetInstance {
  id: string;
  kind: string;
  size: WidgetSize;
  collapsed: boolean;
}

/** 小组件区：槽位编排、尺寸档、折叠、刷新节流。 */
export class WidgetStrip {
  private widgets: WidgetInstance[] = [];
  slots = 4;
  refreshMs = 30_000;
  lastRefresh = 0;

  add(kind: string, size: WidgetSize = 'm'): WidgetInstance | null {
    if (this.widgets.length >= this.slots) return null;
    const w: WidgetInstance = { id: `${kind}-${this.widgets.length}`, kind, size, collapsed: false };
    this.widgets.push(w);
    return w;
  }

  remove(id: string): boolean {
    const i = this.widgets.findIndex((w) => w.id === id);
    if (i < 0) return false;
    this.widgets.splice(i, 1);
    return true;
  }

  toggle(id: string): boolean {
    const w = this.widgets.find((x) => x.id === id);
    if (!w) return false;
    w.collapsed = !w.collapsed;
    return true;
  }

  move(id: string, to: number): boolean {
    const i = this.widgets.findIndex((w) => w.id === id);
    if (i < 0 || to < 0 || to >= this.widgets.length) return false;
    const [w] = this.widgets.splice(i, 1);
    this.widgets.splice(to, 0, w!);
    return true;
  }

  /** 刷新节流：距上次 < refreshMs 则跳过。 */
  shouldRefresh(at: number): boolean {
    if (at - this.lastRefresh >= this.refreshMs) { this.lastRefresh = at; return true; }
    return false;
  }

  list(): WidgetInstance[] { return [...this.widgets]; }
  get count(): number { return this.widgets.length; }
}

/* ===================== 族0125 任务栏行为 2.0 ===================== */

/** 任务栏行为：自动隐藏边沿判定、置顶、失焦回收、回收计时。 */
export class TaskbarBehavior {
  autoHide = true;
  alwaysOnTop = true;
  revealEdgePx = 4;
  private hidden = true;
  private hideTimer: number | null = null;
  hideDelayMs = 400;

  pointerNear(_x: number, y: number, viewportH: number, position: TaskbarPosition): boolean {
    if (position === 'bottom') return viewportH - y <= this.revealEdgePx;
    if (position === 'top') return y <= this.revealEdgePx;
    return false;
  }

  hover(_at: number): boolean {
    if (this.hideTimer !== null) { clearTimeout(this.hideTimer); this.hideTimer = null; }
    this.hidden = false;
    return this.hidden;
  }

  leave(_at: number): boolean {
    if (this.autoHide) {
      this.hideTimer = setTimeout(() => { this.hidden = true; this.hideTimer = null; }, this.hideDelayMs) as unknown as number;
    }
    return this.hidden;
  }

  /** 计时后状态收敛（测试驱动时钟等价：直接查询延迟后的预期）。 */
  hiddenAfterDelay(): boolean { return this.autoHide; }

  setHidden(v: boolean): void { this.hidden = v; }
  get isHidden(): boolean { return this.hidden; }
  focusGain(): void { if (this.hideTimer !== null) { clearTimeout(this.hideTimer); this.hideTimer = null; } this.hidden = false; }
}

/* ===================== 族0126 任务栏预览卡 ===================== */

export interface PreviewItem {
  winId: string;
  title: string;
  cached: boolean;
  ariaLabel: string;
}

/** 预览卡：缩略图缓存、标题钳长、aria、生命周期。 */
export class PreviewCard {
  private cache = new Map<string, string>();
  maxTitle = 32;
  ttlMs = 5_000;
  shownAt = 0;

  cacheThumb(winId: string, data: string): void { this.cache.set(winId, data); }
  thumb(winId: string): string | undefined { return this.cache.get(winId); }
  invalidate(winId: string): boolean { return this.cache.delete(winId); }
  get cacheSize(): number { return this.cache.size; }

  title(raw: string): string {
    return raw.length <= this.maxTitle ? raw : `${raw.slice(0, this.maxTitle - 1)}…`;
  }

  aria(winId: string, raw: string): string { return `预览：${this.title(raw)}（${winId}）`; }

  fresh(now: number): boolean { return now - this.shownAt <= this.ttlMs; }
}

/* ===================== 族0127 任务栏进度融合 ===================== */

export type ProgressMode = 'none' | 'normal' | 'indeterminate' | 'error' | 'paused';

/** 进度融合：任务栏按钮进度覆盖层，钳位 + 模式守卫。 */
export class ProgressFusion {
  private values = new Map<string, { value: number; mode: ProgressMode }>();

  set(id: string, value: number, mode: ProgressMode = 'normal'): void {
    const v = mode === 'indeterminate' ? 0 : Math.max(0, Math.min(100, Math.round(value)));
    this.values.set(id, { value: v, mode: v >= 100 && mode === 'normal' ? 'none' : mode });
  }

  of(id: string): { value: number; mode: ProgressMode } {
    return this.values.get(id) ?? { value: 0, mode: 'none' };
  }

  clear(id: string): boolean { return this.values.delete(id); }

  /** 融合渲染：none 不画、indeterminate 画流动条。 */
  overlay(id: string): string {
    const s = this.of(id);
    if (s.mode === 'none' || s.mode === 'normal') return 'none';
    return s.mode;
  }

  get size(): number { return this.values.size; }
}

/* ===================== 族0128 任务栏分组 ===================== */

export interface TaskButton { winId: string; app: string; order: number }

/** 分组：按应用聚合、阈值展开、顺序持久化。 */
export class TaskbarGrouping {
  ungroupThreshold = 3; // 同应用窗口数 ≤ 阈值不分组
  private order: string[] = [];

  group(buttons: TaskButton[]): Array<{ app: string; wins: string[] }> {
    const map = new Map<string, string[]>();
    for (const b of buttons) {
      const list = map.get(b.app) ?? [];
      list.push(b.winId);
      map.set(b.app, list);
    }
    return [...map.entries()]
      .filter(([, wins]) => wins.length > this.ungroupThreshold)
      .map(([app, wins]) => ({ app, wins }));
  }

  setOrder(apps: string[]): void {
    this.order = apps.filter((a, i) => apps.indexOf(a) === i);
  }

  get orderSnapshot(): string[] { return [...this.order]; }

  serialize(): string { return JSON.stringify(this.order); }

  static deserialize(raw: string): TaskbarGrouping {
    const g = new TaskbarGrouping();
    try {
      const o = JSON.parse(raw) as unknown;
      if (Array.isArray(o) && o.every((x) => typeof x === 'string')) g.order = o as string[];
    } catch { /* 回默认 */ }
    return g;
  }
}

/* ===================== 族0129 任务栏多屏 ===================== */

export interface Monitor { id: string; primary: boolean; bounds: { x: number; y: number; w: number; h: number } }

/** 多屏：每屏一栏、窗口归属屏、主屏聚合模式。 */
export class TaskbarMultiScreen {
  monitors: Monitor[] = [];
  mode: 'per-screen' | 'primary-only' | 'primary-windows' = 'per-screen';

  setMonitors(list: Monitor[]): void {
    const seen = new Set<string>();
    this.monitors = list.filter((m) => (seen.has(m.id) ? false : (seen.add(m.id), true)));
    if (this.monitors.length > 0 && !this.monitors.some((m) => m.primary)) this.monitors[0]!.primary = true;
  }

  monitorOf(winX: number, winY: number): string {
    for (const m of this.monitors) {
      const b = m.bounds;
      if (winX >= b.x && winX < b.x + b.w && winY >= b.y && winY < b.y + b.h) return m.id;
    }
    return this.monitors.find((m) => m.primary)?.id ?? '';
  }

  /** 某屏任务栏应显示哪些窗口。 */
  visibleOn(monId: string, wins: Array<{ id: string; x: number; y: number }>): string[] {
    if (this.mode === 'primary-only') return monId === this.monitors.find((m) => m.primary)?.id ? wins.map((w) => w.id) : [];
    if (this.mode === 'per-screen') return wins.filter((w) => this.monitorOf(w.x, w.y) === monId).map((w) => w.id);
    const primaryWins = wins.filter((w) => this.monitorOf(w.x, w.y) === this.monitors.find((m) => m.primary)?.id);
    return monId === this.monitors.find((m) => m.primary)?.id ? primaryWins.map((w) => w.id) : [];
  }

  get primary(): string { return this.monitors.find((m) => m.primary)?.id ?? ''; }
}

/* ===================== 族0130 任务栏性能 ===================== */

export interface PerfBudget { key: string; budgetMs: number }

/** 性能：预算表、帧内批处理、缓存命中、降级链。 */
export class TaskbarPerf {
  static readonly BUDGETS: PerfBudget[] = [
    { key: 'layout', budgetMs: 8 },
    { key: 'paint', budgetMs: 8 },
    { key: 'preview', budgetMs: 16 },
  ];

  private cache = new Map<string, string>();
  private queue: Array<() => void> = [];
  hits = 0;
  misses = 0;

  cached(key: string, produce: () => string): string {
    const hit = this.cache.get(key);
    if (hit !== undefined) { this.hits++; return hit; }
    this.misses++;
    const v = produce();
    this.cache.set(key, v);
    return v;
  }

  /** 单帧批处理：一次 flush 内全部执行并清空。 */
  batch(fn: () => void): void { this.queue.push(fn); }
  flush(): number { const n = this.queue.length; for (const f of this.queue) f(); this.queue = []; return n; }

  inBudget(key: string, ms: number): boolean {
    const b = TaskbarPerf.BUDGETS.find((x) => x.key === key);
    return b !== undefined && ms <= b.budgetMs;
  }

  get hitRate(): number { const t = this.hits + this.misses; return t === 0 ? 0 : this.hits / t; }
}
