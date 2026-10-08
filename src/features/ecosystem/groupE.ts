// AURORA-10000: AI-55 批次（领域11 开放生态 · 族0271~0275 · F06751~F06875），勿删。
// 自托管 / 可持续 / 质量开放 / 生态精选 / 生态收官。

/* ===================== 族0271 自托管 ===================== */

export interface SelfHostEndpoint {
  id: string;
  service: 'sync' | 'clipboard' | 'caldav' | 'todo' | 'vault' | 'notes' | 'photos' | 'music' | 'backup';
  url: string;
  tls: boolean;
  e2eEncrypted: boolean;
  mdnsDiscovered: boolean;
}

export const SELFHOST_SERVICES: SelfHostEndpoint['service'][] = [
  'sync', 'clipboard', 'caldav', 'todo', 'vault', 'notes', 'photos', 'music', 'backup',
];

export function addEndpoint(list: SelfHostEndpoint[], e: SelfHostEndpoint): SelfHostEndpoint[] {
  if (list.some((x) => x.service === e.service && x.url === e.url)) return list; // 去重
  return [...list, e];
}

export function endpointReachable(e: SelfHostEndpoint): boolean {
  return /^https?:\/\/.+/.test(e.url);
}

export function healthCheck(list: SelfHostEndpoint[]): Array<{ id: string; ok: boolean; reasons: string[] }> {
  return list.map((e) => {
    const reasons: string[] = [];
    if (!endpointReachable(e)) reasons.push('url-invalid');
    if (!e.tls) reasons.push('no-tls');
    return { id: e.id, ok: reasons.length === 0, reasons };
  });
}

export interface SelfHostWizardStep {
  step: number;
  prompt: string;
  done: boolean;
}

export function wizardSteps(): SelfHostWizardStep[] {
  return [
    { step: 1, prompt: '选择服务类型', done: false },
    { step: 2, prompt: '输入服务地址（支持 mDNS 发现）', done: false },
    { step: 3, prompt: 'TLS 证书校验', done: false },
    { step: 4, prompt: '账号认证', done: false },
    { step: 5, prompt: 'E2E 密钥协商', done: false },
    { step: 6, prompt: '首同步与验证', done: false },
  ];
}

export function completeWizard(steps: SelfHostWizardStep[]): SelfHostWizardStep[] {
  return steps.map((s) => ({ ...s, done: true }));
}

/** 同步冲突：三路合并 —— 本地/远端/基线。 */
export type SyncConflictResolution = 'local-wins' | 'remote-wins' | 'newest-wins' | 'duplicate';

export function resolveSyncConflict(
  local: { mtime: number; content: string },
  remote: { mtime: number; content: string },
  base: { content: string } | null,
  strategy: SyncConflictResolution,
): { content: string; conflict: boolean } {
  if (base && local.content === base.content && remote.content !== base.content) return { content: remote.content, conflict: false };
  if (base && remote.content === base.content && local.content !== base.content) return { content: local.content, conflict: false };
  if (local.content === remote.content) return { content: local.content, conflict: false };
  if (strategy === 'local-wins') return { content: local.content, conflict: true };
  if (strategy === 'remote-wins') return { content: remote.content, conflict: true };
  if (strategy === 'newest-wins') return { content: local.mtime >= remote.mtime ? local.content : remote.content, conflict: true };
  return { content: local.content, conflict: true }; // duplicate：保留副本由用户选择
}

/** 离线降级：断网时本地队列暂存，恢复后重放。 */
export class OfflineQueue {
  private queue: Array<{ op: string; payload: string }> = [];

  enqueue(op: string, payload: string): void {
    this.queue.push({ op, payload });
  }

  drain(): Array<{ op: string; payload: string }> {
    const out = [...this.queue];
    this.queue = [];
    return out;
  }

  pending(): number {
    return this.queue.length;
  }
}

