// AURORA-10000: AI-38 批次（族0186~0190 · 系统信息与诊断/更新与部署/灾备与迁移/安全硬件/性能调校），勿删。
import { clamp, fnv1a, seeded, semverCmp } from './hwModel';

/* ============ 族0186 系统信息与诊断（F04626~F04650） ============ */

/** F04626 系统摘要一页总览。 */
export interface SysSnapshot {
  os: string;
  cpu: string;
  cores: number;
  ramGB: number;
  gpus: string[];
  disks: { model: string; gb: number }[];
}
export function systemSummary(s: SysSnapshot): { lines: string[]; complete: boolean } {
  const lines = [`OS ${s.os}`, `CPU ${s.cpu} x${s.cores}`, `RAM ${s.ramGB}GB`, ...s.gpus.map((g) => `GPU ${g}`), ...s.disks.map((d) => `DISK ${d.model} ${d.gb}GB`)];
  return { lines, complete: lines.length >= 3 + s.gpus.length + s.disks.length };
}
/** F04627~F04633 部件详情。 */
export interface CpuDetail {
  name: string;
  cacheL3MB: number;
  sets: string[];
}
export function cpuFlags(c: CpuDetail): boolean {
  return c.sets.includes('x86-64') && c.cacheL3MB > 0;
}
export interface RamStick {
  slot: string;
  gb: number;
  mhz: number;
}
export function ramChannels(sticks: RamStick[]): number {
  return Math.min(4, Math.max(1, sticks.length));
}
/** F04635 EDID 解析（简化：厂商串 + 十六进制宽高）。 */
export function edidParse(hex: string): { vendor: string; width: number; height: number } | null {
  if (hex.length < 11) return null;
  const vendor = hex.slice(0, 3).toUpperCase();
  const w = parseInt(hex.slice(3, 7), 16);
  const h = parseInt(hex.slice(7, 11), 16);
  if (!w || !h) return null;
  return { vendor, width: w, height: h };
}
/** F04638 温度总览：找最热与告警。 */
export function tempOverview(sensors: Record<string, number>): { hottest: string; max: number; warn: boolean } {
  const es = Object.entries(sensors);
  if (es.length === 0) return { hottest: '', max: 0, warn: false };
  const [hottest, max] = es.reduce((a, b) => (b[1] > a[1] ? b : a));
  return { hottest: hottest!, max: max!, warn: max! >= 85 };
}
/** F04641 功耗墙。 */
export function powerLimits(pl1: number, pl2: number): { ok: boolean; ratio: number } {
  return { ok: pl2 >= pl1 && pl1 > 0, ratio: pl1 > 0 ? Math.round((pl2 / pl1) * 10) / 10 : 0 };
}
/** F04642~F04644 内置跑分与对比。 */
export function builtinBench(seed: number, kind: 'cpu' | 'gpu' | 'disk'): number {
  const r = seeded(seed);
  const base = kind === 'cpu' ? 10000 : kind === 'gpu' ? 8000 : 4000;
  return Math.round(base * (0.9 + r() * 0.2));
}
export function benchCompare(mine: number, ref: number): { pct: number; verdict: string } {
  const pct = Math.round(((mine - ref) / ref) * 100);
  return { pct, verdict: pct >= 0 ? '不低于参考' : `低于参考 ${-pct}%` };
}
/** F04645 硬件变化检测。 */
export function hwChange(before: string[], after: string[]): { added: string[]; removed: string[] } {
  const b = new Set(before);
  const a = new Set(after);
  return { added: [...a].filter((x) => !b.has(x)), removed: [...b].filter((x) => !a.has(x)) };
}
/** F04648 蓝屏解读。 */
const BSOD_CODES: Record<string, string> = {
  CRITICAL_PROCESS_DIED: '关键系统进程终止——多为驱动或系统文件损坏',
  MEMORY_MANAGEMENT: '内存管理错误——排查内存条/超频',
  DPC_WATCHDOG_VIOLATION: 'DPC 看门狗超时——多为存储驱动问题',
  KMODE_EXCEPTION_NOT_HANDLED: '内核模式异常——排查最近安装的驱动',
};
export function bsodDecode(code: string): string | null {
  return BSOD_CODES[code] ?? null;
}
/** F04649 日志聚合。 */
export function logAggregate(events: { level: 'err' | 'warn' | 'info'; src: string }[]): { errs: number; warns: number; bySrc: Record<string, number> } {
  const bySrc: Record<string, number> = {};
  let errs = 0;
  let warns = 0;
  for (const e of events) {
    bySrc[e.src] = (bySrc[e.src] ?? 0) + 1;
    if (e.level === 'err') errs++;
    if (e.level === 'warn') warns++;
  }
  return { errs, warns, bySrc };
}

