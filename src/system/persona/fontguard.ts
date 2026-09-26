/**
 * F159 字体安全档 · 完整设计。
 *
 * 主册判据：三档判定阈值实测（构造 0%/8%/20% 缺字字体样本）；缺字清单前 20 字展示
 * 准确；强行应用后回退路径可用。
 *
 * 【功能定义】换字体前自动兼容性预检：缺字率扫描（常用 3500 字+界面词条双集）、
 * 字号档位适配检查；缺字率 >5% 黄色警告、>15% 红色劝阻（可强行但显式确认）。
 *
 * 【状态与异常】扫描超时（超大字体文件）→ 分批后台扫+先给部分结论；字体文件损坏
 * → 拒绝导入三要素；等宽检测（终端用场景）同步显示。
 *
 * 【设计细节】双集定义：通用集 3500 常用汉字+ASCII+高频符号；界面集=当前语言词条
 * 去重字符集（F140 联动——更精准）；缺字率=缺字数/界面集（界面优先口径）；扫描走
 * SIMD（3500 字 <100ms）；等宽判定=等宽数字与字母 advance 一致性。
 */

import { personaStore } from "./store";

export const SECTION = "font";

export const WARN_THRESHOLD = 0.05;  // >5% 黄色警告
export const DANGER_THRESHOLD = 0.15; // >15% 红色劝阻
export const MISSING_LIST_PREVIEW = 20;
export const GENERAL_SET_SIZE = 3500; // 通用集：3500 常用汉字

export type FontVerdict = "ok" | "warn" | "danger";

export interface FontCheckResult {
  fontId: string;
  /** 各集缺字率。 */
  generalMissingRate: number;
  interfaceMissingRate: number;
  /** 判定口径 = 界面集优先。 */
  verdict: FontVerdict;
  /** 缺字清单（前 20 展示口径，内部存全量）。 */
  missingChars: string[];
  /** 是否等宽（终端场景）。 */
  monospace: boolean;
  /** 扫描是否完整（分批后台扫时 false——先给部分结论）。 */
  complete: boolean;
}

export interface FontGuardConfig {
  /** 当前界面字体（null=系统默认）。 */
  activeFontId: string | null;
  /** 预检结果缓存（同字体不重扫）。 */
  cache: Record<string, FontCheckResult>;
  /** 强行应用记录（「我了解缺字影响」勾选后入账——显式确认留痕）。 */
  forceApplied: Record<string, string>; // fontId → 日期
}

export function defaultFontGuardConfig(): FontGuardConfig {
  return { activeFontId: null, cache: {}, forceApplied: {} };
}

export function loadFontGuardConfig(): FontGuardConfig {
  const stored = personaStore.getWith(SECTION, "guard", undefined) as Partial<FontGuardConfig> | undefined;
  return { ...defaultFontGuardConfig(), ...(stored ?? {}) };
}

export function saveFontGuardConfig(c: FontGuardConfig): void {
  personaStore.set(SECTION, { guard: c });
}

// ---------- 常用字集（通用集骨架：ASCII + 高频符号 + 常用汉字采样源） ----------

/** ASCII + 高频全角符号。 */
export function baseCharset(): string[] {
  const out: string[] = [];
  for (let i = 0x20; i <= 0x7e; i++) out.push(String.fromCodePoint(i)); // ASCII 可打印 95
  // 高频全角符号（含引号类字面用码点书写，避免引号嵌套歧义）。
  for (const ch of "\uFF0C\u3002\u3001\uFF1B\uFF1A\uFF1F\uFF01\u201C\u201D\u2018\u2019\uFF08\uFF09\u300A\u300B\u3010\u3011\u00B7\u2014\u2026\uFF05\uFFE5\u2103\u00B0\u00D7\u00F7\u00B1") out.push(ch);
  return out;
}

/**
 * 通用集 3500 常用汉字：按 Unicode 块顺序取常用区间样本。生产环境由 F055 预热表
 * 注入（一处一事实）；本实现为纯逻辑层的确定性采样（同样的 3500 字集合口径），
 * 覆盖 GB2312 一级字库区段（0x4E00 起按频率排序表的前 3500 字由调用方注入，
 * 内置兜底为「一/丁/三…」级联采样——测试以注入表为准）。
 */
export function generalCharset(injected?: string[]): string[] {
  if (injected && injected.length > 0) {
    const set = new Set(injected);
    return [...set];
  }
  // 兜底：CJK 统一表意文字区前 3500 码位采样（U+4E00..），保证集合大小恒定。
  const out: string[] = [];
  for (let i = 0; out.length < GENERAL_SET_SIZE && i < 0x9fff - 0x4e00; i++) {
    out.push(String.fromCodePoint(0x4e00 + i));
  }
  return out;
}

// ---------- cmap 解析（最小 OpenType/TrueType cmap 子集，纯函数） ----------

export interface CmapParseResult {
  ok: boolean;
  /** 支持的码点集合（cmap 中的字符）；失败时为空。 */
  covered: Set<number>;
  reason: string;
}

/**
 * 解析 sfnt 字体的 cmap format 4（BMP 段）——3500 常用汉字全在 BMP 内，
 * format 4 覆盖判据足够；错误路径三要素由调用方包装。
 */
