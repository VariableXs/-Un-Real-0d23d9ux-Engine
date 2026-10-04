/**
 * UNREAL-X-15000 · AI-60 大收官 逻辑核（领域16 · 族0591~0600 · X14751~X15000）。
 * 三方主责：社区运营/终极验收仪式/文档收官/基线冻结/毕业审计/时间胶囊/下一代路线/大收官；
 * C 线同口径镜像（族0593 三线一致性审计 / 族0594 ID 唯一性防线）。
 * 全部确定性算法：零 AI、零随机、零时钟依赖。
 */

// ---- 族0591 社区运营（X14751~X14775）----

export const COMMUNITY_ROLES = ['owner', 'maintainer', 'contributor', 'reader'] as const;
export type CommunityRole = (typeof COMMUNITY_ROLES)[number];

/** 社区运营：提案队列 + 法定票数 + 角色权重 + 双通道（采纳/驳回）。 */
export class CommunityOps {
  proposals: { id: number; title: string; votes: number; status: 'open' | 'accepted' | 'rejected' }[] = [];
  quorum = 3;
  maxOpen = 10;
  clamped = 0;

  static roleWeight(role: string): number {
    const m: Record<CommunityRole, number> = { owner: 3, maintainer: 2, contributor: 1, reader: 0 };
    return (COMMUNITY_ROLES as readonly string[]).includes(role) ? m[role as CommunityRole] : 0;
  }

  propose(title: string): number {
    if (!title.trim() || this.proposals.filter((p) => p.status === 'open').length >= this.maxOpen) {
      this.clamped += 1;
      return 0;
    }
    const id = this.proposals.length + 1;
    this.proposals.push({ id, title: title.trim(), votes: 0, status: 'open' });
    return id;
  }

  vote(id: number, weight = 1): boolean {
    const p = this.proposals.find((x) => x.id === id);
    if (!p || p.status !== 'open' || !Number.isInteger(weight) || weight < 1 || weight > 3) {
      this.clamped += 1;
      return false;
    }
    p.votes += weight;
    return true;
  }

  /** 双通道收尾：达标采纳、未达驳回；非法单返回空串。 */
  close(id: number): 'accepted' | 'rejected' | '' {
    const p = this.proposals.find((x) => x.id === id);
    if (!p || p.status !== 'open') {
      this.clamped += 1;
      return '';
    }
    p.status = p.votes >= this.quorum ? 'accepted' : 'rejected';
    return p.status;
  }

  accepted(): number {
    return this.proposals.filter((p) => p.status === 'accepted').length;
  }

  static narrative(): string {
    return '提案未达法定票数：已转入驳回池，可在下一运营周期重新发起';
  }

  snapshot(): string {
    return JSON.stringify({ q: this.quorum, cap: this.maxOpen, ps: this.proposals });
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as { q: number; cap: number; ps: CommunityOps['proposals'] };
      this.quorum = o.q;
      this.maxOpen = o.cap;
      this.proposals = o.ps;
      return true;
    } catch {
      return false;
    }
  }
}

// ---- 族0592 终极验收仪式（X14776~X14800）----

export const CEREMONY_STAGES = ['prelude', 'verify', 'showcase', 'signoff', 'finale'] as const;
export type CeremonyStage = (typeof CEREMONY_STAGES)[number];

/** 终极验收仪式：五阶段推进 + 五项清单 + 观礼规模钳制 + 终章点灯。 */
export class AcceptanceCeremony {
  stageIdx = 0;
  checklist = [false, false, false, false, false];
  audience = 0;
  lit = false;
  clamped = 0;

  static stageLabel(s: CeremonyStage): string {
    const m: Record<CeremonyStage, string> = {
      prelude: '序章', verify: '验证', showcase: '展示', signoff: '签署', finale: '终章',
    };
    return m[s];
  }

  setAudience(n: number): number {
    this.audience = Math.min(5000, Math.max(0, Math.trunc(n) || 0));
    return this.audience;
  }

  tick(i: number): boolean {
    if (!Number.isInteger(i) || i < 0 || i >= this.checklist.length) {
      this.clamped += 1;
      return false;
    }
    this.checklist[i] = true;
    return true;
  }

  ready(): boolean {
    return this.checklist.every(Boolean);
  }

