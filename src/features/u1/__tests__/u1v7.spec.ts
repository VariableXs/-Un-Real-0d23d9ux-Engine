/**
 * v7 深化接线单测（四面板 × 内核新参数）——判据的执行器。
 * 覆盖：F403 解锁退避 / F405 关闭链与确认倒计时 / F408 自定义项 /
 * F416 搜索框状态机 / F401 铺排与框选与批量落点 / F404 标签生命周期 /
 * F424 IME 组合期门与焦点归还 / F450 回弹键、锁存超时、提示音账。
 * 参数值与 kernel/varix/src/uni1/ v7 语义核同源（一处一事实）。
 */

import { describe, expect, it } from "vitest";
import {
  POWER_CONFIRM_MS, SEARCH_MAX_BYTES, UNLOCK_BACKOFF_CAP_MS, WINX_CUSTOM_CAP,
  WINX_ITEMS, closeChainResolve, closeChainStep, powerConfirmPermille, powerNeedsConfirm,
  startEscStep, startSubmit, startTypeChar, unlockAttempt, unlockBackoffMs,
  winXAddCustom,
  type CloseChainState, type StartSearchState, type WinXItem,
} from "../hotkeys";
import {
  WIN_E_TAB_CAP, batchMoveTargets, flowCell, marqueeSelect, winECloseTab, winEOpenTab,
} from "../explorerkeys";
import { escDispatch, focusReturnOnPeel } from "../menus";
import {
  BOUNCE_REARM_MS, STICKY_LATCH_TIMEOUT_MS, bouncePress, latchExpired, soundRecord,
  type BounceState, type SoundLedger,
} from "../docops";

describe("v7 F403 解锁退避（hotkeys 面板）", () => {
  it("退避表翻倍封顶 + 窗内正确密码诚实拒绝", () => {
    expect(unlockBackoffMs(0)).toBe(0);
    expect(unlockBackoffMs(1)).toBe(1000);
    expect(unlockBackoffMs(3)).toBe(4000);
    expect(unlockBackoffMs(6)).toBe(UNLOCK_BACKOFF_CAP_MS);
    expect(unlockBackoffMs(9)).toBe(UNLOCK_BACKOFF_CAP_MS);
    let s = { failures: 0, lockedOutUntil: null as number | null };
    let r = unlockAttempt(s, 1000, false);
    expect(r.ok).toBe(false);
    s = r.next;
    expect(s.lockedOutUntil).toBe(2000);
    // 窗内正确密码被拒（lockout 归因）。
    r = unlockAttempt(s, 1500, true);
    expect(r.ok).toBe(false);
    expect(r.reason).toBe("lockout");
    // 窗外正确密码解锁归零。
    r = unlockAttempt(s, 2000, true);
    expect(r.ok).toBe(true);
    expect(r.next).toEqual({ failures: 0, lockedOutUntil: null });
  });
});

describe("v7 F405 关闭链与确认倒计时（hotkeys 面板）", () => {
  it("关闭链从栈顶逐个关，脏窗三问，取消放弃余下", () => {
    let s: CloseChainState = { pending: ["w1", "w2", "w3"], activeAsk: null, closed: 0, abandoned: 0 };
    let r = closeChainStep(s, false);
    expect(r).toMatchObject({ kind: "closing", id: "w1" });
    s = r.next;
    r = closeChainStep(s, true);
    expect(r).toMatchObject({ kind: "ask", id: "w2" });
    s = r.next;
    const d = closeChainResolve(s, false);
    expect(d.kind).toBe("done");
    expect(d.next.abandoned).toBe(1);
    expect(d.next.closed).toBe(1);
  });
  it("确认倒计时：睡眠免确认；按满 2s 千分比；空态归零", () => {
    expect(powerNeedsConfirm("睡眠")).toBe(false);
    expect(powerNeedsConfirm("关机")).toBe(true);
    expect(POWER_CONFIRM_MS).toBe(2000);
    expect(powerConfirmPermille(1000, 2000)).toBe(500);
    expect(powerConfirmPermille(1000, 3000)).toBe(1000);
    expect(powerConfirmPermille(null, 9999)).toBe(0);
  });
});

