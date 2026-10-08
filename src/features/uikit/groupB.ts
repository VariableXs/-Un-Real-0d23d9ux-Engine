// AURORA-10000: AI-72 批次（领域15 UI 设计与优化 · 族0356~0360 · F08876~F09000），勿删。
// 加载与骨架 / 错误处理 UI / 空态与引导 / 信息密度优化 / 交互反馈强化。

/* ===================== 族0356 加载与骨架（F08876~F08900） ===================== */

/** F08876 加载状态机：idle→loading→success|error。 */
export class LoadMachine {
  state: 'idle' | 'loading' | 'success' | 'error' = 'idle';
  start() { this.state = 'loading'; return this.state; }
  succeed() { this.state = 'success'; return this.state; }
  fail() { this.state = 'error'; return this.state; }
  reset() { this.state = 'idle'; return this.state; }
}

/** F08877 骨架镜像：与真实布局同构占位。 */
export function skeletonMirror(layout: Array<{ h: number; w: number }>): Array<{ h: number; w: number }> {
  return layout.map((l) => ({ h: l.h, w: Math.round(l.w) }));
}

/** F08878 微光动效令牌：1.2s 循环。 */
export const SHIMMER_MS = 1200;

/** F08879 图片渐进加载：模糊→清晰。 */
export function progressiveImage(loaded: boolean): { filter: string; scale: number } {
  return loaded ? { filter: 'blur(0px)', scale: 1 } : { filter: 'blur(12px)', scale: 1.05 };
}

/** F08880 骨架 vs 转圈：结构已知用骨架。 */
export function loaderChoice(structureKnown: boolean): 'skeleton' | 'spinner' {
  return structureKnown ? 'skeleton' : 'spinner';
}

/** F08881 防闪屏：最短展示时长。 */
export const MIN_LOADING_MS = 300;

/** F08882 旧数据标记：SWR 刷新中不遮内容。 */
export function swrIndicator(stale: boolean, revalidating: boolean): string {
  if (stale && revalidating) return 'refreshing';
  if (stale) return 'stale';
  return 'fresh';
}

/** F08883 无限滚动哨兵：提前 200px 触发。 */
export function infiniteSentinel(scrollBottom: number, threshold = 200): boolean {
  return scrollBottom <= threshold;
}

/** F08884 下拉刷新：阈值回弹。 */
export function pullToRefresh(pullPx: number, trigger = 64): { armed: boolean; offset: number } {
  return { armed: pullPx >= trigger, offset: Math.min(pullPx, trigger * 1.5) };
}

/** F08885 顶部进度条：不确定长条。 */
export const TOP_PROGRESS = { height: 2, indeterminate: true } as const;

/** F08886 按钮内加载：转圈替代文字。 */
export function buttonLoading(loading: boolean): { spinner: boolean; disabled: boolean } {
  return { spinner: loading, disabled: loading };
}

/** F08887 路由懒加载兜底：Suspense 骨架。 */
export function routeFallback(name: string): string {
  return `skeleton:${name}`;
}

/** F08888 文本占位块：三行渐宽。 */
export function textPlaceholder(lines = 3): string[] {
  return Array.from({ length: lines }, (_, i) => `${100 - i * 15}%`);
}

/** F08889 头像微光占位。 */
export const AVATAR_PLACEHOLDER = { size: 40, shape: 'circle', shimmer: true } as const;

/** F08890 表格骨架行数：首屏 5 行。 */
export function tableSkeletonRows(rows = 5): number[] {
  return Array.from({ length: rows }, (_, i) => i);
}

/** F08891 百分比加载：数值可读。 */
export function percentLoader(done: number, total: number): { pct: number; label: string } {
  const pct = total === 0 ? 100 : Math.round((done / total) * 100);
  return { pct, label: `${pct}%` };
}

/** F08892 不确定循环：0→100 伪进度。 */
export function indeterminateLoop(tick: number): number {
  return (tick % 100);
}

/** F08893 加载超时文案：8s 出提示。 */
export function loadTimeout(ms: number, limit = 8000): string | null {
  return ms >= limit ? '仍在加载，网络可能较慢' : null;
}

/** F08894 重试入口：失败态主操作。 */
export function retryAffordance(state: 'idle' | 'loading' | 'success' | 'error'): string | null {
  return state === 'error' ? '重试' : null;
}

/** F08895 预取提示：悬停 100ms 预载。 */
export const PREFETCH_DELAY_MS = 100;

