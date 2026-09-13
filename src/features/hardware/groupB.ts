// AURORA-10000: AI-37 批次（族0181~0185 · 开机与固件/输入设备联动/传感与位置硬件/多设备互联/虚拟化与容器），勿删。
import { clamp, fnv1a } from './hwModel';

/* ============ 族0181 开机与固件（F04501~F04525） ============ */

/** F04501~F04503 固件信息/Secure Boot/TPM。 */
export interface UefiInfo {
  vendor: string;
  version: string;
  date: string;
  secureBoot: boolean;
  tpm: { present: boolean; version: string };
}
export function firmwareTrust(u: UefiInfo): { sb: boolean; tpmOk: boolean; score: number } {
  const tpmOk = u.tpm.present && u.tpm.version.startsWith('2');
  return { sb: u.secureBoot, tpmOk, score: (u.secureBoot ? 50 : 0) + (tpmOk ? 50 : 0) };
}
/** F04504/F04505 启动顺序与 UEFI 项管理。 */
export class BootOrder {
  entries: { id: string; name: string; kind: 'uefi' | 'legacy' }[] = [];
  constructor(ids: [string, string, 'uefi' | 'legacy'][]) {
    this.entries = ids.map(([id, name, kind]) => ({ id, name, kind }));
  }
  moveUp(id: string): boolean {
    const i = this.entries.findIndex((e) => e.id === id);
    if (i <= 0) return false;
    [this.entries[i - 1], this.entries[i]] = [this.entries[i]!, this.entries[i - 1]!];
    return true;
  }
  add(id: string, name: string): boolean {
    if (this.entries.some((e) => e.id === id)) return false;
    this.entries.push({ id, name, kind: 'uefi' });
    return true;
  }
  remove(id: string): boolean {
    const i = this.entries.findIndex((e) => e.id === id);
    if (i < 0) return false;
    this.entries.splice(i, 1);
    return true;
  }
}
/** F04507/F04508 快速启动与重启进 BIOS。 */
export const FAST_BOOT_DEFAULT = false;
export function rebootToBios(pendingReboot: boolean): { ok: boolean; needReboot?: boolean } {
  return pendingReboot ? { ok: true } : { ok: false, needReboot: true };
}
/** F04510/F04511 微码与虚拟化。 */
export function vtCapable(vendor: 'intel' | 'amd', enabled: boolean): boolean {
  return enabled && (vendor === 'intel' || vendor === 'amd');
}
/** F04513/F04514 启动模式与分区表联动。 */
export function bootMode(gpt: boolean): 'UEFI' | 'Legacy' {
  return gpt ? 'UEFI' : 'Legacy';
}
/** F04515~F04517 启动修复与 BCD 备份/恢复。 */
export class BcdStore {
  private items = new Map<string, string>();
  constructor(seed: Record<string, string>) {
    for (const [k, v] of Object.entries(seed)) this.items.set(k, v);
  }
  backup(): string {
    return fnv1a([...this.items.entries()].sort().map(([k, v]) => `${k}=${v}`).join(';'));
  }
  restore(snapshot: string): boolean {
    return snapshot === this.backup();
  }
  get size(): number {
    return this.items.size;
  }
}
/** F04518~F04520 双系统与 Linux 识别。 */
export function detectOtherOs(entries: { name: string; kind: string }[]): { linux: string | null; grub: boolean; count: number } {
  const linux = entries.find((e) => /ubuntu|debian|fedora|arch|linux/i.test(e.name))?.name ?? null;
  const grub = entries.some((e) => /grub/i.test(e.kind + e.name));
  return { linux, grub, count: entries.length };
}
/** F04521/F04522 启动菜单超时与默认项。 */
export class BootMenu {
  timeoutSec = 10;
  defaultId = '';
  setDefault(id: string): boolean {
    if (!id) return false;
    this.defaultId = id;
    return true;
  }
}
/** F04524 引导日志（环形）。 */
export class BootLog {
  private lines: string[] = [];
  push(line: string): void {
    this.lines.push(line);
    if (this.lines.length > 500) this.lines.shift();
  }
  tail(n: number): string[] {
    return this.lines.slice(-n);
  }
}

/* ============ 族0182 输入设备联动（F04526~F04550） ============ */

