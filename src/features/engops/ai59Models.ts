/**
 * UNREAL-X-15000 · AI-59 协作与防线 逻辑核（领域16 · 族0581~0590 · X14501~X14750）。
 * 三方主责：多会话协作纪律/域验收协议/工具链成熟度/知识沉淀/项目记忆/交接运维/可持续迭代；
 * C 线同口径镜像（族0583 回归防线 / 族0585 长稳测试）与 K 线同口径镜像（族0584 混沌工程）。
 * 全部确定性算法：零 AI、零随机（种子 LCG）、零时钟依赖。
 */

// ---- 族0581 多会话协作纪律（X14501~X14525）----

export const DISCIPLINE_TIERS = ['strict', 'standard', 'loose', 'observe', 'archive'] as const;
export type DisciplineTier = (typeof DISCIPLINE_TIERS)[number];

const TIER_CLAIM_CAP: Record<DisciplineTier, number> = {
  strict: 250, standard: 500, loose: 1000, observe: 0, archive: 0,
};

/** 多会话协作纪律：X-ID 区间认领互斥 + 纪律五档 + 半成品续跑。 */
export class CollabDiscipline {
  claims: { session: string; from: number; to: number }[] = [];
  halfDone = '';
  tier: DisciplineTier = 'standard';
  clamped = 0;
  lastReason = '';

  static tierLabel(t: DisciplineTier): string {
    const m: Record<DisciplineTier, string> = {
      strict: '严纪律', standard: '标准', loose: '宽松', observe: '只读观察', archive: '存档',
    };
    return m[t];
  }

  setTier(t: string): DisciplineTier {
    const found = DISCIPLINE_TIERS.find((x) => x === t);
    if (!found) {
      this.clamped += 1;
      this.lastReason = '未知纪律档：已回退当前档';
      return this.tier;
    }
    this.tier = found;
    return found;
  }

  claim(session: string, from: number, to: number): boolean {
    if (!session || TIER_CLAIM_CAP[this.tier] === 0) {
      this.clamped += 1;
      this.lastReason = '会话为空或当前档只读：认领被拒';
      return false;
    }
    if (!Number.isInteger(from) || !Number.isInteger(to) || from < 1 || to > 15000 || from > to) {
      this.clamped += 1;
      this.lastReason = '区间越界：合法范围 X00001~X15000';
      return false;
    }
    if (to - from + 1 > TIER_CLAIM_CAP[this.tier]) {
      this.clamped += 1;
      this.lastReason = '认领跨度超档位上限：请缩小区间或调档';
      return false;
    }
    for (const c of this.claims) {
      if (c.session === session) {
        this.clamped += 1;
        this.lastReason = '一会话一认领：请先释放旧区间';
        return false;
      }
      if (from <= c.to && c.from <= to) {
        this.clamped += 1;
        this.lastReason = '区间与既有认领冲突：请改用空闲区间';
        return false;
      }
    }
    this.claims.push({ session, from, to });
    return true;
  }

  release(session: string): boolean {
    const i = this.claims.findIndex((c) => c.session === session);
    if (i < 0) {
      this.clamped += 1;
      return false;
    }
    this.claims.splice(i, 1);
    return true;
  }

  covered(): number {
    return this.claims.reduce((n, c) => n + (c.to - c.from + 1), 0);
  }

  /** 智能建议：给定跨度，返回首个空闲起点（无认领时为 1）。 */
  static nextFree(claims: { from: number; to: number }[], span: number): number {
    const sorted = [...claims].sort((a, b) => a.from - b.from);
    let start = 1;
    for (const c of sorted) {
      if (c.from - start >= span) return start;
      start = Math.max(start, c.to + 1);
    }
    return Math.min(start, 15001 - span);
  }

  snapshot(): string {
    return JSON.stringify({ tier: this.tier, claims: this.claims, half: this.halfDone });
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as { tier: DisciplineTier; claims: CollabDiscipline['claims']; half?: string };
      if (!DISCIPLINE_TIERS.includes(o.tier)) return false;
      this.tier = o.tier;
      this.claims = o.claims;
      this.halfDone = o.half ?? '';
      return true;
    } catch {
      return false;
    }
  }
}

// ---- 族0582 域验收协议（X14526~X14550）----

export const ACCEPT_GATES = ['G1', 'G2', 'G3', 'G4'] as const;
export type AcceptGate = (typeof ACCEPT_GATES)[number];

