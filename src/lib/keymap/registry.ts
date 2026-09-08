/**
 * Z-08 键位注册表 — 注册/注销/快照/冲突检测。
 *
 * 规则（承实施步骤 阶段 0.1）：
 * - 注册时立即做静态冲突检测：同 scope 同 combo = error（拒绝注册）；
 *   跨 scope 同 combo = warn（记录，放行）。
 * - 同 scope 同 combo 同 id = 重复注册，幂等更新。
 * - 面板卸载必须 unregister，快照 diff 为空（键位零泄漏）。
 */

import type {
  ConflictRecord,
  KeyBinding,
  KeyScope,
  RegisterResult,
} from "./types";
import { normalizeAccel } from "../shortcuts";

const SCOPE_ORDER: Record<KeyScope, number> = { context: 0, window: 1, global: 2 };

interface RegistryState {
  bindings: Map<string, KeyBinding>;
  conflicts: ConflictRecord[];
}

const state: RegistryState = { bindings: new Map(), conflicts: [] };

function comboOf(raw: string): string | null {
  return normalizeAccel(raw);
}

/** 冲突检测：返回与 candidate 冲突的记录（不含历史记录）。 */
export function detectConflicts(candidate: KeyBinding): ConflictRecord[] {
  const out: ConflictRecord[] = [];
  for (const b of state.bindings.values()) {
    if (b.id === candidate.id) continue;
    if (b.combo !== candidate.combo) continue;
    if (b.scope === candidate.scope) {
      out.push({
        kind: "error",
        combo: candidate.combo,
        ids: b.priority >= candidate.priority ? [b.id, candidate.id] : [candidate.id, b.id],
        reason: `same-scope(${candidate.scope})`,
      });
    } else {
      out.push({
        kind: "warn",
        combo: candidate.combo,
        ids: SCOPE_ORDER[b.scope] >= SCOPE_ORDER[candidate.scope]
          ? [b.id, candidate.id]
          : [candidate.id, b.id],
        reason: `cross-scope(${b.scope} vs ${candidate.scope})`,
      });
    }
  }
  return out;
}

/** 注册一个键位。同 scope 同 combo 冲突时拒绝（除非覆盖低优先级者）。 */
export function register(
  binding: Omit<KeyBinding, "combo"> & { combo: string },
): RegisterResult {
  const combo = comboOf(binding.combo);
  if (!combo) {
    return { ok: false, error: `invalid combo: ${binding.combo}`, conflicts: [] };
  }
  const normalized: KeyBinding = { ...binding, combo };
  const hits = detectConflicts(normalized);
  const errors = hits.filter((c) => c.kind === "error");

  // 同 scope 冲突：仅当 candidate 优先级更高时允许挤占（被挤占者保留登记但降级为 shadowed）
  if (errors.length > 0) {
    const loser = [...state.bindings.values()].find(
      (b) => b.combo === normalized.combo && b.scope === normalized.scope,
    );
    if (!loser || loser.priority >= normalized.priority) {
      state.conflicts.push(...hits);
      return { ok: false, error: "combo-conflict", conflicts: hits };
    }
  }

  state.conflicts.push(...hits.filter((c) => c.kind === "warn"));
  state.bindings.set(normalized.id, normalized);
  return { ok: true, binding: normalized };
}

/** 注销；返回是否确实移除。 */
export function unregister(id: string): boolean {
  return state.bindings.delete(id);
}

/** 当前登记快照（按 combo 排序，保证 diff 稳定）。 */
export function snapshot(): KeyBinding[] {
  return [...state.bindings.values()].sort(
    (a, b) => a.combo.localeCompare(b.combo) || a.id.localeCompare(b.id),
  );
}

/** 全部冲突记录（error + warn）。 */
export function conflicts(): ConflictRecord[] {
  return [...state.conflicts];
}

/** 按 scope 查询激活键位（供 Z-12 速查浮层 / Z-13 提示条实时读取）。 */
export function activeByScope(scope: KeyScope): KeyBinding[] {
  return snapshot().filter((b) => b.scope === scope);
}

/** 测试与热重载用：清空注册表。生产代码不得调用。 */
export function __resetForTests(): void {
  state.bindings.clear();
  state.conflicts.length = 0;
}
