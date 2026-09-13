/**
 * UNREAL-X AI-04 · 启动收官与遥测（族0031/0032/0033/0035/0037/0038/0039/0040 的
 * V 线承载 · X00751~X01000）。kernel/varix/src/telemetry.rs（K 线环形缓冲）
 * 与 code-analysis/core/src/ai04.rs（C 线 CheckSet）的桌面消费端模型。
 *
 * 本地遥测：事件名 ≤8 字节、无内容字段（脱敏红线）；环形容量 16 满载挤最旧；
 * 基线库只增不删（回滚守卫）；毕业礼 = G1~G4 四门禁。
 */

// ---- 族0031 启动遥测 ----

export interface TelemetryEvent {
  name: string;
  ms: number;
}

export const TELEMETRY_CAP = 16;
export const TELEMETRY_NAME_MAX = 8;

export class BootTelemetry {
  private events: TelemetryEvent[] = [];
  dropped = 0;

  record(name: string, ms: number): void {
    const safe = { name: name.slice(0, TELEMETRY_NAME_MAX), ms: Math.max(0, Math.round(ms)) };
    if (this.events.length === TELEMETRY_CAP) {
      this.events.shift();
      this.dropped += 1;
    }
    this.events.push(safe);
  }

  get size(): number {
    return this.events.length;
  }

  spanMs(): number {
    return this.events.reduce((acc, e) => acc + e.ms, 0);
  }

  worstMs(): number {
    return this.events.reduce((acc, e) => Math.max(acc, e.ms), 0);
  }

  percentile(p: number): number {
    if (this.events.length === 0) return 0;
    const ms = this.events.map((e) => e.ms).sort((a, b) => a - b);
    const idx = Math.min(ms.length - 1, Math.floor(((ms.length - 1) * Math.min(100, Math.max(0, p))) / 100));
    return ms[idx]!;
  }

  timeline(): readonly TelemetryEvent[] {
    return this.events;
  }
}

// ---- 族0032 失败学习 ----

export class FailureCluster {
  private buckets = new Map<string, number>();

  record(code: string): void {
    this.buckets.set(code, (this.buckets.get(code) ?? 0) + 1);
  }

  total(): number {
    return [...this.buckets.values()].reduce((a, b) => a + b, 0);
  }

  /** 热点：≥3 次的错误码，按频次降序。 */
  hotspots(): string[] {
    return [...this.buckets.entries()]
      .filter(([, n]) => n >= 3)
      .sort((a, b) => b[1] - a[1])
      .map(([c]) => c);
  }

  top(): { code: string; count: number } | undefined {
    let best: { code: string; count: number } | undefined;
    for (const [code, count] of this.buckets) {
      if (!best || count > best.count || (count === best.count && code < best.code)) best = { code, count };
    }
    return best;
  }
}

// ---- 族0033 基线库 ----

export interface BaselineMetric {
  name: string;
  base: number;
  tol: number;
}

export class BaselineLibrary {
  constructor(private metrics: Map<string, BaselineMetric> = new Map()) {}

  define(metric: BaselineMetric): boolean {
    if (this.metrics.has(metric.name)) return false; // 只增不删，重复定义拒绝
    this.metrics.set(metric.name, metric);
    return true;
  }

  regressed(name: string, actual: number): boolean {
    const m = this.metrics.get(name);
    return m !== undefined && actual > m.base + m.tol;
  }

  improved(name: string, actual: number): boolean {
    const m = this.metrics.get(name);
    return m !== undefined && actual < m.base;
  }

  snapshot(): readonly BaselineMetric[] {
    return [...this.metrics.values()];
  }
}

/** 默认启动基线（boot ≤360±40ms / mem ≤256±16MB / fps=60±0）。 */
export function defaultBaseline(): BaselineLibrary {
  const lib = new BaselineLibrary();
  lib.define({ name: "boot_ms", base: 360, tol: 40 });
  lib.define({ name: "mem_mb", base: 256, tol: 16 });
  lib.define({ name: "fps", base: 60, tol: 0 });
  return lib;
}

// ---- 族0035 文档剧场 ----

const DOC_CHAPTERS_ZH = ["第一章 · 冷启动链路", "第二章 · 品牌剧场", "第三章 · 自检与修复", "第四章 · 进入桌面", "附录 · 遥测与基线"] as const;
const DOC_CHAPTERS_EN = ["Ch.1 Cold Boot Chain", "Ch.2 Brand Theater", "Ch.3 Self-test & Repair", "Ch.4 Enter Desktop", "Appendix Telemetry & Baseline"] as const;

export function docChapter(stage: number): { zh: string; en: string } {
  const idx = stage >= 0 && stage < DOC_CHAPTERS_ZH.length ? stage : 4;
  return { zh: DOC_CHAPTERS_ZH[idx]!, en: DOC_CHAPTERS_EN[idx]! };
}

export function docToc(stages: readonly number[]): string[] {
  return stages.map((s) => docChapter(s).zh);
}

// ---- 族0037 回忆录 ----

export interface MemoirEntry {
  day: number;
  text: string;
}

export class Memoir {
  private entries: MemoirEntry[] = [];

  add(day: number, text: string): void {
    if (!this.entries.some((e) => e.day === day && e.text === text)) this.entries.push({ day, text });
  }

  timeline(): string[] {
    return [...this.entries].sort((a, b) => a.day - b.day).map((e) => `D${e.day}: ${e.text}`);
  }

  get size(): number {
    return this.entries.length;
  }
}

// ---- 族0038 毕业礼 ----

export const GRADUATION_GATES = ["G1 族内自检", "G2 域验收", "G3 波次门禁", "G4 终验收"] as const;

export class Graduation {
  constructor(private gates: [boolean, boolean, boolean, boolean] = [false, false, false, false]) {}

  pass(index: 0 | 1 | 2 | 3): void {
    this.gates[index] = true;
  }

  get passed(): number {
    return this.gates.filter(Boolean).length;
  }

  get ready(): boolean {
    return this.passed === 4;
  }

  message(): string {
    return this.ready ? "毕业快乐，Unreal X 计划交付" : "门禁未齐，继续努力";
  }
}

// ---- 族0039 档案馆 ----

export class Archive {
  private shelves = new Map<string, number[]>();

  file(shelf: string, xid: number): void {
    const list = this.shelves.get(shelf) ?? [];
    if (!list.includes(xid)) list.push(xid);
    this.shelves.set(shelf, list);
  }

  find(xid: number): string | undefined {
    for (const [shelf, list] of this.shelves) if (list.includes(xid)) return shelf;
    return undefined;
  }

  get total(): number {
    return [...this.shelves.values()].reduce((a, b) => a + b.length, 0);
  }

  shelfNames(): string[] {
    return [...this.shelves.keys()];
  }
}

// ---- 族0040 大收官 ----

export interface FinaleResult {
  done: number;
  percent: number;
  verdict: string;
}

/** AI-03/AI-04 承载区间：X00501~X01000 共 500 项。 */
export const AI34_SCOPE = { from: 501, to: 1000, total: 500 } as const;

export function finale(done: number, planned: number): FinaleResult {
  const d = Math.min(Math.max(0, Math.round(done)), planned);
  const percent = planned > 0 ? (d / planned) * 100 : 0;
  const verdict = planned === 0 ? "无计划" : d === planned ? "15000 全绿，基线冻结" : percent >= 50 ? "过半，继续推进" : "起步阶段";
  return { done: d, percent, verdict };
}