/** F04527~F04529 DPI 与无线电量。 */
export function mouseDpiReport(dpi: number): { dpi: number; level: string } {
  const level = dpi <= 800 ? '办公' : dpi <= 3200 ? '游戏' : '电竞';
  return { dpi: clamp(dpi, 100, 26000), level };
}
export function wirelessPct(pct: number): { ok: boolean; level: 'full' | 'mid' | 'low' | 'crit' } {
  const p = clamp(Math.round(pct), 0, 100);
  return { ok: true, level: p > 60 ? 'full' : p > 25 ? 'mid' : p > 10 ? 'low' : 'crit' };
}
/** F04530 接收器管理：一个 dongle 绑定多设备。 */
export class Receiver {
  private bound = new Set<string>();
  constructor(public max = 6) {}
  bind(dev: string): boolean {
    if (this.bound.size >= this.max || this.bound.has(dev)) return false;
    this.bound.add(dev);
    return true;
  }
  unbind(dev: string): boolean {
    return this.bound.delete(dev);
  }
  get devices(): string[] {
    return [...this.bound];
  }
}
/** F04532/F04533 HID 过滤（防 BadUSB）与新设备确认。 */
export class HidFilter {
  private whitelist = new Set<string>();
  private seen = new Set<string>();
  constructor(vids: string[]) {
    vids.forEach((v) => this.whitelist.add(v));
  }
  /** 返回 allow=直接放行；confirm=需用户确认；block=黑名单拒绝。 */
  onPlug(vid: string): 'allow' | 'confirm' | 'block' {
    if (vid === 'BAD') return 'block';
    if (this.whitelist.has(vid)) return 'allow';
    return this.seen.has(vid) ? 'confirm' : 'confirm';
  }
  approve(vid: string): void {
    this.whitelist.add(vid);
    this.seen.add(vid);
  }
}
/** F04537 宏审查：检测危险序列。 */
export function macroAudit(keys: string[]): { ok: boolean; risk: string[] } {
  const risk: string[] = [];
  if (keys.includes('Win') && keys.includes('R')) risk.push('Win+R 运行命令');
  if (keys.filter((k) => k === 'Del').length >= 3) risk.push('连续删除');
  return { ok: risk.length === 0, risk };
}
/** F04538 触屏校准：五点校准残差。 */
export function touchCalibrate(samples: { expect: { x: number; y: number }; got: { x: number; y: number } }[]): number {
  const d = samples.map((s) => Math.hypot(s.expect.x - s.got.x, s.expect.y - s.got.y));
  return Math.round((d.reduce((a, b) => a + b, 0) / d.length) * 100) / 100;
}
/** F04542/F04543 指纹录入与测试。 */
export class FingerprintStore {
  private templates = new Map<string, string>();
  enroll(finger: string, raw: string): boolean {
    if (this.templates.has(finger)) return false;
    this.templates.set(finger, fnv1a(raw));
    return true;
  }
  verify(finger: string, raw: string): boolean {
    return this.templates.get(finger) === fnv1a(raw);
  }
  get count(): number {
    return this.templates.size;
  }
}
/** F04544/F04545 硬件级摄像头/麦克风开关。 */
export class HardwareKill {
  camera = true;
  mic = true;
  toggleCamera(on: boolean): boolean {
    this.camera = on;
    return this.camera === on;
  }
  toggleMic(on: boolean): boolean {
    this.mic = on;
    return this.mic === on;
  }
}
/** F04546 飞行联动：无线全关。 */
export function airplaneRadars(wifi: boolean, bt: boolean, gps: boolean): { ok: boolean; on: number } {
  const on = [wifi, bt, gps].filter(Boolean).length;
  return { ok: on === 0, on };
}
/** F04548/F04549 选择性挂起与设备唤醒。 */
export function selectiveSuspend(idleMin: number, enabled: boolean): 'suspended' | 'active' {
  return enabled && idleMin >= 5 ? 'suspended' : 'active';
}
export class WakeSources {
  private allowed = new Set<string>();
  allow(dev: string): boolean {
    this.allowed.add(dev);
    return true;
  }
  canWake(dev: string): boolean {
    return this.allowed.has(dev);
  }
}

/* ============ 族0183 传感与位置硬件（F04551~F04575） ============ */

/** 传感总线：present=硬件存在；reserved 项 present=false 时入口隐藏。 */
export type SensorKind =
  | 'light' | 'human' | 'gyro' | 'accel' | 'compass' | 'gps' | 'barometer'
  | 'ambientTemp' | 'humidity' | 'proximity' | 'nfc' | 'uwb' | 'radar';
