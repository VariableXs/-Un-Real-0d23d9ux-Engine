import { describe, expect, it } from "vitest";
import { MEMORY_CAP, auditEviction, defaultPosition, fitOnScreen } from "../f382-dialogPosition";
import { placementFor, rememberPosition } from "../f382-dialogPosition";
import { __clearMem, memStore } from "../internal/store";

describe("F382 对话框位置记忆", () => {
  it("居中偏上 1/3 基准（判据）", () => {
    expect(defaultPosition({ w: 1920, h: 1080 }, { w: 600, h: 400 })).toEqual({ x: 660, y: 160 });
    expect(defaultPosition({ w: 800, h: 300 }, { w: 900, h: 400 })).toEqual({ x: 0, y: 0 }); // 屏比框小 → 钳 0
  });

  it("拖动记忆（同类归组）：save 组记住右下习惯；open 组互不串", () => {
    __clearMem();
    const s = memStore();
    expect(rememberPosition("save", 1300, 700, 1, s)).toBe(true);
    const save = placementFor("save", { w: 1920, h: 1080 }, { w: 600, h: 400 }, s);
    expect(save).toEqual({ x: 1300, y: 700, fromMemory: true });
    const open = placementFor("open", { w: 1920, h: 1080 }, { w: 600, h: 400 }, s);
    expect(open.fromMemory).toBe(false);
    expect(open.y).toBe(160);
  });

  it("未拖过走基准、拖过走记忆（判据双路）", () => {
    __clearMem();
    const s = memStore();
    const first = placementFor("prompt", { w: 1920, h: 1080 }, { w: 400, h: 200 }, s);
    expect(first.fromMemory).toBe(false);
    rememberPosition("prompt", 100, 50, 1, s);
    expect(placementFor("prompt", { w: 1920, h: 1080 }, { w: 400, h: 200 }, s)).toEqual({ x: 100, y: 50, fromMemory: true });
  });

  it("跨屏完整落屏判据：记忆位置越界 → 钳回焦点屏整体落屏", () => {
    const r = fitOnScreen({ x: 2400, y: 900 }, { w: 600, h: 400 }, { x: 0, y: 0, w: 1920, h: 1080 });
    expect(r).toEqual({ x: 1320, y: 680, clamped: true });
    const fine = fitOnScreen({ x: 500, y: 300 }, { w: 600, h: 400 }, { x: 0, y: 0, w: 1920, h: 1080 });
    expect(fine.clamped).toBe(false);
  });

  it(`记忆容量上限与淘汰：超 ${MEMORY_CAP} 组淘汰最久未用（判据）`, () => {
    __clearMem();
    const s = memStore();
    for (let i = 0; i < MEMORY_CAP + 3; i++) rememberPosition(`group${i}`, i * 10, i * 10, i + 1, s);
    const a = auditEviction(s);
    expect(a.size).toBe(MEMORY_CAP);
    expect(a.oldestSurvivor).toBe("group3");
  });
});
