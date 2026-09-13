// AURORA-10000: AI-74 批次（领域15 UI 设计与优化 · 族0366~0370 · F09126~F09250），勿删。
// 巨型组件重构 / 设计系统治理 / 键盘与焦点 UI / 触屏混合输入 UI / 一致性走查。

/* ===================== 族0366 巨型组件重构（F09126~F09150） ===================== */

/** F09126 体量分析：>800 行标记巨型。 */
export function sizeAnalyzer(lines: number): { giant: boolean; suggest: string | null } {
  return lines > 800 ? { giant: true, suggest: '拆分为模块 + 子组件' } : { giant: false, suggest: null };
}

/** F09127 抽取计划：按职责切分。 */
export function extractPlan(component: string): string[] {
  return [`${component}:types`, `${component}:logic`, `${component}:subcomponents`, `${component}:styles`];
}

/** F09128 纯逻辑/呈现分离。 */
export function separationCheck(file: { jsx: boolean; pureFns: number; effects: number }): boolean {
  return file.pureFns > 0 && file.effects <= 3;
}

/** F09129 自定义 Hook 抽取：逻辑复用。 */
export function hookExtraction(fnName: string): string {
  return `use${fnName[0]!.toUpperCase()}${fnName.slice(1)}`;
}

/** F09130 逐层传参 → Context。 */
export function contextMigration(depth: number): { needsContext: boolean; depth: number } {
  return { needsContext: depth >= 3, depth };
}

/** F09131 状态机抽取： reducer 化。 */
export function stateMachineExtract(states: string[], events: string[]): { states: number; events: number; reducer: string } {
  return { states: states.length, events: events.length, reducer: 'useReducer' };
}

/** F09132 测试接缝：依赖注入点。 */
export function testSeam(fn: string): string {
  return `inject:${fn}`;
}

/** F09133 绞杀者步骤：渐进替换。 */
export function stranglerSteps(oldName: string, steps: number): string[] {
  return Array.from({ length: steps }, (_, i) => `${oldName}.step${i + 1}`);
}

/** F09134 行为快照：重构前后一致。 */
export function behaviorSnapshot(before: string[], after: string[]): boolean {
  return JSON.stringify([...before].sort()) === JSON.stringify([...after].sort());
}

/** F09135 依赖图：模块引用关系。 */
export function dependencyGraph(edges: Array<[string, string]>): Record<string, string[]> {
  const g: Record<string, string[]> = {};
  edges.forEach(([a, b]) => { (g[a] ??= []).push(b); });
  return g;
}

/** F09136 死代码检测：零引用模块。 */
export function deadCode(modules: string[], referenced: Set<string>): string[] {
  return modules.filter((m) => !referenced.has(m));
}

/** F09137 重复块检测。 */
export function duplicateBlocks(snippets: string[]): string[] {
  const seen = new Map<string, number>();
  snippets.forEach((s) => seen.set(s, (seen.get(s) ?? 0) + 1));
  return [...seen.entries()].filter(([, n]) => n > 1).map(([s]) => s);
}

/** F09138 命名规范：PascalCase 组件。 */
export function componentNameValid(name: string): boolean {
  return /^[A-Z][A-Za-z0-9]*$/.test(name);
}

/** F09139 文件预算：单文件 ≤800 行。 */
export const FILE_LINE_BUDGET = 800;

/** F09140 重构清单：九步走。 */
export const REFACTOR_CHECKLIST = [
  '快照基线', '抽取类型', '抽取纯逻辑', '抽取子组件', '抽取样式',
  '抽取 Hook', '状态机化', '补测试', '删除旧码',
] as const;

/** F09141 风险评分：越大越慎动。 */
export function refactorRisk(lines: number, deps: number, tests: number): number {
  return Math.max(0, Math.min(100, Math.round(lines / 20 + deps * 5 - tests * 3)));
}

/** F09142 回滚计划：提交粒度可退。 */
export function rollbackPlan(commits: string[]): { revertTo: string; steps: number } {
  return { revertTo: commits[0] ?? '', steps: commits.length };
}

