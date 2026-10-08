// AURORA-10000: AI-41 批次（族0201~0205 · 窗口嵌入探测/反作弊共存/CEF 内核兼容/独占与全屏让位/老应用兼容），勿删。

/** FNV-1a 32 位哈希（领域内指纹/聚类用）。 */
export function fnv1a(text: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0).toString(16).padStart(8, '0');
}

/* ============ 族0201 窗口嵌入探测（F05001~F05025） ============ */

export interface HostWindow {
  id: number;
  className: string;
  title: string;
  processName: string;
  isSystem?: boolean;
}

/** F05001/F05002 实时枚举 + 类名预判。 */
export function probeWindows(wins: HostWindow[], classLib: Map<string, 'embeddable' | 'forbidden'>): HostWindow[] {
  return wins.filter((w) => !w.isSystem && classLib.get(w.className) !== 'forbidden');
}

/** F05003/F05004 黑白名单裁决：黑名单禁嵌 > 白名单直嵌 > 类名预判。 */
export function embedVerdict(w: HostWindow, blacklist: Set<string>, whitelist: Set<string>, classLib: Map<string, 'embeddable' | 'forbidden'>): 'embed' | 'reject' | 'ask' {
  if (blacklist.has(w.className) || w.isSystem) return 'reject';
  if (whitelist.has(w.processName)) return 'embed';
  return classLib.get(w.className) === 'embeddable' ? 'embed' : 'ask';
}

/** F05005/F05006 双击/拖拽收编入口。 */
export type EmbedTrigger = 'double-click' | 'drag' | 'hotkey';
export function embedTriggerAction(trigger: EmbedTrigger): { target: string; animated: boolean } {
  return { target: 'desktop-host', animated: trigger !== 'hotkey' };
}

/** F05007~F05009 收编动画/失败卡/回退。 */
export interface EmbedAttempt {
  windowId: number;
  state: 'animating' | 'embedded' | 'failed';
  reason?: string;
}
export function embedAttempt(win: HostWindow, ok: boolean, reason = 'unsupported-window-class'): EmbedAttempt {
  return ok ? { windowId: win.id, state: 'embedded' } : { windowId: win.id, state: 'failed', reason };
}
/** 失败回退独立窗。 */
export function rollbackEmbed(a: EmbedAttempt): EmbedAttempt {
  return { ...a, state: 'failed', reason: a.reason ?? 'rolled-back' };
}

/** F05010/F05011 嵌入尺寸自适应 + DPI 重排。 */
export function fitEmbedRect(host: { w: number; h: number }, dpi: number, prevDpi = 96): { w: number; h: number } {
  const scale = dpi / prevDpi;
  return { w: Math.round(host.w * scale), h: Math.round(host.h * scale) };
}

/** F05012/F05013 应用主题色识别 + 深色注入。 */
export function detectAppTheme(bgRgb: [number, number, number]): { theme: 'dark' | 'light'; suggestDarkInjection: boolean } {
  const [r, g, b] = bgRgb;
  const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  const dark = lum < 128;
  return { theme: dark ? 'dark' : 'light', suggestDarkInjection: !dark };
}

/** F05014~F05017 圆角/阴影/标题栏/图标统一。 */
export const EMBED_STYLE_DEFAULTS = {
  cornerRadius: 8,
  shadow: 'elevation-2',
  redrawTitlebar: false,
  unifyIcon: true,
} as const;

/** F05018 进程名友好显示。 */
const FRIENDLY_NAMES: Record<string, string> = { ['WeChat.exe']: '微信', ['QQ.exe']: 'QQ', ['steam.exe']: 'Steam', ['Code.exe']: 'VS Code' };
export function friendlyProcessName(proc: string): string {
  return FRIENDLY_NAMES[proc] ?? proc.replace(/\.exe$/i, '');
}

