// UNREAL-X-15000: AI-36 批次（领域10 安全与隐私 · 族0355~0357 · X08851~X08925），勿删。
// 保险箱 2.0 / 网络隐私 2.0 / 屏幕隐私 2.0。

import { BatchQueue, GuardRegistry } from './core';

/* ===================== 族0355 保险箱 2.0（X08851~X08875） ===================== */

export const VAULT_LEVELS = ['open', 'standard', 'strict', 'paranoid', 'ironbox'] as const;
export type VaultLevel = (typeof VAULT_LEVELS)[number];

export const VAULT_MATRIX: Record<VaultLevel, { autoLockMs: number; clipHistory: boolean; thumbRequired: boolean }> = {
  open: { autoLockMs: 0, clipHistory: true, thumbRequired: false },
  standard: { autoLockMs: 5 * 60_000, clipHistory: true, thumbRequired: false },
  strict: { autoLockMs: 60_000, clipHistory: false, thumbRequired: false },
  paranoid: { autoLockMs: 15_000, clipHistory: false, thumbRequired: true },
  ironbox: { autoLockMs: 5_000, clipHistory: false, thumbRequired: true },
};

export interface VaultItem {
  id: string;
  sizeKb: number;
  tags: string[];
}

export class VaultSystem {
  profileId: VaultLevel;
  clamped = 0;
  private locked = false;
  private items = new Map<string, VaultItem>();
  private pendingIds = new Set<string>();
  private audit: Array<{ op: string; id: string; at: number }> = [];
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  constructor(profileId: string = 'standard') {
    this.profileId = (VAULT_LEVELS as readonly string[]).includes(profileId)
      ? (profileId as VaultLevel)
      : (() => {
          this.clamped = 1;
          return 'standard' as VaultLevel;
        })();
  }

  get profile() {
    return VAULT_MATRIX[this.profileId];
  }

  get autoLockMs(): number {
    return this.pressure ? Math.min(this.profile.autoLockMs, 15_000) : this.profile.autoLockMs;
  }

  lock(): void {
    this.locked = true;
  }
  unlock(): void {
    this.locked = false;
  }
  get isLocked(): boolean {
    return this.locked;
  }

  put(item: VaultItem, at: number): boolean {
    if (this.locked) return false;
    const existed = this.items.has(item.id);
    this.items.set(item.id, item);
    this.pendingIds.delete(item.id);
    this.audit.push({ op: existed ? 'update' : 'create', id: item.id, at });
    return true;
  }
  get(id: string): VaultItem | undefined {
    return this.locked ? undefined : this.items.get(id);
  }
  remove(id: string, at: number): boolean {
    if (this.locked || !this.items.has(id)) return false;
    this.items.delete(id);
    this.audit.push({ op: 'remove', id, at });
    return true;
  }
  get count(): number {
    return this.items.size;
  }
  auditTrail(): ReadonlyArray<{ op: string; id: string; at: number }> {
    return this.audit;
  }
  markPending(id: string): void {
    this.pendingIds.add(id);
  }
  resumePending(): number {
    const n = this.pendingIds.size;
    this.pendingIds.clear();
    return n;
  }
  reset(): void {
    this.items.clear();
    this.pendingIds.clear();
    this.audit = [];
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
  serialize(): string {
    return JSON.stringify({ v: 2, p: this.profileId, items: [...this.items.values()] });
  }
  static deserialize(json: string): VaultSystem {
    try {
      const o = JSON.parse(json) as { v?: number; p?: string };
      if (o.v !== 2) return new VaultSystem();
      return new VaultSystem(o.p ?? 'standard');
    } catch {
      return new VaultSystem();
    }
  }
}

/* ===================== 族0356 网络隐私 2.0（X08876~X08900） ===================== */

export const NETPRIVACY_LEVELS = ['transparent', 'balanced', 'guarded', 'strict', 'ghost'] as const;
export type NetPrivacyLevel = (typeof NETPRIVACY_LEVELS)[number];

export const NET_MATRIX: Record<NetPrivacyLevel, { sendReferrer: boolean; sendUa: boolean; dnsOverTls: boolean; blockFingerprint: boolean }> = {
  transparent: { sendReferrer: true, sendUa: true, dnsOverTls: false, blockFingerprint: false },
  balanced: { sendReferrer: false, sendUa: true, dnsOverTls: true, blockFingerprint: false },
  guarded: { sendReferrer: false, sendUa: false, dnsOverTls: true, blockFingerprint: true },
  strict: { sendReferrer: false, sendUa: false, dnsOverTls: true, blockFingerprint: true },
  ghost: { sendReferrer: false, sendUa: false, dnsOverTls: true, blockFingerprint: true },
};

export class NetPrivacyManager {
  profileId: NetPrivacyLevel;
  clamped = 0;
  private allowlist = new Set<string>();
  private blocked = new Set<string>();
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  constructor(profileId: string = 'balanced') {
    this.profileId = (NETPRIVACY_LEVELS as readonly string[]).includes(profileId)
      ? (profileId as NetPrivacyLevel)
      : (() => {
          this.clamped = 1;
          return 'balanced' as NetPrivacyLevel;
        })();
  }

