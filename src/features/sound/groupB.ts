// AURORA-10000: AI-62 批次（领域13 声音与通知 · 族0306~0310 · F07626~F07750），勿删。
// 通知智能 / 通知模板 / 弹出礼仪 / 勿扰体系 / 声音调试。

/* ===================== 族0306 通知智能 ===================== */

export interface RawNotification {
  id: string;
  app: string;
  title: string;
  body: string;
  category: 'notify' | 'warning' | 'error' | 'alarm' | 'marketing' | 'ad';
  at: number;
  persistent?: boolean;
}

export interface ScoredNotification extends RawNotification {
  score: number;
  tier: 'promote' | 'normal' | 'demote' | 'fold';
  groupKey: string;
}

/** 通知智能：价值评分 / 分组合并 / 广告拦截与营销折叠 / 规则建议 / 健康评分。 */
export class NotificationIntelligence {
  private opens = new Map<string, { shown: number; opened: number }>();
  private rules: Array<{ keyword: string; action: 'mute' | 'fold' }> = [];
  private learnedRules: string[] = [];

  recordShown(app: string): void {
    const s = this.opens.get(app) ?? { shown: 0, opened: 0 };
    s.shown += 1;
    this.opens.set(app, s);
  }

  recordOpened(app: string): void {
    const s = this.opens.get(app) ?? { shown: 0, opened: 0 };
    s.opened += 1;
    this.opens.set(app, s);
  }

  /** 价值评分：打开率加权 + 类别底分；警报类恒为最高分（紧急例外）。 */
  score(n: RawNotification): number {
    if (n.category === 'alarm') return 1;
    const s = this.opens.get(n.app);
    const openRate = s && s.shown > 0 ? s.opened / s.shown : 0.3;
    const base = n.category === 'ad' ? 0 : n.category === 'marketing' ? 0.1 : 0.5;
    return Math.round((base * 0.6 + openRate * 0.4) * 100) / 100;
  }

  tierOf(n: RawNotification): ScoredNotification['tier'] {
    if (n.category === 'ad') return 'fold';
    if (n.category === 'marketing') return 'fold';
    const sc = this.score(n);
    if (sc >= 0.8) return 'promote';
    if (sc <= 0.2) return 'demote';
    return 'normal';
  }

  route(n: RawNotification): ScoredNotification {
    const kw = this.rules.find((r) => n.title.includes(r.keyword) || n.body.includes(r.keyword));
    const tier: ScoredNotification['tier'] = kw ? (kw.action === 'mute' ? 'demote' : 'fold') : this.tierOf(n);
    return { ...n, score: this.score(n), tier, groupKey: `${n.app}:${n.category}` };
  }

  /** 广告拦截：ad 类直接进拦截列表。 */
  isBlocked(n: RawNotification): boolean {
    return n.category === 'ad' || this.rules.some((r) => r.action === 'mute' && n.title.includes(r.keyword));
  }

  addRule(keyword: string, action: 'mute' | 'fold'): void {
    this.rules.push({ keyword, action });
  }

  suggestRule(samples: string[]): string | null {
    const freq = new Map<string, number>();
    for (const t of samples) {
      for (const w of t.split(/\s+/)) {
        if (w.length >= 2) freq.set(w, (freq.get(w) ?? 0) + 1);
      }
    }
    let best: string | null = null;
    let bestN = 1;
    for (const [w, n] of freq) {
      if (n > bestN) { best = w; bestN = n; }
    }
    return best;
  }

  learnRule(keyword: string): void {
    this.learnedRules.push(keyword);
  }

  /** 持久/僵尸通知清理：非持久且已过时效的自动清理。 */
  cleanup(all: RawNotification[], now: number, ttlMs = 24 * 3600_000): string[] {
    return all.filter((n) => !n.persistent && now - n.at > ttlMs).map((n) => n.id);
  }

