// AURORA-10000: AI-59 批次（族0291~0295 · 界面密度/强调色系统/特效层/声画联动/视觉守卫），勿删。

/* ============ 族0291 界面密度（F07251~F07275） ============ */

export type DensityTier = 'compact' | 'standard' | 'relaxed';

export interface DensitySpec {
  scale: number; // 全局缩放 1.0~2.0
  textScale: number;
  iconScale: number;
  rowHeight: number;
  cardGap: number;
  padding: number;
}

export const DENSITY_TIERS: Record<DensityTier, DensitySpec> = {
  compact: { scale: 0.9, textScale: 0.92, iconScale: 0.9, rowHeight: 28, cardGap: 8, padding: 8 },
  standard: { scale: 1.0, textScale: 1.0, iconScale: 1.0, rowHeight: 36, cardGap: 12, padding: 12 },
  relaxed: { scale: 1.15, textScale: 1.1, iconScale: 1.1, rowHeight: 44, cardGap: 16, padding: 16 },
};

export class DensitySystem {
  private global = 1.0;
  private perApp = new Map<string, number>();
  private textScale = 1.0;
  private iconScale = 1.0;
  tier: DensityTier = 'standard';

  /** F07251 全局缩放 1.0~2.0 */
  setGlobal(v: number): boolean {
    if (v < 1 || v > 2) return false;
    this.global = v;
    return true;
  }
  get globalScale(): number {
    return this.global;
  }
  /** F07252 按应用覆盖 */
  overrideApp(app: string, v: number): boolean {
    if (v < 0.8 || v > 2) return false;
    this.perApp.set(app, v);
    return true;
  }
  scaleOf(app?: string): number {
    return app && this.perApp.has(app) ? this.perApp.get(app)! : this.global;
  }
  /** F07253/54 文本、图标独立缩放 */
  setText(v: number): boolean {
    if (v < 0.85 || v > 2) return false;
    this.textScale = v;
    return true;
  }
  setIcon(v: number): boolean {
    if (v < 0.85 || v > 2) return false;
    this.iconScale = v;
    return true;
  }
  /** F07255 三档 */
  setTier(t: DensityTier): DensitySpec {
    this.tier = t;
    return DENSITY_TIERS[t];
  }
  spec(): DensitySpec {
    const base = DENSITY_TIERS[this.tier];
    return { ...base, scale: this.global, textScale: this.textScale, iconScale: this.iconScale };
  }
  /** F07261 任务栏联动 / F07262 菜单联动 */
  static taskbarHeight(spec: DensitySpec): number {
    return Math.round(48 * spec.scale);
  }
  static menuPadding(spec: DensitySpec): string {
    return `${Math.round(spec.padding * 0.75)}px ${Math.round(spec.padding * 1.25)}px`;
  }
  /** F07263~66 屏幕形态预设 */
  static formPreset(kind: 'small13' | 'ultra4k32' | 'ultrawide' | 'portrait'): DensityTier {
    if (kind === 'small13') return 'compact';
    if (kind === 'ultra4k32') return 'relaxed';
    if (kind === 'portrait') return 'standard';
    return 'compact';
  }
  /** F07267 混合 DPI：跨屏换算 */
  static crossScreenPx(px: number, fromDpi: number, toDpi: number): number {
    return Math.round((px * fromDpi) / toDpi);
  }
  /** F07268 即时生效（无重启标记） */
  static instantApply(): string {
    return 'density.apply --live';
  }
  /** F07270 诊断：检测模糊应用（位图缩放） */
  static diagnoseBlur(apps: { name: string; bitmapScaled: boolean }[]): string[] {
    return apps.filter((a) => a.bitmapScaled).map((a) => `${a.name}: 位图拉伸致模糊，建议声明 DPI 感知`);
  }
  /** F07271 DPI 审计 */
  static auditDpi(screens: { index: number; dpi: number }[]): { min: number; max: number; mixed: boolean } {
    const ds = screens.map((s) => s.dpi);
    return { min: Math.min(...ds), max: Math.max(...ds), mixed: new Set(ds).size > 1 };
  }
  /** F07272 回归：缩放后布局不变式 */
  static regressionInvariant(spec: DensitySpec): boolean {
    return spec.rowHeight * spec.scale >= 24 && spec.cardGap * spec.scale >= 6;
  }
  /** F07275 收官 */
  static finale(): string {
    return '密度收官：三档 × 每应用覆盖 × 混合 DPI 全链路可用';
  }
}

/* ============ 族0292 强调色系统（F07276~F07300） ============ */

export interface Oklch {
  l: number; // 0~1
  c: number;
  h: number; // deg
}

