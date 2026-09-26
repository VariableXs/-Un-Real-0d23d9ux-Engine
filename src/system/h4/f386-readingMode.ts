/**
 * F386 阅读模式（H 域 · AI-H4）：
 * 长文阅读的系统级舒适档：任何文本窗口（记事本/帮助中心 F119/文本预览 F339）可开
 * 阅读模式——行距放大 1.6 倍、页宽限制 45 字符/行（居中，防扫视串行）、衬线可选字体；
 * 纯排版调整不动内容（退出模式原文原样）。
 * 判据（主册 F386）：三参数（行距 1.6/45 字/衬线可选）实测；内容零修改判据（开关前后
 * 文本哈希一致）；模式入口统一（查看菜单）；记忆（每应用记住开关态）。
 * 依赖锚点：F119 帮助中心 / F339 文本预览。
 * 存储键：variable:h4:f386（每应用记忆）。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";
import { fnv1a32 } from "./internal/hash";

/** 三参数（判据口径，一处定义）。 */
export const READING_SPEC = {
  lineHeight: 1.6,
  maxCharsPerLine: 45,
  serifOptional: true,
} as const;

export interface ReadingStyle {
  lineHeight: number;
  maxCharsPerLine: number;
  serif: boolean;
}

/** 默认排版（衬线关——可选判据）。 */
export function defaultStyle(): ReadingStyle {
  return { lineHeight: READING_SPEC.lineHeight, maxCharsPerLine: READING_SPEC.maxCharsPerLine, serif: false };
}

/** 衬线切换（可选判据）。 */
export function withSerif(style: ReadingStyle, serif: boolean): ReadingStyle {
  return { ...style, serif };
}

/**
 * 内容零修改（判据核心）：阅读模式只产排版参数，绝不触碰文本本体——
 * 本函数签名上就收不到「修改后的文本」这种东西（结构性保证）。
 * FNV-1a 哈希用于开关前后一致性自证（实现委托 internal/hash 单点——v4 门禁收编私抄件）。
 */
export function contentHash(text: string): string {
  return fnv1a32(text);
}

/** 页宽换算：45 字符 → 容器宽度（CJK 按全角 1em、ASCII 半角 0.5em 估算）。 */
export function pageWidthPx(style: ReadingStyle, fontSizePx: number, text: string): number {
  const sample = text.slice(0, 200);
  const wide = (sample.match(/[^\x00-\xff]/g) ?? []).length;
  const narrow = sample.length - wide;
  const charEm = sample.length === 0 ? 1 : (wide + narrow * 0.5) / sample.length;
  return Math.round(style.maxCharsPerLine * charEm * fontSizePx);
}

/** 行高换算：1.6 倍 × 字号。 */
export function lineHeightPx(style: ReadingStyle, fontSizePx: number): number {
  return Math.round(style.lineHeight * fontSizePx * 10) / 10;
}

/* ---------- 每应用记忆（判据「每应用记住开关态」） ---------- */

const KEY = h4Key("f386", "apps");

type AppMemory = Record<string, { on: boolean; serif: boolean }>;

function isMemory(v: unknown): v is AppMemory {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function load(store: KvStore): AppMemory {
  return readJson<AppMemory>(store, KEY, {}, isMemory);
}

/** 记住应用开关态。 */
export function setAppMode(appId: string, on: boolean, serif = false, store: KvStore = defaultStore()): boolean {
  const mem = load(store);
  return writeJson(store, KEY, { ...mem, [appId]: { on, serif } });
}

/** 读取应用开关态（默认关）。 */
export function appMode(appId: string, store: KvStore = defaultStore()): { on: boolean; serif: boolean } {
  return load(store)[appId] ?? { on: false, serif: false };
}

/** 入口统一（判据）：所有文本窗口共用同一入口 id（查看菜单 → 阅读模式）。 */
export const READING_MODE_ENTRY = "view-menu.reading-mode";
export function unifiedEntry(): string {
  return READING_MODE_ENTRY;
}

/* ================= v5 深化批次五：段落重排视图 / 衬线字体栈 / 截词审计 ================= */

/** 段落重排视图（只产视图行、不碰原文——软换行是纯投影）：按 maxCharsPerLine 折行，CJK 逐字、英文词完整（不截词）。 */
export function reflowParagraphs(text: string, style: ReadingStyle): string[] {
  const lines: string[] = [];
  for (const para of text.split("\n")) {
    if (para.length === 0) {
      lines.push("");
      continue;
    }
    let cur = "";
    let curWidth = 0;
    for (const token of para.match(/[\x00-\xff]+|[^\x00-\xff]/g) ?? []) {
      const w = /[\x00-\xff]/.test(token) ? token.length * 0.5 : token.length;
      if (curWidth + w > style.maxCharsPerLine && cur.length > 0) {
        lines.push(cur);
        cur = token;
        curWidth = w;
      } else {
        cur += token;
        curWidth += w;
      }
    }
    lines.push(cur);
  }
  return lines;
}

/** 视图行零侵入审计：软换行只增换行符——剥掉全部换行后与原文逐字一致（折行不增删字符）。 */
export function auditViewIntegrity(text: string, style: ReadingStyle): { pass: boolean; hashBefore: string; hashAfter: string } {
  const lines = reflowParagraphs(text, style);
  const flatten = (s: string) => s.replace(/\n/g, "");
  const joined = lines.join("\n");
  return { pass: flatten(joined) === flatten(text), hashBefore: contentHash(text), hashAfter: contentHash(joined) };
}

/** 衬线字体族解析（判据「衬线可选」的落地面）：衬线开走宋体系、关走黑体系（栈一处定义）。 */
export const SERIF_STACK = '"Source Han Serif SC", "Noto Serif CJK SC", serif';
export const SANS_STACK = '"Source Han Sans SC", "Noto Sans CJK SC", sans-serif';
export function fontStackFor(style: ReadingStyle): string {
  return style.serif ? SERIF_STACK : SANS_STACK;
}

/** 截词审计：重排行内不允许出现被截断的 ASCII 词（词完整性 = 阅读体验底线）。 */
export function auditNoBrokenWords(lines: string[], original: string): { pass: boolean; broken: string[] } {
  const origWords = new Set(original.match(/[A-Za-z]+/g) ?? []);
  const broken: string[] = [];
  for (const line of lines) {
    for (const w of line.match(/[A-Za-z]+/g) ?? []) {
      if (!origWords.has(w) && w.length > 1 && !origWords.has(w.replace(/-$/, ""))) broken.push(w);
    }
  }
  return { pass: broken.length === 0, broken };
}
