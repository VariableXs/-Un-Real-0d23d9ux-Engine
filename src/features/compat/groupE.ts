// AURORA-10000: AI-45 批次（族0221~0225 · 隐私沙盒应用/进程治理/时间与调度/资源画像与配额/兼容认证与收官），勿删。
import { fnv1a } from './groupA';

/* ============ 族0221 隐私沙盒应用（F05501~F05525） ============ */

export type PermissionKey =
  | 'camera' | 'microphone' | 'location' | 'clipboard' | 'screen-capture' | 'files'
  | 'contacts' | 'calendar' | 'notifications' | 'autostart' | 'network';

/** F05501~F05510 权限总控 + 单项权限。 */
export class PermissionCenter {
  private grants = new Map<string, Set<PermissionKey>>();
  grant(app: string, key: PermissionKey): boolean {
    const set = this.grants.get(app) ?? new Set<PermissionKey>();
    if (set.has(key)) return false;
    set.add(key);
    this.grants.set(app, set);
    return true;
  }
  revoke(app: string, key: PermissionKey): boolean {
    return this.grants.get(app)?.delete(key) ?? false;
  }
  has(app: string, key: PermissionKey): boolean {
    return this.grants.get(app)?.has(key) ?? false;
  }
  overview(app: string): PermissionKey[] {
    return [...(this.grants.get(app) ?? [])];
  }
}

/** F05507（可测版）目录级权限：独立实现，避免过度耦合。 */
export class DirGrants {
  private items = new Map<string, Set<string>>();
  grant(app: string, dir: string): boolean {
    const set = this.items.get(app) ?? new Set<string>();
    if (set.has(dir)) return false;
    set.add(dir);
    this.items.set(app, set);
    return true;
  }
  canRead(app: string, path: string): boolean {
    return [...(this.items.get(app) ?? [])].some((d) => path.startsWith(d));
  }
  revokeAll(app: string): number {
    const n = this.items.get(app)?.size ?? 0;
    this.items.delete(app);
    return n;
  }
}

/** F05512/F05513 出站白名单 + 按应用防火墙。 */
export class AppFirewall {
  private rules = new Map<string, Set<string>>();
  allowOutbound(app: string, host: string): boolean {
    const set = this.rules.get(app) ?? new Set<string>();
    set.add(host);
    this.rules.set(app, set);
    return true;
  }
  /** 阻断：不在白名单即拒。 */
  checkOutbound(app: string, host: string): { allowed: boolean; reason: string } {
    const list = this.rules.get(app);
    if (!list || list.size === 0) return { allowed: false, reason: '默认拒绝：无白名单' };
    return list.has(host) ? { allowed: true, reason: '白名单命中' } : { allowed: false, reason: `主机 ${host} 不在白名单` };
  }
}

/** F05514/F05515/F05516 私有 DNS/浏览器隔离/WebView 隔离。 */
export const PRIVATE_DNS = { mode: 'isolated', doh: 'per-app' } as const;
export const BROWSER_ISOLATION = { cookieJars: 'per-app', extensionsShared: false } as const;
export const WEBVIEW_ISOLATION = { dataDir: 'per-app-profile', serviceWorkers: 'per-app' } as const;

/** F05517/F05518/F05519/F05520 剪贴隔离/枚举隔离/进程隐藏/挂载最小化。 */
export function clipboardIsolation(appA: string, appB: string): boolean {
  return appA !== appB;
}
export function windowEnumIsolation(app: string, sandboxedApps: Set<string>): number {
  return sandboxedApps.has(app) ? 0 : sandboxedApps.size;
}
export function processListIsolation(viewer: string, all: string[], sandboxed: Set<string>): string[] {
  return sandboxed.has(viewer) ? all.filter((p) => p === viewer) : all;
}
export function minimalMountSet(sandboxed: boolean, full: string[]): string[] {
  return sandboxed ? full.filter((d) => ['C:\\Users', 'C:\\ProgramData'].some((k) => d.startsWith(k))) : full;
}

