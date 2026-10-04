// AURORA-10000: AI-40 批次（族0196~0200 · 笔记本场景/台式机 DIY/平板二合一/IoT 与边缘/硬件可靠性），勿删。
import { clamp, fnv1a } from './hwModel';

/* ============ 族0196 笔记本场景（F04876~F04900） ============ */

/** F04876/F04877 合盖行为（含外接屏）。 */
export function lidAction(closed: boolean, externalDisplay: boolean, keepOnline: boolean): 'sleep' | 'external-only' | 'awake' {
  if (!closed) return 'awake';
  return externalDisplay ? 'external-only' : keepOnline ? 'awake' : 'sleep';
}
/** F04878/F04879 键盘背光。 */
export function backlightLevel(lux: number | null, manual: number): number {
  if (lux === null) return clamp(manual, 0, 100);
  return clamp(Math.round(100 - (clamp(lux, 0, 500) / 500) * 80), 20, 100);
}
/** F04883/F04884/F04885 场景模式集合。 */
export interface ScenePlan {
  name: string;
  brightness: number;
  motion: boolean;
  mute: boolean;
  dnd: boolean;
  noSleep: boolean;
  privacy: boolean;
}
export const SCENES: Record<string, ScenePlan> = {
  saver: { name: 'saver', brightness: 40, motion: false, mute: false, dnd: false, noSleep: false, privacy: false },
  meeting: { name: 'meeting', brightness: 70, motion: true, mute: true, dnd: true, noSleep: false, privacy: false },
  presentation: { name: 'presentation', brightness: 90, motion: true, mute: true, dnd: true, noSleep: true, privacy: false },
  cafe: { name: 'cafe', brightness: 60, motion: false, mute: false, dnd: false, noSleep: false, privacy: true },
};
export function applyScene(name: keyof typeof SCENES | string): ScenePlan | null {
  return SCENES[name] ?? null;
}
/** F04889/F04890 热点与流量统计。 */
export class Hotspot {
  on = false;
  private clients = new Set<string>();
  private mb = 0;
  toggle(on: boolean): boolean {
    this.on = on;
    return this.on === on;
  }
  connect(mac: string): boolean {
    if (!this.on || this.clients.has(mac)) return false;
    this.clients.add(mac);
    return true;
  }
  traffic(mb: number): number {
    this.mb += mb;
    return Math.round(this.mb * 10) / 10;
  }
  get count(): number {
    return this.clients.size;
  }
}
/** F04891/F04892 访客模式与共用电脑隐私清理。 */
export function guestRestrictions(isGuest: boolean): { canInstall: boolean; canSeeOthers: boolean; sessionIsolated: boolean } {
  return isGuest ? { canInstall: false, canSeeOthers: false, sessionIsolated: true } : { canInstall: true, canSeeOthers: true, sessionIsolated: false };
}
export function privacySweep(items: string[]): { swept: string[]; kept: string[] } {
  const sweepable = ['recent', 'clipboard', 'temp', 'thumbnails'];
  return { swept: items.filter((i) => sweepable.includes(i)), kept: items.filter((i) => !sweepable.includes(i)) };
}
/** F04896/F04897 刷新自适应与亮度配方。 */
export function adaptiveRefresh(idleSec: number, video: boolean): number {
  if (video) return 120;
  return idleSec > 30 ? 60 : 120;
}
export function brightnessRecipe(indoor: boolean, lux: number): number {
  const base = indoor ? 60 : 85;
  return clamp(base + (lux > 300 ? 15 : 0) - (lux < 50 ? 15 : 0), 20, 100);
}
/** F04900 场景切换彩蛋：低频触发。 */
export function sceneEasterEgg(switches: number, rng: () => number): boolean {
  return switches > 0 && rng() < 0.02;
}

/* ============ 族0197 台式机 DIY（F04901~F04925） ============ */

