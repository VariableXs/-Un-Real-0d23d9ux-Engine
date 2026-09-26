import { describe, expect, it } from "vitest";
import { __clearMem, memStore } from "../internal/store";
import { activeFilter, auditShapeRedundancy, auditSingleFilterPoint, entries, requestFilter, switchLatencyMs, toGrayscale } from "../f387-grayscaleMode";

describe("F387 灰度模式", () => {
  it("默认无滤镜；开启灰度即时生效并持久化", () => {
    __clearMem();
    const s = memStore();
    expect(activeFilter(s)).toBe("none");
    const r = requestFilter(s, "grayscale");
    expect(r).toEqual({ ok: true, now: "grayscale", replaced: "none" });
    expect(activeFilter(s)).toBe("grayscale");
    expect(switchLatencyMs()).toBe(0);
  });

  it("互斥逻辑：灰度/高对比/色弱三滤镜互斥、后到替换；同滤镜再点=关闭", () => {
    __clearMem();
    const s = memStore();
    requestFilter(s, "grayscale");
    const r2 = requestFilter(s, "highContrast");
    expect(r2.now).toBe("highContrast");
    expect(r2.replaced).toBe("grayscale");
    const r3 = requestFilter(s, "highContrast");
    expect(r3.now).toBe("none"); // toggle 关
    requestFilter(s, "colorDeficiency");
    expect(activeFilter(s)).toBe("colorDeficiency");
  });

  it("滤镜单点审计：合成器一处实现，其他点位即违规（判据）", () => {
    expect(auditSingleFilterPoint(["compositor.filter"]).pass).toBe(true);
    const bad = auditSingleFilterPoint(["compositor.filter", "app-local-css-filter"]);
    expect(bad.pass).toBe(false);
    expect(bad.extraSites).toEqual(["app-local-css-filter"]);
  });

  it("形状冗余审计：纯色相状态在灰度下不可辨 = 缺陷（判据 20 任务可用性的判定面）", () => {
    const ok = auditShapeRedundancy([
      { name: "错误红+图标叉", colorOnly: false },
      { name: "成功绿+对勾", colorOnly: false },
    ]);
    expect(ok.pass).toBe(true);
    const bad = auditShapeRedundancy([{ name: "仅红点错误", colorOnly: true }]);
    expect(bad.pass).toBe(false);
    expect(bad.colorOnlyStates).toEqual(["仅红点错误"]);
  });

  it("灰度换算对拍：感知亮度加权（红/绿/蓝不同灰度）", () => {
    expect(toGrayscale(255, 0, 0)).toBe(76); // 0.299×255
    expect(toGrayscale(0, 255, 0)).toBe(150); // 0.587×255
    expect(toGrayscale(0, 0, 255)).toBe(29); // 0.114×255
    expect(toGrayscale(255, 255, 255)).toBe(255);
    expect(toGrayscale(0, 0, 0)).toBe(0);
  });

  it("双入口：快速设置磁贴 + 快捷键（判据）", () => {
    expect(entries()).toHaveLength(2);
    expect(entries()[0]).toContain("quick-settings");
  });
});