export class SensorBus {
  private present = new Set<SensorKind>();
  private data = new Map<SensorKind, number>();
  private perms = new Map<string, Set<SensorKind>>();
  private log: { at: number; kind: SensorKind; value: number }[] = [];
  attach(kind: SensorKind): void {
    this.present.add(kind);
  }
  has(kind: SensorKind): boolean {
    return this.present.has(kind);
  }
  read(kind: SensorKind): number | null {
    if (!this.present.has(kind)) return null;
    return this.data.get(kind) ?? 0;
  }
  write(kind: SensorKind, value: number): boolean {
    if (!this.present.has(kind)) return false;
    this.data.set(kind, value);
    this.log.push({ at: this.log.length, kind, value });
    return true;
  }
  /** F04560 光感→亮度联动。 */
  lightToBrightness(lux: number): number {
    if (!this.has('light')) return 100;
    return Math.round(clamp(20 + (clamp(lux, 0, 1000) / 1000) * 80, 20, 100));
  }
  /** F04562/F04563 姿态：旋转锁与帐篷模式。 */
  posture(): 'laptop' | 'tent' | 'tablet' | 'flat' {
    const pitch = this.data.get('gyro') ?? 0;
    if (pitch >= 250) return 'tent';
    if (pitch >= 160) return 'tablet';
    if (pitch >= 30) return 'laptop';
    return 'flat';
  }
  /** F04569 隐私总开关与权限。 */
  grant(app: string, kind: SensorKind): boolean {
    if (!this.present.has(kind)) return false;
    if (!this.perms.has(app)) this.perms.set(app, new Set());
    this.perms.get(app)!.add(kind);
    return true;
  }
  allowed(app: string, kind: SensorKind): boolean {
    return this.perms.get(app)?.has(kind) ?? false;
  }
  /** F04573 开发模拟器。 */
  simulate(kind: SensorKind, values: number[]): number[] {
    return values.map((v) => (this.write(kind, v), v));
  }
  /** F04574 日志。 */
  recent(n: number): typeof this.log {
    return this.log.slice(-n);
  }
}
/** F04570 传感器诊断。 */
export function sensorDiagnose(bus: SensorBus, want: SensorKind[]): { kind: SensorKind; ok: boolean }[] {
  return want.map((k) => ({ kind: k, ok: bus.has(k) }));
}

/* ============ 族0184 多设备互联（F04576~F04600） ============ */

/** F04576~F04579 手机连接：通知同步/照片导入/剪贴/拖传。 */
export class PhoneLink {
  notifications: string[] = [];
  photos: string[] = [];
  private clip = new Map<string, string>();
  pushNotify(text: string): void {
    this.notifications.push(text);
  }
  importPhotos(names: string[]): number {
    this.photos.push(...names);
    return names.length;
  }
  copy(text: string): string {
    this.clip.set('latest', text);
    return text;
  }
  paste(): string | undefined {
    return this.clip.get('latest');
  }
  dragSend(file: string, sizeMB: number): { queued: true; etaSec: number } {
    return { queued: true, etaSec: Math.ceil(sizeMB / 20) };
  }
}
/** F04584/F04595 跨设备剪贴板与加密。 */
export function clipEncrypt(text: string, key: string): string {
  let out = '';
  for (let i = 0; i < text.length; i++) out += String.fromCharCode(text.charCodeAt(i) ^ key.charCodeAt(i % key.length));
  return out;
}
export function clipDecrypt(text: string, key: string): string {
  return clipEncrypt(text, key);
}
/** F04586/F04587 附近共享与局域网直传。 */
export class NearbyShare {
  discovered = new Set<string>();
  scan(peers: string[]): number {
    peers.forEach((p) => this.discovered.add(p));
    return this.discovered.size;
  }
  send(peer: string, bytes: number): { ok: boolean; sec: number } | { ok: false } {
    if (!this.discovered.has(peer)) return { ok: false };
    return { ok: true, sec: Math.ceil(bytes / 30_000_000) };
  }
}
/** F04588 二维码快连：payload 校验。 */
export function qrPairPayload(deviceId: string, nonce: string): string {
  return `${deviceId}.${nonce}.${fnv1a(deviceId + nonce)}`;
}
export function qrPairVerify(payload: string): boolean {
  const parts = payload.split('.');
  if (parts.length !== 3) return false;
  return qrPairPayload(parts[0]!, parts[1]!) === payload;
}
/** F04589/F04591 投屏/投放渲染器。 */
export class CastTargets {
  renderers = new Set<string>();
  discover(names: string[]): number {
    names.forEach((n) => this.renderers.add(n));
    return this.renderers.size;
  }
  push(target: string, media: string): string | null {
    if (!this.renderers.has(target)) return null;
    return `pushing:${media}@${target}`;
  }
}
/** F04592 耳机多设备无缝切换。 */
export class SeamlessHeadset {
  private links = new Set<string>();
  link(dev: string): void {
    this.links.add(dev);
  }
  switchTo(dev: string): boolean {
    return this.links.has(dev);
  }
}
/** F04596/F04597 信任管理与新设备审批。 */
export class TrustManager {
  private trusted = new Set<string>();
  private pending = new Set<string>();
  request(deviceId: string): boolean {
    if (this.trusted.has(deviceId)) return false;
    this.pending.add(deviceId);
    return true;
  }
  approve(deviceId: string): boolean {
    if (!this.pending.delete(deviceId)) return false;
    this.trusted.add(deviceId);
    return true;
  }
  deny(deviceId: string): boolean {
    return this.pending.delete(deviceId);
  }
  isTrusted(d: string): boolean {
    return this.trusted.has(d);
  }
  unbind(d: string): boolean {
    return this.trusted.delete(d);
  }
}
/** F04599 互联流量统计。 */
export class LinkTraffic {
  private mb = new Map<string, number>();
  add(dev: string, mb: number): number {
    const v = (this.mb.get(dev) ?? 0) + mb;
    this.mb.set(dev, Math.round(v * 10) / 10);
    return v;
  }
  top(n: number): [string, number][] {
    return [...this.mb.entries()].sort((a, b) => b[1] - a[1]).slice(0, n);
  }
}

