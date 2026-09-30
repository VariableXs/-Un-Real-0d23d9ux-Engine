/**
 * F398 界面语言热切（H 域 · AI-H4）：
 * 界面语言切换免重启（F140 开放本地化的用户面）：设置中心选语言 → 1 分钟内全系统换装
 * （与 F225 主题换装同机制复用）；切换不丢状态（打开的窗口、未保存文档原样在）；未翻译
 * 条目回退英文并标注（开发者可见哪些词条缺翻译——F132 差异表公开联动）；RTL 语言镜像
 * 布局支持预留接口。
 * 判据（主册 F398）：换装 <1 分钟实测；状态保持；回退标注机制；词条覆盖率对账 F132；
 * RTL 接口存在性（代码判据）。
 * 依赖锚点：F132 差异表公开 / F140 开放本地化 / F225 主题切换免重启。
 * 存储键：variable:h4:f398:lang
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 换装预算（判据 <1 分钟）。 */
export const RESWAP_BUDGET_MS = 60_000;

export type UiLanguage = "zh-CN" | "en" | string;

/** 词表接口：key → 该语言词条（缺词条=未翻译，走回退标注）。 */
export type Bundle = Record<string, string>;

export interface TranslationIssue {
  key: string;
  lang: UiLanguage;
  /** 回退后实际显示的文本。 */
  fallbackText: string;
  /** 缺译标注（开发者可见——判据「回退标注机制」）。 */
  annotated: true;
}

/**
 * 词条解析：当前语言命中 → 原文；未命中 → 英文回退 + 缺译标注（零机器腔硬凑）。
 */
export function resolveText(bundles: Partial<Record<UiLanguage, Bundle>>, key: string, lang: UiLanguage): { text: string; issue: TranslationIssue | null } {
  const hit = bundles[lang]?.[key];
  if (hit !== undefined && hit !== "") return { text: hit, issue: null };
  const fallback = bundles.en?.[key] ?? key;
  return { text: fallback, issue: { key, lang, fallbackText: fallback, annotated: true } };
}

/** 词条覆盖率对账（判据「对账 F132」）：各语言覆盖率 = 有译词条/总键数。 */
export function coverage(bundles: Partial<Record<UiLanguage, Bundle>>, lang: UiLanguage): { total: number; translated: number; pct: number } {
  const all = new Set<string>();
  for (const b of Object.values(bundles)) for (const k of Object.keys(b ?? {})) all.add(k);
  const translated = [...all].filter((k) => (bundles[lang]?.[k] ?? "").trim().length > 0).length;
  return { total: all.size, translated, pct: all.size === 0 ? 100 : Math.round((translated / all.size) * 100) };
}

/** 差异表条目（F132 联动）：全部缺译键 → 公开清单。 */
export function diffTable(bundles: Partial<Record<UiLanguage, Bundle>>, lang: UiLanguage): TranslationIssue[] {
  const all = new Set<string>();
  for (const b of Object.values(bundles)) for (const k of Object.keys(b ?? {})) all.add(k);
  return [...all]
    .filter((k) => (bundles[lang]?.[k] ?? "").trim().length === 0)
    .map((k) => ({ key: k, lang, fallbackText: bundles.en?.[k] ?? k, annotated: true as const }));
}

/* ---------- 状态保持（判据「切换不丢状态」） ---------- */

export interface SessionSnapshot {
  /** 打开的窗口 id。 */
  openWindows: string[];
  /** 各窗口未保存草稿哈希（切换前后必须一致）。 */
  draftHashes: Record<string, string>;
  lang: UiLanguage;
}

export function snapshotSession(openWindows: string[], draftHashes: Record<string, string>, lang: UiLanguage): SessionSnapshot {
  return { openWindows: [...openWindows], draftHashes: { ...draftHashes }, lang };
}

/** 状态保持校验：窗口集与草稿哈希逐项一致（草稿零丢失判据的机检面）。 */
export function verifyStateKept(before: SessionSnapshot, after: SessionSnapshot): { kept: boolean; lostWindows: string[]; changedDrafts: string[] } {
  const lostWindows = before.openWindows.filter((w) => !after.openWindows.includes(w));
  const changedDrafts = Object.keys(before.draftHashes).filter((k) => before.draftHashes[k] !== after.draftHashes[k]);
  return { kept: lostWindows.length === 0 && changedDrafts.length === 0, lostWindows, changedDrafts };
}

/** 换装预算审计（判据 <1 分钟）。 */
export function withinReswapBudget(startedAtMs: number, doneAtMs: number): boolean {
  return doneAtMs - startedAtMs < RESWAP_BUDGET_MS;
}

/* ---------- 当前语言持久化 ---------- */

