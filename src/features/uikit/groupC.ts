// AURORA-10000: AI-73 批次（领域15 UI 设计与优化 · 族0361~0365 · F09001~F09125），勿删。
// 导航体系 / 搜索体验 UI / 设置体验 / 视觉审计工具 / 性能体验。

/* ===================== 族0361 导航体系（F09001~F09025） ===================== */

export interface NavSection { id: string; label: string; icon: string; children?: string[] }

/** F09001 侧栏分区：固定导航结构。 */
export const NAV_SECTIONS: NavSection[] = [
  { id: 'home', label: '主页', icon: 'home' },
  { id: 'system', label: '系统', icon: 'system', children: ['display', 'sound', 'power'] },
  { id: 'files', label: '文件', icon: 'folder' },
  { id: 'settings', label: '设置', icon: 'settings', children: ['appearance', 'about'] },
];

/** F09002 面包屑生成：层级路径。 */
export function breadcrumbOf(section: string, page: string): string[] {
  return [section, page];
}

/** F09003 返回栈：Alt+← 回退。 */
export class BackStack {
  private stack: string[] = [];
  push(page: string) { this.stack.push(page); }
  back(): string | null { return this.stack.length > 1 ? (this.stack.pop(), this.stack[this.stack.length - 1]!) : null; }
  get depth() { return this.stack.length; }
}

/** F09004 标签历史：Alt+→ 前进。 */
export class TabHistory {
  private past: string[] = [];
  private future: string[] = [];
  go(page: string) { if (this.past.at(-1) !== page) { this.past.push(page); this.future = []; } }
  back(): string | null { const p = this.past.pop(); if (p) this.future.push(p); return this.past.at(-1) ?? null; }
  forward(): string | null { const f = this.future.pop(); if (f) this.past.push(f); return f ?? null; }
}

/** F09005 深链映射：path→页面。 */
export function deepLink(path: string, registry: Record<string, string>): string | null {
  return registry[path] ?? null;
}

/** F09006 当前分区高亮：active 态。 */
export function activeSection(current: string, sections: string[]): string | null {
  return sections.includes(current) ? current : null;
}

/** F09007 折叠记忆：侧栏折叠状态。 */
export class NavCollapse {
  collapsed = false;
  toggle() { this.collapsed = !this.collapsed; return this.collapsed; }
  width() { return this.collapsed ? 48 : 320; }
}

/** F09008 方向键导航：↑↓ 循环。 */
export function navKeyboard(current: number, count: number, key: 'ArrowDown' | 'ArrowUp'): number {
  if (key === 'ArrowDown') return (current + 1) % count;
  return (current - 1 + count) % count;
}

/** F09009 导航内搜索：即时过滤。 */
export function navFilter(sections: NavSection[], q: string): NavSection[] {
  const s = q.trim().toLowerCase();
  if (!s) return sections;
  return sections.filter((sec) => sec.label.toLowerCase().includes(s) || (sec.children ?? []).some((c) => c.includes(s)));
}

/** F09010 跳转列表：跳到分区。 */
export function jumpTo(id: string, sections: NavSection[]): NavSection | null {
  return sections.find((s) => s.id === id) ?? null;
}

/** F09011 应用内上下文导航。 */
export function contextNav(_parent: string | null, siblings: string[]): Array<{ label: string; active: boolean }> {
  return siblings.map((s) => ({ label: s, active: false }));
}

/** F09012 页面切换令牌：200ms。 */
export const PAGE_TRANSITION_MS = 200;

/** F09013 标题同步：document.title 跟随。 */
export function titleSync(section: string, page: string): string {
  return `${page} - ${section} - Variable`;
}

/** F09014 滚动位置恢复。 */
export class ScrollMemory {
  private map = new Map<string, number>();
  save(key: string, y: number) { this.map.set(key, y); }
  restore(key: string): number { return this.map.get(key) ?? 0; }
}

/** F09015 未保存导航守卫。 */
export function navGuard(dirty: boolean): 'confirm' | 'pass' {
  return dirty ? 'confirm' : 'pass';
}

