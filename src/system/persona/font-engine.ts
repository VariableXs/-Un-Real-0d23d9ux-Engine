/**
 * F159 字体引擎深化 · name 表解析 + 回退链构建 + 批量扫描（分批后台协议）。
 *
 * 主册判据延伸：
 * - 【设计细节】「扫描超时（超大字体文件）→ 分批后台扫+先给部分结论」
 *   「等宽检测（终端用场景）同步显示」。
 * - name 表：字体家族名/子族名/双语名（nameID 1/2/16/17）——设置页字体列表
 *   与「预览示例文本」的数据源。
 */

import { scanFont, generalCharset, type FontCheckResult, type ScanInput } from "./fontguard";

// ---------- name 表解析（sfnt name 表 · 双语名） ----------

export interface FontNames {
  family: string;        // nameID 1
  subfamily: string;     // nameID 2
  fullName: string;      // nameID 4
  preferredFamily?: string;   // nameID 16
  preferredSubfamily?: string; // nameID 17
  /** 双语（zh 族名优先——zh 平台记录 0x0804）。 */
  familyZh?: string;
}

const NAME_ID_FAMILY = 1;
const NAME_ID_SUBFAMILY = 2;
const NAME_ID_FULL = 4;
const NAME_ID_PREF_FAMILY = 16;
const NAME_ID_PREF_SUBFAMILY = 17;

/** sfnt 表目录定位 name 表偏移；失败返回 -1。 */
function findNameTableOffset(view: DataView): number {
  const numTables = view.getUint16(4);
  for (let i = 0; i < numTables; i++) {
    const rec = 12 + i * 16;
    const tag = String.fromCharCode(
      view.getUint8(rec), view.getUint8(rec + 1), view.getUint8(rec + 2), view.getUint8(rec + 3),
    );
    if (tag === "name") return view.getUint32(rec + 8);
  }
  return -1;
}

/** 解析 name 表（format 0）：优先 zh-CN 记录，回退 en-US，再回退首个。 */
export function parseFontNames(buf: ArrayBuffer): FontNames | null {
  const v = new DataView(buf);
  try {
    const nameOff = findNameTableOffset(v);
    if (nameOff < 0) return null;
    const count = v.getUint16(nameOff + 2);
    const stringOffset = nameOff + v.getUint16(nameOff + 4);
    // 记录结构：platform(0) encoding(2) language(4) nameID(6) length(8) offset(10)。
    interface Rec { platform: number; language: number; nameId: number; length: number; offset: number }
    const records: Rec[] = [];    for (let i = 0; i < count; i++) {
      const base = nameOff + 6 + i * 12;
      records.push({
        platform: v.getUint16(base),
        language: v.getUint16(base + 4),
        nameId: v.getUint16(base + 6),
        length: v.getUint16(base + 8),
        offset: v.getUint16(base + 10),
      });
    }
    const decode = (r: Rec): string => {
      const bytes = new Uint8Array(buf, stringOffset + r.offset, r.length);
      if (r.platform === 0 || r.platform === 3) {
        // UTF-16BE
        let s = "";
        for (let i = 0; i + 1 < bytes.length; i += 2) {
          s += String.fromCharCode((bytes[i]! << 8) | bytes[i + 1]!);
        }
        return s;
      }
      // platform 1（Mac Roman）简化按 latin——判据只需主要平台。
      let s = "";
      for (const b of bytes) s += String.fromCharCode(b);
      return s;
    };
    const pick = (nameId: number, zh: boolean): string | undefined => {
      const candidates = records.filter((r) => r.nameId === nameId && r.length > 0);
      if (candidates.length === 0) return undefined;
      const langTarget = zh ? 0x0804 : 0x0409;
      const found: Rec | undefined = candidates.find((r) => r.language === langTarget) ?? candidates[0];
      return found ? decode(found) : undefined;
    };
    const names: FontNames = {
      family: pick(NAME_ID_FAMILY, true) ?? pick(NAME_ID_FAMILY, false) ?? "",
      subfamily: pick(NAME_ID_SUBFAMILY, true) ?? pick(NAME_ID_SUBFAMILY, false) ?? "",
      fullName: pick(NAME_ID_FULL, true) ?? pick(NAME_ID_FULL, false) ?? "",
      preferredFamily: pick(NAME_ID_PREF_FAMILY, true),
      preferredSubfamily: pick(NAME_ID_PREF_SUBFAMILY, true),
      familyZh: pick(NAME_ID_FAMILY, true),
    };
    if (!names.family) return null;
    return names;
  } catch {
    return null;
  }
}

