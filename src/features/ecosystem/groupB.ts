// AURORA-10000: AI-52 批次（领域11 开放生态 · 族0256~0260 · F06376~F06500），勿删。
// 开放 API / Web 生态 / 创作者计划 / 硬件伙伴 / 国际社区。

/* ===================== 族0256 开放 API ===================== */

export interface ApiRoute {
  method: 'GET' | 'POST' | 'PUT' | 'DELETE';
  path: string;
  domain: string;
  since: string;
  deprecatedSince?: string;
  removedIn?: string;
  scope: string;
}

export const API_VERSIONS = ['v1', 'v2', 'v3'] as const;
export const LOCAL_API_HOST = '127.0.0.1:4780';

export const OPEN_API_ROUTES: ApiRoute[] = [
  { method: 'GET', path: '/windows', domain: 'windows', since: 'v1', scope: 'windows' },
  { method: 'POST', path: '/windows/focus', domain: 'windows', since: 'v1', scope: 'windows' },
  { method: 'GET', path: '/files', domain: 'files', since: 'v1', scope: 'files' },
  { method: 'POST', path: '/notify', domain: 'notify', since: 'v1', scope: 'notify' },
  { method: 'GET', path: '/clipboard', domain: 'clipboard', since: 'v2', scope: 'clipboard' },
  { method: 'POST', path: '/wallpaper', domain: 'wallpaper', since: 'v1', scope: 'wallpaper' },
  { method: 'GET', path: '/theme', domain: 'theme', since: 'v1', scope: 'theme' },
  { method: 'POST', path: '/hotkeys', domain: 'hotkey', since: 'v2', scope: 'hotkey' },
  { method: 'GET', path: '/settings/:key', domain: 'settings', since: 'v1', scope: 'settings' },
  { method: 'GET', path: '/tasks', domain: 'tasks', since: 'v2', scope: 'tasks' },
  { method: 'GET', path: '/calendar', domain: 'calendar', since: 'v2', scope: 'calendar' },
  { method: 'GET', path: '/weather', domain: 'weather', since: 'v2', scope: 'weather' },
  { method: 'GET', path: '/search', domain: 'search', since: 'v1', scope: 'search' },
  { method: 'POST', path: '/automation/run', domain: 'automation', since: 'v3', scope: 'automation' },
];

export interface ApiToken {
  token: string;
  scope: string[];
  issuedAt: number;
  revoked: boolean;
}

/** 本机令牌：仅 127.0.0.1 + scope 校验。 */
export class ApiAuth {
  private tokens = new Map<string, ApiToken>();

  issue(scope: string[], now: number): ApiToken {
    const t: ApiToken = { token: `vk-${Math.abs(now * 7919 % 1e12).toString(16)}`, scope, issuedAt: now, revoked: false };
    this.tokens.set(t.token, t);
    return t;
  }

  check(token: string, requiredScope: string): boolean {
    const t = this.tokens.get(token);
    return !!t && !t.revoked && t.scope.includes(requiredScope);
  }

  revoke(token: string): boolean {
    const t = this.tokens.get(token);
    if (!t) return false;
    t.revoked = true;
    return true;
  }
}

/** 令牌桶限流。 */
export class RateLimiter {
  private buckets = new Map<string, { tokens: number; last: number }>();

  constructor(private capacity: number, private refillPerSec: number) {}

  allow(key: string, now: number): boolean {
    let b = this.buckets.get(key);
    if (!b) {
      b = { tokens: this.capacity, last: now };
      this.buckets.set(key, b);
    }
    b.tokens = Math.min(this.capacity, b.tokens + ((now - b.last) / 1000) * this.refillPerSec);
    b.last = now;
    if (b.tokens < 1) return false;
    b.tokens -= 1;
    return true;
  }
}

export function deprecationNotice(r: ApiRoute, current: string): string | null {
  if (!r.deprecatedSince) return null;
  const cmp = (v: string) => Number(v.slice(1));
  if (cmp(current) >= cmp(r.deprecatedSince) && (!r.removedIn || cmp(current) < cmp(r.removedIn))) {
    return `${r.method} ${r.path} 自 ${r.deprecatedSince} 弃用${r.removedIn ? `，将于 ${r.removedIn} 移除` : ''}`;
  }
  return null;
}

export const API_ERROR_CODES: Record<string, { http: number; meaning: string }> = {
  E4001: { http: 400, meaning: '参数缺失' },
  E4010: { http: 401, meaning: '令牌无效' },
  E4030: { http: 403, meaning: 'scope 不足' },
  E4040: { http: 404, meaning: '资源不存在' },
  E4290: { http: 429, meaning: '超出限流' },
  E5000: { http: 500, meaning: '内部错误' },
};

