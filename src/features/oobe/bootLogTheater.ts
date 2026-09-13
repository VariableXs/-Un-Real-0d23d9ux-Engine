/**
 * UNREAL-X AI-01 · 族0005 启动日志剧场化（X00101~X00125）。
 *
 * 启动日志解析 + 剧场化渲染数据：把原始启动日志行解析成结构化条目，
 * 按链路阶段聚合成「幕」（act），产出剧场渲染数据（幕序/时长/占比）。
 * 畸形行钳制为 unknown 条目，绝不抛异常。
 */

export type LogLevel = "info" | "warn" | "error" | "unknown";

export interface BootLogEntry {
  ts: number;
  stage: string;
  level: LogLevel;
  message: string;
}

export interface BootAct {
  stage: string;
  entries: BootLogEntry[];
  fromTs: number;
  toTs: number;
  durationMs: number;
  /** 该幕占全程时间比（0~1）。 */
  share: number;
}

/** 剧场化档位：≥5 档。 */
export const THEATER_STYLES = ["minimal", "timeline", "acts", "spotlight", "cinematic"] as const;
export type TheaterStyle = (typeof THEATER_STYLES)[number];

const LEVEL_MAP: Record<string, LogLevel> = {
  i: "info",
  w: "warn",
  e: "error",
};

/** 单行解析：`<ts> <stage> <level:char> <message>`；畸形行降级为 unknown。 */
export function parseLogLine(raw: string): BootLogEntry | null {
  const line = raw.trim();
  if (!line) return null;
  const m = /^(\d+)\s+([A-Za-z0-9_-]{1,16})\s+([iwe])\s+(.*)$/.exec(line);
  if (m) {
    return {
      ts: clampInt(m[1] ?? "0", 0, 3_600_000),
      stage: m[2] ?? "unknown",
      level: LEVEL_MAP[m[3] ?? ""] ?? "unknown",
      message: (m[4] ?? "").slice(0, 160),
    };
  }
  return {
    ts: 0,
    stage: "unknown",
    level: "unknown",
    message: line.slice(0, 160),
  };
}

function clampInt(v: string, min: number, max: number): number {
  const n = Number.parseInt(v, 10);
  if (!Number.isFinite(n)) return min;
  return Math.min(max, Math.max(min, n));
}

/** 整段日志解析（跳过空行）。 */
export function parseBootLog(raw: string): BootLogEntry[] {
  return raw
    .split(/\r?\n/)
    .map((l) => parseLogLine(l))
    .filter((e): e is BootLogEntry => e !== null);
}

/**
 * 剧场化：按 stage 聚合成幕（保持首次出现顺序），时长 = 幕内最大 ts − 幕内最小 ts，
 * share = 幕时长 / 全程时长（全程为 0 时均分）。
 */
export function buildActs(entries: BootLogEntry[]): BootAct[] {
  if (entries.length === 0) return [];
  const order: string[] = [];
  const groups = new Map<string, BootLogEntry[]>();
  for (const e of entries) {
    if (!groups.has(e.stage)) {
      groups.set(e.stage, []);
      order.push(e.stage);
    }
    groups.get(e.stage)?.push(e);
  }
  const acts: BootAct[] = order.map((stage) => {
    const list = groups.get(stage) ?? [];
    const fromTs = Math.min(...list.map((e) => e.ts));
    const toTs = Math.max(...list.map((e) => e.ts));
    return { stage, entries: list, fromTs, toTs, durationMs: toTs - fromTs, share: 0 };
  });
  const total = acts.reduce((a, x) => a + x.durationMs, 0);
  if (total <= 0) {
    const even = 1 / acts.length;
    for (const a of acts) a.share = even;
  } else {
    for (const a of acts) a.share = a.durationMs / total;
  }
  return acts;
}

/** 剧场样式档解析：非法回 timeline（默认档 = 现状）。 */
export function resolveTheaterStyle(id: string): TheaterStyle {
  return (THEATER_STYLES as readonly string[]).includes(id) ? (id as TheaterStyle) : "timeline";
}

/** 渲染数据：幕标题行（供 UI 直接 join）。 */
export function actCaption(act: BootAct): string {
  const pct = Math.round(act.share * 100);
  return `第${act.stage}幕 · ${act.entries.length} 条 · ${act.durationMs}ms · ${pct}%`;
}

/** error 级条目高亮清单（聚光灯档专用）。 */
export function spotlight(entries: BootLogEntry[]): BootLogEntry[] {
  return entries.filter((e) => e.level === "error" || e.level === "warn");
}
