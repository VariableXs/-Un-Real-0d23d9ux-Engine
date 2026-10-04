/**
 * AI-07 · N-14 统一搜索 2.0（聚合层 + 高级语法解析，纯前端可测）：
 * - 五类聚合：应用/文件/内容/命令/设置——各类独立权重、类别 Tab 可锁
 * - 高级语法：ext:md size:>10m modified:week 过滤子句（帮助面板内建）
 * - 隐私：排除目录黑名单 + 保险箱路径硬编码永不入索引（零泄漏红线）
 * - V-47 联动：「不记录」模式零持久化（由 SearchHistory 管道保证）
 * 数据源（文件索引 fsindex.rs / 记录库全文 / 命令注册表）由调用方注入。
 */

export type ResultCategory = "app" | "file" | "content" | "command" | "settings";

export interface UnifiedHit {
  category: ResultCategory;
  title: string;
  subtitle?: string;
  /** 行内快捷动作（文件类三键：打开/定位/复制路径）。 */
  quickActions?: ("open" | "reveal" | "copyPath")[];
  score: number;
}

/** 保险箱路径硬编码排除（零泄漏红线——不可配置放宽）。 */
export const SAFEBOX_EXCLUDES: readonly string[] = ["data/safe/", "data/vault/", "保险箱/"];

export function isExcludedPath(path: string): boolean {
  const norm = path.replace(/\\/g, "/").toLowerCase();
  return SAFEBOX_EXCLUDES.some((p) => norm.includes(p));
}

// ---------- 高级语法 ----------

export interface SearchFilters {
  text: string;
  ext?: string;
  /** 字节；">10m" / "<5k" / "1024" 三形。 */
  sizeMin?: number;
  sizeMax?: number;
  /** modified:week → 7 天内；day/week/month。 */
  modifiedWithin?: "day" | "week" | "month";
}

const SIZE_UNITS: Record<string, number> = { b: 1, k: 1024, m: 1024 ** 2, g: 1024 ** 3 };
const MODIFIED: Record<string, SearchFilters["modifiedWithin"]> = { day: "day", week: "week", month: "month" };

/** 解析查询：`报告 ext:md size:>10m modified:week` → 文本 + 过滤子句。 */
export function parseQuery(raw: string): SearchFilters {
  const f: SearchFilters = { text: "" };
  const words: string[] = [];
  for (const token of raw.split(/\s+/)) {
    const ext = token.match(/^ext:([a-z0-9]+)$/i);
    if (ext) {
      f.ext = ext[1]!.toLowerCase();
      continue;
    }
    const size = token.match(/^size:([<>]?)(\d+(?:\.\d+)?)([bkmgi]?)/i);
    if (size) {
      const unit = SIZE_UNITS[(size[3] || "b").toLowerCase()] ?? 1;
      const v = Math.round(Number(size[2]) * unit);
      if (size[1] === ">") f.sizeMin = v;
      else if (size[1] === "<") f.sizeMax = v;
      else {
        f.sizeMin = v;
        f.sizeMax = v;
      }
      continue;
    }
    const mod = token.match(/^modified:(day|week|month)$/i);
    if (mod) {
      f.modifiedWithin = MODIFIED[mod[1]!.toLowerCase()]!;
      continue;
    }
    words.push(token);
  }
  f.text = words.join(" ");
  return f;
}

/** 文件元数据（过滤执行输入）。 */
export interface FileMeta {
  path: string;
  title: string;
  sizeBytes: number;
  modifiedAt: number; // epoch ms
}

/** 过滤器套用（ext/size/modified + 保险箱排除）。 */
export function applyFileFilters(files: FileMeta[], f: SearchFilters, now = Date.now()): FileMeta[] {
  const windowsMs: Record<string, number> = { day: 86400000, week: 7 * 86400000, month: 30 * 86400000 };
  return files.filter((file) => {
    if (isExcludedPath(file.path)) return false;
    if (f.ext) {
      const ext = file.path.split(".").pop()?.toLowerCase() ?? "";
      if (ext !== f.ext) return false;
    }
    if (f.sizeMin !== undefined && file.sizeBytes < f.sizeMin) return false;
    if (f.sizeMax !== undefined && file.sizeBytes > f.sizeMax) return false;
    if (f.modifiedWithin && now - file.modifiedAt > windowsMs[f.modifiedWithin]!) return false;
    return true;
  });
}

// ---------- 五类聚合 ----------

export interface AggregateInput {
  apps?: { title: string; score?: number }[];
  files?: FileMeta[];
  contents?: { title: string; folderPath: string; score?: number }[];
  commands?: { id: string; title: string; score?: number }[];
  settings?: { title: string; score?: number }[];
}

/** 类别权重（应用 > 文件 > 命令 > 内容 > 设置；Tab 锁定由 UI 层做类别过滤）。 */
export const CATEGORY_WEIGHT: Record<ResultCategory, number> = {
  app: 5,
  file: 4,
  command: 3,
  content: 2,
  settings: 1,
};

export function aggregate(input: AggregateInput, f: SearchFilters, now = Date.now()): UnifiedHit[] {
  const hits: UnifiedHit[] = [];
  const q = f.text.toLowerCase();
  const match = (s: string) => !q || s.toLowerCase().includes(q);
  for (const a of input.apps ?? []) {
    if (match(a.title)) hits.push({ category: "app", title: a.title, score: (a.score ?? 1) * CATEGORY_WEIGHT.app });
  }
  for (const file of applyFileFilters(input.files ?? [], f, now)) {
    if (match(file.title) || match(file.path)) {
      hits.push({
        category: "file",
        title: file.title,
        subtitle: file.path,
        quickActions: ["open", "reveal", "copyPath"],
        score: CATEGORY_WEIGHT.file,
      });
    }
  }
  for (const c of input.contents ?? []) {
    if (match(c.title)) hits.push({ category: "content", title: c.title, subtitle: c.folderPath, score: (c.score ?? 1) * CATEGORY_WEIGHT.content });
  }
  for (const c of input.commands ?? []) {
    if (match(c.title) || match(c.id)) hits.push({ category: "command", title: c.title, subtitle: c.id, score: (c.score ?? 1) * CATEGORY_WEIGHT.command });
  }
  for (const s of input.settings ?? []) {
    if (match(s.title)) hits.push({ category: "settings", title: s.title, score: (s.score ?? 1) * CATEGORY_WEIGHT.settings });
  }
  return hits.sort((a, b) => b.score - a.score);
}

/** 类别 Tab 锁定。 */
export function lockCategory(hits: UnifiedHit[], cat: ResultCategory | null): UnifiedHit[] {
  return cat === null ? hits : hits.filter((h) => h.category === cat);
}
