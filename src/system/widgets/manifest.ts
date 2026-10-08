/**
 * N-09 第三方 Widget SDK（manifest + 沙箱消息白名单 + 生命周期失败计数）。
 *
 * widget-manifest.json：
 * { format:"widget", version:1, id, name, size:"1x1"|"2x1"|"2x2", permissions:[], refresh:秒 }
 *
 * 沙箱模型（诚实边界）：
 * - iframe sandbox="allow-scripts"，不给 allow-same-origin → 唯一 opaque origin，
 *   无法读宿主 localStorage/DOM；网络层面 iframe 与宿主同 CSP，禁网由宿主 CSP
 *   connect-src 策略兜底（Tauri 配置归集成阶段，工坊 UI 已注明）。
 * - postMessage 白名单：入站仅 {type:"widget:refresh"}（心跳）；出站按权限
 *   放行 widget:tick / widget:todos / widget:net 三种只读消息，其余直接拒绝。
 */
import type { WgtSize } from "./layout";

export const WIDGET_FORMAT = "widget";
export const WIDGET_VERSION = 1;

export type WidgetScope = "refresh" | "clock" | "todo:read" | "net:status";

export const WIDGET_SCOPES: WidgetScope[] = ["refresh", "clock", "todo:read", "net:status"];

export interface WidgetManifest {
  format: "widget";
  version: 1;
  id: string;
  name: string;
  size: WgtSize;
  permissions: WidgetScope[];
  /** 刷新秒数（下限 5s，避免第三方刷爆主线程）。 */
  refresh: number;
}

export const WIDGET_REFRESH_MIN = 5;
export const WIDGET_HTML_MAX_BYTES = 256 * 1024;

const ID_RE = /^[a-z0-9][a-z0-9._-]{2,63}$/i;

export function hasPermission(manifest: WidgetManifest, scope: WidgetScope): boolean {
  return manifest.permissions.includes(scope);
}

export function validateManifest(raw: unknown): { ok: boolean; errors: string[]; data: WidgetManifest | null } {
  const errors: string[] = [];
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    return { ok: false, errors: ["manifest 不是 JSON 对象"], data: null };
  }
  const r = raw as Record<string, unknown>;
  if (r.format !== WIDGET_FORMAT) errors.push("format 须为 widget");
  if (r.version !== WIDGET_VERSION) errors.push("version 须为 1");
  if (typeof r.id !== "string" || !ID_RE.test(r.id)) errors.push("id 须为 3..64 位字母数字._-");
  if (typeof r.name !== "string" || r.name.trim().length === 0 || r.name.length > 40) errors.push("name 须为 1..40 字符");
  if (r.size !== "1x1" && r.size !== "2x1" && r.size !== "2x2") errors.push("size 须为 1x1 / 2x1 / 2x2");
  if (!Array.isArray(r.permissions) || r.permissions.some((p) => !WIDGET_SCOPES.includes(p as WidgetScope))) {
    errors.push(`permissions 只能是 ${WIDGET_SCOPES.join(" / ")}`);
  }
  const refresh = Number(r.refresh);
  if (!Number.isFinite(refresh) || refresh < WIDGET_REFRESH_MIN) {
    errors.push(`refresh 须为 ≥${WIDGET_REFRESH_MIN} 的秒数`);
  }
  if (errors.length > 0) return { ok: false, errors, data: null };
  return {
    ok: true,
    errors,
    data: {
      format: WIDGET_FORMAT,
      version: WIDGET_VERSION,
      id: String(r.id),
      name: String(r.name).trim(),
      size: r.size as WgtSize,
      permissions: (r.permissions as WidgetScope[]).slice(),
      refresh: Math.max(WIDGET_REFRESH_MIN, Math.floor(refresh)),
    },
  };
}

// ---------- 消息白名单 ----------

export type WidgetInboundMessage = { type: "widget:refresh" };

export type WidgetOutboundMessage =
  | { type: "widget:tick"; time: string }
  | { type: "widget:todos"; items: { id: number; text: string; done: boolean }[] }
  | { type: "widget:net"; online: boolean };

/** 入站白名单：仅 widget:refresh 心跳；其余一律拒绝。 */
export function isInboundAllowed(msg: unknown): msg is WidgetInboundMessage {
  return (
    typeof msg === "object" && msg !== null &&
    (msg as Record<string, unknown>).type === "widget:refresh"
  );
}

/** 出站白名单：按 manifest 权限决定消息能否发送（无权限直接拒绝）。 */
export function canSendOutbound(manifest: WidgetManifest, msg: WidgetOutboundMessage): boolean {
  switch (msg.type) {
    case "widget:tick":
      return hasPermission(manifest, "clock");
    case "widget:todos":
      return hasPermission(manifest, "todo:read");
    case "widget:net":
      return hasPermission(manifest, "net:status");
    default:
      return false;
  }
}

// ---------- 生命周期：连续失败 3 次自动收起（纯函数，Board 持有状态） ----------

export const WIDGET_FAILURE_LIMIT = 3;

export interface WidgetFailureDoc {
  counts: Record<string, number>;
}

export function bumpFailure(doc: WidgetFailureDoc, id: string): WidgetFailureDoc {
  return { counts: { ...doc.counts, [id]: (doc.counts[id] ?? 0) + 1 } };
}

export function resetFailures(doc: WidgetFailureDoc, id: string): WidgetFailureDoc {
  if (doc.counts[id] === undefined) return doc;
  const next = { ...doc.counts };
  delete next[id];
  return { counts: next };
}

export function shouldCollapse(doc: WidgetFailureDoc, id: string): boolean {
  return (doc.counts[id] ?? 0) >= WIDGET_FAILURE_LIMIT;
}