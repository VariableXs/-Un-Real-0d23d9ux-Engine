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
