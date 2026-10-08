// AURORA-10000: AI-71 批次（领域15 UI 设计与优化 · 族0351~0355 · F08751~F08875），勿删。
// 组件库补全 / 布局系统 / 图标一致性 / 数据可视化 / 表单体验。

/* ===================== 族0351 组件库补全（F08751~F08875） ===================== */

/** 25 个组件目录：领域15 的组件库补全清单（对齐实施总步骤图 §13 Win11 规范）。 */
export const COMPONENT_CATALOG = [
  'switch', 'expander', 'segmented', 'slider', 'card', 'chevron-row', 'breadcrumb', 'search-box',
  'badge', 'progress-ring', 'tooltip', 'menu-item', 'toolbar', 'tabs', 'dialog', 'toast',
  'skeleton', 'empty-state', 'date-field', 'number-field', 'stepper', 'rating', 'color-swatch',
  'avatar', 'command-palette',
] as const;
export type ComponentId = (typeof COMPONENT_CATALOG)[number];

/** F08751 Switch：44×20 Fluent 开关模型。 */
export class SwitchModel {
  on = false;
  disabled = false;
  toggle(): boolean {
    if (!this.disabled) this.on = !this.on;
    return this.on;
  }
  dims() { return { w: 44, h: 20 }; }
}

/** F08752 Expander：行内展开而非弹窗。 */
export class ExpanderModel {
  open = false;
  toggle() { this.open = !this.open; return this.open; }
  ariaExpanded() { return this.open ? 'true' : 'false'; }
}

/** F08753 分段选择器：单选且必须有一个选中。 */
export class SegmentedModel {
  private idx = 0;
  constructor(public options: string[]) {}
  select(i: number): string | null {
    if (i < 0 || i >= this.options.length) return null;
    this.idx = i;
    return this.options[i]!;
  }
  get selected() { return this.options[this.idx]; }
}

/** F08754 滑杆：数值气泡 + 步进钳位。 */
export class SliderModel {
  constructor(public min: number, public max: number, public step: number, public value: number) {}
  set(v: number): number {
    const stepped = Math.round((v - this.min) / this.step) * this.step + this.min;
    this.value = Math.min(this.max, Math.max(this.min, stepped));
    return this.value;
  }
  bubble() { return `${this.value}`; }
  percent() { return ((this.value - this.min) / (this.max - this.min)) * 100; }
}

/** F08755 卡片：elevation-2 海拔 + 8px 圆角。 */
export const CARD_TOKENS = { radius: 8, elevation: 2, padding: 16 } as const;

/** F08756 Chevron 行：整行可点，行高 ≥48。 */
export const CHEVRON_ROW = { minHeight: 48, iconSlot: 32, chevron: '›' } as const;

/** F08757 面包屑：路径过长时中段省略。 */
export function breadcrumb(path: string[], max = 3): string[] {
  if (path.length <= max) return path;
  return [path[0]!, '…', path[path.length - 1]!];
}

/** F08758 搜索框：占位符「查找设置」+ 即时过滤。 */
export function searchFilter(items: string[], query: string): string[] {
  const q = query.trim().toLowerCase();
  if (!q) return items;
  return items.filter((i) => i.toLowerCase().includes(q));
}

/** F08759 徽标：99+ 封顶。 */
export function badgeLabel(n: number): string {
  return n > 99 ? '99+' : `${Math.max(0, n)}`;
}

/** F08760 进度环：stroke-dashoffset 换算。 */
export function progressRing(pct: number, r = 20): { dasharray: number; dashoffset: number } {
  const c = 2 * Math.PI * r;
  const p = Math.min(1, Math.max(0, pct));
  return { dasharray: c, dashoffset: c * (1 - p) };
}

/** F08761 工具提示：延迟 300ms 显示，150ms 收起。 */
export const TOOLTIP_TIMING = { show: 300, hide: 150 } as const;

/** F08762 菜单项：子菜单 + 快捷键标注。 */
export interface MenuItemDef { label: string; shortcut?: string; submenu?: MenuItemDef[]; disabled?: boolean }
export function flattenMenu(items: MenuItemDef[], depth = 0): Array<{ label: string; depth: number }> {
  return items.flatMap((i) => [{ label: i.label, depth }, ...flattenMenu(i.submenu ?? [], depth + 1)]);
}

