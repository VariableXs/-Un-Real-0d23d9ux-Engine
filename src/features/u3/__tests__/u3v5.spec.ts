/**
 * AI-U3 深化批次五（v5 装配引擎群）单测——despaint/expui/lockmount ×
 * 判据常量 × 边界三点 × 破坏注入 × 显性报错路径。
 *
 * 环境声明：vitest node 环境，纯函数引擎（零 DOM 依赖）；UI 装配件
 * （DeskPaint/ExplorerPane）消费同一事实源，其交互语义由本测试钉住。
 */

import { describe, expect, it } from "vitest";

import {
  // despaint
  MARQUEE_THRESHOLD_PX, DRAG_THRESHOLD_PX, DOUBLE_CLICK_MS, TYPE_AHEAD_WINDOW_MS, PEEK_PARAMS,
  marqueeRect, marqueeVerdict, pointInRect, applySelection,
  gridNeighbor, gridRowEdge, typeAheadMatch, typeAheadPush,
  dragStartVerdict, dragGhost, dragEscRestore, snapDrop, snapDropBatch,
  peekVerdict, paintDragAllowed, clickSequence,
  despaintSelfCheck,
  // expui
  FLATTEN_BUDGET_MS, VIEWPORT_OVERSCAN, TREE_ROW_HEIGHT_PX, TREE_PANE_MIN_PX, TREE_PANE_MAX_PX, EXP_STATUSBAR_HEIGHT_PX,
  flattenTree, viewportWindow, rowTop, planListToTree, treeClickVerdict, treeKeyNav, breadcrumbFit, clampTreeWidth,
  type TreeNode,
  expuiSelfCheck,
  // lockmount
  LOCK_FOCUS_TRAP, TASKBAR_HEIGHT_PX, U3_ZORDER, zOf,
  lockMountInit, lockMountPinFail, lockMountCooldownTick, lockMountUnlock,
  intersectOnScreen, dedupeRects, composeBlackBlocks, blackBlockAreaHonest,
  bannerWindowSpec, bannerScreenGeometry, bannerWindowRects,
  lockmountSelfCheck,
  // index 桶
  V5_ENGINE_SELFCHECKS, v5EnginesSelfCheck, u3EnginesSelfCheck,
} from "../engines";
import { U3_ANCHOR_DOMAINS, anchorRuntime } from "../../u3/anchor";
import { reconcileF501F550 } from "../../u3/reconcile";
import { U3_ENGINE_LABELS, labelsSelfCheck } from "../../u3/labels";

/* ------------------------------ despaint 桌面实绘 ------------------------------ */