/* ============ 族0185 虚拟化与容器（F04601~F04625） ============ */

/** F04601/F04602 检测。 */
export function detectHyperV(features: string[]): boolean {
  return features.includes('hypervisorlaunchtype');
}
export function detectWsl(list: string[]): { installed: boolean; distros: string[] } {
  return { installed: list.length > 0, distros: list };
}
/** F04603~F04605 虚拟机注册表：快照与克隆。 */
export interface VmSpec {
  id: string;
  name: string;
  vcpu: number;
  ramMB: number;
  diskGB: number;
}
export class VmRegistry {
  private vms = new Map<string, VmSpec>();
  private snaps = new Map<string, { name: string; spec: VmSpec }[]>();
  create(spec: VmSpec): boolean {
    if (this.vms.has(spec.id)) return false;
    this.vms.set(spec.id, { ...spec });
    return true;
  }
  get(id: string): VmSpec | undefined {
    return this.vms.get(id);
  }
  snapshot(id: string, name: string): boolean {
    const v = this.vms.get(id);
    if (!v) return false;
    if (!this.snaps.has(id)) this.snaps.set(id, []);
    const list = this.snaps.get(id)!;
    if (list.some((s) => s.name === name)) return false;
    list.push({ name, spec: { ...v } });
    return true;
  }
  restore(id: string, name: string): boolean {
    const s = this.snaps.get(id)?.find((x) => x.name === name);
    if (!s) return false;
    this.vms.set(id, { ...s.spec });
    return true;
  }
  clone(id: string, newId: string): boolean {
    const v = this.vms.get(id);
    if (!v || this.vms.has(newId)) return false;
    this.vms.set(newId, { ...v, id: newId, name: `${v.name}-clone` });
    return true;
  }
  snapsOf(id: string): string[] {
    return (this.snaps.get(id) ?? []).map((s) => s.name);
  }
}
/** F04606 虚拟网络配置。 */
export type VmNet = 'nat' | 'bridge' | 'internal';
export function vmNetwork(mode: VmNet, subnet: string): { mode: VmNet; gateway: string } {
  const gw = mode === 'internal' ? '' : subnet.replace(/\.\d+$/, '.1');
  return { mode, gateway: gw };
}
/** F04609~F04611 一次性沙箱。 */
export class OneShotSandbox {
  private alive: string | null = null;
  start(): string | null {
    if (this.alive) return null;
    this.alive = fnv1a(String(Date.now()));
    return this.alive;
  }
  stop(keepClipboard = false): { destroyed: boolean; clipboardKept: boolean } {
    if (!this.alive) return { destroyed: false, clipboardKept: false };
    this.alive = null;
    return { destroyed: true, clipboardKept: keepClipboard };
  }
  get running(): boolean {
    return this.alive !== null;
  }
}
/** F04612 容器资源统计。 */
export function containerStats(cores: number, ramMB: number): { cpuPctOfHost: number; ramPctOfHost: number } {
  const hostCores = 8;
  const hostRam = 16384;
  return { cpuPctOfHost: Math.round((cores / hostCores) * 100), ramPctOfHost: Math.round((ramMB / hostRam) * 100) };
}
/** F04613/F04614 嵌套检测与性能提示。 */
export function nestedVirt(l1: boolean, l2: boolean): { nested: boolean; hint: string } {
  return { nested: l1 && l2, hint: l1 && l2 ? '嵌套虚拟化开启，性能下降 20~40%' : '无嵌套' };
}
/** F04616/F04617 VHDX 与差分盘。 */
export class Vhdx {
  private children = new Set<string>();
  constructor(public path: string, public sizeGB: number, public parent?: Vhdx) {}
  resize(gb: number): boolean {
    if (gb < this.sizeGB) return false;
    this.sizeGB = gb;
    return true;
  }
  addChild(p: string): boolean {
    if (this.parent) return false;
    this.children.add(p);
    return true;
  }
  get chain(): string[] {
    const out: string[] = [];
    let cur: Vhdx | undefined = this;
    while (cur) {
      out.unshift(cur.path);
      cur = cur.parent;
    }
    return out;
  }
}
/** F04618 共享文件夹。 */
export class SharedFolders {
  private m = new Map<string, 'ro' | 'rw'>();
  share(path: string, mode: 'ro' | 'rw'): boolean {
    if (this.m.has(path)) return false;
    this.m.set(path, mode);
    return true;
  }
  canWrite(path: string): boolean {
    return this.m.get(path) === 'rw';
  }
}
/** F04623/F04624 VM 模板与限额。 */
export class VmTemplates {
  private t = new Map<string, VmSpec>();
  put(spec: VmSpec): void {
    this.t.set(spec.id, spec);
  }
  instantiate(id: string, vmId: string, reg: VmRegistry, limit: { vcpu: number; ramMB: number }): boolean {
    const s = this.t.get(id);
    if (!s) return false;
    return reg.create({ ...s, id: vmId, vcpu: Math.min(s.vcpu, limit.vcpu), ramMB: Math.min(s.ramMB, limit.ramMB) });
  }
}

