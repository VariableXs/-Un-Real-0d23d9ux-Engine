import { describe, expect, it } from "vitest";
import { DOCK_SNAP_PX, PIP_BASE, PIP_CONCURRENCY_CAP, applyDocking, enterPip, exitPip, focusPolicy, setTier, togglePlay, type PiPRegistry, type PipSession } from "../f358-globalPip";

const SCREEN = { w: 1920, h: 1080 };

function reg(): PiPRegistry {
  return { sessions: [], queue: [] };
}

describe("F358 全局画中画", () => {
  it("基础尺寸 240×135；进入 PiP 得到该几何", () => {
    expect(PIP_BASE).toEqual({ x: 0, y: 0, w: 240, h: 135 });
    const r = enterPip(reg(), "v1", "教程视频", 123.5);
    expect(r.outcome).toBe("entered");
    expect(r.session!.rect).toEqual(PIP_BASE);
    expect(r.session!.resumeAtSec).toBe(123.5);
  });

  it("并发上限 2：第三个提示排队；退出后队首递补（FIFO）", () => {
    let r = reg();
    r = enterPip(r, "a", "A", 0).registry;
    r = enterPip(r, "b", "B", 0).registry;
    const third = enterPip(r, "c", "C", 0);
    expect(third.outcome).toBe("queued");
    expect(third.session).toBeNull();
    expect(third.registry.queue).toEqual(["c"]);
    const out = exitPip(third.registry, "a");
    expect(out.promoted).toBe("c");
    expect(out.registry.sessions.map((s) => s.winId)).toEqual(["b", "c"]);
    expect(out.registry.queue).toEqual([]);
  });

  it("四键功能：播放/暂停切换；关闭走 exitPip 并带回接续状态（时间点不跳）", () => {
    let r = enterPip(reg(), "a", "A", 42.7).registry;
    expect(r.sessions[0]!.playing).toBe(true);
    r = togglePlay(r, "a");
    expect(r.sessions[0]!.playing).toBe(false);
    const out = exitPip(r, "a");
    expect(out.resumed).toEqual({ winId: "a", resumeAtSec: 42.7, playing: false });
  });

  it("停靠吸附：阈值内落四角贴边；自由位保留 docked=null", () => {
    const nearCorner: PipSession = { winId: "a", sourceTitle: "A", rect: { x: DOCK_SNAP_PX - 1, y: 3, w: 240, h: 135 }, tier: 1, docked: null, resumeAtSec: 0, playing: true };
    const docked = applyDocking(nearCorner, SCREEN);
    expect(docked.docked).toBe("topLeft");
    expect(docked.rect).toEqual({ x: 0, y: 0, w: 240, h: 135 });
    const free = applyDocking({ ...nearCorner, rect: { x: 500, y: 500, w: 240, h: 135 } }, SCREEN);
    expect(free.docked).toBeNull();
    const br = applyDocking({ ...nearCorner, rect: { x: SCREEN.w - 240 + 2, y: SCREEN.h - 135 - 2, w: 240, h: 135 } }, SCREEN);
    expect(br.docked).toBe("bottomRight");
    expect(br.rect.x).toBe(SCREEN.w - 240);
  });

  it("缩放三档：停靠角锚定重排不越屏；档位非法保持原样", () => {
    let r = enterPip(reg(), "a", "A", 0).registry;
    r = setTier(r, "a", 2, SCREEN);
    expect(r.sessions[0]!.rect).toEqual({ x: 0, y: 0, w: 480, h: 270 });
    const bad = setTier(r, "a", 9 as 1, SCREEN);
    expect(bad.sessions[0]!.tier).toBe(2);
    const r2 = setTier(enterPip(reg(), "b", "B", 0).registry, "b", 1.5, { w: 400, h: 300 });
    expect(r2.sessions[0]!.rect.w).toBe(360);
    expect(r2.sessions[0]!.rect.h).toBeLessThanOrEqual(300);
  });

  it("焦点语义：置顶不抢焦点（F248）", () => {
    const p = focusPolicy();
    expect(p.alwaysOnTop).toBe(true);
    expect(p.stealsFocus).toBe(false);
  });

  it("重复进入同一窗口幂等拒绝；并发上限常量=2", () => {
    let r = enterPip(reg(), "a", "A", 0).registry;
    const again = enterPip(r, "a", "A", 5);
    expect(again.outcome).toBe("already");
    expect(again.registry.sessions).toHaveLength(1);
    expect(PIP_CONCURRENCY_CAP).toBe(2);
  });
});