export function parseCmapFormat4(buf: ArrayBuffer): CmapParseResult {
  const view = new DataView(buf);
  const covered = new Set<number>();
  try {
    if (buf.byteLength < 12) return { ok: false, covered, reason: "字体文件过短，不是合法 sfnt" };
    const numTables = view.getUint16(4);
    let cmapOff = -1;
    for (let i = 0; i < numTables; i++) {
      const rec = 12 + i * 16;
      if (rec + 16 > buf.byteLength) return { ok: false, covered, reason: "表目录越界，文件损坏" };
      const tag = String.fromCharCode(
        view.getUint8(rec), view.getUint8(rec + 1), view.getUint8(rec + 2), view.getUint8(rec + 3),
      );
      if (tag === "cmap") cmapOff = view.getUint32(rec + 8);
    }
    if (cmapOff < 0 || cmapOff + 4 > buf.byteLength) return { ok: false, covered, reason: "缺少 cmap 表" };
    const nSub = view.getUint16(cmapOff + 2);
    let subOff = -1;
    for (let i = 0; i < nSub; i++) {
      const rec = cmapOff + 4 + i * 8;
      const platform = view.getUint16(rec);
      const enc = view.getUint16(rec + 2);
      if ((platform === 3 && (enc === 1 || enc === 10)) || platform === 0) {
        subOff = view.getUint32(rec + 4);
        if (view.getUint16(cmapOff + subOff) === 4) break; // 优先 format 4
      }
    }
    if (subOff < 0) return { ok: false, covered, reason: "无受支持的 cmap 子表（需 format 4）" };
    const t = cmapOff + subOff;
    const segCountX2 = view.getUint16(t + 6);
    const segCount = segCountX2 / 2;
    const endCodesOff = t + 14;
    const startCodesOff = endCodesOff + segCountX2 + 2;
    const deltasOff = startCodesOff + segCountX2;
    for (let i = 0; i < segCount; i++) {
      const end = view.getUint16(endCodesOff + i * 2);
      const start = view.getUint16(startCodesOff + i * 2);
      const delta = view.getInt16(deltasOff + i * 2);
      if (start === 0xffff) continue;
      for (let c = start; c <= end && c !== 0xffff; c++) {
        covered.add(c + delta);
      }
    }
    return { ok: true, covered, reason: "cmap format 4 解析成功" };
  } catch (e) {
    return { ok: false, covered, reason: `字体文件损坏: ${String(e)}` };
  }
}

// ---------- 缺字扫描（纯函数核心） ----------

export interface ScanInput {
  fontId: string;
  /** 字体覆盖的码点集合（parseCmapFormat4 输出，或字体包元数据自带覆盖度声明）。 */
  covered: Set<number>;
  /** 界面集字符（F140 词条去重；空则回退通用集口径）。 */
  interfaceChars?: string[];
  /** 通用集注入（F055 预热表）。 */
  generalChars?: string[];
}

/** 缺字率 = 缺字数 / 界面集（界面优先口径）。 */
export function scanFont(input: ScanInput): Omit<FontCheckResult, "monospace" | "complete"> {
  const general = generalCharset(input.generalChars);
  const base = baseCharset();
  const generalAll = [...general, ...base];
  const iface = input.interfaceChars && input.interfaceChars.length > 0 ? [...new Set(input.interfaceChars)] : generalAll;

  const missingGeneral: string[] = [];
  for (const ch of generalAll) {
    if (!input.covered.has(ch.codePointAt(0) ?? 0)) missingGeneral.push(ch);
  }
  const missingIface: string[] = [];
  for (const ch of iface) {
    if (!input.covered.has(ch.codePointAt(0) ?? 0)) missingIface.push(ch);
  }
  const generalMissingRate = generalAll.length > 0 ? missingGeneral.length / generalAll.length : 0;
  const interfaceMissingRate = iface.length > 0 ? missingIface.length / iface.length : 0;
  const rate = interfaceMissingRate; // 界面优先口径
  const verdict: FontVerdict = rate > DANGER_THRESHOLD ? "danger" : rate > WARN_THRESHOLD ? "warn" : "ok";
  return {
    fontId: input.fontId,
    generalMissingRate,
    interfaceMissingRate,
    verdict,
    missingChars: missingIface,
  };
}

/** 判定阈值（三档——主册：>5% 黄、>15% 红）。 */
export function verdictFor(rate: number): FontVerdict {
  return rate > DANGER_THRESHOLD ? "danger" : rate > WARN_THRESHOLD ? "warn" : "ok";
}

/** 等宽判定：等宽数字与字母 advance 一致性（字形 advance 表由调用方注入）。 */
export function monospaceCheck(advances: Record<string, number>, probe = "0123456789ABCDEFabcil"): boolean {
  const vals = [...probe].map((c) => advances[c]).filter((v) => v !== undefined);
  if (vals.length < 2) return false;
  const first = vals[0] ?? 0;
  return vals.every((v) => Math.abs(v - first) < 0.01);
}

/** 强行应用记录（显式确认留痕；回退路径由 undoSection 提供）。 */
export function recordForceApply(config: FontGuardConfig, fontId: string, dateKey: string): FontGuardConfig {
  return { ...config, forceApplied: { ...config.forceApplied, [fontId]: dateKey } };
}
