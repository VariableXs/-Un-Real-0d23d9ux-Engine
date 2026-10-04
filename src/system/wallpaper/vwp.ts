/**
 * N-07 壁纸工坊 · .vwp 壁纸包格式（v1）。
 *
 * 规格（功能全景 L816 + 本车道实现口径）：
 * - .vwp 是 JSON 单文件（未用 zip：免新增依赖，边界已在计划中注明）；
 * - 结构 { format:"vwp", version:1, kind, manifest, uniforms?, resources }；
 * - resources 只允许 data URL（≤2MB/项）；远程 URL 一律拒绝（隐私红线，L818）；
 * - kind 仅 shader | web | generative（video 体积不可能进包，格式上直接排除）。
 */

export const VWP_FORMAT = "vwp";
export const VWP_VERSION = 1;
/** 单个资源 data URL 的体积上限（解码前按 base64 长度估算）。 */
export const VWP_MAX_RESOURCE_BYTES = 2 * 1024 * 1024;

export type VwpKind = "shader" | "web" | "generative";

export interface VwpManifest {
  name: string;
  author?: string;
  description?: string;
  /** generative 预设 id（如 deep-space / calm-night）。 */
  preset?: string;
  /** generative 预设参数（星空引擎 motion/parallax/dprScale 等）。 */
  params?: Record<string, number>;
  /** 恒为 false：vwp 不允许任何网络能力（网页包同样仅本地）。 */
  netAccess?: false;
}

export interface VwpPack {
  format: typeof VWP_FORMAT;
  version: typeof VWP_VERSION;
  kind: VwpKind;
  manifest: VwpManifest;
  /** shader 标量 uniform 快照（滑杆当前值）。 */
  uniforms?: Record<string, number>;
  /** 资源表：文件名 → dataURL（text/plain 的 shader 源码、text/html 的入口页、image/* 缩略图）。 */
  resources: Record<string, string>;
}

export type VwpError =
  | "format"
  | "version"
  | "kind"
  | "manifest"
  | "uniforms"
  | "resource-url"
  | "resource-size"
  | "resource-type";

export type VwpValidation = { ok: true; pack: VwpPack } | { ok: false; errors: VwpError[] };

const KINDS: readonly VwpKind[] = ["shader", "web", "generative"];
/** data URL 白名单前缀（base64）。 */
const DATA_URL_RE = /^data:(text\/plain|text\/html|image\/png|image\/jpeg|image\/webp);base64,/;

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function dataUrlBytes(url: string): number {
  const comma = url.indexOf(",");
  if (comma < 0) return 0;
  return Math.floor(((url.length - comma - 1) * 3) / 4);
}

function validateUniforms(v: unknown): boolean {
  if (v === undefined) return true;
  if (!isRecord(v)) return false;
  return Object.values(v).every((n) => typeof n === "number" && Number.isFinite(n));
}

/**
 * 校验 .vwp 包。红线用例（有单测）：
 * - 远程 URL（http/https/file/协议相对）→ resource-url 拒绝；
 * - 超过 2MB 的资源 → resource-size；
 * - 非 data: 前缀 → resource-url。
 */
export function validateVwp(raw: unknown): VwpValidation {
  const errors: VwpError[] = [];
  if (!isRecord(raw)) return { ok: false, errors: ["format"] };
  if (raw.format !== VWP_FORMAT) errors.push("format");
  if (raw.version !== VWP_VERSION) errors.push("version");
  if (typeof raw.kind !== "string" || !KINDS.includes(raw.kind as VwpKind)) errors.push("kind");

  const m = raw.manifest;
  if (!isRecord(m) || typeof m.name !== "string" || m.name.trim() === "") {
    errors.push("manifest");
  }
  if (!validateUniforms(raw.uniforms)) errors.push("uniforms");

  if (!isRecord(raw.resources)) {
    errors.push("resource-url");
  } else {
    for (const [, url] of Object.entries(raw.resources)) {
      if (typeof url !== "string") {
        errors.push("resource-url");
        continue;
      }
      // 隐私红线：任何远程/本地路径引用都拒绝（L818「manifest 里出现远程 URL 直接拒绝加载并说明原因」）。
      if (/^(https?:|file:|ftp:|blob:|\/\/)/i.test(url)) {
        errors.push("resource-url");
        continue;
      }
      if (!DATA_URL_RE.test(url)) {
        errors.push("resource-type");
        continue;
      }
      if (dataUrlBytes(url) > VWP_MAX_RESOURCE_BYTES) errors.push("resource-size");
    }
  }

  if (errors.length > 0) return { ok: false, errors };
  return {
    ok: true,
    pack: {
      format: VWP_FORMAT,
      version: VWP_VERSION,
      kind: raw.kind as VwpKind,
      manifest: m as VwpManifest,
      uniforms: raw.uniforms === undefined ? undefined : (raw.uniforms as Record<string, number>),
      resources: raw.resources as Record<string, string>,
    },
  };
}

/** 组包（导出侧；不校验体积，导出前由调用方用 validateVwp 兜底）。 */
export function buildVwp(input: {
  kind: VwpKind;
  manifest: VwpManifest;
  uniforms?: Record<string, number>;
  resources?: Record<string, string>;
}): VwpPack {
  return {
    format: VWP_FORMAT,
    version: VWP_VERSION,
    kind: input.kind,
    manifest: input.manifest,
    uniforms: input.uniforms,
    resources: input.resources ?? {},
  };
}

/** 文本 → base64 data URL（小文本：shader 源码 / html 入口）。 */
export function textToDataUrl(text: string, mime: "text/plain" | "text/html"): string {
  // 纯浏览器实现（btoa + TextEncoder），不引入 Node Buffer（vite 前端域）。
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return `data:${mime};base64,${btoa(bin)}`;
}

/** data URL → 文本（导入侧；非文本类型返回 null）。 */
export function dataUrlToText(url: string): string | null {
  const m = /^data:(text\/plain|text\/html);base64,(.+)$/.exec(url);
  if (!m) return null;
  try {
    const bin = atob(m[2] as string);
    const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return null;
  }
}

/** 触发浏览器下载（a[download]；非 DOM 环境静默跳过）。 */
export function downloadVwp(pack: VwpPack, filename: string): boolean {
  if (typeof document === "undefined") return false;
  const blob = new Blob([JSON.stringify(pack, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename.endsWith(".vwp") ? filename : `${filename}.vwp`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  return true;
}

/** 解析 .vwp 文本（JSON.parse → validateVwp）。 */
export function parseVwpText(text: string): VwpValidation {
  let raw: unknown;
  try {
    raw = JSON.parse(text) as unknown;
  } catch {
    return { ok: false, errors: ["format"] };
  }
  return validateVwp(raw);
}