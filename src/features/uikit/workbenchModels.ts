// UNREAL-X AI-53：领域15 族0521~0530「工作台范式与 kit 基础」真实实现（X13001~X13250 口径）。
// 纯逻辑模型：工作台五区 / MenuBar / ActivityBar / 编辑区 / Panel / StatusBar /
// 命令注册表 / kit 输入件 / 容器件 / 反馈件。样式令牌口径见 docs/UI-品质深化 §10/§13。

export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

/* ================= 族0521 工作台五区骨架（§10.1 grid） ================= */

export type WorkbenchRegion = 'menuBar' | 'activityBar' | 'sidePane' | 'editorArea' | 'auxPane' | 'statusBar';

export interface WorkbenchPaneState {
  sideWidth: number; // 240~320，可折叠
  auxWidth: number; // 0~340，可折叠
  sideCollapsed: boolean;
  auxCollapsed: boolean;
  panelCollapsed: boolean;
}

export const WORKBENCH_DEFAULT: WorkbenchPaneState = {
  sideWidth: 320,
  auxWidth: 280,
  sideCollapsed: false,
  auxCollapsed: false,
  panelCollapsed: false,
};

const clamp = (v: number, lo: number, hi: number) => (v < lo ? lo : v > hi ? hi : v);

/** 五区 grid 模板：MenuBar 22px / StatusBar 22px 常驻，四栏可折叠（折叠态留 48px 图标条由 ActivityBar 承担）。 */
export class WorkbenchLayout {
  state: WorkbenchPaneState = { ...WORKBENCH_DEFAULT };
  private collapser = 0;

  gridTemplate(): string {
    const side = this.state.sideCollapsed ? '0px' : `${clamp(this.state.sideWidth, 240, 320)}px`;
    const aux = this.state.auxCollapsed || this.state.auxWidth === 0 ? '0px' : `${clamp(this.state.auxWidth, 0, 340)}px`;
    const panel = this.state.panelCollapsed ? '0px' : 'minmax(120px, 26%)';
    return `"menuBar menuBar menuBar menuBar" 22px "activityBar sidePane editorArea auxPane" 1fr "activityBar sidePane panel auxPane" auto "statusBar statusBar statusBar statusBar" 22px`
      .replace(/"activityBar sidePane editorArea auxPane" 1fr/, `"activityBar ${side} 1fr ${aux}" 1fr`)
      .replace(/"activityBar sidePane panel auxPane" auto/, `"activityBar ${side} ${panel} ${aux}" auto`);
  }

  setSideWidth(w: number): number {
    this.state.sideWidth = clamp(w, 240, 320);
    return this.state.sideWidth;
  }

  setAuxWidth(w: number): number {
    this.state.auxWidth = clamp(w, 0, 340);
    return this.state.auxWidth;
  }

  /** 非法/越界输入回默认档（§13 X*06 口径）。 */
  restore(json: string): boolean {
    try {
      const p = JSON.parse(json) as Partial<WorkbenchPaneState>;
      if (typeof p !== 'object' || p === null) return false;
      const bad =
        (p.sideWidth !== undefined && (typeof p.sideWidth !== 'number' || !isFinite(p.sideWidth))) ||
        (p.auxWidth !== undefined && (typeof p.auxWidth !== 'number' || !isFinite(p.auxWidth)));
      if (bad) {
        this.state = { ...WORKBENCH_DEFAULT };
        return false;
      }
      this.state = {
        sideWidth: clamp(p.sideWidth ?? WORKBENCH_DEFAULT.sideWidth, 240, 320),
        auxWidth: clamp(p.auxWidth ?? WORKBENCH_DEFAULT.auxWidth, 0, 340),
        sideCollapsed: p.sideCollapsed ?? false,
        auxCollapsed: p.auxCollapsed ?? false,
        panelCollapsed: p.panelCollapsed ?? false,
      };
      return true;
    } catch {
      this.state = { ...WORKBENCH_DEFAULT };
      return false;
    }
  }

  snapshot(): string {
    return JSON.stringify(this.state);
  }

  /** 断点续跑：半成品标记后可一键续作（X*08 口径）。 */
  markInterrupted(): void {
    this.collapser += 1;
  }

  resume(): boolean {
    return this.collapser > 0 ? ((this.collapser = 0), true) : false;
  }

  /** 低配降级：全部面板折叠回最小布局（X*09 口径）。 */
  degrade(lowPower: boolean): void {
    if (lowPower) {
      this.state.auxCollapsed = true;
      this.state.panelCollapsed = true;
    }
  }

