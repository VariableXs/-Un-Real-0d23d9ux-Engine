// AURORA-10000: AI-61 批次（领域13 声音与通知 · 族0301~0305 · F07501~F07625），勿删。
// 系统声音设计 / 声音包 / 提示音分级 / 白噪音声景 / 声音可访问。

/* ===================== 族0301 系统声音设计 ===================== */

export const SYSTEM_SOUND_EVENTS = [
  'boot', 'shutdown', 'login', 'logout', 'error', 'warning', 'notify', 'message',
  'mail', 'trash-empty', 'copy-done', 'download-done', 'screenshot', 'record',
  'device-in', 'device-out', 'charge', 'battery-low', 'update-done', 'backup-done',
  'file-delete', 'win-minimize', 'win-restore', 'dialog', 'silence',
] as const;

export type SystemSoundEvent = (typeof SYSTEM_SOUND_EVENTS)[number];

export interface SoundBinding {
  event: SystemSoundEvent;
  packId: string;
  asset: string;
  enabled: boolean;
}

/** 系统声音库：25 个系统事件 × 声包绑定；静音原则 = 主静音开关压倒一切。 */
export class SystemSoundLibrary {
  private bindings = new Map<SystemSoundEvent, SoundBinding>();
  private masterMuted = false;
  private played: Array<{ event: SystemSoundEvent; at: number; audible: boolean }> = [];

  constructor(packId = 'aurora-default') {
    for (const ev of SYSTEM_SOUND_EVENTS) {
      this.bindings.set(ev, { event: ev, packId, asset: `${packId}/${ev}.opus`, enabled: true });
    }
  }

  setMasterMuted(muted: boolean): void {
    this.masterMuted = muted;
  }

  isMasterMuted(): boolean {
    return this.masterMuted;
  }

  bind(event: SystemSoundEvent, packId: string): void {
    const b = this.bindings.get(event);
    if (b) b.packId = packId;
  }

  bindingOf(event: SystemSoundEvent): SoundBinding | undefined {
    return this.bindings.get(event);
  }

  enable(event: SystemSoundEvent, enabled: boolean): void {
    const b = this.bindings.get(event);
    if (b) b.enabled = enabled;
  }

  /** 静默档（F07525）：静音时全静默，播放记录仍保留（供声音历史回放）。 */
  play(event: SystemSoundEvent, at: number): boolean {
    const b = this.bindings.get(event);
    const audible = !this.masterMuted && !!b && b.enabled && event !== 'silence';
    this.played.push({ event, at, audible });
    return audible;
  }

  playHistory(): ReadonlyArray<{ event: SystemSoundEvent; at: number; audible: boolean }> {
    return this.played;
  }
}

/* ===================== 族0302 声音包 ===================== */

export interface SoundPack {
  id: string;
  name: string;
  builtin: boolean;
  tags: string[];
}

export const BUILTIN_SOUND_PACKS: SoundPack[] = [
  { id: 'default', name: '默认声景', builtin: true, tags: ['factory'] },
  { id: 'nature', name: '自然', builtin: true, tags: ['forest', 'sea', 'rainforest'] },
  { id: 'city', name: '城市', builtin: true, tags: ['coffee', 'rainy-night', 'traffic'] },
  { id: 'retro', name: '复古', builtin: true, tags: ['8bit', 'dialup', 'mechanical'] },
  { id: 'instrument', name: '器乐', builtin: true, tags: ['piano', 'music-box', 'guzheng'] },
  { id: 'electronic', name: '电子', builtin: true, tags: ['synth', 'lofi', 'techno'] },
  { id: 'white-noise', name: '白噪音', builtin: true, tags: ['noise'] },
  { id: 'pink-brown', name: '粉棕噪', builtin: true, tags: ['pink', 'brown'] },
  { id: 'asmr', name: 'ASMR', builtin: true, tags: ['soft'] },
  { id: 'zen', name: '禅意', builtin: true, tags: ['meditation'] },
  { id: 'gaming', name: '游戏化', builtin: true, tags: ['pixel'] },
  { id: 'festival', name: '节日', builtin: true, tags: ['spring-festival', 'christmas'] },
  { id: 'low-stim', name: '低刺激', builtin: true, tags: ['near-silent'] },
  { id: 'mono-min', name: '单音极简', builtin: true, tags: ['minimal'] },
  { id: 'market-slot', name: '市场位', builtin: true, tags: ['reserved'] },
];

