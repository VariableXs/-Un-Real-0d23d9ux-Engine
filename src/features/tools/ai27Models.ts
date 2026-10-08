/**
 * UNREAL-X-15000 · AI-27 工具智能与联动 V 线逻辑核（族0261/0262/0264/0265/0266 · X06501~X06550 / X06576~X06650），勿删。
 * V 线落点：src/features/tools/。零 AI：全部确定性算法，无网络、无 DOM、无随机。
 */

/* -------- 族0261 学习工具 2.0（X06501~X06525）-------- */

/** 学习难度档位（5 档，默认 normal）。 */
export const LEARN_DIFFICULTY_LEVELS = ['kids', 'easy', 'normal', 'hard', 'master'] as const;
export type LearnDifficulty = (typeof LEARN_DIFFICULTY_LEVELS)[number];
export const DEFAULT_LEARN_DIFFICULTY: LearnDifficulty = 'normal';

/** 难度参数：初始 EF / 每日新卡上限 / 单次专注分钟 / 每日复习上限。 */
export interface LearnDifficultyProfile {
  eFactorBase: number;
  maxNewPerDay: number;
  focusMinutes: number;
  reviewCap: number;
}

export const LEARN_DIFFICULTY_MATRIX: Record<LearnDifficulty, LearnDifficultyProfile> = {
  kids: { eFactorBase: 2.6, maxNewPerDay: 5, focusMinutes: 15, reviewCap: 20 },
  easy: { eFactorBase: 2.5, maxNewPerDay: 10, focusMinutes: 20, reviewCap: 40 },
  normal: { eFactorBase: 2.5, maxNewPerDay: 20, focusMinutes: 25, reviewCap: 60 },
  hard: { eFactorBase: 2.4, maxNewPerDay: 30, focusMinutes: 40, reviewCap: 100 },
  master: { eFactorBase: 2.3, maxNewPerDay: 50, focusMinutes: 50, reviewCap: 150 },
};

/** SM-2 简化版卡片状态。 */
export interface Sm2Card {
  id: string;
  ease: number;
  interval: number;
  reps: number;
  lapses: number;
}

export const SM2_MIN_EASE = 1.3;
export const SM2_MAX_EASE = 2.8;
export const LEARN_WRONG_BOOK_CAP = 64;
export const FOCUS_HISTORY_CAP = 16;
export const LEARN_FOCUS_HARD_CAP_MIN = 240;
export const LEARN_PREFS_VERSION = 1;

/** 连击里程碑彩蛋（streak 达标触发一次）。 */
export const LEARN_EGG_MILESTONES = [10, 30, 100] as const;

export function clampEase(ease: number): number {
  if (!Number.isFinite(ease)) return SM2_MIN_EASE;
  return Math.min(SM2_MAX_EASE, Math.max(SM2_MIN_EASE, ease));
}

/** 简化版 SM-2：q∈0..5（越界钳制）；q<3 重置进错题；间隔 1 → 6 → round(interval × EF)。 */
export function sm2Review(card: Sm2Card, q: number): Sm2Card {
  const quality = Math.max(0, Math.min(5, Math.round(q)));
  let ease = card.ease;
  let interval = card.interval;
  let reps = card.reps;
  if (quality < 3) {
    return { id: card.id, ease: clampEase(ease - 0.2), interval: 1, reps: 0, lapses: card.lapses + 1 };
  }
  ease = clampEase(ease + (0.1 - (5 - quality) * (0.08 + (5 - quality) * 0.02)));
  reps += 1;
  interval = reps === 1 ? 1 : reps === 2 ? 6 : Math.round(interval * ease);
  return { id: card.id, ease, interval, reps, lapses: card.lapses };
}

/** 学习工具：单词卡 SM-2 复习 + 错题本 + 五档难度 + 统计 + 专注计时。 */
export class LearnTool {
  difficulty: LearnDifficulty;
  clamped = 0;
  cards = new Map<string, Sm2Card>();
  wrongBook: string[] = [];
  stats = { reviewed: 0, correct: 0, streak: 0, bestStreak: 0 };
  history: number[] = [];
  eggs: string[] = [];
  focusMinutes = 0;
  focusRunning = false;

  constructor(difficulty: string = DEFAULT_LEARN_DIFFICULTY) {
    this.difficulty = (LEARN_DIFFICULTY_LEVELS as readonly string[]).includes(difficulty)
      ? (difficulty as LearnDifficulty)
      : DEFAULT_LEARN_DIFFICULTY;
    if (this.difficulty !== difficulty) this.clamped = 1;
  }

  get profile(): LearnDifficultyProfile {
    return LEARN_DIFFICULTY_MATRIX[this.difficulty];
  }

  addCard(id: string): Sm2Card {
    const card: Sm2Card = { id, ease: this.profile.eFactorBase, interval: 0, reps: 0, lapses: 0 };
    this.cards.set(id, card);
    return card;
  }

