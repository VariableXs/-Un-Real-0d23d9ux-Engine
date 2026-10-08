import { describe, expect, it } from "vitest";
import { nudgeBegin, nudgeStep, systemMenuItems } from "../systemMenu";
import { aspectSize, formatSizeHint, sizeHintFor } from "../sizeHint";
import { DRAG_CANCEL_MS, dragActive, dragBegin, dragCancelByEsc, dragSettle } from "../dragCancel";
import { BAND_TOKEN, bandFor, bandStyle, colorBandEnabled, loadBandMap, saveBandMap, setColorBandEnabled } from "../colorBand";
import { minimapGrid, minimapHit, minimapLayout } from "../minimap";
import type { VwmRect } from "../vwm";

describe("V-21 经典系统菜单（三态禁用）", () => {
  it("normal 态：还原禁用、其余可用", () => {
    const m = systemMenuItems("normal");
    const by = (id: string) => m.find((x) => x.id === id)!;
    expect(by("restore").disabled).toBe(true);
    expect(by("move").disabled).toBe(false);
    expect(by("size").disabled).toBe(false);
    expect(by("minimize").disabled).toBe(false);
    expect(by("maximize").disabled).toBe(false);
    expect(by("close").disabled).toBe(false);
  });

  it("max 态：移动/大小/最大化禁用、还原可用", () => {
    const m = systemMenuItems("max");
    const by = (id: string) => m.find((x) => x.id === id)!;
    expect(by("restore").disabled).toBe(false);
    expect(by("move").disabled).toBe(true);
    expect(by("size").disabled).toBe(true);
    expect(by("maximize").disabled).toBe(true);
  });

  it("minimized 态：移动/大小/最小化禁用、还原可用", () => {
    const m = systemMenuItems("minimized");
    const by = (id: string) => m.find((x) => x.id === id)!;
    expect(by("move").disabled).toBe(true);
    expect(by("size").disabled).toBe(true);
    expect(by("minimize").disabled).toBe(true);
    expect(by("restore").disabled).toBe(false);
  });

  it("键盘微调：方向 1px、Shift 10px、size 模式钳制最小值", () => {
    let st = nudgeBegin("move", { x: 100, y: 100, w: 800, h: 600 });
    st = nudgeStep(st, "right", false);
    expect(st.rect.x).toBe(101);
    st = nudgeStep(st, "up", true);
    expect(st.rect.y).toBe(90);
    let sz = nudgeBegin("size", { x: 0, y: 0, w: 305, h: 285 });
    sz = nudgeStep(sz, "left", false);
    expect(sz.rect.w).toBe(304);
    for (let i = 0; i < 20; i++) sz = nudgeStep(sz, "left", false);
    expect(sz.rect.w).toBe(300);
  });
});

describe("V-23 几何提示", () => {
  it("格式化 W×H", () => {
    expect(formatSizeHint(1280.4, 719.6)).toBe("1280×720");
  });
  it("等比模式锁定起点比例（误差 <0.5%）", () => {
    for (let i = 0; i < 20; i++) {
      const { w, h } = aspectSize(1280, 720, 1280 + i * 10);
      expect(Math.abs(h - (w * 720) / 1280)).toBeLessThan(1);
    }
  });
  it("Shift 按住才等比；否则自由尺寸", () => {
    expect(sizeHintFor({ w: 1280, h: 720 }, { w: 1000, h: 500 }, true)).toMatchObject({ aspect: true, h: 563 });
    expect(sizeHintFor({ w: 1280, h: 720 }, { w: 1000, h: 500 }, false)).toEqual({ w: 1000, h: 500, aspect: false });
  });
});

describe("V-30 拖拽中断与回弹", () => {
  it("拖拽中 Esc → 回弹到起点 + 清预览；取消后无残留状态", () => {
    const start: VwmRect = { x: 10, y: 20, w: 800, h: 600 };
    dragBegin("w1", start);
    expect(dragActive()?.winId).toBe("w1");
    const r = dragCancelByEsc();
    expect(r).toEqual({ winId: "w1", rect: start, clearPreview: true });
    expect(dragActive()).toBeNull();
    expect(dragCancelByEsc()).toBeNull();
  });
  it("正常落位后 Esc 不再拦截", () => {
    dragBegin("w1", { x: 0, y: 0, w: 800, h: 600 });
    dragSettle();
    expect(dragCancelByEsc()).toBeNull();
  });
  it("回弹时长常量 150ms", () => {
    expect(DRAG_CANCEL_MS).toBe(150);
  });
});

describe("V-29 色带标记", () => {
  it("默认关闭：bandFor 返回 null（逐像素等于现状）", () => {
    expect(colorBandEnabled()).toBe(false);
    expect(bandFor("write")).toBeNull();
  });
  it("开启后按映射返回语义色；未映射应用返回 null", () => {
    setColorBandEnabled(true);
    saveBandMap({ write: "info", "tp:chat": "success" });
    expect(loadBandMap().write).toBe("info");
    expect(bandFor("write")).toBe("info");
    expect(bandFor("tp:chat")).toBe("success");
    expect(bandFor("fate")).toBeNull();
  });
  it("色带样式：absolute 覆盖绘制 2px、高对比度 3px、色值走令牌", () => {
    const s = bandStyle("warning");
    expect(s.height).toBe(2);
    expect(s.position).toBe("absolute");
    expect(BAND_TOKEN.warning).toContain("var(");
    expect(bandStyle("warning", true).height).toBe(3);
  });
});

describe("V-28 分布小地图", () => {
  const wa: VwmRect = { x: 0, y: 0, w: 1920, h: 1080 };
  const cell = { desktop: 0, display: "main", workArea: wa };

  it("缩略几何按等比缩放，含内边距", () => {
    const r = minimapLayout(cell, [
      { winId: "a", title: "A", rect: { x: 100, y: 100, w: 800, h: 600 }, minimized: false },
    ], 300, 200);
    const p = r.placements[0]!;
    expect(p.thumb.x).toBeGreaterThan(0);
    expect(p.thumb.w).toBeLessThan(300);
    expect(p.thumb.h).toBeLessThan(200);
  });

  it("30 窗口 × 4 桌面分布图布局正确、空桌面如实为空", () => {
    const cells = Array.from({ length: 4 }, (_, d) => ({ desktop: d, display: "main", workArea: wa }));
    const winsByDesktop: Record<number, { winId: string; title: string; rect: VwmRect; minimized: boolean }[]> = { 0: [], 1: [], 2: [], 3: [] };
    for (let i = 0; i < 30; i++) {
      winsByDesktop[i % 4]!.push({ winId: `w${i}`, title: `T${i}`, rect: { x: (i * 37) % 1600, y: (i * 53) % 900, w: 400, h: 300 }, minimized: false });
    }
    const grid = minimapGrid(cells, winsByDesktop, 300, 200);
    expect(grid).toHaveLength(4);
    expect(grid[0]!.placements).toHaveLength(8);
    expect(grid[3]!.placements).toHaveLength(7);
  });

  it("点击直达：命中检测 100%", () => {
    const r = minimapLayout(cell, [
      { winId: "a", title: "A", rect: { x: 0, y: 0, w: 960, h: 1080 }, minimized: false },
      { winId: "b", title: "B", rect: { x: 960, y: 0, w: 960, h: 1080 }, minimized: false },
    ], 300, 200);
    expect(minimapHit(r, 40, 60)).toBe("a");
    expect(minimapHit(r, 200, 60)).toBe("b");
    expect(minimapHit(r, 2, 2)).toBeNull();
  });
});

