/**
 * 异常实时分析 L0 — 统一前端日志门面（FE Unified Logger）。
 *
 * 目标：一次崩溃发生后，AI（或人）能从一份导出里看到"功能哪里异常 + 崩溃前现场"：
 *
 * 采集层（零调用点改造）：
 * - console.warn / console.error 桥接：既有 40+ 处裸 console.* 自动进入时间线，
 *   首参 `[tag]` 前缀被提取为模块标签（如 `[launcher]` → tag=launcher）。
 * - window error / unhandledrejection：进 errBoard 环形（复用 installErrBoardGlobal）
 *   + 崩溃叙事（installSessionNarrative）+ 本时间线，console 镜像保持原样。
 * - logError() 显式出口：时间线 + errBoard + ipc.log（→ Rust variable.log 与
 *   applog 总线，任务管理器「日志」页实时可见）三路同步。
 *
 * 存储：内存环形 600 条（本会话）；visibilitychange 时落盘尾部 200 条到
 * localStorage（`variable:log:v1`），崩溃后下次会话仍能取到"上一会话尾部"。
 *
 * 会话关联：每窗口每次加载生成 8 位十六进制 sessionId，出现在导出报告头部
 * 与 ipc.log 消息前缀中，用于把 variable.log / applog-*.log / 时间线对齐到同一会话。
 *
 * AI 导出：buildDiagnosticReport() = errBoard 聚合报告 + 上一会话尾部 + 本会话
 * 时间线，markdown 格式，设置→质量与诊断一键复制。
 *
 * PII 红线：进 errBoard 的消息经 scrubPII 清洗；本时间线只保留在前端内存/localStorage
 * 与用户主动导出中，不上传任何远端。
 */

import {
  recordError,
  persistErrBoard,
  installErrBoardGlobal,
  aggregateErrors,
  boardRecords,
  buildErrReport,
  type ErrBoardEntry,
} from "./errBoard";
import { installSessionNarrative } from "../system/perf/sessionNarrative";
import { ipc } from "./ipc";

export type LogLevel = "debug" | "info" | "warn" | "error";

export interface LogRecord {
  /** ms since epoch */
  ts: number;
  level: LogLevel;
  /** 模块/链路标签（console 桥接时自动提取自 `[tag]` 前缀） */
  tag: string;
  msg: string;
  /** 产生该条记录的会话 ID（8 位十六进制） */
  sid: string;
}

const RING_MAX = 600;
const PERSIST_KEY = "variable:log:v1";
const PERSIST_LIMIT = 200; // 落盘裁剪（尾部 200 条，防止 localStorage 膨胀）
const MSG_MAX = 500;
const REPORT_TIMELINE_MAX = 300;

// ---------- 会话 ID ----------

let sidCache = "";
/** 当前窗口会话 ID（惰性生成，进程生命周期内恒定；对齐 variable.log/applog 前缀）。 */
export function sessionId(): string {
  if (!sidCache) {
    const c = globalThis.crypto;
    if (c && typeof c.getRandomValues === "function") {
      const b = new Uint8Array(4);
      c.getRandomValues(b);
      sidCache = [...b].map((x) => x.toString(16).padStart(2, "0")).join("");
    } else {
      sidCache = Math.floor(Math.random() * 0xffffffff).toString(16).padStart(8, "0");
    }
  }
  return sidCache;
}

// ---------- 环形缓冲 ----------

const buf: LogRecord[] = [];

function push(level: LogLevel, tag: string, msg: string): void {
  buf.push({
    ts: Date.now(),
    level,
    tag: tag.slice(0, 40),
    msg: msg.length > MSG_MAX ? `${msg.slice(0, MSG_MAX)}…` : msg,
    sid: sessionId(),
  });
  if (buf.length > RING_MAX) buf.splice(0, buf.length - RING_MAX);
}

/** 本会话时间线（只读快照）。 */
export function logRecords(): readonly LogRecord[] {
  return buf;
}

export function clearLogTimeline(): void {
  buf.length = 0;
}