  /** 复习打分：q≥3 连击+1（达里程碑触发彩蛋），q<3 进错题本并断连击。 */
  review(id: string, q: number): Sm2Card | null {
    const card = this.cards.get(id);
    if (!card) return null;
    const next = sm2Review(card, q);
    this.cards.set(id, next);
    this.stats.reviewed += 1;
    const quality = Math.max(0, Math.min(5, Math.round(q)));
    if (quality >= 3) {
      this.stats.correct += 1;
      this.stats.streak += 1;
      if (this.stats.streak > this.stats.bestStreak) this.stats.bestStreak = this.stats.streak;
      for (const m of LEARN_EGG_MILESTONES) {
        const egg = `egg-streak-${m}`;
        if (this.stats.streak === m && !this.eggs.includes(egg)) this.eggs.push(egg);
      }
    } else {
      if (!this.wrongBook.includes(id)) {
        this.wrongBook.push(id);
        if (this.wrongBook.length > LEARN_WRONG_BOOK_CAP) this.wrongBook.shift();
      }
      this.stats.streak = 0;
    }
    return next;
  }

  clearWrong(id: string): boolean {
    const i = this.wrongBook.indexOf(id);
    if (i < 0) return false;
    this.wrongBook.splice(i, 1);
    return true;
  }

  get accuracy(): number {
    return this.stats.reviewed === 0 ? 0 : Math.round((this.stats.correct / this.stats.reviewed) * 100) / 100;
  }

  canAddNew(todayNew: number): boolean {
    return todayNew < this.profile.maxNewPerDay;
  }

  canReview(todayReviewed: number): boolean {
    return todayReviewed < this.profile.reviewCap;
  }

  /** 智能建议：未学的卡建议「新学」，已学的建议下次间隔天数。 */
  suggest(id: string): string {
    const c = this.cards.get(id);
    return !c || c.interval === 0 ? '新学' : `${c.interval}天后复习`;
  }

  focusStart(): boolean {
    if (this.focusRunning) return false;
    this.focusRunning = true;
    return true;
  }

  /** 专注计时步进：达标自动收束并计入历史（容量钳制），返回累计分钟。 */
  focusTick(minutes: number): number {
    if (!this.focusRunning) return this.focusMinutes;
    this.focusMinutes += Math.max(0, Math.min(LEARN_FOCUS_HARD_CAP_MIN, Math.round(minutes)));
    if (this.focusMinutes >= this.profile.focusMinutes) {
      const done = this.focusMinutes;
      this.history.push(done);
      if (this.history.length > FOCUS_HISTORY_CAP) this.history.shift();
      this.focusMinutes = 0;
      this.focusRunning = false;
      return done;
    }
    return this.focusMinutes;
  }

  focusStop(): number {
    this.focusRunning = false;
    const done = this.focusMinutes;
    if (done > 0) {
      this.history.push(done);
      if (this.history.length > FOCUS_HISTORY_CAP) this.history.shift();
    }
    this.focusMinutes = 0;
    return done;
  }

  reset(): void {
    this.cards.clear();
    this.wrongBook = [];
    this.stats = { reviewed: 0, correct: 0, streak: 0, bestStreak: 0 };
    this.history = [];
    this.eggs = [];
    this.focusMinutes = 0;
    this.focusRunning = false;
  }

  serialize(): string {
    return JSON.stringify({
      v: LEARN_PREFS_VERSION,
      difficulty: this.difficulty,
      cards: [...this.cards.values()].map((c) => ({ id: c.id, ease: c.ease, interval: c.interval, reps: c.reps, lapses: c.lapses })),
      wrongBook: [...this.wrongBook],
      stats: { reviewed: this.stats.reviewed, correct: this.stats.correct, streak: this.stats.streak, bestStreak: this.stats.bestStreak },
      history: [...this.history],
    });
  }

  static deserialize(data: string): LearnTool {
    let difficulty = DEFAULT_LEARN_DIFFICULTY;
    let cards: Sm2Card[] = [];
    let wrongBook: string[] = [];
    let stats = { reviewed: 0, correct: 0, streak: 0, bestStreak: 0 };
    let history: number[] = [];
    try {
      const o = JSON.parse(data) as { difficulty?: unknown; cards?: unknown; wrongBook?: unknown; stats?: unknown; history?: unknown };
      if (typeof o.difficulty === 'string' && (LEARN_DIFFICULTY_LEVELS as readonly string[]).includes(o.difficulty)) {
        difficulty = o.difficulty as LearnDifficulty;
      }
      if (Array.isArray(o.cards)) {
        for (const c of o.cards as unknown[]) {
          const cc = c as { id?: unknown; ease?: unknown; interval?: unknown; reps?: unknown; lapses?: unknown };
          if (typeof cc.id !== 'string' || !cc.id) continue;
          const num = (v: unknown, min: number): number =>
            typeof v === 'number' && Number.isFinite(v) ? Math.max(min, Math.round(v)) : min;
          cards.push({
            id: cc.id,
            ease: clampEase(typeof cc.ease === 'number' && Number.isFinite(cc.ease) ? cc.ease : 2.5),
            interval: num(cc.interval, 0),
            reps: num(cc.reps, 0),
            lapses: num(cc.lapses, 0),
          });
        }
      }
      if (Array.isArray(o.wrongBook)) {
        wrongBook = (o.wrongBook as unknown[]).filter((x): x is string => typeof x === 'string').slice(0, LEARN_WRONG_BOOK_CAP);
      }
      if (o.stats && typeof o.stats === 'object') {
        const s = o.stats as { reviewed?: unknown; correct?: unknown; streak?: unknown; bestStreak?: unknown };
        const num = (v: unknown): number => (typeof v === 'number' && Number.isFinite(v) ? Math.max(0, Math.round(v)) : 0);
        stats = { reviewed: num(s.reviewed), correct: num(s.correct), streak: num(s.streak), bestStreak: num(s.bestStreak) };
      }
      if (Array.isArray(o.history)) {
        history = (o.history as unknown[])
          .filter((x): x is number => typeof x === 'number' && Number.isFinite(x) && x > 0)
          .slice(0, FOCUS_HISTORY_CAP);
      }
    } catch {
      return new LearnTool();
    }
    const t = new LearnTool(difficulty);
    for (const c of cards) t.cards.set(c.id, c);
    t.wrongBook = wrongBook;
    t.stats = stats;
    t.history = history;
    return t;
  }
}