  groupOf(scored: ScoredNotification[]): Map<string, ScoredNotification[]> {
    const m = new Map<string, ScoredNotification[]>();
    for (const s of scored) {
      const arr = m.get(s.groupKey) ?? [];
      arr.push(s);
      m.set(s.groupKey, arr);
    }
    return m;
  }

  healthScore(total: number, folded: number, blocked: number): number {
    if (total === 0) return 100;
    const noise = folded + blocked;
    return Math.max(0, Math.round((1 - noise / total) * 100));
  }
}

/* ===================== 族0307 通知模板 ===================== */

export const NOTIFICATION_TEMPLATES = [
  'minimal', 'card', 'big-picture', 'progress', 'media', 'invite', 'mail', 'conversation',
  'location', 'weather', 'stock', 'shopping', 'system-update', 'security-alert', 'backup',
  'download', 'upload', 'print', 'call-slot', 'cross-device-slot', 'dev-docs', 'preview',
  'ab-test', 'spec', 'tutorial',
] as const;

export type NotificationTemplateKind = (typeof NOTIFICATION_TEMPLATES)[number];

export interface TemplatePayload {
  kind: NotificationTemplateKind;
  title: string;
  lines: string[];
  progress?: number;
  actions?: string[];
}

/** 通知模板注册表：25 种版式 + 预览台 + A/B 对比。 */
export class NotificationTemplateRegistry {
  private ab = new Map<string, { a: NotificationTemplateKind; b: NotificationTemplateKind; aShown: number; aClicked: number; bShown: number; bClicked: number }>();

  render(kind: NotificationTemplateKind, payload: Omit<TemplatePayload, 'kind'>): TemplatePayload {
    return { kind, ...payload };
  }

  isSupported(kind: string): kind is NotificationTemplateKind {
    return (NOTIFICATION_TEMPLATES as readonly string[]).includes(kind);
  }

  /** 占位模板（通话/跨设备）按 §15 口径：接口冻结 + 类型存在。 */
  isPlaceholderSlot(kind: NotificationTemplateKind): boolean {
    return kind === 'call-slot' || kind === 'cross-device-slot';
  }

  startAb(key: string, a: NotificationTemplateKind, b: NotificationTemplateKind): void {
    this.ab.set(key, { a, b, aShown: 0, aClicked: 0, bShown: 0, bClicked: 0 });
  }

  recordAb(key: string, arm: 'a' | 'b', clicked: boolean): void {
    const e = this.ab.get(key);
    if (!e) return;
    if (arm === 'a') { e.aShown += 1; if (clicked) e.aClicked += 1; }
    else { e.bShown += 1; if (clicked) e.bClicked += 1; }
  }

  abWinner(key: string): 'a' | 'b' | 'tie' {
    const e = this.ab.get(key);
    if (!e || (e.aShown === 0 && e.bShown === 0)) return 'tie';
    const ra = e.aShown ? e.aClicked / e.aShown : 0;
    const rb = e.bShown ? e.bClicked / e.bShown : 0;
    if (ra === rb) return 'tie';
    return ra > rb ? 'a' : 'b';
  }
}

/* ===================== 族0308 弹出礼仪 ===================== */

export interface PopupContext {
  typing?: boolean;
  gameFullscreen?: boolean;
  presenting?: boolean;
  recording?: boolean;
  focus?: boolean;
}

export interface PopupPolicy {
  stackLimit: number;
  lowPriorityToCenterOnly: boolean;
  followMouseScreen: boolean;
  animationStyle: 'slide' | 'fade' | 'pop' | 'none';
  perTypeDurationMs: Record<string, number>;
}

export const DEFAULT_POPUP_POLICY: PopupPolicy = {
  stackLimit: 3,
  lowPriorityToCenterOnly: true,
  followMouseScreen: true,
  animationStyle: 'slide',
  perTypeDurationMs: { notify: 5000, warning: 8000, error: 0 },
};

/** 弹出礼仪：打断守卫 / 堆叠上限 / 合并 / 行为路由。 */
export class PopupEtiquette {
  private policy: PopupPolicy = { ...DEFAULT_POPUP_POLICY };
  private stack: string[] = [];

