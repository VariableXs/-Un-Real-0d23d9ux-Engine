import { describe, expect, it } from "vitest";
import { parseUniforms } from "../uniforms";

describe("N-07 parseUniforms（shader 源码注释声明 → 滑杆）", () => {
  it("解析 `uniform float uX; // min..max` 与 default 声明", () => {
    const src = `
precision mediump float;
uniform float uSpeed;      // 0..2
uniform float uBright;     // 0..1 default 0.8
uniform int uSteps;        // 1..8 default 4
uniform vec2 uSize;        // 非 float/int：不出滑杆
uniform float uNoRange;
`;
    const list = parseUniforms(src);
    expect(list.map((u) => u.name)).toEqual(["uSpeed", "uBright", "uSteps", "uNoRange"]);
    expect(list[0]).toMatchObject({ type: "float", min: 0, max: 2, value: 1 }); // 无 default → 中点
    expect(list[1]).toMatchObject({ min: 0, max: 1, value: 0.8 });
    expect(list[2]).toMatchObject({ type: "int", min: 1, max: 8, value: 4 });
    expect(list[3]).toMatchObject({ min: 0, max: 1, value: 0.5 });
  });

  it("范围写反时按 min/max 归一；同名首次出现为准", () => {
    const src = "uniform float uA; // 2..0\nuniform float uA; // 5..9";
    const list = parseUniforms(src);
    expect(list).toHaveLength(1);
    expect(list[0]).toMatchObject({ min: 0, max: 2 });
  });

  it("无声明源码返回空数组（不抛错）", () => {
    expect(parseUniforms("void main(){}")).toEqual([]);
    expect(parseUniforms("")).toEqual([]);
  });

  it("clamp default 值进范围内", () => {
    const list = parseUniforms("uniform float uX; // 0..1 default 9");
    expect(list[0]!.value).toBe(1);
  });
});