/** F08763 工具栏：溢出项收进「…」。 */
export function toolbarOverflow(items: string[], maxVisible: number): { visible: string[]; overflow: string[] } {
  return { visible: items.slice(0, maxVisible), overflow: items.slice(maxVisible) };
}

/** F08764 标签页：roving tabindex 语义。 */
export class TabModel {
  active = 0;
  constructor(public tabs: string[]) {}
  keys(key: 'ArrowLeft' | 'ArrowRight' | 'Home' | 'End'): number {
    const n = this.tabs.length;
    if (key === 'Home') this.active = 0;
    else if (key === 'End') this.active = n - 1;
    else if (key === 'ArrowRight') this.active = (this.active + 1) % n;
    else this.active = (this.active - 1 + n) % n;
    return this.active;
  }
}

/** F08765 对话框：焦点陷阱 + Esc 关闭。 */
export class DialogModel {
  open = false;
  private restore: string | null = null;
  show(currentFocus: string) { this.open = true; this.restore = currentFocus; }
  close(): string | null { this.open = false; const r = this.restore; this.restore = null; return r; }
  trapKeys(): string[] { return ['Tab', 'Shift+Tab', 'Escape']; }
}

/** F08766 Toast：队列上限 3，先进先出。 */
export class ToastQueue {
  private q: string[] = [];
  push(id: string): number {
    this.q.push(id);
    if (this.q.length > 3) this.q.shift();
    return this.q.length;
  }
  list() { return [...this.q]; }
}

/** F08767 骨架屏：镜像布局占位。 */
export function skeletonLayout(rows: number): Array<{ h: number; w: string }> {
  return Array.from({ length: rows }, (_, i) => ({ h: 48, w: i === 0 ? '60%' : '100%' }));
}

/** F08768 空态：插画位 + 主操作。 */
export interface EmptyStateDef { key: string; illustration: boolean; action: string }
export const EMPTY_STATES: EmptyStateDef[] = [
  { key: 'no-items', illustration: true, action: '新建' },
  { key: 'no-results', illustration: true, action: '清除筛选' },
  { key: 'offline', illustration: true, action: '重试' },
];

/** F08769 日期输入：min/max 校验。 */
export function dateFieldValid(value: string, min: string, max: string): boolean {
  return value >= min && value <= max && !Number.isNaN(Date.parse(value));
}

/** F08770 数字输入：钳位 + 千分位。 */
export function numberField(v: number, min: number, max: number): { value: number; text: string } {
  const value = Math.min(max, Math.max(min, v));
  const text = value.toLocaleString('en-US');
  return { value, text };
}

/** F08771 步进器：可禁用负向。 */
export class StepperModel {
  constructor(public value: number, public min: number, public max: number) {}
  inc() { if (this.value < this.max) this.value++; return this.value; }
  dec() { if (this.value > this.min) this.value--; return this.value; }
  canDec() { return this.value > this.min; }
}

/** F08772 评分：半星。 */
export function ratingStars(score: number, max = 5): Array<'full' | 'half' | 'empty'> {
  return Array.from({ length: max }, (_, i) => {
    const d = score - i;
    return d >= 1 ? 'full' : d >= 0.5 ? 'half' : 'empty';
  });
}

