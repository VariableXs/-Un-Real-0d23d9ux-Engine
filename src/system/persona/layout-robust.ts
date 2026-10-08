/**
 * 七章 布局鲁棒性 · 超小/超大字号不截断 + 弹性布局断点模型。
 *
 * 主册判据延伸：
 * - 「最小尺寸内不破版，超小/超大字号（用户系统字体缩放）下文字不截断」
 *   ——文本溢出判定（宽度预算 vs 文本度量估算）、弹性换行计算、
 *   窗口断点三档（compact/standard/expansive）；
 * - 判定全部纯函数（布局库无关——CI 可跑的布局回归）。
 */

// ---------- 文本度量估算（字符宽系数——CJK 全角 1.0 / Latin 0.55） ----------

export function estimateTextWidth(text: string, fontPx: number): number {
  let units = 0;
  for (const ch of text) {
    units += /[\u2e80-\u9fff\uf900-\ufaff\uff00-\uffef]/.test(ch) ? 1 : 0.55;
  }
  return Math.ceil(units * fontPx);
}

export interface FitVerdict {
  fits: boolean;
  /** 需要的宽度（含 2px 余量）。 */
  neededPx: number;
  /** 建议动作：放行 / 缩字号（下限 12px）/ 截断+tooltip / 换行。 */
  action: "fit" | "shrink-font" | "truncate-tooltip" | "wrap";
}

/** 文本适配判定（字号缩放 0.8×~2× 下逐一过——七章超小超大字号判据）。 */
export function fitText(text: string, fontPx: number, budgetPx: number, allowWrap: boolean): FitVerdict {
  const neededPx = estimateTextWidth(text, fontPx) + 2;
  if (neededPx <= budgetPx) return { fits: true, neededPx, action: "fit" };
  // 降字号：12px 下限（可读性底线）。
  const minFont = 12;
  if (fontPx > minFont && estimateTextWidth(text, minFont) + 2 <= budgetPx) {
    return { fits: true, neededPx, action: "shrink-font" };
  }
  if (allowWrap) return { fits: true, neededPx, action: "wrap" };
  return { fits: false, neededPx, action: "truncate-tooltip" }; // 截断必须配 tooltip（信息不丢）。
}

// ---------- 弹性布局断点（窗口三档——F214 最小尺寸联动） ----------

export type WindowTier = "compact" | "standard" | "expansive";

export function windowTier(widthPx: number): WindowTier {
  if (widthPx < 720) return "compact";
  if (widthPx < 1200) return "standard";
  return "expansive";
}

/** 断点适配：compact 收侧栏、standard 全功能、expansive 多列（层级递进不淹没）。 */
export function layoutForTier(tier: WindowTier): { columns: number; sidePanel: boolean; density: "comfortable" | "cozy" | "dense" } {
  switch (tier) {
    case "compact":
      return { columns: 1, sidePanel: false, density: "comfortable" };
    case "standard":
      return { columns: 2, sidePanel: true, density: "cozy" };
    case "expansive":
      return { columns: 3, sidePanel: true, density: "dense" };
  }
}

/** 字号缩放 × 窗口断点联合判定（用户 200% 字体 + 最小窗口 = 最恶劣组合必须成立）。 */
export function worstCaseCheck(text: string, baseFontPx: number, budgetPx: number, userScale: number): { ok: boolean; effectiveFontPx: number; action: FitVerdict["action"] } {
  const effectiveFontPx = Math.round(baseFontPx * userScale);
  const v = fitText(text, effectiveFontPx, budgetPx, true); // 正文允许换行——截断只给单行控件。
  return { ok: v.fits, effectiveFontPx, action: v.action };
}
