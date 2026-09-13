// UNREAL-X AI-54：领域15 族0531~0540「kit 系统件与系统范式」真实实现（X13251~X13500 口径）。
// 纯逻辑模型：kit 数据展示件 / 系统★件 / 材质五档 / 数值规范令牌 / 设置 11 页 /
// 开始菜单与任务栏 / 快捷面板与通知中心 / 文件管理器与桌面 / 动效编排 / 手势语言。
// 样式令牌口径见 docs/UI-品质深化 §11/§12/§13/§15。

export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

const clamp = (v: number, lo: number, hi: number) => (v < lo ? lo : v > hi ? hi : v);

/* ================= 族0531 kit 数据展示件（§13 #20~26） ================= */

export function avatarInitialsX(name: string): string {
  const cjk = [...name].filter((c) => /[\u4e00-\u9fff]/.test(c));
  if (cjk.length >= 2) return cjk.slice(-2).join('');
  const parts = name.trim().split(/\s+/);
  if (parts.length >= 2) return (parts[0]![0]! + parts[1]![0]!).toUpperCase();
  return [...name.trim()].slice(0, 2).join('').toUpperCase();
}

export type AvatarStatus = 'online' | 'away' | 'busy' | 'offline';

export class AvatarModelX {
  src: string | null;
  fallback: string;
  status: AvatarStatus = 'online';

  constructor(src: string | null, fallback: string) {
    this.src = src;
    this.fallback = fallback;
  }

  /** 字/图两态 + 状态点。 */
  showsImage(): boolean {
    return this.src !== null && this.src.length > 0;
  }

  label(): string {
    return this.showsImage() ? this.fallback : avatarInitialsX(this.fallback);
  }
}

export function breadcrumbX(items: string[]): string[] {
  if (items.length <= 3) return [...items];
  return [items[0]!, '…', items[items.length - 1]!];
}

export class KbdModelX {
  combo: string;

  constructor(combo: string) {
    this.combo = combo;
  }

  /** 键帽拆分：Ctrl+Shift+P → ['Ctrl','Shift','P']。 */
  keys(): string[] {
    return this.combo.split('+');
  }

  /** 键帽规格：h22 圆角 4（§14.1）。 */
  dims(): { h: number; radius: number } {
    return { h: 22, radius: 4 };
  }
}

/* ================= 族0532 kit 导航与系统★件（§13 ★件） ================= */

export interface NavItem {
  id: string;
  label: string;
  icon: string;
}

export class NavRailModel {
  items: NavItem[];
  active = 0;

  constructor(items: NavItem[]) {
    this.items = items;
  }

  isActive(i: number): boolean {
    return this.active === i;
  }

  /** 选中 = 整行 --bg-raised + 左 3px accent 条；导航行 h44。 */
  rowHeight(): number {
    return 44;
  }

  selectionSpec(): { bg: string; bar: string; barWidth: number } {
    return { bg: 'var(--bg-raised)', bar: 'var(--accent)', barWidth: 3 };
  }

  keys(key: string): number {
    if (key === 'ArrowDown') this.active = Math.min(this.active + 1, this.items.length - 1);
    else if (key === 'ArrowUp') this.active = Math.max(this.active - 1, 0);
    return this.active;
  }
}

export class SearchPillModel {
  focused = false;
  query = '';

  focus(): void {
    this.focused = true;
  }

  /** focus 展开 / ESC 收起；role=searchbox。 */
  escape(): boolean {
    if (this.query) {
      this.query = '';
      return true;
    }
    this.focused = false;
    return false;
  }
}

export interface HeroLink {
  id: string;
  label: string;
}

export class HeroCardModel {
  thumbRatio = '16:9';
  height = 96;
  links: HeroLink[] = [];
  overflow: HeroLink[] = [];

  setLinks(links: HeroLink[]): void {
    this.links = links.slice(0, 3);
    this.overflow = links.slice(3);
  }

  /** 右侧最多 3 个服务入口，超出折叠 `…`。 */
  overflowLabel(): string | null {
    return this.overflow.length > 0 ? '…' : null;
  }
}

export class SettingsRowModel {
  title: string;
  desc: string;
  toggleable: boolean;

