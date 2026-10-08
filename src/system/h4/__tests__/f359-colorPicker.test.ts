import { describe, expect, it } from "vitest";
import { LOUPE_ZOOM, RECENT_COLORS_CAP, auditColorAccuracy, clampChannel, clipboardPayload, loupeGeometry, pickOnce, pushRecentColor, toHex, type Rgb } from "../f359-colorPicker";

const RED: Rgb = { r: 255, g: 0, b: 0 };
const GREEN: Rgb = { r: 0, g: 128, b: 0 };

describe("F359 全局屏幕拾色器", () => {
  it("HEX/RGB 双格式复制（判据双读数）", () => {
    expect(toHex(RED)).toBe("#FF0000");
    const payload = clipboardPayload(GREEN);
    expect(payload.hex).toBe("#008000");
    expect(payload.rgb).toBe("rgb(0, 128, 0)");
  });

  it("通道钳制：NaN→0、超界钳回、小数取整（零非法色）", () => {
    expect(clampChannel(Number.NaN)).toBe(0);
    expect(clampChannel(300)).toBe(255);
    expect(clampChannel(-5)).toBe(0);
    expect(clampChannel(127.6)).toBe(128);
    expect(toHex({ r: 300, g: -1, b: 12.4 })).toBe("#FF000C");
  });

  it("进 F234 最近使用色：容量 8、重复色提队首不重复占位", () => {
    let recent: Rgb[] = [];
    for (let i = 0; i < RECENT_COLORS_CAP + 2; i++) recent = pushRecentColor(recent, { r: i, g: 0, b: 0 });
    expect(recent).toHaveLength(RECENT_COLORS_CAP);
    expect(toHex(recent[0]!)).toBe("#090000"); // 最旧的 #000000、#010000 被挤掉
    const before = recent.length;
    const moved = toHex(recent[3]!);
    recent = pushRecentColor(recent, recent[3]!);
    expect(recent).toHaveLength(before);
    expect(toHex(recent[0]!)).toBe(moved); // 提至队首
    expect(new Set(recent.map(toHex)).size).toBe(before); // 不重复占位
  });

  it("取色准确性审计：色板 20 点零误差判据", () => {
    const board: Rgb[] = Array.from({ length: 20 }, (_, i) => ({ r: i * 13 % 256, g: i * 7 % 256, b: i * 29 % 256 }));
    const samples = board.map((expected) => ({ expected, picked: expected }));
    expect(auditColorAccuracy(samples)).toEqual([]);
    const bad = auditColorAccuracy([{ expected: RED, picked: { r: 254, g: 0, b: 0 } }]);
    expect(bad).toHaveLength(1);
    expect(bad[0]!.picked).toBe("#FE0000");
  });

  it("连续模式：Shift 按住保持 active；松开取一色即退出", () => {
    let s = { active: true, shiftHeld: true, picks: 0 };
    s = pickOnce(s);
    expect(s.active).toBe(true);
    expect(s.picks).toBe(1);
    s = { ...s, shiftHeld: false };
    s = pickOnce(s);
    expect(s.active).toBe(false);
    expect(s.picks).toBe(2);
  });

  it("放大环几何：采样 15px × 8 倍", () => {
    const g = loupeGeometry();
    expect(g.zoom).toBe(LOUPE_ZOOM);
    expect(g.radiusPx).toBe(60);
  });
});