/** 域验收协议：G1~G4 顺序门禁 + 双线（vitest/cargo）门禁并集 + ID 区间审计。 */
export class DomainAcceptance {
  passed = new Set<AcceptGate>();
  records: { gate: AcceptGate; vitest: boolean; cargo: boolean }[] = [];
  veto = false;
  clamped = 0;

  static gateLabel(g: AcceptGate): string {
    const m: Record<AcceptGate, string> = {
      G1: '族内自检', G2: '域验收', G3: '波次门禁', G4: '终验收',
    };
    return m[g];
  }

  /** 失败叙事：禁裸报错。 */
  static narrative(code: 'skip' | 'veto' | 'unknown'): string {
    const m: Record<'skip' | 'veto' | 'unknown', string> = {
      skip: '顺序门禁：前序门未过，不可跳门',
      veto: '双线门禁并集：一线红即否决，请先修复失败线',
      unknown: '未知门禁：仅支持 G1~G4',
    };
    return m[code];
  }

  pass(gate: string, vitest: boolean, cargo: boolean): boolean {
    if (!ACCEPT_GATES.includes(gate as AcceptGate)) {
      this.clamped += 1;
      return false;
    }
    const idx = ACCEPT_GATES.indexOf(gate as AcceptGate);
    for (let i = 0; i < idx; i++) {
      if (!this.passed.has(ACCEPT_GATES[i]!)) {
        this.clamped += 1;
        return false; // 顺序门禁：不可跳门
      }
    }
    if (!vitest || !cargo) {
      this.veto = true;
      return false; // 双线门禁并集：一线红即否决
    }
    this.passed.add(gate as AcceptGate);
    this.records.push({ gate: gate as AcceptGate, vitest, cargo });
    return true;
  }

  /** 域核对：项数 = max-min+1 且 ID 连续无重。 */
  audit(minId: number, maxId: number, items: number[]): boolean {
    if (items.length !== maxId - minId + 1) return false;
    const nums = [...items].sort((a, b) => a - b);
    return nums.every((n, i) => n === minId + i);
  }

  done(): boolean {
    return ACCEPT_GATES.every((g) => this.passed.has(g));
  }

  snapshot(): string {
    return JSON.stringify({ gates: [...this.passed], records: this.records });
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as { gates: AcceptGate[]; records: DomainAcceptance['records'] };
      this.passed = new Set(o.gates.filter((g) => ACCEPT_GATES.includes(g)));
      this.records = o.records;
      return true;
    } catch {
      return false;
    }
  }
}

// ---- 族0583 回归防线（X14551~X14575 · C 线主责，V 线同口径镜像）----

export const REGRESSION_TIERS = ['green', 'yellow', 'red'] as const;
export type RegressionTier = (typeof REGRESSION_TIERS)[number];

/** 回归防线：断言注册表只增不删 + 基线冻结 + 三档判定 + 破坏演练必红。 */
export class RegressionGuard {
  ids: number[] = [];
  frozen = false;
  clamped = 0;

  register(id: number): boolean {
    if (this.frozen) {
      this.clamped += 1;
      return false; // 基线冻结后拒登记
    }
    if (!Number.isInteger(id) || id < 1 || this.ids.includes(id)) {
      this.clamped += 1;
      return false;
    }
    this.ids.push(id);
    return true;
  }

  freeze(): boolean {
    this.frozen = true;
    return this.frozen;
  }

  count(): number {
    return this.ids.length;
  }

  /** 三档判定：不降=green；降幅 <5%=yellow；否则 red。 */
  static verdict(oldScore: number, newScore: number): RegressionTier {
    if (newScore >= oldScore) return 'green';
    if (newScore >= oldScore * 0.95) return 'yellow';
    return 'red';
  }

  /** 破坏演练：注入 -20% 劣化必须判红。 */
  static drill(): boolean {
    return RegressionGuard.verdict(1000, 800) === 'red';
  }

  static narrative(tier: RegressionTier): string {
    const m: Record<RegressionTier, string> = {
      green: '回归全绿：基线无劣化，可合入',
      yellow: '黄色预警：降幅 5% 以内，请复核热路径',
      red: '红色阻断：降幅超 5%，禁止合入并回滚',
    };
    return m[tier];
  }

  snapshot(): string {
    return JSON.stringify({ ids: this.ids, frozen: this.frozen });
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as { ids: number[]; frozen: boolean };
      this.ids = o.ids.filter((n) => Number.isInteger(n) && n >= 1);
      this.frozen = o.frozen;
      return true;
    } catch {
      return false;
    }
  }
}

