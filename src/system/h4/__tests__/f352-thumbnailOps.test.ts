import { describe, expect, it } from "vitest";
import {
  CLOSE_BUTTON_SIZE,
  HIT_TOLERANCE_PX,
  MINI_KEY_RESPONSE_BUDGET_MS,
  auditHitPrecision,
  buildThumbModel,
  hitTest,
  hoverTransition,
  transferProgress,
  withinResponseBudget,
} from "../f352-thumbnailOps";

describe("F352 缩略图窗上直接操作", () => {
  it("三型操作集分型：普通=关闭；媒体=关闭+播放暂停；传输=进度只读无关闭", () => {
    const normal = buildThumbModel("w1", "normal", 200);
    expect(normal.actions.map((a) => a.id)).toEqual(["close"]);
    const media = buildThumbModel("w2", "media", 200, true);
    expect(media.actions.map((a) => a.id)).toEqual(["close", "playpause", "mediaState"]);
    const transfer = buildThumbModel("w3", "transfer", 200);
    expect(transfer.actions.map((a) => a.id)).toEqual(["progress"]);
    expect(transfer.actions.every((a) => !a.enabled)).toBe(true); // 只读不可点
  });

  it("关闭命中精度：中心命中、外扩 2px 命中、外扩 3px 不命中", () => {
    const m = buildThumbModel("w", "normal", 200);
    const close = m.actions[0]!;
    const cx = close.rect.x + CLOSE_BUTTON_SIZE / 2;
    const cy = close.rect.y + CLOSE_BUTTON_SIZE / 2;
    expect(hitTest(m, cx, cy)).toBe("close");
    expect(hitTest(m, close.rect.x - HIT_TOLERANCE_PX, cy)).toBe("close");
    expect(hitTest(m, close.rect.x - HIT_TOLERANCE_PX - 1, cy)).toBeNull();
  });

  it("命中精度审计：重叠命中区被判为缺陷", () => {
    const m = buildThumbModel("w", "media", 64); // 极窄缩略图 → 两键外扩必然相撞
    expect(auditHitPrecision(m).length).toBeGreaterThan(0);
    const wide = buildThumbModel("w", "media", 200);
    expect(auditHitPrecision(wide)).toEqual([]);
  });

  it("迷你键响应预算 ≤100ms（预算即上限，生效判定严格小于）", () => {
    expect(MINI_KEY_RESPONSE_BUDGET_MS).toBeLessThanOrEqual(100);
    expect(withinResponseBudget(1000, 1099)).toBe(true);
    expect(withinResponseBudget(1000, 1100)).toBe(false);
  });

  it("悬停稳定性：区内移动抑制隐藏；离开起计时；回来取消", () => {
    const enter = hoverTransition({ visible: false, hideQueued: false }, "thumb");
    expect(enter.visible).toBe(true);
    const move = hoverTransition({ visible: true, hideQueued: false }, "action");
    expect(move.scheduleHide).toBe(false);
    const leave = hoverTransition({ visible: true, hideQueued: false }, "outside");
    expect(leave.scheduleHide).toBe(true);
    const back = hoverTransition({ visible: true, hideQueued: true }, "thumb");
    expect(back.cancelHide).toBe(true);
    expect(back.visible).toBe(true);
  });

  it("传输进度：归一化、超界钳制、非法输入显式 invalid（零 NaN）", () => {
    expect(transferProgress(50, 200)).toEqual({ ratio: 0.25, clamped: false, valid: true });
    expect(transferProgress(300, 200).clamped).toBe(true);
    expect(transferProgress(300, 200).ratio).toBe(1);
    expect(transferProgress(-1, 100).valid).toBe(false);
    expect(transferProgress(1, 0).valid).toBe(false);
    expect(transferProgress(Number.NaN, 100).valid).toBe(false);
  });
});
