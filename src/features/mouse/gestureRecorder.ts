/**
 * J 鼠标域 · F617 手势录制器与自定义轨迹库（创作面）。
 *
 * 「可自定义轨迹库」判据的创作链路：按住试笔 → 采集原始点列 → 重采样
 * （等距 32 点，抗抖动）→ 八向编码 → 同向合并 → 与既有手势查重（重复拒绝、
 * 显性报错）→ 入库 custom 库（j1store gestures.custom）。
 *
 * 识别与录制共用同一编码管线（录制出来的就是识别用的——一处一事实）；
 * 导出/导入走 round-trip 校验（F623 打包对接同构）。
 */

import { encodeDir8, GESTURE_STEP_PX, collapseRuns, type Dir8 } from "./gestures";

/** 录制重采样点数（归一化基准——与绘制快慢无关）。 */
export const RECORDER_RESAMPLE = 32;
/** 自定义手势库上限（超限拒绝并显性提示——上限提醒不静默）。 */
export const CUSTOM_GESTURE_CAP = 24;

export interface RecordedGesture {
  id: string;
  name: string;
  dirs: Dir8[];
  action: string;
  /** 录制时间与原始点数（调试面同源）。 */
  recordedAt: number;
  rawPoints: number;
}

/**
 * 轨迹重采样：把任意快慢的原始笔画归一为等距点列（录制与试笔共用）。
 * 输入点数 < 2 或总长过短返回空（无效笔画——显性无效而非猜测）。
 */
export function resampleTrail(points: { x: number; y: number }[], n = RECORDER_RESAMPLE): { x: number; y: number }[] {
  if (points.length < 2) return [];
  let total = 0;
  for (let i = 1; i < points.length; i++) {
    total += Math.hypot(points[i]!.x - points[i - 1]!.x, points[i]!.y - points[i - 1]!.y);
  }
  if (total < GESTURE_STEP_PX * 2) return []; // 总长不足两步：视作误触
  const step = total / (n - 1);
  const out: { x: number; y: number }[] = [{ ...points[0]! }];
  let acc = 0;
  let idx = 1;
  let prev = points[0]!;
  while (out.length < n && idx < points.length) {
    const cur = points[idx]!;
    const seg = Math.hypot(cur.x - prev.x, cur.y - prev.y);
    if (acc + seg >= step) {
      const t = (step - acc) / seg;
      const p = { x: prev.x + (cur.x - prev.x) * t, y: prev.y + (cur.y - prev.y) * t };
      out.push(p);
      prev = p;
      acc = 0;
    } else {
      acc += seg;
      prev = cur;
      idx += 1;
    }
  }
  while (out.length < n) out.push({ ...points[points.length - 1]! });
  return out;
}

/** 重采样点列 → 方向序列（录制与识别共用编码器）。 */
export function encodeTrail(points: { x: number; y: number }[]): Dir8[] {
  const rs = resampleTrail(points);
  const dirs: Dir8[] = [];
  for (let i = 1; i < rs.length; i++) {
    dirs.push(encodeDir8(rs[i]!.x - rs[i - 1]!.x, rs[i]!.y - rs[i - 1]!.y));
  }
  return collapseRuns(dirs);
}

export type RecorderState = "idle" | "recording" | "review";

/**
 * 手势录制器状态机：idle →（begin）recording →（finish）review。
 * review 态产出编码结果与查重结论；commit 入库 / discard 丢弃。
 * 中途 Esc/越界 = abort 回 idle（交互状态机完整出路——体验章三）。
 */
export class GestureRecorder {
  state: RecorderState = "idle";
  private trail: { x: number; y: number }[] = [];
  /** 录制过程中的实时方向预览（墨迹与试笔面板同源）。 */
  liveDirs: Dir8[] = [];
  result: { dirs: Dir8[]; rawPoints: number } | null = null;
  /** 最近一次拒绝原因（显性呈现，不静默吞）。 */
  lastError: string | null = null;

  begin(): void {
    this.state = "recording";
    this.trail = [];
    this.liveDirs = [];
    this.result = null;
    this.lastError = null;
  }

  feed(x: number, y: number): void {
    if (this.state !== "recording") return;
    const last = this.trail[this.trail.length - 1];
    this.trail.push({ x, y });
    if (last && Math.hypot(x - last.x, y - last.y) >= GESTURE_STEP_PX) {
      this.liveDirs.push(encodeDir8(x - last.x, y - last.y));
    }
  }

