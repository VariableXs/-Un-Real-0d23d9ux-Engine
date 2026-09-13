/**
 * UNREAL-X-15000 · AI-29 硬件体验面（领域08 · 族0281~0290 · X07001~X07250）V 线逻辑模型，勿删。
 * 主责 Variable 桌面：显示显卡/音频/电池电源/外设中心/存储介质/输入联动/传感/互联 八族（V8），
 * 族0286 固件与族0290 虚拟化走 K 线（kernel/varix/src/checks/ai29.rs）。
 * 每族五层 × 五档 = 25 项；全部确定性算法、零 IO、可序列化。
 */

/* ------------------------------------------------------------------ */
/* 族0281 显示与显卡 2.0 X07001~X07025                                 */
/* ------------------------------------------------------------------ */

export type DisplayMode = 'eco' | 'standard' | 'vivid' | 'game' | 'creator';
export const DISPLAY_MODES: DisplayMode[] = ['eco', 'standard', 'vivid', 'game', 'creator'];
export const DEFAULT_DISPLAY_MODE: DisplayMode = 'standard';

export interface DisplayProfile {
  brightness: number; // 0~100 钳制
  refreshHz: number;
  colorGamut: string;
}
export const DISPLAY_MODE_MATRIX: Record<DisplayMode, DisplayProfile> = {
  eco: { brightness: 60, refreshHz: 60, colorGamut: 'srgb' },
  standard: { brightness: 80, refreshHz: 60, colorGamut: 'srgb' },
  vivid: { brightness: 90, refreshHz: 120, colorGamut: 'p3' },
  game: { brightness: 100, refreshHz: 144, colorGamut: 'p3' },
  creator: { brightness: 85, refreshHz: 60, colorGamut: 'adobe-rgb' },
};

/** 亮度钳制：非法值回 0~100。 */
export function clampBrightness(v: number): number {
  if (!Number.isFinite(v)) return 50;
  return Math.min(100, Math.max(0, Math.round(v)));
}

/** 刷新率白名单：非法回 60。 */
export function clampRefresh(hz: number): number {
  const allowed = [60, 90, 120, 144, 240];
  return allowed.includes(hz) ? hz : 60;
}

export class DisplayPipeline {
  modeId: DisplayMode;
  clamped = 0;
  layers: string[] = [];

  constructor(mode?: string) {
    this.modeId = (DISPLAY_MODES as string[]).includes(mode ?? '')
      ? (mode as DisplayMode)
      : DEFAULT_DISPLAY_MODE;
    if (mode !== undefined && this.modeId !== mode) this.clamped++;
  }

  get profile(): DisplayProfile {
    return DISPLAY_MODE_MATRIX[this.modeId];
  }

  /** 亮度换算：模式基准 + 偏移，钳制 0~100。 */
  brightnessWith(delta: number): number {
    return clampBrightness(this.profile.brightness + delta);
  }

  /** 合成链：图层按序压栈，重复去重。 */
  pushLayer(name: string): number {
    if (!this.layers.includes(name)) this.layers.push(name);
    return this.layers.length;
  }

  /** 帧预算（ms）：刷新率的确定性换算，240Hz 预算最紧。 */
  frameBudgetMs(): number {
    return Math.round(1000 / this.profile.refreshHz * 100) / 100;
  }
}

/* ------------------------------------------------------------------ */
/* 族0282 音频系统 2.0 X07026~X07050                                   */
/* ------------------------------------------------------------------ */

export type AudioScene = 'music' | 'movie' | 'voice' | 'game' | 'flat';
export const AUDIO_SCENES: AudioScene[] = ['music', 'movie', 'voice', 'game', 'flat'];
export const DEFAULT_AUDIO_SCENE: AudioScene = 'flat';

export interface EqProfile {
  bass: number;
  mid: number;
  treble: number;
}
export const AUDIO_SCENE_MATRIX: Record<AudioScene, EqProfile> = {
  music: { bass: 4, mid: 0, treble: 2 },
  movie: { bass: 5, mid: 1, treble: 0 },
  voice: { bass: -2, mid: 4, treble: 1 },
  game: { bass: 3, mid: 0, treble: 3 },
  flat: { bass: 0, mid: 0, treble: 0 },
};

/** 音量钳制 0~100，NaN 回 30。 */
export function clampVolume(v: number): number {
  if (!Number.isFinite(v)) return 30;
  return Math.min(100, Math.max(0, Math.round(v)));
}

/** dB 增益钳制 -12~+12。 */
export function clampGain(db: number): number {
  if (!Number.isFinite(db)) return 0;
  return Math.min(12, Math.max(-12, Math.round(db)));
}

