// UNREAL-X-15000: AI-37 批次（领域10 安全与隐私 · 族0367~0370 · X09151~X09250），勿删。
// 完整性 2.0 / 会话身份 2.0 / 物理安全 2.0 / 备份安全 2.0。

import { BatchQueue, GuardRegistry } from './core';

/* ===================== 族0367 完整性 2.0（X09151~X09175） ===================== */

/** FNV-1a 32 位：引擎内完整性摘要（演示口径）。 */
export function fnv1a(data: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < data.length; i++) {
    h ^= data.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
}

export const INTEGRITY_LEVELS = ['observe', 'warn', 'verify', 'enforce', 'sealed'] as const;
export type IntegrityLevel = (typeof INTEGRITY_LEVELS)[number];

export class IntegrityMonitor {
  profileId: IntegrityLevel;
  clamped = 0;
  private digests = new Map<string, number>();
  private violations: string[] = [];
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  constructor(profileId: string = 'verify') {
    this.profileId = (INTEGRITY_LEVELS as readonly string[]).includes(profileId)
      ? (profileId as IntegrityLevel)
      : (() => {
          this.clamped = 1;
          return 'verify' as IntegrityLevel;
        })();
  }

  get blockOnFail(): boolean {
    return this.profileId === 'enforce' || this.profileId === 'sealed';
  }

  seal(path: string, content: string): number {
    const d = fnv1a(content);
    this.digests.set(path, d);
    return d;
  }
  digestOf(path: string): number | undefined {
    return this.digests.get(path);
  }
  /** 校验：内容被篡改即检出。 */
  verify(path: string, content: string): boolean {
    const sealed = this.digests.get(path);
    if (sealed === undefined) return true;
    const ok = fnv1a(content) === sealed;
    if (!ok) this.violations.push(path);
    return ok;
  }
  get violationCount(): number {
    return this.violations.length;
  }
  get sealedCount(): number {
    return this.digests.size;
  }
  /** 降级：档位向宽松退一档（资源紧张守护）。 */
  degrade(): IntegrityLevel {
    const i = (INTEGRITY_LEVELS as readonly string[]).indexOf(this.profileId);
    this.profileId = INTEGRITY_LEVELS[Math.max(0, i - 1)]!;
    return this.profileId;
  }
  reset(): void {
    this.digests.clear();
    this.violations = [];
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
}

/* ===================== 族0368 会话身份 2.0（X09176~X09200） ===================== */

export const SESSION_LEVELS = ['guest', 'standard', 'confirmed', 'stepped-up', 'locked'] as const;
export type SessionLevel = (typeof SESSION_LEVELS)[number];

export class SessionIdentity {
  level: SessionLevel = 'standard';
  private issuedAt = 0;
  private ttlMs: number;
  private tokens = new Map<string, number>();
  clamped = 0;
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  constructor(ttlMs = 30 * 60_000) {
    this.ttlMs = ttlMs > 0 ? ttlMs : 30 * 60_000;
    if (ttlMs <= 0) this.clamped = 1;
  }

  issue(at: number): string {
    this.issuedAt = at;
    this.level = 'standard';
    const token = `sess-${at}`;
    this.tokens.set(token, at);
    return token;
  }
  get tokenCount(): number {
    return this.tokens.size;
  }
  /** 有效期校验。 */
  alive(at: number): boolean {
    return at - this.issuedAt < this.ttlMs;
  }
  stepUp(): void {
    if (this.level !== 'locked') this.level = 'stepped-up';
  }
  lock(): void {
    this.level = 'locked';
  }
  /** 敏感操作门槛：需要 stepped-up 以上且会话存活。 */
  canSensitive(at: number): boolean {
    return this.alive(at) && (this.level === 'stepped-up' || this.level === 'confirmed');
  }
  revokeAll(): void {
    this.tokens.clear();
  }
  reset(): void {
    this.revokeAll();
    this.level = 'standard';
    this.issuedAt = 0;
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
}

/* ===================== 族0369 物理安全 2.0（X09201~X09225） ===================== */

export const PHYSICAL_LEVELS = ['off', 'notify', 'lock', 'wipe-pin', 'fort-knox'] as const;
export type PhysicalLevel = (typeof PHYSICAL_LEVELS)[number];

const PHYSICAL_MATRIX: Record<PhysicalLevel, { lidCloseLock: boolean; usbGuard: boolean; cameraShutter: boolean; micHardwareKill: boolean }> = {
  off: { lidCloseLock: false, usbGuard: false, cameraShutter: false, micHardwareKill: false },
  notify: { lidCloseLock: false, usbGuard: true, cameraShutter: false, micHardwareKill: false },
  lock: { lidCloseLock: true, usbGuard: true, cameraShutter: false, micHardwareKill: false },
  'wipe-pin': { lidCloseLock: true, usbGuard: true, cameraShutter: true, micHardwareKill: false },
  'fort-knox': { lidCloseLock: true, usbGuard: true, cameraShutter: true, micHardwareKill: true },
};

export class PhysicalSecurity {
  profileId: PhysicalLevel;
  clamped = 0;
  private tamperEvents: Array<{ kind: string; at: number }> = [];
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  constructor(profileId: string = 'lock') {
    this.profileId = (PHYSICAL_LEVELS as readonly string[]).includes(profileId)
      ? (profileId as PhysicalLevel)
      : (() => {
          this.clamped = 1;
          return 'lock' as PhysicalLevel;
        })();
  }

