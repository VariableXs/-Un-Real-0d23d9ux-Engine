/**
 * Z-14 键位方案管理 — 三套预设（JSON 补丁集）。
 *
 * 方案 = 对 SHORTCUT_ACTIONS 默认表的覆盖补丁（记录 override map），
 * 应用 = 合并进 settings.shortcutBinds；导入须过 schema 校验，
 * 冲突（同 combo 多 action / 命中系统保留键）交由 M-36 预览列表用户裁决。
 */

import { SHORTCUT_ACTIONS, findConflicts } from "../shortcuts";
import { SYSTEM_RESERVED } from "./system-combos";

export interface KeymapProfile {
  /** 稳定 id。 */
  id: string;
  /** 展示名（i18n 外置）。 */
  nameKey: string;
  /** 覆盖补丁：actionId → accel。空 = 与默认表一致。 */
  patch: Record<string, string>;
}

export const KEYMAP_PROFILES: KeymapProfile[] = [
  {
    id: "default",
    nameKey: "kmProfileDefault",
    patch: {},
  },
  {
    id: "leftHand",
    nameKey: "kmProfileLeftHand",
    patch: {
      explorer: "ctrl+alt+q",
      settingsCenter: "ctrl+alt+w",
      // AI-20 协同修复：ctrl+alt+r 与 AI-08 Z-28 runDialog 降级口径撞车 → 改 t（同为左手顶排）
      clipboardHistory: "ctrl+alt+t",
      notifyCenter: "ctrl+alt+f",
      quickBluetooth: "ctrl+alt+z",
      quickAudio: "ctrl+alt+x",
    },
  },
  {
    id: "minimal",
    nameKey: "kmProfileMinimal",
    patch: {
      // 关闭装饰性组合（置空 accel = 禁用该 action 键位）
      quickBluetooth: "",
      quickAudio: "",
      dnd: "",
      launch8: "",
      launch9: "",
    },
  },
];

export interface ProfileValidation {
  ok: boolean;
  errors: string[];
  /** 冲突 combo 集合（M-36 预览列表数据源）。 */
  conflictList: string[];
  /** 命中的系统保留键（风险高）。 */
  reservedHits: string[];
}

/** 导入/应用前校验：schema + 占用冲突 + 系统保留键命中。 */
export function validateProfilePatch(patch: unknown): ProfileValidation {
  const errors: string[] = [];
  const conflictList: string[] = [];
  const reservedHits: string[] = [];
  if (typeof patch !== "object" || patch === null || Array.isArray(patch)) {
    return { ok: false, errors: ["schema:patch-not-object"], conflictList: [], reservedHits };
  }
  const validIds = new Set(SHORTCUT_ACTIONS.map((a) => a.id));
  const entries = Object.entries(patch as Record<string, unknown>);
  for (const [k, v] of entries) {
    if (!validIds.has(k)) errors.push(`schema:unknown-action:${k}`);
    if (typeof v !== "string") errors.push(`schema:accel-not-string:${k}`);
  }
  if (errors.length > 0) {
    return { ok: false, errors, conflictList: [], reservedHits };
  }
  const binds = SHORTCUT_ACTIONS.map((a) => ({
    action: a.id,
    accel: (patch as Record<string, string>)[a.id] ?? a.accel,
  })).filter((b) => b.accel !== "");
  for (const c of findConflicts(binds)) conflictList.push(c);
  for (const b of binds) {
    if (SYSTEM_RESERVED.has(b.accel)) reservedHits.push(b.accel);
  }
  return { ok: conflictList.length === 0 && reservedHits.length === 0, errors, conflictList, reservedHits };
}

/** 应用方案：终态 = default 表 + 该方案补丁（切换即重置，保证往返终态一致）。 */
export function applyProfile(
  currentOverrides: Record<string, string>,
  profile: KeymapProfile,
): Record<string, string> {
  void currentOverrides; // 切换即重置：终态 = default + 本方案补丁（Z-14 往返门禁）
  return { ...profile.patch };
}
