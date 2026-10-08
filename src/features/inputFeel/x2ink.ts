/**
 * UNREAL-X AI-19 · 内核输入栈 V 线逻辑核（族0181~0190 · X04501~X04750 代表性模型）。
 * 与 kernel/varix/src/inkstack.rs、code-analysis/core/src/ai19.rs 同口径：
 * 纯逻辑、非法输入钳制、绝不抛异常。勿删。
 */

/* -------- 族0181 驱动抽象 -------- */
export const DRV_KBD = 1;
export const DRV_PTR = 2;
export const DRV_TOUCH = 4;
export const DRV_CKEY = 8;

export interface DriverSlot { name: string; caps: number; probeOrder: number; present: boolean; }

export function probe(drivers: DriverSlot[]): string[] {
  return drivers
    .map((d, i) => ({ d, i }))
    .filter(({ d }) => d.present)
    .sort((a, b) => a.d.probeOrder - b.d.probeOrder || a.i - b.i)
    .map(({ d }) => d.name);
}

export function hasCaps(slot: DriverSlot, want: number): boolean {
  return (slot.caps & want) === want;
}

export function fallbackPick(drivers: DriverSlot[], want: number): string | null {
  const names = probe(drivers);
  for (const n of names) {
    const d = drivers.find((x) => x.name === n);
    if (d && hasCaps(d, want)) return n;
  }
  return null;
}

/* -------- 族0182 事件管线 -------- */
export interface Ev { kind: number; code: number; tMs: number; }

export class EvRing {
  static CAP = 16;
  private buf: (Ev | null)[] = new Array(EvRing.CAP).fill(null);
  private head = 0;
  private len = 0;
  dropped = 0;
  coalesce = true;
  get length(): number { return this.len; }
  push(e: Ev): boolean {
    if (this.coalesce && e.kind === 3 && this.len > 0) {
      const tail = (this.head + this.len - 1) % EvRing.CAP;
      const t = this.buf[tail];
      if (t && t.kind === 3 && t.code === e.code) { this.buf[tail] = e; return true; }
    }
    if (this.len === EvRing.CAP) { this.dropped += 1; return false; }
    this.buf[(this.head + this.len) % EvRing.CAP] = e;
    this.len += 1;
    return true;
  }
  pop(): Ev | null {
    if (this.len === 0) return null;
    const e = this.buf[this.head] ?? null;
    this.buf[this.head] = null;
    this.head = (this.head + 1) % EvRing.CAP;
    this.len -= 1;
    return e;
  }
}

/* -------- 族0183 重复率去抖 -------- */
export interface RepeatCfg { delayMs: number; periodMs: number; }
export const REPEAT_TIERS: RepeatCfg[] = [
  { delayMs: 500, periodMs: 60 },
  { delayMs: 400, periodMs: 50 },
  { delayMs: 300, periodMs: 40 },
  { delayMs: 250, periodMs: 33 },
  { delayMs: 200, periodMs: 25 },
];
export const DEFAULT_REPEAT = 2;

export function repeatCount(cfg: RepeatCfg, tMs: number): number {
  if (tMs < cfg.delayMs) return 0;
  return Math.floor((tMs - cfg.delayMs) / Math.max(1, cfg.periodMs)) + 1;
}
export function debounced(lastMs: number | null, nowMs: number, windowMs: number): boolean {
  if (lastMs === null) return false;
  return Math.max(0, nowMs - lastMs) < windowMs;
}
export function clampRepeatCfg(delayMs: number, periodMs: number): RepeatCfg {
  return { delayMs: Math.min(1000, Math.max(50, delayMs)), periodMs: Math.min(250, Math.max(15, periodMs)) };
}

/* -------- 族0184 组合键引擎 -------- */
export const MOD_CTRL = 1, MOD_SHIFT = 2, MOD_ALT = 4, MOD_WIN = 8;
export interface Chord { mods: number; key: number; }
export function chordMatch(a: Chord, b: Chord): boolean { return a.mods === b.mods && a.key === b.key; }
export function chordLabel(c: Chord): string {
  let s = '';
  if (c.mods & MOD_CTRL) s += 'Ctrl+';
  if (c.mods & MOD_SHIFT) s += 'Shift+';
  if (c.mods & MOD_ALT) s += 'Alt+';
  if (c.mods & MOD_WIN) s += 'Win+';
  return s + String(c.key);
}
export function chordConflict(table: Chord[], cand: Chord): boolean {
  return table.some((c) => chordMatch(c, cand));
}
export function seqProgress(progress: number, stepHit: boolean): number {
  return stepHit ? progress + 1 : 0;
}