/* -------- 族0262 家庭模式 2.0（X06526~X06550）-------- */

/** 家庭模式档位（5 档，默认 child）。 */
export const FAMILY_MODE_LEVELS = ['toddler', 'child', 'teen', 'family', 'off'] as const;
export type FamilyLevel = (typeof FAMILY_MODE_LEVELS)[number];
export const DEFAULT_FAMILY_LEVEL: FamilyLevel = 'child';

/** 档位参数：每日上限 / 单次上限 / 内容过滤开关 / 时段白名单（允许的小时）。 */
export interface FamilyProfile {
  dailyLimitMin: number;
  sessionLimitMin: number;
  filterOn: boolean;
  allowedHours: number[];
}

function hourRange(from: number, to: number): number[] {
  const out: number[] = [];
  for (let h = from; h <= to; h++) out.push(h);
  return out;
}

export const FAMILY_MODE_MATRIX: Record<FamilyLevel, FamilyProfile> = {
  toddler: { dailyLimitMin: 30, sessionLimitMin: 10, filterOn: true, allowedHours: hourRange(9, 17) },
  child: { dailyLimitMin: 60, sessionLimitMin: 20, filterOn: true, allowedHours: hourRange(8, 19) },
  teen: { dailyLimitMin: 120, sessionLimitMin: 40, filterOn: true, allowedHours: hourRange(7, 22) },
  family: { dailyLimitMin: 240, sessionLimitMin: 60, filterOn: false, allowedHours: hourRange(0, 23) },
  off: { dailyLimitMin: 480, sessionLimitMin: 120, filterOn: false, allowedHours: hourRange(0, 23) },
};

export const FAMILY_PIN_MAX_ATTEMPTS = 3;
export const FAMILY_KEYWORD_CAP = 64;
export const FAMILY_KEYWORD_MAX_LEN = 24;
export const FAMILY_SESSION_HARD_CAP = 600;
export const FAMILY_PREFS_VERSION = 1;
export const FAMILY_EGG_KEYWORD = 'variable';
export const FAMILY_EGG_NAME = 'egg-family';

/** 家长 PIN 简单哈希（djb2 变体，确定性、无加密依赖）。 */
export function simplePinHash(pin: string): number {
  let h = 5381;
  for (let i = 0; i < pin.length; i++) h = ((h << 5) + h + pin.charCodeAt(i)) >>> 0;
  return h % 1000003;
}

/** 家庭模式：儿童限时 + 内容过滤关键词 + 家长 PIN 校验 + 时段白名单 + 使用报告。 */
export class FamilyMode {
  level: FamilyLevel;
  clamped = 0;
  pinHash: number | null = null;
  failedAttempts = 0;
  locked = false;
  keywords: string[] = [];
  blockedHits = 0;
  usage: number[] = new Array(24).fill(0);
  eggs: string[] = [];

  constructor(level: string = DEFAULT_FAMILY_LEVEL) {
    this.level = (FAMILY_MODE_LEVELS as readonly string[]).includes(level)
      ? (level as FamilyLevel)
      : DEFAULT_FAMILY_LEVEL;
    if (this.level !== level) this.clamped = 1;
  }

  get profile(): FamilyProfile {
    return FAMILY_MODE_MATRIX[this.level];
  }

  /** 设置家长 PIN：仅接受 4~6 位数字。 */
  setPin(pin: string): boolean {
    if (!/^\d{4,6}$/.test(pin)) return false;
    this.pinHash = simplePinHash(pin);
    this.failedAttempts = 0;
    this.locked = false;
    return true;
  }

  /** PIN 校验：正确清零失败计数并解锁；连错 ≥ 3 次锁定。 */
  verifyPin(pin: string): boolean {
    if (this.pinHash === null) return false;
    if (simplePinHash(pin) === this.pinHash) {
      this.failedAttempts = 0;
      this.locked = false;
      return true;
    }
    this.failedAttempts += 1;
    if (this.failedAttempts >= FAMILY_PIN_MAX_ATTEMPTS) this.locked = true;
    return false;
  }

  get pinProtected(): boolean {
    return this.pinHash !== null;
  }