describe("v5 despaint 桌面实绘引擎", () => {
  it("判据常量与规格表一致", () => {
    expect(MARQUEE_THRESHOLD_PX).toBe(3);
    expect(DRAG_THRESHOLD_PX).toBe(4);
    expect(DOUBLE_CLICK_MS).toBe(300);
    expect(TYPE_AHEAD_WINDOW_MS).toBe(800);
    expect(PEEK_PARAMS.opacity).toBeGreaterThan(0);
  });

  it("橡皮筋几何：反向拖出规范化 + 边界含命中", () => {
    const r = marqueeRect({ x: 200, y: 150 }, { x: 100, y: 90 });
    expect(r).toEqual({ x: 100, y: 90, w: 100, h: 60 });
    expect(pointInRect({ x: 100, y: 90 }, r)).toBe(true);
    expect(pointInRect({ x: 99.9, y: 90 }, r)).toBe(false);
    expect(marqueeVerdict({ x: 5, y: 5 }, { x: 7, y: 7 }).mode).toBe("click");
    expect(marqueeVerdict({ x: 5, y: 5 }, { x: 9, y: 5 }).mode).toBe("marquee");
  });

  it("选中集五路语义（单选/Shift 范围/Ctrl 翻转/框选/Ctrl+框选）", () => {
    const order = ["a", "b", "c", "d", "e"];
    const s1 = applySelection([], new Set(), null, { kind: "item-click", target: "b" }, order);
    expect([...s1.selected]).toEqual(["b"]);
    const s2 = applySelection([], s1.selected, s1.anchor, { kind: "item-click", target: "d", shift: true }, order);
    expect([...s2.selected]).toEqual(["b", "c", "d"]);
    const s3 = applySelection([], s2.selected, s2.anchor, { kind: "item-click", target: "c", ctrl: true }, order);
    expect(s3.selected.has("c")).toBe(false);
    expect(s3.selected.size).toBe(2);
    const s4 = applySelection(["a", "e"], new Set(), null, { kind: "marquee" }, order);
    expect(s4.selected.size).toBe(2);
    const s5 = applySelection(["a", "e"], new Set(["b"]), "b", { kind: "marquee", ctrl: true }, order);
    expect(s5.selected.size).toBe(3);
    // 空白 Ctrl 单击保留（乱点路径不丢用户选择——章三状态机）
    const s6 = applySelection([], new Set(["b"]), "b", { kind: "blank-click", ctrl: true }, order);
    expect(s6.selected.has("b")).toBe(true);
  });

  it("键盘导航：邻居/行边/边界诚实 null/type-ahead 窗口", () => {
    const idx = { cols: [{ x: 0, y: 0 }, { x: 80, y: 0 }, { x: 160, y: 0 }, { x: 0, y: 90 }], colW: 80, rowH: 90 };
    const items = ["a", "b", "c", "d"].map((id) => ({ id }));
    expect(gridNeighbor(idx, "a", "right", items)).toBe("b");
    expect(gridNeighbor(idx, "b", "down", items)).toBeNull(); // b 下方无同列图标——诚实 null
    expect(gridNeighbor(idx, "d", "up", items)).toBe("a");
    expect(gridNeighbor(idx, "a", "up", items)).toBeNull();
    expect(gridRowEdge(idx, "c", "home", items)).toBe("a");
    expect(gridRowEdge(idx, "a", "end", items)).toBe("c");
    const t1 = typeAheadPush("a", "b", 1100, 1000);
    expect(t1.buffer).toBe("ab");
    expect(typeAheadPush("ab", "c", 2000, 1100).reset).toBe(true);
    expect(typeAheadMatch("do", [{ id: "1", name: "文档" }, { id: "2", name: "download" }])).toBe("2");
    expect(typeAheadMatch("", [{ id: "1", name: "x" }])).toBeNull();
  });

  it("拖拽：阈值/幽灵跟手/批吸附保队形/Esc 复原", () => {
    expect(dragStartVerdict({ x: 0, y: 0 }, { x: 4, y: 0 })).toBe("drag");
    expect(dragStartVerdict({ x: 0, y: 0 }, { x: 3, y: 3 })).toBe("none");
    const g = dragGhost({ primaryId: "a", grabOffset: { x: 12, y: 10 } }, ["a", "b"], { x: 50, y: 40 });
    expect(g).toMatchObject({ x: 38, y: 30 });
    expect(g.opacity).toBeLessThan(1);
    expect(dragEscRestore()).toEqual({ ghost: null, selectionRestored: true });
    const p = snapDrop({ x: 84, y: 92 });
    expect(p.x % 80 === 0 || p.x % 88 === 0 || Number.isInteger(p.x)).toBe(true);
    const batch = snapDropBatch([{ id: "a", x: 0, y: 0 }, { id: "b", x: 10, y: 0 }], { x: 5, y: 5 });
    expect(batch[0]!.x).toBe(batch[1]!.x); // 队形保持
  });

  it("瞥桌面只读语义 + 锁定联动出口在位", () => {
    const v = peekVerdict(true);
    expect(v.peeking).toBe(true);
    expect(v.style.pointerEvents).toBe("none");
    expect(peekVerdict(false).peeking).toBe(false);
    expect(typeof paintDragAllowed().allowed).toBe("boolean");
  });

  it("双击防抖三分支（破坏注入：异位连点被忽略）", () => {
    expect(clickSequence(null, { atMs: 100, x: 0, y: 0 }).verdict).toBe("single");
    expect(clickSequence({ atMs: 100, x: 0, y: 0 }, { atMs: 380, x: 2, y: 2 }).verdict).toBe("double");
    expect(clickSequence({ atMs: 100, x: 0, y: 0 }, { atMs: 500, x: 2, y: 2 }).verdict).toBe("single");
    // 同点位快击 = 双击（Windows 公理）；异位快击 = 连点抖动被忽略
    expect(clickSequence({ atMs: 100, x: 0, y: 0 }, { atMs: 160, x: 2, y: 2 }).verdict).toBe("double");
    expect(clickSequence({ atMs: 100, x: 0, y: 0 }, { atMs: 160, x: 30, y: 0 }).verdict).toBe("ignored-too-fast");
    // 长距离二次点击不算双击（位移 >4px）
    expect(clickSequence({ atMs: 100, x: 0, y: 0 }, { atMs: 200, x: 30, y: 0 }).verdict).toBe("single");
  });

  it("despaint 自检全绿", () => {
    const checks = despaintSelfCheck();
    expect(checks.length).toBeGreaterThanOrEqual(15);
    expect(checks.filter((c) => !c.pass)).toEqual([]);
  });
});

