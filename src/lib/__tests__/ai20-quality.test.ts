/**
 * AI-20 质量门禁与收官组单元测试（M-79/80/88/89/90 + V-92 纯逻辑部分）。
 * DOM/IPC 真链路由 dogfood/实机验收覆盖；此处锁死核心算法。
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  scrubPII, stackTopOf, aggregateErrors, buildErrReport,
  recordError, boardRecords, clearErrBoard, persistErrBoard,
  type ErrBoardRecord,
} from "../errBoard";
import { filterIpcTrace, buildWaterfall, traceInvoke, IPC_SLOW_MS, subscribeIpcTrace } from "../ipcTrace";
import { tooltipLabel, truncateTooltip, tipProps } from "../tooltip";
import { formatSpeed, formatCapacity, shouldUseRelative, formatSmartTime } from "../formatUnits";
import { dayKeyOf, DayRolloverWatcher, DAY_ROLLOVER_EVENT } from "../dayRollover";
import { requestPowerAction, cancelPowerCountdown, powerGateStore, POWER_COUNTDOWN_SEC } from "../../system/power/powerGate";

// ---------- M-79 错误聚合看板 ----------

describe("AI-20 M-79：错误环形缓冲与聚合", () => {
  beforeEach(() => {
    clearErrBoard();
    localStorage.clear();
  });

  it("PII 清洗：引号长串剥离、绝对路径截断、超长截断", () => {
    expect(scrubPII('Cannot read file "C:\\Users\\secret\\document.txt"')).not.toContain("secret");
    expect(scrubPII("路径 C:\\Users\\bob\\AppData\\Local\\x.json 出错")).not.toContain("bob");
    expect(scrubPII("x".repeat(300)).length).toBeLessThanOrEqual(201);
    expect(scrubPII("短消息")).toBe("短消息");
  });

  it("栈顶提取：src 帧文件:行", () => {
    const stack = "Error: boom\n    at ComponentA (src/system/x/Foo.tsx:42:7)\n    at next (chunk-a.js:1:1)";
    expect(stackTopOf(stack)).toBe("src/system/x/Foo.tsx:42");
    expect(stackTopOf(undefined)).toBeNull();
  });

  it("注入 3 类组件错误 ×100：聚合计数正确、Top 排序正确", () => {
    const now = Date.now();
    const recs: ErrBoardRecord[] = [];
    for (let i = 0; i < 100; i++) {
      recs.push({ component: "CompA", message: "boom A", stackTop: null, ts: now - i * 1000 });
      recs.push({ component: "CompB", message: "boom B", stackTop: null, ts: now - i * 2000 });
      recs.push({ component: "CompC", message: "boom C", stackTop: null, ts: now - i * 3000 });
    }
    const top = aggregateErrors(recs, 7, 10, now);
    expect(top).toHaveLength(3);
    expect(top.every((t) => t.count === 100)).toBe(true);
    expect(top[0]?.component).toBe("CompA"); // 次数并列按最近时间
  });

  it("7 天窗口：过期错误不计入；limit 生效", () => {
    const now = Date.now();
    const recs = [
      { component: "Old", message: "m", stackTop: null, ts: now - 8 * 24 * 3600_000 },
      { component: "New", message: "m", stackTop: null, ts: now - 1000 },
    ];
    const top = aggregateErrors(recs, 7, 10, now);
    expect(top).toHaveLength(1);
    expect(top[0]?.component).toBe("New");
    const recs2 = Array.from({ length: 15 }, (_, i) => ({ component: `C${i}`, message: "m", stackTop: null, ts: now }));
    expect(aggregateErrors(recs2, 7, 10, now)).toHaveLength(10);
  });

  it("markdown 报告可直接贴 issue（含计数/首末时间）", () => {
    const now = Date.now();
    const rep = buildErrReport(
      [{ component: "X", message: "boom", stackTop: "src/a.ts:1", firstAt: now, lastAt: now, count: 3 }],
      "1.2.3",
    );
    expect(rep).toContain("v1.2.3");
    expect(rep).toContain("### 1. [3 次] X");
    expect(rep).toContain("boom");
    expect(rep).toContain("src/a.ts:1");
    expect(buildErrReport([], "1.0")).toContain("无错误记录");
  });

  it("recordError 落盘/读取闭环（退出落盘 → 下次会话并入）", () => {
    recordError("CompZ", 'msg with "user-secret-input-1234567890"');
    persistErrBoard();
    const loaded = boardRecords();
    expect(loaded.some((r) => r.component === "CompZ")).toBe(true);
    expect(loaded.every((r) => !r.message.includes("user-secret"))).toBe(true);
    clearErrBoard();
    expect(boardRecords()).toHaveLength(0);
  });
});

// ---------- M-80 IPC 追踪 ----------

describe("AI-20 M-80：IPC 追踪环形缓冲与瀑布", () => {
  it("traceInvoke 记录成败与耗时", async () => {
    const ok = await traceInvoke("cmd_ok", async () => 42);
    expect(ok).toBe(42);
    await expect(traceInvoke("cmd_fail", async () => { throw new Error("nope"); })).rejects.toThrow("nope");
    const snap = (await import("../ipcTrace")).ipcTraceSnapshot();
    const names = snap.map((r) => r.cmd);
    expect(names).toContain("cmd_ok");
    expect(names).toContain("cmd_fail");
    const fail = snap.find((r) => r.cmd === "cmd_fail")!;
    expect(fail.ok).toBe(false);
    expect(fail.error).toContain("nope");
  });

  it("按命令名过滤（前缀不区分大小写）", () => {
    const recs = [
      { cmd: "fsindex_query", ms: 1, ok: true, error: null, ts: 1 },
      { cmd: "fsindex_status", ms: 2, ok: true, error: null, ts: 2 },
      { cmd: "other", ms: 3, ok: true, error: null, ts: 3 },
    ];
    expect(filterIpcTrace(recs, "fsindex")).toHaveLength(2);
    expect(filterIpcTrace(recs, "")).toHaveLength(3);
  });

  it("瀑布：慢调用（>100ms）标红、比例归一", () => {
    const recs = [
      { cmd: "fast", ms: 5, ok: true, error: null, ts: 0 },
      { cmd: "slow", ms: 250, ok: true, error: null, ts: 10 },
    ];
    const rows = buildWaterfall(recs);
    expect(rows.find((r) => r.rec.cmd === "slow")!.slow).toBe(true);
    expect(rows.find((r) => r.rec.cmd === "fast")!.slow).toBe(false);
    expect(rows.every((r) => r.left >= 0 && r.left <= 1 && r.width > 0 && r.width <= 1)).toBe(true);
    expect(IPC_SLOW_MS).toBe(100);
  });

  it("订阅退订闭环", () => {
    const fn = vi.fn();
    const off = subscribeIpcTrace(fn);
    off();
    expect(fn).toHaveBeenCalledTimes(1); // 订阅即推送当前快照一次
  });
});

// ---------- M-88 Tooltip 规范 ----------

describe("AI-20 M-88：Tooltip 后缀格式与截断", () => {
  it("快捷键后缀圆括号式（唯一口径）", () => {
    expect(tooltipLabel("复制", "Ctrl+C")).toBe("复制 (Ctrl+C)");
    expect(tooltipLabel("复制", null)).toBe("复制");
    expect(tooltipLabel("复制", "  ")).toBe("复制");
    expect(tooltipLabel("粘贴", "Ctrl+Shift+V")).toBe("粘贴 (Ctrl+Shift+V)");
  });

  it("超长截断 + 完整内容保留", () => {
    const short = "短提示";
    expect(truncateTooltip(short)).toEqual([short, false]);
    const [disp, truncated] = truncateTooltip("很".repeat(40));
    expect(truncated).toBe(true);
    expect(disp.endsWith("…")).toBe(true);
    expect(disp.length).toBeLessThan(40);
    const props = tipProps("很".repeat(40), "Ctrl+K");
    expect(props["data-tip-full"]).toContain("Ctrl+K");
    expect(props["data-tip"]!).not.toBe(props["data-tip-full"]);
  });
});

// ---------- M-89 单位与数字规范 ----------

describe("AI-20 M-89：单位与数字规范", () => {
  it("速度：十进制 1 位小数（20 边界值之一组）", () => {
    expect(formatSpeed(0)).toBe("0 B/s");
    expect(formatSpeed(512)).toBe("512 B/s");
    expect(formatSpeed(999)).toBe("999 B/s");
    expect(formatSpeed(1023)).toBe("1.0 KB/s");
    expect(formatSpeed(1024)).toBe("1.0 KB/s");
    expect(formatSpeed(15360)).toBe("15.4 KB/s");
    expect(formatSpeed(1_000_000)).toBe("1.0 MB/s");
    expect(formatSpeed(12_345_678)).toBe("12.3 MB/s");
    expect(formatSpeed(1_500_000_000)).toBe("1.5 GB/s");
    expect(formatSpeed(-5)).toBe("0 B/s");
    expect(formatSpeed(Number.NaN)).toBe("0 B/s");
  });

  it("容量：默认口径 = 现状（KB 1 位小数 / GB 2 位）", () => {
    expect(formatCapacity(512)).toBe("512 B");
    expect(formatCapacity(2048)).toBe("2.0 KB");
    expect(formatCapacity(5 * 1024 * 1024)).toBe("5.0 MB");
    expect(formatCapacity(1.5 * 1024 ** 3)).toBe("1.50 GB");
  });

  it("容量：M-78 显式口径（binary=KiB / decimal=KB）", () => {
    expect(formatCapacity(2048, "binary")).toContain("KiB");
    expect(formatCapacity(2048, "decimal")).toContain("KB");
    expect(formatCapacity(2048, "decimal")).not.toContain("i");
  });

  it("相对/绝对切换规则（7 天 / 1 天窗口）", () => {
    const now = Date.now();
    expect(shouldUseRelative(now - 3 * 60_000, now)).toBe(true); // 3 分钟前
    expect(shouldUseRelative(now - 6 * 24 * 3600_000, now)).toBe(true); // 6.9 天前
    expect(shouldUseRelative(now - 8 * 24 * 3600_000, now)).toBe(false); // 8 天前 → 绝对
    expect(shouldUseRelative(now + 23 * 3600_000, now)).toBe(true); // 未来 23h
    expect(shouldUseRelative(now + 2 * 24 * 3600_000, now)).toBe(false); // 未来 2 天 → 绝对
  });

  it("智能时间：绝对态带 HH:mm、相对态含相对语义", () => {
    const now = new Date("2026-09-09T12:00:00").getTime();
    const abs = formatSmartTime(now - 8 * 24 * 3600_000, undefined, now);
    expect(abs).toMatch(/\d+\/\d+ \d{2}:\d{2}/);
    // 相对态（3 分钟前）→ 交给 Intl，只断言不是绝对格式
    const rel = formatSmartTime(now - 3 * 60_000, undefined, now);
    expect(rel).not.toMatch(/^\d+\/\d+ \d{2}:\d{2}$/);
  });
});

// ---------- M-90 跨午夜正确性 ----------

describe("AI-20 M-90：日界事件（虚拟时钟）", () => {
  it("连续跨午夜 50 次翻页全部正确", () => {
    const events: { from: string; to: string }[] = [];
    let t = new Date("2026-01-01T23:45:00").getTime();
    const w = new DayRolloverWatcher(
      () => t,
      30_000,
      (d) => events.push(d),
    );
    w.start();
    for (let i = 0; i < 50; i++) {
      t += 24 * 3600_000; // 每次 +1 天
      w.check("tick");
    }
    w.stop();
    expect(events).toHaveLength(50);
    // 翻页链完整：from→to 逐日衔接
    const t0 = new Date("2026-01-01T23:45:00").getTime();
    for (let i = 0; i < events.length; i++) {
      const e = events[i];
      const prev = i > 0 ? events[i - 1] : undefined;
      expect(e?.to).toBe(dayKeyOf(t0 + (i + 1) * 24 * 3600_000));
      if (prev) expect(e?.from).toBe(prev.to);
    }
    expect(events[0]?.from).toBe("2026-01-01");
    expect(events[0]?.to).toBe("2026-01-02");
  });

  it("手动改系统时间 ±1 天各 10 次：timejump 源正确", () => {
    let t = new Date("2026-06-01T10:00:00").getTime();
    const events: { via: string; to: string }[] = [];
    const w = new DayRolloverWatcher(() => t, 30_000, (d) => events.push({ via: d.via, to: d.to }));
    for (let i = 0; i < 10; i++) {
      t -= 24 * 3600_000; // 向过去拨 1 天
      w.check("tick");
      t += 48 * 3600_000; // 向未来拨 2 天（净 +1 天）
      w.check("tick");
    }
    w.stop();
    expect(events.length).toBeGreaterThanOrEqual(20);
    // 拨动幅度大于轮询窗口 → 全部标记 timejump
    expect(events.every((e) => e.via === "timejump")).toBe(true);
  });

  it("同日重复 tick 不发事件", () => {
    let t = new Date("2026-06-01T10:00:00").getTime();
    const fn = vi.fn();
    const w = new DayRolloverWatcher(() => t, 30_000, fn);
    w.check("tick");
    t += 5 * 60_000;
    w.check("tick");
    w.stop();
    expect(fn).not.toHaveBeenCalled();
  });

  it("事件名常量稳定（消费方依赖）", () => {
    expect(DAY_ROLLOVER_EVENT).toBe("day://rollover");
  });
});

// ---------- V-92 关机倒计时 ----------

describe("AI-20 V-92：关机倒计时门禁", () => {
  beforeEach(() => {
    cancelPowerCountdown();
  });

  it("10 秒定档 + 逐秒递减 + 到点执行", () => {
    expect(POWER_COUNTDOWN_SEC).toBe(10);
    vi.useFakeTimers();
    const exec = vi.fn().mockResolvedValue(undefined);
    expect(requestPowerAction("shutdown", { executor: exec })).toBe(true);
    expect(powerGateStore.getState().pending?.remainSec).toBe(10);
    vi.advanceTimersByTime(4000);
    expect(powerGateStore.getState().pending?.remainSec).toBe(6);
    vi.advanceTimersByTime(6000);
    expect(powerGateStore.getState().pending).toBeNull();
    expect(exec).toHaveBeenCalledWith("shutdown");
    vi.useRealTimers();
  });

  it("取消（Esc 通道）= 中断不执行", () => {
    vi.useFakeTimers();
    const exec = vi.fn().mockResolvedValue(undefined);
    requestPowerAction("reboot", { executor: exec });
    vi.advanceTimersByTime(3000);
    expect(cancelPowerCountdown()).toBe(true);
    vi.advanceTimersByTime(30_000);
    expect(exec).not.toHaveBeenCalled();
    expect(powerGateStore.getState().pending).toBeNull();
    vi.useRealTimers();
  });

  it("force 通道跳过倒计时直接执行（脚本明示）", () => {
    const exec = vi.fn().mockResolvedValue(undefined);
    requestPowerAction("shutdown", { force: true, executor: exec });
    expect(powerGateStore.getState().pending).toBeNull();
    expect(exec).toHaveBeenCalledWith("shutdown");
  });

  it("非法动作拒绝", () => {
    expect(requestPowerAction("lock" as never, { executor: vi.fn() })).toBe(false);
  });
});
