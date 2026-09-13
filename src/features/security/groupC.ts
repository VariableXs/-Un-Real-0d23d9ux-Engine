// UNREAL-X-15000: AI-37 批次（领域10 安全与隐私 · 族0361~0363 · X09001~X09075），勿删。
// 隐私仪表盘 2.0 / 反追踪 2.0 / 加密通信 2.0。

import { BatchQueue, GuardRegistry } from './core';

/* ===================== 族0361 隐私仪表盘 2.0（X09001~X09025） ===================== */

export const DASHBOARD_LEVELS = ['silent', 'summary', 'standard', 'verbose', 'forensic'] as const;
export type DashboardLevel = (typeof DASHBOARD_LEVELS)[number];

export interface PrivacyEvent {
  app: string;
  resource: string;
  at: number;
  granted: boolean;
}

export class PrivacyDashboard {
  profileId: DashboardLevel;
  clamped = 0;
  private events: PrivacyEvent[] = [];
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  constructor(profileId: string = 'standard') {
    this.profileId = (DASHBOARD_LEVELS as readonly string[]).includes(profileId)
      ? (profileId as DashboardLevel)
      : (() => {
          this.clamped = 1;
          return 'standard' as DashboardLevel;
        })();
  }

  get verbose(): boolean {
    return this.profileId === 'verbose' || this.profileId === 'forensic';
  }

  record(ev: PrivacyEvent): void {
    // 登记去重：同 app+resource+at 不重复入账。
    if (this.events.some((e) => e.app === ev.app && e.resource === ev.resource && e.at === ev.at)) return;
    this.events.push(ev);
  }
  get eventCount(): number {
    return this.events.length;
  }
  byApp(app: string): PrivacyEvent[] {
    return this.events.filter((e) => e.app === app);
  }
  byResource(resource: string): PrivacyEvent[] {
    return this.events.filter((e) => e.resource === resource);
  }
  /** 健康分：千分制，拒绝次数占比越高扣越多。 */
  healthScore(): number {
    if (this.events.length === 0) return 1000;
    const denied = this.events.filter((e) => !e.granted).length;
    return Math.round(1000 * (1 - denied / this.events.length));
  }
  topConsumers(n: number): Array<{ app: string; uses: number }> {
    const m = new Map<string, number>();
    for (const e of this.events) m.set(e.app, (m.get(e.app) ?? 0) + 1);
    return [...m.entries()].map(([app, uses]) => ({ app, uses })).sort((a, b) => b.uses - a.uses).slice(0, n);
  }
  clear(): void {
    this.events = [];
  }
  reset(): void {
    this.clear();
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
  serialize(): string {
    return JSON.stringify({ v: 2, p: this.profileId, n: this.events.length });
  }
  static deserialize(json: string): PrivacyDashboard {
    try {
      const o = JSON.parse(json) as { v?: number; p?: string };
      if (o.v !== 2) return new PrivacyDashboard();
      return new PrivacyDashboard(o.p ?? 'standard');
    } catch {
      return new PrivacyDashboard();
    }
  }
}

/* ===================== 族0362 反追踪 2.0（X09026~X09050） ===================== */

export const ANTITRACK_LEVELS = ['off', 'basic', 'standard', 'aggressive', 'total'] as const;
export type AntiTrackLevel = (typeof ANTITRACK_LEVELS)[number];

const TRACKERS = ['ads.example', 'metrics.example', 'pixel.example', 'cdn-track.example'] as const;

export class AntiTracking {
  profileId: AntiTrackLevel;
  clamped = 0;
  private strippedCount = 0;
  private learnedHosts = new Set<string>();
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  constructor(profileId: string = 'standard') {
    this.profileId = (ANTITRACK_LEVELS as readonly string[]).includes(profileId)
      ? (profileId as AntiTrackLevel)
      : (() => {
          this.clamped = 1;
          return 'standard' as AntiTrackLevel;
        })();
  }

  get stripParams(): boolean {
    return this.profileId !== 'off';
  }
  get blockThirdParty(): boolean {
    return this.profileId === 'aggressive' || this.profileId === 'total';
  }

