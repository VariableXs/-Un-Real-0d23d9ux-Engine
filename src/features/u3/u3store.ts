/**
 * I 通用域 · AI-U3 三分队（F501-F550）共用底座。
 *
 * 职责（与 J 鼠标域 j1store.ts 同源纪律，一处一事实）：
 * - 单一配置根：U3 全部配置收在 localStorage `variable:u3:v1` 一个键下，
 *   按 F 编号语义分节存储——F550 锚点自检与导出对账直接逐节取表。
 * - 订阅制总线：set() 写入后逐节广播；运行时/面板按节订阅，不轮询。
 * - 原子写 + 异常零静默：先写内存再落盘；落盘失败抛 U3StoreError，
 *   会话内配置仍生效，重启回退上次成功落盘值。
 * - 可回退：每节写前留一帧快照，undoSection() 一键还原，栈深 3。
 *
 * 边界声明（主册 I-0 · 分工书泳道四）：本底座只管「判据逻辑层」的
 * 开关与参数；内核侧判据实装层在 kernel/varix/src/ustar3/（AI-U3 v1），
 * 指针渲染平面归 F335（AI-J1 领地）、主题令牌归 F151（AI-E1 领地）、
 * 任务栏图标排序归 F418（AI-U1 领地）——均不在此重复。
 */

export const U3_LS_KEY = "variable:u3:v1";
export const U3_FORMAT = "u3-config";
export const U3_VERSION = 1;

/** U3 域全部分节（键名 = F 编号语义，与各模块文件头注释一一对应）。 */
export const U3_SECTIONS = [
  "iconRead",      // F501 桌面图标文字可读性
  "iconWrap",      // F502 图标文字两行封顶
  "gridDensity",   // F503 图标网格密度
  "pinUnlock",     // F504 PIN 快速解锁
  "btLock",        // F505 蓝牙动态锁
  "guestMode",     // F506 访客模式
  "lockShield",    // F507 锁屏防截图
  "appShield",     // F508 应用防截标记
  "shred",         // F509 文件粉碎
  "oneCrypt",      // F510 单文件加密
  "clipWipe",      // F511 剪贴板一键清空
  "shotHistory",   // F512 截图历史
  "ctrlFind",      // F513 Ctrl 定位指针
  "soundLight",    // F514 声音视觉提示
  "findReplace",   // F515 查找与替换
  "bannerPos",     // F516 通知横幅位置
  "explorerHome",  // F517 资源管理器启动页
  "imeToggle",     // F518 输入法切换键
  "capsSound",     // F519 大写锁定提示音
  "midMinimize",   // F520 标题栏中键最小化
  "shotTarget",    // F521 截图保存位置
  "pointerTrail",  // F522 指针轨迹显示
  "typeHide",      // F523 打字时隐藏指针
  "undoEmptyBin",  // F524 撤销清空回收站
  "keycardExport", // F525 快捷键速查卡导出
  "statusBar",     // F526 资源管理器状态栏
  "treeCollapse",  // F527 导航树折叠展开
  "treeSync",      // F528 树与列表双向同步
  "spaceCheck",    // F529 复制前空间预检
  "copyVerify",    // F530 复制后校验
  "copyQueue",     // F531 复制任务队列化
  "openDiagnose",  // F532 打开失败人话诊断
  "roRemind",      // F533 只读介质提醒
  "longPath",      // F534 长路径全程支持
  "winNumber",     // F535 Win+数字快捷启动
  "winT",          // F536 Win+T 任务栏遍历
  "peekDesk",      // F537 Win+逗号 瞥桌面
  "altEsc",        // F538 Alt+Esc 窗口循环
  "layoutLock",    // F539 桌面布局锁定
  "memDiag",       // F540 内存诊断
  "netReset",      // F541 网络重置
  "clickLock",     // F542 ClickLock 拖拽锁定
  "devVolume",     // F543 分设备音量记忆
  "notifyVolume",  // F544 通知音量独立分级
  "btBattery",     // F545 蓝牙耳机电量显示
  "deviceNotify",  // F546 新设备接入通知
  "balance",       // F547 音量左右平衡
  "taskmgrTop",    // F548 任务管理器置顶
  "clockHover",    // F549 时钟悬停完整日期
  "anchorB6",      // F550 I 域批次六验收锚点
] as const;

export type U3Section = (typeof U3_SECTIONS)[number];
export type U3Config = Record<U3Section, Record<string, unknown>>;