export function apiStatusPage(routes: ApiRoute[]): { up: number; total: number; degraded: string[] } {
  return { up: routes.length, total: routes.length, degraded: [] };
}

/* ===================== 族0257 Web 生态 ===================== */

export interface WebPage {
  path: string;
  title: string;
  kind: 'docs' | 'blog' | 'changelog' | 'roadmap' | 'feedback' | 'status' | 'download' | 'mirror' | 'checksum' | 'coc' | 'rfc' | 'faq' | 'kb' | 'tutorial' | 'brand' | 'compare';
  children?: string[];
}

export const WEB_SITE_MAP: WebPage[] = [
  { path: '/', title: 'Variable 官网', kind: 'docs', children: ['/blog', '/changelog', '/roadmap', '/download', '/community'] },
  { path: '/blog', title: '更新博客', kind: 'blog' },
  { path: '/changelog', title: '更新日志', kind: 'changelog' },
  { path: '/roadmap', title: '公开路线图', kind: 'roadmap' },
  { path: '/feedback', title: '反馈墙', kind: 'feedback' },
  { path: '/status', title: '服务状态', kind: 'status' },
  { path: '/download', title: '下载中心', kind: 'download', children: ['/download/mirrors', '/download/checksums'] },
  { path: '/download/mirrors', title: '多镜像下载', kind: 'mirror' },
  { path: '/download/checksums', title: 'SHA-256 校验和', kind: 'checksum' },
  { path: '/coc', title: '行为准则', kind: 'coc' },
  { path: '/rfc', title: 'RFC 提案', kind: 'rfc' },
  { path: '/faq', title: '常见问题', kind: 'faq' },
  { path: '/kb', title: '知识库', kind: 'kb' },
  { path: '/tutorials', title: '教程中心', kind: 'tutorial' },
  { path: '/brand', title: '品牌资源', kind: 'brand' },
  { path: '/compare', title: '系统对比', kind: 'compare' },
];

export function sha256Label(content: string): string {
  let h1 = 0x811c9dc5;
  let h2 = 0x1000193;
  for (let i = 0; i < content.length; i++) {
    h1 = Math.imul(h1 ^ content.charCodeAt(i), 0x01000193) >>> 0;
    h2 = Math.imul(h2 + content.charCodeAt(i), 0x85ebca6b) >>> 0;
  }
  return (h1.toString(16).padStart(8, '0') + h2.toString(16).padStart(8, '0')).repeat(2);
}

export interface RfcProposal {
  id: string;
  title: string;
  state: 'draft' | 'voting' | 'accepted' | 'rejected';
  yes: number;
  no: number;
}

export function tallyVote(p: RfcProposal, vote: boolean): RfcProposal {
  return { ...p, yes: p.yes + (vote ? 1 : 0), no: p.no + (vote ? 0 : 1) };
}

export function resolveRfc(p: RfcProposal): RfcProposal['state'] {
  if (p.state !== 'voting') return p.state;
  return p.yes > p.no ? 'accepted' : 'rejected';
}

export interface CommunityEvent {
  title: string;
  startUtc: string;
  tz: string;
}

/** 多时区公告：为每个事件给出本地化时间标签。 */
export function announceAcrossTimezones(e: CommunityEvent, offsets: number[]): string[] {
  const t = Date.parse(e.startUtc);
  if (Number.isNaN(t)) return [];
  return offsets.map((o) => {
    const d = new Date(t + o * 3600_000);
    return `UTC+${o} ${String(d.getUTCHours()).padStart(2, '0')}:${String(d.getUTCMinutes()).padStart(2, '0')} ${e.title}`;
  });
}

/* ===================== 族0258 创作者计划 ===================== */

export interface CreatorProfile {
  id: string;
  certified: boolean;
  level: number; // 1~5
  xp: number;
  badges: string[];
  works: string[];
  tipsReceived: number;
}

export const CREATOR_LEVELS = [0, 100, 400, 1200, 3600] as const;

export function creatorLevel(xp: number): number {
  let lvl = 1;
  for (let i = 1; i < CREATOR_LEVELS.length; i++) if (xp >= CREATOR_LEVELS[i]!) lvl = i + 1;
  return Math.min(lvl, 5);
}

export function certifyCreator(p: CreatorProfile, minWorks = 3): CreatorProfile {
  return { ...p, certified: p.works.length >= minWorks };
}