  /** 卸载净身：不留残档（X*10 口径）。 */
  reset(): void {
    this.state = { ...WORKBENCH_DEFAULT };
    this.collapser = 0;
  }
}

/* ================= 族0522 MenuBar 菜单×功能落位（§10.2 八菜单） ================= */

export interface Command {
  id: string;
  title: string;
  category: string;
  keybinding?: string;
  when?: () => boolean;
  run?: () => void;
}

export const MENU_ORDER = ['文件', '编辑', '选择', '查看', '转到', '运行', '终端', '帮助'] as const;

export const MENU_ITEM_LIMIT = 25;

export class MenuBarModel {
  private registry = new Map<string, Command>();
  private dedupeHits = 0;
  openIndex = -1;
  highlight = -1;
  submenuTimerMs = 300;

  register(cmd: Command): boolean {
    if (this.registry.has(cmd.id)) {
      this.dedupeHits += 1;
      return false;
    }
    this.registry.set(cmd.id, cmd);
    return true;
  }

  get size(): number {
    return this.registry.size;
  }

  get duplicatesRejected(): number {
    return this.dedupeHits;
  }

  /** 菜单树由注册表派生（§10.7 一处注册三处消费）。 */
  menuOf(category: string): Command[] {
    return [...this.registry.values()].filter((c) => c.category === category && (c.when ? c.when() : true));
  }

  /** 顶层菜单溢出：超过容量折叠进 `…`（族0351 溢出算法复用）。 */
  overflow(categories: string[], visibleSlots: number): { visible: string[]; overflowed: string[] } {
    const visible = categories.slice(0, visibleSlots);
    return { visible, overflowed: categories.slice(visibleSlots) };
  }

  open(i: number): void {
    this.openIndex = i;
    this.highlight = 0;
  }

  keys(key: string, itemCount: number): number {
    if (key === 'ArrowDown') this.highlight = Math.min(this.highlight + 1, itemCount - 1);
    else if (key === 'ArrowUp') this.highlight = Math.max(this.highlight - 1, 0);
    else if (key === 'Home') this.highlight = 0;
    else if (key === 'End') this.highlight = itemCount - 1;
    return this.highlight;
  }

  close(): void {
    this.openIndex = -1;
    this.highlight = -1;
  }
}

/* ================= 族0523 ActivityBar 七槽视图（§10.3） ================= */

export interface ActivitySlot {
  id: string;
  view: string;
  badge?: number;
}

export const ACTIVITY_SLOTS: readonly string[] = [
  'explorer', 'search', 'xref', 'analysis', 'mindmap', 'marketplace', 'settings',
];

export class ActivityBarModel {
  active = 0;
  private badges = new Map<string, number>();
  private registered = new Set<string>();

  /** 登记去重：同一槽位重复注册不改变槽位表。 */
  register(id: string): boolean {
    if (this.registered.has(id) || !ACTIVITY_SLOTS.includes(id)) return false;
    this.registered.add(id);
    return true;
  }

  setBadge(id: string, n: number): void {
    if (n > 0) this.badges.set(id, n);
    else this.badges.delete(id);
  }

  badgeOf(id: string): number {
    return this.badges.get(id) ?? 0;
  }

  /** 选中态 = 左 2px 强调条 + icon 变主色；底部齿轮位常驻。 */
  isSelected(i: number): boolean {
    return this.active === i;
  }

  gearIndex(): number {
    return ACTIVITY_SLOTS.length - 1;
  }

  keys(key: string): number {
    if (key === 'ArrowDown') this.active = Math.min(this.active + 1, ACTIVITY_SLOTS.length - 1);
    else if (key === 'ArrowUp') this.active = Math.max(this.active - 1, 0);
    return this.active;
  }

  reset(): void {
    this.badges.clear();
    this.registered.clear();
    this.active = 0;
  }
}

/* ================= 族0524 编辑区 Tab 与欢迎页（§10.4） ================= */

export class EditorTabsModel {
  tabs: string[] = [];
  active = -1;
  recent: string[] = [];

  open(view: string): void {
    if (!this.tabs.includes(view)) this.tabs.push(view);
    this.active = this.tabs.indexOf(view);
    this.recent = [view, ...this.recent.filter((r) => r !== view)].slice(0, 5);
  }

  close(i: number): void {
    this.tabs.splice(i, 1);
    this.active = Math.min(this.active, this.tabs.length - 1);
  }

