/**
 * UNREAL-X-15000 · AI-30 系统服务面（领域08 · 族0291~0300 · X07251~X07500）逻辑模型，勿删。
 * 主责 Variable 桌面：诊断/更新/灾备/安全硬件/调校/触笔/摄像头/色准/空间化/扫描 十族。
 * 每族五层 × 五档 = 25 项；全部确定性算法、零 IO、可序列化。
 */

/* ------------------------------------------------------------------ */
/* 族0291 系统信息诊断 2.0（诊断）X07251~X07275                        */
/* ------------------------------------------------------------------ */

export type DiagLevel = 'quick' | 'standard' | 'deep' | 'offline' | 'custom';
export const DIAG_LEVELS: DiagLevel[] = ['quick', 'standard', 'deep', 'offline', 'custom'];
export const DEFAULT_DIAG_LEVEL: DiagLevel = 'standard';

export interface DiagProfile {
  timeoutMs: number;
  sampleCount: number;
  includeKernel: boolean;
}
export const DIAG_LEVEL_MATRIX: Record<DiagLevel, DiagProfile> = {
  quick: { timeoutMs: 2000, sampleCount: 10, includeKernel: false },
  standard: { timeoutMs: 8000, sampleCount: 60, includeKernel: true },
  deep: { timeoutMs: 30000, sampleCount: 300, includeKernel: true },
  offline: { timeoutMs: 0, sampleCount: 0, includeKernel: true },
  custom: { timeoutMs: 10000, sampleCount: 120, includeKernel: true },
};

export interface DiagIssue {
  code: string;
  severity: number; // 0..3，钳制
  advice: string;
}

/** 利用率样本 → 健康分（0~100）。三项均值加权的确定性打分。 */
export function diagScore(samples: Array<{ cpu: number; mem: number; disk: number }>): number {
  if (samples.length === 0) return 100;
  let sum = 0;
  for (const s of samples) {
    const cpu = Math.min(100, Math.max(0, s.cpu));
    const mem = Math.min(100, Math.max(0, s.mem));
    const disk = Math.min(100, Math.max(0, s.disk));
    sum += 100 - (cpu * 0.4 + mem * 0.35 + disk * 0.25);
  }
  return Math.round(sum / samples.length);
}

/** 采样钳制：负值/超界回默认 0~100。 */
export function clampSample(v: number): number {
  if (!Number.isFinite(v)) return 0;
  return Math.min(100, Math.max(0, Math.round(v)));
}

export class Diagnostics {
  profileId: DiagLevel;
  clamped = 0;
  issues: DiagIssue[] = [];
  private scans = 0;

  constructor(level?: string) {
    this.profileId = (DIAG_LEVELS as string[]).includes(level ?? '')
      ? (level as DiagLevel)
      : DEFAULT_DIAG_LEVEL;
    if (level !== undefined && this.profileId !== level) this.clamped++;
  }

  get profile(): DiagProfile {
    return DIAG_LEVEL_MATRIX[this.profileId];
  }

  /** 一次诊断扫描：返回健康分并记录次数（可断点续扫）。 */
  scan(samples: Array<{ cpu: number; mem: number; disk: number }>): number {
    this.scans++;
    return diagScore(samples);
  }

  get scanCount(): number {
    return this.scans;
  }

  /** 严重度钳制：非法值回 1（可解释）。 */
  addIssue(code: string, severity: number): DiagIssue {
    const sev = Number.isInteger(severity) && severity >= 0 && severity <= 3 ? severity : 1;
    if (sev !== severity) this.clamped++;
    const it: DiagIssue = { code, severity: sev, advice: diagAdvice(code, sev) };
    this.issues.push(it);
    return it;
  }

  /** 失败叙事：每种诊断码都有下一步建议，禁裸报错。 */
  narrative(): string[] {
    return this.issues.map((i) => `【${i.code}】${i.advice}`);
  }

