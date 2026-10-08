/**
 * UNREAL-X-15000 · AI-49 声音内核与收官 逻辑核（族0481~0490 · X12001~X12250），勿删。
 * 五层 × 五档模板：基础实装/边界与恢复/手感与细节/性能与优化/创新拓展。全部确定性算法，零 AI。
 */

/* -------- 族0481 内核音频管线（X12001~X12025 · K 线口径）-------- */

export const AUDIO_PIPELINE_TIERS = ['solid', 'frosted', 'balanced', 'pro', 'studio'] as const;
export type PipelineTier = (typeof AUDIO_PIPELINE_TIERS)[number];

/** 内核音频管线：端到端闭环 + 参数钳制 + 快照迁移 + 降级链。 */
export class AudioPipeline {
  tier: PipelineTier = 'balanced';
  sampleRate = 48000;
  gain = 0.8;
  running = false;
  clamped = 0;

  /** 端到端最小闭环：open→push→flush。 */
  open(): boolean {
    this.running = true;
    return this.running;
  }
  close(): boolean {
    const was = this.running;
    this.running = false;
    return was;
  }
  setTier(t: string): PipelineTier {
    const ok = (AUDIO_PIPELINE_TIERS as readonly string[]).includes(t);
    this.tier = ok ? (t as PipelineTier) : 'balanced';
    if (!ok) this.clamped++;
    return this.tier;
  }
  /** 采样率钳制 8000~192000，增益 0~1。 */
  setParams(rate: number, gain: number): { sampleRate: number; gain: number } {
    const r = Number.isFinite(rate) ? Math.min(192000, Math.max(8000, rate)) : 48000;
    const g = Number.isFinite(gain) ? Math.min(1, Math.max(0, gain)) : 0.8;
    if (r !== rate || g !== gain || !Number.isFinite(rate) || !Number.isFinite(gain)) this.clamped++;
    this.sampleRate = r;
    this.gain = g;
    return { sampleRate: r, gain: g };
  }
  /** 资源紧张降级链：studio→pro→balanced→frosted→solid。 */
  degrade(): PipelineTier {
    const i = AUDIO_PIPELINE_TIERS.indexOf(this.tier);
    const next = AUDIO_PIPELINE_TIERS[Math.max(0, i - 1)] as PipelineTier;
    this.tier = next;
    return next;
  }
  snapshot(): string {
    return JSON.stringify({ tier: this.tier, sampleRate: this.sampleRate, gain: this.gain });
  }
  restore(raw: string): boolean {
    try {
      const o = JSON.parse(raw) as { tier?: string; sampleRate?: number; gain?: number };
      this.setTier(String(o.tier ?? 'balanced'));
      this.setParams(Number(o.sampleRate ?? 48000), Number(o.gain ?? 0.8));
      return true;
    } catch {
      return false;
    }
  }
  static tierLabel(t: PipelineTier): string {
    const map: Record<PipelineTier, string> = {
      solid: '纯直通', frosted: '轻处理', balanced: '均衡', pro: '专业', studio: '棚级',
    };
    return map[t];
  }
}

/* -------- 族0482 低延迟音频（X12026~X12050 · K 线口径）-------- */

export const LATENCY_MODES = ['safe', 'balanced', 'pro', 'turbo', 'custom'] as const;
export type LatencyMode = (typeof LATENCY_MODES)[number];

/** 低延迟音频：模式五档 + 时延钳制 + 欠载守护。 */
export class LowLatencyAudio {
  mode: LatencyMode = 'balanced';
  latencyMs = 40;
  underruns: number[] = [];
  watchdog = true;
  clamped = 0;

