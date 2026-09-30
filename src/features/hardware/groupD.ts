// AURORA-10000: AI-39 批次（族0191~0195 · 触屏与笔/摄像头与影像/显示器色准/声音空间化/扫描与文档摄入），勿删。
import { clamp, fnv1a } from './hwModel';

/* ============ 族0191 触屏与笔（F04751~F04775） ============ */

/** F04751~F04754 手势全套目录与边缘滑出。 */
export const TOUCH_GESTURES = ['tap', 'double-tap', 'long-press', 'swipe', 'pinch', 'rotate', 'edge-swipe', 'three-finger-drag'] as const;
export const EDGE_ZONES = ['left', 'right', 'top', 'bottom'] as const;
export type EdgeZone = (typeof EDGE_ZONES)[number];
export function edgeSwipe(zone: EdgeZone): string {
  return `edge:${zone}`;
}
/** F04758/F04759 压感曲线与倾斜。 */
export function pressureCurve(input: number, gamma = 1.0): number {
  return Math.round(Math.pow(clamp(input, 0, 1), gamma) * 100) / 100;
}
export function tiltStroke(tiltDeg: number, baseWidth: number): number {
  return Math.round(baseWidth * (1 + Math.sin((clamp(tiltDeg, 0, 89) * Math.PI) / 180)) * 100) / 100;
}
/** F04762 圈选：点在多边形内（射线法）。 */
export function lassoHit(poly: { x: number; y: number }[], p: { x: number; y: number }): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const a = poly[i]!;
    const b = poly[j]!;
    if (a.y > p.y !== b.y > p.y && p.x < ((b.x - a.x) * (p.y - a.y)) / (b.y - a.y) + a.x) inside = !inside;
  }
  return inside;
}
/** F04763 笔迹美化：滑动平均平滑。 */
export function strokeSmooth(points: number[], win = 3): number[] {
  const out: number[] = [];
  for (let i = 0; i < points.length; i++) {
    let s = 0;
    let n = 0;
    for (let j = Math.max(0, i - win + 1); j <= i; j++) {
      s += points[j]!;
      n++;
    }
    out.push(Math.round((s / n) * 100) / 100);
  }
  return out;
}
/** F04764 掌压防误：掌触面积大且无笔在场则忽略。 */
export function palmRejection(areaMm2: number, penInRange: boolean, contactCount: number): boolean {
  if (penInRange && areaMm2 > 400) return true;
  return !penInRange && contactCount > 3 && areaMm2 > 900;
}
/** F04765~F04767 惯性/捏合/旋转。 */
export function flingVelocity(v0: number, decay = 0.92, frames = 10): number {
  let v = v0;
  for (let i = 0; i < frames; i++) v *= decay;
  return Math.round(v);
}
export function pinchScale(dist0: number, dist1: number): number {
  if (dist0 <= 0) return 1;
  return Math.round((dist1 / dist0) * 100) / 100;
}
export function twoFingerRotate(a: { x: number; y: number }, b: { x: number; y: number }): number {
  return Math.round((Math.atan2(b.y - a.y, b.x - a.x) * 180) / Math.PI);
}
/** F04770 长按右键。 */
export function longPressMenu(ms: number, threshold = 500): boolean {
  return ms >= threshold;
}

/* ============ 族0192 摄像头与影像（F04776~F04800） ============ */

