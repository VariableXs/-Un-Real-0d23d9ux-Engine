/**
 * J 鼠标域 · AI-J1 分队（F601-F620）共用底座。
 *
 * 职责（与 E 域 persona/store.ts 同源纪律，一处一事实）：
 * - 单一配置根：J1 全部配置收在 localStorage `variable:mouse:j1:v1` 一个键下，
 *   按 F 编号分节存储——F623 鼠标档案入 vxtheme（AI-J2）直接逐节取表。
 * - 订阅制总线：set() 写入后逐节广播；运行时/面板按节订阅，不轮询。
 * - 原子写 + 异常零静默：先写内存再落盘；落盘失败抛 J1StoreError，
 *   会话内配置仍生效，重启回退上次成功落盘值。
 * - 可回退：每节写前留一帧快照，undoSection() 一键还原，栈深 3。
 *
 * 边界声明（主册 J-0）：本底座只管「曲线与档案」层的开关与参数；
 * 基线（1:1 映射/双击速度）归 F250（lib/inputFeel.ts），指针格式规范归
 * F133、编辑器归 F156、切换入口归 E4、渲染平面归 F335——均不在此重复。
 */

export const J1_LS_KEY = "variable:mouse:j1:v1";
export const J1_FORMAT = "mouse-j1-config";
export const J1_VERSION = 1;

/** J1 域全部分节（键名 = F 编号语义，与各模块文件头注释一一对应）。 */
export const J1_SECTIONS = [
  "curve",         // F601 指针速度曲线谱
  "slowTune",      // F602 慢速微调模式
  "liftFilter",    // F603 抬笔滤波
  "autoscroll",    // F604 中键自动滚动
  "wheelNotch",    // F605 滚轮刻度语义
  "tiltWheel",     // F606 倾斜滚轮支持
  "seamGuard",     // F607 跨屏接缝手感
  "magnet",        // F608 指针磁吸对齐
  "dragScroll",    // F609 拖拽边缘自动滚
  "hoverTiming",   // F610 悬停时序自定义
  "tremor",        // F611 手抖过滤
  "wheelGain",     // F612 滚轮自适应增益
  "screenMemory",  // F613 指针跨屏落点记忆
  "devices",       // F614 鼠标分设备档案
  "sideButtons",   // F615 侧键编程
  "appProfiles",   // F616 应用级鼠标档案
  "gestures",      // F617 右键手势层
  "passthrough",   // F618 滚轮穿透开关
  "longPress",     // F619 长按时长统一旋钮
  "overlay",       // F620 指针衬底与投影
] as const;

export type J1Section = (typeof J1_SECTIONS)[number];
export type J1Config = Record<J1Section, Record<string, unknown>>;

export class J1StoreError extends Error {
  readonly section: string;
  constructor(section: string, message: string) {
    super(`[mouse-j1:${section}] ${message}`);
    this.name = "J1StoreError";
    this.section = section;
  }
}

type Listener = (section: J1Section, next: Record<string, unknown>, prev: Record<string, unknown>) => void;

/** 回退栈深 3（E 域同规格，一处一事实）。 */
export const J1_UNDO_STACK_DEPTH = 3;

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function clone<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}

class J1Store {
  private config: J1Config = this.emptyConfig();
  private loaded = false;
  private undoStacks: Partial<Record<J1Section, Record<string, unknown>[]>> = {};
  private listeners = new Set<Listener>();

  private emptyConfig(): J1Config {
    const out = {} as J1Config;
    for (const s of J1_SECTIONS) out[s] = {};
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
      const raw = localStorage.getItem(J1_LS_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as unknown;
      if (!isRecord(parsed) || parsed.format !== J1_FORMAT || parsed.version !== J1_VERSION) {
        console.error("[mouse-j1] 配置格式未知，已按默认处理");
        return;
      }
      const data = parsed.data as Record<string, unknown>;
      for (const s of J1_SECTIONS) {
        if (isRecord(data[s])) this.config[s] = clone(data[s]);
      }
    } catch (e) {
      console.error("[mouse-j1] 配置恢复失败，已重置为默认", e);
    }
  }

  private persist(): void {
    if (typeof localStorage === "undefined") return;
    try {
      localStorage.setItem(
        J1_LS_KEY,
        JSON.stringify({ format: J1_FORMAT, version: J1_VERSION, data: this.config }),
      );
    } catch (e) {
      throw new J1StoreError("*", `配置落盘失败（配额满或存储不可用）: ${String(e)}`);
    }
  }

  get<S extends J1Section>(section: S): Record<string, unknown> {
    this.load();
    return this.config[section];
  }

  /** 类型化读取：键不存在返回默认值。 */
  getWith<S extends J1Section, T>(section: S, key: string, fallback: T): T {
    const v = this.get(section)[key];
    return v === undefined ? fallback : (v as T);
  }

