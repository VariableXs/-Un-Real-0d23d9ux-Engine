/**
 * F108 自定义短语库 · 前端逻辑（C 桌面体验域·后段 · AI-D2）。
 *
 * 判据（主册 G-C-38）：100 条短语库实测输入-候选-上屏全链 <100ms；变量
 * 三族格式 20 例全对；导入导出 round-trip 无损。
 *
 * 移植声明：`kernel/varix/src/stard/phrasebk.rs` 的 TS 同构移植——变量
 * 白名单 9 格式符（族名-格式符双重校验，白名单外拒绝不静默展开）、缩写
 * 32/内容 500 边界、导入三策略、使用计数冷沉底，逐项对齐。round-trip
 * 无损由 JSON 序列化承载（open format，十四章数据开放）。
 */

/** 缩写最大长度（字符）。 */
export const ABBR_MAX_CHARS = 32;
/** 内容最大长度（字符）。 */
export const CONTENT_MAX_CHARS = 500;
/** 词条上限。 */
export const PHRASE_CAP = 1000;
/** 全链延迟判线（ms）。 */
export const CHAIN_BUDGET_MS = 100;

/** 白名单格式符 → 语义 tag（解析器只认这三族——族名与格式符双重校验）。 */
function formatToken(fmt: string): "date" | "time" | "weekday" | null {
  switch (fmt) {
    case "yyyy-MM-dd": case "yyyy/M/d": case "yyyy年M月d日": return "date";
    case "HH:mm": case "HH:mm:ss": case "HH时mm分": return "time";
    case "EEEE": case "周E": case "星期E": return "weekday";
    default: return null;
  }
}

/** 变量白名单（9 格式符——与 Rust WHITELIST 同源）。 */
export const VAR_WHITELIST: readonly string[] = [
  "yyyy-MM-dd", "yyyy/M/d", "yyyy年M月d日", "HH:mm", "HH:mm:ss", "HH时mm分", "EEEE", "周E", "星期E",
];

export interface Var { tag: "date" | "time" | "weekday"; fmt: string }

/** 解析内容中的变量（白名单外格式 → 忽略——不静默展开）。 */
export function parseVars(content: string): Var[] {
  const out: Var[] = [];
  const re = /\{([^{}]*)\}/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(content)) !== null) {
    const inner = m[1] ?? "";
    const idx = inner.indexOf(":");
    if (idx < 0) continue;
    const name = inner.slice(0, idx).trim();
    const fmt = inner.slice(idx + 1).trim();
    if (name !== "date" && name !== "time" && name !== "weekday") continue;
    const tag = formatToken(fmt);
    if (tag && tag === name) out.push({ tag, fmt });
  }
  return out;
}

/** 变量求值（注入本地时刻与星期——由调用方格式化后传入，保持纯函数可测）。 */
export function renderVars(content: string, now: string, weekdayCn: string): string {
  let out = content;
  for (const v of parseVars(content)) {
    const pat = `{${v.tag}:${v.fmt}}`;
    const val = v.tag === "weekday" ? weekdayCn : `${now}（${v.fmt}）`;
    out = out.split(pat).join(val);
  }
  return out;
}

export interface Phrase {
  abbr: string;
  content: string;
  category: string;
  uses: number;
}

export type ImportPolicy = "overwrite" | "skip" | "keepBoth";

export interface ImportReport { added: number; overwritten: number; skipped: number; rejected: string[] }

/** 短语库。 */
export class PhraseBook {
  items: Phrase[] = [];
  /** 短语优先开关（与词库词重 → 短语优先并可配置）。 */
  phrasePriority = true;

  /** 新建/覆盖（校验：缩写 1~32 字符、内容 1~500 字符、上限诚实拒绝）。 */
  add(abbr: string, content: string, category: string): { ok: boolean; error?: string } {
    if (abbr.length === 0 || [...abbr].length > ABBR_MAX_CHARS) {
      return { ok: false, error: "缩写需 1~32 个字符" };
    }
    if (content.length === 0 || [...content].length > CONTENT_MAX_CHARS) {
      return { ok: false, error: "内容需 1~500 个字符" };
    }
    if (!this.items.some((p) => p.abbr === abbr) && this.items.length >= PHRASE_CAP) {
      return { ok: false, error: "短语库已满（1000 条）——请先整理" };
    }
    // 同缩写覆盖（库内唯一——候选面不歧义）。
    this.items = this.items.filter((p) => p.abbr !== abbr);
    this.items.push({ abbr, content, category, uses: 0 });
    return { ok: true };
  }

  remove(abbr: string): boolean {
    const before = this.items.length;
    this.items = this.items.filter((p) => p.abbr !== abbr);
    return this.items.length < before;
  }

