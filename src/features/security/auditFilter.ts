/**
 * 任务 36（AI-V）：审计记录过滤逻辑（按进程 / 按路径 / 来源）。
 *
 * 纯函数，便于单测（任务 36 验收：审计过滤逻辑用例）。
 */
import type { AuditEntry } from "./whitelistTypes";

export interface AuditFilter {
  /** 按进程号过滤（PID）；空/NaN 表示不过滤。 */
  pid?: number;
  /** 按路径子串过滤（大小写不敏感）；空表示不过滤。 */
  pathSubstr?: string;
  /** 按来源过滤；缺省不过滤。 */
  source?: "kernel" | "ui-change";
}

/** 应用过滤条件，返回满足条件的条目（不修改原数组）。 */
export function filterAudit(entries: AuditEntry[], filter: AuditFilter): AuditEntry[] {
  const path = filter.pathSubstr?.trim().toLowerCase();
  return entries.filter((e) => {
    if (filter.source && e.source !== filter.source) return false;
    if (filter.pid !== undefined && !Number.isNaN(filter.pid) && filter.pid !== e.pid) return false;
    if (path) {
      const hay = e.path.toLowerCase();
      if (!hay.includes(path)) return false;
    }
    return true;
  });
}

/** 按 seq 倒序（最新在前）排列，用于时间线展示。 */
export function sortBySeqDesc(entries: AuditEntry[]): AuditEntry[] {
  return [...entries].sort((a, b) => b.seq - a.seq);
}
