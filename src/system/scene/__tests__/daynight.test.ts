import { describe, expect, it } from "vitest";
import { PHASE_TINT, seasonAccent, sunPhase, tintToRgba } from "../daynight";

describe("N-11 sunPhase 时钟粗分（无经纬度口径：6-11/11-16/16-19/其余）", () => {
  it("四段与边界正确", () => {
    expect(sunPhase(new Date(2026, 5, 15, 8, 0)).phase).toBe("dawn");
    expect(sunPhase(new Date(2026, 5, 15, 10, 59)).phase).toBe("dawn");
    expect(sunPhase(new Date(2026, 5, 15, 11, 0)).phase).toBe("noon");
    expect(sunPhase(new Date(2026, 5, 15, 13, 30)).phase).toBe("noon");
    expect(sunPhase(new Date(2026, 5, 15, 16, 0)).phase).toBe("dusk");
    expect(sunPhase(new Date(2026, 5, 15, 18, 59)).phase).toBe("dusk");
    expect(sunPhase(new Date(2026, 5, 15, 19, 0)).phase).toBe("night");
    expect(sunPhase(new Date(2026, 5, 15, 3, 0)).phase).toBe("night");
  });

  it("纬度口径：北纬 40° 冬至前后（1 月 1 日）日出≈7.4h", () => {
    // 1 月 1 日 lat=40：日出≈7.4，日落≈16.6
    expect(sunPhase(new Date(2026, 0, 1, 8, 0), 40).phase).toBe("dawn");
    expect(sunPhase(new Date(2026, 0, 1, 12, 0), 40).phase).toBe("noon");
    expect(sunPhase(new Date(2026, 0, 1, 15, 0), 40).phase).toBe("dusk");
    expect(sunPhase(new Date(2026, 0, 1, 21, 0), 40).phase).toBe("night");
  });

  it("极圈回退：极夜→night，极昼→noon；非法纬度回退时钟", () => {
    expect(sunPhase(new Date(2026, 0, 1, 12, 0), 80).phase).toBe("night");
    expect(sunPhase(new Date(2026, 0, 1, 12, 0), -80).phase).toBe("noon");
    expect(sunPhase(new Date(2026, 5, 15, 8, 0), NaN).phase).toBe("dawn"); // 回退时钟口径
    expect(sunPhase(new Date(2026, 5, 15, 8, 0), null).phase).toBe("dawn");
  });

  it("色温 hex：晨蓝→正午白→暮金→夜靛，格式合法", () => {
    for (const tint of Object.values(PHASE_TINT)) expect(tint).toMatch(/^#[0-9a-f]{6}$/i);
    expect(sunPhase(new Date(2026, 5, 15, 13, 0)).tint).toBe(PHASE_TINT.noon);
    expect(sunPhase(new Date(2026, 5, 15, 3, 0)).tint).toBe(PHASE_TINT.night);
  });

  it("tintToRgba 组装遮罩串（--scene-tint 消费）", () => {
    expect(tintToRgba("#7fa8e6", 0.14)).toBe("rgba(127,168,230,0.14)");
    expect(tintToRgba("bad", 0.2)).toBe("rgba(0,0,0,0.2)");
  });
});

describe("N-11 seasonAccent 季节 OKLCH hue 偏移", () => {
  it("月份口径：3-5 春芽 / 6-8 夏碧 / 9-11 秋赭 / 其余冬霜", () => {
    expect(seasonAccent(new Date(2026, 2, 31))).toEqual({ season: "spring", hueShift: 25 });
    expect(seasonAccent(new Date(2026, 4, 15))).toEqual({ season: "spring", hueShift: 25 });
    expect(seasonAccent(new Date(2026, 5, 1))).toEqual({ season: "summer", hueShift: 70 });
    expect(seasonAccent(new Date(2026, 8, 30))).toEqual({ season: "autumn", hueShift: -35 });
    expect(seasonAccent(new Date(2026, 11, 31))).toEqual({ season: "winter", hueShift: 150 });
    expect(seasonAccent(new Date(2026, 0, 10))).toEqual({ season: "winter", hueShift: 150 });
  });
});