/**
 * F391 选中文本翻译（H 域 · AI-H4）：
 * 选中文本右键「翻译」：短文本（≤500 字符）跳 Edge 在线翻译（无商店宪法同路：翻译
 * 大模型不内置，浏览器是门）；译文回到浮卡（F390 同形制）或浏览器标签（长文本直达）；
 * 源语言自动检测、目标语言设置可改。
 * 判据（主册 F391）：长短分界（500 字符）两路用例；Edge 跳转参数（选中内容带过去）；
 * 浮卡同形制复用 F390 判据；离线时诚实提示「翻译需要网络」。
 * 依赖锚点：F390 查词浮卡（同形制复用）。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 长短分界（判据 500 字符）。 */
export const LENGTH_SPLIT_CHARS = 500;

export type TranslateRoute = "card" | "browserTab" | "offlineNotice";

export interface TranslateRequest {
  text: string;
  targetLang: string;
  online: boolean;
}

export interface TranslatePlan {
  route: TranslateRoute;
  /** 浮卡路径：跳转参数（浏览器拉取译文后回卡）；长文本直达标签页。 */
  edgeUrl: string | null;
  /** 离线诚实提示（判据：不假装会翻）。 */
  offlineMessage: string | null;
  rationale: string;
}

/** 翻译路由（判据三路）：短+在线=浮卡；长+在线=标签直达；离线=诚实提示。 */
export function planTranslate(req: TranslateRequest): TranslatePlan {
  const text = req.text.trim();
  if (!req.online) {
    return {
      route: "offlineNotice",
      edgeUrl: null,
      offlineMessage: "翻译需要网络——离线状态下只提供本地词典查词（F390）",
      rationale: "无商店宪法：翻译大模型不内置，浏览器是门；离线时诚实说做不到",
    };
  }
  const short = text.length <= LENGTH_SPLIT_CHARS;
  return {
    route: short ? "card" : "browserTab",
    edgeUrl: edgeTranslateUrl(text, req.targetLang),
    offlineMessage: null,
    rationale: short ? "短文本：浮卡呼出（F390 同形制），译文回卡不打断阅读" : "长文本：直达 Edge 标签页专业服务（500 字符分界判据）",
  };
}

/** Edge 跳转参数（判据「选中内容带过去」）：URL 编码 + 目标语言参数。 */
export function edgeTranslateUrl(text: string, targetLang: string): string {
  return `https://www.bing.com/translator/?text=${encodeURIComponent(text)}&to=${encodeURIComponent(targetLang)}`;
}

/** 源语言自动检测（轻量启发式：CJK 比例判向——检测置信度如实标注）。 */
export function detectSourceLang(text: string): { lang: "zh" | "en" | "unknown"; confidence: "high" | "low" } {
  const sample = text.slice(0, 200);
  if (!sample.trim()) return { lang: "unknown", confidence: "low" };
  const cjk = (sample.match(/[\u4e00-\u9fff]/g) ?? []).length;
  const ratio = cjk / sample.length;
  if (ratio > 0.15) return { lang: "zh", confidence: "high" };
  if (/[a-zA-Z]/.test(sample)) return { lang: "en", confidence: "high" };
  return { lang: "unknown", confidence: "low" };
}

/* ---------- 目标语言设置（可改判据） ---------- */

const KEY = h4Key("f391", "target");

export function getTargetLang(store: KvStore = defaultStore()): string {
  return readJson<string>(store, KEY, "en", (v): v is string => typeof v === "string" && v.length > 0);
}

export function setTargetLang(lang: string, store: KvStore = defaultStore()): boolean {
  if (!lang.trim()) return false;
  return writeJson(store, KEY, lang);
}

/** 浮卡同形制复用 F390（判据）：卡片状态机直接引用 F390 的模型（一处实现）。 */
export { openCard as reuseCard, outsideClick as reuseOutsideClick } from "./f390-wordLookup";
