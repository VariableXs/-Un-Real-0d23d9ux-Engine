/**
 * NOVA-200 · 域5 输入节奏路（AI-05）—— 键盘与输入手感（W-051…W-063）。
 *
 * 域界（全景 §5）：M-29 管系统全局重复率、Z-24 管主动字符面板、Q-40 管文件命名、
 * Z-12 管列表式速查、U-54 管日程节律、Q-46 管粘贴队列、Z-17 管IME兼容层；
 * 本模块补**分应用节奏、训练与反查、速度反馈、健康照护、热剪贴板与标点智断**。十三项：
 *   W-051 分应用输入节奏记忆  W-052 Shift 大写覆盖  W-053 输入节奏器
 *   W-054 按键长按池          W-055 本地短语暖场  W-056 键盘沙坑
 *   W-057 打字速度光环        W-058 高手时刻      W-059 走神暂停印
 *   W-060 剪贴板环槽          W-061 长句呼吸输入  W-062 指频热区图
 *   W-063 标点智断
 *
 * 纪律（实施总纲 §1/§3）：
 * - 零侵入：全局 keydown/keyup 桥仅在 capture 侧观察（除明确声明的 preventDefault
 *   场景：W-054 长按池、W-060 环槽、W-056 沙坑捕获态），不改任何既有组件内部逻辑；
 * - 前缀：类名 `nova-input-`、事件 `nova://input-*`、存储 `nova.input.*`；
 * - 降级链：reduce-motion / safeMode / static 三态下一切动效归零（motionOK=false）；
 *   非 DOM 环境（vitest node）行为层安全 no-op；
 * - 默认档：手感覆盖类（W-052）默认关；训练/面板类（W-053/056/062）仅用户主动
 *   触发；其余默认开（与 S0 注册表一致）。
 *
 * 诚实边界：
 * - W-056 反查源为 Z-08 键位注册表（`src/lib/keymap/registry.ts`）只读快照，
 *   与注册表 100% 一致；注册表为空时如实显示 FREE，不编造绑定；
 * - W-055 隐身会话（U-36）联动：`setStealth(true)` 或 `nova://input/stealth`
 *   事件推送期间停止记录且不出幽灵建议（S17 接 datavault inc 状态）；
 * - W-060 环槽为纯内存三槽，会话结束即清（跨会话不保留，隐私默认）；
 * - W-054 文本落点经 `insertText` 注入焦点可编辑元素；无可编辑焦点时仅派发
 *   `nova://input-pool` 事件，不越权注入；
 * - W-051 行为层仅记录本环境焦点应用的击键间隔（VWM store 只读）。
 */

import { vwmStore } from "../../windows/vwm";
import { snapshot as keymapSnapshot } from "../../../lib/keymap/registry";
import type { KeyBinding } from "../../../lib/keymap/types";

// ---------------------------------------------------------------------------
// 契约（S0 新星运行时 / 新星中枢消费的最小接口；S0 未落地时自持降级）
// ---------------------------------------------------------------------------

export interface NovaFeatureMeta {
  id: string;
  title: string;
  titleEn: string;
  desc: string;
  defaultOn: boolean;
  params?: Record<string, { def: number | string | boolean; note: string }>;
}

export interface InputNovaCtx {
  on: (id: string) => boolean;
  num: (id: string, key: string) => number;
  bool: (id: string, key: string) => boolean;
  str: (id: string, key: string) => string;
  motionOK: () => boolean;
}

export interface InputNovaHandle {
  activate(ctx?: InputNovaCtx): void;
  deactivate(): void;
  api: {
    openRhythmTrainer(): void;
    openSandbox(): void;
    toggleZonePanel(force?: boolean): void;
    clearZones(): void;
    setStealth(active: boolean): void;
    clipPut(slot: 0 | 1 | 2): boolean;
    clipTake(slot: 0 | 1 | 2): boolean;
    paceOf(appId: string): { delayMs: number; rampMs: number; samples: number } | null;
  };
}

// 事件名（nova:// 前缀纪律）
export const INPUT_OPEN_EVENT = "nova://input/open"; // detail { panel: "rhythm" | "sandbox" | "zones" }
export const INPUT_STEALTH_EVENT = "nova://input/stealth"; // detail { active: boolean }（U-36 联动）
export const INPUT_POOL_EVENT = "nova://input/pool"; // detail { key, char }（W-054 无焦点落点时）
export const INPUT_INSERT_EVENT = "nova://input/insert"; // detail { text }（环槽/短语采纳兜底）
export const INPUT_CAPS_EVENT = "nova://input/caps"; // detail { capsOn, shiftHeld, effective, imeEn }

// ---------------------------------------------------------------------------
// Hub 注册清单（S0 新星中枢消费：功能卡 + 参数 + 降级说明）
// ---------------------------------------------------------------------------

export const INPUT_NOVA_FEATURES: NovaFeatureMeta[] = [
  {
    id: "W-051",
    title: "分应用输入节奏记忆",
    titleEn: "Pace Memory",
    desc: "各应用独立的按键重复曲线：按近 500 次击键间隔中位数自动校准，样本 < 200 击退回全局曲线；应用切换 ≤ 50ms 生效。",
    defaultOn: true,
  },
  {
    id: "W-052",
    title: "Shift 大写覆盖",
    titleEn: "Shift Caps Override",
    desc: "按住 Shift 期间临时反转 Caps 状态（XOR 语义），不改变 Caps 灯；IME 中文态自动跳过，仅英文态生效。",
    defaultOn: false,
  },
  {
    id: "W-053",
    title: "输入节奏器",
    titleEn: "Rhythm Trainer",
    desc: "30s 击键间隔可视化训练：每击一柱、理想节奏带为参考区，结束给变异系数得分与建议；训练数据关窗即焚，零留存零上传。",
    defaultOn: true,
  },
  {
    id: "W-054",
    title: "按键长按池",
    titleEn: "Long-Press Pool",
    desc: "无修饰键长按 . / 、 / ; 500ms 弹出标点池径向环，滚轮或方向选择、松开上屏；仅焦点可编辑元素内生效。",
    defaultOn: true,
  },
  {
    id: "W-055",
    title: "本地短语暖场",
    titleEn: "Phrase Warm-up",
    desc: "输入框获焦浮出本地高频短语幽灵建议（≤3 条），Tab 采纳；纯本地频率表，U-36 隐身会话期间自动禁用。",
    defaultOn: true,
  },
  {
    id: "W-056",
    title: "键盘沙坑",
    titleEn: "Key Trainer",
    desc: "F1 长按 800ms 呼出沙坑：随手按任意组合实时反查 Z-08 注册表的真实绑定；未绑定如实显示 FREE；Esc 两段退出。",
    defaultOn: true,
  },
  {
    id: "W-057",
    title: "打字速度光环",
    titleEn: "Speed Aura",
    desc: "30s 滚动 WPM 映射焦点输入框外圈光环：≥60 暖亮、30–60 稳态白、<30 缓呼吸提示休息；失焦即隐。",
    defaultOn: true,
  },
  {
    id: "W-058",
    title: "高手时刻",
    titleEn: "WPM Moment",
    desc: "30s 均速首次突破 80/100/120 WPM 时输入框下沿绽放 1.2s 微型彩带，同一门槛一生只庆一次（本机记录）。",
    defaultOn: true,
  },
  {
    id: "W-059",
    title: "走神暂停印",
    titleEn: "Flow Recall",
    desc: "输入流中断 > 3s 在任务栏右端亮一枚 8px 呼吸光点 1.5s（非弹窗零打断）；同 5 分钟至多 1 枚，全屏应用内不出现。",
    defaultOn: true,
  },
  {
    id: "W-060",
    title: "剪贴板环槽",
    titleEn: "Clip Slots",
    desc: "Ctrl+Shift+1/2/3 存入三格热剪贴板、Ctrl+Alt+1/2/3 直取直贴；槽空如实提示 EMPTY；纯内存，跨会话不保留。",
    defaultOn: true,
  },
  {
    id: "W-061",
    title: "长句呼吸输入",
    titleEn: "Marathon Typing Care",
    desc: "连续输入 ≥ 300 字无 ≥ 30s 停顿浮出 4s 呼吸气泡泡，每小时至多 3 次；计数含 IME 组段，勿扰下降级为颜色变化。",
    defaultOn: true,
    params: { chars: { def: 300, note: "呼吸提示字数阈值（100–900）" } },
  },
  {
    id: "W-062",
    title: "指频热区图",
    titleEn: "Zone Heatmap",
    desc: "标准 QWERTY 十区（左右手各小指→拇指）击键负载热力图与月度对比；数据仅本机，可一键清零。",
    defaultOn: true,
  },
  {
    id: "W-063",
    title: "标点智断",
    titleEn: "Smart Punct",
    desc: "中文语境句末英文点在下一字符落定后 30ms 内智断：续中文自动转句号；数字与 URL 场景内置豁免；可关。",
    defaultOn: true,
  },
];

export const inputNovaDomain = {
  id: "S5",
  nameZh: "输入节奏",
  nameEn: "Input Rhythm",
  route: "AI-05",
  features: INPUT_NOVA_FEATURES,
} as const;

// ---------------------------------------------------------------------------
// 通用工具（本模块自持）
// ---------------------------------------------------------------------------

export function clamp(v: number, min: number, max: number): number {
  return Math.min(Math.max(v, min), max);
}

/** reduce-motion / safeMode / static 三态统一判定。 */
export function motionOK(): boolean {
  if (typeof document === "undefined") return false;
  const ds = document.documentElement.dataset;
  return ds.reduceMotion !== "true" && ds.safeMode !== "true" && ds.staticMode !== "true";
}

