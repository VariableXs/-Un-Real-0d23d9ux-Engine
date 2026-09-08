/**
 * N-08 `.vtheme` 分享格式（JSON，非 zip —— 与规格差异已注明）：
 * { format:"vtheme", version:1, name, variants:{light?,dark?,hc?}, thumbnail? }
 * 变体 = { colors: Record<tokenKey, hex>, shape:{radius,density,fontScale} }。
 * 导入校验 + 冲突字段报告（未知 token/变体键报告为 conflict，不阻断加载）。
 */

import {
  COLOR_TOKENS, DENSITY_KINDS, FONT_SCALE_KINDS, RADIUS_KINDS,
  type DensityKind, type FontScaleKind, type RadiusKind, type VariantKind,
} from "./tokens";

export const VTHEME_FORMAT = "vtheme";
export const VTHEME_VERSION = 1;

export interface VthemeVariant {
  colors: Record<string, string>;
  shape: { radius: RadiusKind; density: DensityKind; fontScale: FontScaleKind };
}

export interface VthemeFile {
  format: "vtheme";
  version: 1;
  name: string;
  variants: Partial<Record<VariantKind, VthemeVariant>>;
  /** 可选预览缩略图（dataURL；工坊导出生成 SVG 摘要，非截图）。 */
  thumbnail?: string;
}

export type VthemeIssueLevel = "error" | "conflict" | "warning";

export interface VthemeIssue {
  field: string;
  message: string;
  level: VthemeIssueLevel;
}

export interface VthemeParseResult {
  ok: boolean;
  issues: VthemeIssue[];
  data: VthemeFile | null;
}

const TOKEN_KEYS = new Set(COLOR_TOKENS.map((t) => t.key));
const VARIANT_KINDS = new Set<VariantKind>(["light", "dark", "hc"]);
const HEX_RE = /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/;

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function validateShape(raw: unknown, issues: VthemeIssue[], field: string): VthemeVariant["shape"] | null {
  if (!isRecord(raw)) {
    issues.push({ field, message: "shape 缺失或不是对象", level: "error" });
    return null;
  }
  let ok = true;
  const { radius, density, fontScale } = raw as Record<string, unknown>;
  if (typeof radius !== "string" || !RADIUS_KINDS.includes(radius as RadiusKind)) {
    issues.push({ field: `${field}.radius`, message: `圆角档非法: ${String(radius)}`, level: "error" });
    ok = false;
  }
  if (typeof density !== "string" || !DENSITY_KINDS.includes(density as DensityKind)) {
    issues.push({ field: `${field}.density`, message: `密度档非法: ${String(density)}`, level: "error" });
    ok = false;
  }
  if (typeof fontScale !== "string" || !FONT_SCALE_KINDS.includes(fontScale as FontScaleKind)) {
    issues.push({ field: `${field}.fontScale`, message: `字阶档非法: ${String(fontScale)}`, level: "error" });
    ok = false;
  }
  return ok ? { radius: radius as RadiusKind, density: density as DensityKind, fontScale: fontScale as FontScaleKind } : null;
}

function validateVariant(raw: unknown, issues: VthemeIssue[], field: string): VthemeVariant | null {
  if (!isRecord(raw)) {
    issues.push({ field, message: "变体缺失或不是对象", level: "error" });
    return null;
  }
  let ok = true;
  const colors: Record<string, string> = {};
  const rawColors = (raw as Record<string, unknown>).colors;
  if (!isRecord(rawColors)) {
    issues.push({ field: `${field}.colors`, message: "colors 缺失或不是对象", level: "error" });
    ok = false;
  } else {
    for (const [k, v] of Object.entries(rawColors)) {
      if (!TOKEN_KEYS.has(k)) {
        // 冲突字段：未知 token（多半来自更新版本的工坊）——报告但不阻断。
        issues.push({ field: `${field}.colors.${k}`, message: `未知语义 token，导入时忽略`, level: "conflict" });
        continue;
      }
      if (typeof v !== "string" || !HEX_RE.test(v)) {
        issues.push({ field: `${field}.colors.${k}`, message: `颜色值须为 #rrggbb 或 #rrggbbaa`, level: "error" });
        ok = false;
        continue;
      }
      colors[k] = v;
    }
  }
  const shape = validateShape((raw as Record<string, unknown>).shape, issues, `${field}.shape`);
  if (!ok || !shape) return null;
  return { colors, shape };
}