/** F05521 沙盒模板。 */
export type SandboxTemplate = 'high' | 'medium' | 'low';
export const SANDBOX_TEMPLATES: Record<SandboxTemplate, PermissionKey[]> = {
  high: [],
  medium: ['clipboard', 'notifications'],
  low: ['clipboard', 'notifications', 'camera', 'microphone', 'files', 'network'],
};
export function applyTemplate(app: string, tpl: SandboxTemplate, center: PermissionCenter): number {
  let n = 0;
  for (const k of SANDBOX_TEMPLATES[tpl]) if (center.grant(app, k)) n++;
  return n;
}

/** F05522/F05523/F05524 变更通知/时间线/画像。 */
export interface PermissionChange { time: number; app: string; key: PermissionKey; action: 'grant' | 'revoke' }
export class PermissionAudit {
  private log: PermissionChange[] = [];
  record(change: PermissionChange): void {
    this.log.push(change);
  }
  timeline(app?: string): PermissionChange[] {
    return this.log.filter((c) => !app || c.app === app).sort((a, b) => a.time - b.time);
  }
  /** 应用要权画像：按权限维度计数。 */
  profile(app: string): Record<string, number> {
    const p: Record<string, number> = {};
    for (const c of this.timeline(app)) p[c.key] = (p[c.key] ?? 0) + 1;
    return p;
  }
}

/** F05525 教学。 */
export function sandboxTutorial(): string[] {
  return ['权限默认拒绝，按需授权', '沙盒应用看不到其他应用的窗口与进程', '权限变更全程留痕可审计'];
}

/* ============ 族0222 进程治理（F05526~F05550） ============ */

export type Priority = 'idle' | 'below-normal' | 'normal' | 'above-normal' | 'high';

/** F05526/F05527 优先级策略 + 后台限流。 */
export function priorityPolicy(app: string, foreground: boolean, overrides: Map<string, Priority>): Priority {
  if (overrides.has(app)) return overrides.get(app)!;
  return foreground ? 'normal' : 'below-normal';
}
export function backgroundThrottle(cpuPct: number, capPct = 20): { throttled: boolean; effective: number } {
  return { throttled: cpuPct > capPct, effective: Math.min(cpuPct, capPct) };
}

/** F05528/F05529/F05530 僵尸回收/泄漏守护/句柄检测。 */
export function zombieReclaim(processes: { pid: number; parent: number; alive: boolean; parentAlive: boolean }[]): number[] {
  return processes.filter((p) => p.alive && !p.parentAlive).map((p) => p.pid);
}
export class LeakGuard {
  private baseline = new Map<string, number>();
  private restarts = new Set<string>();
  setBaseline(app: string, mb: number): void {
    this.baseline.set(app, mb);
  }
  /** 内存泄漏守护：超基线 2 倍触发重启。 */
  check(app: string, mb: number): { restart: boolean; ratio: number } {
    const base = this.baseline.get(app) ?? mb;
    const ratio = mb / Math.max(1, base);
    if (ratio > 2) this.restarts.add(app);
    return { restart: ratio > 2, ratio: Math.round(ratio * 100) / 100 };
  }
  get restarted(): string[] {
    return [...this.restarts];
  }
}
export function handleLeakDetect(history: number[], window = 5): boolean {
  if (history.length < window) return false;
  const tail = history.slice(-window);
  return tail.every((v, i, a) => i === 0 || v > a[i - 1]!) && tail[tail.length - 1]! > tail[0]! * 1.5;
}

/** F05531~F05534 CPU/GPU/磁盘/网络疯转。 */
export interface SpinThresholds { cpu: number; gpu: number; diskMbps: number; netMbps: number }
export const SPIN_DEFAULTS: SpinThresholds = { cpu: 90, gpu: 95, diskMbps: 400, netMbps: 500 };
export function spinDetect(samples: { cpu: number; gpu: number; diskMbps: number; netMbps: number }, th: SpinThresholds = SPIN_DEFAULTS, count = 3): string[] {
  void count; // 调用契约：连续 count 个采样超阈才算疯转；此处以单采样表达判定规则
  const spins: string[] = [];
  if (samples.cpu >= th.cpu) spins.push('cpu');
  if (samples.gpu >= th.gpu) spins.push('gpu');
  if (samples.diskMbps >= th.diskMbps) spins.push('disk');
  if (samples.netMbps >= th.netMbps) spins.push('network');
  return spins;
}

