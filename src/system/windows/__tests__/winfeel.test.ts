import { describe, expect, it } from "vitest";
import { computeGuide, detectGesture, detectShake, parseScreenDetails, screenShift } from "../winfeel";

/**
 * AI-01 窗口手感组：纯函数层单测（M-01 摇晃 / Z-41 手势 / M-07 参考线 / Z-42-M-05 跨屏）。
 */

describe("M-01 detectShake 摇晃检测", () => {
  const line = (xs: number[], step = 10): Array<{ x: number; t: number }> =>
    xs.map((x, i) => ({ x, t: i * step }));

  const shakeXs = (): number[] => {
    const xs: number[] = [];
    for (let i = 0; i < 20; i++) xs.push(i * 6); // 0→114 向右
    for (let i = 0; i < 20; i++) xs.push(114 - i * 8); // →-38 向左
    for (let i = 0; i < 20; i++) xs.push(-38 + i * 8); // →114 向右
    return xs;
  };

  it("普通拖拽（单调移动）不触发", () => {
    const xs = Array.from({ length: 120 }, (_, i) => i * 3);
    expect(detectShake(line(xs))).toBe(false);
  });

  it("右→左→右 快速摇晃触发（≥2 次反转、行程>60、<1200ms）", () => {
    expect(detectShake(line(shakeXs()))).toBe(true);
  });

  it("样本不足 2 个 → false", () => {
    expect(detectShake([])).toBe(false);
    expect(detectShake([{ x: 10, t: 0 }])).toBe(false);
  });

  it("时长 ≥ 1200ms → false", () => {
    expect(detectShake(line(shakeXs(), 40))).toBe(false); // 59*40=2360ms
  });

  it("行程 ≤ 60px → false", () => {
    expect(detectShake(line([0, 5, 0, 5, 0], 10))).toBe(false);
  });

  it("静止帧（dx=0）不产生方向反转也不破坏判定", () => {
    expect(detectShake(line([0, 50, 50, 50, -50, -50, 60], 10))).toBe(true);
  });
});

describe("Z-41 detectGesture 手势识别", () => {
  it("四个方向", () => {
    expect(detectGesture([{ x: 0, y: 0 }, { x: 100, y: 8 }])).toBe("right");
    expect(detectGesture([{ x: 0, y: 0 }, { x: -80, y: 5 }])).toBe("left");
    expect(detectGesture([{ x: 0, y: 0 }, { x: 6, y: -90 }])).toBe("up");
    expect(detectGesture([{ x: 0, y: 0 }, { x: 10, y: 70 }])).toBe("down");
  });

  it("低于阈值 → null；阈值含等号", () => {
    expect(detectGesture([{ x: 0, y: 0 }, { x: 40, y: 30 }])).toBe(null);
    expect(detectGesture([{ x: 0, y: 0 }, { x: 47, y: 0 }])).toBe(null);
    expect(detectGesture([{ x: 0, y: 0 }, { x: 48, y: 0 }])).toBe("right");
  });

  it("路径点不足 → null", () => {
    expect(detectGesture([])).toBe(null);
    expect(detectGesture([{ x: 1, y: 2 }])).toBe(null);
  });

  it("对角线取主导轴", () => {
    expect(detectGesture([{ x: 0, y: 0 }, { x: 30, y: -60 }])).toBe("up");
    expect(detectGesture([{ x: 0, y: 0 }, { x: 80, y: -40 }])).toBe("right");
  });
});

