// AURORA-10000: AI-36 批次（族0176~0180 · 显示与显卡/音频系统/电池与电源/外设中心/存储介质），勿删。
import { clamp, semverCmp } from './hwModel';

/* ============ 族0176 显示与显卡（F04376~F04400） ============ */

/** F04376~F04379 HDR 档：峰值亮度/SDR 亮度/每屏独立。 */
export interface HdrProfile {
  enabled: boolean;
  peakNits: number;
  sdrBrightness: number;
  perDisplay: Record<string, HdrProfile>;
}
export function makeHdr(enabled: boolean, peakNits: number, sdrBrightness: number): HdrProfile | null {
  if (peakNits < 400 || peakNits > 2000) return null;
  if (sdrBrightness < 0 || sdrBrightness > 100) return null;
  return { enabled, peakNits, sdrBrightness, perDisplay: {} };
}
/** F04382 DDC/CI 亮度直控（0~100 钳制）。 */
export function ddcSetBrightness(_cur: number, next: number): number {
  return clamp(next, 0, 100);
}
/** F04380/F04381 夜灯与色温计划：按小时返回色温（K）。 */
export function kelvinForHour(plan: { hour: number; kelvin: number }[], hour: number): number {
  const sorted = [...plan].sort((a, b) => a.hour - b.hour);
  let best = sorted[0]!.kelvin;
  for (const p of sorted) if (hour >= p.hour) best = p.kelvin;
  return clamp(best, 2700, 6500);
}
/** F04384/F04385 刷新率与 VRR。 */
export function pickRefresh(modes: number[], want: number): number | null {
  const avail = [...new Set(modes)].sort((a, b) => b - a);
  return avail.includes(want) ? want : null;
}
export function vrrRange(min: number, max: number): { ok: boolean; min: number; max: number } {
  return { ok: max > min && min >= 24, min, max };
}
/** F04387 缩放建议：按像素密度档位。 */
export function scalingSuggest(ppi: number): string {
  if (ppi < 100) return '100%';
  if (ppi < 140) return '125%';
  if (ppi < 200) return '150%';
  return '200%';
}
/** F04388~F04390 GPU 信息与驱动检查/回滚。 */
export interface GpuInfo {
  model: string;
  driver: string;
  vramMB: number;
}
export function driverOutdated(cur: string, latest: string): boolean {
  return semverCmp(latest, cur) > 0;
}
export function driverRollback(history: string[]): string | null {
  return history.length >= 2 ? history[history.length - 2]! : null;
}
/** F04395 显示方案（拓扑）保存与恢复。 */
export class TopologyStore {
  private m = new Map<string, string>();
  save(name: string, layout: string): boolean {
    if (this.m.has(name)) return false;
    this.m.set(name, layout);
    return true;
  }
  load(name: string): string | null {
    return this.m.get(name) ?? null;
  }
  list(): string[] {
    return [...this.m.keys()];
  }
}
/** F04396 投影四模式。 */
export const PROJECTION_MODES = ['copy', 'extend', 'second', 'only'] as const;
/** F04399 显示器使用时长（小时累计）。 */
export class DisplayHealth {
  private hours = 0;
  addUsage(h: number): number {
    this.hours = Math.round((this.hours + clamp(h, 0, 24)) * 10) / 10;
    return this.hours;
  }
  get totalHours(): number {
    return this.hours;
  }
}

/* ============ 族0177 音频系统（F04401~F04425） ============ */

