/**
 * U3-v7 引擎二：copyqueue —— 复制队列 UI 装配引擎（AI-U3 · 批次七）。
 *
 * 判据唯一源（主册摘文）：
 * - F531「队列在进度中心可见（F369 任务面——每批独立进度/暂停/取消/优先
 *   级）；『先传这批』右键插队」——队列视图四信息装配（进度/速度/剩余
 *   时间/状态）+ 任务生命周期机检。
 * - F524「清空执行后通知条驻留 5 秒（已清空回收站 N 项——撤销）」——
 *   通知条装配层：文案/倒计时/延寿/撤销四态。
 * - F529「预检在进度对话框出现前完成」——预检失败闸：不过闸不出进度框，
 *   三选出路裁决后才放行。
 *
 * 调度与后悔窗核心算法复用 copyops（F531 scheduleQueue/jumpQueue、F524
 * undoBin*、F529 spaceCheck）——一处一事实，本引擎只做装配面，不重写判据。
 */

import {
  scheduleQueue, jumpQueue, undoBinOpen, undoBinExtend, undoBinTick, undoBinRestore,
  spaceCheck, shortfallMessage, shouldVerify, COPY_VERIFY_AUTO_ABOVE,
  type CopyTask, type CopyTaskState, type SpaceDecision, type VerifyRt,
} from "../copyops";

/* ------------------------------- F531 队列视图装配 ------------------------------- */

/** 调度核心再出口（copyops 同源判据经本引擎一口出——labapi 只认 engines 桶）。 */
export { scheduleQueue, jumpQueue } from "../copyops";

/** 队列作业（视图层比 CopyTask 多装配字段：源目标/字节/速率样本）。 */
export interface CopyJob extends CopyTask {
  srcLabel: string;
  dstLabel: string;
  bytesTotal: number;
  bytesDone: number;
  /** 速率样本（滑动窗口 ms→bytes）。 */
  samples: ReadonlyArray<{ at: number; bytes: number }>;
  /** 复制后校验（F530）挂载位：>1GB 自动开。 */
  verify: boolean;
}

export function newJob(id: string, volume: string, srcLabel: string, dstLabel: string, bytesTotal: number, priority = 5): CopyJob {
  return {
    id, volume, state: "queued", priority, srcLabel, dstLabel,
    bytesTotal, bytesDone: 0, samples: [],
    verify: bytesTotal >= COPY_VERIFY_AUTO_ABOVE,
  };
}

/** 队列视图四信息（判据：每批进度/速度/剩余时间/状态）。 */
export interface JobRow {
  id: string;
  label: string;
  progressPct: number;
  /** 字节/秒；样本 <2 个 → null（诚实「计算中」，不编数）。 */
  speedBps: number | null;
  /** 秒；速度未知 → null。 */
  etaSec: number | null;
  state: CopyTaskState;
}

export function jobRow(j: CopyJob): JobRow {
  // 速度：滑动窗口（首尾样本）——样本不足显性 null
  let speedBps: number | null = null;
  if (j.samples.length >= 2) {
    const first = j.samples[0]!;
    const last = j.samples[j.samples.length - 1]!;
    const dt = last.at - first.at;
    if (dt > 0) speedBps = (last.bytes - first.bytes) / (dt / 1000);
  }
  const remaining = Math.max(0, j.bytesTotal - j.bytesDone);
  const etaSec = speedBps !== null && speedBps > 0 ? remaining / speedBps : null;
  return {
    id: j.id,
    label: `${j.srcLabel} → ${j.dstLabel}`,
    progressPct: j.bytesTotal === 0 ? 0 : Math.min(100, (j.bytesDone / j.bytesTotal) * 100),
    speedBps,
    etaSec,
    state: j.state,
  };
}

/** 进度推进（样本进滑动窗口——窗口上限 8 个，防爆内存）。 */
export const SPEED_SAMPLE_WINDOW = 8;

export function jobProgress(j: CopyJob, now: number, bytesDone: number): CopyJob {
  const samples = [...j.samples, { at: now, bytes: bytesDone }].slice(-SPEED_SAMPLE_WINDOW);
  return { ...j, bytesDone, samples };
}

/** 任务生命周期：暂停/恢复/取消独立（判据：每批独立操作互不牵连）。 */
export function jobPause(j: CopyJob): CopyJob {
  return j.state === "running" ? { ...j, state: "paused" } : j;
}
export function jobResume(j: CopyJob): CopyJob {
  return j.state === "paused" ? { ...j, state: "queued" } : j;
}
export function jobCancel(j: CopyJob): CopyJob {
  return j.state === "done" ? j : { ...j, state: "canceled" };
}
export function jobDone(j: CopyJob): CopyJob {
  return { ...j, state: "done", bytesDone: j.bytesTotal };
}