/** F04901 多盘总览。 */
export function diskOverview(disks: { model: string; gb: number; type: 'ssd' | 'hdd' }[]): { ssd: number; hdd: number; totalTB: number } {
  return {
    ssd: disks.filter((d) => d.type === 'ssd').length,
    hdd: disks.filter((d) => d.type === 'hdd').length,
    totalTB: Math.round((disks.reduce((s, d) => s + d.gb, 0) / 1024) * 10) / 10,
  };
}
/** F04902 机箱灯效。 */
export type RgbEffect = 'off' | 'static' | 'breath' | 'rainbow' | 'wave';
export function rgbApply(effect: RgbEffect, brightness: number): { effect: RgbEffect; brightness: number } {
  return { effect, brightness: clamp(brightness, 0, 100) };
}
/** F04904/F04905 温度悬浮。 */
export function tempOverlay(cpuC: number, gpuC: number): { cpuC: number; gpuC: number; warn: boolean } {
  return { cpuC, gpuC, warn: cpuC >= 90 || gpuC >= 85 };
}
/** F04906 风扇曲线（含滞回）。 */
export class FanCurve {
  private lastRpm = 0;
  constructor(private points: { tempC: number; rpm: number }[]) {}
  rpmAt(tempC: number): number {
    const sorted = [...this.points].sort((a, b) => a.tempC - b.tempC);
    let rpm = sorted[0]!.rpm;
    for (const p of sorted) if (tempC >= p.tempC) rpm = p.rpm;
    // 滞回：变化不超过 ±200 防抖
    const next = Math.abs(rpm - this.lastRpm) > 200 ? rpm : this.lastRpm || rpm;
    this.lastRpm = next;
    return next;
  }
}
/** F04909 POST 快览。 */
export function postSummary(devices: string[], memoryOk: boolean, bootDevice: string): { lines: string[]; pass: boolean } {
  const lines = [`MEM ${memoryOk ? 'OK' : 'FAIL'}`, `BOOT ${bootDevice}`, ...devices.map((d) => `DEV ${d}`)];
  return { lines, pass: memoryOk && !!bootDevice };
}
/** F04911/F04912 温度/功耗历史。 */
export class HistorySeries {
  private pts: { t: number; v: number }[] = [];
  push(t: number, v: number): void {
    this.pts.push({ t, v });
  }
  peak(): number {
    return this.pts.reduce((m, p) => Math.max(m, p.v), 0);
  }
  avg(): number {
    return this.pts.length === 0 ? 0 : Math.round((this.pts.reduce((s, p) => s + p.v, 0) / this.pts.length) * 10) / 10;
  }
}
/** F04916 崩溃自检报告。 */
export function crashSelfCheck(lastBootOk: boolean, dumps: number, uptimeMin: number): { healthy: boolean; hints: string[] } {
  const hints: string[] = [];
  if (!lastBootOk) hints.push('上次启动异常');
  if (dumps > 3) hints.push('转储文件偏多，建议分析');
  if (uptimeMin > 60 * 24 * 14) hints.push('运行超过两周，建议重启');
  return { healthy: hints.length === 0, hints };
}
/** F04917/F04918 硬件履历与装机清单。 */
export class HwLedger {
  private records: { part: string; action: 'install' | 'replace' | 'remove'; at: string }[] = [];
  record(part: string, action: 'install' | 'replace' | 'remove', at: string): boolean {
    this.records.push({ part, action, at });
    return true;
  }
  of(part: string): number {
    return this.records.filter((r) => r.part === part).length;
  }
  buildList(): string[] {
    const latest = new Map<string, string>();
    for (const r of this.records) if (r.action !== 'remove') latest.set(r.part, r.at);
    return [...latest.keys()].sort();
  }
}
/** F04922~F04924 换盘/克隆/退役。 */
export function diskClone(src: { gb: number }, dst: { gb: number }): { ok: boolean; reason?: string } {
  if (dst.gb < src.gb) return { ok: false, reason: '目标盘容量不足' };
  return { ok: true };
}
export function secureErase(passes: number, ssd: boolean): { ok: boolean; method: string } {
  return ssd ? { ok: passes >= 1, method: 'sanitize' } : { ok: passes >= 3, method: 'overwrite' };
}

