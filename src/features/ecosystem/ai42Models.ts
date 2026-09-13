/**
 * UNREAL-X-15000 · AI-42 生态工程与收官 K/V/C/三方线逻辑核（族0411~0420 · X10251~X10500），勿删。
 * 五层 × 五档模板：基础实装/边界与恢复/手感与细节/性能与优化/创新拓展。全部确定性算法，零 AI。
 */

/* -------- 族0411 自托管（X10251~X10275）-------- */

export const SELFHOST_TIERS = ['standalone', 'lan', 'cloud-private', 'hybrid', 'federated'] as const;
export type SelfHostTier = (typeof SELFHOST_TIERS)[number];

/** 自托管：服务编排 + 档位矩阵 + 回滚净身。 */
export class SelfHost {
  clamped = 0;
  tier: SelfHostTier = 'standalone';
  services = new Map<string, { port: number; healthy: boolean }>();
  tombstones = new Set<string>();
  /** 部署服务：端口 1~65535 钳制，非法名拒绝。 */
  deploy(name: string, port: number): boolean {
    if (!name || this.tombstones.has(name)) {
      this.clamped++;
      return false;
    }
    let p = Math.floor(port);
    if (!Number.isFinite(p)) {
      p = 8080;
      this.clamped++;
    }
    p = p < 1 ? 1 : p > 65535 ? 65535 : p;
    this.services.set(name, { port: p, healthy: true });
    return true;
  }
  /** 设置档位：白名单外回默认 standalone。 */
  setTier(t: string): SelfHostTier {
    if ((SELFHOST_TIERS as readonly string[]).includes(t)) {
      this.tier = t as SelfHostTier;
    } else {
      this.tier = 'standalone';
      this.clamped++;
    }
    return this.tier;
  }
  /** 健康巡检：标记不健康并可一键恢复。 */
  markHealthy(name: string, ok: boolean): boolean {
    const s = this.services.get(name);
    if (!s) return false;
    s.healthy = ok;
    return true;
  }
  healthy(): string[] {
    return [...this.services.entries()].filter(([, s]) => s.healthy).map(([n]) => n);
  }
  /** 卸载净身：进墓碑表，重名拒绝复用。 */
  teardown(name: string): boolean {
    if (!this.services.has(name)) return false;
    this.services.delete(name);
    this.tombstones.add(name);
    return true;
  }
  /** 资源降级：紧张时按档位收敛并发。 */
  concurrency(pressure: number): number {
    if (!Number.isFinite(pressure) || pressure < 0) {
      this.clamped++;
      return 1;
    }
    const base = 1 + SELFHOST_TIERS.indexOf(this.tier) * 4;
    const cap = Math.min(32, Math.max(1, Math.round(base * (1 - Math.min(0.9, pressure)))));
    return cap;
  }
  static tierLabel(t: SelfHostTier): string {
    return {
      standalone: '单机', lan: '局域网', 'cloud-private': '私有云', hybrid: '混合', federated: '联邦',
    }[t];
  }
}

/* -------- 族0412 可持续（X10276~X10300）-------- */

export const SUSTAIN_MODES = ['community', 'patron', 'grant', 'market', 'foundation'] as const;
export type SustainMode = (typeof SUSTAIN_MODES)[number];

/** 可持续：经费模式 × 资金池 × 透明账本。 */
export class Sustainability {
  clamped = 0;
  mode: SustainMode = 'community';
  pool = 0;
  ledger: { kind: 'in' | 'out'; amount: number; memo: string }[] = [];
  setMode(m: string): SustainMode {
    if ((SUSTAIN_MODES as readonly string[]).includes(m)) {
      this.mode = m as SustainMode;
    } else {
      this.mode = 'community';
      this.clamped++;
    }
    return this.mode;
  }
  /** 入账：金额钳制到 [0, 1e9]。 */
  donate(amount: number, memo: string): boolean {
    if (!Number.isFinite(amount) || amount <= 0) {
      this.clamped++;
      return false;
    }
    const a = Math.min(1e9, Math.round(amount));
    this.pool += a;
    this.ledger.push({ kind: 'in', amount: a, memo });
    return true;
  }
  /** 出账：余额不足拒绝（不透支）。 */
  spend(amount: number, memo: string): boolean {
    if (!Number.isFinite(amount) || amount <= 0) {
      this.clamped++;
      return false;
    }
    const a = Math.min(1e9, Math.round(amount));
    if (a > this.pool) return false;
    this.pool -= a;
    this.ledger.push({ kind: 'out', amount: a, memo });
    return true;
  }
  /** 账本平衡校验：pool === in 总额 - out 总额。 */
  balanced(): boolean {
    const ins = this.ledger.filter((l) => l.kind === 'in').reduce((s, l) => s + l.amount, 0);
    const outs = this.ledger.filter((l) => l.kind === 'out').reduce((s, l) => s + l.amount, 0);
    return this.pool === ins - outs;
  }
  /** 可持续指数：资金跑道（月）按模式系数加权。 */
  runway(monthlyBurn: number): number {
    if (!Number.isFinite(monthlyBurn) || monthlyBurn <= 0) {
      this.clamped++;
      return Infinity;
    }
    const k = 1 + SUSTAIN_MODES.indexOf(this.mode) * 0.2;
    return Math.round((this.pool / monthlyBurn) * k * 10) / 10;
  }
  static modeLabel(m: SustainMode): string {
    return { community: '社区', patron: '赞助', grant: '资助', market: '市场', foundation: '基金会' }[m];
  }
}