function lsGet<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw == null) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

function lsSet(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* storage full/blocked → 不持久化 */
  }
}

const REGISTRY_KEY = "nova.registry.v1";
const NS = "nova.input";

/**
 * 功能开关：优先消费 S0 注册表（`nova.registry.v1`：`{ "W-051": { on: true } }`），
 * 防御性兼容直接布尔与 `{ features: { "W-051": { enabled } } }` 两种形态；
 * 无注册表 / 解析失败 → 用本 manifest 的 defaultOn（诚实降级，不报错）。
 */
export function flagOn(id: string): boolean {
  const def = INPUT_NOVA_FEATURES.find((f) => f.id === id);
  const fallback = def?.defaultOn ?? false;
  try {
    const raw = localStorage.getItem(REGISTRY_KEY);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const direct = parsed[id];
    if (typeof direct === "boolean") return direct;
    if (direct && typeof direct === "object" && typeof (direct as { on?: unknown }).on === "boolean") {
      return (direct as { on: boolean }).on;
    }
    const feats = parsed.features as Record<string, { enabled?: boolean }> | undefined;
    const fe = feats?.[id];
    if (fe && typeof fe.enabled === "boolean") return fe.enabled;
    return fallback;
  } catch {
    return fallback;
  }
}

/** S0 注册表数值参数（缺失回 manifest 默认）。 */
export function paramNum(id: string, key: string, fallback: number): number {
  try {
    const raw = localStorage.getItem(REGISTRY_KEY);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const direct = parsed[id] as { params?: Record<string, unknown> } | undefined;
    const v = direct?.params?.[key];
    return typeof v === "number" ? v : fallback;
  } catch {
    return fallback;
  }
}

/** 派发 `nova://input-*` 事件（SSR/测试环境安全）。 */
export function novaEvent(name: string, detail?: unknown): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  window.dispatchEvent(new CustomEvent(`nova://input-${name}`, { detail }));
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-051 分应用输入节奏记忆
// ---------------------------------------------------------------------------

/** 样本不足该击数 → 退回全局曲线（验收：样本 < 200 击退回全局）。 */
export const PACE_MIN_SAMPLES = 200;
/** 每应用击键间隔滚动窗口上限。 */
export const PACE_WINDOW = 500;
/** 全局兜底曲线（与 M-29 默认一致量级）。 */
export const PACE_GLOBAL = { delayMs: 250, rampMs: 60 } as const;

/** 中位数（偶数取均值；空 → null）。 */
export function medianOf(nums: number[]): number | null {
  const a = nums.filter((n) => Number.isFinite(n) && n >= 0).sort((x, y) => x - y);
  if (a.length === 0) return null;
  const mid = a.length >> 1;
  return a.length % 2 === 1 ? a[mid]! : (a[mid - 1]! + a[mid]!) / 2;
}

/** 滚动记录：追加并裁剪到窗口上限。 */
export function pushGap(gaps: number[], gapMs: number, max = PACE_WINDOW): number[] {
  if (!(gapMs > 0) || gapMs > 30_000) return gaps; // 离屏/挂起的大间隔不入曲线
  const next = [...gaps, gapMs];
  return next.slice(Math.max(0, next.length - max));
}

export interface PaceCurve {
  delayMs: number;
  rampMs: number;
  samples: number;
}

/**
 * 间隔中位数 → 重复曲线：快节奏（中位数小）→ 更短延迟与爬升；
 * 单调：中位数越小 delay/ramp 越小；样本 < PACE_MIN_SAMPLES → null（退回全局）。
 */
export function calibratePace(gaps: number[]): PaceCurve | null {
  const med = medianOf(gaps);
  if (med == null || gaps.length < PACE_MIN_SAMPLES) return null;
  return {
    delayMs: clamp(Math.round(med * 0.5), 150, 500),
    rampMs: clamp(Math.round(med * 0.25), 20, 200),
    samples: gaps.length,
  };
}

