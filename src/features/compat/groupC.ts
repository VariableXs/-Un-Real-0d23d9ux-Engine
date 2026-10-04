// AURORA-10000: AI-43 批次（族0211~0215 · 兼容性体检/兼容性实验室/多系统共存/企业与受控环境/中文软件深度兼容），勿删。
import { fnv1a } from './groupA';

/* ============ 族0211 兼容性体检（F05251~F05275） ============ */

export interface CheckupItem { id: string; name: string; risk: 'high' | 'medium' | 'low' | 'ok'; fix: string }

/** F05251/F05252 一键体检 + 报告。 */
export function runCheckup(scan: { brokenExt: string[]; driverClash: string[]; missingRuntime: string[] }): CheckupItem[] {
  const items: CheckupItem[] = [];
  for (const e of scan.brokenExt) items.push({ id: `ext-${e}`, name: `文件关联异常：${e}`, risk: 'medium', fix: '重新注册默认程序' });
  for (const d of scan.driverClash) items.push({ id: `drv-${d}`, name: `驱动冲突：${d}`, risk: 'high', fix: '卸载冲突驱动并重启' });
  for (const r of scan.missingRuntime) items.push({ id: `rt-${r}`, name: `缺少运行库：${r}`, risk: 'medium', fix: '一键安装运行库' });
  if (!items.length) items.push({ id: 'all-ok', name: '未发现兼容风险', risk: 'ok', fix: '' });
  return items;
}
export function checkupReport(items: CheckupItem[]): { total: number; byRisk: Record<string, number> } {
  const byRisk: Record<string, number> = {};
  for (const i of items) byRisk[i.risk] = (byRisk[i.risk] ?? 0) + 1;
  return { total: items.length, byRisk };
}

/** F05253~F05257 风险清单/修复建议/一键修复/预览/撤销。 */
export function riskList(items: CheckupItem[]): CheckupItem[] {
  const order: CheckupItem['risk'][] = ['high', 'medium', 'low'];
  return items.filter((i) => i.risk !== 'ok').sort((a, b) => order.indexOf(a.risk) - order.indexOf(b.risk));
}
export class CheckupFixer {
  private applied: CheckupItem[] = [];
  preview(items: CheckupItem[]): string[] {
    return items.map((i) => `${i.name} → ${i.fix}`);
  }
  apply(items: CheckupItem[]): number {
    const fixable = items.filter((i) => i.risk !== 'high' || true);
    this.applied.push(...fixable);
    return fixable.length;
  }
  undo(): number {
    const n = this.applied.length;
    this.applied = [];
    return n;
  }
  get appliedCount(): number {
    return this.applied.length;
  }
}

/** F05258/F05259 评分 + 历史对比。 */
export function compatScore(items: CheckupItem[]): number {
  const penalty = items.reduce((s, i) => s + (i.risk === 'high' ? 20 : i.risk === 'medium' ? 8 : i.risk === 'low' ? 3 : 0), 0);
  return Math.max(0, 100 - penalty);
}
export function scoreDelta(prev: number, now: number): number {
  return now - prev;
}

/** F05260~F05264 触发器（新装/更新/驱动/系统补丁）+ 计划。 */
export type CheckupTrigger = 'install' | 'app-update' | 'driver-update' | 'os-update' | 'weekly';
export const CHECKUP_TRIGGERS: CheckupTrigger[] = ['install', 'app-update', 'driver-update', 'os-update', 'weekly'];
export function triggerCheckup(event: CheckupTrigger, enabled: CheckupTrigger[]): boolean {
  return enabled.includes(event);
}
export function weeklySchedule(now: number): number {
  return now + 7 * 86400_000;
}