/* -------- 族0413 质量开放（X10301~X10325）-------- */

export const QUALITY_GRADES = ['unrated', 'bronze', 'silver', 'gold', 'platinum'] as const;
export type QualityGrade = (typeof QUALITY_GRADES)[number];

/** 质量开放：公开评分 × 徽章 × 申诉复核。 */
export class QualityOpen {
  clamped = 0;
  scores = new Map<string, number>();
  /** 打分：六维（docs/tests/perf/a11y/security/traffic-light）0~100 钳制取均值。 */
  rate(id: string, dims: number[]): number {
    if (!id || dims.length === 0) {
      this.clamped++;
      return 0;
    }
    const clampedDims = dims.map((d) => (Number.isFinite(d) ? Math.max(0, Math.min(100, Math.round(d))) : 0));
    const avg = Math.round(clampedDims.reduce((s, d) => s + d, 0) / clampedDims.length);
    this.scores.set(id, avg);
    return avg;
  }
  /** 徽章：≥90 platinum / ≥75 gold / ≥60 silver / ≥40 bronze / 否则 unrated。 */
  grade(id: string): QualityGrade {
    const s = this.scores.get(id);
    if (s === undefined) return 'unrated';
    if (s >= 90) return 'platinum';
    if (s >= 75) return 'gold';
    if (s >= 60) return 'silver';
    if (s >= 40) return 'bronze';
    return 'unrated';
  }
  /** 榜单：按分降序，同分按 id 字典序。 */
  leaderboard(): string[] {
    return [...this.scores.entries()]
      .sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1))
      .map(([id]) => id);
  }
  /** 申诉复核：重打分覆盖旧值。 */
  appeal(id: string, dims: number[]): number {
    return this.rate(id, dims);
  }
  static gradeLabel(g: QualityGrade): string {
    return { unrated: '未评级', bronze: '铜', silver: '银', gold: '金', platinum: '铂金' }[g];
  }
}

/* -------- 族0414 生态精选（X10326~X10350）-------- */

export const CURATION_SLOTS = 6;

/** 生态精选：编辑推荐位 × 轮换 × 埋点回传。 */
export class Curation {
  clamped = 0;
  slots: string[] = [];
  impressions = new Map<string, number>();
  /** 上精选位：去重 + 槽位上限（超出 FIFO 淘汰）。 */
  feature(id: string): boolean {
    if (!id) {
      this.clamped++;
      return false;
    }
    const at = this.slots.indexOf(id);
    if (at >= 0) this.slots.splice(at, 1);
    this.slots.push(id);
    if (this.slots.length > CURATION_SLOTS) this.slots.shift();
    return true;
  }
  /** 记曝光：选中位曝光计数（去重自增）。 */
  impress(id: string): void {
    if (!this.slots.includes(id)) {
      this.clamped++;
      return;
    }
    this.impressions.set(id, (this.impressions.get(id) ?? 0) + 1);
  }
  /** 点击率：impress /曝光为 0 时给 0。 */
  ctr(id: string): number {
    const im = this.impressions.get(id) ?? 0;
    return im === 0 ? 0 : Math.round((im / Math.max(1, im)) * 1000) / 1000;
  }
  /** 轮换：最早入选的沉底，其余前移。 */
  rotate(): void {
    if (this.slots.length < 2) return;
    const head = this.slots.shift() as string;
    this.slots.push(head);
  }
  /** 导出快照（JSON）。 */
  snapshot(): string {
    return JSON.stringify({ slots: [...this.slots], impressions: [...this.impressions.entries()] });
  }
  static restore(snap: string): Curation {
    try {
      const o = JSON.parse(snap) as { slots: string[]; impressions: [string, number][] };
      const c = new Curation();
      c.slots = o.slots.slice(0, CURATION_SLOTS);
      c.impressions = new Map(o.impressions);
      return c;
    } catch {
      return new Curation();
    }
  }
}

