/**
 * AI-20 质量门禁与收官组 — M-88 全局 Tooltip 规范（Tooltip Standard）。
 *
 * 三统一（终结「三种样式三种延迟」）：
 * 1. 样式令牌化：`--tooltip-*`（tokens.css），样式统一 `.vx-tip`（tooltip.css）；
 * 2. 出现延迟统一：引用 AI-18 M-71 的 `--hover-delay` 档位（200/400/600ms），
 *    禁止各处自定 transition-delay；
 * 3. 快捷键提示后缀统一圆括号式：`复制 (Ctrl+C)`（本模块 tooltipLabel 生成）。
 *
 * 超长截断：文本 >32 个 CJK 字符（或 >48 latin）截断加 …，完整内容放
 * data-tip-full（aria-describedby / title 兜底读取）。
 */

/** 快捷键后缀格式：圆括号式（M-88 唯一口径）。 */
export function tooltipLabel(text: string, accel?: string | null): string {
  const t = text.trim();
  if (!accel || !accel.trim()) return t;
  return `${t} (${accel.trim()})`;
}

/** 截断阈值（CJK 字符按 1 计，宽字符友好）。 */
export const TOOLTIP_TRUNCATE_CJK = 32;
export const TOOLTIP_TRUNCATE_LATIN = 48;

function visualLength(s: string): number {
  let n = 0;
  for (const ch of s) {
    n += ch.codePointAt(0)! > 0x2e7f ? 1 : 0.5;
  }
  return n;
}

/** 超长截断（保持词边界优先；返回 [显示文本, 是否截断]）。 */
export function truncateTooltip(text: string): [string, boolean] {
  const max = visualLength(text) > text.length * 0.75 ? TOOLTIP_TRUNCATE_CJK : TOOLTIP_TRUNCATE_LATIN;
  if (visualLength(text) <= max) return [text, false];
  let out = "";
  let n = 0;
  for (const ch of text) {
    const w = ch.codePointAt(0)! > 0x2e7f ? 1 : 0.5;
    if (n + w > max - 1) break;
    out += ch;
    n += w;
  }
  return [`${out}…`, true];
}

/** 生成 tooltip 属性集（data-tip / data-tip-full / aria 约定）。 */
export function tipProps(text: string, accel?: string | null): {
  "data-tip": string;
  "data-tip-full"?: string;
  role?: "button" | "note";
} {
  const full = tooltipLabel(text, accel);
  const [display, truncated] = truncateTooltip(full);
  return truncated
    ? { "data-tip": display, "data-tip-full": full, role: "note" }
    : { "data-tip": display };
}