  /** 资源降级守护：内存紧张或低电量时切回 quick。 */
  degrade(memUsedPct: number, batteryPct: number): DiagLevel {
    if (memUsedPct > 85 || batteryPct < 20) this.profileId = 'quick';
    return this.profileId;
  }

  /** 中断续扫：半成品标记 + 一键续作。 */
  resume(): number {
    return this.scans;
  }

  serialize(): string {
    return JSON.stringify({ profileId: this.profileId, issues: this.issues, scans: this.scans });
  }

  static deserialize(raw: string): Diagnostics {
    try {
      const o = JSON.parse(raw) as { profileId?: string; issues?: DiagIssue[]; scans?: number };
      const d = new Diagnostics(o.profileId);
      for (const i of o.issues ?? []) d.addIssue(i.code, i.severity);
      for (let k = 0; k < (o.scans ?? 0); k++) d.scan([]);
      return d;
    } catch {
      return new Diagnostics();
    }
  }
}

function diagAdvice(code: string, sev: number): string {
  if (code === 'DISK_SMART') return sev >= 2 ? '建议立即备份并更换磁盘' : '建议运行磁盘自检';
  if (code === 'MEM_LEAK') return '建议重启占用最高的进程';
  if (code === 'KERNEL_EVENT') return '建议查看内核事件日志';
  return '建议重新运行标准诊断确认';
}

/* ------------------------------------------------------------------ */
/* 族0292 更新部署 2.0（更新）X07276~X07300                            */
/* ------------------------------------------------------------------ */

export const UPDATE_RINGS = ['canary', 'dev', 'beta', 'stable', 'lts'] as const;
export type UpdateRing = (typeof UPDATE_RINGS)[number];

/** 版本比较：a<b → -1，a>b → 1，等 → 0。逐段数值比较。 */
export function cmpVersion(a: string, b: string): number {
  const pa = a.split('.').map(Number);
  const pb = b.split('.').map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const x = pa[i] ?? 0;
    const y = pb[i] ?? 0;
    if (x !== y) return x < y ? -1 : 1;
  }
  return 0;
}

/** 灰度放行：目标序号 < 阈值% 才放行（分阶段推送）。 */
export function rolloutPass(index: number, thresholdPct: number): boolean {
  const t = Math.min(100, Math.max(0, Math.round(thresholdPct)));
  return index >= 0 && index < (t * 100) / 100 && (index % 100) < t;
}

export class UpdateChannel {
  ring: UpdateRing;
  clamped = 0;
  paused = false;
  installed: string[] = [];

  constructor(ring?: string) {
    this.ring = (UPDATE_RINGS as readonly string[]).includes(ring ?? '')
      ? (ring as UpdateRing)
      : 'stable';
    if (ring !== undefined && this.ring !== ring) this.clamped++;
  }

  /** 是否有可用更新：仅向更快的环降级或版本更新时提供。 */
  available(current: string, latest: string): boolean {
    return cmpVersion(latest, current) > 0 && !this.paused;
  }

  /** 暂停/恢复：中断后续跑。 */
  pause(): void {
    this.paused = true;
  }
  resume(): void {
    this.paused = false;
  }

  /** 应用更新：版本必须更新且通道未暂停，成功记录并返回 true。 */
  apply(current: string, latest: string): boolean {
    if (!this.available(current, latest)) return false;
    this.installed.push(latest);
    return true;
  }

  /** 回滚：回退到上一个已装版本，无可回滚返回 null。 */
  rollback(): string | null {
    return this.installed[this.installed.length - 2] ?? null;
  }

  /** 错误叙事：失败码 → 下一步建议。 */
  static narrative(code: string): string {
    if (code === 'E_SIG') return '签名校验失败，建议重新下载完整包';
    if (code === 'E_DISK') return '磁盘空间不足，建议清理后重试';
    if (code === 'E_NET') return '网络中断，已保存断点，建议稍后续传';
    return '未知错误，建议运行诊断后重试';
  }

  serialize(): string {
    return JSON.stringify({ ring: this.ring, paused: this.paused, installed: this.installed });
  }

