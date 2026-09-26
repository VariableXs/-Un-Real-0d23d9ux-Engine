/**
 * F169/F162/F170 快捷键与证据引擎深化 · 事件语法解析 + 作用域 + 键位导出 +
 * 证据归档器。
 *
 * 主册判据延伸：
 * - F169【交互设计】「重录态：按下新组合即录，Esc 取消」——事件语法解析器把
 *   KeyboardEvent 归一为 Combo（捕获层独立于输入系统的规范化面）。
 * - F169【状态与异常】「应用级热键冲突 → 提示应用内改（边界诚实）」——作用域
 *   模型区分系统级/应用级。
 * - F170【数据与存储】「三步验收记录归档验收目录；逐项证据（配置哈希前后）」
 *   ——证据归档器把 DomainVerdict 落成 docs/acceptance 惯例 JSON。
 */

import {
  comboSignature, effectiveCombos, loadOverrides, normalizeCombo,
  type Combo, type ShortcutOverrides,
} from "./shortcuts";
import type { DomainVerdict } from "./verdict";

// ---------- 事件语法解析（KeyboardEvent → Combo） ----------

const NAMED_KEY_MAP: Record<string, string> = {
  " ": "Space",
  "Escape": "Esc",
};

export interface ComboParseResult {
  ok: boolean;
  reason: string;
  combo: Combo | null;
  /** 纯修饰键态（录制中——尚未按主键，UI 显示「等待主键」）。 */
  awaitingKey: boolean;
}

/**
 * KeyboardEvent 归一化：修饰键汇总 + 主键规范化（单字符大写、命名键映射）。
 * Esc 由捕获层在更外层消费（取消重录）——不会走到这里。
 */
export function parseComboFromEvent(e: {
  key: string; code: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean;
}): ComboParseResult {
  const modifiersOnly = ["Shift", "Control", "Alt", "Meta"].includes(e.key);
  const combo: Combo = {
    win: e.metaKey,
    ctrl: e.ctrlKey,
    alt: e.altKey,
    shift: e.shiftKey,
    key: "",
  };
  if (modifiersOnly) {
    return { ok: false, reason: "等待主键", combo: null, awaitingKey: true };
  }
  let key: string;
  if (e.key.length === 1) {
    key = e.key.toUpperCase();
  } else if (e.code.startsWith("Key") && e.code.length === 4) {
    key = e.code.slice(3); // IME/布局差异时回退物理码位
  } else if (e.code.startsWith("Digit") && e.code.length === 6) {
    key = e.code.slice(5);
  } else {
    key = NAMED_KEY_MAP[e.key] ?? e.key;
  }
  combo.key = key;
  const hasModifier = combo.win || combo.ctrl || combo.alt || combo.shift;
  if (!hasModifier) {
    return { ok: false, reason: "缺少修饰键——快捷键必须是「修饰键+主键」", combo: null, awaitingKey: false };
  }
  return { ok: true, reason: normalizeCombo(combo), combo, awaitingKey: false };
}

// ---------- 作用域模型（系统级 vs 应用级——边界诚实） ----------

export type ShortcutScope = "system" | "app";

export interface ScopedShortcut {
  id: string;
  scope: ShortcutScope;
  /** 应用级热键归属应用（冲突提示「应用内改」）。 */
  ownerApp?: string;
}

/** 冲突的类型分流：系统-系统冲突可由查看器仲裁；涉应用冲突提示应用内改。 */
export type ConflictKind = "system-system" | "app-involved" | "none";

export interface ConflictVerdict {
  kind: ConflictKind;
  withId: string | null;
  message: string | null;
}

export function classifyConflict(
  scoped: ScopedShortcut[],
  overrides: ShortcutOverrides,
  id: string,
  combo: Combo,
): ConflictVerdict {
  const sig = comboSignature(combo);
  const table = effectiveCombos(overrides);
  for (const [eid, c2] of table) {
    if (eid === id || comboSignature(c2) !== sig) continue;
    const a = scoped.find((s) => s.id === id);
    const b = scoped.find((s) => s.id === eid);
    const involved = a?.scope === "app" || b?.scope === "app";
    if (involved) {
      const owner = a?.scope === "app" ? a.ownerApp : b?.ownerApp;
      return { kind: "app-involved", withId: eid, message: `与「${owner ?? eid}」的应用级热键冲突——请在应用内修改（边界诚实）` };
    }
    return { kind: "system-system", withId: eid, message: `与系统快捷键 ${normalizeCombo(c2)} 冲突` };
  }
  return { kind: "none", withId: null, message: null };
}

// ---------- 键位导出/导入（round-trip——档案 F161 的键位分节加强） ----------

export interface KeymapExport {
  format: "vxkeymap";
  version: 1;
  overrides: Record<string, Combo>;
}

export function exportKeymap(overrides: ShortcutOverrides): KeymapExport {
  return { format: "vxkeymap", version: 1, overrides: JSON.parse(JSON.stringify(overrides.combos)) };
}

export function importKeymap(raw: unknown, current: ShortcutOverrides): { ok: boolean; reason: string; overrides: ShortcutOverrides } {
  if (typeof raw !== "object" || raw === null) return { ok: false, reason: "不是 JSON 对象", overrides: current };
  const p = raw as Partial<KeymapExport>;
  if (p.format !== "vxkeymap" || p.version !== 1 || typeof p.overrides !== "object" || p.overrides === null) {
    return { ok: false, reason: "不是合法 vxkeymap v1", overrides: current };
  }
  return { ok: true, reason: "键位导入完成", overrides: { combos: JSON.parse(JSON.stringify(p.overrides)), undo: current.undo } };
}

/** 当前生效组合快照（导出/对账共用——「全表条目与实际行为一致」的机械面）。 */
export function effectiveSnapshot(): { id: string; combo: string }[] {
  const overrides = loadOverrides();
  return [...effectiveCombos(overrides).entries()]
    .map(([id, combo]) => ({ id, combo: normalizeCombo(combo) }))
    .sort((a, b) => a.id.localeCompare(b.id));
}

// ---------- 证据归档器（F170 · docs/acceptance 惯例） ----------

export interface VerdictEvidenceDoc {
  format: "vx-acceptance";
  domain: "E-personalization";
  ranAt: string;
  verdict: DomainVerdict;
  /** 逐项配置哈希前后（回退零残留的机械证据）。 */
  notes: string[];
}

/** 归档文档组装（写盘由调用方执行——本层保持纯函数可测）。 */
export function buildEvidenceDoc(verdict: DomainVerdict, notes: string[]): VerdictEvidenceDoc {
  return {
    format: "vx-acceptance",
    domain: "E-personalization",
    ranAt: new Date().toISOString(),
    verdict,
    notes,
  };
}

/** 归档完整性自检（证据链完整率 100% 的落地口径）。 */
export function evidenceDocComplete(doc: VerdictEvidenceDoc): boolean {
  return doc.verdict.evidenceComplete
    && doc.verdict.items.every((i) => i.steps.every((s) => s.evidence.length > 0))
    && doc.notes.length > 0;
}
