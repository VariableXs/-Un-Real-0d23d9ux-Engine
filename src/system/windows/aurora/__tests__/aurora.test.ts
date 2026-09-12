import { describe, expect, it } from "vitest";
import { AURORA_WINDOW_SPACE_FAMILIES, AURORA_WINDOW_SPACE_ITEMS } from "../catalog";
import { snapCandidate, type FullSnapCtx } from "../snap";
import { tileLayout, type FullTileCtx } from "../layout";
import {
  MICROFEEL_PRESETS,
  MOTION_PRESETS,
  TITLEBAR_PRESETS,
  EDGE_PRESETS,
  SHADOW_PRESETS,
  MATERIAL_PRESETS,
  applyVisual,
} from "../visual";
import {
  GESTURE_BINDINGS,
  FOCUS_POLICIES,
  allowFocusSteal,
  attentionScore,
  vdeskReduce,
  groupReduce,
  dockReduce,
  presentReduce,
  VDESK_INITIAL,
  GROUP_INITIAL,
  DOCK_INITIAL,
  PRESENT_INITIAL,
} from "../behavior";
import {
  MONITOR_PRESETS,
  PERF_PRESETS,
  degradeLadder,
  mapCursorY,
  ultrawideThirds,
  saveSession,
  restoreSession,
  ghostWins,
  recallOffscreen,
} from "../session";
import {
  CANVAS_INITIAL,
  canvasBookmark,
  canvasSector,
  canvasZoom,
  formatCountdown,
  moonPhase,
  physStep,
  mapProject,
  mapUnproject,
  radarPolar,
  clutterScore,
  organize,
  screenToWorld,
  shichen,
  sunTimes,
  worldToScreen,
  PHYS_MODES,
} from "../spatial";
import { INSPECT_PRESETS, REACH_PRESETS, XR_CAPABILITIES, xrAvailable } from "../labs";
import { applyFamilySelection, effectiveSelections, defaultSelection } from "../engine";

const work = { x: 0, y: 0, w: 1600, h: 900 };
const screen = { x: 0, y: 0, w: 1600, h: 940 };
const baseCtx: FullSnapCtx = {
  drag: { x: 10, y: 10, w: 400, h: 300 },
  work,
  screen,
  neighbor: { x: 800, y: 0, w: 400, h: 300 },
};

describe("AURORA-10000 领域02 catalog（AI-06~AI-10）", () => {
  it("25 族 × 25 项 = 625 项", () => {
    expect(AURORA_WINDOW_SPACE_FAMILIES).toHaveLength(25);
    expect(AURORA_WINDOW_SPACE_ITEMS).toHaveLength(625);
    for (const fam of AURORA_WINDOW_SPACE_FAMILIES) expect(fam.items).toHaveLength(25);
  });

  it("ID 唯一且连续覆盖 F00626~F01250", () => {
    const ids = AURORA_WINDOW_SPACE_ITEMS.map((i) => parseInt(i.id.slice(1), 10));
    expect(new Set(ids).size).toBe(625);
    expect(Math.min(...ids)).toBe(626);
    expect(Math.max(...ids)).toBe(1250);
    const sorted = [...ids].sort((a, b) => a - b);
    for (let k = 0; k < 625; k++) expect(sorted[k]).toBe(626 + k);
  });

  it("族号连续 26~50 且归属齐全", () => {
    expect(AURORA_WINDOW_SPACE_FAMILIES.map((f) => f.fam)).toEqual(
      Array.from({ length: 25 }, (_, k) => 26 + k),
    );
    for (const fam of AURORA_WINDOW_SPACE_FAMILIES) {
      expect(fam.owner.length).toBeGreaterThan(0);
      expect(fam.kind.length).toBeGreaterThan(0);
    }
  });
});

describe("族0026 吸附系统", () => {
  it("F00626 屏幕边缘 → 半屏", () => {
    const r = snapCandidate("F00626", baseCtx);
    expect(r).not.toBeNull();
    expect(r!.rect).toEqual({ x: 0, y: 0, w: 800, h: 900 });
  });
  it("F00635 黄金比例 0.618", () => {
    const r = snapCandidate("F00635", baseCtx);
    expect(r!.rect.w).toBe(Math.round(1600 * 0.618));
  });
  it("F00630 四角四分", () => {
    const r = snapCandidate("F00630", { ...baseCtx, drag: { x: 0, y: 0, w: 100, h: 80 }, threshold: 24 });
    expect(r!.rect).toEqual({ x: 0, y: 0, w: 800, h: 450 });
  });
  it("F00648 Alt 按住时禁用吸附", () => {
    expect(snapCandidate("F00648", { ...baseCtx, altDown: true })).toBeNull();
  });
  it("F00646 提示线返回对齐参考", () => {
    const r = snapCandidate("F00646", baseCtx);
    expect(r!.guides!.x.length).toBeGreaterThanOrEqual(3);
  });
  it("未匹配 ID 返回 null", () => {
    expect(snapCandidate("F99999", baseCtx)).toBeNull();
  });
});

