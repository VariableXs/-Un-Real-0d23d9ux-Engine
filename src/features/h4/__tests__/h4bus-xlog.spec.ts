/// <reference types="node" />
/**
 * H4 深化批次七（v7）深测：
 * ① 契约执法事件总线（bus.ts）：契约执法 / 异常隔离 / 账与回放 / 通配 / 单例对齐契约；
 * ② 统一体验日志总线（xlog.ts）：分级环形 / 隐私红线 / 批量冲刷 / 挫败信号 / 时间轴导出。
 */
import { describe, expect, it } from "vitest";
import { H4Bus, h4Bus } from "../bus";
import { BINDINGS } from "../consumers";
import { XLog, xlog, sanitizeMeta, XLOG_CAP, META_MAX_CHARS, DEAD_CLICK_BUDGET_MS } from "../xlog";

/* ================= ① 契约执法事件总线 ================= */

describe("v7 总线：契约执法", () => {
  it("契约事件数 = BINDINGS 全部去重事件（纸面契约 → 运行时门禁的换算面）", () => {
    const bus = new H4Bus();
    const expectCount = new Set(BINDINGS.flatMap((b) => [...b.eventsIn, ...b.eventsOut])).size;
    expect(bus.contractEventCount).toBe(expectCount);
    expect(h4Bus.contractEventCount).toBe(expectCount); // 单例与契约同源
  });

  it("未登记事件拒绝投递并给出显性原因（零静默——先进册再发）", () => {
    const bus = new H4Bus();
    const v = bus.emit("f999.rogue-event", { x: 1 });
    expect(v.ok).toBe(false);
    expect(v.delivered).toBe(0);
    expect(v.reason).toContain("未在 BINDINGS 契约登记");
  });

  it("登记事件正常投递：订阅者收到原样 payload", () => {
    const bus = new H4Bus();
    const got: unknown[] = [];
    bus.on("f351.snapshot-saved", (p) => got.push(p));
    const v = bus.emit("f351.snapshot-saved", { name: "工作" });
    expect(v.ok).toBe(true);
    expect(v.delivered).toBe(1);
    expect(got).toEqual([{ name: "工作" }]);
  });

  it("退订后不再投递（on 返回的 unsubscribe 生效）", () => {
    const bus = new H4Bus();
    let n = 0;
    const off = bus.on("f363.focus-done", () => n++);
    bus.emit("f363.focus-done", null);
    off();
    bus.emit("f363.focus-done", null);
    expect(n).toBe(1);
  });
});

describe("v7 总线：异常隔离与账", () => {
  it("handler 抛错不炸总线、不吞其他订阅者的投递（错误计数显性）", () => {
    const bus = new H4Bus();
    let good = 0;
    bus.on("f370.final-notice", () => {
      throw new Error("订阅者故意崩");
    });
    bus.on("f370.final-notice", () => good++);
    const v = bus.emit("f370.final-notice", { task: "索引" });
    expect(v.ok).toBe(true);
    expect(v.delivered).toBe(2);
    expect(good).toBe(1);
    expect(bus.handlerErrorCount).toBe(1);
  });

  it("通配订阅：全部投递可见（含事件名与时刻）", () => {
    const bus = new H4Bus();
    const seen: Array<{ name: string }> = [];
    const off = bus.onAny((e) => seen.push({ name: (e as { name: string }).name }));
    bus.emit("f374.sheet-show", null, 1234);
    bus.emit("f374.sheet-hide", null, 1300);
    off();
    bus.emit("f374.sheet-show", null, 1400);
    expect(seen).toEqual([{ name: "f374.sheet-show" }, { name: "f374.sheet-hide" }]);
  });

  it("投递账：环形上限 + journalSlice 末窗", () => {
    const bus = new H4Bus();
    for (let i = 0; i < 505; i++) bus.emit("f354.sample-tick", i, i);
    expect(bus.journalSize).toBeLessThanOrEqual(500);
    const last = bus.journalSlice(1)[0]!;
    expect(last.at).toBe(504);
  });

  it("回放：journal 原样重放到目标总线（调试回放面）", () => {
    const a = new H4Bus();
    const b = new H4Bus();
    const got: unknown[] = [];
    b.on("f372.timeline-row", (p) => got.push(p));
    a.on("f372.timeline-row", (p) => p); // 先真实投递一次入账
    a.emit("f372.timeline-row", { seq: 7 }, 100);
    const n = a.replayTo(b, (e) => e.name === "f372.timeline-row");
    expect(n).toBe(1);
    expect(got.length).toBe(1);
  });
});

/* ================= ② 统一体验日志总线 ================= */

