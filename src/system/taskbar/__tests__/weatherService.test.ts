import { describe, expect, it } from "vitest";
import {
  cacheRead,
  cacheWrite,
  defaultWeatherConfig,
  parseOpenMeteo,
  parseWttr,
  readConfig,
  sanitizeConfig,
  weatherUrl,
  wmoIconKind,
  wmoText,
  writeConfig,
  WEATHER_CACHE_KEY,
  WEATHER_CFG_KEY,
  WEATHER_TTL_MS,
  type StorageLike,
  type WeatherData,
} from "../weatherService";

/** Z-23 天气服务：双供应商 JSON 解析归一化、缓存 TTL、配置校验。 */

/** 测试存储桩（weatherService 全部函数以 StorageLike 注入，不碰真 localStorage）。 */
function fakeStorage(): StorageLike & { dump(): Map<string, string> } {
  const m = new Map<string, string>();
  return {
    getItem: (k) => (m.has(k) ? (m.get(k) as string) : null),
    setItem: (k, v) => {
      m.set(k, v);
    },
    dump: () => m,
  };
}

const WTTR_J1 = JSON.stringify({
  current_condition: [
    {
      temp_C: "22",
      weatherCode: "113",
      weatherDesc: [{ value: "Sunny" }],
      // 其余 j1 字段（FeelsLikeC/visibility…）解析器应忽略
    },
  ],
  weather: [
    { date: "2026-09-08", mintempC: "15", maxtempC: "26", weatherCode: "113" },
    { date: "2026-09-09", mintempC: "16", maxtempC: "24", weatherCode: "116" },
    { date: "2026-09-10", mintempC: "14", maxtempC: "21", weatherCode: "200" },
    { date: "2026-09-11", mintempC: "13", maxtempC: "20", weatherCode: "308" }, // 第 4 天应被截掉
  ],
});

const OPEN_METEO = JSON.stringify({
  latitude: 39.9,
  longitude: 116.4,
  current: { time: "2026-09-08T14:00", temperature_2m: 21.3, weather_code: 3, wind_speed_10m: 8.2 },
  daily: {
    time: ["2026-09-08", "2026-09-09", "2026-09-10", "2026-09-11"],
    weather_code: [0, 3, 61, 71],
    temperature_2m_max: [26.1, 24.5, 19.8, 18.2],
    temperature_2m_min: [15.2, 16.0, 13.9, 12.4],
    sunrise: ["2026-09-08T05:57", "2026-09-09T05:58", "2026-09-10T05:59", "2026-09-11T06:00"],
  },
});

describe("Z-23 解析归一化", () => {
  it("wttr.in j1 → 统一 WeatherData（当前温度/天气码 + 前 3 日预报）", () => {
    const d = parseWttr(WTTR_J1);
    expect(d.provider).toBe("wttr");
    expect(d.tempC).toBe(22);
    expect(d.code).toBe(113);
    expect(d.days).toEqual([
      { date: "2026-09-08", code: 113, minC: 15, maxC: 26 },
      { date: "2026-09-09", code: 116, minC: 16, maxC: 24 },
      { date: "2026-09-10", code: 200, minC: 14, maxC: 21 },
    ]);
    expect(d.days.length).toBe(3); // 第 4 日截断
  });

  it("open-meteo → 统一 WeatherData", () => {
    const d = parseOpenMeteo(OPEN_METEO);
    expect(d.provider).toBe("openmeteo");
    expect(d.tempC).toBeCloseTo(21.3);
    expect(d.code).toBe(3);
    expect(d.days).toEqual([
      { date: "2026-09-08", code: 0, minC: 15.2, maxC: 26.1 },
      { date: "2026-09-09", code: 3, minC: 16.0, maxC: 24.5 },
      { date: "2026-09-10", code: 61, minC: 13.9, maxC: 19.8 },
    ]);
  });

  it("坏载荷抛错（由调用方降级为占位文本）", () => {
    expect(() => parseWttr("not json")).toThrow();
    expect(() => parseWttr("{}")).toThrow();
    expect(() => parseWttr(JSON.stringify({ current_condition: [], weather: [] }))).toThrow();
    expect(() => parseWttr(JSON.stringify({ current_condition: [{ temp_C: "x", weatherCode: "0" }], weather: [] }))).toThrow();
    expect(() => parseOpenMeteo("not json")).toThrow();
    expect(() => parseOpenMeteo("{}")).toThrow();
    expect(() => parseOpenMeteo(JSON.stringify({ current: { temperature_2m: 1 } }))).toThrow();
  });

  it("WMO 码表：文案与图标归类覆盖两家供应商的码段", () => {
    expect(wmoText(0)).toBe("晴");
    expect(wmoText(61)).toBe("雨");
    expect(wmoText(71)).toBe("雪");
    expect(wmoText(95)).toBe("雷雨");
    expect(wmoText(999)).toBe("未知");
    expect(wmoIconKind(0)).toBe("sun");
    expect(wmoIconKind(2)).toBe("partly");
    expect(wmoIconKind(3)).toBe("cloud");
    expect(wmoIconKind(48)).toBe("fog");
    expect(wmoIconKind(80)).toBe("rain");
    expect(wmoIconKind(85)).toBe("snow");
    expect(wmoIconKind(99)).toBe("storm");
  });
});