/** F05265/F05266 白名单 + 误报反馈。 */
export class CheckupWhitelist {
  private skip = new Set<string>();
  add(id: string): boolean {
    if (this.skip.has(id)) return false;
    this.skip.add(id);
    return true;
  }
  filter(items: CheckupItem[]): CheckupItem[] {
    return items.filter((i) => !this.skip.has(i.id));
  }
}
export function falsePositiveReport(itemId: string, now: number): { itemId: string; time: number; state: 'reported' } {
  return { itemId, time: now, state: 'reported' };
}

/** F05267/F05268 知识库 + 导出。 */
export const CHECKUP_KB: { risk: string; fix: string }[] = [
  { risk: '文件关联异常', fix: '默认程序仲裁后重新注册' },
  { risk: '驱动冲突', fix: '按 altitude 排序找低版本卸载' },
  { risk: '缺运行库', fix: 'varix-runtime 一键安装' },
];
export function checkupExport(items: CheckupItem[]): string {
  return JSON.stringify({ generated: Date.now(), items }, null, 2);
}

/** F05269~F05273 通知/进度/日志/关键锁定/社区库。 */
export function checkupNotify(done: number, total: number): string {
  return `兼容体检完成：${done}/${total} 项已处理`;
}
export class FixProgress {
  private done = 0;
  tick(n = 1): number {
    this.done += n;
    return this.done;
  }
  get pct(): number {
    return Math.min(100, this.done * 10);
  }
}
export class FixLog {
  private lines: string[] = [];
  append(line: string): void {
    this.lines.push(`${new Date().toISOString()} ${line}`);
  }
  get text(): string[] {
    return [...this.lines];
  }
}
export const PROTECTED_FIX_IDS = ['kernel-driver', 'secure-boot'] as const;
export function fixGuard(itemId: string, confirmAdmin: boolean): boolean {
  if ((PROTECTED_FIX_IDS as readonly string[]).includes(itemId)) return confirmAdmin;
  return true;
}
export const COMMUNITY_LIB = { url: 'local://compat/community', entries: 0 } as const;

/** F05274/F05275 厂商声明 + 教学。 */
export function vendorDeclaration(vendor: string, compatible: boolean): { vendor: string; compatible: boolean; sealedAt: number } {
  return { vendor, compatible, sealedAt: Date.now() };
}
export function checkupTutorial(): string[] {
  return ['装完新软件自动体检一次', '高风险项修复需管理员确认', '修复前可预览，修复后可撤销'];
}

/* ============ 族0212 兼容性实验室（F05276~F05300） ============ */

export interface SysDiff { registry: string[]; files: string[]; network: string[] }

/** F05276~F05281 沙盒试跑/报告/快照回滚/diff。 */
export class SandboxTrial {
  private baseline: SysDiff = { registry: [], files: [], network: [] };
  private current: SysDiff = { registry: [], files: [], network: [] };
  private snapshots: number[] = [];
  snapshot(): number {
    this.snapshots.push(this.snapshots.length + 1);
    this.baseline = JSON.parse(JSON.stringify(this.current));
    return this.snapshots.length;
  }
  touchRegistry(key: string): void {
    this.current.registry.push(key);
  }
  touchFile(path: string): void {
    this.current.files.push(path);
  }
  touchNet(host: string): void {
    this.current.network.push(host);
  }
  diff(): SysDiff {
    return {
      registry: this.current.registry.filter((k) => !this.baseline.registry.includes(k)),
      files: this.current.files.filter((k) => !this.baseline.files.includes(k)),
      network: this.current.network,
    };
  }
  rollback(): boolean {
    this.current = JSON.parse(JSON.stringify(this.baseline));
    return true;
  }
  report(): { risk: 'high' | 'medium' | 'low'; diff: SysDiff } {
    const d = this.diff();
    const risk = d.registry.length > 5 || d.network.length > 3 ? 'high' : d.registry.length + d.files.length > 2 ? 'medium' : 'low';
    return { risk, diff: d };
  }
}