export class AudioMixer {
  sceneId: AudioScene;
  clamped = 0;
  muted = false;
  private volumeLevel = 50;

  constructor(scene?: string) {
    this.sceneId = (AUDIO_SCENES as string[]).includes(scene ?? '')
      ? (scene as AudioScene)
      : DEFAULT_AUDIO_SCENE;
    if (scene !== undefined && this.sceneId !== scene) this.clamped++;
  }

  get eq(): EqProfile {
    return AUDIO_SCENE_MATRIX[this.sceneId];
  }

  setVolume(v: number): number {
    this.volumeLevel = clampVolume(v);
    if (!Number.isFinite(v) || v !== this.volumeLevel) this.clamped++;
    return this.volumeLevel;
  }

  get volume(): number {
    return this.muted ? 0 : this.volumeLevel;
  }

  /** 声道映射：单声道混音系数（0~100 音量 → 0~1）。 */
  mixFactor(): number {
    return Math.round((this.muted ? 0 : this.volumeLevel) / 100 * 100) / 100;
  }
}

/* ------------------------------------------------------------------ */
/* 族0283 电池电源 2.0 X07051~X07075                                   */
/* ------------------------------------------------------------------ */

export type PowerPlan = 'saver' | 'balanced' | 'performance' | 'studio' | 'custom';
export const POWER_PLANS: PowerPlan[] = ['saver', 'balanced', 'performance', 'studio', 'custom'];
export const DEFAULT_POWER_PLAN: PowerPlan = 'balanced';

export interface PowerProfile {
  cpuPercent: number; // 5~100
  dimAfterMin: number;
}
export const POWER_PLAN_MATRIX: Record<PowerPlan, PowerProfile> = {
  saver: { cpuPercent: 50, dimAfterMin: 1 },
  balanced: { cpuPercent: 80, dimAfterMin: 3 },
  performance: { cpuPercent: 100, dimAfterMin: 10 },
  studio: { cpuPercent: 100, dimAfterMin: 30 },
  custom: { cpuPercent: 80, dimAfterMin: 5 },
};

/** 电量钳制 0~100。 */
export function clampBattery(v: number): number {
  if (!Number.isFinite(v)) return 0;
  return Math.min(100, Math.max(0, Math.round(v)));
}

/** 剩余时长估算（min）：电量 × 容量系数 / 功耗瓦数，非法入参回 0。 */
export function batteryMinutes(pct: number, watt: number, capFactor: number): number {
  const p = clampBattery(pct);
  if (!Number.isFinite(watt) || watt <= 0 || !Number.isFinite(capFactor) || capFactor <= 0) return 0;
  return Math.round((p / 100) * capFactor * 60 / watt);
}

export class PowerManager {
  planId: PowerPlan;
  clamped = 0;
  charging = false;
  private battery = 100;

  constructor(plan?: string) {
    this.planId = (POWER_PLANS as string[]).includes(plan ?? '')
      ? (plan as PowerPlan)
      : DEFAULT_POWER_PLAN;
    if (plan !== undefined && this.planId !== plan) this.clamped++;
  }

  get profile(): PowerProfile {
    return POWER_PLAN_MATRIX[this.planId];
  }

  setBattery(v: number): number {
    const before = this.battery;
    this.battery = clampBattery(v);
    if (this.battery !== v) this.clamped++;
    return before;
  }

  get level(): number {
    return this.battery;
  }

  /** 低电量守卫：≤20% 且未充电 → 建议切省电。 */
  shouldSuggestSaver(): boolean {
    return this.battery <= 20 && !this.charging;
  }
}

/* ------------------------------------------------------------------ */
/* 族0284 外设中心 2.0 X07076~X07100                                   */
/* ------------------------------------------------------------------ */

export type PeripheralKind = 'mouse' | 'keyboard' | 'gamepad' | 'printer' | 'webcam';
export const PERIPHERAL_KINDS: PeripheralKind[] = ['mouse', 'keyboard', 'gamepad', 'printer', 'webcam'];

export interface Peripheral {
  id: string;
  kind: PeripheralKind;
  batteryPct: number; // 有线 = -1，无线 0~100
}

/** 外设电量校验：有线 -1 合法，无线钳制 0~100，非法回 -1。 */
export function clampPeripheralBattery(_kind: PeripheralKind, v: number): number {
  if (v === -1) return -1;
  if (!Number.isFinite(v)) return -1;
  return Math.min(100, Math.max(0, Math.round(v)));
}