describe("Z-23 缓存：30 分钟 TTL", () => {
  const sample = (fetchedAt: number): WeatherData => ({
    provider: "wttr",
    fetchedAt,
    tempC: 20,
    code: 0,
    days: [{ date: "2026-09-08", code: 0, minC: 10, maxC: 22 }],
  });

  it("写入 → TTL 内命中；超过 TTL → null", () => {
    const ls = fakeStorage();
    cacheWrite(ls, sample(1_000_000));
    expect(cacheRead(ls, 1_000_000)).toEqual(sample(1_000_000));
    expect(cacheRead(ls, 1_000_000 + WEATHER_TTL_MS - 1)).toEqual(sample(1_000_000));
    expect(cacheRead(ls, 1_000_000 + WEATHER_TTL_MS)).toBeNull();
    expect(cacheRead(ls, 1_000_000 + WEATHER_TTL_MS + 1)).toBeNull();
  });

  it("空缓存 / 坏 JSON / 结构漂移 → null（不抛）", () => {
    const ls = fakeStorage();
    expect(cacheRead(ls, 0)).toBeNull(); // 空
    ls.setItem(WEATHER_CACHE_KEY, "{broken");
    expect(cacheRead(ls, 0)).toBeNull();
    ls.setItem(WEATHER_CACHE_KEY, JSON.stringify({ provider: "hack", fetchedAt: 1, tempC: 1, code: 1, days: [] }));
    expect(cacheRead(ls, 2)).toBeNull();
    ls.setItem(WEATHER_CACHE_KEY, JSON.stringify({ provider: "wttr", fetchedAt: "yesterday", tempC: 1, code: 1, days: [{}] }));
    expect(cacheRead(ls, 2)).toBeNull();
  });

  it("写缓存失败（配额满）不抛", () => {
    const boom: StorageLike = {
      getItem: () => null,
      setItem: () => {
        throw new Error("QuotaExceeded");
      },
    };
    expect(() => cacheWrite(boom, sample(1))).not.toThrow();
  });
});

describe("Z-23 配置校验与 URL 构造", () => {
  it("sanitizeConfig：非法字段逐级回退默认", () => {
    expect(sanitizeConfig(null)).toEqual(defaultWeatherConfig());
    expect(sanitizeConfig("junk")).toEqual(defaultWeatherConfig());
    expect(sanitizeConfig({ enabled: "yes", provider: "hack", location: "" })).toEqual({
      enabled: false,
      provider: "wttr",
      location: "Beijing",
    });
    expect(sanitizeConfig({ enabled: true, provider: "openmeteo", location: "not-coords" })).toEqual({
      enabled: true,
      provider: "openmeteo",
      location: "39.9,116.4", // openmeteo 位置必须是 lat,lon，否则回默认坐标
    });
    expect(sanitizeConfig({ enabled: true, provider: "openmeteo", location: " 31.2 , 121.5 " })).toEqual({
      enabled: true,
      provider: "openmeteo",
      location: "31.2 , 121.5",
    });
    // 位置超长截断到 64
    const long = sanitizeConfig({ enabled: true, provider: "wttr", location: "x".repeat(100) });
    expect(long.location.length).toBe(64);
  });

  it("配置读写往返（localStorage 桩）", () => {
    const ls = fakeStorage();
    expect(readConfig(ls)).toEqual(defaultWeatherConfig()); // 无配置 → 默认
    const cfg = { enabled: true, provider: "openmeteo" as const, location: "31.2,121.5" };
    writeConfig(ls, cfg);
    expect(ls.dump().has(WEATHER_CFG_KEY)).toBe(true);
    expect(readConfig(ls)).toEqual(cfg);
    ls.setItem(WEATHER_CFG_KEY, "{corrupt");
    expect(readConfig(ls)).toEqual(defaultWeatherConfig()); // 坏配置 → 默认
  });

  it("weatherUrl：两家供应商各自正确拼 URL", () => {
    expect(weatherUrl({ enabled: true, provider: "wttr", location: "Beijing" })).toBe(
      "https://wttr.in/Beijing?format=j1",
    );
    expect(weatherUrl({ enabled: true, provider: "wttr", location: "New York" })).toBe(
      "https://wttr.in/New%20York?format=j1",
    );
    const om = weatherUrl({ enabled: true, provider: "openmeteo", location: "39.9,116.4" });
    expect(om.startsWith("https://api.open-meteo.com/v1/forecast?")).toBe(true);
    expect(om).toContain("latitude=39.9");
    expect(om).toContain("longitude=116.4");
    expect(om).toContain("forecast_days=3");
    // 供应商非法时 sanitize 兜底为 wttr
    expect(weatherUrl({ enabled: true, provider: "openmeteo", location: "bad" })).toContain("latitude=39.9");
  });
});