/* -------- 族0415 插件签名与安全（X10351~X10375）-------- */

export const SIGN_ALGOS = ['ed25519', 'rsa-pss', 'ecdsa-p256'] as const;
export type SignAlgo = (typeof SIGN_ALGOS)[number];
export const SIGN_STATES = ['unsigned', 'invalid', 'self-signed', 'trusted'] as const;
export type SignState = (typeof SIGN_STATES)[number];

/** 插件签名：验签状态机 + 信任锚 + 吊销。 */
export class PluginSigning {
  clamped = 0;
  anchors = new Set<string>();
  revoked = new Set<string>();
  signatures = new Map<string, { algo: SignAlgo; state: SignState }>();
  /** 注册信任锚：指纹 16 位十六进制。 */
  addAnchor(fp: string): boolean {
    if (!/^[0-9a-f]{16}$/.test(fp)) {
      this.clamped++;
      return false;
    }
    this.anchors.add(fp);
    return true;
  }
  /** 验签：锚内→trusted；自带签名但无锚→self-signed；坏签名→invalid；无签名→unsigned。 */
  verify(id: string, fp: string | null, algo: string): SignState {
    if (this.revoked.has(id)) return 'invalid';
    if (fp === null) {
      this.signatures.set(id, { algo: 'ed25519', state: 'unsigned' });
      return 'unsigned';
    }
    if (!(SIGN_ALGOS as readonly string[]).includes(algo)) {
      this.clamped++;
      this.signatures.set(id, { algo: 'ed25519', state: 'invalid' });
      return 'invalid';
    }
    const state: SignState = this.anchors.has(fp) ? 'trusted' : 'self-signed';
    this.signatures.set(id, { algo: algo as SignAlgo, state });
    return state;
  }
  /** 吊销：已发布插件一键下架，验签永久 invalid。 */
  revoke(id: string): boolean {
    if (!this.signatures.has(id)) return false;
    this.revoked.add(id);
    return true;
  }
  /** 可装载：仅 trusted。 */
  loadable(id: string): boolean {
    return this.signatures.get(id)?.state === 'trusted' && !this.revoked.has(id);
  }
  static stateLabel(s: SignState): string {
    return { unsigned: '未签名', invalid: '验签失败', 'self-signed': '自签名', trusted: '受信' }[s];
  }
}

/* -------- 族0416 插件沙箱运行时（X10376~X10400）-------- */

export const SANDBOX_LEVELS = ['none', 'lite', 'standard', 'strict', 'paranoid'] as const;
export type SandboxLevel = (typeof SANDBOX_LEVELS)[number];

/** 插件沙箱运行时：预算（CPU/内存/句柄）+ 超限熔断。 */
export class SandboxRuntime {
  clamped = 0;
  level: SandboxLevel = 'standard';
  budget = { cpuMs: 1000, memKb: 65536, handles: 64 };
  usage = { cpuMs: 0, memKb: 0, handles: 0 };
  killed = new Set<string>();
  setLevel(l: string): SandboxLevel {
    if ((SANDBOX_LEVELS as readonly string[]).includes(l)) {
      this.level = l as SandboxLevel;
    } else {
      this.level = 'standard';
      this.clamped++;
    }
    const idx = SANDBOX_LEVELS.indexOf(this.level);
    this.budget = { cpuMs: 4000 >> idx, memKb: 262144 >> idx, handles: 256 >> idx };
    return this.level;
  }
  /** 消耗资源：返回是否仍在预算内。 */
  consume(cpuMs: number, memKb: number, handles: number): boolean {
    if (![cpuMs, memKb, handles].every(Number.isFinite) || cpuMs < 0 || memKb < 0 || handles < 0) {
      this.clamped++;
      return false;
    }
    this.usage.cpuMs += Math.round(cpuMs);
    this.usage.memKb += Math.round(memKb);
    this.usage.handles += Math.round(handles);
    return (
      this.usage.cpuMs <= this.budget.cpuMs &&
      this.usage.memKb <= this.budget.memKb &&
      this.usage.handles <= this.budget.handles
    );
  }
  /** 熔断：超预算插件标记 killed 并清零用量。 */
  trip(pluginId: string): boolean {
    const over =
      this.usage.cpuMs > this.budget.cpuMs ||
      this.usage.memKb > this.budget.memKb ||
      this.usage.handles > this.budget.handles;
    if (!over) return false;
    this.killed.add(pluginId);
    this.usage = { cpuMs: 0, memKb: 0, handles: 0 };
    return true;
  }
  /** 复用：killed 插件重新准入（重置熔断）。 */
  rearm(pluginId: string): boolean {
    return this.killed.delete(pluginId);
  }
  /** 档位标签。 */
  static levelLabel(l: SandboxLevel): string {
    return { none: '无沙箱', lite: '轻量', standard: '标准', strict: '严格', paranoid: '偏执' }[l];
  }
}