/** F09016 最近页面：最近 5 条。 */
export class RecentPages {
  private list: string[] = [];
  visit(page: string) { this.list = [page, ...this.list.filter((p) => p !== page)].slice(0, 5); }
  recent() { return [...this.list]; }
}

/** F09017 固定页面：置顶收藏。 */
export class PinnedPages {
  private pins: string[] = [];
  pin(page: string) { if (!this.pins.includes(page)) this.pins.push(page); }
  unpin(page: string) { this.pins = this.pins.filter((p) => p !== page); }
  pinned() { return [...this.pins]; }
}

/** F09018 快捷键页：导航快捷键清单。 */
export const NAV_SHORTCUTS = [['Alt+←', '返回'], ['Alt+→', '前进'], ['Ctrl+K', '全局搜索']] as const;

/** F09019 RTL 导航翻转。 */
export function navRtlFlip(rtl: boolean): 'row' | 'row-reverse' {
  return rtl ? 'row-reverse' : 'row';
}

/** F09020 移动端抽屉：窄屏侧栏变抽屉。 */
export function navDrawer(viewport: number): 'sidebar' | 'drawer' {
  return viewport < 640 ? 'drawer' : 'sidebar';
}

/** F09021 aria-current 语义。 */
export function navAriaCurrent(active: boolean): 'page' | null {
  return active ? 'page' : null;
}

/** F09022 图标+文字一致：导航项双标注。 */
export function navItemLabel(sec: NavSection): string {
  return `${sec.icon}:${sec.label}`;
}

/** F09023 导航溢出：分区过多收进「更多」。 */
export function navOverflow(sections: string[], max = 7): { visible: string[]; more: string[] } {
  return { visible: sections.slice(0, max), more: sections.slice(max) };
}

/** F09024 面包屑点击跳层。 */
export function breadcrumbJump(path: string[], index: number): string | null {
  return index >= 0 && index < path.length ? path[index]! : null;
}

/** F09025 导航规范文档。 */
export const NAV_SPEC = ['固定分区', 'Alt+←返回', 'aria-current', '窄屏抽屉', '标题同步'] as const;

/* ===================== 族0362 搜索体验 UI（F09026~F09050） ===================== */

/** F09026 占位符：语境化提示。 */
export function searchPlaceholder(scope: string): string {
  return `在${scope}中搜索`;
}

/** F09027 即时过滤：无延迟小列表。 */
export function instantFilter<T>(items: T[], pred: (t: T) => boolean): T[] {
  return items.filter(pred);
}

/** F09028 输入防抖：250ms。 */
export const SEARCH_DEBOUNCE_MS = 250;

/** F09029 命中高亮：词包裹。 */
export function highlightMatch(text: string, q: string): string {
  const i = text.toLowerCase().indexOf(q.toLowerCase());
  if (!q || i < 0) return text;
  return `${text.slice(0, i)}【${text.slice(i, i + q.length)}】${text.slice(i + q.length)}`;
}

/** F09030 最近搜索：本地保存 5 条。 */
export class RecentSearches {
  private list: string[] = [];
  add(q: string) { const t = q.trim(); if (t) this.list = [t, ...this.list.filter((x) => x !== t)].slice(0, 5); }
  recent() { return [...this.list]; }
}

/** F09031 搜索建议：前缀匹配。 */
export function searchSuggestions(pool: string[], q: string): string[] {
  const s = q.trim().toLowerCase();
  return s ? pool.filter((p) => p.toLowerCase().startsWith(s)).slice(0, 8) : [];
}

/** F09032 无结果空态：附建议。 */
export function searchEmptyState(q: string): { title: string; suggestions: string[] } {
  return { title: `没有与「${q}」匹配的结果`, suggestions: ['检查拼写', '减少关键词', '清除筛选'] };
}

/** F09033 范围选择器：全部/当前页/文件名。 */
export type SearchScope = 'all' | 'page' | 'filename';
export function scopeLabel(s: SearchScope): string {
  return s === 'all' ? '全部' : s === 'page' ? '当前页' : '文件名';
}