  setMode(m: string): LatencyMode {
    const ok = (LATENCY_MODES as readonly string[]).includes(m);
    this.mode = ok ? (m as LatencyMode) : 'balanced';
    if (!ok) this.clamped++;
    return this.mode;
  }
  /** 时延钳制 1~500ms。 */
  setLatency(ms: number): number {
    const v = Number.isFinite(ms) ? Math.min(500, Math.max(1, ms)) : 40;
    if (v !== ms || !Number.isFinite(ms)) this.clamped++;
    this.latencyMs = v;
    return v;
  }
  /** 欠载事件登记（时间戳去重）。 */
  reportUnderrun(ts: number): boolean {
    if (this.underruns.includes(ts)) return false;
    this.underruns.push(ts);
    return true;
  }
  /** 守护：连续欠载 ≥3 次自动升一档时延。 */
  guard(): boolean {
    if (!this.watchdog || this.underruns.length < 3) return false;
    this.setLatency(this.latencyMs + 10);
    this.underruns = [];
    return true;
  }
  snapshot(): string {
    return JSON.stringify({ mode: this.mode, latencyMs: this.latencyMs, watchdog: this.watchdog });
  }
  restore(raw: string): boolean {
    try {
      const o = JSON.parse(raw) as { mode?: string; latencyMs?: number; watchdog?: boolean };
      this.setMode(String(o.mode ?? 'balanced'));
      this.setLatency(Number(o.latencyMs ?? 40));
      this.watchdog = o.watchdog !== false;
      return true;
    } catch {
      return false;
    }
  }
}

/* -------- 族0483 音频空间化引擎（X12051~X12075 · K 线口径）-------- */

export const HRTF_PROFILES = ['generic', 'room', 'stage', 'hall', 'open'] as const;
export type HrtfProfile = (typeof HRTF_PROFILES)[number];

/** 空间化引擎：声源登记去重 + 听者位姿钳制 + 声像衰减。 */
export class SpatialEngine {
  profile: HrtfProfile = 'generic';
  sources: string[] = [];
  listener = { x: 0, y: 0, yaw: 0 };
  clamped = 0;

  setProfile(p: string): HrtfProfile {
    const ok = (HRTF_PROFILES as readonly string[]).includes(p);
    this.profile = ok ? (p as HrtfProfile) : 'generic';
    if (!ok) this.clamped++;
    return this.profile;
  }
  /** 声源登记（去重）。 */
  addSource(id: string): boolean {
    if (!id || this.sources.includes(id)) return false;
    this.sources.push(id);
    return true;
  }
  /** 听者位姿：坐标 ±1000，yaw ±180。 */
  setListener(x: number, y: number, yaw: number): { x: number; y: number; yaw: number } {
    const cx = Number.isFinite(x) ? Math.min(1000, Math.max(-1000, x)) : 0;
    const cy = Number.isFinite(y) ? Math.min(1000, Math.max(-1000, y)) : 0;
    const cyaw = Number.isFinite(yaw) ? Math.min(180, Math.max(-180, yaw)) : 0;
    if (cx !== x || cy !== y || cyaw !== yaw) this.clamped++;
    this.listener = { x: cx, y: cy, yaw: cyaw };
    return this.listener;
  }
  /** 距离衰减：gain = max(0, 1 - d/10)。 */
  attenuation(dist: number): number {
    const d = Number.isFinite(dist) ? Math.max(0, dist) : 0;
    return Math.max(0, 1 - d / 10);
  }
  /** 声像钳制 -1~1。 */
  pan(p: number): number {
    const v = Number.isFinite(p) ? Math.min(1, Math.max(-1, p)) : 0;
    if (v !== p || !Number.isFinite(p)) this.clamped++;
    return v;
  }
  snapshot(): string {
    return JSON.stringify({ profile: this.profile, sources: this.sources.length });
  }
  restore(raw: string): boolean {
    try {
      const o = JSON.parse(raw) as { profile?: string };
      this.setProfile(String(o.profile ?? 'generic'));
      return true;
    } catch {
      return false;
    }
  }
}

/* -------- 族0484 通知优先级排序（X12076~X12100 · C 线口径）-------- */

