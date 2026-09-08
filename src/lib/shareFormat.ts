/**
 * AI-14 Z-54 布局与配置分享格式（Config Share Format）。
 *
 * 统一信封：{ kind, version, payload, checksum }
 * - checksum：FNV-1a 32 位（十六进制），覆盖 kind+version+payload 规范化 JSON；
 * - 导入流程：schema 校验 → checksum 校验 → 内容摘要预览（覆盖计数）→ 用户确认。
 * - 与 .vxs（U-39）边界：vxs 是资源包容器，本项是配置片段轻量交换格式。
 */

export type ShareKind = "keymap" | "layout" | "theme";

export interface ShareEnvelope<T = unknown> {
  kind: ShareKind;
  version: 1;
  payload: T;
  checksum: string;
}

export function fnv1a(input: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    h ^= input.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, "0");
}

function stableStringify(v: unknown): string {
  return JSON.stringify(v, (_k, val) =>
    val && typeof val === "object" && !Array.isArray(val)
      ? Object.fromEntries(Object.entries(val as Record<string, unknown>).sort(([a], [b]) => (a < b ? -1 : 1)))
      : val,
  );
}

export function sealEnvelope<T>(kind: ShareKind, payload: T): ShareEnvelope<T> {
  const checksum = fnv1a(stableStringify({ kind, version: 1, payload }));
  return { kind, version: 1, payload, checksum };
}

export type OpenResult<T> =
  | { ok: true; envelope: ShareEnvelope<T>; summary: string }
  | { ok: false; error: string };

/** 导入校验：schema → checksum → 摘要。坏 checksum 被拒并提示重新分享。 */
export function openEnvelope<T>(raw: string, coverageOf?: (payload: T) => number): OpenResult<T> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return { ok: false, error: "不是有效的 JSON" };
  }
  const env = parsed as ShareEnvelope<T>;
  if (!env || typeof env !== "object" || typeof env.kind !== "string" || env.version !== 1 || env.payload === undefined) {
    return { ok: false, error: "信封格式不符合 schema（需 kind/version/payload/checksum）" };
  }
  if (env.kind !== "keymap" && env.kind !== "layout" && env.kind !== "theme") {
    return { ok: false, error: `未知分享类型: ${String(env.kind)}` };
  }
  const expect = fnv1a(stableStringify({ kind: env.kind, version: 1, payload: env.payload }));
  if (expect !== env.checksum) {
    return { ok: false, error: "校验和不匹配——文件可能被篡改或截断，请向分享者重新获取" };
  }
  const covered = coverageOf ? coverageOf(env.payload) : 0;
  const summary = `${env.kind} · 校验通过 · ${covered > 0 ? `包含 ${covered} 项` : "内容有效"}`;
  return { ok: true, envelope: env, summary };
}
