/**
 * AI-20 质量门禁与收官组 — M-79 前端错误聚合看板（FE Error Board）。
 *
 * ErrorBoundary 捕获的渲染错误 + window.onerror / unhandledrejection 统一进入
 * 环形缓冲（500 条，内存），退出落盘（localStorage JSON）。
 * PII 红线：只保留组件名、错误消息与栈；错误消息中形如用户输入的片段
 * （引号内超长串 / 文件路径后缀）做剥离，绝不聚合用户内容。
 * 设置→质量与诊断区展示近 7 天 Top10（组件、消息、次数、首末时间），
 * 支持一键复制 markdown 诊断报告（可直接贴 issue）。
 */

export interface ErrBoardEntry {
  /** 错误源组件（ErrorBoundary componentName 或 "window"） */
  component: string;
  /** PII 清洗后的错误消息（截断至 200 字符） */
  message: string;
  /** 栈顶帧（仅文件:行，供定位；同样经清洗） */
  stackTop: string | null;
  /** 首次出现时间戳（ms） */
  firstAt: number;
  /** 最近出现时间戳（ms） */
  lastAt: number;
  /** 出现次数 */
  count: number;
}

export interface ErrBoardRecord {
  /** 单条错误事件（聚合前的原始事件） */
  component: string;
  message: string;
  stackTop: string | null;
  ts: number;
}

const RING_MAX = 500;
const PERSIST_KEY = "variable:errboard:v1";
const PERSIST_LIMIT = 200; // 落盘裁剪（环形尾部 200 条，防止 localStorage 膨胀）
const MESSAGE_MAX = 200;

/** PII 清洗：剥离引号内长串、绝对路径前缀，截断超长消息。 */
export function scrubPII(message: string): string {
  let s = String(message ?? "");
  // 引号内 ≥8 字符的串替换为 ⟨…⟩（用户输入内容永不上板）
  s = s.replace(/(["'`])(?:(?!\1).){8,}\1/g, "⟨…⟩");
  // Windows 绝对路径只保留文件名
  s = s.replace(/[A-Za-z]:\\[^\s:]+\\/g, "…\\");
  return s.length > MESSAGE_MAX ? `${s.slice(0, MESSAGE_MAX)}…` : s;
}

/** 从错误栈提取首个项目内帧（src/…）；无栈或无匹配返回 null。 */
export function stackTopOf(stack: string | undefined | null): string | null {
  if (!stack) return null;
  for (const line of String(stack).split("\n").slice(1)) {
    const m = line.match(/\(?((?:src|\/|[A-Za-z]:\\)[^\s()]+\.(?:tsx?|jsx?)):(\d+):(\d+)\)?/);
    const file = m?.[1];
    const lineNo = m?.[2];
    if (file && lineNo) {
      return `${file.replace(/^.*?(src[\\/])/, "src/").replace(/\\/g, "/")}:${lineNo}`;
    }
  }
  return null;
}

/** 环形缓冲（500 条原始事件）。 */
class Ring {
  private buf: ErrBoardRecord[] = [];
  push(r: ErrBoardRecord): void {
    this.buf.push(r);
    if (this.buf.length > RING_MAX) this.buf.splice(0, this.buf.length - RING_MAX);
  }
  all(): readonly ErrBoardRecord[] {
    return this.buf;
  }
  clear(): void {
    this.buf = [];
  }
}

const ring = new Ring();

/** 记录一条错误事件（自动 PII 清洗）。 */
export function recordError(component: string, message: string, stack?: string | null, ts = Date.now()): void {
  ring.push({
    component: String(component || "unknown").slice(0, 80),
    message: scrubPII(message),
    stackTop: stackTopOf(stack),
    ts,
  });
}

/** 聚合：近 `days` 天 Top `limit` 错误（按次数降序，其次最近时间）。 */
export function aggregateErrors(
  records: readonly ErrBoardRecord[],
  days = 7,
  limit = 10,
  now = Date.now(),
): ErrBoardEntry[] {
  const since = now - days * 24 * 3600 * 1000;
  const map = new Map<string, ErrBoardEntry>();
  for (const r of records) {
    if (r.ts < since) continue;
    const key = `${r.component}\u0000${r.message}`;
    const prev = map.get(key);
    if (prev) {
      prev.count++;
      prev.lastAt = Math.max(prev.lastAt, r.ts);
    } else {
      map.set(key, {
        component: r.component,
        message: r.message,
        stackTop: r.stackTop,
        firstAt: r.ts,
        lastAt: r.ts,
        count: 1,
      });
    }
  }
  return [...map.values()]
    .sort((a, b) => b.count - a.count || b.lastAt - a.lastAt)
    .slice(0, limit);
}

/** 生成 markdown 诊断报告（贴 issue 即用）。 */
export function buildErrReport(entries: readonly ErrBoardEntry[], appVersion = "?"): string {
  const lines: string[] = [
    "## Variable 前端错误报告（FE Error Board）",
    "",
    `- 版本：v${appVersion}`,
    `- 导出时间：${new Date().toISOString()}`,
    `- 聚合窗口：近 7 天，Top ${entries.length}`,
    "",
  ];
  if (entries.length === 0) {
    lines.push("（无错误记录）");
    return lines.join("\n");
  }
  for (const [i, e] of entries.entries()) {
    lines.push(
      `### ${i + 1}. [${e.count} 次] ${e.component}`,
      "",
      `- 消息：${e.message}`,
      `- 栈顶：${e.stackTop ?? "（无）"}`,
      `- 首次：${new Date(e.firstAt).toISOString()} ｜ 最近：${new Date(e.lastAt).toISOString()}`,
      "",
    );
  }
  return lines.join("\n");
}

// ---------- 落盘（localStorage；退出 flush + 页面隐藏时兜底） ----------

export function persistErrBoard(): void {
  try {
    const all = ring.all();
    localStorage.setItem(PERSIST_KEY, JSON.stringify(all.slice(-PERSIST_LIMIT)));
  } catch {
    /* localStorage 不可用（隐私模式等）——内存聚合照常，落盘如实失败静默 */
  }
}

export function loadErrBoard(): ErrBoardRecord[] {
  try {
    const raw = localStorage.getItem(PERSIST_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (r): r is ErrBoardRecord =>
        !!r && typeof (r as ErrBoardRecord).component === "string" && typeof (r as ErrBoardRecord).ts === "number",
    );
  } catch {
    return [];
  }
}

/** 供看板 UI：合并「本次会话内存 + 上次落盘」。 */
export function boardRecords(): ErrBoardRecord[] {
  return [...loadErrBoard(), ...ring.all()];
}

export function clearErrBoard(): void {
  ring.clear();
  try {
    localStorage.removeItem(PERSIST_KEY);
  } catch {
    /* ignore */
  }
}

/** 安装全局兜底监听（App 启动调用一次；幂等）。 */
let installed = false;
export function installErrBoardGlobal(): void {
  if (installed || typeof window === "undefined") return;
  installed = true;
  window.addEventListener("error", (e) => {
    recordError("window", e.message ?? String(e.error ?? "error"), e.error instanceof Error ? e.error.stack : null);
  });
  window.addEventListener("unhandledrejection", (e) => {
    const r = e.reason;
    recordError(
      "promise",
      r instanceof Error ? r.message : String(r ?? "unhandled rejection"),
      r instanceof Error ? r.stack : null,
    );
  });
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") persistErrBoard();
  });
}
