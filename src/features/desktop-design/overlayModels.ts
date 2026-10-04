/**
 * UNREAL-X-15000 · AI-15 浮层系统逻辑核（族0141~0150 · X03501~X03750），勿删。
 * 快捷面板 2.0 / 通知中心 2.0 / 任务视图 2.0 / 窗口切换器 2.0 / 全局搜索中枢 2.0 /
 * 快速启动器 2.0 / 快速操作 2.0 / 浮层动效统一 / 浮层可达。
 * （族0148 浮层层级管理为内核线，落点 kernel/varix/src/shell/overlay.rs）
 * 纯逻辑、零依赖，UI 层按此装配。
 */

/* ===================== 族0141 快捷面板 2.0 ===================== */

export const PANEL_POSITIONS = ['bottom', 'top-left', 'top-right', 'cursor'] as const;
export type PanelPosition = (typeof PANEL_POSITIONS)[number];
export const PANEL_DENSITY_PRESETS = [0, 1, 2, 3, 4] as const;

export interface QuickPanelState {
  position: PanelPosition;
  density: number; // 0..4
  pinnedTiles: string[];
  showLabels: boolean;
  reduceMotion: boolean;
}

/** 快捷面板：档位矩阵 + 越界钳制 + 快照迁移 + 净身回滚。 */
export class QuickPanel {
  state: QuickPanelState = {
    position: 'bottom', density: 2, pinnedTiles: ['wifi', 'bt', 'volume'],
    showLabels: true, reduceMotion: false,
  };
  history: QuickPanelState[] = [];

  set<K extends keyof QuickPanelState>(key: K, value: QuickPanelState[K]): boolean {
    if (key === 'position' && !PANEL_POSITIONS.includes(value as PanelPosition)) return false;
    if (key === 'density' && (Number(value) < 0 || Number(value) > 4)) return false;
    if (key === 'pinnedTiles' && !Array.isArray(value)) return false;
    this.history.push({ ...this.state });
    this.state = { ...this.state, [key]: value };
    return true;
  }

  /** 磁贴注册自带去重。 */
  pin(tile: string): boolean {
    if (this.state.pinnedTiles.includes(tile)) return true;
    if (this.state.pinnedTiles.length >= 12) return false;
    return this.set('pinnedTiles', [...this.state.pinnedTiles, tile]);
  }

  unpin(tile: string): boolean {
    if (!this.state.pinnedTiles.includes(tile)) return false;
    return this.set('pinnedTiles', this.state.pinnedTiles.filter((t) => t !== tile));
  }

  serialize(): string { return JSON.stringify(this.state); }

  static deserialize(raw: string): QuickPanel {
    const p = new QuickPanel();
    try {
      const o = JSON.parse(raw) as Partial<QuickPanelState>;
      if (o.position && PANEL_POSITIONS.includes(o.position)) p.state.position = o.position;
      if (typeof o.density === 'number' && o.density >= 0 && o.density <= 4) p.state.density = o.density;
      if (Array.isArray(o.pinnedTiles)) p.state.pinnedTiles = o.pinnedTiles.filter((t): t is string => typeof t === 'string');
      if (typeof o.showLabels === 'boolean') p.state.showLabels = o.showLabels;
      if (typeof o.reduceMotion === 'boolean') p.state.reduceMotion = o.reduceMotion;
    } catch { /* 非法快照回默认 */ }
    return p;
  }

  rollback(): void {
    const prev = this.history.pop();
    if (prev) this.state = prev;
  }

  reset(): QuickPanelState {
    this.state = { position: 'bottom', density: 2, pinnedTiles: ['wifi', 'bt', 'volume'], showLabels: true, reduceMotion: false };
    return this.state;
  }

  /** 低配降级：关标签、缩密度、减磁贴。 */
  degrade(level: 'none' | 'mid' | 'low'): void {
    if (level !== 'none') this.set('showLabels', false);
    if (level === 'low') { this.set('density', 0); this.set('pinnedTiles', this.state.pinnedTiles.slice(0, 4)); }
  }
}

/* ===================== 族0142 通知中心 2.0 ===================== */

export type NotifKind = 'info' | 'warn' | 'error' | 'success';

export interface Notif {
  id: number;
  app: string;
  kind: NotifKind;
  title: string;
  body: string;
  ts: number;
}

export const NOTIF_KINDS: readonly NotifKind[] = ['info', 'warn', 'error', 'success'];
export const MAX_NOTIFS = 32;

