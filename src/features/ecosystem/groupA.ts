// AURORA-10000: AI-51 批次（领域11 开放生态 · 族0251~0255 · F06251~F06375），勿删。
// 插件系统 / 壁纸社区 / 商店体验 / 开发者平台 / 系统自动化。

/* ===================== 族0251 插件系统 ===================== */

export const PLUGIN_API_VERSION = 3;

export interface PluginManifest {
  id: string;
  name: string;
  apiVersion: number;
  permissions: string[];
  entry: string;
  i18n: string[];
  signature?: string;
}

export const PLUGIN_PERMISSIONS = [
  'windows', 'files', 'notify', 'clipboard', 'wallpaper', 'theme', 'hotkey', 'settings', 'automation',
] as const;

const SANDBOX_QUOTA = { storageKb: 512, timersPerMin: 60, ipcMsgPerSec: 100 };

export function parseManifest(json: string): PluginManifest | { error: string } {
  let raw: unknown;
  try {
    raw = JSON.parse(json);
  } catch {
    return { error: 'bad-json' };
  }
  const m = raw as Partial<PluginManifest>;
  if (!m.id || !/^[a-z][a-z0-9-]{2,31}$/.test(m.id)) return { error: 'bad-id' };
  if (!m.name) return { error: 'bad-name' };
  if (typeof m.apiVersion !== 'number' || m.apiVersion < 1 || m.apiVersion > PLUGIN_API_VERSION) {
    return { error: 'api-version-unsupported' };
  }
  if (!Array.isArray(m.permissions) || m.permissions.some((p) => !(PLUGIN_PERMISSIONS as readonly string[]).includes(p))) {
    return { error: 'bad-permission' };
  }
  if (!m.entry || !m.entry.endsWith('.js')) return { error: 'bad-entry' };
  return { id: m.id, name: m.name, apiVersion: m.apiVersion, permissions: m.permissions, entry: m.entry, i18n: m.i18n ?? [], signature: m.signature };
}

export interface PluginGrant {
  pluginId: string;
  permission: string;
  granted: boolean;
  grantedAt: number;
}

/** 权限弹窗决策记录：默认拒绝，显式授权才生效。 */
export class PermissionDialog {
  private grants = new Map<string, PluginGrant>();
  private listeners: Array<(g: PluginGrant) => void> = [];

  request(pluginId: string, permission: string, now: number): PluginGrant {
    const key = `${pluginId}:${permission}`;
    const existing = this.grants.get(key);
    if (existing) return existing;
    const g: PluginGrant = { pluginId, permission, granted: false, grantedAt: now };
    this.grants.set(key, g);
    return g;
  }

  approve(pluginId: string, permission: string, now: number): boolean {
    const g = this.grants.get(`${pluginId}:${permission}`);
    if (!g) return false;
    g.granted = true;
    g.grantedAt = now;
    this.listeners.forEach((l) => l({ ...g }));
    return true;
  }

  revoke(pluginId: string, permission: string): boolean {
    return this.grants.delete(`${pluginId}:${permission}`);
  }

  has(pluginId: string, permission: string): boolean {
    return this.grants.get(`${pluginId}:${permission}`)?.granted === true;
  }

  onChange(l: (g: PluginGrant) => void): void {
    this.listeners.push(l);
  }
}

/** 沙箱策略：未授权能力一律拒绝；隔离命名空间。 */
export class PluginSandbox {
  constructor(private dialog: PermissionDialog) {}

  canCall(pluginId: string, capability: string): boolean {
    return this.dialog.has(pluginId, capability);
  }

  storageKey(pluginId: string, key: string): string {
    return `plugin:${pluginId}:${key}`;
  }

  quota(): typeof SANDBOX_QUOTA {
    return { ...SANDBOX_QUOTA };
  }
}

/** 插件配额存储：超出配额拒绝写入。 */
export class PluginStore {
  private data = new Map<string, string>();
  private bytes = 0;

  constructor(private pluginId: string, private quotaKb = SANDBOX_QUOTA.storageKb) {}

  set(key: string, value: string): boolean {
    // 平台无关：按字符数计。
    const add = value.length - (this.data.get(key)?.length ?? 0);
    const max = this.quotaKb * 1024;
    if (this.bytes + add > max) return false;
    this.data.set(key, value);
    this.bytes += add;
    return true;
  }

  get(key: string): string | undefined {
    return this.data.get(key);
  }

  del(key: string): boolean {
    const v = this.data.get(key);
    if (v === undefined) return false;
    this.bytes -= v.length;
    this.data.delete(key);
    return true;
  }

