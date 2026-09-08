/**
 * N-07 Shader 引擎 uniform 声明解析器（纯函数）。
 *
 * 约定：用户在 .frag/.glsl 源码里用行尾注释声明滑杆范围与默认值：
 *   uniform float uSpeed;      // 0..2
 *   uniform float uBright;     // 0..1 default 0.8
 * 未写范围的 uniform 按 0..1 / 默认中点处理；float/int 之外的类型
 * （vec2/vec3/sampler2D…）不做滑杆（工坊只暴露标量参数）。
 */

export interface ParsedUniform {
  name: string;
  type: "float" | "int";
  min: number;
  max: number;
  value: number;
}

const UNIFORM_RE = /^[ \t]*uniform[ \t]+(float|int)[ \t]+([A-Za-z_]\w*)[ \t]*;[ \t]*(?:\/\/[ \t]*(.*))?$/;
const RANGE_RE = /(-?\d+(?:\.\d+)?)\s*\.\.\s*(-?\d+(?:\.\d+)?)/;
const DEFAULT_RE = /default\s*[:=]?\s*(-?\d+(?:\.\d+)?)/i;

function toNum(s: string | undefined, fallback: number): number {
  const n = s === undefined ? NaN : Number(s);
  return Number.isFinite(n) ? n : fallback;
}

/** 解析 shader 源码中的标量 uniform 滑杆声明；同名以首次出现为准，按出现顺序返回。 */
export function parseUniforms(src: string): ParsedUniform[] {
  const out: ParsedUniform[] = [];
  const seen = new Set<string>();
  for (const line of src.split(/\r?\n/)) {
    const m = UNIFORM_RE.exec(line);
    if (!m) continue;
    const type = m[1] === "int" ? "int" : "float";
    const name = m[2] ?? "";
    if (!name || seen.has(name)) continue;
    const comment = m[3] ?? "";
    const range = RANGE_RE.exec(comment);
    const def = DEFAULT_RE.exec(comment);
    const min = toNum(range?.[1], 0);
    const max = toNum(range?.[2], 1);
    const lo = Math.min(min, max);
    const hi = Math.max(min, max);
    const mid = lo + (hi - lo) / 2;
    const value = Math.min(hi, Math.max(lo, toNum(def?.[1], mid)));
    seen.add(name);
    out.push({ name, type, min: lo, max: hi, value });
  }
  return out;
}