// UNREAL-X：AI-12 族0113「桌面截图美学」（X02801~X02825）。
// 桌面截图/录屏静帧的构图美学评分、参数档位、延迟与格式治理。纯本地计算。

export type ShotMode = 'full' | 'window' | 'region' | 'scroll' | 'timed';
export const SHOT_MODES: ShotMode[] = ['full', 'window', 'region', 'scroll', 'timed'];

export type ShotFormat = 'png' | 'jpeg' | 'webp';
export const SHOT_FORMATS: ShotFormat[] = ['png', 'jpeg', 'webp'];

export type ThemeName = 'dark' | 'light' | 'high-contrast';

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface ShotOptions {
  mode: ShotMode;
  delayMs: number;
  includeCursor: boolean;
  annotate: boolean;
  format: ShotFormat;
  quality: number;
  scale: number;
  theme: ThemeName;
  reduceMotion: boolean;
  watermark: boolean;
}

/** 默认档 = 现状手感：区域截图、无延迟、PNG、1x、跟随系统主题。 */
export const DEFAULT_SHOT: ShotOptions = {
  mode: 'region',
  delayMs: 0,
  includeCursor: false,
  annotate: true,
  format: 'png',
  quality: 90,
  scale: 1,
  theme: 'dark',
  reduceMotion: false,
  watermark: false,
};

export const SHOT_LEVELS: Array<{ level: number; scale: number; quality: number }> = [
  { level: 0, scale: 1, quality: 60 },
  { level: 1, scale: 1, quality: 75 },
  { level: 2, scale: 1, quality: 90 },
  { level: 3, scale: 2, quality: 90 },
  { level: 4, scale: 3, quality: 100 },
];

export const MAX_DELAY_MS = 10_000;
export const MAX_SCALE = 3;

export interface ShotRecord {
  id: string;
  region: Rect;
  mode: ShotMode;
  bytes: number;
  at: number;
  score: number;
}

const clamp = (v: number, lo: number, hi: number): number => (v < lo ? lo : v > hi ? hi : v);

/** 三分线（两条竖线 + 两条横线）。 */
export function thirds(region: Rect): { xs: number[]; ys: number[] } {
  return {
    xs: [region.x + region.w / 3, region.x + (region.w * 2) / 3],
    ys: [region.y + region.h / 3, region.y + (region.h * 2) / 3],
  };
}

/** 主体中心与最近三分交点的贴合度 0~100。 */
export function ruleOfThirds(region: Rect, subject: Rect): number {
  const t = thirds(region);
  const cx = subject.x + subject.w / 2;
  const cy = subject.y + subject.h / 2;
  let best = Number.POSITIVE_INFINITY;
  for (const x of t.xs) {
    for (const y of t.ys) {
      const d = Math.hypot(cx - x, cy - y);
      if (d < best) best = d;
    }
  }
  const diag = Math.hypot(region.w, region.h) || 1;
  return Math.round(100 - clamp((best / (diag / 2)) * 100, 0, 100));
}

/** 四周留白均衡度 0~100（越均匀越高）。 */
export function marginBalance(region: Rect, subject: Rect): number {
  const l = subject.x - region.x;
  const r = region.x + region.w - (subject.x + subject.w);
  const t = subject.y - region.y;
  const b = region.y + region.h - (subject.y + subject.h);
  if (Math.min(l, r, t, b) < 0) return 0;
  const max = Math.max(l, r, t, b) || 1;
  const min = Math.min(l, r, t, b);
  return Math.round((min / max) * 100);
}

const COMMON_RATIOS = [16 / 9, 4 / 3, 1, 3 / 2, 21 / 9];

/** 画幅比例贴近常见比例的程度 0~100。 */
export function aspectScore(region: Rect): number {
  if (region.h <= 0) return 0;
  const r = region.w / region.h;
  let best = Number.POSITIVE_INFINITY;
  for (const c of COMMON_RATIOS) {
    const d = Math.abs(r - c) / c;
    if (d < best) best = d;
  }
  return Math.round(100 - clamp(best * 200, 0, 100));
}

/** 构图美学综合分：三分 40 + 留白 30 + 画幅 30。 */
export function beautyScore(region: Rect, subject: Rect): number {
  return Math.round(ruleOfThirds(region, subject) * 0.4 + marginBalance(region, subject) * 0.3 + aspectScore(region) * 0.3);
}

const FORMAT_FACTOR: Record<ShotFormat, number> = { png: 0.6, jpeg: 0.18, webp: 0.12 };