/** E2E：派生密钥仅存本地（演示级指纹）。 */
export function e2eFingerprint(secret: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < secret.length; i++) {
    h ^= secret.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return `e2e-${h.toString(16).padStart(8, '0')}`;
}

export function mixedModePlan(local: string[], selfHosted: string[]): { localOnly: string[]; synced: string[] } {
  return { localOnly: local.filter((x) => !selfHosted.includes(x)), synced: selfHosted };
}

/* ===================== 族0272 可持续 ===================== */

export const FREE_FOREVER_CORE = [
  '桌面与窗口', '文件管理器', '设置中心', '通知', '输入法', '剪贴板', '截图', '本地搜索', '全部内核能力',
] as const;

export const PAID_BOUNDARY = '核心功能永远免费；不设订阅、不设内购、无广告';

export interface Sponsor {
  name: string;
  tier: 'backer' | 'supporter' | 'patron';
  monthlyCny: number;
  badge: boolean;
}

export function sponsorWall(sponsors: Sponsor[]): Array<{ name: string; tier: Sponsor['tier'] }> {
  return sponsors
    .filter((s) => s.monthlyCny > 0)
    .sort((a, b) => b.monthlyCny - a.monthlyCny)
    .map((s) => ({ name: s.name, tier: s.tier }));
}

export function sponsorBadge(s: Sponsor): boolean {
  return s.tier === 'patron' || s.monthlyCny >= 50;
}

export interface SustainabilityLedger {
  month: string;
  donationsCny: number;
  infraCny: number;
  bountyCny: number;
}

/** 用途公示：捐赠去向 = 基础设施 + 悬赏，结余滚动。 */
export function ledgerSummary(ledger: SustainabilityLedger[]): { totalIn: number; totalOut: number; reserve: number; rows: Array<{ month: string; out: number }> } {
  const totalIn = ledger.reduce((s, l) => s + l.donationsCny, 0);
  const totalOut = ledger.reduce((s, l) => s + l.infraCny + l.bountyCny, 0);
  return {
    totalIn,
    totalOut,
    reserve: totalIn - totalOut,
    rows: ledger.map((l) => ({ month: l.month, out: l.infraCny + l.bountyCny })),
  };
}

export const ECOSYSTEM_COMMISSION_PCT = 0; // 生态 0% 分成声明

export const SUSTAINABILITY_PLANS = [
  '个人捐赠（一次性/月度）', '企业支持服务', '定制开发', '培训服务', '周边商店（开源设计）',
] as const;

export interface LicenseGrant {
  orgType: 'personal' | 'education' | 'enterprise' | 'government';
  feeCny: number;
  seats: number | 'unlimited';
}

export function licenseGrant(org: LicenseGrant['orgType']): LicenseGrant {
  switch (org) {
    case 'personal':
      return { orgType: org, feeCny: 0, seats: 'unlimited' };
    case 'education':
      return { orgType: org, feeCny: 0, seats: 'unlimited' };
    case 'enterprise':
      return { orgType: org, feeCny: 0, seats: 'unlimited' }; // 桌面免费；支持服务另计
    case 'government':
      return { orgType: org, feeCny: 0, seats: 'unlimited' };
  }
}

/* ===================== 族0273 质量开放 ===================== */

export interface QualityReport {
  version: string;
  testsPassed: number;
  testsTotal: number;
  crashRatePpm: number;
  fixRatePct: number;
  coveragePct: number;
  fpsP05: number;
  bootMs: number;
  rssMb: number;
  a11yAAOpen: number;
  i18nMissingKeys: number;
  depAuditCritical: number;
}