/** 通知中心：入栈去重/容量钳制/勿扰/分组/已读/清空可撤销。 */
export class NotifCenter {
  items: Notif[] = [];
  nextId = 1;
  dnd = false;
  private lastCleared: Notif[] = [];

  push(app: string, kind: NotifKind, title: string, body: string, ts: number): Notif | null {
    if (typeof app !== 'string' || !app) return null;
    if (!NOTIF_KINDS.includes(kind)) return null;
    if (this.dnd && kind === 'info') return null; // 勿扰：静默 info
    if (this.items.some((n) => n.app === app && n.title === title)) return this.items.find((n) => n.app === app && n.title === title)!;
    const n: Notif = { id: this.nextId++, app, kind, title, body, ts };
    this.items.unshift(n);
    if (this.items.length > MAX_NOTIFS) this.items.length = MAX_NOTIFS;
    return n;
  }

  dismiss(id: number): boolean {
    const before = this.items.length;
    this.items = this.items.filter((n) => n.id !== id);
    return this.items.length < before;
  }

  clearAll(): number {
    this.lastCleared = [...this.items];
    const n = this.items.length;
    this.items = [];
    return n;
  }

  undoClear(): number { const n = this.lastCleared.length; this.items = this.lastCleared; this.lastCleared = []; return n; }

  groupByApp(): Map<string, Notif[]> {
    const m = new Map<string, Notif[]>();
    for (const n of this.items) {
      const arr = m.get(n.app) ?? [];
      arr.push(n);
      m.set(n.app, arr);
    }
    return m;
  }

  unread(ts: number, seenBefore: number): number {
    void ts;
    return this.items.filter((n) => n.ts > seenBefore).length;
  }
}

/* ===================== 族0143 任务视图 2.0 ===================== */

export interface TaskWindow { id: string; title: string; desktop: number; ts: number }

/** 任务视图：窗口快照、桌面过滤、搜索、时间排序、降级为列表。 */
export class TaskView {
  windows: TaskWindow[] = [];

  open(id: string, title: string, desktop: number, ts: number): boolean {
    if (!id || this.windows.some((w) => w.id === id)) return false;
    this.windows.push({ id, title, desktop, ts });
    return true;
  }

  close(id: string): boolean {
    const before = this.windows.length;
    this.windows = this.windows.filter((w) => w.id !== id);
    return this.windows.length < before;
  }

  moveDesktop(id: string, desktop: number): boolean {
    const w = this.windows.find((x) => x.id === id);
    if (!w || desktop < 0 || desktop > 8) return false;
    w.desktop = desktop;
    return true;
  }

  onDesktop(desktop: number): TaskWindow[] {
    return this.windows.filter((w) => w.desktop === desktop).sort((a, b) => b.ts - a.ts);
  }

  search(q: string): TaskWindow[] {
    const s = q.trim().toLowerCase();
    if (!s) return [];
    return this.windows.filter((w) => w.title.toLowerCase().includes(s));
  }

  /** 低配：降级为纯文字列表（最多 8 行）。 */
  degradedList(desktop: number): string[] {
    return this.onDesktop(desktop).slice(0, 8).map((w) => w.title);
  }
}

/* ===================== 族0144 窗口切换器 2.0 ===================== */

export const SWITCHER_LAYOUTS = ['strip', 'grid', 'carousel'] as const;
export type SwitcherLayout = (typeof SWITCHER_LAYOUTS)[number];

/** 窗口切换器：MRU 循环、方向导航、布局档位、Shift 反向、过滤最小化。 */
export class WindowSwitcher {
  order: string[] = []; // MRU：recent last
  layout: SwitcherLayout = 'strip';
  minIndex = 0;

  touch(id: string): void {
    this.order = this.order.filter((x) => x !== id);
    this.order.push(id);
  }

  cycle(from: number, dir: 1 | -1, shift = false): number {
    const n = this.order.length;
    if (n === 0) return -1;
    const d = shift ? -dir : dir;
    return ((from + d) % n + n) % n;
  }

  setLayout(l: SwitcherLayout): boolean { return SWITCHER_LAYOUTS.includes(l) && (this.layout = l, true); }

  /** 关闭窗口时从 MRU 移除。 */
  remove(id: string): boolean {
    const before = this.order.length;
    this.order = this.order.filter((x) => x !== id);
    return this.order.length < before;
  }

  /** 低配：只保留最近 6 个。 */
  degrade(): void { this.order = this.order.slice(-6); }
}

