/**
 * UNREAL-X AI-18 · 族0171 翻译词典 2.0 + 族0172 屏幕识图 2.0 + 族0173 OCR 提取
 * + 族0174 统计训练（X04251~X04375）。
 *
 * 翻译：本地词典（隐私边界内）/候选排序/术语表。
 * 识图：选区截图 → 区域分块 → 文本块几何（供 OCR 前处理）。
 * OCR：字符框拼接成行/行→段落的几何聚类（纯几何，零 AI）。
 * 统计训练：本地 n-gram 计数/平滑/词频导出（供输入预测消费）。
 */

/* ============================== 族0171 翻译词典 2.0 ============================== */

/** 翻译五档。 */
export const TRANSLATE_PROFILES = [
  { id: "off", name: "关闭", autoDetect: false, hover: false, maxLen: 0 },
  { id: "manual", name: "手动", autoDetect: false, hover: false, maxLen: 2000 },
  { id: "balanced", name: "均衡", autoDetect: true, hover: false, maxLen: 4000 },
  { id: "hover", name: "悬停", autoDetect: true, hover: true, maxLen: 4000 },
  { id: "full", name: "全量", autoDetect: true, hover: true, maxLen: 8000 },
] as const;
export type TranslateProfileId = (typeof TRANSLATE_PROFILES)[number]["id"];
export const DEFAULT_TRANSLATE_ID: TranslateProfileId = "balanced";

export function findTranslate(id: string): (typeof TRANSLATE_PROFILES)[number] {
  return TRANSLATE_PROFILES.find((p) => p.id === id) ?? TRANSLATE_PROFILES[2]!;
}

/** 内置本地词典（子集；词条可由用户术语表补充，隐私边界内不出网）。 */
export const LOCAL_DICT: readonly { src: string; dst: string; note?: string }[] = [
  { src: "window", dst: "窗口" },
  { src: "input", dst: "输入" },
  { src: "keyboard", dst: "键盘" },
  { src: "focus", dst: "焦点" },
  { src: "gesture", dst: "手势" },
  { src: "输入法", dst: "IME", note: "input method editor" },
  { src: "键盘", dst: "keyboard" },
  { src: "窗口", dst: "window" },
];

export interface TranslateTerm {
  term: string;
  fixed: string;
}

/** 翻译引擎：词典直查 + 术语表强制替换 + 长度钳制。 */
export class TranslateEngine {
  profileId: TranslateProfileId;
  terms: TranslateTerm[] = [];
  lookups = 0;
  clamped = 0;

  constructor(profileId: string = DEFAULT_TRANSLATE_ID, terms: TranslateTerm[] = []) {
    const known = TRANSLATE_PROFILES.some((p) => p.id === profileId);
    if (!known) this.clamped += 1;
    this.profileId = findTranslate(profileId).id as TranslateProfileId;
    this.terms = terms.filter((t) => t.term && t.fixed).slice(0, 4096);
  }

  /** 单词查询：术语表优先，词典其次，未命中 null。 */
  lookup(word: string): string | null {
    const w = word.trim().toLowerCase();
    if (!w) return null;
    this.lookups += 1;
    const term = this.terms.find((t) => t.term.toLowerCase() === w);
    if (term) return term.fixed;
    return LOCAL_DICT.find((d) => d.src.toLowerCase() === w)?.dst ?? null;
  }

  /** 整句直译：逐词替换（演示级，真实引擎走内核通道）。 */
  translate(text: string): string {
    const limit = findTranslate(this.profileId).maxLen;
    if (limit === 0) return "";
    const clipped = text.length > limit ? text.slice(0, limit) : text;
    if (text.length > limit) this.clamped += 1;
    return clipped.split(/(\s+)/).map((tok) => {
      const hit = this.lookup(tok);
      return hit ?? tok;
    }).join("");
  }

  /** 语种检测启发式：CJK 占比 → zh；拉丁 → en；空 → unknown。 */
  static detect(text: string): "zh" | "en" | "unknown" {
    if (!text.trim()) return "unknown";
    const cjk = (text.match(/[\u4e00-\u9fff]/g) ?? []).length;
    const latin = (text.match(/[a-zA-Z]/g) ?? []).length;
    if (cjk === 0 && latin === 0) return "unknown";
    return cjk >= latin ? "zh" : "en";
  }

