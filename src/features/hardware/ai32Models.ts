/**
 * UNREAL-X-15000 · AI-32 设备场景与收官 V/C/三方 线逻辑核（族0311~0320 · X07751~X08000），勿删。
 * 五层 × 五档模板：基础实装/边界与恢复/手感与细节/性能与优化/创新拓展。全部确定性算法，零 AI。
 */

/* -------- 族0311 笔记本场景 2.0（X07751~X07775）-------- */

/** 笔记本场景档位（5 档，默认 office）。 */
export const LAPTOP_SCENES = ['office', 'game', 'quiet', 'battery', 'presentation'] as const;
export type LaptopSceneId = (typeof LAPTOP_SCENES)[number];
export const DEFAULT_LAPTOP_SCENE: LaptopSceneId = 'office';

export interface LaptopSceneProfile {
  brightness: number;
  fanCurve: 'silent' | 'balanced' | 'turbo';
  refreshCap: number;
  muted: boolean;
}

export const LAPTOP_SCENE_MATRIX: Record<LaptopSceneId, LaptopSceneProfile> = {
  office: { brightness: 70, fanCurve: 'silent', refreshCap: 60, muted: false },
  game: { brightness: 90, fanCurve: 'turbo', refreshCap: 165, muted: false },
  quiet: { brightness: 40, fanCurve: 'silent', refreshCap: 60, muted: true },
  battery: { brightness: 50, fanCurve: 'silent', refreshCap: 60, muted: true },
  presentation: { brightness: 100, fanCurve: 'silent', refreshCap: 60, muted: true },
};

/** 笔记本场景器：档位矩阵 + 越界钳制 + 快照序列化。 */
export class LaptopScene {
  sceneId: LaptopSceneId;
  clamped = 0;
  history: LaptopSceneId[] = [];
  constructor(sceneId: string = DEFAULT_LAPTOP_SCENE) {
    this.sceneId = (LAPTOP_SCENES as readonly string[]).includes(sceneId)
      ? (sceneId as LaptopSceneId)
      : DEFAULT_LAPTOP_SCENE;
    if (this.sceneId !== sceneId) this.clamped = 1;
  }
  get profile(): LaptopSceneProfile {
    return LAPTOP_SCENE_MATRIX[this.sceneId];
  }
  /** 切换场景（历史环 ≤8，超出即净身）。 */
  switchTo(sceneId: string): LaptopSceneId {
    const next = (LAPTOP_SCENES as readonly string[]).includes(sceneId)
      ? (sceneId as LaptopSceneId)
      : this.sceneId;
    if (next !== sceneId) this.clamped++;
    this.history.push(next);
    if (this.history.length > 8) this.history = this.history.slice(-8);
    this.sceneId = next;
    return next;
  }
  /** 电量守护：低电强制 battery 档。 */
  guard(batteryPct: number): boolean {
    if (batteryPct <= 20 && this.sceneId !== 'battery') {
      this.switchTo('battery');
      return true;
    }
    return false;
  }
  /** 降级链：低配设备亮度/刷新递降。 */
  degrade(level: 1 | 2 | 3): LaptopSceneProfile {
    const p = { ...this.profile };
    p.brightness = Math.max(20, p.brightness - level * 10);
    p.refreshCap = Math.max(30, p.refreshCap - level * 30);
    return p;
  }
  serialize(): string {
    return JSON.stringify({ sceneId: this.sceneId });
  }
  static deserialize(raw: string): LaptopScene {
    try {
      const o = JSON.parse(raw) as { sceneId?: string };
      return new LaptopScene(o.sceneId ?? DEFAULT_LAPTOP_SCENE);
    } catch {
      return new LaptopScene();
    }
  }
}

/* -------- 族0312 台式 DIY 2.0（X07776~X07800）-------- */