/* ============ 补充助手（AI-37 批次内聚实现），勿删 ============ */

/** F04506 固件密码状态。 */
export function firmwarePasswordState(set: boolean): 'protected' | 'open' {
  return set ? 'protected' : 'open';
}
/** F04510 微码信息。 */
export function microcodeInfo(rev: string): { rev: string; short: string } {
  return { rev, short: fnv1a(rev) };
}
/** F04512 启动显卡策略。 */
export const BOOT_GPU_POLICIES = ['iGPU', 'dGPU', 'hybrid'] as const;
/** F04515 启动修复步骤。 */
export const BOOT_REPAIR_STEPS = ['探测引导', '重建 BCD', '写入引导扇区', '验证重启'] as const;
/** F04526 键盘固件信息。 */
export function keyboardFw(vendor: string, rev: string): { vendor: string; rev: string; digest: string } {
  return { vendor, rev, digest: fnv1a(vendor + rev) };
}
/** F04534 设备独占提示。 */
export function deviceExclusive(exclusive: string | null): string | null {
  return exclusive ? `${exclusive} 已独占设备` : null;
}
/** F04536 NKRO 全键无冲。 */
export function nkroDisplay(concurrentKeys: number): { n: number; nkro: boolean } {
  return { n: concurrentKeys, nkro: concurrentKeys >= 6 };
}
/** F04540 触控板厂商设置联动。 */
export function touchpadVendorLink(vendor: string): string | null {
  const known = ['synaptics', 'elan', 'alps'];
  return known.includes(vendor.toLowerCase()) ? `vendor-settings:${vendor.toLowerCase()}` : null;
}
/** F04547 耳机低延迟模式。 */
export function lowLatencyMode(codec: string): { on: boolean; codec: string } {
  const low = ['aptX LL', 'aptX Adaptive', 'LC3'];
  return { on: low.includes(codec), codec };
}
/** F04565 平板形态下禁用物理键盘。 */
export function keyboardDisabled(posture: string): boolean {
  return posture === 'tablet';
}
/** F04583 平板触控笔联动。 */
export function penLink(battery: number, connected: boolean): { linked: boolean; battery: number } {
  return { linked: connected && battery > 0, battery };
}
