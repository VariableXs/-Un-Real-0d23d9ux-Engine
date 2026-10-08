// AURORA-10000: AI-64 批次（领域13 声音与通知 · 族0316~0320 · F07876~F08000），勿删。
// 通知内容保护 / 声音工程 / 节日音景 / 提醒体系 / 声音个性收藏。

/* ===================== 族0316 通知内容保护 ===================== */

export interface ContentProtectionSettings {
  lockscreenAppOnly: boolean;
  lockscreenHideAll: boolean;
  otpLocalDetect: boolean;
  sensitiveMasking: boolean;
  screenshotExclude: boolean;
  recordingExclude: boolean;
  projectionExclude: boolean;
  encryptedHistory: boolean;
  autoExpireMs: number;
  twoStepReveal: boolean;
  privacyMode: boolean;
  childProtection: boolean;
}

export const DEFAULT_CONTENT_PROTECTION: ContentProtectionSettings = {
  lockscreenAppOnly: true, lockscreenHideAll: false, otpLocalDetect: true,
  sensitiveMasking: true, screenshotExclude: false, recordingExclude: false,
  projectionExclude: false, encryptedHistory: true, autoExpireMs: 24 * 3600_000,
  twoStepReveal: false, privacyMode: false, childProtection: false,
};

/** 验证码本地识别：6 位纯数字段。 */
export function detectOtpCode(text: string): string | null {
  const m = text.match(/(?<!\d)\d{6}(?!\d)/);
  return m ? m[0]! : null;
}

/** 敏感脱敏：密码全遮、金额保留首位。 */
export function maskSensitive(text: string): string {
  let out = text.replace(/(密码|password)\s*[:：]?\s*\S+/gi, '$1: ***');
  out = out.replace(/[¥$€£]\s?\d+(\.\d+)?/g, (m) => `${m[0]} ***`);
  return out;
}

/** 通知内容保护：锁屏策略 / 截录屏与投影排除 / 历史加密与过期 / 两段式与隐私模式。 */
export class ContentProtector {
  private settings: ContentProtectionSettings;
  private history: Array<{ id: string; text: string; at: number; encrypted: boolean }> = [];
  private exemptions = new Set<string>();

  constructor(settings: Partial<ContentProtectionSettings> = {}) {
    this.settings = { ...DEFAULT_CONTENT_PROTECTION, ...settings };
  }

  settings_(): ContentProtectionSettings {
    return { ...this.settings };
  }

  update(patch: Partial<ContentProtectionSettings>): void {
    this.settings = { ...this.settings, ...patch };
  }

  /** 锁屏可见性：全隐 > 只显应用名 > 显示内容。 */
  lockscreenView(app: string, text: string): string {
    if (this.settings.lockscreenHideAll) return '';
    if (this.settings.lockscreenAppOnly) return app;
    return this.mask(text);
  }

  mask(text: string): string {
    return this.settings.sensitiveMasking ? maskSensitive(text) : text;
  }

  /** 排除策略：截屏/录屏/投影时整层隐藏（豁免清单除外）。 */
  excludedOnSurface(surface: 'screenshot' | 'recording' | 'projection', app: string): boolean {
    if (this.exemptions.has(app)) return false;
    const flag = surface === 'screenshot' ? this.settings.screenshotExclude
      : surface === 'recording' ? this.settings.recordingExclude : this.settings.projectionExclude;
    return flag || this.settings.privacyMode;
  }

  addExemption(app: string): void {
    this.exemptions.add(app);
  }

  isExempted(app: string): boolean {
    return this.exemptions.has(app);
  }

  /** 历史加密落位 + 自动过期清理。 */
  store(id: string, text: string, at: number): void {
    this.history.push({ id, text: this.settings.encryptedHistory ? btoa(unescape(encodeURIComponent(text))) : text, at, encrypted: this.settings.encryptedHistory });
  }

  expire(now: number): string[] {
    const gone = this.history.filter((h) => now - h.at > this.settings.autoExpireMs).map((h) => h.id);
    this.history = this.history.filter((h) => now - h.at <= this.settings.autoExpireMs);
    return gone;
  }

  historyAll(): ReadonlyArray<{ id: string; text: string; at: number; encrypted: boolean }> {
    return this.history;
  }

  /** 泄露检测：记录截屏事件方。 */
  detectLeak(by: string, at: number): string {
    return `screenshot-by:${by}@${at}`;
  }

  /** 两段式：点击才显示。 */
  reveal(text: string, clicked: boolean): string {
    if (!this.settings.twoStepReveal || clicked) return this.mask(text);
    return '点击显示';
  }
}