/** F08896 字体交换：font-display 策略。 */
export const FONT_SWAP = 'font-display:swap';

/** F08897 图片宽高占位：防布局跳动。 */
export function imagePlaceholder(w: number, h: number): string {
  return `aspect-ratio:${w}/${h}`;
}

/** F08898 列表错峰显现：30ms 交错。 */
export function listStagger(i: number, base = 30): number {
  return Math.min(i * base, 300);
}

/** F08899 首帧预算：<200ms 出骨架。 */
export function firstPaintBudget(ms: number): boolean {
  return ms < 200;
}

/** F08900 加载规范文档。 */
export const LOADING_SPEC = ['骨架优先', '防闪 300ms', '超时提示', '重试入口', '错峰显现'] as const;

/* ===================== 族0357 错误处理 UI（F08901~F08925） ===================== */

export type ErrorLevel = 'info' | 'warn' | 'error' | 'fatal';

/** F08901 错误分级：四档。 */
export function errorLevel(code: string): ErrorLevel {
  if (code.startsWith('E')) return 'error';
  if (code.startsWith('W')) return 'warn';
  if (code.startsWith('F')) return 'fatal';
  return 'info';
}

/** F08902 行内错误：字段级提示。 */
export function inlineError(msg: string | null): { text: string | null; role: string | null } {
  return msg ? { text: msg, role: 'alert' } : { text: null, role: null };
}

/** F08903 Toast 错误：带操作按钮。 */
export function errorToast(msg: string, action?: string): { msg: string; action: string | null; level: ErrorLevel } {
  return { msg, action: action ?? null, level: 'error' };
}

/** F08904 整页错误：重试为主操作。 */
export function fullPageError(recoverable: boolean): { title: string; primary: string } {
  return recoverable ? { title: '出了点问题', primary: '重试' } : { title: '无法恢复', primary: '返回首页' };
}

/** F08905 错误码目录：稳定编码表。 */
export const ERROR_CODES = {
  NET_OFFLINE: 'E1001', NET_TIMEOUT: 'E1002', FILE_NOT_FOUND: 'E2001',
  PERM_DENIED: 'E3001', DISK_FULL: 'W2001', DEPRECATED: 'I1001',
} as const;

/** F08906 人话文案映射。 */
export function humanizeError(code: string): string {
  const map: Record<string, string> = {
    E1001: '网络不可用，请检查连接',
    E1002: '请求超时，稍后再试',
    E2001: '找不到这个文件',
    E3001: '没有权限执行该操作',
    W2001: '磁盘空间不足',
  };
  return map[code] ?? '发生未知错误';
}

/** F08907 重试退避：1s/2s/4s 上限 3 次。 */
export function retryBackoff(attempt: number): number | null {
  if (attempt >= 3) return null;
  return 1000 * 2 ** attempt;
}

/** F08908 离线横幅：网络断开提示。 */
export function offlineBanner(online: boolean): { show: boolean; text: string } {
  return online ? { show: false, text: '' } : { show: true, text: '当前处于离线状态' };
}

/** F08909 网络重试倒计时。 */
export function retryCountdown(sec: number): string {
  return sec > 0 ? `${sec}s 后自动重试` : '正在重试…';
}

/** F08910 表单级错误汇总。 */
export function formErrorSummary(fields: Array<{ name: string; error: string | null }>): string[] {
  return fields.filter((f) => f.error).map((f) => `${f.name}：${f.error}`);
}

/** F08911 404 页：返回入口。 */
export const NOT_FOUND_PAGE = { title: '找不到页面', action: '返回' } as const;

/** F08912 500 页：服务错误。 */
export const SERVER_ERROR_PAGE = { title: '服务暂时不可用', action: '重试' } as const;

/** F08913 崩溃兜底边界：组件树兜底 UI。 */
export function crashFallback(component: string): { boundary: string; message: string } {
  return { boundary: component, message: '该区域暂时无法显示' };
}

/** F08914 错误本地报告：不上传。 */
export function errorReportLocal(code: string, stack: string): { code: string; stack: string; uploaded: false } {
  return { code, stack: stack.slice(0, 2000), uploaded: false };
}

/** F08915 破坏性确认：危险操作二次确认。 */
export function destructiveConfirm(label: string): { confirmText: string; danger: boolean } {
  return { confirmText: `确认${label}`, danger: true };
}