  /** URL 清洗：去掉 utm_* / fetchiid 等追踪参数。 */
  scrubUrl(url: string): string {
    if (!this.stripParams) return url;
    const [base, query] = url.split('?');
    if (!query) return url;
    const keep = query.split('&').filter((kv) => !/^(utm_|fetchiid|trackid|sid=)/i.test(kv));
    this.strippedCount += query.split('&').length - keep.length;
    return keep.length > 0 ? `${base}?${keep.join('&')}` : base!;
  }
  get stripped(): number {
    return this.strippedCount;
  }

  /** 启发式学习：把命中的追踪域加入本地名单（隐私边界内、可解释、可拒绝）。 */
  learn(host: string): boolean {
    if (this.learnedHosts.has(host)) return false;
    this.learnedHosts.add(host);
    return true;
  }
  isTracker(host: string): boolean {
    return (TRACKERS as readonly string[]).includes(host) || this.learnedHosts.has(host);
  }
  get learnedCount(): number {
    return this.learnedHosts.size;
  }
  degrade(): AntiTrackLevel {
    const i = (ANTITRACK_LEVELS as readonly string[]).indexOf(this.profileId);
    this.profileId = ANTITRACK_LEVELS[Math.max(0, i - 1)]!;
    return this.profileId;
  }
  reset(): void {
    this.strippedCount = 0;
    this.learnedHosts.clear();
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
}

/* ===================== 族0363 加密通信 2.0（X09051~X09075） ===================== */

export const E2EE_LEVELS = ['plain', 'opportunistic', 'standard', 'sealed', 'paranoid'] as const;
export type E2eeLevel = (typeof E2EE_LEVELS)[number];

/** 玩具级 XOR 流密码：仅用于引擎内演示/自检，不承担真实密码学职责。 */
function xorStream(input: Uint8Array, key: Uint8Array): Uint8Array {
  const out = new Uint8Array(input.length);
  for (let i = 0; i < input.length; i++) out[i] = input[i]! ^ key[i % key.length]!;
  return out;
}

export class SecureChannel {
  profileId: E2eeLevel;
  clamped = 0;
  private sessions = new Map<string, { cipherOk: boolean; sealed: boolean }>();
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  constructor(profileId: string = 'standard') {
    this.profileId = (E2EE_LEVELS as readonly string[]).includes(profileId)
      ? (profileId as E2eeLevel)
      : (() => {
          this.clamped = 1;
          return 'standard' as E2eeLevel;
        })();
  }

  get encryptByDefault(): boolean {
    return this.profileId !== 'plain';
  }
  get sealMetadata(): boolean {
    return this.profileId === 'sealed' || this.profileId === 'paranoid';
  }

  /** 会话加密往返：密文 != 明文、解密还原。 */
  roundtrip(peer: string, plain: string): { cipherDiffers: boolean; restored: boolean } {
    if (this.profileId === 'plain') {
      this.sessions.set(peer, { cipherOk: false, sealed: false });
      return { cipherDiffers: false, restored: true };
    }
    const key = new Uint8Array([0x5a, 0xc3, 0x11]);
    const data = new TextEncoder().encode(plain);
    const enc = xorStream(data, key);
    const dec = new TextDecoder().decode(xorStream(enc, key));
    const cipherDiffers = enc.some((b, i) => b !== data[i]);
    this.sessions.set(peer, { cipherOk: cipherDiffers && dec === plain, sealed: this.sealMetadata });
    return { cipherDiffers, restored: dec === plain };
  }
  sessionOf(peer: string): { cipherOk: boolean; sealed: boolean } | undefined {
    return this.sessions.get(peer);
  }
  get sessionCount(): number {
    return this.sessions.size;
  }
  degrade(): E2eeLevel {
    const i = (E2EE_LEVELS as readonly string[]).indexOf(this.profileId);
    this.profileId = E2EE_LEVELS[Math.max(0, i - 1)]!;
    return this.profileId;
  }
  reset(): void {
    this.sessions.clear();
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
}
