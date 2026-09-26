import { describe, expect, it } from "vitest";
import { bezierEase, bezierXtoT, springAt, springOvershoot, SPRING_OVERSHOOT_ZETA, advance, reverse, scaledDurationMs, sampleLut, tierScale, CURVES, type InterruptibleState } from "../motion-curve";
import { TooltipMachine, placeTooltip, tabSequence, FocusReturnLedger, trapCycle } from "../hover-focus";
import { InteractionLedger } from "../interaction-ledger";
import { surfaceError, makeCatalogSurfacer, auditHiddenSpots, runStartupChain, degradationFor, heartbeatAudit } from "../error-surface";
import { pageMessages } from "../message-catalog";

// ---------- motion-curve ----------

describe("motion-curve · 动效解析（F124/F160 深化）", () => {
  it("bezier 端点精确、中点单调（enter 曲线先快后缓）", () => {
    expect(bezierEase(CURVES.enter!, 0)).toBe(0);
    expect(bezierEase(CURVES.enter!, 1)).toBeCloseTo(1, 5);
    expect(bezierEase(CURVES.enter!, 0.25)).toBeGreaterThan(0.25); // enter 前段加速。
    expect(bezierEase(CURVES.enter!, 0.75)).toBeLessThan(1);
  });

  it("X→T 反解：牛顿+二分后 y(x) 严格递增（缓动是函数）", () => {
    let prev = -1;
    for (let i = 0; i <= 20; i++) {
      const y = bezierEase(CURVES.emphasized!, i / 20);
      expect(y).toBeGreaterThanOrEqual(prev);
      prev = y;
    }
    expect(bezierXtoT(CURVES.linear!, 0.5)).toBeCloseTo(0.5, 3);
  });

  it("spring 过冲契约：ζ 校准值 → 峰值 ≈1.05（105% 过冲物理可验证）", () => {
    const p = { v0: 0, omega0: 12, zeta: SPRING_OVERSHOOT_ZETA };
    const { peak } = springOvershoot(p);
    expect(peak).toBeGreaterThan(1.04);
    expect(peak).toBeLessThan(1.06);
    expect(springAt(p, 0)).toBe(0);
    expect(springAt(p, 10)).toBeCloseTo(1, 3); // 收敛到目标。
  });

  it("打断可逆：正向到头反向 → 位置/值回到 0（不发散——位置与值分离）", () => {
    let st: InterruptibleState = { pos: 0, value: 0, velocity: 0, direction: 1 };
    for (let i = 0; i < 40; i++) st = advance(st, 16.6667, CURVES.enter!, 300);
    expect(st.pos).toBe(1);
    expect(st.value).toBeCloseTo(1, 5);
    st = reverse(st);
    for (let i = 0; i < 60; i++) st = advance(st, 16.6667, CURVES.enter!, 300);
    expect(st.pos).toBe(0);
    expect(st.value).toBeLessThan(0.001); // 回到起点（旧实现把缓动值当时间位——发散到 0.99998）。
    expect(st.direction).toBe(-1);
  });

  it("三档缩放：按值取整（F160 契约 60% 按值成立）、off=0（瞬显）", () => {
    expect(tierScale("full")).toBe(1);
    expect(scaledDurationMs(320, "reduced")).toBe(192); // 320×0.6 精确成立。
    expect(scaledDurationMs(320, "off")).toBe(0);
    expect(scaledDurationMs(333, "reduced")).toBe(200); // 199.8 → 200（毫秒取整）。
  });

  it("LUT 采样与解析一致（查表不失真）", () => {
    const lut = sampleLut(CURVES.enter!, 64);
    expect(lut[32]).toBeCloseTo(bezierEase(CURVES.enter!, 0.5), 3);
    expect(lut[0]).toBe(0);
    expect(lut[64]).toBeCloseTo(1, 3);
  });
});

// ---------- hover-focus ----------

