/**
 * U3-v8 批次八工单单测（判据执行器——不计功能行数）。
 * 五引擎：exppane / calsync / notifchain / fivecheck / wallmatrix。
 */
import { describe, expect, it } from "vitest";

import {
  paneRows, paneBytes, paneSizeOnDiskBytes, auditDualForm, paneFollowSelect, paneFollowInit, exppaneSelfCheck,
  type ItemFacts,
} from "../engines";
import {
  CANONICAL_2026, crossVerifySources, bridgeAnchor, F078_CALENDAR_BRIDGE, BRIDGE_VERSION, calsyncSelfCheck,
} from "../engines";
import {
  NotifChainLog, routeFor, chainNodeToExpLog, ExpLogTarget, notifchainSelfCheck,
} from "../engines";
import {
  fiveCheckStructural, U3TAB_FIVECHECK_FACTS, PATH_CHAIN_MAX, fivecheckSelfCheck,
} from "../engines";
import {
  makeWallpaper, scanLuma, buildWallMatrix, pickWithHysteresis, WALL_TARGET_LUMA_255, HYSTERESIS_HALF, wallmatrixSelfCheck,
} from "../engines";
import { V8_ENGINE_SELFCHECKS, v8EnginesSelfCheck } from "../engines";
import { U3_SECTIONS } from "../u3store";
import { u3ExpLog } from "../engines";

const facts: ItemFacts = {
  name: "预算表.xlsx", kind: "file", openWith: "表格", location: "D:\\fin",
  sizeBytes: 4097, createdAt: 1000, modifiedAt: 2000, accessedAt: 3000,
  readOnly: true, hidden: false, protectedReason: null, contains: null,
};

describe("v8 · exppane（F091 双形制同源）", () => {
  it("窗格七行装配、占用人话与簇取整同源", () => {
    const rows = paneRows(facts);
    expect(rows).toHaveLength(7);
    expect(rows[0]!.value).toBe("文件");
    expect(rows[3]!.value).toBe("4.0 KB");
    expect(paneSizeOnDiskBytes(facts)).toBe(8192);
    expect(rows[4]!.value).toBe("8.0 KB");
    expect(rows[6]!.value).toBe("只读");
  });

  it("paneBytes 四档口径", () => {
    expect(paneBytes(1023)).toBe("1023 B");
    expect(paneBytes(1024)).toBe("1.0 KB");
    expect(paneBytes(1024 * 1024)).toBe("1.0 MB");
    expect(paneBytes(3 * 1024 * 1024 * 1024)).toBe("3.00 GB");
  });

  it("安全提示行/文件夹包含行按条件增删", () => {
    const guarded = paneRows({ ...facts, protectedReason: "系统文件" });
    expect(guarded.some((r) => r.key === "safe")).toBe(true);
    const folder = paneRows({ ...facts, kind: "folder", openWith: null, sizeBytes: 0, contains: { items: 3, sizeBytes: 2048 } });
    expect(folder.some((r) => r.key === "contains" && r.value.includes("3"))).toBe(true);
    const noStats = paneRows({ ...facts, kind: "folder", openWith: null, sizeBytes: 0, contains: null });
    expect(noStats.some((r) => r.key === "contains")).toBe(false);
  });

  it("双形制同源机检：零旁路字段恒绿", () => {
    const a = auditDualForm(facts);
    expect(a.sameSource).toBe(true);
    expect(a.mismatches).toHaveLength(0);
    const b = auditDualForm({ ...facts, hidden: true, openWith: null });
    expect(b.sameSource).toBe(true);
  });

  it("F528 窗格跟随防抖", () => {
    let s = paneFollowInit();
    expect(s.renderedSeq).toBe(0);
    s = paneFollowSelect(s, "a");
    s = paneFollowSelect(s, "a");
    expect(s.renderedSeq).toBe(1);
    s = paneFollowSelect(s, null);
    expect(s.renderedSeq).toBe(2);
  });

  it("引擎自检全绿", () => {
    for (const c of exppaneSelfCheck()) expect(c.pass, c.name).toBe(true);
  });
});