  /** 拖拽重排（族0032 窗口分组标签算法同口径）。 */
  move(from: number, to: number): void {
    if (from < 0 || from >= this.tabs.length || to < 0 || to >= this.tabs.length) return;
    const [t] = this.tabs.splice(from, 1);
    this.tabs.splice(to, 0, t!);
    if (this.active === from) this.active = to;
  }

  /** Tab 溢出滚动窗口。 */
  viewport(scroll: number, visible: number): { start: number; end: number } {
    const start = clamp(scroll, 0, Math.max(0, this.tabs.length - visible));
    return { start, end: Math.min(this.tabs.length, start + visible) };
  }
}

export const WELCOME_SHORTCUTS: readonly { label: string; combo: string }[] = [
  { label: '打开命令面板', combo: 'Ctrl+Shift+P' },
  { label: '转到文件', combo: 'Ctrl+P' },
  { label: '运行全部检查', combo: 'F5' },
];

export const WELCOME_RECENT_LIMIT = 5;

/* ================= 族0525 Panel 问题/输出区（§10.5） ================= */

export type Severity = 'error' | 'warning' | 'info';

export interface Problem {
  id: string;
  severity: Severity;
  message: string;
  jumpTo: string;
}

export const PANELS = ['问题', '输出', '调试控制台', '终端'] as const;

export class PanelModel {
  activePanel: (typeof PANELS)[number] = '问题';
  collapsed = false;
  private problems: Problem[] = [];
  private logs: string[] = [];
  private jumpTargets = new Map<string, string>();

  addProblem(p: Problem): void {
    this.problems.push(p);
    this.jumpTargets.set(p.id, p.jumpTo);
  }

  get errors(): number {
    return this.problems.filter((p) => p.severity === 'error').length;
  }

  get warnings(): number {
    return this.problems.filter((p) => p.severity === 'warning').length;
  }

  /** 状态栏口径 ✖0 ⚠N。 */
  statusText(): string {
    return `✖${this.errors} ⚠${this.warnings}`;
  }

  jump(problemId: string): string | undefined {
    return this.jumpTargets.get(problemId);
  }

  /** run_all_checks 流式日志（族0381 可观测性落点）。 */
  appendLog(line: string): void {
    this.logs.push(line);
  }

  /** 中断续跑：半成品标记 + 续作（X*08 口径）。 */
  interrupted = false;
  markInterrupted(): void {
    this.interrupted = true;
  }
  resumeOutput(): boolean {
    if (!this.interrupted) return false;
    this.interrupted = false;
    this.appendLog('resume');
    return true;
  }

  /** 资源紧张降级：日志环窗保留末尾 N 行。 */
  degradeLog(max: number): number {
    if (this.logs.length > max) this.logs = this.logs.slice(-max);
    return this.logs.length;
  }

  logLines(): string[] {
    return [...this.logs];
  }

  reset(): void {
    this.problems = [];
    this.logs = [];
    this.jumpTargets.clear();
    this.interrupted = false;
  }
}

/* ================= 族0526 StatusBar 双分区（§10.6） ================= */

export interface StatusBarSlot {
  id: string;
  text: string;
  side: 'left' | 'right';
}

export class StatusBarModel {
  private slots = new Map<string, StatusBarSlot>();

  set(id: string, side: 'left' | 'right', text: string): void {
    this.slots.set(id, { id, side, text });
  }

  remove(id: string): void {
    this.slots.delete(id);
  }

  left(): StatusBarSlot[] {
    return [...this.slots.values()].filter((s) => s.side === 'left');
  }

  right(): StatusBarSlot[] {
    return [...this.slots.values()].filter((s) => s.side === 'right');
  }

  /** 数字列 tabular-nums（§12.1）。 */
  numericAlignment = 'tabular-nums';
}

/* ================= 族0527 命令面板与注册表（§10.7/§17） ================= */

export class CommandRegistry {
  private commands = new Map<string, Command>();
  private recent: string[] = [];
  conflicts: string[] = [];

  add(cmd: Command): boolean {
    if (this.commands.has(cmd.id)) return false;
    this.commands.set(cmd.id, cmd);
    return true;
  }

  get size(): number {
    return this.commands.size;
  }

  /** 快捷键冲突检测（§17.1 纪律：冲突即构建警告）。 */
  detectKeybindingConflicts(): string[] {
    const seen = new Map<string, string>();
    this.conflicts = [];
    for (const c of this.commands.values()) {
      if (!c.keybinding) continue;
      const owner = seen.get(c.keybinding);
      if (owner) this.conflicts.push(`${owner}~${c.id}:${c.keybinding}`);
      else seen.set(c.keybinding, c.id);
    }
    return this.conflicts;
  }