/** F04401/F04402 设备切换。 */
export class AudioRouter {
  outputs: string[] = [];
  inputs: string[] = [];
  activeOut = '';
  activeIn = '';
  switchOutput(name: string): boolean {
    if (!this.outputs.includes(name)) return false;
    this.activeOut = name;
    return true;
  }
  switchInput(name: string): boolean {
    if (!this.inputs.includes(name)) return false;
    this.activeIn = name;
    return true;
  }
}
/** F04404/F04405 混音器：分应用音量与每应用记忆。 */
export class AppMixer {
  private vols = new Map<string, number>();
  private mem = new Map<string, number>();
  setVolume(app: string, v: number): number {
    const c = clamp(v, 0, 100);
    this.vols.set(app, c);
    return c;
  }
  volume(app: string): number {
    return this.vols.get(app) ?? 100;
  }
  remember(app: string): void {
    this.mem.set(app, this.volume(app));
  }
  recall(app: string): number {
    return this.mem.get(app) ?? this.volume(app);
  }
}
/** F04408 响度均衡：按目标 LUFS 归一增益（dB）。 */
export function loudnessGain(currentLufs: number, targetLufs = -16): number {
  return Math.round((targetLufs - currentLufs) * 10) / 10;
}
/** F04415 左右平衡 -100~100。 */
export function balancePan(v: number): { l: number; r: number } {
  const c = clamp(v, -100, 100);
  return { l: c < 0 ? 100 : 100 - c, r: c > 0 ? 100 : 100 + c };
}
/** F04416/F04417 EQ：预设带增益与自定义曲线。 */
export const EQ_PRESETS: Record<string, number[]> = {
  pop: [3, 2, 0, -1, 2],
  classical: [1, 0, 0, 1, 2],
  vocal: [-2, 0, 3, 2, 0],
};
export function applyEq(bands: number[], gainDb: number[]): number[] {
  return bands.map((b, i) => clamp(b + (gainDb[i] ?? 0), -12, 12));
}
/** F04418~F04420 诊断/无声向导/爆音保护。 */
export const AUDIO_DIAG_STEPS = ['设备枚举', '默认输出', '音量与静音', '独占占用', '驱动状态'];
export function popGuard(sample: number, ceiling = 0.98): number {
  return clamp(sample, -ceiling, ceiling);
}
/** F04421/F04422 渐入与夜间上限。 */
export function volumeRamp(step: number, max = 100): number {
  return Math.round(clamp(step * max * 0.1, 0, max));
}
export function nightCap(v: number, cap: number): number {
  return clamp(v, 0, cap);
}
/** F04423 蓝牙编码显示。 */
export const BT_CODECS = ['SBC', 'AAC', 'aptX', 'aptX HD', 'LDAC'] as const;
export function codecRank(c: string): number {
  return BT_CODECS.indexOf(c as (typeof BT_CODECS)[number]);
}

/* ============ 族0178 电池与电源（F04426~F04450） ============ */

/** F04426~F04431 电池健康模型。 */
export class Battery {
  constructor(public designMwh: number, public fullMwh: number, public cycles: number, public chargeMwh = 0) {}
  get healthPct(): number {
    return Math.round((this.fullMwh / this.designMwh) * 1000) / 10;
  }
  get wearPct(): number {
    return Math.round((100 - this.healthPct) * 10) / 10;
  }
  get pct(): number {
    return clamp(Math.round((this.chargeMwh / this.fullMwh) * 100), 0, 100);
  }
  /** F04427 剩余时间（分钟），按当前放电功率。 */
  remainingMinutes(drawMw: number): number {
    if (drawMw <= 0) return Infinity;
    return Math.floor((this.chargeMwh / drawMw) * 60);
  }
}
/** F04432 充电上限（养护）：到阈值即停。 */
export class ChargeLimit {
  constructor(public limit = 80) {}
  tick(chargePct: number): 'charging' | 'hold' | 'resume' {
    if (chargePct < this.limit - 5) return 'resume';
    if (chargePct >= this.limit) return 'hold';
    return 'charging';
  }
}
/** F04433 快充判定。 */
export function fastCharging(watts: number): boolean {
  return watts >= 45;
}
/** F04434/F04435 耗电排行。 */
export function powerRank(items: { name: string; mw: number }[]): { name: string; mw: number }[] {
  return [...items].sort((a, b) => b.mw - a.mw);
}
/** F04436/F04437 省电与三档。 */
export type PowerMode = 'saver' | 'balanced' | 'performance';
export function autoSaver(pct: number, threshold = 20): PowerMode | null {
  return pct <= threshold ? 'saver' : null;
}
/** F04439~F04443 行为策略枚举。 */
export const LID_ACTIONS = ['sleep', 'hibernate', 'shutdown', 'nothing'] as const;
export const BUTTON_ACTIONS = ['sleep', 'hibernate', 'shutdown', 'nothing'] as const;
export const CRITICAL_ACTIONS = ['hibernate', 'shutdown'] as const;
/** F04445 耗电异常：放电速率突增检测。 */
export function drainAnomaly(rateNow: number, rateAvg: number): boolean {
  return rateNow > rateAvg * 1.5 && rateNow > 500;
}
/** F04448 习惯化续航预估。 */
export function enduranceEstimate(fullMwh: number, dailyDrawMwh: number[]): number {
  const avg = dailyDrawMwh.reduce((s, v) => s + v, 0) / Math.max(1, dailyDrawMwh.length);
  if (avg <= 0) return Infinity;
  return Math.round((fullMwh / avg) * 24);
}