// ---------- 回退链构建（缺字时逐级回退的确定性顺序） ----------

export interface FallbackChain {
  /** 逐级字体栈（第一个是用户选择）。 */
  stack: string[];
  /** 每级负责补的字符集说明（诊断呈现）。 */
  notes: string[];
}

/** 回退链：用户字体 → 系统中文栈 → 系统西文栈 → 最后兜底（永不落空）。 */
export function buildFallbackChain(userFont: string, systemFonts: { cjk: string; latin: string; fallback: string }): FallbackChain {
  const stack = [userFont];
  const notes = ["用户选择（缺字率判定来源）"];
  if (systemFonts.cjk !== userFont) {
    stack.push(systemFonts.cjk);
    notes.push("系统中文栈（补 CJK 缺字）");
  }
  if (systemFonts.latin !== userFont && !stack.includes(systemFonts.latin)) {
    stack.push(systemFonts.latin);
    notes.push("系统西文栈（补拉丁/符号）");
  }
  if (!stack.includes(systemFonts.fallback)) {
    stack.push(systemFonts.fallback);
    notes.push("最后兜底（保证可渲染——永不落空）");
  }
  return { stack, notes };
}

// ---------- 批量扫描协议（分批后台扫 + 部分结论） ----------

export interface BatchScanState {
  fontId: string;
  totalBatches: number;
  completedBatches: number;
  /** 部分结论（已完成批的缺字率——先给部分）。 */
  partialMissingRate: number | null;
  done: boolean;
  /** 超时判据：单批 >500ms 触发让步（主线程保护——F209 性能纪律）。 */
  lastBatchMs: number;
}

export const BATCH_SIZE = 500; // 每批字符数
export const BATCH_YIELD_MS = 500;

/** 分批扫描推进器：每 tick 处理一批，随时可取部分结论。 */
export class BatchFontScanner {
  private state: BatchScanState;
  private chars: string[];
  private missingSoFar = 0;

  constructor(private input: ScanInput, now = Date.now()) {
    // 扫描字符集：界面集优先（去重），空则回退通用集口径——与 scanFont 同源。
    const iface = input.interfaceChars && input.interfaceChars.length > 0 ? [...new Set(input.interfaceChars)] : generalCharset();
    this.chars = iface;
    const total = this.chars.length;
    this.state = {
      fontId: input.fontId,
      totalBatches: Math.ceil(total / BATCH_SIZE),
      completedBatches: 0,
      partialMissingRate: null,
      done: total === 0,
      lastBatchMs: 0,
    };
    void now;
  }

  get snapshot(): BatchScanState {
    return { ...this.state };
  }

  /** 处理下一批；返回是否还有剩余。 */
  step(): boolean {
    if (this.state.done) return false;
    const t0 = Date.now();
    const from = this.state.completedBatches * BATCH_SIZE;
    const batch = this.chars.slice(from, from + BATCH_SIZE);
    for (const ch of batch) {
      if (!this.input.covered.has(ch.codePointAt(0) ?? 0)) this.missingSoFar++;
    }
    this.state.completedBatches++;
    const scanned = Math.min(this.state.completedBatches * BATCH_SIZE, this.chars.length);
    this.state.partialMissingRate = scanned > 0 ? this.missingSoFar / scanned : null;
    this.state.lastBatchMs = Date.now() - t0;
    if (this.state.completedBatches >= this.state.totalBatches) {
      this.state.done = true;
      return false;
    }
    return true;
  }

  /** 全部完成 → 完整结论（复用 scanFont 判定口径——一处一事实）。 */
  finalResult(): FontCheckResult | null {
    if (!this.state.done) return null;
    const r = scanFont(this.input);
    return { ...r, monospace: false, complete: true };
  }
}

/** 同步便捷入口（小字体文件——单批内完成）。 */
export function scanFontSync(input: ScanInput): FontCheckResult & { monospace: boolean; complete: boolean } {
  return { ...scanFont(input), monospace: false, complete: true };
}