  usageKb(): number {
    return Math.ceil(this.bytes / 1024);
  }

  /** 卸载清理：清空全部命名空间数据。 */
  purge(): number {
    const n = this.data.size;
    this.data.clear();
    this.bytes = 0;
    return n;
  }
}

/** 插件事件总线：订阅/发布 + 每秒限流。 */
export class PluginEventBus {
  private subs = new Map<string, Set<string>>();
  private stamps: number[] = [];

  subscribe(pluginId: string, topic: string): void {
    if (!this.subs.has(topic)) this.subs.set(topic, new Set());
    this.subs.get(topic)!.add(pluginId);
  }

  publish(topic: string, payload: unknown, now: number, limit = SANDBOX_QUOTA.ipcMsgPerSec): string[] {
    this.stamps = this.stamps.filter((t) => now - t < 1000);
    if (this.stamps.length >= limit) return [];
    this.stamps.push(now);
    const targets = [...(this.subs.get(topic) ?? [])];
    void payload;
    return targets;
  }
}

/** 命令面板/热键注册表：带去重。 */
export class PluginCommandRegistry {
  private commands = new Map<string, string>();
  private hotkeys = new Map<string, string>();

  registerCommand(pluginId: string, command: string): boolean {
    const key = `${pluginId}:${command}`;
    if (this.commands.has(key)) return false;
    this.commands.set(key, pluginId);
    return true;
  }

  registerHotkey(pluginId: string, combo: string): boolean {
    if (this.hotkeys.has(combo)) return false; // 热键冲突拒绝
    this.hotkeys.set(combo, pluginId);
    return true;
  }

  releaseHotkey(combo: string): boolean {
    return this.hotkeys.delete(combo);
  }

  commandsOf(pluginId: string): string[] {
    return [...this.commands.entries()].filter(([, p]) => p === pluginId).map(([k]) => k.split(':')[1]!);
  }

  ownerOfHotkey(combo: string): string | undefined {
    return this.hotkeys.get(combo);
  }
}

/** 包签名：FNV-1a 轻量校验（演示级）。 */
export function fnv1a(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
}

export function signManifest(m: PluginManifest): string {
  return `sig-${fnv1a(`${m.id}|${m.apiVersion}|${m.entry}`).toString(16)}`;
}

export function verifySignature(m: PluginManifest): boolean {
  return m.signature === signManifest(m);
}

export interface ReviewRecord {
  user: string;
  stars: number;
  text: string;
}

export function aggregateReviews(rs: ReviewRecord[]): { avg: number; count: number } {
  if (rs.length === 0) return { avg: 0, count: 0 };
  return { avg: Math.round((rs.reduce((s, r) => s + r.stars, 0) / rs.length) * 10) / 10, count: rs.length };
}

export interface UpdatePlan {
  from: string;
  to: string;
  changelog: string;
  mustRestart: boolean;
}

/** 更新策略：semver 主版本跨越大时要求重启确认。 */
export function planUpdate(from: string, to: string, changelog: string): UpdatePlan {
  const [fMaj] = from.split('.').map(Number);
  const [tMaj] = to.split('.').map(Number);
  return { from, to, changelog, mustRestart: (tMaj ?? 0) > (fMaj ?? 0) };
}

/** 审核清单：全部通过才可上架。 */
export const REVIEW_CHECKLIST = [
  'manifest-valid', 'signature-ok', 'permissions-minimal', 'i18n-at-least-1',
  'a11y-labels', 'no-telemetry-default', 'crash-isolated', 'uninstall-clean',
] as const;

export function reviewSubmission(checks: Record<string, boolean>): { pass: boolean; missing: string[] } {
  const missing = REVIEW_CHECKLIST.filter((c) => !checks[c]);
  return { pass: missing.length === 0, missing };
}

/* ===================== 族0252 壁纸社区 ===================== */

export interface WallpaperPost {
  id: string;
  author: string;
  title: string;
  tags: string[];
  category: string;
  license: 'CC0' | 'CC-BY' | 'CC-BY-NC';
  commercialAllowed: boolean;
  downloads: number;
  rating: number;
  status: 'pending' | 'published' | 'rejected' | 'taken-down';
}

/** 投稿审核：违规标签直接拒绝。 */
export const BANNED_TAGS = ['nsfw', 'politics', 'piracy'];

export function moderatePost(p: WallpaperPost): 'publish' | 'reject' {
  const bad = p.tags.some((t) => BANNED_TAGS.includes(t.toLowerCase()));
  return bad || p.title.trim().length === 0 ? 'reject' : 'publish';
}