/* ============ 族0198 平板二合一（F04926~F04950） ============ */

/** F04926/F04936/F04937 形态感知与旋转。 */
export type FormFactor = 'laptop' | 'tablet' | 'tent' | 'stand';
export function formDetect(keyboardAttached: boolean, angleDeg: number): FormFactor {
  if (keyboardAttached) return angleDeg > 250 ? 'tent' : 'laptop';
  return angleDeg > 200 ? 'stand' : 'tablet';
}
export class RotationLock {
  locked = false;
  private last: FormFactor = 'tablet';
  onRotate(next: FormFactor): { changed: boolean; factor: FormFactor } {
    if (this.locked) return { changed: false, factor: this.last };
    const changed = next !== this.last;
    this.last = next;
    return { changed, factor: next };
  }
}
/** F04928/F04929 大目标与平板菜单。 */
export const TABLET_TARGET_MIN = 44;
export const TABLET_TASKBAR_H = 64;
/** F04931/F04932 分屏手势与边缘滑动。 */
export function splitGesture(tracks: { x: number; y: number }[]): 'left' | 'right' | 'none' {
  if (tracks.length < 2) return 'none';
  const dx = tracks[tracks.length - 1]!.x - tracks[0]!.x;
  const dy = Math.abs(tracks[tracks.length - 1]!.y - tracks[0]!.y);
  if (dy > Math.abs(dx)) return 'none';
  return dx > 40 ? 'right' : dx < -40 ? 'left' : 'none';
}
/** F04938/F04939 平板省电与盖板休眠。 */
export function tabletPowerPolicy(unplugged: boolean, coverClosed: boolean): { limit: number; sleep: boolean } {
  return { limit: unplugged ? 80 : 100, sleep: coverClosed };
}
/** F04941/F04942 儿童与学习模式。 */
export const KIDS_WHITELIST = ['画板', '识字', '数学'];
export function kidsMode(app: string, allowed: string[]): boolean {
  return allowed.includes(app);
}
export function studyMode(timerMin: number, breakEvery = 25): { focus: boolean; breakDue: boolean } {
  return { focus: timerMin > 0, breakDue: timerMin > 0 && timerMin % breakEvery === 0 };
}
/** F04945 绘画免打扰。 */
export function drawModeDnd(drawing: boolean, dnd: boolean): boolean {
  return drawing && dnd;
}

/* ============ 族0199 IoT 与边缘（F04951~F04975） ============ */