  constructor(title: string, desc: string, toggleable = false) {
    this.title = title;
    this.desc = desc;
    this.toggleable = toggleable;
  }

  /** 整行按钮语义；行高 ≥72。 */
  role(): string {
    return this.toggleable ? 'row-with-switch' : 'button';
  }

  minHeight(): number {
    return 72;
  }
}

export class TaskbarItemModel {
  running = false;
  active = false;

  /** 运行=短点 / 激活=长条（§11.3）。 */
  indicator(): 'none' | 'short' | 'long' {
    if (this.active && this.running) return 'long';
    if (this.running) return 'short';
    return 'none';
  }
}

export class CommandPaletteModel {
  private recents: string[] = [];

  pick(id: string): void {
    this.recents = [id, ...this.recents.filter((r) => r !== id)].slice(0, 5);
  }

  /** role=combobox + listbox；空态有引导。 */
  roles(): { combo: string; listbox: string; empty: string } {
    return { combo: 'combobox', listbox: 'listbox', empty: '无匹配命令，换个关键词试试' };
  }
}

/* ================= 族0533 材质五档参数（§12.5） ================= */

export type MaterialId = 'm-solid' | 'm-frosted' | 'm-acrylic' | 'm-mica' | 'm-glow';

export interface MaterialLevel {
  blurPx: number;
  saturate: number;
  noiseOpacity: number;
  bgAlpha: number;
}

/** 五档 ×5 级强度（§12.5 数值表，稿即所得）。 */
export const MATERIAL_TABLE: Record<Exclude<MaterialId, 'm-glow' | 'm-solid'>, MaterialLevel[]> = {
  'm-frosted': [1, 2, 3, 4, 5].map((i) => ({
    blurPx: [8, 12, 16, 20, 28][i - 1]!,
    saturate: 1,
    noiseOpacity: 0,
    bgAlpha: 0.55 + 0.05 * (i - 1),
  })),
  'm-acrylic': [1, 2, 3, 4, 5].map((i) => ({
    blurPx: [8, 12, 16, 20, 28][i - 1]!,
    saturate: 1.2,
    noiseOpacity: 0.02 + 0.0075 * (i - 1),
    bgAlpha: 0.6 + 0.05 * (i - 1),
  })),
  'm-mica': [1, 2, 3, 4, 5].map((i) => ({
    blurPx: 0,
    saturate: 1.1,
    noiseOpacity: 0.03,
    bgAlpha: 0.4 + 0.075 * (i - 1),
  })),
};

export class MaterialSystem {
  current: MaterialId = 'm-solid';
  level = 1;

  apply(id: MaterialId, level = 1): MaterialId {
    this.current = id;
    this.level = clamp(level, 1, 5);
    return this.current;
  }

  params(): MaterialLevel | null {
    const row = MATERIAL_TABLE[this.current as keyof typeof MATERIAL_TABLE];
    return row ? row[this.level - 1]! : null;
  }

  /** HC 铁律：全部材质降级 m-solid（描边由 CSS 层负责）。 */
  downgradeHighContrast(): MaterialId {
    this.current = 'm-solid';
    return this.current;
  }

  /** 低性能降级链 acrylic→frosted→solid 两级（§12.5）。 */
  degradeLowPerf(cores: number, saver: boolean): MaterialId {
    if (saver) return this.apply('m-solid');
    if (cores <= 4) {
      if (this.current === 'm-acrylic') return this.apply('m-frosted');
      if (this.current === 'm-frosted' || this.current === 'm-mica') return this.apply('m-solid');
    }
    return this.current;
  }

  /** reduce-motion：辉光动画降静态。 */
  glowStatic(reduceMotion: boolean): boolean {
    return reduceMotion;
  }
}

/* ================= 族0534 数值规范令牌对稿（§12） ================= */

export const TYPE_SCALE_X = {
  pageTitle: { size: 28, lineHeight: 1.3, weight: 600 },
  windowTitle: { size: 13, lineHeight: 1.4, weight: 600 },
  rowTitle: { size: 15, lineHeight: 1.6, weight: 400 },
  rowSecondary: { size: 12, lineHeight: 1.5, weight: 400 },
  menuItem: { size: 13, lineHeight: 1, weight: 400 },
  statusBar: { size: 12, lineHeight: 1, weight: 400 },
} as const;