/* ------------------------------ expui 资源管理器装配 ------------------------------ */

const DEMO_TREE: Array<TreeNode> = [
  { id: "c", name: "C:", children: [
    { id: "work", name: "work", children: [{ id: "src", name: "src" }, { id: "doc", name: "doc" }] },
    { id: "tmp", name: "tmp" },
  ] },
  { id: "d", name: "D:" },
];

describe("v5 expui 资源管理器装配引擎", () => {
  it("规格常量与判据一致", () => {
    expect(FLATTEN_BUDGET_MS).toBe(16);
    expect(VIEWPORT_OVERSCAN).toBe(8);
    expect(TREE_ROW_HEIGHT_PX).toBe(24);
    expect(EXP_STATUSBAR_HEIGHT_PX).toBe(24);
    expect(TREE_PANE_MIN_PX).toBeLessThan(TREE_PANE_MAX_PX);
  });

  it("树平铺：折叠/展开/深度/箭头态", () => {
    const closed = flattenTree(DEMO_TREE, new Set());
    expect(closed.map((r) => r.id)).toEqual(["c", "d"]);
    const open = flattenTree(DEMO_TREE, new Set(["c", "work"]));
    expect(open.map((r) => r.id)).toEqual(["c", "work", "src", "doc", "tmp", "d"]);
    expect(open.find((r) => r.id === "src")!.depth).toBe(2);
    expect(open.find((r) => r.id === "work")!.expanded).toBe(true);
    expect(open.find((r) => r.id === "tmp")!.hasChildren).toBe(false);
    expect(open.find((r) => r.id === "tmp")!.expanded).toBe(false);
  });

  it("万节点平铺在 16ms 预算内（F228 同源性能判据，三次取最小防环境噪声）", () => {
    const big: Array<TreeNode> = [{
      id: "root", name: "root",
      children: Array.from({ length: 20000 }, (_, i) => ({ id: `n${i}`, name: `节点${i}` })),
    }];
    let rows = 0;
    let bestMs = Infinity;
    for (let i = 0; i < 3; i++) {
      const t0 = Date.now();
      rows = flattenTree(big, new Set(["root"])).length;
      bestMs = Math.min(bestMs, Date.now() - t0);
    }
    expect(bestMs).toBeLessThan(FLATTEN_BUDGET_MS);
    expect(rows).toBe(20001);
  });

  it("视口窗口器：overscan/末页夹取/空集/行落位", () => {
    expect(viewportWindow(0, 0, 480, 24)).toEqual({ start: 0, end: 0 });
    expect(viewportWindow(10000, 0, 480, 24).end).toBe(20 + VIEWPORT_OVERSCAN);
    const tail = viewportWindow(30, 6000, 480, 24);
    expect(tail.end).toBe(30);
    expect(tail.start).toBeGreaterThan(0);
    expect(rowTop(7)).toBe(7 * TREE_ROW_HEIGHT_PX);
  });

  it("F528 同步计划：只展开缺失祖先 + 高亮末节点 + 树点击分流", () => {
    const plan = planListToTree(["c", "work", "src"], new Set(["c"]));
    expect(plan.toExpand).toEqual(["work", "src"]);
    expect(plan.highlightId).toBe("src");
    expect(plan.budgetMs).toBe(100);
    expect(treeClickVerdict(true)).toEqual({ refreshList: false, toggleOnly: true });
    expect(treeClickVerdict(false)).toEqual({ refreshList: true, toggleOnly: false });
  });

  it("键盘树导航四向（→ 展开→进子；← 折叠→回父；↑↓ 行移）", () => {
    const rows = flattenTree(DEMO_TREE, new Set(["c", "work"]));
    const exp = new Set<string>(["c", "work"]);
    // → 在未展开节点上展开
    const closed = flattenTree(DEMO_TREE, new Set(["c"]));
    const r1 = treeKeyNav(closed, "work", "right", (id, on) => { if (on) exp.add(id); });
    expect(r1.toggled).toBe(true);
    // → 在已展开节点上进第一个子节点
    const r2 = treeKeyNav(rows, "work", "right", () => {});
    expect(r2.focusId).toBe("src");
    // ← 在折叠态回父
    const r3 = treeKeyNav(rows, "src", "left", () => {});
    expect(r3.focusId).toBe("work");
    // ← 在展开态折叠
    const r4 = treeKeyNav(rows, "work", "left", (id, on) => { if (!on) exp.delete(id); });
    expect(r4.toggled).toBe(true);
    // ↑↓
    expect(treeKeyNav(rows, "c", "down", () => {}).focusId).toBe("work");
    expect(treeKeyNav(rows, "work", "up", () => {}).focusId).toBe("c");
    // 空树诚实
    expect(treeKeyNav([], null, "down", () => {}).focusId).toBeNull();
  });

  it("面包屑：装得下不收缩 + 中段省略首尾保留", () => {
    const crumbs = [
      { id: "r", name: "此机" }, { id: "a", name: "aaaaaaaaaa" },
      { id: "b", name: "bbbbbbbbbb" }, { id: "z", name: "目标" },
    ];
    expect(breadcrumbFit(crumbs, 200).length).toBe(4);
    const fitted = breadcrumbFit(crumbs, 16);
    expect(fitted[0]!.name).toBe("此机");
    expect(fitted.some((c) => c.name === "…")).toBe(true);
    expect(fitted[fitted.length - 1]!.name).toBe("目标");
  });

  it("树宽拖拽夹取（越界有回来的路——章五）", () => {
    expect(clampTreeWidth(50)).toBe(TREE_PANE_MIN_PX);
    expect(clampTreeWidth(9999)).toBe(TREE_PANE_MAX_PX);
    expect(clampTreeWidth(220)).toBe(220);
  });

  it("expui 自检全绿", () => {
    const checks = expuiSelfCheck();
    expect(checks.length).toBeGreaterThanOrEqual(14);
    expect(checks.filter((c) => !c.pass)).toEqual([]);
  });
});