/** F08916 未保存守卫：拦截导航。 */
export function unsavedGuard(dirty: boolean): { block: boolean; message: string } {
  return { block: dirty, message: '有未保存的更改' };
}

/** F08917 恢复建议清单。 */
export function recoverySuggestions(code: string): string[] {
  const map: Record<string, string[]> = {
    E1001: ['检查网络', '切换离线模式', '稍后重试'],
    W2001: ['清理回收站', '运行磁盘清理'],
  };
  return map[code] ?? ['重试一次'];
}

/** F08918 错误去重：同类 30s 内不重复弹。 */
export function errorDedup(lastAt: number, now: number, windowMs = 30_000): boolean {
  return now - lastAt < windowMs;
}

/** F08919 升级路径：反复失败引导人工。 */
export function escalationPath(failures: number): 'auto' | 'manual' {
  return failures >= 3 ? 'manual' : 'auto';
}

/** F08920 role=alert 语义。 */
export const ALERT_ROLE = 'alert';

/** F08921 焦点移至错误。 */
export function focusOnError(hasError: boolean, targetId: string): string | null {
  return hasError ? `#${targetId}` : null;
}

/** F08922 复制诊断按钮。 */
export function copyDiagnostics(entries: Record<string, string>): string {
  return Object.entries(entries).map(([k, v]) => `${k}=${v}`).join(';');
}

/** F08923 错误文案 i18n 键规范。 */
export function errorI18nKey(code: string): string {
  return `error.${code.toLowerCase()}`;
}

/** F08924 遥测默认关：错误上报需授权。 */
export function errorTelemetryDefault(): { enabled: false; askConsent: true } {
  return { enabled: false, askConsent: true };
}

/** F08925 错误处理规范文档。 */
export const ERROR_SPEC = ['分级四档', '先让位再提示', '重试退避', '人话文案', '遥测默认关'] as const;

/* ===================== 族0358 空态与引导（F08926~F08950） ===================== */

export interface EmptyStateSpec { key: string; title: string; hint: string; action: string }

/** F08926 空态目录：10 个标准空态。 */
export const EMPTY_STATE_CATALOG: EmptyStateSpec[] = [
  { key: 'no-items', title: '这里还是空的', hint: '创建第一项开始使用', action: '新建' },
  { key: 'no-results', title: '没有匹配结果', hint: '换个关键词或清除筛选', action: '清除筛选' },
  { key: 'no-network', title: '网络不可用', hint: '恢复连接后自动刷新', action: '重试' },
  { key: 'no-permission', title: '没有访问权限', hint: '请联系所有者授权', action: '申请权限' },
  { key: 'all-done', title: '全部完成', hint: '今天的事都办完了', action: '查看归档' },
  { key: 'empty-trash', title: '回收站是空的', hint: '删除的内容会出现在这里', action: '返回' },
  { key: 'no-notifications', title: '没有新通知', hint: '有消息时会在这里提醒', action: '设置通知' },
  { key: 'no-downloads', title: '没有下载任务', hint: '添加任务开始下载', action: '新建下载' },
  { key: 'no-widgets', title: '还没有微件', hint: '添加微件装点桌面', action: '添加微件' },
  { key: 'no-history', title: '暂无历史记录', hint: '操作记录会保存在这里', action: '开始使用' },
];

/** F08927 插画位：空态插画槽。 */
export function emptyIllustration(key: string): string {
  return `illustration:${key}`;
}

/** F08928 空态主操作唯一：一个主按钮。 */
export function emptyPrimaryAction(spec: EmptyStateSpec): string {
  return spec.action;
}

/** F08929 搜索空态建议。 */
export function emptySearchSuggestions(query: string): string[] {
  return ['清除筛选', '检查拼写', `搜索"${query}"相关`];
}

/** F08930 引导步骤：首用教学 5 步。 */
export const ONBOARDING_STEPS = ['认识桌面', '打开应用', '管理窗口', '搜索一切', '个性化外观'] as const;

/** F08931 气泡引导：coach mark 定位。 */
export function coachMark(anchor: string, text: string): { anchor: string; text: string } {
  return { anchor, text };
}

/** F08932 首用高亮：目标元素描边。 */
export function firstUseHighlight(target: string): string {
  return `highlight:${target}`;
}

/** F08933 可关闭提示：记忆已读。 */
export function dismissibleHint(id: string, dismissed: string[]): boolean {
  return !dismissed.includes(id);
}

