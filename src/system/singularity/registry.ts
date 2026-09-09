/**
 * SINGULARITY-100（奇点计划）· Q-01…Q-100 功能注册表。
 *
 * 单一事实源：100 项功能的元数据（编号/领域/默认态/参数）与开关状态。
 * - 状态持久化：localStorage `variable.singularity.v1`（仅本机，零网络）；
 * - 订阅通知：`subscribeSingu`（SingularityRuntime / Hub / 各领域模块共用）；
 * - React 绑定：`useSingu`（useSyncExternalStore）。
 *
 * 纪律：默认态克制（重动效默认关、观测类默认关、纯工具默认不自动激活），
 * reduce-motion / safeMode / static 的降级由各模块自行实现并在描述中注明。
 */

export type SinguDomainId =
  | "boot"
  | "windows"
  | "desktop"
  | "taskbar"
  | "input"
  | "files"
  | "tools"
  | "hardware"
  | "compat"
  | "privacy"
  | "eco"
  | "vision"
  | "sound"
  | "a11y"
  | "quality";

export interface SinguDomainDef {
  id: SinguDomainId;
  index: number;
  zh: string;
  en: string;
}

export type SinguParamValue = number | boolean | string;

export interface SinguParamDef {
  key: string;
  /** labels.ts 内的词条键（singuP_<key> 或功能内自定义）。 */
  labelKey: string;
  type: "toggle" | "slider" | "select";
  default: SinguParamValue;
  min?: number;
  max?: number;
  step?: number;
  options?: Array<{ value: string; labelKey: string }>;
}

export interface SinguFeatureDef {
  /** Q-01 … Q-100 */
  id: string;
  domain: SinguDomainId;
  /** 默认开关（克制原则：氛围重动效默认关）。 */
  on: boolean;
  params?: SinguParamDef[];
  /** 该功能有独立 overlay 工具窗（ai04:open-feature 的 feature id）。 */
  overlay?: string;
}

export interface SinguFeatureState {
  on: boolean;
  params: Record<string, SinguParamValue>;
}

/** 十五域定义（与全景文档 §0.3 一致）。 */
export const SINGU_DOMAINS: SinguDomainDef[] = [
  { id: "boot", index: 1, zh: "启动与品牌剧场", en: "Boot & Brand Theater" },
  { id: "windows", index: 2, zh: "窗口与空间管理", en: "Windows & Space" },
  { id: "desktop", index: 3, zh: "桌面与图标表达", en: "Desktop & Icons" },
  { id: "taskbar", index: 4, zh: "任务栏与开始菜单", en: "Taskbar & Start" },
  { id: "input", index: 5, zh: "键盘与输入手感", en: "Keyboard & Input" },
  { id: "files", index: 6, zh: "文件与数据能力", en: "Files & Data" },
  { id: "tools", index: 7, zh: "效率与工具中枢", en: "Tools & Flow" },
  { id: "hardware", index: 8, zh: "系统集成与硬件", en: "System & Hardware" },
  { id: "compat", index: 9, zh: "兼容性防线", en: "Compat Defense" },
  { id: "privacy", index: 10, zh: "安全与隐私", en: "Security & Privacy" },
  { id: "eco", index: 11, zh: "开放生态", en: "Open Ecosystem" },
  { id: "vision", index: 12, zh: "视觉、个性化与氛围", en: "Visual & Ambience" },
  { id: "sound", index: 13, zh: "声音与通知", en: "Sound & Notify" },
  { id: "a11y", index: 14, zh: "无障碍与本地化", en: "A11y & Locale" },
  { id: "quality", index: 15, zh: "工程质量、性能与收官", en: "Quality & Finale" },
];

const S = (
  key: string,
  min: number,
  max: number,
  step: number,
  def: number,
): SinguParamDef => ({ key, labelKey: `singuP_${key}`, type: "slider", default: def, min, max, step });

const T = (key: string, def: boolean): SinguParamDef => ({ key, labelKey: `singuP_${key}`, type: "toggle", default: def });