/** 纯校验：格式/版本/名称/变体/颜色/形档 + 冲突字段报告。 */
export function validateVtheme(raw: unknown): VthemeParseResult {
  const issues: VthemeIssue[] = [];
  if (!isRecord(raw)) {
    return { ok: false, issues: [{ field: "", message: "不是 JSON 对象", level: "error" }], data: null };
  }
  if (raw.format !== VTHEME_FORMAT) {
    issues.push({ field: "format", message: `format 须为 "${VTHEME_FORMAT}"`, level: "error" });
  }
  if (raw.version !== VTHEME_VERSION) {
    issues.push({ field: "version", message: `version 须为 ${VTHEME_VERSION}`, level: "error" });
  }
  const name = raw.name;
  if (typeof name !== "string" || name.trim().length === 0 || name.length > 60) {
    issues.push({ field: "name", message: "name 须为 1..60 字符", level: "error" });
  }
  if (raw.thumbnail !== undefined && (typeof raw.thumbnail !== "string" || !raw.thumbnail.startsWith("data:image/"))) {
    issues.push({ field: "thumbnail", message: "thumbnail 须为 data:image/* dataURL（已忽略）", level: "warning" });
  }
  const variants: Partial<Record<VariantKind, VthemeVariant>> = {};
  const rawVariants = raw.variants;
  if (!isRecord(rawVariants) || Object.keys(rawVariants).length === 0) {
    issues.push({ field: "variants", message: "variants 须为非空对象", level: "error" });
  } else {
    for (const [k, v] of Object.entries(rawVariants)) {
      if (!VARIANT_KINDS.has(k as VariantKind)) {
        issues.push({ field: `variants.${k}`, message: `未知变体键（支持 light/dark/hc），导入时忽略`, level: "conflict" });
        continue;
      }
      const parsed = validateVariant(v, issues, `variants.${k}`);
      if (parsed) variants[k as VariantKind] = parsed;
    }
  }
  const ok = !issues.some((i) => i.level === "error") && Object.keys(variants).length > 0;
  return {
    ok,
    issues,
    data: ok
      ? {
          format: VTHEME_FORMAT,
          version: VTHEME_VERSION,
          name: typeof name === "string" ? name.trim() : "",
          variants,
          thumbnail: typeof raw.thumbnail === "string" ? raw.thumbnail : undefined,
        }
      : null,
  };
}

export function parseVtheme(jsonText: string): VthemeParseResult {
  let raw: unknown;
  try {
    raw = JSON.parse(jsonText) as unknown;
  } catch (e) {
    return { ok: false, issues: [{ field: "", message: `JSON 解析失败: ${String(e)}`, level: "error" }], data: null };
  }
  return validateVtheme(raw);
}

/** 导出（含可选缩略图）。 */
export function serializeVtheme(file: VthemeFile): string {
  return JSON.stringify(file, null, 2);
}

/** 导出用 SVG 摘要缩略图（诚实边界：是程序生成的色板摘要，非桌面截图）。 */
export function buildThumbnail(variant: VthemeVariant, name: string): string {
  const c = variant.colors;
  const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="320" height="200" viewBox="0 0 320 200">` +
    `<rect width="320" height="200" fill="${esc(c["--bg-canvas"] ?? "#000000")}"/>` +
    `<rect x="20" y="20" width="280" height="120" rx="12" fill="${esc(c["--bg-surface"] ?? "#222222")}"/>` +
    `<rect x="36" y="40" width="160" height="12" rx="6" fill="${esc(c["--text-primary"] ?? "#eeeeee")}"/>` +
    `<rect x="36" y="62" width="120" height="8" rx="4" fill="${esc(c["--text-secondary"] ?? "#999999")}"/>` +
    `<rect x="36" y="88" width="88" height="28" rx="8" fill="${esc(c["--accent"] ?? "#4466cc")}"/>` +
    `<rect x="0" y="164" width="320" height="36" fill="${esc(c["--bg-raised"] ?? "#1a1a1a")}"/>` +
    `<text x="20" y="194" font-size="12" fill="${esc(c["--text-secondary"] ?? "#999999")}">${esc(name)}</text>` +
    `</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}
