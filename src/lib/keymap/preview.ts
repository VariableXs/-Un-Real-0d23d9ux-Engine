/**
 * M-36 键位变更预览 — 保存前 diff + 空闲槽建议。
 *
 * 1. effectiveBinds(newOverrides) 与当前表 diff：找被挤占 action + 系统保留键命中标注；
 * 2. 「自动挑空闲槽」：扫描 ctrl+alt+* 未占用槽建议。
 */

import { SHORTCUT_ACTIONS, effectiveBinds } from "../shortcuts";
import { SYSTEM_RESERVED } from "./system-combos";

export type ChangeRisk = "high" | "low";

export interface BindingChange {
  action: string;
  from: string;
  to: string;
  /** to 命中系统保留键（风险高）。 */
  reservedHit: boolean;
}

export interface DisplacedAction {
  /** 被挤占的 action（其原 accel 被新方案占用）。 */
  action: string;
  /** 被谁挤占。 */
  by: string;
  accel: string;
}

export interface PreviewReport {
  changes: BindingChange[];
  displaced: DisplacedAction[];
  conflicts: string[];
}

/** 保存前 diff 报告。 */
export function previewChanges(newOverrides: Record<string, string>): PreviewReport {
  const current = new Map(effectiveBinds({}).map((b) => [b.action, b.accel]));
  const next = effectiveBinds(newOverrides);
  const changes: BindingChange[] = [];
  const counts = new Map<string, number>();
  for (const b of next) {
    if (b.accel === "") continue; // 禁用项不参与占用
    counts.set(b.accel, (counts.get(b.accel) ?? 0) + 1);
  }
  for (const b of next) {
    const from = current.get(b.action) ?? "";
    if (from === b.accel) continue;
    changes.push({
      action: b.action,
      from,
      to: b.accel,
      reservedHit: b.accel !== "" && SYSTEM_RESERVED.has(b.accel),
    });
  }
  const conflicts = [...counts.entries()].filter(([, n]) => n > 1).map(([accel]) => accel);
  const displaced: DisplacedAction[] = [];
  for (const accel of conflicts) {
    const owners = next.filter((b) => b.accel === accel);
    const keeper = owners[0];
    if (!keeper) continue;
    // 保留默认归属（默认表顺序第一个），其余视为被挤占
    for (const victim of owners.slice(1)) {
      displaced.push({ action: victim.action, by: keeper.action, accel });
    }
  }
  return { changes, displaced, conflicts };
}

/** 空闲槽建议：ctrl+alt+<key> 扫描（数字/字母），未被默认表与保留库占用。 */
export function suggestFreeSlots(takenAccels: Iterable<string>, limit = 5): string[] {
  const taken = new Set(takenAccels);
  const out: string[] = [];
  const candidates = "abcdefghijklmnopqrstuvwxyz".split("").concat("0123456789".split(""));
  for (const key of candidates) {
    const combo = `ctrl+alt+${key}`;
    if (taken.has(combo) || SYSTEM_RESERVED.has(combo)) continue;
    if (SHORTCUT_ACTIONS.some((a) => a.accel === combo)) continue;
    out.push(combo);
    if (out.length >= limit) break;
  }
  return out;
}