  advance(): boolean {
    if (this.stageIdx >= CEREMONY_STAGES.length - 1) {
      this.clamped += 1;
      return false;
    }
    if (!this.ready()) {
      this.clamped += 1;
      return false;
    }
    this.stageIdx += 1;
    return true;
  }

  stage(): CeremonyStage {
    return CEREMONY_STAGES[this.stageIdx]!;
  }

  /** 终章点灯：仅 finale 阶段且五项全勾。 */
  light(): boolean {
    if (this.stage() !== 'finale' || !this.ready()) {
      this.clamped += 1;
      return false;
    }
    this.lit = true;
    return true;
  }

  static narrative(): string {
    return '仪式未就绪：请先勾满五项清单再推进阶段';
  }

  snapshot(): string {
    return JSON.stringify({ s: this.stageIdx, c: this.checklist, a: this.audience, l: this.lit });
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as { s: number; c: boolean[]; a: number; l: boolean };
      if (o.s < 0 || o.s >= CEREMONY_STAGES.length || o.c.length !== 5) return false;
      this.stageIdx = o.s;
      this.checklist = o.c;
      this.audience = o.a;
      this.lit = o.l;
      return true;
    } catch {
      return false;
    }
  }
}

// ---- 族0593 三线一致性审计（X14801~X14825 · C 线主责，V 线同口径镜像）----

/** 三线一致性审计：逐族记录 K/V/C 三线计数 + 最大偏斜 + 平衡判定。 */
export class ConsistencyAudit {
  rows: { family: number; k: number; v: number; c: number }[] = [];
  clamped = 0;

  /** 每族三线之和必须恰为 25（与族容量一致）。 */
  record(family: number, k: number, v: number, c: number): boolean {
    if (!Number.isInteger(family) || family < 1 || family > 600) {
      this.clamped += 1;
      return false;
    }
    for (const n of [k, v, c]) {
      if (!Number.isInteger(n) || n < 0) {
        this.clamped += 1;
        return false;
      }
    }
    if (k + v + c !== 25 || this.rows.some((r) => r.family === family)) {
      this.clamped += 1;
      return false;
    }
    this.rows.push({ family, k, v, c });
    return true;
  }

  /** 平衡判定：所有行 k===v===c（即 25/25 无法整除时按最大偏斜 ≤1 判定）。 */
  balanced(): boolean {
    return this.rows.length > 0 && this.rows.every((r) => Math.max(r.k, r.v, r.c) - Math.min(r.k, r.v, r.c) <= 1);
  }

  maxSkew(): number {
    let m = 0;
    for (const r of this.rows) {
      m = Math.max(m, Math.max(r.k, r.v, r.c) - Math.min(r.k, r.v, r.c));
    }
    return m;
  }

  verdict(): 'balanced' | 'skewed' | 'empty' {
    if (this.rows.length === 0) return 'empty';
    return this.balanced() ? 'balanced' : 'skewed';
  }

  total(): number {
    return this.rows.length * 25;
  }

  static narrative(v: 'balanced' | 'skewed' | 'empty'): string {
    const m: Record<'balanced' | 'skewed' | 'empty', string> = {
      balanced: '三线分布平衡：K/V/C 偏斜 ≤1，可入终验',
      skewed: '三线偏斜超限：请回填弱势线后再审计',
      empty: '审计为空：请先登记至少一族的三线计数',
    };
    return m[v];
  }
}

// ---- 族0594 ID 唯一性防线（X14826~X14850 · C 线主责，V 线同口径镜像）----

/** ID 唯一性防线：X-ID 合法域（1~15000）+ 全局去重 + 批量重复检测 + 区间覆盖。 */
export class IdUniqueness {
  seen = new Set<number>();
  clamped = 0;

  static validate(id: number): boolean {
    return Number.isInteger(id) && id >= 1 && id <= 15000;
  }

  static spanOk(min: number, max: number): boolean {
    return IdUniqueness.validate(min) && IdUniqueness.validate(max) && min <= max;
  }

  admit(id: number): boolean {
    if (!IdUniqueness.validate(id) || this.seen.has(id)) {
      this.clamped += 1;
      return false;
    }
    this.seen.add(id);
    return true;
  }