export interface PackScheduleRule {
  fromHour: number;
  toHour: number;
  packId: string;
}

/** 声音包管理：安装/混搭/独立音量/定时换包/主题与季节联动。 */
export class SoundPackManager {
  private installed = new Map<string, SoundPack>();
  private active = 'default';
  private volumes = new Map<string, number>();
  private schedule: PackScheduleRule[] = [];
  private links = { theme: false, season: false };

  constructor() {
    for (const p of BUILTIN_SOUND_PACKS) this.installed.set(p.id, p);
  }

  install(pack: SoundPack): boolean {
    if (this.installed.has(pack.id)) return false;
    this.installed.set(pack.id, pack);
    return true;
  }

  activate(packId: string): boolean {
    if (!this.installed.has(packId)) return false;
    this.active = packId;
    return true;
  }

  activePack(): string {
    return this.active;
  }

  mixInto(packId: string): boolean {
    // 跨包混搭：目标包已安装即可叠加为当前包。
    return this.activate(packId);
  }

  setPackVolume(packId: string, v: number): void {
    this.volumes.set(packId, Math.max(0, Math.min(1, v)));
  }

  packVolume(packId: string): number {
    return this.volumes.get(packId) ?? 1;
  }

  addSchedule(rule: PackScheduleRule): void {
    this.schedule.push(rule);
  }

  /** 定时换包：按当前小时取命中的规则（支持跨零点区间）。 */
  packForHour(hour: number): string {
    for (const r of this.schedule) {
      if (r.fromHour <= r.toHour ? r.fromHour <= hour && hour < r.toHour : hour >= r.fromHour || hour < r.toHour) {
        return r.packId;
      }
    }
    return this.active;
  }

  setLink(kind: 'theme' | 'season', on: boolean): void {
    this.links[kind] = on;
  }

  linkedTo(kind: 'theme' | 'season'): boolean {
    return this.links[kind];
  }

  packApiVersion(): number {
    return 1;
  }
}

/* ===================== 族0303 提示音分级 ===================== */

export type SoundLevel = 'silent' | 'low' | 'medium' | 'high';
export type SoundCategory = 'notify' | 'warning' | 'error' | 'alarm';

export interface LevelContext {
  night?: boolean;
  meeting?: boolean;
  gameFullscreen?: boolean;
  casting?: boolean;
  recording?: boolean;
  focus?: boolean;
  study?: boolean;
}

/** 提示音分级：四档 × 应用 × 类型 × 场景静默 × 例外与疲劳保护。 */
export class SoundLevelSystem {
  private perApp = new Map<string, SoundLevel>();
  private perCategory = new Map<SoundCategory, SoundLevel>();
  private dndRules: Array<{ keyword: string; level: SoundLevel }> = [];
  private vipContacts = new Set<string>();
  private lastPlayed = new Map<string, number>();
  private fatigue = { maxPerMinute: 6, windowStart: 0, count: 0 };
  private nightCap = 0.4;
  private profiles = new Map<string, Map<string, SoundLevel>>();

  constructor() {
    this.perCategory.set('notify', 'medium');
    this.perCategory.set('warning', 'medium');
    this.perCategory.set('error', 'high');
    this.perCategory.set('alarm', 'high');
  }

  setAppLevel(app: string, level: SoundLevel): void {
    this.perApp.set(app, level);
  }

  appLevel(app: string): SoundLevel {
    return this.perApp.get(app) ?? 'medium';
  }

