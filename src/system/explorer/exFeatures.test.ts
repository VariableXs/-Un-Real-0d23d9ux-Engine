import { describe, expect, it } from "vitest";
import {
  fromSavedView, toSavedView, highlightParts, nameIssues, sameTypeGroup,
  fuzzyScore, fuzzySuggestions, pageSlice,
} from "./exFeatures";

describe("V-31 视图记忆映射", () => {
  it("后端 → 前端", () => {
    expect(fromSavedView("icon")).toBe("icons");
    expect(fromSavedView("list")).toBe("list");
    expect(fromSavedView("column")).toBe("column");
    expect(fromSavedView("poster")).toBeNull();
  });
  it("前端 → 后端（thumbs 退化为 icon）", () => {
    expect(toSavedView("list")).toBe("list");
    expect(toSavedView("icons")).toBe("icon");
    expect(toSavedView("thumbs")).toBe("icon");
    expect(toSavedView("column")).toBe("column");
  });
});

describe("V-32 过滤高亮", () => {
  it("命中第一处（大小写不敏感）", () => {
    expect(highlightParts("Report-2026.pdf", "2026")).toEqual(["Report-", "2026", ".pdf"]);
    expect(highlightParts("Hello.txt", "hello")).toEqual(["", "Hello", ".txt"]);
  });
  it("未命中 / 空查询", () => {
    expect(highlightParts("abc", "zzz")).toBeNull();
    expect(highlightParts("abc", "  ")).toBeNull();
  });
});

describe("V-36 特殊名防呆", () => {
  it("非法字符", () => {
    expect(nameIssues('a<b>:c')).toContain("invalid:<>:");
  });
  it("保留设备名", () => {
    expect(nameIssues("CON")).toContain("reserved");
    expect(nameIssues("com1.txt")).toContain("reserved");
    expect(nameIssues("console.txt")).not.toContain("reserved");
  });
  it("结尾点/空格", () => {
    expect(nameIssues("abc. ")).toContain("trailing");
  });
  it("过长与空名", () => {
    expect(nameIssues("x".repeat(201))).toContain("too-long");
    expect(nameIssues("")).toEqual(["empty"]);
  });
  it("正常名通过", () => {
    expect(nameIssues("我的文件-v2.txt")).toEqual([]);
  });
});

describe("V-37 按类型选取", () => {
  const dir = { kind: "dir" };
  const png = { kind: "file", ext: "png" };
  const jpg = { kind: "file", ext: "jpg" };
  it("目录只匹配目录", () => {
    expect(sameTypeGroup(dir, { kind: "dir" })).toBe(true);
    expect(sameTypeGroup(dir, png)).toBe(false);
  });
  it("文件按扩展名（大小写不敏感）", () => {
    expect(sameTypeGroup(png, { kind: "file", ext: "PNG" })).toBe(true);
    expect(sameTypeGroup(png, jpg)).toBe(false);
    expect(sameTypeGroup(jpg, { kind: "file", ext: null as unknown as string })).toBe(false);
  });
});

describe("V-39 模糊跳转", () => {
  it("子序列命中与未命中", () => {
    expect(fuzzyScore("usr", "C:\\Users\\v")).not.toBeNull();
    expect(fuzzyScore("zzz", "C:\\Users\\v")).toBeNull();
  });
  it("连续/词首更优", () => {
    const cont = fuzzyScore("us", "C:\\Users")!;
    const scatter = fuzzyScore("us", "u1u2s")!;
    expect(cont).toBeLessThan(scatter);
  });
  it("候选池排序与去重", () => {
    const pool = ["C:\\Users\\v", "C:\\Users\\v", "D:\\ Videos", "E:\\Backups"];
    const out = fuzzySuggestions("users", pool);
    expect(out[0]).toBe("C:\\Users\\v");
    expect(out.length).toBe(1);
  });
  it("空查询无建议", () => {
    expect(fuzzySuggestions("  ", ["C:\\"])).toEqual([]);
  });
});

describe("U-16 分页切片", () => {
  it("越界页码收敛", () => {
    const r = pageSlice([1, 2, 3], 9, 2);
    expect(r.page).toBe(1);
    expect(r.pages).toBe(2);
    expect(r.rows).toEqual([3]);
  });
  it("空列表返回 1 页", () => {
    expect(pageSlice([], 0, 500).pages).toBe(1);
    expect(pageSlice([], 0, 500).rows).toEqual([]);
  });
  it("整页切片", () => {
    const r = pageSlice(Array.from({ length: 1200 }, (_, i) => i), 2, 500);
    expect(r.rows.length).toBe(200);
    expect(r.rows[0]).toBe(1000);
  });
});