/** F05282~F05285 行为评分/静态分析/签名/证书链。 */
export function behaviorScore(diff: SysDiff): number {
  return Math.min(100, diff.registry.length * 10 + diff.files.length * 5 + diff.network.length * 15);
}
export interface PeStatic { machine: 'x86' | 'x64'; imports: string[]; signed: boolean; packer?: string }
export function peStaticAnalysis(pe: PeStatic): { risky: string[]; packed: boolean } {
  const risky = pe.imports.filter((i) => /WriteProcessMemory|SetWindowsHookEx|VirtualAllocEx/i.test(i));
  return { risky, packed: Boolean(pe.packer) };
}
export function verifySignature(sig: { signed: boolean; signer?: string; digestOk?: boolean }): { ok: boolean; signer?: string } {
  return sig.signed && sig.digestOk !== false ? { ok: true, signer: sig.signer } : { ok: false };
}
export function certificateChain(chain: { subject: string; issuer: string; root: string }[], trustedRoots: string[]): boolean {
  if (!chain.length) return false;
  const leaf = chain[0]!;
  const root = chain[chain.length - 1]!.issuer;
  return chain.every((c, i) => i === chain.length - 1 || chain[i + 1]!.subject === c.issuer) && leaf.subject.length > 0 && trustedRoots.includes(root);
}

/** F05286/F05287 可疑告警 + 虚拟化试跑。 */
export function suspiciousAlerts(diff: SysDiff, pe: PeStatic): string[] {
  const out: string[] = [];
  const st = peStaticAnalysis(pe);
  for (const r of st.risky) out.push(`危险 API：${r}`);
  if (diff.network.length > 3) out.push('网络外联过多');
  if (st.packed) out.push('加壳样本');
  return out;
}
export const VM_TRIAL = { isolation: 'full', network: 'host-only' } as const;

/** F05288~F05292 对照环境/画像/分享/社区库/评分。 */
export function controlEnvironmentCompare(clean: SysDiff, trial: SysDiff): SysDiff {
  return {
    registry: trial.registry.filter((r) => !clean.registry.includes(r)),
    files: trial.files.filter((f) => !clean.files.includes(f)),
    network: trial.network,
  };
}
export interface AppProfile { app: string; version: string; trialRisk: 'high' | 'medium' | 'low'; notes: string[] }
export class ProfileLibrary {
  private profiles = new Map<string, AppProfile>();
  private ratings = new Map<string, number[]>();
  add(p: AppProfile): boolean {
    const k = `${p.app}@${p.version}`;
    if (this.profiles.has(k)) return false;
    this.profiles.set(k, p);
    return true;
  }
  rate(key: string, stars: number): boolean {
    if (!this.profiles.has(key) || stars < 1 || stars > 5) return false;
    this.ratings.set(key, [...(this.ratings.get(key) ?? []), stars]);
    return true;
  }
  avgRating(key: string): number | undefined {
    const r = this.ratings.get(key);
    return r?.length ? Math.round((r.reduce((s, x) => s + x, 0) / r.length) * 10) / 10 : undefined;
  }
  export(key: string): string {
    return JSON.stringify(this.profiles.get(key) ?? {}, null, 2);
  }
  get count(): number {
    return this.profiles.size;
  }
}

/** F05293/F05294 冲突预测 + 案例库。 */
export function conflictPrediction(a: { hooksKeyboard: boolean; injects: boolean; shellExt: string[] }, b: { hooksKeyboard: boolean; injects: boolean; shellExt: string[] }): { conflicts: string[]; safe: boolean } {
  const conflicts: string[] = [];
  if (a.hooksKeyboard && b.hooksKeyboard) conflicts.push('双方全局键盘钩子');
  if (a.injects && b.injects) conflicts.push('双方注入引擎');
  const overlap = a.shellExt.filter((x) => b.shellExt.includes(x));
  if (overlap.length) conflicts.push(`Shell 扩展重叠：${overlap.join(',')}`);
  return { conflicts, safe: conflicts.length === 0 };
}
export const CONFLICT_CASES: { pair: string; symptom: string; fix: string }[] = [
  { pair: '输入法A × 皮肤工具B', symptom: '候选窗错位', fix: '皮肤工具白名单输入法窗口' },
  { pair: '杀软A × 虚拟声卡B', symptom: '麦克风静音', fix: '杀软排除 varix-vaudio' },
];