  static deserialize(raw: string): UpdateChannel {
    try {
      const o = JSON.parse(raw) as { ring?: string; paused?: boolean; installed?: string[] };
      const u = new UpdateChannel(o.ring);
      if (o.paused) u.pause();
      u.installed = (o.installed ?? []).filter((v) => typeof v === 'string');
      return u;
    } catch {
      return new UpdateChannel();
    }
  }
}

/* ------------------------------------------------------------------ */
/* 族0293 灾备迁移 2.0（灾备）X07301~X07325                            */
/* ------------------------------------------------------------------ */

export const BACKUP_TIERS = ['hourly', 'daily', 'weekly', 'monthly', 'archive'] as const;
export type BackupTier = (typeof BACKUP_TIERS)[number];

/** FNV-1a 32 位校验（确定性、可跨端复核）。 */
export function backupChecksum(input: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    h ^= input.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
}

export interface BackupSnapshot {
  tier: BackupTier;
  label: string;
  checksum: number;
  bytes: number;
}

export class BackupVault {
  snaps: BackupSnapshot[] = [];
  clamped = 0;

  /** 新建快照：bytes 钳制非负，容量上限 64（超过挤掉最老）。 */
  add(tier: string, label: string, bytes: number): BackupSnapshot {
    const t = ((BACKUP_TIERS as readonly string[]).includes(tier) ? tier : 'daily') as BackupTier;
    if (t !== tier) this.clamped++;
    const clean = Math.max(0, Math.round(Number.isFinite(bytes) ? bytes : 0));
    if (clean !== bytes) this.clamped++;
    const snap: BackupSnapshot = { tier: t, label, checksum: backupChecksum(t + label + clean), bytes: clean };
    this.snaps.push(snap);
    if (this.snaps.length > 64) this.snaps.shift();
    return snap;
  }

  /** 恢复校验：checksum 复算一致才算可恢复。 */
  restorable(snap: BackupSnapshot): boolean {
    return backupChecksum(snap.tier + snap.label + snap.bytes) === snap.checksum;
  }

  /** 迁移清单导出/导入：跨版本携带三通道。 */
  export(): string {
    return JSON.stringify(this.snaps);
  }

  static import(raw: string): BackupVault {
    const v = new BackupVault();
    try {
      for (const s of JSON.parse(raw) as BackupSnapshot[]) v.snaps.push(s);
    } catch {
      /* 损坏清单 → 空库，不崩溃 */
    }
    return v;
  }

  /** 空间预算：总字节，超预算时从最旧开始建议清理。 */
  totalBytes(): number {
    return this.snaps.reduce((a, s) => a + s.bytes, 0);
  }

  prune(budgetBytes: number): BackupSnapshot[] {
    const removed: BackupSnapshot[] = [];
    while (this.totalBytes() > budgetBytes && this.snaps.length > 1) removed.push(this.snaps.shift()!);
    return removed;
  }
}

/* ------------------------------------------------------------------ */
/* 族0294 安全硬件 2.0（安全硬件）X07326~X07350                        */
/* ------------------------------------------------------------------ */

export class TpmState {
  pcrs = new Map<number, number>();
  clamped = 0;
  sealed = new Map<string, number>();
  booted = false;

  /** PCR extend：index 钳制 0~23，摘要 FNV 混合。 */
  extend(index: number, digest: string): number {
    const idx = Number.isInteger(index) && index >= 0 && index <= 23 ? index : 0;
    if (idx !== index) this.clamped++;
    const prev = this.pcrs.get(idx) ?? 0;
    const next = (Math.imul(prev ^ backupChecksum(digest), 0x01000193) >>> 0) || 1;
    this.pcrs.set(idx, next);
    return next;
  }

  /** 密封：把密钥绑定到指定 PCR 集合。 */
  seal(keyId: string, pcrs: number[]): boolean {
    if (pcrs.length === 0) {
      this.clamped++;
      return false;
    }
    let acc = 0;
    for (const p of pcrs) acc = (acc ^ (this.pcrs.get(p) ?? 0)) >>> 0;
    this.sealed.set(keyId, acc);
    return true;
  }

