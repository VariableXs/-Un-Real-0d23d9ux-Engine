/**
 * UNREAL-X AI-17 · 族0161 按键手感 2.0 + 族0162 文本编辑手感 2.0 + 族0163 代码输入 2.0
 * （X04001~X04075）。
 *
 * 纯逻辑模块：五档档位矩阵、非法输入钳制、错误叙事码（KF/EF/CI-xxx，禁裸报错）、
 * 快照导出/导入（跨版本携带）、低资源降级链、回滚净身。React 只消费这里的数据。
 */

/* ============================== 族0161 按键手感 2.0 ============================== */

/** 按键手感五档（默认 = balanced 不改既有手感）。 */
export const KEY_FEEL_PROFILES = [
  { id: "linear", name: "直感", repeatDelayMs: 500, repeatRateMs: 60, pressDepth: 0.4, haptic: 0 },
  { id: "brisk", name: "轻快", repeatDelayMs: 420, repeatRateMs: 50, pressDepth: 0.6, haptic: 1 },
  { id: "balanced", name: "均衡", repeatDelayMs: 360, repeatRateMs: 40, pressDepth: 0.8, haptic: 2 },
  { id: "deep", name: "沉浸", repeatDelayMs: 300, repeatRateMs: 32, pressDepth: 1.0, haptic: 3 },
  { id: "mech", name: "机械", repeatDelayMs: 260, repeatRateMs: 26, pressDepth: 1.2, haptic: 4 },
] as const;
export type KeyFeelProfileId = (typeof KEY_FEEL_PROFILES)[number]["id"];
export const DEFAULT_KEY_FEEL_ID: KeyFeelProfileId = "balanced";

export function findKeyFeel(id: string): (typeof KEY_FEEL_PROFILES)[number] {
  return KEY_FEEL_PROFILES.find((p) => p.id === id) ?? KEY_FEEL_PROFILES[2]!;
}

/** 按键叙事码（禁裸报错：每码带下一步建议）。 */
export const KEY_FEEL_NARRATIVES: readonly { code: string; title: string; next: string }[] = [
  { code: "KF-401", title: "重复率配置越界", next: "已回默认档，可在输入手感里重新调节。" },
  { code: "KF-402", title: "连击缓冲溢出", next: "已丢弃最旧事件并提示，可在守护里开丢弃策略。" },
  { code: "KF-403", title: "触感回馈不可用", next: "设备不支持触感，已降级为视觉按压反馈。" },
  { code: "KF-404", title: "省电档触感关闭", next: "接通电源或退出省电档即可恢复。" },
];

export function findKeyNarrative(code: string): { code: string; title: string; next: string } {
  const up = code.toUpperCase();
  return KEY_FEEL_NARRATIVES.find((n) => n.code === up)
    ?? { code: "KF-000", title: "未知按键状况", next: "重跑输入手感自检，或回默认档。" };
}

export interface KeyStroke {
  code: string;
  atMs: number;
}

/** 按键事件管线：连击缓冲 + 丢弃策略 + 重复节拍计算。 */
export class KeyFeelPipeline {
  profileId: KeyFeelProfileId;
  /** 连击缓冲上限（越界触发 KF-402 丢弃最旧）。 */
  bufferLimit: number;
  buffer: KeyStroke[] = [];
  clamped = 0;
  droppedOldest = 0;
  narratives: string[] = [];

  constructor(profileId: string = DEFAULT_KEY_FEEL_ID, bufferLimit = 32) {
    const known = KEY_FEEL_PROFILES.some((p) => p.id === profileId);
    if (!known) this.clamped += 1;
    this.profileId = (findKeyFeel(profileId).id) as KeyFeelProfileId;
    this.bufferLimit = Math.max(1, Math.min(256, bufferLimit));
  }

  get profile() {
    return findKeyFeel(this.profileId);
  }

  /** 推入按键事件；溢出丢弃最旧（守护开关）。 */
  push(stroke: KeyStroke, dropOldestOnOverflow = true): boolean {
    if (this.buffer.length >= this.bufferLimit) {
      if (!dropOldestOnOverflow) return false;
      this.buffer.shift();
      this.droppedOldest += 1;
      if (!this.narratives.includes("KF-402")) this.narratives.push("KF-402");
    }
    this.buffer.push(stroke);
    return true;
  }