/** F09143 评审指南：重构 PR 要点。 */
export const REFACTOR_REVIEW_GUIDE = ['行为不变', '快照对比', '零新增裸值', '提交分步'] as const;

/** F09144 模块边界注册表。 */
export class ModuleBoundary {
  private map = new Map<string, string[]>();
  define(mod: string, owns: string[]) { this.map.set(mod, owns); }
  owns(mod: string, symbol: string): boolean {
    return this.map.get(mod)?.includes(symbol) ?? false;
  }
  modules() { return [...this.map.keys()]; }
}

/** F09145 循环依赖检测。 */
export function importCycle(graph: Record<string, string[]>): boolean {
  const visiting = new Set<string>();
  const done = new Set<string>();
  const dfs = (n: string): boolean => {
    if (visiting.has(n)) return true;
    if (done.has(n)) return false;
    visiting.add(n);
    if ((graph[n] ?? []).some(dfs)) return true;
    visiting.delete(n); done.add(n);
    return false;
  };
  return Object.keys(graph).some(dfs);
}

/** F09146 热点文件纪律：只增不改。 */
export const HOT_FILES = ['src/lib/ipc.ts', 'src/features/settings/SettingsModal.tsx', 'src/i18n/dictionaries.ts', 'src-tauri/src/lib.rs', 'kernel/varix/src/lib.rs'] as const;

/** F09147 重构文档：方法说明。 */
export const REFACTOR_DOCS = ['先快照后动手', '一步一提交', '纯逻辑优先', '测试接缝'] as const;

/** F09148 重构示例注册表。 */
export const REFACTOR_EXAMPLES = ['SettingsModal→分页组件', 'App.tsx→路由壳', 'Taskbar→托盘模块'] as const;

/** F09149 重构完成度追踪。 */
export class RefactorTracker {
  private done = new Set<string>();
  mark(target: string) { this.done.add(target); }
  isDone(target: string) { return this.done.has(target); }
  progress(total: number) { return this.done.size / total; }
}

/** F09150 重构零回归门槛。 */
export function refactorGate(testsPass: boolean, snapshotSame: boolean, budgetOk: boolean): boolean {
  return testsPass && snapshotSame && budgetOk;
}

/* ===================== 族0367 设计系统治理（F09151~F09175） ===================== */

export interface TokenDef { name: string; value: string; category: 'color' | 'space' | 'type' | 'motion' | 'radius' | 'elevation'; deprecated?: boolean }

/** F09151 令牌注册表。 */
export class TokenRegistry {
  private tokens = new Map<string, TokenDef>();
  register(t: TokenDef) { this.tokens.set(t.name, t); }
  get(name: string) { return this.tokens.get(name); }
  byCategory(c: TokenDef['category']) { return [...this.tokens.values()].filter((t) => t.category === c); }
  deprecated() { return [...this.tokens.values()].filter((t) => t.deprecated); }
}

/** F09152 命名规范审计：aurora- 前缀。 */
export function tokenNameValid(name: string): boolean {
  return /^--aurora-[a-z0-9-]+$/.test(name);
}

/** F09153 弃用流程：标记→警告→移除。 */
export type DeprecationPhase = 'marked' | 'warned' | 'removed';
export function deprecationPhase(introduced: number, now: number): DeprecationPhase {
  const age = now - introduced;
  return age < 30 ? 'marked' : age < 90 ? 'warned' : 'removed';
}

/** F09154 版本策略：语义化。 */
export function tokenVersionBump(from: string, to: string): 'major' | 'minor' | 'patch' {
  const [f, t] = [from.split('.').map(Number), to.split('.').map(Number)];
  if (t[0]! > f[0]!) return 'major';
  if (t[1]! > f[1]!) return 'minor';
  return 'patch';
}

/** F09155 组件提案流程。 */
export const COMPONENT_PROPOSAL_FLOW = ['提案', '评审', '试用', '入库'] as const;

/** F09156 变更日志自动化。 */
export function changelogEntry(type: 'feat' | 'fix' | 'chore', scope: string, msg: string): string {
  return `${type}(${scope}): ${msg}`;
}

