/**
 * C 桌面体验域·后段 · AI-D2 分队（F093-F110）前端共用底座。
 *
 * 职责（与 J1 底座 j1store.ts 同源纪律，一处一事实）：
 * - 单一配置根：D2 全部配置收在 localStorage `variable:desktop:d2:v1` 一个键下，
 *   按 F 编号分节存储——面板/运行时/工具窗口逐节取表，不各自散落。
 * - 订阅制总线：set() 写入后逐节广播；运行时/面板按节订阅，不轮询。
 * - 原子写 + 异常零静默：先写内存再落盘；落盘失败抛 D2StoreError，
 *   会话内配置仍生效，重启回退上次成功落盘值（十三·补：显性化不吞）。
 * - 可回退：每节写前留一帧快照，undoSection() 一键还原，栈深 3。
 *
 * 边界声明（一处一事实）：
 * - 判据实装层（Rust 模型面）在 `kernel/varix/src/stard/`——本底座只管
 *   「前端配置与开关」；数值判线常量在对应 TS 模块内声明并注明与 Rust 常量同源。
 * - F109 剪贴板历史的录制/落盘走 Tauri 后端（AI-07 N-15 面），此处只存
 *   面板侧偏好（自动粘贴/回贴确认），不碰后端策略——不重复造第二套。
 * - F101 天气件、F104 录音件属主册 F200 存量冻结候删——不在分节清单内
 *   （结构性守护：配置面也无此二项）。
 */

export const D2_LS_KEY = "variable:desktop:d2:v1";
export const D2_FORMAT = "desktop-d2-config";
export const D2_VERSION = 1;

/** D2 域全部分节（键名 = F 编号语义，与各模块文件头注释一一对应）。 */
export const D2_SECTIONS = [
  "thumbeng",     // F093 图片缩略图引擎（缓存策略/上限）
  "mediainfo",    // F094 媒体信息悬浮（悬停延迟/开关）
  "term2",        // F095 终端 2.0（字号/光标样式/回看上限显示）
  "termpalette",  // F096 终端命令面板（收藏/开关）
  "notepad",      // F097 记事本类编辑器（自动保存/大文件只读门）
  "snipshot",     // F098 截图工具（默认模式/保存格式）
  "calcx",        // F099 计算器（历史轮数/角度制锚点）
  "clocksuite",   // F100 时钟套件（倒计时到点链开关）
  "sticknote",    // F102 便签（置顶/字号/上限）
  "sketchpad",    // F103 画图件（笔刷/自动草稿）
  "photolib",     // F105 相册（放映节奏/缩放档）
  "keyhud",       // F106 键盘提示 HUD（开关/位置）
  "imefloat",     // F107 输入法状态浮窗（模式/隐私隐藏）
  "phrasebk",     // F108 自定义短语库（词条/优先开关）
  "cliphist",     // F109 剪贴板历史（面板侧偏好）
  "osk",          // F110 屏幕键盘（透明度/学习模式/吸附）
] as const;

export type D2Section = (typeof D2_SECTIONS)[number];
export type D2Config = Record<D2Section, Record<string, unknown>>;

/** 主册 F200 存量冻结候删名单（编号不复用；正式删除待 Variable 确认）。
 *  结构性守护：D2_SECTIONS 与本清单必须互斥——违反即配置面结构缺陷。 */
export const D2_FROZEN_ITEMS: readonly string[] = ["F101", "F104"];

export class D2StoreError extends Error {
  readonly section: string;
  constructor(section: string, message: string) {
    super(`[desktop-d2:${section}] ${message}`);
    this.name = "D2StoreError";
    this.section = section;
  }
}

type Listener = (section: D2Section, next: Record<string, unknown>, prev: Record<string, unknown>) => void;

/** 回退栈深 3（J1/E 域同规格，一处一事实）。 */
export const D2_UNDO_STACK_DEPTH = 3;

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function clone<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}

class D2Store {
  private config: D2Config = this.emptyConfig();
  private loaded = false;
  private undoStacks: Partial<Record<D2Section, Record<string, unknown>[]>> = {};
  private listeners = new Set<Listener>();

  private emptyConfig(): D2Config {
    const out = {} as D2Config;
    for (const s of D2_SECTIONS) out[s] = {};
    return out;
  }

