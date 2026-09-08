/**
 * N-12 `.vicon` 图标包格式（JSON manifest，非 zip —— 与规格差异已在 UI 注明，
 * 未用 zip 免新增依赖）：{ format:"vicon", version:1, name, icons: Record<key, 资源> }。
 * 资源 = 内联 SVG 字符串（以 "<svg" 开头）或 PNG dataURL（data:image/*）。
 * 导入校验：大小 ≤ 5MB、键名规范（区域:名称，如 desktop:sys-recycle / file:.md）、
 * 资源形态合法；SVG 进入 DOM 前经 scrubSvg 去除脚本/事件属性。
 */

export const VICON_FORMAT = "vicon";
export const VICON_VERSION = 1;
export const VICON_MAX_BYTES = 5 * 1024 * 1024;

/** 键规范：区域段(桌面/开始/文件/任务栏) + 名称；允许中文文件扩展名场景由消费方映射。 */
export const VICON_KEY_RE = /^[a-z0-9][a-z0-9_.-]*:[a-z0-9][a-z0-9_. -]*$/i;

export interface ViconFile {
  format: "vicon";
  version: 1;
  name: string;
  icons: Record<string, string>;
}

export interface ViconParseResult {
  ok: boolean;
  errors: string[];
  data: ViconFile | null;
}

function byteLength(s: string): number {
  return typeof TextEncoder !== "undefined" ? new TextEncoder().encode(s).length : s.length;
}

function isValidResource(v: unknown): v is string {
  if (typeof v !== "string" || v.length === 0 || v.length > 512 * 1024) return false;
  const s = v.trimStart();
  return s.startsWith("<svg") || s.startsWith("data:image/");
}

export function validateVicon(rawText: string): ViconParseResult {
  const errors: string[] = [];
  if (byteLength(rawText) > VICON_MAX_BYTES) {
    return { ok: false, errors: ["包体超过 5MB 上限"], data: null };
  }
  let raw: unknown;
  try {
    raw = JSON.parse(rawText) as unknown;
  } catch (e) {
    return { ok: false, errors: [`JSON 解析失败: ${String(e)}`], data: null };
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    return { ok: false, errors: ["不是 JSON 对象"], data: null };
  }
  const r = raw as Record<string, unknown>;
  if (r.format !== VICON_FORMAT) errors.push("format 须为 vicon");
  if (r.version !== VICON_VERSION) errors.push("version 须为 1");
  if (typeof r.name !== "string" || r.name.trim().length === 0 || r.name.length > 60) {
    errors.push("name 须为 1..60 字符");
  }
  const icons: Record<string, string> = {};
  if (typeof r.icons !== "object" || r.icons === null || Array.isArray(r.icons)) {
    errors.push("icons 须为对象");
  } else {
    const entries = Object.entries(r.icons as Record<string, unknown>);
    if (entries.length === 0) errors.push("icons 不能为空");
    if (entries.length > 4096) errors.push("icons 超过 4096 键上限");
    for (const [k, v] of entries) {
      if (!VICON_KEY_RE.test(k)) {
        errors.push(`键名不规范: ${k.slice(0, 40)}`);
        continue;
      }
      if (!isValidResource(v)) {
        errors.push(`资源须为内联 SVG 或 data:image/*: ${k.slice(0, 40)}`);
        continue;
      }
      icons[k] = v;
    }
  }
  if (errors.length > 0) return { ok: false, errors, data: null };
  return {
    ok: true,
    errors,
    data: { format: VICON_FORMAT, version: VICON_VERSION, name: String(r.name).trim(), icons },
  };
}

/**
 * SVG 清洗（进入 innerHTML 前的防线）：去 <script>/<foreignObject>、on* 事件属性、
 * javascript: href。诚实边界：正则清洗非完备解析器，纵深防御还需宿主 CSP。
 */
export function scrubSvg(svg: string): string {
  return svg
    .replace(/<script[\s\S]*?<\/script\s*>/gi, "")
    .replace(/<script[^>]*\/?>/gi, "")
    .replace(/<foreignObject[\s\S]*?<\/foreignObject\s*>/gi, "")
    .replace(/\son[a-z]+\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)/gi, "")
    .replace(/(href|xlink:href)\s*=\s*("javascript:[^"]*"|'javascript:[^']*'|javascript:[^\s>]+)/gi, "");
}

/** 资源 → 可渲染 payload（img src 或净化后的 svg 标记）。 */
export function iconResource(res: string): { kind: "svg" | "img"; value: string } {
  const s = res.trimStart();
  return s.startsWith("<svg") ? { kind: "svg", value: scrubSvg(s) } : { kind: "img", value: s };
}

/** 12 宫格预览取样键（覆盖桌面/开始/文件/任务栏四区域）。 */
export const PREVIEW_KEYS: string[] = [
  "desktop:sys-recycle", "desktop:sys-network", "desktop:sys-computer", "desktop:app-write",
  "start:app-mindmap", "start:app-project", "start:app-fate", "start:sys-settings",
  "file:.md", "file:.png", "taskbar:start", "taskbar:search",
];

/** 导出骨架：从当前生效包（或预览键模板）生成可再创作的空包。 */
export function exportSkeleton(currentIcons: Record<string, string> | null, name: string): ViconFile {
  const keys = currentIcons && Object.keys(currentIcons).length > 0 ? Object.keys(currentIcons) : PREVIEW_KEYS;
  const icons: Record<string, string> = {};
  for (const k of keys) icons[k] = "";
  return { format: VICON_FORMAT, version: VICON_VERSION, name: `${name} skeleton`, icons };
}