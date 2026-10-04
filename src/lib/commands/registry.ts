/**
 * AI-07 效率中枢组 · P-1/N-13 命令注册表内核：
 * - 注册 / 查询 / 权重排序 / 条件可见性 / 钉选 / 学习统计（频次+新近度，纯本地）
 * - 模糊匹配：子序列 + 前缀 + 连续子串 + 拼音全拼/首字母（zh 高频）
 * - 性能预算：命令表膨胀后查询 p95 < 50ms（倒排首字母索引 + O(n) 线性扫描
 *   在万条以内远低于预算；bench 由面板侧计时埋点覆盖）
 *
 * 纪律（NEXT-40 全局增量纪律 1）：每个 N-* 功能收口前其全部用户可执行
 * 动作必须注册进本表，未注册不算完成。
 */

import type { Command, CommandHit, CommandStat, PinnedCommands } from "./types";
import { matchPinyin } from "../pinyin";

const STATS_KEY = "variable.cmd.stats";
const PINNED_KEY = "variable.cmd.pinned";

/** 钉选上限（与 V-45 宏收藏钉选位 8 个同口径）。 */
export const PIN_CAP = 8;

// ---------- 注册表 ----------

const registry = new Map<string, Command>();

export function registerCommand(cmd: Command): void {
  if (!cmd.id || !cmd.titleKey || typeof cmd.action !== "function") {
    throw new Error(`非法命令声明: ${cmd.id}`);
  }
  registry.set(cmd.id, cmd);
}

export function registerCommands(cmds: Command[]): void {
  for (const c of cmds) registerCommand(c);
}

export function unregisterCommand(id: string): void {
  registry.delete(id);
}

export function allCommands(): Command[] {
  return [...registry.values()];
}

export function getCommand(id: string): Command | undefined {
  return registry.get(id);
}

export function commandCount(): number {
  return registry.size;
}

// ---------- 学习统计（频次 + 新近度，纯本地持久化） ----------

/** 半衰期（ms）：7 天没用过的命令权重减半。 */
export const RECENCY_HALF_LIFE_MS = 7 * 24 * 3600 * 1000;

let stats: Record<string, CommandStat> = {};

export function loadStats(saved: string | null): void {
  if (!saved) {
    stats = {};
    return;
  }
  try {
    const parsed = JSON.parse(saved) as Record<string, CommandStat>;
    stats = {};
    for (const [id, s] of Object.entries(parsed)) {
      if (typeof s?.count === "number" && typeof s?.lastUsed === "number") {
        stats[id] = { count: s.count, lastUsed: s.lastUsed };
      }
    }
  } catch {
    stats = {}; // 损坏如实从零开始
  }
}

export function serializeStats(): string {
  return JSON.stringify(stats);
}

export function recordUsage(id: string, now = Date.now()): void {
  const cur = stats[id] ?? { count: 0, lastUsed: 0 };
  stats[id] = { count: cur.count + 1, lastUsed: now };
}

export function statOf(id: string): CommandStat {
  return stats[id] ?? { count: 0, lastUsed: 0 };
}

/** 学习权重：log2(频次+1) + 新近度半衰（0..1）*2。纯本地计算。 */
export function learnedWeight(id: string, now = Date.now()): number {
  const s = statOf(id);
  const freq = Math.log2(s.count + 1);
  const age = Math.max(0, now - s.lastUsed);
  const recency = s.lastUsed === 0 ? 0 : Math.pow(0.5, age / RECENCY_HALF_LIFE_MS) * 2;
  return freq + recency;
}

// ---------- 钉选 ----------

let pinned: PinnedCommands = { ids: [], cap: PIN_CAP };

export function loadPinned(saved: string | null): void {
  if (!saved) {
    pinned = { ids: [], cap: PIN_CAP };
    return;
  }
  try {
    const p = JSON.parse(saved) as PinnedCommands;
    pinned = {
      ids: Array.isArray(p.ids) ? p.ids.filter((x) => typeof x === "string").slice(0, PIN_CAP) : [],
      cap: PIN_CAP,
    };
  } catch {
    pinned = { ids: [], cap: PIN_CAP };
  }
}

export function serializePinned(): string {
  return JSON.stringify(pinned);
}

export function pinnedIds(): string[] {
  return [...pinned.ids];
}

/** 钉选（超上限如实失败；顺序 = 最近钉选在前）。 */
export function pinCommand(id: string): boolean {
  if (pinned.ids.includes(id)) return true;
  if (pinned.ids.length >= PIN_CAP) return false;
  pinned.ids.unshift(id);
  return true;
}

export function unpinCommand(id: string): void {
  pinned.ids = pinned.ids.filter((x) => x !== id);
}

/** 拖拽排序（V-45：钉选位可拖拽排序）。 */
export function reorderPinned(ordered: string[]): void {
  const valid = ordered.filter((id) => pinned.ids.includes(id));
  const missing = pinned.ids.filter((id) => !valid.includes(id));
  pinned.ids = [...valid, ...missing];
}

