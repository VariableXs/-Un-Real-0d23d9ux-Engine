/**
 * U-02 VARIABLE 字形库（手绘单路径 SVG 轮廓）。
 *
 * 设计语言：航空仪表 × 精细制表 —— 几何骨架、统一笔画（视觉 ≈14/120 字高）、
 * 开放字怀（open apertures）、锐利端点。同一 path 双形态：
 * - fill（fill-rule: evenodd，实心字）→ ready 后实心化 / 12% 底衬
 * - stroke（fill: none，描边字）→ 逐字母按真实进度描边（dasharray = length）
 *
 * 长度常量为校准值（calibrated constants）：按路径几何近似测量取整，
 * jsdom/node 无 SVGPathElement.getTotalLength，测试锁定常量表而非运行时测量。
 * 描边动画只影响视觉，进度本身 100% 来自后端真实事件。
 */

export interface WordmarkGlyph {
  /** 单路径轮廓（含 counter 子路径，evenodd 填充出孔洞）。 */
  path: string;
  /** 校准长度常量（viewBox 单位）：dasharray/dashoffset 用，测试锁定。 */
  length: number;
  /** 统一画布：字高 120，字宽 100，侧边距 ~15。 */
  viewBox: string;
}

export const WORDMARK_LETTERS = ["V", "A", "R", "I", "A", "B", "L", "E"] as const;

export const GLYPHS: Record<string, WordmarkGlyph> = {
  // V：外缘下探至尖底 (50,105)，内缘交汇 (50,60)，双臂水平笔画宽 15。
  V: {
    path: "M20 15 L50 105 L80 15 H65 L50 60 H35 Z",
    length: 315,
    viewBox: "0 0 100 120",
  },
  // A：外三角 (15,105)-(50,15)-(85,105)，内顶 (50,53.6)，横梁 y=70..84，字脚下开口。
  A: {
    path: "M15 105 L50 15 L85 105 H70 L61.8 84 L38.2 84 L30 105 Z M50 53.6 L43.6 70 L56.4 70 Z",
    length: 340,
    viewBox: "0 0 100 120",
  },
  // R：竖干 20..34，碗形半圆弧 r=23.5 收腰 y=62，斜腿出锋 (83,105)，counter r=9.5。
  R: {
    path: "M20 15 H56 A23.5 23.5 0 0 1 56 62 H60 L83 105 H68 L45 62 H34 V105 H20 Z M34 29 H55 A9.5 9.5 0 0 1 55 48 H34 Z",
    length: 475,
    viewBox: "0 0 100 120",
  },
  // I：无衬线纯竖干 43..57（仪表刻度式的极简）。
  I: {
    path: "M43 15 H57 V105 H43 Z",
    length: 208,
    viewBox: "0 0 100 120",
  },
  // B：上碗 r=22 / 下碗 r=23（下大上小），双 counter，腰部 y=59 平接。
  B: {
    path: "M20 15 H52 A22 22 0 0 1 52 59 H58 A23 23 0 0 1 58 105 H20 Z M34 29 H51 A8 8 0 0 1 51 45 H34 Z M34 73 H57 A9 9 0 0 1 57 91 H34 Z",
    length: 475,
    viewBox: "0 0 100 120",
  },
  // L：竖干 + 底部横臂至 x=82，臂厚 14。
  L: {
    path: "M20 15 H34 V91 H82 V105 H20 Z",
    length: 237,
    viewBox: "0 0 100 120",
  },
  // E：三横臂（上/下至 82，中臂收短至 74），笔画统一 14。
  E: {
    path: "M20 15 H82 V29 H34 V53 H74 V67 H34 V91 H82 V105 H20 Z",
    length: 480,
    viewBox: "0 0 100 120",
  },
};