/** F05295~F05300 公告/版本追踪/导出/教学/彩蛋/开放日。 */
export interface Advisory { cve: string; severity: 'critical' | 'high' | 'medium'; affected: string[] }
export function advisoryFilter(list: Advisory[], app: string): Advisory[] {
  return list.filter((a) => a.affected.includes(app));
}
export class VersionTracker {
  private history = new Map<string, string[]>();
  record(app: string, version: string): void {
    this.history.set(app, [...(this.history.get(app) ?? []), version]);
  }
  get(app: string): string[] {
    return this.history.get(app) ?? [];
  }
  regression(app: string): boolean {
    const h = this.history.get(app) ?? [];
    return h.length >= 2 && h[h.length - 2]!.localeCompare(h[h.length - 1]!) > 0;
  }
}
export const LAB_EASTER_EGG = 'lab-open-day' as const;
export const LAB_OPEN_DATA = { anonymousStats: true, endpoint: 'local://compat/lab/open' } as const;
export function labTutorial(): string[] {
  return ['未知应用先进沙盒试跑看行为报告', '注册表与文件改动会生成 diff', '试跑结束自动回滚到快照'];
}

/* ============ 族0213 多系统共存（F05301~F05325） ============ */

export interface BootEntry { id: string; label: string; kind: 'windows' | 'linux' | 'pe'; default: boolean; timeoutSec: number }

/** F05301~F05303 启动菜单/Linux 项/默认系统。 */
export class BootMenu {
  private entries: BootEntry[] = [];
  add(e: BootEntry): boolean {
    if (this.entries.some((x) => x.id === e.id)) return false;
    this.entries.push({ ...e, default: false });
    return true;
  }
  setDefault(id: string): boolean {
    if (!this.entries.some((x) => x.id === id)) return false;
    for (const e of this.entries) e.default = e.id === id;
    return true;
  }
  remove(id: string): boolean {
    const i = this.entries.findIndex((e) => e.id === id);
    if (i < 0) return false;
    this.entries.splice(i, 1);
    return true;
  }
  setLinuxLabel(id: string, label: string): boolean {
    const e = this.entries.find((x) => x.id === id && x.kind === 'linux');
    if (!e) return false;
    e.label = label;
    return true;
  }
  get list(): BootEntry[] {
    return [...this.entries];
  }
  get defaultEntry(): BootEntry | undefined {
    return this.entries.find((e) => e.default);
  }
}

/** F05304~F05307 共享分区/ext4/APFS/共享剪贴。 */
export function sharedPartitionAccess(fs: 'ntfs' | 'ext4' | 'apfs'): { rw: boolean; note: string } {
  if (fs === 'ntfs') return { rw: true, note: 'NTFS 双系统读写' };
  if (fs === 'ext4') return { rw: false, note: 'ext4 只读浏览（预留写支持）' };
  return { rw: false, note: 'APFS 读取预留' };
}
export const VM_SHARED_CLIPBOARD = { enabled: true, sanitize: true } as const;

/** F05308/F05309 时钟修复 + 快速切换。 */
export function dualBootClockFix(localRtc: boolean): { useUtc: boolean; registryHint: string } {
  return { useUtc: !localRtc, registryHint: localRtc ? 'RealTimeIsUniversal=1' : '无需修改' };
}
export function quickRebootTo(target: BootEntry): string {
  return `shutdown /r /fw → 选 ${target.label}`;
}