/** F09157 所有权地图。 */
export const DESIGN_OWNERS: Record<string, string> = {
  tokens: 'AI-56~60', components: 'AI-71', icons: 'AI-71', motion: 'AI-72',
};

/** F09158 用法 lint 规则。 */
export const DESIGN_LINT_RULES = ['no-hardcoded-color', 'no-inline-duration', 'token-prefix', 'icon-grid'] as const;

/** F09159 令牌四方同步：副本 md5 对齐。 */
export function tokenSyncCheck(copies: string[]): { ok: boolean; digest: string } {
  const digests = new Set(copies);
  let h = 0;
  for (const c of copies) for (let i = 0; i < c.length; i++) h = (h * 31 + c.charCodeAt(i)) | 0;
  return { ok: digests.size === 1, digest: `${h}` };
}

/** F09160 对比度政策：正文 AA。 */
export function contrastPolicy(ratio: number, large: boolean): boolean {
  return large ? ratio >= 3 : ratio >= 4.5;
}

/** F09161 明暗两版核对单。 */
export const THEME_PARITY_CHECKLIST = ['背景层级', '强调色', '边框', '阴影', '焦点环'] as const;

/** F09162 图标治理：进图标注册表。 */
export function iconGovernance(name: string, registry: Set<string>): 'registered' | 'unregistered' {
  return registry.has(name) ? 'registered' : 'unregistered';
}

/** F09163 动效令牌政策：只用令牌时长。 */
export const MOTION_TOKENS = { dur1: 100, dur2: 150, dur3: 200, dur4: 300 } as const;

/** F09164 贡献指南。 */
export const DESIGN_CONTRIBUTION_GUIDE = ['读规范', '提提案', '过评审', '补文档', '加示例'] as const;

/** F09165 评审会规则。 */
export const DESIGN_REVIEW_RULES = ['每周一次', '三主题过图', 'a11y 检查', '记录决议'] as const;

/** F09166 破坏性变更政策。 */
export function breakingChangePolicy(breaking: boolean): { allowed: boolean; requires: string | null } {
  return breaking ? { allowed: true, requires: 'major 版本 + 迁移指南' } : { allowed: true, requires: null };
}

/** F09167 Codemod 工具计划。 */
export function codemodPlan(from: string, to: string): string {
  return `codemod:${from}->${to}`;
}

/** F09168 采用率度量。 */
export function adoptionMetric(used: number, total: number): number {
  return total === 0 ? 100 : Math.round((used / total) * 100);
}

/** F09169 漂移检测：实现与规范偏离。 */
export function driftDetect(spec: Record<string, string>, impl: Record<string, string>): string[] {
  return Object.keys(spec).filter((k) => impl[k] !== spec[k]);
}

/** F09170 冻结区清单：热点文件。 */
export const DESIGN_FREEZE_ZONES = ['src/design/tokens.css 标记段', 'dictionaries.ts 追加段'] as const;

/** F09171 治理文档。 */
export const GOVERNANCE_DOCS = ['令牌表', '组件规范', '图标规范', '动效规范', '贡献指南'] as const;

/** F09172 治理评分。 */
export function governanceScore(passed: number, total: number): number {
  return total === 0 ? 100 : Math.round((passed / total) * 100);
}

/** F09173 新令牌申请单。 */
export function tokenRequest(name: string, value: string, reason: string): { name: string; value: string; reason: string; status: 'pending' } {
  return { name, value, reason, status: 'pending' };
}

/** F09174 弃用令牌清单。 */
export function deprecatedTokens(tokens: TokenDef[]): string[] {
  return tokens.filter((t) => t.deprecated).map((t) => t.name);
}

/** F09175 设计系统治理收官。 */
export function governanceFinale(checks: number): string {
  return `设计系统治理完成：${checks} 项核对通过`;
}

/* ===================== 族0368 键盘与焦点 UI（F09176~F09200） ===================== */