/** F05019~F05022 多实例/子窗定位/弹窗跟随/模态捕获。 */
export class EmbedRegistry {
  private embedded = new Map<number, { owner: number; kind: 'main' | 'child' | 'popup' | 'modal' }>();
  private mainWindows = new Set<number>();
  embed(id: number, owner: number | null, kind: 'main' | 'child' | 'popup' | 'modal' = 'main'): boolean {
    if (kind === 'main' && owner !== null) return false;
    if (kind === 'popup' && owner === null) return false;
    this.embedded.set(id, { owner: owner ?? 0, kind });
    if (kind === 'main') this.mainWindows.add(owner ?? id);
    return true;
  }
  isEmbedded(id: number): boolean {
    return this.embedded.has(id);
  }
  /** 弹窗跟随：找 owner。 */
  ownerOf(id: number): number | undefined {
    return this.embedded.get(id)?.owner;
  }
  /** F05024 去重显示：同一主窗的子窗只算一次。 */
  taskbarEntries(): number[] {
    const seen = new Set<number>();
    for (const [id, v] of this.embedded) {
      if (v.kind === 'main') seen.add(id);
    }
    for (const id of this.mainWindows) if (!this.embedded.has(id)) seen.add(id);
    return [...seen];
  }
}

/** F05023 托盘收编登记。 */
export class TrayAdoption {
  private items = new Map<string, { visible: boolean; friendly: string }>();
  adopt(process: string): boolean {
    if (this.items.has(process)) return false;
    this.items.set(process, { visible: true, friendly: friendlyProcessName(process) });
    return true;
  }
  remove(process: string): boolean {
    return this.items.delete(process);
  }
  get list(): string[] {
    return [...this.items.keys()];
  }
}

/** F05025 教学。 */
export function embedTutorial(): string[] {
  return ['双击任务栏窗口即可收编', '拖动窗口到桌面边缘可收编', '收编失败会退回独立窗，原因见失败卡'];
}

/* ============ 族0202 反作弊共存（F05026~F05050） ============ */

export type AntiCheatKind = 'EAC' | 'BattlEye' | 'Vanguard' | 'nProtect';

/** F05026~F05029 检测：按进程/驱动名识别反作弊。 */
export const ANTI_CHEAT_SIGNATURES: Record<AntiCheatKind, RegExp> = {
  EAC: /EasyAntiCheat(_x64)?\.sys|EasyAntiCheat\.exe/i,
  BattlEye: /BEService(\.exe|\.sys)|BEDaisy\.sys/i,
  Vanguard: /vgk(\.sys)?|vgtray\.exe|VGC/i,
  nProtect: /GameMon\.exe|GG-Aegis|nProtect/i,
};
export function detectAntiCheat(processes: string[]): AntiCheatKind[] {
  const found = new Set<AntiCheatKind>();
  for (const p of processes) for (const [k, re] of Object.entries(ANTI_CHEAT_SIGNATURES)) if (re.test(p)) found.add(k as AntiCheatKind);
  return [...found];
}

/** F05030~F05032 让位状态机：检测→退出桌面→独占尊重→退出后回归。 */
export type YieldPhase = 'normal' | 'yielding' | 'yielded' | 'restoring';
export class AntiCheatYield {
  phase: YieldPhase = 'normal';
  private reasons: string[] = [];
  onDetected(kind: AntiCheatKind): boolean {
    if (this.phase !== 'normal') return false;
    this.phase = 'yielding';
    this.reasons.push(kind);
    return true;
  }
  settle(): boolean {
    if (this.phase !== 'yielding') return false;
    this.phase = 'yielded';
    return true;
  }
  onGameExit(): boolean {
    if (this.phase !== 'yielded') return false;
    this.phase = 'restoring';
    this.reasons = [];
    return true;
  }
  restoreDone(): boolean {
    if (this.phase !== 'restoring') return false;
    this.phase = 'normal';
    return true;
  }
  /** 读取当前阶段（避免调用方对可变属性的窄化误判）。 */
  phaseNow(): YieldPhase {
    return this.phase;
  }
  get detected(): AntiCheatKind[] {
    return this.reasons as AntiCheatKind[];
  }
}

