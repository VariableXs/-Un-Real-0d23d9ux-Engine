/**
 * AI-14 Z-55 开放数据连接器（前端层）。
 *
 * 连接器 = 文件路径 + 类型 + 只读声明。Rust 侧执行真正的只读查询
 * （openhub_connector_query），这里提供类型定义与刷新策略（手动/定时，最短 10s）。
 */

export type ConnectorKind = "json" | "csv" | "sqlite";

export interface Connector {
  id: string;
  name: string;
  path: string;
  kind: ConnectorKind;
  /** sqlite 时的 SELECT 语句（仅 SELECT，Rust 侧白名单校验二次拦截） */
  sql?: string;
  /** 刷新秒数；0 = 手动（最短 10s） */
  refreshSec: number;
}

export function coerceConnector(raw: unknown): Connector | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  if (typeof r.id !== "string" || typeof r.path !== "string") return null;
  const kind = r.kind;
  if (kind !== "json" && kind !== "csv" && kind !== "sqlite") return null;
  const refresh = Number(r.refreshSec);
  return {
    id: r.id,
    name: typeof r.name === "string" ? r.name : r.id,
    path: r.path,
    kind,
    sql: typeof r.sql === "string" ? r.sql : undefined,
    refreshSec: Number.isFinite(refresh) && refresh >= 10 ? Math.floor(refresh) : 0,
  };
}

export function coerceConnectors(raw: unknown): Connector[] {
  if (!Array.isArray(raw)) return [];
  return raw.map(coerceConnector).filter((c): c is Connector => c !== null);
}

/** 刷新间隔规范化：0（手动）或 ≥10s。 */
export function normalizeRefresh(sec: number): number {
  return Number.isFinite(sec) && sec >= 10 ? Math.min(3600, Math.floor(sec)) : 0;
}
