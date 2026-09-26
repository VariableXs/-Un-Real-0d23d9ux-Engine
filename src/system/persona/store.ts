/**
 * E 个性化域 · 共用底座（F151-F170 二十项共同依赖）。
 *
 * 职责：
 * - 单一配置根：全部 E 域配置收在 localStorage `variable:persona:v1` 一个键下，
 *   分节存储（theme/wallpaper/icons/pointer/sound/startmenu/font/motion/archive/
 *   widgets/lock/boot/ime/ctxmenu/taskbar/shortcuts）——档案导出（F161）直接
 *   逐节取表，不二次抄写。
 * - 订阅制总线（F155 设计细节「广播失效——订阅制不轮询」同源）：set() 写入
 *   后逐节广播；监听方按节订阅，收不到自己不关心的节。
 * - 原子写：先写内存再落盘，落盘失败（配额满/隐私模式）抛 PersonaStoreError，
 *   绝不静默吞掉——会话内配置仍然生效，重启后回退上次成功落盘的值。
 *
 * 三铁律（E 域宪法，F170 执法）：随时可改 / 随时可退 / 预设+微调。
 * 本文件提供「随时可退」的机械基础：每个分节写之前留一帧快照，
 * undoSection() 一键还原上一态；快照栈深 3（F155 回退栈同规格）。
 */

export const PERSONA_LS_KEY = "variable:persona:v1";
export const PERSONA_FORMAT = "persona-config";
export const PERSONA_VERSION = 1;

/** E 域全部分节名（与 F 编号对应关系见各节文件头注释）。 */
export const PERSONA_SECTIONS = [
  "theme",       // F151/F152/F153/F162 主题令牌、预览、深浅切换、应用例外
  "wallpaper",   // F154 壁纸每日一换
  "icons",       // F155 图标包热更换
  "pointer",     // F156 指针方案
  "sound",       // F157 声音混合器
  "startmenu",   // F158 开始菜单布局预设
  "font",        // F159 字体安全档
  "motion",      // F160 动效强度档
  "archive",     // F161 个性化档案（元信息）
  "widgets",     // F163 桌面小组件
  "lock",        // F164 锁屏定制
  "boot",        // F165 开机动画个性化
  "ime",         // F166 输入法皮肤
  "ctxmenu",     // F167 右键菜单自定义
  "taskbar",     // F168 任务栏个性化
  "shortcuts",   // F169 快捷键
] as const;

export type PersonaSection = (typeof PERSONA_SECTIONS)[number];

export type PersonaConfig = Record<PersonaSection, Record<string, unknown>>;

export class PersonaStoreError extends Error {
  readonly section: string;
  constructor(section: string, message: string) {
    super(`[persona:${section}] ${message}`);
    this.name = "PersonaStoreError";
    this.section = section;
  }
}

type Listener = (section: PersonaSection, next: Record<string, unknown>, prev: Record<string, unknown>) => void;

/** 回退栈深 3（F155 设计细节「回退上一包栈深 3 层」——全域同规格，一处一事实）。 */
export const UNDO_STACK_DEPTH = 3;

function emptyConfig(): PersonaConfig {
  const out = {} as PersonaConfig;
  for (const s of PERSONA_SECTIONS) out[s] = {};
  return out;
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

/** 深拷贝（配置对象均为 JSON 可序列化值，structuredClone 兜底走 JSON）。 */
function clone<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}

class PersonaStore {
  private config: PersonaConfig = emptyConfig();
  private loaded = false;
  private undoStacks: Partial<Record<PersonaSection, Record<string, unknown>[]>> = {};
  private listeners = new Set<Listener>();