/** F05033~F05037 全屏期建议与禁用集合。 */
export interface GameModeFlags {
  fullscreenHint: boolean;
  overlaysDisabled: boolean;
  fpsOverlayOff: boolean;
  screenshotProtected: boolean;
  recordingProtected: boolean;
}
export function gameModeFlags(cheats: AntiCheatKind[], hasExclusive: boolean): GameModeFlags {
  const active = cheats.length > 0 || hasExclusive;
  return {
    fullscreenHint: active && !hasExclusive,
    overlaysDisabled: active,
    fpsOverlayOff: active,
    screenshotProtected: active,
    recordingProtected: active,
  };
}

/** F05038/F05040 注入禁用与内核承诺。 */
export const INJECTION_POLICY = { injectGameProcess: false, kernelTouchGame: false } as const;

/** F05039 内核检测感知（只读感知通道）。 */
export function kernelPerception(driverNames: string[]): { known: AntiCheatKind[]; readOnly: true } {
  return { known: detectAntiCheat(driverNames), readOnly: true };
}

/** F05041~F05043 白名单库/误判日志/启动预检。 */
export class AntiCheatLedger {
  private verified = new Set<string>();
  private misjudgments: { process: string; time: number; note: string }[] = [];
  verifyGame(game: string): boolean {
    if (this.verified.has(game)) return false;
    this.verified.add(game);
    return true;
  }
  isVerified(game: string): boolean {
    return this.verified.has(game);
  }
  logMisjudgment(process: string, time: number, note = 'desktop-terminated-by-mistake'): void {
    this.misjudgments.push({ process, time, note });
  }
  get misjudgmentCount(): number {
    return this.misjudgments.length;
  }
  /** 启动预检：返回放行或需要让位。 */
  preflight(game: string, processes: string[]): { proceed: boolean; yields: AntiCheatKind[] } {
    const yields = detectAntiCheat(processes);
    return { proceed: yields.length === 0 && this.isVerified(game), yields };
  }
}

/** F05044~F05047 崩溃联动/帧率保护/暂停壁纸动效。 */
export function gameCrashReport(game: string, exitCode: number): { game: string; code: number; severity: 'ok' | 'crash' | 'hang' } {
  return { game, code: exitCode, severity: exitCode === 0 ? 'ok' : exitCode < 0 ? 'crash' : 'hang' };
}
export function embedFpsBudget(embeddedCount: number, baseFps = 60): number {
  return Math.max(30, baseFps - Math.max(0, embeddedCount - 1) * 5);
}
export interface WallpaperState { running: boolean; animations: boolean }
export function suspendForGame(_state: WallpaperState): WallpaperState {
  return { running: false, animations: false };
}

/** F05048/F05049 策略编辑 + 知识库。 */
export interface YieldPolicy {
  autoYield: boolean;
  pauseWallpaper: boolean;
  pauseEffects: boolean;
  silentNotify: boolean;
}
export const DEFAULT_YIELD_POLICY: YieldPolicy = { autoYield: true, pauseWallpaper: true, pauseEffects: true, silentNotify: true };
export const ANTI_CHEAT_KB: { kind: AntiCheatKind; note: string }[] = [
  { kind: 'EAC', note: 'Easy AntiCheat：启动后桌面自动让位，退出后回归' },
  { kind: 'BattlEye', note: 'BattlEye：检测到 BEService 即退避，不注入游戏' },
  { kind: 'Vanguard', note: 'Vanguard：内核级，随系统启动即让位' },
  { kind: 'nProtect', note: 'nProtect GameMon：让位并停用截图' },
];
export function antiCheatTutorial(): string[] {
  return ['让位优先：检测到反作弊先退出桌面再提示', '可在策略编辑中关闭自动让位', '误判会记录到日志并进入白名单流程'];
}

/* ============ 族0203 CEF 内核兼容（F05051~F05075） ============ */