describe("族0027 布局引擎", () => {
  const tileCtx: FullTileCtx = { work, n: 5, focus: 1, current: [] };
  it("全部 25 种布局返回 5 个在工作区内的矩形", () => {
    for (let f = 51; f <= 75; f++) {
      const id = `F00${f}`;
      const rects = tileLayout(id, tileCtx);
      expect(rects).toHaveLength(5);
      for (const rc of rects) {
        expect(rc.w).toBeGreaterThan(0);
        expect(rc.h).toBeGreaterThan(0);
      }
    }
  });
  it("F00653 主副列主窗约 62%", () => {
    const rects = tileLayout("F00653", { work, n: 4, focus: 0 });
    expect(rects[0]!.w).toBe(Math.round(1600 * 0.62));
  });
  it("F00674 临时全屏焦点窗占满", () => {
    const rects = tileLayout("F00674", { work, n: 3, focus: 2 });
    expect(rects[2]).toEqual({ x: 0, y: 0, w: 1600, h: 900 });
  });
});

describe("视觉参数族（0028/0046~0050）", () => {
  it("六族全部有 25 档且 applyVisual 有输出", () => {
    const tables = [MOTION_PRESETS, TITLEBAR_PRESETS, EDGE_PRESETS, SHADOW_PRESETS, MATERIAL_PRESETS, MICROFEEL_PRESETS];
    for (const t of tables) expect(Object.keys(t)).toHaveLength(25);
    for (const fam of [28, 46, 47, 48, 49, 50]) {
      const vars = applyVisual(fam, `F${String(fam === 28 ? 676 : fam === 50 ? 876 : fam === 46 ? 1151 : fam === 47 ? 1176 : fam === 48 ? 1201 : 1226).padStart(5, "0")}`);
      expect(Object.keys(vars).length).toBeGreaterThan(0);
    }
  });
  it("F00700 降级静态时长为 0", () => {
    expect(MOTION_PRESETS.F00700!.enter).toBe(0);
  });
  it("F01216 性能档阴影全关", () => {
    expect(SHADOW_PRESETS.F01216!.opacity).toBe(0);
    expect(applyVisual(48, "F01216")["--aurora-shadow-blur"]).toBe("0px");
  });
});