export const COLOR_SEMANTICS_X = {
  bgCanvas: 'oklch(0.14 0.02 262)',
  bgSurface: 'oklch(0.18 0.02 262 / 0.72)',
  bgRaised: 'oklch(0.20 0.02 262 / 0.92)',
  accent: 'oklch(0.68 0.09 262)',
  accentSoft: 'oklch(0.68 0.09 262 / 0.16)',
} as const;

export const RADII_X = { window: 16, card: 12, control: 8, chip: 4 } as const;

export const ELEVATION_X = ['elev-0', 'elev-1', 'elev-2', 'elev-3', 'elev-4', 'elev-5', 'elev-6'] as const;

export const CONTROL_HEIGHTS_X = { compact: 28, standard: 32, touch: 44 } as const;

/** 相对亮度（简化 WCAG）+ 对比度。 */
function relLum(rgb: [number, number, number]): number {
  const f = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * f(rgb[0]) + 0.7152 * f(rgb[1]) + 0.0722 * f(rgb[2]);
}

export function contrastX(a: [number, number, number], b: [number, number, number]): number {
  const la = relLum(a);
  const lb = relLum(b);
  const hi = Math.max(la, lb);
  const lo = Math.min(la, lb);
  return (hi + 0.05) / (lo + 0.05);
}

/** 正文 ≥4.5:1 红线。 */
export function aaTextOk(fg: [number, number, number], bg: [number, number, number]): boolean {
  return contrastX(fg, bg) >= 4.5;
}

/* ================= 族0535 设置 11 页收敛（§11.1） ================= */

export const SETTINGS_PAGES: readonly NavItem[] = [
  { id: 'home', label: '主页', icon: '房子' },
  { id: 'system', label: '系统', icon: '显示器' },
  { id: 'devices', label: '蓝牙和其他设备', icon: '蓝牙' },
  { id: 'network', label: '网络和 Internet', icon: 'Wi-Fi' },
  { id: 'personalize', label: '个性化', icon: '画笔' },
  { id: 'apps', label: '应用', icon: '方块' },
  { id: 'accounts', label: '账户', icon: '人' },
  { id: 'timeLang', label: '时间和语言', icon: '时钟' },
  { id: 'a11y', label: '辅助功能', icon: '无障碍人形' },
  { id: 'privacy', label: '隐私和安全性', icon: '盾' },
  { id: 'update', label: '系统更新', icon: '循环箭头' },
];

/** 27 Tab → 11 页收敛映射（每 Tab 恰好归位一页）。 */
export const TAB_TO_PAGE: Record<string, string> = {
  SystemCenter: 'system', Snapshot: 'system', StorageRecovery: 'system',
  Network: 'network', Browsers: 'network',
  Vision: 'personalize', Ambience: 'personalize', BootTheater: 'personalize',
  Extensions: 'apps', Marketplace: 'apps', WinFeel: 'apps',
  Accounts: 'accounts', Passwords: 'accounts',
  L10n: 'timeLang', Ime: 'timeLang',
  A11y: 'a11y', InputFeel: 'a11y',
  Security: 'privacy', AuroraD4: 'privacy',
  Quality: 'update', Perf: 'update', CodeDeploy: 'update',
  Hardware: 'devices', Notifications: 'system', Sound: 'system',
  Language: 'timeLang', Privacy2: 'privacy',
};

export class SettingsNavModel {
  active = 0;

  pageOf(tab: string): NavItem | undefined {
    const pid = TAB_TO_PAGE[tab];
    return SETTINGS_PAGES.find((p) => p.id === pid);
  }

  /** 收敛校验：27 个 Tab 全部可达且每页至少承接一个。 */
  coverage(): { mapped: number; uncovered: string[]; emptyPages: string[] } {
    const tabs = Object.keys(TAB_TO_PAGE);
    const uncovered = tabs.filter((t) => !this.pageOf(t));
    const emptyPages = SETTINGS_PAGES.filter((p) => !Object.values(TAB_TO_PAGE).includes(p.id)).map((p) => p.id);
    return { mapped: tabs.length - uncovered.length, uncovered, emptyPages };
  }