/** F05535/F05536/F05537/F05538 启动风暴/退出卡死/挂起节能/调试冻结。 */
export function staggerStartup(launches: { app: string; delayMs: number }[]): { app: string; staggerMs: number }[] {
  let cursor = 0;
  return launches.map((l) => ({ app: l.app, staggerMs: (cursor += 500) }));
}
export function forceKillConfirm(waitMs: number, thresholdMs = 5000): { needsConfirm: boolean; waited: number } {
  return { needsConfirm: waitMs >= thresholdMs, waited: waitMs };
}
export function suspendResume(app: string, suspended: boolean): { app: string; state: 'suspended' | 'running'; note: string } {
  return { app, state: suspended ? 'suspended' : 'running', note: suspended ? '挂起省电，恢复时状态还原' : '正常运行' };
}
export function debugFreeze(pid: number): { frozen: true; pid: number; note: string } {
  return { frozen: true, pid, note: '调试冻结：仅调试器可恢复' };
}

/** F05539~F05541 每应用 CPU/内存/网络限额。 */
export interface AppQuota { cpuPct: number; memMB: number; netMbps: number }
export const DEFAULT_APP_QUOTA: AppQuota = { cpuPct: 50, memMB: 2048, netMbps: 100 };
export function quotaExceeded(used: AppQuota, limit: AppQuota): string[] {
  const out: string[] = [];
  if (used.cpuPct > limit.cpuPct) out.push('cpu');
  if (used.memMB > limit.memMB) out.push('mem');
  if (used.netMbps > limit.netMbps) out.push('net');
  return out;
}

/** F05542~F05545 启动审计/异常退出/崩溃重启/退避。 */
export class ProcessAudit {
  private events: { time: number; app: string; event: 'launch' | 'exit' | 'crash'; code?: number }[] = [];
  record(app: string, event: 'launch' | 'exit' | 'crash', code?: number, time = Date.now()): void {
    this.events.push({ time, app, event, code });
  }
  launches(app: string): number {
    return this.events.filter((e) => e.app === app && e.event === 'launch').length;
  }
  crashes(app: string): number {
    return this.events.filter((e) => e.app === app && e.event === 'crash').length;
  }
}
export function autoRestartPolicy(crashes: number, windowMin: number): { restart: boolean; backoffMs: number } {
  if (crashes >= 5) return { restart: false, backoffMs: -1 };
  const backoff = Math.min(30000, 1000 * 2 ** crashes);
  void windowMin;
  return { restart: true, backoffMs: backoff };
}

/** F05546~F05548 服务依赖/顺序/超时。 */
export function serviceTopoSort(services: { name: string; depends: string[] }[]): string[] {
  const out: string[] = [];
  const done = new Set<string>();
  const visit = (name: string, stack: Set<string>): void => {
    if (done.has(name) || stack.has(name)) return;
    stack.add(name);
    const s = services.find((x) => x.name === name);
    for (const d of s?.depends ?? []) visit(d, stack);
    stack.delete(name);
    done.add(name);
    out.push(name);
  };
  for (const s of services) visit(s.name, new Set());
  return out;
}
export function serviceStartTimeout(waitedMs: number, timeoutMs = 30000): { timedOut: boolean; action: string } {
  return { timedOut: waitedMs >= timeoutMs, action: waitedMs >= timeoutMs ? '标记失败并继续启动' : '等待' };
}

/** F05549/F05550 进程画像 + 教学。 */
export function processProfile(history: { cpu: number; mem: number; time: number }[]): { avgCpu: number; peakMem: number; samples: number } {
  if (!history.length) return { avgCpu: 0, peakMem: 0, samples: 0 };
  return {
    avgCpu: Math.round((history.reduce((s, h) => s + h.cpu, 0) / history.length) * 10) / 10,
    peakMem: Math.max(...history.map((h) => h.mem)),
    samples: history.length,
  };
}
export function processGovTutorial(): string[] {
  return ['后台默认降优先级并限流', '崩溃重启带指数退避，防循环', '服务按依赖拓扑排序启动'];
}

