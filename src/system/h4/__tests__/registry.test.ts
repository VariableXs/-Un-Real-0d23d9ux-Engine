import { describe, expect, it } from "vitest";
import { H4_REGISTRY, auditRegistry, registryAsTitles } from "../registry";

describe("H4 批次登记册", () => {
  it("登记册完整性自检：50 项连续编号、文件名挂编号（账册检对齐的「册」）", () => {
    expect(auditRegistry()).toEqual({ pass: true, problems: [] });
    expect(H4_REGISTRY).toHaveLength(50);
  });

  it("每项判据摘文非空且含分号分隔的判据组", () => {
    expect(H4_REGISTRY.every((e) => e.criteria.length > 10)).toBe(true);
    expect(H4_REGISTRY.every((e) => e.criteria.includes("；") || e.criteria.length > 30)).toBe(true);
  });

  it("一行账数据源与登记册同源（50 项）", () => {
    const titles = registryAsTitles();
    expect(titles).toHaveLength(50);
    expect(titles[0]).toEqual({ item: "F351", title: "工作区快照" });
    expect(titles[49]).toEqual({ item: "F400", title: "H 域收官登记" });
  });
});