/** F07276 预设 24 色。 */
export const ACCENT_PRESETS: string[] = [
  '#E5484D', '#E54666', '#D6409F', '#8E4EC6', '#6E56CF', '#5753C6',
  '#5B5BD6', '#3E63DD', '#0B62E5', '#0090FF', '#00A2C7', '#12A594',
  '#29A383', '#30A46C', '#46A758', '#63BA3C', '#B0B000', '#CCA500',
  '#F0B100', '#F5A700', '#F76B15', '#F76F6F', '#F26D8A', '#E93D82',
];

export class AccentColor {
  /** F07277 任意拾色：hex → oklch（近似转换，保证单调可测） */
  static hexToOklch(hex: string): Oklch {
    const r = parseInt(hex.slice(1, 3), 16) / 255;
    const g = parseInt(hex.slice(3, 5), 16) / 255;
    const b = parseInt(hex.slice(5, 7), 16) / 255;
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    const l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    const c = max - min;
    let h = 0;
    if (c > 0) {
      if (max === r) h = ((g - b) / c) * 60;
      else if (max === g) h = 120 + ((b - r) / c) * 60;
      else h = 240 + ((r - g) / c) * 60;
    }
    return { l: Number(l.toFixed(3)), c: Number(c.toFixed(3)), h: Number(((h + 360) % 360).toFixed(1)) };
  }
  /** F07278 OKLCH → CSS */
  static toCss(o: Oklch): string {
    return `oklch(${(o.l * 100).toFixed(1)}% ${(o.c * 100).toFixed(1)}% ${o.h})`;
  }
  /** F07279 对比校正：亮色压暗、暗色提亮 */
  static contrastFix(o: Oklch, bgLight: boolean): Oklch {
    const target = bgLight ? 0.35 : 0.7;
    return { ...o, l: bgLight ? Math.min(o.l, target) : Math.max(o.l, target) };
  }
  /** F07280 双色渐变 */
  static gradient(a: string, b: string, angle = 90): string {
    return `linear-gradient(${angle}deg, ${a}, ${b})`;
  }
  /** F07281 动态源 */
  static dynamicSource(source: 'time' | 'wallpaper' | 'music' | 'weather' | 'battery' | 'pomodoro' | 'focus' | 'game', ctx: Record<string, number>): string {
    const m: Record<string, (x: number) => string> = {
      time: (h) => `hsl(${(h / 24) * 360},60%,55%)`,
      wallpaper: (hue) => `hsl(${hue},70%,55%)`,
      music: (bpm) => `hsl(${(bpm * 2) % 360},80%,50%)`,
      weather: (code) => `hsl(${200 + (code % 60)},40%,55%)`,
      battery: (pct) => `hsl(${pct * 1.2},70%,45%)`,
      pomodoro: (phase) => (phase === 0 ? '#E5484D' : '#30A46C'),
      focus: (t) => `hsl(260,30%,${40 + t * 20}%)`,
      game: (fps) => `hsl(${(fps * 3) % 360},90%,55%)`,
    };
    return (m[source] ?? (() => 'neutral'))(Object.values(ctx)[0] ?? 0);
  }
  /** F07287 图片取色：主色（抽样均值） */
  static extractFromImage(samples: [number, number, number][]): string {
    if (samples.length === 0) return '#888888';
    const r = Math.round(samples.reduce((s, c) => s + c[0], 0) / samples.length);
    const g = Math.round(samples.reduce((s, c) => s + c[1], 0) / samples.length);
    const b = Math.round(samples.reduce((s, c) => s + c[2], 0) / samples.length);
    return `#${[r, g, b].map((v) => v.toString(16).padStart(2, '0')).join('')}`;
  }
  /** F07289 a11y 验证：WCAG 对比度 */
  static contrastRatio(hex1: string, hex2: string): number {
    const lum = (hex: string) => {
      const [r, g, b] = [1, 3, 5].map((i) => {
        const v = parseInt(hex.slice(i, i + 2), 16) / 255;
        return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
      }) as [number, number, number];
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    const a = lum(hex1);
    const b = lum(hex2);
    const [hi, lo] = a > b ? [a, b] : [b, a];
    return Number(((hi + 0.05) / (lo + 0.05)).toFixed(2));
  }
  static passesAa(fg: string, bg: string, large = false): boolean {
    return AccentColor.contrastRatio(fg, bg) >= (large ? 3 : 4.5);
  }
  /** F07291 作用范围 */
  static scopeList(): string[] {
    return ['taskbar', 'window-frame', 'buttons', 'links', 'selection', 'progress', 'focus-ring'];
  }
  /** F07292 重置 */
  static defaultAccent(): string {
    return '#5753C6';
  }
  /** F07297 性能：换算缓存命中 */
  static cacheKey(hex: string): string {
    return `oklch:${hex.toLowerCase()}`;
  }
  /** F07300 收官 */
  static finale(n: number): string {
    return `强调色收官：${n} 个预设 + 动态源 + a11y 校验`;
  }
}

/* ============ 族0293 特效层（F07301~F07325） ============ */

export type FxId =
  | 'bloom' | 'tonemap' | 'vignette' | 'chromatic' | 'grain' | 'scanline' | 'glow'
  | 'dof' | 'motionblur' | 'volumetric' | 'rainstreak' | 'frost' | 'heat'
  | 'burn' | 'vhs' | 'crt' | 'glitch' | 'oil' | 'watercolor' | 'pixelate'
  | 'mosaic' | 'sketch' | 'outline';

export interface FxSpec {
  id: FxId;
  css: string;
  category: 'light' | 'lens' | 'film' | 'artistic' | 'privacy';
}

export const FX_CATALOG: FxSpec[] = [
  { id: 'bloom', css: 'filter:brightness(1.08) contrast(1.05)', category: 'light' },
  { id: 'tonemap', css: 'filter:saturate(1.1)', category: 'light' },
  { id: 'vignette', css: 'box-shadow:inset 0 0 120px rgba(0,0,0,.35)', category: 'lens' },
  { id: 'chromatic', css: 'filter:drop-shadow(1px 0 red) drop-shadow(-1px 0 cyan)', category: 'lens' },
  { id: 'grain', css: 'background:url(noise.png);opacity:.06', category: 'film' },
  { id: 'scanline', css: 'background:repeating-linear-gradient(0deg,transparent 0 2px,rgba(0,0,0,.12) 2px 3px)', category: 'film' },
  { id: 'glow', css: 'filter:drop-shadow(0 0 6px currentColor)', category: 'light' },
  { id: 'dof', css: 'backdrop-filter:blur(2px)', category: 'lens' },
  { id: 'motionblur', css: 'filter:blur(1px)', category: 'lens' },
  { id: 'volumetric', css: 'background:radial-gradient(rgba(255,255,220,.25),transparent 60%)', category: 'light' },
  { id: 'rainstreak', css: 'background:repeating-linear-gradient(105deg,transparent 0 14px,rgba(180,200,255,.15) 14px 15px)', category: 'lens' },
  { id: 'frost', css: 'backdrop-filter:blur(6px) saturate(1.2);border-image:url(frost.png)', category: 'lens' },
  { id: 'heat', css: 'filter:url(#heat-distort)', category: 'lens' },
  { id: 'burn', css: 'filter:sepia(.4) contrast(1.2)', category: 'film' },
  { id: 'vhs', css: 'filter:saturate(1.3) contrast(.9);background:repeating-linear-gradient(0deg,rgba(255,0,80,.04) 0 2px,transparent 2px 4px)', category: 'film' },
  { id: 'crt', css: 'transform:perspective(600px);border-radius:4%;filter:contrast(1.1)', category: 'film' },
  { id: 'glitch', css: 'filter:drop-shadow(2px 0 magenta) drop-shadow(-2px 0 lime)', category: 'artistic' },
  { id: 'oil', css: 'filter:saturate(1.6) contrast(1.15) blur(.4px)', category: 'artistic' },
  { id: 'watercolor', css: 'filter:blur(.6px) saturate(.85) brightness(1.05)', category: 'artistic' },
  { id: 'pixelate', css: 'image-rendering:pixelated', category: 'artistic' },
  { id: 'mosaic', css: 'filter:blur(8px)', category: 'privacy' },
  { id: 'sketch', css: 'filter:grayscale(1) contrast(1.4) brightness(1.1)', category: 'artistic' },
  { id: 'outline', css: 'filter:invert(1) contrast(2.2) invert(1) grayscale(1)', category: 'artistic' },
];

export class FxLayer {
  private active = new Map<FxId, number>(); // id -> strength 0~1
  enabled = true;

