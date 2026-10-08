import { beforeEach, describe, expect, it } from "vitest";
import {
  HISTORY_CAP,
  HISTORY_KEY,
  addColorHistory,
  clampZoom,
  gridLayout,
  gridLines,
  hexToRgb,
  hslToRgb,
  loadHistory,
  parseHistory,
  pickPixel,
  rgbToHex,
  rgbToHsl,
  saveHistory,
  sourceRect,
} from "../magnifierCore";

/**
 * Z-25 纯逻辑核心回归：
 * - 颜色互转往返 / HSL 黑白灰边界
 * - 网格显示判定（≥8× 才画网格线）
 * - 放大源矩形钳制
 * - 取色历史上限淘汰（16 色）与 localStorage 持久化
 */

beforeEach(() => {
  globalThis.localStorage?.clear?.();
});

describe("HEX/RGB/HSL 互转", () => {
  it("hexToRgb 解析 #RGB / #RRGGBB / 裸 RRGGBB", () => {
    expect(hexToRgb("#fff")).toEqual({ r: 255, g: 255, b: 255 });
    expect(hexToRgb("#ff8800")).toEqual({ r: 255, g: 136, b: 0 });
    expect(hexToRgb("00ff7f")).toEqual({ r: 0, g: 255, b: 127 });
  });

  it("hexToRgb 非法输入返回 null（如实失败）", () => {
    expect(hexToRgb("#ff")).toBeNull();
    expect(hexToRgb("zzzzzz")).toBeNull();
    expect(hexToRgb("")).toBeNull();
    expect(hexToRgb("#12345")).toBeNull();
  });

  it("rgbToHex 补零与钳制", () => {
    expect(rgbToHex({ r: 0, g: 136, b: 255 })).toBe("#0088ff");
    expect(rgbToHex({ r: 300, g: -1, b: 15.6 })).toBe("#ff0010");
  });

  it("RGB→HSL→RGB 往返（误差 ≤1）", () => {
    const samples = [
      { r: 255, g: 136, b: 0 },
      { r: 18, g: 52, b: 86 },
      { r: 200, g: 200, b: 30 },
      { r: 1, g: 254, b: 128 },
      { r: 77, g: 77, b: 77 },
    ];
    for (const c of samples) {
      const hsl = rgbToHsl(c);
      const back = hslToRgb(hsl);
      expect(Math.abs(back.r - c.r)).toBeLessThanOrEqual(1);
      expect(Math.abs(back.g - c.g)).toBeLessThanOrEqual(1);
      expect(Math.abs(back.b - c.b)).toBeLessThanOrEqual(1);
    }
  });

  it("HSL 边界：黑 / 白 / 灰（s=0）", () => {
    expect(rgbToHsl({ r: 0, g: 0, b: 0 })).toEqual({ h: 0, s: 0, l: 0 });
    expect(rgbToHsl({ r: 255, g: 255, b: 255 })).toEqual({ h: 0, s: 0, l: 100 });
    const gray = rgbToHsl({ r: 128, g: 128, b: 128 });
    expect(gray.s).toBe(0);
    expect(gray.l).toBeCloseTo(50.2, 0);
    // 灰阶往返仍是灰阶
    const back = hslToRgb(gray);
    expect(back.r).toBe(back.g);
    expect(back.g).toBe(back.b);
  });

  it("HSL 基色角度：红 0 / 绿 120 / 蓝 240", () => {
    expect(rgbToHsl({ r: 255, g: 0, b: 0 }).h).toBe(0);
    expect(rgbToHsl({ r: 0, g: 255, b: 0 }).h).toBe(120);
    expect(rgbToHsl({ r: 0, g: 0, b: 255 }).h).toBe(240);
    expect(hslToRgb({ h: 0, s: 100, l: 50 })).toEqual({ r: 255, g: 0, b: 0 });
    expect(hslToRgb({ h: 240, s: 100, l: 50 })).toEqual({ r: 0, g: 0, b: 255 });
  });

  it("hslToRgb h 超界取模、s/l 越界钳制", () => {
    expect(hslToRgb({ h: 360, s: 100, l: 50 })).toEqual({ r: 255, g: 0, b: 0 });
    expect(hslToRgb({ h: -360, s: 100, l: 50 })).toEqual({ r: 255, g: 0, b: 0 });
    expect(hslToRgb({ h: 0, s: 200, l: 100 })).toEqual({ r: 255, g: 255, b: 255 });
    expect(hslToRgb({ h: 0, s: -10, l: 0 })).toEqual({ r: 0, g: 0, b: 0 });
  });
});

