/**
 * F509/F511/F512 深化引擎 · 粉碎计划器、剪贴板清空流、截图历史淘汰（AI-U3 · shredplan）。
 *
 * 判据唯一源（主册摘文）：
 * - F509「折叠入口；三重警示与焦点；覆写执行或诚实标注两分支；普通删除不受
 *   影响；耗时提示（大文件覆写慢）」。
 * - F511「双入口；当前+历史全清；确认框条数；密码后提示触发与 5s 时序；清空
 *   后粘贴行为（空——应用得到诚实失败）」。
 * - F512「20 条上限与淘汰；临时/已保存两态；关机清理与通知；重编辑链路；另存
 *   路径」。
 *
 * 深化点：
 * 1. 粉碎计划器：按文件大小与道数产出**执行计划**（每道偏移/块大小/预计耗时/
 *    是否含验证道），SSD 上「覆写执行或诚实标注」分支由介质探测结果决定——
 *    分支选择是数据驱动的，不是开关拍的。
 * 2. 剪贴板清空时序机：确认框条数 → 5s 密码后提示窗 → 全清 → 诚实失败注入。
 * 3. 截图历史：两态（临时/已保存）淘汰引擎，20 条上限、临时优先淘汰、关机
 *    清理通知只发一次。
 */

/* ------------------------------ F509 粉碎计划 ------------------------------ */

/** 默认覆写道数（判据族常用 3 道口径，与 store shred.passes 默认一致）。 */
export const SHRED_DEFAULT_PASSES = 3;
/** 验证道：最后一道后全文件读回校验（判据「覆写执行」的完整性）。 */
export const SHRED_VERIFY_PASS = true;
/** 覆写吞吐估算基准（MB/s，机械盘保守值——耗时提示的数字来源）。 */
export const SHRED_THROUGHPUT_MBPS = 40;
/** SSD 诚实标注文案（判据「诚实标注」分支）。 */
export const SHRED_SSD_NOTICE = "此文件位于 SSD：磨损均衡可能留下残迹，覆写降低可恢复性但不保证清零。" as const;

export type ShredBranch = "overwrite" | "honest-ssd";

export interface ShredPlan {
  branch: ShredBranch;
  passes: number;
  verifyPass: boolean;
  fileSizeBytes: number;
  chunkBytes: number;
  /** 预计总耗时 ms（含验证道——判据「耗时提示（大文件覆写慢）」）。 */
  estimateMs: number;
  notice: string;
}

/** 1 MiB 块（覆写按块推进——进度条可诚实滚动）。 */
export const SHRED_CHUNK_BYTES = 1024 * 1024;

export function planShred(fileSizeBytes: number, passes: number, isSsd: boolean): ShredPlan {
  if (passes < 1 || passes > 7) {
    throw new Error(`[u3:F509] 覆写道数越界 ${passes}——支持 1-7 道`);
  }
  const branch: ShredBranch = isSsd ? "honest-ssd" : "overwrite";
  const writeBytes = fileSizeBytes * (passes + (SHRED_VERIFY_PASS ? 1 : 0));
  const estimateMs = Math.ceil((writeBytes / (SHRED_THROUGHPUT_MBPS * 1024 * 1024)) * 1000);
  return {
    branch,
    passes,
    verifyPass: SHRED_VERIFY_PASS,
    fileSizeBytes,
    chunkBytes: SHRED_CHUNK_BYTES,
    estimateMs,
    notice: branch === "honest-ssd" ? SHRED_SSD_NOTICE : "",
  };
}

/** 耗时人话（三要素文案：发生了什么/为什么/下一步——大文件诚实说慢）。 */
export function shredEtaText(plan: ShredPlan): string {
  const sec = Math.max(1, Math.round(plan.estimateMs / 1000));
  if (sec < 60) return `覆写 ${plan.passes} 道 + 校验 1 道，约 ${sec} 秒`;
  const min = Math.round(sec / 60);
  return `覆写 ${plan.passes} 道 + 校验 1 道，文件较大约 ${min} 分钟——粉碎期间可最小化窗口，完成后会通知`;
}

/* ------------------------------ F511 剪贴板清空 ------------------------------ */

export type ClipWipePhase = "idle" | "confirm" | "postsecret-warn" | "wiped";

export interface ClipWipeState {
  phase: ClipWipePhase;
  itemCount: number;
  warnUntilMs: number;
}

export const CLIP_SECRET_WARN_MS = 5000;

/** 清空确认框条数文案（判据「确认框条数」：当前+历史分开报数）。 */
export function clipWipeConfirmText(current: number, history: number): string {
  return `将清空剪贴板当前内容（${current} 条）与全部历史（${history} 条），此操作不可撤销`;
}