  /** 批量重复计数：数组内重复 ID 的出现次数（每个重复 ID 计一次）。 */
  static dupes(ids: number[]): number {
    const set = new Set<number>();
    let n = 0;
    for (const id of ids) {
      if (set.has(id)) n += 1;
      else set.add(id);
    }
    return n;
  }

  /** 区间覆盖：已收录 ID 落在 [min,max] 的数量。 */
  coverage(min: number, max: number): number {
    if (!IdUniqueness.spanOk(min, max)) return 0;
    let n = 0;
    for (const id of this.seen) {
      if (id >= min && id <= max) n += 1;
    }
    return n;
  }

  conflicts(ids: number[]): number {
    return ids.filter((id) => this.seen.has(id)).length;
  }

  static narrative(): string {
    return '检出重复 X-ID：防线已拦截，请改用空闲编号';
  }

  snapshot(): string {
    return JSON.stringify([...this.seen]);
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as number[];
      this.seen = new Set(o.filter((n) => IdUniqueness.validate(n)));
      return true;
    } catch {
      return false;
    }
  }
}

// ---- 族0595 文档收官（X14851~X14875）----

export const DOC_GATES = ['scope', 'ids', 'links', 'gates', 'signoff'] as const;
export type DocGate = (typeof DOC_GATES)[number];

/** 文档收官：五门顺序推进 + 断链归零 + 收官判定。 */
export class DocFinale {
  passed = new Set<DocGate>();
  brokenLinks = 0;
  clamped = 0;

  static narrative(code: 'order' | 'links' | 'unknown'): string {
    const m: Record<'order' | 'links' | 'unknown', string> = {
      order: '文档门禁按序推进：scope→ids→links→gates→signoff',
      links: '存在断链：修复全部失效链接后方可过 links 门',
      unknown: '未知文档门：仅支持 scope/ids/links/gates/signoff',
    };
    return m[code];
  }

  pass(gate: string): boolean {
    if (!(DOC_GATES as readonly string[]).includes(gate)) {
      this.clamped += 1;
      return false;
    }
    const idx = DOC_GATES.indexOf(gate as DocGate);
    for (let i = 0; i < idx; i++) {
      if (!this.passed.has(DOC_GATES[i]!)) {
        this.clamped += 1;
        return false;
      }
    }
    if (gate === 'links' && this.brokenLinks > 0) {
      this.clamped += 1;
      return false;
    }
    this.passed.add(gate as DocGate);
    return true;
  }

  reportBroken(n: number): boolean {
    if (!Number.isInteger(n) || n < 0 || n > 99) {
      this.clamped += 1;
      return false;
    }
    this.brokenLinks = n;
    return true;
  }

  fixAll(): boolean {
    if (this.brokenLinks === 0) {
      this.clamped += 1;
      return false;
    }
    this.brokenLinks = 0;
    return true;
  }

  done(): boolean {
    return DOC_GATES.every((g) => this.passed.has(g)) && this.brokenLinks === 0;
  }

  snapshot(): string {
    return JSON.stringify({ g: [...this.passed], b: this.brokenLinks });
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as { g: DocGate[]; b: number };
      this.passed = new Set(o.g.filter((g) => (DOC_GATES as readonly string[]).includes(g)));
      this.brokenLinks = o.b;
      return true;
    } catch {
      return false;
    }
  }
}

// ---- 族0596 基线冻结（X14876~X14900）----

/** 基线冻结：指标入册 + 冻结封印（FNV 指纹）+ 篡改检测。 */
export class BaselineFreeze {
  metrics: number[] = [];
  frozen = false;
  clamped = 0;

  addMetric(v: number): boolean {
    if (this.frozen || !Number.isFinite(v) || Math.trunc(v) !== v) {
      this.clamped += 1;
      return false;
    }
    this.metrics.push(v);
    return true;
  }

  freeze(): boolean {
    if (this.frozen || this.metrics.length === 0) {
      this.clamped += 1;
      return false;
    }
    this.frozen = true;
    return true;
  }

  static fnv(nums: number[]): number {
    let h = 0x811c9dc5;
    const s = nums.join(',');
    for (let i = 0; i < s.length; i++) {
      h ^= s.charCodeAt(i);
      h = Math.imul(h, 0x01000193);
    }
    return h >>> 0;
  }

  seal(): number {
    return this.frozen ? BaselineFreeze.fnv(this.metrics) : 0;
  }