describe("hover-focus · 悬停与焦点（F205/F206 深化）", () => {
  it("tooltip 状态机：冷启动等 500ms、邻近即切、200ms 宽限防闪烁", () => {
    const m = new TooltipMachine();
    expect(m.hoverEnter("a")).toBe(false);
    expect(m.state.phase).toBe("waiting-show");
    expect(m.showDue()).toBe(true);
    expect(m.state.phase).toBe("visible");
    expect(m.hoverEnter("b")).toBe(true); // 已显示 → 换目标不重等。
    expect(m.hoverLeave()).toBe(true);
    expect(m.state.phase).toBe("waiting-hide");
    expect(m.hoverReenter("b")).toBe(true); // 宽限内取消隐藏。
    expect(m.state.phase).toBe("visible");
  });

  it("四角翻转：右下角目标翻 top、左上角翻 bottom（放不下按剩余空间）", () => {
    const vp = { width: 400, height: 240 };
    const tip = { w: 120, h: 32 };
    const br = placeTooltip({ x: 380, y: 220, w: 40, h: 20 }, tip, 6, vp, "bottom");
    expect(br.placement).toBe("top");
    const tl = placeTooltip({ x: 0, y: 0, w: 40, h: 20 }, tip, 6, vp, "bottom");
    expect(tl.placement).toBe("bottom");
    // 视口夹紧：x 永远在界内。
    expect(br.x).toBeGreaterThanOrEqual(4);
    expect(br.x + tip.w).toBeLessThanOrEqual(vp.width - 4);
  });

  it("Tab 序：跳过禁用、循环回绕、反向遍历", () => {
    const nodes = [
      { id: "a", order: 0, role: "button" as const, disabled: false },
      { id: "b", order: 1, role: "button" as const, disabled: true },
      { id: "c", order: 2, role: "custom" as const, disabled: false },
    ];
    expect(tabSequence(nodes, 0)).toBe(2); // b 跳过。
    expect(tabSequence(nodes, 2)).toBe(0); // 循环。
    expect(tabSequence(nodes, 0, true)).toBe(2); // 反向。
    expect(tabSequence([], 0)).toBe(-1); // 空 = 显性 -1。
  });

  it("焦点归还配对：开关配对归还、重复关闭显性 null、泄漏可审计", () => {
    const l = new FocusReturnLedger();
    l.open("p1", "btn-a");
    expect(l.close("p1")!.returnTo).toBe("btn-a");
    expect(l.close("p1")).toBeNull();
    l.open("p2", "btn-b");
    l.open("p3", "btn-c");
    expect(l.close("p3")!.returnTo).toBe("btn-c"); // 后开先关（栈序不强求——按 id 精确配对）。
    expect(l.leaks()).toEqual(["p2"]); // 泄漏显性。
  });

  it("对话框焦点陷阱：Tab 锁定在容器内循环", () => {
    expect(trapCycle(["x", "y", "z"], "z")).toBe("x");
    expect(trapCycle(["x", "y", "z"], "x", true)).toBe("z");
  });
});

// ---------- interaction-ledger ----------