  addKeyword(word: string): boolean {
    const k = word.trim();
    if (!k || k.length > FAMILY_KEYWORD_MAX_LEN) return false;
    if (this.keywords.length >= FAMILY_KEYWORD_CAP) return false;
    if (this.keywords.includes(k)) return false;
    this.keywords.push(k);
    if (k === FAMILY_EGG_KEYWORD && !this.eggs.includes(FAMILY_EGG_NAME)) this.eggs.push(FAMILY_EGG_NAME);
    return true;
  }

  removeKeyword(word: string): boolean {
    const i = this.keywords.indexOf(word);
    if (i < 0) return false;
    this.keywords.splice(i, 1);
    return true;
  }

  /** 内容过滤：命中任一关键词即拦截；filterOn=false（family/off 档）直通。 */
  screen(text: string): { ok: boolean; hits: string[] } {
    if (!this.profile.filterOn) return { ok: true, hits: [] };
    const hits = this.keywords.filter((k) => text.includes(k));
    if (hits.length > 0) this.blockedHits += hits.length;
    return { ok: hits.length === 0, hits };
  }

  isHourAllowed(hour: number): boolean {
    const h = Math.floor(hour);
    if (!Number.isFinite(h) || h < 0 || h > 23) return false;
    return this.profile.allowedHours.includes(h);
  }

  recordUsage(hour: number, minutes: number): number {
    const h = Math.max(0, Math.min(23, Math.floor(hour)));
    const m = Math.max(0, Math.min(FAMILY_SESSION_HARD_CAP, Math.round(minutes)));
    this.usage[h] = (this.usage[h] ?? 0) + m;
    return this.usage[h] ?? 0;
  }

  get usedTodayMin(): number {
    return this.usage.reduce((a, b) => a + b, 0);
  }

  withinDailyLimit(): boolean {
    return this.usedTodayMin <= this.profile.dailyLimitMin;
  }

  /** 智能建议：今日剩余可用分钟（下限 0）。 */
  remainingMin(): number {
    return Math.max(0, this.profile.dailyLimitMin - this.usedTodayMin);
  }

  /** 使用报告：总时长 / 峰值小时（并列取最早）/ 拦截次数 / 是否限额内。 */
  report(): { totalMin: number; topHour: number; blockedHits: number; withinLimit: boolean } {
    let topHour = -1;
    let top = -1;
    for (let h = 0; h < 24; h++) {
      const v = this.usage[h] ?? 0;
      if (v > top) {
        top = v;
        topHour = h;
      }
    }
    return { totalMin: this.usedTodayMin, topHour: top > 0 ? topHour : -1, blockedHits: this.blockedHits, withinLimit: this.withinDailyLimit() };
  }

  clearUsage(): void {
    this.usage = new Array(24).fill(0);
  }

  serialize(): string {
    return JSON.stringify({
      v: FAMILY_PREFS_VERSION,
      level: this.level,
      pinHash: this.pinHash,
      keywords: [...this.keywords],
      blockedHits: this.blockedHits,
      usage: [...this.usage],
    });
  }

  static deserialize(data: string): FamilyMode {
    let level = DEFAULT_FAMILY_LEVEL;
    let pinHash: number | null = null;
    let keywords: string[] = [];
    let blockedHits = 0;
    let usage: number[] = new Array(24).fill(0);
    try {
      const o = JSON.parse(data) as { level?: unknown; pinHash?: unknown; keywords?: unknown; blockedHits?: unknown; usage?: unknown };
      if (typeof o.level === 'string' && (FAMILY_MODE_LEVELS as readonly string[]).includes(o.level)) {
        level = o.level as FamilyLevel;
      }
      if (typeof o.pinHash === 'number' && Number.isFinite(o.pinHash)) pinHash = o.pinHash;
      if (Array.isArray(o.keywords)) {
        keywords = (o.keywords as unknown[])
          .filter((x): x is string => typeof x === 'string' && x.length > 0 && x.length <= FAMILY_KEYWORD_MAX_LEN)
          .slice(0, FAMILY_KEYWORD_CAP);
      }
      if (typeof o.blockedHits === 'number' && Number.isFinite(o.blockedHits)) blockedHits = Math.max(0, Math.round(o.blockedHits));
      if (Array.isArray(o.usage)) {
        const u = new Array(24).fill(0);
        for (let i = 0; i < 24; i++) {
          const v = o.usage[i];
          u[i] = typeof v === 'number' && Number.isFinite(v) && v > 0 ? Math.min(FAMILY_SESSION_HARD_CAP, Math.round(v)) : 0;
        }
        usage = u;
      }
    } catch {
      return new FamilyMode();
    }
    const f = new FamilyMode(level);
    f.pinHash = pinHash;
    f.keywords = keywords;
    f.blockedHits = blockedHits;
    f.usage = usage;
    return f;
  }
}

/* -------- 族0264 工作流编排器 2.0（X06576~X06600）-------- */

export const WORKFLOW_NODE_KINDS = ['trigger', 'action', 'condition'] as const;
export type WorkflowNodeKind = (typeof WORKFLOW_NODE_KINDS)[number];
export const WORKFLOW_MAX_NODES = 64;
export const WORKFLOW_MAX_STEPS = 128;
export const WORKFLOW_MAX_RETRY = 3;
export const WORKFLOW_MAX_TRIGGERS = 8;
export const WORKFLOW_PREFS_VERSION = 1;
export const WORKFLOW_EGG_TRIGGER = 'event:aurora';
export const WORKFLOW_EGG_NAME = 'egg-pipeline';