/* -------- 族0185 输入法框架 -------- */
export const IME_IDLE = 0, IME_COMPOSING = 1, IME_CAND = 2, IME_COMMIT = 3;
export class Ime {
  state = IME_IDLE;
  buf = '';
  cands: string[] = [];
  sel = 0;
  typeKey(ch: string): void { this.state = IME_COMPOSING; this.buf += ch; }
  candidates(pool: string[]): void {
    this.cands = pool.filter((w) => w.startsWith(this.buf)).slice(0, 9);
    this.state = this.cands.length > 0 ? IME_CAND : IME_COMPOSING;
    this.sel = 0;
  }
  nav(delta: number): void {
    const n = this.cands.length;
    if (n === 0) return;
    this.sel = Math.min(n - 1, Math.max(0, this.sel + delta));
  }
  commit(): string | null {
    if (this.state !== IME_CAND) return null;
    const w = this.cands[this.sel] ?? null;
    this.state = IME_COMMIT;
    this.buf = '';
    this.cands = [];
    this.sel = 0;
    return w;
  }
  esc(): void { this.state = IME_IDLE; this.buf = ''; this.cands = []; }
}

/* -------- 族0186 无线延迟 -------- */
export interface LinkBudget { pollMs: number; retryMs: number; jitterMs: number; }
export function linkTotal(b: LinkBudget): number { return b.pollMs + b.retryMs + b.jitterMs; }
export function feelsWired(b: LinkBudget): boolean { return linkTotal(b) <= 16; }
export function jitterGrade(j: number): 'good' | 'fair' | 'poor' {
  return j <= 2 ? 'good' : j <= 5 ? 'fair' : 'poor';
}
export function p95(samples: number[]): number {
  if (samples.length === 0) return 0;
  const v = [...samples].sort((a, b) => a - b);
  return v[Math.min(v.length - 1, Math.floor(v.length * 0.95))] ?? 0;
}

/* -------- 族0187 缓冲回放 -------- */
export class ReplayLog {
  private events: [number, number][] = [];
  constructor(public cap: number) { this.cap = Math.min(32, Math.max(1, cap)); }
  get length(): number { return this.events.length; }
  record(code: number, tMs: number): boolean {
    if (this.events.length >= this.cap) return false;
    this.events.push([code, tMs]);
    return true;
  }
  replay(): number[] {
    return [...this.events].sort((a, b) => a[1] - b[1]).map((e) => e[0]);
  }
  chainHash(): number {
    let h = 0x811c9dc5;
    for (const [c, t] of this.events) {
      h = Math.imul(h ^ c, 0x01000193);
      h = Math.imul(h ^ (t % 0x100000000), 0x01000193);
    }
    return h >>> 0;
  }
  truncate(n: number): void { this.events = this.events.slice(0, Math.max(0, n)); }
}

/* -------- 族0188 输入功耗 -------- */
export const INPUT_POWER_TIERS = [800, 600, 450, 320, 220];
export function idleTier(idleMs: number): number {
  if (idleMs >= 60000) return 4;
  if (idleMs >= 10000) return 3;
  if (idleMs >= 5000) return 2;
  if (idleMs >= 1000) return 1;
  return 0;
}
export function wakePenalty(tier: number): number { return Math.min(4, tier) * 5; }
export function powerGuard(batteryPct: number, tier: number): number {
  return batteryPct < 20 ? 4 : Math.min(4, tier);
}
export function keyEnergy(tier: number, pressMs: number): number {
  return (INPUT_POWER_TIERS[Math.min(4, tier)] ?? 0) * pressMs;
}

/* -------- 族0189 输入基准（对齐 code-analysis ai19） -------- */
export interface BenchCase { name: string; pollMs: number; translateMs: number; renderMs: number; }
export const BENCH_SUITE: BenchCase[] = [
  { name: 'cold', pollMs: 8, translateMs: 2, renderMs: 4 },
  { name: 'steady', pollMs: 4, translateMs: 1, renderMs: 2 },
  { name: 'chord', pollMs: 6, translateMs: 3, renderMs: 3 },
  { name: 'ime', pollMs: 6, translateMs: 8, renderMs: 4 },
  { name: 'replay', pollMs: 4, translateMs: 6, renderMs: 2 },
];
export function benchTotal(b: BenchCase): number { return b.pollMs + b.translateMs + b.renderMs; }
export function benchScore(b: BenchCase, budgetMs: number): number {
  return Math.max(0, 1000 - Math.max(0, benchTotal(b) - budgetMs) * 20);
}
export function regressed(oldScore: number, newScore: number): boolean {
  return newScore < (oldScore * 95) / 100;
}

/* -------- 族0190 输入遥测 -------- */
export class TelemRing {
  private slots: [number, number][];
  private head = 0;
  constructor(cap: number) { this.slots = new Array(Math.min(16, Math.max(1, cap))).fill([0, 0]); }
  emit(id: number, tMs: number): void { this.slots[this.head] = [id, tMs]; this.head = (this.head + 1) % this.slots.length; }
  count(id: number): number { return this.slots.filter((s) => s[0] === id).length; }
  fingerprint(): number {
    let h = 0x811c9dc5;
    for (const [id, t] of this.slots) {
      h = Math.imul(h ^ id, 0x01000193);
      h = Math.imul(h ^ (t % 0x100000000), 0x01000193);
    }
    return h >>> 0;
  }
}
export function clampRate(r: number): number { return Math.min(100, Math.max(0, r)); }