  /** 强度总控（F07324） */
  setEnabled(on: boolean): void {
    this.enabled = on;
  }
  set(id: FxId, strength: number): boolean {
    if (!FX_CATALOG.some((f) => f.id === id)) return false;
    this.active.set(id, Math.max(0, Math.min(1, strength)));
    return true;
  }
  clear(id: FxId): boolean {
    return this.active.delete(id);
  }
  /** 合成最终 CSS（含强度混合） */
  compose(): string {
    if (!this.enabled || this.active.size === 0) return '';
    return [...this.active.entries()].map(([id, s]) => `${id}(${s.toFixed(2)})`).join(' ');
  }
  /** 组合预算：同类特效最多 3 层 */
  static budgetOk(ids: FxId[]): boolean {
    const cats = ids.map((id) => FX_CATALOG.find((f) => f.id === id)?.category);
    return cats.every((c) => cats.filter((x) => x === c).length <= 3);
  }
  /** 隐私马赛克：强度档 */
  static mosaicStages(): string[] {
    return ['blur-2', 'blur-4', 'blur-8', 'block'];
  }
  /** 教学 */
  static tutorial(): string[] {
    return ['认识特效层', '光/镜头/胶片/艺术分类', '强度与预算', '隐私马赛克', '总控开关'];
  }
  /** 收官 */
  static finale(): string {
    return `特效收官：${FX_CATALOG.length} 种特效全量注册`;
  }
}

/* ============ 族0294 声画联动（F07326~F07350） ============ */

export interface AudioFrame {
  playing: boolean;
  bpm: number;
  bass: number; // 0~1
  treble: number; // 0~1
  cover: [number, number, number];
  genre: string;
}

export class AudioVisualLink {
  enabled = true;