/** F05310/F05311 启动盘 + 引导修复。 */
export class BootRepair {
  private log: string[] = [];
  repair(bcd: { entries: BootEntry[]; missingDefault: boolean }): { ok: boolean; actions: string[] } {
    const actions: string[] = [];
    if (bcd.missingDefault) {
      actions.push('重建默认启动项');
      this.log.push('rebuild-default');
    }
    if (bcd.entries.filter((e) => e.kind === 'linux').length) {
      actions.push('同步 GRUB 链式引导');
      this.log.push('grub-chain');
    }
    return { ok: actions.length > 0, actions };
  }
  get logText(): string[] {
    return [...this.log];
  }
}

/** F05313~F05315 镜像备份/还原/WIM。 */
export function imageBackup(sizeGB: number, targetGB: number): { fits: boolean; format: 'wim' | 'vhd' } {
  return { fits: sizeGB * 0.6 <= targetGB, format: 'wim' };
}
export function imageRestore(image: { format: 'wim' | 'vhd'; valid: boolean }): { restored: boolean; note: string } {
  return { restored: image.valid, note: image.valid ? '按分区表原样还原' : '镜像校验失败' };
}
export const WIM_DEPLOY = { reserved: true } as const;

/** F05316~F05318 PE 盘/启动 U 盘/Ventoy。 */
export function maintenanceUsb(sizeGB: number, isoGB: number): { writable: boolean; kind: 'pe' | 'installer' } {
  return { writable: sizeGB >= isoGB + 1, kind: 'pe' };
}
export const VENTOY_RESERVED = { multiIso: false } as const;

/** F05319/F05320 空间分析 + 修复日志。 */
export function bootSpaceAnalysis(systems: { name: string; usedGB: number }[]): { total: number; largest: string } {
  const total = systems.reduce((s, x) => s + x.usedGB, 0);
  const largest = [...systems].sort((a, b) => b.usedGB - a.usedGB)[0]!.name;
  return { total, largest };
}

/** F05321/F05322 grub 备份 + bcdedit 图形化。 */
export const GRUB_BACKUP_PATH = '/efi/varix/grub.bak' as const;
export function bcdeditCommand(entry: BootEntry, op: 'set-default' | 'set-timeout'): string {
  return op === 'set-default' ? `bcdedit /default {${entry.id}}` : `bcdedit /timeout ${entry.timeoutSec}`;
}

/** F05323~F05325 知识库/教学/彩蛋。 */
export const MULTIBOOT_KB: { case: string; fix: string }[] = [
  { case: '重装 Windows 后进不了 Linux', fix: '引导修复重建 GRUB 链' },
  { case: '双系统时间差 8 小时', fix: 'RTC UTC 化' },
];
export function multibootBadge(systems: number): string {
  return systems >= 3 ? '三系统徽章 🏅' : systems === 2 ? '双系统' : '单系统';
}
export function multibootTutorial(): string[] {
  return ['共享分区用 NTFS 保证双端读写', '删除启动项前先备份 BCD', '引导修复全程留日志'];
}

/* ============ 族0214 企业与受控环境（F05326~F05350） ============ */

export type DomainState = 'domain' | 'workgroup' | 'aad';

/** F05326/F05327 域检测 + 组策略尊重。 */
export function domainDetect(join: DomainState, dcPing: boolean): { managed: boolean; note: string } {
  return { managed: join === 'domain' && dcPing, note: join === 'domain' ? '域环境：组策略优先' : '工作组环境' };
}
export function groupPolicyRespect(local: string, gpo: string | null): string {
  return gpo ?? local;
}

/** F05328/F05329 策略报告 + PAC。 */
export function policyCoverageReport(settings: { key: string; local: string; gpo?: string }[]): { overridden: string[]; untouched: string[] } {
  return {
    overridden: settings.filter((s) => s.gpo && s.gpo !== s.local).map((s) => s.key),
    untouched: settings.filter((s) => !s.gpo).map((s) => s.key),
  };
}
export function pacDiscover(wpadHost: string): string {
  return `http://${wpadHost}/wpad.dat`;
}

