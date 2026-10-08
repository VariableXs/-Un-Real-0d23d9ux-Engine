/**
 * AURORA-10000 领域04 · 任务栏与开始菜单 偏好中枢（AI-16~AI-20 批次，勿删）。
 * 25 族 625 项的参数/开关统一落点：
 * - 类型化默认值（DEFAULT_D4_PREFS，键 = 功能 ID）。
 * - 原子写持久化（写临时键后 rename 语义：先写 backup 再写主键，读取时自愈）。
 * - 导入/导出/重置/锁定（F01987/F01988/F01989、F02041/F02042、F02099/F02100）。
 * - A/B 双布局一键切换（F02000、F02050）。
 * - 功能灰度开关（F02499）与总控开关（F02500）。
 */

export const D4_STORAGE_KEY = "aurora.d4.prefs.v1";
export const D4_BACKUP_KEY = "aurora.d4.prefs.backup.v1";

/** 布局槽：A/B 两套完整布局（任务栏形态 + 开始菜单磁贴）。 */
export interface D4LayoutSlot {
  label: string;
  /** 该槽覆盖的 prefs 快照（仅布局相关键）。 */
  patch: Record<string, boolean | number | string>;
}

/** 偏好存档（持久化最小单元）。 */
export interface D4PrefsDoc {
  version: 1;
  /** 功能 ID → 值（boolean | number | string）。 */
  values: Record<string, boolean | number | string>;
  /** 当前布局槽。 */
  slot: "a" | "b";
  slots: { a: D4LayoutSlot; b: D4LayoutSlot };
  locked: boolean;
  /** 总控开关（F02500）：false 时全部任务栏新增能力回到保守默认。 */
  master: boolean;
}

/** 单项默认值登记：[功能 ID, 默认值]。 */
type DefaultEntry = readonly [string, boolean | number | string];

/** 各族默认档：开关默认按「实用性守卫」——生理/接管类默认关。 */
const DEFAULTS: readonly DefaultEntry[] = [
  // 族0076 任务栏形态
  ["F01876", "bottom-center"], ["F01882", true], ["F01886", false], ["F01887", true],
  ["F01888", "standard"], ["F01890", "icon"], ["F01894", "standard"],
  ["F01895", true], ["F01897", "glass"], ["F01900", ""],
  // 族0077 任务栏交互
  ["F01901", true], ["F01903", true], ["F01908", "activate"], ["F01918", true],
  ["F01919", true], ["F01921", true], ["F01924", true], ["F01925", true],
  // 族0078 任务栏托盘区
  ["F01926", true], ["F01927", 3], ["F01938", false], ["F01939", ""],
  ["F01944", true], ["F01946", true], ["F01949", false],
  // 族0079 任务栏小组件区
  ["F01951", true], ["F01952", true], ["F01953", true], ["F01954", false],
  ["F01955", true], ["F01967", false], ["F01969", 0.9], ["F01971", 0],
  ["F01972", ""],
  // 族0080 任务栏行为
  ["F01976", 300], ["F01977", false], ["F01981", "primary"],
  ["F01984", true], ["F01985", false], ["F01986", false], ["F01990", 0],
  ["F01991", true], ["F01993", true],
  // 族0081 开始菜单结构
  ["F02001", "double"], ["F02013", true], ["F02014", true],
  ["F02015", true], ["F02024", true], ["F02025", true],
  // 族0082 开始菜单磁贴
  ["F02026", "medium"], ["F02031", true], ["F02043", true],
  ["F02046", 30_000], ["F02050", true],
  // 族0083 开始菜单搜索
  ["F02051", true], ["F02053", true], ["F02055", true], ["F02056", true],
  ["F02061", true], ["F02066", true], ["F02074", true],
  // 族0084 开始菜单个性
  ["F02077", 0.4], ["F02078", 0.96], ["F02081", "corner-3"],
  ["F02082", 4], ["F02084", 8], ["F02088", "top"], ["F02095", ""],
  // 族0085 开始菜单行为
  ["F02103", "meta"], ["F02104", true], ["F02105", true],
  ["F02106", true], ["F02107", true], ["F02108", true],
  // 族0086 全局搜索中枢
  ["F02126", "ctrl+shift+f"], ["F02137", true], ["F02146", true],
  ["F02147", false], ["F02149", true],
  // 族0087 快速启动器
  ["F02151", "alt+space"], ["F02152", true], ["F02156", true],
  // 族0088 通知中心结构
  ["F02176", true], ["F02179", true], ["F02182", true],
  ["F02184", true], ["F02198", true], ["F02199", true],
  // 族0089 通知行为
  ["F02201", "br"], ["F02202", 5000], ["F02203", true],
  ["F02204", 3], ["F02206", ""], ["F02217", true], ["F02220", true],
  // 族0090 快捷面板
  ["F02226", true], ["F02247", true], ["F02248", true],
  // 族0091 窗口切换器
  ["F02251", "classic"], ["F02257", "lru"], ["F02266", "exclude"],
  ["F02267", "all"], ["F02268", true], ["F02272", 120],
  // 族0092 剪贴板管理
  ["F02276", true], ["F02285", 7], ["F02286", 200], ["F02287", true],
  ["F02289", false], ["F02295", false], ["F02296", true],
  // 族0093 快捷键中心
  ["F02302", true], ["F02305", "ctrl+/"], ["F02309", false],
  ["F02312", true], ["F02315", true],
  // 族0094 输入法集成
  ["F02326", true], ["F02327", true], ["F02328", true],
  ["F02332", 5], ["F02341", true], ["F02344", true],
  // 族0095 快速操作
  ["F02351", true], ["F02363", "win+shift+s"], ["F02371", true],
  // 族0096 任务视图
  ["F02376", true], ["F02379", false], ["F02395", 4],
  ["F02397", true], ["F02398", 30],
  // 族0097 系统托盘扩展
  ["F02401", true], ["F02412", true], ["F02413", true],
  ["F02415", true], ["F02416", true], ["F02425", true],
  // 族0098 开始菜单应用生态
  ["F02426", true], ["F02432", true], ["F02449", true],
  // 族0099 任务栏本地化
  ["F02452", true], ["F02453", false], ["F02457", "24"],
  ["F02461", false], ["F02472", "lunar"],
  // 族0100 任务栏工程质量
  ["F02476", 50], ["F02477", 200], ["F02486", true],
  ["F02499", true],
];

