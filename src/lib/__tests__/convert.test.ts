import { describe, expect, it } from "vitest";
import {
  CONV_CATS,
  CONVERT_TABLE,
  convert,
  convertRate,
  formatConvResult,
  radixConvert,
} from "../convert";

// 8 大类各抽 2 组与权威值比对（国际定义值，非近似）
describe("convert 单位换算（权威值）", () => {
  it("长度：1 mi = 1609.344 m；1 in = 2.54 cm", () => {
    expect(convert(1, "length", "mi", "m")).toBeCloseTo(1609.344, 10);
    expect(convert(1, "length", "in", "cm")).toBeCloseTo(2.54, 10);
  });

  it("质量：1 lb = 0.45359237 kg；1 oz = 28.349523125 g", () => {
    expect(convert(1, "mass", "lb", "kg")).toBeCloseTo(0.45359237, 12);
    expect(convert(1, "mass", "oz", "g")).toBeCloseTo(28.349523125, 12);
  });

  it("温度：100°F = 37.7778°C；0°C = 273.15 K", () => {
    expect(convert(100, "temperature", "°F", "°C")).toBeCloseTo(37.7777777777778, 10);
    expect(convert(0, "temperature", "°C", "K")).toBeCloseTo(273.15, 12);
  });

  it("面积：1 mi² = 2589988.110336 m²；1 ac = 4046.8564224 m²", () => {
    expect(convert(1, "area", "mi²", "m²")).toBeCloseTo(2589988.110336, 6);
    expect(convert(1, "area", "ac", "m²")).toBeCloseTo(4046.8564224, 9);
  });

  it("体积：1 gal(US) = 3.785411784 L；1 ft³ = 28.316846592 L", () => {
    expect(convert(1, "volume", "gal(US)", "L")).toBeCloseTo(3.785411784, 12);
    expect(convert(1, "volume", "ft³", "L")).toBeCloseTo(28.316846592, 12);
  });

  it("速度：1 mph = 0.44704 m/s；1 kn = 1852/3600 m/s", () => {
    expect(convert(1, "speed", "mph", "m/s")).toBeCloseTo(0.44704, 12);
    expect(convert(1, "speed", "kn", "m/s")).toBeCloseTo(1852 / 3600, 12);
  });

  it("数据量：1 KB = 1024 B；1 GB = 1073741824 B", () => {
    expect(convert(1, "data", "KB", "B")).toBe(1024);
    expect(convert(1, "data", "GB", "B")).toBe(1073741824);
  });

  it("时间：1 h = 3600 s；1 d = 86400 s", () => {
    expect(convert(1, "time", "h", "s")).toBe(3600);
    expect(convert(1, "time", "d", "s")).toBe(86400);
  });

  it("单位表规模：8 大类、60+ 单位", () => {
    expect(CONV_CATS).toHaveLength(8);
    const total = CONV_CATS.reduce((n, c) => n + CONVERT_TABLE[c].units.length, 0);
    expect(total).toBeGreaterThanOrEqual(60);
  });

  it("未知单位如实抛错", () => {
    expect(() => convert(1, "length", "xx", "m")).toThrow();
  });
});

describe("convert 温度往返", () => {
  it("°F → °C → °F 往返还原", () => {
    const c = convert(100, "temperature", "°F", "°C");
    expect(convert(c, "temperature", "°C", "°F")).toBeCloseTo(100, 9);
  });

  it("K → °C → K 往返还原", () => {
    const c = convert(300, "temperature", "K", "°C");
    expect(convert(c, "temperature", "°C", "K")).toBeCloseTo(300, 9);
  });
});

describe("radixConvert 进制转换（64 位边界）", () => {
  it("十进制 u64 上限 ↔ 十六进制", () => {
    expect(radixConvert("18446744073709551615", 10, 16)).toBe("FFFFFFFFFFFFFFFF");
    expect(radixConvert("FFFFFFFFFFFFFFFF", 16, 10)).toBe("18446744073709551615");
  });

  it("二 / 八 / 十六互转", () => {
    expect(radixConvert("1010", 2, 10)).toBe("10");
    expect(radixConvert("255", 10, 16)).toBe("FF");
    expect(radixConvert("255", 10, 2)).toBe("11111111");
    expect(radixConvert("0x1F", 16, 10)).toBe("31"); // 宽容 0x 前缀
    expect(radixConvert("377", 8, 10)).toBe("255");
    expect(radixConvert("ff", 16, 2)).toBe("11111111"); // 小写输入
  });

  it("超上限如实抛错", () => {
    expect(() => radixConvert("18446744073709551616", 10, 2)).toThrow();
    expect(() => radixConvert("1000000000000000000000000", 10, 16)).toThrow();
    expect(() => radixConvert("FFFFFFFFFFFFFFFFF", 16, 10)).toThrow(); // 17 个 F
    expect(() => radixConvert("-1", 10, 16)).toThrow(); // 负数不在 u64 值域
  });

  it("非法字符 / 非法进制如实抛错", () => {
    expect(() => radixConvert("12", 2, 10)).toThrow(); // 二进制出现 2
    expect(() => radixConvert("1G", 16, 10)).toThrow(); // 十六进制出现 G
    expect(() => radixConvert("", 10, 16)).toThrow(); // 空串
    expect(() => radixConvert("10", 1, 10)).toThrow(); // 进制越界
    expect(() => radixConvert("10", 10, 37)).toThrow();
  });

  it("0 与边界值正常", () => {
    expect(radixConvert("0", 10, 16)).toBe("0");
    expect(radixConvert("0000", 2, 10)).toBe("0");
    expect(radixConvert("1", 10, 2)).toBe("1");
  });
});

describe("convertRate 汇率换算（基准 USD）", () => {
  const rates = { USD: 1, CNY: 7.2, EUR: 0.92, JPY: 151.5 };

  it("from 货币金额折算为 USD", () => {
    expect(convertRate(7.2, "CNY", rates)).toBeCloseTo(1, 12);
    expect(convertRate(7.2, "USD", rates)).toBeCloseTo(7.2, 12);
    expect(convertRate(92, "EUR", rates)).toBeCloseTo(100, 12);
  });

  it("目标货币换算（convertRate × rates[to]）", () => {
    // 144 CNY → USD → EUR
    expect(convertRate(144, "CNY", rates) * rates.EUR!).toBeCloseTo(0.92 * 20, 12);
    // 151500 JPY → 1000 USD
    expect(convertRate(151500, "JPY", rates)).toBeCloseTo(1000, 12);
  });

  it("缺少汇率 / 非法汇率如实抛错", () => {
    expect(() => convertRate(1, "GBP", rates)).toThrow();
    expect(() => convertRate(1, "CNY", { USD: 1, CNY: 0 })).toThrow();
  });
});

describe("formatConvResult 格式化", () => {
  it("10 位有效数字", () => {
    expect(formatConvResult(1 / 3)).toBe("0.3333333333");
    expect(formatConvResult(100 / 3)).toBe("33.33333333");
  });

  it("去尾零", () => {
    expect(formatConvResult(1609.344)).toBe("1609.344");
    expect(formatConvResult(0.1 + 0.2)).toBe("0.3");
    expect(formatConvResult(37.77777777777778)).toBe("37.77777778");
  });

  it("整数直出 / 零 / 非有限值", () => {
    expect(formatConvResult(86400)).toBe("86400");
    expect(formatConvResult(0)).toBe("0");
    expect(formatConvResult(Number.NaN)).toBe("—");
    expect(formatConvResult(1e30)).toBe("1e+30");
  });
});