/** 密码复制后 5s 提示时序推进（nowMs 单调）：secretCopiedAtMs 起算。 */
export function clipWipeTick(
  st: ClipWipeState,
  nowMs: number,
  secretCopiedAtMs: number | null,
): ClipWipeState {
  if (st.phase === "confirm" || st.phase === "wiped") return st;
  if (secretCopiedAtMs !== null && nowMs - secretCopiedAtMs >= CLIP_SECRET_WARN_MS && st.phase !== "postsecret-warn") {
    return { ...st, phase: "postsecret-warn", warnUntilMs: nowMs + CLIP_SECRET_WARN_MS };
  }
  return st;
}

/** 清空后的粘贴语义（判据「应用得到诚实失败」）：读端返回显性失败而非空串。 */
export function clipPasteAfterWipe(): { ok: false; reason: "clipboard-empty" } {
  return { ok: false, reason: "clipboard-empty" };
}

/* ------------------------------ F512 截图历史 ------------------------------ */

export type ShotState = "temp" | "saved";

export interface ShotEntry {
  id: string;
  state: ShotState;
  sizeBytes: number;
  savedPath?: string;
}

export const SHOT_HISTORY_CAP = 20;

/**
 * 淘汰引擎（判据「20 条上限与淘汰」）：超限时先淘汰最旧临时件；临时清完
 * 淘汰最旧已保存件（用户显式保存的优先保——数据安全十二章）。
 * 返回被逐出的 id（调用方负责通知与释放）。
 */
export function evictShots(entries: ShotEntry[], cap = SHOT_HISTORY_CAP): Array<string> {
  const evicted: string[] = [];
  const work = [...entries];
  while (work.length > cap) {
    let idx = -1;
    for (let i = 0; i < work.length; i++) {
      if (work[i]!.state === "temp") { idx = i; break; }
    }
    if (idx === -1) idx = 0; // 全是已保存：逐最旧
    evicted.push(work[idx]!.id);
    work.splice(idx, 1);
  }
  return evicted;
}

/** 关机清理通知（判据「关机清理与通知」：通知只发一次——notifiedOnce 闸门）。 */
export function shutdownPurgeNotice(alreadyNotified: boolean): { notify: boolean; text: string } {
  if (alreadyNotified) return { notify: false, text: "" };
  return { notify: true, text: "关机时将清除未保存的临时截图（已保存的不受影响）" };
}

/* ------------------------------ 自检 ------------------------------ */

export function shredplanSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 计划器：机械盘 10MB 3 道 → 4 倍写入量
  const p1 = planShred(10 * 1024 * 1024, 3, false);
  checks.push({ name: "F509 覆写计划含验证道", pass: p1.branch === "overwrite" && p1.passes === 3 && p1.verifyPass });
  // SSD 诚实分支
  const p2 = planShred(1024, 3, true);
  checks.push({ name: "F509 SSD 诚实标注", pass: p2.branch === "honest-ssd" && p2.notice === SHRED_SSD_NOTICE });
  // 道数越界抛错
  let threw = false;
  try { planShred(1024, 8, false); } catch { threw = true; }
  checks.push({ name: "F509 道数越界显性", pass: threw });
  // 耗时提示含道数与分钟级人话
  const big = planShred(8 * 1024 * 1024 * 1024, 3, false);
  checks.push({ name: "F509 大文件耗时人话", pass: shredEtaText(big).includes("分钟") });
  // 清空确认条数
  checks.push({ name: "F511 确认框条数", pass: clipWipeConfirmText(1, 19).includes("1 条") && clipWipeConfirmText(1, 19).includes("19 条") });
  // 5s 密码后提示时序
  let w: ClipWipeState = { phase: "idle", itemCount: 5, warnUntilMs: 0 };
  w = clipWipeTick(w, 1000, 2000);
  const warned = clipWipeTick(w, 7200, 2000);
  checks.push({ name: "F511 5s 提示时序", pass: w.phase === "idle" && warned.phase === "postsecret-warn" });
  // 清空后诚实失败
  checks.push({ name: "F511 诚实失败语义", pass: clipPasteAfterWipe().ok === false && clipPasteAfterWipe().reason === "clipboard-empty" });
  // 淘汰：22 条（3 临时）→ 逐 2 条最旧临时
  const shots: ShotEntry[] = Array.from({ length: 22 }, (_, i) => ({
    id: `s${i}`,
    state: i < 3 ? "temp" : "saved",
    sizeBytes: 1,
  }));
  const ev = evictShots(shots);
  checks.push({ name: "F512 临时优先淘汰", pass: ev.length === 2 && ev.every((id) => id === "s0" || id === "s1") });
  // 全已保存逐最旧
  const savedOnly: ShotEntry[] = Array.from({ length: 21 }, (_, i) => ({ id: `t${i}`, state: "saved", sizeBytes: 1 }));
  const ev2 = evictShots(savedOnly);
  checks.push({ name: "F512 全保存逐最旧", pass: ev2.length === 1 && ev2[0] === "t0" });
  // 关机通知一次性
  checks.push({ name: "F512 关机通知一次", pass: shutdownPurgeNotice(false).notify && !shutdownPurgeNotice(true).notify });
  return checks;
}
