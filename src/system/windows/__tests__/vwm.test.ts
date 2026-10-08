import { describe, expect, it, beforeAll, beforeEach } from "vitest";
import {
  closeVwmWin,
  focusVwmWin,
  hideVwmWin,
  hiddenVwmWins,
  minimizeVwmWin,
  openVwmTpNew,
  setVwmWorkArea,
  unhideAllVwm,
  unhideVwmWin,
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

describe("vwm 隐藏窗口状态机（批次F）", () => {
  beforeAll(() => {
    (globalThis as { window?: unknown }).window ??= { setTimeout };
  });
  beforeEach(() => {
    globalThis.localStorage?.clear?.();
    vwmStore.setState({ wins: [], focusedId: null, topZ: 10 });
    setVwmWorkArea(wa);
  });

  it("隐藏：hidden=true、焦点让给其余可见窗口、重复隐藏幂等", () => {
    const id1 = openVwmTpNew("tp:a");
    const id2 = openVwmTpNew("tp:b");
    const beforeZ = vwmStore.getState().topZ;
    hideVwmWin(id1);
    let s = vwmStore.getState();
    expect(s.wins.find((w) => w.id === id1)?.hidden).toBe(true);
    // 焦点让给剩余可见窗口 b（z 更高者）
    expect(s.focusedId).toBe(id2);
    expect(s.wins.find((w) => w.id === id2)?.hidden ?? false).toBe(false);
    // 幂等：再次隐藏不改变任何状态
    hideVwmWin(id1);
    s = vwmStore.getState();
    expect(s.wins.find((w) => w.id === id1)?.hidden).toBe(true);
    expect(s.wins.find((w) => w.id === id2)?.z ?? 0).toBeLessThanOrEqual(beforeZ + 1);
    // hiddenVwmWins 只列隐藏窗口
    expect(hiddenVwmWins().map((w) => w.id)).toEqual([id1]);
  });

  it("隐藏的是唯一窗口时焦点置空（不残留幽灵焦点）", () => {
    const id = openVwmTpNew("tp:solo");
    hideVwmWin(id);
    expect(vwmStore.getState().focusedId).toBeNull();
  });

  it("隐藏最小化窗口时清除最小化态（恢复时不带双重态）", async () => {
    const id = openVwmTpNew("tp:a");
    minimizeVwmWin(id);
    await new Promise((r) => setTimeout(r, 250)); // 等飞行动画落地
    hideVwmWin(id);
    let w = vwmStore.getState().wins.find((x) => x.id === id);
    expect(w?.hidden).toBe(true);
    expect(w?.minimized).toBe(false);
    expect(w?.minimizedAt).toBeNull();
    unhideVwmWin(id);
    w = vwmStore.getState().wins.find((x) => x.id === id);
    expect(w?.hidden).toBe(false);
    expect(w?.minimized).toBe(false); // 恢复 = 显示，不是还原到最小化
    expect(vwmStore.getState().focusedId).toBe(id);
  });

  it("恢复：置顶（z 提升）并聚焦", () => {
    const id1 = openVwmTpNew("tp:a");
    const id2 = openVwmTpNew("tp:b");
    hideVwmWin(id1);
    const zBefore = vwmStore.getState().topZ;
    unhideVwmWin(id1);
    const s = vwmStore.getState();
    expect(s.wins.find((w) => w.id === id1)?.hidden).toBe(false);
    expect(s.wins.find((w) => w.id === id1)?.z).toBe(zBefore + 1);
    expect(s.focusedId).toBe(id1);
    // 幂等：恢复未隐藏窗口无副作用
    const snap = JSON.stringify(vwmStore.getState().wins);
    unhideVwmWin(id1);
    expect(JSON.stringify(vwmStore.getState().wins)).toBe(snap);
    expect(id2).toBeTruthy();
  });

  it("focusVwmWin 对隐藏窗口 = 恢复（任务栏点击隐藏窗口即召回）", () => {
    const id = openVwmTpNew("tp:a");
    hideVwmWin(id);
    focusVwmWin(id);
    const s = vwmStore.getState();
    expect(s.wins.find((w) => w.id === id)?.hidden).toBe(false);
    expect(s.focusedId).toBe(id);
  });

  it("unhideAll：全部恢复、逐层置顶、聚焦最后隐藏者、返回正确计数", () => {
    const id1 = openVwmTpNew("tp:a");
    const id2 = openVwmTpNew("tp:b");
    const id3 = openVwmTpNew("tp:c");
    hideVwmWin(id1);
    hideVwmWin(id2);
    hideVwmWin(id3);
    expect(hiddenVwmWins()).toHaveLength(3);
    const n = unhideAllVwm();
    expect(n).toBe(3);
    const s = vwmStore.getState();
    expect(s.wins.every((w) => !w.hidden)).toBe(true);
    expect(s.focusedId).toBe(id3); // 最后隐藏者优先召回
    // 逐层置顶：三个窗口 z 互不相同且均高于隐藏前的 topZ 基线附近
    const zs = s.wins.map((w) => w.z);
    expect(new Set(zs).size).toBe(3);
    // 无隐藏时返回 0（幂等）
    expect(unhideAllVwm()).toBe(0);
  });

  it("隐藏窗口不参与焦点接力（nextFocus 跳过 hidden）", () => {
    const id1 = openVwmTpNew("tp:a");
    const id2 = openVwmTpNew("tp:b");
    const id3 = openVwmTpNew("tp:c");
    hideVwmWin(id3); // z 最高的窗口被隐藏
    hideVwmWin(id2);
    // 关掉 id1 时焦点应落到 null（其余窗口全隐藏），而不是隐藏窗口
    focusVwmWin(id1);
    hideVwmWin(id1);
    expect(vwmStore.getState().focusedId).toBeNull();
  });
});
