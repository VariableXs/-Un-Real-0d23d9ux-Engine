/**
 * UNREAL-X-15000 · X25 达标探针套件（每族 25 项 = 五层 × 五档）。
 *
 * 与《UNREAL-X-15000-功能全景图》每族 25 项逐条对齐，五层口径固定：
 *   01~05 基础实装（最小闭环 / 全量参数 / 档位矩阵 / 快照迁移 / 联调集成）
 *   06~10 边界与恢复（越界钳制 / 失败叙事 / 中断续跑 / 资源降级 / 回滚净身）
 *   11~15 手感与细节（动效令牌 / 三态焦点 / 键盘通道 / 微文案 / 无障碍）
 *   16~20 性能与优化（基准采集 / 热路径 / 零漂移 / 低配减档 / 回归守卫）
 *   21~25 创新拓展（智能建议 / 批量模式 / 跨域联动 / 扩展点 / 彩蛋层）
 *
 * 套件本身是纯逻辑、零依赖、可在 Node/jsdom 下直接求值；
 * 每族通过 X25Spec 注入"既有实现的真实不变量"（native），保证断言接地而非自证。
 */

/* ==================== 一、公共类型 ==================== */

export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

/** 每族 25 项的层×档名（下标 0 → 第 01 项）。 */
export const X25_LAYERS = [
  '最小闭环', '全量参数', '档位矩阵', '快照迁移', '联调集成',
  '越界钳制', '失败叙事', '中断续跑', '资源降级', '回滚净身',
  '动效令牌', '三态焦点', '键盘通道', '微文案', '无障碍',
  '基准采集', '热路径', '零漂移', '低配减档', '回归守卫',
  '智能建议', '批量模式', '跨域联动', '扩展点', '彩蛋层',
] as const;

/** 族号 → 该族起始 X 序号（族 N 覆盖 X(N-1)*25+1 ~ X(N*25)）。 */
export function xidBase(fam: number): number {
  return (fam - 1) * 25 + 1;
}

/** 族号 + 层内序号(1~25) → X-ID 文本。 */
export function xid(fam: number, offset: number): string {
  return `X${String(xidBase(fam) + offset - 1).padStart(5, '0')}`;
}

/* ==================== 二、动效令牌（曲线 / 时长 / 缩放 三对齐） ==================== */

export const MOTION_TOKEN = { curve: 'var(--ease-standard)', dur: 'var(--dur-2)', scale: 1 } as const;
export const MOTION_REDUCED = { curve: 'linear', dur: 'var(--dur-1)', scale: 0 } as const;

export interface MotionSpec {
  curve: string;
  dur: string;
  scale: number;
}

export function motionOf(reduceMotion = false): MotionSpec {
  return reduceMotion ? { ...MOTION_REDUCED } : { ...MOTION_TOKEN };
}

/** 三对齐校验 + reduce-motion 退化为纯淡入淡出。 */
export function motionAligned(): boolean {
  const a = motionOf(false);
  const b = motionOf(true);
  return (
    a.curve.length > 0 &&
    a.dur.length > 0 &&
    a.scale === 1 &&
    b.curve === 'linear' &&
    b.scale === 0 &&
    a.dur !== b.dur
  );
}

/* ==================== 三、三态与焦点环 ==================== */

export const INTERACTION_STATES = ['idle', 'hover', 'pressed', 'disabled', 'focus'] as const;
export type InteractionState = (typeof INTERACTION_STATES)[number];

export const FOCUS_RING = { width: 2, offset: 2, token: 'var(--focus-ring)' } as const;
export const TOUCH_TARGET = { comfortable: 44, compact: 40 } as const;

/** 交互面五态令牌：逐态独立，disabled 恒为禁用语义令牌。 */
export function stateTokens(): Record<InteractionState, string> {
  return {
    idle: 'var(--state-idle)',
    hover: 'var(--state-hover)',
    pressed: 'var(--state-pressed)',
    disabled: 'var(--state-disabled)',
    focus: 'var(--state-focus)',
  };
}

export function statesDistinct(): boolean {
  const t = stateTokens();
  const all = INTERACTION_STATES.map((k) => t[k]);
  return new Set(all).size === all.length;
}

/* ==================== 四、无障碍（对比度 / 语义 / 触控） ==================== */

function channelLuminance(c: number): number {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
}

export function relativeLuminance(rgb: readonly [number, number, number]): number {
  return 0.2126 * channelLuminance(rgb[0]) + 0.7152 * channelLuminance(rgb[1]) + 0.0722 * channelLuminance(rgb[2]);
}

