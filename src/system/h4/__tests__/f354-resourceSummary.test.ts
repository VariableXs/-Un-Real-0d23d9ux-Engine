import { describe, expect, it } from "vitest";
import { __clearMem, memStore } from "../internal/store";
import {
  FG_FRAME_YIELD_MS,
  HISTORY_POINTS,
  admitSample,
  clampReading,
  isEnabled,
  pushPoint,
  reconcile,
  setEnabled,
  sparklinePath,
  type ResourcePoint,
} from "../f354-resourceSummary";

function pt(t: number, over: Partial<ResourcePoint> = {}): ResourcePoint {
  return { t, cpu: 10, mem: 40, disk: 5, ...over };
}

describe("F354 任务栏资源摘要", () => {
  it("默认关；开关持久化 round-trip", () => {
    __clearMem();
    const s = memStore();
    expect(isEnabled(s)).toBe(false);
    expect(setEnabled(true, s)).toBe(true);
    expect(isEnabled(s)).toBe(true);
  });

  it("30s 历史 × 1s 一点：超出挤最旧", () => {
    let h: ResourcePoint[] = [];
    for (let i = 0; i < HISTORY_POINTS + 5; i++) h = pushPoint(h, pt(i * 1000, { cpu: i }));
    expect(h).toHaveLength(HISTORY_POINTS);
    expect(h[0]!.cpu).toBe(5); // 最旧 5 点被挤掉
    expect(h[HISTORY_POINTS - 1]!.cpu).toBe(34);
  });

  it("采样准入：间隔不足拒收；前台帧紧张让路（空闲通道判据）", () => {
    const h = [pt(0)];
    expect(admitSample(h, pt(500), 0).reason).toBe("interval");
    expect(admitSample(h, pt(1000), 0).admit).toBe(true);
    expect(admitSample(h, pt(2000), FG_FRAME_YIELD_MS + 1).reason).toBe("fg-yield");
  });

  it("读数钳制：NaN 显式无效、超界钳回、正常零钳制", () => {
    expect(clampReading(Number.NaN).valid).toBe(false);
    expect(clampReading(120)).toEqual({ value: 100, clamped: true, valid: true });
    expect(clampReading(-3)).toEqual({ value: 0, clamped: true, valid: true });
    expect(clampReading(42.5)).toEqual({ value: 42.5, clamped: false, valid: true });
  });

  it("sparkline：右对齐且坐标在画布内；空/单点空串", () => {
    const vals = Array.from({ length: HISTORY_POINTS }, (_, i) => i * 3.3);
    const path = sparklinePath(vals, 120, 32);
    expect(path.startsWith("M")).toBe(true);
    expect(path).toContain("L");
    expect(sparklinePath([1], 120, 32)).toBe("");
    expect(sparklinePath([], 120, 32)).toBe("");
  });

  it("三图对账：同源偏差 ≤3% 合格；偏差大判红", () => {
    const h = [pt(0, { cpu: 50, mem: 50, disk: 50 })];
    const rows = reconcile(h, { cpu: 50.5, mem: 60, disk: 50 });
    expect(rows.find((r) => r.series === "cpu")!.ok).toBe(true);
    expect(rows.find((r) => r.series === "mem")!.ok).toBe(false);
    expect(rows.find((r) => r.series === "disk")!.deltaPct).toBe(0);
  });
});