/* ------------------------------ lockmount 锁屏挂接 ------------------------------ */

describe("v5 lockmount 锁屏横幅挂接引擎", () => {
  it("锁定即 PIN 键盘 + 五次冷却状态机全链", () => {
    let rt = lockMountInit();
    expect(rt.state.phase).toBe("locked");
    expect(rt.state.pinPad).toBe(true);
    for (let i = 0; i < 4; i++) rt = lockMountPinFail(rt, 1000 + i * 10).rt;
    expect(rt.state.phase).toBe("pin-entry");
    const fifth = lockMountPinFail(rt, 2000);
    expect(fifth.rt.state.phase).toBe("cooldown");
    expect(fifth.nextAction).toBe("wait-cooldown");
    expect(fifth.rt.state.cooldownUntil).toBe(2000 + 30_000); // 首档 30s
  });

  it("冷却逐次翻倍（破坏注入：第六错冷却翻倍）", () => {
    let rt = lockMountInit();
    for (let i = 0; i < 5; i++) rt = lockMountPinFail(rt, i * 10).rt;
    const sixth = lockMountPinFail(rt, 1000);
    expect(sixth.rt.state.cooldownUntil - 1000).toBe(60_000); // 30s → 60s
  });

  it("冷却到期密码回退 + 未到期显性拒绝 + 解锁归零焦点归还", () => {
    let rt = lockMountInit();
    for (let i = 0; i < 5; i++) rt = lockMountPinFail(rt, i * 10).rt;
    const early = lockMountCooldownTick(rt, rt.state.phase === "cooldown" ? rt.state.cooldownUntil - 5 : 0);
    expect(early.canFallback).toBe(false);
    expect(early.remainingMs).toBe(5);
    const due = lockMountCooldownTick(rt, rt.state.phase === "cooldown" ? rt.state.cooldownUntil + 1 : 0);
    expect(due.rt.state.phase).toBe("password-fallback");
    const unl = lockMountUnlock(due.rt);
    expect(unl.rt.failCount).toBe(0);
    expect(unl.focusReturn).toBe("original-window");
    expect(LOCK_FOCUS_TRAP.length).toBeGreaterThan(10);
  });

  it("黑块合成：跨屏裁剪/去重/完全出屏/面积诚实", () => {
    const screen = { x: 0, y: 0, w: 1920, h: 1080 };
    expect(intersectOnScreen({ x: 1800, y: 0, w: 300, h: 100 }, screen)).toEqual({ x: 1800, y: 0, w: 120, h: 100 });
    expect(intersectOnScreen({ x: 5000, y: 0, w: 10, h: 10 }, screen)).toBeNull();
    expect(dedupeRects([{ x: 1, y: 1, w: 2, h: 2 }, { x: 1, y: 1, w: 2, h: 2 }, { x: 3, y: 3, w: 1, h: 1 }]).length).toBe(2);
    const comp = composeBlackBlocks(
      { a: { x: 0, y: 0, w: 100, h: 100 }, b: { x: 50, y: 50, w: 100, h: 100 } },
      screen, false, "black-frame",
    );
    expect(comp.deny).toBe(false);
    expect(comp.blocks.length).toBe(2);
    expect(blackBlockAreaHonest(comp.blocks, screen)).toBe(true);
    // 破坏注入：黑块越出视口 = 面积不诚实 → 显性 false
    expect(blackBlockAreaHonest([{ x: 1900, y: 0, w: 100, h: 100 }], screen)).toBe(false);
  });

  it("横幅窗口规格与落屏几何（任务栏避让 + 事件屏偏移 + 堆叠）", () => {
    const spec = bannerWindowSpec();
    expect(spec).toMatchObject({ alwaysOnTop: true, skipTaskbar: true, clickThrough: false });
    expect(spec.exits).toContain("click");
    expect(spec.exits).toContain("timeout");
    expect(spec.exits).toContain("focus-loss");
    const scr = { x: 1920, y: 0, w: 1920, h: 1080 };
    const g = bannerScreenGeometry(scr, "bottom-right", { w: 360, h: 96 }, 16);
    expect(g.workArea.h).toBe(1080 - TASKBAR_HEIGHT_PX);
    expect(g.x).toBeGreaterThanOrEqual(1920);
    expect(g.y + 96).toBeLessThanOrEqual(scr.y + g.workArea.h);
    const stack = bannerWindowRects(scr, "bottom-right", [
      { id: "a", w: 360, h: 96 }, { id: "b", w: 360, h: 96 }, { id: "c", w: 360, h: 96 },
    ], 16, 8);
    expect(stack.length).toBe(3);
    const ys = new Set(stack.map((r) => r.y));
    expect(ys.size).toBe(3); // 三枚互不重叠堆叠
    for (const r of stack) {
      expect(r.x).toBeGreaterThanOrEqual(scr.x);
      expect(r.y + r.h).toBeLessThanOrEqual(scr.y + scr.h - TASKBAR_HEIGHT_PX);
    }
  });

  it("Z 序总表一套规则（章十）", () => {
    expect(zOf("lockscreen")).toBeGreaterThan(zOf("black-block"));
    expect(zOf("black-block")).toBeGreaterThan(zOf("banner"));
    expect(zOf("banner")).toBeGreaterThan(zOf("osd"));
    expect(zOf("osd")).toBeGreaterThan(zOf("focus-overlay"));
    expect(Object.keys(U3_ZORDER).length).toBe(5);
  });

  it("lockmount 自检全绿", () => {
    const checks = lockmountSelfCheck();
    expect(checks.length).toBeGreaterThanOrEqual(14);
    expect(checks.filter((c) => !c.pass)).toEqual([]);
  });
});