/** F08934 功能发现徽章：新功能角标。 */
export function featureBadge(isNew: boolean, seen: boolean): 'new' | null {
  return isNew && !seen ? 'new' : null;
}

/** F08935 键盘提示卡：快捷键速记。 */
export function keyboardHintCard(shortcuts: Array<[string, string]>): string[] {
  return shortcuts.map(([k, d]) => `${k} — ${d}`);
}

/** F08936 渐进披露：先少后多。 */
export function progressiveDisclosure(all: string[], initial = 5): { shown: string[]; more: number } {
  return { shown: all.slice(0, initial), more: Math.max(0, all.length - initial) };
}

/** F08937 示例数据播种：可一键填充演示。 */
export function sampleDataSeed(kind: 'notes' | 'tasks' | 'files'): string[] {
  return kind === 'notes' ? ['示例笔记 A', '示例笔记 B'] : kind === 'tasks' ? ['示例任务'] : ['示例文件.txt'];
}

/** F08938 空回收站状态。 */
export const EMPTY_TRASH = EMPTY_STATE_CATALOG[5]!;

/** F08939 空通知状态。 */
export const EMPTY_NOTIFICATIONS = EMPTY_STATE_CATALOG[6]!;

/** F08940 空下载状态。 */
export const EMPTY_DOWNLOADS = EMPTY_STATE_CATALOG[7]!;

/** F08941 空微件状态。 */
export const EMPTY_WIDGETS = EMPTY_STATE_CATALOG[8]!;

/** F08942 引导完成追踪。 */
export class TourTracker {
  private done = new Set<string>();
  complete(step: string) { this.done.add(step); }
  isComplete(step: string) { return this.done.has(step); }
  progress(total: number) { return this.done.size / total; }
}

/** F08943 引导跳过：随时可退。 */
export function tourSkip(enabled: boolean): { skip: boolean; remember: boolean } {
  return { skip: enabled, remember: enabled };
}

/** F08944 引导重播：设置里可再看。 */
export function tourReplay(): boolean {
  return true;
}

/** F08945 帮助链接位置：空态右上。 */
export function helpLinkPlacement(): 'top-right' {
  return 'top-right';
}

/** F08946 引导本地化：不出站。 */
export const TOUR_LOCAL_ONLY = true;

/** F08947 引导节奏：每步一停。 */
export function tourPace(steps: number, current: number): 'step' | 'done' {
  return current >= steps - 1 ? 'done' : 'step';
}

/** F08948 空态图示统一风格：线性插画。 */
export const EMPTY_ILLUSTRATION_STYLE = 'line-art';

/** F08949 引导无障碍：读屏可导航。 */
export function tourA11y(step: number, total: number): string {
  return `第 ${step + 1} 步，共 ${total} 步`;
}

/** F08950 空态规范文档。 */
export const EMPTY_STATE_SPEC_DOCS = ['一图一话一操作', '空态≠错误', '引导可跳过', '全部本地'] as const;

/* ===================== 族0359 信息密度优化（F08951~F08975） ===================== */

export type Density = 'compact' | 'comfortable' | 'spacious';

/** F08951 密度三档：行高映射。 */
export function densityTokens(d: Density): { row: number; pad: number; font: number } {
  return d === 'compact' ? { row: 32, pad: 4, font: 12 } : d === 'comfortable' ? { row: 40, pad: 8, font: 14 } : { row: 48, pad: 12, font: 14 };
}

/** F08952 表格密度：紧凑模式行距。 */
export function tableDensity(d: Density): number {
  return densityTokens(d).row;
}

/** F08953 列表内边距令牌。 */
export function listPadding(d: Density): string {
  return `padding:${densityTokens(d).pad}px`;
}

/** F08954 字号随密度：紧凑 12px。 */
export function densityFont(d: Density): number {
  return densityTokens(d).font;
}

/** F08955 图标随密度：16/20/24。 */
export function densityIconSize(d: Density): 16 | 20 | 24 {
  return d === 'compact' ? 16 : d === 'spacious' ? 24 : 20;
}

/** F08956 视口自适应密度：小屏自动紧凑。 */
export function adaptiveDensity(w: number): Density {
  return w < 720 ? 'compact' : w < 1200 ? 'comfortable' : 'spacious';
}

/** F08957 留白比例：内容/留白评估。 */
export function whitespaceRatio(contentPx: number, totalPx: number): number {
  return totalPx === 0 ? 0 : Math.round(((totalPx - contentPx) / totalPx) * 100) / 100;
}