/** F04951/F04952 内网设备扫描与面板。 */
export interface LanDevice {
  ip: string;
  mac: string;
  hostname: string;
  openPorts: number[];
}
export function lanScan(subnet: string, hosts: LanDevice[]): { subnet: string; found: number } {
  return { subnet, found: hosts.length };
}
export function devicePanel(d: LanDevice): { title: string; lines: string[] } {
  return { title: d.hostname, lines: [d.ip, d.mac, `ports:${d.openPorts.join(',')}`] };
}
/** F04953 WOL 魔术包。 */
export function magicPacket(mac: string): string | null {
  const m = mac.replace(/[:-]/g, '').toUpperCase();
  if (!/^[0-9A-F]{12}$/.test(m)) return null;
  return 'FF'.repeat(6) + m.repeat(16);
}
/** F04957/F04958 NAS 面板与网关信息。 */
export function nasPanel(disks: number, freeTB: number, raidOk: boolean): { disks: number; freeTB: number; healthy: boolean } {
  return { disks, freeTB, healthy: raidOk };
}
export function gatewayInfo(ip: string): { gw: string; network: string } {
  const p = ip.split('.');
  return { gw: `${p[0]}.${p[1]}.${p[2]}.1`, network: `${p[0]}.${p[1]}.${p[2]}.0/24` };
}
/** F04959 IP 冲突检测。 */
export function ipConflict(table: Map<string, string>): { conflict: boolean; ip: string | null } {
  const seen = new Set<string>();
  for (const ip of table.values()) {
    if (seen.has(ip)) return { conflict: true, ip };
    seen.add(ip);
  }
  return { conflict: false, ip: null };
}
/** F04961/F04962 上线通知与陌生告警。 */
export class DeviceWatch {
  private known = new Set<string>();
  constructor(known: string[]) {
    known.forEach((k) => this.known.add(k));
  }
  onAppear(mac: string): 'known' | 'stranger' {
    return this.known.has(mac) ? 'known' : 'stranger';
  }
  adopt(mac: string): void {
    this.known.add(mac);
  }
}
/** F04965 拓扑图。 */
export function topologyEdges(devices: LanDevice[], gw: string): [string, string][] {
  return devices.map((d) => [gw, d.ip]);
}
/** F04968 ping 监控。 */
export function pingStats(results: number[]): { lossPct: number; avgMs: number } {
  const ok = results.filter((r) => r >= 0);
  const loss = results.length - ok.length;
  return {
    lossPct: results.length === 0 ? 100 : Math.round((loss / results.length) * 100),
    avgMs: ok.length === 0 ? -1 : Math.round((ok.reduce((s, v) => s + v, 0) / ok.length) * 10) / 10,
  };
}
/** F04969 端口占用检查。 */
export function portFree(used: number[], port: number): boolean {
  return !used.includes(port);
}
/** F04971/F04972 远程协助。 */
export type AssistRole = 'controlled' | 'controller';
export function assistSession(role: AssistRole, peerTrust: boolean): { allowed: boolean; role: AssistRole } {
  return { allowed: peerTrust || role === 'controlled', role };
}
/** F04973 扫描边界声明。 */
export const SCAN_BOUNDARY = '仅限本机所在 /24 网段，默认只读探测，不尝试登录';

/* ============ 族0200 硬件可靠性（F04976~F05000） ============ */

