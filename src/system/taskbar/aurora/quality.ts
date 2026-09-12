/**
 * AURORA-10000 领域04 · 族0100 任务栏工程质量（AI-20 批次，勿删）。
 * 预算/度量/看门狗/原子写/自愈/本地遥测/回归基线/诊断包/灰度/总控。
 */

/** 预算常量（F02476/77/95）。 */
export const BUDGETS = {
  taskbarMemoryMB: 50,
  panelFirstFrameMs: 200,
  frameMs: 16.7,
  bundleIncrementKB: 120,
} as const;

/** P95 度量（F02480）。 */
export function p95(samples: readonly number[]): number {
  if (samples.length === 0) return 0;
  const s = [...samples].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor(s.length * 0.95))] ?? 0;
}

/** 掉帧看门狗（F02478）：帧间隔超预算计数。 */
export class FrameWatchdog {
  private over = 0;
  private frames = 0;
  feed(deltaMs: number): void {
    this.frames++;
    if (deltaMs > BUDGETS.frameMs * 2) this.over++;
  }
  /** 掉帧率（0~1）。 */
  rate(): number { return this.frames === 0 ? 0 : this.over / this.frames; }
  reset(): void { this.over = 0; this.frames = 0; }
}

/** 缓存命中计数（F02479）。 */
export class CacheStats {
  hits = 0; misses = 0;
  hit(): void { this.hits++; }
  miss(): void { this.misses++; }
  rate(): number { const t = this.hits + this.misses; return t === 0 ? 0 : this.hits / t; }
}

/** 原子写（F02484）：先备份旧值再写主值。 */
export function atomicWrite(store: { getItem(k: string): string | null; setItem(k: string, v: string): void }, key: string, value: string): void {
  const prev = store.getItem(key);
  if (prev != null) store.setItem(key + ".bak", prev);
  store.setItem(key, value);
}

/** 配置自恢复（F02485）：解析失败回退备份，再失败回退默认。 */
export function selfHeal<T>(parse: (raw: string) => T | null, primary: string | null, backup: string | null, fallback: T): { value: T; source: "primary" | "backup" | "default" } {
  if (primary != null) {
    const v = parse(primary);
    if (v != null) return { value: v, source: "primary" };
  }
  if (backup != null) {
    const v = parse(backup);
    if (v != null) return { value: v, source: "backup" };
  }
  return { value: fallback, source: "default" };
}

/** 本地遥测环形缓冲（F02486：仅内存，默认开，不上传出站）。 */
export class LocalTelemetry {
  private buf: string[] = [];
  constructor(private cap = 500) {}
  record(event: string): void {
    this.buf.push(`${Date.now()}\t${event}`);
    if (this.buf.length > this.cap) this.buf.shift();
  }
  dump(): readonly string[] { return [...this.buf]; }
  clear(): void { this.buf = []; }
}

/** 性能回归基线（F02487）：metric → 基线值。 */
export class RegressionBaseline {
  private base = new Map<string, number>();
  setBaseline(metric: string, value: number): void { this.base.set(metric, value); }
  /** 超预算（>基线×1.15）返回 true。 */
  exceeded(metric: string, current: number): boolean {
    const b = this.base.get(metric);
    return b != null ? current > b * 1.15 : false;
  }
  snapshot(): Record<string, number> { return Object.fromEntries(this.base); }
}

/** 诊断包（F02498）：聚合指标与遥测。 */
export function diagBundle(parts: { metrics?: Record<string, number>; telemetry?: readonly string[]; errors?: readonly string[] }): string {
  return JSON.stringify({ ts: new Date().toISOString(), ...parts }, null, 2);
}

/** 灰度开关（F02499）：功能 ID → 百分比。 */
export function grayEnabled(flag: string, percent: number, seed: string): boolean {
  // 稳定哈希：同一 seed 结果一致。
  let h = 0;
  const s = flag + ":" + seed;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0;
  return (h % 100) < Math.round(percent);
}