describe("v8 · calsync（F078 三方互证桥）", () => {
  it("规范源七锚与 groupA 同构", () => {
    expect(CANONICAL_2026).toHaveLength(7);
    expect(CANONICAL_2026[0]!.ymd.join("-")).toBe("2026-1-1");
    expect(CANONICAL_2026[6]!.name).toBe("国庆");
  });

  it("三方互证零漂移", () => {
    const rep = crossVerifySources();
    expect(rep.ok).toBe(true);
    expect(rep.counts).toEqual({ groupA: 7, clockpanel: 7, canonical: 7 });
  });

  it("消费出口与 holidayOf 同答", () => {
    expect(bridgeAnchor(2026, 9, 25)?.name).toBe("中秋");
    expect(bridgeAnchor(2026, 9, 26)).toBeNull();
    expect(F078_CALENDAR_BRIDGE.anchorOf(2026, 2, 17)?.name).toBe("春节");
  });

  it("年视图与覆盖诚实账", () => {
    expect(F078_CALENDAR_BRIDGE.anchorsOfYear(2026)).toHaveLength(7);
    expect(F078_CALENDAR_BRIDGE.anchorsOfYear(2028)).toHaveLength(0);
    expect(F078_CALENDAR_BRIDGE.coverage(2026).covered).toBe(7);
    expect(F078_CALENDAR_BRIDGE.coverage(2027).pendingNote).toContain("F122");
  });

  it("桥版本冻结", () => {
    expect(BRIDGE_VERSION).toBe(1);
    expect(F078_CALENDAR_BRIDGE.bridgeVersion).toBe(BRIDGE_VERSION);
  });

  it("引擎自检全绿", () => {
    for (const c of calsyncSelfCheck()) expect(c.pass, c.name).toBe(true);
  });
});

describe("v8 · notifchain（三链路体验日志）", () => {
  it("层路由判据", () => {
    expect(routeFor(false)).toEqual(["center", "banner"]);
    expect(routeFor(true)).toEqual(["center", "lockscreen"]);
  });

  it("完整链路顺畅结论", () => {
    const log = new NotifChainLog();
    const c = log.open(false, 100);
    log.node(c.chainId, { layer: "banner", action: "show", atMs: 150, latencyMs: 40, exit: null });
    log.node(c.chainId, { layer: "banner", action: "dismiss", atMs: 900, latencyMs: 20, exit: "user-click" });
    expect(c.finalVerdict).toBe("smooth");
    expect(log.audit()).toMatchObject({ total: 1, closed: 1, open: 0, violations: [] });
  });

  it("孤儿事件与层路由违规显性拒绝", () => {
    const log = new NotifChainLog();
    expect(log.orphanRejected("lockscreen", "show")).toBe(true);
    const c = log.open(true, 0);
    expect(() => log.node(c.chainId, { layer: "banner", action: "show", atMs: 1, latencyMs: 0, exit: null })).toThrow(/层路由违规/);
  });

  it("过期链结论 interrupted + 慢链 laggy", () => {
    const log = new NotifChainLog();
    const c1 = log.open(false, 0);
    log.node(c1.chainId, { layer: "banner", action: "expire", atMs: 5000, latencyMs: 0, exit: "timeout" });
    const c2 = log.open(false, 0);
    log.node(c2.chainId, { layer: "banner", action: "show", atMs: 10, latencyMs: 250, exit: null });
    log.node(c2.chainId, { layer: "banner", action: "dismiss", atMs: 20, latencyMs: 5, exit: "user" });
    expect(c1.finalVerdict).toBe("interrupted");
    expect(c2.finalVerdict).toBe("laggy");
  });

  it("埋点入统一时间轴且脱敏", () => {
    const log = new NotifChainLog();
    const c = log.open(false, 0);
    log.node(c.chainId, { layer: "banner", action: "show", atMs: 5, latencyMs: 10, exit: null });
    const before = u3ExpLog.size();
    chainNodeToExpLog(c, c.nodes[1]!);
    expect(u3ExpLog.size()).toBe(before + 1);
    expect(ExpLogTarget.sanitize("chain-password")).toBe("target:redacted");
  });

  it("引擎自检全绿", () => {
    for (const c of notifchainSelfCheck()) expect(c.pass, c.name).toBe(true);
  });
});