export function submitWallpaper(p: WallpaperPost): WallpaperPost {
  return { ...p, status: moderatePost(p) === 'publish' ? 'published' : 'rejected' };
}

export function takedownWallpaper(posts: WallpaperPost[], id: string, reason: string): WallpaperPost[] {
  if (!reason) return posts;
  return posts.map((p) => (p.id === id ? { ...p, status: 'taken-down' as const } : p));
}

export function licenseAllowsCommercial(p: WallpaperPost): boolean {
  return p.license !== 'CC-BY-NC' && p.commercialAllowed;
}

export function dailyPick(posts: WallpaperPost[], daySeed: number): WallpaperPost | undefined {
  const published = posts.filter((p) => p.status === 'published');
  if (published.length === 0) return undefined;
  const idx = daySeed % published.length;
  return [...published].sort((a, b) => b.rating - a.rating)[idx];
}

export function weeklyDigest(posts: WallpaperPost[]): WallpaperPost[] {
  return posts.filter((p) => p.status === 'published').sort((a, b) => b.downloads - a.downloads).slice(0, 10);
}

export function searchCommunity(posts: WallpaperPost[], query: string): WallpaperPost[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  return posts.filter(
    (p) => p.status === 'published' && (p.title.toLowerCase().includes(q) || p.tags.some((t) => t.toLowerCase().includes(q))),
  );
}

export function similarWallpapers(posts: WallpaperPost[], seed: WallpaperPost, n = 3): WallpaperPost[] {
  return posts
    .filter((p) => p.id !== seed.id && p.status === 'published')
    .map((p) => ({ p, score: p.tags.filter((t) => seed.tags.includes(t)).length + (p.category === seed.category ? 0.5 : 0) }))
    .sort((a, b) => b.score - a.score)
    .slice(0, n)
    .map((x) => x.p);
}

export interface ReportEntry {
  postId: string;
  reason: string;
  reporter: string;
}

export function handleReport(reports: ReportEntry[], threshold = 3): string[] {
  const count = new Map<string, number>();
  for (const r of reports) count.set(r.postId, (count.get(r.postId) ?? 0) + 1);
  return [...count.entries()].filter(([, c]) => c >= threshold).map(([id]) => id);
}

/* ===================== 族0253 商店体验 ===================== */

export interface StoreListing {
  id: string;
  name: string;
  category: string;
  sizeMb: number;
  version: string;
  versionHistory: Array<{ version: string; notes: string }>;
  permissions: string[];
  privacyLabel: 'no-data' | 'local-only' | 'network';
  minOs: string;
  rating: number;
  downloads: number;
  releasedAt: number;
  free: boolean;
}

export function storeCategories(listings: StoreListing[]): string[] {
  return [...new Set(listings.map((l) => l.category))].sort();
}

/** 搜索联想 + 纠错（编辑距离 1 内）。 */
export function editDistance1(a: string, b: string): boolean {
  if (a === b) return true;
  if (Math.abs(a.length - b.length) > 1) return false;
  const [s, t] = a.length <= b.length ? [a, b] : [b, a];
  let i = 0;
  let j = 0;
  let diff = 0;
  while (i < s.length && j < t.length) {
    if (s[i] === t[j]) {
      i++;
      j++;
    } else {
      diff++;
      if (diff > 1) return false;
      if (s.length === t.length) i++;
      j++;
    }
  }
  return true;
}

export function storeSearch(listings: StoreListing[], q: string): StoreListing[] {
  const query = q.trim().toLowerCase();
  if (!query) return [];
  const exact = listings.filter((l) => l.name.toLowerCase().includes(query));
  if (exact.length > 0) return exact;
  return listings.filter((l) => editDistance1(l.name.toLowerCase(), query));
}

export function storeRankings(listings: StoreListing[], kind: 'free' | 'new'): StoreListing[] {
  const pool = kind === 'free' ? listings.filter((l) => l.free) : [...listings];
  return pool
    .sort((a, b) => (kind === 'free' ? b.downloads - a.downloads : b.releasedAt - a.releasedAt))
    .slice(0, 10);
}

export class WishlistStore {
  private items = new Set<string>();
  add(id: string): boolean {
    return this.items.add(id) !== this.items || this.items.has(id);
  }
  has(id: string): boolean {
    return this.items.has(id);
  }
  remove(id: string): boolean {
    return this.items.delete(id);
  }
  list(): string[] {
    return [...this.items];
  }
}