const SEL = (key: string, def: string, options: Array<[string, string]>): SinguParamDef => ({
  key,
  labelKey: `singuP_${key}`,
  type: "select",
  default: def,
  options: options.map(([value, labelKey]) => ({ value, labelKey: `singuO_${labelKey}` })),
});

/**
 * 100 项功能定义。默认态说明：
 * - 无副作用观测（Q-37/54/64/69/70/72/93/95/97/98/100 等）默认开（hub 可见即价值）；
 * - 重动效/氛围层默认关（用户按需开启，守护既有手感）；
 * - 纯工具 overlay 不自动激活（按需呼出）。
 */
export const SINGU_FEATURES: SinguFeatureDef[] = [
  // ---- 域1 启动与品牌剧场（Q-01…Q-07）----
  { id: "Q-01", domain: "boot", on: true, params: [S("wakeMin", 1, 30, 1, 5)] },
  { id: "Q-02", domain: "boot", on: true },
  { id: "Q-03", domain: "boot", on: true },
  { id: "Q-04", domain: "boot", on: true, params: [SEL("script", "aurora", [["aurora", "aurora"], ["snowfield", "snowfield"], ["matrix", "matrix"], ["paperplane", "paperplane"]])] },
  { id: "Q-05", domain: "boot", on: false },
  { id: "Q-06", domain: "boot", on: true },
  { id: "Q-07", domain: "boot", on: true },
  // ---- 域2 窗口与空间管理（Q-08…Q-15）----
  { id: "Q-08", domain: "windows", on: false, params: [S("ringMs", 1000, 5000, 500, 2000)] },
  { id: "Q-09", domain: "windows", on: false, params: [S("trailMs", 200, 800, 50, 350)] },
  { id: "Q-10", domain: "windows", on: false, params: [S("ageMin", 5, 120, 5, 30)] },
  { id: "Q-11", domain: "windows", on: true, params: [SEL("layout", "grid", [["grid", "grid"], ["spiral", "spiral"], ["columns", "columns"], ["quadrants", "quadrants"]])] },
  { id: "Q-12", domain: "windows", on: false, params: [S("peekMs", 200, 1000, 100, 400)] },
  { id: "Q-13", domain: "windows", on: false },
  { id: "Q-14", domain: "windows", on: true, params: [S("maxTilt", 0, 4, 0.5, 2)] },
  { id: "Q-15", domain: "windows", on: true },
  // ---- 域3 桌面与图标表达（Q-16…Q-22）----
  { id: "Q-16", domain: "desktop", on: true },
  { id: "Q-17", domain: "desktop", on: true },
  { id: "Q-18", domain: "desktop", on: true },
  { id: "Q-19", domain: "desktop", on: true },
  { id: "Q-20", domain: "desktop", on: true, params: [T("autoReturn", true)] },
  { id: "Q-21", domain: "desktop", on: true },
  { id: "Q-22", domain: "desktop", on: false },
  // ---- 域4 任务栏与开始菜单（Q-23…Q-29）----
  { id: "Q-23", domain: "taskbar", on: false },
  { id: "Q-24", domain: "taskbar", on: true },
  { id: "Q-25", domain: "taskbar", on: false, params: [T("manualGhost", false)] },
  { id: "Q-26", domain: "taskbar", on: true },
  { id: "Q-27", domain: "taskbar", on: false },
  { id: "Q-28", domain: "taskbar", on: true },
  { id: "Q-29", domain: "taskbar", on: true },
  // ---- 域5 键盘与输入手感（Q-30…Q-36）----
  { id: "Q-30", domain: "input", on: false, params: [S("maxSpeed", 2, 12, 1, 8), T("bounce", true)] },
  { id: "Q-31", domain: "input", on: false, params: [S("bufferMs", 300, 1000, 100, 500)] },
  { id: "Q-32", domain: "input", on: true, params: [SEL("timing", "standard", [["strict", "strict"], ["standard", "standard"], ["lenient", "lenient"]])] },
  { id: "Q-33", domain: "input", on: false },
  { id: "Q-34", domain: "input", on: false },
  { id: "Q-35", domain: "input", on: false, params: [T("bigMode", true)] },
  { id: "Q-36", domain: "input", on: false },
  // ---- 域6 文件与数据能力（Q-37…Q-43）----
  { id: "Q-37", domain: "files", on: true, overlay: "singu-campfire" },
  { id: "Q-38", domain: "files", on: true },
  { id: "Q-39", domain: "files", on: true },
  { id: "Q-40", domain: "files", on: true },
  { id: "Q-41", domain: "files", on: true },
  { id: "Q-42", domain: "files", on: true },
  { id: "Q-43", domain: "files", on: true },
  // ---- 域7 效率与工具中枢（Q-44…Q-50）----
  { id: "Q-44", domain: "tools", on: false, overlay: "singu-pomodoro", params: [S("focusMin", 10, 60, 5, 25), S("breakMin", 3, 15, 1, 5)] },
  { id: "Q-45", domain: "tools", on: false, overlay: "singu-ruler" },
  { id: "Q-46", domain: "tools", on: false },
  { id: "Q-47", domain: "tools", on: true },
  { id: "Q-48", domain: "tools", on: false },
  { id: "Q-49", domain: "tools", on: false, overlay: "singu-countdown", params: [S("leadMin", 1, 10, 1, 5)] },
  { id: "Q-50", domain: "tools", on: false, overlay: "singu-stash" },
  // ---- 域8 系统集成与硬件（Q-51…Q-57）----
  { id: "Q-51", domain: "hardware", on: false },
  { id: "Q-52", domain: "hardware", on: false },
  { id: "Q-53", domain: "hardware", on: true },
  { id: "Q-54", domain: "hardware", on: true, overlay: "singu-campfire", params: [T("tideNotify", true)] },
  { id: "Q-55", domain: "hardware", on: true },
  { id: "Q-56", domain: "hardware", on: true },
  { id: "Q-57", domain: "hardware", on: true },
  // ---- 域9 兼容性防线（Q-58…Q-63）----
  { id: "Q-58", domain: "compat", on: true, params: [S("confirmSec", 5, 30, 1, 10)] },
  { id: "Q-59", domain: "compat", on: true },
  { id: "Q-60", domain: "compat", on: true, params: [S("low1", 5, 30, 1, 15), S("low2", 3, 15, 1, 7)] },
  { id: "Q-61", domain: "compat", on: true },
  { id: "Q-62", domain: "compat", on: true, params: [T("askResume", true)] },
  { id: "Q-63", domain: "compat", on: true },
  // ---- 域10 安全与隐私（Q-64…Q-70）----
  { id: "Q-64", domain: "privacy", on: true, overlay: "singu-journal" },
  { id: "Q-65", domain: "privacy", on: true, params: [S("ttlSec", 30, 600, 10, 90)] },
  { id: "Q-66", domain: "privacy", on: true },
  { id: "Q-67", domain: "privacy", on: true },
  { id: "Q-68", domain: "privacy", on: true, overlay: "singu-journal" },
  { id: "Q-69", domain: "privacy", on: true, overlay: "singu-journal" },
  { id: "Q-70", domain: "privacy", on: true, overlay: "singu-iceberg" },
  // ---- 域11 开放生态（Q-71…Q-76）----
  { id: "Q-71", domain: "eco", on: true },
  { id: "Q-72", domain: "eco", on: true, overlay: "singu-eco" },
  { id: "Q-73", domain: "eco", on: false, overlay: "singu-playground" },
  { id: "Q-74", domain: "eco", on: false, overlay: "singu-events" },
  { id: "Q-75", domain: "eco", on: true, overlay: "singu-packview" },
  { id: "Q-76", domain: "eco", on: false, overlay: "singu-cli" },
  // ---- 域12 视觉、个性化与氛围（Q-77…Q-81）----
  { id: "Q-77", domain: "vision", on: false, params: [SEL("season", "auto", [["auto", "auto"], ["spring", "spring"], ["summer", "summer"], ["autumn", "autumn"], ["winter", "winter"]])] },
  { id: "Q-78", domain: "vision", on: false, params: [S("parallax", 0, 100, 5, 40)] },
  { id: "Q-79", domain: "vision", on: false, overlay: "singu-boutique" },
  { id: "Q-80", domain: "vision", on: false, overlay: "singu-boutique" },
  { id: "Q-81", domain: "vision", on: true },
  // ---- 域13 声音与通知（Q-82…Q-86）----
  { id: "Q-82", domain: "sound", on: true },
  { id: "Q-83", domain: "sound", on: false, params: [S("hour", 18, 23, 1, 21)] },
  { id: "Q-84", domain: "sound", on: false, overlay: "singu-soundlens" },
  { id: "Q-85", domain: "sound", on: false },
  { id: "Q-86", domain: "sound", on: true },
  // ---- 域14 无障碍与本地化（Q-87…Q-92）----
  { id: "Q-87", domain: "a11y", on: true, params: [SEL("fuzzy", "low", [["off", "off"], ["low", "low"], ["high", "high"]])] },
  { id: "Q-88", domain: "a11y", on: false, params: [S("scale", 85, 130, 5, 100)] },
  { id: "Q-89", domain: "a11y", on: false, params: [SEL("cvd", "none", [["none", "none"], ["protan", "protan"], ["deutan", "deutan"], ["tritan", "tritan"]])] },
  { id: "Q-90", domain: "a11y", on: false },
  { id: "Q-91", domain: "a11y", on: false, params: [S("holdMs", 2000, 8000, 500, 3000)] },
  { id: "Q-92", domain: "a11y", on: true },
  // ---- 域15 工程质量、性能与收官（Q-93…Q-100）----
  { id: "Q-93", domain: "quality", on: true, overlay: "singu-quality" },
  { id: "Q-94", domain: "quality", on: true, overlay: "singu-quality" },
  { id: "Q-95", domain: "quality", on: true, overlay: "singu-quality" },
  { id: "Q-96", domain: "quality", on: false, overlay: "singu-quality" },
  { id: "Q-97", domain: "quality", on: true, overlay: "singu-quality" },
  { id: "Q-98", domain: "quality", on: true, overlay: "singu-quality" },
  { id: "Q-99", domain: "quality", on: true, overlay: "singu-quality" },
  { id: "Q-100", domain: "quality", on: true, overlay: "singu-quality" },
];