/** F04776~F04780 相机模式。 */
export const CAMERA_MODES = ['photo', 'video', 'burst', 'timer', 'scan'] as const;
export type CameraMode = (typeof CAMERA_MODES)[number];
export class CameraSession {
  mode: CameraMode = 'photo';
  private shots: string[] = [];
  setMode(m: CameraMode): boolean {
    this.mode = m;
    return true;
  }
  capture(frame: string): string {
    const id = fnv1a(frame + this.shots.length);
    this.shots.push(id);
    return id;
  }
  get count(): number {
    return this.shots.length;
  }
  burst(n: number): string[] {
    const out: string[] = [];
    for (let i = 0; i < n; i++) out.push(this.capture(`b${i}`));
    return out;
  }
}
/** F04781/F04782 构图网格与水平仪。 */
export const GRID_TYPES = ['off', 'rule-of-thirds', 'golden', 'square'] as const;
export function horizonLevel(pitchDeg: number): { level: boolean; offset: number } {
  return { level: Math.abs(pitchDeg) < 1, offset: Math.round(pitchDeg * 100) / 100 };
}
/** F04787 扫码：EAN-13 校验位。 */
export function ean13Check(digits: string): boolean {
  if (!/^\d{13}$/.test(digits)) return false;
  let sum = 0;
  for (let i = 0; i < 12; i++) sum += Number(digits[i]) * (i % 2 === 0 ? 1 : 3);
  return (10 - (sum % 10)) % 10 === Number(digits[12]);
}
/** F04788 文档拍摄：四角检测与自动裁边。 */
export function docCrop(corners: { x: number; y: number }[], frame: { w: number; h: number }): { ok: boolean; rect: { x: number; y: number; w: number; h: number } } | { ok: false } {
  if (corners.length !== 4) return { ok: false };
  const xs = corners.map((c) => c.x);
  const ys = corners.map((c) => c.y);
  const x0 = Math.max(0, Math.min(...xs));
  const y0 = Math.max(0, Math.min(...ys));
  const x1 = Math.min(frame.w, Math.max(...xs));
  const y1 = Math.min(frame.h, Math.max(...ys));
  if (x1 - x0 < 10 || y1 - y0 < 10) return { ok: false };
  return { ok: true, rect: { x: x0, y: y0, w: x1 - x0, h: y1 - y0 } };
}
/** F04791/F04792 隐私灯与遮挡提示。 */
export function privacyLed(streaming: boolean, ledOn: boolean): boolean {
  return streaming === ledOn;
}
export function coverHint(lux: number): 'covered' | 'clear' {
  return lux < 5 ? 'covered' : 'clear';
}
/** F04793 多摄切换。 */
export class MultiCam {
  private cams: string[] = [];
  constructor(ids: string[]) {
    this.cams = [...ids];
  }
  get count(): number {
    return this.cams.length;
  }
  active = 0;
  cycle(): string | null {
    if (this.cams.length === 0) return null;
    this.active = (this.active + 1) % this.cams.length;
    return this.cams[this.active]!;
  }
}
/** F04797/F04798 画质设置与参数显示。 */
export const QUALITY_PRESETS = [
  { name: '1080p30', w: 1920, h: 1080, fps: 30 },
  { name: '1080p60', w: 1920, h: 1080, fps: 60 },
  { name: '4K30', w: 3840, h: 2160, fps: 30 },
] as const;
export function pickQuality(maxW: number, maxFps: number): string | null {
  return QUALITY_PRESETS.find((q) => q.w <= maxW && q.fps <= maxFps)?.name ?? null;
}

/* ============ 族0193 显示器色准（F04801~F04825） ============ */