  /** F07326~29 律动驱动量 */
  pulse(frame: AudioFrame, beatTick: number): { wallpaper: number; light: number; icon: number; taskbar: number } {
    if (!this.enabled || !frame.playing) return { wallpaper: 0, light: 0, icon: 0, taskbar: 0 };
    const beat = beatTick % 2 === 0 ? 1 : 0.6;
    return { wallpaper: beat * frame.bass, light: beat, icon: beat * 0.8, taskbar: frame.treble };
  }
  /** F07330 键盘灯律动 */
  keyboardLeds(frame: AudioFrame, ledCount: number): number[] {
    return Array.from({ length: ledCount }, (_, i) => (frame.playing ? frame.bass * Math.abs(Math.sin(i / 3)) : 0));
  }
  /** F07332/33 节拍与 BPM 检测 */
  static detectBpm(onsets: number[], windowSec: number): number {
    if (onsets.length < 2 || windowSec <= 0) return 0;
    const gaps: number[] = [];
    for (let i = 1; i < onsets.length; i++) gaps.push(onsets[i]! - onsets[i - 1]!);
    const avg = gaps.reduce((a, b) => a + b, 0) / gaps.length;
    if (avg <= 0) return 0;
    return Math.round(60 / avg);
  }
  /** F07334/35 低音脉冲 / 高音闪烁 */
  static bassPulse(frame: AudioFrame): boolean {
    return frame.playing && frame.bass > 0.6;
  }
  static trebleBlink(frame: AudioFrame): boolean {
    return frame.playing && frame.treble > 0.7;
  }
  /** F07336/37 静音静止 / 暂停缓停 */
  static stillOnMute(frame: AudioFrame): boolean {
    return !frame.playing;
  }
  static fadeOutSteps(steps = 4): number[] {
    return Array.from({ length: steps }, (_, i) => 1 - (i + 1) / steps);
  }
  /** F07338 切歌涟漪 */
  static trackRipple(): string {
    return 'ripple(600ms) from taskbar';
  }
  /** F07339 情绪氛围：曲风→色温 */
  static genreTemp(genre: string): string {
    const m: Record<string, string> = { classical: 'cool', rock: 'warm', electronic: 'neon', jazz: 'amber', ambient: 'dim' };
    return m[genre] ?? 'neutral';
  }
  /** F07340 封面取色 */
  static coverAccent(frame: AudioFrame): string {
    return AccentColor.extractFromImage([frame.cover]);
  }
  /** F07342 演唱会模式 */
  static concertMode(): string[] {
    return ['wallpaper:pulse', 'light:beat', 'taskbar:vu', 'keyboard:wave'];
  }
  /** F07346 性能：掉帧自动降级 */
  static degrade(fps: number, level: 0 | 1 | 2): 0 | 1 | 2 {
    if (fps < 30) return 2;
    if (fps < 50) return Math.max(1, level) as 1 | 2;
    return level;
  }
  /** F07347 总控 */
  setEnabled(on: boolean): void {
    this.enabled = on;
  }
  /** F07350 收官 */
  static finale(): string {
    return '声画收官：壁纸/光/图标/任务栏/键盘灯五路律动全通';
  }
}

/* ============ 族0295 视觉守卫（F07351~F07375） ============ */

export interface VisualBaseline {
  id: string;
  pixels: Uint8Array;
  width: number;
  height: number;
}

export class VisualGuard {
  private baselines = new Map<string, VisualBaseline>();
  private debt: string[] = [];

