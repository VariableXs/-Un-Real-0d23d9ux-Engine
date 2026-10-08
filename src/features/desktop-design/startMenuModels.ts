/**
 * UNREAL-X-15000 · AI-14 开始菜单逻辑核（族0131~0140 · X03251~X03500），勿删。
 * 结构 2.0 / 磁贴生态 2.0 / 搜索 2.0 / 个性 2.0 / 行为 2.0 / 推荐引擎 /
 * 应用目录学 / 动效语法 / 可达性 / 性能。纯逻辑、零依赖，UI 层按此装配。
 */

/* ===================== 族0131 开始菜单结构 2.0 ===================== */

export interface MenuRegion { id: string; visible: boolean; order: number }

/** 开始菜单结构：搜索/固定/推荐/电源四区编排 + 栏目栅格。 */
export class StartMenuStructure {
  private regions: MenuRegion[] = [
    { id: 'search', visible: true, order: 0 },
    { id: 'pinned', visible: true, order: 1 },
    { id: 'recommended', visible: true, order: 2 },
    { id: 'power', visible: true, order: 3 },
  ];
  pinnedColumns = 6;
  pinnedRows = 3;

  toggle(id: string): boolean {
    const r = this.regions.find((x) => x.id === id);
    if (!r) return false;
    r.visible = !r.visible;
    return true;
  }

  move(id: string, to: number): boolean {
    const i = this.regions.findIndex((r) => r.id === id);
    if (i < 0 || to < 0 || to >= this.regions.length) return false;
    const [r] = this.regions.splice(i, 1);
    this.regions.splice(to, 0, r!);
    return true;
  }

  order(): string[] { return this.regions.map((r) => r.id); }

  visibleIds(): string[] { return this.regions.filter((r) => r.visible).map((r) => r.id); }

  get pinnedCapacity(): number { return this.pinnedColumns * this.pinnedRows; }
  get regionsSnapshot(): MenuRegion[] { return this.regions.map((r) => ({ ...r })); }
}

/* ===================== 族0132 磁贴生态 2.0 ===================== */

export type TileSize = 'small' | 'medium' | 'wide';

export interface Tile {
  id: string;
  size: TileSize;
  group: string;
  live: boolean;
  badge: number;
}

/** 磁贴生态：尺寸三档、分组、活动磁贴刷新、拖拽重排。 */
export class TilesEcosystem {
  private tiles: Tile[] = [];
  groups: string[] = [];
  maxBadge = 999;

  add(id: string, size: TileSize = 'medium', group = '默认'): boolean {
    if (this.tiles.some((t) => t.id === id)) return false;
    if (!this.groups.includes(group)) this.groups.push(group);
    this.tiles.push({ id, size, group, live: false, badge: 0 });
    return true;
  }

  resize(id: string, size: TileSize): boolean {
    const t = this.tiles.find((x) => x.id === id);
    if (!t) return false;
    t.size = size;
    return true;
  }

  setLive(id: string, live: boolean): boolean {
    const t = this.tiles.find((x) => x.id === id);
    if (!t) return false;
    t.live = live;
    return true;
  }

  setBadge(id: string, n: number): boolean {
    const t = this.tiles.find((x) => x.id === id);
    if (!t) return false;
    t.badge = Math.max(0, Math.min(n, this.maxBadge));
    return true;
  }

  /** 拖拽重排：跨组移动自动改 group。 */
  move(id: string, toIndex: number, toGroup?: string): boolean {
    const i = this.tiles.findIndex((t) => t.id === id);
    if (i < 0 || toIndex < 0 || toIndex > this.tiles.length) return false;
    const [t] = this.tiles.splice(i, 1);
    this.tiles.splice(toIndex, 0, t!);
    if (toGroup !== undefined) {
      if (!this.groups.includes(toGroup)) this.groups.push(toGroup);
      t!.group = toGroup;
    }
    return true;
  }

  inGroup(group: string): Tile[] { return this.tiles.filter((t) => t.group === group); }
  get count(): number { return this.tiles.length; }
}

/* ===================== 族0133 开始菜单搜索 2.0 ===================== */

export interface SearchRecord { name: string; kind: 'app' | 'setting' | 'file' | 'web'; hot: number }

/** 搜索：类别加权排序、前缀优先、空查询守卫、节流。 */
export class StartSearch {
  private records: SearchRecord[] = [];
  debounceMs = 120;
  static KIND_WEIGHT: Record<SearchRecord['kind'], number> = { app: 4, setting: 3, file: 2, web: 1 };

  index(records: SearchRecord[]): void { this.records = records; }

  query(q: string): SearchRecord[] {
    if (!q.trim()) return [];
    const lower = q.toLowerCase();
    return this.records
      .filter((r) => r.name.toLowerCase().includes(lower))
      .sort((a, b) => {
        const ap = a.name.toLowerCase().startsWith(lower) ? 1 : 0;
        const bp = b.name.toLowerCase().startsWith(lower) ? 1 : 0;
        if (ap !== bp) return bp - ap;
        const w = StartSearch.KIND_WEIGHT;
        return (w[b.kind] - w[a.kind]) || (b.hot - a.hot);
      })
      .slice(0, 10);
  }

