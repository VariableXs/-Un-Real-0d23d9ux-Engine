/**
 * F357 下载收口体验（H 域 · AI-H4）：
 * Edge 下载在系统层的统一收口：下载中任务栏/托盘微进度提示、完成通知一条
 * （文件名+大小+「打开」「所在文件夹」双钮）、大文件下载时系统不睡眠（F316 闲置豁免
 * 同媒体判据）、下载完整性校验（哈希不符重下提示）。
 * 宪法边界（判据「系统不越界审计」）：下载历史 Edge 内管——系统侧只做通知与豁免，
 * 不做下载管理器、不造第二套下载 UI；本模块因此刻意不提供列表/历史/删除接口。
 * 判据（主册 F357）：通知双钮链路；睡眠豁免；校验失败重下路径；系统不越界审计。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 睡眠豁免门槛：活动下载总余量超过该值即豁免（判据同 F316 媒体口径的下载侧）。 */
export const SLEEP_EXEMPT_REMAINING_BYTES = 64 * 1024 * 1024;
/** 下载中托盘微进度的刷新粒度（百分比整数步进，避免高频重绘）。 */
export const TRAY_STEP_PCT = 5;

export interface ActiveDownload {
  id: string;
  fileName: string;
  totalBytes: number;
  doneBytes: number;
}

/** 下载中托盘微进度：整数百分比、TRAY_STEP_PCT 步进取整（重绘节流）。 */
export function trayProgress(d: ActiveDownload): { pct: number; stepped: number; valid: boolean } {
  if (d.totalBytes <= 0 || d.doneBytes < 0) return { pct: 0, stepped: 0, valid: false };
  const raw = Math.min(1, d.doneBytes / d.totalBytes) * 100;
  const stepped = Math.floor(raw / TRAY_STEP_PCT) * TRAY_STEP_PCT;
  return { pct: Math.round(raw * 10) / 10, stepped, valid: true };
}

/** 睡眠豁免判定：存在未完成下载且总余量 ≥ 门槛 → 豁免（判据「大文件不睡死」）。 */
export function sleepExempt(active: ActiveDownload[]): { exempt: boolean; remainingBytes: number; reason: string } {
  const remaining = active.reduce((sum, d) => sum + Math.max(0, d.totalBytes - d.doneBytes), 0);
  if (remaining === 0) return { exempt: false, remainingBytes: 0, reason: "无活动下载" };
  if (remaining >= SLEEP_EXEMPT_REMAINING_BYTES) return { exempt: true, remainingBytes: remaining, reason: "大文件下载中（F316 闲置豁免同媒体判据）" };
  return { exempt: false, remainingBytes: remaining, reason: "小余量下载不豁免（正常闲置策略接管）" };
}

export interface CompletionNotice {
  id: string;
  title: string;
  body: string;
  /** 双钮链路（判据）：打开 / 所在文件夹。 */
  actions: Array<{ id: "open" | "reveal"; label: string }>;
}

export function completionNotice(id: string, fileName: string, sizeBytes: number): CompletionNotice {
  const mb = sizeBytes / (1024 * 1024);
  const sizeText = mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.max(1, Math.round(sizeBytes / 1024))} KB`;
  return {
    id,
    title: `下载完成：${fileName}`,
    body: `大小 ${sizeText}`,
    actions: [
      { id: "open", label: "打开" },
      { id: "reveal", label: "所在文件夹" },
    ],
  };
}

export interface VerifyFailPlan {
  /** 重下提示文案（三要素：发生了什么/为什么/怎么办）。 */
  message: string;
  action: "redownload";
}

/** 校验失败路径：哈希不符 → 重下提示（诚实，不静默留坏文件不提）。 */
export function verifyFailedPlan(fileName: string): VerifyFailPlan {
  return {
    message: `「${fileName}」完整性校验未通过——文件可能在下载中损坏。建议删除后重新下载。`,
    action: "redownload",
  };
}

export interface BoundaryAuditRow {
  /** 审计面名称。 */
  facet: string;
  /** 系统是否越界（造了第二套下载 UI）。 */
  violated: boolean;
  detail: string;
}

/**
 * 系统不越界审计（判据）：逐面确认系统没有做下载管理器的事。
 * 任何一项 violated = 缺陷（宪法边界被突破）。
 */
export function auditBoundary(claims: { hasDownloadListUi?: boolean; hasDownloadHistoryUi?: boolean; hasPauseResumeUi?: boolean }): BoundaryAuditRow[] {
  return [
    { facet: "下载列表 UI", violated: !!claims.hasDownloadListUi, detail: "下载列表归 Edge 管——系统不造第二套" },
    { facet: "下载历史 UI", violated: !!claims.hasDownloadHistoryUi, detail: "历史归 Edge 管（无商店宪法边界自觉）" },
    { facet: "暂停/恢复 UI", violated: !!claims.hasPauseResumeUi, detail: "传输控制归 Edge 管；系统只做收口通知与睡眠豁免" },
  ];
}

/** 通知记录（收口动作留痕——体验日志十三章纪律；只记动作不记文件内容）。 */
export interface ClosureLogEntry {
  at: number;
  noticeId: string;
  /** 用户点了哪个钮：open / reveal / dismiss。 */
  action: "open" | "reveal" | "dismiss";
}

export function logClosure(entry: ClosureLogEntry, store: KvStore = defaultStore()): boolean {
  const all = readJson<ClosureLogEntry[]>(store, h4Key("f357", "log"), [], Array.isArray);
  return writeJson(store, h4Key("f357", "log"), [...all.slice(-99), entry]);
}

export function closureLog(store: KvStore = defaultStore()): ClosureLogEntry[] {
  return readJson<ClosureLogEntry[]>(store, h4Key("f357", "log"), [], Array.isArray);
}