/** 状态人话（十章文案一致：queued/running/paused/done/canceled 五态）。 */
export function jobStateText(s: CopyTaskState): string {
  const map: Record<CopyTaskState, string> = {
    queued: "排队中", running: "复制中", paused: "已暂停", done: "已完成", canceled: "已取消",
  };
  return map[s];
}

/** F530 校验决策装配（复用 copyops.shouldVerify——>1GB 自动开、可关闭）。 */
export function jobVerifyDecision(rt: VerifyRt, j: CopyJob, userForced?: boolean): boolean {
  return shouldVerify(rt, j.bytesTotal, userForced ?? (j.verify ? undefined : false));
}

/* ------------------------------- F524 后悔窗通知条装配 ------------------------------- */

export interface UndoBanner {
  visible: boolean;
  text: string;
  /** 剩余毫秒（倒计时显示——诚实不给假进度）。 */
  remainingMs: number;
  /** 延寿可用（上限内才可点）。 */
  canExtend: boolean;
}

/** 通知条装配（判据：文案「已清空回收站 N 项——撤销」+ 5s 驻留 + 延寿）。 */
export function undoBannerAssemble(items: string[], now: number): { rt: ReturnType<typeof undoBinOpen>; banner: UndoBanner } {
  const rt = undoBinOpen(items, now);
  return {
    rt,
    banner: {
      visible: true,
      text: `已清空回收站 ${items.length} 项——撤销`,
      remainingMs: rt.expiresAt - now,
      canExtend: rt.extends < 2,
    },
  };
}

/** 通知条 tick（判据：超时真释放——撤销条消失且不可再撤）。 */
export function undoBannerTick(
  rt: ReturnType<typeof undoBinOpen>, now: number,
): { rt: ReturnType<typeof undoBinOpen>; banner: UndoBanner } {
  const { expired } = undoBinTick(rt, now);
  return {
    rt,
    banner: {
      visible: !rt.released,
      text: expired ? "已释放（超过后悔窗——诚实不可恢复）" : "",
      remainingMs: Math.max(0, rt.expiresAt - now),
      canExtend: rt.extends < 2 && !rt.released,
    },
  };
}

/** 通知条延寿按钮（判据：长按再给 10 秒——装配层把结果写回条面）。 */
export function undoBannerExtend(rt: ReturnType<typeof undoBinOpen>): { rt: ReturnType<typeof undoBinOpen>; banner: UndoBanner } {
  const extended = undoBinExtend(rt);
  return {
    rt,
    banner: {
      visible: !rt.released,
      text: extended ? "已延寿 10 秒" : "",
      remainingMs: rt.expiresAt - Date.now(),
      canExtend: extended ? rt.extends < 2 : false,
    },
  };
}

/** 通知条撤销（判据：窗口内全量还原）。 */
export function undoBannerRestore(rt: ReturnType<typeof undoBinOpen>): { restored: string[] | null; banner: UndoBanner } {
  const restored = undoBinRestore(rt);
  return {
    restored,
    banner: { visible: false, text: restored ? `已还原 ${restored.length} 项` : "", remainingMs: 0, canExtend: false },
  };
}

/* ------------------------------- F529 预检前置闸 ------------------------------- */

export type GateVerdict =
  | { stage: "blocked"; message: string; decisions: ReadonlyArray<SpaceDecision> }
  | { stage: "passed"; message: string; decisions: ReadonlyArray<SpaceDecision> };

/**
 * 预检失败闸（判据：预检在进度对话框出现**前**完成；三选出路）。
 * 不带空间数据时（全部放行）直接过闸；带 shortfalls 时按裁决分流：
 * 「仍要复制」过闸（记录越权），「换目标/取消」不出进度框。
 */
export function preflightGate(
  input: Parameters<typeof spaceCheck>[0] | null,
  decision: SpaceDecision,
): GateVerdict {
  if (!input) return { stage: "passed", message: "", decisions: ["proceed", "change-target", "cancel"] };
  const sc = spaceCheck(input);
  if (sc.ok) return { stage: "passed", message: "", decisions: ["proceed", "change-target", "cancel"] };
  const msg = shortfallMessage(sc.shortfalls[0]!);
  if (decision === "proceed") return { stage: "passed", message: `${msg}（用户选择仍要复制）`, decisions: ["proceed", "change-target", "cancel"] };
  return { stage: "blocked", message: msg, decisions: ["proceed", "change-target", "cancel"] };
}

/* ------------------------------- 自检 ------------------------------- */

