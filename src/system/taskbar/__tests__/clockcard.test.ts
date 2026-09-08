import { describe, expect, it } from "vitest";
import { CLOCK_ZONES_MAX, isoWeek, sanitizeClockZones, timeInZone } from "../clockcard";

/** M-12：时钟多时区与细节（IANA sanitize ≤3；ISO 8601 周数；Intl 时区渲染）。 */
describe("clockcard (M-12)", () => {
  it("sanitize：非法/重复/超限时如实拒绝或截断", () => {
    expect(sanitizeClockZones(["Asia/Shanghai", "Asia/Shanghai", "no spaces here!", 42, "Europe/London", "America/New_York"]))
      .toEqual(["Asia/Shanghai", "Europe/London", "America/New_York"]);
    expect(sanitizeClockZones("not an array")).toEqual([]);
    expect(sanitizeClockZones(["", "  "])).toEqual([]);
  });

  it("最多 3 个时区", () => {
    const zones = sanitizeClockZones(["A/AA", "B/BB", "C/CC", "D/DD"]);
    expect(zones.length).toBe(CLOCK_ZONES_MAX);
  });

  it("ISO 8601 周数：年初/年末归属边界", () => {
    // 2026-01-01 属于 2026 第 1 周（周四在 1/1 所在周）
    expect(isoWeek(new Date(2026, 0, 1))).toBe(1);
    // 2024-12-30（周一）属于 2025 第 1 周（ISO 年归属）
    expect(isoWeek(new Date(2024, 11, 30))).toBe(1);
    // 2026-12-28（周一）属于 2026 第 53 周
    expect(isoWeek(new Date(2026, 11, 28))).toBe(53);
  });

  it("timeInZone：夏令时由 Intl 处理；非法时区返回 null 降级", () => {
    const summer = new Date("2026-07-01T12:00:00Z");
    const winter = new Date("2026-01-01T12:00:00Z");
    const s = timeInZone(summer, "Europe/London");
    const w = timeInZone(winter, "Europe/London");
    expect(s).not.toBeNull();
    expect(w).not.toBeNull();
    // 夏令时 7 月伦敦 = UTC+1，1 月 = UTC+0 → 同一 UTC 时刻小时不同
    expect(s).not.toEqual(w);
    expect(timeInZone(summer, "Not/AZone")).toBeNull();
  });
});
