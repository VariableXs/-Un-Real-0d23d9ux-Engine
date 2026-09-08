/**
 * AI-11 U-48 性能模式切换器 + V-54 保持唤醒（纯逻辑模块）。
 *
 * 性能模式 = 电源计划 + 保持唤醒的预设组合：
 * - eco：节能计划，不保持唤醒
 * - balanced：平衡计划（默认）
 * - boost：高性能计划 + 保持唤醒（防止演示中休眠）
 * 计划 GUID 由运行时 power_schemes_list 动态匹配（各机器 GUID 不同，
 * 按名称启发式匹配，匹配不到则该预设降级为仅保持唤醒位）。
 * 用户选择持久化 localStorage；一切系统写入仍走显式 IPC（红线）。
 */

export type PerfMode = "eco" | "balanced" | "boost";

export interface PerfModeProfile {
  mode: PerfMode;
  /** 电源计划名称匹配关键词（小写子串，按系统语言兜底多组） */
  planHints: string[];
  /** 保持唤醒（演示防休眠） */
  keepAwake: boolean;
}

export const PERF_PROFILES: Record<PerfMode, PerfModeProfile> = {
  eco: { mode: "eco", planHints: ["power saver", "节能"], keepAwake: false },
  balanced: { mode: "balanced", planHints: ["balanced", "平衡"], keepAwake: false },
  boost: { mode: "boost", planHints: ["high performance", "高性能"], keepAwake: true },
};

const MODE_KEY = "variable:ai11:perfmode";

export function loadPerfMode(): PerfMode {
  try {
    const raw = localStorage.getItem(MODE_KEY);
    if (raw === "eco" || raw === "balanced" || raw === "boost") return raw;
  } catch {
    /* storage blocked */
  }
  return "balanced";
}

export function savePerfMode(mode: PerfMode): void {
  try {
    localStorage.setItem(MODE_KEY, mode);
  } catch {
    /* storage blocked */
  }
}

/** 在系统计划列表里按预设关键词匹配 GUID（找不到返回 null，前端如实降级）。 */
export function matchSchemeGuid(
  schemes: { guid: string; name: string; active: boolean }[],
  mode: PerfMode,
): string | null {
  const hints = PERF_PROFILES[mode].planHints;
  for (const hint of hints) {
    const hit = schemes.find((s) => s.name.toLowerCase().includes(hint));
    if (hit) return hit.guid;
  }
  return null;
}

// ---------- V-54 保持唤醒倒计时（纯逻辑：分钟 → 到期时刻） ----------

/** 计算保持唤醒到期时刻（毫秒时间戳）；minutes ≤ 0 视为不限时（返回 null）。 */
export function keepAwakeDeadline(nowMs: number, minutes: number): number | null {
  if (!Number.isFinite(minutes) || minutes <= 0) return null;
  return nowMs + Math.round(minutes * 60_000);
}

/** 到期判定（HUD/定时器共用）。 */
export function keepAwakeExpired(nowMs: number, deadlineMs: number | null): boolean {
  if (deadlineMs === null) return false;
  return nowMs >= deadlineMs;
}