describe("倍率与网格", () => {
  it("clampZoom 钳制 2..16", () => {
    expect(clampZoom(1)).toBe(2);
    expect(clampZoom(0)).toBe(2);
    expect(clampZoom(99)).toBe(16);
    expect(clampZoom(8.6)).toBe(9);
    expect(clampZoom(Number.NaN)).toBe(2);
  });

  it("网格显示判定：<8× 不显示，≥8× 显示且 step=倍率", () => {
    expect(gridLayout(2, 400, 300).show).toBe(false);
    expect(gridLayout(4, 400, 300).show).toBe(false);
    expect(gridLayout(7, 400, 300).show).toBe(false);
    expect(gridLayout(7.4, 400, 300).show).toBe(false); // 7.4 钳制取整仍 <8
    const g8 = gridLayout(8, 400, 300);
    expect(g8.show).toBe(true);
    expect(g8.step).toBe(8);
    expect(g8.cols).toBe(50);
    expect(g8.rows).toBe(37);
    const g16 = gridLayout(16, 320, 160);
    expect(g16.show).toBe(true);
    expect(g16.cols).toBe(20);
    expect(g16.rows).toBe(10);
  });

  it("gridLines：<8× 无线；≥8× 内部线数 = cols-1 / rows-1", () => {
    expect(gridLines(4, 400, 300)).toEqual({ vxs: [], hys: [] });
    const g = gridLines(8, 80, 60);
    expect(g.vxs).toEqual([8, 16, 24, 32, 40, 48, 56, 64, 72]); // 80/8=10 列 → 9 条内部竖线
    expect(g.hys).toEqual([8, 16, 24, 32, 40, 48]); // 60/8=7 行 → 6 条内部横线
  });

  it("sourceRect：以光标为中心并在快照内钳制", () => {
    // 视口 320×240，倍率 8 → 源区 40×30；屏幕中央光标
    const mid = sourceRect({ x: 960, y: 540 }, 8, 320, 240, 1920, 1080);
    expect(mid).toEqual({ sx: 940, sy: 525, sw: 40, sh: 30 });
    // 光标贴左上角 → 钳到 0,0
    const tl = sourceRect({ x: 0, y: 0 }, 8, 320, 240, 1920, 1080);
    expect(tl.sx).toBe(0);
    expect(tl.sy).toBe(0);
    // 光标贴右下角 → 钳到快照右下
    const br = sourceRect({ x: 1919, y: 1079 }, 8, 320, 240, 1920, 1080);
    expect(br.sx).toBe(1920 - 40);
    expect(br.sy).toBe(1080 - 30);
    // 快照比源区还小 → 全图
    const tiny = sourceRect({ x: 5, y: 5 }, 2, 400, 400, 10, 10);
    expect(tiny).toEqual({ sx: 0, sy: 0, sw: 200, sh: 200 });
  });

  it("pickPixel：正常读取与越界 null", () => {
    // 2×1 图：红 | 蓝
    const data = new Uint8ClampedArray([255, 0, 0, 255, 0, 0, 255, 255]);
    expect(pickPixel(data, 2, 0, 0)).toEqual({ r: 255, g: 0, b: 0 });
    expect(pickPixel(data, 2, 1, 0)).toEqual({ r: 0, g: 0, b: 255 });
    expect(pickPixel(data, 2, 2, 0)).toBeNull();
    expect(pickPixel(data, 2, -1, 0)).toBeNull();
    expect(pickPixel(data, 2, 0, 5)).toBeNull();
  });
});

describe("取色历史", () => {
  it("parseHistory：坏 JSON / 非数组 / 非法色值全部过滤", () => {
    expect(parseHistory(null)).toEqual([]);
    expect(parseHistory("")).toEqual([]);
    expect(parseHistory("{oops")).toEqual([]);
    expect(parseHistory('["#ff0000", 42, "#bad", "#00ff00"]')).toEqual(["#ff0000", "#00ff00"]);
  });

  it("addColorHistory：新色置顶、重复上移去重、16 色上限淘汰最旧", () => {
    let list: string[] = [];
    for (let i = 0; i < 20; i++) list = addColorHistory(list, `#aa${i.toString(16).padStart(2, "0")}00`);
    expect(list).toHaveLength(HISTORY_CAP);
    expect(list[0]).toBe("#aa1300"); // 第 20 次（0x14→"14"）置顶
    expect(list).not.toContain("#aa0000"); // 最旧的 4 个被淘汰
    // 重复取色 → 上移到顶且不重复
    list = addColorHistory(list, "#aa0500");
    expect(list[0]).toBe("#aa0500");
    expect(list.filter((c) => c === "#aa0500")).toHaveLength(1);
    expect(list).toHaveLength(HISTORY_CAP);
    // 大小写视为同一色
    list = addColorHistory(list, "#AA0500");
    expect(list.filter((c) => c.toLowerCase() === "#aa0500")).toHaveLength(1);
    // 非法色不入历史
    expect(addColorHistory(list, "red")).toBe(list);
  });

  it("loadHistory / saveHistory 走 localStorage 指定键并往返一致", () => {
    expect(loadHistory()).toEqual([]);
    saveHistory(addColorHistory(addColorHistory([], "#112233"), "#445566"));
    expect(globalThis.localStorage?.getItem(HISTORY_KEY)).toBe('["#445566","#112233"]');
    expect(loadHistory()).toEqual(["#445566", "#112233"]);
    // 超上限持久化时截断
    const many: string[] = [];
    for (let i = 0; i < 30; i++) many.push(`#00${i.toString(16).padStart(2, "0")}ff`);
    saveHistory(many);
    expect(loadHistory()).toHaveLength(HISTORY_CAP);
  });
});
