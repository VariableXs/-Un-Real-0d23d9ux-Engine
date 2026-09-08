/**
 * AI-16 单测：Z-44 勿扰日程（dndSchedule 纯函数）。
 */
import { describe, expect, it } from "vitest";
import {
  effectiveNotifySound,
  inScheduleWindow,
  isDndActive,
  parseHM,
} from "../dndSchedule";

describe("parseHM", () => {
  it("解析合法 HH:MM", () => {
    expect(parseHM("00:00")).toBe(0);
    expect(parseHM("9:30")).toBe(570);
    expect(parseHM("23:59")).toBe(1439);
    expect(parseHM(" 22:00 ")).toBe(1320);
  });
  it("拒绝非法输入", () => {
    expect(parseHM("24:00")).toBeNull();
    expect(parseHM("12:60")).toBeNull();
    expect(parseHM("ab:cd")).toBeNull();
    expect(parseHM("")).toBeNull();
  });
});

describe("inScheduleWindow（跨午夜支持）", () => {
  it("同日时段 09:00→17:00", () => {
    expect(inScheduleWindow(600, 540, 1020)).toBe(true); // 10:00
    expect(inScheduleWindow(500, 540, 1020)).toBe(false); // 08:20
    expect(inScheduleWindow(1020, 540, 1020)).toBe(false); // 17:00 端点不含
  });
  it("跨午夜时段 22:00→07:00", () => {
    expect(inScheduleWindow(1320, 1320, 420)).toBe(true); // 22:00
    expect(inScheduleWindow(300, 1320, 420)).toBe(true); // 05:00
    expect(inScheduleWindow(600, 1320, 420)).toBe(false); // 10:00
  });
  it("零长度时段 = 无勿扰", () => {
    expect(inScheduleWindow(600, 600, 600)).toBe(false);
  });
});

describe("isDndActive", () => {
  const sched = { enabled: true, start: "22:00", end: "07:00" };
  it("手动勿扰无条件优先", () => {
    expect(isDndActive(true, { enabled: false, start: "09:00", end: "10:00" })).toBe(true);
  });
  it("日程关闭时不勿扰", () => {
    expect(isDndActive(false, { ...sched, enabled: false })).toBe(false);
  });
  it("日程生效（23:30 与 06:00 都在窗口内）", () => {
    expect(isDndActive(false, sched, new Date(2026, 0, 1, 23, 30))).toBe(true);
    expect(isDndActive(false, sched, new Date(2026, 0, 1, 6, 0))).toBe(true);
    expect(isDndActive(false, sched, new Date(2026, 0, 1, 12, 0))).toBe(false);
  });
  it("非法配置诚实降级为不勿扰", () => {
    expect(isDndActive(false, { enabled: true, start: "bad", end: "07:00" })).toBe(false);
  });
});

describe("effectiveNotifySound", () => {
  it("勿扰关闭全响", () => {
    expect(effectiveNotifySound("other", false, false)).toBe(true);
  });
  it("闹钟永远响；提醒按豁免；其余静默", () => {
    expect(effectiveNotifySound("alarm", true, false)).toBe(true);
    expect(effectiveNotifySound("reminder", true, true)).toBe(true);
    expect(effectiveNotifySound("reminder", true, false)).toBe(false);
    expect(effectiveNotifySound("other", true, true)).toBe(false);
  });
});