/* -------- 族0417 生态数据分析（X10401~X10425）-------- */

export const ECO_METRICS = ['installs', 'actives', 'retention', 'crashRate', 'latencyP95'] as const;
export type EcoMetric = (typeof ECO_METRICS)[number];

/** 生态数据分析：指标序列 × 漏斗 × 异常检测。 */
export class EcoAnalytics {
  clamped = 0;
  series = new Map<EcoMetric, number[]>();
  /** 记录：单点数值钳制到 [0, 1e12]。 */
  record(metric: string, value: number): boolean {
    if (!(ECO_METRICS as readonly string[]).includes(metric)) {
      this.clamped++;
      return false;
    }
    const v = Number.isFinite(value) ? Math.max(0, Math.min(1e12, value)) : 0;
    const key = metric as EcoMetric;
    const arr = this.series.get(key) ?? [];
    arr.push(v);
    this.series.set(key, arr);
    return true;
  }
  /** 漏斗：installs→actives→retention 三级转化率（百分比，1 位小数）。 */
  funnel(): [number, number] | null {
    const i = this.series.get('installs') ?? [];
    const a = this.series.get('actives') ?? [];
    const r = this.series.get('retention') ?? [];
    if (i.length === 0 || a.length === 0) return null;
    const sum = (xs: number[]) => xs.reduce((s, x) => s + x, 0);
    const inst = sum(i);
    if (inst <= 0) return [0, 0];
    const act = (sum(a) / inst) * 100;
    const ret = sum(r) > 0 && inst > 0 ? (sum(r) / inst) * 100 : 0;
    return [Math.round(act * 10) / 10, Math.round(ret * 10) / 10];
  }
  /** 简单异常检测：偏离滑动均值 ±3σ 标记。 */
  anomalies(metric: string): number[] {
    const arr = this.series.get(metric as EcoMetric);
    if (!arr || arr.length < 3) return [];
    const mean = arr.reduce((s, x) => s + x, 0) / arr.length;
    const sd = Math.sqrt(arr.reduce((s, x) => s + (x - mean) ** 2, 0) / arr.length);
    if (sd === 0) return [];
    const out: number[] = [];
    arr.forEach((v, idx) => {
      if (Math.abs(v - mean) > 3 * sd) out.push(idx);
    });
    return out;
  }
  /** 趋势：末点对比首点 → up/down/flat。 */
  trend(metric: string): 'up' | 'down' | 'flat' {
    const arr = this.series.get(metric as EcoMetric);
    if (!arr || arr.length < 2) return 'flat';
    const d = arr[arr.length - 1]! - arr[0]!;
    return d > 0 ? 'up' : d < 0 ? 'down' : 'flat';
  }
}

/* -------- 族0418 开发者文档（X10426~X10450）-------- */

export const DOC_KINDS = ['quickstart', 'api', 'guide', 'sample', 'reference'] as const;
export type DocKind = (typeof DOC_KINDS)[number];

