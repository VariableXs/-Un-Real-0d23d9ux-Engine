/**
 * J1 深化批次四（v4）单测：手势重绑定 / 快捷键归一化 / 磁吸平滑 / 时间轴回放 /
 * 屏对护边覆盖 / 十二查对账引擎 / 条目元数据完整性。
 */

import { describe, expect, it, beforeEach } from "vitest";
import {
  effectiveGestureAction,
  resolveGestureAction,
  smoothPathD,
  validateBinding,
  BUILTIN_GESTURES,
  type GestureLibraryConfig,
} from "../gestures";import { normalizeKeyCombo, normalizeMainKey } from "../shortcutRecorder";
import { lerpToward } from "../magnet";
import { buildReplay, j1Telemetry, type J1Event } from "../telemetry";
import { SeamGuard, seamPairKey, type MonitorInfo } from "../screen";
import { J1_ITEMS } from "../checklist";
import { twelveChecks, twelveChecksSummary } from "../evidence";
import { j1Store, J1_DEFAULTS } from "../j1store";

beforeEach(() => {
  j1Store.reset();
});

/* ------------------------------- F617 手势重绑定 ------------------------------- */

describe("F617 手势重绑定（轨迹不变换动作）", () => {
  const base: GestureLibraryConfig = { enabled: true, trailFadeMs: 120, custom: {}, bindings: {} };

  it("无绑定 = 原动作（默认态零迁移）", () => {
    expect(resolveGestureAction({ id: "close-tab", action: "window.close-tab" }, base)).toBe("window.close-tab");
    expect(effectiveGestureAction("close-tab", base)).toBe("window.close-tab");
  });

  it("绑定覆盖原动作；空串绑定视同无绑定", () => {
    const bound: GestureLibraryConfig = { ...base, bindings: { "close-tab": "sys.custom-close" } };
    expect(resolveGestureAction({ id: "close-tab", action: "window.close-tab" }, bound)).toBe("sys.custom-close");
    const blank: GestureLibraryConfig = { ...base, bindings: { "close-tab": "  " } };
    expect(resolveGestureAction({ id: "close-tab", action: "window.close-tab" }, blank)).toBe("window.close-tab");
  });

  it("自定义手势同样可重绑（effectiveGestureAction 双源解析）", () => {
    const cfg: GestureLibraryConfig = {
      ...base,
      custom: { "custom-x": { name: "自定义X", dirs: [2, 2], action: "custom.x" } },
      bindings: { "custom-x": "file.new" },
    };
    expect(effectiveGestureAction("custom-x", cfg)).toBe("file.new");
  });

  it("绑定校验：空动作拒绝（清空=删除绑定，不经校验）", () => {
    expect(validateBinding("").length).toBeGreaterThan(0);
    expect(validateBinding("nav.back")).toEqual([]);
  });

  it("12 内置手势全部可解析出有效动作（重绑定面的完整性前提）", () => {
    for (const g of BUILTIN_GESTURES) {
      expect(effectiveGestureAction(g.id, base)).toBe(g.action);
    }
  });

  it("墨迹平滑路径：两点点线、三点起二次贝塞尔（拐角圆顺）、零点空串", () => {
    expect(smoothPathD([])).toBe("");
    expect(smoothPathD([{ x: 0, y: 0 }, { x: 10, y: 0 }])).toContain("L 10 0");
    const d = smoothPathD([{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 20, y: 10 }]);
    expect(d).toContain("Q 10 0 15 5"); // 中点法：控制点=拐点、终点=中点
    expect(d.endsWith("L 20 10")).toBe(true);
  });
});

/* ------------------------------- F615 快捷键归一化 ------------------------------- */