/** F08958 内容最大宽：1000px 上限。 */
export const CONTENT_MAX_WIDTH = 1000;

/** F08959 行长守卫：CJK 25~40 字。 */
export function lineLengthGuard(chars: number): 'ok' | 'too-long' | 'too-short' {
  if (chars > 40) return 'too-long';
  if (chars < 25) return 'too-short';
  return 'ok';
}

/** F08960 长列表分块：200 条一页。 */
export function chunkList<T>(items: T[], size = 200): T[][] {
  const out: T[][] = [];
  for (let i = 0; i < items.length; i += size) out.push(items.slice(i, i + size));
  return out;
}

/** F08961 可折叠分区：默认展开第一段。 */
export function collapsibleSections(names: string[]): Array<{ name: string; open: boolean }> {
  return names.map((n, i) => ({ name: n, open: i === 0 }));
}

/** F08962 先展示 5 条 + 展开。 */
export function showFiveThenExpand<T>(items: T[]): { shown: T[]; rest: T[] } {
  return { shown: items.slice(0, 5), rest: items.slice(5) };
}

/** F08963 表格数字等宽：tabular-nums。 */
export const TABULAR_NUMS = 'font-variant-numeric:tabular-nums';

/** F08964 路径省略：中段省略。 */
export function middleEllipsis(path: string, max = 40): string {
  if (path.length <= max) return path;
  const head = Math.ceil((max - 1) / 2); const tail = Math.floor((max - 1) / 2);
  return `${path.slice(0, head)}…${path.slice(path.length - tail)}`;
}

/** F08965 层级靠字重：粗细优先于字号。 */
export const HIERARCHY_RULE = 'font-weight-first';

/** F08966 间距刻度审计：越界值报告。 */
export function spacingAudit(values: number[]): number[] {
  const scale = [0, 4, 8, 12, 16, 20, 24, 32, 40, 48];
  return values.filter((v) => !scale.includes(v));
}

/** F08967 面板折叠记忆。 */
export class PanelMemory {
  private map = new Map<string, boolean>();
  set(id: string, collapsed: boolean) { this.map.set(id, collapsed); }
  collapsed(id: string) { return this.map.get(id) ?? false; }
}

/** F08968 网格/列表密度切换。 */
export function densityViewToggle(view: 'grid' | 'list'): 'grid' | 'list' {
  return view === 'grid' ? 'list' : 'grid';
}

/** F08969 打印密度：紧凑省纸。 */
export function printDensity(): Density {
  return 'compact';
}

/** F08970 按应用覆盖密度。 */
export class AppDensityOverride {
  private map = new Map<string, Density>();
  set(app: string, d: Density) { this.map.set(app, d); }
  of(app: string, fallback: Density): Density {
    return this.map.get(app) ?? fallback;
  }
}

/** F08971 触控目标不随密度缩小：≥44px 保底。 */
export function minTouchTarget(d: Density, targetPx: number): boolean {
  return densityTokens(d).row >= 32 && targetPx >= 44;
}

/** F08972 密度令牌接线：CSS 变量输出。 */
export function densityCssVars(d: Density): string {
  const t = densityTokens(d);
  return `--row:${t.row}px;--pad:${t.pad}px;--font:${t.font}px;`;
}

/** F08973 密度预览：切换前可预览。 */
export function densityPreview(current: Density, next: Density): { from: Density; to: Density; live: boolean } {
  return { from: current, to: next, live: true };
}

/** F08974 密度规范文档。 */
export const DENSITY_SPEC = ['三档', '触控保底 44px', '间距走刻度', '数字等宽'] as const;

/** F08975 密度一致性评分。 */
export function densityScore(rows: number[], d: Density): number {
  const target = densityTokens(d).row;
  const ok = rows.filter((r) => Math.abs(r - target) <= 2).length;
  return rows.length === 0 ? 100 : Math.round((ok / rows.length) * 100);
}

/* ===================== 族0360 交互反馈强化（F08976~F09000） ===================== */

/** F08976 按压涟漪：位置与扩散。 */
export function pressRipple(x: number, y: number, maxR = 80): { x: number; y: number; r: number } {
  return { x, y, r: maxR };
}

/** F08977 悬停抬升： translateY(-1px)。 */
export const HOVER_LIFT = 'translateY(-1px)';

/** F08978 焦点环常显：键盘可见。 */
export const FOCUS_RING_VISIBLE = '2px solid var(--aurora-accent)';