/** F05051/F05052 崩溃防护 + 专用通道。 */
export interface CefCrashEvent { kind: 'breakpoint-0x80000003' | 'render-hang' | 'gpu-lost'; handled: boolean }
export function cefCrashGuard(exitCode: number): CefCrashEvent {
  return { kind: 'breakpoint-0x80000003', handled: exitCode === 0x80000003 };
}

export type CefChannel = 'shared' | 'dedicated';
export interface CefApp {
  name: string;
  channel?: CefChannel;
  multiProcess?: boolean;
  gpuIsolated?: boolean;
  version?: string;
}
export function cefChannelFor(app: CefApp): CefChannel {
  return app.channel ?? (app.name.toLowerCase().includes('steam') ? 'dedicated' : 'shared');
}

/** F05053/F05054 多进程适配 + GPU 隔离。 */
export function cefProcessLayout(app: CefApp, cores = 4): { children: string[]; gpuIsolated: boolean } {
  const children = app.multiProcess ? ['browser', 'render', 'gpu', 'utility'].slice(0, Math.min(4, cores)) : ['browser'];
  return { children, gpuIsolated: app.gpuIsolated ?? app.multiProcess === true };
}

/** F05055/F05056 置顶取消 + 低负载联动。 */
export function cefTopMostArbitration(appTopMost: boolean, gameExclusive: boolean): { keepTopMost: boolean; reason: string } {
  return gameExclusive ? { keepTopMost: false, reason: 'exclusive-fullscreen-wins' } : { keepTopMost: appTopMost, reason: 'no-conflict' };
}
export function wallpaperLoadForCef(cefRunning: boolean): 'full' | 'low' | 'paused' {
  return cefRunning ? 'low' : 'full';
}

/** F05057~F05062 应用收编识别库。 */
const CEF_KNOWN: Record<string, string> = {
  steam: 'Steam', epicgameslauncher: 'Epic', discord: 'Discord',
  cloudmusic: '网易云音乐', ['QQMusic']: 'QQ 音乐', kugou: '酷狗',
  bilibili: 'B 站', iqiyi: '爱奇艺',
};
export function knownCefApp(processName: string): string | undefined {
  const key = processName.replace(/\.exe$/i, '');
  return CEF_KNOWN[key] ?? CEF_KNOWN[key.toLowerCase()];
}

/** F05063~F05065 白屏/卡死检测 + 降级。 */
export function cefHealthCheck(paintMs: number, frameGapMs: number, whiteThresholdMs = 3000, hangThresholdMs = 5000): 'ok' | 'white' | 'hang' {
  if (paintMs >= whiteThresholdMs) return 'white';
  if (frameGapMs >= hangThresholdMs) return 'hang';
  return 'ok';
}
export function cefDegradePath(health: ReturnType<typeof cefHealthCheck>): 'reload' | 'restart-renderer' | 'software-render' | 'none' {
  if (health === 'white') return 'reload';
  if (health === 'hang') return 'restart-renderer';
  return 'none';
}

/** F05066~F05069 GPU 协商/透明/无边框/层级。 */
export function cefGpuNegotiation(hasHwAccel: boolean, transparent: boolean): { mode: 'hardware' | 'software'; transparentOk: boolean } {
  return { mode: hasHwAccel ? 'hardware' : 'software', transparentOk: hasHwAccel || !transparent };
}
export type CefWindowStyle = 'framed' | 'frameless' | 'layered';
export function cefLayerPolicy(style: CefWindowStyle, gameExclusive: boolean): { zOrder: 'top' | 'normal'; frameless: boolean } {
  return { zOrder: gameExclusive ? 'normal' : 'top', frameless: style !== 'framed' };
}

/** F05070/F05071 多标签/内存配额。 */
export class CefTabGroup {
  private tabs: string[] = [];
  addTab(id: string): boolean {
    if (this.tabs.includes(id)) return false;
    this.tabs.push(id);
    return true;
  }
  closeTab(id: string): boolean {
    const i = this.tabs.indexOf(id);
    if (i < 0) return false;
    this.tabs.splice(i, 1);
    return true;
  }
  get count(): number {
    return this.tabs.length;
  }
}
export function cefMemoryQuota(tabs: number, perTabMB = 128, capMB = 1024): { budgetMB: number; over: boolean } {
  const budget = Math.min(tabs * perTabMB, capMB);
  return { budgetMB: budget, over: tabs * perTabMB > capMB };
}