  /** 触发展开：缩写命中 → 内容（含变量求值）+ 使用计数。null = 未命中
   *  （普通词库走原路——短语优先时也只对命中缩写生效）。 */
  expand(abbr: string, now: string, weekdayCn: string): string | null {
    const p = this.items.find((x) => x.abbr === abbr);
    if (!p) return null;
    p.uses += 1;
    return renderVars(p.content, now, weekdayCn);
  }

  /** 搜索（缩写/内容/分类三列子串）。 */
  search(q: string): Phrase[] {
    if (q === "") return [...this.items];
    return this.items.filter((p) => p.abbr.includes(q) || p.content.includes(q) || p.category.includes(q));
  }

  /** 分类树分组（计数）。 */
  categories(): Array<{ name: string; count: number }> {
    const map = new Map<string, number>();
    for (const p of this.items) map.set(p.category, (map.get(p.category) ?? 0) + 1);
    return [...map.entries()].map(([name, count]) => ({ name, count }));
  }

  /** 使用计数排序（冷短语沉底——stable，同频保持插入序）。 */
  ranked(): Phrase[] {
    return [...this.items].sort((a, b) => b.uses - a.uses);
  }

  /** 导出（open format：phrases 数组 + 格式标记——round-trip 无损载体）。 */
  export(): string {
    return JSON.stringify({ format: "varix-phrases", version: 1, phrasePriority: this.phrasePriority, items: this.items }, null, 2);
  }

  /** 导入（三策略逐条裁决；非法条目诚实拒绝入 rejected——零静默）。 */
  import(json: string, policy: ImportPolicy): ImportReport {
    const report: ImportReport = { added: 0, overwritten: 0, skipped: 0, rejected: [] };
    let parsed: unknown;
    try {
      parsed = JSON.parse(json);
    } catch {
      report.rejected.push("整包：不是合法 JSON");
      return report;
    }
    const obj = parsed as { items?: unknown; phrasePriority?: unknown };
    if (typeof obj?.phrasePriority === "boolean") this.phrasePriority = obj.phrasePriority;
    const items = Array.isArray(obj?.items) ? (obj.items as unknown[]) : null;
    if (!items) {
      report.rejected.push("整包：缺少 items 数组");
      return report;
    }
    for (let i = 0; i < items.length; i++) {
      const raw = items[i] as Partial<Phrase> | null;
      const abbr = typeof raw?.abbr === "string" ? raw.abbr : "";
      const content = typeof raw?.content === "string" ? raw.content : "";
      const category = typeof raw?.category === "string" ? raw.category : "";
      const uses = typeof raw?.uses === "number" ? raw.uses : 0;
      if (abbr === "" || content === "") {
        report.rejected.push(`第 ${i + 1} 条：缩写或内容为空`);
        continue;
      }
      if ([...abbr].length > ABBR_MAX_CHARS || [...content].length > CONTENT_MAX_CHARS) {
        report.rejected.push(`第 ${i + 1} 条（${abbr}）：超出长度边界`);
        continue;
      }
      const existing = this.items.find((p) => p.abbr === abbr);
      if (existing) {
        if (policy === "skip") { report.skipped += 1; continue; }
        if (policy === "keepBoth") {
          // 都留：同缩写冲突时后缀编号（候选面不歧义）。
          let n = 2;
          let candidate = `${abbr}#${n}`;
          while (this.items.some((p) => p.abbr === candidate)) candidate = `${abbr}#${++n}`;
          this.items.push({ abbr: candidate, content, category, uses });
          report.added += 1;
          continue;
        }
        existing.content = content;
        existing.category = category;
        existing.uses = uses;
        report.overwritten += 1;
        continue;
      }
      if (this.items.length >= PHRASE_CAP) {
        report.rejected.push(`第 ${i + 1} 条（${abbr}）：库满未导入`);
        continue;
      }
      this.items.push({ abbr, content, category, uses });
      report.added += 1;
    }
    return report;
  }
}

/** 候选混排：短语候选与普通候选混排（短语带徽标；短语优先开关生效）。
 *  normal 由调用方给（词库面）；本函数只裁决排序与徽标——一处一事实。 */
export interface Candidate { text: string; phrase: boolean }

export function mergeCandidates(phrases: Phrase[], normal: string[], phrasePriority: boolean): Candidate[] {
  const ph: Candidate[] = phrases.map((p) => ({ text: p.content, phrase: true }));
  const nm: Candidate[] = normal.map((t) => ({ text: t, phrase: false }));
  if (!phrasePriority) return [...nm, ...ph];
  return [...ph, ...nm];
}