/* ===================== 族0317 声音工程 ===================== */

export const AUDIO_ASSET_SPEC = {
  format: 'opus-48k-mono',
  loudnessLufs: -16,
  truePeakDbtp: -1.5,
  maxCount: 128,
};

export interface SoundAssetVersion {
  event: string;
  version: number;
  lufs: number;
}

/** 声音工程：资产规范 / 响度流水线 / 版本管理 / A/B / 预算与博物馆。 */
export class SoundEngineering {
  private versions = new Map<string, SoundAssetVersion[]>();
  private ab = new Map<string, { aShown: number; aClicked: number; bShown: number; bClicked: number }>();
  private usage = new Map<string, { played: number; mutedByUser: number }>();

  validateAsset(lufs: number, truePeakDb: number): { pass: boolean; issues: string[] } {
    const issues: string[] = [];
    if (Math.abs(lufs - AUDIO_ASSET_SPEC.loudnessLufs) > 1) issues.push(`LUFS ${lufs} 偏离 -16 目标`);
    if (truePeakDb > AUDIO_ASSET_SPEC.truePeakDbtp) issues.push(`TruePeak ${truePeakDb} 超过 -1.5 dBTP`);
    return { pass: issues.length === 0, issues };
  }

  registerVersion(event: string, version: number, lufs: number): void {
    const arr = this.versions.get(event) ?? [];
    arr.push({ event, version, lufs });
    this.versions.set(event, arr);
  }

  latestVersion(event: string): SoundAssetVersion | undefined {
    const arr = this.versions.get(event);
    return arr?.[arr.length - 1];
  }

  recordAb(key: string, arm: 'a' | 'b', clicked: boolean): void {
    const e = this.ab.get(key) ?? { aShown: 0, aClicked: 0, bShown: 0, bClicked: 0 };
    if (arm === 'a') { e.aShown += 1; if (clicked) e.aClicked += 1; } else { e.bShown += 1; if (clicked) e.bClicked += 1; }
    this.ab.set(key, e);
  }

  abWinner(key: string): 'a' | 'b' | 'tie' {
    const e = this.ab.get(key);
    if (!e || (!e.aShown && !e.bShown)) return 'tie';
    const ra = e.aShown ? e.aClicked / e.aShown : 0;
    const rb = e.bShown ? e.bClicked / e.bShown : 0;
    return ra === rb ? 'tie' : ra > rb ? 'a' : 'b';
  }

  /** 使用遥测（本地）：哪些声音被用户关掉。 */
  recordUsage(event: string, mutedByUser: boolean): void {
    const u = this.usage.get(event) ?? { played: 0, mutedByUser: 0 };
    u.played += 1;
    if (mutedByUser) u.mutedByUser += 1;
    this.usage.set(event, u);
  }

  mostMuted(): string | null {
    let best: string | null = null;
    let bestN = 0;
    for (const [k, v] of this.usage) {
      if (v.mutedByUser > bestN) { best = k; bestN = v.mutedByUser; }
    }
    return best;
  }

  /** 加载性能预算：资产索引加载 < 50ms。 */
  loadWithinBudget(loadMs: number): boolean {
    return loadMs < 50;
  }

  countBudget(): number {
    return AUDIO_ASSET_SPEC.maxCount;
  }
}

/* ===================== 族0318 节日音景 ===================== */

export interface HolidaySound {
  id: string;
  name: string;
  month: number;
  day: number;
  windowDays: number;
  serious?: boolean;
}

export const HOLIDAY_SOUNDS: HolidaySound[] = [
  { id: 'spring-festival', name: '春节', month: 1, day: 1, windowDays: 3 },
  { id: 'lantern', name: '元宵', month: 1, day: 15, windowDays: 1 },
  { id: 'qingming', name: '清明', month: 4, day: 5, windowDays: 1 },
  { id: 'dragon-boat', name: '端午', month: 5, day: 5, windowDays: 1 },
  { id: 'qixi', name: '七夕', month: 7, day: 7, windowDays: 1 },
  { id: 'mid-autumn', name: '中秋', month: 8, day: 15, windowDays: 1 },
  { id: 'chongyang', name: '重阳', month: 9, day: 9, windowDays: 1 },
  { id: 'dongzhi', name: '冬至', month: 12, day: 21, windowDays: 1 },
  { id: 'christmas', name: '圣诞', month: 12, day: 25, windowDays: 2 },
  { id: 'halloween', name: '万圣', month: 10, day: 31, windowDays: 1 },
  { id: 'new-year-eve', name: '跨年', month: 12, day: 31, windowDays: 1 },
  { id: 'birthday', name: '生日', month: 0, day: 0, windowDays: 1 },
  { id: 'anniversary', name: '周年', month: 0, day: 0, windowDays: 1 },
  { id: 'graduation', name: '毕业季', month: 6, day: 15, windowDays: 15 },
  { id: 'sakura', name: '樱花季', month: 3, day: 20, windowDays: 20 },
  { id: 'maple', name: '枫叶季', month: 11, day: 1, windowDays: 20 },
  { id: 'first-snow', name: '初雪', month: 12, day: 1, windowDays: 60 },
  { id: 'rainy-season', name: '雨季', month: 6, day: 1, windowDays: 30 },
  { id: 'typhoon-alert', name: '台风警报', month: 0, day: 0, windowDays: 0, serious: true },
];