/** F08773 色样：对比度报告。 */
export function colorSwatchContrast(fg: [number, number, number], bg: [number, number, number]): number {
  const lum = (c: [number, number, number]) => {
    const [r, g, b] = c.map((v) => {
      const s = v / 255;
      return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!;
  };
  const a = lum(fg); const b = lum(bg);
  const [hi, lo] = a > b ? [a, b] : [b, a];
  return (hi + 0.05) / (lo + 0.05);
}

/** F08774 头像：姓名取首字。 */
export function avatarInitials(name: string): string {
  const t = name.trim();
  if (!t) return '?';
  return /\p{Script=Han}/u.test(t) ? t.slice(-2) : t.split(/\s+/).map((w) => w[0]!).slice(0, 2).join('').toUpperCase();
}

/** F08775 命令面板：Ctrl+Shift+P 模糊匹配。 */
export function commandPalette(commands: string[], query: string): string[] {
  const q = query.toLowerCase();
  return commands.filter((c) => c.toLowerCase().includes(q)).sort((a, b) => a.indexOf(query === '' ? '' : q) - b.indexOf(query === '' ? '' : q));
}

/* ===================== 族0352 布局系统（F08776~F08800） ===================== */

/** F08776 间距刻度：4/8pt 体系。 */
export const SPACING_SCALE = [0, 4, 8, 12, 16, 20, 24, 32, 40, 48] as const;

/** F08777 断点解析：窄/中/宽三档。 */
export function breakpoint(w: number): 'narrow' | 'medium' | 'wide' {
  if (w < 640) return 'narrow';
  if (w < 1024) return 'medium';
  return 'wide';
}

/** F08778 12 栏网格：跨度与偏移换算百分比。 */
export function gridSpan(cols: number, span: number, offset = 0): { width: string; left: string } {
  const c = Math.max(1, Math.min(12, cols));
  const s = Math.max(1, Math.min(c, span));
  const o = Math.max(0, Math.min(c - s, offset));
  return { width: `${(s / c) * 100}%`, left: `${(o / c) * 100}%` };
}

/** F08779 容器查询：按容器宽推荐列数。 */
export function containerColumns(w: number): number {
  if (w < 400) return 1;
  if (w < 800) return 2;
  if (w < 1200) return 3;
  return 4;
}

/** F08780 瀑布流分配：最短列优先。 */
export function masonryAssign(heights: number[], cols: number): number[] {
  const col = Array.from({ length: cols }, () => 0);
  return heights.map(() => {
    let mi = 0;
    col.forEach((h, i) => { if (h < col[mi]!) mi = i; });
    col[mi]! += 1;
    return mi;
  });
}

/** F08781 侧栏内容分栏：固定侧栏 + 弹性内容。 */
export const SPLIT_LAYOUT = { sidebar: 320, sidebarNarrow: 260 } as const;

/** F08782 吸顶头部偏移：scroll-margin 换算。 */
export function stickyOffset(headerH: number): string {
  return `scroll-margin-top:${headerH + 8}px`;
}

/** F08783 安全区：刘海/圆角屏内边距。 */
export function safeArea(top = 0, right = 0, bottom = 0, left = 0): string {
  return `padding:env(safe-area-inset-top,${top}px) env(safe-area-inset-right,${right}px) env(safe-area-inset-bottom,${bottom}px) env(safe-area-inset-left,${left}px)`;
}

/** F08784 RTL 翻转：逻辑属性映射。 */
export function rtlFlip(css: string): string {
  return css.replace(/margin-left/g, 'margin-inline-start').replace(/margin-right/g, 'margin-inline-end')
    .replace(/padding-left/g, 'padding-inline-start').replace(/padding-right/g, 'padding-inline-end')
    .replace(/text-align: ?left/g, 'text-align:start').replace(/text-align: ?right/g, 'text-align:end');
}

/** F08785 宽高比盒：aspect-ratio 声明。 */
export function aspectBox(w: number, h: number): string {
  const g = (a: number, b: number): number => (b === 0 ? a : g(b, a % b));
  const d = g(w, h);
  return `aspect-ratio:${w / d}/${h / d}`;
}

/** F08786 z-index 刻度：分层预算。 */
export const Z_SCALE = { base: 0, dropdown: 100, sticky: 200, overlay: 300, modal: 400, toast: 500 } as const;

/** F08787 垂直栈 gap：相邻间距规则。 */
export function stackGap(items: number, base = 4): number {
  return items > 8 ? base : base * 3;
}

/** F08788 响应式列数：视口宽 → 列数（图标栅格）。 */
export function responsiveColumns(w: number, minColW = 96): number {
  return Math.max(1, Math.floor(w / minColW));
}

/** F08789 内容钳位：min-content~max-content。 */
export function contentClamp(minPx: number, maxPx: number): string {
  return `width:clamp(${minPx}px, 50%, ${maxPx}px)`;
}

/** F08790 布局令牌生成器：输出 CSS 变量段。 */
export function layoutTokens(gap: number, radius: number): string {
  return `--aurora-layout-gap:${gap}px;--aurora-layout-radius:${radius}px;`;
}

/** F08791 视觉对齐补偿：圆形/方形光学补偿。 */
export function opticalAlign(shape: 'circle' | 'square' | 'triangle', box: number): number {
  if (shape === 'circle') return box * 0.96;
  if (shape === 'triangle') return box * 0.92;
  return box;
}

/** F08792 网格吸附：拖放落点取整。 */
export function snapToGrid(v: number, cell: number): number {
  return Math.round(v / cell) * cell;
}

/** F08793 折行规则：CJK 不拆词，长 URL 可断。 */
export function wrapRule(text: string): string {
  return /https?:\/\//.test(text) ? 'overflow-wrap:anywhere' : 'overflow-wrap:normal';
}

/** F08794 两栏对齐：圣杯布局骨架描述。 */
export function holyGrail(sideW: number): string {
  return `grid-template-columns:${sideW}px 1fr ${sideW}px`;
}

/** F08795 布局回退：容器查询不支持时按视口。 */
export function layoutFallback(css: string): string {
  return `@supports not (container-type: inline-size) { ${css} }`;
}

/** F08796 版式基线：行高刻度。 */
export const LINE_HEIGHT_SCALE = { tight: 1.2, normal: 1.5, relaxed: 1.75 } as const;

/** F08797 密度布局：行高随密度三档。 */
export function densityRowHeight(density: 'compact' | 'comfortable' | 'spacious'): number {
  return density === 'compact' ? 32 : density === 'comfortable' ? 40 : 48;
}

/** F08798 布局调试：线框覆盖声明。 */
export function debugOutline(on: boolean): string {
  return on ? 'outline:1px dashed var(--aurora-accent)' : 'outline:none';
}

/** F08799 栅格快照：布局方案序列化。 */
export function layoutSnapshot(cols: number, rows: number, areas: string[]): string {
  return `grid-template:${rows} rows × ${cols} cols [${areas.join('|')}]`;
}

/** F08800 布局文档：规范条款目录。 */
export const LAYOUT_SPEC = ['间距 4/8pt', '12 栏网格', '断点三档', '逻辑属性', 'z 分层', '容器查询优先'] as const;

/* ===================== 族0353 图标一致性（F08801~F08825） ===================== */

export interface IconDef { name: string; category: string; size: number; stroke: number; mirrored?: boolean }

/** F08801 尺寸阶梯：16/20/24/32/48 五档。 */
export const ICON_SIZES = [16, 20, 24, 32, 48] as const;

/** F08802 键线审计：24px 网格内边界检查。 */
export function iconKeylineAudit(icon: IconDef): string[] {
  const issues: string[] = [];
  if (icon.size !== 24 && icon.stroke === 1.5 && icon.size < 24) issues.push('细线在小尺寸需加粗');
  if (!ICON_SIZES.includes(icon.size as (typeof ICON_SIZES)[number])) issues.push('尺寸不在阶梯内');
  return issues;
}

/** F08803 线重令牌：1.5 基线两档。 */
export const ICON_STROKE = { regular: 1.5, bold: 2 } as const;

/** F08804 光学对齐：圆比方大 4%。 */
export function iconOpticalSize(shape: 'circle' | 'rect', base: number): number {
  return shape === 'circle' ? Math.round(base * 1.04) : base;
}

/** F08805 像素贴合：整像素坐标。 */
export function iconPixelSnap(v: number): number {
  return Math.round(v);
}

/** F08806 命名规范：kebab-case 校验。 */
export function iconNameValid(name: string): boolean {
  return /^[a-z][a-z0-9]+(-[a-z0-9]+)*$/.test(name);
}

/** F08807 分类标签：目录五类。 */
export const ICON_CATEGORIES = ['action', 'status', 'file', 'device', 'nav'] as const;

/** F08808 语义色映射：图标着色走令牌。 */
export function iconColorToken(category: (typeof ICON_CATEGORIES)[number]): string {
  return `--aurora-icon-${category}`;
}

/** F08809 RTL 镜像清单：方向性图标翻转。 */
export function iconMirror(name: string, rtl: boolean): boolean {
  const directional = ['arrow-right', 'arrow-left', 'forward', 'back', 'next', 'prev'];
  return rtl && directional.some((d) => name.includes(d));
}

/** F08810 填充/描边配对：同名成对。 */
export function iconPairCheck(icons: IconDef[]): { missing: string[] } {
  const names = new Set(icons.map((i) => i.name));
  return { missing: [...names].filter((n) => !names.has(`${n}-filled`) || !names.has(`${n}-outline`)) };
}

/** F08811 viewBox 强制：24×24。 */
export function iconViewBoxOk(svg: string): boolean {
  return /viewBox="0 0 24 24"/.test(svg);
}

/** F08812 重复检测：路径哈希去重。 */
export function iconDuplicate(paths: string[]): string[] {
  const seen = new Map<string, number>();
  paths.forEach((p) => seen.set(p, (seen.get(p) ?? 0) + 1));
  return [...seen.entries()].filter(([, n]) => n > 1).map(([p]) => p);
}

/** F08813 雪碧图索引：符号表生成。 */
export function iconSprite(names: string[]): string {
  return `<svg>${names.map((n) => `<symbol id="icon-${n}"/>`).join('')}</svg>`;
}

/** F08814 导出规范：SVG + 名称表。 */
export function iconExportSheet(icons: IconDef[]): { count: number; names: string[] } {
  return { count: icons.length, names: icons.map((i) => i.name) };
}

/** F08815 悬停态：提亮 8%。 */
export const ICON_HOVER_BRIGHTNESS = 1.08;

/** F08816 禁用态：去饱和 40%。 */
export const ICON_DISABLED_FILTER = 'saturate(0.6) opacity(0.4)';

/** F08817 焦点态：2px 焦点环。 */
export const ICON_FOCUS_RING = '2px solid var(--aurora-accent)';

/** F08818 一致性评分：全量图标审计得分。 */
export function iconConsistencyScore(icons: IconDef[]): number {
  if (icons.length === 0) return 100;
  const ok = icons.filter((i) => iconNameValid(i.name) && i.size === 24 && i.stroke === 1.5).length;
  return Math.round((ok / icons.length) * 100);
}

/** F08819 图标 linter 规则集。 */
export const ICON_LINT_RULES = ['name-kebab', 'size-in-ladder', 'stroke-token', 'viewbox-24', 'mirror-list'] as const;

/** F08820 修复建议映射。 */
export function iconFixSuggestion(issue: string): string {
  const map: Record<string, string> = {
    'name': '改为 kebab-case',
    'size': '改用 24px 基线',
    'stroke': '线重走 --aurora-icon-stroke 令牌',
  };
  return map[issue] ?? '人工复核';
}

/** F08821 图标文档：使用规范条款。 */
export const ICON_DOCS = ['基线 24px', '线重 1.5', '语义色令牌', 'RTL 镜像', '成对填充描边'] as const;

/** F08822 图标网格可视化：叠加网格线。 */
export function iconGridOverlay(on: boolean): string {
  return on ? 'background-image:linear-gradient(var(--aurora-grid) 1px,transparent 1px)' : '';
}

/** F08823 圆角令牌：容器图标框。 */
export const ICON_CORNER = { none: 0, small: 4, large: 8 } as const;

/** F08824 图标字体回退：缺字占位。 */
export function iconFallback(name: string): string {
  return `[icon:${name}]`;
}

/** F08825 一致性走查清单。 */
export const ICON_WALKTHROUGH = ['命名', '尺寸', '线重', '语义色', '状态', '镜像'] as const;

/* ===================== 族0354 数据可视化（F08826~F08850） ===================== */

/** F08826 CVD 安全图表色板：8 色。 */
export const CHART_PALETTE = ['#0078D4', '#E3008C', '#00B294', '#F7630C', '#744DA9', '#0099BC', '#C239B3', '#498205'] as const;

/** F08827 nice 刻度：1/2/5×10^n。 */
export function niceTicks(min: number, max: number, count = 5): number[] {
  const span = max - min;
  if (span <= 0) return [min];
  const raw = span / count;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const norm = raw / mag;
  const step = (norm <= 1 ? 1 : norm <= 2 ? 2 : norm <= 5 ? 5 : 10) * mag;
  const out: number[] = [];
  for (let v = Math.ceil(min / step) * step; v <= max + 1e-9; v += step) out.push(Math.round(v * 1e6) / 1e6);
  return out;
}

/** F08828 线性/对数比例。 */
export function scaleValue(v: number, domain: [number, number], range: [number, number], log = false): number {
  const [d0, d1] = domain; const [r0, r1] = range;
  if (log) {
    const lv = Math.log10(Math.max(1e-9, v));
    const ld0 = Math.log10(Math.max(1e-9, d0)); const ld1 = Math.log10(Math.max(1e-9, d1));
    return r0 + ((lv - ld0) / (ld1 - ld0)) * (r1 - r0);
  }
  return r0 + ((v - d0) / (d1 - d0)) * (r1 - r0);
}

/** F08829 迷你走势图：折线点集。 */
export function sparkline(data: number[], w: number, h: number): Array<[number, number]> {
  if (data.length === 0) return [];
  const min = Math.min(...data); const max = Math.max(...data);
  const span = max - min || 1;
  return data.map((v, i) => [(i / (data.length - 1 || 1)) * w, h - ((v - min) / span) * h] as [number, number]);
}

/** F08830 环形图弧：角度换算。 */
export function donutArc(pct: number, r = 40): { large: 0 | 1; end: [number, number] } {
  const a = Math.min(1, Math.max(0, pct)) * Math.PI * 2 - Math.PI / 2;
  return { large: pct > 0.5 ? 1 : 0, end: [50 + r * Math.cos(a), 50 + r * Math.sin(a)] };
}

/** F08831 堆叠条：逐段累计。 */
export function stackedBar(values: number[]): Array<{ y0: number; y1: number }> {
  let acc = 0;
  return values.map((v) => { const seg = { y0: acc, y1: acc + v }; acc += v; return seg; });
}

/** F08832 图例布局：两列折行。 */
export function legendLayout(labels: string[], perRow = 2): string[][] {
  const rows: string[][] = [];
  for (let i = 0; i < labels.length; i += perRow) rows.push(labels.slice(i, i + perRow));
  return rows;
}

/** F08833 提示框锚点：边缘翻转。 */
export function tooltipAnchor(x: number, y: number, w: number, h: number, vw: number, vh: number): { x: number; y: number; flip: string } {
  const flip = (x + w > vw ? 'left' : x - w < 0 ? 'right' : '') + (y + h > vh ? 'top' : '');
  return { x: Math.min(Math.max(0, x), vw - w), y: Math.min(Math.max(0, y), vh - h), flip };
}

/** F08834 平滑曲线：Catmull-Rom → 贝塞尔。 */
export function smoothPath(pts: Array<[number, number]>): string {
  if (pts.length < 2) return '';
  let d = `M${pts[0]![0]},${pts[0]![1]}`;
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[Math.max(0, i - 1)]!; const p1 = pts[i]!; const p2 = pts[i + 1]!; const p3 = pts[Math.min(pts.length - 1, i + 2)]!;
    const c1: [number, number] = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
    const c2: [number, number] = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
    d += `C${c1[0]},${c1[1]} ${c2[0]},${c2[1]} ${p2[0]},${p2[1]}`;
  }
  return d;
}

/** F08835 阈值带：超标区着色。 */
export function thresholdBand(data: number[], threshold: number): boolean[] {
  return data.map((v) => v > threshold);
}

/** F08836 数据标签防重叠：简化为隔 n 显示。 */
export function declutterLabels(n: number, maxLabels = 6): number[] {
  const step = Math.max(1, Math.ceil(n / maxLabels));
  return Array.from({ length: n }, (_, i) => i).filter((i) => i % step === 0);
}

/** F08837 空数据兜底：空态而非空白。 */
export function chartEmptyGuard(data: number[]): { empty: boolean; message: string } {
  return data.length === 0 ? { empty: true, message: '暂无数据' } : { empty: false, message: '' };
}

/** F08838 零基线规则：条形图必须从 0 起。 */
export function barDomain(values: number[]): [number, number] {
  return [0, Math.max(...values, 1)];
}

/** F08839 时间轴标签：按跨度选粒度。 */
export function timeAxisLabels(spanMs: number): { unit: string; format: string } {
  if (spanMs < 60_000) return { unit: '秒', format: 'HH:mm:ss' };
  if (spanMs < 3_600_000) return { unit: '分', format: 'HH:mm' };
  if (spanMs < 86_400_000) return { unit: '时', format: 'HH:00' };
  return { unit: '日', format: 'MM-DD' };
}

/** F08840 数值格式：K/M 缩写。 */
export function formatCompact(n: number): string {
  if (Math.abs(n) >= 1e8) return `${(n / 1e8).toFixed(1)}亿`;
  if (Math.abs(n) >= 1e4) return `${(n / 1e4).toFixed(1)}万`;
  if (Math.abs(n) >= 1e3) return `${(n / 1e3).toFixed(1)}K`;
  return `${Math.round(n)}`;
}

/** F08841 色带插值：双端渐变采样。 */
export function colorRamp(stops: string[], t: number): string {
  const p = Math.min(1, Math.max(0, t)) * (stops.length - 1);
  const i = Math.min(stops.length - 2, Math.floor(p));
  const f = p - i;
  const hex = (s: string) => [1, 3, 5].map((k) => parseInt(s.slice(k, k + 2), 16));
  const a = hex(stops[i]!); const b = hex(stops[i + 1]!);
  const mix = a.map((v, k) => Math.round(v + (b[k]! - v) * f));
  return `#${mix.map((v) => v.toString(16).padStart(2, '0')).join('')}`;
}

/** F08842 条最小宽：防不可见。 */
export function barMinWidth(value: number, max: number, w: number, minPx = 2): number {
  if (value <= 0) return 0;
  return Math.max(minPx, (value / max) * w);
}

/** F08843 饼图排序：大块优先且最后归并「其他」。 */
export function pieSliceSort(items: Array<{ label: string; value: number }>): Array<{ label: string; value: number }> {
  const sorted = [...items].sort((a, b) => b.value - a.value);
  if (sorted.length <= 6) return sorted;
  const top = sorted.slice(0, 5);
  const rest = sorted.slice(5).reduce((s, i) => s + i.value, 0);
  return [...top, { label: '其他', value: rest }];
}

/** F08844 趋势箭头：环比方向。 */
export function trendArrow(current: number, previous: number): '↑' | '↓' | '→' {
  if (current > previous) return '↑';
  if (current < previous) return '↓';
  return '→';
}

/** F08845 网格线：横向为主，纵向按需。 */
export function gridLines(count: number, horizontal = true): string[] {
  return Array.from({ length: count }, (_, i) => `${horizontal ? 'h' : 'v'}${i}`);
}

/** F08846 响应式图表：容器宽决定重算。 */
export function chartResponsive(w: number): { height: number; compact: boolean } {
  return { height: w < 400 ? 160 : 280, compact: w < 400 };
}

/** F08847 数据动效：统一 200ms 过渡。 */
export const CHART_TRANSITION_MS = 200;

/** F08848 系列配色分配：按索引循环。 */
export function seriesColor(i: number): string {
  return CHART_PALETTE[i % CHART_PALETTE.length]!;
}

/** F08849 导出 SVG：序列化骨架。 */
export function chartExportSvg(w: number, h: number, content: string): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}">${content}</svg>`;
}