export class PeripheralHub {
  clamped = 0;
  devices: Peripheral[] = [];

  add(id: string, kind: string, battery: number): Peripheral | null {
    if (!(PERIPHERAL_KINDS as string[]).includes(kind)) {
      this.clamped++;
      return null;
    }
    if (this.devices.some((d) => d.id === id)) return null;
    const dev: Peripheral = { id, kind: kind as PeripheralKind, batteryPct: clampPeripheralBattery(kind as PeripheralKind, battery) };
    this.devices.push(dev);
    return dev;
  }

  remove(id: string): boolean {
    const n = this.devices.length;
    this.devices = this.devices.filter((d) => d.id !== id);
    return this.devices.length < n;
  }

  /** 低电清单：无线且电量 ≤15%。 */
  lowBatteryIds(): string[] {
    return this.devices.filter((d) => d.batteryPct >= 0 && d.batteryPct <= 15).map((d) => d.id);
  }

  countByKind(kind: PeripheralKind): number {
    return this.devices.filter((d) => d.kind === kind).length;
  }
}

/* ------------------------------------------------------------------ */
/* 族0285 存储介质 2.0 X07101~X07125                                   */
/* ------------------------------------------------------------------ */

export type StorageKind = 'nvme' | 'sata' | 'usb' | 'sd' | 'ramdisk';
export const STORAGE_KINDS: StorageKind[] = ['nvme', 'sata', 'usb', 'sd', 'ramdisk'];

/** 介质健康分：写入量占比 → 确定性健康分（0~100）。 */
export function storageHealth(kind: StorageKind, usedPctOfLife: number): number {
  const base: Record<StorageKind, number> = { nvme: 100, sata: 95, usb: 85, sd: 80, ramdisk: 100 };
  const u = Math.min(100, Math.max(0, usedPctOfLife));
  return Math.round(base[kind] - u * 0.5);
}

export class StoragePool {
  clamped = 0;
  vols: Array<{ id: string; kind: StorageKind; usedGb: number; totalGb: number }> = [];

  add(id: string, kind: string, usedGb: number, totalGb: number): boolean {
    if (!(STORAGE_KINDS as string[]).includes(kind)) {
      this.clamped++;
      return false;
    }
    if (this.vols.some((v) => v.id === id)) return false;
    const total = totalGb > 0 && Number.isFinite(totalGb) ? totalGb : 1;
    const used = Math.min(total, Math.max(0, Number.isFinite(usedGb) ? usedGb : 0));
    this.vols.push({ id, kind: kind as StorageKind, usedGb: used, totalGb: total });
    return true;
  }

  freeGb(id: string): number {
    const v = this.vols.find((x) => x.id === id);
    return v ? Math.round((v.totalGb - v.usedGb) * 100) / 100 : 0;
  }

  /** 满盘预警：剩余 ≤10% 容量的卷。 */
  nearlyFullIds(): string[] {
    return this.vols.filter((v) => (v.totalGb - v.usedGb) / v.totalGb <= 0.1).map((v) => v.id);
  }

  totalUsedGb(): number {
    return Math.round(this.vols.reduce((s, v) => s + v.usedGb, 0) * 100) / 100;
  }
}

/* ------------------------------------------------------------------ */
/* 族0287 输入设备联动 2.0 X07151~X07175                               */
/* ------------------------------------------------------------------ */

export type InputProfile = 'office' | 'precision' | 'gaming' | 'presentation' | 'accessibility';
export const INPUT_PROFILES: InputProfile[] = ['office', 'precision', 'gaming', 'presentation', 'accessibility'];
export const DEFAULT_INPUT_PROFILE: InputProfile = 'office';

export interface InputParams {
  pointerSpeed: number; // 1~20 钳制
  scrollDir: 1 | -1;
}
export const INPUT_PROFILE_MATRIX: Record<InputProfile, InputParams> = {
  office: { pointerSpeed: 10, scrollDir: 1 },
  precision: { pointerSpeed: 4, scrollDir: 1 },
  gaming: { pointerSpeed: 14, scrollDir: 1 },
  presentation: { pointerSpeed: 8, scrollDir: 1 },
  accessibility: { pointerSpeed: 6, scrollDir: -1 },
};

/** 指针速度钳制 1~20，非法回 10。 */
export function clampPointerSpeed(v: number): number {
  if (!Number.isFinite(v)) return 10;
  return Math.min(20, Math.max(1, Math.round(v)));
}

export class InputRouter {
  profileId: InputProfile;
  clamped = 0;
  bindings: Array<{ key: string; action: string }> = [];

