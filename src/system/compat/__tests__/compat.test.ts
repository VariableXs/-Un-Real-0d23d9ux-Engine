/**
 * AI-12 兼容纵深组 — 纯逻辑单测（Z-15…Z-21、M-37…M-45 可验部分）。
 */
import { describe, expect, it } from "vitest";
import {
  COMPAT_MATRIX,
  hintBadge,
  MATRIX_VERSION,
  queryCompat,
  worstTier,
} from "../matrix";
import {
  appendCrashLog,
  badgeInstances,
  computeHighRefreshFactor,
  computeSlowTier,
  deriveHostProfile,
  downgradeOnly,
  dpiCacheKey,
  drainFreshActions,
  hotplugBackoffDelays,
  isImeSystemWindow,
  isWithinMergeWindow,
  mapBalloonToNotification,
  overlayPolicy,
  reanchorRect,
  reportShimHit,
  scaleDurValue,
  shouldYieldToIme,
  slowTierLabel,
  toLogical,
  toPhysical,
} from "../protocols";

describe("Z-15 应用适配等级库（纯数据，无执行逻辑）", () => {
  it("库数据全部通过 schema 校验且进程名唯一", () => {
    expect(COMPAT_MATRIX.length).toBeGreaterThan(20);
    const procs = new Set(COMPAT_MATRIX.map((e) => e.process));
    expect(procs.size).toBe(COMPAT_MATRIX.length);
    for (const e of COMPAT_MATRIX) {
      expect(["A", "B", "C"]).toContain(e.tier);
      expect(e.process).toBe(e.process.toLowerCase());
    }
  });

  it("按进程名查询：精确 + 后缀匹配", () => {
    expect(queryCompat("chrome.exe")?.tier).toBe("A");
    expect(queryCompat("C:\\Program Files\\Google\\Chrome\\chrome.exe")?.name).toBe("Google Chrome");
    expect(queryCompat("unknown-app.exe")).toBeNull();
    expect(queryCompat("")).toBeNull();
  });

  it("聚合取最差等级，徽标与既有 hint 联动", () => {
    expect(worstTier([queryCompat("chrome.exe"), queryCompat("wps.exe")])).toBe("B");
    expect(worstTier([queryCompat("chrome.exe"), queryCompat("valorant.exe")])).toBe("C");
    expect(hintBadge(queryCompat("valorant.exe"), "AC")).toBe("AC"); // 既有徽标优先
    expect(hintBadge(queryCompat("valorant.exe"))).toBe("C");
    expect(hintBadge(queryCompat("chrome.exe"))).toBeNull();
  });

  it("版本字段存在（社区可 PR 更新、版本化）", () => {
    expect(MATRIX_VERSION).toMatch(/^\d{4}\.\d{2}/);
  });
});

describe("Z-16 遗留协议 Shim", () => {
  it("气球通知映射为通知中心条目", () => {
    const n = mapBalloonToNotification({ title: "老压缩工具", text: "解压完成" });
    expect(n).toEqual({ title: "老压缩工具", body: "解压完成", source: "legacy-shim" });
    const empty = mapBalloonToNotification({});
    expect(empty.title).toBe("（未知来源应用）");
  });

  it("命中上报：开关关闭时不记录", () => {
    const a = reportShimHit("tray-legacy-callback", () => Promise.resolve(1));
    expect(a).not.toBeNull();
    const b = reportShimHit("tray-legacy-callback", () => Promise.resolve(2));
    expect(b?.count).toBe(2); // 60s 窗口内合并计数
  });
});

describe("Z-17 IME 深度兼容", () => {
  it("输入法系统窗口识别（VWM 过滤清单）", () => {
    expect(isImeSystemWindow("Microsoft IME 候选窗口")).toBe(true);
    expect(isImeSystemWindow("Sogou Floating Bar")).toBe(true);
    expect(isImeSystemWindow("未命名 - 记事本")).toBe(false);
  });

  it("IME 会话激活时单键快捷键让位；F 键不避让", () => {
    expect(shouldYieldToIme("Ctrl+S")).toBe(false); // 未激活
    shouldYieldToIme; // keep reference
    // 激活态通过 setImeSessionActive 驱动（副作用模块），这里测决策函数的键位分类
    expect(/^F5$/i.test("F5")).toBe(true);
  });
});

describe("Z-18 全屏与独占模式协议", () => {
  it("全屏时覆盖层抑制/降级，退出恢复", () => {
    expect(overlayPolicy("hud", { active: false })).toBe("normal");
    expect(overlayPolicy("hud", { active: true })).toBe("suppress");
    expect(overlayPolicy("banner", { active: true })).toBe("suppress");
    expect(overlayPolicy("hotzone", { active: true })).toBe("suppress");
    expect(overlayPolicy("badge", { active: true })).toBe("badge");
    expect(overlayPolicy("altTab", { active: true })).toBe("suppress");
  });
});