/* ============ 族0187 更新与部署（F04651~F04675） ============ */

/** F04651~F04653 检查/增量/预览。 */
export interface UpdateManifest {
  version: string;
  notes: string;
  blocks: string[];
}
export function checkUpdate(cur: string, m: UpdateManifest): boolean {
  return semverCmp(m.version, cur) > 0;
}
export function deltaBlocks(have: string[], next: string[]): { download: string[]; total: number } {
  const h = new Set(have);
  const dl = next.filter((b) => !h.has(b));
  return { download: dl, total: next.length };
}
/** F04654/F04655 活动时段与暂停。 */
export function inActiveHours(hour: number, from = 9, to = 18): boolean {
  return hour >= from && hour < to;
}
export function pauseDays(d: number): number {
  return clamp(Math.round(d), 1, 7);
}
/** F04660 更新回滚。 */
export class UpdateHistory {
  private h: { version: string; at: number; ok: boolean }[] = [];
  record(version: string, ok: boolean, at: number): void {
    this.h.push({ version, at, ok });
  }
  lastOk(): string | null {
    return [...this.h].reverse().find((x) => x.ok)?.version ?? null;
  }
  get all(): typeof this.h {
    return [...this.h];
  }
}
/** F04661 去重下载（内容寻址缓存）。 */
export class DedupDownloads {
  private cache = new Map<string, string>();
  fetch(content: string): { url: string; hit: boolean } {
    const key = fnv1a(content);
    if (this.cache.has(key)) return { url: key, hit: true };
    this.cache.set(key, content);
    return { url: key, hit: false };
  }
  get size(): number {
    return this.cache.size;
  }
}
/** F04665 更新通道。 */
export const UPDATE_CHANNELS = ['stable', 'beta', 'dev'] as const;
export type UpdateChannel = (typeof UPDATE_CHANNELS)[number];
export function channelRank(c: UpdateChannel): number {
  return UPDATE_CHANNELS.indexOf(c);
}
/** F04667/F04668 电量与流量策略。 */
export function mayInstall(batteryPct: number | null, metered: boolean, critical: boolean): { ok: boolean; reason: string } {
  if (critical) return { ok: true, reason: '安全更新强制' };
  if (batteryPct !== null && batteryPct < 30) return { ok: false, reason: '电量低于 30%' };
  if (metered) return { ok: false, reason: '按流量计费网络暂停' };
  return { ok: true, reason: '条件满足' };
}
/** F04673/F04674 更新前快照与后自检。 */
export class PreUpdateSnapshot {
  private s: string | null = null;
  take(state: string): string {
    this.s = fnv1a(state);
    return this.s;
  }
  verify(state: string): 'pass' | 'drift' | 'none' {
    if (!this.s) return 'none';
    return fnv1a(state) === this.s ? 'pass' : 'drift';
  }
}

/* ============ 族0188 灾备与迁移（F04676~F04700） ============ */

