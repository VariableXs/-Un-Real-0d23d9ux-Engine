import { describe, expect, it } from "vitest";
import { GLYPHS, WORDMARK_LETTERS } from "./glyphs";

/**
 * U-02：jsdom/node 无 SVGPathElement.getTotalLength —— 本测试锁定「校准常量表」，
 * 而非运行时测量。常量被锁死后，描边动画 (dasharray=length) 的视觉映射稳定。
 */

describe("U-02 字形常量表", () => {
  it("VARIABLE 八个字母全部存在（含重复的 A）", () => {
    for (const letter of WORDMARK_LETTERS) {
      expect(GLYPHS[letter], `字形 ${letter} 缺失`).toBeDefined();
    }
    expect(WORDMARK_LETTERS).toEqual(["V", "A", "R", "I", "A", "B", "L", "E"]);
  });

  it("唯一字母集 = V/A/R/I/B/L/E，无多余字形", () => {
    expect(Object.keys(GLYPHS).sort()).toEqual(["A", "B", "E", "I", "L", "R", "V"]);
  });

  it("每个长度常量为正的有限数", () => {
    for (const [letter, g] of Object.entries(GLYPHS)) {
      expect(Number.isFinite(g.length), `${letter}.length 非有限数`).toBe(true);
      expect(g.length, `${letter}.length 必须 > 0`).toBeGreaterThan(0);
    }
  });

  it("viewBox 统一为 0 0 100 120 且格式合法", () => {
    for (const [letter, g] of Object.entries(GLYPHS)) {
      expect(g.viewBox, `${letter}.viewBox`).toBe("0 0 100 120");
      const parts = g.viewBox.trim().split(/\s+/).map(Number);
      expect(parts).toHaveLength(4);
      expect(parts.every((n) => Number.isFinite(n) && n >= 0)).toBe(true);
      expect(parts[2]).toBeGreaterThan(0); // width
      expect(parts[3]).toBeGreaterThan(0); // height
    }
  });

  it("path 数据非空、指令合法、坐标在画布内、无重复坏数据", () => {
    const seen = new Set<string>();
    for (const [letter, g] of Object.entries(GLYPHS)) {
      expect(g.path.length, `${letter}.path 为空`).toBeGreaterThan(0);
      // 无重复畸形 path（同一路径数据不应在两个字母下重复）
      expect(seen.has(g.path), `${letter}.path 与其他字母重复`).toBe(false);
      seen.add(g.path);
      // 指令白名单：M/L/H/V/A/Z（手绘几何骨架，禁曲线滥用之外的字节垃圾）
      const tokens = g.path.match(/[MLHVAZ]|-?\d+(?:\.\d+)?/g) ?? [];
      expect(tokens.length, `${letter}.path 无法分词`).toBeGreaterThan(0);
      for (const tk of tokens) {
        expect(/^(?:[MLHVAZ]|-?\d+(?:\.\d+)?)$/.test(tk), `${letter}.path 非法 token ${tk}`).toBe(true);
      }
      // 坐标范围：数值（含弧半径）都应在合理画布区间 [-5, 125] 内
      for (const tk of tokens) {
        if (/^-?\d/.test(tk)) {
          const n = Number(tk);
          expect(n, `${letter}.path 坐标越界 ${tk}`).toBeGreaterThanOrEqual(-5);
          expect(n, `${letter}.path 坐标越界 ${tk}`).toBeLessThanOrEqual(125);
        }
      }
      // 子路径必须闭合（evenodd 填充依赖闭合轮廓）
      const moves = (g.path.match(/M/g) ?? []).length;
      const closes = (g.path.match(/Z/g) ?? []).length;
      expect(moves, `${letter} 子路径数`).toBeGreaterThanOrEqual(1);
      expect(closes, `${letter} 每个子路径必须 Z 闭合`).toBe(moves);
    }
  });

  it("长度常量与粗略几何量级一致（防手滑写错数量级）", () => {
    // 字高 90 + 笔画宽 14 的骨架：单字母轮廓应落在 [150, 700] 区间
    for (const [letter, g] of Object.entries(GLYPHS)) {
      expect(g.length, `${letter}.length 量级异常`).toBeGreaterThanOrEqual(150);
      expect(g.length, `${letter}.length 量级异常`).toBeLessThanOrEqual(700);
    }
  });
});