  reset(): void {
    this.lookups = 0;
    this.clamped = 0;
  }
}

/* ============================== 族0172 屏幕识图 2.0 ============================== */

/** 识图五档。 */
export const SCREEN_READ_PROFILES = [
  { id: "off", name: "关闭", ocr: false, describe: false, maxArea: 0 },
  { id: "region", name: "选区", ocr: true, describe: false, maxArea: 1e6 },
  { id: "balanced", name: "均衡", ocr: true, describe: true, maxArea: 4e6 },
  { id: "window", name: "整窗", ocr: true, describe: true, maxArea: 1.6e7 },
  { id: "full", name: "全屏", ocr: true, describe: true, maxArea: 6.4e7 },
] as const;
export type ScreenReadProfileId = (typeof SCREEN_READ_PROFILES)[number]["id"];
export const DEFAULT_SCREEN_READ_ID: ScreenReadProfileId = "balanced";

export function findScreenRead(id: string): (typeof SCREEN_READ_PROFILES)[number] {
  return SCREEN_READ_PROFILES.find((p) => p.id === id) ?? SCREEN_READ_PROFILES[2]!;
}

export interface Rect { x: number; y: number; w: number; h: number }

/** 识图区准备：越界钳制 + 面积上限（档位）+ 分块网格。 */
export function prepareReadRegion(r: Rect, profileId: string): { rect: Rect; tiles: Rect[]; clamped: boolean } {
  const p = findScreenRead(profileId);
  const rect: Rect = {
    x: Math.max(0, r.x),
    y: Math.max(0, r.y),
    w: Math.max(1, Math.min(16384, r.w)),
    h: Math.max(1, Math.min(16384, r.h)),
  };
  let clamped = rect.x !== r.x || rect.y !== r.y || rect.w !== r.w || rect.h !== r.h;
  const area = rect.w * rect.h;
  if (p.maxArea > 0 && area > p.maxArea) {
    const k = Math.sqrt(p.maxArea / area);
    rect.w = Math.max(1, Math.floor(rect.w * k));
    rect.h = Math.max(1, Math.floor(rect.h * k));
    clamped = true;
  }
  // 4×4 分块网格（OCR 前处理/描述分块通用）。
  const tiles: Rect[] = [];
  const tw = Math.max(1, Math.floor(rect.w / 4));
  const th = Math.max(1, Math.floor(rect.h / 4));
  for (let gy = 0; gy < 4; gy++) {
    for (let gx = 0; gx < 4; gx++) {
      tiles.push({ x: rect.x + gx * tw, y: rect.y + gy * th, w: tw, h: th });
    }
  }
  return { rect, tiles, clamped };
}

/* ============================== 族0173 OCR 提取 ============================== */

export interface CharBox {
  ch: string;
  rect: Rect;
}

/** OCR 行聚类：字符框按 y 重叠聚合为行，行内按 x 排序拼接。 */
export function ocrGroupLines(boxes: CharBox[], overlapRatio = 0.4): { text: string; rect: Rect }[] {
  const valid = boxes.filter((b) => b.rect.w > 0 && b.rect.h > 0);
  const sorted = [...valid].sort((a, b) => a.rect.y - b.rect.y);
  const lines: CharBox[][] = [];
  for (const b of sorted) {
    const last = lines[lines.length - 1];
    if (last) {
      const ref = last[last.length - 1]!;
      const overlapH = Math.min(ref.rect.y + ref.rect.h, b.rect.y + b.rect.h) - Math.max(ref.rect.y, b.rect.y);
      const minH = Math.min(ref.rect.h, b.rect.h);
      if (overlapH >= minH * overlapRatio) {
        last.push(b);
        continue;
      }
    }
    lines.push([b]);
  }
  return lines.map((line) => {
    line.sort((a, b) => a.rect.x - b.rect.x);
    const x = Math.min(...line.map((c) => c.rect.x));
    const y = Math.min(...line.map((c) => c.rect.y));
    const r = Math.max(...line.map((c) => c.rect.x + c.rect.w));
    const btm = Math.max(...line.map((c) => c.rect.y + c.rect.h));
    return { text: line.map((c) => c.ch).join(""), rect: { x, y, w: r - x, h: btm - y } };
  });
}

