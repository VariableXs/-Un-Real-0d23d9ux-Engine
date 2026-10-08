import { describe, expect, it } from "vitest";
import {
  INITIAL_CEREMONY,
  SKIP_THRESHOLD,
  ceremonyReducer,
  pacingTimings,
  type CeremonyState,
} from "./ceremony";

/**
 * U-01/U-06 仪式状态机四场景：快机 / 慢机 / 跳过 / 回放。
 * 全部断言阶段迁移正确且进度绝不回退。
 */

function run(events: Parameters<typeof ceremonyReducer>[1][], from: CeremonyState = INITIAL_CEREMONY): CeremonyState {
  return events.reduce((s, ev) => ceremonyReducer(s, ev), from);
}

describe("U-01 ceremony reducer", () => {
  it("快机：入场期 ready 早到 → ENTER_DONE 直达 readyHold，不空等 streaming", () => {
    const s = run([
      { type: "PROGRESS", progress: 0.4 },
      { type: "PROGRESS", progress: 1 },
      { type: "READY" }, // 入场未结束就 ready
    ]);
    expect(s.phase).toBe("entering"); // 仍处入场，但已记账 ready
    expect(s.ready).toBe(true);
    expect(s.progress).toBe(1);

    const s2 = ceremonyReducer(s, { type: "ENTER_DONE" });
    expect(s2.phase).toBe("readyHold"); // 直达，不经过 streaming
    expect(s2.progress).toBe(1);

    const s3 = ceremonyReducer(s2, { type: "EXIT_DONE" });
    expect(s3.phase).toBe("done");
  });

  it("慢机：ENTER_DONE → streaming 拉伸 → READY → readyHold → EXIT_DONE → done", () => {
    const s1 = ceremonyReducer(INITIAL_CEREMONY, { type: "ENTER_DONE" });
    expect(s1.phase).toBe("streaming");
    expect(s1.ready).toBe(false);

    const s2 = run(
      [
        { type: "PROGRESS", progress: 0.12 },
        { type: "PROGRESS", progress: 0.34 },
        { type: "PROGRESS", progress: 0.71 },
      ],
      s1,
    );
    expect(s2.phase).toBe("streaming"); // 未 ready 前一直在 streaming
    expect(s2.progress).toBe(0.71);

    const s3 = ceremonyReducer(s2, { type: "READY" });
    expect(s3.phase).toBe("readyHold");
    expect(s3.ready).toBe(true);

    const s4 = ceremonyReducer(s3, { type: "EXIT_DONE" });
    expect(s4.phase).toBe("done");
    expect(s4.progress).toBe(0.71); // 进度保持
  });

  it("跳过：<30% 拒绝；≥30% 接受并进入 exiting（skipped 记账）", () => {
    // <30% 拒绝：状态完全不变
    const low = run([{ type: "PROGRESS", progress: 0.29 }]);
    const denied = ceremonyReducer(low, { type: "SKIP" });
    expect(denied).toBe(low); // 同一引用：纯拒绝
    expect(denied.phase).toBe("entering");
    expect(denied.skipped).toBe(false);

    // 恰好在阈值（≥30%）接受
    const mid = run([{ type: "PROGRESS", progress: SKIP_THRESHOLD }]);
    const skipped = ceremonyReducer(mid, { type: "SKIP" });
    expect(skipped.phase).toBe("exiting");
    expect(skipped.skipped).toBe(true);

    // exiting 后再 SKIP / EXIT_DONE 行为
    const again = ceremonyReducer(skipped, { type: "SKIP" });
    expect(again).toBe(skipped); // 已在 exiting，重复跳过无效果
    const done = ceremonyReducer(skipped, { type: "EXIT_DONE" });
    expect(done.phase).toBe("done");
    expect(done.skipped).toBe(true);
  });

  it("回放：旧 seq 的低进度事件绝不回退，重复 READY/ENTER_DONE 幂等", () => {
    const s1 = run([
      { type: "ENTER_DONE" },
      { type: "PROGRESS", progress: 0.8 },
      { type: "READY" },
    ]);
    expect(s1.phase).toBe("readyHold");
    expect(s1.progress).toBe(0.8);

    // 回放旧事件：进度更低 → 不回退
    const s2 = ceremonyReducer(s1, { type: "PROGRESS", progress: 0.31 });
    expect(s2.progress).toBe(0.8);
    expect(s2).toBe(s1); // 数值未变时返回原状态

    // 回放乱序超高值也不会超过 1
    const s3 = ceremonyReducer(s1, { type: "PROGRESS", progress: 1.7 });
    expect(s3.progress).toBe(1);

    // 重复 READY / 迟到的 ENTER_DONE：不改变 readyHold、不倒退阶段
    const s4 = run([{ type: "READY" }, { type: "ENTER_DONE" }], s1);
    expect(s4.phase).toBe("readyHold");
    expect(s4.ready).toBe(true);

    // 跳过后迟到的 READY：只记账，不再改阶段（后台加载继续）
    const s5 = ceremonyReducer(run([{ type: "PROGRESS", progress: 0.5 }, { type: "SKIP" }]), { type: "READY" });
    expect(s5.phase).toBe("exiting");
    expect(s5.ready).toBe(true);
    expect(s5.progress).toBe(0.5);
  });
});

describe("U-06 pacingTimings", () => {
  it("cinematic / brisk / instant 三档时长", () => {
    expect(pacingTimings("cinematic")).toEqual({ enter: 900, readyHold: 1200 });
    expect(pacingTimings("brisk")).toEqual({ enter: 500, readyHold: 400 });
    expect(pacingTimings("instant")).toEqual({ enter: 0, readyHold: 0 });
  });
});