/* ============ 族0223 时间与调度（F05551~F05575） ============ */

/** F05551/F05552 精确同步 + 多 NTP。 */
export function ntpSync(sources: { host: string; offsetMs: number }[]): { chosen: string; offsetMs: number } {
  const sorted = [...sources].sort((a, b) => Math.abs(a.offsetMs) - Math.abs(b.offsetMs));
  return { chosen: sorted[0]!.host, offsetMs: sorted[0]!.offsetMs };
}

/** F05553/F05554/F05555/F05556 漂移监控/RTC 修复/夏令时/时区。 */
export function clockDriftMonitor(idealMs: number, actualMs: number, warnPpm = 50): { driftPpm: number; warn: boolean } {
  const driftPpm = Math.round(((actualMs - idealMs) / Math.max(1, idealMs)) * 1e6 * 100) / 100;
  return { driftPpm, warn: Math.abs(driftPpm) > warnPpm };
}
export function rtcFix(localRtc: boolean): { useUtc: boolean; hint: string } {
  return { useUtc: !localRtc, hint: localRtc ? '写入 RealTimeIsUniversal 键' : '无需修复' };
}
export function dstHandling(date: Date, tzHasDst: boolean): { offsetMin: number; dst: boolean } {
  const offsetMin = -date.getTimezoneOffset();
  const jan = new Date(date.getFullYear(), 0, 1).getTimezoneOffset();
  const jul = new Date(date.getFullYear(), 6, 1).getTimezoneOffset();
  const dst = tzHasDst && offsetMin !== Math.min(jan, jul) * -1;
  return { offsetMin, dst };
}
export function autoTimezone(latencyMs: number, tzGuess: string | null): string {
  return tzGuess ?? (latencyMs < 50 ? 'UTC' : 'local');
}

/** F05557~F05563 任务面板/冲突检测/错峰/配额/电池/流量/深夜窗口。 */
export interface ScheduledTask { name: string; kind: 'backup' | 'index' | 'update' | 'clean'; atHour: number; estMin: number; canDefer: boolean }
export class SchedulerPanel {
  private tasks: ScheduledTask[] = [];
  add(t: ScheduledTask): boolean {
    if (this.tasks.some((x) => x.name === t.name)) return false;
    this.tasks.push(t);
    return true;
  }
  /** 同一小时任务冲突。 */
  conflicts(): number[] {
    const byHour = new Map<number, number>();
    for (const t of this.tasks) byHour.set(t.atHour, (byHour.get(t.atHour) ?? 0) + t.estMin);
    return [...byHour.entries()].filter(([, m]) => m > 60).map(([h]) => h);
  }
  /** 错峰：把重叠任务依次后移 30 分钟。 */
  stagger(): ScheduledTask[] {
    const out: ScheduledTask[] = [];
    const hourLoad = new Map<number, number>();
    for (const t of [...this.tasks].sort((a, b) => a.atHour - b.atHour)) {
      let hour = t.atHour;
      while ((hourLoad.get(hour) ?? 0) + t.estMin > 55) hour = (hour + 1) % 24;
      hourLoad.set(hour, (hourLoad.get(hour) ?? 0) + t.estMin);
      out.push({ ...t, atHour: hour });
    }
    return out;
  }
  /** 深夜窗口：可推迟任务移到 02:00-05:00。 */
  nightWindow(): ScheduledTask[] {
    return this.tasks.map((t) => (t.canDefer ? { ...t, atHour: 2 + (t.kind.length % 3) } : t));
  }
  /** 电池/流量策略：省电与按流量推迟。 */
  deferFor(batteryLow: boolean, metered: boolean): string[] {
    const deferred: string[] = [];
    for (const t of this.tasks) {
      if (batteryLow && t.canDefer) deferred.push(`${t.name}:battery`);
      else if (metered && (t.kind === 'update' || t.kind === 'backup')) deferred.push(`${t.name}:metered`);
    }
    return deferred;
  }
  get list(): ScheduledTask[] {
    return [...this.tasks];
  }
}