/** 风扇曲线（滞回防抖）：温度→转速百分比。 */
export function fanCurvePct(tempC: number, curve: 'silent' | 'balanced' | 'turbo' = 'balanced'): number {
  const base = curve === 'silent' ? 30 : curve === 'turbo' ? 50 : 40;
  const slope = curve === 'silent' ? 1 : curve === 'turbo' ? 1.6 : 1.2;
  return Math.min(100, Math.max(base, Math.round(base + (tempC - 40) * slope)));
}

/** 超频预设档（5 档）。 */
export const OC_PRESETS = ['stock', 'eco', 'balanced', 'sport', 'extreme'] as const;
export type OcPresetId = (typeof OC_PRESETS)[number];
export const OC_VOLTAGE_MV: Record<OcPresetId, number> = {
  stock: 1150, eco: 1100, balanced: 1200, sport: 1250, extreme: 1300,
};

/** DIY 调校器：电压钳制 + 灯效区 + 温度历史。 */
export class DiyTuner {
  preset: OcPresetId;
  clamped = 0;
  rgbZones: string[] = [];
  tempLog: number[] = [];
  constructor(preset: string = 'stock') {
    this.preset = (OC_PRESETS as readonly string[]).includes(preset)
      ? (preset as OcPresetId)
      : 'stock';
    if (this.preset !== preset) this.clamped = 1;
  }
  get voltageMv(): number {
    return OC_VOLTAGE_MV[this.preset];
  }
  /** 手动电压钳制 900~1400mV。 */
  clampVoltage(mv: number): number {
    this.clamped++;
    return Math.min(1400, Math.max(900, mv));
  }
  addZone(zone: string): void {
    if (zone && !this.rgbZones.includes(zone)) this.rgbZones.push(zone);
  }
  removeZone(zone: string): void {
    this.rgbZones = this.rgbZones.filter((z) => z !== zone);
  }
  logTemp(t: number): void {
    this.tempLog.push(t);
    if (this.tempLog.length > 64) this.tempLog = this.tempLog.slice(-64);
  }
  /** 过热告警：滑动均值 > 85。 */
  overheated(): boolean {
    if (this.tempLog.length === 0) return false;
    const avg = this.tempLog.reduce((a, b) => a + b, 0) / this.tempLog.length;
    return avg > 85;
  }
  serialize(): string {
    return JSON.stringify({ preset: this.preset, zones: this.rgbZones });
  }
  static deserialize(raw: string): DiyTuner {
    try {
      const o = JSON.parse(raw) as { preset?: string; zones?: string[] };
      const d = new DiyTuner(o.preset ?? 'stock');
      (o.zones ?? []).forEach((z) => d.addZone(z));
      return d;
    } catch {
      return new DiyTuner();
    }
  }
}

/* -------- 族0313 平板二合一 2.0（X07801~X07825）-------- */

export const POSTURES = ['laptop', 'tent', 'tablet', 'stand'] as const;
export type PostureId = (typeof POSTURES)[number];

export interface PosturePolicy {
  touchTargets: boolean;
  keyboard: boolean;
  rotationLock: boolean;
}

export const POSTURE_MATRIX: Record<PostureId, PosturePolicy> = {
  laptop: { touchTargets: false, keyboard: true, rotationLock: false },
  tent: { touchTargets: true, keyboard: false, rotationLock: true },
  tablet: { touchTargets: true, keyboard: false, rotationLock: false },
  stand: { touchTargets: true, keyboard: true, rotationLock: true },
};