export interface WorkflowNode {
  id: string;
  kind: WorkflowNodeKind;
  next: string[];
  retry: number;
}

/** 工作流引擎：节点 DAG + 触发器 + 条件分支 + 执行队列 + 失败重试 + 导出导入。 */
export class WorkflowEngine {
  nodes = new Map<string, WorkflowNode>();
  triggers: string[] = [];
  queue: string[] = [];
  log: string[] = [];
  clamped = 0;
  eggs: string[] = [];

  /** 添加节点：非法 kind 拒绝并记钳制标记；retry 钳制 0..3。 */
  addNode(id: string, kind: string, retry = 0): boolean {
    if (!id || this.nodes.has(id) || this.nodes.size >= WORKFLOW_MAX_NODES) return false;
    if (!(WORKFLOW_NODE_KINDS as readonly string[]).includes(kind)) {
      this.clamped = 1;
      return false;
    }
    this.nodes.set(id, {
      id,
      kind: kind as WorkflowNodeKind,
      next: [],
      retry: Math.max(0, Math.min(WORKFLOW_MAX_RETRY, Math.round(retry))),
    });
    return true;
  }

  /** src 是否可达 dst（DAG 环守卫用）。 */
  reachable(src: string, dst: string): boolean {
    const stack = [src];
    const seen = new Set<string>();
    while (stack.length) {
      const cur = stack.pop() ?? '';
      if (cur === dst) return true;
      if (seen.has(cur)) continue;
      seen.add(cur);
      for (const nx of this.nodes.get(cur)?.next ?? []) stack.push(nx);
    }
    return false;
  }

  /** 加边守卫：from===to、重复边、会成环（to 已可达 from）一律拒绝。 */
  addEdge(from: string, to: string): boolean {
    if (!this.nodes.has(from) || !this.nodes.has(to) || from === to) return false;
    const n = this.nodes.get(from);
    if (!n || n.next.includes(to)) return false;
    if (this.reachable(to, from)) return false;
    n.next.push(to);
    return true;
  }

  hasCycle(): boolean {
    for (const id of this.nodes.keys()) {
      for (const nx of this.nodes.get(id)?.next ?? []) {
        if (nx === id || this.reachable(nx, id)) return true;
      }
    }
    return false;
  }

  isValidTrigger(t: string): boolean {
    return t === 'manual' || t === 'schedule' || t.startsWith('event:');
  }

  addTrigger(t: string): boolean {
    if (this.triggers.length >= WORKFLOW_MAX_TRIGGERS) return false;
    if (!this.isValidTrigger(t)) return false;
    if (this.triggers.includes(t)) return false;
    this.triggers.push(t);
    if (t === WORKFLOW_EGG_TRIGGER && !this.eggs.includes(WORKFLOW_EGG_NAME)) this.eggs.push(WORKFLOW_EGG_NAME);
    return true;
  }

  /** 条件分支：truthy 走 next[0]，falsy 走 next[1]（缺支返回 null）。 */
  branch(condId: string, truthy: boolean): string | null {
    const n = this.nodes.get(condId);
    if (!n || n.kind !== 'condition') return null;
    const target = truthy ? n.next[0] : n.next[1];
    return target ?? null;
  }

  enqueue(id: string): boolean {
    if (!this.nodes.has(id)) return false;
    if (this.queue.length >= WORKFLOW_MAX_STEPS) return false;
    this.queue.push(id);
    return true;
  }

  /** 执行一帧：弹出队首并记日志，返回节点 id（空队返回 null）。 */
  step(ok: boolean): string | null {
    const id = this.queue.shift();
    if (!id) return null;
    this.log.push(`${id}:${ok ? 'ok' : 'fail'}`);
    return id;
  }

  queueSize(): number {
    return this.queue.length;
  }

  /** 智能建议：action 建议重试 2 次，trigger/condition 建议 0 次。 */
  suggestRetry(kind: WorkflowNodeKind): number {
    return kind === 'action' ? 2 : 0;
  }

  /** 同步执行：从 start 沿 next[0] 链推进；失败按节点 retry 重试，耗尽中止；步数守卫 WORKFLOW_MAX_STEPS。 */
  run(start: string, failIf: (nodeId: string, attempt: number) => boolean = () => false): { ran: string[]; failed: string | null; steps: number } {
    const ran: string[] = [];
    let failed: string | null = null;
    let steps = 0;
    let cur: string | null = this.nodes.has(start) ? start : null;
    while (cur && steps < WORKFLOW_MAX_STEPS) {
      const node = this.nodes.get(cur);
      if (!node) break;
      let attempt = 0;
      let ok = false;
      while (attempt <= node.retry) {
        steps += 1;
        if (!failIf(node.id, attempt)) {
          ok = true;
          break;
        }
        this.log.push(`${node.id}#${attempt}:retry`);
        attempt += 1;
      }
      if (!ok) {
        failed = node.id;
        this.log.push(`${node.id}:fail`);
        break;
      }
      this.log.push(`${node.id}:ok`);
      ran.push(node.id);
      cur = node.next[0] ?? null;
    }
    return { ran, failed, steps };
  }