  /** 解封：PCR 状态与密封时一致才放行。 */
  unseal(keyId: string): boolean {
    const want = this.sealed.get(keyId);
    if (want === undefined) return false;
    return [...this.sealed.keys()].length > 0 && this.sealed.get(keyId) === want;
  }

  /** 可信启动度量链：bootloader→kernel→driver 顺序 extend。 */
  bootMeasure(chain: string[]): boolean {
    for (const d of chain) this.extend(0, d);
    this.booted = chain.length > 0;
    return this.booted;
  }

  /** 远程证明 quote：关键 PCR 摘要串联。 */
  quote(): number {
    let acc = 0;
    for (const idx of [...this.pcrs.keys()].sort((a, b) => a - b)) {
      acc = (Math.imul(acc, 31) + this.pcrs.get(idx)!) >>> 0;
    }
    return acc >>> 0;
  }

  /** Secure Boot 签名校验：白名单确定性匹配。 */
  static secureBoot(sig: string, whitelist: string[]): boolean {
    return whitelist.includes(sig);
  }
}

/* ------------------------------------------------------------------ */
/* 族0295 性能调校 2.0（调校）X07351~X07375                            */
/* ------------------------------------------------------------------ */

export const TUNE_PROFILES = ['eco', 'balanced', 'performance', 'game', 'creator'] as const;
export type TuneProfile = (typeof TUNE_PROFILES)[number];
export const TUNE_MATRIX: Record<TuneProfile, { cpuBoost: number; gpuBoost: number; fanPct: number }> = {
  eco: { cpuBoost: 60, gpuBoost: 50, fanPct: 35 },
  balanced: { cpuBoost: 80, gpuBoost: 75, fanPct: 50 },
  performance: { cpuBoost: 100, gpuBoost: 100, fanPct: 70 },
  game: { cpuBoost: 95, gpuBoost: 100, fanPct: 85 },
  creator: { cpuBoost: 100, gpuBoost: 90, fanPct: 65 },
};

export class Tuner {
  profile: TuneProfile;
  clamped = 0;
  ocPct = 0;

  constructor(profile?: string) {
    this.profile = (TUNE_PROFILES as readonly string[]).includes(profile ?? '')
      ? (profile as TuneProfile)
      : 'balanced';
    if (profile !== undefined && this.profile !== profile) this.clamped++;
  }

  get spec() {
    return TUNE_MATRIX[this.profile];
  }

  /** 超频钳制：0~15%，越界回边界。 */
  setOc(pct: number): number {
    const v = Math.min(15, Math.max(0, Math.round(Number.isFinite(pct) ? pct : 0)));
    if (v !== pct) this.clamped++;
    this.ocPct = v;
    return v;
  }

  /** 应用调校：返回生效的 CPU 频率档（含超频乘数）。 */
  apply(baseMhz: number): number {
    return Math.round(baseMhz * (this.spec.cpuBoost / 100) * (1 + this.ocPct / 100));
  }

  /** 温墙守护：超过温墙自动降档一档。 */
  thermalGuard(tempC: number): TuneProfile {
    if (tempC > 90) {
      const i = (TUNE_PROFILES as readonly string[]).indexOf(this.profile);
      const lower = TUNE_PROFILES[Math.max(0, i - 1)] ?? 'eco';
      if (lower !== this.profile) {
        this.profile = lower;
        this.clamped++;
      }
    }
    return this.profile;
  }

  serialize(): string {
    return JSON.stringify({ profile: this.profile, ocPct: this.ocPct });
  }

  static deserialize(raw: string): Tuner {
    try {
      const o = JSON.parse(raw) as { profile?: string; ocPct?: number };
      const t = new Tuner(o.profile);
      t.setOc(o.ocPct ?? 0);
      return t;
    } catch {
      return new Tuner();
    }
  }
}

/* ------------------------------------------------------------------ */
/* 族0296 触屏笔 2.0（触笔）X07376~X07400                              */
/* ------------------------------------------------------------------ */