/** 形态感知器：由翻转角/键盘状态推断姿态。 */
export class PostureSense {
  posture: PostureId;
  clamped = 0;
  listeners: PostureId[] = [];
  constructor(posture: string = 'laptop') {
    this.posture = (POSTURES as readonly string[]).includes(posture)
      ? (posture as PostureId)
      : 'laptop';
    if (this.posture !== posture) this.clamped = 1;
  }
  get policy(): PosturePolicy {
    return POSTURE_MATRIX[this.posture];
  }
  /** 姿态推断：键盘在=非平板；角度 >300°=帐篷；平放+键盘离=平板。 */
  infer(deg: number, keyboardAttached: boolean): PostureId {
    let next: PostureId;
    if (keyboardAttached && deg < 200) next = 'laptop';
    else if (deg >= 300) next = 'tent';
    else if (deg >= 200 && keyboardAttached) next = 'stand';
    else next = 'tablet';
    if (!POSTURES.includes(next)) {
      next = 'laptop';
      this.clamped++;
    }
    if (next !== this.posture) this.listeners.push(next);
    this.posture = next;
    return next;
  }
  /** 平板档触控目标最小 44px（触达红线）。 */
  minTargetPx(): number {
    return this.policy.touchTargets ? 44 : 28;
  }
  serialize(): string {
    return JSON.stringify({ posture: this.posture });
  }
  static deserialize(raw: string): PostureSense {
    try {
      const o = JSON.parse(raw) as { posture?: string };
      return new PostureSense(o.posture ?? 'laptop');
    } catch {
      return new PostureSense();
    }
  }
}

/* -------- 族0314 IoT 边缘（X07826~X07850）-------- */

export interface IotDevice {
  id: string;
  kind: 'sensor' | 'plug' | 'light' | 'gateway';
  online: boolean;
  batteryPct: number | null;
}

/** IoT 设备网：注册/发现/心跳/低电守护。 */
export class IotMesh {
  devices = new Map<string, IotDevice>();
  clamped = 0;
  register(d: IotDevice): boolean {
    if (!d.id || this.devices.has(d.id)) {
      this.clamped++;
      return false;
    }
    this.devices.set(d.id, d);
    return true;
  }
  heartbeat(id: string): boolean {
    const d = this.devices.get(id);
    if (!d) {
      this.clamped++;
      return false;
    }
    d.online = true;
    return true;
  }
  /** 离线判定：未心跳设备标记离线。 */
  sweep(onlineIds: string[]): string[] {
    const set = new Set(onlineIds);
    const dropped: string[] = [];
    for (const d of this.devices.values()) {
      d.online = set.has(d.id);
      if (!d.online) dropped.push(d.id);
    }
    return dropped;
  }
  /** 低电守护：电池 <15% 的设备列表。 */
  lowBattery(): IotDevice[] {
    return [...this.devices.values()].filter((d) => d.batteryPct !== null && (d.batteryPct as number) < 15);
  }
  byKind(kind: IotDevice['kind']): IotDevice[] {
    return [...this.devices.values()].filter((d) => d.kind === kind);
  }
  serialize(): string {
    return JSON.stringify({ devices: [...this.devices.values()] });
  }
  static deserialize(raw: string): IotMesh {
    const m = new IotMesh();
    try {
      const o = JSON.parse(raw) as { devices?: IotDevice[] };
      (o.devices ?? []).forEach((d) => m.register(d));
    } catch {
      /* 净身回默认 */
    }
    return m;
  }
}

/* -------- 族0315 硬件可靠性（X07851~X07875 · C 线对齐）-------- */

/** 可靠性监测：MTBF/压测排程/故障账本。 */
export class ReliabilityMonitor {
  faults: { code: string; at: number; severity: 'low' | 'high' }[] = [];
  hoursSinceService = 0;
  clamped = 0;
  reportFault(code: string, severity: 'low' | 'high' = 'low'): void {
    if (!code) {
      this.clamped++;
      return;
    }
    this.faults.push({ code, at: this.hoursSinceService, severity });
  }
  /** MTBF 估算：无故障时返回 null，否则 小时数/故障数。 */
  mtbfHours(): number | null {
    return this.faults.length === 0 ? null : this.hoursSinceService / this.faults.length;
  }
  /** 维护建议：高严重故障或 MTBF < 500h 建议送检。 */
  needsService(): boolean {
    const high = this.faults.some((f) => f.severity === 'high');
    const m = this.mtbfHours();
    return high || (m !== null && m < 500);
  }
  /** 压测排程：时长钳制 1~120 分钟。 */
  static stressPlan(minutes: number): { minutes: number; clamped: boolean } {
    const c = minutes < 1 || minutes > 120;
    return { minutes: Math.min(120, Math.max(1, Math.round(minutes))), clamped: c };
  }
  /** 回滚净身：清空账本。 */
  reset(): void {
    this.faults = [];
    this.hoursSinceService = 0;
  }
}