  constructor(profile?: string) {
    this.profileId = (INPUT_PROFILES as string[]).includes(profile ?? '')
      ? (profile as InputProfile)
      : DEFAULT_INPUT_PROFILE;
    if (profile !== undefined && this.profileId !== profile) this.clamped++;
  }

  get params(): InputParams {
    return INPUT_PROFILE_MATRIX[this.profileId];
  }

  /** 快捷键登记：重复键拒绝，返回登记后总数。 */
  bind(key: string, action: string): number {
    if (this.bindings.some((b) => b.key === key)) return this.bindings.length;
    this.bindings.push({ key, action });
    return this.bindings.length;
  }

  /** 冲突检测：同键双绑即冲突。 */
  conflictKeys(): string[] {
    const seen = new Set<string>();
    const dup: string[] = [];
    for (const b of this.bindings) {
      if (seen.has(b.key)) dup.push(b.key);
      seen.add(b.key);
    }
    return dup;
  }
}

/* ------------------------------------------------------------------ */
/* 族0288 传感位置 2.0 X07176~X07200                                   */
/* ------------------------------------------------------------------ */

export type SensorKind = 'light' | 'accel' | 'gyro' | 'gps' | 'compass';
export const SENSOR_KINDS: SensorKind[] = ['light', 'accel', 'gyro', 'gps', 'compass'];

/** 环境光 → 亮度建议（0~100）：三段确定性映射。 */
export function ambientToBrightness(lux: number): number {
  const v = Number.isFinite(lux) ? Math.max(0, lux) : 0;
  if (v < 50) return 30;
  if (v < 500) return 30 + Math.round((v - 50) / 450 * 50);
  return Math.min(100, 80 + Math.round((v - 500) / 100));
}

export class SensorFusion {
  clamped = 0;
  readings: Array<{ kind: SensorKind; value: number; ts: number }> = [];
  private counter = 0;

  push(kind: string, value: number): boolean {
    if (!(SENSOR_KINDS as string[]).includes(kind)) {
      this.clamped++;
      return false;
    }
    this.readings.push({ kind: kind as SensorKind, value: Number.isFinite(value) ? value : 0, ts: this.counter++ });
    return true;
  }

  latest(kind: SensorKind): number | null {
    const r = [...this.readings].reverse().find((x) => x.kind === kind);
    return r ? r.value : null;
  }

  /** 姿态判定：加速度模长偏差 → 平放/倾斜/运动 三态。 */
  static poseFromAccel(x: number, y: number, z: number): 'flat' | 'tilt' | 'motion' {
    const mag = Math.sqrt(x * x + y * y + z * z);
    const dev = Math.abs(mag - 9.8);
    if (dev > 1.5) return 'motion';
    if (dev > 0.3) return 'tilt';
    return 'flat';
  }
}

/* ------------------------------------------------------------------ */
/* 族0289 多设备互联 2.0 X07201~X07225                                 */
/* ------------------------------------------------------------------ */

export type LinkKind = 'cast' | 'clipboard' | 'file' | 'input' | 'audio';
export const LINK_KINDS: LinkKind[] = ['cast', 'clipboard', 'file', 'input', 'audio'];

export interface PeerDevice {
  id: string;
  trusted: boolean;
  rssi: number; // -100~0 钳制
}

/** 信号强度钳制 -100~0，非法回 -100。 */
export function clampRssi(v: number): number {
  if (!Number.isFinite(v)) return -100;
  return Math.min(0, Math.max(-100, Math.round(v)));
}

/** 互联质量分：RSSI → 0~100 确定性换算。 */
export function linkQuality(rssi: number): number {
  const r = clampRssi(rssi);
  return Math.round((r + 100) * 100 / 100);
}

export class LinkMesh {
  clamped = 0;
  peers: PeerDevice[] = [];

  add(id: string, rssi: number, trusted = false): boolean {
    if (this.peers.some((p) => p.id === id)) return false;
    this.peers.push({ id, trusted, rssi: clampRssi(rssi) });
    return true;
  }

  trust(id: string): boolean {
    const p = this.peers.find((x) => x.id === id);
    if (!p) return false;
    p.trusted = true;
    return true;
  }

  /** 可用通道：受信且信号 ≥ -70。 */
  usableIds(): string[] {
    return this.peers.filter((p) => p.trusted && p.rssi >= -70).map((p) => p.id);
  }

  /** 会话建立：仅受信设备可发起指定通道。 */
  canStart(id: string, _kind: LinkKind): boolean {
    return this.peers.some((p) => p.id === id && p.trusted);
  }
}