  updatePolicy(patch: Partial<PopupPolicy>): void {
    this.policy = { ...this.policy, ...patch };
  }

  policy_(): PopupPolicy {
    return { ...this.policy };
  }

  /** 是否允许此刻弹出：打字/游戏/演示/录制/专注期间静默（进中心不弹）。 */
  canPopup(ctx: PopupContext, priority: 'low' | 'high'): boolean {
    if (ctx.typing || ctx.gameFullscreen || ctx.presenting || ctx.recording || ctx.focus) return false;
    if (priority === 'low' && this.policy.lowPriorityToCenterOnly) return false;
    return true;
  }

  /** 同源合并 key。 */
  mergeKey(app: string, category: string): string {
    return `${app}/${category}`;
  }

  /** 堆叠上限：最多 stackLimit 条，超出最旧出栈。 */
  push(id: string): string | null {
    this.stack.push(id);
    if (this.stack.length > this.policy.stackLimit) return this.stack.shift() ?? null;
    return null;
  }

  stackAll(): readonly string[] {
    return this.stack;
  }

  clear(): void {
    this.stack = [];
  }

  durationFor(type: string): number {
    return this.policy.perTypeDurationMs[type] ?? 5000;
  }
}

/* ===================== 族0309 勿扰体系 ===================== */

export type DndSource = 'manual' | 'schedule' | 'game' | 'presenting' | 'recording' | 'pomodoro' | 'meeting' | 'night' | 'child';

export interface DndState {
  active: boolean;
  source: DndSource;
  startedAt: number;
  endsAt?: number;
}

export interface MissedNotification {
  id: string;
  app: string;
  title: string;
  at: number;
}

/** 勿扰体系：开关/计划/例外/重呼放行/自动场景/历史与结束摘要。 */
export class DndSystem {
  private state: DndState = { active: false, source: 'manual', startedAt: 0 };
  private schedules: Array<{ fromHour: number; toHour: number; source: DndSource }> = [];
  private starred = new Set<string>();
  private missed: MissedNotification[] = [];
  private sessions: Array<{ startedAt: number; endedAt: number; count: number }> = [];
  private recentCalls = new Map<string, number[]>();

  enable(source: DndSource, at: number, endsAt?: number): void {
    this.state = { active: true, source, startedAt: at, endsAt };
  }

  disable(at: number): MissedNotification[] {
    const count = this.missed.length;
    if (this.state.active) this.sessions.push({ startedAt: this.state.startedAt, endedAt: at, count });
    this.state = { active: false, source: 'manual', startedAt: 0 };
    return this.missed.splice(0);
  }

  isActive(): boolean {
    return this.state.active;
  }

  addSchedule(fromHour: number, toHour: number, source: DndSource): void {
    this.schedules.push({ fromHour, toHour, source });
  }

  /** 计划判定：按小时自动进入勿扰。 */
  scheduledAt(hour: number): DndSource | null {
    for (const s of this.schedules) {
      if (s.fromHour <= hour && hour < s.toHour) return s.source;
    }
    return null;
  }

  star(id: string): void {
    this.starred.add(id);
  }

  isStarred(id: string): boolean {
    return this.starred.has(id);
  }

  /** 重呼放行：同一 key 3 分钟内呼到第 3 次则放行。 */
  shouldPassThrough(key: string, at: number): boolean {
    const arr = (this.recentCalls.get(key) ?? []).filter((t) => at - t < 3 * 60_000);
    arr.push(at);
    this.recentCalls.set(key, arr);
    return arr.length >= 3;
  }

  /** 收到通知：星标或重呼放行的直接响，其余记为错过。 */
  receive(n: MissedNotification, at: number): boolean {
    if (!this.state.active) return true;
    if (this.isStarred(n.id) || this.shouldPassThrough(`${n.app}:${n.title}`, at)) return true;
    this.missed.push(n);
    return false;
  }