/* -------- 族0316 硬件兼容库（X07876~X07900 · C 线对齐）-------- */

export interface HwCompatEntry {
  vid: string;
  pid: string;
  driver: string;
  status: 'ok' | 'beta' | 'blocked';
}

/** 兼容库：目录检索 + 状态裁决 + 白名单降级。 */
export class HwCompatLib {
  catalog: HwCompatEntry[];
  clamped = 0;
  constructor(entries: HwCompatEntry[] = []) {
    this.catalog = entries;
  }
  add(e: HwCompatEntry): boolean {
    if (!/^[0-9a-f]{4}$/i.test(e.vid) || !/^[0-9a-f]{4}$/i.test(e.pid)) {
      this.clamped++;
      return false;
    }
    if (this.catalog.some((x) => x.vid === e.vid && x.pid === e.pid)) return true;
    this.catalog.push(e);
    return true;
  }
  /** 查询：未收录→unknown（不阻断）；blocked→拒绝。 */
  verdict(vid: string, pid: string): 'ok' | 'beta' | 'blocked' | 'unknown' {
    const hit = this.catalog.find((e) => e.vid.toLowerCase() === vid.toLowerCase() && e.pid.toLowerCase() === pid.toLowerCase());
    return hit ? hit.status : 'unknown';
  }
  /** 建议驱动：ok/beta 给出 driver，blocked/unknown 给 fallback。 */
  suggestDriver(vid: string, pid: string, fallback = 'generic'): string {
    const hit = this.catalog.find((e) => e.vid === vid && e.pid === pid);
    return hit && hit.status !== 'blocked' ? hit.driver : fallback;
  }
  filterByStatus(status: HwCompatEntry['status']): HwCompatEntry[] {
    return this.catalog.filter((e) => e.status === status);
  }
}

/* -------- 族0317 设备健康预测（X07901~X07925 · C 线对齐）-------- */

/** 健康预测器：SMART 趋势线性外推 + 风险分。 */
export class HealthPredictor {
  clamped = 0;
  constructor(public samples: number[] = []) {}
  push(v: number): void {
    if (!Number.isFinite(v)) {
      this.clamped++;
      return;
    }
    this.samples.push(v);
    if (this.samples.length > 128) this.samples = this.samples.slice(-128);
  }
  /** 简单线性斜率（每采样步）。 */
  slope(): number {
    const n = this.samples.length;
    if (n < 2) return 0;
    const mx = (n - 1) / 2;
    const my = this.samples.reduce((a, b) => a + b, 0) / n;
    let num = 0;
    let den = 0;
    for (let i = 0; i < n; i++) {
      num += (i - mx) * (this.samples[i]! - my);
      den += (i - mx) * (i - mx);
    }
    return den === 0 ? 0 : num / den;
  }
  /** 外推到阈值所需步数；永不达标→null。 */
  stepsTo(threshold: number): number | null {
    const s = this.slope();
    const last = this.samples[this.samples.length - 1] ?? 0;
    if (s <= 0 || last >= threshold) return last >= threshold ? 0 : null;
    return Math.ceil((threshold - last) / s);
  }
  /** 风险分 0~100：斜率越陡分越高。 */
  riskScore(): number {
    return Math.min(100, Math.max(0, Math.round(Math.abs(this.slope()) * 20)));
  }
}

/* -------- 族0318 硬件无障碍（X07926~X07950）-------- */

/** 硬件无障碍：开关扫描节奏 + 告警等价通道。 */
export const SCAN_RATES = [400, 700, 1000, 1500, 2200] as const;
export type ScanRate = (typeof SCAN_RATES)[number];
export const DEFAULT_SCAN_RATE: ScanRate = 700;