/** F04976 看门狗：UI 卡死检测与自恢复。 */
export class Watchdog {
  private lastBeat = 0;
  constructor(private timeoutMs: number) {}
  beat(atMs: number): void {
    this.lastBeat = atMs;
  }
  check(nowMs: number): { stalled: boolean; recover: boolean } {
    const stalled = nowMs - this.lastBeat > this.timeoutMs;
    return { stalled, recover: stalled };
  }
}
/** F04977/F04978/F04979 蓝屏自恢复与转储。 */
export class DumpStore {
  private dumps: { code: string; at: number }[] = [];
  add(code: string, at: number): void {
    this.dumps.push({ code, at });
  }
  autoRebootCount(): number {
    return this.dumps.length;
  }
  analyze(): { top: string | null; count: number } {
    const m = new Map<string, number>();
    this.dumps.forEach((d) => m.set(d.code, (m.get(d.code) ?? 0) + 1));
    const top = [...m.entries()].sort((a, b) => b[1] - a[1])[0];
    return { top: top?.[0] ?? null, count: top?.[1] ?? 0 };
  }
}
/** F04980/F04981 WHEA 与 ECC。 */
export function wheaLog(errors: { type: string; severity: 'corrected' | 'fatal' }[]): { corrected: number; fatal: number } {
  return {
    corrected: errors.filter((e) => e.severity === 'corrected').length,
    fatal: errors.filter((e) => e.severity === 'fatal').length,
  };
}
export function eccStats(corrected: number, total: number): { rate: number; ok: boolean } {
  const rate = total === 0 ? 0 : Math.round((corrected / total) * 10000) / 100;
  return { rate, ok: rate < 0.1 };
}
/** F04982/F04983 过热保护与通知。 */
export function thermalGuard(tempC: number, limit = 95): { throttle: boolean; notify: boolean } {
  return { throttle: tempC >= limit, notify: tempC >= limit - 10 };
}
/** F04985 文件系统脏位自检。 */
export function dirtyBitRecovery(dirty: boolean, journalOk: boolean): { repaired: boolean; fsckNeeded: boolean } {
  if (!dirty) return { repaired: false, fsckNeeded: false };
  return { repaired: journalOk, fsckNeeded: !journalOk };
}
/** F04991/F04992 系统文件与配置自愈。 */
export function systemFileRepair(hashes: Record<string, string>, golden: Record<string, string>): { repaired: string[]; intact: boolean } {
  const repaired = Object.keys(golden).filter((k) => hashes[k] !== golden[k]);
  return { repaired, intact: repaired.length === 0 };
}
export function configSelfHeal(current: string | null, fallback: string): string {
  return current && current.trim().length > 0 ? current : fallback;
}
/** F04993/F04994 启动循环修复与安全模式。 */
export function bootLoopDetect(attempts: number, threshold = 3): { loop: boolean; action: string } {
  return { loop: attempts >= threshold, action: attempts >= threshold ? '进入恢复环境' : '正常启动' };
}
export const SAFE_MODE_KINDS = ['minimal', 'with-network', 'with-cmd'] as const;
/** F04996/F04997 基准报告与月度可靠性。 */
export function reliabilityMonthly(days: { crashes: number; uptimeH: number }[]): { crashes: number; uptimeH: number; score: number } {
  const crashes = days.reduce((s, d) => s + d.crashes, 0);
  const uptimeH = days.reduce((s, d) => s + d.uptimeH, 0);
  const score = Math.max(0, Math.round(100 - crashes * 5));
  return { crashes, uptimeH, score };
}
/** F04998 事件关联：崩溃前行为分析。 */
export function eventCorrelation(events: { at: number; kind: 'io-error' | 'temp-high' | 'driver-warn' | 'crash' }[], windowMs = 60000): string | null {
  const crash = events.find((e) => e.kind === 'crash');
  if (!crash) return null;
  const pre = events.filter((e) => e.kind !== 'crash' && crash.at - e.at <= windowMs && crash.at - e.at >= 0);
  const counts = new Map<string, number>();
  pre.forEach((e) => counts.set(e.kind, (counts.get(e.kind) ?? 0) + 1));
  const top = [...counts.entries()].sort((a, b) => b[1] - a[1])[0];
  return top ? top[0] : null;
}

/* ============ 补充助手（AI-40 批次内聚实现），勿删 ============ */

