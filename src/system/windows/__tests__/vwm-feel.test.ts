import { describe, expect, it, beforeAll, beforeEach } from "vitest";
import {
  applyLayoutSnapshot,
  closeVwmWin,
  cycleVwmFocusFiltered,
  deleteLayoutSnapshot,
  ferryVwmWin,
  focusVwmWin,
  listLayoutSnapshots,
  minimizeVwmWin,
  minimizedOrder,
  moveVwmWin,
  openVwmApp,
  openVwmTpNew,
  restoreGeomFor,
  rollVwmWin,
  saveLayoutSnapshot,
  setVwmOpacity,
  setVwmTopmost,
  setVwmWorkArea,
  settleVwmWin,
  snapVwmRect,
  snapVwmWin,
  toggleMaxVwmWin,
  VWM_TITLEBAR_H,
  vwmStore,
  type VwmApp,
  type VwmWin,
} from "../vwm";

/**
 * AI-01 窗口手感组：数据层回归（M-02 卷帘 / M-03 抽屉序 / Z-36 透明度置顶 /
 * Z-37 关闭记忆 / Z-40 布局快照 / M-08 过滤轮转 / Z-42 跨屏摆渡）。
 * node 环境：window / localStorage 均为最小 shim（与 vwm.test.ts / snapshots.test.ts 同策略）。
 */

const wa = { x: 0, y: 0, w: 1920, h: 1046 };

let winSeq = 0;
function makeWin(p: Partial<VwmWin> & { id: string }): VwmWin {
  winSeq++;
  return {
    id: p.id,
    app: p.app ?? "notes",
    path: null,
    x: p.x ?? 10,
    y: p.y ?? 20,
    w: p.w ?? 800,
    h: p.h ?? 600,
    state: p.state ?? "normal",
    minimized: p.minimized ?? false,
    hidden: p.hidden ?? false,
    z: p.z ?? winSeq,
    restore: p.restore ?? null,
    group: null,
    groupActive: false,
    rolledUp: p.rolledUp ?? false,
    minimizedAt: p.minimizedAt ?? null,
    opacity: p.opacity ?? 1,
    topmost: p.topmost ?? false,
  };
}

function seed(wins: VwmWin[]): void {
  vwmStore.setState((s) => ({ wins, closing: [], flying: [], seq: s.seq + 1 }));
}

/** 打开一款软件并返回新实例 id（openVwmApp 会聚焦新建实例）。 */
function openApp(app: VwmApp): string {
  openVwmApp(app, { forceNew: true });
  const id = vwmStore.getState().focusedId;
  expect(id).toBeTruthy();
  return id!;
}

function winOf(id: string): VwmWin {
  const w = vwmStore.getState().wins.find((x) => x.id === id);
  expect(w).toBeTruthy();
  return w!;
}

beforeAll(() => {
  const g = globalThis as { window?: unknown; localStorage?: Storage };
  g.window ??= { setTimeout };
  if (!g.localStorage) {
    const mem = new Map<string, string>();
    g.localStorage = {
      getItem: (k: string) => mem.get(k) ?? null,
      setItem: (k: string, v: string) => void mem.set(k, String(v)),
      removeItem: (k: string) => void mem.delete(k),
      clear: () => void mem.clear(),
      key: (i: number) => [...mem.keys()][i] ?? null,
      get length() {
        return mem.size;
      },
    } as Storage;
  }
});

beforeEach(() => {
  localStorage.clear();
  vwmStore.setState({ wins: [], focusedId: null, topZ: 10, snapPreview: null, closing: [], flying: [] });
  setVwmWorkArea(wa);
});

describe("M-02 卷帘 rollVwmWin", () => {
  it("收起钳制到标题栏高度，展开还原原高", () => {
    const id = openApp("calc");
    const h0 = winOf(id).h;
    expect(h0).toBeGreaterThan(VWM_TITLEBAR_H);
    rollVwmWin(id, true);
    let w = winOf(id);
    expect(w.rolledUp).toBe(true);
    expect(w.h).toBe(VWM_TITLEBAR_H);
    expect(w.rolledFromH).toBe(h0);
    rollVwmWin(id, false);
    w = winOf(id);
    expect(w.rolledUp).toBe(false);
    expect(w.h).toBe(h0);
  });

  it("贴靠（snapVwmWin / snapVwmRect）前自动展开", () => {
    const id = openApp("calc");
    const h0 = winOf(id).h;
    rollVwmWin(id, true);
    snapVwmWin(id, "left");
    let w = winOf(id);
    expect(w.rolledUp).toBe(false);
    expect(w.h).toBe(wa.h); // 左右半屏 = 全高（贴靠矩形语义）
    expect(w.restore!.h).toBe(h0); // 还原几何携带展开后的原高
    expect(w.x).toBe(wa.x);
    expect(w.w).toBe(Math.round(wa.w / 2));
    rollVwmWin(id, true);
    snapVwmRect(id, { x: 10, y: 10, w: 400, h: 300 });
    w = winOf(id);
    expect(w.rolledUp).toBe(false);
    expect(w.h).toBe(300);
  });

  it("最大化 / 还原前自动展开", () => {
    const id = openApp("calc");
    rollVwmWin(id, true);
    snapVwmRect(id, { x: 10, y: 10, w: 400, h: 300 });
    rollVwmWin(id, true);
    toggleMaxVwmWin(id);
    let w = winOf(id);
    expect(w.state).toBe("max");
    expect(w.rolledUp).toBe(false);
    toggleMaxVwmWin(id);
    w = winOf(id);
    expect(w.state).toBe("normal");
    expect(w.h).toBe(300);
  });
});