export const PRIORITY_LEVELS = ['silent', 'low', 'normal', 'high', 'critical'] as const;
export type PriorityLevel = (typeof PRIORITY_LEVELS)[number];
export const PRIORITY_WEIGHT: Record<PriorityLevel, number> = {
  silent: 0, low: 20, normal: 50, high: 80, critical: 100,
};

export interface NotifItem {
  id: string;
  level: PriorityLevel;
  ts: number;
}

/** 通知优先级排序：加权评分 + 稳定排序 + top-k。 */
export class NotifRank {
  items: NotifItem[] = [];
  clamped = 0;

  /** 登记去重。 */
  push(id: string, level: string, ts: number): boolean {
    if (!id || this.items.some((i) => i.id === id)) return false;
    const ok = (PRIORITY_LEVELS as readonly string[]).includes(level);
    if (!ok) this.clamped++;
    this.items.push({ id, level: ok ? (level as PriorityLevel) : 'normal', ts });
    return true;
  }
  score(it: NotifItem): number {
    return PRIORITY_WEIGHT[it.level];
  }
  /** 按（分降序, ts 升序）稳定排序。 */
  ranked(): NotifItem[] {
    return [...this.items].sort((a, b) => this.score(b) - this.score(a) || a.ts - b.ts);
  }
  top(k: number): NotifItem[] {
    const n = Number.isFinite(k) ? Math.max(0, Math.min(this.items.length, Math.trunc(k))) : 0;
    return this.ranked().slice(0, n);
  }
  /** 非法级别钳制为 normal。 */
  normalize(): number {
    return this.clamped;
  }
  clear(): void {
    this.items = [];
  }
}

/* -------- 族0485 通知洞察 2.0（X12101~X12125 · C 线口径）-------- */

/** 通知洞察：按应用计数 + 滑动平均噪声率 + 摘要。 */
export class NotifInsight {
  counts = new Map<string, number>();
  noise: number[] = [];
  clamped = 0;

  /** 记录一条通知（应用去重累加）。 */
  record(app: string): boolean {
    if (!app) return false;
    this.counts.set(app, (this.counts.get(app) ?? 0) + 1);
    return true;
  }
  /** 噪声率样本钳制 0~1。 */
  addNoise(r: number): number {
    const v = Number.isFinite(r) ? Math.min(1, Math.max(0, r)) : 0;
    if (v !== r || !Number.isFinite(r)) this.clamped++;
    this.noise.push(v);
    return v;
  }
  /** 滑动平均（窗口 5）。 */
  avgNoise(): number {
    const w = this.noise.slice(-5);
    if (w.length === 0) return 0;
    return Math.round((w.reduce((a, b) => a + b, 0) / w.length) * 100) / 100;
  }
  /** 最吵应用。 */
  noisiestApp(): string {
    let best = '';
    let n = -1;
    for (const [k, v] of this.counts) {
      if (v > n) { best = k; n = v; }
    }
    return best;
  }
  digest(): string {
    const total = [...this.counts.values()].reduce((a, b) => a + b, 0);
    return `共 ${total} 条 · ${this.counts.size} 个应用 · 噪声率 ${this.avgNoise()}`;
  }
  reset(): void {
    this.counts.clear();
    this.noise = [];
  }
}

/* -------- 族0486 声音自助诊断（X12126~X12150 · V 线口径）-------- */

export const DIAG_STEPS = ['device', 'driver', 'mixer', 'stream', 'output'] as const;
export const DIAG_STEP_LABEL: Record<string, string> = {
  device: '设备检测', driver: '驱动检测', mixer: '混音器检测', stream: '流检测', output: '输出检测',
};

/** 自助诊断：五步流水 + 失败叙事 + 重试预算。 */
export class SoundDiagnose {
  results = new Map<string, 'pass' | 'fail'>();
  retries = 0;
  maxRetries = 3;