export function qualityGate(q: QualityReport): { pass: boolean; violations: string[] } {
  const violations: string[] = [];
  if (q.testsPassed !== q.testsTotal) violations.push('tests-failing');
  if (q.crashRatePpm > 50) violations.push('crash-rate');
  if (q.fixRatePct < 80) violations.push('fix-rate');
  if (q.coveragePct < 70) violations.push('coverage');
  if (q.fpsP05 < 55) violations.push('fps');
  if (q.bootMs > 3000) violations.push('boot');
  if (q.rssMb > 500) violations.push('memory');
  if (q.a11yAAOpen > 0) violations.push('a11y-aa');
  if (q.i18nMissingKeys > 0) violations.push('i18n');
  if (q.depAuditCritical > 0) violations.push('dep-audit');
  return { pass: violations.length === 0, violations };
}

export function checksumOf(artifact: string, content: string): string {
  const s = `${artifact}:${content}`;
  let h1 = 0x811c9dc5;
  let h2 = 0x01000193;
  for (let i = 0; i < s.length; i++) {
    h1 = Math.imul(h1 ^ s.charCodeAt(i), 0x01000193) >>> 0;
    h2 = Math.imul(h2 + i * s.charCodeAt(i), 0x27d4eb2f) >>> 0;
  }
  return (h1.toString(16).padStart(8, '0') + h2.toString(16).padStart(8, '0')).repeat(2);
}

export interface ReleaseArtifact {
  name: string;
  kind: 'installer' | 'portable' | 'iso' | 'symbols' | 'sdk';
  sha256: string;
}

export const QUALITY_SLA = { P0FixDays: 7, P1FixDays: 30, regressionTriageDays: 3 } as const;

export function qualityWeeklyReport(cur: QualityReport, prev: QualityReport): Array<{ metric: string; delta: string }> {
  const pct = (a: number, b: number) => (b === 0 ? 'n/a' : `${a >= b ? '+' : ''}${Math.round(((a - b) / b) * 100)}%`);
  return [
    { metric: 'crashRatePpm', delta: pct(cur.crashRatePpm, prev.crashRatePpm) },
    { metric: 'coveragePct', delta: `${cur.coveragePct - prev.coveragePct}pt` },
    { metric: 'bootMs', delta: pct(cur.bootMs, prev.bootMs) },
    { metric: 'rssMb', delta: pct(cur.rssMb, prev.rssMb) },
  ];
}

/* ===================== 族0274 生态精选 ===================== */

export interface CuratedCollection {
  id: string;
  audience: 'productivity' | 'developer' | 'creator' | 'learning' | 'fun' | 'wellness' | 'a11y' | 'kids' | 'parents' | 'minimal';
  appIds: string[];
  month: string;
  editor: string;
}

export const CURATED_AUDIENCES: CuratedCollection['audience'][] = [
  'productivity', 'developer', 'creator', 'learning', 'fun', 'wellness', 'a11y', 'kids', 'parents', 'minimal',
];

export interface CurationCriteria {
  qualityScore: number; // 0~100
  maintenanceDays: number;
  a11yBadge: 'none' | 'A' | 'AA';
  privacyLabel: 'no-data' | 'local-only' | 'network';
  i18nComplete: boolean;
}

/** 入选标准：质量/维护/a11y/隐私/多语五门。 */
export function curationScore(c: CurationCriteria): number {
  let s = c.qualityScore * 0.5;
  s += c.maintenanceDays <= 90 ? 20 : c.maintenanceDays <= 180 ? 10 : 0;
  s += c.a11yBadge === 'AA' ? 15 : c.a11yBadge === 'A' ? 8 : 0;
  s += c.privacyLabel === 'no-data' ? 10 : c.privacyLabel === 'local-only' ? 8 : 0;
  s += c.i18nComplete ? 5 : 0;
  return Math.round(s);
}

export function curationPass(c: CurationCriteria, threshold = 70): boolean {
  return curationScore(c) >= threshold;
}

export class CuratedBoard {
  private collections = new Map<string, CuratedCollection>();

  upsert(c: CuratedCollection): boolean {
    this.collections.set(c.id, c);
    return true;
  }