  kinds(q: string): Array<SearchRecord['kind']> {
    const seen = new Set<SearchRecord['kind']>();
    for (const r of this.query(q)) seen.add(r.kind);
    return [...seen];
  }
}

/* ===================== 族0134 开始菜单个性 2.0 ===================== */

export interface MenuAppearance {
  accent: string;
  density: 'compact' | 'comfortable';
  showRecentlyAdded: boolean;
  showMostUsed: boolean;
}

/** 个性：外观参数矩阵 + 持久化 + 非法回默认。 */
export class StartPersonalization {
  state: MenuAppearance = { accent: '#0078d4', density: 'comfortable', showRecentlyAdded: true, showMostUsed: true };
  private history: MenuAppearance[] = [];

  set<K extends keyof MenuAppearance>(key: K, value: MenuAppearance[K]): boolean {
    if (key === 'density' && value !== 'compact' && value !== 'comfortable') return false;
    if (key === 'accent' && (typeof value !== 'string' || !/^#[0-9a-fA-F]{6}$/.test(value))) return false;
    this.history.push({ ...this.state });
    this.state = { ...this.state, [key]: value };
    return true;
  }

  serialize(): string { return JSON.stringify(this.state); }

  static deserialize(raw: string): StartPersonalization {
    const p = new StartPersonalization();
    try {
      const o = JSON.parse(raw) as Partial<MenuAppearance>;
      if (typeof o.accent === 'string' && /^#[0-9a-fA-F]{6}$/.test(o.accent)) p.state.accent = o.accent;
      if (o.density === 'compact' || o.density === 'comfortable') p.state.density = o.density;
      if (typeof o.showRecentlyAdded === 'boolean') p.state.showRecentlyAdded = o.showRecentlyAdded;
      if (typeof o.showMostUsed === 'boolean') p.state.showMostUsed = o.showMostUsed;
    } catch { /* 回默认 */ }
    return p;
  }

  rollback(): void {
    const prev = this.history.pop();
    if (prev) this.state = prev;
  }

  reset(): MenuAppearance {
    this.state = { accent: '#0078d4', density: 'comfortable', showRecentlyAdded: true, showMostUsed: true };
    return this.state;
  }
}

/* ===================== 族0135 开始菜单行为 2.0 ===================== */

export type DismissReason = 'outside-click' | 'escape' | 'launch' | 'win-focus';

/** 行为：开合状态机 + 锚点 + 关闭原因记忆。 */
export class StartMenuBehavior {
  open = false;
  private dismissals: DismissReason[] = [];
  static readonly OPEN_TOKENS = { dur: 200, ease: 'standard' } as const;

  toggle(): boolean { this.open = !this.open; return this.open; }
  dismiss(reason: DismissReason): boolean {
    if (!this.open) return false;
    this.open = false;
    this.dismissals.push(reason);
    return true;
  }
  get dismissalLog(): DismissReason[] { return [...this.dismissals]; }
  /** 打开时焦点落在搜索框（键盘可达入口）。 */
  initialFocus(): string { return this.open ? 'search' : ''; }
  anchorEdge(edge: 'bottom' | 'top' | 'left' | 'right'): string { return `menu-anchor-${edge}`; }
}

/* ===================== 族0136 推荐引擎 ===================== */

export interface AppEvent { app: string; at: number }

/** 推荐引擎：频次 × 时近衰减打分，可解释、可拒绝。 */
export class RecommendationEngine {
  private events: AppEvent[] = [];
  private rejected = new Set<string>();
  halfLifeMs = 86_400_000; // 1 天半衰
  now = 1_700_000_000_000;

  record(app: string, at: number): void { this.events.push({ app, at }); }
  reject(app: string): void { this.rejected.add(app); }
  isRejected(app: string): boolean { return this.rejected.has(app); }

  score(app: string): number {
    if (this.rejected.has(app)) return 0;
    let s = 0;
    for (const e of this.events) {
      if (e.app !== app) continue;
      const age = Math.max(0, this.now - e.at);
      s += Math.pow(0.5, age / this.halfLifeMs);
    }
    return Math.round(s * 1000) / 1000;
  }

  top(n: number): Array<{ app: string; score: number }> {
    const apps = new Set(this.events.map((e) => e.app));
    return [...apps]
      .map((a) => ({ app: a, score: this.score(a) }))
      .filter((x) => x.score > 0)
      .sort((a, b) => b.score - a.score)
      .slice(0, n);
  }

  /** 建议可解释：给出最近一次触达时间。 */
  explain(app: string): string {
    const last = this.events.filter((e) => e.app === app).at(-1);
    return last ? `最近使用于 ${last.at}` : '暂无使用记录';
  }
}

/* ===================== 族0137 应用目录学 ===================== */

export interface CatalogApp { id: string; name: string; category: string }

/** 应用目录学：索引去重、分类、A-Z、按类检索。 */
export class AppCatalogography {
  private apps = new Map<string, CatalogApp>();
  static CATEGORIES = ['生产力', '开发', '媒体', '系统', '游戏'] as const;

  register(id: string, name: string, category: string): boolean {
    if (this.apps.has(id) || !AppCatalogography.CATEGORIES.includes(category as never)) return false;
    this.apps.set(id, { id, name, category });
    return true;
  }

  unregister(id: string): boolean { return this.apps.delete(id); }

  byCategory(category: string): CatalogApp[] {
    return [...this.apps.values()].filter((a) => a.category === category);
  }

  /** A-Z 拼音直排（本地 locale 比较即测试口径）。 */
  alphabetical(): CatalogApp[] {
    return [...this.apps.values()].sort((a, b) => a.name.localeCompare(b.name, 'zh-Hans-CN'));
  }

  get size(): number { return this.apps.size; }
}

/* ===================== 族0138 动效语法 ===================== */

/** 动效语法：时长×曲线配对全走既有令牌，reduce-motion 降级。 */
export class MenuMotion {
  static readonly PAIRS: Array<{ name: string; durMs: number; ease: string }> = [
    { name: 'menu-open', durMs: 200, ease: 'standard' },
    { name: 'menu-close', durMs: 150, ease: 'standard' },
    { name: 'tile-press', durMs: 100, ease: 'decelerate' },
    { name: 'page-fade', durMs: 250, ease: 'standard' },
    { name: 'list-stagger', durMs: 50, ease: 'decelerate' },
  ];

  pair(name: string): { durMs: number; ease: string } | undefined {
    return MenuMotion.PAIRS.find((p) => p.name === name);
  }

  /** reduce-motion：全部降级为纯淡入淡出（≤100ms，linear）。 */
  static reduced(p: { durMs: number; ease: string }): { durMs: number; ease: string } {
    return { durMs: Math.min(p.durMs, 100), ease: 'linear' };
  }

  stagger(count: number, base = 50): number[] {
    return Array.from({ length: Math.max(0, count) }, (_, i) => i * base);
  }
}

/* ===================== 族0139 可达性 ===================== */

/** 可达性：焦点陷阱、aria、对比度守卫。 */
export class MenuA11y {
  private focusables: string[] = [];
  focusIndex = 0;

  setFocusables(list: string[]): void { this.focusables = list.filter((x) => x.length > 0); }

  /** roving：循环导航。 */
  nav(key: 'ArrowDown' | 'ArrowUp'): string {
    if (this.focusables.length === 0) return '';
    if (key === 'ArrowDown') this.focusIndex = (this.focusIndex + 1) % this.focusables.length;
    else this.focusIndex = (this.focusIndex - 1 + this.focusables.length) % this.focusables.length;
    return this.focusables[this.focusIndex]!;
  }

  trapKeys(): string[] { return ['Tab', 'Escape']; }

  static contrast(fg: [number, number, number], bg: [number, number, number]): number {
    const lum = (c: [number, number, number]) => {
      const f = (v: number) => { const s = v / 255; return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4); };
      return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]);
    };
    const l1 = lum(fg);
    const l2 = lum(bg);
    return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
  }