describe("族0029/0033/0031/0032/0041/0044 行为层", () => {
  it("25 种手势绑定", () => expect(GESTURE_BINDINGS).toHaveLength(25));
  it("25 种焦点档且防抢策略生效", () => {
    expect(FOCUS_POLICIES).toHaveLength(25);
    const anti = FOCUS_POLICIES.find((p) => p.id === "F00803")!;
    expect(allowFocusSteal(anti, { app: "game", topmost: true, urgent: false })).toBe(false);
    expect(allowFocusSteal(anti, { app: "pip", topmost: true, urgent: false })).toBe(false);
  });
  it("注意力评分边界", () => {
    expect(attentionScore(0, 0)).toBe(100);
    expect(attentionScore(100, 100)).toBe(0);
  });
  it("虚拟桌面：创建/上限/固定/关闭", () => {
    let s = VDESK_INITIAL;
    for (let i = 0; i < 12; i++) s = vdeskReduce(s, { t: "create" });
    expect(s.desks.length).toBe(9);
    s = vdeskReduce(s, { t: "pin", id: s.active, app: "write" });
    expect(s.desks.find((d) => d.id === s.active)!.pinned).toContain("write");
    const after = vdeskReduce(s, { t: "close", id: s.active });
    expect(after.desks.length).toBe(8);
  });
  it("虚拟桌面模板", () => {
    const s = vdeskReduce(VDESK_INITIAL, { t: "create", template: "study" });
    expect(s.desks.at(-1)!.name).toBe("学习");
    expect(s.desks.at(-1)!.pinned).toEqual(["write", "mind"]);
  });
  it("分组：成组/加入/拆分", () => {
    let g = groupReduce(GROUP_INITIAL, { t: "make", win: "w1" });
    g = groupReduce(g, { t: "join", win: "w2", group: g.groups[0]!.id });
    expect(g.member["w2"]).toBe(g.groups[0]!.id);
    g = groupReduce(g, { t: "leave", win: "w2" });
    expect(g.groups).toHaveLength(1);
    expect(g.member["w2"]).toBeUndefined();
  });
  it("收纳坞：容量与置顶", () => {
    let d = dockReduce({ ...DOCK_INITIAL, capacity: 2 }, { t: "add", win: "a" });
    d = dockReduce(d, { t: "add", win: "b" });
    d = dockReduce(d, { t: "add", win: "c" });
    expect(d.items).toHaveLength(2);
    d = dockReduce(d, { t: "pin", win: "b", pinned: true });
    d = dockReduce(d, { t: "sort", by: "pinned" });
    expect(d.items[0]!.win).toBe("b");
    d = dockReduce(d, { t: "clear" });
    expect(d.items.map((i) => i.win)).toEqual(["b"]);
  });
  it("放映：进入/步进/退出", () => {
    let p = presentReduce(PRESENT_INITIAL, { t: "enter", steps: [{ win: "a", note: "" }, { win: "b", note: "" }] }, 1000);
    expect(p.active).toBe(true);
    p = presentReduce(p, { t: "next" }, 2000);
    expect(p.index).toBe(1);
    p = presentReduce(p, { t: "next" }, 3000);
    expect(p.index).toBe(1);
    p = presentReduce(p, { t: "exit" }, 4000);
    expect(p.active).toBe(false);
  });
});

describe("族0030/0034/0035 会话层", () => {
  it("25 种多屏档 + 带鱼三分 + 光标对齐", () => {
    expect(MONITOR_PRESETS).toHaveLength(25);
    const thirds = ultrawideThirds({ x: 0, y: 0, w: 3000, h: 1000 });
    expect(thirds.reduce((a, t) => a + t.w, 0)).toBe(3000);
    expect(mapCursorY(50, { x: 0, y: 0, w: 100, h: 100 }, { x: 100, y: 0, w: 100, h: 200 })).toBe(100);
  });
  it("持久化：保存/恢复/幽灵清理/离屏召回", () => {
    const wins = [
      { win: "a", rect: { x: 10, y: 10, w: 400, h: 300 }, minimized: false, maximized: false, vdesk: 0, group: null },
      { win: "ghost", rect: { x: -800, y: 10, w: 400, h: 300 }, minimized: false, maximized: false, vdesk: 0, group: null },
    ];
    const snap = saveSession(wins, 2, 0, 1234);
    expect(snap.version).toBe(1);
    // 恢复前：幽灵窗可被检出
    expect(ghostWins(snap.wins, work)).toEqual(["ghost"]);
    const recalled = recallOffscreen(snap.wins, work);
    expect(ghostWins(recalled, work)).toEqual([]);
    // 恢复：拓扑变化自愈夹取回工作区
    const restored = restoreSession(snap, work);
    expect(restored).toHaveLength(2);
    for (const w of restored) {
      expect(w.rect.x).toBeGreaterThanOrEqual(work.x);
      expect(w.rect.y).toBeGreaterThanOrEqual(work.y);
    }
  });
  it("25 种性能档 + 降级阶梯单调", () => {
    expect(PERF_PRESETS).toHaveLength(25);
    expect(degradeLadder(0.01, 24)).toEqual([]);
    expect(degradeLadder(0.2, 24)).toEqual(["blur", "transparency", "shadow", "animation"]);
  });
});