  /** 模糊匹配：子序列命中 + 高亮区间。 */
  static fuzzy(query: string, title: string): { hit: boolean; ranges: [number, number][] } {
    const q = [...query.toLowerCase()];
    const t = [...title.toLowerCase()];
    const ranges: [number, number][] = [];
    let ti = 0;
    for (const ch of q) {
      const at = t.indexOf(ch, ti);
      if (at < 0) return { hit: false, ranges: [] };
      ranges.push([at, at + 1]);
      ti = at + 1;
    }
    return { hit: true, ranges };
  }

  search(query: string): Command[] {
    const all = [...this.commands.values()];
    if (!query) return all;
    return all.filter((c) => CommandRegistry.fuzzy(query, c.title).hit);
  }

  run(id: string): boolean {
    const c = this.commands.get(id);
    if (!c || (c.when && !c.when())) return false;
    this.recent = [id, ...this.recent.filter((r) => r !== id)].slice(0, 5);
    c.run?.();
    return true;
  }

  /** 最近使用置顶。 */
  ordered(): Command[] {
    const rec = this.recent.map((id) => this.commands.get(id)).filter((c): c is Command => !!c);
    const rest = [...this.commands.values()].filter((c) => !this.recent.includes(c.id));
    return [...rec, ...rest];
  }
}

/* ================= 族0528 kit 基础输入件九件（§13 #1~9） ================= */

export type ButtonVariant = 'primary' | 'secondary' | 'danger' | 'subtle';
export type ButtonSize = 'sm' | 'md' | 'tg';

export const BUTTON_VARIANTS: readonly ButtonVariant[] = ['primary', 'secondary', 'danger', 'subtle'];

export const BUTTON_HEIGHT: Record<ButtonSize, number> = { sm: 28, md: 32, tg: 44 };

export class InputModel {
  value = '';
  invalid = false;
  cleared = 0;

  set(v: string): void {
    this.value = v;
  }

  clear(): void {
    this.value = '';
    this.cleared += 1;
  }

  validate(min: number, max: number): boolean {
    const n = Number(this.value);
    this.invalid = !(n >= min && n <= max);
    return !this.invalid;
  }
}

export class TextAreaModel {
  lines = 1;
  maxRows: number;

  constructor(maxRows = 8) {
    this.maxRows = maxRows;
  }

  /** 自动高度：随内容行数增长，钳制在 maxRows。 */
  autoResize(text: string): number {
    this.lines = clamp(text.split('\n').length, 1, this.maxRows);
    return this.lines;
  }
}

export class SelectModel {
  options: string[];
  open = false;
  highlighted = -1;
  selected = -1;
  disabled: Set<number> = new Set();

  constructor(options: string[]) {
    this.options = options;
  }

  toggle(): boolean {
    this.open = !this.open;
    if (this.open && this.highlighted < 0) {
      this.highlighted = this.options.findIndex((_, i) => !this.disabled.has(i));
    }
    return this.open;
  }

  keys(key: string): number {
    const step = (d: number) => {
      let i = this.highlighted;
      for (let n = 0; n < this.options.length; n++) {
        i = clamp(i + d, 0, this.options.length - 1);
        if (!this.disabled.has(i)) break;
      }
      this.highlighted = i;
    };
    if (key === 'ArrowDown') step(1);
    else if (key === 'ArrowUp') step(-1);
    return this.highlighted;
  }

  choose(): string | null {
    if (!this.open || this.highlighted < 0 || this.disabled.has(this.highlighted)) return null;
    this.selected = this.highlighted;
    this.open = false;
    return this.options[this.selected] ?? null;
  }
}

export class CheckboxModel {
  checked = false;
  indeterminate = false;

  toggle(): boolean {
    this.indeterminate = false;
    this.checked = !this.checked;
    return this.checked;
  }

  setIndeterminate(): void {
    this.indeterminate = true;
    this.checked = false;
  }
}

export class RadioGroupModel {
  options: string[];
  selected = -1;

  constructor(options: string[]) {
    this.options = options;
  }

  select(i: number): string | null {
    if (i < 0 || i >= this.options.length) return null;
    this.selected = i;
    return this.options[i]!;
  }
}

export const SWITCH_DIMS = { w: 44, h: 20 };

export class SwitchModelX {
  on = false;

  toggle(): boolean {
    this.on = !this.on;
    return this.on;
  }

  dims(): { w: number; h: number } {
    return { ...SWITCH_DIMS };
  }

  /** role=switch + aria-checked 语义。 */
  ariaChecked(): string {
    return this.on ? 'true' : 'false';
  }
}