// ---------- 显式日志 API ----------

/** 调试细节（仅进本会话时间线，不镜像 console）。 */
export function logDebug(tag: string, msg: string): void {
  push("debug", tag, msg);
}

/** 常规事件（仅进本会话时间线，不镜像 console）。 */
export function logInfo(tag: string, msg: string): void {
  push("info", tag, msg);
}

/** 警告（时间线 + console.warn 镜像）。 */
export function logWarn(tag: string, msg: string): void {
  push("warn", tag, msg);
  mirrorConsole("warn", `[${tag}]`, msg);
}

/**
 * 错误统一出口：时间线 + console.error 镜像 + errBoard 聚合 +
 * ipc.log（→ Rust variable.log 与 applog 总线实时推送）。
 */
export function logError(tag: string, msg: string, stack?: string | null): void {
  push("error", tag, msg);
  recordError(tag, msg, stack);
  mirrorConsole("error", `[${tag}]`, msg);
  void ipc.log("error", `[${sessionId()}] ${tag}: ${msg}`).catch(() => {
    /* 无 Tauri 运行时（纯浏览器 dev）——落盘如实失败 */
  });
}

// ---------- console 桥接（零调用点改造） ----------

let muting = false;
let tapInstalled = false;
let origError: ((...a: unknown[]) => void) | null = null;
let origWarn: ((...a: unknown[]) => void) | null = null;

function argToString(a: unknown): string {
  if (typeof a === "string") return a;
  if (a instanceof Error) return a.message;
  try {
    return JSON.stringify(a) ?? String(a);
  } catch {
    return String(a);
  }
}

function mirrorConsole(level: "warn" | "error", prefix: string, msg: string): void {
  const orig = level === "error" ? origError : origWarn;
  muting = true;
  try {
    (orig ?? (level === "error" ? console.error : console.warn)).call(console, prefix, msg);
  } finally {
    muting = false;
  }
}

/** console.warn/error → 时间线（提取 `[tag]` 前缀为模块标签；console 原输出保持不变）。 */
export function captureConsoleMessage(level: "warn" | "error", args: readonly unknown[]): void {
  if (muting) return;
  let tag = "console";
  let msg = args.map(argToString).join(" ");
  const m = msg.match(/^\[([A-Za-z][\w-]*)\]\s*/);
  if (m) {
    tag = m[1] ?? "console";
    msg = msg.slice(m[0].length);
  }
  push(level, tag, msg);
}

function installConsoleTap(): void {
  if (tapInstalled) return;
  tapInstalled = true;
  origError = console.error;
  origWarn = console.warn;
  console.error = (...a: unknown[]) => {
    captureConsoleMessage("error", a);
    origError?.apply(console, a);
  };
  console.warn = (...a: unknown[]) => {
    captureConsoleMessage("warn", a);
    origWarn?.apply(console, a);
  };
}

// ---------- 全局安装（各窗口入口 setupEntryRuntime 调用一次；幂等） ----------

let captureInstalled = false;

export function installLogCapture(site: string): void {
  if (captureInstalled) return;
  if (typeof window === "undefined" || typeof window.addEventListener !== "function") return;
  captureInstalled = true;

  // 既有基础设施接线（此前从未激活）：errBoard 全局错误环形 + 落盘
  installErrBoardGlobal();
  // U-23 崩溃叙事：30s 心跳快照 + 下次启动恢复卡片数据
  installSessionNarrative(site);
  // console 桥接：既有裸 console.* 自动进时间线
  installConsoleTap();

  window.addEventListener("error", (e) => {
    muting = true;
    try {
      push("error", "window", String(e.message ?? e.error ?? "error"));
      (origError ?? console.error).call(console, "[Variable] uncaught", e.error ?? e.message);
    } finally {
      muting = false;
    }
  });
  window.addEventListener("unhandledrejection", (e) => {
    muting = true;
    try {
      const r = (e as PromiseRejectionEvent).reason;
      push("error", "promise", r instanceof Error ? r.message : String(r ?? "unhandled rejection"));
      (origError ?? console.error).call(console, "[Variable] unhandled rejection", r);
    } finally {
      muting = false;
    }
  });
  if (typeof document !== "undefined") {
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "hidden") {
        persistLogTail();
        persistErrBoard();
      }
    });
  }
}