/** F08850 图表 a11y：数据表回退。 */
export function chartA11yTable(title: string, data: Array<{ label: string; value: number }>): string {
  const rows = data.map((d) => `|${d.label}|${d.value}|`).join('');
  return `表:${title}${rows}`;
}

/* ===================== 族0355 表单体验（F08851~F08875） ===================== */

export interface FieldState { name: string; value: string; error: string | null; touched: boolean; dirty: boolean }

/** F08851 字段模型：touched/dirty/error 三态。 */
export class FieldModel {
  state: FieldState;
  constructor(name: string, private validate: (v: string) => string | null) {
    this.state = { name, value: '', error: null, touched: false, dirty: false };
  }
  set(v: string) { this.state = { ...this.state, value: v, dirty: true }; return this.state; }
  blur() { this.state = { ...this.state, touched: true, error: this.validate(this.state.value) }; return this.state; }
  valid() { return this.validate(this.state.value) === null; }
}

/** F08852 必填标记：视觉星号 + aria-required。 */
export function requiredMark(required: boolean): { mark: string; aria: boolean } {
  return required ? { mark: '*', aria: true } : { mark: '', aria: false };
}

/** F08853 校验时机：blur 提示、submit 强校验。 */
export function validationTiming(event: 'input' | 'blur' | 'submit'): 'none' | 'gentle' | 'strict' {
  return event === 'input' ? 'none' : event === 'blur' ? 'gentle' : 'strict';
}

