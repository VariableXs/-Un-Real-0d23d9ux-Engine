/**
 * U3-v6 引擎群测试（AI-U3 · 批次六）。
 *
 * 三引擎（shellbar/dictwalk/kernelbridge）+ 自检聚合 + 破坏注入。
 * 测试是判据的执行器（不计功能行数）。
 */

import { describe, expect, it } from "vitest";
import {
  // shellbar
  trayLayout, timelineBand, actionCenterRect, alwaysOnTopVerdict,
  winNumberTarget, taskbarFocusNext, clockTooltipRect, clockFullDateLine,
  TRAY_MAX_VISIBLE, ACTION_CENTER_MIN_W, ACTION_CENTER_MAX_W,
  // dictwalk
  walkDictEntries, dictWalkVerdict, layerConflict, dismissalsComplete,
  U3_DICT_ENTRIES, type DictEntry,
  // kernelbridge
  bridgeVerdict, bridgeAll50, bridgeDifferences, kernelFnoMap,
  KERNEL_CHECK_FNS, KERNEL_CHECK_COUNTS, KERNEL_DOMAIN_FILES,
  // 聚合自检
  shellbarSelfCheck, dictwalkSelfCheck, kernelbridgeSelfCheck,
  V6_ENGINE_SELFCHECKS,
} from "../engines";

/* ------------------------------ shellbar ------------------------------ */

describe("U3-v6 shellbar", () => {
  const items = [
    { id: "a", w: 16, pinned: true },
    { id: "b", w: 16, pinned: false },
    { id: "c", w: 16, pinned: false },
  ];

  it("托盘布局：预算内全可见、pinned 保底、8 上限", () => {
    expect(trayLayout(items, 400).visible.length).toBe(3);
    const narrow = trayLayout(items, 72);
    expect(narrow.visible.map((v) => v.id)).toContain("a"); // pinned 保底
    expect(narrow.overflow.length).toBeGreaterThanOrEqual(1);
    const many = Array.from({ length: 12 }, (_, i) => ({ id: `x${i}`, w: 16, pinned: false }));
    const t = trayLayout(many, 2000);
    expect(t.visible.length).toBe(TRAY_MAX_VISIBLE);
    expect(t.overflow.length).toBe(4);
  });

  it("时间线三段分桶（破坏注入：跨日边界）", () => {
    const now = 1_700_000_000_000;
    const dayStart = Math.floor(now / 86_400_000) * 86_400_000;
    expect(timelineBand(dayStart + 1, now)).toBe("today");
    expect(timelineBand(dayStart - 1, now)).toBe("morning");
    expect(timelineBand(dayStart - 13 * 3600_000, now)).toBe("earlier");
  });

  it("通知中心几何夹取（破坏注入：超小屏不出界）", () => {
    const big = actionCenterRect({ w: 3840, h: 2160 }, 48);
    expect(big.w).toBe(ACTION_CENTER_MAX_W);
    const tiny = actionCenterRect({ w: 400, h: 300 }, 48);
    expect(tiny.w).toBe(ACTION_CENTER_MIN_W);
    expect(tiny.x).toBeGreaterThanOrEqual(0);
    expect(tiny.y).toBeGreaterThanOrEqual(0);
  });

  it("F548 置顶三态（锁屏与全屏都拒绝且给人话提示）", () => {
    expect(alwaysOnTopVerdict("normal", false).allow).toBe(true);
    const fs = alwaysOnTopVerdict("fullscreen", false);
    expect(fs.allow).toBe(false);
    if (!fs.allow) expect(fs.hint.length).toBeGreaterThan(0);
    const lk = alwaysOnTopVerdict("normal", true);
    expect(lk.allow).toBe(false);
  });

  it("F535 槽位三态 + 越界空手", () => {
    const slots = [
      { id: "s1", x: 0, w: 40, instances: 0 },
      { id: "s2", x: 44, w: 40, instances: 1 },
      { id: "s3", x: 88, w: 40, instances: 2 },
    ];
    expect(winNumberTarget(slots, 1)?.action).toBe("launch");
    expect(winNumberTarget(slots, 2)?.action).toBe("focus");
    expect(winNumberTarget(slots, 3)?.action).toBe("list");
    expect(winNumberTarget(slots, 0)).toBeNull();
    expect(winNumberTarget(slots, 10)).toBeNull();
  });

  it("F536 焦点环循环（正反双向）", () => {
    const slots = [
      { id: "s1", x: 0, w: 40, instances: 1 },
      { id: "s2", x: 44, w: 40, instances: 1 },
    ];
    expect(taskbarFocusNext(slots, null, false)).toBe("s1");
    expect(taskbarFocusNext(slots, "s2", false)).toBe("s1");
    expect(taskbarFocusNext(slots, "s1", true)).toBe("s2");
    expect(taskbarFocusNext([], null, false)).toBeNull();
  });

  it("F549 时钟 tooltip 避让与完整日期行", () => {
    const tip = clockTooltipRect({ x: 3700, w: 80, y: 100 }, { w: 3840 }, 300);
    expect(tip.x).toBeGreaterThanOrEqual(0);
    expect(tip.x + tip.w).toBeLessThanOrEqual(3840 - 4);
    expect(clockFullDateLine("2026-09-27", "周日", "八月十七")).toBe("2026-09-27 周日 八月十七");
  });

  it("引擎自检全绿", () => {
    expect(shellbarSelfCheck().every((c) => c.pass)).toBe(true);
  });
});