/** F05072/F05073 崩溃自愈 + 版本库。 */
export function cefSelfHeal(crashesInWindow: number, max = 3): 'restart' | 'safe-mode' | 'give-up' {
  return crashesInWindow < max ? 'restart' : crashesInWindow === max ? 'safe-mode' : 'give-up';
}
export const CEF_VERSION_KB = ['cef-120', 'cef-118', 'cef-109', 'cef-103'] as const;
export function cefVersionSupported(v: string): boolean {
  return (CEF_VERSION_KB as readonly string[]).includes(v);
}

/** F05074/F05075 报告 + 教学。 */
export function cefCompatReport(apps: CefApp[]): { app: string; channel: CefChannel; ok: boolean }[] {
  return apps.map((a) => ({ app: a.name, channel: cefChannelFor(a), ok: !a.version || cefVersionSupported(a.version) }));
}
export function cefTutorial(): string[] {
  return ['Steam/音乐软件自动走专用通道', '白屏自动刷新，卡死重启渲染进程', '连续崩溃三次进入安全模式'];
}

/* ============ 族0204 独占与全屏让位（F05076~F05100） ============ */

export interface GameSession {
  game: string;
  exclusive: boolean;
  borderless: boolean;
  resolution: { w: number; h: number };
  refreshHz: number;
}

/** F05076/F05077 独占检测 + 自动最小化。 */
export function detectExclusive(session: GameSession): boolean {
  return session.exclusive;
}
export function yieldDesktop(_session: GameSession): { minimized: boolean; taskbarHidden: boolean } {
  return { minimized: true, taskbarHidden: true };
}

/** F05078~F05080 焦点锁定/动画/回归恢复。 */
export class YieldSession {
  private saved: { layout: string } | null = null;
  enter(layout: string): boolean {
    if (this.saved) return false;
    this.saved = { layout };
    return true;
  }
  exit(): string | undefined {
    const l = this.saved?.layout;
    this.saved = null;
    return l;
  }
  get active(): boolean {
    return this.saved !== null;
  }
  /** 让位过渡动画描述。 */
  static transitionMs(exclusive: boolean): number {
    return exclusive ? 0 : 200;
  }
}

/** F05081/F05082 AltTab/Win 键兼容：让行给游戏。 */
export function hotkeyPassThrough(exclusiveGame: boolean, hotkey: 'alt-tab' | 'win' | 'ctrl-shift-esc'): boolean {
  return exclusiveGame && hotkey !== 'ctrl-shift-esc';
}

/** F05083~F05086 全屏期静默/停壁纸/停动效/藏栏。 */
export interface FullscreenPolicy {
  notifySilent: boolean;
  wallpaperPaused: boolean;
  effectsPaused: boolean;
  taskbarHidden: boolean;
}
export function fullscreenPolicy(exclusive: boolean): FullscreenPolicy {
  return { notifySilent: exclusive, wallpaperPaused: exclusive, effectsPaused: exclusive, taskbarHidden: exclusive };
}

/** F05087/F05088 全屏可截/可录。 */
export function captureAllowed(exclusive: boolean, gameForbids: boolean): { screenshot: boolean; recording: boolean } {
  return { screenshot: !gameForbids, recording: !gameForbids || !exclusive };
}

/** F05089~F05092 多游戏/游戏库/启动器/时长统计。 */
export class GameLibrary {
  private games = new Map<string, { launcher: string; playedMs: number }>();
  register(game: string, launcher: string): boolean {
    if (this.games.has(game)) return false;
    this.games.set(game, { launcher, playedMs: 0 });
    return true;
  }
  addPlayTime(game: string, ms: number): void {
    const g = this.games.get(game);
    if (g) g.playedMs += ms;
  }
  playedMs(game: string): number {
    return this.games.get(game)?.playedMs ?? 0;
  }
  byLauncher(launcher: string): string[] {
    return [...this.games.entries()].filter(([, v]) => v.launcher === launcher).map(([k]) => k);
  }
  get count(): number {
    return this.games.size;
  }
}

