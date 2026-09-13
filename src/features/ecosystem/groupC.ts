// AURORA-10000: AI-53 批次（领域11 开放生态 · 族0261~0265 · F06501~F06625），勿删。
// 反馈与成长 / 互操作联盟 / 教育合作 / 无障碍开放 / 生态健康。

/* ===================== 族0261 反馈与成长 ===================== */

export type FeedbackState = 'received' | 'triaged' | 'in-progress' | 'fixed' | 'wontfix';

export interface FeedbackItem {
  id: string;
  category: 'bug' | 'feature' | 'experience';
  title: string;
  body: string;
  fingerprint: string;
  state: FeedbackState;
  votes: number;
  anonymous: boolean;
  screenshot: boolean;
  logsAttached: boolean;
  createdAt: number;
}

export function feedbackFingerprint(title: string, stack: string): string {
  let h = 0x811c9dc5;
  const s = `${title}|${stack}`.toLowerCase().replace(/\s+/g, ' ');
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return `fp-${h.toString(16)}`;
}

/** 反馈去重：同指纹合并并叠加联署。 */
export class FeedbackBoard {
  private items = new Map<string, FeedbackItem>();
  private seq = 0;

  submit(f: Omit<FeedbackItem, 'id' | 'state' | 'votes' | 'fingerprint'> & { stack?: string }): { item: FeedbackItem; deduped: boolean } {
    const fp = feedbackFingerprint(f.title, f.stack ?? f.body);
    const existing = [...this.items.values()].find((i) => i.fingerprint === fp);
    if (existing) {
      existing.votes += 1;
      return { item: existing, deduped: true };
    }
    this.seq += 1;
    const item: FeedbackItem = { ...f, id: `fb-${String(this.seq).padStart(4, '0')}`, state: 'received', votes: 1, fingerprint: fp };
    this.items.set(item.id, item);
    return { item, deduped: false };
  }

  transition(id: string, to: FeedbackState): boolean {
    const i = this.items.get(id);
    if (!i) return false;
    const legal: Record<FeedbackState, FeedbackState[]> = {
      received: ['triaged', 'wontfix'],
      triaged: ['in-progress', 'wontfix'],
      'in-progress': ['fixed', 'wontfix'],
      fixed: [],
      wontfix: [],
    };
    if (!legal[i.state].includes(to)) return false;
    i.state = to;
    return true;
  }

  get(id: string): FeedbackItem | undefined {
    return this.items.get(id);
  }

  publicWall(): FeedbackItem[] {
    return [...this.items.values()].sort((a, b) => b.votes - a.votes);
  }

  size(): number {
    return this.items.size;
  }
}

export const FEEDBACK_SLA_HOURS: Record<FeedbackItem['category'], number> = { bug: 72, feature: 336, experience: 168 };

export function slaMet(item: FeedbackItem, respondedAt: number): boolean {
  return respondedAt - item.createdAt <= FEEDBACK_SLA_HOURS[item.category] * 3600_000;
}

/** NPS：推荐者% - 贬损者%。 */
export function nps(scores: number[]): number {
  if (scores.length === 0) return 0;
  const promoters = scores.filter((s) => s >= 9).length;
  const detractors = scores.filter((s) => s <= 6).length;
  return Math.round(((promoters - detractors) / scores.length) * 100);
}

export function versionSatisfaction(scores: number[]): number {
  if (scores.length === 0) return 0;
  return Math.round((scores.reduce((s, x) => s + x, 0) / scores.length) * 10) / 10;
}

/* ===================== 族0262 互操作联盟 ===================== */

export const OPEN_FORMATS = ['ODF', 'OGG', 'WebM', 'PDF', 'Markdown', 'iCal', 'vCard'] as const;

export const ALLIANCE_PROTOCOLS = ['WebDAV', 'CalDAV', 'CardDAV', 'RSS', 'SMB', 'mDNS'] as const;

export interface BridgeStatus {
  name: 'WSL' | 'ADB' | 'iOS';
  status: 'available' | 'experimental' | 'reserved';
}

export const ALLIANCE_BRIDGES: BridgeStatus[] = [
  { name: 'WSL', status: 'available' },
  { name: 'ADB', status: 'experimental' },
  { name: 'iOS', status: 'reserved' },
];

/** 数据可携：导出全部用户数据清单。 */
export const EXPORT_MANIFEST = ['settings.json', 'bookmarks.json', 'notes/*.md', 'todos.json', 'calendar.ics', 'wallpapers/', 'plugin-data/'] as const;