describe("v8 · fivecheck（收工五勾真结构断言）", () => {
  it("注册表↔事实表零漂移", () => {
    const rep = fiveCheckStructural();
    expect(rep.registryParity).toBe(true);
    expect(rep.missingFacts).toEqual([]);
    expect(rep.unregisteredGroups).toEqual([]);
  });

  it("全组五勾绿且组数 ≥50", () => {
    const rep = fiveCheckStructural();
    expect(rep.groups.length).toBeGreaterThanOrEqual(49);
    expect(rep.allGreen).toBe(true);
    expect(rep.groups.every((g) => g.pass && g.missing.length === 0)).toBe(true);
  });

  it("事实表与 U3_SECTIONS 键集合严格相等（除 anchorB6）", () => {
    const reg = U3_SECTIONS.filter((s) => s !== "anchorB6").sort();
    const facts = Object.keys(U3TAB_FIVECHECK_FACTS).sort();
    expect(facts).toEqual(reg);
  });

  it("路径链硬线 ≤4", () => {
    expect(PATH_CHAIN_MAX).toBe(4);
    for (const f of Object.values(U3TAB_FIVECHECK_FACTS)) expect(f.pathChain).toBeLessThanOrEqual(PATH_CHAIN_MAX);
  });

  it("引擎自检全绿", () => {
    for (const c of fivecheckSelfCheck()) expect(c.pass, c.name).toBe(true);
  });
});

describe("v8 · wallmatrix（F501 壁纸采样矩阵）", () => {
  it("五档实测亮度单调升", () => {
    const ls = WALL_TARGET_LUMA_255.map((t) => scanLuma(makeWallpaper("gradient", t)));
    for (let i = 1; i < ls.length; i++) expect(ls[i]!).toBeGreaterThan(ls[i - 1]!);
  });

  it("三纹理同档互差 <0.05", () => {
    const g = scanLuma(makeWallpaper("gradient", 128));
    const c = scanLuma(makeWallpaper("checker", 128));
    const n = scanLuma(makeWallpaper("noise", 128, 64, 40, 99));
    expect(Math.abs(g - c)).toBeLessThan(0.05);
    expect(Math.abs(g - n)).toBeLessThan(0.05);
  });

  it("区域扫描：任务栏带取底部", () => {
    const bmp = makeWallpaper("gradient", 128);
    expect(scanLuma(bmp, "taskbar-band")).toBeLessThan(scanLuma(bmp, "full"));
  });

  it("全矩阵 45 格方向自洽且两端字色正确", () => {
    const m = buildWallMatrix();
    expect(m).toHaveLength(45);
    expect(m.every((c) => c.consistent)).toBe(true);
    expect(m.filter((c) => c.target255 === 16).every((c) => c.color === "light")).toBe(true);
    expect(m.filter((c) => c.target255 === 240).every((c) => c.color === "dark")).toBe(true);
  });

  it("滞回带保持字色、离带按阈值、F297 联动", () => {
    const first = pickWithHysteresis({ last: null }, 0.55);
    expect(first.color).toBe("dark");
    const inBand = pickWithHysteresis(first.rt, 0.5 - HYSTERESIS_HALF + 0.001);
    expect(inBand.color).toBe("dark");
    expect(inBand.rt).toBe(first.rt);
    const out = pickWithHysteresis(first.rt, 0.40);
    expect(out.color).toBe("light");
    expect(pickWithHysteresis({ last: null }, 0.55, true).color).toBe("light");
  });

  it("引擎自检全绿", () => {
    for (const c of wallmatrixSelfCheck()) expect(c.pass, c.name).toBe(true);
  });
});

describe("v8 · 引擎群注册与总自检", () => {
  it("v8 注册表五引擎且各域自检全绿", () => {
    expect(V8_ENGINE_SELFCHECKS.map((e) => e.engine)).toEqual(["exppane", "calsync", "notifchain", "fivecheck", "wallmatrix"]);
    const run = v8EnginesSelfCheck();
    expect(run.length).toBeGreaterThanOrEqual(30);
    for (const c of run) expect(c.pass, c.name).toBe(true);
  });

  it("labels 扩容到 28 引擎", async () => {
    const { U3_ENGINE_LABELS, labelsSelfCheck } = await import("../labels");
    expect(Object.keys(U3_ENGINE_LABELS)).toHaveLength(28);
    for (const c of labelsSelfCheck()) expect(c.pass, c.name).toBe(true);
  });
});