/** F05093~F05095 窗口化切换/分辨率匹配/刷新率联动。 */
export function displayFollow(session: GameSession, desktop: { w: number; h: number; refreshHz: number }): { matchResolution: boolean; matchRefresh: boolean } {
  return {
    matchResolution: desktop.w === session.resolution.w && desktop.h === session.resolution.h,
    matchRefresh: desktop.refreshHz === session.refreshHz,
  };
}

/** F05096 手柄唤起（模型化：按键组合判断）。 */
export function controllerSummon(buttons: string[]): boolean {
  return buttons.includes('guide') && buttons.includes('b') && buttons.length <= 3;
}

/** F05097/F05098 退出恢复。 */
export function restoreOnExit(before: { wallpaper: boolean; tray: boolean }): { wallpaper: boolean; tray: boolean } {
  return { wallpaper: before.wallpaper, tray: before.tray };
}

/** F05099/F05100 策略编辑 + 教学。 */
export interface YieldRule { pattern: string; policy: YieldPolicy }
export class YieldRuleEditor {
  private rules: YieldRule[] = [];
  add(rule: YieldRule): boolean {
    if (this.rules.some((r) => r.pattern === rule.pattern)) return false;
    this.rules.push(rule);
    return true;
  }
  match(game: string): YieldPolicy | undefined {
    return this.rules.find((r) => new RegExp(r.pattern, 'i').test(game))?.policy;
  }
  get list(): YieldRule[] {
    return [...this.rules];
  }
}
export function yieldTutorial(): string[] {
  return ['独占全屏自动最小化桌面', '退出后布局与壁纸自动恢复', '让位规则可按游戏名正则自定义'];
}

/* ============ 族0205 老应用兼容（F05101~F05125） ============ */

/** F05101/F05102 DPI 修复 + 缩放覆盖。 */
export function dpiFix(appDpiAware: boolean, systemDpi = 144): { override: boolean; mode: 'system' | 'per-monitor' | 'none'; scalePct: number } {
  if (!appDpiAware) return { override: true, mode: 'system', scalePct: Math.round((systemDpi / 96) * 100) };
  return { override: false, mode: 'per-monitor', scalePct: 100 };
}
export function dpiScaleOverride(targetDpi: number): { pct: number } {
  return { pct: Math.round((targetDpi / 96) * 100) };
}

/** F05103/F05104 16 色位模拟/单实例绕过（预留位）。 */
export const LEGACY_COLOR_SIM = { enabled: false, depth: 16 } as const;
export const LEGACY_SINGLE_INSTANCE = { bypass: false } as const;

/** F05105/F05106 管理员检测 + UAC 虚拟化。 */
export function adminNeed(manifest: { requestedExecutionLevel?: 'asInvoker' | 'highestAvailable' | 'requireAdministrator' }): { needsAdmin: boolean; uacVirtualized: boolean } {
  const lvl = manifest.requestedExecutionLevel ?? 'asInvoker';
  return { needsAdmin: lvl === 'requireAdministrator', uacVirtualized: lvl === 'asInvoker' };
}

/** F05107/F05108 兼容模式 + 旧系统提示。 */
export interface CompatMode {
  os: 'win7' | 'win8' | 'winxp' | 'none';
  reducedColors: boolean;
  disableFullscreenOptimizations: boolean;
  runAsAdmin: boolean;
}
export const COMPAT_MODE_DEFAULT: CompatMode = { os: 'none', reducedColors: false, disableFullscreenOptimizations: true, runAsAdmin: false };
export function legacyOsSuggest(year: number): CompatMode['os'] {
  if (year <= 2009) return 'winxp';
  if (year <= 2012) return 'win7';
  if (year <= 2015) return 'win8';
  return 'none';
}