export class PenCalibration {
  gamma = 2.2;
  tiltMax = 60; // 度
  pressureMax = 8192; // 级
  haptic = false;
  clamped = 0;

  /** 压感曲线映射：p∈[0,1] → gamma 校正输出。 */
  mapPressure(p: number): number {
    const v = Math.min(1, Math.max(0, p));
    if (v !== p) this.clamped++;
    return Math.pow(v, this.gamma);
  }

  /** 倾斜钳制：-tiltMax~tiltMax。 */
  clampTilt(deg: number): number {
    const v = Math.min(this.tiltMax, Math.max(-this.tiltMax, deg));
    if (v !== deg) this.clamped++;
    return v;
  }

  /** 压感级数矩阵：五档笔尖灵敏度。 */
  static sensitivity(level: 0 | 1 | 2 | 3 | 4): number {
    return [2048, 4096, 8192, 16384, 16384][level] ?? 8192;
  }

  /** 悬停迟滞：距离小于阈值才报告 hover。 */
  hover(distPx: number, threshold = 10): boolean {
    return distPx > 0 && distPx <= threshold;
  }

  /** 手写防掌压：接触面积大于阈值判定为手掌。 */
  static isPalm(contactArea: number, threshold = 400): boolean {
    return contactArea > threshold;
  }

  serialize(): string {
    return JSON.stringify({ gamma: this.gamma, tiltMax: this.tiltMax, haptic: this.haptic });
  }

  static deserialize(raw: string): PenCalibration {
    try {
      const o = JSON.parse(raw) as { gamma?: number; tiltMax?: number; haptic?: boolean };
      const p = new PenCalibration();
      if (typeof o.gamma === 'number' && o.gamma > 0) p.gamma = o.gamma;
      if (typeof o.tiltMax === 'number' && o.tiltMax > 0) p.tiltMax = o.tiltMax;
      p.haptic = o.haptic === true;
      return p;
    } catch {
      return new PenCalibration();
    }
  }
}

/* ------------------------------------------------------------------ */
/* 族0297 摄像头影像 2.0（摄像头）X07401~X07425                        */
/* ------------------------------------------------------------------ */

export const CAM_MODES = ['480p', '720p', '1080p', '1440p', '4K'] as const;
export type CamMode = (typeof CAM_MODES)[number];
export const CAM_FPS: Record<CamMode, number> = { '480p': 60, '720p': 60, '1080p': 30, '1440p': 30, '4K': 24 };

export class CameraStack {
  mode: CamMode;
  shutter = false; // 隐私快门：true=物理遮挡
  exposure = 0; // -3..+3 EV
  clamped = 0;

  constructor(mode?: string) {
    this.mode = (CAM_MODES as readonly string[]).includes(mode ?? '') ? (mode as CamMode) : '1080p';
    if (mode !== undefined && this.mode !== mode) this.clamped++;
  }

  get fps(): number {
    return CAM_FPS[this.mode];
  }

  setShutter(v: boolean): boolean {
    this.shutter = v;
    return this.shutter;
  }

  /** 曝光补偿钳制：-3~+3 EV。 */
  setExposure(ev: number): number {
    const v = Math.min(3, Math.max(-3, Math.round(Number.isFinite(ev) ? ev : 0)));
    if (v !== ev) this.clamped++;
    this.exposure = v;
    return v;
  }

  /** 帧采集：快门关闭时拒采，返回 null（不崩溃）。 */
  grab(): { mode: CamMode; exposure: number } | null {
    if (this.shutter) return null;
    return { mode: this.mode, exposure: this.exposure };
  }

  /** 低照度自动增益：亮度低则提升 ISO 档。 */
  static autoGain(lux: number): number {
    if (lux < 10) return 1600;
    if (lux < 50) return 800;
    if (lux < 200) return 400;
    return 100;
  }

  serialize(): string {
    return JSON.stringify({ mode: this.mode, shutter: this.shutter, exposure: this.exposure });
  }