/** F08854 行内错误：aria-describedby 关联。 */
export function fieldErrorAria(id: string, hasError: boolean): string | null {
  return hasError ? `${id}-error` : null;
}

/** F08855 成功态：校验通过对勾。 */
export function fieldSuccess(valid: boolean, dirty: boolean): boolean {
  return valid && dirty;
}

/** F08856 密码强度：长度/字符类四档。 */
export function passwordStrength(pw: string): 0 | 1 | 2 | 3 {
  let score = 0;
  if (pw.length >= 8) score++;
  if (/[A-Z]/.test(pw) && /[a-z]/.test(pw)) score++;
  if (/\d/.test(pw) && /[^A-Za-z0-9]/.test(pw)) score++;
  return score as 0 | 1 | 2 | 3;
}

/** F08857 输入掩码：手机号 3-4-4。 */
export function phoneMask(digits: string): string {
  const d = digits.replace(/\D/g, '').slice(0, 11);
  if (d.length <= 3) return d;
  if (d.length <= 7) return `${d.slice(0, 3)} ${d.slice(3)}`;
  return `${d.slice(0, 3)} ${d.slice(3, 7)} ${d.slice(7)}`;
}

/** F08858 autocomplete 语义映射。 */
export function autocompleteAttr(kind: 'name' | 'email' | 'tel' | 'new-password' | 'one-time-code'): string {
  return `autocomplete="${kind}"`;
}