// ---------------------------------------------------------------------------
// 状态存储（订阅 + localStorage 持久化）
// ---------------------------------------------------------------------------

const STORAGE_KEY = "variable.singularity.v1";

type Listener = () => void;
const listeners = new Set<Listener>();

function defaultState(): Record<string, SinguFeatureState> {
  const st: Record<string, SinguFeatureState> = {};
  for (const f of SINGU_FEATURES) {
    const params: Record<string, SinguParamValue> = {};
    for (const p of f.params ?? []) params[p.key] = p.default;
    st[f.id] = { on: f.on, params };
  }
  return st;
}

function coerce(raw: unknown): Record<string, SinguFeatureState> | null {
  if (typeof raw !== "object" || raw === null) return null;
  const out: Record<string, SinguFeatureState> = {};
  const known = new Map(SINGU_FEATURES.map((f) => [f.id, f]));
  for (const [id, val] of Object.entries(raw as Record<string, unknown>)) {
    const def = known.get(id);
    if (!def || typeof val !== "object" || val === null) continue;
    const v = val as { on?: unknown; params?: unknown };
    const params: Record<string, SinguParamValue> = {};
    for (const p of def.params ?? []) {
      const pv = (v.params as Record<string, unknown> | undefined)?.[p.key];
      if (typeof pv === typeof p.default) params[p.key] = pv as SinguParamValue;
      else params[p.key] = p.default;
    }
    out[id] = { on: v.on === true, params };
  }
  // 补齐缺失项（升级容错）
  for (const f of SINGU_FEATURES) if (!out[f.id]) out[f.id] = defaultState()[f.id]!;
  return out;
}