  /** 重复节拍：第 n 次自动重复应到达的时刻。 */
  repeatAt(stroke: KeyStroke, nthRepeat: number): number {
    const p = this.profile;
    const n = Math.max(0, Math.min(1000, nthRepeat));
    return stroke.atMs + p.repeatDelayMs + p.repeatRateMs * n;
  }

  /** 低资源降级链：core≤2 或省电 → 触感归零、深度减半。 */
  degrade(cpuCores: number, batterySaver: boolean): { haptic: number; pressDepth: number } {
    const p = this.profile;
    if (batterySaver) return { haptic: 0, pressDepth: p.pressDepth / 2 };
    if (cpuCores <= 2) return { haptic: Math.min(1, p.haptic), pressDepth: p.pressDepth * 0.75 };
    return { haptic: p.haptic, pressDepth: p.pressDepth };
  }

  snapshot(): string {
    return JSON.stringify({ profileId: this.profileId, limit: this.bufferLimit, v: 2 });
  }

  restore(snap: string | null): boolean {
    if (!snap) return false;
    try {
      const o = JSON.parse(snap) as { profileId?: string; limit?: number; v?: number };
      if (o.v !== 2 || typeof o.profileId !== "string") return false;
      this.profileId = findKeyFeel(o.profileId).id as KeyFeelProfileId;
      if (typeof o.limit === "number") this.bufferLimit = Math.max(1, Math.min(256, o.limit));
      this.buffer = [];
      return true;
    } catch {
      return false;
    }
  }

  /** 回滚净身：不留残档。 */
  reset(): void {
    this.buffer = [];
    this.narratives = [];
    this.droppedOldest = 0;
    this.clamped = 0;
  }
}

/* ============================== 族0162 文本编辑手感 2.0 ============================== */

/** 编辑手感五档（光标/选区/自动配对等行为档）。 */
export const EDIT_FEEL_PROFILES = [
  { id: "plain", name: "朴素", autoPair: false, smartIndent: false, cursorBlinkMs: 0 },
  { id: "light", name: "轻量", autoPair: false, smartIndent: true, cursorBlinkMs: 900 },
  { id: "balanced", name: "均衡", autoPair: true, smartIndent: true, cursorBlinkMs: 530 },
  { id: "smart", name: "智能", autoPair: true, smartIndent: true, cursorBlinkMs: 400 },
  { id: "flow", name: "心流", autoPair: true, smartIndent: true, cursorBlinkMs: 0 },
] as const;
export type EditFeelProfileId = (typeof EDIT_FEEL_PROFILES)[number]["id"];
export const DEFAULT_EDIT_FEEL_ID: EditFeelProfileId = "balanced";

export function findEditFeel(id: string): (typeof EDIT_FEEL_PROFILES)[number] {
  return EDIT_FEEL_PROFILES.find((p) => p.id === id) ?? EDIT_FEEL_PROFILES[2]!;
}

const PAIR_OPEN = { "(": ")", "[": "]", "{": "}", '"': '"', "'": "'", "（": "）", "「": "」", "『": "』" } as Record<string, string>;

/** 自动配对引擎：CJK 全角括号与西文引号统一处理（族0162 核心逻辑）。 */
export class PairEngine {
  enabled: boolean;
  openStack: string[] = [];
  clamped = 0;

  constructor(enabled = true) {
    this.enabled = enabled;
  }

  /** 输入一个字符，返回建议补全（无建议返回 ""）。 */
  type(ch: string): string {
    if (!this.enabled) return "";
    const close = PAIR_OPEN[ch];
    if (close) {
      this.openStack.push(ch);
      if (this.openStack.length > 64) {
        this.openStack.shift();
        this.clamped += 1;
      }
      return close;
    }
    // 输入的是闭合符且栈顶匹配 → 吞掉（跳过）而非重复插入。
    const top = this.openStack[this.openStack.length - 1];
    if (top && PAIR_OPEN[top] === ch) {
      this.openStack.pop();
      return "\u0000skip";
    }
    return "";
  }

  /** 手动输入闭合符时是否应当跳过。 */
  shouldSkipClose(ch: string): boolean {
    return this.type(ch) === "\u0000skip";
  }

  /** 全选删除/退格的配对回滚：栈弹一个。 */
  backspace(): void {
    this.openStack.pop();
  }

  reset(): void {
    this.openStack = [];
    this.clamped = 0;
  }
}