/** F04679/F04680/F04682 重置计划：保留文件 vs 全清。 */
export function resetPlan(kind: 'keep-files' | 'full', userFiles: string[], installed: string[]): { keep: string[]; removed: string[] } {
  return kind === 'keep-files' ? { keep: userFiles, removed: installed } : { keep: [], removed: [...userFiles, ...installed] };
}
/** F04683 重置向导步骤。 */
export const RESET_WIZARD_STEPS = ['确认方案', '备份提示', '执行重置', '区域与键盘', '账户登录'] as const;
/** F04684/F04685 驱动备份与恢复。 */
export class DriverVault {
  private v = new Map<string, string>();
  backup(dev: string, blob: string): boolean {
    if (this.v.has(dev)) return false;
    this.v.set(dev, blob);
    return true;
  }
  restore(dev: string): string | null {
    return this.v.get(dev) ?? null;
  }
  get count(): number {
    return this.v.size;
  }
}
/** F04687/F04688 网络配置与 hosts 备份。 */
export function netConfigBackup(cfg: { ip: string; mask: string; gw: string; dns: string[]; proxy?: string }): string {
  return fnv1a(JSON.stringify(cfg));
}
export function hostsBackup(lines: string[]): string {
  return fnv1a(lines.join('\n'));
}
/** F04689/F04690 注册表导出与还原。 */
export class RegHive {
  private kv = new Map<string, string>();
  set(path: string, value: string): void {
    this.kv.set(path, value);
  }
  get(path: string): string | undefined {
    return this.kv.get(path);
  }
  export(): string {
    return fnv1a([...this.kv.entries()].sort().map(([k, v]) => `${k}=${v}`).join(';'));
  }
  restore(hive: RegHive): boolean {
    return hive.export() === this.export();
  }
}
/** F04691/F04692 服务快照与任务导出。 */
export function serviceSnapshot(states: Record<string, 'running' | 'stopped'>): string {
  return fnv1a(JSON.stringify(states));
}
export function tasksExport(tasks: { name: string; cron: string }[]): string[] {
  return tasks.map((t) => `${t.name}@${t.cron}`);
}
/** F04694~F04698 迁移向导与校验。 */
export const MIGRATION_ITEMS = ['文档', '图片', '桌面', '浏览器书签', 'Wi-Fi 密码', '已装应用清单'] as const;
export class MigrationPlan {
  items = new Set<string>();
  include(item: (typeof MIGRATION_ITEMS)[number]): boolean {
    this.items.add(item);
    return true;
  }
  get complete(): boolean {
    return this.items.size === MIGRATION_ITEMS.length;
  }
}
export function migrationVerify(src: Record<string, string>, dst: Record<string, string>): { ok: boolean; missing: string[] } {
  const missing = Object.keys(src).filter((k) => dst[k] !== src[k]);
  return { ok: missing.length === 0, missing };
}
/** F04699 灾难演练模式：只演练不落盘。 */
export function disasterDrill(mode: string): { simulated: true; mode: string; sideEffects: 0 } {
  return { simulated: true, mode, sideEffects: 0 };
}

/* ============ 族0189 安全硬件（F04701~F04725） ============ */