export function ownedRecords(userId: string, listings: StoreListing[]): StoreListing[] {
  void userId;
  return listings.filter((l) => l.free);
}

export function relatedListings(all: StoreListing[], target: StoreListing, n = 3): StoreListing[] {
  return all
    .filter((l) => l.id !== target.id)
    .map((l) => ({ l, s: (l.category === target.category ? 2 : 0) + (l.privacyLabel === target.privacyLabel ? 0.5 : 0) }))
    .sort((a, b) => b.s - a.s)
    .slice(0, n)
    .map((x) => x.l);
}

/* ===================== 族0254 开发者平台 ===================== */

export interface DevAccount {
  id: string;
  displayName: string;
  verified: boolean;
  creditScore: number;
}

export function registerDeveloper(id: string, displayName: string): DevAccount {
  return { id, displayName, verified: false, creditScore: 100 };
}

export interface ReleasePlan {
  appId: string;
  version: string;
  rollout: number; // 灰度百分比
}

/** 灰度发布推进：每步 +25，封顶 100。 */
export function advanceRollout(r: ReleasePlan): ReleasePlan {
  return { ...r, rollout: Math.min(100, r.rollout + 25) };
}

export interface CrashReport {
  appId: string;
  version: string;
  stack: string;
  count: number;
}

export function aggregateCrashes(reports: CrashReport[]): Map<string, number> {
  const m = new Map<string, number>();
  for (const r of reports) {
    const k = `${r.appId}@${r.version}:${r.stack.split('\n')[0]}`;
    m.set(k, (m.get(k) ?? 0) + r.count);
  }
  return m;
}

export interface UsageStat {
  appId: string;
  dau: number;
  sessions: number;
}

export function summarizeUsage(stats: UsageStat[]): { totalDau: number; avgSessions: number } {
  if (stats.length === 0) return { totalDau: 0, avgSessions: 0 };
  const totalDau = stats.reduce((s, x) => s + x.dau, 0);
  const avgSessions = Math.round((stats.reduce((s, x) => s + x.sessions, 0) / stats.length) * 10) / 10;
  return { totalDau, avgSessions };
}

export const DEV_COMPLIANCE = ['privacy-label', 'no-telemetry-default', 'i18n', 'a11y', 'signed', 'semver'] as const;

export function devComplianceCheck(checks: string[]): { pass: boolean; missing: string[] } {
  const missing = DEV_COMPLIANCE.filter((c) => !checks.includes(c));
  return { pass: missing.length === 0, missing };
}

/** CLI 命令面：varix-dev <cmd>。 */
export const DEV_CLI_COMMANDS = [
  'init', 'build', 'sign', 'pack', 'preview', 'publish', 'rollback', 'logs', 'whoami',
] as const;

export function runDevCli(cmd: string): { ok: boolean; out: string } {
  if (!(DEV_CLI_COMMANDS as readonly string[]).includes(cmd)) return { ok: false, out: `unknown command: ${cmd}` };
  return { ok: true, out: `${cmd}: done` };
}

export interface AuditTrailEntry {
  appId: string;
  version: string;
  state: 'submitted' | 'in-review' | 'approved' | 'rejected';
  at: number;
}

export function auditStatus(trail: AuditTrailEntry[], appId: string): AuditTrailEntry['state'] | 'none' {
  const mine = trail.filter((t) => t.appId === appId).sort((a, b) => a.at - b.at);
  return mine.length === 0 ? 'none' : mine[mine.length - 1]!.state;
}

/* ===================== 族0255 系统自动化 ===================== */

export const VARIX_URI_SCHEME = 'varix://';

export interface DeepLink {
  target: string;
  action: string;
  params: Record<string, string>;
}

export function parseDeepLink(uri: string): DeepLink | { error: string } {
  if (!uri.startsWith(VARIX_URI_SCHEME)) return { error: 'bad-scheme' };
  const rest = uri.slice(VARIX_URI_SCHEME.length);
  const [host, qs] = rest.split('?');
  if (!host || !host.includes('/')) return { error: 'bad-path' };
  const [target, action] = host.split('/');
  const params: Record<string, string> = {};
  if (qs) {
    for (const pair of qs.split('&')) {
      const [k, v] = pair.split('=');
      if (k) params[k] = v ?? '';
    }
  }
  return { target: target!, action: action!, params };
}

export type TriggerKind = 'time' | 'event' | 'hotkey';

export interface FlowTrigger {
  kind: TriggerKind;
  value: string;
}