// ---- 族0584 混沌工程（X14576~X14600 · K 线主责，V 线同口径镜像）----

export const CHAOS_FAULTS = ['panic', 'oom', 'net-drop', 'disk-slow', 'irq-storm'] as const;
export const CHAOS_SEVERITY = ['trace', 'mild', 'steady', 'harsh', 'extreme'] as const;
export type ChaosFault = (typeof CHAOS_FAULTS)[number];
export type ChaosSeverity = (typeof CHAOS_SEVERITY)[number];

/** 混沌工程：故障注入器（预算/爆炸半径/止血开关/确定性调度/可恢复判定）。 */
export class ChaosEngine {
  killSwitch = false;
  budget = 100;
  blastCap = 8;
  injected = 0;
  clamped = 0;

  setBudget(n: number): number {
    this.budget = Math.min(1000, Math.max(1, Math.trunc(n) || 1));
    return this.budget;
  }

  setBlastCap(n: number): number {
    this.blastCap = Math.min(8, Math.max(1, Math.trunc(n) || 1));
    return this.blastCap;
  }

  /** 确定性 LCG 调度：同种子同序列（可重放）。 */
  static schedule(seed: number, count: number): ChaosFault[] {
    let s = seed >>> 0 || 1;
    const out: ChaosFault[] = [];
    for (let i = 0; i < count; i++) {
      s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
      out.push(CHAOS_FAULTS[s % 5]!);
    }
    return out;
  }

  inject(fault: string, severity: string, affected: number): boolean {
    if (this.killSwitch) {
      this.clamped += 1;
      return false; // 止血开关：全局停注
    }
    if (!CHAOS_FAULTS.includes(fault as ChaosFault) || !CHAOS_SEVERITY.includes(severity as ChaosSeverity)) {
      this.clamped += 1;
      return false;
    }
    if (this.injected >= this.budget) {
      this.clamped += 1;
      return false; // 预算熔断
    }
    if (!Number.isInteger(affected) || affected < 1 || affected > this.blastCap) {
      this.clamped += 1;
      return false; // 爆炸半径钳制
    }
    this.injected += 1;
    return true;
  }

  /** 可恢复判定：仅「极端 × panic」需人工介入，其余自动恢复。 */
  static recoverable(fault: ChaosFault, severity: ChaosSeverity): boolean {
    return !(fault === 'panic' && severity === 'extreme');
  }

  static narrative(fault: string): string {
    const m: Record<string, string> = {
      panic: '内核恐慌注入：守护进程将拉起快照恢复',
      oom: '内存耗尽注入：按配额逐级回收后自愈',
      'net-drop': '网络丢包注入：链路预算重算后重建',
      'disk-slow': '磁盘迟滞注入：IO 队列降级为同步直写',
      'irq-storm': '中断风暴注入：合并窗口自动放大',
    };
    return m[fault] ?? '未知故障：拒绝注入并回默认档';
  }

  static faultLabel(f: ChaosFault): string {
    const m: Record<ChaosFault, string> = {
      panic: '恐慌', oom: '内存', 'net-drop': '丢包', 'disk-slow': '迟滞', 'irq-storm': '风暴',
    };
    return m[f];
  }

  snapshot(): string {
    return JSON.stringify({ budget: this.budget, cap: this.blastCap, injected: this.injected });
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as { budget: number; cap: number; injected: number };
      this.budget = o.budget;
      this.blastCap = o.cap;
      this.injected = o.injected;
      return true;
    } catch {
      return false;
    }
  }
}

// ---- 族0585 长稳测试（X14601~X14625 · C 线主责，V 线同口径镜像）----

/** 长稳测试：72h 抽样 + 环形采样窗 + 泄漏/漂移检测 + 断点续跑。 */
export class SoakRunner {
  hours = 72;
  leakBudgetKb = 2048;
  samples: number[] = [];
  checkpoint = 0;
  clamped = 0;

  setHours(h: number): number {
    this.hours = Math.min(720, Math.max(1, Math.trunc(h) || 1));
    return this.hours;
  }

  /** 环形采样：容量 24，NaN 拒绝。 */
  sample(value: number): boolean {
    if (!Number.isFinite(value)) {
      this.clamped += 1;
      return false;
    }
    this.samples.push(value);
    if (this.samples.length > 24) this.samples.shift();
    return true;
  }

  /** 泄漏检测：逐 24h 窗内存增量超预算即计一次泄漏。 */
  leaks(deltas: number[]): number {
    let n = 0;
    for (const d of deltas) {
      if (Number.isFinite(d) && d > this.leakBudgetKb) n += 1;
    }
    return n;
  }

