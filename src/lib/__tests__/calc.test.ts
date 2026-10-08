import { describe, expect, it } from "vitest";
import { calcEval, progEval, progFormat, progParse, unitConvert, fmtResult } from "../calc";

describe("calcEval 标准/科学", () => {
  it("四则与优先级", () => {
    expect(calcEval("1+2*3")).toBe(7);
    expect(calcEval("(1+2)*3")).toBe(9);
    expect(calcEval("10/4")).toBe(2.5);
    expect(calcEval("2^3^2")).toBeCloseTo(512); // 右结合：2^(3^2)
  });

  it("一元负号、百分比、阶乘", () => {
    expect(calcEval("-3+5")).toBe(2);
    expect(calcEval("50%*200")).toBeCloseTo(100);
    expect(calcEval("5!")).toBe(120);
    expect(calcEval("--4")).toBe(4);
  });

  it("函数与常量（角度/弧度）", () => {
    expect(calcEval("sin(30)")).toBeCloseTo(0.5);
    expect(calcEval("sin(pi/6)", "rad")).toBeCloseTo(0.5);
    expect(calcEval("ln(e)")).toBeCloseTo(1);
    expect(calcEval("log(1000)")).toBeCloseTo(3);
    expect(calcEval("sqrt(9)")).toBe(3);
  });

  it("非法表达式如实抛错", () => {
    expect(() => calcEval("1++")).toThrow();
    expect(() => calcEval("(1+2")).toThrow();
    expect(() => calcEval("foo(2)")).toThrow();
    expect(() => calcEval("(-1)!")).toThrow(); // 阶乘不支持负数
    expect(calcEval("-5!")).toBe(-120); // -(5!)
  });
});

describe("progEval 程序员模式", () => {
  it("解析与多进制展示", () => {
    expect(progParse("ff", 16)).toBe(255n);
    expect(progParse("1010", 2)).toBe(10n);
    expect(progParse("-17", 8)).toBe(-15n);
    expect(progFormat(255n)).toEqual({ hex: "ff", dec: "255", oct: "377", bin: "11111111" });
  });

  it("位运算与算术（64 位环绕）", () => {
    expect(progEval(0b1100n, "and", 0b1010n)).toBe(0b1000n);
    expect(progEval(0b1100n, "or", 0b1010n)).toBe(0b1110n);
    expect(progEval(0b1100n, "xor", 0b1010n)).toBe(0b0110n);
    expect(progEval(1n, "shl", 10n)).toBe(1024n);
    expect(progEval(1024n, "shr", 3n)).toBe(128n);
    expect(progEval(7n, "div", 2n)).toBe(3n);
    expect(() => progEval(1n, "div", 0n)).toThrow();
  });

  it("64 位有符号环绕", () => {
    const max = (1n << 63n) - 1n;
    expect(progEval(max, "add", 1n)).toBe(-9223372036854775808n);
    expect(progEval(0n, "not")).toBe(-1n);
  });
});

describe("unitConvert 单位换算", () => {
  it("倍率类", () => {
    expect(unitConvert(1, "length", "km", "m")).toBe(1000);
    expect(unitConvert(1024, "data", "KB", "B")).toBe(1048576);
    expect(unitConvert(36, "speed", "km/h", "m/s")).toBeCloseTo(10);
  });

  it("温度经摄氏中转", () => {
    expect(unitConvert(100, "temperature", "°C", "°F")).toBeCloseTo(212);
    expect(unitConvert(32, "temperature", "°F", "°C")).toBeCloseTo(0);
    expect(unitConvert(0, "temperature", "°C", "K")).toBeCloseTo(273.15);
  });

  it("未知单位抛错", () => {
    expect(() => unitConvert(1, "length", "km", "lb")).toThrow();
  });
});

describe("fmtResult", () => {
  it("整数与有限小数", () => {
    expect(fmtResult(42)).toBe("42");
    expect(fmtResult(0.1 + 0.2)).toBe("0.3");
  });
  it("非有限值如实标注", () => {
    expect(fmtResult(Infinity)).toBe("∞");
    expect(fmtResult(NaN)).toBe("错误");
  });
});
