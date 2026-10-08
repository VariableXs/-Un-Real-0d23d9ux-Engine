// UNREAL-X-15000: AI-37 批次（领域10 安全与隐私 · 族0364~0366 · X09076~X09150），勿删。
// 数据主权 2.0 / 漏洞管理 / 异常检测。

import { BatchQueue, GuardRegistry } from './core';

/* ===================== 族0364 数据主权 2.0（X09076~X09100） ===================== */

export const SOVEREIGNTY_LEVELS = ['cloud-first', 'hybrid', 'local-first', 'local-only', 'airgapped'] as const;
export type SovereigntyLevel = (typeof SOVEREIGNTY_LEVELS)[number];

export class DataSovereignty {
  profileId: SovereigntyLevel;
  clamped = 0;
  private exported: object[] = [];
  private purged = 0;
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  constructor(profileId: string = 'local-first') {
    this.profileId = (SOVEREIGNTY_LEVELS as readonly string[]).includes(profileId)
      ? (profileId as SovereigntyLevel)
      : (() => {
          this.clamped = 1;
          return 'local-first' as SovereigntyLevel;
        })();
  }

  get cloudUpload(): boolean {
    return this.profileId === 'cloud-first' || this.profileId === 'hybrid';
  }

  /** 全量导出（可携带权）：JSON 通道。 */
  exportAll(data: object): string {
    const payload = { v: 2, data };
    this.exported.push(data);
    return JSON.stringify(payload);
  }
  importAll(json: string): object | null {
    try {
      const o = JSON.parse(json) as { v?: number; data?: object };
      return o.v === 2 && o.data ? o.data : null;
    } catch {
      return null;
    }
  }
  get exportCount(): number {
    return this.exported.length;
  }
  /** 删除权：彻底清除计数。 */
  purge(n: number): number {
    this.purged += n;
    return this.purged;
  }
  get purgedCount(): number {
    return this.purged;
  }
  degrade(): SovereigntyLevel {
    const i = (SOVEREIGNTY_LEVELS as readonly string[]).indexOf(this.profileId);
    this.profileId = SOVEREIGNTY_LEVELS[Math.min(SOVEREIGNTY_LEVELS.length - 1, i + 1)]!;
    return this.profileId;
  }
  reset(): void {
    this.exported = [];
    this.purged = 0;
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
}

/* ===================== 族0365 漏洞管理（X09101~X09125） ===================== */

export const VULN_SEVERITY = ['info', 'low', 'medium', 'high', 'critical'] as const;
export type VulnSeverity = (typeof VULN_SEVERITY)[number];

export interface VulnReport {
  id: string;
  severity: VulnSeverity;
  component: string;
  fixed: boolean;
}

const SEV_RANK: Record<VulnSeverity, number> = { info: 0, low: 1, medium: 2, high: 3, critical: 4 };

export class VulnManager {
  private reports = new Map<string, VulnReport>();
  clamped = 0;
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  /** 上报去重：同 id 只保留首条。 */
  file(report: VulnReport): boolean {
    if (this.reports.has(report.id)) return false;
    this.reports.set(report.id, report);
    return true;
  }
  get(id: string): VulnReport | undefined {
    return this.reports.get(id);
  }
  get count(): number {
    return this.reports.size;
  }
  open(): VulnReport[] {
    return [...this.reports.values()].filter((r) => !r.fixed);
  }
  fix(id: string): boolean {
    const r = this.reports.get(id);
    if (!r || r.fixed) return false;
    this.reports.set(id, { ...r, fixed: true });
    return true;
  }
  /** 风险分：未修复按严重度加权。 */
  riskScore(): number {
    let s = 0;
    for (const r of this.reports.values()) if (!r.fixed) s += SEV_RANK[r.severity];
    return s;
  }
  /** SLA：critical/high 优先队列。 */
  slaQueue(): VulnReport[] {
    return this.open().sort((a, b) => SEV_RANK[b.severity] - SEV_RANK[a.severity]);
  }
  reset(): void {
    this.reports.clear();
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
}

/* ===================== 族0366 异常检测（X09126~X09150） ===================== */

export const ANOMALY_LEVELS = ['quiet', 'notable', 'standard', 'sensitive', 'hair-trigger'] as const;
export type AnomalyLevel = (typeof ANOMALY_LEVELS)[number];

const ANOMALY_THRESHOLD_PMIL: Record<AnomalyLevel, number> = {
  quiet: 900,
  notable: 800,
  standard: 700,
  sensitive: 550,
  'hair-trigger': 400,
};

export class AnomalyDetector {
  profileId: AnomalyLevel;
  clamped = 0;
  private baseline: number[] = [];
  private alerts: Array<{ at: number; scorePmil: number }> = [];
  guards = new GuardRegistry();
  queue = new BatchQueue(0);
  suggestionActive = false;
  eggOn = false;
  pressure = false;

  constructor(profileId: string = 'standard') {
    this.profileId = (ANOMALY_LEVELS as readonly string[]).includes(profileId)
      ? (profileId as AnomalyLevel)
      : (() => {
          this.clamped = 1;
          return 'standard' as AnomalyLevel;
        })();
  }

  get thresholdPmil(): number {
    return ANOMALY_THRESHOLD_PMIL[this.profileId];
  }

  learnBaseline(values: number[]): void {
    this.baseline = [...values];
  }
  get baselineReady(): boolean {
    return this.baseline.length >= 3;
  }
  /** 均值偏差（permille）。 */
  deviationPmil(value: number): number {
    if (!this.baselineReady) return 0;
    const mean = this.baseline.reduce((a, b) => a + b, 0) / this.baseline.length;
    if (mean === 0) return value === 0 ? 0 : 1000;
    return Math.min(1000, Math.round((Math.abs(value - mean) / Math.abs(mean)) * 1000));
  }
  /** 观测：超阈值即告警。 */
  observe(value: number, at: number): boolean {
    const d = this.deviationPmil(value);
    const hit = d >= this.thresholdPmil;
    if (hit) this.alerts.push({ at, scorePmil: d });
    return hit;
  }
  get alertCount(): number {
    return this.alerts.length;
  }
  get alertLog(): ReadonlyArray<{ at: number; scorePmil: number }> {
    return this.alerts;
  }
  /** 静默误报：把某模式加入白名单。 */
  private whitelist = new Set<number>();
  whitelistScore(scorePmil: number): boolean {
    if (this.whitelist.has(scorePmil)) return false;
    this.whitelist.add(scorePmil);
    return true;
  }
  isWhitelisted(scorePmil: number): boolean {
    return this.whitelist.has(scorePmil);
  }
  degrade(): AnomalyLevel {
    const i = (ANOMALY_LEVELS as readonly string[]).indexOf(this.profileId);
    this.profileId = ANOMALY_LEVELS[Math.max(0, i - 1)]!;
    return this.profileId;
  }
  reset(): void {
    this.baseline = [];
    this.alerts = [];
    this.whitelist.clear();
    this.queue = new BatchQueue(0);
    this.suggestionActive = false;
    this.eggOn = false;
  }
}
