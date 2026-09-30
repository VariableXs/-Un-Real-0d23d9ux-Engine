/**
 * AI-13 U-19 启动加速流水线 + M-52 冷启动 A/B 对照（前端时间戳侧）
 *
 * boot phase 协议字段纪律（ASCENT）：P0=阻断桌面 / P1=可延迟 / P2=托盘化；
 * 枚举只追加不重排。阶段时间戳经 perf_boot_stage 落后端（M-52 快照数据源）。
 */

export type BootPriority = 0 | 1 | 2;

export interface BootMark {
  name: string;
  priority: BootPriority;
  /** performance.now() 相对值 */
  atMs: number;
}

const t0 = performance.now();
const marks: BootMark[] = [];
let stageRecord: ((m: { name: string; priority: BootPriority }) => void) | null = null;

/** 接入后端落盘（ipc.perfBootStage）。 */
export function setBootStageRecorder(fn: (m: { name: string; priority: BootPriority }) => void): void {
  stageRecord = fn;
}

/** 标记一个阶段（P0/P1/P2 分级）。 */
export function markBootStage(name: string, priority: BootPriority): BootMark {
  const m: BootMark = { name, priority, atMs: Math.round(performance.now() - t0) };
  marks.push(m);
  stageRecord?.({ name, priority });
  return m;
}

export function bootMarks(): BootMark[] {
  return [...marks];
}

/** 阶段耗时（相邻标记差）。 */
export function bootDurations(): { name: string; ms: number; priority: BootPriority }[] {
  const out: { name: string; ms: number; priority: BootPriority }[] = [];
  for (let i = 0; i < marks.length; i += 1) {
    const cur = marks[i] as BootMark;
    const next = marks[i + 1];
    out.push({ name: cur.name, ms: next ? next.atMs - cur.atMs : 0, priority: cur.priority });
  }
  return out;
}

/** P1/P2 阶段是否在桌面 ready 之后才需要完成（剧场 ready 提前的判定依据）。 */
export function deferrableAfterReady(): BootMark[] {
  return marks.filter((m) => m.priority >= 1);
}

/** 快照 JSON（M-52：boot-<version>.json 形状）。 */
export function bootSnapshot(version: string): { version: string; takenAt: number; marks: BootMark[] } {
  return { version, takenAt: Date.now(), marks: bootMarks() };
}

/** 与上一版对照（M-52 判定核心，纯函数供单测）：同名阶段回归 >10% → 红。 */
export function bootDiff(baseline: BootMark[], current: BootMark[]): { name: string; baseMs: number; curMs: number; regressed: boolean }[] {
  const bDur = durationsOf(baseline);
  const cDur = durationsOf(current);
  const out: { name: string; baseMs: number; curMs: number; regressed: boolean }[] = [];
  for (const [name, ms] of bDur) {
    const curMs = cDur.get(name) ?? 0;
    const regressed = ms > 0 && curMs > ms * 1.1;
    out.push({ name, baseMs: Math.round(ms), curMs: Math.round(curMs), regressed });
  }
  return out;
}

function durationsOf(marks: BootMark[]): Map<string, number> {
  const out = new Map<string, number>();
  for (let i = 0; i < marks.length; i += 1) {
    const cur = marks[i]!;
    const next = marks[i + 1];
    if (next) out.set(cur.name, next.atMs - cur.atMs);
  }
  return out;
}

/** 测试隔离。 */
export function resetBootTimeline(): void {
  marks.length = 0;
  stageRecord = null;
}