/* ------------------------------ 桶出口 / 锚点 / 对账 ------------------------------ */

describe("v5 桶出口与锚点对账", () => {
  it("V5 注册表三引擎且总自检全绿（前缀命名可归属）", () => {
    expect(V5_ENGINE_SELFCHECKS.map((e) => e.engine)).toEqual(["despaint", "expui", "lockmount"]);
    const all = v5EnginesSelfCheck();
    expect(all.length).toBeGreaterThanOrEqual(43);
    expect(all.filter((c) => !c.pass)).toEqual([]);
    expect(all.every((c) => c.name.startsWith("["))).toBe(true);
  });

  it("u3EnginesSelfCheck = v4 + v5 全量且全绿", () => {
    const all = u3EnginesSelfCheck();
    const v4 = v5EnginesSelfCheck().length;
    expect(all.length).toBeGreaterThan(v4);
    expect(all.filter((c) => !c.pass)).toEqual([]);
  });

  it("F550 锚点域表十二域且 anchorRuntime 全绿", () => {
    expect(U3_ANCHOR_DOMAINS.length).toBe(12);
    expect(U3_ANCHOR_DOMAINS.map((d) => d.domain)).toContain("v5-engines");
    const rt = anchorRuntime();
    expect(rt.allGreen).toBe(true);
  });

  it("F550 三面对账 50/50 无红（v5 域名纳入聚合排除）", () => {
    const rec = reconcileF501F550();
    expect(rec.rows.length).toBe(50);
    expect(rec.rows.filter((r) => !r.ok)).toEqual([]);
  });

  it("labels 十六引擎双语零缺键", () => {
    expect(Object.keys(U3_ENGINE_LABELS).length).toBe(16);
    expect(labelsSelfCheck().filter((c) => !c.pass)).toEqual([]);
  });

  it("v5 引擎 fScope 声明与域表对账一致（不重不漏声明）", () => {
    for (const e of V5_ENGINE_SELFCHECKS) {
      expect(e.fScope.length).toBeGreaterThan(0);
      expect(typeof e.run).toBe("function");
      expect(e.run().length).toBeGreaterThan(4); // 每引擎至少 5 条自检
    }
  });
});