describe("M-07 computeGuide 对齐参考线", () => {
  const cand = { x: 100, y: 100, w: 400, h: 300 };

  it("左边缘 ≤1px 精确对齐", () => {
    const r = computeGuide([cand], { x: 99.5, y: 0, w: 200, h: 200 });
    expect(r.snapX).toBe(100);
    expect(r.guideXs).toContain(100);
  });

  it("阈值内取最近线吸附；全部命中线都进参考线", () => {
    // 左缘 106 vs 候选左 100（差 6）；右缘 306 vs 候选中线 300（差 6）→ 取先到的左缘
    const r = computeGuide([cand], { x: 106, y: 0, w: 200, h: 200 });
    expect(r.snapX).toBe(100);
    expect(r.guideXs).toEqual([100, 300]);
  });

  it("垂直方向吸附候选上缘", () => {
    const r = computeGuide([cand], { x: 0, y: 105, w: 200, h: 100 });
    expect(r.snapY).toBe(100);
    expect(r.guideYs).toEqual([100]);
  });

  it("工作区中线（lines 参数）参与吸附", () => {
    const r = computeGuide([], { x: 966, y: 534, w: 100, h: 100 }, 8, { xs: [960], ys: [540] });
    expect(r.snapX).toBe(960);
    expect(r.snapY).toBe(540);
    expect(r.guideXs).toEqual([960]);
    expect(r.guideYs).toEqual([540]);
  });

  it("阈值外 → null 且无参考线", () => {
    const r = computeGuide([cand], { x: 700, y: 700, w: 100, h: 100 });
    expect(r.snapX).toBe(null);
    expect(r.snapY).toBe(null);
    expect(r.guideXs).toEqual([]);
    expect(r.guideYs).toEqual([]);
  });
});

describe("Z-42/M-05 screenShift 跨屏摆渡", () => {
  const twoScreens = [
    { left: 0, top: 0, width: 1920, height: 1080 },
    { left: 1920, top: 0, width: 1920, height: 1080 },
  ];

  it("向右摆渡到邻屏（x 平移屏宽差，y 不变）", () => {
    const r = screenShift({ x: 100, y: 100, w: 800, h: 600 }, twoScreens, "right")!;
    expect(r.x).toBe(2020);
    expect(r.y).toBe(100);
  });

  it("钳制进邻屏右边界", () => {
    const r = screenShift({ x: 1500, y: 0, w: 800, h: 600 }, twoScreens, "right")!;
    expect(r.x).toBe(3040); // 1920+1920-800
  });

  it("向左摆渡回主屏", () => {
    const r = screenShift({ x: 2000, y: 100, w: 800, h: 600 }, twoScreens, "left")!;
    expect(r.x).toBe(80);
  });

  it("单屏无邻居 → null；上下堆叠屏不算水平邻居", () => {
    expect(screenShift({ x: 100, y: 100, w: 800, h: 600 }, [twoScreens[0]!], "right")).toBe(null);
    const stacked = [
      { left: 0, top: 0, width: 1920, height: 1080 },
      { left: 0, top: 1080, width: 1920, height: 1080 },
    ];
    expect(screenShift({ x: 100, y: 100, w: 800, h: 600 }, stacked, "right")).toBe(null);
  });

  it("空屏幕数组 → null", () => {
    expect(screenShift({ x: 0, y: 0, w: 100, h: 100 }, [], "left")).toBe(null);
  });
});

describe("parseScreenDetails 屏幕抽取", () => {
  it("重定基到 currentScreen 原点（视口局部坐标）", () => {
    const screens = parseScreenDetails({
      currentScreen: { left: 1920, top: 0 },
      screens: [
        { left: 1920, top: 0, width: 1920, height: 1080 },
        { left: 0, top: 0, width: 1920, height: 1080 },
      ],
    });
    expect(screens).toEqual([
      { left: 0, top: 0, width: 1920, height: 1080 },
      { left: -1920, top: 0, width: 1920, height: 1080 },
    ]);
  });

  it("结构不完整 → null", () => {
    expect(parseScreenDetails(null)).toBe(null);
    expect(parseScreenDetails("x")).toBe(null);
    expect(parseScreenDetails({ screens: "bad" })).toBe(null);
    expect(parseScreenDetails({ screens: [], currentScreen: {} })).toBe(null);
  });
});