export class ScreenshotStudio {
  private opts: ShotOptions = { ...DEFAULT_SHOT };
  private shots: ShotRecord[] = [];

  constructor(patch?: Partial<ShotOptions>) {
    if (patch) this.configure(patch);
  }

  get options(): ShotOptions {
    return { ...this.opts };
  }

  /** 参数与配置面：全部输入钳制，非法值回落默认档。 */
  configure(patch: Partial<ShotOptions>): boolean {
    let ok = true;
    if (patch.mode !== undefined) {
      if (SHOT_MODES.includes(patch.mode)) this.opts.mode = patch.mode;
      else ok = false;
    }
    if (patch.delayMs !== undefined) {
      if (patch.delayMs >= 0 && patch.delayMs <= MAX_DELAY_MS) this.opts.delayMs = patch.delayMs;
      else {
        this.opts.delayMs = 0;
        ok = false;
      }
    }
    if (patch.quality !== undefined) {
      if (patch.quality >= 1 && patch.quality <= 100) this.opts.quality = Math.round(patch.quality);
      else {
        this.opts.quality = DEFAULT_SHOT.quality;
        ok = false;
      }
    }
    if (patch.scale !== undefined) {
      if (patch.scale >= 1 && patch.scale <= MAX_SCALE) this.opts.scale = patch.scale;
      else {
        this.opts.scale = 1;
        ok = false;
      }
    }
    if (patch.format !== undefined) {
      if (SHOT_FORMATS.includes(patch.format)) this.opts.format = patch.format;
      else ok = false;
    }
    if (patch.theme !== undefined) this.opts.theme = patch.theme;
    if (patch.includeCursor !== undefined) this.opts.includeCursor = !!patch.includeCursor;
    if (patch.annotate !== undefined) this.opts.annotate = !!patch.annotate;
    if (patch.reduceMotion !== undefined) this.opts.reduceMotion = !!patch.reduceMotion;
    if (patch.watermark !== undefined) this.opts.watermark = !!patch.watermark;
    return ok;
  }

  /** 档位矩阵：五档独立可交付。 */
  applyLevel(level: number): ShotOptions {
    const idx = clamp(Math.round(level), 0, SHOT_LEVELS.length - 1);
    const l = SHOT_LEVELS[idx]!;
    this.configure({ scale: l.scale, quality: l.quality });
    return this.options;
  }

  /** 体积估算：宽×高×4B×格式系数×质量系数×缩放²。 */
  estimateBytes(region: Rect): number {
    const px = Math.max(0, region.w) * Math.max(0, region.h);
    const q = this.opts.format === 'png' ? 1 : this.opts.quality / 100;
    return Math.round(px * 4 * FORMAT_FACTOR[this.opts.format] * q * this.opts.scale * this.opts.scale);
  }

  /** 截图：去重（同区域同模式 5s 内不重复登记）。 */
  capture(region: Rect, subject: Rect, at: number, id?: string): { ok: boolean; reason?: string } {
    if (region.w <= 0 || region.h <= 0) return { ok: false, reason: 'E01 · 选区为空，请重新框选后重试' };
    const dupe = this.shots.some(
      (s) => s.region.x === region.x && s.region.y === region.y && s.region.w === region.w && s.region.h === region.h && at - s.at < 5000,
    );
    if (dupe) return { ok: false, reason: 'E04 · 刚刚已截过同一区域，可稍后重截或调整选区' };
    const score = beautyScore(region, subject);
    this.shots.push({
      id: id ?? `shot-${this.shots.length + 1}`,
      region,
      mode: this.opts.mode,
      bytes: this.estimateBytes(region),
      at,
      score,
    });
    return { ok: true };
  }

  list(): ShotRecord[] {
    return this.shots.map((s) => ({ ...s }));
  }

  get count(): number {
    return this.shots.length;
  }

  best(): ShotRecord | undefined {
    return this.shots.slice().sort((a, b) => b.score - a.score)[0];
  }

  /** 微文案：中文语境、长度克制。 */
  hint(): string {
    return this.opts.delayMs > 0 ? `延迟 ${this.opts.delayMs} 毫秒后截图` : '立即截图';
  }

  /** 无障碍等价通道：HC 主题下强制关闭水印与动效。 */
  a11yAdjust(): ShotOptions {
    if (this.opts.theme === 'high-contrast') {
      this.opts.watermark = false;
      this.opts.reduceMotion = true;
    }
    return this.options;
  }

  /** 回滚与卸载净身。 */
  uninstall(): boolean {
    this.shots = [];
    this.opts = { ...DEFAULT_SHOT };
    return this.shots.length === 0;
  }
}