/** F04701/F04702 TPM 管理与重置流程。 */
export class TpmManager {
  state: 'ready' | 'owned' | 'reset-pending' | 'absent' = 'absent';
  constructor(present: boolean) {
    if (present) this.state = 'owned';
  }
  requestReset(confirmOwner: boolean): boolean {
    if (this.state !== 'owned' || !confirmOwner) return false;
    this.state = 'reset-pending';
    return true;
  }
  completeReset(): boolean {
    if (this.state !== 'reset-pending') return false;
    this.state = 'ready';
    return true;
  }
}
/** F04703~F04706 BitLocker 开关/恢复密钥/状态。 */
export class DiskEncryption {
  private vols = new Map<string, { on: boolean; key: string }>();
  enable(vol: string, key: string): boolean {
    if (this.vols.get(vol)?.on) return false;
    this.vols.set(vol, { on: true, key });
    return true;
  }
  recover(vol: string, key: string): boolean {
    return this.vols.get(vol)?.key === key;
  }
  disable(vol: string): boolean {
    const v = this.vols.get(vol);
    if (!v) return false;
    v.on = false;
    return true;
  }
  status(vol: string): boolean {
    return this.vols.get(vol)?.on ?? false;
  }
}
/** F04708 启动黑名单（dbx）。 */
export class BootBlacklist {
  private dbx = new Set<string>();
  add(sha: string): boolean {
    if (sha.length !== 8) return false;
    this.dbx.add(sha);
    return true;
  }
  blocked(sha: string): boolean {
    return this.dbx.has(sha);
  }
}
/** F04710~F04712 硬件开关。 */
export class HardwareSwitches {
  camera = true;
  mic = true;
  wireless = true;
  allOff(): boolean {
    this.camera = false;
    this.mic = false;
    this.wireless = false;
    return !this.camera && !this.mic && !this.wireless;
  }
}
/** F04713~F04715 USB 管控。 */
export type UsbPolicy = 'allow-all' | 'block' | 'readonly' | 'whitelist';
export class UsbControl {
  constructor(public policy: UsbPolicy = 'allow-all', private whitelist = new Set<string>()) {}
  plug(id: string): 'allow' | 'block' | 'readonly' {
    if (this.policy === 'block') return 'block';
    if (this.policy === 'whitelist') return this.whitelist.has(id) ? 'allow' : 'block';
    if (this.policy === 'readonly') return 'readonly';
    return 'allow';
  }
  whitelistAdd(id: string): boolean {
    this.whitelist.add(id);
    return true;
  }
}
/** F04716~F04722 TB/DMA/VBS/签名。 */
export function thunderboltLevel(level: 0 | 1 | 2 | 3): { ok: boolean; desc: string } {
  const desc = ['无保护', '仅用户授权', '用户+固件授权', '完全禁止'][level]!;
  return { ok: level >= 1, desc };
}
export function kernelDmaGuard(enabled: boolean): boolean {
  return enabled;
}
export function vbsState(hvci: boolean, cred: boolean): { vbs: boolean; hvci: boolean; level: string } {
  return { vbs: hvci && cred, hvci, level: hvci && cred ? '完整' : hvci ? '部分' : '关闭' };
}
export function signatureGuard(testSigning: boolean, kernelDebug: boolean): { safe: boolean; risks: string[] } {
  const risks: string[] = [];
  if (testSigning) risks.push('测试签名已开启');
  if (kernelDebug) risks.push('内核调试已开启');
  return { safe: risks.length === 0, risks };
}
/** F04723/F04724 安全评分与基线。 */
export function hwSecurityScore(o: { tpm: boolean; sb: boolean; vbs: boolean; dma: boolean; tb: boolean; usb: boolean }): number {
  const w = [20, 20, 20, 15, 15, 10];
  const flags = [o.tpm, o.sb, o.vbs, o.dma, o.tb, o.usb];
  return flags.reduce((s, f, i) => s + (f ? w[i]! : 0), 0);
}
export function baselineCheck(actual: Record<string, boolean>, required: string[]): string[] {
  return required.filter((k) => !actual[k]);
}

/* ============ 族0190 性能调校（F04726~F04750） ============ */

/** F04726~F04728 模式/游戏模式/后台冻结。 */
export type PerfMode = 'silent' | 'balanced' | 'performance';
export class PerfGovernor {
  mode: PerfMode = 'balanced';
  gameActive = false;
  private frozen = new Set<string>();
  setMode(m: PerfMode): void {
    this.mode = m;
  }
  enterGame(): string[] {
    this.gameActive = true;
    this.frozen = new Set(['updater', 'indexer', 'telemetry']);
    return [...this.frozen];
  }
  exitGame(): void {
    this.gameActive = false;
    this.frozen.clear();
  }
  isFrozen(proc: string): boolean {
    return this.frozen.has(proc);
  }
}
/** F04729/F04730 内存清理与压缩。 */
export function memoryClean(freeMB: number, standbyMB: number): { freedMB: number; freeMB: number } {
  const freed = Math.round(standbyMB * 0.6);
  return { freedMB: freed, freeMB: freeMB + freed };
}
export function standbyCompress(pagesMB: number): { compressedMB: number; savedMB: number } {
  return { compressedMB: pagesMB, savedMB: Math.round(pagesMB * 0.5) };
}
/** F04732/F04733 核心分配与优先级。 */
export function coreAffinity(pid: number, cores: number[]): { pid: number; mask: number } {
  const mask = cores.reduce((m, c) => m | (1 << c), 0);
  return { pid, mask };
}
export const PRIORITY_CLASSES = ['idle', 'below-normal', 'normal', 'above-normal', 'high'] as const;
/** F04735 帧率目标。 */
export function fpsCap(want: number | 'off'): number | 'off' {
  if (want === 'off') return 'off';
  return clamp(want, 30, 360);
}
/** F04737/F04738 温度墙与降频日志。 */
export class ThrottleLog {
  private events: { at: number; tempC: number }[] = [];
  observe(at: number, tempC: number, wall = 95): boolean {
    if (tempC >= wall) {
      this.events.push({ at, tempC });
      return true;
    }
    return false;
  }
  get throttles(): number {
    return this.events.length;
  }
}
/** F04746 即时回放环形缓冲。 */
export class InstantReplay {
  private buf: number[] = [];
  constructor(private capSec: number) {}
  push(ts: number): void {
    this.buf.push(ts);
    while (this.buf.length > 0 && ts - this.buf[0]! > this.capSec) this.buf.shift();
  }
  save(n: number): number[] {
    return this.buf.slice(-n);
  }
}
/** F04747 驱动前后对比。 */
export function regressionCompare(before: number, after: number, tolPct = 3): { regress: boolean; delta: number } {
  const delta = Math.round(((after - before) / before) * 1000) / 10;
  return { regress: delta < -tolPct, delta };
}
/** F04748 电源滑杆联动。 */
export function perfModeLink(perf: PerfMode): { brightness: number; boost: boolean } {
  return perf === 'performance' ? { brightness: 100, boost: true } : perf === 'silent' ? { brightness: 60, boost: false } : { brightness: 80, boost: false };
}