/** 智能缩进计算：给定当前行缩进与触发键，算下一行缩进（空格数）。 */
export function nextIndent(curIndent: number, trigger: "enter" | "colon" | "brace-open" | "brace-close", tabWidth = 4): number {
  const w = Math.max(1, Math.min(8, tabWidth));
  const base = Math.max(0, Math.min(256, curIndent));
  switch (trigger) {
    case "enter": return base;
    case "colon": return base + w;
    case "brace-open": return base + w;
    case "brace-close": return Math.max(0, base - w);
    default: return base;
  }
}

/* ============================== 族0163 代码输入 2.0 ============================== */

/** 代码输入五档（补全触发/括号跳格/缩进策略）。 */
export const CODE_INPUT_PROFILES = [
  { id: "manual", name: "手动", autoTrigger: false, tabJumps: false, indent: "space2" },
  { id: "light", name: "轻量", autoTrigger: false, tabJumps: true, indent: "space4" },
  { id: "balanced", name: "均衡", autoTrigger: true, tabJumps: true, indent: "space4" },
  { id: "eager", name: "积极", autoTrigger: true, tabJumps: true, indent: "space2" },
  { id: "tabby", name: "全动", autoTrigger: true, tabJumps: true, indent: "tab" },
] as const;
export type CodeInputProfileId = (typeof CODE_INPUT_PROFILES)[number]["id"];
export const DEFAULT_CODE_INPUT_ID: CodeInputProfileId = "balanced";

export function findCodeInput(id: string): (typeof CODE_INPUT_PROFILES)[number] {
  return CODE_INPUT_PROFILES.find((p) => p.id === id) ?? CODE_INPUT_PROFILES[2]!;
}

export interface CodeSuggestion {
  label: string;
  kind: "keyword" | "symbol" | "snippet";
  score: number;
}

/** 补全触发器：前缀匹配 + 打分排序 + 越界钳制。 */
export class CompletionTrigger {
  profileId: CodeInputProfileId;
  symbols: string[] = [];
  lastTriggerAt = -Infinity;
  /** 触发节流（ms）：防抖动重复触发。 */
  debounceMs: number;
  clamped = 0;

  constructor(profileId: string = DEFAULT_CODE_INPUT_ID, symbols: string[] = [], debounceMs = 120) {
    const known = CODE_INPUT_PROFILES.some((p) => p.id === profileId);
    if (!known) this.clamped += 1;
    this.profileId = findCodeInput(profileId).id as CodeInputProfileId;
    this.symbols = symbols.slice(0, 4096);
    this.debounceMs = Math.max(0, Math.min(2000, debounceMs));
  }

  /** 前缀建议：score = 前缀长度占比 ×100，同前缀按字典序稳定排序。 */
  suggest(prefix: string, limit = 8): CodeSuggestion[] {
    const p = Math.max(0, Math.min(64, prefix.length));
    if (p === 0) return [];
    const lower = prefix.toLowerCase();
    const hits = this.symbols
      .filter((s) => s.toLowerCase().startsWith(lower))
      .map((s) => ({ label: s, kind: "symbol" as const, score: Math.round((p / Math.max(1, s.length)) * 100) }))
      .sort((a, b) => b.score - a.score || a.label.localeCompare(b.label));
    return hits.slice(0, Math.max(1, Math.min(64, limit)));
  }

  /** 是否允许本次触发（节流）。 */
  canTrigger(atMs: number): boolean {
    if (!findCodeInput(this.profileId).autoTrigger) return false;
    if (atMs - this.lastTriggerAt < this.debounceMs) return false;
    this.lastTriggerAt = atMs;
    return true;
  }

  /** Tab 跳格：返回下一个光标停靠偏移（相对当前 token 起点）。 */
  nextTabStop(tokenLen: number, tabWidth = 4): number {
    if (!findCodeInput(this.profileId).tabJumps) return -1;
    const w = Math.max(1, Math.min(16, tabWidth));
    const len = Math.max(0, tokenLen);
    return (Math.floor(len / w) + 1) * w;
  }

  snapshot(): string {
    return JSON.stringify({ profileId: this.profileId, sym: this.symbols.length, v: 2 });
  }

  restore(snap: string | null): boolean {
    if (!snap) return false;
    try {
      const o = JSON.parse(snap) as { profileId?: string; sym?: number; v?: number };
      if (o.v !== 2 || typeof o.profileId !== "string") return false;
      this.profileId = findCodeInput(o.profileId).id as CodeInputProfileId;
      return true;
    } catch {
      return false;
    }
  }
}