describe("F615 快捷键录制归一化（词典同构）", () => {
  const ev = (key: string, o: Partial<{ ctrl: boolean; alt: boolean; shift: boolean; meta: boolean }> = {}) => ({
    key,
    ctrlKey: o.ctrl ?? false,
    altKey: o.alt ?? false,
    shiftKey: o.shift ?? false,
    metaKey: o.meta ?? false,
  });

  it("修饰键定序 Ctrl+Alt+Shift+Meta；字母大写", () => {
    expect(normalizeKeyCombo(ev("v", { ctrl: true, shift: true }))).toBe("Ctrl+Shift+V");
    expect(normalizeKeyCombo(ev("b", { ctrl: true, alt: true, shift: true, meta: true }))).toBe("Ctrl+Alt+Shift+Meta+B");
  });

  it("空格规范化为 Space；单键无修饰直接可用", () => {
    expect(normalizeKeyCombo(ev(" "))).toBe("Space");
    expect(normalizeKeyCombo(ev("F5"))).toBe("F5");
    expect(normalizeMainKey("a")).toBe("A");
  });

  it("只按修饰键 = 尚无主键（null——半套不收）", () => {
    expect(normalizeKeyCombo(ev("Shift", { shift: true }))).toBeNull();
    expect(normalizeKeyCombo(ev("Control", { ctrl: true }))).toBeNull();
  });
});

/* ------------------------------- F608 磁吸视觉平滑 ------------------------------- */

describe("F608 磁吸视觉平滑（微滑不瞬移）", () => {
  it("偏移按系数渐近且步进收敛（有界不振荡）", () => {
    let v = { x: 0, y: 0 };
    const target = { x: 10, y: -4 };
    let prevDist = Infinity;
    for (let i = 0; i < 40; i++) {
      v = lerpToward(v, target, 0.35);
      const dist = Math.hypot(target.x - v.x, target.y - v.y);
      expect(dist).toBeLessThanOrEqual(prevDist + 1e-9);
      prevDist = dist;
    }
    expect(v).toEqual(target); // 收敛贴合
  });

  it("小距离直接贴合（防无限小数拖尾）；目标回零同样平滑归位", () => {
    expect(lerpToward({ x: 10.03, y: -3.99 }, { x: 10, y: -4 })).toEqual({ x: 10, y: -4 });
    let v = lerpToward({ x: 10, y: -4 }, { x: 0, y: 0 }, 0.35);
    expect(Math.abs(v.x)).toBeLessThan(10);
  });
});

/* ------------------------------- 章十三 时间轴回放 ------------------------------- */

describe("章十三 时间轴回放（分幕 + 挫败归幕）", () => {
  const ev = (at: number, kind: J1Event["kind"] = "click"): J1Event => ({
    at, kind, verdict: "smooth", target: "button", durMs: 0, zone: 5,
  });

  it("间隔超阈值切幕；幕内事件有序", () => {
    const tl = buildReplay([ev(1000), ev(1500), ev(9999), ev(10000)], [], 2500);
    expect(tl.scenes).toHaveLength(2);
    expect(tl.scenes[0]!.events).toHaveLength(2);
    expect(tl.scenes[1]!.events).toHaveLength(2);
    expect(tl.total).toBe(4);
  });

  it("挫败信号归入时间所在幕（无宿主幕时归最后一幕——不丢条目）", () => {
    const tl = buildReplay(
      [ev(0), ev(100), ev(50000)],
      [{ at: 50, kind: "rage-click", detail: "x", zone: 5 }, { at: 99999, kind: "dead-click", detail: "y", zone: 5 }],
      2500,
    );
    expect(tl.scenes[0]!.frustrations).toHaveLength(1);
    expect(tl.scenes[1]!.frustrations).toHaveLength(1);
  });

  it("空输入零幕；实 ring 数据回放自洽（与导出同源）", () => {
    expect(buildReplay([], []).scenes).toEqual([]);
    j1Telemetry.clear();
    j1Telemetry.log("click", "smooth", "b", 1, 1);
    j1Telemetry.log("wheel", "smooth", "w", 2, 2);
    const tl = j1Telemetry.replayTimeline();
    expect(tl.total).toBe(2);
    expect(tl.scenes).toHaveLength(1);
  });
});