/** F04801~F04803 ICC 管理与校准向导。 */
export interface IccProfile {
  name: string;
  whitePointK: number;
  gamma: number;
  display: string;
}
export class IccManager {
  private profiles = new Map<string, IccProfile>();
  private assigned = new Map<string, string>();
  add(p: IccProfile): boolean {
    if (this.profiles.has(p.name)) return false;
    this.profiles.set(p.name, p);
    return true;
  }
  assign(display: string, profile: string): boolean {
    if (!this.profiles.has(profile)) return false;
    this.assigned.set(display, profile);
    return true;
  }
  of(display: string): IccProfile | undefined {
    const n = this.assigned.get(display);
    return n ? this.profiles.get(n) : undefined;
  }
}
export const CAL_STEPS = ['亮度对比', '伽马', '白点', '色域确认'] as const;
/** F04804/F04805 伽马与白点校准。 */
export function gammaStep(rgbIn: number, gamma = 2.2): number {
  return Math.round(Math.pow(clamp(rgbIn, 0, 1), 1 / gamma) * 1000) / 1000;
}
export function whitePointDelta(curK: number, targetK: number): number {
  return Math.round(Math.abs(curK - targetK));
}
/** F04808 多屏一致性。 */
export function displayConsistency(kelvins: number[], tol = 150): { ok: boolean; spread: number } {
  if (kelvins.length < 2) return { ok: true, spread: 0 };
  const spread = Math.max(...kelvins) - Math.min(...kelvins);
  return { ok: spread <= tol, spread };
}
/** F04809 渐进夜灯：随日落线性降色温。 */
export function gradualNightLight(hour: number, dayK = 6500, nightK = 3400, start = 19, end = 22): number {
  if (hour <= start) return dayK;
  if (hour >= end) return nightK;
  return Math.round(dayK - ((dayK - nightK) * (hour - start)) / (end - start));
}
/** F04814 色盲模拟（通道重映射近似）。 */
export function cvdSimulate(r: number, g: number, b: number, kind: 'protan' | 'deutan' | 'tritan'): [number, number, number] {
  const m =
    kind === 'protan'
      ? [0.567, 0.433, 0]
      : kind === 'deutan'
        ? [0.625, 0.375, 0]
        : [0.95, 0.05, 0];
  return [Math.round(r * m[0]! + g * m[1]! + b * m[2]!), g, b];
}
/** F04815 对比度检查器（WCAG）。 */
export function contrastRatio(fg: [number, number, number], bg: [number, number, number]): number {
  const lum = (c: [number, number, number]) => {
    const s = c.map((v) => {
      const x = v / 255;
      return x <= 0.03928 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
    });
    return 0.2126 * s[0]! + 0.7152 * s[1]! + 0.0722 * s[2]!;
  };
  const l1 = lum(fg);
  const l2 = lum(bg);
  const [hi, lo] = l1 > l2 ? [l1, l2] : [l2, l1];
  return Math.round(((hi + 0.05) / (lo + 0.05)) * 100) / 100;
}
/** F04817 坏点检测：纯色帧序列。 */
export function deadPixelPatterns(): string[] {
  return ['white', 'black', 'red', 'green', 'blue'];
}
/** F04821/F04822 蓝光剂量与护眼报告。 */
export class BlueLightMeter {
  private dose = 0;
  expose(luxBlue: number, minutes: number): number {
    this.dose += (luxBlue * minutes) / 60;
    return Math.round(this.dose);
  }
  get total(): number {
    return Math.round(this.dose);
  }
}

/* ============ 族0194 声音空间化（F04826~F04850） ============ */

/** F04826/F04828/F04829 空间音频与窗声像。 */
export class SpatialAudio {
  enabled = false;
  level = 0;
  toggle(on: boolean): boolean {
    this.enabled = on;
    return this.enabled;
  }
  setLevel(l: number): number {
    this.level = clamp(l, 0, 100);
    return this.level;
  }
  /** 声随窗位：窗中心 x 归一 -1~1 → 左右增益。 */
  panForWindow(xNorm: number): { l: number; r: number } {
    const x = clamp(xNorm, -1, 1);
    return { l: Math.round(100 - ((x + 1) / 2) * 100), r: Math.round(((x + 1) / 2) * 100) };
  }
}
/** F04832 辨位训练：方位角判分。 */
export function localizationQuiz(answerDeg: number, truthDeg: number, tol = 15): boolean {
  const d = Math.abs(((answerDeg - truthDeg + 540) % 360) - 180);
  return d <= tol;
}
/** F04835/F04836 对白增强与夜间压缩。 */
export function dialogueBoost(centerGain: number, surroundGain: number): number {
  return clamp(centerGain - surroundGain * 0.5, 0, 12);
}
export function nightCompress(sample: number, ratio = 4, threshold = 0.3): number {
  const s = clamp(sample, -1, 1);
  const a = Math.abs(s);
  return a <= threshold ? s : Math.sign(s) * (threshold + (a - threshold) / ratio);
}
/** F04837 等响度：小音量补偿。 */
export function loudnessCompensate(volPct: number): number {
  const v = clamp(volPct, 0, 100) / 100;
  return Math.round((v < 0.3 ? (0.3 - v) * 20 : 0) * 10) / 10;
}
/** F04838/F04839 采样率与位深。 */
export const SAMPLE_RATES = [44100, 48000, 96000, 192000] as const;
export const BIT_DEPTHS = [16, 24, 32] as const;
export function formatValid(sr: number, bits: number): boolean {
  return (SAMPLE_RATES as readonly number[]).includes(sr) && (BIT_DEPTHS as readonly number[]).includes(bits);
}
/** F04842 环路测延迟。 */
export function loopbackLatency(framesIn: number, framesOut: number, sampleRate = 48000): number {
  const d = Math.abs(framesIn - framesOut);
  return Math.round((d / sampleRate) * 1000 * 10) / 10;
}
/** F04844/F04845 静音检测与爆音修复。 */
export function silenceDetect(samples: number[], threshold = 0.001): boolean {
  return samples.every((s) => Math.abs(s) <= threshold);
}
export function clipRepair(samples: number[]): number[] {
  return samples.map((s) => clamp(s, -0.98, 0.98));
}
/** F04847/F04848 内录与多路输出。 */
export function multiOutput(devices: string[]): { ok: boolean; count: number } {
  const uniq = new Set(devices);
  return { ok: uniq.size === devices.length && uniq.size >= 2, count: uniq.size };
}
export function routeOutput(stream: string, device: string): string {
  return `${stream}→${device}`;
}