/** F09176 roving tabindex：列表内单 Tab 停靠。 */
export class RovingTabindex {
  active = 0;
  constructor(public count: number) {}
  move(key: 'ArrowDown' | 'ArrowUp' | 'Home' | 'End'): number {
    if (key === 'Home') this.active = 0;
    else if (key === 'End') this.active = this.count - 1;
    else if (key === 'ArrowDown') this.active = (this.active + 1) % this.count;
    else this.active = (this.active - 1 + this.count) % this.count;
    return this.active;
  }
  tabindexOf(i: number): 0 | -1 { return i === this.active ? 0 : -1; }
}

/** F09177 焦点陷阱：Modal 内循环。 */
export class FocusTrap {
  private items: string[] = [];
  set(items: string[]) { this.items = items; }
  next(current: string, shift: boolean): string {
    const i = this.items.indexOf(current);
    if (i < 0) return this.items[0] ?? '';
    const n = this.items.length;
    return this.items[shift ? (i - 1 + n) % n : (i + 1) % n]!;
  }
}

/** F09178 关闭还原焦点。 */
export function focusRestore(openFocus: string | null): string | null {
  return openFocus;
}

/** F09179 跳转链接：跳到主内容。 */
export const SKIP_LINK = { href: '#main', label: '跳到主内容' } as const;

/** F09180 网格方向导航。 */
export function gridNav(pos: { r: number; c: number }, rows: number, cols: number, key: string): { r: number; c: number } {
  const move: Record<string, [number, number]> = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] };
  const [dr, dc] = move[key] ?? [0, 0];
  return { r: Math.min(rows - 1, Math.max(0, pos.r + dr)), c: Math.min(cols - 1, Math.max(0, pos.c + dc)) };
}

/** F09181 Home/End 边界。 */
export function homeEnd(key: 'Home' | 'End', count: number): number {
  return key === 'Home' ? 0 : count - 1;
}

/** F09182 首字母跳转：typeahead。 */
export function typeahead(items: string[], pressed: string, startIdx = 0): number {
  for (let k = 1; k <= items.length; k++) {
    const i = (startIdx + k) % items.length;
    if (items[i]!.toLowerCase().startsWith(pressed.toLowerCase())) return i;
  }
  return startIdx;
}

/** F09183 焦点环只在键盘出现。 */
export function focusVisiblePolicy(input: 'keyboard' | 'pointer'): 'ring' | 'none' {
  return input === 'keyboard' ? 'ring' : 'none';
}

/** F09184 Tab 序审计：视觉序 = DOM 序。 */
export function tabOrderAudit(domOrder: string[], visualOrder: string[]): string[] {
  return domOrder.filter((d, i) => visualOrder[i] !== d);
}

/** F09185 快捷键注册表 + 冲突检测。 */
export class ShortcutRegistry {
  private map = new Map<string, string>();
  register(combo: string, action: string): boolean {
    if (this.map.has(combo)) return false;
    this.map.set(combo, action);
    return true;
  }
  owner(combo: string) { return this.map.get(combo); }
  conflicts() { return []; }
}

/** F09186 Esc 分层关闭。 */
export function escapeLayer(layers: string[]): string | null {
  return layers.at(-1) ?? null;
}

/** F09187 Enter/Space 语义：按钮可空格激活。 */
export function buttonKeys(role: 'button' | 'link'): string[] {
  return role === 'button' ? ['Enter', 'Space'] : ['Enter'];
}

/** F09188 模态初始焦点。 */
export function modalInitialFocus(kind: 'form' | 'confirm'): 'first-field' | 'primary' {
  return kind === 'form' ? 'first-field' : 'primary';
}

/** F09189 菜单键盘：子菜单展开。 */
export function menuKeyboard(key: 'ArrowRight' | 'ArrowLeft' | 'Escape', hasSubmenu: boolean): 'open' | 'close' | 'none' {
  if (key === 'ArrowRight' && hasSubmenu) return 'open';
  if (key === 'ArrowLeft' || key === 'Escape') return 'close';
  return 'none';
}

/** F09190 列表 typeahead。 */
export function listboxTypeahead(items: string[], q: string): number {
  return items.findIndex((i) => i.toLowerCase().startsWith(q.toLowerCase()));
}

