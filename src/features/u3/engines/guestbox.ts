/**
 * F506 深化引擎 · 访客模式沙盒账（AI-U3 · guestbox）。
 *
 * 判据唯一源（主册摘文）：「沙盒隔离四判据（文件/设置/权限/网络共享）；退出
 * 清理完整性（注入残留文件验证）；入口开关；极简形制；会话时长上限提示（默认
 * 2h）」。
 *
 * 深化点：
 * 1. 四轴隔离不是四个布尔，而是**四本账**：访客会话内每个写动作按轴记账
 *    （文件落点/设置改动/权限申请/共享暴露），退出时逐轴清算——「清理完整」
 *    是账目平衡，不是口头承诺。
 * 2. 残留注入验证：退出后注入残留文件样本，扫描器必须全数检出（判据的
 *    验证方法本体进引擎）。
 * 3. 会话 2h 上限：剩余时间分级提示（30min/5min），到期收尾语义明确。
 * 4. 极简形制：访客会话 UI 允许面白名单（超出白名单的请求显性拒绝）。
 */

/* ------------------------------ 会话账本 ------------------------------ */

export type GuestAxis = "file" | "settings" | "permission" | "netshare";
export const GUEST_AXES: readonly GuestAxis[] = ["file", "settings", "permission", "netshare"];

export interface GuestWrite {
  axis: GuestAxis;
  /** 写入标识：文件路径 / 设置键 / 权限名 / 共享点。 */
  target: string;
  atMs: number;
  /** 清理方式：删除文件 / 还原设置 / 回收授权 / 关闭共享。 */
  rollback: "delete" | "restore" | "revoke" | "close";
}

export interface GuestSession {
  startedAtMs: number;
  maxMinutes: number;
  writes: GuestWrite[];
  closed: boolean;
}

export const GUEST_DEFAULT_MAX_MINUTES = 120;
/** 剩余时间提示分级（分钟阈值）。 */
export const GUEST_WARN_MINUTES = [30, 5] as const;

export function guestOpenSession(nowMs: number, maxMinutes = GUEST_DEFAULT_MAX_MINUTES): GuestSession {
  return { startedAtMs: nowMs, maxMinutes, writes: [], closed: false };
}

/** 记账：只收四轴写入，未知轴诚实抛错（零静默）。 */
export function guestRecord(s: GuestSession, w: GuestWrite): void {
  if (s.closed) throw new Error("[u3:F506] 会话已关闭——关闭后不允许再记账");
  if (!GUEST_AXES.includes(w.axis)) {
    throw new Error(`[u3:F506] 未知隔离轴 ${String(w.axis)}——只接受 ${GUEST_AXES.join("/")}`);
  }
  s.writes.push(w);
}

/** 剩余分钟与提示级（-1=已到期）。 */
export function guestRemaining(s: GuestSession, nowMs: number): { minutesLeft: number; warnLevel: 0 | 1 | 2 | 3 } {
  const elapsedMin = (nowMs - s.startedAtMs) / 60000;
  const left = s.maxMinutes - elapsedMin;
  if (left <= 0) return { minutesLeft: 0, warnLevel: 3 };
  if (left <= GUEST_WARN_MINUTES[1]) return { minutesLeft: left, warnLevel: 2 };
  if (left <= GUEST_WARN_MINUTES[0]) return { minutesLeft: left, warnLevel: 1 };
  return { minutesLeft: left, warnLevel: 0 };
}

/* ------------------------------ 退出清算 ------------------------------ */

export interface GuestCloseReport {
  rolledBack: Array<GuestWrite>;
  failed: Array<{ write: GuestWrite; reason: string }>;
  clean: boolean;
}

/**
 * 退出清算（判据：退出清理完整性）：逆序回滚（后写的先撤——依赖顺序），
 * 每笔回滚结果显性；任一失败 clean=false（调用方必须把失败摆到用户面前）。
 * rollbackFn 注入式执行器——引擎不碰真实文件系统，测试注入成败样本。
 */
export function guestClose(
  s: GuestSession,
  rollbackFn: (w: GuestWrite) => { ok: boolean; reason?: string },
): GuestCloseReport {
  const rolledBack: GuestWrite[] = [];
  const failed: Array<{ write: GuestWrite; reason: string }> = [];
  for (let i = s.writes.length - 1; i >= 0; i--) {
    const w = s.writes[i]!;
    const r = rollbackFn(w);
    if (r.ok) rolledBack.push(w);
    else failed.push({ write: w, reason: r.reason ?? "回滚器未说明原因" });
  }
  s.closed = true;
  return { rolledBack, failed, clean: failed.length === 0 };
}

