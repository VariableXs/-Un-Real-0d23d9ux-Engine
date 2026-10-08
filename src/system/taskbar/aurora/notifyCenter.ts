/**
 * AURORA-10000 领域04 · 族0088 通知中心结构 + 族0089 通知行为（AI-18 批次，勿删）。
 * 通知模型：分组/置顶/稍后提醒/归档/批量清除/规则引擎 + 横幅行为/勿扰联动/徽标/晨报。
 */

export interface Notice {
  id: string;
  appId: string;
  title: string;
  body: string;
  ts: number;
  kind: "info" | "progress" | "media" | "reply" | "actions";
  /** 重要（置顶 F02181）。 */
  important: boolean;
  /** 免打扰集归集（F02180）。 */
  muted?: boolean;
  actions?: readonly { id: string; label: string }[];
  progress?: number;
  archived?: boolean;
  snoozeUntil?: number;
}

export interface NotifyBehaviorOpts {
  /** 勿扰计划（F02206）："22:00-07:00"。 */
  dndPlan: string;
  /** 专注联动（F02207）。 */
  focusMode: boolean;
  /** 全屏静默（F02208）。 */
  fullscreen: boolean;
  /** 投屏静默（F02209）。 */
  projecting: boolean;
  /** 白名单（F02211）。 */
  whitelist: readonly string[];
  /** 关键词提升（F02212）。 */
  keywords: readonly string[];
}

/** 今日/更早分区（F02179）。 */
export function partitionToday(notices: readonly Notice[], now = Date.now()): { today: Notice[]; earlier: Notice[] } {
  const start = new Date(now); start.setHours(0, 0, 0, 0);
  const today: Notice[] = []; const earlier: Notice[] = [];
  for (const n of notices) (n.ts >= start.getTime() ? today : earlier).push(n);
  return { today, earlier };
}

/** 展示排序：重要置顶 → 时间倒序；稍后提醒未到期的不出现（F02181/82）。 */
export function displayOrder(notices: readonly Notice[], now = Date.now()): Notice[] {
  return notices
    .filter((n) => !n.snoozeUntil || n.snoozeUntil <= now)
    .sort((a, b) => (a.important === b.important ? b.ts - a.ts : a.important ? -1 : 1));
}

/** 批量已读/清除 + 清除前自动归档（F02184/85/220）。 */
export function clearAll(notices: readonly Notice[], autoArchive: boolean): Notice[] {
  return autoArchive ? notices.map((n) => ({ ...n, archived: true })) : [];
}

/** 找回误删（F02221）：从归档恢复。 */
export function restoreArchived(notices: readonly Notice[]): Notice[] {
  return notices.map((n) => ({ ...n, archived: false }));
}

/** 规则引擎（F02199）：每应用静音 + 关键词提升。 */
export function applyRules(n: Notice, opts: NotifyBehaviorOpts): Notice {
  let next = { ...n };
  if (opts.whitelist.includes(n.appId)) return { ...next, muted: false, important: true };
  if (opts.keywords.some((k) => n.title.includes(k) || n.body.includes(k))) next.important = true;
  if (opts.focusMode || opts.fullscreen || opts.projecting) next.muted = true;
  return next;
}

/** 勿扰时段判定（F02206），格式同组件区。 */
export function inDndPlan(plan: string, now = new Date()): boolean {
  const m = /^(\d{1,2}):(\d{2})-(\d{1,2}):(\d{2})$/.exec(plan.trim());
  if (!m || m[1] == null || m[2] == null || m[3] == null || m[4] == null) return false;
  const mins = now.getHours() * 60 + now.getMinutes();
  const a = parseInt(m[1], 10) * 60 + parseInt(m[2], 10);
  const b = parseInt(m[3], 10) * 60 + parseInt(m[4], 10);
  return a <= b ? mins >= a && mins < b : mins >= a || mins < b;
}

/** 横幅决策（F02201~F02204）：位置/时长/堆叠。 */
export function bannerPlan(count: number, corner: "tl" | "tr" | "bl" | "br" = "br", durationMs = 5000): { corner: "tl" | "tr" | "bl" | "br"; durationMs: number; stack: number } {
  return { corner, durationMs, stack: Math.min(3, Math.max(1, count)) };
}

/** 晨报（F02222）：把夜间静默通知汇总为一条摘要。 */
export function morningDigest(notices: readonly Notice[], now = Date.now()): Notice | null {
  const start = new Date(now); start.setHours(0, 0, 0, 0);
  const night = notices.filter((n) => n.ts < start.getTime() && n.ts > start.getTime() - 8 * 3600_000);
  if (night.length === 0) return null;
  return {
    id: "digest-" + start.getTime(),
    appId: "system",
    title: `夜间汇总 · ${night.length} 条通知`,
    body: night.slice(0, 5).map((n) => n.title).join("、") + (night.length > 5 ? " 等" : ""),
    ts: now,
    kind: "info",
    important: false,
  };
}

/** 打扰热度（F02296 近似 F02196 热度图）：按天计数。 */
export function heatmap(notices: readonly Notice[]): Map<string, number> {
  const m = new Map<string, number>();
  for (const n of notices) {
    const d = new Date(n.ts);
    const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    m.set(key, (m.get(key) ?? 0) + 1);
  }
  return m;
}
