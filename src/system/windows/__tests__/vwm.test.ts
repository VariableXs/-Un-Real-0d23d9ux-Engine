import { describe, expect, it, beforeAll, beforeEach } from "vitest";
import {
  closeVwmWin,
  openVwmTpNew,
  setVwmWorkArea,
  vwmStore,
} from "../vwm";

/**
 * 批次W-1 回归：嵌入注册中心的前端语义——
 * - 第三方占位窗口强制新开实例（openVwmTpNew），embed_id 一一对应；
 * - ≥2 个第三方窗口可同时存在（双嵌入用例）；
 * - 关闭单个窗口只移除该实例，不影响其它嵌入。
 */

const wa = { x: 0, y: 0, w: 1920, h: 1046 };

describe("vwm tp 多嵌入（W-1）", () => {
  beforeAll(() => {
    // node 环境无 window：closeVwmWin 的关闭动画 setTimeout 兜底
    (globalThis as { window?: unknown }).window ??= { setTimeout };
  });
  beforeEach(() => {
    // node 环境无 localStorage（几何持久化被 vwm.ts 内部 try/catch 兜住）
    globalThis.localStorage?.clear?.();
    vwmStore.setState({ wins: [], focusedId: null, topZ: 10 });
    setVwmWorkArea(wa);
  });

  it("双嵌入：两个第三方实例 id 互不相同且同时存活", () => {
    const id1 = openVwmTpNew("tp:notepad");
    const id2 = openVwmTpNew("tp:calc");
    expect(id1).not.toBe(id2);
    const wins = vwmStore.getState().wins;
    expect(wins).toHaveLength(2);
    expect(wins.map((w) => w.id).sort()).toEqual([id1, id2].sort());
    expect(wins.map((w) => w.app)).toEqual(["tp:notepad", "tp:calc"]);
  });

  it("同款第三方重复启动也各得独立实例（多开）", () => {
    const id1 = openVwmTpNew("tp:notepad");
    const id2 = openVwmTpNew("tp:notepad");
    expect(id1).not.toBe(id2);
    expect(vwmStore.getState().wins).toHaveLength(2);
  });

  it("关闭单个占位窗口不影响另一个嵌入实例", async () => {
    const id1 = openVwmTpNew("tp:notepad");
    const id2 = openVwmTpNew("tp:calc");
    closeVwmWin(id1);
    // 关闭动画（setTimeout 170ms）后真正移除——同步等定时器清场再断言
    await new Promise((r) => setTimeout(r, 200));
    const wins = vwmStore.getState().wins;
    expect(wins).toHaveLength(1);
    expect(wins[0]?.id).toBe(id2);
  });
});