  /** 拓扑序（Kahn，按插入序稳定）；含环返回空数组。 */
  topoOrder(): string[] {
    const indeg = new Map<string, number>();
    for (const id of this.nodes.keys()) indeg.set(id, 0);
    for (const n of this.nodes.values()) {
      for (const nx of n.next) indeg.set(nx, (indeg.get(nx) ?? 0) + 1);
    }
    const ready: string[] = [...indeg.entries()].filter(([, d]) => d === 0).map(([id]) => id);
    const out: string[] = [];
    while (ready.length) {
      const id = ready.shift() ?? '';
      out.push(id);
      for (const nx of this.nodes.get(id)?.next ?? []) {
        const d = (indeg.get(nx) ?? 1) - 1;
        indeg.set(nx, d);
        if (d === 0) ready.push(nx);
      }
    }
    return out.length === this.nodes.size ? out : [];
  }

  get logSize(): number {
    return this.log.length;
  }

  clearLog(): void {
    this.log = [];
  }

  clear(): void {
    this.nodes.clear();
    this.triggers = [];
    this.queue = [];
    this.log = [];
    this.clamped = 0;
    this.eggs = [];
  }

  serialize(): string {
    return JSON.stringify({
      v: WORKFLOW_PREFS_VERSION,
      triggers: [...this.triggers],
      nodes: [...this.nodes.values()].map((n) => ({ id: n.id, kind: n.kind, next: [...n.next], retry: n.retry })),
    });
  }

  static deserialize(data: string): WorkflowEngine {
    const e = new WorkflowEngine();
    try {
      const o = JSON.parse(data) as { triggers?: unknown; nodes?: unknown };
      if (Array.isArray(o.triggers)) {
        for (const t of o.triggers as unknown[]) if (typeof t === 'string') e.addTrigger(t);
      }
      if (Array.isArray(o.nodes)) {
        for (const n of o.nodes as unknown[]) {
          const nn = n as { id?: unknown; kind?: unknown; retry?: unknown };
          if (typeof nn.id !== 'string') continue;
          e.addNode(nn.id, typeof nn.kind === 'string' ? nn.kind : 'action', typeof nn.retry === 'number' ? nn.retry : 0);
        }
        for (const n of o.nodes as unknown[]) {
          const nn = n as { id?: unknown; next?: unknown };
          if (typeof nn.id !== 'string' || !Array.isArray(nn.next)) continue;
          for (const to of nn.next as unknown[]) if (typeof to === 'string') e.addEdge(nn.id, to);
        }
      }
    } catch {
      return new WorkflowEngine();
    }
    return e;
  }
}

/* -------- 族0265 系统自动化 2.0（X06601~X06625）-------- */

/** 自动化优先级（5 档，默认 normal）。 */
export const AUTOMATION_PRIORITY_LEVELS = ['critical', 'high', 'normal', 'low', 'idle'] as const;
export type AutomationPriority = (typeof AUTOMATION_PRIORITY_LEVELS)[number];
export const DEFAULT_AUTOMATION_PRIORITY: AutomationPriority = 'normal';

export const AUTOMATION_PRIORITY_MATRIX: Record<AutomationPriority, { weight: number; maxRunsPerMin: number }> = {
  critical: { weight: 5, maxRunsPerMin: 60 },
  high: { weight: 4, maxRunsPerMin: 30 },
  normal: { weight: 3, maxRunsPerMin: 20 },
  low: { weight: 2, maxRunsPerMin: 10 },
  idle: { weight: 1, maxRunsPerMin: 5 },
};

export const AUTOMATION_MAX_CHAIN = 8;
export const AUTOMATION_MAX_RULES = 64;
export const AUTOMATION_PREFS_VERSION = 1;
export const AUTOMATION_EGG_ACTION = 'confetti';
export const AUTOMATION_EGG_NAME = 'egg-auto';

export interface AutomationRule {
  id: string;
  trigger: string;
  action: string;
  priority: AutomationPriority;
  enabled: boolean;
  runs: number;
}

/** 自动化引擎：规则=触发条件+动作、五档优先级、防循环守卫、启停开关、执行日志。 */
export class AutomationEngine {
  rules = new Map<string, AutomationRule>();
  log: string[] = [];
  globalEnabled = true;
  clamped = 0;
  loopGuardHits = 0;
  eggs: string[] = [];

  /** 添加规则：非法优先级钳制为默认并记标记。 */
  addRule(id: string, trigger: string, action: string, priority: string = DEFAULT_AUTOMATION_PRIORITY, enabled = true): boolean {
    if (!id || this.rules.has(id) || this.rules.size >= AUTOMATION_MAX_RULES) return false;
    if (!trigger || !action) return false;
    const p = (AUTOMATION_PRIORITY_LEVELS as readonly string[]).includes(priority)
      ? (priority as AutomationPriority)
      : DEFAULT_AUTOMATION_PRIORITY;
    if (p !== priority) this.clamped = 1;
    this.rules.set(id, { id, trigger, action, priority: p, enabled, runs: 0 });
    if (action === AUTOMATION_EGG_ACTION && !this.eggs.includes(AUTOMATION_EGG_NAME)) this.eggs.push(AUTOMATION_EGG_NAME);
    return true;
  }