/** F531/F524/F529 装配面自检。 */
export function copyqueueSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];

  // 队列视图：进度/速度/剩余（样本不足显性 null——诚实）
  let j = newJob("1", "D:", "D:\\照片", "E:\\备份", 1000);
  checks.push({ name: "F531 四信息初值诚实", pass: jobRow(j).speedBps === null && jobRow(j).etaSec === null && jobRow(j).state === "queued" });
  j = jobProgress(j, 0, 100);
  j = jobProgress(j, 1000, 600);
  const row = jobRow(j);
  checks.push({ name: "F531 速度与剩余估算", pass: row.speedBps === 500 && Math.abs(row.etaSec! - 0.8) < 1e-9 && row.progressPct === 60 });
  checks.push({ name: "F531 样本窗口上限", pass: (() => {
    let jj = newJob("2", "D:", "a", "b", 100000);
    for (let t = 0; t < 20; t++) jj = jobProgress(jj, t * 100, t * 500);
    return jj.samples.length === SPEED_SAMPLE_WINDOW;
  })() });

  // 生命周期独立：暂停→恢复→取消；done 不可取消
  const a = jobDone(jobCancel(jobResume(jobPause(newJob("3", "C:", "s", "d", 10)))));
  checks.push({ name: "F531 生命周期链", pass: a.state === "done" && a.bytesDone === 10 });
  const doneJob = jobDone(newJob("4", "C:", "s", "d", 10));
  checks.push({ name: "F531 已完成不可取消", pass: jobCancel(doneJob).state === "done" });
  checks.push({ name: "F531 状态人话五态", pass: jobStateText("queued") === "排队中" && jobStateText("running") === "复制中" && jobStateText("paused") === "已暂停" && jobStateText("done") === "已完成" && jobStateText("canceled") === "已取消" });

  // F530 装配：>1GB 自动开、小文件默认关、用户强制优先
  const vrt: VerifyRt = { enabled: true, autoAboveBytes: COPY_VERIFY_AUTO_ABOVE };
  checks.push({ name: "F530 大文件自动校验", pass: jobVerifyDecision(vrt, newJob("5", "C:", "s", "d", 2 * 1024 ** 3)) === true });
  checks.push({ name: "F530 小文件默认不校验", pass: jobVerifyDecision(vrt, newJob("6", "C:", "s", "d", 1000)) === false });
  checks.push({ name: "F530 用户强制优先", pass: jobVerifyDecision(vrt, newJob("7", "C:", "s", "d", 1000), true) === true });

  // F524 通知条：文案/倒计时/延寿上限/撤销/超时消失
  const bn = undoBannerAssemble(["a", "b", "c"], 1000);
  checks.push({ name: "F524 通知条文案+5s", pass: bn.banner.text === "已清空回收站 3 项——撤销" && bn.banner.remainingMs === 5000 && bn.banner.canExtend });
  const tick1 = undoBannerTick(bn.rt, 3000);
  checks.push({ name: "F524 倒计时推进", pass: tick1.banner.visible && tick1.banner.remainingMs === 3000 });
  const ex = undoBannerExtend(tick1.rt);
  checks.push({ name: "F524 延寿回写条面", pass: ex.banner.text === "已延寿 10 秒" && ex.banner.canExtend });
  const restore = undoBannerRestore(ex.rt);
  checks.push({ name: "F524 撤销全量还原", pass: restore.restored?.length === 3 && !restore.banner.visible });
  const bn2 = undoBannerAssemble(["x"], 0);
  const tick2 = undoBannerTick(bn2.rt, 5001);
  checks.push({ name: "F524 超时条消失", pass: !tick2.banner.visible && undoBannerRestore(tick2.rt).restored === null });

  // F529 预检闸：不过闸不出进度框；三选出路
  const short: Parameters<typeof spaceCheck>[0] = { totalBytes: 0, perTargetFree: { "E:": 2 * 1024 ** 3 }, perTargetNeed: { "E:": 3 * 1024 ** 3 } };
  const blocked = preflightGate(short, "cancel");
  checks.push({ name: "F529 不足默认拦下", pass: blocked.stage === "blocked" && blocked.message.includes("放不下") });
  const proceed = preflightGate(short, "proceed");
  checks.push({ name: "F529 仍要复制越权放行", pass: proceed.stage === "passed" && proceed.message.includes("仍要复制") });
  const changeT = preflightGate(short, "change-target");
  checks.push({ name: "F529 换目标不出进度框", pass: changeT.stage === "blocked" });
  checks.push({ name: "F529 足量直过闸", pass: preflightGate(null, "proceed").stage === "passed" && preflightGate({ totalBytes: 0, perTargetFree: { "E:": 9 * 1024 ** 3 }, perTargetNeed: { "E:": 3 * 1024 ** 3 } }, "proceed").stage === "passed" });

  // 复用对账：调度核心与 copyops 同源（插队+同盘串行穿越装配层仍成立）
  const q = [newJob("a", "C:", "s1", "d1", 1, 5), newJob("b", "C:", "s2", "d2", 1, 1), newJob("c", "D:", "s3", "d3", 1, 9)];
  const jumped = jumpQueue(q, "c");
  const picks = scheduleQueue(jumped.map((t) => ({ ...t })), new Set(), 2);
  checks.push({ name: "F531 装配层复用调度判据", pass: picks[0]?.id === "c" && picks.length === 2 && picks.filter((p) => p.volume === "C:").length === 1 });

  return checks;
}