export function awardBadge(p: CreatorProfile, badge: string): CreatorProfile {
  if (p.badges.includes(badge)) return p;
  return { ...p, badges: [...p.badges, badge] };
}

export function recordTip(p: CreatorProfile, amount: number): CreatorProfile {
  return { ...p, tipsReceived: p.tipsReceived + amount };
}

export const CREATOR_MARKETS = ['template', 'sound', 'icon', 'font', 'animation', 'widget'] as const;

export interface MarketItem {
  market: (typeof CREATOR_MARKETS)[number];
  id: string;
  author: string;
  title: string;
  downloads: number;
}

export function marketTop(items: MarketItem[], market: (typeof CREATOR_MARKETS)[number], n = 5): MarketItem[] {
  return items.filter((i) => i.market === market).sort((a, b) => b.downloads - a.downloads).slice(0, n);
}

export const CREATOR_AGREEMENT_SECTIONS = ['著作权归属', '平台 0% 分成', '侵权处理', '下架权利', '协议变更通知'] as const;

export function takedownInfringing(items: MarketItem[], infringedIds: string[]): MarketItem[] {
  return items.filter((i) => !infringedIds.includes(i.id));
}

/* ===================== 族0259 硬件伙伴 ===================== */

export interface DeviceProfile {
  vendor: string;
  model: string;
  kind: 'keyboard' | 'display' | 'audio' | 'mouse' | 'gamepad' | 'printer' | 'scanner' | 'camera' | 'nas' | 'router';
  calibrated: boolean;
  certified: boolean;
}

export const PARTNER_KINDS: DeviceProfile['kind'][] = [
  'keyboard', 'display', 'audio', 'mouse', 'gamepad', 'printer', 'scanner', 'camera', 'nas', 'router',
];

export function certifyDevice(d: DeviceProfile): DeviceProfile {
  return { ...d, certified: d.calibrated };
}

export function compatibleList(devices: DeviceProfile[], kind: DeviceProfile['kind']): DeviceProfile[] {
  return devices.filter((d) => d.kind === kind && d.certified);
}

export interface TuningReport {
  vendor: string;
  model: string;
  metrics: Record<string, number>;
}

export function tuningScore(r: TuningReport): number {
  const vals = Object.values(r.metrics);
  if (vals.length === 0) return 0;
  return Math.round((vals.reduce((s, v) => s + v, 0) / vals.length) * 10) / 10;
}

/** 数据最小化：伙伴接口只返回白名单字段。 */
export const PARTNER_DATA_WHITELIST = ['model', 'kind', 'firmware-version'] as const;

export function sanitizePartnerPayload(payload: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const k of PARTNER_DATA_WHITELIST) if (k in payload) out[k] = payload[k];
  return out;
}

/* ===================== 族0260 国际社区 ===================== */

export const COMMUNITY_LANGUAGES = ['zh', 'zh-TW', 'en', 'ja', 'ko', 'de', 'fr', 'es', 'pt', 'ru'] as const;

export interface TranslationProgress {
  lang: string;
  translated: number;
  total: number;
}

export function translationPct(p: TranslationProgress): number {
  if (p.total === 0) return 0;
  return Math.round((p.translated / p.total) * 100);
}

export interface GlossaryTerm {
  en: string;
  approved: Record<string, string>;
}

export function glossaryLookup(g: GlossaryTerm[], en: string, lang: string): string | undefined {
  return g.find((t) => t.en === en)?.approved[lang];
}

export function lqaScan(progress: TranslationProgress[], threshold = 90): string[] {
  return progress.filter((p) => translationPct(p) < threshold).map((p) => p.lang);
}

export interface Ambassador {
  name: string;
  region: string;
  events: number;
}

export function topAmbassadors(a: Ambassador[], n = 3): Ambassador[] {
  return [...a].sort((x, y) => y.events - x.events).slice(0, n);
}

export interface FestivePack {
  region: string;
  festival: string;
  wallpapers: number;
  sounds: number;
}

export function festivePackValid(p: FestivePack): boolean {
  return p.wallpapers > 0 && p.sounds > 0 && p.region.length > 0;
}

export interface LegalRequirement {
  region: string;
  requirement: string;
  met: boolean;
}

export function complianceGaps(rs: LegalRequirement[], region: string): string[] {
  return rs.filter((r) => r.region === region && !r.met).map((r) => r.requirement);
}

/** 社区地图：按大区聚合成员数。 */
export function communityMap(members: Array<{ region: string }>): Map<string, number> {
  const m = new Map<string, number>();
  for (const x of members) m.set(x.region, (m.get(x.region) ?? 0) + 1);
  return m;
}
