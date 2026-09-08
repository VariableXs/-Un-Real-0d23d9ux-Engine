import { describe, expect, it } from "vitest";
import { cycleWeather, normalizeWeather, WEATHER_CYCLE } from "../weatherState";

describe("N-11 天气状态机（手工模式，零网络）", () => {
  it("normalizeWeather：合法值透传（大小写/空白容忍），脏值兜底 off", () => {
    expect(normalizeWeather("rain")).toBe("rain");
    expect(normalizeWeather(" RAIN ")).toBe("rain");
    expect(normalizeWeather("fall-leaves")).toBe("fall-leaves");
    expect(normalizeWeather("hail")).toBe("off");
    expect(normalizeWeather("")).toBe("off");
    expect(normalizeWeather(null)).toBe("off");
    expect(normalizeWeather(undefined)).toBe("off");
  });

  it("cycleWeather 沿循环转移并回绕（off→sunny→…→fall-leaves→off）", () => {
    expect(cycleWeather("off", 1)).toBe("sunny");
    expect(cycleWeather("sunny", 1)).toBe("rain");
    expect(cycleWeather("rain", 1)).toBe("snow");
    expect(cycleWeather("snow", 1)).toBe("fall-leaves");
    expect(cycleWeather("fall-leaves", 1)).toBe("off");
    expect(cycleWeather("off", -1)).toBe("fall-leaves");
  });

  it("状态集合封闭：normalize 后必在 WEATHER_CYCLE 内", () => {
    for (const raw of ["rain", "junk", "", "SNOW", "1", "sunny"]) {
      expect(WEATHER_CYCLE).toContain(normalizeWeather(raw));
    }
  });
});