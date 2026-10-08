/**
 * 任务 36（AI-V）：审计记录导出（JSON + CSV）。
 *
 * CSV 转义规则（RFC 4180）：
 *   - 字段含逗号、双引号、换行（\n）/回车（\r）时，整体用双引号包裹；
 *   - 字段内双引号转义为两个双引号 ""。
 * 审计理由码 / 规则摘要 summary 可能含逗号，必须正确引用转义（任务 36 验收）。
 */
import type { AuditEntry } from "./whitelistTypes";

/** 单个 CSV 字段转义。 */
export function csvEscapeField(value: string): string {
  if (/[",\r\n]/.test(value)) {
    return `"${value.replace(/"/g, '""')}"`;
  }
  return value;
}

export interface CsvColumn<T> {
  header: string;
  get: (row: T) => string | number | boolean;
}

/** 通用 CSV 序列化（表头 + 行），字段统一经 csvEscapeField。 */
export function toCsv<T>(rows: T[], columns: CsvColumn<T>[]): string {
  const header = columns.map((c) => csvEscapeField(c.header)).join(",");
  const body = rows
    .map((r) => columns.map((c) => csvEscapeField(String(c.get(r)))).join(","))
    .join("\r\n");
  return body.length > 0 ? `${header}\r\n${body}\r\n` : `${header}\r\n`;
}

const AUDIT_COLUMNS: CsvColumn<AuditEntry>[] = [
  { header: "seq", get: (e) => e.seq },
  { header: "source", get: (e) => e.source },
  { header: "pid", get: (e) => e.pid },
  { header: "operator", get: (e) => e.operator ?? "" },
  { header: "action", get: (e) => e.action ?? (e.allow ? (e.write ? "allow-write" : "allow-read") : e.write ? "deny-write" : "deny-read") },
  { header: "allow", get: (e) => (e.allow ? 1 : 0) },
  { header: "write", get: (e) => (e.write ? 1 : 0) },
  { header: "path", get: (e) => e.path },
  { header: "path_len", get: (e) => e.pathLen },
  { header: "summary", get: (e) => e.summary ?? "" },
];

/** 审计记录 → CSV 字符串（含表头）。 */
export function auditToCsv(entries: AuditEntry[]): string {
  return toCsv(entries, AUDIT_COLUMNS);
}

/** 审计记录 → JSON 字符串（pretty）。 */
export function auditToJson(entries: AuditEntry[]): string {
  return JSON.stringify(entries, null, 2);
}