/* ============ 族0195 扫描与文档摄入（F04851~F04875） ============ */

/** F04851~F04854 扫描会话与多页合并。 */
export class ScanSession {
  private pages: string[] = [];
  source: 'flatbed' | 'adf' | 'photo' = 'flatbed';
  setSource(s: 'flatbed' | 'adf' | 'photo'): void {
    this.source = s;
  }
  addPage(content: string): string {
    const id = fnv1a(content + this.pages.length);
    this.pages.push(id);
    return id;
  }
  mergePdf(): { pages: number; digest: string } {
    return { pages: this.pages.length, digest: fnv1a(this.pages.join('|')) };
  }
  cancel(): number {
    const n = this.pages.length;
    this.pages = [];
    return n;
  }
}
/** F04855/F04856 自动裁边与纠偏。 */
export function deskew(angleDeg: number, tol = 0.5): { ok: boolean; corrected: number } {
  const corrected = Math.round(angleDeg * 100) / 100;
  return { ok: Math.abs(angleDeg) <= 15, corrected: Math.abs(angleDeg) > tol ? corrected : 0 };
}
/** F04857/F04858 去底色与增强。 */
export function removeBackground(px: number, bg: number, tol = 30): number {
  return Math.abs(px - bg) <= tol ? 255 : px;
}
export function enhanceContrast(px: number, low: number, high: number): number {
  if (high <= low) return px;
  return Math.round(clamp(((px - low) / (high - low)) * 255, 0, 255));
}
/** F04860 命名建议。 */
export function scanNaming(date: string, kind: string, seq: number): string {
  return `${date}_${kind}_${String(seq).padStart(3, '0')}`;
}
/** F04861~F04864 格式/双面/DPI/色彩模式。 */
export const SCAN_FORMATS = ['pdf', 'jpg', 'png'] as const;
export const SCAN_DPI = [150, 300, 600, 1200] as const;
export const SCAN_COLORMODES = ['color', 'gray', 'bw'] as const;
export function scanConfigValid(dpi: number, mode: string, fmt: string): boolean {
  return (SCAN_DPI as readonly number[]).includes(dpi) && (SCAN_COLORMODES as readonly string[]).includes(mode) && (SCAN_FORMATS as readonly string[]).includes(fmt);
}
/** F04868 扫描预设。 */
export interface ScanPreset {
  name: string;
  dpi: number;
  color: string;
  fmt: string;
}
export class PresetStore {
  private m = new Map<string, ScanPreset>();
  save(p: ScanPreset): boolean {
    if (this.m.has(p.name)) return false;
    this.m.set(p.name, p);
    return true;
  }
  get(name: string): ScanPreset | undefined {
    return this.m.get(name);
  }
}
/** F04872 驱动状态。 */
export function scannerDriver(state: { loaded: boolean; twain: boolean; wia: boolean }): 'ok' | 'partial' | 'bad' {
  if (state.loaded && (state.twain || state.wia)) return 'ok';
  if (state.loaded) return 'partial';
  return 'bad';
}

/* ============ 补充助手（AI-39 批次内聚实现），勿删 ============ */