  /** 漂移判定：p95 相对基线偏离 >10% 即漂移。 */
  static drift(baselineP95: number, nowP95: number): boolean {
    if (baselineP95 <= 0) return false;
    return Math.abs(nowP95 - baselineP95) / baselineP95 > 0.1;
  }

  /** 断点续跑：从检查点继续，只接受更大的小时数。 */
  resume(fromHour: number): boolean {
    if (!Number.isInteger(fromHour) || fromHour < this.checkpoint) {
      this.clamped += 1;
      return false;
    }
    this.checkpoint = fromHour;
    return true;
  }

  /** 长稳判定：时长达标 + 无泄漏样本 + 检查点推进到目标。 */
  verdict(): 'pass' | 'fail' {
    if (this.checkpoint < this.hours) return 'fail';
    if (this.leaks(this.samples) > 0) return 'fail';
    return 'pass';
  }

  static narrative(): string {
    return '长稳未达标：请从最近检查点一键续跑，无需重放全时长';
  }

  snapshot(): string {
    return JSON.stringify({ hours: this.hours, cp: this.checkpoint, n: this.samples.length });
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as { hours: number; cp: number; n: number };
      this.hours = o.hours;
      this.checkpoint = o.cp;
      return true;
    } catch {
      return false;
    }
  }
}

// ---- 族0586 工具链成熟度（X14626~X14650）----

export const TOOLCHAIN_TOOLS = ['cargo-ktest', 'vitest', 'tsc', 'gen-unrealx', 'md5-sync'] as const;
export const MATURITY_TIERS = ['experimental', 'beta', 'stable', 'hardened', 'frozen'] as const;
export type MaturityTier = (typeof MATURITY_TIERS)[number];
export type ToolName = (typeof TOOLCHAIN_TOOLS)[number];

/** 工具链成熟度：五工具 × 五档定级 + 冻结终态 + 三线覆盖 + 人工降级叙事。 */
export class ToolchainMaturity {
  tools = new Map<string, MaturityTier>();
  clamped = 0;

  static lineOf(tool: string): 'K' | 'V' | 'C' {
    if (tool === 'cargo-ktest') return 'K';
    if (tool === 'vitest' || tool === 'tsc') return 'V';
    return 'C';
  }

  static tierLabel(t: MaturityTier): string {
    const m: Record<MaturityTier, string> = {
      experimental: '实验', beta: '试用', stable: '稳定', hardened: '加固', frozen: '冻结',
    };
    return m[t];
  }

  register(tool: string, tier: string): boolean {
    if (!TOOLCHAIN_TOOLS.includes(tool as ToolName) || !MATURITY_TIERS.includes(tier as MaturityTier)) {
      this.clamped += 1;
      return false;
    }
    if (this.tools.has(tool)) {
      this.clamped += 1;
      return false; // 已登记：走 upgrade 通道
    }
    this.tools.set(tool, tier as MaturityTier);
    return true;
  }

  /** 只升不降；frozen 为终态。 */
  upgrade(tool: string, tier: string): boolean {
    const cur = this.tools.get(tool);
    if (!cur || !MATURITY_TIERS.includes(tier as MaturityTier)) {
      this.clamped += 1;
      return false;
    }
    const next = tier as MaturityTier;
    if (MATURITY_TIERS.indexOf(next) <= MATURITY_TIERS.indexOf(cur)) {
      this.clamped += 1;
      return false;
    }
    this.tools.set(tool, next);
    return true;
  }

  freeze(tool: string): boolean {
    return this.upgrade(tool, 'frozen');
  }

  coveredLines(): ('K' | 'V' | 'C')[] {
    const lines = new Set<'K' | 'V' | 'C'>();
    for (const t of this.tools.keys()) lines.add(ToolchainMaturity.lineOf(t));
    return [...lines].sort();
  }

  score(): number {
    let s = 0;
    for (const t of this.tools.values()) s += (MATURITY_TIERS.indexOf(t) + 1) * 20;
    return s;
  }

  static fallback(tool: string): string {
    return `工具 ${tool} 缺位：请按人工步骤执行同口径检查后再合入`;
  }

  snapshot(): string {
    return JSON.stringify([...this.tools.entries()]);
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as [string, MaturityTier][];
      this.tools = new Map(o.filter(([k, v]) => TOOLCHAIN_TOOLS.includes(k as ToolName) && MATURITY_TIERS.includes(v)));
      return true;
    } catch {
      return false;
    }
  }
}