export class U3StoreError extends Error {
  readonly section: string;
  constructor(section: string, message: string) {
    super(`[u3:${section}] ${message}`);
    this.name = "U3StoreError";
    this.section = section;
  }
}

type Listener = (section: U3Section, next: Record<string, unknown>, prev: Record<string, unknown>) => void;

/** 回退栈深 3（E 域/J1 同规格，一处一事实）。 */
export const U3_UNDO_STACK_DEPTH = 3;

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function clone<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}

/** 各分节默认值集中登记处——面板「恢复默认」与运行时兜底共用同一份（单一事实源）。 */
export const U3_DEFAULTS: U3Config = {
  iconRead:      { mode: "auto", shadowOpacity: 0.4, shadowBlurPx: 2, selectedPill: true },
  iconWrap:      { maxLines: 2, charsPerLine: 8, centerAlign: true },
  gridDensity:   { density: "standard", customColPx: 80, customRowPx: 80, stepPx: 8 },
  pinUnlock:     { enabled: false, pin: "", length: 4, failCount: 0, coolUntil: 0 },
  btLock:        { enabled: false, keyDeviceId: "", awaySeconds: 30 },
  guestMode:     { entryOpen: true, maxMinutes: 120, sandbox: true },
  lockShield:    { mode: "black-frame" },
  appShield:     { markers: {} },
  shred:         { passes: 3, honestLabel: true },
  oneCrypt:      { keepOriginal: false, iterMs: 200000 },
  clipWipe:      { hotkey: "Ctrl+Shift+Delete", afterSecretPromptMs: 5000 },
  shotHistory:   { cap: 20, purgeOnShutdown: true, notifiedOnce: false },
  ctrlFind:      { enabled: true, holdMs: 1000, ripples: 3, totalMs: 1500 },
  soundLight:    { notify: true, warn: true, battery: true, edgePx: 8, pulses: 2 },
  findReplace:   { matchCase: false, wholeWord: false, previewCount: true },
  bannerPos:     { position: "bottom-right" },
  explorerHome:  { mode: "thispc", fixedFolder: "" },
  imeToggle:     { scheme: "win-space", capsLongPressMs: 600 },
  capsSound:     { enabled: false },
  midMinimize:   { enabled: true },
  shotTarget:    { target: "pictures", prefix: "截图", askEach: false },
  pointerTrail:  { enabled: false, length: "medium" },
  typeHide:      { enabled: false, opacity: 0.3, resumeMs: 2000 },
  undoEmptyBin:  { windowMs: 5000, extendMs: 10000, extends: 0, maxExtends: 2 },
  keycardExport: { format: "png", colorize: true },
  statusBar:     { visible: true, heightPx: 24 },
  treeCollapse:  { remember: true, expanded: {} },
  treeSync:      { enabled: true, scrollIntoView: true },
  spaceCheck:    { bufferPct: 10 },
  copyVerify:    { enabled: true, autoAboveBytes: 1073741824 },
  copyQueue:     { sameDiskSerial: true, crossDiskParallel: 2 },
  openDiagnose:  { enabled: true },
  roRemind:      { remindOncePerMount: true },
  longPath:      { enabled: true, tailKeep: 24 },
  winNumber:     { enabled: true, shiftNewInstance: true },
  winT:          { enabled: true },
  peekDesk:      { opacity: 0.15, fadeMs: 120 },
  altEsc:        { enabled: true, stepMs: 100 },
  layoutLock:    { locked: false, shakeMs: 120, cornerBadge: false },
  memDiag:       { mode: "standard", passes: 2 },
  netReset:      { countdownSec: 90 },
  clickLock:     { enabled: false, thresholdMs: 1100 },
  devVolume:     { devices: {}, newDeviceDefault: 40, cap: 10 },
  notifyVolume:  { notify: 50 },
  btBattery:     { lowPct: 20, throttleMs: 3600000, smoothStep: 2 },
  deviceNotify:  { readyDwellMs: 2000 },
  balance:       { pan: 0, testTone: true },
  taskmgrTop:    { remember: false },
  clockHover:    { showLunar: true, delayMs: 500 },
  anchorB6:      { lastRun: 0, lastAllGreen: false },
} as const;

class U3Store {
  private config: U3Config = this.freshConfig();
  private loaded = false;
  private undoStacks: Partial<Record<U3Section, Record<string, unknown>[]>> = {};
  private listeners = new Set<Listener>();

  /** 默认值预填的全新配置（get 永远有值；undo 回到默认档——单一事实源在 store 内生效）。 */
  private freshConfig(): U3Config {
    const out = {} as U3Config;
    for (const s of U3_SECTIONS) out[s] = clone(U3_DEFAULTS[s] ?? {});
    return out;
  }

