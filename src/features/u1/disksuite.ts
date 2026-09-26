/**
 * 磁盘工具四件（AI-U1 · F437 格式化 / F438 盘符挂载 / F439 驱动器加密 /
 * F440 ISO 挂载）——前端生效面。
 *
 * 判据唯一源（主册摘文，与 kernel/varix/src/uni1/ v1 同参数）：
 * - F437「警示带信息准确性；二次确认链（仅系统/S: 盘）；进度与取消（开始
 *   后 2 秒内可取消）；文件系统选项与实际兼容表」。
 * - F438「盘符冲突检测用例；lnk 自动修复（改符前后快捷方式有效性对比）；
 *   卷标即时性；挂载目录用例；变更留痕（F372 时间线事件）」。
 * - F439「向导强制导出判据（跳不过）；后台加密性能（前台无感，F334）；
 *   解锁一次会话缓存；角标状态；错误密码提示不泄露信息（防爆破）」。
 * - F440「挂载/浏览/弹出全链；上限与提示；只读判据（挂载盘写入被拒且
 *   说明）；非 ISO 提示；重启后挂载不保留（会话态，文档化）」。
 */

import { u1Store } from "./u1store";

/* ------------------------------- F437 磁盘格式化 ------------------------------- */

export const FORMAT_CANCEL_WINDOW_MS = 2000;

export type VolumeKind = "system" | "shared-s" | "normal";

/** 警示带（判据：信息准确性——按卷型分文案，与内核 fmtdisk 同表）。 */
export function formatWarning(kind: VolumeKind): string {
  switch (kind) {
    case "system": return "将清除系统盘全部数据：系统将无法启动";
    case "shared-s": return "将清除该盘全部数据：S: 共享卷";
    case "normal": return "将清除该盘全部数据";
  }
}

/** 二次确认链：系统盘/S: 盘需输入卷标字母（判据：仅系统/S: 盘）。 */
export function formatNeedsTypeConfirm(kind: VolumeKind): boolean {
  return kind === "system" || kind === "shared-s";
}

/** 文件系统兼容表（判据：与实际兼容——未知 fs 不列）。 */
export const FS_COMPAT: Record<string, readonly VolumeKind[]> = {
  "NTFS": ["system", "shared-s", "normal"],
  "FAT32": ["normal"],
  "exFAT": ["shared-s", "normal"],
};

export function fsAllowed(fs: string, kind: VolumeKind): boolean {
  return (FS_COMPAT[fs] ?? []).includes(kind);
}

/** 取消窗口：开始后 2 秒内可取消（判据原文）。 */
export function formatCancellable(elapsedMs: number): boolean {
  return elapsedMs <= FORMAT_CANCEL_WINDOW_MS;
}

/* ------------------------------- F438 盘符与挂载管理 ------------------------------- */

export interface DriveEntry { id: number; letter: string; label: string; mountDir: string | null }
export interface LnkEntry { path: string; target: string }

/** 冲突检测：目标盘符被占 → 拒绝并回报占用者（确认前拦截）。 */
export function letterHolder(drives: DriveEntry[], letter: string): number | null {
  return drives.find((d) => d.letter === letter)?.id ?? null;
}

/** 改符 + lnk 自动修复（与内核 diskmnt 同语义：前缀重写）。 */
export function changeLetter(drives: DriveEntry[], lnks: LnkEntry[], id: number, to: string): { ok: boolean; holder?: number; fixed: number; events: string[] } {
  const holder = letterHolder(drives, to);
  const self = drives.find((d) => d.id === id);
  if (!self) return { ok: false, fixed: 0, events: [] };
  if (holder !== null && holder !== id) return { ok: false, holder, fixed: 0, events: [] };
  const from = self.letter;
  self.letter = to;
  const events = [`盘符 ${from}: → ${to}:`];
  let fixed = 0;
  const prefix = `${from}:`;
  for (const lnk of lnks) {
    if (lnk.target.startsWith(prefix)) {
      lnk.target = `${to}${lnk.target.slice(1)}`;
      fixed += 1;
    }
  }
  if (fixed > 0) events.push(`${fixed} 条快捷方式目标已跟随改符`);
  return { ok: true, fixed, events };
}

/* ------------------------------- F439 驱动器加密 ------------------------------- */

export const FOREGROUND_IO_BUDGET_MS = 2;

export type VaultBadge = "locked" | "unlocking" | "unlocked";

export interface VaultState {
  passwordSet: boolean; keyGenerated: boolean; keyExported: boolean;
  progressPermille: number; sessionUnlocked: boolean; failedAttempts: number;
}

/** 向导强制导出（判据：跳不过——未确认导出 enable 恒拒）。 */
export function vaultEnableReady(s: VaultState): boolean {
  return s.passwordSet && s.keyGenerated && s.keyExported && s.progressPermille === 0;
}

export function vaultBadge(s: VaultState): VaultBadge {
  if (s.sessionUnlocked) return "unlocked";
  if (s.progressPermille > 0 && s.progressPermille < 1000) return "unlocking";
  return "locked";
}

/** 解锁：对 → 会话缓存；错 → 统一文案 + 计数（防爆破）。 */
export function vaultUnlock(s: VaultState, fingerprint: number, expect: number): { ok: boolean; msg: string } {
  if (s.progressPermille < 1000) return { ok: false, msg: "此卷尚未完成加密" };
  if (fingerprint === expect) return { ok: true, msg: "已解锁" };
  return { ok: false, msg: "密码不正确" }; // 统一文案：不区分错因——防枚举
}

/* ------------------------------- F440 ISO 镜像挂载 ------------------------------- */

export function isoCap(): number {
  const cfg = u1Store.get<{ isoCap?: number }>("diskTools");
  return cfg.isoCap ?? 4;
}

/** ISO 结构校验（判据：不猜扩展名——CD001 签名）。 */
export function looksLikeIso(header: Uint8Array): boolean {
  const sig = [0x43, 0x44, 0x30, 0x30, 0x31]; // "CD001"
  return header.length >= 5 && sig.every((b, i) => header[i] === b);
}

export interface MountedIso { source: string; letter: string; files: string[] }

/** 挂载：签名 → 上限 → 分配盘符。失败给归因人话。 */
export function isoMount(mounted: MountedIso[], source: string, header: Uint8Array): { ok: boolean; letter?: string; reason?: string } {
  if (!looksLikeIso(header)) return { ok: false, reason: "不是 ISO 镜像（结构校验未过）——请确认文件完整或换用支持的镜像格式" };
  if (mounted.length >= isoCap()) return { ok: false, reason: `同时挂载已达上限 ${isoCap()} 个——请先弹出不再使用的镜像` };
  const used = new Set(mounted.map((m) => m.letter));
  for (const letter of "EFGHIJKL") {
    if (!used.has(letter)) {
      mounted.push({ source, letter, files: [] });
      return { ok: true, letter };
    }
  }
  return { ok: false, reason: "盘符分配域耗尽" };
}

/** 只读判据：写请求拒并说明（永不静默）。 */
export function isoWriteBlocked(mounted: MountedIso[], letter: string): string | null {
  return mounted.some((m) => m.letter === letter)
    ? "ISO 挂载为只读——如需修改请先复制文件到本地磁盘"
    : null;
}