/** F05109~F05111 中文路径/空格路径/长路径检测。 */
export interface PathIssue { kind: 'non-ascii' | 'space' | 'long'; path: string }
export function pathIssues(path: string): PathIssue[] {
  const out: PathIssue[] = [];
  if (/[^\x20-\x7e]/.test(path)) out.push({ kind: 'non-ascii', path });
  if (path.includes(' ')) out.push({ kind: 'space', path });
  if (path.length > 260) out.push({ kind: 'long', path });
  return out;
}

/** F05112/F05113 依赖检测 + 运行库一键位。 */
export const RUNTIME_LIBS = ['msvcr100.dll', 'msvcp140.dll', 'd3dx9_43.dll', 'xinput1_3.dll', 'vcruntime140.dll'] as const;
export function missingDeps(required: string[], present: string[]): string[] {
  return required.filter((r) => !present.includes(r));
}
export function runtimeInstallerCmd(missing: string[]): string {
  return `varix-runtime install ${missing.join(' ')}`.trim();
}

/** F05114/F05115 字体缺失 + 乱码修复。 */
export function missingFonts(needed: string[], installed: string[]): string[] {
  return needed.filter((f) => !installed.includes(f));
}
/** ANSI→Unicode 修复（GBK 双字节模型）。 */
export function mojibakeFixGBK(bytes: number[]): string {
  let out = '';
  for (let i = 0; i < bytes.length; i++) {
    const b = bytes[i]!;
    if (b >= 0x81 && b <= 0xfe && i + 1 < bytes.length && bytes[i + 1]! >= 0x40) {
      out += `U+${(((b << 8) | bytes[i + 1]!) >>> 0).toString(16).toUpperCase().padStart(4, '0')}`;
      i++;
    } else out += String.fromCharCode(b);
  }
  return out;
}

/** F05116/F05117 闪退收集 + 崩溃分析。 */
export interface CrashLog { app: string; time: number; exitCode: number; module?: string }
export class CrashCollector {
  private logs: CrashLog[] = [];
  record(log: CrashLog): void {
    this.logs.push(log);
  }
  analyze(app: string): { crashes: number; suspectedModule?: string } {
    const forApp = this.logs.filter((l) => l.app === app);
    const mod = forApp.filter((l) => l.module).map((l) => l.module!);
    const count = new Map<string, number>();
    for (const m of mod) count.set(m, (count.get(m) ?? 0) + 1);
    const suspected = [...count.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
    return { crashes: forApp.length, suspectedModule: suspected };
  }
}

/** F05118~F05123 各类老应用故障修复策略表。 */
export type LegacySymptom = 'no-sound' | 'flicker' | 'focus-loss' | 'tray-gone' | 'context-menu-dead' | 'clipboard-dead';
export const LEGACY_FIXES: Record<LegacySymptom, string> = {
  'no-sound': '切换 WASAPI 共享模式并重建设备',
  flicker: '禁用全屏优化并关闭硬件加速',
  'focus-loss': '授予前台锁定权限并禁用后台抢焦',
  'tray-gone': '重启 Explorer 钩子并重挂托盘图标',
  'context-menu-dead': '隔离崩溃的第三方 Shell 扩展',
  'clipboard-dead': '重启剪贴板服务并清空监听链',
};
export function legacyFix(symptom: LegacySymptom): string {
  return LEGACY_FIXES[symptom];
}

/** F05124/F05125 知识库 + 教学。 */
export const LEGACY_KB: { app: string; issue: LegacySymptom }[] = [
  { app: '某 2005 年国产播放器', issue: 'no-sound' },
  { app: '老 Delphi 工具', issue: 'flicker' },
  { app: 'Legacy MUD 客户端', issue: 'focus-loss' },
];
export function legacyTutorial(): string[] {
  return ['模糊应用先开 DPI 修复', '闪退先查缺失运行库与依赖 DLL', '中文路径问题优先建议迁移安装目录'];
}
