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
 * FNV-1a 哈希用于开关前后一致性自证。
 */
export function contentHash(text: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16);
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
