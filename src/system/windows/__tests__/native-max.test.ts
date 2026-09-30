import { describe, expect, it, beforeAll, beforeEach } from "vitest";
import { nativeMaximizeVwm, nativeMinimizeVwm, openVwmTpNew, setVwmWorkArea, vwmStore } from "../vwm";

/**
 * M5：原生 □ 最大化/还原同步（embed://native-max → nativeMaximizeVwm）。
 * 真值是原生窗口：只在状态翻转时改写；最大化快照 restore（布局快照/贴靠依据），
 * 还原写回实测几何并清快照。不走 toggleMaxVwmWin（那是自有窗口语义）。
 */

const wa = { x: 0, y: 0, w: 1920, h: 1046 };

function seedGeom(id: string) {
  vwmStore.setState({
    wins: vwmStore.getState().wins.map((w) => (w.id === id ? { ...w, x: 100, y: 80, w: 800, h: 600 } : w)),
  });
}

describe("nativeMaximizeVwm（M5 最大化同步）", () => {
  beforeAll(() => {
    (globalThis as { window?: unknown }).window ??= { setTimeout };
  });
  beforeEach(() => {
    globalThis.localStorage?.clear?.();
    vwmStore.setState({ wins: [], focusedId: null, topZ: 10 });
    setVwmWorkArea(wa);
  });

  it("最大化：state→max、restore 快照原几何、几何=实测最大化矩形", () => {
    const id = openVwmTpNew("tp:notepad");
    seedGeom(id);
    nativeMaximizeVwm(id, true, { x: 0, y: 0, w: 1920, h: 1040 });
    const w = vwmStore.getState().wins.find((x) => x.id === id)!;
    expect(w.state).toBe("max");
    expect(w.restore).toEqual({ x: 100, y: 80, w: 800, h: 600 });
    expect(w.x).toBe(0);
    expect(w.w).toBe(1920);
  });

  it("还原：state→normal、几何=实测还原矩形、restore 清空", () => {
    const id = openVwmTpNew("tp:notepad");
    seedGeom(id);
    nativeMaximizeVwm(id, true, { x: 0, y: 0, w: 1920, h: 1040 });
    nativeMaximizeVwm(id, false, { x: 120, y: 90, w: 780, h: 580 });
    const w = vwmStore.getState().wins.find((x) => x.id === id)!;
    expect(w.state).toBe("normal");
    expect(w.restore).toBeNull();
    expect([w.x, w.y, w.w, w.h]).toEqual([120, 90, 780, 580]);
  });

  it("幂等：重复同向广播不改写（防事件风暴/重嵌回环）", () => {
    const id = openVwmTpNew("tp:notepad");
    seedGeom(id);
    nativeMaximizeVwm(id, true, { x: 0, y: 0, w: 1920, h: 1040 });
    const restoreSnap = vwmStore.getState().wins.find((x) => x.id === id)!.restore;
    // 事件重复（maximized=true 再次到达）→ restore 不被覆盖成 max 几何
    nativeMaximizeVwm(id, true, { x: 0, y: 0, w: 1920, h: 1040 });
    expect(vwmStore.getState().wins.find((x) => x.id === id)!.restore).toEqual(restoreSnap);
    // max 态收到 maximized=false 之外的意外次序（先 false 后 true 不可能，但
    // normal 态收 false 必须无操作）
    const id2 = openVwmTpNew("tp:calc");
    nativeMaximizeVwm(id2, false, { x: 5, y: 5, w: 100, h: 100 });
    expect(vwmStore.getState().wins.find((x) => x.id === id2)!.state).toBe("normal");
  });

  it("最小化协同：最大化窗口再最小化，恢复后 state 仍为 max（IsZoomed 恒真 → 后端不发翻转）", () => {
    const id = openVwmTpNew("tp:notepad");
    seedGeom(id);
    nativeMaximizeVwm(id, true, { x: 0, y: 0, w: 1920, h: 1040 });
    nativeMinimizeVwm(id, true);
    expect(vwmStore.getState().wins.find((x) => x.id === id)!.minimized).toBe(true);
    nativeMinimizeVwm(id, false);
    const w = vwmStore.getState().wins.find((x) => x.id === id)!;
    expect(w.minimized).toBe(false);
    expect(w.state).toBe("max");
    expect(w.restore).toEqual({ x: 100, y: 80, w: 800, h: 600 });
  });

  it("iconic 矩形防御：还原广播携带最小化动画矩形（<160×100）时丢弃，不污染 state/快照", () => {
    // 实机取证：Win11 记事本最小化动画产生「还原(185×27)→最大化」翻转对，
    // 后端 IsIconic 滤一层，前端对同款矩形兜底（is_adoptable 同阈值）。
    const id = openVwmTpNew("tp:notepad");
    seedGeom(id);
    nativeMaximizeVwm(id, true, { x: 0, y: 0, w: 1920, h: 1040 });
    nativeMaximizeVwm(id, false, { x: 7, y: 1046, w: 185, h: 27 });
    const w = vwmStore.getState().wins.find((x) => x.id === id)!;
    expect(w.state).toBe("max");
    expect(w.restore).toEqual({ x: 100, y: 80, w: 800, h: 600 });
    expect([w.x, w.w]).toEqual([0, 1920]);
  });
});