  /** 设置搜索：命中行卡标题/副标题 + 页内跳转。 */
  search(rows: { title: string; desc: string; page: string }[], q: string): string[] {
    if (!q) return [];
    return rows.filter((r) => r.title.includes(q) || r.desc.includes(q)).map((r) => r.page);
  }

  pageTitleStyle(): { size: number; weight: number } {
    return { size: TYPE_SCALE_X.pageTitle.size, weight: 600 };
  }
}

/* ================= 族0536 开始菜单与任务栏皮（§11.2/11.3） ================= */

export class StartMenuModel {
  pinned: string[] = [];
  page = 0;
  readonly perPage = 24; // 6 列 ×4 行

  setPinned(ids: string[]): void {
    this.pinned = [...new Set(ids)];
  }

  get pages(): number {
    return Math.max(1, Math.ceil(this.pinned.length / this.perPage));
  }

  pageItems(): string[] {
    return this.pinned.slice(this.page * this.perPage, (this.page + 1) * this.perPage);
  }

  nextPage(): void {
    this.page = (this.page + 1) % this.pages;
  }

  /** 推荐区最近文件 ≤6。 */
  recent(files: string[]): string[] {
    return files.slice(0, 6);
  }

  /** 打开动效：从任务栏锚点生长。 */
  openAnim(): { ease: string; dur: string; origin: string } {
    return { ease: 'var(--ease-emphasized)', dur: 'var(--dur-5)', origin: 'taskbar' };
  }
}

export const TASKBAR_TRAY = ['quickPanel', 'network', 'volume', 'battery', 'clock', 'showDesktop'] as const;

export class TaskbarModel {
  private items = new Map<string, TaskbarItemModel>();

  pin(id: string): void {
    if (!this.items.has(id)) this.items.set(id, new TaskbarItemModel());
  }

  setRunning(id: string, running: boolean, active = false): void {
    const it = this.items.get(id);
    if (it) {
      it.running = running;
      it.active = active && running;
    }
  }

  /** 居中图标组（Win11 布局）。 */
  centered(): string[] {
    return [...this.items.keys()];
  }

  indicatorOf(id: string): string {
    return this.items.get(id)?.indicator() ?? 'none';
  }
}

/* ================= 族0537 快捷面板与通知中心（§11.4） ================= */

export const QUICK_TILES = ['wifi', 'bluetooth', 'airplane', 'saver', 'dnd', 'project'] as const;

export class QuickPanelModel {
  tiles: Record<string, boolean> = {};
  brightness = 70;
  volume = 40;
  battery = 86;

  toggle(tile: (typeof QUICK_TILES)[number]): boolean {
    this.tiles[tile] = !this.tiles[tile];
    return this.tiles[tile]!;
  }

  setBrightness(v: number): number {
    this.brightness = clamp(Math.round(v), 0, 100);
    return this.brightness;
  }

  setVolume(v: number): number {
    this.volume = clamp(Math.round(v), 0, 100);
    return this.volume;
  }
}

export interface Notice {
  id: string;
  app: string;
  title: string;
  body: string;
}

export class NotificationCenterModel {
  private items: Notice[] = [];
  dnd = false;
  suppressedByDnd = 0;

  push(n: Notice): boolean {
    if (this.dnd) {
      this.suppressedByDnd += 1;
      return false;
    }
    this.items.push(n);
    return true;
  }

  /** 按应用分组。 */
  groups(): { app: string; notices: Notice[] }[] {
    const m = new Map<string, Notice[]>();
    for (const n of this.items) {
      const arr = m.get(n.app) ?? [];
      arr.push(n);
      m.set(n.app, arr);
    }
    return [...m.entries()].map(([app, notices]) => ({ app, notices }));
  }

  clearAll(): number {
    const n = this.items.length;
    this.items = [];
    return n;
  }

  /** 下半月历（占位天数 = 当月天数的一半，向上取整，供日历算法复用）。 */
  secondHalfDays(daysInMonth: number): number {
    return Math.ceil(daysInMonth / 2);
  }
}

/* ================= 族0538 文件管理器与桌面层（§11.5/11.6） ================= */

export type ExplorerView = 'largeIcons' | 'list' | 'details';
export type SortKey = 'name' | 'size' | 'date';

