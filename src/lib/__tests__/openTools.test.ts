/**
 * AI-15 开放工具组 — openTools.ts 纯函数单测。
 * 覆盖：V-83 触发预览（nextFireTime/triggerSummary）、V-86 安全启动项镜像、
 * V-82 PATH 分行、字节/时刻格式化、V-89 diff 色类。
 */
import { describe, expect, it } from "vitest";
import {
  diffKindClass,
  fmtBytes,
  fmtTime,
  isSafeStartupMirror,
  nextFireTime,
  splitPathRows,
  triggerSummary,
} from "../openTools";

describe("nextFireTime（V-83 触发预览）", () => {
  const base = new Date(2026, 8, 8, 10, 0, 0).getTime(); // 2026-09-08 10:00 本地

  it("at：今日未过 → 今日同一时刻", () => {
    const t = nextFireTime({ type: "at", hour: 23, minute: 30 }, base);
    const d = new Date(t);
    expect(d.getHours()).toBe(23);
    expect(d.getMinutes()).toBe(30);
    expect(d.getDate()).toBe(8);
  });

  it("at：已过 → 明日同一时刻", () => {
    const t = nextFireTime({ type: "at", hour: 8, minute: 0 }, base);
    const d = new Date(t);
    expect(d.getHours()).toBe(8);
    expect(d.getDate()).toBe(9);
  });

  it("at：恰好等于 now → 明日（下一次，不含当刻）", () => {
    const t = nextFireTime({ type: "at", hour: 10, minute: 0 }, base);
    expect(t).toBe(base + 24 * 3600 * 1000);
  });

  it("interval：从未触发 → now + 间隔", () => {
    expect(nextFireTime({ type: "interval", secs: 60 }, base)).toBe(base + 60_000);
  });

  it("interval：上次触发过 → lastFired + 间隔", () => {
    const last = base - 30_000;
    expect(nextFireTime({ type: "interval", secs: 60 }, base, last)).toBe(last + 60_000);
  });

  it("idle/login：运行时态 → 0（前端显示待机语义）", () => {
    expect(nextFireTime({ type: "idle", secs: 300 }, base)).toBe(0);
    expect(nextFireTime({ type: "login", secs: 90 }, base)).toBe(0);
  });
});

describe("isSafeStartupMirror（V-86 安全类启动项，与 workshop.rs 白名单镜像）", () => {
  it("杀毒/安全类命中（名称或命令行，大小写不敏感）", () => {
    expect(isSafeStartupMirror("Windows Defender", "C:/MsMpEng.exe")).toBe(true);
    expect(isSafeStartupMirror("Foo", "D:/360/safe.exe")).toBe(true);
    expect(isSafeStartupMirror("Huorong Main", "D:/hr.exe")).toBe(true);
  });

  it("安全卫士关键词命中", () => {
    expect(isSafeStartupMirror("某安全卫士", "x.exe")).toBe(true);
  });

  it("驱动类命中", () => {
    expect(isSafeStartupMirror("Foo Bar", "C:/driver/assistant.exe")).toBe(true);
  });

  it("普通应用不命中", () => {
    expect(isSafeStartupMirror("WeChat", "C:/Program Files/wechat.exe")).toBe(false);
    expect(isSafeStartupMirror("", "")).toBe(false);
  });
});

describe("splitPathRows（V-82 PATH 分行）", () => {
  it("分号切分 + 去空段 + 保序", () => {
    expect(splitPathRows("C:\\a;D:\\b;;C:\\c")).toEqual(["C:\\a", "D:\\b", "C:\\c"]);
  });

  it("两端空格去除", () => {
    expect(splitPathRows("  C:\\a ; D:\\b  ")).toEqual(["C:\\a", "D:\\b"]);
  });

  it("空串 → 空数组", () => {
    expect(splitPathRows("")).toEqual([]);
    expect(splitPathRows(";;;")).toEqual([]);
  });
});

describe("fmtBytes", () => {
  it("各量级", () => {
    expect(fmtBytes(512)).toBe("512 B");
    expect(fmtBytes(2048)).toBe("2.0 KB");
    expect(fmtBytes(5 * 1024 * 1024)).toBe("5.0 MB");
    expect(fmtBytes(3 * 1024 * 1024 * 1024)).toBe("3.00 GB");
  });
});

describe("diffKindClass（V-89 三色标记）", () => {
  it("add/del/mod 各自色类，未知为空", () => {
    expect(diffKindClass("add")).toBe("ot-kind-add");
    expect(diffKindClass("del")).toBe("ot-kind-del");
    expect(diffKindClass("mod")).toBe("ot-kind-mod");
    expect(diffKindClass("same")).toBe("");
  });
});

describe("fmtTime", () => {
  it("非正数 → 占位符", () => {
    expect(fmtTime(0)).toBe("—");
    expect(fmtTime(-1)).toBe("—");
  });

  it("正常格式化（本地时区，补零）", () => {
    const d = new Date(2026, 8, 8, 9, 5, 3);
    expect(fmtTime(d.getTime())).toBe("2026-09-08 09:05:03");
  });
});

describe("triggerSummary", () => {
  it("四种触发器文本", () => {
    expect(triggerSummary({ type: "at", hour: 9, minute: 5 })).toBe("at 09:05");
    expect(triggerSummary({ type: "interval", secs: 30 })).toBe("interval 30s");
    expect(triggerSummary({ type: "idle", secs: 300 })).toBe("idle 300s");
    expect(triggerSummary({ type: "login", secs: 60 })).toBe("login 60s");
  });
});
