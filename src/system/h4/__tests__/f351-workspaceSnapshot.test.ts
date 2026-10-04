import { describe, expect, it } from "vitest";
import { memStore, __clearMem } from "../internal/store";
import {
  SNAPSHOT_CAP,
  deleteSnapshot,
  displaysMatch,
  listSnapshots,
  mapRect,
  planRestore,
  saveSnapshot,
  validateSnapshot,
  type OpenWindowRef,
  type SnapshotWindow,
  type WorkspaceSnapshot,
} from "../f351-workspaceSnapshot";

function store() {
  __clearMem();
  return memStore();
}

function win(appId: string, over: Partial<SnapshotWindow> = {}): SnapshotWindow {
  return { appId, title: null, x: 100, y: 100, w: 800, h: 600, vdesk: 0, display: 0, minimized: false, z: 1, ...over };
}

const DISP_1080 = ["1920x1080@1.00"];
const DISP_4K = ["3840x2160@2.00"];

describe("F351 工作区快照", () => {
  it("保存-读取 round-trip：名称/窗口/显示器签名全量保真", () => {
    const s = store();
    const r = saveSnapshot("写稿模式", [win("notepad", { z: 2 }), win("explorer", { z: 1 })], DISP_1080, 1000, s);
    expect(r.ok).toBe(true);
    const all = listSnapshots(s);
    expect(all).toHaveLength(1);
    expect(all[0]!.name).toBe("写稿模式");
    expect(all[0]!.windows).toHaveLength(2);
    expect(all[0]!.displays).toEqual(DISP_1080);
  });

  it("上限 10：第 11 份淘汰最早创建的（同名覆盖不占额、不触发淘汰）", () => {
    const s = store();
    for (let i = 0; i < SNAPSHOT_CAP; i++) saveSnapshot(`快照${i}`, [win("a")], DISP_1080, i + 1, s);
    saveSnapshot("快照0", [win("a")], DISP_1080, 999, s); // 同名覆盖
    expect(listSnapshots(s)).toHaveLength(SNAPSHOT_CAP);
    const r = saveSnapshot("新快照", [win("a")], DISP_1080, 1000, s);
    expect(r.evicted).toBe("快照1"); // 快照0 已被刷新 createdAt=999，最老变成快照1
    expect(listSnapshots(s)).toHaveLength(SNAPSHOT_CAP);
  });

  it("恢复计划（精确模式）：几何逐字段一致（<1px 判据即 0 漂移）", () => {
    const snap: WorkspaceSnapshot = {
      name: "s",
      createdAt: 1,
      displays: DISP_1080,
      windows: [win("note", { x: 10, y: 20, w: 640, h: 480, minimized: true, z: 5 })],
    };
    const open: OpenWindowRef[] = [{ appId: "note", title: null, currentRect: { x: 0, y: 0, w: 100, h: 100 }, vdesk: 0, minimized: false }];
    const plan = planRestore(snap, open, [{ w: 1920, h: 1080 }]);
    expect(plan.mode).toBe("exact");
    expect(plan.maxDriftPx).toBe(0);
    expect(plan.entries[0]!.rect).toEqual({ x: 10, y: 20, w: 640, h: 480 });
    expect(plan.entries[0]!.minimized).toBe(true); // 最小化态还原
    expect(plan.entries[0]!.matched).toBe(true);
  });

  it("未开应用进启动队列，且按快照 Z 序降序排队（前排先起）", () => {
    const snap: WorkspaceSnapshot = {
      name: "s",
      createdAt: 1,
      displays: DISP_1080,
      windows: [win("a", { z: 1 }), win("b", { z: 9 }), win("c", { z: 5 })],
    };
    const plan = planRestore(snap, [], [{ w: 1920, h: 1080 }]);
    expect(plan.launchQueue).toEqual(["b", "c", "a"]);
  });

  it("异签名显示器走比例映射（4K 快照恢复到 1080p）", () => {
    const snap: WorkspaceSnapshot = {
      name: "s",
      createdAt: 1,
      displays: DISP_4K,
      windows: [win("a", { x: 1000, y: 500, w: 2000, h: 1500 })],
    };
    const plan = planRestore(snap, [], [{ w: 1920, h: 1080 }]);
    expect(plan.mode).toBe("scaled");
    expect(plan.entries[0]!.rect).toEqual({ x: 500, y: 250, w: 1000, h: 750 });
  });

  it("mapRect 同签名恒等；异尺寸比例换算且最小 1px 钳制", () => {
    const r = { x: 12, y: 34, w: 56, h: 78 };
    expect(mapRect(r, { w: 1920, h: 1080 }, { w: 1920, h: 1080 })).toEqual(r);
    const tiny = mapRect({ x: 0, y: 0, w: 1, h: 1 }, { w: 4000, h: 2000 }, { w: 40, h: 20 });
    expect(tiny.w).toBeGreaterThanOrEqual(1);
    expect(tiny.h).toBeGreaterThanOrEqual(1);
  });

  it("同名覆盖保留快照身份；删除不存在的快照返回 false", () => {
    const s = store();
    saveSnapshot("m", [win("a")], DISP_1080, 1, s);
    saveSnapshot("m", [win("b")], DISP_1080, 2, s);
    expect(listSnapshots(s)).toHaveLength(1);
    expect(listSnapshots(s)[0]!.windows[0]!.appId).toBe("b");
    expect(deleteSnapshot("不存在", s)).toBe(false);
    expect(deleteSnapshot("m", s)).toBe(true);
    expect(listSnapshots(s)).toHaveLength(0);
  });

  it("validateSnapshot：空名/空窗/非法几何/重复记录逐项报出", () => {
    const bad: WorkspaceSnapshot = {
      name: " ",
      createdAt: 0,
      displays: DISP_1080,
      windows: [win("a", { w: 0 }), win("a", { h: -1 })],
    };
    const problems = validateSnapshot(bad);
    expect(problems.some((p) => p.includes("名称"))).toBe(true);
    expect(problems.some((p) => p.includes("尺寸非法"))).toBe(true);
    expect(problems.some((p) => p.includes("重复"))).toBe(true);
    expect(validateSnapshot({ name: "ok", createdAt: 0, displays: DISP_1080, windows: [win("a")] })).toEqual([]);
  });

  it("displaysMatch 顺序敏感", () => {
    expect(displaysMatch(["a", "b"], ["a", "b"])).toBe(true);
    expect(displaysMatch(["a", "b"], ["b", "a"])).toBe(false);
  });
});