describe("v7 F408 自定义项（hotkeys 面板）", () => {
  it("三检拒绝 + 上限 4 + 钉选生效", () => {
    let r = winXAddCustom(WINX_ITEMS, [], { letter: "j", label: "备份", page: "backup" });
    expect(r.ok).toBe(true);
    expect(r.customs).toEqual([{ letter: "j", label: "备份", page: "backup" }]);
    expect(winXAddCustom(WINX_ITEMS, [{ letter: "b", label: "x", page: "y" }], { letter: "b", label: "z", page: "z2" })).toEqual({ ok: false, reason: "letter-taken" });
    expect(winXAddCustom(WINX_ITEMS, [], { letter: "q", label: "x", page: "taskmgr" })).toEqual({ ok: false, reason: "page-dup" });
    const four: WinXItem[] = [
      { letter: "b", label: "1", page: "p1" },
      { letter: "n", label: "2", page: "p2" },
      { letter: "m", label: "3", page: "p3" },
      { letter: "q", label: "4", page: "p4" },
    ];
    expect(winXAddCustom(WINX_ITEMS, four, { letter: "j", label: "5", page: "p5" })).toEqual({ ok: false, reason: "cap-full" });
    expect(WINX_CUSTOM_CAP).toBe(4);
  });
});

describe("v7 F416 搜索框状态机（hotkeys 面板）", () => {
  it("打字进框 + 容量诚实拒绝（字节计长）", () => {
    const s: StartSearchState = { open: true, text: "", composing: false };
    const t1 = startTypeChar(s, "v");
    expect(t1.ok).toBe(true);
    expect(t1.next.text).toBe("v");
    // 组合期不接收。
    expect(startTypeChar({ open: true, text: "", composing: true }, "x").ok).toBe(false);
    // 64 字节上限：63 字节后 1 字节放行、再 1 字节拒绝。
    const almost = "字".repeat(21); // 63 字节
    expect(startTypeChar({ open: true, text: almost, composing: false }, "x").ok).toBe(true);
    expect(startTypeChar({ open: true, text: almost + "x", composing: false }, "y").ok).toBe(false);
    expect(SEARCH_MAX_BYTES).toBe(64);
  });
  it("Esc 分段：组合取消 → 清文本 → 关菜单；空查询不提交", () => {
    const comp = startEscStep({ open: true, text: "v", composing: true });
    expect(comp.kind).toBe("ime-cancel");
    expect(comp.next.composing).toBe(false);
    expect(comp.next.open).toBe(true);
    expect(comp.next.text).toBe("v");
    const c1 = startEscStep({ open: true, text: "hi", composing: false });
    expect(c1.kind).toBe("clear");
    const c2 = startEscStep({ open: true, text: "", composing: false });
    expect(c2.kind).toBe("close");
    expect(startEscStep({ open: false, text: "", composing: false }).kind).toBe("noop");
    // Enter：提交清框菜单保持；空查询无动作。
    const sub = startSubmit({ open: true, text: "varix", composing: false });
    expect(sub).toEqual({ query: "varix", next: { open: true, text: "", composing: false } });
    expect(startSubmit({ open: true, text: "", composing: false }).query).toBeNull();
  });
});