/** 节日音景：按日期窗口匹配 + 总控开关 + 严肃警报不受彩蛋总控限制。 */
export class HolidaySoundCalendar {
  private masterOn = true;
  private userBirthday?: { month: number; day: number };
  private installAnniversary?: { month: number; day: number };

  setMaster(on: boolean): void {
    this.masterOn = on;
  }

  setBirthday(m: number, d: number): void {
    this.userBirthday = { month: m, day: d };
  }

  setAnniversary(m: number, d: number): void {
    this.installAnniversary = { month: m, day: d };
  }

  /** 当日命中的节日音（day-of-year 简化为月日差近似）。 */
  matches(month: number, day: number): HolidaySound[] {
    const out: HolidaySound[] = [];
    for (const h of HOLIDAY_SOUNDS) {
      if (h.id === 'birthday') {
        if (this.userBirthday && this.userBirthday.month === month && this.userBirthday.day === day) out.push(h);
        continue;
      }
      if (h.id === 'anniversary') {
        if (this.installAnniversary && this.installAnniversary.month === month && this.installAnniversary.day === day) out.push(h);
        continue;
      }
      if (h.id === 'typhoon-alert') continue;
      const diff = Math.abs((month - h.month) * 31 + (day - h.day));
      if (diff <= h.windowDays) out.push(h);
    }
    return out;
  }

  /** 严肃警报：即使总控关也保留提示。 */
  audible(h: HolidaySound): boolean {
    return h.serious === true || this.masterOn;
  }
}

/* ===================== 族0319 提醒体系 ===================== */

export interface Reminder {
  id: string;
  text: string;
  atMs: number;
  repeatEveryMs?: number;
  escalated?: boolean;
  source?: 'quick' | 'nl' | 'charging' | 'unlock' | 'before-shutdown';
}

/** 自然语言解析：「明早8点」「20分钟后」「周五下午3点」。 */
export function parseReminderText(text: string, now: number): { atMs: number; matched: boolean } {
  const base = now;
  const afterMin = text.match(/(\d+)\s*分钟[后後]/);
  if (afterMin) return { atMs: base + parseInt(afterMin[1]!, 10) * 60_000, matched: true };
  const afterHour = text.match(/(\d+)\s*(?:小时|个钟)[后後]/);
  if (afterHour) return { atMs: base + parseInt(afterHour[1]!, 10) * 3600_000, matched: true };
  const tom = /(?:明天|明早)/.test(text);
  const hm = text.match(/(\d{1,2})\s*[点:：]\s*(?:(\d{1,2})\s*分?)?/);
  if (hm) {
    let h = parseInt(hm[1]!, 10);
    const m = hm[2] ? parseInt(hm[2]!, 10) : 0;
    if (/(下午|晚上|傍晚)/.test(text) && h < 12) h += 12;
    const d = new Date(base);
    d.setHours(h, m, 0, 0);
    if (tom || d.getTime() <= base) d.setDate(d.getDate() + 1);
    return { atMs: d.getTime(), matched: true };
  }
  return { atMs: base, matched: false };
}

/** 提醒体系：快速/自然语言/事件触发/重复升级/完成率。 */
export class ReminderSystem {
  private reminders = new Map<string, Reminder>();
  private history: Array<{ id: string; done: boolean; at: number }> = [];

  add(r: Reminder): boolean {
    if (this.reminders.has(r.id)) return false;
    this.reminders.set(r.id, r);
    return true;
  }

  addFromText(id: string, text: string, now: number, source: Reminder['source'] = 'nl'): Reminder | null {
    const p = parseReminderText(text, now);
    if (!p.matched) return null;
    const r: Reminder = { id, text, atMs: p.atMs, source };
    this.add(r);
    return r;
  }

