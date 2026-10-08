// UNREAL-X-15000: AI-36 批次（领域10 安全与隐私 · 族0358~0360 · X08926~X09000），勿删。
// 文件隐私 2.0 / 生物认证 2.0 / 防火墙中心 2.0。

import { BatchQueue, GuardRegistry } from './core';

/* ===================== 族0358 文件隐私 2.0（X08926~X08950） ===================== */

export const FILEPRIVACY_LEVELS = ['standard', 'safe', 'hidden', 'vaulted', 'shredded'] as const;
export type FilePrivacyLevel = (typeof FILEPRIVACY_LEVELS)[number];

export class FilePrivacyManager {
  profileId: FilePrivacyLevel;
  clamped = 0;
  private marked = new Set<string>();
  private shredded: string[] = [];
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  constructor(profileId: string = 'standard') {
    this.profileId = (FILEPRIVACY_LEVELS as readonly string[]).includes(profileId)
      ? (profileId as FilePrivacyLevel)
      : (() => {
          this.clamped = 1;
          return 'standard' as FilePrivacyLevel;
        })();
  }

  get hideExtensions(): boolean {
    return this.profileId !== 'standard';
  }
  get shredOnDelete(): boolean {
    return this.profileId === 'shredded' || this.profileId === 'vaulted';
  }

  markSensitive(path: string): boolean {
    if (this.marked.has(path)) return false;
    this.marked.add(path);
    return true;
  }
  isMarked(path: string): boolean {
    return this.marked.has(path);
  }
  shred(path: string): boolean {
    if (!this.marked.has(path)) return false;
    this.marked.delete(path);
    this.shredded.push(path);
    return true;
  }
  get shreddedCount(): number {
    return this.shredded.length;
  }
  /** 对外展示路径：敏感路径打码。 */
  displayPath(path: string): string {
    return this.isMarked(path) ? path.replace(/[^/]+$/, '•••') : path;
  }
  markPending(path: string): void {
    this.marked.add(path);
  }
  resumePending(): number {
    return this.shredded.length;
  }
  reset(): void {
    this.marked.clear();
    this.shredded = [];
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
  serialize(): string {
    return JSON.stringify({ v: 2, p: this.profileId, marked: [...this.marked] });
  }
  static deserialize(json: string): FilePrivacyManager {
    try {
      const o = JSON.parse(json) as { v?: number; p?: string };
      if (o.v !== 2) return new FilePrivacyManager();
      return new FilePrivacyManager(o.p ?? 'standard');
    } catch {
      return new FilePrivacyManager();
    }
  }
}

/* ===================== 族0359 生物认证 2.0（X08951~X08975） ===================== */

export const BIOAUTH_LEVELS = ['off', 'convenience', 'standard', 'sensitive', 'fortress'] as const;
export type BioAuthLevel = (typeof BIOAUTH_LEVELS)[number];

const BIO_MATRIX: Record<BioAuthLevel, { maxTries: number; cooldownMs: number; fallbackPin: boolean; liveDetection: boolean }> = {
  off: { maxTries: 0, cooldownMs: 0, fallbackPin: true, liveDetection: false },
  convenience: { maxTries: 5, cooldownMs: 1_000, fallbackPin: true, liveDetection: false },
  standard: { maxTries: 3, cooldownMs: 5_000, fallbackPin: true, liveDetection: false },
  sensitive: { maxTries: 3, cooldownMs: 30_000, fallbackPin: true, liveDetection: true },
  fortress: { maxTries: 2, cooldownMs: 60_000, fallbackPin: false, liveDetection: true },
};

export class BioAuthSystem {
  profileId: BioAuthLevel;
  clamped = 0;
  private enrolled = new Map<string, number>();
  private fails = new Map<string, number>();
  private lockedUntil = new Map<string, number>();
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  constructor(profileId: string = 'standard') {
    this.profileId = (BIOAUTH_LEVELS as readonly string[]).includes(profileId)
      ? (profileId as BioAuthLevel)
      : (() => {
          this.clamped = 1;
          return 'standard' as BioAuthLevel;
        })();
  }

  get profile() {
    return BIO_MATRIX[this.profileId];
  }