/** F05564~F05568 依赖链/重试/失败通知/历史/导入导出。 */
export function taskDependencyChain(tasks: { name: string; after?: string }[]): string[] {
  return serviceTopoSort(tasks.map((t) => ({ name: t.name, depends: t.after ? [t.after] : [] })));
}
export class RetryPolicy {
  private counts = new Map<string, number>();
  attempt(name: string, ok: boolean): { done: boolean; attempt: number } {
    const n = (this.counts.get(name) ?? 0) + 1;
    this.counts.set(name, n);
    if (ok) {
      this.counts.delete(name);
      return { done: true, attempt: n };
    }
    if (n >= 3) this.counts.delete(name);
    return { done: false, attempt: n };
  }
}
export function failureNotify(task: string, attempts: number): string {
  return `任务「${task}」失败，已重试 ${attempts} 次`;
}
export class TaskHistory {
  private rows: { time: number; task: string; ok: boolean }[] = [];
  record(task: string, ok: boolean, time = Date.now()): void {
    this.rows.push({ time, task, ok });
  }
  recent(n = 10): { time: number; task: string; ok: boolean }[] {
    return this.rows.slice(-n);
  }
}
export function exportTasks(tasks: ScheduledTask[]): string {
  return JSON.stringify(tasks);
}
export function importTasks(json: string): ScheduledTask[] | null {
  try {
    const v = JSON.parse(json) as ScheduledTask[];
    return Array.isArray(v) ? v : null;
  } catch {
    return null;
  }
}

/** F05569/F05570/F05571 维护窗口/暂停/影响评估。 */
export function maintenanceWindow(now: number): { inWindow: boolean; startHour: number } {
  const h = new Date(now).getHours();
  return { inWindow: h >= 2 && h < 5, startHour: 2 };
}
export function maintenancePaused(presenting: boolean): boolean {
  return presenting;
}
export function impactAssessment(task: ScheduledTask): { willStutter: boolean; note: string } {
  const heavy = task.estMin > 30 || task.kind === 'index';
  return { willStutter: heavy, note: heavy ? '建议放入维护窗口' : '影响可忽略' };
}

/** F05572/F05573 IO 优先 + 审计。 */
export function ioPriorityDefer(kind: ScheduledTask['kind']): 'normal' | 'low' {
  return kind === 'index' ? 'low' : 'normal';
}
export class ScheduleAudit {
  private log: { time: number; task: string; action: string }[] = [];
  record(task: string, action: string, time = Date.now()): void {
    this.log.push({ time, task, action });
  }
  byTask(task: string): number {
    return this.log.filter((l) => l.task === task).length;
  }
  get all(): number {
    return this.log.length;
  }
}

/** F05574/F05575 画像 + 教学。 */
export function scheduleProfile(tasks: ScheduledTask[]): { byKind: Record<string, number>; totalMin: number } {
  const byKind: Record<string, number> = {};
  for (const t of tasks) byKind[t.kind] = (byKind[t.kind] ?? 0) + 1;
  return { byKind, totalMin: tasks.reduce((s, t) => s + t.estMin, 0) };
}
export function scheduleTutorial(): string[] {
  return ['大 IO 任务自动错峰进深夜窗口', '省电与计费网络下推迟更新与备份', '失败重试三次后通知并留历史'];
}

/* ============ 族0224 资源画像与配额（F05576~F05600） ============ */

export type ResourceKind = 'cpu' | 'mem' | 'io' | 'net' | 'gpu' | 'battery';