// ---- 族0587 知识沉淀（X14651~X14675）----

/** 知识沉淀：族号索引知识条目（去重/标签/搜索/净身导出/ADR 连号）。 */
export class KnowledgeBase {
  entries: { family: number; title: string; tags: string[] }[] = [];
  adrNext = 1;
  clamped = 0;

  /** 族号归一：'族0581' → 581；非法 → 0。 */
  static normalizeFamily(s: string): number {
    const m = /^族0*(\d{1,3})$/.exec(s.trim());
    if (!m) return 0;
    const n = Number(m[1]);
    return n >= 1 && n <= 600 ? n : 0;
  }

  add(family: number, title: string, tags: string[] = []): boolean {
    if (!Number.isInteger(family) || family < 1 || family > 600 || !title.trim()) {
      this.clamped += 1;
      return false;
    }
    if (this.entries.some((e) => e.family === family)) {
      this.clamped += 1;
      return false; // 一族一条：去重
    }
    this.entries.push({ family, title: title.trim(), tags });
    return true;
  }

  search(q: string): typeof this.entries {
    const needle = q.trim().toLowerCase();
    if (!needle) return [];
    return this.entries.filter((e) => e.title.toLowerCase().includes(needle));
  }

  byTag(tag: string): typeof this.entries {
    return this.entries.filter((e) => e.tags.includes(tag));
  }

  /** ADR 连号：每次递增，不重号。 */
  adr(): number {
    return this.adrNext++;
  }

  /** 净身导出：剔除内部路径痕迹。 */
  exportSanitized(): string {
    return JSON.stringify(this.entries.map((e) => ({ family: e.family, title: e.title, tags: e.tags })));
  }
}

// ---- 族0588 项目记忆（X14676~X14700）----

/** 项目记忆：FNV 哈希链（防篡改）+ 去重追加 + 净身导出。 */
export class ProjectMemory {
  chain: { hash: number; ai: number; family: number; items: number }[] = [];
  clamped = 0;

  static fnv(data: string): number {
    let h = 0x811c9dc5;
    for (let i = 0; i < data.length; i++) {
      h ^= data.charCodeAt(i);
      h = Math.imul(h, 0x01000193);
    }
    return h >>> 0;
  }

  append(ai: number, family: number, items: number): boolean {
    if (!Number.isInteger(ai) || ai < 1 || ai > 60 || !Number.isInteger(family) || family < 1 || family > 600 || items !== 25) {
      this.clamped += 1;
      return false;
    }
    if (this.chain.some((e) => e.ai === ai && e.family === family)) {
      this.clamped += 1;
      return false; // 同 AI 同族只记一次
    }
    const prev = this.chain.length ? this.chain[this.chain.length - 1]!.hash : 0x811c9dc5;
    const hash = ProjectMemory.fnv(`${prev}:${ai}:${family}:${items}`);
    this.chain.push({ hash, ai, family, items });
    return true;
  }

  /** 链校验：重算全链哈希一致即未篡改。 */
  verify(): boolean {
    let prev = 0x811c9dc5;
    for (const e of this.chain) {
      if (e.hash !== ProjectMemory.fnv(`${prev}:${e.ai}:${e.family}:${e.items}`)) return false;
      prev = e.hash;
    }
    return true;
  }

  tally(): number {
    return this.chain.reduce((n, e) => n + e.items, 0);
  }

  exportSanitized(): string {
    return JSON.stringify(this.chain.map((e) => ({ ai: e.ai, family: e.family, items: e.items })));
  }
}

// ---- 族0589 交接运维（X14701~X14725）----

export const HANDOVER_CHECKLIST = ['代码', '文档', '基线', '门禁', '未决'] as const;
export type HandoverItem = (typeof HANDOVER_CHECKLIST)[number];

/** 交接运维：W→W 交接单（五项清单/确认制/SLA/巡检/回滚净身）。 */
export class HandoverOps {
  cards: { id: number; from: string; to: string; done: string[]; ack: boolean }[] = [];
  clamped = 0;

  create(from: string, to: string): number {
    if (!from || !to || from === to) {
      this.clamped += 1;
      return 0;
    }
    const id = this.cards.length + 1;
    this.cards.push({ id, from, to, done: [], ack: false });
    return id;
  }

  check(id: number, item: string): boolean {
    const c = this.cards.find((x) => x.id === id);
    if (!c || !HANDOVER_CHECKLIST.includes(item as HandoverItem) || c.done.includes(item)) {
      this.clamped += 1;
      return false;
    }
    c.done.push(item);
    return true;
  }