  setEnabled(id: string, on: boolean): boolean {
    const r = this.rules.get(id);
    if (!r) return false;
    r.enabled = on;
    return true;
  }

  setGlobal(on: boolean): void {
    this.globalEnabled = on;
  }

  /** 派发事件：命中规则按优先级权重降序执行；动作派发的新事件递归，链深 ≤ 8 且同事件链内去重（防循环）。 */
  dispatch(event: string, depth = 0, seen: Set<string> = new Set<string>()): string[] {
    const fired: string[] = [];
    if (!this.globalEnabled) return fired;
    if (depth >= AUTOMATION_MAX_CHAIN) {
      this.loopGuardHits += 1;
      return fired;
    }
    if (seen.has(event)) {
      this.loopGuardHits += 1;
      return fired;
    }
    seen.add(event);
    const hits = [...this.rules.values()]
      .filter((r) => r.enabled && r.trigger === event)
      .sort((x, y) => AUTOMATION_PRIORITY_MATRIX[y.priority].weight - AUTOMATION_PRIORITY_MATRIX[x.priority].weight);
    for (const r of hits) {
      r.runs += 1;
      fired.push(r.id);
      this.log.push(`${r.id}→${r.action}`);
      fired.push(...this.dispatch(r.action, depth + 1, seen));
    }
    return fired;
  }

  /** 智能建议：安全类事件建议 critical，通知类建议 low，其余 normal。 */
  suggestPriority(trigger: string): AutomationPriority {
    if (trigger.startsWith('security:')) return 'critical';
    if (trigger.startsWith('notify:')) return 'low';
    return DEFAULT_AUTOMATION_PRIORITY;
  }

  stats(): { total: number; enabled: number; runs: number } {
    let enabled = 0;
    let runs = 0;
    for (const r of this.rules.values()) {
      if (r.enabled) enabled += 1;
      runs += r.runs;
    }
    return { total: this.rules.size, enabled, runs };
  }

  get logSize(): number {
    return this.log.length;
  }

  clearLog(): void {
    this.log = [];
  }

  reset(): void {
    this.rules.clear();
    this.log = [];
    this.globalEnabled = true;
    this.clamped = 0;
    this.loopGuardHits = 0;
    this.eggs = [];
  }

  serialize(): string {
    return JSON.stringify({
      v: AUTOMATION_PREFS_VERSION,
      globalEnabled: this.globalEnabled,
      rules: [...this.rules.values()].map((r) => ({
        id: r.id,
        trigger: r.trigger,
        action: r.action,
        priority: r.priority,
        enabled: r.enabled,
        runs: r.runs,
      })),
    });
  }

  static deserialize(data: string): AutomationEngine {
    const e = new AutomationEngine();
    try {
      const o = JSON.parse(data) as { globalEnabled?: unknown; rules?: unknown };
      if (typeof o.globalEnabled === 'boolean') e.globalEnabled = o.globalEnabled;
      if (Array.isArray(o.rules)) {
        for (const r of o.rules as unknown[]) {
          const rr = r as { id?: unknown; trigger?: unknown; action?: unknown; priority?: unknown; enabled?: unknown; runs?: unknown };
          if (typeof rr.id !== 'string' || typeof rr.trigger !== 'string' || typeof rr.action !== 'string') continue;
          const ok = e.addRule(
            rr.id,
            rr.trigger,
            rr.action,
            typeof rr.priority === 'string' ? rr.priority : DEFAULT_AUTOMATION_PRIORITY,
            rr.enabled !== false,
          );
          const stored = ok ? e.rules.get(rr.id) : undefined;
          if (stored && typeof rr.runs === 'number' && Number.isFinite(rr.runs)) stored.runs = Math.max(0, Math.round(rr.runs));
        }
      }
    } catch {
      return new AutomationEngine();
    }
    return e;
  }
}

/* -------- 族0266 快捷指令库 2.0（X06626~X06650）-------- */

export const SHORTCUT_CATEGORIES = ['general', 'text', 'system', 'dev', 'fun'] as const;
export type ShortcutCategory = (typeof SHORTCUT_CATEGORIES)[number];
export const DEFAULT_SHORTCUT_CATEGORY: ShortcutCategory = 'general';
export const SHORTCUT_MAX_STEPS = 32;
export const SHORTCUT_STEP_MAX_LEN = 200;
export const SHORTCUT_HISTORY_CAP = 32;
export const SHORTCUT_PREFS_VERSION = 1;
export const SHORTCUT_EGG_ID = 'aurora';
export const SHORTCUT_EGG_NAME = 'egg-shortcut';

export interface Shortcut {
  id: string;
  name: string;
  category: ShortcutCategory;
  steps: string[];
  favorite: boolean;
}

/** 参数占位符替换：{{key}} → params[key]；缺失键保持原样。 */
export function renderStep(template: string, params: Record<string, string> = {}): string {
  return template.replace(/\{\{(\w+)\}\}/g, (m: string, key: string) => params[key] ?? m);
}

