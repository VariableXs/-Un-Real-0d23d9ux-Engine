import { describe, expect, it } from "vitest";
import { OUTPUT_SPEC, OVERHEAD_BUDGET_FPS, breathingFrameRect, elapsedMs, fileNameFor, overheadWithinBudget, pauseRecording, privacyIndicator, resumeRecording, startRecording, stopRecording } from "../f361-screenRecorder";

const FULL:Parameters<typeof startRecording>[2] = { x: 0, y: 0, w: 1920, h: 1080 };

describe("F361 屏幕录制", () => {
  it("三模式开录：全屏/窗口/区域；产物规格入册（webm/vp9/60fps）", () => {
    expect(OUTPUT_SPEC.container).toBe("webm");
    expect(OUTPUT_SPEC.fps).toBe(60);
    for (const mode of ["fullscreen", "window", "region"] as const) {
      const r = startRecording("s1", mode, FULL, { system: true, mic: false }, 1000);
      expect(r.session!.mode).toBe(mode);
    }
  });

  it("区域模式空矩形 / 双轨全关 → 拒录并给三要素错误（零静默）", () => {
    const empty = startRecording("s", "region", { x: 0, y: 0, w: 0, h: 0 }, { system: true, mic: false }, 0);
    expect(empty.session).toBeNull();
    expect(empty.error).toContain("框选");
    const silent = startRecording("s", "fullscreen", FULL, { system: false, mic: false }, 0);
    expect(silent.session).toBeNull();
    expect(silent.error).toContain("至少选择一路音频");
  });

  it("诚实计时：暂停不计时、恢复续走、停止得总时长", () => {
    let s = startRecording("s", "fullscreen", FULL, { system: true, mic: true }, 0).session!;
    expect(elapsedMs(s, 10_000)).toBe(10_000);
    s = pauseRecording(s, 10_000);
    expect(elapsedMs(s, 60_000)).toBe(10_000); // 暂停 50s 不计
    s = resumeRecording(s, 60_000);
    expect(elapsedMs(s, 62_000)).toBe(12_000);
    const done = stopRecording(s, 62_000);
    expect(done.durationMs).toBe(12_000);
  });

  it("暂停/恢复状态机：重复暂停/恢复幂等无害", () => {
    let s = startRecording("s", "region", FULL, { system: true, mic: false }, 0).session!;
    s = pauseRecording(s, 1000);
    const again = pauseRecording(s, 2000);
    expect(again.accumulatedMs).toBe(s.accumulatedMs);
    s = resumeRecording(s, 3000);
    expect(resumeRecording(s, 4000)).toEqual(s);
  });

  it("呼吸红框：区域外扩 2px 环带", () => {
    expect(breathingFrameRect({ x: 10, y: 10, w: 100, h: 50 })).toEqual({ x: 8, y: 8, w: 104, h: 54 });
  });

  it("隐私联动：录麦克风必亮指示，停止后双灯全灭（F322/F323 无豁免）", () => {
    const s = startRecording("s", "fullscreen", FULL, { system: true, mic: true }, 0).session!;
    expect(privacyIndicator(s)).toEqual({ micLight: true, systemLight: true });
    const stopped = { ...s, state: "stopped" as const };
    expect(privacyIndicator(stopped)).toEqual({ micLight: false, systemLight: false });
  });

  it("开销预算 <5fps（判据硬线）", () => {
    expect(OVERHEAD_BUDGET_FPS).toBe(5);
    expect(overheadWithinBudget(80, 76).ok).toBe(true);
    expect(overheadWithinBudget(80, 74).ok).toBe(false);
  });

  it("产物命名：「录屏 YYYY-MM-DD HH-mm」", () => {
    expect(fileNameFor(new Date(2026, 8, 25, 14, 30))).toBe("录屏 2026-09-25 14-30");
  });
});