/** 行→段落：行距小于 1.8 倍行高即并入同段。 */
export function ocrParagraphs(lines: { text: string; rect: Rect }[]): string[] {
  const paras: string[] = [];
  let cur = "";
  let prev: Rect | null = null;
  for (const l of lines) {
    const join = prev && l.rect.y - (prev.y + prev.h) <= prev.h * 0.8;
    cur = cur ? (join ? `${cur}${l.text}` : (paras.push(cur), l.text)) : l.text;
    prev = l.rect;
  }
  if (cur) paras.push(cur);
  return paras;
}

/* ============================== 族0174 统计训练 ============================== */

/** 统计训练五档。 */
export const STATS_TRAIN_PROFILES = [
  { id: "off", name: "关闭", ngram: 0, smooth: false, cap: 0 },
  { id: "tiny", name: "微量", ngram: 1, smooth: false, cap: 1000 },
  { id: "balanced", name: "均衡", ngram: 2, smooth: true, cap: 20000 },
  { id: "rich", name: "丰富", ngram: 3, smooth: true, cap: 100000 },
  { id: "full", name: "全量", ngram: 4, smooth: true, cap: 500000 },
] as const;
export type StatsTrainProfileId = (typeof STATS_TRAIN_PROFILES)[number]["id"];
export const DEFAULT_STATS_TRAIN_ID: StatsTrainProfileId = "balanced";

export function findStatsTrain(id: string): (typeof STATS_TRAIN_PROFILES)[number]["id"] {
  return (STATS_TRAIN_PROFILES.find((p) => p.id === id) ?? STATS_TRAIN_PROFILES[2]!).id as StatsTrainProfileId;
}

/** 本地 n-gram 训练器：纯计数 + 拉普拉斯平滑（隐私边界内，不出网）。 */
export class NgramTrainer {
  profileId: StatsTrainProfileId;
  counts = new Map<string, number>();
  total = 0;
  clamped = 0;

  constructor(profileId: string = DEFAULT_STATS_TRAIN_ID) {
    this.profileId = findStatsTrain(profileId);
  }

  get config() {
    return STATS_TRAIN_PROFILES.find((p) => p.id === this.profileId)!;
  }

  /** 喂入文本：按 n-gram 长度切分计数（上限裁剪即钳制）。 */
  feed(text: string): void {
    const n = this.config.ngram;
    if (n === 0) return;
    const chars = [...text].filter((c) => c.trim() !== "");
    for (let i = 0; i + n <= chars.length; i++) {
      const key = chars.slice(i, i + n).join("");
      if (this.total >= this.config.cap) {
        this.clamped += 1;
        return;
      }
      this.counts.set(key, (this.counts.get(key) ?? 0) + 1);
      this.total += 1;
    }
  }

  /** 候选打分：拉普拉斯平滑概率（未见面返回 1/(total+V)）。 */
  score(key: string): number {
    const v = this.counts.size + 1;
    const c = this.counts.get(key) ?? 0;
    if (!this.config.smooth) return this.total === 0 ? 0 : c / this.total;
    return (c + 1) / (this.total + v);
  }

  /** top-k 高频片段（训练产物，供输入预测消费）。 */
  top(k = 8): { key: string; count: number }[] {
    const n = Math.max(1, Math.min(256, k));
    return [...this.counts.entries()]
      .map(([key, count]) => ({ key, count }))
      .sort((a, b) => b.count - a.count || a.key.localeCompare(b.key))
      .slice(0, n);
  }

  /** 导出为可持久化 JSON（含版本与钳制计数）。 */
  export(): string {
    return JSON.stringify({ v: 2, profileId: this.profileId, total: this.total, top: this.top(64), clamped: this.clamped });
  }

  restore(snap: string | null): boolean {
    if (!snap) return false;
    try {
      const o = JSON.parse(snap) as { v?: number; profileId?: string; top?: { key: string; count: number }[] };
      if (o.v !== 2 || typeof o.profileId !== "string" || !Array.isArray(o.top)) return false;
      this.profileId = findStatsTrain(o.profileId);
      this.counts = new Map(o.top.map((t) => [t.key, t.count] as const));
      this.total = o.top.reduce((s, t) => s + t.count, 0);
      return true;
    } catch {
      return false;
    }
  }

  reset(): void {
    this.counts = new Map();
    this.total = 0;
    this.clamped = 0;
  }
}