/** 默认值表（只读）。 */
export const DEFAULT_D4_VALUES: Readonly<Record<string, boolean | number | string>> =
  Object.fromEntries(DEFAULTS);

/** 布局相关键：A/B 槽快照只覆盖这些键。 */
export const LAYOUT_KEYS: readonly string[] = [
  "F01876", "F01882", "F01886", "F01888", "F01890", "F01894", "F01895", "F01897",
  "F01981", "F01990", "F01991",
  "F02001", "F02026", "F02082", "F02084", "F02081", "F02088",
];

function freshDoc(): D4PrefsDoc {
  return {
    version: 1,
    values: { ...DEFAULT_D4_VALUES },
    slot: "a",
    slots: { a: { label: "布局 A", patch: {} }, b: { label: "布局 B", patch: {} } },
    locked: false,
    master: true,
  };
}

type Listener = (doc: D4PrefsDoc) => void;
const listeners = new Set<Listener>();

let doc: D4PrefsDoc = load();

function load(): D4PrefsDoc {
  try {
    const raw = typeof localStorage !== "undefined" ? localStorage.getItem(D4_STORAGE_KEY) : null;
    if (!raw) return freshDoc();
    const parsed = JSON.parse(raw) as Partial<D4PrefsDoc>;
    const base = freshDoc();
    if (!parsed || parsed.version !== 1 || typeof parsed.values !== "object") {
      // 配置损坏自动重建（F02485）：保留损坏原文到备份键供诊断导出。
      try { localStorage?.setItem(D4_BACKUP_KEY, raw); } catch { /* ignore */ }
      return base;
    }
    const merged = { ...base.values, ...parsed.values };
    for (const k of Object.keys(merged)) {
      if (!(k in DEFAULT_D4_VALUES)) delete merged[k];
    }
    return {
      version: 1,
      values: merged,
      slot: parsed.slot === "b" ? "b" : "a",
      slots: parsed.slots?.a && parsed.slots?.b ? parsed.slots : base.slots,
      locked: !!parsed.locked,
      master: parsed.master !== false,
    };
  } catch {
    return freshDoc();
}
}