  private emptyConfig(): U3Config {
    return this.freshConfig();
  }

  /** 首次访问从 localStorage 恢复；损坏配置整体重置并报备（启动链不能因配置炸掉）。 */
  load(): void {
    if (this.loaded || typeof localStorage === "undefined") {
      this.loaded = true;
      return;
    }
    this.loaded = true;
    try {
      const raw = localStorage.getItem(U3_LS_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as unknown;
      if (!isRecord(parsed) || parsed.format !== U3_FORMAT || parsed.version !== U3_VERSION) {
        console.error("[u3] 配置格式未知，已按默认处理");
        return;
      }
      const data = parsed.data as Record<string, unknown>;
      for (const s of U3_SECTIONS) {
        if (isRecord(data[s])) this.config[s] = clone(data[s]);
      }
    } catch (e) {
      console.error("[u3] 配置恢复失败，已重置为默认", e);
    }
  }

  private persist(): void {
    if (typeof localStorage === "undefined") return;
    try {
      localStorage.setItem(
        U3_LS_KEY,
        JSON.stringify({ format: U3_FORMAT, version: U3_VERSION, data: this.config }),
      );
    } catch (e) {
      throw new U3StoreError("*", `配置落盘失败（配额满或存储不可用）: ${String(e)}`);
    }
  }

  get<S extends U3Section>(section: S): Record<string, unknown> {
    this.load();
    return this.config[section];
  }

  /** 类型化读取：键不存在返回默认值。 */
  getWith<S extends U3Section, T>(section: S, key: string, fallback: T): T {
    const v = this.get(section)[key];
    return v === undefined ? fallback : (v as T);
  }

  /** 写入一节的若干键；写前快照；落盘失败抛 U3StoreError（内存态仍生效）。 */
  set<S extends U3Section>(section: S, patch: Record<string, unknown>): void {
    this.load();
    const prev = clone(this.config[section]);
    const next = { ...this.config[section], ...clone(patch) };
    const stack: Record<string, unknown>[] = (this.undoStacks[section] ??= []);
    stack.push(prev as Record<string, unknown>);
    if (stack.length > U3_UNDO_STACK_DEPTH) stack.splice(0, stack.length - U3_UNDO_STACK_DEPTH);
    this.config[section] = next;
    this.persist();
    this.emit(section, next, prev);
  }

  /** 还原一节的上一态；无可回退历史返回 false。 */
  undoSection<S extends U3Section>(section: S): boolean {
    const stack = this.undoStacks[section];
    const prev = stack?.pop();
    if (!prev) return false;
    const cur = clone(this.config[section]);
    this.config[section] = prev;
    this.persist();
    this.emit(section, prev, cur);
    return true;
  }

  canUndo<S extends U3Section>(section: S): boolean {
    return (this.undoStacks[section]?.length ?? 0) > 0;
  }

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(section: U3Section, next: Record<string, unknown>, prev: Record<string, unknown>): void {
    for (const l of this.listeners) {
      try {
        l(section, next, prev);
      } catch (e) {
        console.error(`[u3] 监听器处理 ${section} 变更失败`, e);
      }
    }
  }

  /** 导出整包（F396 备份范围 / F305 设置导出对接：直接逐节取表，不二次抄写）。 */
  exportAll(): U3Config {
    this.load();
    return clone(this.config);
  }

  /** 导入整包：先全部校验再统一切换（中断原子性）；无合法分节整包拒绝。 */
  importAll(data: Record<string, unknown>): void {
    this.load();
    const next = this.emptyConfig();
    let touched = false;
    for (const s of U3_SECTIONS) {
      if (isRecord(data[s])) {
        next[s] = clone(data[s]);
        touched = true;
      }
    }
    if (!touched) throw new U3StoreError("*", "导入包中没有任何合法分节");
    const prev = clone(this.config);
    this.config = next;
    this.persist();
    for (const s of U3_SECTIONS) this.emit(s, next[s], prev[s]);
  }

  /** 测试与恢复出厂用。 */
  reset(): void {
    this.config = this.emptyConfig();
    this.undoStacks = {};
    if (typeof localStorage !== "undefined") {
      try {
        localStorage.removeItem(U3_LS_KEY);
      } catch {
        /* ignore */
      }
    }
    for (const s of U3_SECTIONS) this.emit(s, {}, clone(this.config[s]));
  }
}

/** U3 域唯一 store 实例（一处一事实）。 */
export const u3Store = new U3Store();