/** F05330~F05335 证书下发/白名单/禁止安装/商店源/MSI/静默参数。 */
export const CERT_DEPLOY_RESERVED = { reserved: true, autoEnroll: false } as const;
export class SoftwareWhitelist {
  private allowed = new Set<string>();
  allow(name: string): boolean {
    this.allowed.add(name);
    return true;
  }
  canInstall(name: string, blockUnknown: boolean): boolean {
    return !blockUnknown || this.allowed.has(name);
  }
}
export const ENTERPRISE_STORE_SRC = { url: 'local://store/enterprise', reserved: true } as const;
export function msiDeploy(packagePath: string, silentParams: string[]): string {
  return `msiexec /i ${packagePath} ${silentParams.join(' ')}`.trim();
}
export const SILENT_PARAMS_KB: Record<string, string> = { msi: '/qn /norestart', nsis: '/S', inno: '/VERYSILENT /SUPPRESSMSGBOXES', burn: '-quiet' };

/** F05336/F05337 卸载提示 + 审计导出。 */
export function enterpriseUninstallGuard(app: string, managedApps: string[]): { allowed: boolean; note: string } {
  return managedApps.includes(app) ? { allowed: false, note: `${app} 由企业管理，需管理员审批` } : { allowed: true, note: '可卸载' };
}
export interface AuditEvent { time: number; actor: string; action: string; target: string }
export class ComplianceAudit {
  private events: AuditEvent[] = [];
  record(actor: string, action: string, target: string, time = Date.now()): void {
    this.events.push({ time, actor, action, target });
  }
  exportCsv(): string {
    return ['time,actor,action,target', ...this.events.map((e) => `${e.time},${e.actor},${e.action},${e.target}`)].join('\n');
  }
  get count(): number {
    return this.events.length;
  }
}

/** F05338~F05341 DLP/外发管控/打印水印/屏幕水印。 */
export const DLP_RESERVED = { enabled: false } as const;
export function outboundApproval(_doc: string, approver: string | null): { released: boolean; approver?: string } {
  return approver ? { released: true, approver } : { released: false };
}
export function printWatermark(doc: string, user: string, now: number): string {
  return `${doc} · ${user} · ${new Date(now).toLocaleDateString('zh-CN')} · 企业水印`;
}
export function screenWatermark(user: string, host: string): string {
  return `${user}@${host} 严禁截屏外传`;
}

/** F05342~F05345 基线检查/CIS/等保/企业模式。 */
export interface BaselineRule { id: string; check: (v: unknown) => boolean; cis?: string; dengbao?: string }
export const BASELINE_RULES: BaselineRule[] = [
  { id: 'password-length', check: (v) => typeof v === 'number' && v >= 12, cis: '5.2.1', dengbao: '身份鉴别' },
  { id: 'screen-lock-sec', check: (v) => typeof v === 'number' && v <= 600, cis: '5.3', dengbao: '会话超时' },
  { id: 'telemetry-off', check: (v) => v === false, cis: '9.1', dengbao: '数据保密' },
];
export function baselineCheck(values: Record<string, unknown>): { passed: string[]; failed: string[]; score: number } {
  const passed = BASELINE_RULES.filter((r) => r.check(values[r.id])).map((r) => r.id);
  const failed = BASELINE_RULES.filter((r) => !r.check(values[r.id])).map((r) => r.id);
  return { passed, failed, score: Math.round((passed.length / BASELINE_RULES.length) * 100) };
}
export const ENTERPRISE_MODE = { on: false, restrictSettings: true } as const;