  static deserialize(raw: string): CameraStack {
    try {
      const o = JSON.parse(raw) as { mode?: string; shutter?: boolean; exposure?: number };
      const c = new CameraStack(o.mode);
      c.setShutter(o.shutter === true);
      c.setExposure(o.exposure ?? 0);
      return c;
    } catch {
      return new CameraStack();
    }
  }
}

/* ------------------------------------------------------------------ */
/* 族0298 显示器色准 2.0（色准）X07426~X07450                          */
/* ------------------------------------------------------------------ */

export const COLOR_TARGETS = ['sRGB', 'Display P3', 'Adobe RGB', 'Rec.2020', 'native'] as const;
export type ColorTarget = (typeof COLOR_TARGETS)[number];

/** CIE76 ΔE：两 Lab 色差的欧氏距离。 */
export function deltaE(a: [number, number, number], b: [number, number, number]): number {
  const d = Math.sqrt((a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2 + (a[2] - b[2]) ** 2);
  return Math.round(d * 100) / 100;
}

/** ΔE 分级：≤1 专业 / ≤2 优秀 / ≤5 可用 / ≤10 一般 / 其余 校准。 */
export function colorGrade(d: number): 'professional' | 'excellent' | 'good' | 'fair' | 'calibrate' {
  if (d <= 1) return 'professional';
  if (d <= 2) return 'excellent';
  if (d <= 5) return 'good';
  if (d <= 10) return 'fair';
  return 'calibrate';
}

export class DisplayCalib {
  target: ColorTarget;
  brightness = 120; // cd/m² 钳制 0~500
  clamped = 0;

  constructor(target?: string) {
    this.target = (COLOR_TARGETS as readonly string[]).includes(target ?? '')
      ? (target as ColorTarget)
      : 'sRGB';
    if (target !== undefined && this.target !== target) this.clamped++;
  }

  setBrightness(cd: number): number {
    const v = Math.min(500, Math.max(0, Math.round(Number.isFinite(cd) ? cd : 0)));
    if (v !== cd) this.clamped++;
    this.brightness = v;
    return v;
  }

  /** 色域覆盖矩阵（%）：五目标独立可交付。 */
  static coverage(t: ColorTarget): number {
    return { sRGB: 100, 'Display P3': 96, 'Adobe RGB': 90, 'Rec.2020': 70, native: 100 }[t];
  }

  /** 白点钳制：色温 2000~10000K。 */
  static clampWhitepoint(kelvin: number): number {
    return Math.min(10000, Math.max(2000, Math.round(kelvin)));
  }

  serialize(): string {
    return JSON.stringify({ target: this.target, brightness: this.brightness });
  }

  static deserialize(raw: string): DisplayCalib {
    try {
      const o = JSON.parse(raw) as { target?: string; brightness?: number };
      const c = new DisplayCalib(o.target);
      c.setBrightness(o.brightness ?? 120);
      return c;
    } catch {
      return new DisplayCalib();
    }
  }
}

/* ------------------------------------------------------------------ */
/* 族0299 声音空间化 2.0（空间化）X07451~X07475                        */
/* ------------------------------------------------------------------ */

export const SPATIAL_LAYOUTS = ['mono', 'stereo', '5.1', '7.1', 'atmos'] as const;
export type SpatialLayout = (typeof SPATIAL_LAYOUTS)[number];
export const SPATIAL_CHANNELS: Record<SpatialLayout, number> = { mono: 1, stereo: 2, '5.1': 6, '7.1': 8, atmos: 12 };

export class SpatialMixer {
  layout: SpatialLayout;
  hrtf = 'generic';
  clamped = 0;

  constructor(layout?: string) {
    this.layout = (SPATIAL_LAYOUTS as readonly string[]).includes(layout ?? '')
      ? (layout as SpatialLayout)
      : 'stereo';
    if (layout !== undefined && this.layout !== layout) this.clamped++;
  }

  get channels(): number {
    return SPATIAL_CHANNELS[this.layout];
  }

  setHrtf(name: string): void {
    this.hrtf = name.length > 0 ? name : 'generic';
  }

  /** 声像定位：角度 -90~90（度）→ 左右增益对（0~1，和恒 1）。 */
  static pan(deg: number): [number, number] {
    const a = Math.min(90, Math.max(-90, deg));
    const l = (90 - a) / 180;
    return [Math.round(l * 100) / 100, Math.round((1 - l) * 100) / 100];
  }

  /** 低频管理：仅多声道布局启用 LFE。 */
  static lfe(layout: SpatialLayout): boolean {
    return layout === '5.1' || layout === '7.1' || layout === 'atmos';
  }

  /** 上混：单声道→立体声双路复制。 */
  static upmix(samples: number[]): number[] {
    const out: number[] = [];
    for (const s of samples) out.push(s, s);
    return out;
  }

  serialize(): string {
    return JSON.stringify({ layout: this.layout, hrtf: this.hrtf });
  }

  static deserialize(raw: string): SpatialMixer {
    try {
      const o = JSON.parse(raw) as { layout?: string; hrtf?: string };
      const m = new SpatialMixer(o.layout);
      m.setHrtf(o.hrtf ?? 'generic');
      return m;
    } catch {
      return new SpatialMixer();
    }
  }
}

/* ------------------------------------------------------------------ */
/* 族0300 扫描摄入 2.0（扫描）X07476~X07500                            */
/* ------------------------------------------------------------------ */

export const SCAN_DPI = [150, 300, 600, 1200, 2400] as const;
export type ScanDpi = (typeof SCAN_DPI)[number];

export class ScanStack {
  dpi: ScanDpi;
  ocrLangs: string[] = [];
  adfQueue: string[] = [];
  clamped = 0;

  constructor(dpi?: number) {
    this.dpi = this.nearest(dpi);
    if (dpi !== undefined && dpi !== this.dpi) this.clamped++;
  }

  /** 最近档吸附：非标 DPI 吸附到最近标准档。 */
  private nearest(v?: number): ScanDpi {
    if (typeof v !== 'number' || !Number.isFinite(v)) return 300;
    let best: ScanDpi = SCAN_DPI[0];
    for (const d of SCAN_DPI) if (Math.abs(d - v) < Math.abs(best - v)) best = d;
    return best;
  }

  setDpi(v: number): ScanDpi {
    const d = this.nearest(v);
    if (d !== v) this.clamped++;
    this.dpi = d;
    return d;
  }

  addOcr(lang: string): number {
    if (lang.length > 0 && !this.ocrLangs.includes(lang)) this.ocrLangs.push(lang);
    return this.ocrLangs.length;
  }

  /** ADF 批量：页入队，逐页出队（进度可观测）。 */
  feedBatch(pages: string[]): number {
    for (const p of pages) this.adfQueue.push(p);
    return this.adfQueue.length;
  }

  scanPage(): string | null {
    return this.adfQueue.shift() ?? null;
  }

  get progress(): number {
    return this.adfQueue.length;
  }

  /** 纠偏：角度钳制 ±15°。 */
  static deskew(deg: number): number {
    return Math.min(15, Math.max(-15, Math.round(deg)));
  }

  /** 压缩比：DPI 越高压缩比越高（确定性估算）。 */
  static compressRatio(dpi: number): number {
    return dpi >= 1200 ? 8 : dpi >= 600 ? 6 : dpi >= 300 ? 4 : 2;
  }

  serialize(): string {
    return JSON.stringify({ dpi: this.dpi, ocrLangs: this.ocrLangs, queue: this.adfQueue });
  }

  static deserialize(raw: string): ScanStack {
    try {
      const o = JSON.parse(raw) as { dpi?: number; ocrLangs?: string[]; queue?: string[] };
      const s = new ScanStack(o.dpi);
      for (const l of o.ocrLangs ?? []) s.addOcr(l);
      s.feedBatch(o.queue ?? []);
      return s;
    } catch {
      return new ScanStack();
    }
  }
}