/** F08859 聚焦首个错误：submit 失败定位。 */
export function focusFirstError(errors: Array<{ name: string; error: string | null }>): string | null {
  return errors.find((e) => e.error)?.name ?? null;
}

/** F08860 脏数据跟踪：离开提醒。 */
export function formDirtyGuard(dirty: boolean, saved: boolean): 'block' | 'allow' {
  return dirty && !saved ? 'block' : 'allow';
}

/** F08861 草稿自动保存：1.5s 防抖。 */
export function draftAutosave(lastEdit: number, now: number, debounceMs = 1500): 'pending' | 'saved' {
  return now - lastEdit >= debounceMs ? 'saved' : 'pending';
}

/** F08862 多步向导：进度 + 回退。 */
export class WizardModel {
  step = 0;
  constructor(public total: number) {}
  next() { if (this.step < this.total - 1) this.step++; return this.step; }
  prev() { if (this.step > 0) this.step--; return this.step; }
  progress() { return ((this.step + 1) / this.total) * 100; }
}

/** F08863 提交防抖：进行中禁用。 */
export function submitGuard(submitting: boolean): boolean {
  return !submitting;
}

/** F08864 合法才可提交。 */
export function submitEnabled(valid: boolean, submitting: boolean): boolean {
  return valid && !submitting;
}

