/**
 * AI-13 Z-62 本地统计面板（Local Usage Stats）
 * 红线：零外发（只写 localStorage）；一键清除必须彻底。
 */

export interface DayBucket {
  /** YYYY-MM-DD */
  day: string;
  /** action/appKey → 次数 */
  counts: Record<string, number>;
}

export interface UsageStatsState {
  /** 键位使用 Top 序列（滚动保留 90 天） */
  days: DayBucket[];
  /** 应用时长（秒）累计：appKey → seconds */
  appSeconds: Record<string, number>;
}

const KEY = "vxs:usageStats";
const KEEP_DAYS = 90;
let cache: UsageStatsState | null = null;

export function dayKey(d = new Date()): string {
  const p = (n: number): string => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function loadStats(): UsageStatsState {
  if (cache) return cache;
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as UsageStatsState;
      if (Array.isArray(parsed.days) && typeof parsed.appSeconds === "object") {
        cache = parsed;
        return cache;
      }
    }
  } catch { /* 损坏即重置（Z-62 清除语义） */ }
  cache = { days: [], appSeconds: {} };
  return cache;
}

function persist(): void {
  if (!cache) return;
  try {
    localStorage.setItem(KEY, JSON.stringify(cache));
  } catch { /* 配额满静默丢弃（统计非关键数据） */ }
}

/** 记一次动作（键位/命令等）。 */
export function recordAction(action: string, n = 1): void {
  const st = loadStats();
  let today = st.days[st.days.length - 1];
  const key = dayKey();
  if (!today || today.day !== key) {
    today = { day: key, counts: {} };
    st.days.push(today);
    gc(st);
  }
  today.counts[action] = (today.counts[action] ?? 0) + n;
  persist();
}

/** 累计应用时长（秒）。 */
export function recordAppTime(appKey: string, seconds: number): void {
  const st = loadStats();
  st.appSeconds[appKey] = (st.appSeconds[appKey] ?? 0) + seconds;
  persist();
}

function gc(st: UsageStatsState): void {
  while (st.days.length > KEEP_DAYS) st.days.shift();
}

/** 键位使用 Top N（默认 20，跨天聚合）。 */
export function topActions(n = 20): { action: string; count: number }[] {
  const agg = new Map<string, number>();
  for (const d of loadStats().days) {
    for (const [k, v] of Object.entries(d.counts)) {
      agg.set(k, (agg.get(k) ?? 0) + v);
    }
  }
  return [...agg.entries()]
    .map(([action, count]) => ({ action, count }))
    .sort((a, b) => b.count - a.count)
    .slice(0, n);
}

/** 最近 n 天的日总量（周/月图表用）。 */
export function dailyTotals(n = 7): { day: string; total: number }[] {
  const st = loadStats();
  return st.days.slice(-n).map((d) => ({
    day: d.day,
    total: Object.values(d.counts).reduce((a, b) => a + b, 0),
  }));
}

/** 一键清除（不可恢复）。 */
export function clearStats(): void {
  cache = { days: [], appSeconds: {} };
  try { localStorage.removeItem(KEY); } catch { /* ignore */ }
}

/** 测试隔离。 */
export function resetStatsForTest(): void {
  cache = null;
}