/* ============ 族0179 外设中心（F04451~F04475） ============ */

/** F04451/F04452 友好版设备管理器。 */
export interface HwDevice {
  id: string;
  name: string;
  kind: string;
  enabled: boolean;
  driver: string;
}
export class DeviceCenter {
  devices: HwDevice[] = [];
  add(d: HwDevice): void {
    this.devices.push(d);
  }
  connected(): HwDevice[] {
    return this.devices.filter((d) => d.enabled);
  }
  toggle(id: string): boolean {
    const d = this.devices.find((x) => x.id === id);
    if (!d) return false;
    d.enabled = !d.enabled;
    return true;
  }
  rename(id: string, alias: string): boolean {
    const d = this.devices.find((x) => x.id === id);
    if (!d || !alias.trim()) return false;
    d.name = alias;
    return true;
  }
}
/** F04453/F04454 蓝牙配对流程。 */
export class BluetoothMgr {
  paired = new Set<string>();
  pairing: { target: string; pin: string } | null = null;
  beginPair(target: string, rng: () => number): string {
    this.pairing = { target, pin: String(100000 + Math.floor(rng() * 899999)) };
    return this.pairing.pin;
  }
  confirm(pin: string): boolean {
    if (!this.pairing || this.pairing.pin !== pin) return false;
    this.paired.add(this.pairing.target);
    this.pairing = null;
    return true;
  }
  unpair(target: string): boolean {
    return this.paired.delete(target);
  }
}
/** F04460/F04463 手柄震动与数位板压感测试。 */
export function rumbleTest(motor: 'left' | 'right' | 'both', strength: number): boolean {
  return strength > 0 && strength <= 100 && (motor === 'both' || motor === 'left' || motor === 'right');
}
export function pressureTest(levels: number[]): boolean {
  return levels.length === 8 && levels.every((l, i) => l === i + 1);
}
/** F04464 U 盘安全弹出：有占用句柄时拒绝。 */
export function safeEject(openHandles: number): { ok: boolean; reason?: string } {
  if (openHandles > 0) return { ok: false, reason: '有程序正在使用设备' };
  return { ok: true };
}
/** F04465 移动硬盘健康。 */
export function externalHealth(smartOk: boolean, tempC: number): 'good' | 'watch' | 'bad' {
  if (!smartOk) return 'bad';
  if (tempC >= 60) return 'watch';
  return 'good';
}
/** F04467/F04468 USB 信息与速度分级。 */
export function usbSpeed(gbps: number): '2.0' | '3.0' | '3.1' | '3.2' | '4.0' | 'unknown' {
  if (gbps <= 0.48) return '2.0';
  if (gbps <= 5) return '3.0';
  if (gbps <= 10) return '3.1';
  if (gbps <= 20) return '3.2';
  if (gbps <= 40) return '4.0';
  return 'unknown';
}
/** F04470 设备故障诊断步骤。 */
export const DEVICE_DIAG_STEPS = ['枚举', '供电', '驱动加载', '带宽分配', '固件握手'];
/** F04474 新设备接入通知。 */
export class ArrivalNotify {
  seen = new Set<string>();
  pending: string[] = [];
  onArrive(id: string): boolean {
    if (this.seen.has(id)) return false;
    this.seen.add(id);
    this.pending.push(id);
    return true;
  }
  ackAll(): number {
    const n = this.pending.length;
    this.pending = [];
    return n;
  }
}

/* ============ 族0180 存储介质（F04476~F04500） ============ */