const KEY = h4Key("f398", "lang");

export function getLang(store: KvStore = defaultStore()): UiLanguage {
  return readJson<UiLanguage>(store, KEY, "zh-CN", (v): v is UiLanguage => typeof v === "string" && v.length > 0);
}

export function setLang(lang: UiLanguage, store: KvStore = defaultStore()): boolean {
  if (!lang.trim()) return false;
  return writeJson(store, KEY, lang);
}

/* ---------- RTL 接口预留（判据「RTL 接口存在性——代码判据」） ---------- */

/** RTL 语言表（阿拉伯语/希伯来语/波斯语——接口预留的判定源）。 */
export const RTL_LANGS: ReadonlySet<string> = new Set(["ar", "he", "fa"]);

/** 布局方向接口：RTL 语言返回 rtl（镜像布局的单一入口——接口存在即判据达成）。 */
export function directionFor(lang: UiLanguage): "ltr" | "rtl" {
  return RTL_LANGS.has(lang.split("-")[0] ?? "") ? "rtl" : "ltr";
}

/* ================= v4 深化批次四：占位符校验 / 复数规则 / 词表差异 / 缺译分组 / 镜像令牌 ================= */

/** 占位符校验：译文与基准（英文）的 {slot} 必须一致——多占少占都是缺陷（回退标注的防线上移）。 */
export function validatePlaceholders(reference: string, translation: string): { ok: boolean; missing: string[]; extra: string[] } {
  const slots = (s: string) => new Set([...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]!));
  const ref = slots(reference);
  const got = slots(translation);
  const missing = [...ref].filter((k) => !got.has(k));
  const extra = [...got].filter((k) => !ref.has(k));
  return { ok: missing.length === 0 && extra.length === 0, missing, extra };
}

/** 复数范畴（zh：无单复数恒 other；en：one/other——F140 开放本地化的接口实做面）。 */
export type PluralCategory = "zero" | "one" | "other";

export function pluralCategory(lang: UiLanguage, n: number): PluralCategory {
  const base = lang.split("-")[0] ?? "";
  if (base === "en") return n === 1 ? "one" : "other";
  return "other";
}

export function pickPlural(bundle: Bundle | undefined, key: string, lang: UiLanguage, n: number): { text: string; issue: TranslationIssue | null } {
  const k = `${key}.${pluralCategory(lang, n)}`;
  const hit = bundle?.[k];
  if (hit !== undefined && hit !== "") return { text: hit, issue: null };
  const fb = bundle?.[key] ?? k;
  return { text: fb, issue: { key: k, lang, fallbackText: fb, annotated: true } };
}

export interface BundleDiffRow {
  key: string;
  kind: "added" | "removed" | "changed";
  from: string | null;
  to: string | null;
}

/** 词表差异（F132 联动的结构化面）：两词表间的新增/缺失/改动三类逐键列出。 */
export function bundleDiff(from: Bundle, to: Bundle): BundleDiffRow[] {
  const rows: BundleDiffRow[] = [];
  const keys = new Set([...Object.keys(from), ...Object.keys(to)]);
  for (const k of [...keys].sort()) {
    const a = from[k];
    const b = to[k];
    if (a === undefined && b !== undefined) rows.push({ key: k, kind: "added", from: null, to: b });
    else if (a !== undefined && b === undefined) rows.push({ key: k, kind: "removed", from: a, to: null });
    else if (a !== b) rows.push({ key: k, kind: "changed", from: a ?? null, to: b ?? null });
  }
  return rows;
}

/** 缺译报告（按命名空间分组——翻译派单的直读面，多大的洞一眼可见）。 */
export function missingKeyReport(bundles: Partial<Record<UiLanguage, Bundle>>, lang: UiLanguage): Array<{ namespace: string; keys: string[] }> {
  const byNs = new Map<string, string[]>();
  for (const issue of diffTable(bundles, lang)) {
    const ns = issue.key.split(".")[0] ?? issue.key;
    byNs.set(ns, [...(byNs.get(ns) ?? []), issue.key]);
  }
  return [...byNs.entries()].map(([namespace, keys]) => ({ namespace, keys })).sort((a, b) => b.keys.length - a.keys.length);
}

/** 镜像令牌（RTL 接口的存在性实做面）：方向感符号在 RTL 下镜像（接口从「存在」到「可用」）。 */
const MIRROR_TOKENS: ReadonlyMap<string, string> = new Map([
  ["→", "←"],
  ["←", "→"],
  ["»", "«"],
  ["«", "»"],
]);

export function mirrorToken(token: string, dir: "ltr" | "rtl" = "ltr"): string {
  if (dir !== "rtl") return token;
  return MIRROR_TOKENS.get(token) ?? token;
}