  get profile() {
    return NET_MATRIX[this.profileId];
  }

  /** 出站请求裁剪：按档位抹掉 referrer/UA 指纹。 */
  outbound(url: string): { url: string; referrer: string; ua: string } {
    const p = this.profile;
    return {
      url,
      referrer: p.sendReferrer ? 'https://ref.example' : '',
      ua: p.sendUa ? 'VarixUA/2.0' : '',
    };
  }

  block(host: string): boolean {
    if (this.blocked.has(host)) return false;
    this.blocked.add(host);
    return true;
  }
  isBlocked(host: string): boolean {
    return this.blocked.has(host);
  }
  allow(host: string): void {
    this.allowlist.add(host);
  }
  get allowedHosts(): string[] {
    return [...this.allowlist];
  }
  get blockedCount(): number {
    return this.blocked.size;
  }
  /** 压力降级：ghost→guarded（保留隐私底线、放开部分开销）。 */
  degrade(): NetPrivacyLevel {
    if (this.profileId === 'ghost') this.profileId = 'guarded';
    return this.profileId;
  }
  reset(): void {
    this.blocked.clear();
    this.allowlist.clear();
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
  serialize(): string {
    return JSON.stringify({ v: 2, p: this.profileId, blocked: [...this.blocked] });
  }
  static deserialize(json: string): NetPrivacyManager {
    try {
      const o = JSON.parse(json) as { v?: number; p?: string };
      if (o.v !== 2) return new NetPrivacyManager();
      const m = new NetPrivacyManager(o.p ?? 'balanced');
      return m;
    } catch {
      return new NetPrivacyManager();
    }
  }
}

/* ===================== 族0357 屏幕隐私 2.0（X08901~X08925） ===================== */

export const SCREENPRIVACY_LEVELS = ['off', 'dim', 'blur', 'mask', 'blackout'] as const;
export type ScreenPrivacyLevel = (typeof SCREENPRIVACY_LEVELS)[number];

export const SCREEN_MATRIX: Record<ScreenPrivacyLevel, { opacityPmil: number; blurPx: number; shoulderGuard: boolean }> = {
  off: { opacityPmil: 1000, blurPx: 0, shoulderGuard: false },
  dim: { opacityPmil: 700, blurPx: 0, shoulderGuard: false },
  blur: { opacityPmil: 1000, blurPx: 12, shoulderGuard: false },
  mask: { opacityPmil: 600, blurPx: 20, shoulderGuard: true },
  blackout: { opacityPmil: 0, blurPx: 0, shoulderGuard: true },
};

export class ScreenPrivacyShield {
  profileId: ScreenPrivacyLevel;
  clamped = 0;
  private zones = new Set<string>();
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;
  private history: Array<{ zone: string; level: ScreenPrivacyLevel }> = [];

  constructor(profileId: string = 'off') {
    this.profileId = (SCREENPRIVACY_LEVELS as readonly string[]).includes(profileId)
      ? (profileId as ScreenPrivacyLevel)
      : (() => {
          this.clamped = 1;
          return 'off' as ScreenPrivacyLevel;
        })();
  }

  get profile() {
    return SCREEN_MATRIX[this.profileId];
  }

  /** 低配降级链：mask→blur→dim。 */
  static DEGRADE_CHAIN = ['blackout', 'blur', 'dim'] as const;

  addZone(zone: string): boolean {
    if (this.zones.has(zone)) return false;
    this.zones.add(zone);
    this.history.push({ zone, level: this.profileId });
    return true;
  }
  hasZone(zone: string): boolean {
    return this.zones.has(zone);
  }
  get zoneCount(): number {
    return this.zones.size;
  }
  setLevel(level: string): void {
    if ((SCREENPRIVACY_LEVELS as readonly string[]).includes(level)) {
      this.profileId = level as ScreenPrivacyLevel;
    } else {
      this.clamped += 1;
    }
  }
  get eventHistory(): ReadonlyArray<{ zone: string; level: ScreenPrivacyLevel }> {
    return this.history;
  }
  degrade(): ScreenPrivacyLevel {
    if (this.profileId === 'mask') this.profileId = 'blur';
    else if (this.profileId === 'blur') this.profileId = 'dim';
    return this.profileId;
  }
  reset(): void {
    this.zones.clear();
    this.history = [];
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
}
