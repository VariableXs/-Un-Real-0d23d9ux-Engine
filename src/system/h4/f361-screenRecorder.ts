/**
 * F361 屏幕录制（H 域 · AI-H4）：
 * 截图工具（F098）的录制分支：全屏/窗口/区域三选录制（区域可 F360 标尺辅助），
 * 录制中屏幕边缘呼吸红框 + 工具浮条（暂停/停止/时长计时），产物 .webm（硬编码走 GPU）；
 * 系统音/麦克风两轨可选（F322/F323 隐私指示联动——录麦克风必亮指示）。
 * 判据（主册 F361）：三模式录制用例；开销实测（录制时前台帧率降幅 <5fps）；
 * 双轨音频；红框与浮条；产物规格（帧率/码率入册）。
 * 依赖锚点：F098 截图 / F322-F323 隐私指示 / F360 标尺。
 */

export type RecordMode = "fullscreen" | "window" | "region";

export interface RecordRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** 产物规格入册（判据「帧率/码率入册」——一处定义，处处引用）。 */
export const OUTPUT_SPEC = {
  container: "webm",
  codec: "vp9",
  fps: 60,
  /** 视频码率（bps）——4K 区域录制上限口径。 */
  videoBitrate: 20_000_000,
  /** 音频码率（bps/轨）。 */
  audioBitratePerTrack: 128_000,
} as const;

/** 开销预算：录制引入的前台帧率降幅 <5fps（判据硬线）。 */
export const OVERHEAD_BUDGET_FPS = 5;

export interface AudioTracks {
  system: boolean;
  mic: boolean;
}

export interface RecordSession {
  id: string;
  mode: RecordMode;
  /** 录制区域（fullscreen=整屏；window=窗口矩形；region=用户框选）。 */
  rect: RecordRect;
  tracks: AudioTracks;
  state: "recording" | "paused" | "stopped";
  startedAt: number;
  /** 累计已录时长（ms）——暂停不计时（时长计时的诚实口径）。 */
  accumulatedMs: number;
  /** 最近一次恢复时刻（null=当前未在录）。 */
  resumedAt: number | null;
}

/** 开录：区域模式必须给正面积矩形（零容错校验）。 */
export function startRecording(id: string, mode: RecordMode, rect: RecordRect, tracks: AudioTracks, now: number): { session: RecordSession | null; error: string | null } {
  if (mode !== "fullscreen" && (rect.w <= 0 || rect.h <= 0)) {
    return { session: null, error: mode === "region" ? "录制区域为空——请先框选区域" : "窗口矩形非法" };
  }
  if (!tracks.system && !tracks.mic) {
    return { session: null, error: "至少选择一路音频（系统音或麦克风）；纯视频请走截图工具" };
  }
  return {
    session: { id, mode, rect, tracks, state: "recording", startedAt: now, accumulatedMs: 0, resumedAt: now },
    error: null,
  };
}

/** 暂停：把当前段计入累计时长（诚实计时——暂停期间不虚增）。 */
export function pauseRecording(s: RecordSession, now: number): RecordSession {
  if (s.state !== "recording" || s.resumedAt === null) return s;
  return { ...s, state: "paused", accumulatedMs: s.accumulatedMs + (now - s.resumedAt), resumedAt: null };
}

/** 恢复。 */
export function resumeRecording(s: RecordSession, now: number): RecordSession {
  if (s.state !== "paused") return s;
  return { ...s, state: "recording", resumedAt: now };
}

/** 停止：出时长（ms）与产物文件名建议。 */
export function stopRecording(s: RecordSession, now: number): { durationMs: number; fileName: string } {
  const total = s.accumulatedMs + (s.resumedAt !== null ? now - s.resumedAt : 0);
  return { durationMs: total, fileName: fileNameFor(new Date(s.startedAt)) };
}

/** 录制中当前时长（浮条计时显示用）。 */
export function elapsedMs(s: RecordSession, now: number): number {
  return s.accumulatedMs + (s.resumedAt !== null ? now - s.resumedAt : 0);
}

/** 呼吸红框：2px 外扩的边缘环带（合成矩形 = 区域 + 红框环带）。 */
export function breathingFrameRect(rect: RecordRect): RecordRect {
  return { x: rect.x - 2, y: rect.y - 2, w: rect.w + 4, h: rect.h + 4 };
}

/** 麦克风隐私联动（F322/F323）：录麦克风 = 指示必亮，无豁免。 */
export function privacyIndicator(s: RecordSession): { micLight: boolean; systemLight: boolean } {
  return { micLight: s.tracks.mic && s.state !== "stopped", systemLight: s.tracks.system && s.state !== "stopped" };
}

/** 开销审计：帧率降幅是否在预算内（判据 <5fps）。 */
export function overheadWithinBudget(fpsWithout: number, fpsWith: number): { drop: number; ok: boolean } {
  const drop = fpsWithout - fpsWith;
  return { drop, ok: drop < OVERHEAD_BUDGET_FPS };
}

/** 产物命名（F362 同源）：「录屏 YYYY-MM-DD HH-mm」。 */
export function fileNameFor(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `录屏 ${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}-${p(d.getMinutes())}`;
}