describe("interaction-ledger · 交互台账（十三章深化）", () => {
  it("rage click：1.5s 内同元素 3 点 → 信号（第 3 点触发，不重复报）", () => {
    const l = new InteractionLedger();
    const base = 1000;
    const sigs: unknown[] = [];
    for (let i = 0; i < 6; i++) {
      sigs.push(...l.record({ page: "p", elementId: "btn", at: base + i * 300, action: "click", feedbackMs: 300, durationMs: null, verdict: "laggy" }));
    }
    const rages = sigs.filter((s) => (s as { kind: string }).kind === "rage-click");
    expect(rages).toHaveLength(2); // 第 3、6 点各触发一次。
    const s0 = rages[0] as { message: string };
    expect(s0.message).toContain("连点");
  });

  it("dead click：no-feedback 判定 → 显性信号", () => {
    const l = new InteractionLedger();
    const sigs = l.record({ page: "p", elementId: "fake", at: 0, action: "click", feedbackMs: null, durationMs: null, verdict: "no-feedback" });
    expect(sigs).toHaveLength(1);
    expect(sigs[0]!.kind).toBe("dead-click");
  });

  it("浮层反复开关：30s 内 3 次 → flap（人话指向交互目标未满足）", () => {
    const l = new InteractionLedger();
    expect(l.togglePopover("p", "card", 0)).toBeNull();
    expect(l.togglePopover("p", "card", 5000)).toBeNull();
    const third = l.togglePopover("p", "card", 10000);
    expect(third).not.toBeNull();
    expect(third!.message).toContain("3 次");
    expect(l.togglePopover("p", "card", 20000)).toBeNull(); // 窗口滑出（第一个时间戳过期后 3 次条件不再满足）。
  });

  it("导出零内容字段 + P95 反馈对账 100ms 红线 + 回放可读", () => {
    const l = new InteractionLedger();
    l.record({ page: "p", elementId: "a", at: 0, action: "click", feedbackMs: 40, durationMs: null, verdict: "smooth" });
    l.record({ page: "p", elementId: "b", at: 100, action: "keydown", feedbackMs: 250, durationMs: null, verdict: "laggy" });
    const exp = l.exportLedger(200);
    expect(exp.format).toBe("vx-interaction-ledger");
    expect(exp.events.every((e) => !("value" in e) && !("text" in e))).toBe(true); // 结构即隐私。
    expect(exp.stats.p95FeedbackMs).toBe(250);
    expect(exp.stats.smoothRatio).toBe(0.5);
    expect(l.replay(0, 200)[0]).toContain("反馈 40ms");
    expect(l.topFrustrations(10).every((s) => ["rage-click", "dead-click", "popover-flap", "error-wander"].includes(s.kind))).toBe(true);
  });
});

// ---------- error-surface ----------

describe("error-surface · 异常显性化（十三章补深化）", () => {
  it("统一包装：三要素 + 技术折叠 + 严重级（裸 catch 不可能）", () => {
    const s = surfaceError("tokens/save", "error", { what: "写入失败", why: "配额满", next: "清理后重试" }, new TypeError("x is undefined"), 0);
    expect(s.triad.what).toBe("写入失败");
    expect(s.technical).toContain("TypeError");
    expect(s.detail).toContain("写入失败");
  });

  it("目录注入 surfacer：page 对齐 message-catalog（tokens 页错误三要素直接可用）", () => {
    const surfacer = makeCatalogSurfacer((page) => pageMessages(page, "zh")?.error ?? null);
    const s = surfacer("tokens/save", "error", new Error("boom"), 0);
    expect(s!.triad.why).toContain("锁定");
    const unknown = surfacer("nonexistent/x", "error", new Error("boom"), 0);
    expect(unknown!.triad.why).toContain("未登记");
  });

  it("隐蔽捕获点巡检：六类全覆盖（async/静默 catch/后台/竞态/泄漏/早期）", () => {
    const a = auditHiddenSpots();
    expect(a.spots.map((s) => s.spot)).toEqual(["async-callback", "silent-catch", "background-task", "race-timing", "resource-leak", "early-failure"]);
    expect(a.allCovered).toBe(true);
  });

  it("启动链探针：全绿/首失败环定位 + 降级路径映射", () => {
    const healthy = runStartupChain(() => null, () => null, () => null);
    expect(healthy.healthy).toBe(true);
    expect(degradationFor(healthy).level).toBe("full");
    const broken = runStartupChain(() => null, () => "令牌表损坏", () => null);
    expect(broken.healthy).toBe(false);
    expect(broken.firstFailure!.name).toBe("tokens");
    expect(degradationFor(broken).level).toBe("default-tokens");
    const storeDown = runStartupChain(() => "LS 不可用", () => null, () => null);
    expect(degradationFor(storeDown).level).toBe("safe-mode");
  });

  it("心跳看护：超期 2.5 周期 → 迟到显性 + 重启建议", () => {
    const now = 1_000_000;
    const beats = heartbeatAudit([{ source: "ok", beatAt: now - 10_000, intervalMs: 30_000 }, { source: "late", beatAt: now - 200_000, intervalMs: 30_000 }], now);
    expect(beats[0]!.late).toBe(false);
    expect(beats[1]!.late).toBe(true);
    expect(beats[1]!.advice).toContain("重启");
  });
});
