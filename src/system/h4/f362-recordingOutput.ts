/**
 * F362 录屏产物管理（H 域 · AI-H4）：
 * 录制完成弹收尾条：预览缩略图 + 自动命名（「录屏 2026-09-25 14-30」）+ 保存位置
 * （默认视频目录，可改）+ 大小预估；长录制（>10 分钟）每 5 分钟分节
 * （防单文件过大损坏全丢）；录制中断电恢复（F311 同源：已录分节保留）。
 * 判据（主册 F362）：自动命名格式；分节 5 分钟边界；中断恢复用例（保留已完成分节）；
 * 大小预估误差 <10%。
 * 依赖锚点：F311 断电恢复 / F361 屏幕录制。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 分节时长（判据：5 分钟）。 */
export const SEGMENT_MS = 5 * 60 * 1000;
/** 长录制分节门槛（判据：>10 分钟才分节）。 */
export const SEGMENT_THRESHOLD_MS = 10 * 60 * 1000;
/** 大小预估允许误差（判据 <10%）。 */
export const ESTIMATE_TOLERANCE = 0.1;

/** 自动命名：「录屏 YYYY-MM-DD HH-mm」（F361 同源）。 */
export function autoName(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `录屏 ${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}-${p(d.getMinutes())}`;
}

/**
 * 分节计划：总时长 → 每节时长数组。
 * - ≤10 分钟：单节（不分节）；
 * - >10 分钟：整节 5 分钟切齐，尾节为余量（可为任意 <5min 长度，含 0 → 不产生空节）。
 */
export function segmentPlan(totalMs: number): number[] {
  if (totalMs <= SEGMENT_THRESHOLD_MS) return [totalMs];
  const full = Math.floor(totalMs / SEGMENT_MS);
  const tail = totalMs - full * SEGMENT_MS;
  const segments = Array.from({ length: full }, () => SEGMENT_MS);
  if (tail > 0) segments.push(tail);
  return segments;
}

/** 分节文件名：主体 + 序号（自第二节起）。 */
export function segmentFileName(base: string, index: number): string {
  return index === 0 ? `${base}.webm` : `${base} · 第${index + 1}节.webm`;
}

/** 大小预估：码率 × 时长（双轨计音频）。 */
export function estimateSizeBytes(durationMs: number, videoBps: number, audioTracks: number): number {
  if (durationMs <= 0) return 0;
  return Math.round(((videoBps + audioTracks * 128_000) * durationMs) / 8000);
}

/** 预估误差审计：|实-估|/实 ≤10% 判合格（判据「误差 <10%」）。 */
export function estimateAccuracy(estimateBytes: number, actualBytes: number): { errorPct: number; ok: boolean } {
  if (actualBytes <= 0) return { errorPct: actualBytes === estimateBytes ? 0 : Number.POSITIVE_INFINITY, ok: actualBytes === estimateBytes };
  const errorPct = Math.abs(actualBytes - estimateBytes) / actualBytes;
  return { errorPct, ok: errorPct < ESTIMATE_TOLERANCE };
}

/* ---------- 断电恢复账本（F311 同源：分节落盘即记账，恢复按账重建） ---------- */

export interface SegmentRecord {
  /** 节序号。 */
  index: number;
  fileName: string;
  bytes: number;
  /** 落盘完成时刻（null=进行中——进行中的节不算「已完成分节」）。 */
  writtenAt: number | null;
}

export interface RecordingLedger {
  sessionId: string;
  baseName: string;
  segments: SegmentRecord[];
  interrupted: boolean;
}

const KEY = h4Key("f362", "ledger");

function isLedger(v: unknown): v is RecordingLedger {
  return !!v && typeof (v as RecordingLedger).sessionId === "string" && Array.isArray((v as RecordingLedger).segments);
}

export function saveLedger(l: RecordingLedger, store: KvStore = defaultStore()): boolean {
  return writeJson(store, KEY, l);
}

export function loadLedger(store: KvStore = defaultStore()): RecordingLedger | null {
  return readJson<RecordingLedger | null>(store, KEY, null, isLedger);
}

/** 分节落盘记账：一节写完立即记账（断电后该节可找回）。 */
export function recordSegmentWritten(l: RecordingLedger, index: number, fileName: string, bytes: number, at: number): RecordingLedger {
  return { ...l, segments: [...l.segments, { index, fileName, bytes, writtenAt: at }] };
}

/**
 * 中断恢复：取账上全部「已落盘」分节——已完成分节保留（判据）；
 * 进行中的节如实标注丢弃（半文件不可信，诚实边界）。
 */
export function recoverInterrupted(l: RecordingLedger): { recovered: SegmentRecord[]; droppedInProgress: number; totalBytes: number } {
  const recovered = l.segments.filter((s) => s.writtenAt !== null);
  return {
    recovered,
    droppedInProgress: l.segments.length - recovered.length,
    totalBytes: recovered.reduce((sum, s) => sum + s.bytes, 0),
  };
}

/** 收尾条模型：预览缩略图 + 命名 + 位置 + 大小预估（判据四件套）。 */
export function closingBar(base: Date, totalMs: number, saveDir: string, videoBps: number, audioTracks: number): { name: string; dir: string; estimateBytes: number; segments: number; previewNote: string } {
  const plan = segmentPlan(totalMs);
  return {
    name: autoName(base),
    dir: saveDir,
    estimateBytes: estimateSizeBytes(totalMs, videoBps, audioTracks),
    segments: plan.length,
    previewNote: "预览缩略图取末节首帧",
  };
}