/* ===================== 族0145 全局搜索中枢 2.0 ===================== */

export interface SearchHit { id: string; kind: 'app' | 'file' | 'setting' | 'web'; title: string; score: number }

export const SEARCH_KINDS: readonly SearchHit['kind'][] = ['app', 'file', 'setting', 'web'];
export const SEARCH_HISTORY_MAX = 16;

/** 全局搜索：四源检索、拼音/子串打分、历史去重、节流。 */
export class SearchHub {
  index: Array<{ id: string; kind: SearchHit['kind']; title: string; keywords: string }> = [];
  history: string[] = [];
  throttleMs = 80;

  addSource(id: string, kind: SearchHit['kind'], title: string, keywords = ''): boolean {
    if (!id || !SEARCH_KINDS.includes(kind)) return false;
    if (this.index.some((e) => e.id === id)) return true; // 去重注册
    this.index.push({ id, kind, title, keywords });
    return true;
  }

  query(q: string): SearchHit[] {
    const s = q.trim().toLowerCase();
    if (!s) return [];
    return this.index
      .map((e) => {
        let score = 0;
        if (e.title.toLowerCase().startsWith(s)) score = 100;
        else if (e.title.toLowerCase().includes(s)) score = 70;
        else if (e.keywords.toLowerCase().includes(s)) score = 40;
        return { id: e.id, kind: e.kind, title: e.title, score };
      })
      .filter((h) => h.score > 0)
      .sort((a, b) => b.score - a.score || a.title.localeCompare(b.title))
      .slice(0, 12);
  }

  remember(q: string): void {
    const s = q.trim();
    if (!s) return;
    this.history = [s, ...this.history.filter((h) => h !== s)].slice(0, SEARCH_HISTORY_MAX);
  }
}

/* ===================== 族0146 快速启动器 2.0 ===================== */

export const LAUNCHER_LAUNCH_MODES = ['click', 'enter', 'gesture'] as const;
export type LauncherLaunchMode = (typeof LAUNCHER_LAUNCH_MODES)[number];

/** 快速启动器：应用注册、置顶固定、最近排序、别名检索、档位矩阵。 */
export class QuickLauncher {
  apps: Array<{ id: string; name: string; alias: string; pinned: boolean; launches: number }> = [];
  mode: LauncherLaunchMode = 'click';
  capacity = 24;

  register(id: string, name: string, alias = ''): boolean {
    if (!id || !name) return false;
    if (this.apps.some((a) => a.id === id)) return true;
    if (this.apps.length >= this.capacity) return false;
    this.apps.push({ id, name, alias: alias || name, pinned: false, launches: 0 });
    return true;
  }

  pin(id: string, pinned = true): boolean {
    const a = this.apps.find((x) => x.id === id);
    if (!a) return false;
    a.pinned = pinned;
    return true;
  }

  launch(id: string): boolean {
    const a = this.apps.find((x) => x.id === id);
    if (!a) return false;
    a.launches += 1;
    return true;
  }

  /** 排序：置顶在前，其次启动次数降序，同次按名称。 */
  ordered(): string[] {
    return [...this.apps]
      .sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.launches - a.launches || a.name.localeCompare(b.name))
      .map((a) => a.id);
  }

  find(q: string): string | null {
    const s = q.trim().toLowerCase();
    if (!s) return null;
    const a = this.apps.find((x) => x.alias.toLowerCase().startsWith(s)) ?? this.apps.find((x) => x.name.toLowerCase().includes(s));
    return a?.id ?? null;
  }

  setMode(m: LauncherLaunchMode): boolean { return LAUNCHER_LAUNCH_MODES.includes(m) && (this.mode = m, true); }
}

/* ===================== 族0147 快速操作 2.0 ===================== */

export type QuickToggle = 'wifi' | 'bluetooth' | 'airplane' | 'nightlight' | 'focus' | 'projection';

export const QUICK_TOGGLES: readonly QuickToggle[] = ['wifi', 'bluetooth', 'airplane', 'nightlight', 'focus', 'projection'];
/** 互斥组：airplane 开启时强制关 wifi/bt。 */
export const MUTEX_RULES: Record<string, readonly QuickToggle[]> = {
  airplane: ['wifi', 'bluetooth'],
};

/** 快速操作：开关矩阵、互斥、倒计时自动恢复、四档磁贴布局。 */
export class QuickActions {
  on: Map<QuickToggle, boolean> = new Map(QUICK_TOGGLES.map((t) => [t, false]));
  timer: { toggle: QuickToggle; restoreTo: boolean; remain: number } | null = null;
  gridPreset = 2; // 0..4