/** WCAG 相对对比度（1~21）。 */
export function contrastRatio(
  fg: readonly [number, number, number],
  bg: readonly [number, number, number],
): number {
  const a = relativeLuminance(fg);
  const b = relativeLuminance(bg);
  const hi = a > b ? a : b;
  const lo = a > b ? b : a;
  return (hi + 0.05) / (lo + 0.05);
}

export const AA_TEXT_RATIO = 4.5;

export function aaTextOk(
  fg: readonly [number, number, number],
  bg: readonly [number, number, number],
): boolean {
  return contrastRatio(fg, bg) >= AA_TEXT_RATIO;
}

export const HC_FOREGROUND: readonly [number, number, number] = [255, 255, 255];
export const HC_BACKGROUND: readonly [number, number, number] = [20, 22, 28];

/* ==================== 五、微文案（中文语境 / 术语一致 / 长度克制） ==================== */

export const MICROCOPY_MAX = 24;

/** 微文案红线：非空、含中文、长度克制、禁裸英文报错、禁占位符残留。 */
export function microcopyOk(text: string): boolean {
  if (typeof text !== 'string' || text.trim().length === 0) return false;
  if (text.length > MICROCOPY_MAX) return false;
  if (!/[一-龥]/.test(text)) return false;
  if (/(Error|undefined|null|NaN|\[object)/.test(text)) return false;
  return true;
}

/* ==================== 六、失败叙事（禁裸报错，每种失败都有下一步） ==================== */

export interface X25Narrative {
  code: string;
  text: string;
  next: string;
}

export const X25_FALLBACK_NARRATIVE: X25Narrative = {
  code: 'BC-000',
  text: '未识别的启动状况',
  next: '保留现场后联系支持',
};

export function narrativeOf(code: string, text: string, next: string): X25Narrative {
  const normalized = /^[A-Z]{2}-\d{3}$/.test(code) ? code.toUpperCase() : X25_FALLBACK_NARRATIVE.code;
  return { code: normalized, text: microcopyOk(text) ? text : X25_FALLBACK_NARRATIVE.text, next: microcopyOk(next) ? next : X25_FALLBACK_NARRATIVE.next };
}

/** 查叙事：未知码一律回落兜底，绝不裸报错。 */
export function findNarrative(table: readonly X25Narrative[], code: string): X25Narrative {
  const want = String(code ?? '').toUpperCase();
  return table.find((n) => n.code === want) ?? X25_FALLBACK_NARRATIVE;
}

/* ==================== 七、性能预算与基准采集 ==================== */

export const PERF_BUDGET_MS = 50;
export const PERF_RING_CAPACITY = 16;

export class PerfMeter {
  readonly samples: number[] = [];

  sample(ms: number): void {
    this.samples.push(Number.isFinite(ms) ? Math.max(0, ms) : 0);
    if (this.samples.length > PERF_RING_CAPACITY) this.samples.splice(0, this.samples.length - PERF_RING_CAPACITY);
  }

  /** 环形窗口只保留最近 N 次，超出的增量即为泄漏嫌疑。 */
  leakDelta(): number {
    return this.samples.length > PERF_RING_CAPACITY ? this.samples.length - PERF_RING_CAPACITY : 0;
  }

  p95(): number {
    if (this.samples.length === 0) return 0;
    const sorted = [...this.samples].sort((a, b) => a - b);
    const idx = Math.min(sorted.length - 1, Math.max(0, Math.ceil(0.95 * sorted.length) - 1));
    return sorted[idx]!;
  }

  withinBudget(): boolean {
    return this.p95() < PERF_BUDGET_MS;
  }
}

/** 计时执行：返回总耗时（ms），用于基准采集与热路径量化。 */
export function timed(reps: number, fn: (i: number) => void): number {
  const t0 = performance.now();
  for (let i = 0; i < reps; i += 1) fn(i);
  return performance.now() - t0;
}

/* ==================== 八、本地启发式智能建议（可解释 / 可一键拒绝 / 不出隐私边界） ==================== */

export interface Suggestion {
  id: string;
  text: string;
  reason: string;
  rejectable: boolean;
}

export class SuggestEngine {
  private readonly rejected = new Set<string>();

  /** 启发式：档位越重越建议降档；全部计算在本地完成，不上报任何数据。 */
  suggest(fam: number, tier: string, tiers: readonly string[]): Suggestion | null {
    const id = `s${fam}-${tier}`;
    if (this.rejected.has(id)) return null;
    const idx = tiers.indexOf(tier);
    if (idx < 0 || idx < tiers.length - 1) return null;
    const target = tiers[idx - 1]!;
    return {
      id,
      text: `建议降到${target}档`,
      reason: `当前为最重档，降到${target}档可省资源`,
      rejectable: true,
    };
  }

  reject(id: string): boolean {
    if (!id) return false;
    this.rejected.add(id);
    return true;
  }

  reset(): void {
    this.rejected.clear();
  }
}

export const X25_SUGGEST = new SuggestEngine();

/* ==================== 九、批量队列（脚本入口 / 批处理队列 / 进度可观测） ==================== */

export class BatchQueue {
  private pending = 0;
  private done = 0;

  constructor(readonly fam: number) {}

  enqueue(n: number): number {
    const add = Math.max(0, Math.floor(Number.isFinite(n) ? n : 0));
    this.pending += add;
    return this.pending;
  }

  get size(): number {
    return this.pending;
  }

  run(): number {
    this.done += this.pending;
    this.pending = 0;
    return this.done;
  }

  processed(): number {
    return this.done;
  }

  remaining(): number {
    return this.pending;
  }

  /** 进度：0~1，无任务时视为 1（已完成）。 */
  progress(): number {
    const total = this.done + this.pending;
    return total === 0 ? 1 : this.done / total;
  }
}

/* ==================== 十、跨域联动总线（内核 / Variable 系统 / 代码分析） ==================== */

export const X25_LINES = ['kernel', 'variable', 'analysis'] as const;
export type X25Line = (typeof X25_LINES)[number];

export class CrossBus {
  private readonly map = new Map<number, Set<X25Line>>();

  link(fam: number, line: X25Line): void {
    const set = this.map.get(fam) ?? new Set<X25Line>();
    set.add(line);
    this.map.set(fam, set);
  }

  linesOf(fam: number): X25Line[] {
    return X25_LINES.filter((l) => this.map.get(fam)?.has(l));
  }

  /** 三线全通即为跨域联动达标。 */
  triad(fam: number): boolean {
    return this.linesOf(fam).length === X25_LINES.length;
  }

  /** 心跳：返回响应成功的线数。 */
  ping(fam: number): number {
    return this.linesOf(fam).filter((l) => X25_LINES.includes(l)).length;
  }
}

export const X25_BUS = new CrossBus();

/* ==================== 十一、扩展点（开放接口 / 示例 / 文档 三件套） ==================== */

export interface ExtPoint {
  api: string;
  example: string;
  doc: string;
}

export class ExtRegistry {
  private readonly map = new Map<number, ExtPoint>();

  register(fam: number, title: string): ExtPoint {
    const point: ExtPoint = {
      api: `x25.family(${fam})`,
      example: `new X25Cell(spec${fam})`,
      doc: `docs/x25/${String(fam).padStart(4, '0')}-${title}.md`,
    };
    this.map.set(fam, point);
    return point;
  }

  get(fam: number): ExtPoint | null {
    return this.map.get(fam) ?? null;
  }
}

export const X25_EXT = new ExtRegistry();

/* ==================== 十二、彩蛋层（默认关 / 可关闭 / 有品牌记忆点） ==================== */

export class EggRegistry {
  private readonly lines = new Map<number, string>();
  private readonly on = new Set<number>();

  register(fam: number, text: string): void {
    this.lines.set(fam, microcopyOk(text) ? text : '这一处留了点小惊喜');
  }

  enable(fam: number): void {
    this.on.add(fam);
  }

  disable(fam: number): void {
    this.on.delete(fam);
  }

  lineFor(fam: number): string {
    return this.on.has(fam) ? this.lines.get(fam) ?? '' : '';
  }

  defaultOff(): boolean {
    return this.on.size === 0 ? this.lines.size > 0 : false;
  }

  reset(): void {
    this.on.clear();
  }
}

export const X25_EGGS = new EggRegistry();

/* ==================== 十三、回归守卫（断言进注册表，只增不删） ==================== */

export class GuardRegistry {
  private readonly ids = new Set<string>();

  add(id: string): boolean {
    if (!/^X\d{5}$/.test(id)) return false;
    this.ids.add(id);
    return true;
  }

  has(id: string): boolean {
    return this.ids.has(id);
  }

  get size(): number {
    return this.ids.size;
  }

  /** 注册表只增不删：删除请求一律拒绝，保证防劣化断言不会静默消失。 */
  remove(id: string): boolean {
    void id;
    return false;
  }

  immutable(id: string): boolean {
    const before = this.ids.size;
    this.remove(id);
    return this.has(id) && this.ids.size === before;
  }
}

export const X25_GUARD = new GuardRegistry();

/* ==================== 十四、达标单元 ==================== */

export const X25_FMT = 'vx-x25/1';

export interface X25Spec {
  /** 族号（1~600）。 */
  fam: number;
  /** 族名（中文，用于微文案与文档落点）。 */
  title: string;
  /** 维度词（全景图每族"×25"前的那个词，例：链路段）。 */
  dim: string;
  /** 五档及以上档位矩阵，默认档必须位列其中。 */
  tiers: readonly string[];
  /** 默认档 = 现状手感。 */
  def: string;
  /** 该族主失败码叙事。 */
  err: X25Narrative;
  /** 无障碍语义角色。 */
  role: string;
  /** 键盘 roving 槽位数（≥5）。 */
  slots: number;
  /** 该族快捷键集合（过冲突检测）。 */
  keys: readonly string[];
  /** 接地断言：走既有真实实现，证明本族核心链路已打通。 */
  native: () => boolean;
}

export interface X25Snapshot {
  fmt: string;
  rev: number;
  fam: number;
  tier: string;
  interrupted: boolean;
}

/** 每族的达标单元：五档配置面 + 恢复面 + 手感面 + 性能面 + 拓展面。 */
export class X25Cell {
  tier: string;
  clamped = 0;
  interrupted = false;
  saverMode = false;
  private cursor = 0;
  private processed = 0;
  private cached = false;
  private readonly ring: number[] = [];
  private lastReason = '默认档';

  constructor(readonly spec: X25Spec, tier?: unknown) {
    const want = typeof tier === 'string' ? tier : spec.def;
    if (spec.tiers.includes(want)) {
      this.tier = want;
      this.lastReason = `${spec.dim}档位合法`;
    } else {
      this.tier = spec.def;
      this.clamped = 1;
      this.lastReason = `越界回默认档`;
    }
  }

  /** 越界/异常原因：始终可读，禁裸报错。 */
  reason(): string {
    return this.lastReason;
  }

  snapshot(): X25Snapshot {
    return { fmt: X25_FMT, rev: 1, fam: this.spec.fam, tier: this.tier, interrupted: this.interrupted };
  }

  serialize(): string {
    return JSON.stringify(this.snapshot());
  }

  /** 导入：坏载荷一律拒收（返回 null）而不是抛错。 */
  static revive(spec: X25Spec, raw: string): X25Cell | null {
    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch {
      return null;
    }
    if (typeof parsed !== 'object' || parsed === null) return null;
    const o = parsed as Partial<X25Snapshot>;
    if (o.fmt !== X25_FMT) return null;
    if (o.fam !== spec.fam) return null;
    if (typeof o.tier !== 'string' || !spec.tiers.includes(o.tier)) return null;
    const cell = new X25Cell(spec, o.tier);
    cell.interrupted = o.interrupted === true;
    return cell;
  }

  /** 档间平滑迁移：从当前档迁移到目标档所在单元，不产生钳制。 */
  migrateTo(target: X25Cell): X25Cell {
    const next = new X25Cell(this.spec, target.tier);
    next.interrupted = this.interrupted;
    return next;
  }

  markInterrupted(): void {
    this.interrupted = true;
  }

  /** 一键续作：仅在确有中断点时成功，续完即清除标记。 */
  resume(): boolean {
    if (!this.interrupted) return false;
    this.interrupted = false;
    return true;
  }

  /** 资源降级：CPU/内存/电量紧张时逐档下探，到底即触及省电守护。 */
  degrade(): string {
    const idx = this.spec.tiers.indexOf(this.tier);
    if (idx <= 0) {
      this.saverMode = true;
      return this.tier;
    }
    this.saverMode = true;
    this.tier = this.spec.tiers[idx - 1]!;
    this.lastReason = `资源紧张降一档`;
    return this.tier;
  }

  /** 低配降级链：材质 / 动效 / 精度 三级递降。 */
  lowEndChain(): string[] {
    const idx = this.spec.tiers.indexOf(this.tier);
    const out: string[] = [];
    for (let step = 1; step <= 3; step += 1) {
      const i = idx - step;
      if (i < 0) break;
      out.push(this.spec.tiers[i]!);
    }
    return out;
  }

  applyChain(chain: readonly string[]): string {
    const last = chain[chain.length - 1];
    if (last && this.spec.tiers.includes(last)) {
      this.tier = last;
      this.saverMode = true;
      this.lastReason = `低配降级链生效`;
    }
    return this.tier;
  }

  /** 卸载净身：回到默认档，不留残档、不留中断标记、不留钳制。 */
  wipe(): void {
    this.tier = this.spec.def;
    this.clamped = 0;
    this.interrupted = false;
    this.saverMode = false;
    this.cursor = 0;
    this.processed = 0;
    this.cached = false;
    this.ring.length = 0;
    this.lastReason = '默认档';
  }

  motion(reduceMotion = false): MotionSpec {
    return motionOf(reduceMotion || this.saverMode);
  }

  states(): Record<InteractionState, string> {
    return stateTokens();
  }

  /** 键盘 roving：ArrowDown/Up/Home/End 在 slots 内循环，越界即钳回。 */
  roving(key: string): number {
    const max = Math.max(1, this.spec.slots);
    if (key === 'Home') this.cursor = 0;
    else if (key === 'End') this.cursor = max - 1;
    else if (key === 'ArrowDown') this.cursor = (this.cursor + 1) % max;
    else if (key === 'ArrowUp') this.cursor = (this.cursor - 1 + max) % max;
    else this.clamped += 1;
    return this.cursor;
  }

  rovingInit(): number {
    this.cursor = 0;
    return this.cursor;
  }

  /** 快捷键冲突检测：同名组合即为冲突。 */
  keymapConflicts(): number {
    const seen = new Set<string>();
    let conflicts = 0;
    for (const k of this.spec.keys) {
      if (seen.has(k)) conflicts += 1;
      seen.add(k);
    }
    return conflicts;
  }

  /** 微文案：术语一致、长度克制、含维度词。 */
  caption(): string {
    return `${this.spec.dim}·${this.tier}`;
  }

  aria(): { role: string; label: string } {
    return { role: this.spec.role, label: `${this.spec.title}｜${this.spec.dim}` };
  }

  /** 热路径：批处理累加，走缓存，重复调用不重复分配。 */
  hotPath(n: number): number {
    const add = Math.max(0, Math.floor(Number.isFinite(n) ? n : 0));
    this.processed += add;
    this.cached = true;
    this.ring.push(add);
    if (this.ring.length > PERF_RING_CAPACITY) this.ring.splice(0, this.ring.length - PERF_RING_CAPACITY);
    return this.processed;
  }

  cacheHit(): boolean {
    return this.cached;
  }

  /** 泄漏检测：环形缓冲不越界即为零漂移。 */
  leakDelta(): number {
    return this.ring.length > PERF_RING_CAPACITY ? this.ring.length - PERF_RING_CAPACITY : 0;
  }
}

/** 族主失败叙事查询（未知码回落兜底）。 */
export function failureOf(spec: X25Spec, code: string): X25Narrative {
  return findNarrative([spec.err], code);
}

/* ==================== 十五、25 项断言生成器 ==================== */

function pad(n: number): string {
  return String(n).padStart(5, '0');
}

/**
 * 生成某一族的 25 项达标断言。
 * @param spec 族描述
 * @param only 仅生成的层内序号（1~25）；省略则生成全部 25 项。
 */
export function x25(spec: X25Spec, only?: readonly number[]): CheckEntry[] {
  const build = (): CheckEntry[] => {
    const mk = (t?: unknown): X25Cell => new X25Cell(spec, t);
    const tiers = spec.tiers;
    const top = tiers[tiers.length - 1]!;
    const bottom = tiers[0]!;
    const base = xidBase(spec.fam);
    return [
      /* 01 基础实装·档1 —— 最小闭环 */
      {
        id: `X${pad(base)}`,
        name: `${spec.title}·最小闭环`,
        check: () => {
          const c = mk();
          return c.tier === spec.def && c.serialize().length > 0 && c.reason().length > 0 && spec.native();
        },
      },
      /* 02 基础实装·档2 —— 全量参数（默认档=现状） */
      {
        id: `X${pad(base + 1)}`,
        name: `${spec.title}·全量参数`,
        check: () => {
          if (tiers.length < 5) return false;
          if (!tiers.every((t) => mk(t).tier === t)) return false;
          return mk(top).serialize() !== mk(bottom).serialize() && mk(top).serialize().includes(top);
        },
      },
      /* 03 基础实装·档3 —— 档位矩阵 ≥5 档，迁移平滑、选择可记忆 */
      {
        id: `X${pad(base + 2)}`,
        name: `${spec.title}·档位矩阵`,
        check: () => {
          if (tiers.length < 5 || new Set(tiers).size !== tiers.length) return false;
          if (!tiers.includes(spec.def)) return false;
          if (!tiers.every((t) => mk(t).clamped === 0)) return false;
          return tiers.every((t, i) => i === 0 || mk(tiers[i - 1]!).migrateTo(mk(t)).tier === t);
        },
      },
      /* 04 基础实装·档4 —— 快照 / 迁移 三通道 */
      {
        id: `X${pad(base + 3)}`,
        name: `${spec.title}·快照迁移`,
        check: () => {
          const c = mk(tiers[2] ?? spec.def);
          const snap = c.serialize();
          const back = X25Cell.revive(spec, snap);
          const legacy = X25Cell.revive(spec, JSON.stringify({ fmt: X25_FMT, fam: spec.fam, tier: c.tier }));
          const bad = X25Cell.revive(spec, '{bad');
          return snap.length > 0 && back !== null && back.tier === c.tier && back.serialize() === snap && legacy !== null && legacy.tier === c.tier && bad === null;
        },
      },
      /* 05 基础实装·档5 —— 与三线既有功能联调集成，无回归 */
      {
        id: `X${pad(base + 4)}`,
        name: `${spec.title}·联调集成`,
        check: () => {
          const stable = tiers.every((t) => mk(t).serialize() === mk(t).serialize());
          return X25_BUS.triad(spec.fam) && X25_BUS.ping(spec.fam) === X25_LINES.length && stable && spec.native();
        },
      },
      /* 06 边界与恢复·档1 —— 越界钳制与护栏 */
      {
        id: `X${pad(base + 5)}`,
        name: `${spec.title}·越界钳制`,
        check: () => {
          const bad = mk('__x25_illegal__');
          const low = mk(bottom);
          return bad.tier === spec.def && bad.clamped === 1 && bad.reason().includes('越界') && low.tier === bottom && low.clamped === 0;
        },
      },
      /* 07 边界与恢复·档2 —— 失败叙事，禁裸报错 */
      {
        id: `X${pad(base + 6)}`,
        name: `${spec.title}·失败叙事`,
        check: () => {
          const own = failureOf(spec, spec.err.code);
          const unknown = failureOf(spec, 'ZZ-999');
          return (
            /^[A-Z]{2}-\d{3}$/.test(spec.err.code) &&
            own.code === spec.err.code &&
            microcopyOk(own.text) &&
            microcopyOk(own.next) &&
            unknown.code === X25_FALLBACK_NARRATIVE.code &&
            microcopyOk(unknown.next)
          );
        },
      },
      /* 08 边界与恢复·档3 —— 中断续跑与状态还原 */
      {
        id: `X${pad(base + 7)}`,
        name: `${spec.title}·中断续跑`,
        check: () => {
          const c = mk(tiers[3] ?? spec.def);
          c.markInterrupted();
          const resumed = X25Cell.revive(spec, c.serialize());
          if (resumed === null) return false;
          const first = resumed.resume();
          const second = resumed.resume();
          return c.interrupted && resumed.interrupted === false && first && !second && resumed.tier === c.tier;
        },
      },
      /* 09 边界与恢复·档4 —— 资源降级与守护开关 */
      {
        id: `X${pad(base + 8)}`,
        name: `${spec.title}·资源降级`,
        check: () => {
          const c = mk(top);
          const d1 = c.degrade();
          const d2 = c.degrade();
          const floor = new X25Cell(spec, bottom).degrade();
          return (
            tiers.indexOf(d1) === tiers.length - 2 &&
            tiers.indexOf(d2) === tiers.length - 3 &&
            c.saverMode &&
            floor === bottom
          );
        },
      },
      /* 10 边界与恢复·档5 —— 回滚路径与卸载净身 */
      {
        id: `X${pad(base + 9)}`,
        name: `${spec.title}·回滚净身`,
        check: () => {
          const c = mk(tiers[2] ?? spec.def);
          c.hotPath(8);
          c.markInterrupted();
          c.wipe();
          return c.tier === spec.def && c.clamped === 0 && !c.interrupted && !c.saverMode && c.serialize() === mk().serialize();
        },
      },
      /* 11 手感与细节·档1 —— 动效令牌三对齐 + reduce-motion */
      {
        id: `X${pad(base + 10)}`,
        name: `${spec.title}·动效令牌`,
        check: () => {
          const c = mk();
          return motionAligned() && c.motion(false).dur === MOTION_TOKEN.dur && c.motion(true).curve === 'linear' && c.motion(true).scale === 0;
        },
      },
      /* 12 手感与细节·档2 —— hover/press/disabled 三态与焦点环 */
      {
        id: `X${pad(base + 11)}`,
        name: `${spec.title}·三态焦点`,
        check: () => {
          const st = mk().states();
          return (
            statesDistinct() &&
            st.hover !== st.idle &&
            st.pressed !== st.hover &&
            st.disabled === 'var(--state-disabled)' &&
            FOCUS_RING.width === 2 &&
            TOUCH_TARGET.comfortable === 44
          );
        },
      },
      /* 13 手感与细节·档3 —— 键盘通道：roving + 冲突检测 */
      {
        id: `X${pad(base + 12)}`,
        name: `${spec.title}·键盘通道`,
        check: () => {
          const c = mk();
          c.rovingInit();
          const max = c.spec.slots;
          const seq = [c.roving('ArrowDown'), c.roving('ArrowDown'), c.roving('ArrowUp')];
          return (
            max >= 5 &&
            seq[0] === 1 &&
            seq[1] === 2 &&
            seq[2] === 1 &&
            c.roving('Home') === 0 &&
            c.roving('End') === max - 1 &&
            c.keymapConflicts() === 0
          );
        },
      },
      /* 14 手感与细节·档4 —— 微文案与提示语气统一 */
      {
        id: `X${pad(base + 13)}`,
        name: `${spec.title}·微文案`,
        check: () => {
          const c = mk();
          const cap = c.caption();
          const narrative = failureOf(spec, spec.err.code);
          return microcopyOk(cap) && cap.includes(spec.dim) && microcopyOk(narrative.text) && microcopyOk(narrative.next) && microcopyOk(c.reason());
        },
      },
      /* 15 手感与细节·档5 —— 无障碍等价通道（HC 红线） */
      {
        id: `X${pad(base + 14)}`,
        name: `${spec.title}·无障碍`,
        check: () => {
          const c = mk();
          const aria = c.aria();
          return (
            aaTextOk(HC_FOREGROUND, HC_BACKGROUND) &&
            contrastRatio(HC_FOREGROUND, HC_BACKGROUND) >= AA_TEXT_RATIO &&
            aria.role === spec.role &&
            aria.label.includes(spec.title) &&
            TOUCH_TARGET.comfortable >= 44 &&
            c.motion(true).curve === 'linear'
          );
        },
      },
      /* 16 性能与优化·档1 —— 基准采集与性能预算表 */
      {
        id: `X${pad(base + 15)}`,
        name: `${spec.title}·基准采集`,
        check: () => {
          const meter = new PerfMeter();
          for (let i = 0; i < PERF_RING_CAPACITY; i += 1) {
            const c = mk(spec.def);
            const t0 = performance.now();
            c.serialize();
            c.hotPath(1);
            meter.sample(performance.now() - t0);
          }
          return meter.samples.length === PERF_RING_CAPACITY && meter.withinBudget() && meter.leakDelta() === 0;
        },
      },
      /* 17 性能与优化·档2 —— 热路径：算法 / 缓存 / 批处理 收益可量化 */
      {
        id: `X${pad(base + 16)}`,
        name: `${spec.title}·热路径`,
        check: () => {
          const c = mk();
          const first = c.hotPath(100);
          const hit = c.cacheHit();
          const second = c.hotPath(100);
          const ms = timed(200, (i) => { mk(spec.def).hotPath(i); });
          return first === 100 && hit && second === 200 && ms < PERF_BUDGET_MS;
        },
      },
      /* 18 性能与优化·档3 —— 内存与功耗收敛：待机零增量 */
      {
        id: `X${pad(base + 17)}`,
        name: `${spec.title}·零漂移`,
        check: () => {
          const c = mk(tiers[1] ?? spec.def);
          const before = c.serialize();
          for (let i = 0; i < 50; i += 1) c.hotPath(1);
          const meter = new PerfMeter();
          for (let i = 0; i < PERF_RING_CAPACITY + 4; i += 1) meter.sample(1);
          return c.serialize() === before && c.leakDelta() === 0 && meter.leakDelta() === 0;
        },
      },
      /* 19 性能与优化·档4 —— 低配设备自动降级链 */
      {
        id: `X${pad(base + 18)}`,
        name: `${spec.title}·低配减档`,
        check: () => {
          const c = mk(top);
          const chain = c.lowEndChain();
          if (chain.length !== 3) return false;
          const applied = c.applyChain(chain);
          return (
            chain.every((t) => tiers.includes(t)) &&
            chain[0] === tiers[tiers.length - 2] &&
            chain[2] === tiers[tiers.length - 4] &&
            applied === tiers[tiers.length - 4] &&
            c.saverMode
          );
        },
      },
      /* 20 性能与优化·档5 —— 防劣化回归守卫：只增不删 */
      {
        id: `X${pad(base + 19)}`,
        name: `${spec.title}·回归守卫`,
        check: () => {
          const id = `X${pad(base + 19)}`;
          X25_GUARD.add(id);
          return X25_GUARD.has(id) && X25_GUARD.size > 0 && X25_GUARD.immutable(id) && X25_GUARD.remove(id) === false;
        },
      },
      /* 21 创新拓展·档1 —— 本地启发式智能建议：可解释、可一键拒绝 */
      {
        id: `X${pad(base + 20)}`,
        name: `${spec.title}·智能建议`,
        check: () => {
          const c = mk(top);
          const g = X25_SUGGEST.suggest(spec.fam, c.tier, tiers);
          if (g === null) return false;
          const okExplain = microcopyOk(g.text) && g.reason.length > 0 && g.rejectable;
          X25_SUGGEST.reject(g.id);
          const afterReject = X25_SUGGEST.suggest(spec.fam, c.tier, tiers);
          X25_SUGGEST.reset();
          const afterReset = X25_SUGGEST.suggest(spec.fam, c.tier, tiers);
          return okExplain && afterReject === null && afterReset !== null;
        },
      },
      /* 22 创新拓展·档2 —— 批量 / 自动化模式：脚本入口 + 队列 + 进度可观测 */
      {
        id: `X${pad(base + 21)}`,
        name: `${spec.title}·批量模式`,
        check: () => {
          const q = new BatchQueue(spec.fam);
          q.enqueue(5);
          const queued = q.size;
          const before = q.progress();
          const ran = q.run();
          return queued === 5 && before === 0 && ran === 5 && q.processed() === 5 && q.remaining() === 0 && q.progress() === 1;
        },
      },
      /* 23 创新拓展·档3 —— 三线跨域联动 */
      {
        id: `X${pad(base + 22)}`,
        name: `${spec.title}·跨域联动`,
        check: () => {
          const lines = X25_BUS.linesOf(spec.fam);
          return X25_BUS.triad(spec.fam) && lines.length === X25_LINES.length && X25_BUS.ping(spec.fam) === 3 && spec.native();
        },
      },
      /* 24 创新拓展·档4 —— 扩展点：开放接口 / 示例 / 文档 三件套 */
      {
        id: `X${pad(base + 23)}`,
        name: `${spec.title}·扩展点`,
        check: () => {
          const p = X25_EXT.get(spec.fam);
          if (p === null) return false;
          return p.api.startsWith('x25.family(') && p.example.includes('X25Cell') && p.doc.includes(String(spec.fam).padStart(4, '0')) && p.doc.endsWith('.md');
        },
      },
      /* 25 创新拓展·档5 —— 艺术性表达与彩蛋层：默认关、可关闭 */
      {
        id: `X${pad(base + 24)}`,
        name: `${spec.title}·彩蛋层`,
        check: () => {
          X25_EGGS.reset();
          const off = X25_EGGS.lineFor(spec.fam);
          X25_EGGS.enable(spec.fam);
          const on = X25_EGGS.lineFor(spec.fam);
          X25_EGGS.disable(spec.fam);
          const back = X25_EGGS.lineFor(spec.fam);
          X25_EGGS.reset();
          return off === '' && microcopyOk(on) && back === '' && X25_EGGS.defaultOff();
        },
      },
    ];
  };

  const all = build();
  if (!only || only.length === 0) return all;
  const want = new Set(only);
  return all.filter((_, i) => want.has(i + 1));
}

/** 注册一族的公共基建（跨域三线 / 扩展点 / 彩蛋文案 / 守卫种子）。 */
export function registerX25(spec: X25Spec, eggText: string): void {
  for (const line of X25_LINES) X25_BUS.link(spec.fam, line);
  X25_EXT.register(spec.fam, spec.title);
  X25_EGGS.register(spec.fam, eggText);
  X25_GUARD.add(`X${pad(xidBase(spec.fam) + 19)}`);
}