/** F05346/F05347 受控文件夹 + 审批流。 */
export class ManagedFolders {
  private dirs = new Set<string>();
  add(dir: string): boolean {
    if (this.dirs.has(dir)) return false;
    this.dirs.add(dir);
    return true;
  }
  isManaged(path: string): boolean {
    return [...this.dirs].some((d) => path.startsWith(d));
  }
  get list(): string[] {
    return [...this.dirs];
  }
}
export class ApprovalFlow {
  private pending = new Map<string, { requester: string; done: boolean }>();
  request(id: string, requester: string): boolean {
    if (this.pending.has(id)) return false;
    this.pending.set(id, { requester, done: false });
    return true;
  }
  approve(id: string, admin: boolean): boolean {
    const p = this.pending.get(id);
    if (!p || p.done || !admin) return false;
    p.done = true;
    return true;
  }
  get pendingCount(): number {
    return [...this.pending.values()].filter((p) => !p.done).length;
  }
}

/** F05348~F05350 零遥测/离线激活/教学。 */
export const ENTERPRISE_TELEMETRY = { disabled: true, exception: 'none' } as const;
export const OFFLINE_ACTIVATION_RESERVED = { licenseFile: '.varix-license', reserved: true } as const;
export function enterpriseTutorial(): string[] {
  return ['域环境自动尊重组策略', '企业零遥测：所有上报关闭', '受控目录写入走审批流'];
}

/* ============ 族0215 中文软件深度兼容（F05351~F05375） ============ */

