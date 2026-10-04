/**
 * J 鼠标域 · 会话聚合与日报引擎（v5 · 深化批次五 · 章十三体验日志）。
 *
 * v4 的遥测是「事件环 + 回放」——逐事件可看，但没有「这段时间用得怎么样」
 * 的聚合视图。本模块把事件流升维成会话与日报：
 * - 会话分桶：间隔 >30min 切会话（一顿饭的功夫就算另一段使用）；
 * - 每会话：事件数、结论分布（顺畅/卡顿/无反馈/被打断/报错）、
 *   交互耗时 P50/P95/P99、挫败信号计数与排行；
 * - 挫败热区排行：3×3 象限聚合计数——「哪里最挫败」一列数字；
 * - 日报：纯 Markdown 字符串（粘贴进文档/导出均可——开放格式纪律）。
 *
 * 隐私红线与遥测层同规：只聚合粗粒度（结论/象限/耗时），无文本内容。
 */

import type { FrustrationSignal, J1Event } from "./telemetry";

export interface J1Session {
  /** 会话起始/结束（ms）。 */
  from: number;
  to: number;
  events: J1Event[];
  frustrations: FrustrationSignal[];
}

/** 会话分桶：事件时间排序后按间隔切（gapMs 默认 30 分钟）。 */
export function bucketSessions(events: J1Event[], frustrations: FrustrationSignal[], gapMs = 30 * 60 * 1000): J1Session[] {
  if (events.length === 0) return [];
  const sorted = [...events].sort((a, b) => a.at - b.at);
  const sessions: J1Session[] = [];
  let cur: J1Event[] = [sorted[0]!];
  for (let i = 1; i < sorted.length; i++) {
    if (sorted[i]!.at - cur[cur.length - 1]!.at > gapMs) {
      sessions.push(toSession(cur, frustrations));
      cur = [];
    }
    cur.push(sorted[i]!);
  }
  sessions.push(toSession(cur, frustrations));
  return sessions;
}

function toSession(events: J1Event[], frustrations: FrustrationSignal[]): J1Session {
  const from = events[0]!.at;
  const to = events[events.length - 1]!.at;
  return {
    from,
    to,
    events,
    frustrations: frustrations.filter((f) => f.at >= from && f.at <= to),
  };
}

/** 分位数（线性插值法——P95 不是「取第 95 个」的粗糙口径）。 */
export function percentile(sortedMs: number[], p: number): number {
  if (sortedMs.length === 0) return 0;
  if (sortedMs.length === 1) return sortedMs[0]!;
  const idx = (p / 100) * (sortedMs.length - 1);
  const lo = Math.floor(idx);
  const hi = Math.ceil(idx);
  return sortedMs[lo]! + (sortedMs[hi]! - sortedMs[lo]!) * (idx - lo);
}

export interface SessionStats {
  events: number;
  verdicts: Record<J1Event["verdict"], number>;
  latency: { p50: number; p95: number; p99: number };
  frustrationCount: number;
  /** 挫败热区排行（象限→计数，降序）。 */
  hotZones: { zone: number; count: number }[];
  /** 顺畅率（smooth / 总数）。 */
  smoothRate: number;
}

/** 单会话统计（面板与会话日报共用——一处一事实）。 */
export function sessionStats(s: J1Session): SessionStats {
  const verdicts: Record<J1Event["verdict"], number> = { smooth: 0, laggy: 0, "no-feedback": 0, interrupted: 0, error: 0 };
  for (const e of s.events) verdicts[e.verdict]++;
  const durs = s.events.filter((e) => e.durMs > 0).map((e) => e.durMs).sort((a, b) => a - b);
  const zoneCount = new Map<number, number>();
  for (const f of s.frustrations) zoneCount.set(f.zone, (zoneCount.get(f.zone) ?? 0) + 1);
  const hotZones = [...zoneCount.entries()].map(([zone, count]) => ({ zone, count })).sort((a, b) => b.count - a.count);
  return {
    events: s.events.length,
    verdicts,
    latency: {
      p50: Math.round(percentile(durs, 50)),
      p95: Math.round(percentile(durs, 95)),
      p99: Math.round(percentile(durs, 99)),
    },
    frustrationCount: s.frustrations.length,
    hotZones,
    smoothRate: s.events.length === 0 ? 0 : Math.round((verdicts.smooth / s.events.length) * 1000) / 1000,
  };
}

/**
 * 最挫败的 N 个会话（章十三「最挫败的十次操作」维度）：挫败密度 =
 * 挫败信号数/事件数——小会话里一记 rage-click 比大会话的三记更刺眼。
 */
export function worstSessions(sessions: J1Session[], n = 5): { session: J1Session; stats: SessionStats; density: number }[] {
  return sessions
    .map((session) => {
      const stats = sessionStats(session);
      return { session, stats, density: session.events.length === 0 ? 0 : Math.round((stats.frustrationCount / session.events.length) * 1000) / 1000 };
    })
    .filter((r) => r.stats.frustrationCount > 0)
    .sort((a, b) => b.density - a.density)
    .slice(0, n);
}

/* ------------------------------- 日报 ------------------------------- */

function fmtClock(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/**
 * 日报（Markdown）：会话逐段统计 + 全天挫败排行 + 结论行。
 * 开放格式（十四章）：字符串即交付，粘进任何文档都能用。
 */
export function dailyReport(sessions: J1Session[], dateLabel: string): string {
  const lines: string[] = [];
  lines.push(`# J 域体验日报 · ${dateLabel}`);
  if (sessions.length === 0) {
    lines.push("", "（无交互记录——这一天鼠标域零事件，静默也是诚实的结论。）");
    return lines.join("\n");
  }
  const total = sessions.reduce((acc, s) => acc + s.events.length, 0);
  const totalFru = sessions.reduce((acc, s) => acc + s.frustrations.length, 0);
  const agg = new Map<J1Event["verdict"], number>();
  for (const s of sessions) {
    const st = sessionStats(s);
    for (const [v, c] of Object.entries(st.verdicts)) agg.set(v as J1Event["verdict"], (agg.get(v as J1Event["verdict"]) ?? 0) + c);
  }
  lines.push(
    "",
    `**总计**：${sessions.length} 段会话 · ${total} 次交互 · ${totalFru} 条挫败信号`,
    "",
    `| 结论 | 次数 |`,
    `| --- | --- |`,
  );
  for (const [v, c] of agg) lines.push(`| ${v} | ${c} |`);
  lines.push("", "## 会话明细", "", "| 时段 | 事件 | 顺畅率 | P50/P95/P99(ms) | 挫败 |", "| --- | --- | --- | --- | --- |");
  for (const s of sessions) {
    const st = sessionStats(s);
    lines.push(`| ${fmtClock(s.from)}–${fmtClock(s.to)} | ${st.events} | ${(st.smoothRate * 100).toFixed(0)}% | ${st.latency.p50}/${st.latency.p95}/${st.latency.p99} | ${st.frustrationCount} |`);
  }
  const worst = worstSessions(sessions);
  if (worst.length > 0) {
    lines.push("", "## 最挫败的会话（挫败密度降序）");
    for (const w of worst) {
      const hot = w.stats.hotZones[0];
      lines.push(`- ${fmtClock(w.session.from)} 密度 ${w.density}${hot ? ` · 最热区:象限${hot.zone}(${hot.count}次)` : ""}`);
    }
  }
  lines.push("", "> 隐私口径：本日报只含结论/象限/耗时聚合，无任何输入内容（章十三红线）。");
  return lines.join("\n");
}
