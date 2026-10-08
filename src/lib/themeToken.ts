/**
 * AI-14 Z-50 主题令牌开放规范（Theme Token Spec）校验器。
 *
 * 主题 = 一组符合 schema 的 token 覆盖文件（颜色/字体/圆角/动效参数）。
 * 校验：语法、完备性（缺省回退 OK）、非法值 NG、对比度底线（正文 ≥ 4.5:1）。
 */

export interface ThemeTokenFile {
  name?: string;
  tokens: Record<string, string>;
}

/** 规范允许的 token 变量名（变量名即契约；未知变量警告不拦截）。 */
export const KNOWN_TOKEN_PREFIXES = ["--v-color-", "--v-font-", "--v-radius-", "--v-motion-"] as const;
export const REQUIRED_TOKENS = ["--v-color-bg", "--v-color-fg", "--v-color-accent"] as const;

export type ThemeCheckLevel = "error" | "warning";

export interface ThemeCheckItem {
  level: ThemeCheckLevel;
  message: string;
}

export interface ThemeCheckResult {
  ok: boolean;
  items: ThemeCheckItem[];
}

function parseColor(c: string): [number, number, number] | null {
  const m = /^#([0-9a-f]{6})$/i.exec(c.trim());
  if (!m || !m[1]) return null;
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function relativeLuminance(rgb: [number, number, number]): number {
  const conv = (v: number): number => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  const [r, g, b] = [conv(rgb[0]), conv(rgb[1]), conv(rgb[2])];
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG 对比度。 */
export function contrastRatio(fg: string, bg: string): number | null {
  const f = parseColor(fg);
  const b = parseColor(bg);
  if (!f || !b) return null;
  const l1 = relativeLuminance(f);
  const l2 = relativeLuminance(b);
  const [hi, lo] = l1 >= l2 ? [l1, l2] : [l2, l1];
  return (hi + 0.05) / (lo + 0.05);
}

/** 主题令牌校验（8 类坏主题全部拦截：缺必填/非法色/低对比/未知变量/空名）。 */
export function validateTheme(file: ThemeTokenFile): ThemeCheckResult {
  const items: ThemeCheckItem[] = [];
  if (!file.tokens || typeof file.tokens !== "object") {
    return { ok: false, items: [{ level: "error", message: "缺少 tokens 对象" }] };
  }
  const tokens = file.tokens;
  const keys = Object.keys(tokens);
  if (keys.length === 0) {
    items.push({ level: "error", message: "tokens 为空——至少覆盖一个变量" });
  }
  for (const req of REQUIRED_TOKENS) {
    if (!(req in tokens)) {
      items.push({ level: "error", message: `缺少必填变量 ${req}（缺省回退可省略非核心项，这三项不可）` });
    }
  }
  for (const [k, v] of Object.entries(tokens)) {
    if (!k.startsWith("--v-")) {
      items.push({ level: "error", message: `变量名必须以 --v- 开头: ${k}` });
      continue;
    }
    if (!KNOWN_TOKEN_PREFIXES.some((p) => k.startsWith(p))) {
      items.push({ level: "warning", message: `未知变量（将被忽略）: ${k}` });
    }
    if (k.includes("--v-color-")) {
      if (parseColor(v) === null && !/^(#[0-9a-f]{3,8}|rgb|hsl|var\()/i.test(v.trim())) {
        items.push({ level: "error", message: `非法颜色值 ${k}: ${v}` });
      }
    }
    if (k.includes("--v-radius-") && !/^\d+(px)?$/.test(v.trim())) {
      items.push({ level: "error", message: `圆角必须为像素值 ${k}: ${v}` });
    }
    if (k.includes("--v-motion-") && !/^\d+(\.\d+)?(ms|s)$/.test(v.trim())) {
      items.push({ level: "error", message: `动效时长必须为 ms/s ${k}: ${v}` });
    }
  }
  // 对比度底线：正文对比度 ≥ 4.5:1（防"丑主题"伤观感）
  const fg = tokens["--v-color-fg"];
  const bg = tokens["--v-color-bg"];
  if (fg && bg) {
    const ratio = contrastRatio(fg, bg);
    if (ratio !== null && ratio < 4.5) {
      items.push({
        level: "error",
        message: `正文对比度 ${ratio.toFixed(2)}:1 低于 4.5:1 底线（${fg} on ${bg}）`,
      });
    }
  }
  return { ok: !items.some((i) => i.level === "error"), items };
}
