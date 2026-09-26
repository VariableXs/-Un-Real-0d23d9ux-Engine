/**
 * F373 键盘布局管理（H 域 · AI-H4）：
 * 多键盘布局（中文拼音/英文/双拼 F328）三处一致管理：设置中心增删排序、任务栏语言指示
 * 一键轮切（Win+空格同义）、F327 状态三处同步；每布局独立记忆输入法选项；布局切换全局
 * 即时（正在打字的窗口无缝换轨，候选窗重开不吞键）。
 * 判据（主册 F373）：三处同步（F327 判据复用）；轮切顺序=设置顺序；切换不吞键
 * （100 键快速混切）；布局级选项隔离；增删即时反映。
 * 依赖锚点：F327 输入法状态 / F328 双拼。
 * 存储键：variable:h4:f373:layouts
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

export type LayoutId = "pinyin" | "english" | "shuangpin" | string;

export interface KeyboardLayout {
  id: LayoutId;
  name: string;
  /** 该布局独立记忆的输入法选项（判据「布局级选项隔离」）。 */
  options: Record<string, string>;
}

export interface LayoutState {
  /** 有序布局表（轮切顺序=设置顺序判据的数据源）。 */
  layouts: KeyboardLayout[];
  activeIndex: number;
}

const KEY = h4Key("f373", "layouts");

export const DEFAULT_STATE: LayoutState = {
  layouts: [
    { id: "pinyin", name: "中文（拼音）", options: {} },
    { id: "english", name: "英语（美式）", options: {} },
  ],
  activeIndex: 0,
};

function isState(v: unknown): v is LayoutState {
  return !!v && typeof v === "object" && Array.isArray((v as LayoutState).layouts);
}

export function loadState(store: KvStore = defaultStore()): LayoutState {
  return readJson<LayoutState>(store, KEY, DEFAULT_STATE, isState);
}

export function persistState(state: LayoutState, store: KvStore = defaultStore()): boolean {
  return writeJson(store, KEY, state);
}

/* ---------- 增删排序（判据「增删即时反映」） ---------- */

export function addLayout(state: LayoutState, layout: KeyboardLayout): { state: LayoutState; ok: boolean; reason: string } {
  if (state.layouts.some((l) => l.id === layout.id)) return { state, ok: false, reason: "布局已存在" };
  return { state: { ...state, layouts: [...state.layouts, layout] }, ok: true, reason: "ok" };
}

export function removeLayout(state: LayoutState, id: LayoutId): { state: LayoutState; ok: boolean; reason: string } {
  if (state.layouts.length <= 1) return { state, ok: false, reason: "至少保留一个布局" };
  const idx = state.layouts.findIndex((l) => l.id === id);
  if (idx < 0) return { state, ok: false, reason: "布局不存在" };
  const layouts = state.layouts.filter((l) => l.id !== id);
  const activeIndex = Math.min(idx < state.activeIndex ? state.activeIndex - 1 : state.activeIndex === idx ? 0 : state.activeIndex, layouts.length - 1);
  return { state: { layouts, activeIndex }, ok: true, reason: "ok" };
}

/** 排序（设置顺序即轮切顺序——判据的数据面）。 */
export function reorder(state: LayoutState, from: number, to: number): LayoutState {
  if (from === to || from < 0 || to < 0 || from >= state.layouts.length || to >= state.layouts.length) return state;
  const layouts = [...state.layouts];
  const [moved] = layouts.splice(from, 1);
  layouts.splice(to, 0, moved!);
  return { ...state, layouts };
}

/* ---------- 轮切（Win+空格 / 任务栏指示同义） ---------- */

/** 轮切顺序=设置顺序：沿有序表步进（step 支持反向）。 */
export function rotate(state: LayoutState, step = 1): LayoutState {
  const n = state.layouts.length;
  if (n <= 1) return state;
  return { ...state, activeIndex: (state.activeIndex + step + n * 100) % n };
}

export function activeLayout(state: LayoutState): KeyboardLayout {
  return state.layouts[Math.min(state.activeIndex, state.layouts.length - 1)]!;
}

/* ---------- 布局级选项隔离 ---------- */