  /** 首次访问从 localStorage 恢复；损坏配置整体重置并报备（启动链不能因配置炸掉）。 */
  load(): void {
    if (this.loaded || typeof localStorage === "undefined") {
      this.loaded = true;
      return;
    }
    this.loaded = true;
    try {
      const raw = localStorage.getItem(D2_LS_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as unknown;
      if (!isRecord(parsed) || parsed.format !== D2_FORMAT || parsed.version !== D2_VERSION) {
        console.error("[desktop-d2] 配置格式未知，已按默认处理");
        return;
      }
      const data = parsed.data as Record<string, unknown>;
      for (const s of D2_SECTIONS) {
        if (isRecord(data[s])) this.config[s] = clone(data[s]);
      }
    } catch (e) {
      console.error("[desktop-d2] 配置恢复失败，已重置为默认", e);
    }
  }

  private persist(): void {
    if (typeof localStorage === "undefined") return;
    try {
      localStorage.setItem(
        D2_LS_KEY,
        JSON.stringify({ format: D2_FORMAT, version: D2_VERSION, data: this.config }),
      );
    } catch (e) {
      throw new D2StoreError("*", `配置落盘失败（配额满或存储不可用）: ${String(e)}`);
    }
  }

  get<S extends D2Section>(section: S): Record<string, unknown> {
    this.load();
    return this.config[section];
  }

  /** 类型化读取：键不存在返回默认值。 */
  getWith<S extends D2Section, T>(section: S, key: string, fallback: T): T {
    const v = this.get(section)[key];
    return v === undefined ? fallback : (v as T);
  }

  /** 写入一节的若干键；写前快照；落盘失败抛 D2StoreError（内存态仍生效）。 */
  set<S extends D2Section>(section: S, patch: Record<string, unknown>): void {
    this.load();
    const prev = clone(this.config[section]);
    const next = { ...this.config[section], ...clone(patch) };
    const stack: Record<string, unknown>[] = (this.undoStacks[section] ??= []);
    stack.push(prev as Record<string, unknown>);
    if (stack.length > D2_UNDO_STACK_DEPTH) stack.splice(0, stack.length - D2_UNDO_STACK_DEPTH);
    this.config[section] = next;
    this.persist();
    this.emit(section, next, prev);
  }

  /** 还原一节的上一态；无可回退历史返回 false。 */
  undoSection<S extends D2Section>(section: S): boolean {
    const stack = this.undoStacks[section];
    const prev = stack?.pop();
    if (!prev) return false;
    const cur = clone(this.config[section]);
    this.config[section] = prev;
    this.persist();
    this.emit(section, prev, cur);
    return true;
  }

  canUndo<S extends D2Section>(section: S): boolean {
    return (this.undoStacks[section]?.length ?? 0) > 0;
  }

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(section: D2Section, next: Record<string, unknown>, prev: Record<string, unknown>): void {
    for (const l of this.listeners) {
      try {
        l(section, next, prev);
      } catch (e) {
        console.error(`[desktop-d2] 监听器处理 ${section} 变更失败`, e);
      }
    }
  }

  /** 导出整包（备份/诊断面——与设置导入导出同格式）。 */
  exportAll(): D2Config {
    this.load();
    return clone(this.config);
  }

  /** 导入整包：先全部校验再统一切换（中断原子性）；无合法分节整包拒绝。 */
  importAll(data: Record<string, unknown>): void {
    this.load();
    const next = this.emptyConfig();
    let touched = false;
    for (const s of D2_SECTIONS) {
      if (isRecord(data[s])) {
        next[s] = clone(data[s]);
        touched = true;
      }
    }
    if (!touched) throw new D2StoreError("*", "导入包中没有任何合法分节");
    const prev = clone(this.config);
    this.config = next;
    this.persist();
    for (const s of D2_SECTIONS) this.emit(s, next[s], prev[s]);
  }

  /** 测试与恢复出厂用。 */
  reset(): void {
    this.config = this.emptyConfig();
    this.undoStacks = {};
    if (typeof localStorage !== "undefined") {
      try {
        localStorage.removeItem(D2_LS_KEY);
      } catch {
        /* ignore */
      }
    }
    for (const s of D2_SECTIONS) this.emit(s, {}, clone(this.config[s]));
  }
}

/** D2 域唯一 store 实例（一处一事实）。 */
export const d2Store = new D2Store();

/** 冻结守护：分节清单（16 实现项）与冻结名单（F101/F104）互斥断言。 */
export function assertFrozenGuard(): boolean {
  const ids = ["F093", "F094", "F095", "F096", "F097", "F098", "F099", "F100", "F102", "F103", "F105", "F106", "F107", "F108", "F109", "F110"];
  return D2_FROZEN_ITEMS.every((f) => !ids.some((i) => i === f));
}

/** 各分节默认值集中登记处——面板「恢复默认」与运行时兜底共用同一份（单一事实源）。
 *  默认值 = 主册判据默认（与 Rust 模型面常量同源）。 */
export const D2_DEFAULTS: D2Config = {
  thumbeng: { enabled: true, cacheCapMb: 2048, progressive: true },          // F093：2GB 上限/渐进占位
  mediainfo: { enabled: true, hoverDelayMs: 800 },                            // F094：悬停 800ms
  term2: { fontSizeStep: 3, cursorStyle: "block", cursorBlink: true },        // F095：16px 档/块光标
  termpalette: { enabled: true },                                             // F096
  notepad: { autoSave: true, bigFileReadOnlyGate: true },                     // F097
  snipshot: { defaultMode: "region", includeCursor: false },                  // F098
  calcx: { historyCap: 20, angle: "deg" },                                    // F099：回填 20 轮
  clocksuite: { countdownFullChain: true, alarmSnoozeMin: 5 },                // F100：到点全链/贪睡 5 分钟
  sticknote: { cap: 20, fontSize: 14, colorIndex: 0 },                        // F102：20 张上限（诚实拒绝）
  sketchpad: { brush: "pen", autoDraftSec: 30 },                              // F103：30s 自动草稿
  photolib: { slideIntervalSec: 5, crossfadeMs: 250, zoomStep: 3 },           // F105：自动 5s/交叉 250ms
  keyhud: { enabled: true, cornerMode: false },                               // F106
  imefloat: { mode: "follow", hideOnPassword: true },                         // F107：跟随/密码隐藏
  phrasebk: { phrasePriority: true },                                         // F108：短语优先
  cliphist: { confirmPasteBack: false },                                      // F109：面板侧偏好
  osk: { full: true, opacity: 90, learning: true, clickThrough: false, alwaysOnTop: true }, // F110
} as const;
