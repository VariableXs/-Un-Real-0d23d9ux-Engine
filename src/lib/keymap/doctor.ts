/**
 * V-94 键位体检医生 — 全量键位健康检查。
 *
 * 检查项（依赖本轮所有键位登记完毕后运行）：
 * 1. 注册表内 error 级冲突（应恒为 0）；
 * 2. 裸 keydown 泄漏（由 tools/keymap-audit.cjs 静态扫描，此处汇总其报告）；
 * 3. 用户绑定命中系统保留键；
 * 4. 同 combo 跨 scope warn 汇总；
 * 5. 空闲槽建议（M-36 建议函数复用）。
 */

import type { KeyBinding } from "./types";
import { snapshot, conflicts } from "./registry";
import { effectiveBinds } from "../shortcuts";
import { SYSTEM_RESERVED } from "./system-combos";
import { suggestFreeSlots } from "./preview";

export type DoctorSeverity = "info" | "warn" | "error";

export interface DoctorFinding {
  severity: DoctorSeverity;
  code: string;
  message: string;
}

export interface DoctorReport {
  healthy: boolean;
  findings: DoctorFinding[];
  freeSlots: string[];
  checkedAt: string;
}

export interface DoctorInput {
  /** 用户覆盖表（settings.shortcutBinds）。 */
  overrides: Record<string, string>;
  /** keymap-audit.cjs 静态扫描出的裸 keydown 处数。 */
  bareKeydownCount?: number;
}

export function runKeymapDoctor(input: DoctorInput): DoctorReport {
  const findings: DoctorFinding[] = [];

  for (const c of conflicts()) {
    if (c.kind === "error") {
      findings.push({ severity: "error", code: "registry:conflict", message: `${c.combo} 同作用域冲突：${c.ids.join(" ↔ ")}` });
    } else {
      findings.push({ severity: "warn", code: "registry:cross-scope", message: `${c.combo} 跨作用域共存：${c.ids.join(" ↔ ")}` });
    }
  }

  const binds = effectiveBinds(input.overrides);
  for (const b of binds) {
    if (b.accel !== "" && SYSTEM_RESERVED.has(b.accel)) {
      findings.push({ severity: "error", code: "reserved:hit", message: `${b.action} 占用系统保留键 ${b.accel}` });
    }
  }
  const dup = new Map<string, string[]>();
  for (const b of binds) {
    if (b.accel === "") continue;
    dup.set(b.accel, [...(dup.get(b.accel) ?? []), b.action]);
  }
  for (const [accel, owners] of dup) {
    if (owners.length > 1) {
      findings.push({ severity: "error", code: "binds:conflict", message: `${accel} 被 ${owners.join(", ")} 重复绑定` });
    }
  }

  if ((input.bareKeydownCount ?? 0) > 0) {
    findings.push({
      severity: "warn",
      code: "audit:bare-keydown",
      message: `发现 ${input.bareKeydownCount} 处未走注册表的裸 keydown（运行 tools/keymap-audit.cjs 查看明细）`,
    });
  }

  const disabled = binds.filter((b) => b.accel === "").length;
  if (disabled > 0) {
    findings.push({ severity: "info", code: "binds:disabled", message: `${disabled} 个动作键位已禁用（方案补丁）` });
  }

  const taken = new Set(binds.map((b) => b.accel).filter((a) => a !== ""));
  return {
    healthy: !findings.some((f) => f.severity === "error"),
    findings,
    freeSlots: suggestFreeSlots(taken, 5),
    checkedAt: new Date().toISOString(),
  };
}

/** 注册表绑定列表（体检页展示）。 */
export function listBindings(): KeyBinding[] {
  return snapshot();
}
