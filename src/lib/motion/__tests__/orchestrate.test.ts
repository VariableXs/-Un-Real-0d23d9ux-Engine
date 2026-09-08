import { describe, expect, it, vi } from "vitest";
import {
  FRAME_BUDGET_MS,
  REDUCE_MS,
  STAGGER_MS,
  flipMeasure,
  motionBudgetExceeded,
  orchestrate,
  resetMotionBudget,
} from "../orchestrate";
import type { OrchestrateTarget } from "../orchestrate";

// node 环境无 DOM：用最小桩充当 HTMLElement（编排器只依赖 rect/style/rAF 表面）
function fakeEl(rect: { left: number; top: number }): HTMLElement {
  return {
    getBoundingClientRect: () => ({ left: rect.left, top: rect.top }),
    style: {} as CSSStyleDeclaration,
    addEventListener: () => {},
    removeEventListener: () => {},
  } as unknown as HTMLElement;
}

function target(n: number, spy: (i: number) => void): OrchestrateTarget[] {
  return Array.from({ length: n }, (_, i) => ({ el: fakeEl({ left: 0, top: 0 }), play: () => spy(i) }));
}

describe("N-06 orchestrate", () => {
  it("空目标不产生任何调度", () => {
    const raf = vi.fn((cb: () => void) => 1);
    orchestrate([], { raf });
    expect(raf).not.toHaveBeenCalled();
  });

  it("批量动效按 30ms 错峰（第 i 个延迟 i*30）", () => {
    vi.useFakeTimers();
    const played: number[] = [];
    orchestrate(target(3, (i) => played.push(i)), { kind: "spring" });
    vi.advanceTimersByTime(1);
    expect(played).toEqual([0]);
    vi.advanceTimersByTime(STAGGER_MS);
    expect(played).toEqual([0, 1]);
    vi.advanceTimersByTime(STAGGER_MS);
    expect(played).toEqual([0, 1, 2]);
    vi.useRealTimers();
  });

  it("instant 档合并到单 rAF 批次直接落位", () => {
    const rafCbs: (() => void)[] = [];
    const raf = (cb: () => void) => {
      rafCbs.push(cb);
      return 1;
    };
    const played: number[] = [];
    orchestrate(target(4, (i) => played.push(i)), { kind: "instant", raf });
    expect(rafCbs).toHaveLength(1);
    rafCbs[0]();
    expect(played).toEqual([0, 1, 2, 3]);
  });

  it("reduce-motion：错峰压缩为 0、时长 ≤80ms（契约：全部动效 ≤80ms 或消失）", () => {
    vi.useFakeTimers();
    const played: number[] = [];
    orchestrate(target(3, (i) => played.push(i)), { reduceMotion: true });
    vi.advanceTimersByTime(1);
    expect(played).toEqual([0, 1, 2]);
    vi.useRealTimers();
    expect(REDUCE_MS).toBeLessThanOrEqual(80);
  });

  it("帧预算超支静默降级并计数", () => {
    resetMotionBudget();
    const raf = (cb: () => void) => {
      cb();
      return 1;
    };
    const t = target(1, () => {
      const start = performance.now();
      while (performance.now() - start < FRAME_BUDGET_MS + 2) {
        /* 模拟超支 */
      }
    });
    orchestrate(t, { kind: "instant", raf });
    expect(motionBudgetExceeded()).toBe(1);
  });
});

describe("N-06 FLIP", () => {
  it("零位移返回 0（无需动画）", () => {
    const el = fakeEl({ left: 10, top: 10 });
    const rec = flipMeasure(el);
    expect(flipPlaySafe(rec)).toBe(0);
  });

  it("reduce-motion 时不写 transform（直接落位）", () => {
    const el = fakeEl({ left: 10, top: 10 });
    const rec = flipMeasure(el);
    (rec.el as unknown as { getBoundingClientRect: () => { left: number; top: number } }).getBoundingClientRect = () => ({
      left: 50,
      top: 10,
    });
    const dist = flipPlaySafe(rec, { reduceMotion: true });
    expect(dist).toBe(40);
    expect((rec.el as unknown as { style: { transform?: string } }).style.transform).toBeFalsy();
  });
});

// flipPlay 非动画路径不触碰 rAF/DOM 动画，动画路径在本测试环境跳过
import { flipPlay } from "../orchestrate";
function flipPlaySafe(rec: ReturnType<typeof flipMeasure>, opts?: { reduceMotion?: boolean }): number {
  return flipPlay(rec, opts);
}

