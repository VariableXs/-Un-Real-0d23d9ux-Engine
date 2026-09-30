/**
 * H4 深化批次九（v9）深测：快速设置磁贴纯模型（quickTiles.ts）+ 滤镜通道唯一化（h4ui → h4Bus）。
 * 判据锚定：F387「快速设置磁贴+快捷键双入口」（磁贴入口补缺）/ 三滤镜互斥 / 单点应用。
 */
import { describe, expect, it } from "vitest";
import * as grayscale from "../../../system/h4/f387-grayscaleMode";
import { memStore, __clearMem } from "../../../system/h4/internal/store";
import { buildQuickTiles, toggleGrayscale } from "../quickTiles";
import { h4Bus } from "../bus";

const store = (): ReturnType<typeof memStore> => {
  __clearMem();
  return memStore();
};

describe("v9 快速设置磁贴：模型与双入口判据", () => {
  it("三枚磁贴：灰度（引擎态投影）+ 拾色器/标尺呼出（v10 起走 h4Bus 契约事件）", () => {
    const none = buildQuickTiles("none");
    expect(none.map((t) => t.id)).toEqual(["grayscale", "picker", "ruler"]);
    expect(none[0]!.pressed).toBe(false);
    expect(none[1]!.summon).toBe("settings.summon-picker");
    expect(none[2]!.summon).toBe("settings.summon-ruler");
    const gray = buildQuickTiles("grayscale");
    expect(gray[0]!.pressed).toBe(true);
    expect(gray[0]!.summon).toBeNull(); // 磁贴是开关不是呼出——动作语义分明
    // 按压态是滤镜位投影：其他滤镜在位时灰度磁贴不亮（互斥单点）
    expect(buildQuickTiles("highContrast")[0]!.pressed).toBe(false);
  });

  it("toggle 语义：无滤镜→开、灰度→关（同一磁贴 toggle）", () => {
    const s = store();
    const on = toggleGrayscale(s);
    expect(on.now).toBe("grayscale");
    expect(on.replaced).toBe("none");
    const off = toggleGrayscale(s);
    expect(off.now).toBe("none");
    expect(off.replaced).toBe("grayscale");
  });

  it("互斥替换：高对比度在位时开灰度 = 替换（三滤镜互斥判据经磁贴路径复测）", () => {
    const s = store();
    grayscale.requestFilter(s, "highContrast");
    const r = toggleGrayscale(s);
    expect(r.now).toBe("grayscale");
    expect(r.replaced).toBe("highContrast");
    expect(grayscale.activeFilter(s)).toBe("grayscale");
  });

  it("通道唯一化：announceFilterChanged 走 h4Bus（v9 迁移自 window CustomEvent 双轨）", () => {
    const s = store();
    const seen: Array<{ name: string; payload: unknown }> = [];
    const off = h4Bus.on("f387.filter-changed", (p) => seen.push({ name: "f387.filter-changed", payload: p }));
    toggleGrayscale(s);
    off();
    expect(seen.length).toBe(1);
    expect((seen[0]!.payload as { now: string }).now).toBe("grayscale");
    // v10 通道唯一化：toggle 全程只走 bus——window 双轨（vx-h4-*）零派发
    expect(seen.length).toBe(1);
  });
});