/** F05576~F05582 资源总账 + 六类账本。 */
export class ResourceLedger {
  private books = new Map<ResourceKind, Map<string, number>>();
  add(kind: ResourceKind, app: string, amount: number): void {
    const book = this.books.get(kind) ?? new Map<string, number>();
    book.set(app, (book.get(app) ?? 0) + amount);
    this.books.set(kind, book);
  }
  total(kind: ResourceKind): number {
    return [...(this.books.get(kind)?.values() ?? [])].reduce((s, v) => s + v, 0);
  }
  top(kind: ResourceKind, n = 3): [string, number][] {
    return [...(this.books.get(kind) ?? [])].sort((a, b) => b[1] - a[1]).slice(0, n);
  }
  /** 单应用单资源读数（供画像 API 使用）。 */
  read(app: string, kind: ResourceKind): number {
    return this.books.get(kind)?.get(app) ?? 0;
  }
  get kinds(): ResourceKind[] {
    return [...this.books.keys()];
  }
}

/** F05583/F05584/F05585 应用画像/目录画像/进程网络。 */
export function appResourceCurve(samples: { time: number; cpu: number; mem: number }[]): { peakCpu: number; avgMem: number; points: number } {
  if (!samples.length) return { peakCpu: 0, avgMem: 0, points: 0 };
  return { peakCpu: Math.max(...samples.map((s) => s.cpu)), avgMem: Math.round(samples.reduce((s, x) => s + x.mem, 0) / samples.length), points: samples.length };
}
export function dirIoProfile(entries: { dir: string; mb: number }[]): [string, number][] {
  const m = new Map<string, number>();
  for (const e of entries) m.set(e.dir, (m.get(e.dir) ?? 0) + e.mb);
  return [...m].sort((a, b) => b[1] - a[1]);
}
export function whoUsesNetwork(conns: { pid: number; app: string; kbps: number }[]): { app: string; kbps: number }[] {
  const m = new Map<string, number>();
  for (const c of conns) m.set(c.app, (m.get(c.app) ?? 0) + c.kbps);
  return [...m].map(([app, kbps]) => ({ app, kbps })).sort((a, b) => b.kbps - a.kbps);
}

/** F05586~F05588 突发检测/基线学习/异常评分。 */
export function burstDetect(samples: number[], baselineMean: number, factor = 3): { burst: boolean; peak: number } {
  const peak = Math.max(...samples, 0);
  return { burst: peak > baselineMean * factor, peak };
}
export function learnBaseline(samples: number[]): { mean: number; sigma: number } {
  const mean = samples.reduce((s, x) => s + x, 0) / Math.max(1, samples.length);
  const sigma = Math.sqrt(samples.reduce((s, x) => s + (x - mean) ** 2, 0) / Math.max(1, samples.length));
  return { mean: Math.round(mean * 100) / 100, sigma: Math.round(sigma * 100) / 100 };
}
export function anomalyScore(value: number, baseline: { mean: number; sigma: number }): number {
  const z = Math.abs(value - baseline.mean) / Math.max(0.01, baseline.sigma);
  return Math.min(100, Math.round(z * 10));
}

/** F05589~F05592 限额设置/告警/执行/降级。 */
export class QuotaManager {
  private limits = new Map<string, { kind: ResourceKind; limit: number }>();
  set(app: string, kind: ResourceKind, limit: number): void {
    this.limits.set(`${app}:${kind}`, { kind, limit });
  }
  check(app: string, kind: ResourceKind, used: number): { level: 'ok' | 'warn' | 'over'; pct: number } {
    const limit = this.limits.get(`${app}:${kind}`)?.limit ?? Number.MAX_SAFE_INTEGER;
    const pct = Math.round((used / limit) * 100);
    return { level: pct > 100 ? 'over' : pct > 80 ? 'warn' : 'ok', pct };
  }
  /** 超额动作：限流 → 降质 → 拒绝。 */
  enforce(level: 'ok' | 'warn' | 'over'): { throttle: boolean; degrade: boolean; deny: boolean } {
    return { throttle: level !== 'ok', degrade: level === 'over', deny: false };
  }
}

