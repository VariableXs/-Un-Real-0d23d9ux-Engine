/**
 * AI-10 文件管理器 2.0 纯逻辑（V-31~V-40 / U-16）：
 * - V-31 视图记忆映射（后端枚举 icon|list|column ↔ 前端 list|icons|thumbs|column）
 * - V-32 过滤高亮分段
 * - V-36 特殊名/长路径防呆检测
 * - V-37 按类型选取
 * - V-39 地址栏模糊跳转评分
 * - U-16 分页虚拟化切片
 */

export type ExViewMode = "list" | "icons" | "thumbs" | "column";

/** 后端持久化枚举。 */
export type SavedView = "icon" | "list" | "column";

/** 后端保存值 → 前端视图（未知值返回 null，维持当前视图）。 */
export function fromSavedView(v: string): ExViewMode | null {
  if (v === "icon") return "icons";
  if (v === "list") return "list";
  if (v === "column") return "column";
  return null;
}

/** 前端视图 → 后端保存值（thumbs 无后端枚举，退化为 icon）。 */
export function toSavedView(v: ExViewMode): SavedView {
  return v === "column" ? "column" : v === "list" ? "list" : "icon";
}

/** V-32：过滤高亮。返回 [前, 命中, 后]；未命中返回 null。大小写不敏感，命中第一处。 */
export function highlightParts(
  name: string,
  q: string,
): [string, string, string] | null {
  const query = q.trim().toLowerCase();
  if (!query) return null;
  const idx = name.toLowerCase().indexOf(query);
  if (idx < 0) return null;
  return [name.slice(0, idx), name.slice(idx, idx + query.length), name.slice(idx + query.length)];
}

const INVALID_CHARS = /[<>:"/\\|?*]/g;
const RESERVED = new Set([
  "CON", "PRN", "AUX", "NUL",
  ...(Array.from({ length: 9 }, (_, i) => `COM${i + 1}`)),
  ...(Array.from({ length: 9 }, (_, i) => `LPT${i + 1}`)),
]);

/**
 * V-36：新建/重命名防呆检测。返回问题列表（空 = 通过）。
 * - 非法字符（Windows 保留：<>:"/\|?*）
 * - 系统保留设备名（CON / PRN / COM1…）
 * - 结尾点/空格（Windows 会静默剥离，导致"看起来改名成功"）
 * - 名称过长（>200 字符，叠加父路径易超 MAX_PATH）
 */
export function nameIssues(name: string): string[] {
  const issues: string[] = [];
  if (!name) {
    issues.push("empty");
    return issues;
  }
  const invalid = [...name.matchAll(INVALID_CHARS)].map((m) => m[0]);
  if (invalid.length > 0) issues.push(`invalid:${[...new Set(invalid)].join("")}`);
  const stem = name.replace(/\..*$/, "").toUpperCase();
  if (RESERVED.has(stem)) issues.push("reserved");
  if (/[. ]$/.test(name)) issues.push("trailing");
  if (name.length > 200) issues.push("too-long");
  return issues;
}

/** V-37：与基准条目同类型（目录 ↔ 目录；文件按扩展名分组，无扩展名单独一组）。 */
export function sameTypeGroup<T extends { kind: string; ext?: string | null }>(base: T, other: T): boolean {
  if (base.kind !== other.kind) return false;
  if (base.kind === "dir") return true;
  return (base.ext ?? "").toLowerCase() === (other.ext ?? "").toLowerCase();
}

/**
 * V-39：模糊子序列评分。query 的字符按顺序出现在 target 中即命中。
 * 返回分数（越小越优）：连续命中 + 间距小 + 首字母命中加分。未命中返回 null。
 */
export function fuzzyScore(query: string, target: string): number | null {
  const q = query.toLowerCase();
  const t = target.toLowerCase();
  if (!q) return 0;
  let score = 0;
  let ti = 0;
  let last = -1;
  for (const ch of q) {
    const idx = t.indexOf(ch, ti);
    if (idx < 0) return null;
    score += idx - ti; // 间距惩罚
    if (idx === last + 1 && last >= 0) score -= 2; // 连续奖励
    if (idx === 0 || /\W/.test(t[idx - 1] ?? "")) score -= 3; // 词首奖励
    last = idx;
    ti = idx + 1;
  }
  return score;
}

/** V-39：候选池模糊匹配 → 按分数升序取前 limit 个（去重）。 */
export function fuzzySuggestions(
  query: string,
  pool: string[],
  limit = 8,
): string[] {
  const q = query.trim();
  if (!q) return [];
  const seen = new Set<string>();
  const scored: { p: string; s: number }[] = [];
  for (const p of pool) {
    if (seen.has(p)) continue;
    seen.add(p);
    const tail = p.split("\\").pop() ?? p;
    const sPath = fuzzyScore(q, p);
    const sTail = fuzzyScore(q, tail);
    // 两路都未命中才排除；命中取更优（尾段命中减 1 让位于词首的路径优先）
    if (sPath === null && sTail === null) continue;
    scored.push({ p, s: Math.min(sPath ?? Number.MAX_SAFE_INTEGER, (sTail ?? Number.MAX_SAFE_INTEGER) - 1) });
  }
  return scored
    .sort((a, b) => a.s - b.s)
    .slice(0, limit)
    .map((x) => x.p);
}

/** U-16：分页虚拟化切片（page 从 0 起）。 */
export function pageSlice<T>(items: T[], page: number, size: number): { rows: T[]; pages: number; page: number } {
  const pages = Math.max(1, Math.ceil(items.length / size));
  const p = Math.min(Math.max(0, page), pages - 1);
  return { rows: items.slice(p * size, (p + 1) * size), pages, page: p };
}
