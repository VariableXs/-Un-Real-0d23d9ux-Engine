import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  AUTO_DEBOUNCE_MS,
  TIMELINE_CAP,
  cancelAutoSnap,
  clearTimeline,
  listSnaps,
  planRestore,
  pushSnap,
  restoreTargets,
  snapshotNow,
  scheduleAutoSnap,
  takeSnap,
} from "../timeline";
import type { TimelineSnap } from "../timeline";
import type { VwmWin } from "../vwm";

function win(id: string, x = 0, y = 0, z = 1): VwmWin {
  return {
    id,
    app: "write",
    path: null,
    x,
    y,
    w: 800,
    h: 600,
    state: "normal",
    minimized: false,
    z,
    restore: null,
    group: null,
    groupActive: false,
    rolledUp: false,
    minimizedAt: null,
    opacity: 1,
    topmost: false,
  };
}

beforeEach(() => {
  localStorage.clear();
  cancelAutoSnap();
});

describe("N-01 快照", () => {
  it("快照含 ts/名称/全部窗口几何与 z 序", () => {
    const snap = takeSnap([win("a", 10, 20, 3)], "写作现场", 12345);
    expect(snap.ts).toBe(12345);
    expect(snap.name).toBe("写作现场");
    expect(snap.vwm[0]).toMatchObject({ id: "a", x: 10, y: 20, z: 3 });
  });

  it("快照损坏时 UI 报错不影响其余快照（跳过坏条目）", () => {
    const good = takeSnap([win("a")], undefined, 1000);
    pushSnap(good);
    localStorage.setItem(
      "variable:vwm:timeline",
      JSON.stringify([{ broken: true }, good, "junk", null]),
    );
    const all = listSnaps();
    expect(all).toHaveLength(1);
    expect(all[0]!.ts).toBe(1000);
  });

  it("保留策略：超上限 FIFO 淘汰", () => {
    for (let i = 0; i < TIMELINE_CAP + 5; i++) {
      pushSnap(takeSnap([win(`a${i}`)], undefined, i));
    }
    const all = listSnaps();
    expect(all).toHaveLength(TIMELINE_CAP);
    expect(all.some((s) => s.name === undefined && s.ts === 0)).toBe(false); // 最旧的被淘汰
    expect(all[0]!.ts).toBe(TIMELINE_CAP + 4); // 最新保留
  });

  it("自动快照去抖 8s：期间多次触发只落一张", () => {
    vi.useFakeTimers();
    const base = [win("a")];
    snapshotNow(base);
    pushSnap(takeSnap(base, undefined, 2));
    // 去抖 8s：期间只落一张
    scheduleAutoSnap(base);
    vi.advanceTimersByTime(AUTO_DEBOUNCE_MS - 1);
    expect(listSnaps().length).toBe(2);
    vi.advanceTimersByTime(1);
    expect(listSnaps().length).toBe(3);
    vi.useRealTimers();
  });
});

describe("N-01 恢复三级策略", () => {
  const snap: TimelineSnap = {
    ts: 1,
    vwm: [win("alive", 100, 100, 5), { ...win("dead", 0, 0, 2), app: "code" }],
  };

  it("存活窗口走精确恢复（仅重摆位置）；死亡窗口如实报告 missing", () => {
    const plan = planRestore(snap, [win("alive"), win("other")]);
    expect(plan.exact.map((w) => w.id)).toEqual(["alive"]);
    expect(plan.missing).toEqual(["dead"]);
  });

  it("restoreTargets 给出 id → 几何映射（与快照逐项一致）", () => {
    const plan = planRestore(snap, [win("alive")]);
    const t = restoreTargets(plan);
    expect(t["alive"]).toEqual({ x: 100, y: 100, w: 800, h: 600, z: 5 });
  });

  it("清空时间线返回清除条数", () => {
    pushSnap(takeSnap([win("a")]));
    expect(clearTimeline()).toBe(1);
    expect(listSnaps()).toHaveLength(0);
  });
});

