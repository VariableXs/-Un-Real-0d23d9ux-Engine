/**
 * F169 快捷键深化 · 组合匹配 + 序列缓冲（VSCode 式两段键）。
 *
 * 主册判据延伸：
 * - F169「全表条目与实际行为一致性抽测」——一致性需要一个权威匹配器：
 *   本引擎是「按下什么 → 触发什么」的唯一判定面（shortcut-engine 的
 *   parse/classify 管录入与冲突，本模块管运行期匹配）；
 * - 两段序列（Ctrl+K Ctrl+C）：缓冲区 + 超时（1s）+ Esc 清空——
 *   序列中途按别的键 = 缓冲作废（诚实失败，不误触发）；
 * - IME 纪律（B-904）：组合期输入一律不匹配（compositionFlags 消费）。
 */

// ---------- 键事件归一 ----------

export interface KeyEvent {
  key: string;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
}

/** 修饰键规范序：Ctrl → Alt → Shift → Meta（shortcut-engine 规范序同源）。 */
export function normalizeEvent(ev: KeyEvent): string {
  const parts: string[] = [];
  if (ev.ctrl) parts.push("Ctrl");
  if (ev.alt) parts.push("Alt");
  if (ev.shift) parts.push("Shift");
  if (ev.meta) parts.push("Meta");
  parts.push(displayKey(ev.key));
  return parts.join("+");
}