  ariaExpanded(): 'true' | 'false' { return this.focusables.length > 0 ? 'true' : 'false'; }
  get size(): number { return this.focusables.length; }
}

/* ===================== 族0140 性能 ===================== */

/** 性能：预算表 + 惰性渲染 + 索引缓存。 */
export class MenuPerf {
  static readonly BUDGETS = [
    { key: 'open', budgetMs: 100 },
    { key: 'search', budgetMs: 50 },
    { key: 'tile-reflow', budgetMs: 16 },
  ] as const;

  private renderedPages: Record<string, number> = {};
  private indexCache: Map<string, string[]> | null = null;

  inBudget(key: string, ms: number): boolean {
    const b = MenuPerf.BUDGETS.find((x) => x.key === key);
    return b !== undefined && ms <= b.budgetMs;
  }

  /** 惰性渲染：只渲染可见页，页号记录防重渲。 */
  renderPage(page: string, items: number): number {
    const prev = this.renderedPages[page];
    if (prev !== undefined) return prev;
    this.renderedPages[page] = items;
    return items;
  }

  buildIndex(source: string[], key: string): string[] {
    if (this.indexCache === null) this.indexCache = new Map();
    const hit = this.indexCache.get(key);
    if (hit) return hit;
    const idx = source.map((s) => s.toLowerCase());
    this.indexCache.set(key, idx);
    return idx;
  }

  get pages(): number { return Object.keys(this.renderedPages).length; }
}