  missedAll(): readonly MissedNotification[] {
    return this.missed;
  }

  endSummary(_at: number): { startedAt: number; endedAt: number; count: number } | null {
    const s = this.sessions[this.sessions.length - 1];
    return s ?? null;
  }

  sessionsCount(): number {
    return this.sessions.length;
  }
}

/* ===================== 族0310 声音调试 ===================== */

export interface AudioPathProbe {
  device: string;
  muted: boolean;
  volume: number;
  exclusiveOccupied?: string;
  sampleRate?: number;
  codec?: string;
}

/** 声音调试：测试台 / 路径检查 / 延迟与编解码 / 独占采样诊断 / 无声向导 / 计量表。 */
export class AudioDebugConsole {
  private logs: string[] = [];
  private events: Array<{ name: string; at: number; payload?: unknown }> = [];

  probePath(steps: AudioPathProbe[]): { ok: boolean; blockers: string[] } {
    const blockers: string[] = [];
    for (const s of steps) {
      if (s.muted) blockers.push(`${s.device}:muted`);
      if (s.volume <= 0) blockers.push(`${s.device}:zero-volume`);
      if (s.exclusiveOccupied) blockers.push(`${s.device}:exclusive-by-${s.exclusiveOccupied}`);
    }
    this.logs.push(`path-check ${blockers.length} blockers`);
    return { ok: blockers.length === 0, blockers };
  }

  measureLatency(cb: () => void): number {
    const t0 = performance.now();
    cb();
    const ms = Math.round((performance.now() - t0) * 100) / 100;
    this.logs.push(`latency ${ms}ms`);
    return ms;
  }

  detectCrackle(samples: number[]): boolean {
    let peaks = 0;
    for (let i = 1; i < samples.length; i++) {
      if (Math.abs(samples[i]! - samples[i - 1]!) > 0.9) peaks += 1;
    }
    return peaks >= 3;
  }

  noSoundWizard(probes: AudioPathProbe[]): { advice: string } {
    const { blockers } = this.probePath(probes);
    if (blockers.some((b) => b.includes(':muted'))) return { advice: '设备处于静音，请取消静音' };
    if (blockers.some((b) => b.includes(':zero-volume'))) return { advice: '音量为 0，请调高音量' };
    if (blockers.some((b) => b.includes(':exclusive-by-'))) return { advice: '被独占模式占用，请关闭独占或结束占用方' };
    return { advice: '路径正常，请检查应用内音量与默认设备路由' };
  }

  log(line: string): void {
    this.logs.push(line);
  }

  logsAll(): readonly string[] {
    return this.logs;
  }

  recordEvent(name: string, at: number, payload?: unknown): void {
    this.events.push({ name, at, payload });
  }

  /** 事件回放：按时间序重放全部记录事件。 */
  replay(): Array<{ name: string; at: number; payload?: unknown }> {
    return [...this.events].sort((a, b) => a.at - b.at);
  }

  /** 响度表：近似 LUFS（对短窗 RMS 的简化估计）。 */
  lufs(samples: number[]): number {
    const rms = Math.sqrt(samples.reduce((s, v) => s + v * v, 0) / Math.max(1, samples.length));
    const lufs = 20 * Math.log10(Math.max(rms, 1e-9)) - 0.691;
    return Math.round(lufs * 100) / 100;
  }

  truePeak(samples: number[]): number {
    return Math.max(...samples.map((s) => Math.abs(s)), 0);
  }

  normalizeAdvice(lufs: number, target = -16): string {
    const d = Math.round((target - lufs) * 10) / 10;
    if (Math.abs(d) < 0.5) return '响度已达标';
    return d > 0 ? `建议提升 ${d} dB` : `建议降低 ${-d} dB`;
  }

  routeGraph(devices: string[]): Array<{ from: string; to: string }> {
    return devices.slice(1).map((d, i) => ({ from: devices[i]!, to: d }));
  }
}
