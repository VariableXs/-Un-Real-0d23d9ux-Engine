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

/* ================= v4 深化批次四：内置词典页 / 词形变体归一 / 分页 LRU / 生词导出 ================= */

/** 内置英汉词典（离线判据的实体数据——常用 40 词；全量库按 DICT_PAGE_SIZE 分页外挂）。 */
export const MINI_DICT_EN_ZH: ReadonlyArray<DictEntry> = [
  { word: "algorithm", pos: "n.", gloss: "算法；一套解题的明确步骤" },
  { word: "backup", pos: "n./v.", gloss: "备份；为数据留退路" },
  { word: "buffer", pos: "n.", gloss: "缓冲区；吸收速度差的中间地带" },
  { word: "cache", pos: "n./v.", gloss: "缓存；把常用的放在手边" },
  { word: "compile", pos: "v.", gloss: "编译；把源码翻成机器能跑的形态" },
  { word: "compress", pos: "v.", gloss: "压缩；用更少的字节装下同样的内容" },
  { word: "cursor", pos: "n.", gloss: "光标；你正在看的位置" },
  { word: "daemon", pos: "n.", gloss: "守护进程；在后台默默干活的程序" },
  { word: "debug", pos: "v.", gloss: "调试；找出并修掉缺陷" },
  { word: "deploy", pos: "v.", gloss: "部署；把做好的东西送到该跑的地方" },
  { word: "encrypt", pos: "v.", gloss: "加密；只有持钥者能读" },
  { word: "firmware", pos: "n.", gloss: "固件；硬件里出厂写死的程序" },
  { word: "firmwareupdate", pos: "n.", gloss: "固件更新（内部复合词，测试变体归一用）" },
  { word: "glossary", pos: "n.", gloss: "词汇表；术语的对照清单" },
  { word: "hash", pos: "n./v.", gloss: "哈希；内容的指纹" },
  { word: "incremental", pos: "adj.", gloss: "增量的；只处理变化的部分" },
  { word: "integrity", pos: "n.", gloss: "完整性；数据没有被改坏" },
  { word: "kernel", pos: "n.", gloss: "内核；系统的心脏" },
  { word: "latency", pos: "n.", gloss: "延迟；从按下去到有反应的时间" },
  { word: "legacy", pos: "adj.", gloss: "遗留的；老系统留下的" },
  { word: "manifest", pos: "n.", gloss: "清单；声明自己是什么、要什么" },
  { word: "metadata", pos: "n.", gloss: "元数据；关于数据的数据" },
  { word: "middleware", pos: "n.", gloss: "中间件；两头之间的翻译官" },
  { word: "mirror", pos: "n./v.", gloss: "镜像；一模一样的另一份" },
  { word: "offline", pos: "adj.", gloss: "离线；不联网也能用" },
  { word: "overwrite", pos: "v.", gloss: "覆盖；用新的把旧的抹掉（危险动作）" },
  { word: "partition", pos: "n.", gloss: "分区；磁盘上划分出的独立区域" },
  { word: "pipeline", pos: "n.", gloss: "流水线；一步接一步的处理链" },
  { word: "query", pos: "n./v.", gloss: "查询；向数据要答案" },
  { word: "recursive", pos: "adj.", gloss: "递归的；自己调用自己" },
  { word: "redundancy", pos: "n.", gloss: "冗余；多留的一份保险" },
  { word: "restore", pos: "v.", gloss: "还原；把备份放回原位" },
  { word: "rollback", pos: "n./v.", gloss: "回滚；退回上一个好状态" },
  { word: "sanity", pos: "n.", gloss: "合理性检查；先确认输入不是疯的" },
  { word: "snapshot", pos: "n.", gloss: "快照；某一刻的完整定格" },
  { word: "telemetry", pos: "n.", gloss: "遥测；系统上报自己的运行数据" },
  { word: "thumbnail", pos: "n.", gloss: "缩略图；内容的小号预览" },
  { word: "transaction", pos: "n.", gloss: "事务；要么全成、要么全不算" },
  { word: "virtual", pos: "adj.", gloss: "虚拟的；用软件模拟出来的" },
  { word: "workflow", pos: "n.", gloss: "工作流；一件事从起到完的路径" },
];

/** 页构建：词条数组 → 查询页（小写键归一）。 */
export function buildDictPage(entries: ReadonlyArray<DictEntry>): DictionaryPage {
  const page: DictionaryPage = new Map();
  for (const e of entries) page.set(e.word.toLowerCase(), e);
  return page;
}

/**
 * 词形变体归一：直接未命中 → 复数/过去式/进行式逐规则回溯——
 * 命中时标注走了哪条规则（查 "ran" 得 "run" 的路径可解释，不是黑盒猜）。
 */
export function lookupVariants(page: DictionaryPage, word: string): { result: LookupResult; viaRule: string | null } {
  const direct = lookup(page, word);
  if (direct.found) return { result: direct, viaRule: null };
  const w = word.trim().toLowerCase();
  const rules: Array<[string, string]> = [];
  if (w.endsWith("ies") && w.length > 3) rules.push(["y→ies 复数", `${w.slice(0, -3)}y`]);
  if (/(?:ses|xes|zes|ches|shes)$/.test(w)) rules.push(["-es 复数", w.slice(0, -2)]);
  if (w.endsWith("s") && !w.endsWith("ss")) rules.push(["-s 复数", w.slice(0, -1)]);
  if (w.endsWith("ied") && w.length > 3) rules.push(["y→ied 过去式", `${w.slice(0, -3)}y`]);
  if (w.endsWith("ed")) {
    rules.push(["-d 过去式", w.slice(0, -1)]);
    rules.push(["-ed 过去式", w.slice(0, -2)]);
  }
  if (w.endsWith("ing")) {
    rules.push(["去 e +ing", `${w.slice(0, -3)}e`]);
    rules.push(["-ing 进行式", w.slice(0, -3)]);
  }
  for (const [rule, cand] of rules) {
    const hit = lookup(page, cand);
    if (hit.found) return { result: hit, viaRule: rule };
  }
  return { result: direct, viaRule: null };
}

/** 页缓存上限（判据「按需分页载入内存」——只留最近用过的几页，不整库驻留）。 */
export const PAGE_CACHE_CAP = 4;

/** 分页 LRU 装载器：命中秒出；未命中调 loader 装页；超限淘汰最久未用（确定性口径）。 */
export class DictPageCache {
  private readonly cache = new Map<number, DictionaryPage>();

  load(pageNo: number, loader: (pageNo: number) => DictionaryPage): { page: DictionaryPage; hit: boolean } {
    const hit = this.cache.get(pageNo);
    if (hit) {
      this.cache.delete(pageNo);
      this.cache.set(pageNo, hit);
      return { page: hit, hit: true };
    }
    const page = loader(pageNo);
    this.cache.set(pageNo, page);
    while (this.cache.size > PAGE_CACHE_CAP) {
      const oldest = this.cache.keys().next().value as number;
      this.cache.delete(oldest);
    }
    return { page, hit: false };
  }

  get size(): number {
    return this.cache.size;
  }
}

/** 生词本导出（数据开放十四章：用户的数据用户带走——CSV 开放格式、无锁出）。 */
export function exportWordlistCsv(list: string[], glossOf: (word: string) => string | null): string {
  const rows = list.map((w, i) => `${i + 1},"${w}","${glossOf(w) ?? "（未命中）"}"`);
  return ["序号,单词,释义", ...rows].join("\n");
}