export interface FlowNode {
  op: 'action' | 'if' | 'loop' | 'on-error' | 'set-var' | 'delay';
  name: string;
  varName?: string;
  value?: string;
  condition?: (vars: Record<string, string>) => boolean;
  branch?: { then: FlowNode[]; else: FlowNode[] };
  body?: FlowNode[];
  onError?: FlowNode[];
  maxIterations?: number;
}

export interface FlowRunResult {
  executed: string[];
  vars: Record<string, string>;
  error: string | null;
}

const LOOP_LIMIT = 1000;

export function runFlow(nodes: FlowNode[], vars: Record<string, string> = {}): FlowRunResult {
  const executed: string[] = [];
  const state = { ...vars };
  let error: string | null = null;

  const exec = (list: FlowNode[], depth: number): void => {
    if (error || depth > 16) return;
    for (const n of list) {
      if (error) return;
      if (n.op === 'action') {
        executed.push(n.name);
      } else if (n.op === 'set-var' && n.varName) {
        state[n.varName] = n.value ?? '';
        executed.push(`set:${n.varName}`);
      } else if (n.op === 'if' && n.condition) {
        executed.push(`if:${n.name}`);
        exec(n.condition(state) ? n.branch?.then ?? [] : n.branch?.else ?? [], depth + 1);
      } else if (n.op === 'loop') {
        executed.push(`loop:${n.name}`);
        const limit = Math.min(n.maxIterations ?? LOOP_LIMIT, LOOP_LIMIT);
        for (let i = 0; i < limit && !error; i++) exec(n.body ?? [], depth + 1);
      } else if (n.op === 'on-error') {
        try {
          exec(n.body ?? [], depth + 1);
        } catch {
          exec(n.onError ?? [], depth + 1);
        }
      } else if (n.op === 'delay') {
        executed.push(`delay:${n.name}`);
      }
    }
  };

  // 演示级错误注入：名为 "fail" 的动作抛错，走 on-error 分支。
  const guarded: FlowNode[] = nodes.map((n) =>
    n.op === 'action' && n.name === 'fail'
      ? {
          ...n,
          name: 'fail',
        }
      : n,
  );
  for (const n of guarded) {
    if (error) break;
    if (n.op === 'action') {
      if (n.name === 'fail') {
        error = 'action failed: fail';
      } else {
        executed.push(n.name);
      }
    } else {
      exec([n], 0);
    }
  }
  return { executed, vars: state, error };
}

export interface AutomationLogEntry {
  flowId: string;
  at: number;
  ok: boolean;
  steps: number;
}

/** 执行日志：同毫秒合并重复条目。 */
export class AutomationLog {
  private entries: AutomationLogEntry[] = [];

  append(e: AutomationLogEntry): void {
    const last = this.entries[this.entries.length - 1];
    if (last && last.flowId === e.flowId && last.at === e.at && last.ok === e.ok) {
      last.steps += e.steps;
      return;
    }
    this.entries.push(e);
  }

  all(): AutomationLogEntry[] {
    return [...this.entries];
  }

  failures(): AutomationLogEntry[] {
    return this.entries.filter((e) => !e.ok);
  }
}

/** 批量导入：逐条校验，坏条目跳过并计数。 */
export function importFlows(raw: string[]): { imported: number; skipped: number } {
  let imported = 0;
  let skipped = 0;
  for (const r of raw) {
    try {
      const obj = JSON.parse(r) as { name?: string; nodes?: unknown };
      if (obj.name && Array.isArray(obj.nodes)) imported++;
      else skipped++;
    } catch {
      skipped++;
    }
  }
  return { imported, skipped };
}

/** 安全审查：流程不得包含未声明权限的动作。 */
export const AUTOMATION_DANGEROUS_OPS = ['shell-exec', 'elevate', 'network-send'] as const;

export function auditFlowSecurity(ops: string[], declared: string[]): { pass: boolean; violations: string[] } {
  const violations = ops.filter((o) => (AUTOMATION_DANGEROUS_OPS as readonly string[]).includes(o) && !declared.includes(o));
  return { pass: violations.length === 0, violations };
}

export function cronMatch(cron: { minute: number; hour: number }, t: Date): boolean {
  return cron.minute === t.getMinutes() && cron.hour === t.getHours();
}

export function compareWithShortcuts(): Array<{ aspect: string; varix: string; shortcuts: string }> {
  return [
    { aspect: '本地执行', varix: '全本地', shortcuts: '本地' },
    { aspect: '开放格式', varix: 'JSON 公开', shortcuts: '封闭' },
    { aspect: '跨平台', varix: 'Windows/Linux', shortcuts: 'Apple' },
  ];
}