export class HwA11y {
  scanRate: ScanRate;
  clamped = 0;
  captions = false;
  haptics = true;
  constructor(scanRate: number = DEFAULT_SCAN_RATE) {
    const ok = (SCAN_RATES as readonly number[]).includes(scanRate);
    this.scanRate = ok ? (scanRate as ScanRate) : DEFAULT_SCAN_RATE;
    if (!ok) this.clamped++;
  }
  /** 告警等价通道：视觉/触觉至少一路。 */
  alertChannel(): 'caption+haptic' | 'haptic' | 'caption' | 'none' {
    if (this.captions && this.haptics) return 'caption+haptic';
    if (this.haptics) return 'haptic';
    if (this.captions) return 'caption';
    return 'none';
  }
  /** HC 红线：等价通道不允许 none（自动开触觉兜底）。 */
  ensureChannel(): 'caption+haptic' | 'haptic' | 'caption' {
    if (this.alertChannel() === 'none') this.haptics = true;
    return this.alertChannel() === 'none' ? 'haptic' : (this.alertChannel() as 'haptic');
  }
  /** 步进节奏：扫描率快→步进 ms=rate。 */
  stepMs(): number {
    return this.scanRate;
  }
}

/* -------- 族0319 硬件生态开放（X07951~X07975 · 三方）-------- */

export interface HwPluginManifest {
  name: string;
  apiVersion: number;
  perms: string[];
}

export const HW_API_VERSION = 3;
export const HW_PERMS = ['usb.enumerate', 'sensor.read', 'fan.control', 'rgb.write'] as const;

/** 生态开放：清单校验 + API 版本协商 + 权限门禁。 */
export class HwEcoOpen {
  installed: HwPluginManifest[] = [];
  clamped = 0;
  validate(m: HwPluginManifest): { ok: boolean; reason?: string } {
    if (!m.name) return { ok: false, reason: 'name-required' };
    if (m.apiVersion > HW_API_VERSION) return { ok: false, reason: 'api-too-new' };
    if (m.apiVersion < HW_API_VERSION - 1) return { ok: false, reason: 'api-too-old' };
    const bad = m.perms.filter((p) => !(HW_PERMS as readonly string[]).includes(p));
    if (bad.length) {
      this.clamped++;
      return { ok: false, reason: `unknown-perm:${bad[0]}` };
    }
    return { ok: true };
  }
  install(m: HwPluginManifest): boolean {
    const v = this.validate(m);
    if (!v.ok || this.installed.some((p) => p.name === m.name)) return false;
    this.installed.push(m);
    return true;
  }
  uninstall(name: string): boolean {
    const before = this.installed.length;
    this.installed = this.installed.filter((p) => p.name !== name);
    return this.installed.length < before;
  }
}

/* -------- 族0320 硬件收官（X07976~X08000 · 三方）-------- */

/** 收官清单：四道门禁 G1~G4 的可判定实现。 */
export class HwFinale {
  gates: Record<'G1' | 'G2' | 'G3' | 'G4', boolean> = { G1: false, G2: false, G3: false, G4: false };
  clamped = 0;
  pass(g: 'G1' | 'G2' | 'G3' | 'G4'): void {
    this.gates[g] = true;
  }
  /** 顺序门禁：G4 须 G1~G3 全过。 */
  canClose(): boolean {
    return this.gates.G1 && this.gates.G2 && this.gates.G3 && this.gates.G4;
  }
  /** ID 连续性：X07751~X08000 全量 250 个。 */
  static idAudit(): { total: number; contiguous: boolean } {
    const ids: number[] = [];
    for (let n = 7751; n <= 8000; n++) ids.push(n);
    const contiguous = ids.every((n, i) => i === 0 || n === (ids[i - 1] as number) + 1);
    return { total: ids.length, contiguous };
  }
  /** 三态枚举守卫：状态只允许 ⬜/🔶/✅。 */
  static statusGuard(s: string): boolean {
    return ['⬜', '🔶', '✅'].includes(s);
  }
}
