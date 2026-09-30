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

/* ================= v4 深化批次四：收口编排器（观察侧状态机——系统只观察不拥有） ================= */

/** 下载观察态（Edge 侧事实在系统层的投影；系统侧零命令面——宪法边界的结构保持）。 */
export type DownloadPhase = "active" | "completed" | "verify-failed";

/** 下载事件流（Edge 上报：started/progress/completed/verifyFailed）。 */
export type DownloadEvent =
  | { type: "started"; id: string; fileName: string; totalBytes: number; at: number }
  | { type: "progress"; id: string; doneBytes: number; at: number }
  | { type: "completed"; id: string; fileName: string; sizeBytes: number; at: number }
  | { type: "verifyFailed"; id: string; fileName: string; at: number };

export interface DownloadObservation {
  id: string;
  fileName: string;
  phase: DownloadPhase;
  doneBytes: number;
  totalBytes: number;
  updatedAt: number;
}

/** 事件归约器：把 Edge 事件流折叠成系统侧观察态（进度外推与豁免判定的数据源）。 */
export function reduceEvents(events: DownloadEvent[]): Map<string, DownloadObservation> {
  const map = new Map<string, DownloadObservation>();
  for (const e of events) {
    const cur = map.get(e.id);
    if (e.type === "started") {
      map.set(e.id, { id: e.id, fileName: e.fileName, phase: "active", doneBytes: 0, totalBytes: e.totalBytes, updatedAt: e.at });
    } else if (e.type === "progress" && cur) {
      map.set(e.id, { ...cur, doneBytes: Math.max(cur.doneBytes, e.doneBytes), updatedAt: e.at });
    } else if (e.type === "completed" && cur) {
      map.set(e.id, { ...cur, phase: "completed", doneBytes: cur.totalBytes, updatedAt: e.at });
    } else if (e.type === "verifyFailed" && cur) {
      map.set(e.id, { ...cur, phase: "verify-failed", updatedAt: e.at });
    }
  }
  return map;
}

/** 托盘聚合态（多下载并存时的单点呈现：活动优先、余量取和、步进取整复用 trayProgress 口径）。 */
export function traySummary(obs: Map<string, DownloadObservation>): { activeCount: number; remainingBytes: number; steppedPct: number; exempt: boolean } {
  const all = [...obs.values()];
  const active = all.filter((o) => o.phase === "active");
  const remaining = active.reduce((s, o) => s + Math.max(0, o.totalBytes - o.doneBytes), 0);
  const donePct = active.length === 0 ? 100 : (active.reduce((s, o) => s + (o.totalBytes > 0 ? o.doneBytes / o.totalBytes : 1), 0) / active.length) * 100;
  const stepped = Math.floor(donePct / TRAY_STEP_PCT) * TRAY_STEP_PCT;
  return { activeCount: active.length, remainingBytes: remaining, steppedPct: stepped, exempt: remaining >= SLEEP_EXEMPT_REMAINING_BYTES };
}

/** 完成通知合并窗口与阈值（完成风暴不轰炸——通知也是一种体验）。 */
export const NOTICE_COALESCE_WINDOW_MS = 4000;
export const NOTICE_COALESCE_THRESHOLD = 3;

export interface CoalescedNotice {
  digest: boolean;
  title: string;
  body: string;
  ids: string[];
}

/** 完成通知合并：窗口内 ≥3 条合一摘要；散条走双钮原文案（双钮链路判据不因合并丢失）。 */
export function coalesceNotices(completions: Array<{ id: string; fileName: string; sizeBytes: number; at: number }>): CoalescedNotice[] {
  const sorted = [...completions].sort((a, b) => a.at - b.at);
  const out: CoalescedNotice[] = [];
  let bucket: typeof sorted = [];
  const flush = () => {
    if (bucket.length === 0) return;
    if (bucket.length >= NOTICE_COALESCE_THRESHOLD) {
      const totalMb = bucket.reduce((s, c) => s + c.sizeBytes, 0) / (1024 * 1024);
      out.push({ digest: true, title: `${bucket.length} 个下载已完成`, body: `共 ${totalMb.toFixed(1)} MB`, ids: bucket.map((c) => c.id) });
    } else {
      for (const c of bucket) {
        const n = completionNotice(c.id, c.fileName, c.sizeBytes);
        out.push({ digest: false, title: n.title, body: n.body, ids: [c.id] });
      }
    }
    bucket = [];
  };
  for (const c of sorted) {
    if (bucket.length > 0 && c.at - bucket[0]!.at > NOTICE_COALESCE_WINDOW_MS) flush();
    bucket.push(c);
  }
  flush();
  return out;
}

/** 完整性校验判定（verifyFailedPlan 的判定面）：期望哈希存在且不符才走重下路径——无期望哈希如实 unknown。 */
export function verifyVerdict(expectedHash: string | null, actualHash: string): { verdict: "match" | "mismatch" | "unknown"; plan: VerifyFailPlan | null } {
  if (expectedHash === null) return { verdict: "unknown", plan: null };
  if (expectedHash === actualHash) return { verdict: "match", plan: null };
  return { verdict: "mismatch", plan: verifyFailedPlan("下载文件") };
}

/** 豁免时间线（体验日志十三章：豁免何时开始、因何开始——可回放不黑箱）。 */
export interface ExemptionSpan {
  from: number;
  to: number | null;
  reason: string;
}

export function exemptionTimeline(events: DownloadEvent[]): ExemptionSpan[] {
  const obs = reduceEvents(events);
  return [...obs.values()]
    .filter((o) => o.phase === "active" && o.totalBytes - o.doneBytes >= SLEEP_EXEMPT_REMAINING_BYTES)
    .map((o) => ({ from: o.updatedAt, to: null, reason: `「${o.fileName}」余量 ${Math.round((o.totalBytes - o.doneBytes) / 1048576)} MB` }));
}