/** F04476~F04480 盘信息与 SMART。 */
export interface SmartAttrs {
  reallocated: number;
  pendingSectors: number;
  tempC: number;
  pctRemaining: number;
}
export function smartHealth(a: SmartAttrs): 'good' | 'watch' | 'bad' {
  if (a.reallocated > 100 || a.pendingSectors > 10 || a.pctRemaining < 60) return 'bad';
  if (a.reallocated > 0 || a.pendingSectors > 0 || a.tempC >= 65 || a.pctRemaining < 85) return 'watch';
  return 'good';
}
/** F04479 TBW 累计写入（TB）。 */
export class TbwMeter {
  private tb = 0;
  write(bytes: number): number {
    this.tb = Math.round((this.tb + bytes / 2 ** 40) * 1000) / 1000;
    return this.tb;
  }
  get total(): number {
    return this.tb;
  }
}
/** F04481/F04482 测速。 */
export function benchSpeed(rng: () => number, mb: number): { read: number; write: number } {
  const r = rng();
  return { read: Math.round(200 + r * 3000), write: Math.round((200 + r * 3000) * (mb > 1024 ? 0.8 : 1)) };
}
/** F04483 安全格式化：覆写次数计划。 */
export function secureFormatPasses(capacityGB: number): number {
  return capacityGB > 256 ? 3 : 1;
}
/** F04484~F04490 分区管理。 */
export interface Partition {
  letter: string;
  startMB: number;
  sizeMB: number;
  active: boolean;
}
export class DiskLayout {
  parts: Partition[] = [];
  private totalMB = 512 * 1024;
  constructor(totalMB?: number) {
    if (totalMB) this.totalMB = totalMB;
  }
  create(letter: string, startMB: number, sizeMB: number): boolean {
    if (!/^[A-Z]$/.test(letter)) return false;
    if (sizeMB < 16) return false;
    if (startMB < 0 || startMB + sizeMB > this.totalMB) return false;
    for (const p of this.parts) {
      if (p.letter === letter) return false;
      if (startMB < p.startMB + p.sizeMB && p.startMB < startMB + sizeMB) return false;
    }
    this.parts.push({ letter, startMB, sizeMB, active: false });
    return true;
  }
  remove(letter: string): boolean {
    const i = this.parts.findIndex((p) => p.letter === letter);
    if (i < 0) return false;
    this.parts.splice(i, 1);
    return true;
  }
  extend(letter: string, extraMB: number): boolean {
    const p = this.parts.find((x) => x.letter === letter);
    if (!p || extraMB <= 0) return false;
    const end = p.startMB + p.sizeMB + extraMB;
    for (const q of this.parts) {
      if (q.letter === letter) continue;
      if (end > q.startMB && q.startMB + q.sizeMB > p.startMB + p.sizeMB) return false;
    }
    p.sizeMB += extraMB;
    return true;
  }
  markActive(letter: string): boolean {
    const p = this.parts.find((x) => x.letter === letter);
    if (!p) return false;
    this.parts.forEach((q) => (q.active = q.letter === letter));
    return true;
  }
  get table(): 'GPT' | 'MBR' {
    return this.parts.length > 4 ? 'GPT' : 'MBR';
  }
}
/** F04491 4K 对齐检查。 */
export function aligned4k(offsetMB: number): boolean {
  return (offsetMB * 1024 * 1024) % 4096 === 0;
}
/** F04492/F04493 TRIM 与写缓存策略。 */
export type WriteCache = 'on' | 'off';
export function trimPolicy(ssd: boolean, osSupport: boolean): 'enabled' | 'disabled' | 'n/a' {
  if (!ssd) return 'n/a';
  return osSupport ? 'enabled' : 'disabled';
}
/** F04497/F04498 BitLocker 状态机。 */
export class BitLockerVol {
  state: 'off' | 'encrypting' | 'on' | 'locked' = 'off';
  private key = '';
  enable(recoveryKey: string): boolean {
    if (this.state !== 'off') return false;
    this.key = recoveryKey;
    this.state = 'encrypting';
    return true;
  }
  finishEncrypt(): void {
    if (this.state === 'encrypting') this.state = 'on';
  }
  unlock(key: string): boolean {
    if (this.state !== 'locked') return this.state === 'on';
    if (key === this.key) {
      this.state = 'on';
      return true;
    }
    return false;
  }
  lock(): boolean {
    if (this.state !== 'on') return false;
    this.state = 'locked';
    return true;
  }
  get locked(): boolean {
    return this.state === 'locked';
  }
}

/* ============ 补充助手（AI-36 批次内聚实现），勿删 ============ */