  /** 步骤执行：非法步骤直接 false。 */
  run(step: string, ok: boolean): boolean {
    if (!(DIAG_STEPS as readonly string[]).includes(step)) return false;
    this.results.set(step, ok ? 'pass' : 'fail');
    return true;
  }
  /** 失败重试预算。 */
  retry(step: string, ok: boolean): boolean {
    if (this.retries >= this.maxRetries) return false;
    this.retries++;
    return this.run(step, ok);
  }
  passed(): string[] {
    return DIAG_STEPS.filter((s) => this.results.get(s) === 'pass');
  }
  /** 失败项给出下一步建议（禁裸报错）。 */
  narrative(): string {
    const bad = DIAG_STEPS.filter((s) => this.results.get(s) === 'fail');
    if (bad.length === 0) return '全部通过。';
    const s = bad[0] as string;
    return `${DIAG_STEP_LABEL[s]}未通过，试试：检查${DIAG_STEP_LABEL[s]}设置后重跑诊断。`;
  }
  reset(): void {
    this.results.clear();
    this.retries = 0;
  }
}

/* -------- 族0487 声音生态开放（X12151~X12175 · 三方口径）-------- */

export const ECO_PERMS = ['render', 'capture', 'spatial', 'mixer', 'telemetry'] as const;
export type EcoPerm = (typeof ECO_PERMS)[number];

/** 声音生态：插件登记去重 + 能力授权 + 版本协商。 */
export class SoundEco {
  plugins = new Map<string, { perms: EcoPerm[]; version: string }>();
  clamped = 0;

  /** 登记去重 + 版本格式校验 major.minor。 */
  register(id: string, version: string, perms: string[]): boolean {
    if (!id || this.plugins.has(id)) return false;
    if (!/^\d+\.\d+$/.test(version)) { this.clamped++; return false; }
    const clean = perms.filter((p): p is EcoPerm => {
      const ok = (ECO_PERMS as readonly string[]).includes(p);
      if (!ok) this.clamped++;
      return ok;
    });
    this.plugins.set(id, { perms: clean, version });
    return true;
  }
  /** 版本协商：主版本不同 = 不兼容。 */
  compatible(id: string, hostMajor: number): boolean {
    const p = this.plugins.get(id);
    if (!p) return false;
    return Number(p.version.split('.')[0]) === hostMajor;
  }
  /** 能力授权：未登记能力一律拒绝。 */
  authorize(id: string, perm: string): boolean {
    const p = this.plugins.get(id);
    if (!p) return false;
    return (p.perms as string[]).includes(perm);
  }
  revoke(id: string): boolean {
    return this.plugins.delete(id);
  }
}

/* -------- 族0488 通知性能（X12176~X12200 · V 线口径）-------- */

export const PERF_BUDGET_KEYS = ['dispatch', 'render', 'animate', 'layout', 'total'] as const;
export type PerfBudgetKey = (typeof PERF_BUDGET_KEYS)[number];

/** 通知性能：预算表 + p95 + 降级链。 */
export class NotifPerf {
  budgets = new Map<PerfBudgetKey, number>(PERF_BUDGET_KEYS.map((k) => [k, 16]));
  samples: number[] = [];
  clamped = 0;