  verify(seal: number): boolean {
    return this.frozen && this.seal() === seal;
  }

  static narrative(): string {
    return '基线未冻结或为空：先入册指标再冻结封印';
  }

  snapshot(): string {
    return JSON.stringify({ m: this.metrics, f: this.frozen });
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as { m: number[]; f: boolean };
      this.metrics = o.m.filter((n) => Number.isFinite(n));
      this.frozen = o.f;
      return true;
    } catch {
      return false;
    }
  }
}

// ---- 族0597 毕业审计（X14901~X14925）----

export const GRAD_CRITERIA = ['lines', 'gates', 'docs', 'debt', 'innovation'] as const;
export type GradCriterion = (typeof GRAD_CRITERIA)[number];

/** 毕业审计：五准则打分（0~100 钳制）+ 总分 + 三档判定。 */
export class GraduationAudit {
  scores = new Map<string, number>();
  clamped = 0;

  static criterionLabel(c: GradCriterion): string {
    const m: Record<GradCriterion, string> = {
      lines: '三线覆盖', gates: '门禁全绿', docs: '文档完备', debt: '债务清偿', innovation: '创新保底',
    };
    return m[c];
  }

  setScore(criterion: string, v: number): boolean {
    if (!(GRAD_CRITERIA as readonly string[]).includes(criterion) || !Number.isFinite(v)) {
      this.clamped += 1;
      return false;
    }
    this.scores.set(criterion, Math.min(100, Math.max(0, Math.trunc(v))));
    return true;
  }

  complete(): boolean {
    return GRAD_CRITERIA.every((c) => this.scores.has(c));
  }

  total(): number {
    if (!this.complete()) return 0;
    let s = 0;
    for (const c of GRAD_CRITERIA) s += this.scores.get(c)!;
    return Math.round(s / GRAD_CRITERIA.length);
  }

  verdict(): 'excellent' | 'pass' | 'fail' {
    if (!this.complete()) return 'fail';
    const t = this.total();
    if (t >= 90) return 'excellent';
    return t >= 60 ? 'pass' : 'fail';
  }

  static narrative(v: 'excellent' | 'pass' | 'fail'): string {
    const m: Record<'excellent' | 'pass' | 'fail', string> = {
      excellent: '优秀毕业：五准则均分 ≥90，授予金星印记',
      pass: '合格毕业：五准则均分 ≥60，准予出仓',
      fail: '未达毕业线：请补齐缺席准则或提升低分项',
    };
    return m[v];
  }

  snapshot(): string {
    return JSON.stringify([...this.scores.entries()]);
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as [string, number][];
      this.scores = new Map(o.filter(([k]) => (GRAD_CRITERIA as readonly string[]).includes(k)));
      return true;
    } catch {
      return false;
    }
  }
}

// ---- 族0598 时间胶囊（X14926~X14950）----

/** 时间胶囊：入册（上限 50）+ 封存 + 启封（只读快照）+ FNV 指纹。 */
export class TimeCapsule {
  items: string[] = [];
  sealed = false;
  maxItems = 50;
  clamped = 0;

  put(item: string): boolean {
    if (this.sealed || !item.trim() || this.items.length >= this.maxItems) {
      this.clamped += 1;
      return false;
    }
    this.items.push(item.trim());
    return true;
  }

  seal(): boolean {
    if (this.sealed || this.items.length === 0) {
      this.clamped += 1;
      return false;
    }
    this.sealed = true;
    return true;
  }

  /** 启封：仅已封存胶囊可读，返回只读副本。 */
  open(): string[] {
    if (!this.sealed) {
      this.clamped += 1;
      return [];
    }
    return [...this.items];
  }

  static fnv(items: string[]): number {
    let h = 0x811c9dc5;
    const s = items.join('|');
    for (let i = 0; i < s.length; i++) {
      h ^= s.charCodeAt(i);
      h = Math.imul(h, 0x01000193);
    }
    return h >>> 0;
  }

  fingerprint(): number {
    return this.sealed ? TimeCapsule.fnv(this.items) : 0;
  }

  static narrative(): string {
    return '胶囊未封存或为空：先入册纪念品再封存';
  }