describe("M-03 minimizedAt 与抽屉排序", () => {
  it("最小化记录时刻，聚焦清除", () => {
    const id = openApp("calc");
    minimizeVwmWin(id);
    let w = winOf(id);
    expect(w.minimized).toBe(true);
    expect(typeof w.minimizedAt).toBe("number");
    focusVwmWin(id);
    w = winOf(id);
    expect(w.minimized).toBe(false);
    expect(w.minimizedAt).toBe(null);
  });

  it("minimizedOrder 只含最小化窗口且按最近优先", () => {
    const a = makeWin({ id: "a", minimized: true, minimizedAt: 100 });
    const b = makeWin({ id: "b", minimized: true, minimizedAt: 300 });
    const c = makeWin({ id: "c" });
    expect(minimizedOrder([a, b, c]).map((w) => w.id)).toEqual(["b", "a"]);
  });
});

describe("Z-36 不透明度与置顶", () => {
  it("opacity 钳制 0.2..1 并保留两位小数", () => {
    const id = openApp("calc");
    setVwmOpacity(id, 0.05);
    expect(winOf(id).opacity).toBe(0.2);
    setVwmOpacity(id, 2);
    expect(winOf(id).opacity).toBe(1);
    setVwmOpacity(id, 0.666);
    expect(winOf(id).opacity).toBe(0.67);
    setVwmOpacity(id, 0.754);
    expect(winOf(id).opacity).toBe(0.75);
  });

  it("置顶窗在聚焦普通窗口时保持其上；取消后恢复常规 Z 序", () => {
    const a = openVwmTpNew("tp:topa");
    const c = openVwmTpNew("tp:topc");
    setVwmTopmost(a, true);
    expect(winOf(a).topmost).toBe(true);
    focusVwmWin(c);
    expect(winOf(c).z).toBeLessThan(winOf(a).z);
    setVwmTopmost(a, false);
    focusVwmWin(c);
    expect(winOf(c).z).toBe(vwmStore.getState().topZ);
    expect(winOf(a).z).toBeLessThan(winOf(c).z);
  });
});

describe("Z-40 布局快照", () => {
  it("保存 → 列表 → 应用（几何还原）", () => {
    const a = openVwmTpNew("tp:la");
    moveVwmWin(a, 111, 222);
    saveLayoutSnapshot("work");
    expect(listLayoutSnapshots()).toEqual([{ name: "work", count: 1 }]);
    moveVwmWin(a, 500, 500);
    expect(applyLayoutSnapshot("work")).toBe(true);
    const w = winOf(a);
    expect(w.x).toBe(111);
    expect(w.y).toBe(222);
  });

  it("缺失的 app → 新开窗口并摆到保存矩形（按工作区钳制）", () => {
    seed([makeWin({ id: "g1", app: "tp:ghost", x: 30, y: 40, w: 600, h: 400 })]);
    saveLayoutSnapshot("ghost");
    seed([]);
    expect(applyLayoutSnapshot("ghost")).toBe(true);
    const wins = vwmStore.getState().wins;
    expect(wins).toHaveLength(1);
    expect(wins[0]!.app).toBe("tp:ghost");
    expect(wins[0]!.x).toBe(30);
    expect(wins[0]!.y).toBe(40);
    expect(wins[0]!.w).toBe(820); // clampRect 最小宽
    expect(wins[0]!.h).toBe(540);
  });

  it("删除与未知名字", () => {
    seed([makeWin({ id: "d1" })]);
    saveLayoutSnapshot("tmp");
    deleteLayoutSnapshot("tmp");
    expect(listLayoutSnapshots()).toEqual([]);
    expect(applyLayoutSnapshot("tmp")).toBe(false);
    expect(applyLayoutSnapshot("nope")).toBe(false);
  });

  it("同名覆盖原地更新；上限 20 份按插入序淘汰最旧", () => {
    seed([makeWin({ id: "o1", x: 1 })]);
    saveLayoutSnapshot("dup");
    seed([makeWin({ id: "o1", x: 1 }), makeWin({ id: "o2", x: 2 })]);
    saveLayoutSnapshot("dup");
    expect(listLayoutSnapshots()).toEqual([{ name: "dup", count: 2 }]);
    for (let i = 0; i < 20; i++) {
      seed([makeWin({ id: "c" + i })]);
      saveLayoutSnapshot("cap" + i);
    }
    const names = listLayoutSnapshots().map((e) => e.name);
    expect(names).toHaveLength(20);
    expect(names[0]).toBe("cap0");
    expect(names).not.toContain("dup");
  });
});