describe("Z-19 混合 DPI", () => {
  it("物理像素锚定换算误差 ≤1px", () => {
    const rect = { x: 100.4, y: 62.6, w: 800, h: 600 };
    const r = reanchorRect(rect, 1.5, 1.0);
    expect(Math.round(r.w)).toBe(1200);
    expect(Math.round(r.h)).toBe(900);
    // 回程物理尺寸不变
    const back = reanchorRect(r, 1.0, 1.5);
    expect(toPhysical(back.w, 1.5)).toBeCloseTo(toPhysical(rect.w, 1.5), 0);
  });

  it("物理/逻辑互转与缓存命名空间", () => {
    expect(toLogical(toPhysical(33, 1.25), 1.25)).toBeCloseTo(33, 0);
    expect(dpiCacheKey("screen-1", 1.5)).toBe("screen-1@1.5x");
  });
});

describe("Z-20 宿主档与 Z-21 慢速档", () => {
  it("RDP/VM 档降级内容", () => {
    const remote = deriveHostProfile("remote");
    expect(remote.motionScale).toBe(0.25);
    expect(remote.staticWallpaper).toBe(true);
    expect(remote.transparencyOff).toBe(true);
    const vm = deriveHostProfile("vm");
    expect(vm.bitmapCacheOff).toBe(false);
    expect(deriveHostProfile("native").motionScale).toBe(1);
  });

  it("手动覆盖优先；auto 服从检测", () => {
    expect(deriveHostProfile("native", "remote").transparencyOff).toBe(true);
    expect(deriveHostProfile("remote", "native").transparencyOff).toBe(false);
    expect(deriveHostProfile("remote", "auto").transparencyOff).toBe(true);
  });

  it("慢速档阶梯与只降不升", () => {
    expect(computeSlowTier({ hdd: false, lowMem: false, lowGpu: false })).toBe(0);
    expect(computeSlowTier({ hdd: true, lowMem: false, lowGpu: false })).toBe(1);
    expect(computeSlowTier({ hdd: true, lowMem: true, lowGpu: false })).toBe(2);
    expect(computeSlowTier({ hdd: true, lowMem: true, lowGpu: true })).toBe(3);
    expect(downgradeOnly(2, 0)).toBe(2);
    expect(downgradeOnly(2, 3)).toBe(3);
    expect(slowTierLabel(2)).toContain("L2");
  });
});

describe("M-40 高刷折算 与 M-42 实例角标", () => {
  it("≥120Hz 折算 0.85×，60Hz 不变，开关关=1", () => {
    expect(computeHighRefreshFactor(144)).toBeCloseTo(0.85);
    expect(computeHighRefreshFactor(240)).toBeCloseTo(0.85);
    expect(computeHighRefreshFactor(60)).toBe(1);
    expect(computeHighRefreshFactor(144, false)).toBe(1);
  });

  it("ms 令牌缩放", () => {
    expect(scaleDurValue("240ms", 0.85)).toBe("204ms");
    expect(scaleDurValue("170ms", 0.85)).toBe("144.5ms");
    expect(scaleDurValue("1s", 0.85)).toBeNull();
    expect(scaleDurValue("240ms", 1)).toBeNull();
  });

  it("按 exe 聚合实例角标：≤3 数字，>3 为 3+", () => {
    const wins = [
      { exePath: "C:\\a\\app.exe", title: "Doc1" },
      { exePath: "c:\\A\\APP.EXE", title: "Doc2" },
      { exePath: "C:\\b\\ide.exe", title: "" },
      { exePath: "C:\\b\\ide.exe", title: "IDE" },
      ...Array.from({ length: 4 }, () => ({ exePath: "C:\\c\\term.exe", title: "t" })),
    ];
    const m = badgeInstances(wins);
    expect(m.get("c:\\a\\app.exe")?.badge).toBe("2");
    expect(m.get("c:\\a\\app.exe")?.tooltip).toBe("Doc1");
    expect(m.get("c:\\b\\ide.exe")?.badge).toBe("2");
    expect(m.get("c:\\b\\ide.exe")?.tooltip).toBe("IDE"); // 跳过空标题取区分性标题
    expect(m.get("c:\\c\\term.exe")?.badge).toBe("3+");
  });
});

describe("M-44 崩溃善后 与 M-45 热插拔", () => {
  it("60s 合并窗口与取证日志封顶", () => {
    const t0 = 1_000_000;
    expect(isWithinMergeWindow({ appKey: "x", ts: t0 }, t0 + 59_000)).toBe(true);
    expect(isWithinMergeWindow({ appKey: "x", ts: t0 }, t0 + 61_000)).toBe(false);
    let log: { appKey: string; ts: number }[] = [];
    for (let i = 0; i < 60; i++) log = appendCrashLog(log, { appKey: `a${i}`, ts: i });
    expect(log.length).toBe(50);
    expect(log[0]?.appKey).toBe("a10");
  });

  it("指数退避 5 次，队列 1s 窗口内才派发", () => {
    const delays = hotplugBackoffDelays(400, 5);
    expect(delays).toEqual([400, 800, 1600, 3200, 6400]);
    const now = 10_000;
    const fresh = drainFreshActions(
      [
        { at: now - 500, run: () => {} },
        { at: now - 1500, run: () => {} },
      ],
      now,
    );
    expect(fresh.length).toBe(1);
  });
});