  /** 写入一节的若干键；写前快照；落盘失败抛 J1StoreError（内存态仍生效）。 */
  set<S extends J1Section>(section: S, patch: Record<string, unknown>): void {
    this.load();
    const prev = clone(this.config[section]);
    const next = { ...this.config[section], ...clone(patch) };
    const stack: Record<string, unknown>[] = (this.undoStacks[section] ??= []);
    stack.push(prev as Record<string, unknown>);
    if (stack.length > J1_UNDO_STACK_DEPTH) stack.splice(0, stack.length - J1_UNDO_STACK_DEPTH);
    this.config[section] = next;
    this.persist();
    this.emit(section, next, prev);
  }

  /** 还原一节的上一态；无可回退历史返回 false。 */
  undoSection<S extends J1Section>(section: S): boolean {
    const stack = this.undoStacks[section];
    const prev = stack?.pop();
    if (!prev) return false;
    const cur = clone(this.config[section]);
    this.config[section] = prev;
    this.persist();
    this.emit(section, prev, cur);
    return true;
  }

  canUndo<S extends J1Section>(section: S): boolean {
    return (this.undoStacks[section]?.length ?? 0) > 0;
  }

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(section: J1Section, next: Record<string, unknown>, prev: Record<string, unknown>): void {
    for (const l of this.listeners) {
      try {
        l(section, next, prev);
      } catch (e) {
        console.error(`[mouse-j1] 监听器处理 ${section} 变更失败`, e);
      }
    }
  }

  /** 导出整包（F623 打包对接 AI-J2：直接逐节取表，不二次抄写）。 */
  exportAll(): J1Config {
    this.load();
    return clone(this.config);
  }

  /** 导入整包：先全部校验再统一切换（中断原子性）；无合法分节整包拒绝。 */
  importAll(data: Record<string, unknown>): void {
    this.load();
    const next = this.emptyConfig();
    let touched = false;
    for (const s of J1_SECTIONS) {
      if (isRecord(data[s])) {
        next[s] = clone(data[s]);
        touched = true;
      }
    }
    if (!touched) throw new J1StoreError("*", "导入包中没有任何合法分节");
    const prev = clone(this.config);
    this.config = next;
    this.persist();
    for (const s of J1_SECTIONS) this.emit(s, next[s], prev[s]);
  }

  /** 测试与恢复出厂用。 */
  reset(): void {
    this.config = this.emptyConfig();
    this.undoStacks = {};
    if (typeof localStorage !== "undefined") {
      try {
        localStorage.removeItem(J1_LS_KEY);
      } catch {
        /* ignore */
      }
    }
    for (const s of J1_SECTIONS) this.emit(s, {}, clone(this.config[s]));
  }
}

/** J1 域唯一 store 实例（一处一事实）。 */
export const j1Store = new J1Store();

/** 各分节默认值集中登记处——面板「恢复默认」与运行时兜底共用同一份（单一事实源）。 */
export const J1_DEFAULTS: J1Config = {
  curve: { id: "classic", cp1x: 0.35, cp1y: 0.55, cp2x: 0.7, cp2y: 1.0, sens: 1.0 },
  slowTune: { enabled: true, ratio: 0.1, key: "shift" },
  liftFilter: { enabled: true },
  autoscroll: { enabled: true, deadZonePx: 8, maxPx: 60 },
  wheelNotch: { mode: "per-app", overrides: {}, linesPerNotch: 3 },
  tiltWheel: { enabled: true, colsPerNotch: 3, repeatDelayMs: 350, repeatRateMs: 40, hasTilt: true },
  seamGuard: { enabled: true, edgePx: 4, dwellMs: 200, cornerPx: 8, pairs: {} },
  magnet: { enabled: false, radiusPx: 12 },
  dragScroll: { enabled: true, bandPx: 24 },
  hoverTiming: { menuDelayMs: 400, tooltipDelayMs: 500 },
  tremor: { level: "off" },
  wheelGain: { enabled: true, minLines: 3, maxLines: 12, accelMs: 220 },
  screenMemory: { enabled: true, points: {} },
  devices: { profiles: [], notifyOnClone: true },
  sideButtons: { global: { back: "nav-back", forward: "nav-forward" }, apps: {} },
  appProfiles: { profiles: {}, currentApp: "" },
  gestures: { enabled: false, trailFadeMs: 120, custom: {}, bindings: {}, shapes: {} },
  passthrough: { enabled: true, exemptTypes: ["scrollable-layer", "select", "menu"] },
  longPress: { scale: 1.0, registry: {} },
  overlay: { outline: true, shadow: false, ring: false },
} as const;
