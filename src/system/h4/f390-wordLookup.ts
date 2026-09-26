/**
 * F390 选中文本查词（H 域 · AI-H4）：
 * 选中文本右键「查词」：浮卡显示释义（离线词典——英汉/汉英双库内置，U 盘系统离线可用），
 * 浮卡不抢焦点、点外即关；生词本轻量（查过的词留最近 20 条，浮卡底部翻看）。
 * 判据（主册 F390）：离线判据（断网全功能）；释义卡时序（<300ms 呼出）；不抢焦点；
 * 生词 20 条；词典大小与加载（按需分页载入内存）。
 * 存储键：variable:h4:f390（生词本）。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 释义卡呼出预算（判据 <300ms）。 */
export const CARD_BUDGET_MS = 300;
/** 生词本容量（判据最近 20 条）。 */
export const WORDLIST_CAP = 20;
/** 词典分页尺寸（判据「按需分页载入内存」）。 */
export const DICT_PAGE_SIZE = 500;

export interface DictEntry {
  word: string;
  /** 词性标签（n./v./adj.…）。 */
  pos: string;
  gloss: string;
}

/** 内置词典数据面（离线判据的载体；生产由分页词典文件供给——此处为引擎接口的内存页）。 */
export type DictionaryPage = Map<string, DictEntry>;

export interface LookupResult {
  word: string;
  found: boolean;
  entry: DictEntry | null;
  /** 查询是否走了网络（离线判据：恒 false——引擎无网络路径）。 */
  usedNetwork: false;
}

/** 查词：小写归一 → 页内命中；未命中如实返回 found=false（零编造释义）。 */
export function lookup(page: DictionaryPage, word: string): LookupResult {
  const key = word.trim().toLowerCase();
  const entry = key ? page.get(key) ?? null : null;
  return { word: key, found: entry !== null, entry, usedNetwork: false };
}

/** 生词本记账：查过即留痕、去重提首、容量 20（判据）。 */
export function pushWordlist(list: string[], word: string): string[] {
  const key = word.trim().toLowerCase();
  if (!key) return list;
  const rest = list.filter((w) => w !== key);
  return [key, ...rest].slice(0, WORDLIST_CAP);
}

const KEY = h4Key("f390", "wordlist");

export function saveWordlist(list: string[], store: KvStore = defaultStore()): boolean {
  return writeJson(store, KEY, list.slice(0, WORDLIST_CAP));
}

export function loadWordlist(store: KvStore = defaultStore()): string[] {
  return readJson<string[]>(store, KEY, [], Array.isArray);
}

/** 浮卡交互模型（判据「不抢焦点、点外即关」）：纯状态机，无全局焦点索取。 */
export interface CardState {
  visible: boolean;
  /** false = 永不索取键盘焦点（阅读心流不被打断）。 */
  stealsFocus: false;
}

export function openCard(): CardState {
  return { visible: true, stealsFocus: false };
}

/** 点外即关。 */
export function outsideClick(card: CardState): CardState {
  return { ...card, visible: false };
}

/** 释义卡时序审计（判据 <300ms）。 */
export function withinCardBudget(openedAtMs: number, readyAtMs: number): boolean {
  return readyAtMs - openedAtMs < CARD_BUDGET_MS;
}

/** 词典分页加载器（判据「按需分页载入内存」）：仅载入命中词所在页。 */
export function pageFor(word: string, pages: number): { page: number; inRange: boolean } {
  const key = word.trim().toLowerCase();
  const idx = key ? Math.abs(hash(key)) % pages : -1;
  return { page: idx, inRange: idx >= 0 && idx < pages };
}

function hash(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
  return h;
}

/** 汉英向查词接口（双库判据）：同引擎、独立页表。 */
export function lookupZh(page: DictionaryPage, word: string): LookupResult {
  return lookup(page, word);
}