describe("v7 xlog：分级环形与隐私红线", () => {
  it("sanitize：长元数据截断 + 显式标记（正文全文永远进不了日志）", () => {
    const s = sanitizeMeta("x".repeat(META_MAX_CHARS + 50));
    expect(s.truncated).toBe(true);
    expect(s.text.length).toBeLessThanOrEqual(META_MAX_CHARS + 40);
    expect(s.text).toContain("截断");
    const ok = sanitizeMeta({ a: 1 });
    expect(ok.truncated).toBe(false);
    expect(ok.text).toBe('{"a":1}');
    const cyc: Record<string, unknown> = {};
    cyc.self = cyc;
    expect(sanitizeMeta(cyc).text).toContain("不可序列化"); // 循环引用拦截不炸
  });

  it("环形缓冲：超 500 挤最旧、seq 单调", () => {
    const l = new XLog("t");
    for (let i = 0; i < XLOG_CAP + 10; i++) l.log("debug", "t", `e${i}`);
    const tl = l.timeline();
    expect(tl.length).toBe(XLOG_CAP);
    expect(tl[0]!.event).toBe("e10"); // 最旧十条被挤出
    expect(tl[tl.length - 1]!.seq).toBeGreaterThan(tl[0]!.seq);
  });

  it("时间轴过滤：按级别/来源", () => {
    const l = new XLog("t");
    l.log("info", "F371", "a");
    l.log("warn", "h4-bus", "b");
    l.log("error", "h4-bus", "c");
    expect(l.timeline({ level: "warn" }).length).toBe(1);
    expect(l.timeline({ source: "h4-bus" }).length).toBe(2);
    expect(l.timeline({ source: "F371" })[0]!.event).toBe("a");
  });

  it("导出：JSON 带会话 id 与全量条目", () => {
    const l = new XLog("s7");
    l.log("info", "t", "x");
    const j = JSON.parse(l.exportJson()) as { sessionId: string; count: number };
    expect(j.sessionId).toBe("s7");
    expect(j.count).toBe(1);
  });
});

describe("v7 xlog：批量冲刷（写日志绝不阻塞交互）", () => {
  it("攒批 flush：sink 收到批、清空 pending、再 flush 为 0", () => {
    const l = new XLog("t");
    const batches: number[] = [];
    l.setSink((entries) => batches.push(entries.length));
    l.log("info", "t", "a");
    l.log("info", "t", "b");
    expect(l.flushNow()).toBe(2);
    expect(batches).toEqual([2]);
    expect(l.flushNow()).toBe(0);
  });

  it("无 sink 时不攒不丢（flushNow 恒 0，日志仍在环形缓冲可查）", () => {
    const l = new XLog("t");
    l.log("info", "t", "orphan");
    expect(l.flushNow()).toBe(0);
    expect(l.timeline().length).toBe(1);
  });

  it("sink 抛错显性化：错误落环（异常零静默——写不出去本身要留痕）", () => {
    const l = new XLog("t");
    l.setSink(() => {
      throw new Error("盘满");
    });
    l.log("info", "t", "will-fail");
    const flushed = l.flushNow();
    expect(flushed).toBe(0);
    expect(l.timeline({ source: "xlog" }).some((e) => e.event === "sink-failed")).toBe(true);
  });
});

describe("v7 xlog：挫败信号（十三章指纹自动标记）", () => {
  it("狂点：1.5s 内 24px 半径 ≥3 次自动标记", () => {
    const l = new XLog("t");
    expect(l.click(100, 100, 0, 10).rage).toBe(false);
    expect(l.click(105, 105, 500, 10).rage).toBe(false);
    const third = l.click(110, 110, 900, 10);
    expect(third.rage).toBe(true);
    // 远处点击不计入
    expect(l.click(500, 500, 1200, 10).rage).toBe(false);
    expect(l.frustrationSummary().rageClicks).toBe(1);
  });

  it("死点：反馈超 100ms 预算自动标记", () => {
    const l = new XLog("t");
    expect(l.click(0, 0, 0, 50).dead).toBe(false);
    expect(l.click(0, 0, 100, DEAD_CLICK_BUDGET_MS + 40).dead).toBe(true);
    expect(l.frustrationSummary().deadClicks).toBe(1);
  });

  it("狂点窗口过期后不复燃（窗口语义正确）", () => {
    const l = new XLog("t");
    l.click(10, 10, 0, 10);
    l.click(12, 12, 100, 10);
    const late = l.click(14, 14, 1600, 10); // 1.6s——首点已出窗，仅 2 点在窗
    expect(late.rage).toBe(false);
  });
});

describe("v7 总线 × 日志联动", () => {
  it("总线投递落 xlog（十三章：每次投递可回放）；契约拒绝落 warn", () => {
    const bus = new H4Bus();
    bus.emit("f351.snapshot-saved", { ok: 1 });
    bus.emit("f999.rogue", null);
    const busEntries = xlog.timeline({ source: "h4-bus" }).filter((e) => e.event === "emit" || e.event === "contract-reject");
    expect(busEntries.some((e) => e.event === "contract-reject")).toBe(true);
  });
});