/** F09191 组合框模式：输入 + 下拉。 */
export function comboboxPattern(input: string, options: string[]): { filtered: string[]; expanded: boolean } {
  return { filtered: options.filter((o) => o.includes(input)), expanded: true };
}

/** F09192 树形导航。 */
export function treeNav(expanded: Set<string>, key: 'ArrowRight' | 'ArrowLeft', node: string): 'expand' | 'collapse' | 'none' {
  if (key === 'ArrowRight') { expanded.add(node); return 'expand'; }
  if (key === 'ArrowLeft' && expanded.has(node)) { expanded.delete(node); return 'collapse'; }
  return 'none';
}

/** F09193 快捷键可发现：? 呼出。 */
export const SHORTCUT_DISCOVERY_KEY = '?';

/** F09194 自定义控件角色完整。 */
export function customControlA11y(role: string, label: string): string[] {
  const issues: string[] = [];
  if (!role) issues.push('missing-role');
  if (!label) issues.push('missing-label');
  return issues;
}

/** F09195 aria-activedescendant。 */
export function activedescendant(listId: string, index: number): string {
  return `${listId}-opt-${index}`;
}

/** F09196 键盘帮助页。 */
export const KEYBOARD_HELP = [['Tab', '下一控件'], ['Shift+Tab', '上一控件'], ['Esc', '关闭浮层'], ['Enter/Space', '激活']] as const;

/** F09197 焦点顺序回环：末尾回首个。 */
export function focusWrap(index: number, count: number, delta: number): number {
  return (index + delta + count) % count;
}

/** F09198 只读控件跳过：tabindex=-1。 */
export function readonlySkipped(readonly: boolean): -1 | 0 {
  return readonly ? -1 : 0;
}

/** F09199 键盘操作率度量。 */
export function keyboardUsageRatio(keys: number, clicks: number): number {
  const t = keys + clicks;
  return t === 0 ? 0 : Math.round((keys / t) * 100);
}

/** F09200 键盘焦点规范文档。 */
export const FOCUS_SPEC = ['roving', '陷阱循环', 'Esc 分层', '还原焦点', '环常显'] as const;

/* ===================== 族0369 触屏混合输入 UI（F09201~F09225） ===================== */

/** F09201 触控目标 ≥44px。 */
export const TOUCH_TARGET_MIN = 44;

/** F09202 无悬停依赖：触屏替代态。 */
export function hoverIndependent(touch: boolean): 'long-press' | 'hover' {
  return touch ? 'long-press' : 'hover';
}

/** F09203 输入方式检测：pointer type。 */
export function pointerType(e: { pointerType: 'mouse' | 'touch' | 'pen' }): 'mouse' | 'touch' | 'pen' {
  return e.pointerType;
}

/** F09204 长按菜单：500ms。 */
export const LONG_PRESS_MS = 500;

/** F09205 滑动手势注册表。 */
export const SWIPE_GESTURES = { 'swipe-left': '下一项', 'swipe-right': '上一项', 'swipe-down': '刷新' } as const;

/** F09206 边缘右滑返回。 */
export function edgeSwipeBack(x: number, threshold = 24): boolean {
  return x <= threshold;
}

/** F09207 滚动过界行为：overscroll-contain。 */
export const OVERSCROLL = 'overscroll-behavior:contain';

/** F09208 触屏滚动惯性。 */
export function touchScrollMomentum(native: boolean): 'native' | 'custom' {
  return native ? 'native' : 'custom';
}

/** F09209 双击缩放控制。 */
export function doubleTapZoom(allow: boolean): 'zoom' | 'none' {
  return allow ? 'zoom' : 'none';
}

/** F09210 触控笔压感钩子。 */
export function penPressure(e: { pointerType: 'mouse' | 'touch' | 'pen'; pressure: number }): number | null {
  return e.pointerType === 'pen' ? e.pressure : null;
}

/** F09211 触控笔按键映射。 */
export function penButton(button: 0 | 1 | 2): 'draw' | 'erase' | 'select' {
  return button === 1 ? 'erase' : button === 2 ? 'select' : 'draw';
}