  snapshot(): string {
    return JSON.stringify({ i: this.items, s: this.sealed });
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as { i: string[]; s: boolean };
      this.items = o.i;
      this.sealed = o.s;
      return true;
    } catch {
      return false;
    }
  }
}

// ---- 族0599 下一代路线（X14951~X14975）----

export const ROADMAP_PHASES = ['explore', 'design', 'prototype', 'validate', 'commit'] as const;
export type RoadmapPhase = (typeof ROADMAP_PHASES)[number];

/** 下一代路线：主题提名 + 加权投票（0~99 钳制）+ 榜首 + 五阶段推进。 */
export class NextGenRoadmap {
  themes: { name: string; score: number }[] = [];
  phaseIdx = 0;
  clamped = 0;

  static phaseLabel(p: RoadmapPhase): string {
    const m: Record<RoadmapPhase, string> = {
      explore: '探索', design: '设计', prototype: '原型', validate: '验证', commit: '立项',
    };
    return m[p];
  }

  nominate(theme: string): boolean {
    if (!theme.trim() || this.themes.some((t) => t.name === theme.trim())) {
      this.clamped += 1;
      return false;
    }
    this.themes.push({ name: theme.trim(), score: 0 });
    return true;
  }

  vote(theme: string, weight = 1): boolean {
    const t = this.themes.find((x) => x.name === theme.trim());
    if (!t || !Number.isInteger(weight) || weight < 1 || t.score + weight > 99) {
      this.clamped += 1;
      return false;
    }
    t.score += weight;
    return true;
  }

  /** 榜首：并列时取先提名者。 */
  top(): string {
    if (this.themes.length === 0) return '';
    let best = this.themes[0]!;
    for (const t of this.themes) {
      if (t.score > best.score) best = t;
    }
    return best.name;
  }

  /** 阶段推进：至少 2 个提名主题方可立项探索。 */
  advance(): boolean {
    if (this.phaseIdx >= ROADMAP_PHASES.length - 1) {
      this.clamped += 1;
      return false;
    }
    if (this.phaseIdx === 0 && this.themes.length < 2) {
      this.clamped += 1;
      return false;
    }
    this.phaseIdx += 1;
    return true;
  }

  phase(): RoadmapPhase {
    return ROADMAP_PHASES[this.phaseIdx]!;
  }

  static narrative(): string {
    return '路线推进受阻：探索期至少需要 2 个提名主题';
  }

  snapshot(): string {
    return JSON.stringify({ t: this.themes, p: this.phaseIdx });
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as { t: NextGenRoadmap['themes']; p: number };
      if (o.p < 0 || o.p >= ROADMAP_PHASES.length) return false;
      this.themes = o.t;
      this.phaseIdx = o.p;
      return true;
    } catch {
      return false;
    }
  }
}

// ---- 族0600 大收官（X14976~X15000）----

/** 大收官：九波交付 + 15000 项满额判定 + 封版（终态不可逆）。 */
export class GrandFinale {
  wavesUsed = 0;
  waveCap = 9;
  delivered = 0;
  sealed = false;
  clamped = 0;

  deliverWave(items: number): boolean {
    if (this.sealed || this.wavesUsed >= this.waveCap) {
      this.clamped += 1;
      return false;
    }
    if (!Number.isInteger(items) || items < 1 || this.delivered + items > 15000) {
      this.clamped += 1;
      return false;
    }
    this.wavesUsed += 1;
    this.delivered += items;
    return true;
  }

  progress(): number {
    return this.delivered / 15000;
  }

  complete(): boolean {
    return this.delivered >= 15000;
  }

  /** 封版：15000 项满额后方可封版；封版后拒绝一切交付。 */
  seal(): boolean {
    if (this.sealed || !this.complete()) {
      this.clamped += 1;
      return false;
    }
    this.sealed = true;
    return true;
  }

  static narrative(): string {
    return '大收官未满 15000 项：请按波次补齐后再封版';
  }

  snapshot(): string {
    return JSON.stringify({ w: this.wavesUsed, d: this.delivered, s: this.sealed });
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as { w: number; d: number; s: boolean };
      if (o.w < 0 || o.w > this.waveCap || o.d < 0 || o.d > 15000) return false;
      this.wavesUsed = o.w;
      this.delivered = o.d;
      this.sealed = o.s;
      return true;
    } catch {
      return false;
    }
  }
}