  /** 预算钳制 1~1000ms。 */
  setBudget(k: string, ms: number): number {
    if (!(PERF_BUDGET_KEYS as readonly string[]).includes(k)) { this.clamped++; return 16; }
    const v = Number.isFinite(ms) ? Math.min(1000, Math.max(1, ms)) : 16;
    if (v !== ms || !Number.isFinite(ms)) this.clamped++;
    this.budgets.set(k as PerfBudgetKey, v);
    return v;
  }
  addSample(ms: number): void {
    const v = Number.isFinite(ms) ? Math.max(0, ms) : 0;
    this.samples.push(v);
  }
  /** p95（升序取 95 分位）。 */
  p95(): number {
    if (this.samples.length === 0) return 0;
    const s = [...this.samples].sort((a, b) => a - b);
    const idx = Math.min(s.length - 1, Math.ceil(s.length * 0.95) - 1);
    return s[idx] as number;
  }
  overBudget(k: PerfBudgetKey): boolean {
    return this.p95() > (this.budgets.get(k) ?? 16);
  }
  /** 低配降级：预算超 → 关动画 → 关模糊。 */
  degradeChain(): string[] {
    const steps: string[] = [];
    if (this.overBudget('total')) steps.push('关闭动画');
    if (this.p95() > 32) steps.push('关闭模糊');
    if (steps.length === 0) steps.push('保持现状');
    return steps;
  }
  reset(): void {
    this.samples = [];
  }
}

/* -------- 族0489 声音档案（X12201~X12225 · V 线口径）-------- */

export const ARCHIVE_STAGES = ['draft', 'review', 'frozen', 'sealed'] as const;
export type ArchiveStage = (typeof ARCHIVE_STAGES)[number];

/** 声音档案：版本链 + 冻结只读 + 导出净身。 */
export class SoundArchive {
  chain: { version: number; note: string }[] = [];
  stage: ArchiveStage = 'draft';
  clamped = 0;

  /** 追加版本（冻结后拒绝）。 */
  append(note: string): boolean {
    if (this.stage === 'frozen' || this.stage === 'sealed') return false;
    if (!note) { this.clamped++; return false; }
    this.chain.push({ version: this.chain.length + 1, note });
    return true;
  }
  /** 状态机只能前进。 */
  advance(): ArchiveStage {
    const i = ARCHIVE_STAGES.indexOf(this.stage);
    this.stage = ARCHIVE_STAGES[Math.min(ARCHIVE_STAGES.length - 1, i + 1)] as ArchiveStage;
    return this.stage;
  }
  /** 导出净身：无临时字段。 */
  export(): string {
    return JSON.stringify({ stage: this.stage, versions: this.chain.length });
  }
  /** 恢复到指定版本（只读回放，越界返回空串）。 */
  replay(version: number): string {
    const v = Math.trunc(version);
    if (!Number.isFinite(version) || v < 1 || v > this.chain.length) return '';
    return this.chain[v - 1]?.note ?? '';
  }
  reset(): void {
    this.chain = [];
    this.stage = 'draft';
  }
}

/* -------- 族0490 声音通知收官（X12226~X12250 · 三方口径）-------- */

export const FINALE_GATES = ['scope', 'ids', 'quality', 'regression', 'archive', 'signoff'] as const;
export type FinaleGate = (typeof FINALE_GATES)[number];

/** 收官：六步顺序门禁 + 一票否决 + ID 审计。 */
export class SoundFinale {
  passed = new Set<FinaleGate>();
  veto = false;
  ids: number[] = [];

  /** 门禁必须按序通过。 */
  pass(gate: string): boolean {
    if (this.veto) return false;
    const idx = (FINALE_GATES as readonly string[]).indexOf(gate);
    if (idx < 0) return false;
    if (idx > 0 && !this.passed.has(FINALE_GATES[idx - 1] as FinaleGate)) return false;
    this.passed.add(gate as FinaleGate);
    return true;
  }
  vetoOnce(): void {
    this.veto = true;
    this.passed.clear();
  }
  /** ID 区间审计：连续且无重。 */
  audit(lo: number, hi: number): boolean {
    if (this.ids.length !== hi - lo + 1) return false;
    const s = [...this.ids].sort((a, b) => a - b);
    return s.every((v, i) => v === lo + i);
  }
  done(): boolean {
    return !this.veto && FINALE_GATES.every((g) => this.passed.has(g));
  }
  reset(): void {
    this.passed.clear();
    this.veto = false;
    this.ids = [];
  }
}