/** F08979 成功对勾描画：路径动画。 */
export function successCheckmark(strokeLen: number): { dasharray: number; animate: boolean } {
  return { dasharray: strokeLen, animate: true };
}

/** F08980 错误抖动：左右两下。 */
export function errorShake(): number[] {
  return [-4, 4, -2, 0];
}

/** F08981 触觉模式：短/长/双击。 */
export type HapticPattern = 'tick' | 'long' | 'double';
export function hapticPattern(kind: HapticPattern): number[] {
  return kind === 'tick' ? [10] : kind === 'long' ? [50] : [10, 40, 10];
}

/** F08982 声音反馈：静音时静默。 */
export function soundFeedback(muted: boolean): 'tick' | 'silent' {
  return muted ? 'silent' : 'tick';
}

/** F08983 乐观更新：先显示后确认。 */
export class OptimisticUi<T> {
  private pending = new Map<string, T>();
  apply(id: string, optimistic: T, actual: T, ok: boolean): T {
    if (ok) { this.pending.delete(id); return actual; }
    this.pending.set(id, optimistic);
    return actual;
  }
  rollback(id: string, previous: T): T { this.pending.delete(id); return previous; }
}

/** F08984 待办→完成状态迁移。 */
export function actionState(step: 'pending' | 'running' | 'done' | 'failed'): string {
  return step;
}

/** F08985 数字滚动：计数动画。 */
export function countUp(from: number, to: number, t: number): number {
  const p = Math.min(1, Math.max(0, t));
  return Math.round(from + (to - from) * p);
}

/** F08986 开关滑块弹簧：过冲回弹。 */
export function toggleSpring(t: number): number {
  const p = Math.min(1, Math.max(0, t));
  return 1 + 0.08 * Math.sin(p * Math.PI) * (1 - p);
}

/** F08987 拖拽幽灵半透。 */
export const DRAG_GHOST_OPACITY = 0.6;

/** F08988 放置目标高亮。 */
export function dropTargetHighlight(hovering: boolean): string {
  return hovering ? 'outline:2px dashed var(--aurora-accent)' : '';
}

/** F08989 复制成功闪示：1s 恢复。 */
export function copyFlash(now: number, copiedAt: number): boolean {
  return now - copiedAt < 1000;
}

/** F08990 已保存指示：静默呈现。 */
export function saveIndicator(state: 'dirty' | 'saving' | 'saved'): string {
  return state === 'saved' ? '已保存' : state === 'saving' ? '保存中…' : '未保存';
}

/** F08991 操作进度反馈。 */
export function actionProgress(done: number, total: number): string {
  return `${done}/${total}`;
}

/** F08992 撤销 Toast：5s 窗口。 */
export function undoToast(now: number, actionAt: number, windowMs = 5000): boolean {
  return now - actionAt < windowMs;
}

/** F08993 按钮冷却：防连点。 */
export function buttonCooldown(lastAt: number, now: number, cdMs = 500): boolean {
  return now - lastAt < cdMs;
}

/** F08994 长按进度环。 */
export function longPressProgress(heldMs: number, needMs = 600): number {
  return Math.min(1, heldMs / needMs);
}

/** F08995 下拉刷新吸附。 */
export function pullSnap(released: boolean, armed: boolean): 'snap-back' | 'refresh' {
  return released && armed ? 'refresh' : 'snap-back';
}

/** F08996 指针审计：可点元素 cursor:pointer。 */
export function cursorAudit(clickable: boolean, cursor: string): string[] {
  return clickable && cursor !== 'pointer' ? ['clickable-missing-pointer'] : [];
}

/** F08997 键盘回声：按键操作有反馈。 */
export function keyboardEcho(pressed: boolean): 'flash' | 'none' {
  return pressed ? 'flash' : 'none';
}

/** F08998 读屏实时区：aria-live。 */
export function liveRegion(polite: boolean): string {
  return `aria-live=${polite ? 'polite' : 'assertive'}`;
}

/** F08999 减动效回退：prefers-reduced-motion。 */
export function reducedMotionFallback(prefersReduced: boolean): { animate: boolean; durationMs: number } {
  return prefersReduced ? { animate: false, durationMs: 0 } : { animate: true, durationMs: 200 };
}

/** F09000 反馈令牌接线。 */
export const FEEDBACK_TOKENS = { press: 100, hover: 150, settle: 200, undo: 5000 } as const;