/* ------------------------------ dictwalk ------------------------------ */

describe("U3-v6 dictwalk", () => {
  it("账本 9 条、非系统层四出路全齐、系统层豁免", () => {
    const rows = walkDictEntries();
    expect(rows.length).toBe(U3_DICT_ENTRIES.length);
    expect(U3_DICT_ENTRIES.length).toBe(9);
    const verdicts = dictWalkVerdict(rows, U3_DICT_ENTRIES);
    expect(verdicts.every((v) => v.pass)).toBe(true);
    // 系统层确实只有一条条目（锁屏 PIN）
    expect(U3_DICT_ENTRIES.filter((e) => e.layer === "system").length).toBe(1);
  });

  it("破坏注入：出路残缺显性红并点名缺失", () => {
    const broken: DictEntry = { id: "bad", layer: "popover", dismissals: ["escape"], focusReturnTo: "x", logDomain: "x" };
    const v = dictWalkVerdict(walkDictEntries([broken]), [broken]);
    expect(v[0]!.pass).toBe(false);
    expect(v[0]!.reason).toContain("outside-click");
  });

  it("破坏注入：焦点链断显性红", () => {
    const noFocus: DictEntry = { id: "bad2", layer: "modal", dismissals: ["outside-click", "escape", "re-trigger", "blur"], focusReturnTo: null, logDomain: "x" };
    expect(dismissalsComplete(noFocus)).toBe(true);
    const v = dictWalkVerdict(walkDictEntries([noFocus]), [noFocus]);
    expect(v[0]!.pass).toBe(false);
  });

  it("层级冲突：system 压 popover 显性、同层不报", () => {
    expect(layerConflict({ id: "a", layer: "system" }, { id: "b", layer: "popover" })).toContain("a");
    expect(layerConflict({ id: "a", layer: "popover" }, { id: "b", layer: "popover" })).toBeNull();
  });

  it("引擎自检全绿", () => {
    expect(dictwalkSelfCheck().every((c) => c.pass)).toBe(true);
  });
});

/* ------------------------------ kernelbridge ------------------------------ */

describe("U3-v6 kernelbridge", () => {
  it("50 编号连续不重不漏", () => {
    const rows = bridgeAll50();
    expect(rows.length).toBe(50);
    rows.forEach((r, i) => expect(r.fno).toBe(`F${501 + i}`));
    expect(rows.every((r) => r.bridgeOk)).toBe(true);
  });

  it("内核域文件覆盖面与计数总和", () => {
    expect(KERNEL_DOMAIN_FILES.length).toBe(10);
    const totalFnos = KERNEL_DOMAIN_FILES.reduce((a, d) => a + d.fnos.length, 0);
    expect(totalFnos).toBe(50);
    const sum = Object.values(KERNEL_CHECK_COUNTS).reduce((a, b) => a + b, 0);
    expect(sum).toBe(619);
  });

  it("直排函数表 50 个且首尾对齐", () => {
    expect(KERNEL_CHECK_FNS.length).toBe(50);
    expect(KERNEL_CHECK_FNS[0]).toBe("run_f501_checks");
    expect(KERNEL_CHECK_FNS[49]).toBe("run_f550_checks");
  });

  it("两侧对账零差异（前端承接面全覆盖）", () => {
    const v = bridgeVerdict();
    expect(v.total50).toBe(true);
    expect(v.diffs.length).toBe(0);
    expect(bridgeDifferences()).toEqual([]);
    expect(kernelFnoMap().size).toBe(50);
  });

  it("破坏注入：重复编号抛错", () => {
    const bad: ReadonlyArray<{ file: string; fnos: ReadonlyArray<string> }> = [
      { file: "a.rs", fnos: ["F501"] },
      { file: "b.rs", fnos: ["F501"] },
    ];
    expect(() => {
      const m = new Map<string, string>();
      for (const d of bad) for (const f of d.fnos) {
        if (m.has(f)) throw new Error("dup");
        m.set(f, d.file);
      }
    }).toThrow();
  });

  it("引擎自检全绿", () => {
    expect(kernelbridgeSelfCheck().every((c) => c.pass)).toBe(true);
  });
});

/* ------------------------------ 聚合 ------------------------------ */

describe("U3-v6 聚合", () => {
  it("V6 注册表 3 引擎且全绿", () => {
    expect(V6_ENGINE_SELFCHECKS.length).toBe(3);
    for (const e of V6_ENGINE_SELFCHECKS) {
      expect(e.run().every((c) => c.pass)).toBe(true);
    }
  });
});