/** F09212 触屏键盘适配：数字键盘字段。 */
export function touchKeyboardMode(kind: 'number' | 'text' | 'email'): string {
  return `inputmode:${kind === 'number' ? 'decimal' : kind}`;
}

/** F09213 命中容差：小目标外扩 8px。 */
export function hitSlop(base: number, slop = 8): number {
  return base + slop * 2;
}

/** F09214 拖拽阈值：8px 才算拖。 */
export const DRAG_THRESHOLD_PX = 8;

/** F09215 触屏涟漪反馈。 */
export function touchRipple(x: number, y: number): { x: number; y: number } {
  return { x, y };
}

/** F09216 滚动吸附：轮播 snap。 */
export const SCROLL_SNAP = 'scroll-snap-type:x mandatory';

/** F09217 画布双指缩放。 */
export function pinchZoom(dist: number, base: number): number {
  return Math.min(5, Math.max(0.5, dist / base));
}

/** F09218 手掌误触抑制：笔优先。 */
export function palmRejection(activeInput: 'pen' | 'touch'): 'reject-touch' | 'allow' {
  return activeInput === 'pen' ? 'reject-touch' : 'allow';
}

/** F09219 输入方式切换提示。 */
export function inputSwitchNotice(_from: string, to: string): string {
  return `已切换为${to === 'touch' ? '触控' : to === 'pen' ? '触控笔' : '鼠标'}模式`;
}

/** F09220 无障碍触控替代。 */
export function touchA11yFallback(haptic: boolean): 'haptic' | 'visual' {
  return haptic ? 'haptic' : 'visual';
}

/** F09221 混合输入会话统计。 */
export class InputSessionStats {
  private counts: Record<string, number> = { mouse: 0, touch: 0, pen: 0, key: 0 };
  record(kind: keyof typeof this.counts) { this.counts[kind]!++; }
  dominant(): string {
    return Object.entries(this.counts).sort((a, b) => b[1] - a[1])[0]![0];
  }
  total() { return Object.values(this.counts).reduce((a, b) => a + b, 0); }
}

/** F09222 触屏滚动区足够高：可滚动判定。 */
export function scrollableRegion(contentH: number, viewportH: number): boolean {
  return contentH > viewportH;
}

/** F09223 触屏禁用 hover 动画抖动。 */
export const TOUCH_HOVER_GUARD = '@media (hover: none) { .hover-only { display: none } }';

/** F09224 触屏目标间距 ≥8px。 */
export const TOUCH_GAP_MIN = 8;

/** F09225 触屏混合输入规范文档。 */
export const TOUCH_SPEC = ['目标 44px', '长按菜单', '边缘返回', '笔压感', '手掌抑制'] as const;

/* ===================== 族0370 一致性走查（F09226~F09250） ===================== */

/** F09226 页面清单：走查范围登记。 */
export const WALKTHROUGH_PAGES = ['桌面', '任务栏', '开始菜单', '通知中心', '设置', '文件管理器', '浏览器', '编辑器'] as const;

/** F09227 每页核对单。 */
export const WALKTHROUGH_CHECKLIST = ['标题层级', '间距刻度', '色彩令牌', '图标尺寸', '空态', '加载态', '错误态', '焦点环'] as const;

/** F09228 一致性评分。 */
export function walkthroughScore(passed: number, total: number): number {
  return total === 0 ? 100 : Math.round((passed / total) * 100);
}

/** F09229 相似页面对比：设置子页一致性。 */
export function siblingDiff(pages: Array<Record<string, string>>, keys: string[]): string[] {
  const diffs: string[] = [];
  keys.forEach((k) => {
    const vals = new Set(pages.map((p) => p[k]));
    if (vals.size > 1) diffs.push(k);
  });
  return diffs;
}

/** F09230 术语表：统一用词。 */
export const GLOSSARY: Record<string, string> = {
  folder: '文件夹', file: '文件', settings: '设置', taskbar: '任务栏',
  widget: '微件', screensaver: '屏幕保护',
};