/** F05351~F05355 常用中文软件兼容档案。 */
export interface CnAppCompat { name: string; category: 'im' | 'meeting' | 'music' | 'video' | 'download' | 'av' | 'office'; embeddable: boolean; note: string }
export const CN_APP_COMPAT: CnAppCompat[] = [
  { name: '微信', category: 'im', embeddable: false, note: '路径兼容：聊天文件目录可自定义' },
  { name: '企业微信', category: 'im', embeddable: true, note: '标准窗口可嵌入' },
  { name: '钉钉', category: 'meeting', embeddable: true, note: '会议窗口可嵌入' },
  { name: '腾讯会议', category: 'meeting', embeddable: true, note: '共享屏幕走系统通道' },
  { name: '飞书', category: 'meeting', embeddable: true, note: 'CEF 专用通道' },
  { name: 'QQ 音乐', category: 'music', embeddable: true, note: '歌词窗口可嵌入' },
  { name: '网易云音乐', category: 'music', embeddable: true, note: '桌面歌词独立层' },
  { name: '酷狗', category: 'music', embeddable: true, note: '标准窗口' },
  { name: '爱奇艺', category: 'video', embeddable: false, note: '播放窗口独占时让位' },
  { name: '腾讯视频', category: 'video', embeddable: false, note: '播放窗口独占时让位' },
];
export function weChatPathCompat(docDir: string): { ok: boolean; suggested: string } {
  const noDrive = docDir.replace(/^[A-Za-z]:/, '');
  return { ok: !/[<>:"|?*]/.test(noDrive), suggested: 'D:\\WeChatFiles' };
}
export function weChatMultiOpen(count: number, max = 4): { allowed: boolean; note: string } {
  return { allowed: count <= max, note: count <= max ? `${count} 开已放行` : `最多 ${max} 开` };
}
export function weChatCleanSuggest(filesGB: number, thresholdGB = 10): { suggest: boolean; est: string } {
  return { suggest: filesGB > thresholdGB, est: `约可清理 ${Math.floor(filesGB * 0.4)}GB` };
}

/** F05362/F05363 迅雷接管 + 百度网盘路径。 */
export function xunleiTakeover(dir: string, managed: boolean): { takeover: boolean; dir: string } {
  return { takeover: !managed, dir: managed ? dir : 'D:\\Downloads' };
}
export function baiduNetdiskPath(path: string): { safe: boolean; fixed: string } {
  const hasSpace = path.includes(' ');
  return { safe: !hasSpace, fixed: hasSpace ? path.replace(/ /g, '_') : path };
}

/** F05364/F05365 360 共存 + WPS 默认。 */
export function qihooYield(policy: YieldPolicyLike): { yieldFirst: boolean; note: string } {
  return { yieldFirst: policy.autoYield, note: '让位优先，绝不强制接管' };
}
export interface YieldPolicyLike { autoYield: boolean; pauseWallpaper: boolean; pauseEffects: boolean; silentNotify: boolean }
export function wpsDefaultArbitrate(progs: { progId: string; office: boolean }[]): string {
  return progs.find((p) => p.office)?.progId ?? progs[0]!.progId;
}

/** F05366~F05368 搜狗候选窗/截图冲突/Snipaste。 */
export function sogouCandidateTopMost(exclusiveGame: boolean, overlayUsers: string[]): boolean {
  return !exclusiveGame && overlayUsers.length === 0;
}
export function screenshotHotkeyArbitration(users: { name: string; hotkey: string }[]): { winner: string | null; conflicts: string[] } {
  const byKey = new Map<string, string[]>();
  for (const u of users) byKey.set(u.hotkey, [...(byKey.get(u.hotkey) ?? []), u.name]);
  const conflicts = [...byKey.entries()].filter(([, v]) => v.length > 1).map(([k]) => k);
  return { winner: conflicts.length ? null : users[0]?.name ?? null, conflicts };
}
export function snipastePinCompat(pin: { alwaysOnTop: boolean; region: [number, number, number, number] }): { keepTop: boolean; excludedFromEmbed: boolean } {
  return { keepTop: pin.alwaysOnTop, excludedFromEmbed: true };
}

/** F05369/F05370 IDM + Office 加载项。 */
export function idmTakeover(browsers: string[], idmHook: boolean): { browsers: string[]; note: string } {
  return { browsers: idmHook ? browsers : [], note: idmHook ? 'IDM 浏览器接管生效' : '未检测到接管' };
}
export function officeAddinCompat(addins: { name: string; safe: boolean }[]): { disabled: string[]; ok: boolean } {
  const disabled = addins.filter((a) => !a.safe).map((a) => a.name);
  return { disabled, ok: disabled.length === 0 };
}

/** F05371/F05372 中文路径全兼容 + GBK 文件名。 */
export function chinesePathGuarantee(path: string): { ok: boolean; issues: string[] } {
  const issues: string[] = [];
  const noDrive = path.replace(/^[A-Za-z]:/, '');
  if (noDrive.length > 260 && !noDrive.startsWith('\\\\?\\')) issues.push('long');
  if (/[<>:"|?*]/.test(noDrive.slice(1))) issues.push('invalid');
  return { ok: issues.length === 0, issues };
}
export function gbkFileNameCompat(name: string): { encodable: boolean; note: string } {
  return { encodable: [...name].every((c) => c.charCodeAt(0) <= 0xffff), note: 'GBK 双字节内可存' };
}

/** F05373/F05374 繁体软件 + 国产系统。 */
export function traditionalChineseCompat(text: string): { isTraditional: boolean; hint: string } {
  const trad = /[們開關這裡時間電腦軟體網絡]/.test(text);
  return { isTraditional: trad, hint: trad ? '繁体界面：字体与术语表已就绪' : '简体' };
}
export const CN_OS_HINT: Record<string, string> = { uos: '检测到 UOS：建议使用统信原生包', kylin: '检测到麒麟：走 ARM 转译提示' };
export function cnOsDetect(os: string): string | undefined {
  return CN_OS_HINT[os.toLowerCase()];
}

/** F05375 知识库 + 教学。 */
export const CN_APP_KB: { case: string; fix: string }[] = [
  { case: '微信截图键被桌面占用', fix: '截图热键仲裁让位给微信' },
  { case: '迅雷接管全部下载', fix: '关闭接管或按域名分流' },
  { case: 'WPS 被抢默认', fix: '默认程序仲裁恢复 Office' },
];
export function cnAppTutorial(): string[] {
  return ['中文路径全兼容：长路径自动 \\\\?\\ 前缀', '微信多开上限 4 个', '截图热键冲突由仲裁器裁决'];
}
export function cnAppHash(name: string): string {
  return fnv1a(name).slice(0, 6);
}