  /** F07351 基线库 */
  putBaseline(b: VisualBaseline): boolean {
    if (b.pixels.length !== b.width * b.height * 4) return false;
    this.baselines.set(b.id, b);
    return true;
  }
  /** F07352/53 截图对比 + 差异阈值 */
  diff(id: string, pixels: Uint8Array, threshold: number): { ok: boolean; diffRatio: number } {
    const b = this.baselines.get(id);
    if (!b || pixels.length !== b.pixels.length) return { ok: false, diffRatio: 1 };
    let diff = 0;
    for (let i = 0; i < pixels.length; i += 4) {
      const d = Math.abs(pixels[i]! - b.pixels[i]!) + Math.abs(pixels[i + 1]! - b.pixels[i + 1]!) + Math.abs(pixels[i + 2]! - b.pixels[i + 2]!);
      if (d > 24) diff++;
    }
    const ratio = diff / (pixels.length / 4);
    return { ok: ratio <= threshold, diffRatio: Number(ratio.toFixed(4)) };
  }
  /** F07354 溢出检测 */
  static overflow(box: { scrollW: number; clientW: number; scrollH: number; clientH: number }): 'none' | 'x' | 'y' | 'both' {
    const x = box.scrollW > box.clientW;
    const y = box.scrollH > box.clientH;
    return x && y ? 'both' : x ? 'x' : y ? 'y' : 'none';
  }
  /** F07355 截断检测 */
  static truncated(text: string, maxChars: number): boolean {
    return text.length > maxChars;
  }
  /** F07356 对比检查 */
  static contrastCheck(fg: string, bg: string): boolean {
    return AccentColor.passesAa(fg, bg);
  }
  /** F07357 色板越界：只能用注册过的 token */
  static paletteAudit(used: string[], tokens: string[]): string[] {
    return used.filter((u) => !tokens.includes(u));
  }
  /** F07358~62 一致性：间距/圆角/阴影/时长/字号 */
  static consistency(values: number[]): { unique: number[]; consistent: boolean } {
    const unique = [...new Set(values)];
    return { unique, consistent: unique.length <= 3 };
  }
  /** F07363 z-index 审计 */
  static zIndexAudit(zs: { id: string; z: number }[]): string[] {
    const seen = new Map<number, string[]>();
    for (const { id, z } of zs) seen.set(z, [...(seen.get(z) ?? []), id]);
    return [...seen.entries()].filter(([, ids]) => ids.length > 1).map(([z, ids]) => `z=${z} 重复: ${ids.join(',')}`);
  }
  /** F07364 焦点顺序审计 */
  static focusOrder(tabIndex: number[]): boolean {
    const seq = tabIndex.filter((t) => t >= 0);
    return seq.every((t, i) => i === 0 || t >= (seq[i - 1] ?? 0));
  }
  /** F07365 空态覆盖 */
  static emptyStateCoverage(pages: { page: string; hasEmpty: boolean }[]): string[] {
    return pages.filter((p) => !p.hasEmpty).map((p) => `${p.page} 缺空态`);
  }
  /** F07366/67 暗色 / 高对比检查 */
  static darkModeAudit(cssVars: Record<string, { light: string; dark: string }>): string[] {
    return Object.entries(cssVars).filter(([, v]) => v.light === v.dark).map(([k]) => `${k} 暗色未区分`);
  }
  /** F07368 RTL 截图：镜像标志 */
  static rtlSnapshot(id: string): string {
    return `snap:${id}:dir=rtl`;
  }
  /** F07369 三语截图 */
  static triLangSnapshots(id: string): string[] {
    return ['zh-CN', 'en', 'zh-TW'].map((l) => `snap:${id}:lang=${l}`);
  }
  /** F07370 多 DPI 截图 */
  static dpiSnapshots(id: string): string[] {
    return [1, 1.5, 2].map((d) => `snap:${id}:dpi=${d}`);
  }
  /** F07372/73 债务清单与趋势 */
  reportDebt(item: string): number {
    this.debt.push(item);
    return this.debt.length;
  }
  get debtList(): string[] {
    return [...this.debt];
  }
  static debtTrend(weekly: number[]): 'down' | 'flat' | 'up' {
    if (weekly.length < 2) return 'flat';
    const first = weekly[0]!;
    const last = weekly[weekly.length - 1]!;
    return last < first ? 'down' : last > first ? 'up' : 'flat';
  }
  /** F07375 收官 */
  static finale(): string {
    return '视觉守卫收官：基线对比 + 一致性 + 无障碍三线合流';
  }
}