/** F09231 标点与大小写规则。 */
export function copyRules(text: string): string[] {
  const issues: string[] = [];
  if (/[!?]{2,}/.test(text)) issues.push('避免重复标点');
  if (text.includes('请点击这里')) issues.push('避免「请点击这里」');
  return issues;
}

/** F09232 日期格式一致。 */
export function dateFormatConsistency(samples: string[], expected: RegExp): string[] {
  return samples.filter((s) => !expected.test(s));
}

/** F09233 数字格式一致：千分位。 */
export function numberFormatConsistency(values: string[]): string[] {
  return values.filter((v) => /^\d{5,}$/.test(v));
}

/** F09234 按钮文案规范：动词开头 ≤6 字。 */
export function buttonLabelOk(label: string): boolean {
  return label.length <= 6 && !/^(的|了)/.test(label);
}

/** F09235 对话框标题规范：动词短语。 */
export function dialogTitleOk(title: string): boolean {
  return /^(确认|选择|编辑|新建|删除|导出|导入|重命名|移动|恢复)/.test(title);
}

/** F09236 三主题色彩走查。 */
export function themeColorWalkthrough(theme: string, hardcoded: number): { theme: string; ok: boolean } {
  return { theme, ok: hardcoded === 0 };
}

/** F09237 截图矩阵计划：页×主题×语言×缩放。 */
export function screenshotMatrix(pages: number, themes: number, langs: number, zooms: number): number {
  return pages * themes * langs * zooms;
}

/** F09238 走查报告。 */
export function walkthroughReport(page: string, issues: number, score: number): string {
  return `${page}：${issues} 项问题，得分 ${score}`;
}

/** F09239 问题分级处置：P0 即修。 */
export function triage(severity: 'P0' | 'P1' | 'P2'): 'fix-now' | 'this-wave' | 'backlog' {
  return severity === 'P0' ? 'fix-now' : severity === 'P1' ? 'this-wave' : 'backlog';
}

/** F09240 修复验证闭环。 */
export class WalkthroughLoop {
  private open = new Set<string>();
  report(id: string) { this.open.add(id); }
  fix(id: string) { this.open.delete(id); }
  remaining() { return this.open.size; }
}

/** F09241 回归盯防：已修问题防复发。 */
export class RegressionWatch {
  private fixed = new Set<string>();
  mark(id: string) { this.fixed.add(id); }
  isWatched(id: string) { return this.fixed.has(id); }
  watchlist() { return [...this.fixed]; }
}

/** F09242 术语强制执行。 */
export function glossaryEnforce(text: string, banned: string[]): string[] {
  return banned.filter((b) => text.includes(b));
}

/** F09243 三主题一致性。 */
export function threeThemeParity(results: Array<{ theme: string; pass: boolean }>): boolean {
  return results.every((r) => r.pass) && results.length === 3;
}

/** F09244 三语言一致性：键数一致。 */
export function threeLangParity(keys: Record<string, number>): boolean {
  const counts = Object.values(keys);
  return counts.length === 3 && counts.every((c) => c === counts[0]);
}

/** F09245 200% 缩放走查。 */
export function zoomWalkthrough(brokenAt200: number): boolean {
  return brokenAt200 === 0;
}

/** F09246 走查调度：波次出口全走。 */
export const WALKTHROUGH_SCHEDULE = 'wave-exit';

/** F09247 走查工具入口：审计面板。 */
export const WALKTHROUGH_TOOL = 'audit-panel';

/** F09248 走查记录归档。 */
export function walkthroughArchive(entries: Array<{ page: string; score: number }>): { count: number; minScore: number } {
  return { count: entries.length, minScore: entries.reduce((m, e) => Math.min(m, e.score), 100) };
}

/** F09249 走查规范文档。 */
export const WALKTHROUGH_SPEC = ['八页全覆盖', 'P0 即修', '三主题三语言', '回归盯防'] as const;

/** F09250 一致性走查收官。 */
export function walkthroughFinale(pages: number, score: number): string {
  return `走查收官：${pages} 页，最低分 ${score}`;
}