// ---------- 落盘（localStorage；崩溃后下一会话仍可读"上一会话尾部"） ----------

export function persistLogTail(): void {
  try {
    localStorage.setItem(PERSIST_KEY, JSON.stringify(buf.slice(-PERSIST_LIMIT)));
  } catch {
    /* localStorage 不可用（隐私模式等）——内存时间线照常 */
  }
}

function isLogRecord(r: unknown): r is LogRecord {
  const v = r as LogRecord;
  return (
    !!v &&
    typeof v.ts === "number" &&
    (v.level === "debug" || v.level === "info" || v.level === "warn" || v.level === "error") &&
    typeof v.tag === "string" &&
    typeof v.msg === "string"
  );
}

/** 上一会话落盘尾部（崩溃前现场；与当前时间线重叠的旧记录已按 ts 去重）。 */
export function loadPreviousTail(): LogRecord[] {
  try {
    const raw = localStorage.getItem(PERSIST_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isLogRecord);
  } catch {
    return [];
  }
}

// ---------- AI 可读诊断报告 ----------

function stamp(ts: number): string {
  const d = new Date(ts);
  const p = (n: number, w = 2): string => String(n).padStart(w, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
}

export function formatRecord(r: LogRecord, withSid = false): string {
  const sid = withSid ? ` [${r.sid}]` : "";
  return `${stamp(r.ts)} [${r.level.toUpperCase()}]${sid} [${r.tag}] ${r.msg}`;
}

/**
 * 组合诊断报告（markdown，贴给 AI 即可定位）：
 * errBoard 近 7 天聚合 + 上一会话日志尾部（崩溃前现场）+ 本会话时间线。
 */
export function buildDiagnosticReport(entries: readonly ErrBoardEntry[], appVersion = "?"): string {
  const sections: string[] = [buildErrReport(entries, appVersion)];

  // 上一会话尾部：只保留早于本会话最早记录的条目（同页隐藏/恢复不重复）
  const cutoff = buf.length > 0 ? buf[0]!.ts : Number.POSITIVE_INFINITY;
  const prev = loadPreviousTail().filter((r) => r.ts < cutoff);
  if (prev.length > 0) {
    sections.push(
      "",
      "## 上一会话日志尾部（可能包含崩溃前现场）",
      "",
      ...prev.slice(-REPORT_TIMELINE_MAX).map((r) => `- ${formatRecord(r, true)}`),
    );
  }

  sections.push(
    "",
    "## 本会话日志时间线",
    "",
    `- 会话 ID：${sessionId()}（variable.log / applog-*.log 中 \`[<sid>]\` 前缀与之对应）`,
    `- 记录数：${buf.length}（内存环形上限 ${RING_MAX}）`,
    "",
  );
  if (buf.length === 0) {
    sections.push("（本会话暂无日志记录）");
  } else {
    sections.push(...buf.slice(-REPORT_TIMELINE_MAX).map((r) => `- ${formatRecord(r)}`));
  }

  // 附：近 7 天异常聚合的原始计数口径说明（与 errBoard 上板数据同源）
  const total = boardRecords().length;
  const top = aggregateErrors(boardRecords(), 7, 1);
  if (top.length > 0 && top[0]) {
    sections.push("", `> 备注：errBoard 共 ${total} 条原始事件（含上次落盘），最高频异常「${top[0].component}: ${top[0].message}」×${top[0].count}。`);
  }
  return sections.join("\n");
}

// ---------- 测试辅助 ----------

/** 仅供单测重置模块状态（安装标志/缓冲/会话 ID/console 桥不还原）。 */
export function _resetLoggerForTest(): void {
  buf.length = 0;
  sidCache = "";
  muting = false;
}