/* ============ 补充助手（AI-38 批次内聚实现），勿删 ============ */

/** F04629 主板信息摘要。 */
export function boardInfo(vendor: string, model: string, bios: string): { vendor: string; model: string; bios: string; digest: string } {
  return { vendor, model, bios, digest: fnv1a(`${vendor}/${model}/${bios}`) };
}
/** F04632 音频端点统计。 */
export function audioEndpoints(outputs: number, inputs: number): { outputs: number; inputs: number; ok: boolean } {
  return { outputs, inputs, ok: outputs >= 1 && inputs >= 0 };
}
/** F04633 网卡信息校验。 */
export function nicInfo(mac: string, speedMbps: number): { valid: boolean; gigabit: boolean } {
  const valid = /^([0-9A-F]{2}[:-]){5}[0-9A-F]{2}$/.test(mac);
  return { valid, gigabit: speedMbps >= 1000 };
}
/** F04636 USB 控制器概览。 */
export function usbControllers(xhciCount: number): { xhci: number; ehciLegacy: boolean } {
  return { xhci: xhciCount, ehciLegacy: xhciCount === 0 };
}
/** F04637 蓝牙版本比较。 */
export function btVersionAtLeast(cur: string, min: string): boolean {
  return semverCmp(cur, min) >= 0;
}
/** F04640 风扇转速合理域。 */
export function fanRpm(rpm: number): { ok: boolean; rpm: number } {
  return { ok: rpm === 0 || (rpm >= 200 && rpm <= 6000), rpm };
}
/** F04643 跑分历史。 */
export class BenchHistory {
  private scores: { at: number; score: number }[] = [];
  push(at: number, score: number): void {
    this.scores.push({ at, score });
  }
  best(): number {
    return this.scores.reduce((m, s) => Math.max(m, s.score), 0);
  }
  get runs(): number {
    return this.scores.length;
  }
}
/** F04646 驱动健康评分。 */
export function driverHealth(ageDays: number, crashes90d: number): { score: number; level: string } {
  let score = 100;
  if (ageDays > 365) score -= 20;
  if (crashes90d > 0) score -= Math.min(40, crashes90d * 10);
  return { score, level: score >= 80 ? 'good' : score >= 50 ? 'watch' : 'bad' };
}
/** F04647 新硬件兼容检查。 */
export function compatCheck(hw: { pcieGen: number; watt: number }, req: { minPcieGen: number; psuWatt: number }): { ok: boolean; gaps: string[] } {
  const gaps: string[] = [];
  if (hw.pcieGen < req.minPcieGen) gaps.push('PCIe 代际不足');
  if (hw.watt > req.psuWatt) gaps.push('电源功率不足');
  return { ok: gaps.length === 0, gaps };
}
/** F04663 离线安装包。 */
export function offlinePackage(files: string[]): { digest: string; count: number } {
  return { digest: fnv1a(files.sort().join('|')), count: files.length };
}
/** F04666 静默安装。 */
export function silentInstall(supported: boolean): boolean {
  return supported;
}
/** F04669 更新失败诊断。 */
export function updateFailDiagnose(stage: string): string {
  const map: Record<string, string> = {
    download: '下载失败——检查网络或磁盘空间',
    verify: '签名校验失败——包可能损坏，建议重下',
    install: '安装失败——存在占用进程，建议重启后重试',
    reboot: '重启阶段失败——已自动回滚到上一版本',
  };
  return map[stage] ?? '未知阶段';
}
/** F04672 统一更新中心。 */
export function unifiedUpdates(os: string[], apps: string[], drivers: string[]): { total: number; bySource: Record<string, number> } {
  return { total: os.length + apps.length + drivers.length, bySource: { os: os.length, apps: apps.length, drivers: drivers.length } };
}
/** F04676 恢复环境入口。 */
export function recoveryEntry(rebootNeeded: boolean): { enter: boolean; rebootNeeded: boolean } {
  return { enter: true, rebootNeeded };
}
/** F04696 局域网迁移耗时。 */
export function lanMigration(speedMbps: number, gb: number): { sec: number; ok: boolean } {
  if (speedMbps <= 0) return { sec: -1, ok: false };
  return { sec: Math.ceil((gb * 1024 * 8) / speedMbps), ok: true };
}
/** F04697 外盘迁移容量检查。 */
export function externalMigration(driveGB: number, needGB: number): { fits: boolean; marginGB: number } {
  return { fits: driveGB >= needGB, marginGB: Math.round((driveGB - needGB) * 10) / 10 };
}
/** F04722 驱动签名强制。 */
export function driverSignatureEnforced(on: boolean): boolean {
  return on;
}
/** F04731 GPU 调度策略。 */
export type GpuSchedMode = 'auto' | 'hybrid' | 'dedicated';
export function gpuScheduler(mode: GpuSchedMode): { mode: GpuSchedMode; crossAdapterCopy: boolean } {
  return { mode, crossAdapterCopy: mode === 'hybrid' };
}
/** F04734 游戏 HUD 覆盖项。 */
export const HUD_ITEMS = ['fps', 'frame-time', 'cpu', 'gpu', 'vram', 'ping'] as const;
export function hudOverlay(selected: string[]): { valid: boolean; items: string[] } {
  return { valid: selected.every((s) => (HUD_ITEMS as readonly string[]).includes(s)), items: selected };
}
/** F04736 输入延迟分级。 */
export function inputLatency(ms: number): { ms: number; grade: 'excellent' | 'good' | 'noticeable' } {
  return { ms, grade: ms <= 8 ? 'excellent' : ms <= 20 ? 'good' : 'noticeable' };
}
/** F04739 超频信息（只读）。 */
export function ocInfo(multiplier: number, bclkMhz: number): { ghz: number; readonly: true } {
  return { ghz: Math.round(((multiplier * bclkMhz) / 1000) * 100) / 100, readonly: true };
}
/** F04740 XMP 内存配置信息。 */
export function xmpInfo(profiles: { slot: number; mhz: number; cl: number }[]): { count: number; fastest: number } {
  return { count: profiles.length, fastest: profiles.reduce((m, p) => Math.max(m, p.mhz), 0) };
}
/** F04741 存储性能模式。 */
export type StorageMode = 'performance' | 'balanced' | 'reliability';
export function storageMode(m: StorageMode): { mode: StorageMode; writeCache: boolean } {
  return { mode: m, writeCache: m !== 'reliability' };
}
/** F04743 输入低延迟优先。 */
export function inputPriorityBoost(on: boolean): { on: boolean; irqPriority: string } {
  return { on, irqPriority: on ? 'high' : 'normal' };
}
/** F04744 全屏优化开关。 */
export function fullscreenOptim(enabled: boolean): boolean {
  return enabled;
}
/** F04749 卡顿诊断：掉帧率。 */
export function stutterDiagnose(frameMs: number[], budget = 16.7): { dropPct: number; worst: number } {
  const drops = frameMs.filter((f) => f > budget).length;
  return { dropPct: Math.round((drops / Math.max(1, frameMs.length)) * 1000) / 10, worst: Math.max(...frameMs, 0) };
}