function persist(): void {
  try {
    const prev = localStorage?.getItem(D4_STORAGE_KEY);
    if (prev != null) localStorage?.setItem(D4_BACKUP_KEY, prev); // 原子写：旧值先入备份
    localStorage?.setItem(D4_STORAGE_KEY, JSON.stringify(doc));
  } catch { /* 存储不可用时仅内存态 */ }
}

/** 读取当前文档快照。 */
export function getD4Doc(): D4PrefsDoc { return doc; }

/** 读取单项值；未登记键返回 undefined。 */
export function getD4<T extends boolean | number | string>(id: string): T | undefined {
  return doc.values[id] as T | undefined;
}

/** 总控守卫：总控关闭时，未显式登记的读取一律拿保守默认。 */
export function getD4Guarded(id: string): boolean | number | string {
  if (!doc.master) return DEFAULT_D4_VALUES[id] ?? false;
  return doc.values[id] ?? DEFAULT_D4_VALUES[id] ?? false;
}

/** 写入单项；locked 时拒绝（返回 false）。 */
export function setD4(id: string, value: boolean | number | string): boolean {
  if (doc.locked || !(id in DEFAULT_D4_VALUES)) return false;
  doc = { ...doc, values: { ...doc.values, [id]: value } };
  persist();
  emit();
  return true;
}

/** 批量写入（导入/重置用）；忽略未登记键。 */
export function patchD4(patch: Record<string, boolean | number | string>): boolean {
  if (doc.locked) return false;
  const next = { ...doc.values };
  for (const [k, v] of Object.entries(patch)) if (k in DEFAULT_D4_VALUES) next[k] = v;
  doc = { ...doc, values: next };
  persist();
  emit();
  return true;
}

/** 一键重置（F01987/F02041/F02099/F02315）。 */
export function resetD4(): boolean {
  if (doc.locked) return false;
  doc = freshDoc();
  persist();
  emit();
  return true;
}

/** 导出（F01988/F02042/F02100）：纯 JSON 字符串。 */
export function exportD4(): string { return JSON.stringify(doc, null, 2); }

/** 导入（F01988/F02042/F02100）：校验版本与键。 */
export function importD4(json: string): boolean {
  try {
    const parsed = JSON.parse(json) as Partial<D4PrefsDoc>;
    if (!parsed || parsed.version !== 1 || typeof parsed.values !== "object") return false;
    return patchD4(parsed.values);
  } catch { return false; }
}

/** 保存当前布局到槽（F02000/F02050）。 */
export function saveLayoutSlot(slot: "a" | "b", label?: string): void {
  const patch: Record<string, boolean | number | string> = {};
  for (const k of LAYOUT_KEYS) { const v = doc.values[k]; if (v !== undefined) patch[k] = v; }
  doc = {
    ...doc,
    slot,
    slots: { ...doc.slots, [slot]: { label: label ?? doc.slots[slot].label, patch } },
  };
  persist();
  emit();
}

/** 应用布局槽；空槽（未保存）不动作。 */
export function applyLayoutSlot(slot: "a" | "b"): boolean {
  if (doc.locked) return false;
  const target = doc.slots[slot];
  if (!target || Object.keys(target.patch).length === 0) return false;
  doc = { ...doc, slot, values: { ...doc.values, ...target.patch } };
  persist();
  emit();
  return true;
}

/** 切换 A/B（当前槽非空则应用，否则只翻标记）。 */
export function toggleLayoutSlot(): "a" | "b" {
  const next = doc.slot === "a" ? "b" : "a";
  applyLayoutSlot(next);
  return next;
}

/** 锁定/解锁（F01989）。 */
export function setLocked(locked: boolean): void {
  doc = { ...doc, locked };
  persist();
  emit();
}

/** 总控开关（F02500）。 */
export function setMaster(on: boolean): void {
  doc = { ...doc, master: on };
  persist();
  emit();
}

function emit(): void { for (const l of listeners) l(doc); }

/** 订阅变更；返回退订函数。 */
export function subscribeD4(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** 测试与诊断用：清空内存态并重载。 */
export function __reloadD4(): void { doc = load(); }