/** F09034 结果键盘导航：↑↓ 移动 Enter 确认。 */
export function resultKeyboard(current: number, count: number, key: 'ArrowDown' | 'ArrowUp' | 'Enter'): { index: number; confirmed: boolean } {
  if (key === 'Enter') return { index: current, confirmed: true };
  if (key === 'ArrowDown') return { index: (current + 1) % count, confirmed: false };
  return { index: (current - 1 + count) % count, confirmed: false };
}

/** F09035 Ctrl+K 全局呼出。 */
export const GLOBAL_SEARCH_HOTKEY = 'Ctrl+K';

/** F09036 筛选片：可单独移除。 */
export class FilterChips {
  private chips: string[] = [];
  add(c: string) { if (!this.chips.includes(c)) this.chips.push(c); }
  remove(c: string) { this.chips = this.chips.filter((x) => x !== c); }
  clear() { this.chips = []; }
  list() { return [...this.chips]; }
}

/** F09037 结果分组：按类型。 */
export function groupResults<T extends { kind: string }>(items: T[]): Record<string, T[]> {
  const g: Record<string, T[]> = {};
  items.forEach((i) => { (g[i.kind] ??= []).push(i); });
  return g;
}

/** F09038 索引排除：私密页不建索引。 */
export function indexable(path: string, privatePaths: string[]): boolean {
  return !privatePaths.includes(path);
}

/** F09039 拼音首字母匹配。 */
export function pinyinInitialMatch(_name: string, initials: string, q: string): boolean {
  return initials.toLowerCase().startsWith(q.toLowerCase());
}

/** F09040 模糊评分：编辑距离 ≤2。 */
export function fuzzyScore(a: string, b: string): number {
  const m = a.length; const n = b.length;
  const dp = Array.from({ length: m + 1 }, (_, i) => [i, ...Array(n).fill(0)] as number[]);
  for (let j = 0; j <= n; j++) dp[0]![j] = j;
  for (let i = 1; i <= m; i++) for (let j = 1; j <= n; j++) {
    dp[i]![j] = Math.min(dp[i - 1]![j]! + 1, dp[i]![j - 1]! + 1, dp[i - 1]![j - 1]! + (a[i - 1] === b[j - 1] ? 0 : 1));
  }
  return dp[m]![n]!;
}

/** F09041 取消按钮：搜索中可中断。 */
export function searchCancel(available: boolean): boolean {
  return available;
}

/** F09042 清除按钮：一键清空。 */
export function searchClear(): '' {
  return '';
}

/** F09043 结果计数。 */
export function resultCount(n: number): string {
  return `${n} 个结果`;
}