  setCategoryLevel(cat: SoundCategory, level: SoundLevel): void {
    this.perCategory.set(cat, level);
  }

  categoryLevel(cat: SoundCategory): SoundLevel {
    return this.perCategory.get(cat) ?? 'medium';
  }

  addDndRule(keyword: string, level: SoundLevel): void {
    this.dndRules.push({ keyword, level });
  }

  markVip(contact: string): void {
    this.vipContacts.add(contact);
  }

  isVip(contact: string): boolean {
    return this.vipContacts.has(contact);
  }

  /** 重复静默：同一 key 5 分钟内只响一次。 */
  shouldPlayDedup(key: string, at: number): boolean {
    const last = this.lastPlayed.get(key);
    if (last !== undefined && at - last < 5 * 60_000) return false;
    this.lastPlayed.set(key, at);
    return true;
  }

  /** 疲劳保护：每分钟连响上限。 */
  underFatigueLimit(at: number): boolean {
    if (at - this.fatigue.windowStart >= 60_000) {
      this.fatigue.windowStart = at;
      this.fatigue.count = 0;
    }
    if (this.fatigue.count >= this.fatigue.maxPerMinute) return false;
    this.fatigue.count += 1;
    return true;
  }

  nightVolumeCap(): number {
    return this.nightCap;
  }

  /** 场景静默判定：会议/游戏/投屏/录制/专注全静；学习只留提醒；警报例外永远响。 */
  mutedByContext(cat: SoundCategory, ctx: LevelContext): boolean {
    if (cat === 'alarm') return false;
    if (ctx.meeting || ctx.gameFullscreen || ctx.casting || ctx.recording || ctx.focus) return true;
    if (ctx.study && cat === 'notify') return true;
    return false;
  }

  saveProfile(name: string): void {
    const snap = new Map<string, SoundLevel>();
    for (const [k, v] of this.perCategory) snap.set(k, v);
    for (const [k, v] of this.perApp) snap.set(`app:${k}`, v);
    this.profiles.set(name, snap);
  }

  hasProfile(name: string): boolean {
    return this.profiles.has(name);
  }

  exportProfile(): string {
    return JSON.stringify([...this.perApp.entries(), ...this.perCategory.entries()]);
  }

  importProfile(json: string): boolean {
    try {
      const raw = JSON.parse(json) as Array<[string, SoundLevel]>;
      for (const [k, v] of raw) {
        if (k.startsWith('app:')) this.perApp.set(k.slice(4), v);
        else this.perCategory.set(k as SoundCategory, v);
      }
      return true;
    } catch {
      return false;
    }
  }
}

/* ===================== 族0304 白噪音声景 ===================== */

export const AMBIENCE_SCENES = [
  'drizzle', 'rain', 'downpour', 'thunderstorm', 'stream', 'waterfall', 'waves', 'wind',
  'birds', 'crickets', 'campfire', 'fireplace', 'cafe', 'library', 'keyboard', 'pages',
  'fan', 'train', 'cabin', 'cave', 'snowfall', 'city-night',
] as const;

export type AmbienceScene = (typeof AMBIENCE_SCENES)[number];

/** 声景混合器：22 场景多源混合 + 助眠定时 + 渐弱。 */
export class AmbienceMixer {
  private gains = new Map<AmbienceScene, number>();
  private sleepTimerMs: number | null = null;
  private startAt = 0;
  private fade = false;
  private masterVolume = 1;

  constructor() {
    for (const s of AMBIENCE_SCENES) this.gains.set(s, 0);
  }

  setGain(scene: AmbienceScene, gain: number): void {
    this.gains.set(scene, Math.max(0, Math.min(1, gain)));
  }

  gainOf(scene: AmbienceScene): number {
    return this.gains.get(scene) ?? 0;
  }

  activeScenes(): AmbienceScene[] {
    return [...this.gains.entries()].filter(([, g]) => g > 0).map(([s]) => s);
  }