/* ------------------------------- F607 屏对护边覆盖 ------------------------------- */

describe("F607 屏对护边覆盖（覆盖 > 全局）", () => {
  const monitors: MonitorInfo[] = [
    { id: "a", x: 0, y: 0, width: 1920, height: 1080, edidFingerprint: "EDID-A", scale: 1 },
    { id: "b", x: 1920, y: 0, width: 1920, height: 1080, edidFingerprint: "EDID-B", scale: 1 },
  ];

  it("屏对显式关护边 = 跨缝直通（即使全局开）", () => {
    const guard = new SeamGuard(
      () => monitors,
      () => ({ enabled: true, edgePx: 4, dwellMs: 200, cornerPx: 8 }),
      (pairKey) => (pairKey === seamPairKey("EDID-A", "EDID-B") ? false : undefined),
    );
    expect(guard.feed(1918, 500, 0)).toBe("pass"); // 立即放行
  });

  it("无覆盖回退全局；undefined 语义=无覆盖（不误关）", () => {
    const guard = new SeamGuard(
      () => monitors,
      () => ({ enabled: true, edgePx: 4, dwellMs: 200, cornerPx: 8 }),
      () => undefined,
    );
    expect(guard.feed(1918, 500, 0)).toBe("hold"); // 全局开 + 无覆盖 → 正常护边
  });
});

/* ------------------------------- 十二查对账引擎 ------------------------------- */

describe("十二查对账引擎（检查项对账的机器层）", () => {
  it("20 项 × 12 查全覆盖；状态枚举合法", () => {
    const audits = twelveChecks();
    expect(audits).toHaveLength(20);
    for (const a of audits) {
      expect(a.checks).toHaveLength(12);
      for (const c of a.checks) {
        expect(["pass", "partial", "gated"]).toContain(c.status);
        expect(c.note.length).toBeGreaterThan(0);
      }
    }
  });

  it("性能线探针全过（20/20——判据硬线的机械表达）", () => {
    const s = twelveChecksSummary();
    expect(s.items).toBe(20);
    expect(s.probePass).toBe(20);
  });

  it("实机项如实 gated 不冒领（4K 走查=gated；无感标准=partial）", () => {
    const audits = twelveChecks();
    for (const a of audits) {
      expect(a.checks.find((c) => c.no === 4)?.status).toBe("gated"); // 4K 走查
      expect(a.checks.find((c) => c.no === 2)?.status).toBe("partial"); // 无感标准
    }
  });

  it("逻辑面收工 = 全部 20 项（探针+说明句+路径链全绿）", () => {
    for (const a of twelveChecks()) {
      expect(a.logicGreen).toBe(true);
    }
  });
});

/* ------------------------------- 条目元数据完整性 ------------------------------- */

describe("J1_ITEMS 元数据完整性（对账源纪律）", () => {
  it("20 项、F 编号唯一连续、说明句与最丑角落非空、路径链 ≤4 段", () => {
    expect(J1_ITEMS).toHaveLength(20);
    const fs = J1_ITEMS.map((i) => i.f);
    expect(new Set(fs).size).toBe(20);
    for (let i = 0; i < 20; i++) {
      expect(fs[i]).toBe(`F${601 + i}`);
      expect(J1_ITEMS[i]!.row.hint.trim().length).toBeGreaterThan(4);
      expect(J1_ITEMS[i]!.ugly.trim().length).toBeGreaterThan(4);
      expect(J1_ITEMS[i]!.navChain.length).toBeLessThanOrEqual(4);
    }
  });

  it("v4 默认值登记：gestures.bindings 与 seamGuard.pairs 入 J1_DEFAULTS", () => {
    expect((J1_DEFAULTS.gestures as { bindings: Record<string, string> }).bindings).toEqual({});
    expect((J1_DEFAULTS.seamGuard as { pairs: Record<string, { enabled: boolean }> }).pairs).toEqual({});
  });
});
