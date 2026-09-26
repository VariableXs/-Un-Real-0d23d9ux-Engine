import { describe, expect, it } from "vitest";
import { SEGMENT_MS, SEGMENT_THRESHOLD_MS, autoName, closingBar, estimateAccuracy, estimateSizeBytes, loadLedger, recoverInterrupted, recordSegmentWritten, saveLedger, segmentFileName, segmentPlan, type RecordingLedger } from "../f362-recordingOutput";
import { __clearMem, memStore } from "../internal/store";

describe("F362 录屏产物管理", () => {
  it("自动命名格式（判据）", () => {
    expect(autoName(new Date(2026, 8, 25, 14, 30))).toBe("录屏 2026-09-25 14-30");
  });

  it("分节边界：≤10 分钟单节；>10 分钟 5 分钟切齐 + 尾节（零空节）", () => {
    expect(segmentPlan(9 * 60 * 1000)).toEqual([9 * 60 * 1000]);
    expect(segmentPlan(SEGMENT_THRESHOLD_MS)).toEqual([SEGMENT_THRESHOLD_MS]);
    expect(segmentPlan(12 * 60 * 1000)).toEqual([SEGMENT_MS, SEGMENT_MS, 2 * 60 * 1000]);
    expect(segmentPlan(15 * 60 * 1000)).toEqual([SEGMENT_MS, SEGMENT_MS, SEGMENT_MS]);
    expect(segmentPlan(10 * 60 * 1000 + 1)).toEqual([SEGMENT_MS, SEGMENT_MS, 1]);
  });

  it("分节文件名：首节原样、后续带节号", () => {
    expect(segmentFileName("录屏 A", 0)).toBe("录屏 A.webm");
    expect(segmentFileName("录屏 A", 1)).toBe("录屏 A · 第2节.webm");
  });

  it("大小预估与误差 <10% 判据", () => {
    const est = estimateSizeBytes(60_000, 20_000_000, 2);
    const acc = estimateAccuracy(est, est);
    expect(acc.ok).toBe(true);
    expect(estimateAccuracy(est, Math.round(est * 1.15)).ok).toBe(false);
    expect(estimateAccuracy(0, 0).ok).toBe(true);
  });

  it("断电恢复：已落盘分节保留、进行中的节如实报丢弃", () => {
    let ledger: RecordingLedger = { sessionId: "s1", baseName: "录屏 A", segments: [], interrupted: false };
    ledger = recordSegmentWritten(ledger, 0, "a1.webm", 1000, 1);
    ledger = recordSegmentWritten(ledger, 1, "a2.webm", 2000, 2);
    ledger = { ...ledger, segments: [...ledger.segments, { index: 2, fileName: "a3.webm", bytes: 500, writtenAt: null }], interrupted: true };
    const rec = recoverInterrupted(ledger);
    expect(rec.recovered).toHaveLength(2);
    expect(rec.droppedInProgress).toBe(1);
    expect(rec.totalBytes).toBe(3000);
  });

  it("账本持久化 round-trip；损坏数据显式回退 null", () => {
    __clearMem();
    const s = memStore();
    const l: RecordingLedger = { sessionId: "s1", baseName: "x", segments: [], interrupted: false };
    expect(saveLedger(l, s)).toBe(true);
    expect(loadLedger(s)!.sessionId).toBe("s1");
    s.setItem("variable:h4:f362:ledger", "{broken");
    expect(loadLedger(s)).toBeNull();
  });

  it("收尾条：命名+位置+大小+分节数四件套", () => {
    const bar = closingBar(new Date(2026, 8, 25, 14, 30), 13 * 60 * 1000, "S:/Videos", 20_000_000, 1);
    expect(bar.name).toBe("录屏 2026-09-25 14-30");
    expect(bar.segments).toBe(3);
    expect(bar.estimateBytes).toBeGreaterThan(0);
    expect(bar.dir).toBe("S:/Videos");
  });
});