export class ExplorerToolbarModel {
  view: ExplorerView = 'details';
  sortKey: SortKey = 'name';
  asc = true;

  setView(v: ExplorerView): void {
    this.view = v;
  }

  sortBy(k: SortKey): void {
    if (this.sortKey === k) this.asc = !this.asc;
    else {
      this.sortKey = k;
      this.asc = true;
    }
  }

  /** 工具栏动作表：新建/剪切/复制/粘贴/重命名/删除 + 排序/查看/…。 */
  static actions(): string[] {
    return ['new', 'cut', 'copy', 'paste', 'rename', 'delete', 'sort', 'view', 'more'];
  }
}

export class ExplorerAddressBar {
  segments: string[] = [];

  enter(parts: string[]): void {
    this.segments = parts;
  }

  /** 每段可下拉。 */
  dropdown(i: number): string | null {
    return this.segments[i] ?? null;
  }

  breadcrumb(): string[] {
    return breadcrumbX(this.segments);
  }
}

export class ExplorerStatusModel {
  selected = 0;
  usedGb = 128;
  totalGb = 512;

  capacityRatio(): number {
    return this.totalGb <= 0 ? 0 : clamp(this.usedGb / this.totalGb, 0, 1);
  }

  text(): string {
    return `已选 ${this.selected} 项`;
  }
}

/** 桌面右键菜单分组（组间 Divider）。 */
export const DESKTOP_CONTEXT_GROUPS: readonly string[][] = [
  ['查看', '排序方式', '刷新'],
  ['新建'],
  ['显示设置', '个性化'],
];

export function contextMenuFlat(): (string | '---')[] {
  const out: (string | '---')[] = [];
  DESKTOP_CONTEXT_GROUPS.forEach((g, i) => {
    if (i > 0) out.push('---');
    out.push(...g);
  });
  return out;
}

/* ================= 族0539 动效编排表（§15.1 七场景） ================= */

export interface MotionScene {
  id: string;
  dur: string;
  ease: string;
  orchestration: string;
}

export const MOTION_SCENES: readonly MotionScene[] = [
  { id: 'hover', dur: '--dur-2', ease: '--ease-standard', orchestration: '无延迟；离开 80ms' },
  { id: 'menu', dur: '--dur-3', ease: '--ease-standard', orchestration: '透明度+scale .98→1，origin 顶' },
  { id: 'overlay', dur: '--dur-5', ease: '--ease-emphasized', orchestration: 'scale .96+8px 位移，任务栏锚点' },
  { id: 'notice', dur: '--dur-4', ease: '--ease-spring', orchestration: '右缘入，5s 消散' },
  { id: 'page', dur: '--dur-3', ease: '--ease-standard', orchestration: '旧页 -8px 淡出，新页 +8px 淡入' },
  { id: 'skeletonSwap', dur: '--dur-3', ease: '--ease-standard', orchestration: '交叉溶解，禁跳变' },
  { id: 'reduceMotion', dur: '--dur-1', ease: 'linear', orchestration: '全部位移→纯淡入淡出' },
];

export function motionOf(id: string): MotionScene | undefined {
  return MOTION_SCENES.find((m) => m.id === id);
}

export function reduceMotionFallback(): MotionScene {
  return MOTION_SCENES[MOTION_SCENES.length - 1]!;
}

/* ================= 族0540 手势语言（§15.2） ================= */

export interface Gesture {
  id: string;
  input: 'touch' | 'trackpad';
  action: string;
}

export const GESTURES: readonly Gesture[] = [
  { id: 'twoFingerScroll', input: 'trackpad', action: '内容滚动' },
  { id: 'pinchZoom', input: 'trackpad', action: '图库/画布缩放' },
  { id: 'threeFingerSwipe', input: 'trackpad', action: '虚拟桌面切换' },
  { id: 'edgeRightSwipe', input: 'touch', action: '通知中心' },
  { id: 'edgeLeftSwipe', input: 'touch', action: '小组件' },
];

/** 触控命中 44px 红线；compact 密度例外放宽至 40px 并记录豁免。 */
export function touchTarget(density: 'compact' | 'comfortable'): { min: number; exemption: boolean } {
  return density === 'compact' ? { min: 40, exemption: true } : { min: 44, exemption: false };
}