/** F04882 电池图标样式。 */
export const BATTERY_ICON_STYLES = ['default', 'percentage', 'ring', 'minimal'] as const;
export function batteryIconStyle(style: string): boolean {
  return (BATTERY_ICON_STYLES as readonly string[]).includes(style);
}
/** F04887 离线工作状态。 */
export function offlineWork(wifi: boolean): { offline: boolean; ready: boolean } {
  return { offline: !wifi, ready: true };
}
/** F04888 离线包准备：剩余下载。 */
export function offlinePack(items: string[], done: string[]): { remaining: string[]; pct: number } {
  const d = new Set(done);
  const remaining = items.filter((i) => !d.has(i));
  return { remaining, pct: Math.round(((items.length - remaining.length) / Math.max(1, items.length)) * 100) };
}
/** F04894 充电提示音。 */
export function chargeSound(on: boolean, plugged: boolean): { play: boolean; on: boolean } {
  return { play: on && plugged, on };
}
/** F04950 平板彩蛋：转圈次数触发。 */
export function tabletEasterEgg(rotations: number, rng: () => number): boolean {
  return rotations >= 3 && rng() < 0.1;
}
/** F04927 虚拟触板区域。 */
export function virtualTouchpad(area: { w: number; h: number }): { ok: boolean; area: { w: number; h: number } } {
  return { ok: area.w >= 400 && area.h >= 300, area };
}
/** F04929 平板开始菜单列数。 */
export const TABLET_MENU_COLS = 4;
/** F04930 小屏全屏建议。 */
export function fullscreenSuggest(diagInch: number): boolean {
  return diagInch < 11;
}
/** F04935 笔失联提醒。 */
export function penMissing(linked: boolean, lastSeenMin: number): { remind: boolean; lastSeenMin: number } {
  return { remind: !linked && lastSeenMin >= 10, lastSeenMin };
}
/** F04943 一键笔记。 */
export function oneKeyNote(action: string): { action: string; launched: boolean } {
  return { action, launched: action === 'new-note' };
}
/** F04944 一键阅读。 */
export function oneKeyRead(mode: string): { mode: string; applied: boolean } {
  const ok = ['sepia', 'dark', 'paper'].includes(mode);
  return { mode, applied: ok };
}
/** F04947 平板任务卡片网格。 */
export function taskCards(n: number): { cards: number; cols: number } {
  return { cards: n, cols: Math.min(2, Math.max(1, n)) };
}
/** F04948 应用双开。 */
export function dualApp(a: string, b: string): { ok: boolean; pair: [string, string] } {
  return { ok: a !== b, pair: [a, b] };
}
/** F04956 局域网打印机发现。 */
export function discoverPrinters(hosts: { ip: string; ports: number[] }[]): string[] {
  return hosts.filter((h) => h.ports.includes(9100) || h.ports.includes(631)).map((h) => h.ip);
}
/** F04960 带宽分配建议。 */
export function qosHint(totalMbps: number, devices: number): { perDeviceMbps: number; suggestion: string } {
  const per = devices > 0 ? Math.round(totalMbps / devices) : 0;
  return { perDeviceMbps: per, suggestion: per < 20 ? '建议限制大流量设备' : '带宽充足' };
}
/** F04963 来宾网络。 */
export function guestNetwork(enable: boolean): { enabled: boolean; isolated: boolean; ssid: string } {
  return { enabled: enable, isolated: enable, ssid: enable ? 'Variable-Guest' : '' };
}
/** F04964 设备命名标签。 */
export function deviceLabel(mac: string, alias: string): { mac: string; alias: string; digest: string } {
  return { mac, alias, digest: fnv1a(mac + alias) };
}
/** F04967 掉线历史。 */
export class OfflineLog {
  private m = new Map<string, number>();
  down(mac: string): void {
    this.m.set(mac, (this.m.get(mac) ?? 0) + 1);
  }
  count(mac: string): number {
    return this.m.get(mac) ?? 0;
  }
}
/** F04966 带宽监控：Top talker。 */
export function bandwidthMonitor(mbPerDev: { mac: string; mb: number }[]): { top: string | null; mb: number } {
  const t = [...mbPerDev].sort((a, b) => b.mb - a.mb)[0];
  return { top: t?.mac ?? null, mb: t?.mb ?? 0 };
}
/** F04984 断电恢复（快速启动标志）。 */
export function powerLossRecovery(fastStartup: boolean): { recover: boolean; fastStartup: boolean } {
  return { recover: true, fastStartup };
}
/** F04988 备份健康提醒。 */
export function backupReminder(lastBackupDays: number, maxDays = 14): { due: boolean; lastBackupDays: number } {
  return { due: lastBackupDays >= maxDays, lastBackupDays };
}
/** F04995 最小系统诊断服务集。 */
export const MINIMAL_SERVICES = ['kernel', 'storage', 'input', 'display'] as const;
export function minimalSystem(services: string[]): { ok: boolean; missing: string[] } {
  const missing = (MINIMAL_SERVICES as readonly string[]).filter((s) => !services.includes(s));
  return { ok: missing.length === 0, missing };
}
/** F04999 诊断包（需明示同意）。 */
export function diagPackage(include: string[], consent: boolean): { ok: boolean; items: string[] } {
  return { ok: consent, items: consent ? include : [] };
}
/** F04975 网络地图彩蛋（低频）。 */
export function networkMapEgg(nodes: number, rng: () => number): boolean {
  return nodes >= 5 && rng() < 0.03;
}