export function exportAll(baseDir: string): Array<{ path: string; kind: string }> {
  return EXPORT_MANIFEST.map((p) => ({ path: `${baseDir}/${p}`, kind: p.includes('/') && p.endsWith('/') ? 'dir' : 'file' }));
}

export interface InteropTestCase {
  protocol: string;
  peer: string;
  pass: boolean;
}

export function interopMatrix(tests: InteropTestCase[]): { coverage: number; failing: string[] } {
  const failing = tests.filter((t) => !t.pass).map((t) => `${t.protocol}/${t.peer}`);
  const protocols = new Set(tests.map((t) => t.protocol));
  const okProtocols = new Set(tests.filter((t) => t.pass).map((t) => t.protocol));
  return { coverage: protocols.size === 0 ? 0 : Math.round((okProtocols.size / protocols.size) * 100), failing };
}

/* ===================== 族0263 教育合作 ===================== */

export interface ClassroomConfig {
  edition: 'education';
  teacherId: string;
  students: string[];
  focusMode: boolean;
}

export function createClassroom(teacherId: string, students: string[]): ClassroomConfig {
  return { edition: 'education', teacherId, students, focusMode: false };
}

export function toggleFocus(c: ClassroomConfig, on: boolean): ClassroomConfig {
  return { ...c, focusMode: on };
}

export function presentStudent(c: ClassroomConfig, studentId: string): boolean {
  return c.students.includes(studentId);
}

export const EDU_FREE_PROMISE = '教育版核心功能对认证教育机构永久免费';

export interface LabExercise {
  id: string;
  title: string;
  module: string;
  difficulty: 1 | 2 | 3;
  image: 'qemu-varix-lab';
}

export const KERNEL_COURSE_LABS: LabExercise[] = [
  { id: 'lab-01', title: 'Hello 内核：串口输出', module: 'boot', difficulty: 1, image: 'qemu-varix-lab' },
  { id: 'lab-02', title: 'GDT/IDT 与中断', module: 'arch', difficulty: 2, image: 'qemu-varix-lab' },
  { id: 'lab-03', title: '页表与内存布局', module: 'mm', difficulty: 2, image: 'qemu-varix-lab' },
  { id: 'lab-04', title: '调度器实验', module: 'sched', difficulty: 3, image: 'qemu-varix-lab' },
  { id: 'lab-05', title: 'VFS 实验包', module: 'vfs', difficulty: 3, image: 'qemu-varix-lab' },
  { id: 'lab-06', title: '调试实验：QEMU + 串口日志定位', module: 'debug', difficulty: 2, image: 'qemu-varix-lab' },
];

export function childSafeConfig(): Record<string, boolean> {
  return { telemetry: false, externalLinks: false, purchases: false, chat: false, camera: false };
}

/* ===================== 族0264 无障碍开放 ===================== */

export interface A11yApi {
  name: string;
  consumers: string;
  stable: boolean;
}

export const OPEN_A11Y_APIS: A11yApi[] = [
  { name: 'screen-reader.bridge', consumers: '第三方读屏', stable: true },
  { name: 'switch-control.map', consumers: '开关扫描', stable: true },
  { name: 'eye-tracking.axis', consumers: '眼动设备', stable: false },
  { name: 'captions.render', consumers: '听障字幕', stable: true },
  { name: 'magnifier.lens', consumers: '视障放大', stable: true },
];

export const A11Y_FIX_SLA_DAYS = { P0: 14, P1: 30, P2: 90 } as const;

export interface A11yDefect {
  id: string;
  severity: keyof typeof A11Y_FIX_SLA_DAYS;
  openDays: number;
  fixed: boolean;
}

export function a11ySlaBreaches(defects: A11yDefect[]): A11yDefect[] {
  return defects.filter((d) => !d.fixed && d.openDays > A11Y_FIX_SLA_DAYS[d.severity]);
}

export interface A11yOneClickProfile {
  name: string;
  settings: Record<string, boolean | number>;
}

export const A11Y_ONE_CLICK_PROFILES: A11yOneClickProfile[] = [
  { name: '低视力', settings: { magnifier: 2, contrast: 'high', cursorSize: 3 } },
  { name: '听障', settings: { captions: true, visualAlerts: true } },
  { name: '运动', settings: { stickyKeys: true, dwellClick: true, scanMode: true } },
  { name: '认知', settings: { simplifyUi: true, readAloud: true, reduceMotion: true } },
];