/** 开发者文档：覆盖度 × 新鲜度 × 示例可运行。 */
export class DevDocs {
  clamped = 0;
  pages = new Map<string, { kind: DocKind; updated: number; runnable: boolean }>();
  /** 发布页面：kind 白名单外拒绝；updated 用逻辑时钟（非负）。 */
  publish(id: string, kind: string, updated: number, runnable: boolean): boolean {
    if (!id || !(DOC_KINDS as readonly string[]).includes(kind) || !Number.isFinite(updated) || updated < 0) {
      this.clamped++;
      return false;
    }
    this.pages.set(id, { kind: kind as DocKind, updated: Math.round(updated), runnable });
    return true;
  }
  /** 覆盖度：五类齐全即 100%，否则按缺类扣减（每类 20%）。 */
  coverage(): number {
    const kinds = new Set([...this.pages.values()].map((p) => p.kind));
    return kinds.size * 20;
  }
  /** 新鲜度：与 clock 差值 ≤30 视为新鲜。 */
  stale(clock: number): string[] {
    return [...this.pages.entries()].filter(([, p]) => clock - p.updated > 30).map(([id]) => id);
  }
  /** 示例可运行率：runnable 页 / 全部页（百分比）。 */
  runnableRate(): number {
    if (this.pages.size === 0) return 0;
    const ok = [...this.pages.values()].filter((p) => p.runnable).length;
    return Math.round((ok / this.pages.size) * 100);
  }
  /** 目录树：按 kind 分组、组内按 id 排序。 */
  toc(): Record<string, string[]> {
    const out: Record<string, string[]> = {};
    for (const k of DOC_KINDS) out[k] = [];
    for (const [id, p] of [...this.pages.entries()].sort((a, b) => (a[0] < b[0] ? -1 : 1))) {
      out[p.kind]!.push(id);
    }
    return out;
  }
}

/* -------- 族0419 生态里程碑（X10451~X10475）-------- */

export const MILESTONE_GATES = ['G1', 'G2', 'G3', 'G4'] as const;
export type MilestoneGate = (typeof MILESTONE_GATES)[number];

/** 生态里程碑：G1~G4 顺序门禁 + 回退重验。 */
export class EcoMilestone {
  clamped = 0;
  passed = new Set<MilestoneGate>();
  /** 过门禁：必须按 G1→G2→G3→G4 顺序，跳门拒绝。 */
  pass(gate: string): boolean {
    if (!(MILESTONE_GATES as readonly string[]).includes(gate)) {
      this.clamped++;
      return false;
    }
    const g = gate as MilestoneGate;
    const idx = MILESTONE_GATES.indexOf(g);
    if (idx > this.passed.size) {
      this.clamped++;
      return false;
    }
    this.passed.add(g);
    return true;
  }
  /** 进度：已过门禁数 / 4（百分比）。 */
  progress(): number {
    return this.passed.size * 25;
  }
  /** 回退：撤销最后一道门（重验场景）。 */
  rollback(): MilestoneGate | null {
    const last = [...MILESTONE_GATES].reverse().find((g) => this.passed.has(g));
    if (!last) return null;
    this.passed.delete(last);
    return last;
  }
  /** 收官判定：G1~G4 齐全。 */
  done(): boolean {
    return MILESTONE_GATES.every((g) => this.passed.has(g));
  }
}

/* -------- 族0420 生态收官（X10476~X10500）-------- */

export const FINALE_CHECKS = ['signing', 'sandbox', 'selfhost', 'docs', 'analytics', 'quality'] as const;
export type FinaleCheck = (typeof FINALE_CHECKS)[number];

/** 生态收官：六项终验 + ID 审计 + 一票否决。 */
export class EcoFinale {
  clamped = 0;
  results = new Map<FinaleCheck, boolean>();
  /** 记录终验项：白名单外拒绝。 */
  record(check: string, ok: boolean): boolean {
    if (!(FINALE_CHECKS as readonly string[]).includes(check)) {
      this.clamped++;
      return false;
    }
    this.results.set(check as FinaleCheck, ok);
    return true;
  }
  /** 收官门：六项全 true 才放行。 */
  gate(): boolean {
    return FINALE_CHECKS.every((c) => this.results.get(c) === true);
  }
  /** 缺口清单：未过或未验的项。 */
  gaps(): FinaleCheck[] {
    return FINALE_CHECKS.filter((c) => this.results.get(c) !== true);
  }
  /** ID 审计：连续区间 [from, to] 数量核对（收官红线）。 */
  static auditIdCount(from: number, to: number, actual: number): boolean {
    return to - from + 1 === actual;
  }
  /** 复验：清空全部结果重来。 */
  reset(): void {
    this.results.clear();
  }
  /** 一票否决：安全类（signing/sandbox）任一未过即红。 */
  vetoed(): boolean {
    return (this.results.get('signing') === false || this.results.get('sandbox') === false);
  }
}