describe("Z-37 关闭几何记忆", () => {
  it("settle 持久化 + restoreGeomFor 命中", () => {
    const id = openVwmTpNew("tp:mem");
    moveVwmWin(id, 300, 200);
    settleVwmWin(id);
    expect(restoreGeomFor("tp:mem", wa)).toEqual({ x: 300, y: 200, w: 1180, h: 760 });
  });

  it("normal 态关闭也持久化（增强）", async () => {
    const id = openVwmTpNew("tp:closemem");
    moveVwmWin(id, 320, 240);
    closeVwmWin(id);
    await new Promise((r) => setTimeout(r, 200));
    expect(vwmStore.getState().wins.find((w) => w.id === id)).toBeUndefined();
    expect(restoreGeomFor("tp:closemem", wa)).toEqual({ x: 320, y: 240, w: 1180, h: 760 });
  });

  it("max 态关闭不持久化", async () => {
    const id = openVwmTpNew("tp:maxclose");
    toggleMaxVwmWin(id);
    closeVwmWin(id);
    await new Promise((r) => setTimeout(r, 200));
    expect(restoreGeomFor("tp:maxclose", wa)).toBe(null);
  });

  it("完全出屏的记忆 → null（调用方居中）", () => {
    localStorage.setItem(
      "variable:vwm:geom:v2",
      JSON.stringify({ calc: { x: 5000, y: -3000, w: 1180, h: 760 } }),
    );
    expect(restoreGeomFor("calc", wa)).toBe(null);
  });
});

describe("M-08 cycleVwmFocusFiltered", () => {
  it("过滤集为空 → false", () => {
    expect(cycleVwmFocusFiltered()).toBe(false);
    openVwmTpNew("tp:only");
    expect(cycleVwmFocusFiltered({ byApp: "tp:nope" })).toBe(false);
  });

  it("按应用轮转 / 反向轮转", () => {
    const a1 = openVwmTpNew("tp:dup");
    const a2 = openVwmTpNew("tp:dup");
    openVwmTpNew("tp:other");
    expect(cycleVwmFocusFiltered({ byApp: "tp:dup" })).toBe(true);
    expect(vwmStore.getState().focusedId).toBe(a2);
    expect(cycleVwmFocusFiltered({ byApp: "tp:dup" })).toBe(true);
    expect(vwmStore.getState().focusedId).toBe(a1);
    expect(cycleVwmFocusFiltered({ byApp: "tp:dup", backward: true })).toBe(true);
    expect(vwmStore.getState().focusedId).toBe(a2);
  });

  it("无过滤参数时在全部未最小化窗口间轮转", () => {
    const a = openVwmTpNew("tp:x1");
    openVwmTpNew("tp:x2");
    expect(cycleVwmFocusFiltered()).toBe(true);
    expect(vwmStore.getState().focusedId).toBe(a);
  });
});

describe("Z-42/M-05 ferryVwmWin 跨屏摆渡", () => {
  const g = globalThis as unknown as { window?: Record<string, unknown> };

  it("屏幕信息不可用 → false", () => {
    const id = openVwmTpNew("tp:ferry");
    delete g.window!.getScreenDetails;
    expect(ferryVwmWin(id, "right")).toBe(false);
  });

  it("有邻屏 → 平移并钳制进邻屏范围", () => {
    const id = openVwmTpNew("tp:ferry2");
    g.window!.getScreenDetails = () => ({
      currentScreen: { left: 0, top: 0 },
      screens: [
        { left: 0, top: 0, width: 1920, height: 1080 },
        { left: 1920, top: 0, width: 1920, height: 1080 },
      ],
    });
    try {
      expect(ferryVwmWin(id, "right")).toBe(true);
      expect(winOf(id).x).toBe(2290); // 370 + 1920，未触及钳制边界
    } finally {
      delete g.window!.getScreenDetails;
    }
  });
});