describe("v7 F401/F404 桌面与标签（explorerkeys 面板）", () => {
  it("铺排方向：列优先/行优先换算 + 框选 + 批量落点让位", () => {
    expect(flowCell(5, 3, 6, "column")).toEqual({ col: 2, row: 1 });
    expect(flowCell(7, 3, 6, "row")).toEqual({ col: 1, row: 1 });
    const placed = [
      { id: 1, col: 0, row: 0 }, { id: 2, col: 1, row: 0 },
      { id: 3, col: 0, row: 1 }, { id: 4, col: 1, row: 1 },
    ];
    expect(marqueeSelect(placed, { col: 0, row: 0 }, { col: 1, row: 1 })).toEqual([1, 2, 3, 4]);
    // 批量落点：占用格跳过、网格内回绕。
    const targets = batchMoveTargets(new Set(["0,0", "1,0"]), 3, { col: 0, row: 0 }, 3, 6, "column");
    expect(targets).toEqual([{ col: 2, row: 0 }, { col: 0, row: 1 }, { col: 1, row: 1 }]);
  });
  it("标签生命周期：去重跳转 + 上限诚实拒绝 + 关标签激活规则", () => {
    const o1 = winEOpenTab(["此机"], "D:/资料");
    expect(o1.kind).toBe("opened");
    const o2 = winEOpenTab(o1.tabs, "D:/资料");
    expect(o2.kind).toBe("jumped");
    expect(o2.tabs).toHaveLength(2);
    // 上限 32。
    const full = Array.from({ length: WIN_E_TAB_CAP }, (_, i) => `p${i}`);
    expect(winEOpenTab(full, "overflow").kind).toBe("cap-full");
    // 关标签：右邻接管；末位左邻；最后一张关窗。
    const c1 = winECloseTab(["此机", "A1", "A2"], 0);
    expect(c1).toEqual({ tabs: ["A1", "A2"], active: 0, windowClosed: false });
    const c2 = winECloseTab(["此机", "A1"], 1);
    expect(c2).toEqual({ tabs: ["此机"], active: 0, windowClosed: false });
    const c3 = winECloseTab(["此机"], 0);
    expect(c3).toEqual({ tabs: [], active: 0, windowClosed: true });
  });
});

describe("v7 F424 IME 组合期门与焦点归还（menus 面板）", () => {
  it("组合期 Esc 先取消组合（层不动）→ 再按剥层 → 空栈无动作", () => {
    const stack = ["modal", "panel", "popup"] as const;
    const s1 = escDispatch([...stack], true);
    expect(s1.kind).toBe("ime-cancel");
    expect(s1.next.composing).toBe(false);
    const s2 = escDispatch([...stack], false);
    expect(s2).toMatchObject({ kind: "peel", peeled: "popup" });
    expect(escDispatch([], false).kind).toBe("noop");
  });
  it("焦点归还：登记层归还、未登记层不虚计", () => {
    const registry = { dlg: "doc.editor" };
    expect(focusReturnOnPeel(registry, "dlg")).toBe("doc.editor");
    expect(focusReturnOnPeel(registry, "tooltip")).toBeNull();
  });
});

describe("v7 F450 回弹键/锁存超时/提示音账（docops 面板）", () => {
  it("同键窗内连击抑制，异键放行，窗外恢复", () => {
    let s: BounceState = { enabled: true, rearmMs: BOUNCE_REARM_MS, lastAt: {}, suppressed: 0 };
    let r = bouncePress(s, "a", 0);
    expect(r.pass).toBe(true);
    s = r.next;
    expect(bouncePress(s, "a", 100).pass).toBe(false);
    expect(bouncePress(s, "b", 110).pass).toBe(true);
    expect(bouncePress(s, "a", 201).pass).toBe(true);
    // 关闭时全通。
    const off: BounceState = { enabled: false, rearmMs: BOUNCE_REARM_MS, lastAt: { a: 0 }, suppressed: 0 };
    expect(bouncePress(off, "a", 0).pass).toBe(true);
  });
  it("锁存超时：恰达线不清、超窗清锁、可关", () => {
    expect(latchExpired(1000, 2, 1000 + STICKY_LATCH_TIMEOUT_MS, STICKY_LATCH_TIMEOUT_MS)).toBe(0);
    expect(latchExpired(1000, 2, 1000 + STICKY_LATCH_TIMEOUT_MS + 1, STICKY_LATCH_TIMEOUT_MS)).toBe(2);
    expect(latchExpired(1000, 1, 99_999_999, null)).toBe(0);
    expect(latchExpired(null, 1, 99_999_999, STICKY_LATCH_TIMEOUT_MS)).toBe(0);
  });
  it("提示音账：静音期记 skip 不进环，解除即恢复", () => {
    let s: SoundLedger = { muted: false, events: [], mutedSkips: 0 };
    s = soundRecord(s, 10, "latch");
    s = { ...s, muted: true };
    s = soundRecord(s, 20, "reject");
    s = { ...s, muted: false };
    s = soundRecord(s, 30, "accept");
    expect(s.events).toHaveLength(2);
    expect(s.mutedSkips).toBe(1);
    expect(s.events[0]).toEqual({ atMs: 10, kind: "latch" });
  });
});