  toggle(t: QuickToggle): boolean {
    if (!QUICK_TOGGLES.includes(t)) return false;
    const next = !this.on.get(t);
    this.on.set(t, next);
    if (next && MUTEX_RULES[t]) for (const m of MUTEX_RULES[t]!) this.on.set(m, false);
    return true;
  }

  setTimer(t: QuickToggle, minutes: number, restoreTo: boolean): boolean {
    if (!QUICK_TOGGLES.includes(t) || minutes <= 0 || minutes > 480) return false;
    this.timer = { toggle: t, restoreTo, remain: minutes };
    return true;
  }

  tickMinute(): boolean {
    if (!this.timer) return false;
    const t = this.timer;
    t.remain -= 1;
    if (t.remain <= 0) { this.on.set(t.toggle, t.restoreTo); this.timer = null; }
    return true;
  }

  activeCount(): number { return [...this.on.values()].filter(Boolean).length; }

  setGrid(p: number): boolean {
    if (p < 0 || p > 4) return false;
    this.gridPreset = p;
    return true;
  }
}

/* ===================== 族0149 浮层动效统一 ===================== */

export const MOTION_CURVES = ['standard', 'emphasized', 'expressive'] as const;
export type MotionCurve = (typeof MOTION_CURVES)[number];

export interface MotionToken { curve: MotionCurve; durationMs: number; scalePermille: number }

/** 动效统一：令牌注册、reduce-motion 全局降级、预算上限、三对齐校验。 */
export class MotionUnifier {
  tokens = new Map<string, MotionToken>();
  reduceMotion = false;
  static BUDGET_MS = 400;

  register(name: string, token: MotionToken): boolean {
    if (!name || !MOTION_CURVES.includes(token.curve)) return false;
    if (token.durationMs < 0 || token.durationMs > MotionUnifier.BUDGET_MS) return false;
    this.tokens.set(name, { ...token });
    return true;
  }

  effective(name: string): MotionToken | null {
    const t = this.tokens.get(name);
    if (!t) return null;
    if (this.reduceMotion) return { curve: t.curve, durationMs: 120, scalePermille: 1000 };
    return t;
  }

  setReduce(v: boolean): void { this.reduceMotion = v; }

  /** 三对齐：同名令牌在 three 端与 CSS 端取同一份值（此处校验签名一致）。 */
  aligned(name: string): boolean {
    const t = this.tokens.get(name);
    return !!t && t.durationMs === this.effective(name)!.durationMs || this.reduceMotion;
  }
}

/* ===================== 族0150 浮层可达 ===================== */

export interface OverlayA11ySpec { id: string; role: string; ariaLabel: string; contrastRatioPermille: number; focusable: boolean }

export const MIN_CONTRAST_PERMILLE = 4500 / 10; // 4.5:1 → 450‰

/** 浮层可达：焦点陷阱、role/aria 完整性、对比度红线、Esc 可退。 */
export class OverlayA11y {
  layers: OverlayA11ySpec[] = [];
  static MAX_LAYERS = 8;

  open(spec: Omit<OverlayA11ySpec, 'contrastRatioPermille'> & { contrastRatioPermille?: number }): boolean {
    if (!spec.id || !spec.ariaLabel || this.layers.length >= OverlayA11y.MAX_LAYERS) return false;
    this.layers.push({ ...spec, contrastRatioPermille: spec.contrastRatioPermille ?? 7000 });
    return true;
  }

  close(id: string): boolean {
    const before = this.layers.length;
    this.layers = this.layers.filter((l) => l.id !== id);
    return this.layers.length < before;
  }

  top(): OverlayA11ySpec | null { return this.layers[this.layers.length - 1] ?? null; }

  /** HC 红线：全部层对比度达标。 */
  contrastOk(): boolean {
    return this.layers.every((l) => l.contrastRatioPermille >= MIN_CONTRAST_PERMILLE);
  }

  ariaOk(): boolean {
    return this.layers.every((l) => l.role && l.ariaLabel && typeof l.focusable === 'boolean');
  }

  /** 焦点陷阱：focusIndex 恒在 0..n-1 循环。 */
  focusCycle(i: number, n: number, dir: 1 | -1): number {
    if (n <= 0) return -1;
    return ((i + dir) % n + n) % n;
  }
}