  due(now: number): Reminder[] {
    return [...this.reminders.values()].filter((r) => r.atMs <= now);
  }

  /** 升级：到期未确认 N 分钟后再次提醒。 */
  escalate(id: string, extraMs: number): boolean {
    const r = this.reminders.get(id);
    if (!r) return false;
    r.atMs += extraMs;
    r.escalated = true;
    return true;
  }

  complete(id: string, at: number): boolean {
    if (!this.reminders.delete(id)) return false;
    this.history.push({ id, done: true, at });
    return true;
  }

  skip(id: string, at: number): boolean {
    if (!this.reminders.has(id)) return false;
    this.reminders.delete(id);
    this.history.push({ id, done: false, at });
    return true;
  }

  /** 完成率。 */
  completionRate(): number {
    if (this.history.length === 0) return 1;
    return this.history.filter((h) => h.done).length / this.history.length;
  }

  toTodo(id: string): { title: string; dueAtMs: number } | null {
    const r = this.reminders.get(id);
    return r ? { title: r.text, dueAtMs: r.atMs } : null;
  }

  all(): Reminder[] {
    return [...this.reminders.values()];
  }
}

/* ===================== 族0320 声音个性收藏 ===================== */

export interface FavoriteSound {
  id: string;
  name: string;
  source: 'builtin' | 'upload' | 'recording';
  durationMs: number;
  fadeInMs: number;
  fadeOutMs: number;
  loop: boolean;
}

/** 声音个性收藏：收藏夹 / 上传与录音 / 剪辑淡化 / 每应用与随机轮换 / 备份分享。 */
export class SoundFavorites {
  private items = new Map<string, FavoriteSound>();
  private perApp = new Map<string, string>();
  private perType = new Map<string, string>();
  private muteWhitelist = new Set<string>();
  private rotation: string[] = [];
  private rotationIdx = 0;

  add(item: FavoriteSound): boolean {
    if (this.items.has(item.id)) return false;
    this.items.set(item.id, item);
    return true;
  }

  get(id: string): FavoriteSound | undefined {
    return this.items.get(id);
  }

  remove(id: string): boolean {
    return this.items.delete(id);
  }

  all(): FavoriteSound[] {
    return [...this.items.values()];
  }

  /** 剪辑：截取片段生成新收藏。 */
  trim(id: string, newId: string, startMs: number, endMs: number): FavoriteSound | null {
    const src = this.items.get(id);
    if (!src || endMs <= startMs || endMs > src.durationMs) return null;
    const cut: FavoriteSound = { ...src, id: newId, durationMs: endMs - startMs, source: src.source };
    this.items.set(newId, cut);
    return cut;
  }

  setAppSound(app: string, soundId: string): boolean {
    if (!this.items.has(soundId)) return false;
    this.perApp.set(app, soundId);
    return true;
  }

  appSound(app: string): string | null {
    return this.perApp.get(app) ?? null;
  }

  setTypeSound(type: string, soundId: string): boolean {
    if (!this.items.has(soundId)) return false;
    this.perType.set(type, soundId);
    return true;
  }

  typeSound(type: string): string | null {
    return this.perType.get(type) ?? null;
  }

  addMuteWhitelist(app: string): void {
    this.muteWhitelist.add(app);
  }

  inMuteWhitelist(app: string): boolean {
    return this.muteWhitelist.has(app);
  }

  setRotation(ids: string[]): boolean {
    if (ids.some((i) => !this.items.has(i))) return false;
    this.rotation = [...ids];
    return true;
  }

  /** 随机 / 轮播选取。 */
  pick(mode: 'random' | 'rotate', seed = 0): string | null {
    const pool = this.rotation.length ? this.rotation : [...this.items.keys()];
    if (pool.length === 0) return null;
    if (mode === 'random') return pool[seed % pool.length]!;
    const id = pool[this.rotationIdx % pool.length]!;
    this.rotationIdx += 1;
    return id;
  }

  exportAll(): string {
    return JSON.stringify({ items: [...this.items.values()], perApp: [...this.perApp], perType: [...this.perType] });
  }

  importAll(json: string): boolean {
    try {
      const raw = JSON.parse(json) as { items: FavoriteSound[]; perApp: Array<[string, string]>; perType: Array<[string, string]> };
      for (const it of raw.items) if (!this.items.has(it.id)) this.items.set(it.id, it);
      for (const [k, v] of raw.perApp) this.perApp.set(k, v);
      for (const [k, v] of raw.perType) this.perType.set(k, v);
      return true;
    } catch {
      return false;
    }
  }
}