  /** 确认制：五项清单全勾才可确认交接。 */
  acknowledge(id: number): boolean {
    const c = this.cards.find((x) => x.id === id);
    if (!c || c.done.length < HANDOVER_CHECKLIST.length) {
      this.clamped += 1;
      return false;
    }
    c.ack = true;
    return true;
  }

  pending(): typeof this.cards {
    return this.cards.filter((c) => !c.ack);
  }

  /** SLA：交接确认 ≤3 天为达标（钳制 0~14）。 */
  static sla(days: number): boolean {
    const d = Math.min(14, Math.max(0, days));
    return d <= 3;
  }

  /** 五点巡检：全绿才通过。 */
  static patrol(items: boolean[]): boolean {
    return items.length === HANDOVER_CHECKLIST.length && items.every(Boolean);
  }

  /** 回滚净身：整单撤销，不留残档。 */
  rollback(id: number): boolean {
    const i = this.cards.findIndex((x) => x.id === id);
    if (i < 0) {
      this.clamped += 1;
      return false;
    }
    this.cards.splice(i, 1);
    return true;
  }
}

// ---- 族0590 可持续迭代（X14726~X14750）----

export const ITERATION_WAVES = ['W0', 'W1', 'W2', 'W3', 'W4', 'W5', 'W6', 'W7'] as const;

/** 可持续迭代：波次顺序推进 + 燃尽稳定 + 债务 20% 上限 + 创新 10% 保底。 */
export class SustainIteration {
  waveIdx = 0;
  delivered = 0;
  planned = 0;
  debt: { id: number; points: number }[] = [];
  clamped = 0;

  /** 燃尽比：计划为 0 时视 0（未开工）。 */
  burn(): number {
    return this.planned <= 0 ? 0 : this.delivered / this.planned;
  }

  /** 节奏稳定：燃尽比落在 0.9~1.1。 */
  static stable(burn: number): boolean {
    return burn >= 0.9 && burn <= 1.1;
  }

  plan(items: number): boolean {
    if (!Number.isInteger(items) || items < 0 || items > 15000) {
      this.clamped += 1;
      return false;
    }
    this.planned = items;
    return true;
  }

  deliver(items: number): boolean {
    if (!Number.isInteger(items) || items < 0) {
      this.clamped += 1;
      return false;
    }
    this.delivered += items;
    return true;
  }

  /** 波次推进：出口判据 = 本波交付 ≥ 计划。 */
  advance(): boolean {
    if (this.waveIdx >= ITERATION_WAVES.length - 1) {
      this.clamped += 1;
      return false; // W7 之后无下一波
    }
    if (this.delivered < this.planned) {
      this.clamped += 1;
      return false;
    }
    this.waveIdx += 1;
    this.delivered = 0;
    this.planned = 0;
    return true;
  }

  wave(): string {
    return ITERATION_WAVES[this.waveIdx]!;
  }

  /** 债务登记：去重 + 总分不超过波容量 20%。 */
  takeDebt(id: number, points: number, waveCapacity: number): boolean {
    if (!Number.isInteger(id) || id < 1 || !Number.isInteger(points) || points < 1 || points > 5) {
      this.clamped += 1;
      return false;
    }
    if (this.debt.some((d) => d.id === id)) {
      this.clamped += 1;
      return false;
    }
    const total = this.debt.reduce((n, d) => n + d.points, 0) + points;
    if (waveCapacity <= 0 || total > Math.floor(waveCapacity * 0.2)) {
      this.clamped += 1;
      return false;
    }
    this.debt.push({ id, points });
    return true;
  }

  debtLoad(waveCapacity: number): number {
    if (waveCapacity <= 0) return 0;
    return this.debt.reduce((n, d) => n + d.points, 0) / waveCapacity;
  }

  /** 创新保底：每波预留 ≥10% 容量。 */
  static innovationReserve(waveCapacity: number): number {
    return Math.floor(waveCapacity * 0.1);
  }

  snapshot(): string {
    return JSON.stringify({ w: this.waveIdx, d: this.delivered, p: this.planned });
  }

  restore(s: string): boolean {
    try {
      const o = JSON.parse(s) as { w: number; d: number; p: number };
      if (o.w < 0 || o.w >= ITERATION_WAVES.length) return false;
      this.waveIdx = o.w;
      this.delivered = o.d;
      this.planned = o.p;
      return true;
    } catch {
      return false;
    }
  }
}