  /** 退选：从未入选集合移除并记录。 */
  remove(collectionId: string, appId: string): boolean {
    const c = this.collections.get(collectionId);
    if (!c) return false;
    const before = c.appIds.length;
    c.appIds = c.appIds.filter((a) => a !== appId);
    return c.appIds.length < before;
  }

  monthlyTopic(month: string, audience: CuratedCollection['audience']): CuratedCollection | undefined {
    return [...this.collections.values()].find((c) => c.month === month && c.audience === audience);
  }

  list(): CuratedCollection[] {
    return [...this.collections.values()];
  }
}

export function communityVoteCollections(candidates: string[], votes: Map<string, number>, n = 3): string[] {
  return [...candidates].sort((a, b) => (votes.get(b) ?? 0) - (votes.get(a) ?? 0)).slice(0, n);
}

/* ===================== 族0275 生态收官 ===================== */

export const ECOSYSTEM_MILESTONES = [100, 500, 1000] as const;

export function milestoneReached(appCount: number): Array<(typeof ECOSYSTEM_MILESTONES)[number]> {
  return ECOSYSTEM_MILESTONES.filter((m) => appCount >= m);
}

export interface EcoYearbook {
  year: number;
  appsPublished: number;
  developersJoined: number;
  topCategory: string;
  stories: string[];
}

export function yearbookDigest(y: EcoYearbook): string {
  return `${y.year}：上架 ${y.appsPublished} 个应用，新增 ${y.developersJoined} 名开发者，最热类目「${y.topCategory}」，收录故事 ${y.stories.length} 篇`;
}

export interface EcoRiskItem {
  risk: string;
  likelihood: 1 | 2 | 3;
  impact: 1 | 2 | 3;
  mitigation: string;
}

export function ecoRiskRegister(risks: EcoRiskItem[]): EcoRiskItem[] {
  return [...risks].sort((a, b) => b.likelihood * b.impact - a.likelihood * a.impact);
}

export interface EcoHandoffDoc {
  section: string;
  owner: string;
  complete: boolean;
}

export function handoffComplete(docs: EcoHandoffDoc[]): boolean {
  return docs.length > 0 && docs.every((d) => d.complete);
}

export interface EcoAuditFinding {
  area: string;
  finding: string;
  severity: 'info' | 'warn' | 'critical';
}

export function ecoAudit(findings: EcoAuditFinding[]): { pass: boolean; critical: number; warns: number } {
  const critical = findings.filter((f) => f.severity === 'critical').length;
  const warns = findings.filter((f) => f.severity === 'warn').length;
  return { pass: critical === 0, critical, warns };
}

export interface EcoTimelineEvent {
  date: string;
  event: string;
  era: 'aurora-1000' | 'varix-500' | 'nova-500' | 'aurora-10000';
}

export const ECOSYSTEM_TIMELINE: EcoTimelineEvent[] = [
  { date: 'T0', event: 'AURORA-1000 规划线启动', era: 'aurora-1000' },
  { date: 'T1', event: 'VARIX-500 内核纪元', era: 'varix-500' },
  { date: 'T2', event: 'NOVA-500 收官', era: 'nova-500' },
  { date: 'T3', event: 'AURORA-10000 开放生态 W5 落地', era: 'aurora-10000' },
];

export const FINALE_FAQ: Array<{ q: string; a: string }> = [
  { q: '生态会收费吗？', a: '核心永久免费，生态 0% 分成，无广告无订阅。' },
  { q: '自托管被支持吗？', a: '同步/日历/待办/密码库等九类服务均提供自托管接入。' },
  { q: '数据可以全部导出吗？', a: '可以，全部数据可携，格式公开。' },
  { q: '如何参与治理？', a: '通过 RFC 提案、路线图投票与工作组例会参与。' },
];

export function finalThanks(contributors: string[], partners: string[]): string {
  return `致谢 ${contributors.length} 位贡献者与 ${partners.length} 家伙伴 —— 生态属于每一位建设者。`;
}