/** F04753/F04754 手势→动作映射。 */
export function gestureAction(g: string): string {
  const map: Record<string, string> = {
    'three-finger-drag': 'task-view',
    'four-finger-swipe-left': 'switch-desk',
    'edge-swipe-left': 'switch-desk',
    'edge-swipe-top': 'task-view',
  };
  return map[g] ?? 'none';
}
/** F04755/F04933 触屏键盘行数。 */
export function touchKeyboardRows(compact: boolean): number {
  return compact ? 4 : 5;
}
/** F04756/F04934 手写面板最小尺寸。 */
export function handwritingArea(w: number, h: number): { ok: boolean; w: number; h: number } {
  return { ok: w >= 200 && h >= 80, w, h };
}
/** F04757/F04760 笔尾按钮动作。 */
export type PenTailAction = 'eraser' | 'menu' | 'launch';
export function penTailAction(mode: PenTailAction): { action: PenTailAction; available: true } {
  return { action: mode, available: true };
}
/** F04761 双击笔身切换工具。 */
export function barrelDoubleClick(ms: number, gap = 400): boolean {
  return ms > 0 && ms <= gap;
}
/** F04768 文本选择柄。 */
export function selectionHandles(active: boolean): { count: number; active: boolean } {
  return { count: active ? 2 : 0, active };
}
/** F04769 精确光标模式。 */
export function preciseCursor(enabled: boolean): { enabled: boolean; dotRadius: number } {
  return { enabled, dotRadius: enabled ? 4 : 0 };
}
/** F04771 拖拽助手出手柄。 */
export function dragHelper(distancePx: number, threshold = 120): boolean {
  return distancePx > threshold;
}
/** F04772 浮动键盘可拖动。 */
export function floatingKeyboard(docked: boolean): { movable: boolean; docked: boolean } {
  return { movable: !docked, docked };
}
/** F04773 支架模式（仅触屏输入）。 */
export function standMode(angleDeg: number): { active: boolean; keyboard: boolean } {
  return { active: angleDeg > 200, keyboard: false };
}
/** F04774 儿童触屏手势简化。 */
export function kidsTouchSimplify(gestures: string[]): string[] {
  const allowed = ['tap', 'double-tap', 'swipe'];
  return gestures.filter((g) => allowed.includes(g));
}
/** F04778 录像时长。 */
export function videoDuration(frames: number, fps: number): number {
  return fps > 0 ? Math.round((frames / fps) * 10) / 10 : 0;
}
/** F04780 定时拍摄倒计时。 */
export function timerShot(sec: number): number[] {
  return Array.from({ length: Math.max(0, sec) }, (_, i) => sec - i);
}
/** F04785 实时滤镜。 */
export const FILTERS = ['none', 'mono', 'warm', 'cool', 'vivid'] as const;
export type FilterName = (typeof FILTERS)[number];
export function applyFilter(px: [number, number, number], f: FilterName): [number, number, number] {
  const [r, g, b] = px;
  if (f === 'mono') return [Math.round(0.299 * r + 0.587 * g + 0.114 * b), Math.round(0.299 * r + 0.587 * g + 0.114 * b), Math.round(0.299 * r + 0.587 * g + 0.114 * b)];
  if (f === 'warm') return [clamp(r + 10, 0, 255), g, clamp(b - 10, 0, 255)];
  if (f === 'cool') return [clamp(r - 10, 0, 255), g, clamp(b + 10, 0, 255)];
  if (f === 'vivid') return [clamp(r * 1.2, 0, 255), clamp(g * 1.2, 0, 255), clamp(b * 1.2, 0, 255)].map(Math.round) as [number, number, number];
  return [r, g, b];
}
/** F04789 白板增强：二值化。 */
export function whiteboardEnhance(px: number, threshold = 160): number {
  return px >= threshold ? 255 : 0;
}
/** F04790 OCR 拍摄：词数统计。 */
export function ocrShot(text: string): { words: number; chars: number } {
  return { words: text.split(/\s+/).filter(Boolean).length, chars: text.length };
}
/** F04806 色彩空间。 */
export const COLOR_SPACES = ['sRGB', 'Display P3', 'Adobe RGB', 'DCI-P3'] as const;
/** F04803 亮度对比校准步。 */
export function brightnessContrastStep(bright: number, contrast: number): { ok: boolean; bright: number; contrast: number } {
  return { ok: bright >= 0 && bright <= 100 && contrast >= 0 && contrast <= 100, bright: clamp(bright, 0, 100), contrast: clamp(contrast, 0, 100) };
}
/** F04810 校准提醒。 */
export function calibReminder(lastDaysAgo: number, intervalDays: number): boolean {
  return lastDaysAgo >= intervalDays;
}
/** F04811 校准历史。 */
export class CalibHistory {
  private h: { date: string; deltaE: number }[] = [];
  push(date: string, deltaE: number): void {
    this.h.push({ date, deltaE });
  }
  get last(): { date: string; deltaE: number } | null {
    return this.h[this.h.length - 1] ?? null;
  }
  get count(): number {
    return this.h.length;
  }
}
/** F04812 设计师（DTP）模式。 */
export function dtpMode(on: boolean): { on: boolean; softproof: boolean } {
  return { on, softproof: on };
}
/** F04813 CMYK 预览（近似转换）。 */
export function cmykPreview(r: number, g: number, b: number): { c: number; m: number; y: number; k: number } {
  const rr = r / 255;
  const gg = g / 255;
  const bb = b / 255;
  const k = 1 - Math.max(rr, gg, bb);
  const d = 1 - k || 1;
  return { c: Math.round(((1 - rr - k) / d) * 100), m: Math.round(((1 - gg - k) / d) * 100), y: Math.round(((1 - bb - k) / d) * 100), k: Math.round(k * 100) };
}
/** F04822 护眼报告。 */
export function eyeCareReport(blueDose: number, _nightLightHours: number): { strain: 'low' | 'mid' | 'high'; advice: string } {
  const strain = blueDose < 500 ? 'low' : blueDose < 1500 ? 'mid' : 'high';
  return { strain, advice: strain === 'high' ? '建议开启夜灯并每 20 分钟远眺' : '用眼状态良好' };
}
/** F04831 游戏声像增强。 */
export function gameSpatial(enabled: boolean): { enabled: boolean; boostDb: number } {
  return { enabled, boostDb: enabled ? 3 : 0 };
}
/** F04833 低音管理分频点。 */
export function bassManage(xoverHz: number): { ok: boolean; xoverHz: number } {
  const v = clamp(xoverHz, 60, 200);
  return { ok: v === xoverHz, xoverHz: v };
}
/** F04840 独占授权。 */
export function exclusiveAuthorize(grant: boolean): { granted: boolean; exclusive: boolean } {
  return { granted: grant, exclusive: grant };
}
/** F04843 逐声道测试。 */
export const CHANNEL_ORDER = ['FL', 'FR', 'FC', 'LFE', 'SL', 'SR'] as const;
export function channelTest(order: readonly string[]): boolean {
  return CHANNEL_ORDER.every((c, i) => order[i] === c);
}
/** F04846 耳返监听。 */
export function earReturn(on: boolean, latencyMs: number): { on: boolean; ok: boolean } {
  return { on, ok: !on || latencyMs <= 10 };
}
/** F04847 内录（系统声录制）。 */
export function loopbackRecord(device: string): { stream: string; source: 'loopback' } {
  return { stream: `loopback:${device}`, source: 'loopback' };
}
/** F04862 双面扫描页序。 */
export function duplexScan(pages: number): number[] {
  const out: number[] = [];
  for (let i = 1; i <= pages; i += 2) {
    out.push(i);
    if (i + 1 <= pages) out.push(i + 1);
  }
  return out;
}
/** F04865 扫描历史。 */
export class ScanHistory {
  private h: string[] = [];
  push(name: string): void {
    this.h.push(name);
  }
  recent(n: number): string[] {
    return this.h.slice(-n);
  }
  get count(): number {
    return this.h.length;
  }
}
/** F04867 扫描保存位置。 */
export function scanSaveFolder(base: string, date: string): string {
  return `${base}/${date}`;
}
/** F04871 扫描诊断步骤。 */
export const SCAN_DIAG_STEPS = ['设备枚举', '驱动加载', '试扫', '接口吞吐'] as const;
/** F04873 翻拍眩光检测。 */
export function rephoto(glarePct: number): { ok: boolean; glarePct: number } {
  return { ok: glarePct < 10, glarePct };
}