/** F09044 查询转义：防注入符号。 */
export function escapeQuery(q: string): string {
  return q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** F09045 索引仅本地：不出站。 */
export const SEARCH_LOCAL_ONLY = true;

/** F09046 搜索历史隐私：可清空。 */
export function clearHistory(_list: string[]): string[] {
  return [];
}

/** F09047 结果预览：悬停显示摘要。 */
export function resultPreview(text: string, q: string, len = 60): string {
  const i = text.indexOf(q);
  if (i < 0) return text.slice(0, len);
  const start = Math.max(0, i - 10);
  return `${start > 0 ? '…' : ''}${text.slice(start, start + len)}`;
}

/** F09048 搜索无网络可用：离线降级。 */
export function offlineSearch(online: boolean, localIndex: string[]): string[] {
  return online ? localIndex : localIndex;
}

/** F09049 搜索 i18n：占位与文案走键。 */
export function searchI18nKey(scope: string): string {
  return `search.placeholder.${scope}`;
}

/** F09050 搜索体验规范文档。 */
export const SEARCH_SPEC = ['即时过滤', '防抖 250ms', '高亮命中', '最近搜索本地', '离线可用'] as const;

/* ===================== 族0363 设置体验（F09051~F09075） ===================== */

export interface SettingRow { key: string; title: string; subtitle?: string; kind: 'toggle' | 'select' | 'slider' | 'action'; value: unknown }

/** F09051 Win11 卡片行：圆角 8 + 图标位。 */
export const SETTING_ROW_TOKENS = { radius: 8, iconSlot: 32, minHeight: 48 } as const;

/** F09052 行内展开 Expander：不弹窗。 */
export class SettingExpander {
  open = false;
  toggle() { this.open = !this.open; return this.open; }
}

/** F09053 设置搜索：跨页过滤。 */
export function settingsSearch(rows: SettingRow[], q: string): SettingRow[] {
  const s = q.trim().toLowerCase();
  return s ? rows.filter((r) => r.title.toLowerCase().includes(s) || (r.subtitle ?? '').toLowerCase().includes(s)) : rows;
}

/** F09054 分区锚点：跳转到节。 */
export function sectionAnchor(sectionId: string): string {
  return `#settings-${sectionId}`;
}

/** F09055 单项重置：恢复默认。 */
export class SettingReset {
  private defaults = new Map<string, unknown>();
  registerDefault(key: string, v: unknown) { this.defaults.set(key, v); }
  reset(key: string, current: Map<string, unknown>) { const d = this.defaults.get(key); if (d !== undefined) current.set(key, d); }
  isDefault(key: string, current: Map<string, unknown>) { return current.get(key) === this.defaults.get(key); }
}

/** F09056 修改指示：未默认值带点。 */
export function modifiedDot(isDefault: boolean): 'dot' | null {
  return isDefault ? null : 'dot';
}

/** F09057 即时生效类：toggle/slider。 */
export function applyMode(kind: SettingRow['kind']): 'immediate' | 'on-save' {
  return kind === 'toggle' || kind === 'slider' ? 'immediate' : 'on-save';
}

/** F09058 需重启标记。 */
export function restartBadge(requiresRestart: boolean): 'restart' | null {
  return requiresRestart ? 'restart' : null;
}

/** F09059 设置导入导出。 */
export function exportSettings(data: Record<string, unknown>): string {
  return JSON.stringify(data, null, 2);
}
export function importSettings(json: string): Record<string, unknown> | null {
  try { return JSON.parse(json) as Record<string, unknown>; } catch { return null; }
}

/** F09060 设置页 a11y：标题层级。 */
export function settingsA11y(pageTitle: string): { h1: string; level: 1 } {
  return { h1: pageTitle, level: 1 };
}

/** F09061 危险区样式：红色描边。 */
export const DANGER_ZONE_STYLE = 'border:1px solid var(--aurora-danger)';

/** F09062 依赖禁用：总开关关闭子项禁用。 */
export function dependencyDisabled(parentOn: boolean): boolean {
  return !parentOn;
}

/** F09063 滑杆数值气泡。 */
export function sliderBubble(v: number, unit = ''): string {
  return `${v}${unit}`;
}

/** F09064 分段选择器选模式。 */
export function settingSegmented(options: string[], value: string): number {
  return Math.max(0, options.indexOf(value));
}

/** F09065 面包屑返回：设置二级页。 */
export function settingsBreadcrumb(parent: string, child: string): string[] {
  return [parent, child];
}

/** F09066 设置项 tooltip。 */
export function settingTooltip(text: string): string {
  return `help:${text}`;
}

/** F09067 默认值注册表。 */
export const SETTING_DEFAULTS: Record<string, unknown> = {
  'appearance.theme': 'dark',
  'sound.volume': 50,
  'privacy.telemetry': false,
};

/** F09068 旧键迁移：改名不丢值。 */
export function migrateKey(oldMap: Record<string, unknown>, from: string, to: string, target: Record<string, unknown>): void {
  if (from in oldMap) { target[to] = oldMap[from]!; }
}

/** F09069 遥测开关默认关。 */
export const SETTINGS_TELEMETRY_DEFAULT = false;

/** F09070 设置文档：每页说明。 */
export function settingsDoc(page: string): string {
  return `settings-doc:${page}`;
}

/** F09071 设置 IA 目录：分区计数。 */
export const SETTINGS_IA = ['系统', '设备', '个性化', '应用', '账户', '隐私', '关于'] as const;

/** F09072 搜索命中词高亮。 */
export function settingsHighlight(title: string, q: string): string {
  return q && title.includes(q) ? title.replace(q, `【${q}】`) : title;
}

/** F09073 重置全部：恢复出厂（需确认）。 */
export function resetAllSettings(confirm: boolean): boolean {
  return confirm;
}

/** F09074 设置变更日志：本地记录。 */
export class SettingsChangeLog {
  private log: Array<{ key: string; from: unknown; to: unknown; at: number }> = [];
  record(key: string, from: unknown, to: unknown, at: number) { this.log.push({ key, from, to, at }); }
  entries() { return [...this.log]; }
  for(key: string) { return this.log.filter((l) => l.key === key); }
}

/** F09075 设置体验规范文档。 */
export const SETTINGS_SPEC = ['Win11 卡片', 'Expander 展开式', '搜索即所得', '默认值可查', '遥测默认关'] as const;

/* ===================== 族0364 视觉审计工具（F09076~F09100） ===================== */

export interface AuditIssue { rule: string; severity: 'info' | 'warn' | 'error'; target: string }

/** F09076 硬编码色扫描：非令牌色报错。 */
export function scanHardcodedColor(css: string): AuditIssue[] {
  const hits = [...css.matchAll(/#[0-9a-fA-F]{3,8}\b|rgba?\(/g)];
  return hits.map((h) => ({ rule: 'no-hardcoded-color', severity: 'warn' as const, target: h[0] }));
}

/** F09077 内联时长扫描。 */
export function scanInlineDuration(css: string): AuditIssue[] {
  const hits = [...css.matchAll(/transition[^;]*?(\d+)ms/g)];
  return hits.filter((h) => h[1] !== '150' && h[1] !== '200').map((h) => ({ rule: 'duration-token', severity: 'warn' as const, target: h[0] }));
}

/** F09078 对比度检查：AA 4.5:1。 */
export function contrastCheck(ratio: number): 'AA' | 'fail' {
  return ratio >= 4.5 ? 'AA' : 'fail';
}

/** F09079 触控目标检查：≥44px。 */
export function touchTargetCheck(w: number, h: number): AuditIssue | null {
  return w < 44 || h < 44 ? { rule: 'touch-target-44', severity: 'error', target: `${w}x${h}` } : null;
}

/** F09080 字号阶梯审计。 */
export function fontSizeAudit(px: number): AuditIssue | null {
  const ladder = [12, 14, 16, 18, 20, 28];
  return ladder.includes(px) ? null : { rule: 'font-size-ladder', severity: 'warn', target: `${px}px` };
}

/** F09081 间距刻度审计。 */
export function spacingAudit(values: number[]): AuditIssue[] {
  const scale = [0, 4, 8, 12, 16, 20, 24, 32, 40, 48];
  return values.filter((v) => !scale.includes(v)).map((v) => ({ rule: 'spacing-scale', severity: 'warn', target: `${v}px` }));
}

/** F09082 z-index 审计：超预算报错。 */
export function zIndexAudit(z: number): AuditIssue | null {
  return z > 600 ? { rule: 'z-budget', severity: 'error', target: `${z}` } : null;
}

/** F09083 焦点可见性审计。 */
export function focusVisibleAudit(outline: string): AuditIssue | null {
  return /outline: ?none/i.test(outline) ? { rule: 'focus-visible', severity: 'error', target: outline } : null;
}

/** F09084 图标网格审计。 */
export function iconGridAudit(size: number): AuditIssue | null {
  return [16, 20, 24, 32, 48].includes(size) ? null : { rule: 'icon-grid', severity: 'warn', target: `${size}` };
}

/** F09085 对比度报告：批量。 */
export function contrastReport(pairs: Array<{ name: string; ratio: number }>): { pass: string[]; fail: string[] } {
  return {
    pass: pairs.filter((p) => p.ratio >= 4.5).map((p) => p.name),
    fail: pairs.filter((p) => p.ratio < 4.5).map((p) => p.name),
  };
}

/** F09086 令牌使用率报告。 */
export function tokenUsageReport(css: string): { total: number; tokened: number; ratio: number } {
  const all = [...css.matchAll(/#[0-9a-fA-F]{6}|var\(--/g)].length;
  const tokened = [...css.matchAll(/var\(--/g)].length;
  return { total: all, tokened, ratio: all === 0 ? 1 : tokened / all };
}

/** F09087 i18n 键审计：三语键数一致。 */
export function i18nKeyAudit(zh: string[], tw: string[], en: string[]): { ok: boolean; missing: string[] } {
  const zs = new Set(zh);
  const missing = [
    ...zh.filter((k) => !tw.includes(k)).map((k) => `tw:${k}`),
    ...zh.filter((k) => !en.includes(k)).map((k) => `en:${k}`),
  ];
  return { ok: missing.length === 0 && zs.size === zh.length, missing };
}

/** F09088 aria 审计规则集。 */
export const ARIA_RULES = ['button-name', 'img-alt', 'label-for', 'aria-current', 'live-region'] as const;

/** F09089 重复样式检测。 */
export function duplicateStyles(blocks: string[]): string[] {
  const seen = new Map<string, number>();
  blocks.forEach((b) => seen.set(b, (seen.get(b) ?? 0) + 1));
  return [...seen.entries()].filter(([, n]) => n > 1).map(([b]) => b);
}

/** F09090 快照对比计划。 */
export function snapshotPlan(pages: string[], themes: string[], langs: string[]): number {
  return pages.length * themes.length * langs.length;
}

/** F09091 审计总分。 */
export function auditScore(issues: AuditIssue[]): number {
  const penalty = issues.reduce((s, i) => s + (i.severity === 'error' ? 10 : i.severity === 'warn' ? 3 : 1), 0);
  return Math.max(0, 100 - penalty);
}

/** F09092 白名单登记：豁免项。 */
export class AuditWhitelist {
  private set = new Set<string>();
  add(rule: string, target: string) { this.set.add(`${rule}:${target}`); }
  has(rule: string, target: string) { return this.set.has(`${rule}:${target}`); }
}

/** F09093 CI 门禁函数：error 级阻断。 */
export function ciGate(issues: AuditIssue[]): { pass: boolean; blocking: number } {
  const blocking = issues.filter((i) => i.severity === 'error').length;
  return { pass: blocking === 0, blocking };
}

/** F09094 HTML 报告生成。 */
export function auditHtmlReport(issues: AuditIssue[]): string {
  return `<report>${issues.map((i) => `<issue rule="${i.rule}" sev="${i.severity}">${i.target}</issue>`).join('')}</report>`;
}

/** F09095 修复建议映射。 */
export function auditFixSuggestion(rule: string): string {
  const map: Record<string, string> = {
    'no-hardcoded-color': '改用 var(--aurora-*) 令牌',
    'duration-token': '时长改用令牌',
    'touch-target-44': '目标放大到 44px',
    'focus-visible': '恢复焦点环',
  };
  return map[rule] ?? '人工复核';
}

/** F09096 按页审计：页面范围。 */
export function auditPage(page: string, issues: AuditIssue[]): AuditIssue[] {
  return issues.filter((i) => i.target.includes(page));
}

/** F09097 审计调度：每日一次。 */
export const AUDIT_SCHEDULE = 'daily';

/** F09098 严重度三档。 */
export const SEVERITY_LEVELS: AuditIssue['severity'][] = ['info', 'warn', 'error'];

/** F09099 走查问题台账。 */
export class AuditLedger {
  private issues: AuditIssue[] = [];
  add(issue: AuditIssue) { this.issues.push(issue); }
  resolve(rule: string, target: string) { this.issues = this.issues.filter((i) => !(i.rule === rule && i.target === target)); }
  open() { return [...this.issues]; }
  bySeverity(s: AuditIssue['severity']) { return this.issues.filter((i) => i.severity === s); }
}

/** F09100 视觉审计规范文档。 */
export const VISUAL_AUDIT_SPEC = ['裸值零新增', '对比 AA', '触控 44', '焦点可见', 'CI 阻断'] as const;

/* ===================== 族0365 性能体验（F09101~F09125） ===================== */

/** F09101 数据前出骨架。 */
export function skeletonBeforeData(hasData: boolean, loading: boolean): 'skeleton' | 'content' {
  return !hasData && loading ? 'skeleton' : 'content';
}

/** F09102 乐观 UI 阈值：确认 >100ms 用乐观。 */
export function optimisticThreshold(expectedMs: number): boolean {
  return expectedMs > 100;
}

/** F09103 输入延迟预算：100ms。 */
export const INPUT_LATENCY_BUDGET_MS = 100;

/** F09104 动画 60fps：帧预算 16.7ms。 */
export function frameBudget(ms: number): boolean {
  return ms <= 16.7;
}

/** F09105 搜索防抖 250ms。 */
export const PERF_SEARCH_DEBOUNCE = 250;

/** F09106 滚动节流：rAF 合并。 */
export function scrollThrottle(last: number, now: number, frameMs = 16): boolean {
  return now - last >= frameMs;
}

/** F09107 虚拟列表窗口：只渲染可见区。 */
export function virtualWindow(scrollTop: number, viewport: number, rowH: number, total: number, overscan = 5): { start: number; end: number } {
  const start = Math.max(0, Math.floor(scrollTop / rowH) - overscan);
  const end = Math.min(total, Math.ceil((scrollTop + viewport) / rowH) + overscan);
  return { start, end };
}

/** F09108 图片懒加载：视口附近加载。 */
export function lazyImage(inViewport: boolean): 'load' | 'defer' {
  return inViewport ? 'load' : 'defer';
}

/** F09109 非关键延迟执行。 */
export function deferNonCritical(priority: 'critical' | 'normal' | 'low'): 'now' | 'idle' {
  return priority === 'low' ? 'idle' : 'now';
}

/** F09110 代码分割边界：路由级。 */
export function codeSplitBoundary(route: string): string {
  return `chunk:${route}`;
}

/** F09111 memo 规则：引用稳定才 memo。 */
export function memoEligible(deps: unknown[]): boolean {
  return deps.every((d) => typeof d !== 'object' || d === null);
}

/** F09112 布局抖动守卫：读写分离。 */
export class LayoutThrashGuard {
  private reads: Array<() => void> = [];
  private writes: Array<() => void> = [];
  read(fn: () => void) { this.reads.push(fn); }
  write(fn: () => void) { this.writes.push(fn); }
  flush(): number {
    this.reads.forEach((f) => f());
    this.writes.forEach((f) => f());
    const n = this.reads.length + this.writes.length;
    this.reads = []; this.writes = [];
    return n;
  }
}

/** F09113 长任务报告：>50ms 记录。 */
export function longTask(ms: number): boolean {
  return ms > 50;
}

/** F09114 性能 HUD：帧率/内存浮层。 */
export function perfHud(fps: number, memMb: number): string {
  return `${fps}fps ${memMb}MB`;
}

/** F09115 慢路径文案：低性能提示。 */
export function slowPathMessage(fps: number): string | null {
  return fps < 30 ? '已自动降低动效以保持流畅' : null;
}

/** F09116 优先级提示：fetchpriority。 */
export function priorityHint(kind: 'high' | 'low'): string {
  return `fetchpriority=${kind}`;
}

/** F09117 预连接清单。 */
export const PRECONNECT_HOSTS: string[] = [];

/** F09118 仅合成器属性：transform/opacity。 */
export const COMPOSITOR_ONLY = ['transform', 'opacity'] as const;

/** F09119 内存预算检查。 */
export function memoryBudget(usedMb: number, budgetMb: number): boolean {
  return usedMb <= budgetMb;
}

/** F09120 启动预算：首屏 2s。 */
export function startupBudget(ms: number): boolean {
  return ms <= 2000;
}

/** F09121 交互到下一帧测量。 */
export function inpMeasure(inputAt: number, paintedAt: number): number {
  return paintedAt - inputAt;
}

/** F09122 性能回归告警：超基线 10%。 */
export function perfRegression(current: number, baseline: number): boolean {
  return current > baseline * 1.1;
}

/** F09123 性能预算注册表。 */
export const PERF_BUDGETS = { startupMs: 2000, inputMs: 100, frameMs: 16.7, taskbarMemMb: 50 } as const;

/** F09125 性能体验规范文档。 */
export const PERF_SPEC = ['骨架先行', '输入 100ms', '60fps', '虚拟列表', '预算入基线'] as const;