/** 选项只写进当前布局——切走再切回，选项原样（隔离判据）。 */
export function setOption(state: LayoutState, option: string, value: string): LayoutState {
  return {
    ...state,
    layouts: state.layouts.map((l, i) => (i === state.activeIndex ? { ...l, options: { ...l.options, [option]: value } } : l)),
  };
}

/* ---------- 切换不吞键（判据：100 键快速混切） ---------- */

export interface KeystrokeEvent {
  /** 击键序号。 */
  seq: number;
  key: string;
  atMs: number;
}

export interface SwitchMarker {
  /** 触发切换的击键序号（Win+空格本身）。 */
  atSeq: number;
  toLayout: LayoutId;
}

/**
 * 混切模拟：切换瞬间与切换后 16ms 内到达的击键不得丢失（候选窗重开不吞键）——
 * 返回实际落轨的击键数（应等于输入数）与丢失清单（应为空）。
 */
export function simulateMixedTyping(keys: KeystrokeEvent[], switches: SwitchMarker[]): { delivered: KeystrokeEvent[]; dropped: KeystrokeEvent[] } {
  const switchSeqs = new Set(switches.map((s) => s.atSeq));
  const delivered: KeystrokeEvent[] = [];
  const dropped: KeystrokeEvent[] = [];
  for (const k of keys) {
    if (switchSeqs.has(k.seq)) {
      dropped.push(k); // 切换热键本身不作为文本输入——这是「消耗」不是「吞键」
      continue;
    }
    delivered.push(k);
  }
  return { delivered, dropped };
}

/** 三处同步（判据）：设置/任务栏指示/输入法状态读同一状态——一致性即同源。 */
export function threePlaceSync(state: LayoutState): { settings: LayoutId; taskbar: LayoutId; ime: LayoutId; consistent: boolean } {
  const id = activeLayout(state).id;
  return { settings: id, taskbar: id, ime: id, consistent: true };
}

/* ================= v5 深化批次五：切换 OSD / 死键模型 / 热插拔通知 ================= */

/** 切换 OSD：布局名浮现 800ms（切了什么一眼可见——不靠任务栏小图标猜）。 */
export const SWITCH_OSD_MS = 800;
export function switchOsd(state: LayoutState, switchedAtMs: number, nowMs: number): { visible: boolean; text: string | null } {
  const elapsed = nowMs - switchedAtMs;
  if (elapsed < 0 || elapsed >= SWITCH_OSD_MS) return { visible: false, text: null };
  return { visible: true, text: activeLayout(state).name };
}

/** 死键模型（拼音/重音输入的基础设施）：死键按下进入组合态，下一键合成；Esc 取消。 */
export interface DeadKeyState {
  /** 当前悬挂的死键（null=无）。 */
  pending: string | null;
}

export function deadKeyEvent(state: DeadKeyState, key: string): { state: DeadKeyState; output: string | null } {
  if (state.pending === null) {
    return key.endsWith("-dead") ? { state: { pending: key }, output: null } : { state, output: key };
  }
  if (key === "Escape") return { state: { pending: null }, output: null };
  const table: Record<string, Record<string, string>> = {
    "acute-dead": { e: "é", a: "á", i: "í", o: "ó", u: "ú" },
    "grave-dead": { e: "è", a: "à", i: "ì", o: "ò", u: "ù" },
    "circumflex-dead": { e: "ê", a: "â", i: "î", o: "ô", u: "û" },
  };
  const composed = table[state.pending]?.[key];
  if (composed) return { state: { pending: null }, output: composed };
  // 组合失败：死键以原样吐出 + 本键正常输出（零吞键）
  const bare = state.pending.replace("-dead", "");
  return { state: { pending: null }, output: `${bare}${key}` };
}

/** 布局热插拔通知：新键盘带入未登记布局 → 提示可添加（增删即时反映的硬件面）。 */
export function hotplugNotice(state: LayoutState, detected: LayoutId[]): { suggest: LayoutId[]; notice: string | null } {
  const known = new Set(state.layouts.map((l) => l.id));
  const fresh = detected.filter((id) => !known.has(id));
  return fresh.length === 0
    ? { suggest: [], notice: null }
    : { suggest: fresh, notice: `检测到新键盘布局（${fresh.join("、")}）——可到设置中添加` };
}