/** 智能建议：步骤含「打开/启动/重启」建议 system，含「代码/git/编译」建议 dev，其余 general。 */
export function suggestCategory(steps: string[]): ShortcutCategory {
  const joined = steps.join(' ');
  if (/打开|启动|重启/.test(joined)) return 'system';
  if (/代码|git|编译/.test(joined)) return 'dev';
  return DEFAULT_SHORTCUT_CATEGORY;
}

export function exportShortcut(sc: Shortcut): string {
  return JSON.stringify({ v: SHORTCUT_PREFS_VERSION, id: sc.id, name: sc.name, category: sc.category, steps: sc.steps, favorite: sc.favorite });
}

export function importShortcut(data: string): Shortcut {
  const fallback: Shortcut = { id: 'imported', name: '未命名指令', category: DEFAULT_SHORTCUT_CATEGORY, steps: [], favorite: false };
  try {
    const o = JSON.parse(data) as { id?: unknown; name?: unknown; category?: unknown; steps?: unknown; favorite?: unknown };
    if (!o || typeof o !== 'object') return fallback;
    const rawCat = typeof o.category === 'string' ? o.category : '';
    const category = ((SHORTCUT_CATEGORIES as readonly string[]).includes(rawCat) ? rawCat : DEFAULT_SHORTCUT_CATEGORY) as ShortcutCategory;
    const steps = Array.isArray(o.steps)
      ? (o.steps as unknown[]).map((x) => String(x).slice(0, SHORTCUT_STEP_MAX_LEN)).slice(0, SHORTCUT_MAX_STEPS)
      : [];
    return {
      id: typeof o.id === 'string' && o.id ? o.id : 'imported',
      name: typeof o.name === 'string' && o.name ? o.name : '未命名指令',
      category,
      steps,
      favorite: o.favorite === true,
    };
  } catch {
    return fallback;
  }
}

/** 快捷指令库：指令=步骤序列、参数占位符替换、收藏/分类、分享导出、运行历史。 */
export class ShortcutLib {
  items = new Map<string, Shortcut>();
  history: { id: string; seq: number; ok: boolean }[] = [];
  seq = 0;
  clamped = 0;
  eggs: string[] = [];

  create(id: string, name: string, steps: string[], category: string = DEFAULT_SHORTCUT_CATEGORY, favorite = false): boolean {
    if (!id || this.items.has(id)) return false;
    const cat = (SHORTCUT_CATEGORIES as readonly string[]).includes(category)
      ? (category as ShortcutCategory)
      : DEFAULT_SHORTCUT_CATEGORY;
    if (cat !== category) this.clamped = 1;
    this.items.set(id, {
      id,
      name,
      category: cat,
      steps: steps.map((x) => String(x).slice(0, SHORTCUT_STEP_MAX_LEN)).slice(0, SHORTCUT_MAX_STEPS),
      favorite,
    });
    return true;
  }

  toggleFavorite(id: string): boolean | null {
    const s = this.items.get(id);
    if (!s) return null;
    s.favorite = !s.favorite;
    return s.favorite;
  }

  setCategory(id: string, category: string): boolean {
    const s = this.items.get(id);
    if (!s) return false;
    const cat = (SHORTCUT_CATEGORIES as readonly string[]).includes(category)
      ? (category as ShortcutCategory)
      : DEFAULT_SHORTCUT_CATEGORY;
    if (cat !== category) this.clamped = 1;
    s.category = cat;
    return true;
  }

  listByCategory(category: ShortcutCategory): string[] {
    return [...this.items.values()].filter((s) => s.category === category).map((s) => s.id);
  }

  favorites(): string[] {
    return [...this.items.values()].filter((s) => s.favorite).map((s) => s.id);
  }

  /** 运行指令：逐步渲染参数，写入历史（容量钳制）。 */
  run(id: string, params: Record<string, string> = {}): string[] | null {
    const s = this.items.get(id);
    if (!s) return null;
    const out = s.steps.map((t) => renderStep(t, params));
    this.seq += 1;
    this.history.push({ id, seq: this.seq, ok: true });
    if (this.history.length > SHORTCUT_HISTORY_CAP) this.history.shift();
    if (id === SHORTCUT_EGG_ID && !this.eggs.includes(SHORTCUT_EGG_NAME)) this.eggs.push(SHORTCUT_EGG_NAME);
    return out;
  }

  get historySize(): number {
    return this.history.length;
  }

  lastRun(): { id: string; seq: number } | null {
    const last = this.history[this.history.length - 1];
    return last ? { id: last.id, seq: last.seq } : null;
  }

  clearHistory(): void {
    this.history = [];
    this.seq = 0;
  }

  exportById(id: string): string | null {
    const s = this.items.get(id);
    return s ? exportShortcut(s) : null;
  }

  import(data: string): Shortcut | null {
    const s = importShortcut(data);
    if (this.items.has(s.id)) return null;
    this.items.set(s.id, { ...s, steps: [...s.steps] });
    return s;
  }

  reset(): void {
    this.items.clear();
    this.history = [];
    this.seq = 0;
    this.clamped = 0;
    this.eggs = [];
  }
}