  get profile() {
    return PHYSICAL_MATRIX[this.profileId];
  }

  /** 篡改事件：开盖 / 拔线 / 外接键盘。 */
  tamper(kind: string, at: number): 'log' | 'lock' {
    this.tamperEvents.push({ kind, at });
    return this.profile.lidCloseLock ? 'lock' : 'log';
  }
  get tamperCount(): number {
    return this.tamperEvents.length;
  }
  get tamperLog(): ReadonlyArray<{ kind: string; at: number }> {
    return this.tamperEvents;
  }
  degrade(): PhysicalLevel {
    const i = (PHYSICAL_LEVELS as readonly string[]).indexOf(this.profileId);
    this.profileId = PHYSICAL_LEVELS[Math.max(0, i - 1)]!;
    return this.profileId;
  }
  reset(): void {
    this.tamperEvents = [];
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
}

/* ===================== 族0370 备份安全 2.0（X09226~X09250） ===================== */

export const BACKUP_LEVELS = ['manual', 'daily', 'hourly', 'continuous', 'immutable'] as const;
export type BackupLevel = (typeof BACKUP_LEVELS)[number];

export class BackupSecurity {
  profileId: BackupLevel;
  clamped = 0;
  private snapshots: Array<{ id: number; at: number; sizeKb: number; sealed: boolean }> = [];
  private nextId = 1;
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  constructor(profileId: string = 'daily') {
    this.profileId = (BACKUP_LEVELS as readonly string[]).includes(profileId)
      ? (profileId as BackupLevel)
      : (() => {
          this.clamped = 1;
          return 'daily' as BackupLevel;
        })();
  }

  get immutable(): boolean {
    return this.profileId === 'immutable';
  }
  get sealedByDefault(): boolean {
    return this.profileId !== 'manual';
  }

  /** 备份登记去重：同一时刻只留一条。 */
  backup(at: number, sizeKb: number): number {
    if (this.snapshots.some((s) => s.at === at)) return -1;
    const id = this.nextId;
    this.nextId += 1;
    this.snapshots.push({ id, at, sizeKb, sealed: this.sealedByDefault });
    return id;
  }
  get snapshotCount(): number {
    return this.snapshots.length;
  }
  snapshotOf(id: number) {
    return this.snapshots.find((s) => s.id === id);
  }
  /** 降级：备份频率向宽松退一档（资源紧张守护）。 */
  degrade(): BackupLevel {
    const i = (BACKUP_LEVELS as readonly string[]).indexOf(this.profileId);
    this.profileId = BACKUP_LEVELS[Math.max(0, i - 1)]!;
    return this.profileId;
  }
  /** 恢复点选择：取 <= 目标时刻最近一个。 */
  restorePoint(at: number) {
    let best: (typeof this.snapshots)[number] | undefined;
    for (const s of this.snapshots) if (s.at <= at && (!best || s.at > best.at)) best = s;
    return best;
  }
  /** 不可变档：拒绝删除已密封快照。 */
  deleteSnapshot(id: number): boolean {
    if (this.immutable) return false;
    const i = this.snapshots.findIndex((s) => s.id === id);
    if (i < 0) return false;
    this.snapshots.splice(i, 1);
    return true;
  }
  reset(): void {
    this.snapshots = [];
    this.nextId = 1;
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
}