  startSleepTimer(ms: number, at: number, fade: boolean): void {
    this.sleepTimerMs = ms;
    this.startAt = at;
    this.fade = fade;
  }

  /** 定时渐弱：45 分钟助眠档随时间线性降至 0；无定时则保持主音量。 */
  currentVolume(at: number): number {
    if (this.sleepTimerMs === null) return this.masterVolume;
    const elapsed = at - this.startAt;
    if (elapsed >= this.sleepTimerMs) return 0;
    if (!this.fade) return this.masterVolume;
    const remain = 1 - elapsed / this.sleepTimerMs;
    return this.masterVolume * remain;
  }

  clearSleepTimer(): void {
    this.sleepTimerMs = null;
  }

  setMasterVolume(v: number): void {
    this.masterVolume = Math.max(0, Math.min(1, v));
  }
}

/* ===================== 族0305 声音可访问 ===================== */

export interface SoundA11ySettings {
  visualizer: boolean;
  mono: boolean;
  balance: number; // -1 左 … 1 右
  highFreqBoost: boolean;
  speechClarity: boolean;
  envCaptions: boolean;
  flashAlerts: boolean;
  flashIntensity: number;
  vibrationAlerts: boolean;
  loudnessNormalization: boolean;
  burstLimitDb: number;
  bedtimeFade: boolean;
  wakeupRamp: boolean;
  hearingAidCompat: boolean;
  cochlearMode: boolean;
  soundIdentities: boolean;
}

export const DEFAULT_SOUND_A11Y: SoundA11ySettings = {
  visualizer: false, mono: false, balance: 0, highFreqBoost: false, speechClarity: false,
  envCaptions: false, flashAlerts: false, flashIntensity: 0.6, vibrationAlerts: false,
  loudnessNormalization: true, burstLimitDb: -6, bedtimeFade: true, wakeupRamp: true,
  hearingAidCompat: false, cochlearMode: false, soundIdentities: true,
};

export interface SoundEventRecord {
  at: number;
  source: string;
  caption?: string;
  direction?: 'left' | 'right' | 'center';
}

/** 声音可访问：环境字幕 / 闪光与振动替代 / 声音历史与回放 / 炸耳保护。 */
export class SoundAccessibility {
  private settings: SoundA11ySettings;
  private history: SoundEventRecord[] = [];

  constructor(settings: Partial<SoundA11ySettings> = {}) {
    this.settings = { ...DEFAULT_SOUND_A11Y, ...settings };
  }

  settings_(): SoundA11ySettings {
    return { ...this.settings };
  }

  update(patch: Partial<SoundA11ySettings>): void {
    this.settings = { ...this.settings, ...patch };
  }

  /** 环境字幕：可听事件转成可见文案（如「门铃响」）。 */
  captionFor(source: string, at: number): string | null {
    if (!this.settings.envCaptions) return null;
    const known: Record<string, string> = {
      doorbell: '门铃响', phone: '电话响', water: '水开了', timer: '计时结束',
    };
    const text = known[source] ?? `${source}响了`;
    this.history.push({ at, source, caption: text });
    return text;
  }

  /** 炸耳保护：输出端限制突发增益。 */
  applyBurstLimit(gainDb: number): number {
    return Math.min(gainDb, this.settings.burstLimitDb);
  }

  /** 睡前渐降 / 渐强唤醒曲线。 */
  bedtimeCurve(progress01: number): number {
    return 1 - progress01;
  }

  wakeupCurve(progress01: number): number {
    return progress01;
  }

  record(ev: SoundEventRecord): void {
    this.history.push(ev);
  }

  historyAll(): ReadonlyArray<SoundEventRecord> {
    return this.history;
  }

  /** 「刚才是什么响」回放：返回最近一条记录。 */
  replayLast(): SoundEventRecord | undefined {
    return this.history[this.history.length - 1];
  }
}
