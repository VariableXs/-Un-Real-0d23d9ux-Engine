/**
 * 十章一致性 + 十二章防丢失 + 十四章开放性 · 交叉对拍与守护三件套。
 *
 * - format 对拍：各引擎导出函数的 format 字段 ↔ 注册表（一处一事实）；
 * - 词典静态扫描：页组源码字面量违例扫描（调用方喂入内容——引擎零 fs 依赖）；
 * - 自动保存守护：脏标记 + 节拍保存 + 关窗提示 + 崩溃草稿（十二章防丢失）。
 */

import { EXPORT_FORMATS } from "./export-formats";

// ---------- format 对拍（生产 ↔ 注册表） ----------

export interface FormatSample {
  /** 生产模块名（对拍键）。 */
  producer: string;
  /** 该模块导出产物里的 format 字段值。 */
  formatValue: string;
}

export interface FormatCrosscheckVerdict {
  matched: Array<{ producer: string; formatValue: string }>;
  /** 生产了注册表没有的格式（未登记——开放性红线）。 */
  unregistered: Array<{ producer: string; formatValue: string }>;
  /** 注册表有但无法对拍的（缺样本——机检覆盖不足显性化）。 */
  unsampled: string[];
  ok: boolean;
}

/** 对拍：生产样本逐条查注册表；注册表 active 格式抽查覆盖。 */
export function crosscheckFormats(samples: FormatSample[]): FormatCrosscheckVerdict {
  const matched: FormatCrosscheckVerdict["matched"] = [];
  const unregistered: FormatCrosscheckVerdict["unregistered"] = [];
  for (const s of samples) {
    const hit = EXPORT_FORMATS.find((f) => f.id === s.formatValue);
    if (hit) matched.push({ producer: s.producer, formatValue: s.formatValue });
    else unregistered.push({ producer: s.producer, formatValue: s.formatValue });
  }
  const sampledProducers = new Set(samples.map((s) => s.producer));
  const unsampled = EXPORT_FORMATS.filter((f) => f.status === "active" && !sampledProducers.has(f.producer)).map((f) => f.id);
  return { matched, unregistered, unsampled, ok: unregistered.length === 0 };
}

// ---------- 词典静态扫描（页组源码字面量违例） ----------

export interface ScanRule {
  id: string;
  /** 违例模式（正则——字面量扫描的确定性口径）。 */
  pattern: RegExp;
  ruleId: string;
  what: string;
}

/** 字面量扫描规则（词典 D-* 规则的可扫子集——数据驱动可扩展）。 */
export const SCAN_RULES: ScanRule[] = [
  { id: "S-CONFIRM-OK", pattern: />(确定|OK)</, ruleId: "D-CONFIRM-01", what: "确认钮写了'确定/OK'——动词不具体（禁词表同源）" },
  { id: "S-CONFIRM-POS", pattern: /确定.{0,12}取消|OK.{0,12}Cancel/i, ruleId: "D-CONFIRM-01", what: "确定在前取消在后——取消必须在安全侧" },
  { id: "S-EMPTY-PLAIN", pattern: /(暂无|没有)数据</, ruleId: "D-EMPTY-01", what: "空态只写'暂无数据'——缺标题/引导/入口三件套" },
  { id: "S-ERR-CODE", pattern: /错误码?[:：]\s*\d+/, ruleId: "D-LOADING-01", what: "裸异常码直接呈现——应三要素+详情折叠" },
];

export interface ScanFinding {
  ruleId: string;
  file: string;
  line: number;
  what: string;
  snippet: string;
}

/** 静态扫描：逐文件逐行匹配（调用方喂入 {文件名: 内容}——引擎零 fs 依赖，CI 与页面都可消费）。 */
export function scanSources(files: Record<string, string>): { findings: ScanFinding[]; scannedFiles: number; scannedLines: number } {
  const findings: ScanFinding[] = [];
  let scannedLines = 0;
  for (const [file, content] of Object.entries(files)) {
    const lines = content.split("\n");
    scannedLines += lines.length;
    lines.forEach((line, i) => {
      for (const rule of SCAN_RULES) {
        if (rule.pattern.test(line)) {
          findings.push({ ruleId: rule.ruleId, file, line: i + 1, what: rule.what, snippet: line.trim().slice(0, 60) });
        }
      }
    });
  }
  return { findings, scannedFiles: Object.keys(files).length, scannedLines };
}

// ---------- 自动保存守护（十二章防丢失的节拍器） ----------

export type SavePhase = "clean" | "dirty" | "saving" | "saved" | "save-failed";

export interface AutosaveState {
  phase: SavePhase;
  /** 脏时刻（关窗提示的依据）。 */
  dirtySince: number | null;
  /** 最近一次成功保存时刻。 */
  lastSavedAt: number | null;
  /** 节拍计数（防抖 5s + 强制 2min 的双节拍执行记录）。 */
  saves: number;
}

export const AUTOSAVE_DEBOUNCE_MS = 5_000;
export const AUTOSAVE_FORCE_MS = 120_000;

export function initialAutosave(): AutosaveState {
  return { phase: "clean", dirtySince: null, lastSavedAt: null, saves: 0 };
}

/** 变更登记（clean→dirty——脏时刻开始计时）。 */
export function markDirty(s: AutosaveState, now: number): AutosaveState {
  if (s.phase === "dirty" || s.phase === "saving") return s;
  return { ...s, phase: "dirty", dirtySince: now };
}

/** 节拍判定：脏超 5s → 该节拍保存；脏超 2min → 强制保存（防丢失优先于防抖）。 */
export function dueSave(s: AutosaveState, now: number): { due: boolean; forced: boolean; note: string } {
  if (s.phase !== "dirty" || s.dirtySince === null) return { due: false, forced: false, note: "无脏数据" };
  const age = now - s.dirtySince;
  if (age >= AUTOSAVE_FORCE_MS) return { due: true, forced: true, note: "脏超 2 分钟——强制保存（防丢失优先）" };
  if (age >= AUTOSAVE_DEBOUNCE_MS) return { due: true, forced: false, note: "脏超 5 秒——节拍保存" };
  return { due: false, forced: false, note: `防抖中（${Math.round(age / 1000)}s/5s）` };
}

/** 保存完成（saved/failed 显性——失败给三要素，不静默）。 */
export function completeSave(s: AutosaveState, ok: boolean, now: number): AutosaveState {
  if (ok) return { phase: "saved", dirtySince: null, lastSavedAt: now, saves: s.saves + 1 };
  return { ...s, phase: "save-failed" };
}

/** 关窗拦截判定：dirty/saving/save-failed = 有未保存内容（提示而不是静默丢）。 */
export function needsCloseConfirmation(s: AutosaveState): boolean {
  return s.phase === "dirty" || s.phase === "saving" || s.phase === "save-failed";
}

/** 崩溃恢复：草稿快照（脏数据即时镜像——重启能找回）。 */
export interface DraftSnapshot {
  takenAt: number;
  dirtyAgeMs: number;
  note: string;
}

export function draftSnapshot(s: AutosaveState, now: number): DraftSnapshot | null {
  if (s.dirtySince === null) return null;
  return { takenAt: now, dirtyAgeMs: now - s.dirtySince, note: "草稿已镜像——崩溃后重启从此恢复（用户永远不必重做工作）" };
}