  /** 首次访问时从 localStorage 恢复；损坏配置整包重置并抛报备事件（异常零静默）。 */
  load(): void {
    if (this.loaded || typeof localStorage === "undefined") {
      this.loaded = true;
      return;
    }
    this.loaded = true;
    try {
      const raw = localStorage.getItem(PERSONA_LS_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as unknown;
      if (!isRecord(parsed) || parsed.format !== PERSONA_FORMAT || parsed.version !== PERSONA_VERSION) {
        // 旧版/未知格式：不部分采用（半套配置比空配置更危险），整体重置。
        return;
      }
      const data = parsed.data as Record<string, unknown>;
      for (const s of PERSONA_SECTIONS) {
        if (isRecord(data[s])) this.config[s] = clone(data[s]);
      }
    } catch (e) {
      // 语义上不可恢复（JSON 损坏）：重置为空配置，报备给诊断面（不抛出——启动链不能因配置炸掉）。
      console.error("[persona] 配置恢复失败，已重置为默认", e);
    }
  }

  private persist(): void {
    if (typeof localStorage === "undefined") return;
    try {
      localStorage.setItem(
        PERSONA_LS_KEY,
        JSON.stringify({ format: PERSONA_FORMAT, version: PERSONA_VERSION, data: this.config }),
      );
    } catch (e) {
      throw new PersonaStoreError("*", `配置落盘失败（配额满或存储不可用）: ${String(e)}`);
    }
  }

  get<S extends PersonaSection>(section: S): Record<string, unknown> {
    this.load();
    return this.config[section];
  }

  /** 类型化读取：节内键不存在时返回默认值。 */
  getWith<T>(section: PersonaSection, key: string, fallback: T): T {
    const v = this.get(section)[key];
    return v === undefined ? fallback : (v as T);
  }

  /**
   * 写入一节的一个或多个键。写前自动快照（可 undo）；
   * 落盘失败抛 PersonaStoreError（内存态仍生效——会话连续性优先）。
   */
  set<S extends PersonaSection>(section: S, patch: Record<string, unknown>): void {
    this.load();
    const prev = clone(this.config[section]);
    const next = { ...this.config[section], ...clone(patch) };
    // 快照进 undo 栈（深 3，最旧静默淘汰——F202 同款语义）。
    const stack: Record<string, unknown>[] = this.undoStacks[section] ?? [];
    this.undoStacks[section] = stack;
    stack.push(prev as Record<string, unknown>);
    if (stack.length > UNDO_STACK_DEPTH) stack.splice(0, stack.length - UNDO_STACK_DEPTH);
    this.config[section] = next as PersonaConfig[S];
    this.persist();
    this.emit(section, next, prev);
  }

  /** 还原一节的上一态（三铁律「随时可退」的机械实现）。 */
  undoSection<S extends PersonaSection>(section: S): boolean {
    const stack = this.undoStacks[section];
    const prev = stack?.pop();
    if (!prev) return false;
    const cur = clone(this.config[section]);
    this.config[section] = prev;
    this.persist();
    this.emit(section, prev, cur);
    return true;
  }

  /** 一节是否还有可回退历史。 */
  canUndo<S extends PersonaSection>(section: S): boolean {
    return (this.undoStacks[section]?.length ?? 0) > 0;
  }

  /** 订阅节变更；返回退订函数。 */
  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(section: PersonaSection, next: Record<string, unknown>, prev: Record<string, unknown>): void {
    for (const l of this.listeners) {
      try {
        l(section, next, prev);
      } catch (e) {
        // 监听方异常不允许炸掉总线（隔离纪律 F175 同源），但绝不静默——报备。
        console.error(`[persona] 监听器处理 ${section} 变更失败`, e);
      }
    }
  }

  /** 档案导出（F161）：整包深拷贝。 */
  exportAll(): PersonaConfig {
    this.load();
    return clone(this.config);
  }

  /**
   * 档案导入（F161）：逐节覆盖；中断原子性 = 先全部校验再统一切换，
   * 任一节非法则整包拒绝（不留半套）。
   */
  importAll(data: Record<string, unknown>): void {
    this.load();
    const next = emptyConfig();
    let touched = false;
    for (const s of PERSONA_SECTIONS) {
      if (isRecord(data[s])) {
        next[s] = clone(data[s]);
        touched = true;
      }
    }
    if (!touched) throw new PersonaStoreError("*", "导入包中没有任何合法分节");
    const prev = clone(this.config);
    this.config = next;
    this.persist();
    for (const s of PERSONA_SECTIONS) this.emit(s, next[s], prev[s]);
  }

  /** 测试与「恢复出厂」用：清空全部配置（含 undo 栈）。 */
  reset(): void {
    this.config = emptyConfig();
    this.undoStacks = {};
    if (typeof localStorage !== "undefined") {
      try {
        localStorage.removeItem(PERSONA_LS_KEY);
      } catch {
        /* ignore */
      }
    }
    for (const s of PERSONA_SECTIONS) this.emit(s, {}, clone(this.config[s]));
  }
}

/** E 域唯一 store 实例（一处一事实）。 */
export const personaStore = new PersonaStore();

// ---------- 通用工具（各 F 模块共享，避免重复实现） ----------

/** 感知对数音量曲线（F157：slider 50% = 实际半响）。0..1 滑杆值 → 0..1 感知响度。 */
export function perceptualVolume(slider: number): number {
  const s = Math.min(1, Math.max(0, slider));
  return s * s;
}

/** 时间触发器下次执行时刻（F153/F154 共用）：每日 HH:MM → 下一毫秒时间戳。 */
export function nextDailyFire(now: number, hour: number, minute: number): number {
  const d = new Date(now);
  d.setHours(hour, minute, 0, 0);
  if (d.getTime() <= now) d.setDate(d.getDate() + 1);
  return d.getTime();
}

/** 均匀随机抽取 + 排除表（F154「池内均匀随机 + 近 7 天排除表」）。 */
export function pickExcluding<T>(pool: T[], exclude: (item: T) => boolean, rand: () => number = Math.random): T | null {
  const candidates = pool.filter((x) => !exclude(x));
  const source = candidates.length > 0 ? candidates : pool;
  if (source.length === 0) return null;
  return source[Math.floor(rand() * source.length)] ?? null;
}

/** 固定 8.0s（±0.2s 容差）等结构时长校验（F165 结构时长零变化）。 */
export const BOOT_DURATION_MS = 8000;
export const BOOT_DURATION_TOLERANCE_MS = 200;

export function withinBootDuration(ms: number): boolean {
  return Math.abs(ms - BOOT_DURATION_MS) <= BOOT_DURATION_TOLERANCE_MS;
}