/** 应用切换时的 ≤50ms 读法：同步查表，未命中退全局。 */
export function paceOfApp(cache: Record<string, PaceCurve>, appId: string): PaceCurve {
  return cache[appId] ?? { ...PACE_GLOBAL, samples: 0 };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-052 Shift 大写覆盖
// ---------------------------------------------------------------------------

/**
 * 覆盖语义：按住 Shift 期间临时反转 Caps（XOR）；松开恢复。
 * 注意这是「本环境输入层」的意图模型——不改 Caps 灯（真实灯由 OS 管）。
 */
export function effectiveCaps(capsOn: boolean, shiftHeld: boolean): boolean {
  return capsOn !== shiftHeld;
}

/** IME 仲裁：仅英文态生效，中文态自动跳过。 */
export function capsAppliesToIme(ime: "en" | "zh"): boolean {
  return ime === "en";
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-053 输入节奏器
// ---------------------------------------------------------------------------

/** 单次训练窗口。 */
export const RHYTHM_WINDOW_MS = 30_000;

export interface RhythmScore {
  /** 稳定性得分 0..100（= (1 - 变异系数) 截断，可解释）。 */
  stability: number;
  /** 变异系数（std/mean），得分依据。 */
  cv: number;
  /** 击键数。 */
  hits: number;
  band: "stable" | "wavy" | "erratic";
}

/** 得分公式：cv = std/mean（样本 < 3 → null，如实不评）。 */
export function rhythmScore(gapsMs: number[]): RhythmScore | null {
  const g = gapsMs.filter((n) => Number.isFinite(n) && n > 0);
  if (g.length < 3) return null;
  const mean = g.reduce((a, b) => a + b, 0) / g.length;
  const variance = g.reduce((a, b) => a + (b - mean) ** 2, 0) / g.length;
  const cv = Math.sqrt(variance) / mean;
  const stability = Math.round(clamp((1 - cv) * 100, 0, 100));
  return { stability, cv: Math.round(cv * 100) / 100, hits: g.length + 1, band: cv <= 0.35 ? "stable" : cv <= 0.6 ? "wavy" : "erratic" };
}

/** 建议（band → 一行建议；hub 展示）。 */
export function rhythmAdvice(s: RhythmScore, lang: "zh" | "en" = "zh"): string {
  if (s.band === "stable") return lang === "en" ? "Stable groove. Keep it." : "节奏稳定，保持这个手感。";
  if (s.band === "wavy") return lang === "en" ? "Slight drift — slow down 10%." : "略有漂移——放慢 10% 更稳。";
  return lang === "en" ? "Erratic rhythm — try a metronome pace." : "节奏波动大——试试匀速敲击。";
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-054 按键长按池
// ---------------------------------------------------------------------------

/** 长按触发时长。 */
export const POOL_HOLD_MS = 500;

/** 三池定义（spec：. → 。．… / 、 → ，、；： / ; → ；;:|）；`\` 为 、 池的别名源。 */
export const POOLS: Record<string, string[]> = {
  ".": ["。", "．", "…"],
  "、": ["，", "、", "；", "："],
  ";": ["；", ";", ":", "|"],
};

export function poolFor(key: string): string[] | null {
  const k = key === "\\" ? "、" : key;
  const pool = POOLS[k];
  return pool ? [...pool] : null;
}

/** 径向环布局：n 个符号位，从正上方起顺时针均分；返回相对中心坐标（y 向下）。 */
export function radialLayout(n: number, radius: number): Array<{ x: number; y: number }> {
  if (n <= 0) return [];
  const out: Array<{ x: number; y: number }> = [];
  for (let i = 0; i < n; i++) {
    const angle = -Math.PI / 2 + (i * 2 * Math.PI) / n;
    out.push({ x: Math.round(Math.cos(angle) * radius), y: Math.round(Math.sin(angle) * radius) });
  }
  return out;
}

/** 环上选择步进（dir=+1 顺时针 / -1 逆时针，模 n）。 */
export function nextRingIndex(current: number, dir: 1 | -1, n: number): number {
  if (n <= 0) return 0;
  return ((current + dir) % n + n) % n;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-055 本地短语暖场
// ---------------------------------------------------------------------------

/** 入围最低出现次数（防一次性输入混入）。 */
export const PHRASE_MIN_COUNT = 3;
/** 短语长度边界（过短噪声、过长非短语）。 */
export const PHRASE_MIN_LEN = 2;
export const PHRASE_MAX_LEN = 24;
/** 每应用频率表上限。 */
export const PHRASE_MAX_ENTRIES = 200;

/** 规范化一行候选：压缩空白、去首尾、长度过滤。 */
export function normalizePhrase(raw: string): string | null {
  const s = raw.replace(/\s+/g, " ").trim();
  if (s.length < PHRASE_MIN_LEN || s.length > PHRASE_MAX_LEN) return null;
  return s;
}

/** 幽灵建议：频次 ≥ min，按频次降序、同频按字典序稳定排序，取前 n。 */
export function ghostPhrases(freq: Record<string, number>, n = 3, minCount = PHRASE_MIN_COUNT): string[] {
  return Object.entries(freq)
    .filter(([, c]) => c >= minCount)
    .sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1))
    .slice(0, Math.max(0, n))
    .map(([k]) => k);
}

/** 频率表记录（cap 条目数，频次封顶 999 防溢出膨胀）。 */
export function bumpPhrase(freq: Record<string, number>, phrase: string): Record<string, number> {
  const p = normalizePhrase(phrase);
  if (!p) return freq;
  const next = { ...freq, [p]: Math.min(999, (freq[p] ?? 0) + 1) };
  if (Object.keys(next).length <= PHRASE_MAX_ENTRIES) return next;
  // 超限：淘汰频次最低的一半（同频按字典序，保证确定性）
  const entries = Object.entries(next).sort((a, b) => a[1] - b[1] || (a[0] < b[0] ? -1 : 1));
  const keep = new Map(entries.slice(Math.floor(entries.length / 2)));
  const out: Record<string, number> = {};
  for (const [k, v] of keep) out[k] = v;
  return out;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-056 键盘沙坑（反查源 = Z-08 注册表只读）
// ---------------------------------------------------------------------------

/** F1 长按呼出时长。 */
export const SANDBOX_HOLD_MS = 800;

/** 键盘事件 → 规范 combo（与 Z-08 normalizeAccel 同序：ctrl/alt/shift/super + 键尾）。 */
export function comboFromEvent(e: { ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean; key: string }): string | null {
  const key = e.key;
  if (!key) return null;
  const k = key.length === 1 ? key.toLowerCase() : key.toLowerCase();
  if (["control", "alt", "shift", "meta", "capslock"].includes(k)) return null; // 纯修饰键不成 combo
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("ctrl");
  if (e.altKey) mods.push("alt");
  if (e.shiftKey) mods.push("shift");
  if (e.metaKey) mods.push("super");
  return [...mods, k].join("+");
}

export interface SandboxHit {
  combo: string;
  bound: boolean;
  id?: string;
  descKey?: string;
  scope?: KeyBinding["scope"];
  source?: string;
}

/** 反查：与 Z-08 注册表快照 100% 一致（同 normalizeAccel 串匹配）；未命中 → FREE。 */
export function lookupBinding(combo: string, bindings: KeyBinding[]): SandboxHit {
  const hit = bindings.find((b) => b.combo === combo);
  if (!hit) return { combo, bound: false };
  return { combo, bound: true, id: hit.id, descKey: hit.descKey, scope: hit.scope, source: hit.source };
}

/** Esc 两段退出：有已捕获组合 → 第一段清捕获，否则直接关沙坑。 */
export function nextEscState(hasCapture: boolean): "clear" | "close" {
  return hasCapture ? "clear" : "close";
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-057 打字速度光环 / W-058 高手时刻
// ---------------------------------------------------------------------------

/** 滚动测速窗口（30s，与 W-058 均速同源）。 */
export const WPM_WINDOW_MS = 30_000;
/** 光环阈值。 */
export const AURA_WARM_WPM = 60;
export const AURA_STEADY_WPM = 30;

/**
 * 滚动 WPM：窗口内击键数 / 5 ×（60000/窗口ms）。
 * 窗口为空或超窗 → 0。
 */
export function rollingWpm(hitTimes: number[], now: number, windowMs = WPM_WINDOW_MS): number {
  const inWin = hitTimes.filter((t) => t > 0 && now - t <= windowMs);
  if (inWin.length === 0) return 0;
  const spanMs = Math.min(now - Math.min(...inWin), windowMs);
  if (spanMs <= 0) return 0;
  return Math.round((inWin.length / 5) * (60_000 / Math.max(spanMs, 3_000)));
}

export interface AuraState {
  mode: "warm" | "steady" | "rest" | "idle";
  intensity: number;
}

/** WPM → 光环状态（0 击 → idle 不显示；<30 → rest 呼吸提示休息）。 */
export function auraState(wpm: number): AuraState {
  if (wpm <= 0) return { mode: "idle", intensity: 0 };
  if (wpm >= AURA_WARM_WPM) return { mode: "warm", intensity: 1 };
  if (wpm >= AURA_STEADY_WPM) return { mode: "steady", intensity: 0.6 };
  return { mode: "rest", intensity: 0.3 };
}

/** 三门槛（W-058）。 */
export const WPM_THRESHOLDS = [80, 100, 120] as const;
/** 彩带绽放时长。 */
export const WPM_MOMENT_MS = 1200;

/** 首破门槛：返回本次新跨过的门槛（一门槛一次，已庆过不再触发）。 */
export function crossedThresholds(wpm: number, already: number[]): number[] {
  return WPM_THRESHOLDS.filter((t) => wpm >= t && !already.includes(t)) as unknown as number[];
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-059 走神暂停印
// ---------------------------------------------------------------------------

/** 输入流中断判定。 */
export const FLOW_GAP_MS = 3_000;
/** 同 5 分钟至多 1 枚。 */
export const RECALL_RATE_MS = 5 * 60_000;
/** 呼吸时长。 */
export const RECALL_BREATH_MS = 1_500;

/** 中断超 3s 且距上次 ≥ 5min 才出印。 */
export function shouldRecall(gapMs: number, lastShownAt: number, now: number): boolean {
  return gapMs >= FLOW_GAP_MS && now - lastShownAt >= RECALL_RATE_MS;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-060 剪贴板环槽
// ---------------------------------------------------------------------------

/** 三槽，纯内存（模块级，activate 时清空）。 */
export interface ClipSlots {
  slots: [string | null, string | null, string | null];
}

export function emptySlots(): ClipSlots {
  return { slots: [null, null, null] };
}

/** 存入（覆盖同槽；截断 100KB 防巨文本拖慢）。 */
export function slotPut(s: ClipSlots, i: 0 | 1 | 2, text: string): ClipSlots {
  if (typeof text !== "string" || text.length === 0) return s;
  const t = text.length > 100_000 ? text.slice(0, 100_000) : text;
  const slots = [...s.slots] as ClipSlots["slots"];
  slots[i] = t;
  return { slots };
}

/** 直取：槽空 → { ok:false, empty:true }（HUD 如实 EMPTY）。 */
export function slotTake(s: ClipSlots, i: 0 | 1 | 2): { ok: boolean; empty: boolean; text: string } {
  const t = s.slots[i];
  if (!t) return { ok: false, empty: true, text: "" };
  return { ok: true, empty: false, text: t };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-061 长句呼吸输入
// ---------------------------------------------------------------------------

/** 连续判定：停顿 ≥ 该值重置计数。 */
export const BREATH_PAUSE_RESET_MS = 30_000;
/** 气泡时长。 */
export const BREATH_BUBBLE_MS = 4_000;
/** 每小时上限。 */
export const BREATH_MAX_PER_HOUR = 3;
/** 默认字数阈值（S0 参数 novaP_chars 可覆盖 100–900）。 */
export const BREATH_CHARS_DEFAULT = 300;

export interface BreathState {
  chars: number;
  lastKeyAt: number;
  shownAt: number[];
}

export function breathInit(): BreathState {
  return { chars: 0, lastKeyAt: 0, shownAt: [] };
}

/** 每次输入字符后的推进：停顿重置 → 累计 → 达阈值且小时配额未满 → 触发。 */
export function breathTick(
  st: BreathState,
  addedChars: number,
  now: number,
  charsNeeded = BREATH_CHARS_DEFAULT,
): { fire: boolean; next: BreathState } {
  const paused = st.lastKeyAt > 0 && now - st.lastKeyAt >= BREATH_PAUSE_RESET_MS;
  const chars = paused ? addedChars : st.chars + addedChars;
  const next: BreathState = { chars, lastKeyAt: now, shownAt: st.shownAt.filter((t) => now - t < 3_600_000) };
  if (chars < charsNeeded) return { fire: false, next };
  if (next.shownAt.length >= BREATH_MAX_PER_HOUR) return { fire: false, next };
  next.shownAt = [...next.shownAt, now];
  return { fire: true, next: { ...next, chars: 0 } };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-062 指频热区图（标准 QWERTY 十区）
// ---------------------------------------------------------------------------

/** 十区索引：0 左小指 1 左无名 2 左中指 3 左食指 4 左拇指 5 右拇指 6 右食指 7 右中指 8 右无名 9 右小指。 */
export const ZONE_COUNT = 10;
export const ZONE_LABELS: readonly string[] = [
  "左小指", "左无名", "左中指", "左食指", "左拇指",
  "右拇指", "右食指", "右中指", "右无名", "右小指",
];

const ZONE_TABLE: Record<string, number> = {
  // 数字排 + 符号（左半）
  "`": 0, "1": 0, "2": 1, "3": 2, "4": 3, "5": 3,
  // 字母左半
  q: 0, a: 0, z: 0,
  w: 1, s: 1, x: 1,
  e: 2, d: 2, c: 2,
  r: 3, f: 3, v: 3, t: 3, g: 3, b: 3,
  // 字母右半 + 数字右半
  "6": 6, "7": 6,
  y: 6, h: 6, n: 6, u: 6, j: 6, m: 6,
  "8": 7, i: 7, k: 7, ",": 7,
  "9": 8, o: 8, l: 8, ".": 8,
  // 右小指
  "0": 9, p: 9, ";": 9, "'": 9, "/": 9, "[": 9, "]": 9, "\\": 9, "-": 9, "=": 9,
  enter: 9, backspace: 9,
  // 通用：空格右拇指；shift 左小指（左 shift 为高频小指键）
  " ": 5,
  shift: 0,
};

/** 单键 → 分区（未知键如实返回 null，不入统计）。 */
export function zoneOfKey(eKey: string): number | null {
  const k = eKey.length === 1 ? eKey.toLowerCase() : eKey.toLowerCase();
  const z = ZONE_TABLE[k];
  return typeof z === "number" ? z : null;
}

/** 合计 → 计数与百分比（0 键 → 全 0 pct）。 */
export function zoneTotals(counts: number[]): { counts: number[]; pct: number[]; total: number } {
  const c = Array.from({ length: ZONE_COUNT }, (_, i) => counts[i] ?? 0);
  const total = c.reduce((a, b) => a + b, 0);
  const pct = total === 0 ? c.map(() => 0) : c.map((v) => Math.round((v / total) * 1000) / 10);
  return { counts: c, pct, total };
}

/** 月度对比：本月 - 上月 的百分点差（按 pct 比较）。 */
export function zoneDelta(cur: number[], prev: number[]): number[] {
  const a = zoneTotals(cur).pct;
  const b = zoneTotals(prev).pct;
  return a.map((v, i) => Math.round((v - (b[i] ?? 0)) * 10) / 10);
}

/** 月度键（YYYY-MM，本地时区）。 */
export function zoneMonthKey(d = new Date()): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-063 标点智断
// ---------------------------------------------------------------------------

/** 判定窗口（验收：延迟 ≤ 30ms）。 */
export const PUNCT_DECISION_MS = 30;

const CJK_RE = /[\u4e00-\u9fff\u3400-\u4dbf]/;
const URL_TAIL_RE = /(?:https?:\/\/|www\.)[^\s]*$/i;

export function isCJK(ch: string | null | undefined): boolean {
  return !!ch && CJK_RE.test(ch);
}

/** URL 豁免：光标前文命中 http(s)://… 或 www.… 连续段。 */
export function isUrlContext(textBefore: string): boolean {
  return URL_TAIL_RE.test(textBefore);
}

export interface PunctCtx {
  /** 句点前一个字符（空串 = 行首）。 */
  prevChar: string;
  /** 触发智断时的下一字符（null = 尚未落定/行尾）。 */
  nextChar: string | null;
  /** 句点前文（用于 URL 豁免）。 */
  textBefore: string;
}

/**
 * 标点智断状态机：返回 "cn"（转为 。）或 "en"（保留 .）。
 * - 数字场景（点前/后任一为数字）→ en；
 * - URL 场景 → en；
 * - 前文非中文 → en（不干涉纯英文输入）；
 * - 前文中文 + 下一字符中文 → cn；
 * - 前文中文 + 行尾/空格 → cn（中文句末惯例）。
 */
export function punctDecision(ctx: PunctCtx): "cn" | "en" {
  const { prevChar, nextChar, textBefore } = ctx;
  if (/\d/.test(prevChar) || (nextChar != null && /\d/.test(nextChar))) return "en";
  if (isUrlContext(textBefore)) return "en";
  if (!isCJK(prevChar)) return "en";
  if (nextChar == null) return "cn";
  if (nextChar === " ") return "cn";
  return isCJK(nextChar) ? "cn" : "en";
}

// ---------------------------------------------------------------------------
// 行为层（零侵入 DOM 叠层 + 事件）
// ---------------------------------------------------------------------------

let active = false;
type Unsub = () => void;
let bag: Unsub[] = [];
let ctx: InputNovaCtx = {
  on: flagOn,
  num: (id, key) => paramNum(id, key, INPUT_NOVA_FEATURES.find((f) => f.id === id)?.params?.[key]?.def as number ?? 0),
  bool: () => false,
  str: () => "",
  motionOK,
};

// ---- W-051 分应用节奏 ----
const paceGaps = new Map<string, number[]>();
const paceCache = new Map<string, PaceCurve>();
let lastKeyAtGlobal = 0;

function focusedAppId(): string {
  try {
    const s = vwmStore.getState();
    const w = s.wins.find((x) => x.id === s.focusedId);
    return w?.app ?? "desktop";
  } catch {
    return "desktop";
  }
}

function recordPace(appId: string, now: number): void {
  if (lastKeyAtGlobal > 0) {
    const gap = now - lastKeyAtGlobal;
    if (gap > 0 && gap <= 30_000) {
      const gaps = pushGap(paceGaps.get(appId) ?? [], gap);
      paceGaps.set(appId, gaps);
      const curve = calibratePace(gaps);
      if (curve) paceCache.set(appId, curve);
      else if (gaps.length < PACE_MIN_SAMPLES) paceCache.delete(appId);
    }
  }
  lastKeyAtGlobal = now;
}

// ---- W-052 Shift 大写覆盖 ----
let shiftHeld = false;
let capsOn = false;
let imeEn = true;

function emitCaps(): void {
  novaEvent("caps", {
    capsOn,
    shiftHeld,
    effective: effectiveCaps(capsOn, shiftHeld),
    imeEn,
    applied: flagOn("W-052") && capsAppliesToIme(imeEn ? "en" : "zh"),
  });
}

// ---- W-053 节奏器（即焚会话） ----
let rhythmOn = false;
let rhythmT0 = 0;
const rhythmHits: number[] = [];
let rhythmPanel: HTMLElement | null = null;
let rhythmTimer: ReturnType<typeof setInterval> | null = null;

export function startRhythmSession(): void {
  if (!flagOn("W-053")) return;
  rhythmHits.length = 0;
  rhythmT0 = Date.now();
  rhythmOn = true;
  novaEvent("rhythm", { phase: "start" });
}

export function stopRhythmSession(): RhythmScore | null {
  if (!rhythmOn) return null;
  rhythmOn = false;
  const score = rhythmScore(pairGaps(rhythmHits));
  rhythmHits.length = 0; // 训练即焚：数据不留存
  novaEvent("rhythm", { phase: "done", score });
  if (rhythmTimer) clearInterval(rhythmTimer);
  rhythmTimer = null;
  return score;
}

function pairGaps(times: number[]): number[] {
  const out: number[] = [];
  for (let i = 1; i < times.length; i++) {
    const g = times[i]! - times[i - 1]!;
    if (g > 0 && g <= 2_000) out.push(g); // >2s 视为中断，不计入节奏
  }
  return out;
}

// ---- W-054 长按池 ----
interface PoolState {
  key: string;
  pool: string[];
  timer: ReturnType<typeof setTimeout> | null;
  ringIdx: number;
  /** 径向环是否已弹出（未满 500ms 松开 = 正常输入，静默取消）。 */
  shown: boolean;
}
let poolState: PoolState | null = null;
let poolRing: HTMLElement | null = null;

function poolBegin(key: string, e: KeyboardEvent): void {
  const pool = poolFor(key);
  if (!pool || e.ctrlKey || e.altKey || e.metaKey) return;
  const st: PoolState = { key, pool, timer: null, ringIdx: 0, shown: false };
  st.timer = setTimeout(() => {
    st.timer = null;
    if (!ctx.on("W-054") || !document.activeElement || !isEditable(document.activeElement)) {
      poolState = null;
      return;
    }
    st.shown = true;
    poolState = st;
    renderPoolRing(st, caretRect());
  }, POOL_HOLD_MS);
  poolState = st;
}

function poolAdvance(dir: 1 | -1): void {
  const st = poolState;
  if (!st || !st.shown) return; // 未触发环前不算
  st.ringIdx = nextRingIndex(st.ringIdx, dir, st.pool.length);
  renderPoolRing(st, caretRect());
}

function poolCommit(): void {
  const st = poolState;
  poolState = null;
  if (st?.timer) clearTimeout(st.timer);
  if (!st || !st.shown) {
    removePoolRing();
    return;
  }
  removePoolRing();
  const char = st.pool[st.ringIdx] ?? "";
  if (!insertTextAtCaret(char)) novaEvent("pool", { key: st.key, char });
}

function poolCancel(): void {
  const st = poolState;
  poolState = null;
  if (st?.timer) clearTimeout(st.timer);
  removePoolRing();
}

// ---- W-055 短语暖场 ----
let stealth = false;
let phrasesByApp: Record<string, Record<string, number>> = {};
let ghostEl: HTMLElement | null = null;
let ghostTarget: HTMLElement | null = null;
let draftRun = "";

function loadPhrases(): void {
  phrasesByApp = lsGet<Record<string, Record<string, number>>>(`${NS}.phrases`, {});
}

function appPhrases(appId: string): Record<string, number> {
  return phrasesByApp[appId] ?? {};
}

function recordPhraseRun(appId: string): void {
  if (stealth || !draftRun.trim()) return;
  const tokens = draftRun.split(/[\n]+/).map((t) => t.trim()).filter(Boolean);
  draftRun = "";
  if (tokens.length === 0) return;
  const cur = appPhrases(appId);
  let next = cur;
  for (const t of tokens) next = bumpPhrase(next, t);
  if (next !== cur) {
    phrasesByApp[appId] = next;
    lsSet(`${NS}.phrases`, phrasesByApp);
  }
}

function showGhosts(el: HTMLElement): void {
  hideGhosts();
  if (stealth || !flagOn("W-055")) return;
  const appId = focusedAppId();
  const list = ghostPhrases(appPhrases(appId), 3);
  if (list.length === 0) return;
  const row = document.createElement("div");
  row.className = "nova-input-ghosts";
  row.setAttribute("role", "listbox");
  row.setAttribute("aria-label", "本地短语暖场");
  list.forEach((p, i) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "nova-input-ghost";
    item.setAttribute("role", "option");
    item.textContent = i === 0 ? `${p}  ⇥Tab` : p;
    item.addEventListener("click", () => {
      insertTextAtCaret(p) || novaEvent("insert", { text: p });
      hideGhosts();
    });
    row.appendChild(item);
  });
  positionNearCaret(row, el);
  document.getElementById("nova-input-layer")?.appendChild(row);
  ghostEl = row;
  ghostTarget = el;
}

function hideGhosts(): void {
  ghostEl?.remove();
  ghostEl = null;
  ghostTarget = null;
}

// ---- W-056 沙坑 ----
let sandboxPanel: HTMLElement | null = null;
let sandboxHits: SandboxHit[] = [];
let f1HoldTimer: ReturnType<typeof setTimeout> | null = null;

function sandboxOpen(): void {
  if (!flagOn("W-056") || sandboxPanel?.isConnected) return;
  const panel = document.createElement("div");
  panel.className = "nova-input-sandbox";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", "键盘沙坑 · 按键反查");
  const title = document.createElement("div");
  title.className = "nova-input-sandbox-title";
  title.textContent = "KEY SANDBOX · 按任意组合反查真实绑定";
  const body = document.createElement("div");
  body.className = "nova-input-sandbox-body";
  body.textContent = "待捕获…（Esc 清除 / 再按关闭）";
  panel.append(title, body);
  document.getElementById("nova-input-layer")?.appendChild(panel);
  sandboxPanel = panel;
  sandboxHits = [];
  novaEvent("sandbox", { phase: "open" });
}

function sandboxClose(): void {
  sandboxPanel?.remove();
  sandboxPanel = null;
  sandboxHits = [];
  novaEvent("sandbox", { phase: "close" });
}

function sandboxCapture(e: KeyboardEvent): void {
  if (!sandboxPanel?.isConnected) return;
  const combo = comboFromEvent(e);
  const body = sandboxPanel.querySelector<HTMLElement>(".nova-input-sandbox-body");
  if (!body) return;
  if (!combo) {
    body.textContent = `修饰键中… ctrl:${e.ctrlKey} alt:${e.altKey} shift:${e.shiftKey}`;
    return;
  }
  e.preventDefault();
  e.stopPropagation();
  const hit = lookupBinding(combo, keymapSnapshot());
  sandboxHits.push(hit);
  const line = document.createElement("div");
  line.className = "nova-input-sandbox-hit";
  line.textContent = hit.bound
    ? `${hit.combo} → ${hit.id}（${hit.descKey}）[${hit.scope}] @${hit.source}`
    : `${hit.combo} → FREE（注册表未绑定 · 可在键位设置自定义）`;
  body.textContent = "";
  body.appendChild(line);
  novaEvent("sandbox", { phase: "capture", hit });
}

// ---- W-057 光环 / W-058 高手时刻 ----
let auraEl: HTMLElement | null = null;
let auraTarget: HTMLElement | null = null;
const hitTimes: number[] = [];
let auraTimer: ReturnType<typeof setInterval> | null = null;

function auraEnsure(el: HTMLElement): void {
  if (!ctx.on("W-057")) return;
  auraRemove();
  const ring = document.createElement("div");
  ring.className = `nova-input-aura${motionOK() ? "" : " nova-static"}`;
  ring.setAttribute("aria-hidden", "true");
  positionAround(ring, el);
  document.getElementById("nova-input-layer")?.appendChild(ring);
  auraEl = ring;
  auraTarget = el;
}

function auraRemove(): void {
  auraEl?.remove();
  auraEl = null;
  auraTarget = null;
}

function auraTick(): void {
  if (!auraEl || !auraTarget?.isConnected) {
    if (auraEl) auraRemove();
    return;
  }
  const now = Date.now();
  const wpm = rollingWpm(hitTimes, now);
  const st = auraState(wpm);
  auraEl.dataset.mode = st.mode;
  auraEl.style.setProperty("--nova-aura", String(st.intensity));
  // W-058 高手时刻（同源 30s 均速）
  if (flagOn("W-058") && wpm > 0) {
    const done = lsGet<number[]>(`${NS}.wpmBadges`, []);
    const fresh = crossedThresholds(wpm, done);
    if (fresh.length > 0) {
      lsSet(`${NS}.wpmBadges`, [...done, ...fresh]);
      confettiMoment(auraTarget, Math.max(...fresh), wpm);
    }
  }
}

function confettiMoment(anchor: HTMLElement, threshold: number, wpm: number): void {
  if (!motionOK()) return;
  const el = document.createElement("div");
  el.className = "nova-input-confetti";
  el.setAttribute("role", "status");
  el.textContent = `${threshold} WPM · ${wpm}`;
  positionBelow(el, anchor);
  document.getElementById("nova-input-layer")?.appendChild(el);
  setTimeout(() => el.remove(), WPM_MOMENT_MS);
}

// ---- W-059 走神暂停印 ----
let lastTypeAt = 0;
let lastRecallAt = 0;
let recallTimer: ReturnType<typeof setTimeout> | null = null;
let recallDot: HTMLElement | null = null;

function scheduleRecall(): void {
  if (recallTimer) clearTimeout(recallTimer);
  if (!flagOn("W-059")) return;
  recallTimer = setTimeout(() => {
    recallTimer = null;
    const gap = Date.now() - lastTypeAt;
    if (lastTypeAt === 0 || gap < FLOW_GAP_MS) return;
    if (!shouldRecall(gap, lastRecallAt, Date.now())) return;
    if (typeof document !== "undefined" && document.fullscreenElement) return; // 全屏内不出现
    showRecallDot();
  }, FLOW_GAP_MS + 50);
}

function showRecallDot(): void {
  const host = document.querySelector<HTMLElement>(".taskbar") ?? document.body;
  recallDot?.remove();
  const dot = document.createElement("div");
  dot.className = `nova-input-recall${motionOK() ? "" : " nova-static"}`;
  dot.setAttribute("aria-hidden", "true");
  host.appendChild(dot);
  recallDot = dot;
  lastRecallAt = Date.now();
  novaEvent("recall", { at: lastRecallAt });
  setTimeout(() => {
    dot.remove();
    if (recallDot === dot) recallDot = null;
  }, RECALL_BREATH_MS);
}

// ---- W-060 剪贴板环槽 ----
let slots: ClipSlots = emptySlots();
let slotHud: HTMLElement | null = null;
let slotHudTimer: ReturnType<typeof setTimeout> | null = null;

function slotHudFlash(msg: string): void {
  slotHud?.remove();
  const el = document.createElement("div");
  el.className = "nova-input-slot-hud";
  el.setAttribute("role", "status");
  el.textContent = msg;
  document.getElementById("nova-input-layer")?.appendChild(el);
  slotHud = el;
  if (slotHudTimer) clearTimeout(slotHudTimer);
  slotHudTimer = setTimeout(() => {
    el.remove();
    if (slotHud === el) slotHud = null;
  }, 900);
}

/** 存入槽 n：优先选区，兜底剪贴板（best-effort，失败如实提示）。 */
export function clipPut(slot: 0 | 1 | 2): boolean {
  if (!flagOn("W-060")) return false;
  const sel = typeof document !== "undefined" ? (document.getSelection()?.toString() ?? "") : "";
  if (sel) {
    slots = slotPut(slots, slot, sel);
    slotHudFlash(`SLOT ${slot + 1} STORED`);
    return true;
  }
  if (typeof navigator !== "undefined" && navigator.clipboard?.readText) {
    navigator.clipboard
      .readText()
      .then((t) => {
        slots = slotPut(slots, slot, t);
        slotHudFlash(`SLOT ${slot + 1} STORED`);
      })
      .catch(() => slotHudFlash("SLOT EMPTY"));
    return true;
  }
  slotHudFlash("SLOT EMPTY");
  return false;
}

/** 直取直贴：槽空如实 EMPTY。 */
export function clipTake(slot: 0 | 1 | 2): boolean {
  if (!flagOn("W-060")) return false;
  const t = slotTake(slots, slot);
  if (!t.ok) {
    slotHudFlash(`SLOT ${slot + 1} EMPTY`);
    return false;
  }
  insertTextAtCaret(t.text) || novaEvent("insert", { text: t.text });
  slotHudFlash(`SLOT ${slot + 1} PASTED`);
  return true;
}

// ---- W-061 长句呼吸 ----
let breath = breathInit();

// ---- W-062 热区图 ----
let zones: Record<string, number[]> = {};
let zonePanel: HTMLElement | null = null;

function loadZones(): void {
  zones = lsGet<Record<string, number[]>>(`${NS}.zones`, {});
}

function recordZone(eKey: string): void {
  if (!ctx.on("W-062")) return;
  const z = zoneOfKey(eKey);
  if (z == null) return;
  const mk = zoneMonthKey();
  const cur = zones[mk] ?? Array.from({ length: ZONE_COUNT }, () => 0);
  cur[z] = (cur[z] ?? 0) + 1;
  zones = { ...zones, [mk]: cur };
  // 只保留近 2 个月，防膨胀
  const keys = Object.keys(zones).sort().slice(-2);
  const trimmed: Record<string, number[]> = {};
  for (const k of keys) trimmed[k] = zones[k]!;
  zones = trimmed;
  lsSet(`${NS}.zones`, zones);
}

export function clearZones(): void {
  zones = {};
  lsSet(`${NS}.zones`, zones);
  renderZonePanel();
}

export function toggleZonePanel(force?: boolean): void {
  if (!ctx.on("W-062")) return;
  const want = force ?? zonePanel == null;
  if (!want) {
    zonePanel?.remove();
    zonePanel = null;
    return;
  }
  if (zonePanel?.isConnected) return;
  const panel = document.createElement("div");
  panel.className = "nova-input-zones";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", "指频热区图");
  const clearBtn = document.createElement("button");
  clearBtn.type = "button";
  clearBtn.className = "nova-input-zones-clear";
  clearBtn.textContent = "清零";
  clearBtn.addEventListener("click", () => clearZones());
  panel.appendChild(clearBtn);
  document.getElementById("nova-input-layer")?.appendChild(panel);
  zonePanel = panel;
  renderZonePanel();
}

function renderZonePanel(): void {
  const panel = zonePanel;
  if (!panel?.isConnected) return;
  panel.querySelectorAll(".nova-input-zone-row").forEach((n) => n.remove());
  const mk = zoneMonthKey();
  const prevKey = Object.keys(zones).filter((k) => k < mk).sort().pop();
  const { counts, pct, total } = zoneTotals(zones[mk] ?? []);
  const delta = prevKey ? zoneDelta(zones[mk] ?? [], zones[prevKey] ?? []) : null;
  const maxPct = Math.max(1, ...pct);
  for (let i = 0; i < ZONE_COUNT; i++) {
    const row = document.createElement("div");
    row.className = "nova-input-zone-row";
    const label = document.createElement("span");
    label.className = "nova-input-zone-label";
    label.textContent = ZONE_LABELS[i] ?? `Z${i}`;
    const bar = document.createElement("span");
    bar.className = "nova-input-zone-bar";
    bar.style.setProperty("--nova-zone-w", `${Math.round((pct[i] ?? 0) / maxPct * 100)}%`);
    const val = document.createElement("span");
    val.className = "nova-input-zone-val";
    val.textContent = delta
      ? `${pct[i] ?? 0}%（${delta[i]! > 0 ? "+" : ""}${delta[i]!} vs 上月）`
      : `${pct[i] ?? 0}% · ${counts[i] ?? 0}`;
    row.append(label, bar, val);
    panel.appendChild(row);
  }
  if (total === 0) {
    const empty = document.createElement("div");
    empty.className = "nova-input-zone-row";
    empty.textContent = "本月暂无击键记录";
    panel.appendChild(empty);
  }
}

// ---- W-063 标点智断 ----
let punctPending: { el: HTMLElement; node: Node | null; off: number; textBefore: string; prevChar: string } | null = null;
let punctTimer: ReturnType<typeof setTimeout> | null = null;

/** 光标是否位于该可编辑元素全部文本的末尾。 */
function atTextEnd(el: HTMLElement): boolean {
  const sel = document.getSelection();
  if (!sel) return false;
  const full = el.tagName === "INPUT" || el.tagName === "TEXTAREA" ? (el as HTMLInputElement).value : el.textContent ?? "";
  const off = el.tagName === "INPUT" || el.tagName === "TEXTAREA" ? (el as HTMLInputElement).selectionStart ?? 0 : sel.anchorOffset;
  return off >= full.length;
}

function punctObserve(data: string, el: HTMLElement): void {
  if (!ctx.on("W-063")) return;
  if (punctTimer) clearTimeout(punctTimer);
  if (data === ".") {
    const sel = document.getSelection();
    const node = sel?.anchorNode ?? null;
    const text = node?.textContent ?? "";
    const off = sel?.anchorOffset ?? text.length;
    const beforeDot = text.slice(0, Math.max(0, off - 1)); // 刚插入的 "." 之前
    punctPending = { el, node, off, textBefore: beforeDot, prevChar: beforeDot.slice(-1) };
    // 行尾兜底：30ms 内无后续输入且确实处于文本末尾 → 按中文句末惯例智断
    punctTimer = setTimeout(() => {
      punctTimer = null;
      if (!punctPending) return;
      if (atTextEnd(punctPending.el) && punctDecision({ ...punctPending, nextChar: null }) === "cn") {
        punctConvert();
      }
      // 非行尾：保持 pending，交给下一次 input 的 nextChar 判定
    }, PUNCT_DECISION_MS);
    return;
  }
  if (punctPending && punctPending.el === el && data && data.length > 0) {
    const nextChar = data[0]!;
    if (punctDecision({ prevChar: punctPending.prevChar, nextChar, textBefore: punctPending.textBefore }) === "cn") {
      punctConvert();
    }
    punctPending = null;
    return;
  }
  punctPending = null;
}

function punctConvert(): void {
  try {
    const p = punctPending;
    const sel = document.getSelection();
    if (!p || !sel) return;
    // 首选：插入句点时记录的文本节点/偏移（insertText 通常不拆节点）
    if (p.node && p.node.nodeType === Node.TEXT_NODE && p.node.parentElement && p.el.contains(p.node)) {
      const text = p.node.textContent ?? "";
      if (text[p.off - 1] === ".") {
        p.node.textContent = text.slice(0, p.off - 1) + "。" + text.slice(p.off);
        sel.collapse(p.node, p.off);
        return;
      }
    }
    // 兜底：从当前光标向前最多 6 字符找句点
    const node = sel.anchorNode;
    const off = sel.anchorOffset;
    if (node?.nodeType === Node.TEXT_NODE && p.el.contains(node)) {
      const text = node.textContent ?? "";
      for (let back = 1; back <= 6 && off - back >= 0; back++) {
        if (text[off - back] === ".") {
          node.textContent = text.slice(0, off - back) + "。" + text.slice(off - back + 1);
          sel.collapse(node, off);
          return;
        }
      }
    }
  } catch {
    /* 转换失败如实放弃（不打断输入） */
  }
}

// ---- DOM 工具 ----

function isEditable(el: Element | null): boolean {
  if (!el) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || (el as HTMLElement).isContentEditable === true;
}

function caretRect(): DOMRect | null {
  const sel = typeof document !== "undefined" ? document.getSelection() : null;
  if (sel && sel.rangeCount > 0) return sel.getRangeAt(0).getBoundingClientRect();
  return null;
}

function positionNearCaret(el: HTMLElement, anchor: HTMLElement): void {
  const r = caretRect() ?? anchor.getBoundingClientRect();
  el.style.left = `${Math.round(r.left)}px`;
  el.style.top = `${Math.round(r.bottom + 6)}px`;
}

function positionBelow(el: HTMLElement, anchor: HTMLElement): void {
  const r = anchor.getBoundingClientRect();
  el.style.left = `${Math.round(r.left)}px`;
  el.style.top = `${Math.round(r.bottom + 4)}px`;
}

function positionAround(el: HTMLElement, anchor: HTMLElement): void {
  const r = anchor.getBoundingClientRect();
  el.style.left = `${Math.round(r.left - 4)}px`;
  el.style.top = `${Math.round(r.top - 4)}px`;
  el.style.width = `${Math.round(r.width + 8)}px`;
  el.style.height = `${Math.round(r.height + 8)}px`;
}

/** 文本注入：焦点可编辑元素 → insertText；失败返回 false（调用方兜底派发事件）。 */
function insertTextAtCaret(text: string): boolean {
  if (typeof document === "undefined") return false;
  const el = document.activeElement;
  if (!isEditable(el)) return false;
  try {
    return document.execCommand("insertText", false, text);
  } catch {
    return false;
  }
}

// ---- W-054 径向环渲染 ----

function renderPoolRing(st: { pool: string[]; ringIdx: number }, caret: DOMRect | null): void {
  removePoolRing();
  const cx = caret?.left ?? window.innerWidth / 2;
  const cy = caret?.top ?? window.innerHeight / 2;
  const radius = 64;
  const ring = document.createElement("div");
  ring.className = "nova-input-ring";
  ring.setAttribute("role", "menu");
  const pos = radialLayout(st.pool.length, radius);
  st.pool.forEach((ch, i) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "nova-input-ring-item";
    item.textContent = ch;
    item.dataset.active = i === st.ringIdx ? "1" : "0";
    item.style.left = `${Math.round(cx + (pos[i]?.x ?? 0) - 18)}px`;
    item.style.top = `${Math.round(cy + (pos[i]?.y ?? 0) - 18)}px`;
    ring.appendChild(item);
  });
  document.getElementById("nova-input-layer")?.appendChild(ring);
  poolRing = ring;
}

function removePoolRing(): void {
  poolRing?.remove();
  poolRing = null;
}

// ---- CSS（唯一注入点，nova-input- 前缀，全部走 tokens 语义变量） ----

const STYLE_ID = "nova-input-style";
const STYLE_TEXT = `
#nova-input-layer{position:fixed;inset:0;z-index:946;pointer-events:none;font-size:12px}
#nova-input-layer>*{pointer-events:auto}
.nova-input-ghosts{position:fixed;display:flex;gap:8px;padding:2px 6px;border-radius:8px;background:var(--panel,var(--bg-raised,#111));box-shadow:0 8px 24px oklch(0 0 0 / .28);opacity:.3;transition:opacity var(--dur-2) var(--ease-standard)}
.nova-input-ghosts:hover{opacity:.92}
.nova-input-ghost{font:inherit;border:0;background:transparent;color:var(--ink,var(--fg,#ddd));cursor:pointer;padding:2px 6px;border-radius:6px}
.nova-input-ghost:hover{background:var(--accent-soft)}
.nova-input-aura{position:fixed;border-radius:10px;pointer-events:none!important;box-shadow:0 0 0 2px oklch(0.75 0.09 var(--accent-hue,262) / calc(var(--nova-aura,.3) * .8));transition:box-shadow var(--dur-3) var(--ease-standard)}
.nova-input-aura[data-mode="warm"]{box-shadow:0 0 0 2px oklch(0.78 0.14 55 / .9),0 0 14px oklch(0.78 0.14 55 / .45)}
.nova-input-aura[data-mode="steady"]{box-shadow:0 0 0 2px oklch(0.8 0.02 262 / .7)}
.nova-input-aura[data-mode="rest"]{animation:nova-input-rest 3.2s var(--ease-standard) infinite}
@keyframes nova-input-rest{0%,100%{box-shadow:0 0 0 2px oklch(0.7 0.05 262 / .25)}50%{box-shadow:0 0 0 2px oklch(0.7 0.05 262 / .7)}}
.nova-input-aura.nova-static{animation:none!important}
[data-reduce-motion="true"] .nova-input-aura{animation:none!important;transition:none!important}
.nova-input-confetti{position:fixed;padding:4px 10px;border-radius:8px;background:var(--accent-soft);color:var(--ink,var(--fg,#ddd));font-weight:600;letter-spacing:.06em;animation:nova-input-pop 1.2s var(--ease-standard) forwards}
@keyframes nova-input-pop{0%{transform:translateY(4px);opacity:0}18%{transform:translateY(0);opacity:1}80%{opacity:1}100%{opacity:0}}
.nova-input-recall{width:8px;height:8px;border-radius:50%;background:var(--accent);animation:nova-input-recall 1.5s var(--ease-standard) forwards;margin-left:auto}
@keyframes nova-input-recall{0%{opacity:0;transform:scale(.6)}30%{opacity:1;transform:scale(1.15)}100%{opacity:0;transform:scale(1)}}
.nova-input-recall.nova-static{animation:none;opacity:.8}
[data-reduce-motion="true"] .nova-input-recall{animation:none;opacity:.8}
.nova-input-ring{position:fixed;inset:0;pointer-events:none!important}
.nova-input-ring-item{position:fixed;width:36px;height:36px;border-radius:50%;border:1px solid var(--accent-soft);background:var(--panel,var(--bg-raised,#111));color:var(--ink,var(--fg,#ddd));font-size:15px;cursor:pointer;pointer-events:auto}
.nova-input-ring-item[data-active="1"]{background:var(--accent-soft);box-shadow:0 0 0 2px var(--accent)}
.nova-input-sandbox{position:fixed;right:16px;top:64px;z-index:947;min-width:280px;max-width:360px;padding:12px 14px;border-radius:12px;background:var(--panel,var(--bg-raised,#111));box-shadow:0 12px 36px oklch(0 0 0 / .3);color:var(--ink,var(--fg,#ddd))}
.nova-input-sandbox-title{font-weight:600;letter-spacing:.04em;margin-bottom:8px}
.nova-input-sandbox-body{opacity:.9;line-height:1.7}
.nova-input-sandbox-hit{font-family:var(--mono,Consolas,monospace)}
.nova-input-slot-hud{position:fixed;left:50%;bottom:56px;transform:translateX(-50%);padding:3px 10px;border-radius:7px;background:var(--panel,var(--bg-raised,#111));color:var(--ink,var(--fg,#ddd));box-shadow:0 8px 24px oklch(0 0 0 / .3);letter-spacing:.08em}
.nova-input-breath{position:fixed;padding:4px 12px;border-radius:999px;background:var(--accent-soft);color:var(--ink,var(--fg,#ddd));animation:nova-input-breath 4s var(--ease-standard) forwards}
.nova-input-breath.nova-dnd{animation:none;opacity:.6;background:oklch(0.7 0.05 262 / .3)}
@keyframes nova-input-breath{0%{opacity:0;transform:scale(.9)}12%{opacity:1;transform:scale(1.04)}70%{opacity:1}100%{opacity:0;transform:scale(1)}}
[data-reduce-motion="true"] .nova-input-breath{animation:none;opacity:.85}
.nova-input-zones{position:fixed;left:12px;bottom:56px;min-width:250px;max-width:330px;padding:12px 14px;border-radius:12px;background:var(--panel,var(--bg-raised,#111));box-shadow:0 12px 36px oklch(0 0 0 / .3);color:var(--ink,var(--fg,#ddd))}
.nova-input-zones h3{margin:0 0 8px;font-size:12px;font-weight:600;letter-spacing:.04em}
.nova-input-zones-clear{font:inherit;margin-bottom:8px;padding:2px 10px;border-radius:6px;border:1px solid var(--accent-soft);background:transparent;color:inherit;cursor:pointer}
.nova-input-zone-row{display:flex;align-items:center;gap:8px;padding:2px 0}
.nova-input-zone-label{width:44px;flex:none;opacity:.75}
.nova-input-zone-bar{flex:1;height:6px;border-radius:3px;background:var(--accent-soft);position:relative;overflow:hidden}
.nova-input-zone-bar::after{content:"";position:absolute;inset:0;width:var(--nova-zone-w,0%);background:var(--accent);border-radius:3px;transition:width var(--dur-3) var(--ease-standard)}
.nova-input-zone-val{width:118px;flex:none;text-align:right;opacity:.8;font-variant-numeric:tabular-nums}
.nova-input-rhythm{position:fixed;left:50%;top:72px;transform:translateX(-50%);min-width:320px;max-width:440px;padding:14px 16px;border-radius:12px;background:var(--panel,var(--bg-raised,#111));box-shadow:0 12px 36px oklch(0 0 0 / .3);color:var(--ink,var(--fg,#ddd))}
.nova-input-rhythm h3{margin:0 0 8px;font-size:12px;font-weight:600;letter-spacing:.04em}
.nova-input-rhythm-bars{display:flex;align-items:flex-end;gap:2px;height:56px;margin:8px 0}
.nova-input-rhythm-bar{flex:1;min-width:2px;background:var(--accent);border-radius:1px 1px 0 0;opacity:.75}
.nova-input-rhythm-band{border-bottom:1px dashed oklch(0.75 0.1 145 / .8);position:relative}
.nova-input-rhythm-score{font-variant-numeric:tabular-nums;letter-spacing:.06em}
.nova-input-rhythm-actions{display:flex;gap:8px;margin-top:8px}
.nova-input-rhythm-actions button{font:inherit;padding:3px 10px;border-radius:6px;border:1px solid var(--accent-soft);background:transparent;color:inherit;cursor:pointer}
.nova-input-rhythm-actions button:hover{background:var(--accent-soft)}
`;

function ensureStyle(): void {
  if (typeof document === "undefined") return;
  if (document.getElementById(STYLE_ID)) return;
  const style = document.createElement("style");
  style.id = STYLE_ID;
  style.textContent = STYLE_TEXT;
  document.head.appendChild(style);
  bag.push(() => document.getElementById(STYLE_ID)?.remove());
}

function ensureLayer(): void {
  if (typeof document === "undefined") return;
  if (!document.getElementById("nova-input-layer")) {
    const layer = document.createElement("div");
    layer.id = "nova-input-layer";
    layer.setAttribute("aria-hidden", "false");
    document.body.appendChild(layer);
    bag.push(() => document.getElementById("nova-input-layer")?.remove());
  }
}

// ---- W-053 节奏器面板 ----

function openRhythmTrainer(): void {
  if (!flagOn("W-053")) return;
  rhythmPanel?.remove();
  const panel = document.createElement("div");
  panel.className = "nova-input-rhythm";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", "输入节奏器");
  const h = document.createElement("h3");
  h.textContent = "RHYTHM TRAINER · 30s 击键节奏（训练即焚）";
  const bars = document.createElement("div");
  bars.className = "nova-input-rhythm-bars";
  const scoreEl = document.createElement("div");
  scoreEl.className = "nova-input-rhythm-score";
  scoreEl.textContent = "READY";
  const actions = document.createElement("div");
  actions.className = "nova-input-rhythm-actions";
  const startBtn = document.createElement("button");
  startBtn.type = "button";
  startBtn.textContent = "开始 30s";
  const stopBtn = document.createElement("button");
  stopBtn.type = "button";
  stopBtn.textContent = "结束";
  actions.append(startBtn, stopBtn);
  panel.append(h, bars, scoreEl, actions);
  document.getElementById("nova-input-layer")?.appendChild(panel);
  rhythmPanel = panel;

  const render = (): void => {
    const gaps = pairGaps(rhythmHits);
    bars.textContent = "";
    for (const g of gaps.slice(-60)) {
      const bar = document.createElement("span");
      bar.className = "nova-input-rhythm-bar";
      bar.style.height = `${clamp(Math.round(g / 8), 2, 56)}px`;
      bars.appendChild(bar);
    }
    const ideal = medianOf(gaps) ?? 220;
    bars.classList.add("nova-input-rhythm-band");
    bars.style.borderBottomWidth = "0";
    bars.style.boxShadow = `inset 0 -${clamp(Math.round(ideal / 8), 2, 56)}px 0 oklch(0.75 0.1 145 / .25)`;
  };

  startBtn.addEventListener("click", () => {
    startRhythmSession();
    scoreEl.textContent = "RECORDING…";
    if (rhythmTimer) clearInterval(rhythmTimer);
    rhythmTimer = setInterval(() => {
      render();
      if (rhythmOn && Date.now() - rhythmT0 >= RHYTHM_WINDOW_MS) {
        const score = stopRhythmSession();
        scoreEl.textContent = score
          ? `STABILITY ${score.stability}/100 · CV ${score.cv} · ${rhythmAdvice(score)}`
          : "SAMPLES < 3 · 本次不评级";
        render();
      }
    }, 250);
  });
  stopBtn.addEventListener("click", () => {
    const score = stopRhythmSession();
    scoreEl.textContent = score
      ? `STABILITY ${score.stability}/100 · CV ${score.cv} · ${rhythmAdvice(score)}`
      : "SAMPLES < 3 · 本次不评级";
    if (rhythmTimer) clearInterval(rhythmTimer);
    rhythmTimer = null;
  });
  novaEvent("rhythm", { phase: "panel" });
}

// ---- 全局桥（keydown/keyup/focusin/input capture） ----

function onKeyDown(e: KeyboardEvent): void {
  const now = Date.now();
  const target = e.target as HTMLElement | null;
  const inEditable = isEditable(target);

  // W-053 节奏器会话采样（全键，含编辑器外）
  if (rhythmOn && now - rhythmT0 <= RHYTHM_WINDOW_MS) rhythmHits.push(now);

  // W-051 分应用节奏 / W-062 热区 / W-057 光环采样（真实键击）
  if (!e.repeat || true) {
    const appId = focusedAppId();
    recordPace(appId, now);
    recordZone(e.key);
  }
  hitTimes.push(now);
  if (hitTimes.length > 400) hitTimes.splice(0, hitTimes.length - 400);
  lastTypeAt = now;
  scheduleRecall();

  // W-052 Shift 覆盖模型（Caps 模型与真实修饰态同步）
  if (typeof e.getModifierState === "function") {
    const realCaps = e.getModifierState("CapsLock");
    if (realCaps !== capsOn) {
      capsOn = realCaps;
      emitCaps();
    }
  }
  if (e.key === "Shift" && !shiftHeld) {
    shiftHeld = true;
    emitCaps();
  }

  // W-056 沙坑捕获态（优先级最高，吞掉其余处理）
  if (sandboxPanel?.isConnected) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (nextEscState(sandboxHits.length > 0) === "clear") {
        sandboxHits = [];
        const body = sandboxPanel.querySelector<HTMLElement>(".nova-input-sandbox-body");
        if (body) body.textContent = "已清除 · 继续按组合或 Esc 关闭";
      } else sandboxClose();
      return;
    }
    sandboxCapture(e);
    return;
  }

  // W-056 F1 长按 800ms 呼出
  if (e.key === "F1" && !e.ctrlKey && !e.altKey && !e.metaKey && !e.repeat && flagOn("W-056")) {
    if (f1HoldTimer) clearTimeout(f1HoldTimer);
    f1HoldTimer = setTimeout(() => {
      f1HoldTimer = null;
      sandboxOpen();
    }, SANDBOX_HOLD_MS);
  }

  // W-060 环槽
  if (e.ctrlKey && e.shiftKey && ["1", "2", "3"].includes(e.key)) {
    e.preventDefault();
    e.stopPropagation();
    clipPut((Number(e.key) - 1) as 0 | 1 | 2);
    return;
  }
  if (e.ctrlKey && e.altKey && !e.shiftKey && ["1", "2", "3"].includes(e.key)) {
    e.preventDefault();
    e.stopPropagation();
    clipTake((Number(e.key) - 1) as 0 | 1 | 2);
    return;
  }

  if (!inEditable) return;

  // W-054 长按池起按（无修饰键）
  if (!e.repeat && !e.ctrlKey && !e.altKey && !e.metaKey && poolFor(e.key)) {
    poolBegin(e.key, e);
  }
  // W-054 环上方向选择
  if (poolState?.shown) {
    if (e.key === "ArrowRight" || e.key === "ArrowDown") {
      e.preventDefault();
      poolAdvance(1);
      return;
    }
    if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
      e.preventDefault();
      poolAdvance(-1);
      return;
    }
  }
  // W-063 标点智断观察（keydown 侧记录，转换在 input 侧）
  if (e.key === "." && !e.ctrlKey && !e.altKey && !e.metaKey) {
    draftRun += ".";
  }
}

function onKeyUp(e: KeyboardEvent): void {
  if (e.key === "Shift") {
    shiftHeld = false;
    emitCaps();
  }
  // W-056 F1 松开取消（未满 800ms）
  if (e.key === "F1" && f1HoldTimer) {
    clearTimeout(f1HoldTimer);
    f1HoldTimer = null;
  }
  // W-054 松开：同键上屏 / 换键取消（环未弹出则静默清理，不干扰正常输入）
  if (poolState) {
    if (e.key === poolState.key) poolCommit();
    else poolCancel();
  }
}

function onFocusIn(e: FocusEvent): void {
  const el = e.target as HTMLElement | null;
  if (!el || !isEditable(el)) return;
  // W-055 幽灵建议
  showGhosts(el);
  // W-057 光环
  if (ctx.on("W-057")) auraEnsure(el);
}

function onFocusOut(e: FocusEvent): void {
  const el = e.target as HTMLElement | null;
  if (!el || !isEditable(el)) return;
  if (ghostTarget === el) hideGhosts();
  if (auraTarget === el) auraRemove();
  // 结束一段草稿 → 记短语
  const appId = focusedAppId();
  recordPhraseRun(appId);
}

function onInput(e: Event): void {
  const el = e.target as HTMLElement | null;
  if (!el || !isEditable(el)) return;
  const data = (e as InputEvent).data ?? "";

  // W-061 长句呼吸
  if (flagOn("W-061")) {
    const added = data.length > 0 ? data.length : 1; // IME 组段确认等无 data 的也计 1
    const now = Date.now();
    const need = paramNum("W-061", "chars", 300);
    const r = breathTick(breath, added, now, clamp(need, 100, 900));
    breath = r.next;
    if (r.fire) showBreath(el);
  }

  // W-055 草稿累计（短语暖场语料）
  if (data) draftRun += data;

  // W-063 标点智断
  punctObserve(data, el);
}

function showBreath(anchor: HTMLElement): void {
  const el = document.createElement("div");
  const dnd = document.documentElement.dataset.dnd === "true"; // Z-44 勿扰降级为颜色变化
  el.className = `nova-input-breath${dnd ? " nova-dnd" : ""}`;
  el.setAttribute("role", "status");
  el.textContent = dnd ? "BREATHE" : "深呼吸 · BREATHE";
  positionBelow(el, anchor);
  document.getElementById("nova-input-layer")?.appendChild(el);
  setTimeout(() => el.remove(), BREATH_BUBBLE_MS);
}

function onWheel(e: WheelEvent): void {
  if (!poolState?.timer) return;
  e.preventDefault();
  poolAdvance(e.deltaY > 0 ? 1 : -1);
}

// ---- 隐身会话（U-36 联动） ----

export function setStealth(activeSession: boolean): void {
  stealth = activeSession;
  if (stealth) {
    hideGhosts();
    draftRun = "";
  }
  novaEvent("stealth", { active: stealth });
}

// ---- 激活 / 卸载（幂等） ----

export function activateInputNova(custom?: Partial<InputNovaCtx>): void {
  if (active || typeof document === "undefined") return;
  active = true;
  ctx = { ...ctx, ...custom };
  ensureStyle();
  ensureLayer();
  loadPhrases();
  loadZones();
  slots = emptySlots();
  breath = breathInit();
  hitTimes.length = 0;
  lastTypeAt = 0;
  lastRecallAt = 0;

  document.addEventListener("keydown", onKeyDown, true);
  bag.push(() => document.removeEventListener("keydown", onKeyDown, true));
  document.addEventListener("keyup", onKeyUp, true);
  bag.push(() => document.removeEventListener("keyup", onKeyUp, true));
  document.addEventListener("focusin", onFocusIn, true);
  bag.push(() => document.removeEventListener("focusin", onFocusIn, true));
  document.addEventListener("focusout", onFocusOut, true);
  bag.push(() => document.removeEventListener("focusout", onFocusOut, true));
  document.addEventListener("input", onInput, true);
  bag.push(() => document.removeEventListener("input", onInput, true));
  document.addEventListener("wheel", onWheel, { capture: true, passive: false });
  bag.push(() => document.removeEventListener("wheel", onWheel, true));

  // S0/面板事件入口（napkin：中枢经 CustomEvent 派发，零 import 依赖）
  const onOpen = (e: Event): void => {
    const panel = (e as CustomEvent).detail?.panel as string | undefined;
    if (panel === "rhythm") openRhythmTrainer();
    else if (panel === "sandbox") sandboxOpen();
    else if (panel === "zones") toggleZonePanel();
  };
  window.addEventListener(INPUT_OPEN_EVENT, onOpen);
  bag.push(() => window.removeEventListener(INPUT_OPEN_EVENT, onOpen));
  const onStealth = (e: Event): void => {
    const v = (e as CustomEvent).detail?.active;
    if (typeof v === "boolean") setStealth(v);
  };
  window.addEventListener(INPUT_STEALTH_EVENT, onStealth);
  bag.push(() => window.removeEventListener(INPUT_STEALTH_EVENT, onStealth));

  auraTimer = setInterval(auraTick, 1000);
  bag.push(() => {
    if (auraTimer) clearInterval(auraTimer);
    auraTimer = null;
  });
}

export function deactivateInputNova(): void {
  if (!active) return;
  active = false;
  for (const fn of bag) {
    try {
      fn();
    } catch {
      /* 卸载容错 */
    }
  }
  bag = [];
  // 全部运行态归零（W-060 环槽跨会话不保留；W-053 训练即焚）
  stopRhythmSession();
  if (rhythmPanel) rhythmPanel.remove();
  rhythmPanel = null;
  poolCancel();
  hideGhosts();
  sandboxClose();
  auraRemove();
  slots = emptySlots();
  breath = breathInit();
  paceGaps.clear();
  paceCache.clear();
  hitTimes.length = 0;
  lastTypeAt = 0;
  lastRecallAt = 0;
  draftRun = "";
  punctPending = null;
  shiftHeld = false;
  if (recallDot) recallDot.remove();
  recallDot = null;
  if (f1HoldTimer) clearTimeout(f1HoldTimer);
  f1HoldTimer = null;
  if (slotHudTimer) clearTimeout(slotHudTimer);
  slotHudTimer = null;
  slotHud?.remove();
  slotHud = null;
  zonePanel?.remove();
  zonePanel = null;
  if (punctTimer) clearTimeout(punctTimer);
  punctTimer = null;
  document.querySelectorAll(".nova-input-recall").forEach((e) => e.remove());
}

export function isInputNovaActive(): boolean {
  return active;
}

/** 句柄（S0 NovaRuntime / 中枢直接消费）。 */
export const inputNova: InputNovaHandle = {
  activate(custom) {
    activateInputNova(custom);
  },
  deactivate() {
    deactivateInputNova();
  },
  api: {
    openRhythmTrainer() {
      openRhythmTrainer();
    },
    openSandbox() {
      if (flagOn("W-056")) sandboxOpen();
    },
    toggleZonePanel(force) {
      toggleZonePanel(force);
    },
    clearZones() {
      clearZones();
    },
    setStealth(v) {
      setStealth(v);
    },
    clipPut(slot) {
      return clipPut(slot);
    },
    clipTake(slot) {
      return clipTake(slot);
    },
    paceOf(appId) {
      return paceCache.get(appId) ?? null;
    },
  },
};