/** 键名显示归一（KeyA→A、ArrowUp→↑ 等显示面）。 */
export function displayKey(key: string): string {
  if (key.length === 1 && key >= "a" && key <= "z") return key.toUpperCase();
  const map: Record<string, string> = { ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→", Escape: "Esc", " ": "Space", Enter: "Enter", Tab: "Tab", Backspace: "Backspace", Delete: "Del", Home: "Home", End: "End", PageUp: "PgUp", PageDown: "PgDn" };
  return map[key] ?? key;
}

/** 纯修饰键事件（单独按 Ctrl/Shift 不该触发任何组合——修饰键不是键）。 */
export function isModifierOnly(ev: KeyEvent): boolean {
  return ["Control", "Shift", "Alt", "Meta"].includes(ev.key);
}

// ---------- 序列登记与匹配 ----------

export interface ChordBinding {
  /** 动作 id（表侧主键）。 */
  actionId: string;
  /** 组合序列（1 段或 2 段，如 ["Ctrl+K","Ctrl+C"]）。 */
  sequence: string[];
  enabled: boolean;
}

export type MatchResult =
  | { type: "match"; actionId: string; sequence: string[] }
  | { type: "pending"; consumed: string; remaining: string[]; timeoutMs: number }
  | { type: "none" }
  | { type: "conflict"; actionIds: string[] };

export const SEQUENCE_TIMEOUT_MS = 1000;

/**
 * 匹配引擎（有状态——序列缓冲）。构建时索引：
 * - 单段绑定 → 直接表；
 * - 两段绑定 → 首段 → 尾段集。
 * 冲突绑定（同序列多动作）在构建期检出——匹配到冲突序列返回 conflict
 * （一致性抽测的权威口径：冲突组合谁也不触发）。
 */
export class ChordMatcher {
  private single = new Map<string, string>();
  private singleConflicts = new Map<string, string[]>();
  private multiFirst = new Map<string, Map<string, string>>();
  private multiFirstConflicts = new Map<string, string[]>();
  private buffer: string[] = [];
  private bufferStart = 0;

  constructor(bindings: ChordBinding[]) {
    for (const b of bindings) {
      if (!b.enabled || b.sequence.length === 0) continue;
      if (b.sequence.length === 1) {
        const k = b.sequence[0]!;
        const existing = this.single.get(k);
        if (existing && existing !== b.actionId) {
          this.singleConflicts.set(k, [...(this.singleConflicts.get(k) ?? []), b.actionId]);
          this.single.delete(k); // 冲突序列谁也不触发（可预期 > 碰运气）。
        } else if (!existing) {
          this.single.set(k, b.actionId);
        }
      } else if (b.sequence.length === 2) {
        const first = b.sequence[0]!;
        const second = b.sequence[1]!;
        let secondMap = this.multiFirst.get(first);
        if (!secondMap) {
          secondMap = new Map();
          this.multiFirst.set(first, secondMap);
        }
        const existing = secondMap.get(second);
        if (existing && existing !== b.actionId) {
          this.multiFirstConflicts.set(first, [...(this.multiFirstConflicts.get(first) ?? []), b.actionId]);
          secondMap.delete(second);
        } else if (!existing) {
          secondMap.set(second, b.actionId);
        }
      }
      // 3 段以上：v1 契约不支持（shortcut-engine 录入侧同样拒绝——口径一致）。
    }
  }

  /** 当前缓冲内容（诊断面板——「序列中途」状态可见）。 */
  get pending(): string[] {
    return [...this.buffer];
  }

  /** 喂入一次按键。 */
  feed(ev: KeyEvent, now: number, composing = false): MatchResult {
    if (composing || isModifierOnly(ev)) {
      // 组合期/纯修饰键：不匹配不清缓冲（IME 纪律——组合结束前序列保持）。
      return { type: "none" };
    }
    if (ev.key === "Escape") {
      this.buffer = [];
      return { type: "none" }; // Esc = 清缓冲（真话：不触发任何绑定）。
    }
    const combo = normalizeEvent(ev);
    // 超时作废。
    if (this.buffer.length > 0 && now - this.bufferStart > SEQUENCE_TIMEOUT_MS) this.buffer = [];
    if (this.buffer.length === 0) {
      // 第一段。
      const direct = this.single.get(combo);
      if (direct) return { type: "match", actionId: direct, sequence: [combo] };
      const conflict = this.singleConflicts.get(combo);
      if (conflict) return { type: "conflict", actionIds: conflict };
      const secondMap = this.multiFirst.get(combo);
      if (secondMap && secondMap.size > 0) {
        this.buffer = [combo];
        this.bufferStart = now;
        const [second] = secondMap.keys();
        return { type: "pending", consumed: combo, remaining: second ? [second] : [], timeoutMs: SEQUENCE_TIMEOUT_MS };
      }
      return { type: "none" };
    }
    // 第二段。
    const first = this.buffer[0]!;
    const secondMap = this.multiFirst.get(first);
    if (secondMap) {
      const hit = secondMap.get(combo);
      this.buffer = [];
      if (hit) return { type: "match", actionId: hit, sequence: [first, combo] };
      return { type: "none" }; // 序列失败：缓冲清空（诚实——不串到别的首段）。
    }
    this.buffer = [];
    return { type: "none" };
  }

  /** 缓冲超时巡检（定时器消费——超时清空并报告）。 */
  sweepTimeout(now: number): boolean {
    if (this.buffer.length > 0 && now - this.bufferStart > SEQUENCE_TIMEOUT_MS) {
      this.buffer = [];
      return true;
    }
    return false;
  }
}

// ---------- 一致性对拍（F169 判据「表与实际行为一致」的机械面） ----------

export interface ConsistencyCheck {
  binding: ChordBinding;
  /** 表里登记的组合按序喂入 → 是否命中同 actionId。 */
  consistent: boolean;
  reason?: string;
}

/** 对拍一个绑定：按表序列合成键事件流喂入，期望命中同一动作。 */
export function checkBinding(target: ChordMatcher, b: ChordBinding): ConsistencyCheck {
  if (!b.enabled) return { binding: b, consistent: true, reason: "停用项不参与对拍" };
  let now = 0;
  for (const combo of b.sequence) {
    const ev = eventFromCombo(combo);
    if (!ev) return { binding: b, consistent: false, reason: `组合不可解析：${combo}` };
    const r = target.feed(ev, now);
    now += 100;
    if (r.type === "match") {
      return r.actionId === b.actionId
        ? { binding: b, consistent: true }
        : { binding: b, consistent: false, reason: `命中了 ${r.actionId}（应为 ${b.actionId}）` };
    }
    if (r.type === "conflict") return { binding: b, consistent: false, reason: `序列冲突：${r.actionIds.join(",")}` };
    if (r.type === "none" && b.sequence.length > 1) return { binding: b, consistent: false, reason: `首段未进入缓冲：${combo}` };
  }
  return { binding: b, consistent: false, reason: "序列喂完未命中" };
}

/** 组合串 → 键事件（对拍用合成事件——显示键名映射回事件键名）。 */
export function eventFromCombo(combo: string): KeyEvent | null {
  const parts = combo.split("+");
  const key = parts[parts.length - 1]!;
  const ctrl = parts.includes("Ctrl");
  const alt = parts.includes("Alt");
  const shift = parts.includes("Shift");
  const meta = parts.includes("Meta");
  // 键名白名单：单字符 ASCII 可见键、F 键、或显示名映射表内的键——
  // 其余（注入符/非 ASCII 串）拒绝（表侧录入已保证，这里是双保险）。
  const rev: Record<string, string> = { "↑": "ArrowUp", "↓": "ArrowDown", "←": "ArrowLeft", "→": "ArrowRight", Esc: "Escape", Space: " ", Del: "Delete", PgUp: "PageUp", PgDn: "PageDown", Enter: "Enter", Tab: "Tab", Backspace: "Backspace", Home: "Home", End: "End" };
  const known = key.length === 1 && key >= "!" && key <= "~";
  const isFKey = /^F([1-9]|1[0-9])$/.test(key);
  if (!known && !isFKey && !(key in rev)) return null;
  const ev: KeyEvent = { key: rev[key] ?? key, ctrl, alt, shift, meta };
  // 往返一致性：合成事件归一化后必须等于输入（不规范组合拒绝）。
  return normalizeEvent(ev) === combo ? ev : null;
}

/** 全表对拍（F169 抽测 20 条的机械化——全量更便宜）。 */
export function auditAllBindings(bindings: ChordBinding[]): { checks: ConsistencyCheck[]; consistent: number; total: number } {
  const matcher = new ChordMatcher(bindings);
  const checks = bindings.filter((b) => b.enabled).map((b) => checkBinding(matcher, b));
  return { checks, consistent: checks.filter((c) => c.consistent).length, total: checks.length };
}