/** F04383 内容亮度自适应（能力位）。 */
export function adaptiveBrightnessCap(lux: number | null): 'supported' | 'reserved' {
  return lux === null ? 'reserved' : 'supported';
}
/** F04386 分辨率列表与切换。 */
const RES_STANDARD = ['3840x2160', '2560x1440', '1920x1080', '1366x768'] as const;
export function listResolutions(maxW: number): string[] {
  return RES_STANDARD.filter((r) => Number(r.split('x')[0]) <= maxW);
}
export function setResolution(avail: string[], want: string): boolean {
  return avail.includes(want);
}
/** F04403 耳机电量。 */
export function earbudBattery(l: number, r: number): { min: number; warn: boolean } {
  const min = Math.min(clamp(l, 0, 100), clamp(r, 0, 100));
  return { min, warn: min <= 20 };
}
/** F04406 独占占用提示。 */
export function exclusiveHint(inUse: string | null): string | null {
  return inUse ? `独占模式被 ${inUse} 占用` : null;
}
/** F04409 低音增强。 */
export function bassBoost(db: number): number {
  return clamp(db, -6, 12);
}
/** F04410 虚拟环绕上混。 */
export function virtualSurround(channels: number): { upmix: boolean; layout: string } {
  return channels >= 2 ? { upmix: true, layout: '2.0→5.1' } : { upmix: false, layout: 'mono' };
}
/** F04411 麦克风降噪。 */
export function micDenoise(snrInDb: number, reductionDb: number): number {
  return Math.round((snrInDb + clamp(reductionDb, 0, 30)) * 10) / 10;
}
/** F04412 侧音电平。 */
export function sidetone(level: number): number {
  return clamp(level, 0, 100);
}
/** F04413 回声消除残差。 */
export function aecResidual(withAec: boolean): number {
  return withAec ? -40 : -5;
}
/** F04414 回声测试回环。 */
export function echoTest(ms: number, budget = 200): { ok: boolean; ms: number } {
  return { ok: ms > 0 && ms <= budget, ms };
}
/** F04419 无声向导步骤。 */
export const NO_SOUND_STEPS = ['确认输出设备', '检查静音', '独占占用', '驱动与线缆'] as const;
/** F04437 三档电源模式表。 */
export const POWER_MODES: PowerMode[] = ['saver', 'balanced', 'performance'];
/** F04438 亮度策略。 */
export function brightnessFor(mode: PowerMode): number {
  return mode === 'performance' ? 100 : mode === 'balanced' ? 80 : 50;
}
/** F04439 睡眠计划（分钟，5~180 钳制）。 */
export function sleepTimeout(min: number): number {
  return clamp(min, 5, 180);
}
/** F04442 低电通知。 */
export function lowBattNotify(pct: number, threshold = 20): boolean {
  return pct <= threshold;
}
/** F04444 待机功耗分级。 */
export function standbyRate(watts: number): 'modern-standby' | 's3' | 'high' {
  if (watts <= 0.5) return 'modern-standby';
  if (watts <= 2) return 's3';
  return 'high';
}
/** F04446 耗电历史。 */
export class UsageHistory {
  private d: { day: string; mwh: number }[] = [];
  push(day: string, mwh: number): void {
    this.d.push({ day, mwh });
  }
  total(): number {
    return this.d.reduce((s, x) => s + x.mwh, 0);
  }
  get days(): number {
    return this.d.length;
  }
}
/** F04447 插拔记录。 */
export class ChargeHistory {
  private events: { at: number; plugged: boolean }[] = [];
  plug(at: number): void {
    this.events.push({ at, plugged: true });
  }
  unplug(at: number): void {
    this.events.push({ at, plugged: false });
  }
  get sessions(): number {
    return this.events.filter((e) => e.plugged).length;
  }
  get all(): { at: number; plugged: boolean }[] {
    return [...this.events];
  }
}
/** F04455 打印机管理。 */
export class PrinterMgr {
  private queue: string[] = [];
  default = '';
  setDefault(name: string): boolean {
    if (!name.trim()) return false;
    this.default = name;
    return true;
  }
  submit(job: string): number {
    this.queue.push(job);
    return this.queue.length;
  }
  cancel(): number {
    const n = this.queue.length;
    this.queue = [];
    return n;
  }
  get pending(): number {
    return this.queue.length;
  }
}
/** F04466 SD 卡速度等级。 */
export function sdCardClass(speedMBs: number): string {
  if (speedMBs >= 90) return 'UHS-III';
  if (speedMBs >= 30) return 'UHS-I';
  if (speedMBs >= 10) return 'Class 10';
  return 'Class 4';
}
/** F04472 设备图标映射。 */
export function iconFor(kind: string): string {
  const map: Record<string, string> = { keyboard: '⌨', mouse: '🖱', gamepad: '🎮', printer: '🖨', camera: '📷', mic: '🎙', storage: '💾' };
  return map[kind] ?? '🔌';
}
/** F04494 弹出策略。 */
export function ejectPolicy(fast: boolean): 'quick-removal' | 'better-performance' {
  return fast ? 'quick-removal' : 'better-performance';
}