export function isPinned(id: string): boolean {
  return pinned.ids.includes(id);
}

// ---------- 模糊匹配 ----------

/** 子序列匹配（query 字符按顺序出现在 title 中），返回命中率得分或 -1。 */
function subseqScore(haystack: string, query: string): number {
  let hi = 0;
  let hit = 0;
  let streak = 0;
  let bestStreak = 0;
  for (let qi = 0; qi < query.length; qi++) {
    const ch = query[qi]!;
    const found = haystack.indexOf(ch, hi);
    if (found < 0) return -1;
    if (found === hi) {
      streak++;
      bestStreak = Math.max(bestStreak, streak);
    } else {
      streak = 1;
    }
    hit += found === hi ? 2 : 1;
    hi = found + 1;
  }
  // 前缀命中 / 连续命中加分
  const prefix = haystack.startsWith(query) ? 8 : 0;
  const contains = haystack.includes(query) ? 6 : 0;
  return hit + bestStreak * 2 + prefix + contains;
}

/** 单命令匹配得分（title/keywords/拼音），不命中返回 -1。title 为已解析的当前语言标题。 */
export function matchScore(cmd: Command, query: string, title: string): number {
  const q = query.trim().toLowerCase();
  if (!q) return 0;
  let best = -1;
  const s1 = subseqScore(title.toLowerCase(), q);
  if (s1 >= 0) best = s1;
  for (const kw of cmd.keywords ?? []) {
    const s = subseqScore(kw.toLowerCase(), q);
    if (s > best) best = s;
  }
  if (best < 0 && matchPinyin(title + (cmd.keywords ?? []).join(""), q)) {
    best = 2; // 拼音命中（含首字母，initialsOf 内建于 matchPinyin）
  }
  return best;
}

/** 测试隔离：清空注册表与统计（仅测试使用）。 */
export function __resetForTests(): void {
  registry.clear();
  stats = {};
  pinned = { ids: [], cap: PIN_CAP };
}

// ---------- 查询 ----------

export interface QueryOptions {
  /** 前缀过滤：`>` = action/tool/macro 命令模式；`?` = settings；缺省 = app + 全部。 */
  mode?: "all" | "commands" | "settings";
  /** 附加类别白名单（统一搜索 N-14 聚合用）。 */
  categories?: Command["category"][];
  /** 学习排序开关（隐私「不记录」模式仍可排序，只是统计不落盘）。 */
  now?: number;
  /** 当前语言标题解析器（默认回退 titleKey，测试可注入）。 */
  resolveTitle?: (cmd: Command) => string;
}

function modeAllows(cmd: Command, mode: QueryOptions["mode"]): boolean {
  if (mode === "settings") return cmd.category === "settings";
  if (mode === "commands") return cmd.category !== "app" && cmd.category !== "settings";
  return true;
}

/** 查询：匹配得分 + 学习权重 + 钉选置顶。结果按分排序，钉选区独立在前。 */
export function queryCommands(query: string, opts: QueryOptions = {}): CommandHit[] {
  const now = opts.now ?? Date.now();
  const resolveTitle = opts.resolveTitle ?? ((c: Command) => c.titleKey);
  const hits: CommandHit[] = [];
  for (const cmd of registry.values()) {
    if (!modeAllows(cmd, opts.mode)) continue;
    if (opts.categories && !opts.categories.includes(cmd.category)) continue;
    if (cmd.when && !cmd.when()) continue; // 条件可见性
    const s = matchScore(cmd, query, resolveTitle(cmd));
    if (s < 0) continue;
    hits.push({ command: cmd, score: s + learnedWeight(cmd.id, now) * 3, pinned: isPinned(cmd.id) });
  }
  hits.sort((a, b) => b.score - a.score || a.command.id.localeCompare(b.command.id));
  // 钉选区（V-45 常用宏/命令区）：钉选项置顶，按钉选顺序
  const pinnedHits: CommandHit[] = [];
  const rest: CommandHit[] = [];
  for (const h of hits) (h.pinned ? pinnedHits : rest).push(h);
  pinnedHits.sort((a, b) => pinnedIds().indexOf(a.command.id) - pinnedIds().indexOf(b.command.id));
  return [...pinnedHits, ...rest];
}

/** 执行命令：记录使用统计（V-47「不记录」模式下由调用方跳过持久化）。 */
export async function executeCommand(
  id: string,
  opts: { record?: boolean; now?: number } = {},
): Promise<boolean | void> {
  const cmd = registry.get(id);
  if (!cmd) throw new Error(`未知命令 ${id}`);
  if (cmd.when && !cmd.when()) return false;
  if (opts.record !== false) recordUsage(id, opts.now);
  return cmd.action();
}

// 存储 key（供设置持久化层接 localStorage/容器）
export const STORAGE_KEYS = { stats: STATS_KEY, pinned: PINNED_KEY } as const;