describe("族0036/0037/0038/0039/0043 空间层", () => {
  it("画布变换往返", () => {
    const z = canvasZoom(CANVAS_INITIAL, 2, 100, 100);
    const w = screenToWorld(z, 100, 100);
    const back = worldToScreen(z, w.x, w.y);
    expect(back.x).toBeCloseTo(100);
    expect(back.y).toBeCloseTo(100);
    const b = canvasBookmark(z, { t: "add", name: "home" });
    expect(b.bookmarks[0]!.name).toBe("home");
    expect(canvasSector(3000, 100).col).toBe(1);
  });
  it("物理：重力档最终落在底边内", () => {
    const mode = PHYS_MODES.F00926!;
    let bodies = [{ id: "w", rect: { x: 100, y: 0, w: 200, h: 150 }, vx: 0, vy: 0, hue: 1 }];
    for (let i = 0; i < 500; i++) bodies = physStep(mode, bodies, work);
    const b = bodies[0]!;
    expect(b.rect.y).toBeLessThanOrEqual(work.h - b.rect.h + 1);
    expect(b.rect.x).toBeGreaterThanOrEqual(work.x);
  });
  it("小地图映射往返 + 雷达极坐标", () => {
    const spec = { size: 160, span: 3200, mode: "rect" as const, opacity: 0.8, autoHide: true };
    const world = { x: 0, y: 0, w: 3200, h: 1800 };
    const p = mapProject(world, spec, 800, 450);
    const q = mapUnproject(world, spec, p.x, p.y);
    expect(q).toEqual({ x: 800, y: 450 });
    const radar = radarPolar(spec, 1600, 900);
    expect(radar.deg).toBe(Math.round(((Math.atan2(900, 1600) * 180) / Math.PI + 360) % 360));
  });
  it("节律：日出日落/时辰/月相/倒计时", () => {
    const st = sunTimes(31.2, 172, (120 - 121.47) / 15); // 上海夏至附近
    expect(st.sunrise).toBeGreaterThan(3);
    expect(st.sunrise).toBeLessThan(st.sunset);
    expect(st.sunset).toBeLessThan(24);
    expect(shichen(23)).toBe("子");
    expect(shichen(14)).toBe("未");
    const ph = moonPhase(new Date("2026-09-13T00:00:00Z"));
    expect(ph).toBeGreaterThanOrEqual(0);
    expect(ph).toBeLessThan(1);
    expect(formatCountdown(90061000)).toBe("1天 01:01:01");
    expect(formatCountdown(-1)).toBe("已到达");
  });
  it("整理：策略排序与整洁评分", () => {
    const wins = [
      { id: "a", app: "write", rect: { x: 0, y: 0, w: 800, h: 450 }, lastUsed: 3, freq: 1 },
      { id: "b", app: "mind", rect: { x: 100, y: 100, w: 800, h: 450 }, lastUsed: 9, freq: 9 },
      { id: "c", app: "calc", rect: { x: 900, y: 500, w: 700, h: 400 }, lastUsed: 1, freq: 3 },
    ];
    const rects = organize("byFreq", wins, work);
    expect(rects).toHaveLength(3);
    for (const rc of rects) {
      expect(rc.x).toBeGreaterThanOrEqual(work.x);
      expect(rc.w).toBeGreaterThan(0);
    }
    expect(clutterScore([wins[0]!])).toBe(100);
    expect(clutterScore(wins)).toBeLessThan(100);
  });
});

describe("族0040/0042/0045 实验与可达层", () => {
  it("25 种可达档且目标 ≥44px", () => {
    expect(REACH_PRESETS).toHaveLength(25);
    for (const p of REACH_PRESETS) expect(p.minTarget).toBeGreaterThanOrEqual(44);
  });
  it("25 种信息档", () => expect(INSPECT_PRESETS).toHaveLength(25));
  it("XR：25 项能力全部默认关 + 无硬件隐藏", () => {
    expect(XR_CAPABILITIES).toHaveLength(25);
    for (const c of XR_CAPABILITIES) expect(c.defaultOn).toBe(false);
    const avail = xrAvailable(XR_CAPABILITIES, []);
    expect(avail.every((c) => !c.needsHardware)).toBe(true);
    expect(avail.length).toBeLessThan(25);
  });
});

describe("总控 engine", () => {
  it("缺省回退首项且非法选择被回退", () => {
    const fam = AURORA_WINDOW_SPACE_FAMILIES[0]!;
    expect(defaultSelection(fam)).toBe(fam.items[0]!.id);
    const sel = effectiveSelections({ 26: "F99999", 27: "F00653" });
    expect(sel[26]).toBe(fam.items[0]!.id);
    expect(sel[27]).toBe("F00653");
  });
  it("每族应用都产生登记变量", () => {
    for (const fam of AURORA_WINDOW_SPACE_FAMILIES) {
      const vars = applyFamilySelection(fam.fam, fam.items[0]!.id);
      expect(vars[`--aurora-ws-f${fam.fam}`]).toBe(fam.items[0]!.id);
    }
  });
});