/* --------------------------- 残留注入扫描 --------------------------- */

/**
 * 残留扫描器（判据「注入残留文件验证」的方法本体）：清算后按账本 target
 * 存在性探测——账上有、盘上还有 = 残留。返回残留清单（应为空）。
 * existsFn 注入式存在性探测。
 */
export function guestScanResidue(s: GuestSession, existsFn: (target: string) => boolean): Array<GuestWrite> {
  return s.writes.filter((w) => w.rollback !== "revoke" && w.rollback !== "close" && existsFn(w.target));
}

/* --------------------------- 极简形制白名单 --------------------------- */

/** 访客会话允许的界面面（判据「极简形制」白名单）。 */
export const GUEST_UI_ALLOWLIST = ["desktop", "explorer", "browser", "notepad", "settings-read-only"] as const;
export type GuestUiFace = (typeof GUEST_UI_ALLOWLIST)[number];

export function guestUiAllowed(face: string): { allowed: boolean; hint?: string } {
  if ((GUEST_UI_ALLOWLIST as readonly string[]).includes(face)) return { allowed: true };
  return { allowed: false, hint: `访客模式不提供「${face}」——主账户登录后可用` };
}

/* ------------------------------ 自检 ------------------------------ */

export function guestboxSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 记账+逆序清算：三笔全成功 → clean
  const s1 = guestOpenSession(0);
  guestRecord(s1, { axis: "file", target: "C:/guest/dl.bin", atMs: 1, rollback: "delete" });
  guestRecord(s1, { axis: "settings", target: "wallpaper", atMs: 2, rollback: "restore" });
  guestRecord(s1, { axis: "permission", target: "camera", atMs: 3, rollback: "revoke" });
  const order: string[] = [];
  const rep = guestClose(s1, (w) => { order.push(w.target); return { ok: true }; });
  checks.push({ name: "F506 四轴记账清算", pass: rep.clean && rep.rolledBack.length === 3 && order[0] === "camera" });
  // 未知轴诚实抛错
  let threw = false;
  try { guestRecord(guestOpenSession(0), { axis: "network" as GuestAxis, target: "x", atMs: 0, rollback: "delete" }); } catch { threw = true; }
  checks.push({ name: "F506 未知轴显性报错", pass: threw });
  // 失败回滚 → clean=false 且失败理由显性
  const s2 = guestOpenSession(0);
  guestRecord(s2, { axis: "file", target: "C:/guest/a.tmp", atMs: 1, rollback: "delete" });
  const rep2 = guestClose(s2, () => ({ ok: false, reason: "文件被占用" }));
  checks.push({ name: "F506 回滚失败显性", pass: !rep2.clean && rep2.failed[0]?.reason === "文件被占用" });
  // 残留注入扫描：注入三处、清掉两处 → 检出一处
  const s3 = guestOpenSession(0);
  const residueTarget = "C:/guest/leftover.bin";
  guestRecord(s3, { axis: "file", target: residueTarget, atMs: 1, rollback: "delete" });
  guestRecord(s3, { axis: "file", target: "C:/guest/gone.bin", atMs: 2, rollback: "delete" });
  const found = guestScanResidue(s3, (t) => t === residueTarget);
  checks.push({ name: "F506 残留注入检出", pass: found.length === 1 && found[0]?.target === residueTarget });
  // 2h 时限提示分级
  const s4 = guestOpenSession(0);
  checks.push({ name: "F506 2h 提示分级", pass: guestRemaining(s4, 0).warnLevel === 0 && guestRemaining(s4, 95 * 60000).warnLevel === 1 && guestRemaining(s4, 116 * 60000).warnLevel === 2 && guestRemaining(s4, 121 * 60000).warnLevel === 3 });
  // 白名单：notepad 放行、terminal 拒绝带提示
  checks.push({ name: "F506 极简白名单", pass: guestUiAllowed("notepad").allowed && !guestUiAllowed("terminal").allowed && guestUiAllowed("terminal").hint !== undefined });
  return checks;
}