  enroll(userId: string, scorePmil: number): boolean {
    // 质量门：指纹/掌纹质量 >= 850 permille 才可登记。
    if (scorePmil < 850) return false;
    this.enrolled.set(userId, scorePmil);
    return true;
  }
  isEnrolled(userId: string): boolean {
    return this.enrolled.has(userId);
  }
  get enrolledCount(): number {
    return this.enrolled.size;
  }

  /** 认证尝试：连续失败触发冷却锁。 */
  verify(userId: string, ok: boolean, now: number): { pass: boolean; lockedFor: number } {
    const until = this.lockedUntil.get(userId) ?? 0;
    if (now < until) return { pass: false, lockedFor: until - now };
    const tries = this.fails.get(userId) ?? 0;
    if (ok) {
      this.fails.delete(userId);
      return { pass: true, lockedFor: 0 };
    }
    const next = tries + 1;
    this.fails.set(userId, next);
    if (next >= this.profile.maxTries && this.profile.maxTries > 0) {
      this.lockedUntil.set(userId, now + this.profile.cooldownMs);
      this.fails.delete(userId);
      return { pass: false, lockedFor: this.profile.cooldownMs };
    }
    return { pass: false, lockedFor: 0 };
  }

  degrade(): BioAuthLevel {
    const i = (BIOAUTH_LEVELS as readonly string[]).indexOf(this.profileId);
    this.profileId = BIOAUTH_LEVELS[Math.max(0, i - 1)]!;
    return this.profileId;
  }
  reset(): void {
    this.enrolled.clear();
    this.fails.clear();
    this.lockedUntil.clear();
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
}

/* ===================== 族0360 防火墙中心 2.0（X08976~X09000） ===================== */

export const FIREWALL_LEVELS = ['allow-all', 'outbound', 'filtered', 'strict', 'airlock'] as const;
export type FirewallLevel = (typeof FIREWALL_LEVELS)[number];

export interface FwRule {
  id: number;
  host: string;
  port: number;
  allow: boolean;
}

export class FirewallCenter {
  profileId: FirewallLevel;
  clamped = 0;
  private rules = new Map<number, FwRule>();
  private nextId = 1;
  private logged: Array<{ ruleId: number; verdict: 'allow' | 'deny'; at: number }> = [];
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  constructor(profileId: string = 'outbound') {
    this.profileId = (FIREWALL_LEVELS as readonly string[]).includes(profileId)
      ? (profileId as FirewallLevel)
      : (() => {
          this.clamped = 1;
          return 'outbound' as FirewallLevel;
        })();
  }

  /** 默认策略：越严格默认越拒绝。 */
  get defaultDeny(): boolean {
    return this.profileId !== 'allow-all';
  }

  addRule(host: string, port: number, allow: boolean): FwRule {
    // 端口钳制：0~65535 之外回 443。
    const p = port >= 0 && port <= 65535 ? port : 443;
    const rule: FwRule = { id: this.nextId, host, port: p, allow };
    this.nextId += 1;
    this.rules.set(rule.id, rule);
    return rule;
  }
  ruleOf(id: number): FwRule | undefined {
    return this.rules.get(id);
  }
  get ruleCount(): number {
    return this.rules.size;
  }

  /** 规则匹配 + 默认策略兜底。 */
  decide(host: string, port: number, at: number): 'allow' | 'deny' {
    let verdict: 'allow' | 'deny' = this.defaultDeny ? 'deny' : 'allow';
    for (const r of this.rules.values()) {
      if ((r.host === '*' || r.host === host) && (r.port === 0 || r.port === port)) {
        verdict = r.allow ? 'allow' : 'deny';
      }
    }
    this.logged.push({ ruleId: 0, verdict, at });
    return verdict;
  }
  get log(): ReadonlyArray<{ ruleId: number; verdict: 'allow' | 'deny'; at: number }> {
    return this.logged;
  }
  markPending(id: number): void {
    const r = this.rules.get(id);
    if (r) this.rules.set(id, { ...r });
  }
  resumePending(): number {
    return this.logged.length;
  }
  degrade(): FirewallLevel {
    const i = (FIREWALL_LEVELS as readonly string[]).indexOf(this.profileId);
    this.profileId = FIREWALL_LEVELS[Math.max(0, i - 1)]!;
    return this.profileId;
  }
  reset(): void {
    this.rules.clear();
    this.logged = [];
    this.nextId = 1;
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
}
