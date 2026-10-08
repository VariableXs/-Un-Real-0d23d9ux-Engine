/**
 * AI-14 Z-52 插件运行时 2.0 —— 插件清单（plugin.json）校验。
 *
 * 红线：未声明权限一律拒绝；engine 区间不匹配提示升级；
 * entry 仅允许相对路径（防绝对路径/协议逃逸）；id 规范 [a-z0-9-]。
 */

export interface PluginManifest {
  id: string;
  name: string;
  version: string;
  /** 引擎兼容区间，如 ">=1.5 <2" */
  engine?: string;
  /** 声明的权限：ui.notify / fs.read / fs.write / net.loopback / events.read */
  permissions: string[];
  /** 入口（相对插件根目录） */
  entry: string;
}

export const PLUGIN_KNOWN_PERMISSIONS = [
  "ui.notify",
  "fs.read",
  "fs.write",
  "net.loopback",
  "events.read",
] as const;

export type PluginCheckLevel = "error" | "warning";

export interface PluginCheckItem {
  level: PluginCheckLevel;
  message: string;
}

export interface PluginCheckResult {
  ok: boolean;
  items: PluginCheckItem[];
}

const ID_RE = /^[a-z0-9][a-z0-9-]{1,62}$/;
const SEMVER_RE = /^\d+\.\d+\.\d+$/;
const ENGINE_RE = /^(>=\s*\d+\.\d+(\.\d+)?\s*)?(<\s*\d+(\.\d+(\.\d+)?)?\s*)?$/;

function coerceManifest(raw: unknown): PluginManifest | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  if (typeof r.id !== "string" || typeof r.name !== "string" || typeof r.version !== "string") return null;
  const perms = Array.isArray(r.permissions) ? r.permissions.filter((p): p is string => typeof p === "string") : [];
  return {
    id: r.id,
    name: r.name,
    version: r.version,
    engine: typeof r.engine === "string" ? r.engine : undefined,
    permissions: perms,
    entry: typeof r.entry === "string" ? r.entry : "",
  };
}

/** 插件清单校验：schema → id 规范 → 权限白名单 → engine 区间 → entry 相对路径。 */
export function validatePluginManifest(raw: unknown): PluginCheckResult {
  const items: PluginCheckItem[] = [];
  const m = coerceManifest(raw);
  if (!m) return { ok: false, items: [{ level: "error", message: "清单缺少 id/name/version 基本字段" }] };
  if (!ID_RE.test(m.id)) items.push({ level: "error", message: `id 需为 [a-z0-9-] 且 2~63 字符: ${m.id}` });
  if (!SEMVER_RE.test(m.version)) items.push({ level: "error", message: `version 需为 semver（x.y.z）: ${m.version}` });
  for (const p of m.permissions) {
    if (!(PLUGIN_KNOWN_PERMISSIONS as readonly string[]).includes(p)) {
      items.push({ level: "error", message: `未知权限（红线：未声明一律拒绝）: ${p}` });
    }
  }
  if (m.permissions.length === 0) {
    items.push({ level: "warning", message: "未声明任何权限——插件将只能使用零能力沙箱" });
  }
  if (m.engine !== undefined && !ENGINE_RE.test(m.engine.trim())) {
    items.push({ level: "error", message: `engine 区间语法非法: ${m.engine}` });
  }
  if (!m.entry) {
    items.push({ level: "error", message: "缺少 entry 入口" });
  } else if (/^([a-uw-zA-UW-Z]:|\\\\|\/|file:|https?:)/.test(m.entry)) {
    items.push({ level: "error", message: `entry 必须为插件根目录内的相对路径: ${m.entry}` });
  } else if (m.entry.includes("..")) {
    items.push({ level: "error", message: "entry 不得包含 ..（防目录逃逸）" });
  }
  return { ok: !items.some((i) => i.level === "error"), items };
}
