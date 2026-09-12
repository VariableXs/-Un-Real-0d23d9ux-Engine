/**
 * AURORA-10000 领域04 · 族0086 全局搜索中枢 + 族0087 快速启动器（AI-18 批次，勿删）。
 * 统一索引 + 过滤器 + 运算符/正则/通配 + 启动器工具集（编解码/JSON/正则/密码/骰子/倒计时）。
 */

/** 统一索引条目（F02127~F02135；F02136 邮件位预留）。 */
export interface IndexEntry {
  id: string;
  source: "file" | "doc" | "window" | "setting" | "command" | "clipboard" | "bookmark" | "note" | "mail";
  title: string;
  body?: string;
  path?: string;
  ext?: string;
  mtime?: number;
  size?: number;
}

export interface SearchFilters {
  /** 路径前缀过滤（F02139）。 */
  path?: string;
  /** 类型过滤（F02140）。 */
  exts?: readonly string[];
  /** 时间过滤：mtime >=（F02141）。 */
  since?: number;
  /** 大小过滤：size <=（F02142）。 */
  maxSize?: number;
  /** 正则模式（F02147，默认关）。 */
  regex?: boolean;
  /** 通配符（F02138）：* ? 转 regex。 */
  wildcard?: boolean;
}

/** 运算符语法（F02146）：AND/OR/NOT。 */
export function applyOperators(query: string): { terms: string[]; or: boolean; not: string[] } {
  const not: string[] = [];
  const terms: string[] = [];
  const or = /\bOR\b/.test(query);
  for (const tok of query.split(/\s+/)) {
    const t = tok.trim();
    if (!t) continue;
    if (t.startsWith("NOT:")) not.push(t.slice(4).toLowerCase());
    else if (t !== "AND" && t !== "OR") terms.push(t.toLowerCase());
  }
  return { terms, or, not };
}

function toRegex(pattern: string, flags = "i"): RegExp | null {
  try { return new RegExp(pattern, flags); } catch { return null; }
}

/** 统一搜索（F02126~F02150 的模型核）。 */
export function searchAll(entries: readonly IndexEntry[], query: string, filters: SearchFilters = {}): IndexEntry[] {
  if (!query.trim()) return [];
  let matcher: (e: IndexEntry) => boolean;
  const { terms, or, not } = applyOperators(query);
  if (filters.regex) {
    const re = toRegex(query);
    if (!re) return [];
    matcher = (e) => re.test(e.title) || re.test(e.body ?? "");
  } else if (filters.wildcard) {
    const re = toRegex(terms.map((t) => t.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".")).join("|"));
    if (!re) return [];
    matcher = (e) => re.test(e.title) || re.test(e.body ?? "");
  } else {
    matcher = (e) => {
      const hay = `${e.title}\n${e.body ?? ""}`.toLowerCase();
      return or ? terms.some((t) => hay.includes(t)) : terms.every((t) => hay.includes(t));
    };
  }
  const q = query.toLowerCase();
  return entries
    .filter((e) => {
      if (filters.path && !(e.path ?? "").startsWith(filters.path)) return false;
      if (filters.exts?.length && (!e.ext || !filters.exts.includes(e.ext))) return false;
      if (filters.since != null && (e.mtime ?? 0) < filters.since) return false;
      if (filters.maxSize != null && (e.size ?? 0) > filters.maxSize) return false;
      if (not.some((n) => hayMatch(e, n))) return false;
      return matcher(e);
    })
    .sort((a, b) => scoreEntry(b, q) - scoreEntry(a, q));
}

function hayMatch(e: IndexEntry, needle: string): boolean {
  return `${e.title}\n${e.body ?? ""}`.toLowerCase().includes(needle);
}
function scoreEntry(e: IndexEntry, q: string): number {
  const t = e.title.toLowerCase();
  if (t === q) return 100;
  if (t.startsWith(q)) return 80;
  return t.includes(q) ? 60 : 20;
}

/* ---------------- 族0087 快速启动器工具 ---------------- */

/** Base64/URL 编解码（F02165）。 */
export function codecEncode(s: string, mode: "base64" | "url"): string {
  if (mode === "url") return encodeURIComponent(s);
  try { return btoa(unescape(encodeURIComponent(s))); } catch { return ""; }
}
export function codecDecode(s: string, mode: "base64" | "url"): string {
  if (mode === "url") { try { return decodeURIComponent(s); } catch { return s; } }
  try { return decodeURIComponent(escape(atob(s))); } catch { return ""; }
}

/** JSON 格式化（F02166）。 */
export function jsonFormat(s: string): string | null {
  try { return JSON.stringify(JSON.parse(s), null, 2); } catch { return null; }
}

/** 正则即时测试（F02167）。 */
export function regexTest(pattern: string, text: string): { ok: boolean; matches: string[] } {
  const re = toRegex(pattern, "g");
  if (!re) return { ok: false, matches: [] };
  const matches = [...text.matchAll(re)].map((m) => m[0]).slice(0, 50);
  return { ok: true, matches };
}

/** 随机强密码（F02169）。 */
export function genPassword(len = 16): string {
  const chars = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789!@#$%^&*";
  const out: string[] = [];
  for (let i = 0; i < Math.max(8, Math.min(64, len)); i++) {
    out.push(chars.charAt(Math.floor(Math.random() * chars.length)));
  }
  return out.join("");
}

/** 掷骰子（F02171）：nNdM。 */
export function rollDice(spec: string): number[] | null {
  const m = /^(\d{0,2})d(\d{1,3})$/i.exec(spec.trim());
  if (!m) return null;
  const n = Math.min(20, parseInt(m[1] ?? "1", 10) || 1);
  const sides = Math.max(2, parseInt(m[2] ?? "6", 10));
  return Array.from({ length: n }, () => 1 + Math.floor(Math.random() * sides));
}

/** 命令别名表（F02156）。 */
const aliases = new Map<string, string>();
export function setAlias(alias: string, target: string): void { aliases.set(alias, target); }
export function resolveAlias(input: string): string | undefined {
  const parts = input.trim().split(/\s+/);
  const head = parts[0] ?? "";
  const rest = parts.slice(1);
  const target = aliases.get(head);
  return target ? [target, ...rest].join(" ") : undefined;
}