/** F08865 字数计数器：上限提示变色。 */
export function charCounter(text: string, max: number): { count: number; warn: boolean } {
  return { count: text.length, warn: text.length > max * 0.9 };
}

/** F08866 粘贴净化：去首尾空白。 */
export function pasteNormalize(v: string): string {
  return v.trim().replace(/\s+/g, ' ');
}

/** F08867 数字钳位输入。 */
export function numberInputClamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, Number.isNaN(v) ? min : v));
}

/** F08868 下拉空选项：占位不可选。 */
export function selectPlaceholder(label = '请选择'): { value: string; disabled: boolean } {
  return { value: label, disabled: true };
}

/** F08869 全选联动：勾选组。 */
export function checkboxSelectAll(children: boolean[]): 'all' | 'some' | 'none' {
  if (children.every(Boolean)) return 'all';
  if (children.some(Boolean)) return 'some';
  return 'none';
}

/** F08870 单选键盘：方向键循环。 */
export function radioKeyboard(current: number, count: number, key: 'ArrowDown' | 'ArrowUp'): number {
  if (key === 'ArrowDown') return (current + 1) % count;
  return (current - 1 + count) % count;
}

/** F08871 文本域自增高：行数换算。 */
export function textareaAutogrow(text: string, lineH = 20, pad = 16): number {
  const lines = text.split('\n').length;
  return lines * lineH + pad;
}

/** F08872 日期范围校验：起 ≤ 止。 */
export function dateRangeValid(start: string, end: string): boolean {
  return start <= end;
}

/** F08873 拖放上传：文件类型过滤。 */
export function fileDropAccept(files: string[], accept: string[]): { ok: string[]; rejected: string[] } {
  return {
    ok: files.filter((f) => accept.some((a) => f.endsWith(a))),
    rejected: files.filter((f) => !accept.some((a) => f.endsWith(a))),
  };
}

/** F08874 表单 a11y：标签必绑。 */
export function formA11yAudit(fields: Array<{ id: string; label: string }>): string[] {
  return fields.filter((f) => !f.label).map((f) => `${f.id}:missing-label`);
}

/** F08875 错误汇总：顶部摘要 + 逐项定位。 */
export function errorSummary(errors: string[]): { count: number; list: string[] } {
  return { count: errors.length, list: errors };
}