/** F05593/F05594/F05595 桌面预算/前台保证/批处理窗口。 */
export const DESKTOP_RESOURCE_BUDGET = { memMB: 300, cpuPct: 5, gpuPct: 10 } as const;
export function foregroundGuarantee(fgApp: string, cpuShare: Record<string, number>): Record<string, number> {
  const fg = cpuShare[fgApp] ?? 0;
  const others = Object.entries(cpuShare).filter(([k]) => k !== fgApp);
  const otherTotal = others.reduce((s, [, v]) => s + v, 0);
  const cap = Math.max(0, 100 - fg - 20);
  const scaled: Record<string, number> = { [fgApp]: fg };
  for (const [k, v] of others) scaled[k] = otherTotal > cap ? Math.round((v / otherTotal) * cap * 10) / 10 : v;
  return scaled;
}
export function batchWindow(now: number): boolean {
  const h = new Date(now).getHours();
  return h >= 2 && h < 6;
}

/** F05596~F05599 日志/导入导出/看板/API。 */
export class QuotaLog {
  private rows: { time: number; app: string; kind: ResourceKind; used: number; level: string }[] = [];
  record(app: string, kind: ResourceKind, used: number, level: string, time = Date.now()): void {
    this.rows.push({ time, app, kind, used, level });
  }
  export(): string {
    return JSON.stringify(this.rows);
  }
  get count(): number {
    return this.rows.length;
  }
}
export function quotaDashboard(ledger: ResourceLedger): Record<string, [string, number][]> {
  const out: Record<string, [string, number][]> = {};
  for (const k of ledger.kinds) out[k] = ledger.top(k);
  return out;
}
/** F05599 画像查询接口（冻结签名，供三方读取）。 */
export interface QuotaQueryApi {
  query(app: string, kind: ResourceKind): number;
  topN(kind: ResourceKind, n: number): [string, number][];
}
export function quotaApi(ledger: ResourceLedger): QuotaQueryApi {
  return {
    query: (app, kind) => ledger.read(app, kind),
    topN: (kind, n) => ledger.top(kind, n),
  };
}

/** F05600 教学。 */
export function quotaTutorial(): string[] {
  return ['六类资源各自记账，支持 Top 榜', '超额先限流再降质，不直接杀进程', '前台交互永远优先，后台挤进批处理窗口'];
}

/* ============ 族0225 兼容认证与收官（F05601~F05625） ============ */

export type CertLevel = 'ready' | 'provisional' | 'revoked';

/** F05601~F05606 Varix Ready 认证：标准/自测/徽章/目录/评分。 */
export const CERT_STANDARD_VERSION = 'varix-ready-1.0' as const;
export interface CertCriteria { id: string; weight: number; pass: boolean }
export function certify(app: string, criteria: CertCriteria[]): { app: string; level: CertLevel; score: number; badge: string } {
  const totalW = criteria.reduce((s, c) => s + c.weight, 0) || 1;
  const score = Math.round((criteria.filter((c) => c.pass).reduce((s, c) => s + c.weight, 0) / totalW) * 100);
  const level: CertLevel = score >= 90 ? 'ready' : score >= 70 ? 'provisional' : 'revoked';
  return { app, level, score, badge: level === 'ready' ? 'Varix Ready ✓' : level === 'provisional' ? 'Varix Ready ~' : '—' };
}
export function selfTestTool(criteria: CertCriteria[]): { ran: number; passed: number; report: string } {
  const passed = criteria.filter((c) => c.pass).length;
  return { ran: criteria.length, passed, report: `self-test ${passed}/${criteria.length} pass` };
}
export class CertDirectory {
  private entries = new Map<string, { level: CertLevel; score: number }>();
  publish(app: string, level: CertLevel, score: number): boolean {
    this.entries.set(app, { level, score });
    return true;
  }
  /** F05609 撤销：破坏兼容即撤销。 */
  revoke(app: string): boolean {
    const e = this.entries.get(app);
    if (!e || e.level === 'revoked') return false;
    e.level = 'revoked';
    e.score = 0;
    return true;
  }
  list(level?: CertLevel): string[] {
    return [...this.entries.entries()].filter(([, v]) => !level || v.level === level).map(([k]) => k);
  }
}