export class SliderModelX {
  min: number;
  max: number;
  step: number;
  value: number;
  dragging = false;

  constructor(min: number, max: number, step: number, value: number) {
    this.min = min;
    this.max = max;
    this.step = step;
    this.value = clamp(value, min, max);
  }

  set(v: number): number {
    const snapped = Math.round((v - this.min) / this.step) * this.step + this.min;
    this.value = clamp(snapped, this.min, this.max);
    return this.value;
  }

  bubble(): string {
    return `${this.value}`;
  }

  keys(key: string): number {
    if (key === 'ArrowRight') return this.set(this.value + this.step);
    if (key === 'ArrowLeft') return this.set(this.value - this.step);
    if (key === 'Home') return this.set(this.min);
    if (key === 'End') return this.set(this.max);
    return this.value;
  }
}

/* ================= 族0529 kit 容器与布局件（§13 #10~15） ================= */

export const CARD_TOKENS_X = { radius: 12, padding: 8 };

export class CardModel {
  clickable = false;

  constructor(clickable = false) {
    this.clickable = clickable;
  }

  role(): 'button' | 'group' {
    return this.clickable ? 'button' : 'group';
  }
}

export class CardGroupModel {
  title: string;
  rows: string[] = [];

  constructor(title: string) {
    this.title = title;
  }

  add(row: string): void {
    if (!this.rows.includes(row)) this.rows.push(row);
  }
}

export class SidePaneModelX {
  side: 'left' | 'right' = 'left';
  width = 320;
  collapsed = false;
  /** 折叠/展开 200ms（§13 #13）。 */
  transitionMs = 200;

  toggle(): boolean {
    this.collapsed = !this.collapsed;
    return !this.collapsed;
  }

  ariaExpanded(): string {
    return this.collapsed ? 'false' : 'true';
  }
}

export class DividerModel {
  vertical = false;
  inset = 0;

  role(): string {
    return 'separator';
  }
}

export class ToolbarModelX {
  /** 溢出折叠进 `…`。 */
  static overflow(items: string[], visibleSlots: number): { visible: string[]; overflowMenu: string[] } {
    const visible = items.slice(0, Math.max(0, visibleSlots));
    return { visible, overflowMenu: items.slice(visibleSlots) };
  }
}

export type TabsVariant = 'underline' | 'pill';

export class TabsModelX {
  tabs: string[];
  active = 0;
  variant: TabsVariant = 'underline';

  constructor(tabs: string[], variant: TabsVariant = 'underline') {
    this.tabs = tabs;
    this.variant = variant;
  }

  keys(key: string): number {
    if (key === 'ArrowRight') this.active = (this.active + 1) % this.tabs.length;
    else if (key === 'ArrowLeft') this.active = (this.active - 1 + this.tabs.length) % this.tabs.length;
    else if (key === 'Home') this.active = 0;
    else if (key === 'End') this.active = this.tabs.length - 1;
    return this.active;
  }
}

/* ================= 族0530 kit 反馈与状态件（§13 #16~23） ================= */

export function badgeLabelX(count: number, max = 99): string {
  if (count <= 0) return '0';
  return count > max ? `${max}+` : `${count}`;
}

export class ChipModel {
  label: string;
  selected = false;
  removable: boolean;

  constructor(label: string, removable = false) {
    this.label = label;
    this.removable = removable;
  }

  toggle(): boolean {
    this.selected = !this.selected;
    return this.selected;
  }
}

export class ToastQueueX {
  private items: string[] = [];
  static LIMIT = 3;

  push(id: string): void {
    this.items.push(id);
    if (this.items.length > ToastQueueX.LIMIT) this.items.shift();
  }

  list(): string[] {
    return [...this.items];
  }
}

export const TOOLTIP_TIMING_X = { show: 300, hide: 150 };

export class SkeletonModelX {
  /** 加载 >300ms 才显示骨架（§5 族0405 口径）。 */
  static shouldShow(elapsedMs: number): boolean {
    return elapsedMs > 300;
  }

  static shape(kind: 'text' | 'rect' | 'circle'): string {
    return kind;
  }
}

export class ProgressModelX {
  value = 0;

  set(v: number): number {
    this.value = clamp(v, 0, 100);
    return this.value;
  }
}

export function progressRingX(ratio: number): { dasharray: number; dashoffset: number } {
  const dasharray = 2 * Math.PI * 28;
  const p = clamp(ratio, 0, 1);
  return { dasharray, dashoffset: dasharray * (1 - p) };
}

export function spinnerRole(): string {
  return 'progressbar';
}