  /** 结束录制 → review 态；无效笔画返回 false 并写 lastError。 */
  finish(): boolean {
    if (this.state !== "recording") return false;
    const dirs = encodeTrail(this.trail);
    if (dirs.length === 0) {
      this.lastError = "笔画无效：总长不足或点数过少——画长一点再松手";
      this.state = "idle";
      return false;
    }
    this.result = { dirs, rawPoints: this.trail.length };
    this.state = "review";
    return true;
  }

  /**
   * 查重：与内置库 + 已有自定义库比较（同向合并后序列相等即视为重复，
   * 容差走 matchDirs 的 ±1 档——录制噪声不应产生"近重复"手势）。
   */
  duplicateOf(existing: { id: string; name: string; dirs: Dir8[] }[]): { id: string; name: string } | null {
    if (!this.result) return null;
    for (const g of existing) {
      if (matchSequence(this.result.dirs, g.dirs)) return { id: g.id, name: g.name };
    }
    return null;
  }

  /** 入库（查重 + 上限校验由调用方或本方法完成——失败写 lastError）。 */
  commit(
    name: string,
    action: string,
    custom: Record<string, { name: string; dirs: Dir8[]; action: string }>,
  ): { ok: boolean; error?: string } {
    if (this.state !== "review" || !this.result) {
      this.lastError = "没有可入库的录制结果";
      return { ok: false, error: this.lastError! };
    }
    if (!name.trim()) {
      this.lastError = "手势名称不能为空";
      return { ok: false, error: this.lastError! };
    }
    if (Object.keys(custom).length >= CUSTOM_GESTURE_CAP) {
      this.lastError = `自定义手势已达上限 ${CUSTOM_GESTURE_CAP} 条——先删除再创建（上限提醒不静默）`;
      return { ok: false, error: this.lastError! };
    }
    const id = `custom-${Date.now().toString(36)}`;
    custom[id] = { name: name.trim(), dirs: this.result.dirs, action: action.trim() || `custom.${id}` };
    this.state = "idle";
    this.trail = [];
    this.result = null;
    return { ok: true };
  }

  discard(): void {
    this.state = "idle";
    this.trail = [];
    this.liveDirs = [];
    this.result = null;
    this.lastError = null;
  }

  /** 试笔轨迹副本（面板画布渲染用）。 */
  get trailCopy(): { x: number; y: number }[] {
    return this.trail.map((p) => ({ ...p }));
  }
}

/** 序列容差匹配（与 matchDirs 同语义：等长 + 逐步 ±1 档）。 */
export function matchSequence(a: Dir8[], b: Dir8[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((x, i) => {
    const diff = Math.abs(x - b[i]!);
    return Math.min(diff, 8 - diff) <= 1;
  });
}

/** 自定义手势导出（round-trip 校验在 import 侧——F623 同构）。 */
export function exportCustomGestures(custom: Record<string, { name: string; dirs: Dir8[]; action: string }>): string {
  return JSON.stringify({ format: "vx-gestures", version: 1, data: custom }, null, 2);
}

/** 自定义手势导入：结构校验逐条过，非法条目显性列出（半套不收）。 */
export function importCustomGestures(
  json: string,
): { ok: true; data: Record<string, { name: string; dirs: Dir8[]; action: string }> } | { ok: false; errors: string[] } {
  const errors: string[] = [];
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch (e) {
    return { ok: false, errors: [`JSON 解析失败: ${String(e)}`] };
  }
  const obj = parsed as { format?: string; version?: number; data?: Record<string, unknown> };
  if (obj?.format !== "vx-gestures" || obj?.version !== 1) {
    return { ok: false, errors: ["格式头不匹配（期望 vx-gestures v1）"] };
  }
  const out: Record<string, { name: string; dirs: Dir8[]; action: string }> = {};
  for (const [id, g] of Object.entries(obj.data ?? {})) {
    const rec = g as { name?: unknown; dirs?: unknown; action?: unknown };
    if (typeof rec?.name !== "string" || !rec.name) errors.push(`${id}: name 缺失`);
    else if (!Array.isArray(rec?.dirs) || rec.dirs.length < 1 || !rec.dirs.every((d) => typeof d === "number" && d >= 0 && d <= 7)) {
      errors.push(`${id}: dirs 非法（需 0..7 的方向数组）`);
    } else if (typeof rec?.action !== "string" || !rec.action) errors.push(`${id}: action 缺失`);
    else out[id] = { name: rec.name, dirs: rec.dirs as Dir8[], action: rec.action };
  }
  if (errors.length > 0) return { ok: false, errors };
  return { ok: true, data: out };
}