export function serializeA11yProfile(p: A11yOneClickProfile): string {
  return JSON.stringify({ v: 1, name: p.name, settings: p.settings });
}

export function parseA11yProfile(json: string): A11yOneClickProfile | { error: string } {
  try {
    const o = JSON.parse(json) as { v?: number; name?: string; settings?: Record<string, boolean | number> };
    if (o.v !== 1 || !o.name || typeof o.settings !== 'object') return { error: 'bad-profile' };
    return { name: o.name, settings: o.settings };
  } catch {
    return { error: 'bad-json' };
  }
}

export const A11Y_BADGE_LEVELS = ['A', 'AA', 'AAA'] as const;

export function grantA11yBadge(violationsP0: number, violationsAA: number): 'none' | 'A' | 'AA' {
  if (violationsP0 > 0) return 'none';
  return violationsAA === 0 ? 'AA' : 'A';
}

/* ===================== 族0265 生态健康 ===================== */

export interface EcoApp {
  id: string;
  name: string;
  category: string;
  lastUpdatedDays: number;
  installs: number;
  rating: number;
  counterfeitOf?: string;
  suspiciousReviews?: number;
}

/** 停更检测：>365 天视为死亡应用。 */
export function staleApps(apps: EcoApp[], limitDays = 365): EcoApp[] {
  return apps.filter((a) => a.lastUpdatedDays > limitDays);
}

export function suggestAlternatives(apps: EcoApp[], dead: EcoApp[]): Array<{ dead: string; alt: string }> {
  return dead
    .map((d) => {
      const alt = apps
        .filter((a) => a.category === d.category && a.id !== d.id && a.lastUpdatedDays <= 365)
        .sort((x, y) => y.installs - x.installs)[0];
      return alt ? { dead: d.id, alt: alt.id } : null;
    })
    .filter((x): x is { dead: string; alt: string } => x !== null);
}

/** 仿冒检测：名称高度相似 + 同类目。 */
export function detectCounterfeits(apps: EcoApp[]): EcoApp[] {
  return apps.filter((a) => {
    if (!a.counterfeitOf) return false;
    const origin = apps.find((x) => x.id === a.counterfeitOf);
    return !!origin && a.name.toLowerCase().replace(/[\s-]/g, '') === origin.name.toLowerCase().replace(/[\s-]/g, '') && a.id !== origin.id;
  });
}

/** 刷评识别：短窗口评论激增占比。 */
export function detectReviewFraud(app: EcoApp, totalReviews: number, suspicious: number, threshold = 0.3): boolean {
  if (totalReviews === 0) return false;
  return suspicious / totalReviews > threshold && app.suspiciousReviews !== undefined;
}

export function developerCreditScore(base: number, events: Array<{ kind: 'violation' | 'good'; weight: number }>): number {
  let s = base;
  for (const e of events) s += e.kind === 'good' ? e.weight : -e.weight;
  return Math.max(0, Math.min(100, s));
}

export interface EcoHealthModel {
  apps: number;
  activeApps: number;
  avgRating: number;
  takedownQuarter: number;
}

export function ecoHealthScore(m: EcoHealthModel): number {
  const freshness = m.apps === 0 ? 0 : m.activeApps / m.apps;
  const quality = m.avgRating / 5;
  const hygiene = 1 / (1 + m.takedownQuarter * 0.1);
  return Math.round((freshness * 0.4 + quality * 0.4 + hygiene * 0.2) * 100);
}

export function categoryDiversity(apps: EcoApp[]): { categories: number; hhi: number } {
  const counts = new Map<string, number>();
  for (const a of apps) counts.set(a.category, (counts.get(a.category) ?? 0) + 1);
  const total = apps.length;
  let hhi = 0;
  for (const c of counts.values()) {
    const share = c / total;
    hhi += share * share;
  }
  return { categories: counts.size, hhi: Math.round(hhi * 1000) / 1000 };
}

export function gapReport(apps: EcoApp[], expectedCategories: string[]): string[] {
  const have = new Set(apps.map((a) => a.category));
  return expectedCategories.filter((c) => !have.has(c));
}

export function mostWanted(wishes: Array<{ appId: string; votes: number }>, n = 5): Array<{ appId: string; votes: number }> {
  return [...wishes].sort((a, b) => b.votes - a.votes).slice(0, n);
}