let state: Record<string, SinguFeatureState> = defaultState();

function persist(): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    /* 无 localStorage（测试环境）如实跳过 */
  }
}

function load(): void {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return;
    const parsed = coerce(JSON.parse(raw));
    if (parsed) state = parsed;
  } catch {
    /* 损坏数据：回默认（诚实降级，不抛错） */
  }
}

if (typeof window !== "undefined") load();

function emit(): void {
  for (const fn of listeners) fn();
}

/** 订阅状态变化（返回反订阅）。 */
export function subscribeSingu(fn: Listener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function getSinguState(): Record<string, SinguFeatureState> {
  return state;
}

export function setSinguOn(id: string, on: boolean): void {
  if (!state[id] || state[id].on === on) return;
  state = { ...state, [id]: { ...state[id], on } };
  persist();
  emit();
}

export function setSinguParam(id: string, key: string, value: SinguParamValue): void {
  const cur = state[id];
  if (!cur || cur.params[key] === value) return;
  state = { ...state, [id]: { ...cur, params: { ...cur.params, [key]: value } } };
  persist();
  emit();
}

export function resetSinguDomain(domain: SinguDomainId): void {
  const next = { ...state };
  for (const f of SINGU_FEATURES) {
    if (f.domain !== domain) continue;
    next[f.id] = defaultState()[f.id]!;
  }
  state = next;
  persist();
  emit();
}

export function resetSinguAll(): void {
  state = defaultState();
  persist();
  emit();
}

// ---------------------------------------------------------------------------
// 只读便捷读取
// ---------------------------------------------------------------------------

/** 功能是否启用（未知 id 一律 false，安全默认）。 */
export function singuOn(id: string): boolean {
  return state[id]?.on === true;
}

/** 读取参数（带默认回退；类型不匹配回退默认值）。 */
export function singuParam(id: string, key: string): SinguParamValue {
  const def = SINGU_FEATURES.find((f) => f.id === id);
  const p = def?.params?.find((x) => x.key === key);
  const v = state[id]?.params[key];
  if (v === undefined) return p?.default ?? false;
  return v;
}

export function singuNum(id: string, key: string): number {
  const v = singuParam(id, key);
  return typeof v === "number" ? v : 0;
}

export function singuStr(id: string, key: string): string {
  const v = singuParam(id, key);
  return typeof v === "string" ? v : "";
}

export function singuBool(id: string, key: string): boolean {
  const v = singuParam(id, key);
  return v === true;
}

/** 某域是否有任一功能启用（模块整体挂载判据）。 */
export function domainActive(domain: SinguDomainId): boolean {
  return SINGU_FEATURES.some((f) => f.domain === domain && state[f.id]?.on);
}

export function featuresOfDomain(domain: SinguDomainId): SinguFeatureDef[] {
  return SINGU_FEATURES.filter((f) => f.domain === domain);
}

/**
 * 全局运动降级（模块共读；与既有 reduce-motion 令牌同源语义）。
 * App.tsx 以 String(bool) 写入 data-reduce-motion（"true"/"false"），
 * 故只认 "true" 为降级（"false" 与未写入均视为运动允许）。
 */
export function singuMotionOK(): boolean {
  if (typeof document === "undefined") return false;
  return document.documentElement.dataset.reduceMotion !== "true";
}

/** 统计（hub 首页卡片用）。 */
export function singuStats(): { total: number; on: number } {
  let on = 0;
  for (const f of SINGU_FEATURES) if (state[f.id]?.on) on++;
  return { total: SINGU_FEATURES.length, on };
}

/** React 绑定（useSyncExternalStore 快照）。 */
let cachedSnapshot: Record<string, SinguFeatureState> | null = null;
export function singuSnapshot(): Record<string, SinguFeatureState> {
  if (cachedSnapshot !== state) cachedSnapshot = state;
  return cachedSnapshot;
}