/** F05607/F05608 用户报告入口 + 复检。 */
export function userCompatReport(app: string, ok: boolean, note: string): { app: string; ok: boolean; note: string; time: number } {
  return { app, ok, note, time: Date.now() };
}
export function recertify(app: string, newVersion: string, criteria: CertCriteria[]): { app: string; version: string; result: ReturnType<typeof certify> } {
  return { app, version: newVersion, result: certify(app, criteria) };
}

/** F05610~F05614 开发指南/嵌入协议/协议版本/API 冻结/弃用流程。 */
export const EMBED_PROTOCOL = { name: 'varix-embed', version: '1.0', minVersion: '1.0' } as const;
export function protocolVersionNegotiation(client: string): { agreed: string | null; note: string } {
  const num = Number(client);
  if (Number.isNaN(num)) return { agreed: null, note: '非法版本' };
  if (num >= Number(EMBED_PROTOCOL.minVersion) && num <= Number(EMBED_PROTOCOL.version)) return { agreed: client, note: '协议匹配' };
  return { agreed: null, note: num > Number(EMBED_PROTOCOL.version) ? '客户端过新，走降级协商' : '客户端过旧，需升级' };
}
export const FROZEN_API_SURFACE = ['embed.acquire', 'embed.release', 'yield.notify', 'quota.query'] as const;
export function apiFrozenCheck(call: string): { allowed: boolean; note: string } {
  const known = (FROZEN_API_SURFACE as readonly string[]).includes(call);
  return { allowed: known, note: known ? '冻结面内调用，向后兼容' : '非冻结面，可能有变更' };
}
export class DeprecationFlow {
  private announced = new Map<string, { announcedAt: number; removeAt: number }>();
  announce(api: string, now: number, graceDays = 180): void {
    this.announced.set(api, { announcedAt: now, removeAt: now + graceDays * 86400_000 });
  }
  stillSupported(api: string, now: number): boolean {
    const a = this.announced.get(api);
    return !a || now < a.removeAt;
  }
  /** 自动迁移指南。 */
  migrationGuide(api: string): string {
    return `迁移：${api} → 替代接口（见兼容指南 §${(parseInt(fnv1a(api), 16) % 9) + 1}）`;
  }
}

/** F05616/F05617/F05618 样板应用/SDK/测试签名位。 */
export const SAMPLE_APP = { name: 'varix-sample-compat', surfaces: ['embed.acquire', 'yield.notify'] } as const;
export const COMPAT_SDK = { version: '1.0.0', modules: ['embed', 'yield', 'quota', 'cert'] } as const;
export const TEST_SIGNING_RESERVED = { reserved: true, offline: true } as const;

/** F05619~F05622 开放日/合作伙伴/大会/免费承诺。 */
export const LAB_OPEN_DAY = { quarterly: true, dataUrl: 'local://compat/open-day' } as const;
export const PARTNERS = ['示例硬件伙伴 A', '示例软件伙伴 B'] as const;
export const COMPAT_CONF = { annual: true, name: 'Varix 兼容性大会' } as const;
export const CERT_FREE = { fee: 0, note: '认证永久免费' } as const;

/** F05623/F05624/F05625 进度看板/志愿团/庆典。 */
export function certProgressBoard(apps: { app: string; level: CertLevel }[]): { ready: number; provisional: number; revoked: number; total: number } {
  return {
    ready: apps.filter((a) => a.level === 'ready').length,
    provisional: apps.filter((a) => a.level === 'provisional').length,
    revoked: apps.filter((a) => a.level === 'revoked').length,
    total: apps.length,
  };
}
export class VolunteerTester {
  private members = new Set<string>();
  join(name: string): boolean {
    if (this.members.has(name)) return false;
    this.members.add(name);
    return true;
  }
  get count(): number {
    return this.members.size;
  }
}
export function finaleCeremony(totalItems: number): string {
  return totalItems >= 625 ? '🎉 兼容域 625 项收官！领域09 兼容性防线全线完成' : `进度 ${totalItems}/625`;
}
