/**
 * N-32 智能通知整理（本地学习，可单测的纯逻辑 + localStorage 持久化）。
 *
 * - 学习信号：来源 × 用户动作（opened 点开 / dismissed 划掉）频次表，纯本地统计；
 * - 三堆策略：instant（高互动来源，正常弹出）/ digest（低互动，收进定点摘要）/
 *   silent（反复划掉不点开的来源，建议静默）；
 * - 透明性：learnedStats 输出每来源学习结论与依据，用户可覆盖（override 表）；
 * - 建议权在机器，决定权在用户。
 */

const STORAGE_KEY = "variable.notifyLearn.v1";
const MAX_DAYS = 14; // 滚动窗口（学习数据保留 14 天）

export interface SourceLearn {
  opened: number;
  dismissed: number;
  lastAt: number;
}

export interface LearnTable {
  /** source → 计数。 */
  sources: Record<string, SourceLearn>;
  /** 用户覆盖：source → "instant" | "digest" | "silent"（优先于学习结论）。 */
  overrides: Record<string, NotifyStack>;
}

export type NotifyStack = "instant" | "digest" | "silent";

export interface LearnedConclusion {
  source: string;
  stack: NotifyStack;
  /** 依据（透明性）：如「你 14 天内划掉了 32 条、点开 0 条」。 */
  basis: string;
  overridden: boolean;
}

export const EMPTY_LEARN: LearnTable = { sources: {}, overrides: {} };

/** 冷启动阈值：数据不足 totalMin 条时不分层（默认全即时堆）。 */
export const COLD_START_MIN = 8;
/** 静默判定：划掉 ≥ SILENT_DISMISS 且点开为 0。 */
export const SILENT_DISMISS = 10;
/** 即时堆判定：点开率 ≥ INSTANT_RATIO。 */
export const INSTANT_RATIO = 0.3;

export function recordAction(
  table: LearnTable,
  source: string,
  action: "opened" | "dismissed",
  now = Date.now(),
): LearnTable {
  const key = source.trim();
  if (!key) return table;
  const prev = table.sources[key] ?? { opened: 0, dismissed: 0, lastAt: 0 };
  const next: SourceLearn = {
    opened: prev.opened + (action === "opened" ? 1 : 0),
    dismissed: prev.dismissed + (action === "dismissed" ? 1 : 0),
    lastAt: now,
  };
  return { ...table, sources: { ...table.sources, [key]: next } };
}

/** 单来源分堆结论（纯函数）。 */
export function classifySource(_source: string, learn: SourceLearn | undefined, override?: NotifyStack): NotifyStack {
  if (override) return override;
  if (!learn) return "instant"; // 无学习数据 = 默认即时（冷启动）
  const total = learn.opened + learn.dismissed;
  if (total < COLD_START_MIN) return "instant";
  if (learn.dismissed >= SILENT_DISMISS && learn.opened === 0) return "silent";
  if (learn.opened / total >= INSTANT_RATIO) return "instant";
  return "digest";
}

/** 全来源学习结论（透明性：含依据文本与覆盖状态）。 */
export function learnedStats(table: LearnTable): LearnedConclusion[] {
  const out: LearnedConclusion[] = [];
  for (const [source, learn] of Object.entries(table.sources)) {
    const stack = classifySource(source, learn, table.overrides[source]);
    const total = learn.opened + learn.dismissed;
    const overridden = table.overrides[source] !== undefined;
    out.push({
      source,
      stack,
      basis: `你 ${MAX_DAYS} 天内点开 ${learn.opened} 条、划掉 ${learn.dismissed} 条（共 ${total} 条）`,
      overridden,
    });
  }
  return out.sort((a, b) => a.source.localeCompare(b.source));
}

export function setOverride(table: LearnTable, source: string, stack: NotifyStack | null): LearnTable {
  const overrides = { ...table.overrides };
  if (stack === null) delete overrides[source];
  else overrides[source] = stack;
  return { ...table, overrides };
}

// ---- localStorage 持久化（best-effort，失败静默回内存） ----

export function loadLearn(): LearnTable {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return structuredClone(EMPTY_LEARN);
    const parsed = JSON.parse(raw) as LearnTable;
    if (!parsed || typeof parsed !== "object" || typeof parsed.sources !== "object") {
      return structuredClone(EMPTY_LEARN);
    }
    return { sources: parsed.sources ?? {}, overrides: parsed.overrides ?? {} };
  } catch {
    return structuredClone(EMPTY_LEARN);
  }
}

export function saveLearn(table: LearnTable): void {
  try {
    // 滚动窗口：只保留 14 天内有互动的来源
    const cutoff = Date.now() - MAX_DAYS * 86_400_000;
    const sources: Record<string, SourceLearn> = {};
    for (const [k, v] of Object.entries(table.sources)) {
      if (v.lastAt >= cutoff) sources[k] = v;
    }
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ sources, overrides: table.overrides }));
  } catch {
    /* 配额满等：静默 */
  }
}

/**
 * 摘要堆构建：把 digest 堆的通知按来源聚合成定点摘要卡片
 * （「过去 N 小时 M 条通知：A×5 B×4…」，可展开逐条）。
 */
export interface DigestGroup {
  source: string;
  count: number;
  titles: string[];
}

export function buildDigests(
  items: { source: string; title: string; ts: number }[],
  stackOf: (source: string) => NotifyStack,
  sinceMs: number,
): DigestGroup[] {
  const recent = items.filter((it) => it.ts >= sinceMs);
  const map = new Map<string, DigestGroup>();
  for (const it of recent) {
    if (stackOf(it.source) !== "digest") continue;
    const g = map.get(it.source) ?? { source: it.source, count: 0, titles: [] };
    g.count += 1;
    if (g.titles.length < 3) g.titles.push(it.title); // 摘要行最多预览 3 条
    map.set(it.source, g);
  }
  return [...map.values()].sort((a, b) => b.count - a.count);
}
