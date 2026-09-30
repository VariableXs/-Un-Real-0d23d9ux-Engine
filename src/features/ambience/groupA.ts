// AURORA-10000: AI-56 批次（族0276~0280 · 氛围光/屏保复兴/字体生态/图标包生态/触觉反馈），勿删。

/* ============ 族0276 氛围光（F06876~F06900） ============ */

export type AmbienceSource =
  | 'screen' | 'wallpaper' | 'content' | 'music' | 'game' | 'notify'
  | 'pomodoro' | 'focus' | 'sleep' | 'sunrise' | 'sunset' | 'weather'
  | 'time' | 'calendar' | 'note' | 'download' | 'charging' | 'battery'
  | 'dnd' | 'recording' | 'camera' | 'mic' | 'manual';

export interface AmbienceFrame {
  source: AmbienceSource;
  rgb: [number, number, number];
  intensity: number; // 0~1
}

export const clamp01 = (x: number) => (x < 0 ? 0 : x > 1 ? 1 : x);

/** F06876 屏幕环境光：按四边采样色生成四周背光。 */
export function screenAmbience(samples: [number, number, number][]): { top: string; right: string; bottom: string; left: string } {
  const avg = (arr: [number, number, number][]) => {
    if (arr.length === 0) return 'rgb(0,0,0)';
    const r = Math.round(arr.reduce((s, c) => s + c[0], 0) / arr.length);
    const g = Math.round(arr.reduce((s, c) => s + c[1], 0) / arr.length);
    const b = Math.round(arr.reduce((s, c) => s + c[2], 0) / arr.length);
    return `rgb(${r},${g},${b})`;
  };
  const n = Math.max(1, Math.floor(samples.length / 4));
  return {
    top: avg(samples.slice(0, n)),
    right: avg(samples.slice(n, 2 * n)),
    bottom: avg(samples.slice(2 * n, 3 * n)),
    left: avg(samples.slice(3 * n)),
  };
}

/** F06877 随壁纸：从壁纸主色派生光色。 */
export function ambienceFromWallpaper(hue: number, sat = 0.6): AmbienceFrame {
  const h = ((hue % 360) + 360) % 360;
  const c = sat * 255;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = 255 - c;
  const seg = Math.floor(h / 60) % 6;
  const tbl: [number, number, number][] = [[c, x, 0], [x, c, 0], [0, c, x], [0, x, c], [x, 0, c], [c, 0, x]];
  const [r, g, b] = (tbl[seg] ?? [m, m, m]).map((v) => Math.round(v + m)) as [number, number, number];
  return { source: 'wallpaper', rgb: [r, g, b], intensity: 0.5 };
}

/** F06878 随内容：画面平均色驱动。 */
export function ambienceFromContent(avg: [number, number, number]): AmbienceFrame {
  return { source: 'content', rgb: avg, intensity: clamp01((avg[0] + avg[1] + avg[2]) / (3 * 255)) };
}

/** F06879 强度调节。 */
export function setIntensity(frame: AmbienceFrame, v: number): AmbienceFrame {
  return { ...frame, intensity: clamp01(v) };
}

/** F06880 色温：2000K~9000K 映射 RGB。 */
export function kelvinToRgb(k: number): [number, number, number] {
  const t = clamp01((k - 2000) / 7000);
  const r = 255;
  const g = Math.round(120 + 135 * t);
  const b = Math.round(20 + 235 * t * t);
  return [r, g, b];
}

/** F06881 音乐律动：节拍驱动光强。 */
export function ambienceMusicPulse(frame: AmbienceFrame, beat: boolean, level: number): AmbienceFrame {
  return { ...frame, source: 'music', intensity: clamp01(beat ? 1 : level) };
}

/** F06882 游戏联动：帧率低于阈值自动降档。 */
export function ambienceGameMode(fps: number, frame: AmbienceFrame): { frame: AmbienceFrame; degraded: boolean } {
  if (fps >= 55) return { frame: { ...frame, source: 'game', intensity: 0.8 }, degraded: false };
  if (fps >= 30) return { frame: { ...frame, source: 'game', intensity: 0.4 }, degraded: false };
  return { frame: { ...frame, source: 'game', intensity: 0 }, degraded: true };
}

/** F06883 通知闪烁：两短一长。 */
export function ambienceNotifyFlash(ticks: number): boolean {
  return ticks % 4 === 0 || ticks % 4 === 1;
}

/** F06884 番茄光：工作红 / 休息绿。 */
export function pomodoroLight(phase: 'work' | 'break'): AmbienceFrame {
  return phase === 'work'
    ? { source: 'pomodoro', rgb: [220, 60, 50], intensity: 0.7 }
    : { source: 'pomodoro', rgb: [60, 200, 90], intensity: 0.5 };
}

/** F06885 专注呼吸：正弦呼吸光。 */
export function focusBreath(ticks: number): number {
  return clamp01(0.375 + 0.125 * Math.sin((ticks / 12) * Math.PI * 2));
}

/** F06886 睡眠光：30 分钟渐暗至熄灭。 */
export function sleepLight(minutes: number): number {
  return clamp01(1 - minutes / 30);
}

/** F06887 日出光：深蓝→暖金。 */
export function sunriseLight(progress: number): [number, number, number] {
  const p = clamp01(progress);
  return [Math.round(30 + 225 * p), Math.round(40 + 165 * p), Math.round(90 - 30 * p)];
}

/** F06888 日落光：亮金→暗紫。 */
export function sunsetLight(progress: number): [number, number, number] {
  const p = clamp01(progress);
  return [Math.round(255 - 160 * p), Math.round(170 - 110 * p), Math.round(60 + 80 * p)];
}

/** F06889 天气光：雨蓝 / 晴金。 */
export function weatherLight(kind: 'rain' | 'sunny' | 'cloudy'): AmbienceFrame {
  if (kind === 'rain') return { source: 'weather', rgb: [70, 110, 220], intensity: 0.6 };
  if (kind === 'sunny') return { source: 'weather', rgb: [250, 200, 80], intensity: 0.6 };
  return { source: 'weather', rgb: [160, 160, 170], intensity: 0.35 };
}

/** F06890 时间光：清晨/白天/黄昏/夜晚。 */
export function timeLight(hour: number): AmbienceFrame {
  if (hour >= 5 && hour < 9) return { source: 'time', rgb: [255, 190, 130], intensity: 0.5 };
  if (hour >= 9 && hour < 17) return { source: 'time', rgb: [220, 230, 255], intensity: 0.4 };
  if (hour >= 17 && hour < 20) return { source: 'time', rgb: [255, 140, 90], intensity: 0.5 };
  return { source: 'time', rgb: [90, 100, 190], intensity: 0.3 };
}

/** F06891 日历事件：会议前 10 分钟提示光。 */
export function calendarGlow(nowMin: number, meetingMin: number): boolean {
  return meetingMin - nowMin > 0 && meetingMin - nowMin <= 10;
}

/** F06892 便签提醒：闪烁三下。 */
export function noteBlink(ticks: number): boolean {
  return ticks % 6 < 3;
}

/** F06893 下载进度：进度映射光强。 */
export function downloadGlow(progress: number): number {
  return clamp01(progress);
}

/** F06894 充电光：呼吸周期。 */
export function chargingBreath(ticks: number): number {
  return clamp01(0.5 + 0.5 * Math.sin((ticks / 8) * Math.PI * 2));
}

/** F06895 低电红：<=15% 红色警示。 */
export function lowBatteryLight(pct: number): AmbienceFrame | null {
  if (pct > 15) return null;
  return { source: 'battery', rgb: [230, 40, 40], intensity: pct <= 5 ? 1 : 0.7 };
}

/** F06896 勿扰紫。 */
export function dndLight(): AmbienceFrame {
  return { source: 'dnd', rgb: [150, 80, 220], intensity: 0.4 };
}

/** F06897 录音红。 */
export function recordingLight(): AmbienceFrame {
  return { source: 'recording', rgb: [220, 30, 30], intensity: 0.9 };
}

/** F06898 摄像头绿。 */
export function cameraLight(on: boolean): AmbienceFrame {
  return on
    ? { source: 'camera', rgb: [40, 220, 90], intensity: 1 }
    : { source: 'camera', rgb: [40, 220, 90], intensity: 0 };
}

/** F06899 麦克风橙。 */
export function micLight(on: boolean): AmbienceFrame {
  return on
    ? { source: 'mic', rgb: [240, 160, 40], intensity: 1 }
    : { source: 'mic', rgb: [240, 160, 40], intensity: 0 };
}

/** F06900 氛围光教学：五步引导。 */
export const AMBIENCE_TUTORIAL = ['认识氛围光', '选择触发源', '调节强度与色温', '设置例外场景', '高级：自定义源'] as const;

/* ============ 族0277 屏保复兴（F06901~F06925） ============ */

export interface ScreensaverDef {
  id: string;
  name: string;
  kind: 'sim' | 'photo' | 'text' | 'video' | 'game' | 'clock';
}

/** F06901 屏保框架：25 款内置注册表。 */
export const SCREENSAVERS: ScreensaverDef[] = [
  { id: 'pipes', name: '3D 管道', kind: 'sim' },
  { id: 'text3d', name: '3D 文字', kind: 'text' },
  { id: 'starfield', name: '星空', kind: 'sim' },
  { id: 'aquarium', name: '鱼缸', kind: 'sim' },
  { id: 'garden', name: '花园', kind: 'sim' },
  { id: 'traffic', name: '车流', kind: 'sim' },
  { id: 'rainwindow', name: '雨窗', kind: 'sim' },
  { id: 'aurora', name: '极光', kind: 'sim' },
  { id: 'nebula', name: '星云', kind: 'sim' },
  { id: 'maze', name: '迷宫', kind: 'game' },
  { id: 'clockfall', name: '时钟瀑布', kind: 'clock' },
  { id: 'photowall', name: '照片墙', kind: 'photo' },
  { id: 'album', name: '相册轮播', kind: 'photo' },
  { id: 'quotes', name: '名言', kind: 'text' },
  { id: 'coderain', name: '代码雨', kind: 'sim' },
  { id: 'matrix', name: '矩阵雨', kind: 'sim' },
  { id: 'snake', name: '贪吃蛇', kind: 'game' },
  { id: 'bounce', name: '弹球', kind: 'game' },
  { id: 'chaos', name: '混沌吸引子', kind: 'sim' },
  { id: 'waves', name: '波浪', kind: 'sim' },
  { id: 'hourglass', name: '沙漏', kind: 'clock' },
  { id: 'sundial', name: '日晷', kind: 'clock' },
  { id: 'video', name: '自定义视频', kind: 'video' },
  { id: 'lissajous', name: '利萨茹', kind: 'sim' },
  { id: 'fireworks', name: '烟花', kind: 'sim' },
];

/** F06902 管道屏保：确定性步进。 */
export function pipesStep(state: { x: number; y: number; dir: number }, grid: number): { x: number; y: number; dir: number; turned: boolean } {
  const turn = (state.x * 7 + state.y * 13 + state.dir * 3) % 3 === 0;
  const dir = turn ? (state.dir + 1) % 4 : state.dir;
  const dx = [0, 1, 0, -1][dir] ?? 0;
  const dy = [1, 0, -1, 0][dir] ?? 0;
  let x = state.x + dx;
  let y = state.y + dy;
  let d = dir;
  if (x < 0 || y < 0 || x >= grid || y >= grid) {
    d = (d + 2) % 4;
    x = state.x + ([0, 1, 0, -1][d] ?? 0);
    y = state.y + ([1, 0, -1, 0][d] ?? 0);
  }
  return { x, y, dir: d, turned: turn };
}

/** F06903 3D 文字：浮动相位。 */
export function float3dText(t: number): { y: number; scale: number } {
  return { y: Math.sin(t / 20) * 10, scale: 1 + Math.sin(t / 40) * 0.05 };
}

/** F06904 星空：按深度前移。 */
export function starfieldStep(stars: { x: number; y: number; z: number }[]): { x: number; y: number; z: number }[] {
  return stars.map((s) => {
    const z = s.z - 0.01;
    return z <= 0 ? { x: Math.random(), y: Math.random(), z: 1 } : { x: s.x, y: s.y, z };
  });
}

/** F06905 鱼缸：鱼群巡游转向。 */
export function fishStep(fish: { x: number; vx: number }[], width: number): { x: number; vx: number }[] {
  return fish.map((f) => {
    let { x, vx } = f;
    x += vx;
    if (x < 0 || x > width) vx = -vx;
    return { x: Math.max(0, Math.min(width, x)), vx };
  });
}

/** F06906 花园：植物按天数生长到上限。 */
export function gardenGrowth(day: number, max = 100): number {
  return Math.min(max, day * 8);
}

/** F06907 车流：循环车道。 */
export function trafficStep(cars: number[], lanes: number, speed: number): number[] {
  return cars.map((p) => (p + speed) % lanes);
}

/** F06908 雨窗：雨滴下滑。 */
export function rainWindowStep(drops: { y: number }[], h: number): { y: number }[] {
  return drops.map((d) => ({ y: d.y + 2 > h ? 0 : d.y + 2 }));
}

/** F06909 极光：色带相位。 */
export function auroraBand(t: number): [number, number, number] {
  return [Math.round(60 + 60 * Math.sin(t / 15)), Math.round(160 + 80 * Math.sin(t / 9)), Math.round(120 + 60 * Math.cos(t / 12))];
}

/** F06910 星云：粒子密度场。 */
export function nebulaDensity(particles: { x: number; y: number }[], cx: number, cy: number, r: number): number {
  const inR = particles.filter((p) => (p.x - cx) ** 2 + (p.y - cy) ** 2 <= r * r).length;
  return particles.length === 0 ? 0 : inR / particles.length;
}

/** F06911 迷宫：右手探路一步。 */
export function mazeRightHand(facing: number, wallAhead: boolean, wallRight: boolean): number {
  if (!wallRight) return (facing + 1) % 4;
  if (!wallAhead) return facing;
  return (facing + 3) % 4;
}

/** F06912 时钟瀑布：秒列下落。 */
export function clockFallRow(sec: number, rows: number): number {
  return sec % rows;
}

/** F06913 照片墙：网格布局。 */
export function photoWallLayout(count: number, cols: number): { row: number; col: number }[] {
  return Array.from({ length: count }, (_, i) => ({ row: Math.floor(i / cols), col: i % cols }));
}

/** F06914 相册轮播：定时切换。 */
export function albumCarousel(total: number, sec: number, perSlide = 5): number {
  return total === 0 ? 0 : Math.floor(sec / perSlide) % total;
}

/** F06915 名言滚动：索引轮换。 */
export function quoteRoll(quotes: string[], t: number, per = 8): string {
  return quotes.length === 0 ? '' : quotes[Math.floor(t / per) % quotes.length] ?? '';
}

/** F06916 代码雨：列头下落。 */
export function codeRainStep(heads: number[], rows: number): number[] {
  return heads.map((h) => (h + 1) % rows);
}

/** F06917 矩阵雨：字符集。 */
export const MATRIX_GLYPHS = '01アイウエオカキクケコサシスセソ'.split('');
export function matrixGlyph(col: number, row: number): string {
  return MATRIX_GLYPHS[(col * 31 + row * 17) % MATRIX_GLYPHS.length] ?? '0';
}

/** F06918 贪吃蛇：步进与自撞判定。 */
export function snakeStep(body: [number, number][], dir: [number, number], grow: boolean): { body: [number, number][]; dead: boolean } {
  const head: [number, number] = [body[0]![0] + dir[0]!, body[0]![1] + dir[1]!];
  const next = [head, ...body];
  if (!grow) next.pop();
  const dead = next.slice(1).some((s) => s[0] === head[0] && s[1] === head[1]);
  return { body: next, dead };
}

/** F06919 弹球：边界反弹。 */
export function bounceStep(b: { x: number; y: number; vx: number; vy: number }, w: number, h: number): typeof b {
  let { x, y, vx, vy } = b;
  x += vx;
  y += vy;
  if (x < 0 || x > w) vx = -vx;
  if (y < 0 || y > h) vy = -vy;
  return { x: Math.max(0, Math.min(w, x)), y: Math.max(0, Math.min(h, y)), vx, vy };
}

/** F06920 混沌吸引子：洛伦兹一步。 */
export function lorenzStep(p: { x: number; y: number; z: number }): typeof p {
  const dt = 0.01;
  const dx = 10 * (p.y - p.x) * dt;
  const dy = (p.x * (28 - p.z) - p.y) * dt;
  const dz = (p.x * p.y - (8 / 3) * p.z) * dt;
  return { x: p.x + dx, y: p.y + dy, z: p.z + dz };
}

/** F06921 波浪：正弦叠加。 */
export function waveHeight(x: number, t: number): number {
  return Math.sin(x / 10 + t / 8) * 0.6 + Math.sin(x / 23 - t / 13) * 0.4;
}

/** F06922 沙漏：进度与翻转。 */
export function hourglassState(sec: number, cycle: number): { progress: number; flipped: boolean } {
  const phase = sec % (cycle * 2);
  return { progress: (phase % cycle) / cycle, flipped: phase >= cycle };
}

/** F06923 日晷：影子角度。 */
export function sundialShadow(hour: number): number {
  return ((hour - 12) / 12) * 180;
}

/** F06924 自定义视频：播放列表循环。 */
export function videoPlaylist(files: string[], sec: number, perFile = 60): string {
  return files.length === 0 ? '' : files[Math.floor(sec / perFile) % files.length] ?? '';
}

/** F06925 屏保教学：触发与退出规则。 */
export function screensaverPolicy(idleSec: number, resumeInput: boolean): { active: boolean; exited: boolean } {
  return { active: idleSec >= 300, exited: resumeInput };
}

/* ============ 族0278 字体生态（F06926~F06950） ============ */

export interface FontEntry {
  family: string;
  category: 'sans' | 'serif' | 'mono' | 'handwrite' | 'display';
  scripts: string[]; // 'latin' | 'cjk' ...
  axes?: string[]; // 可变字体轴
  license: 'free' | 'commercial' | 'unknown';
  active: boolean;
  installed: boolean;
  usage: number;
}

export class FontManager {
  private fonts = new Map<string, FontEntry>();
  private favs = new Set<string>();
  private temps = new Set<string>();
  private backups: FontEntry[] = [];

  add(f: FontEntry): boolean {
    if (this.fonts.has(f.family)) return false;
    this.fonts.set(f.family, f);
    return true;
  }
  /** F06926 全量列表 */
  list(): FontEntry[] {
    return [...this.fonts.values()];
  }
  get(family: string): FontEntry | undefined {
    return this.fonts.get(family);
  }
  /** F06927 样张预览串 */
  static preview(family: string): string {
    return `${family}: The quick brown fox 永和国安发 alphabets 0123456789`;
  }
  /** F06928 分类浏览 */
  byCategory(c: FontEntry['category']): FontEntry[] {
    return this.list().filter((f) => f.category === c);
  }
  /** F06929 收藏 */
  toggleFav(family: string): boolean {
    if (this.favs.has(family)) {
      this.favs.delete(family);
      return false;
    }
    if (!this.fonts.has(family)) return false;
    this.favs.add(family);
    return true;
  }
  get favorites(): string[] {
    return [...this.favs];
  }
  /** F06930 多字体对比 */
  static compare(families: string[]): string[] {
    return families.map((f) => FontManager.preview(f));
  }
  /** F06931 拖入安装（去重） */
  install(family: string): boolean {
    const f = this.fonts.get(family);
    if (!f) return false;
    if (f.installed) return false;
    f.installed = true;
    return true;
  }
  /** F06932 安全卸载：被引用的拒绝卸载 */
  uninstall(family: string, inUse: boolean): boolean {
    const f = this.fonts.get(family);
    if (!f || !f.installed || inUse) return false;
    f.installed = false;
    return true;
  }
  /** F06933 激活/停用 */
  setActive(family: string, active: boolean): boolean {
    const f = this.fonts.get(family);
    if (!f) return false;
    f.active = active;
    return true;
  }
  /** F06934 缺字回退提示 */
  static fallbackChain(family: string, missingScript: string): string {
    return `${family} 缺 ${missingScript} 字形，回退: Noto Sans CJK`;
  }
  /** F06935 渲染平滑设置 */
  static renderSettings(mode: 'none' | 'grayscale' | 'subpixel'): string {
    return `font-smooth:${mode}`;
  }
  /** F06936 可变字体轴 */
  static variableAxes(f: FontEntry): string[] {
    return f.axes ?? [];
  }
  /** F06937 子集预览（CJK 常用 3500 抽样） */
  static subsetPreview(script: string, size: number): string[] {
    const base = script === 'cjk' ? '永安一国是在了我有和人这中大为上个' : 'abcdefghijklmnopqrstuvwxyz';
    return Array.from({ length: Math.min(size, base.length) }, (_, i) => base[i] ?? base[0]!);
  }
  /** F06938 版权信息 */
  licenseOf(family: string): string {
    const f = this.fonts.get(family);
    return f ? f.license : 'unknown';
  }
  /** F06939 免费可商用推荐 */
  freeCommercial(): FontEntry[] {
    return this.list().filter((f) => f.license === 'free');
  }
  /** F06940 中文专区 */
  cjkZone(): FontEntry[] {
    return this.list().filter((f) => f.scripts.includes('cjk'));
  }
  /** F06941 手写专区 */
  handwriteZone(): FontEntry[] {
    return this.byCategory('handwrite');
  }
  /** F06942 编程专区（等宽） */
  codeZone(): FontEntry[] {
    return this.byCategory('mono');
  }
  /** F06943 终端联动 */
  static terminalFont(family: string): string {
    return `terminal.font-family=${family}, monospace`;
  }
  /** F06944 主题联动 */
  static themeFont(theme: string, family: string): string {
    return `${theme}.ui-font=${family}`;
  }
  /** F06945 临时加载（不安装使用） */
  tempLoad(family: string): boolean {
    if (!this.fonts.has(family)) return false;
    this.temps.add(family);
    return true;
  }
  get tempLoaded(): string[] {
    return [...this.temps];
  }
  /** F06946 备份 */
  backup(): number {
    this.backups = this.list().map((f) => ({ ...f }));
    return this.backups.length;
  }
  get backupCount(): number {
    return this.backups.length;
  }
  /** F06947 使用统计 */
  stats(): { family: string; usage: number }[] {
    return this.list()
      .map((f) => ({ family: f.family, usage: f.usage }))
      .sort((a, b) => b.usage - a.usage);
  }
  /** F06948 缺失修复建议 */
  static repairSuggestion(family: string, installed: boolean): string {
    return installed ? `${family} 正常` : `${family} 未安装，建议从备份恢复或重新安装`;
  }
  /** F06949 教学 */
  static tutorial(): string[] {
    return ['浏览与预览', '安装与停用', '可变字体调轴', '缺字与回退', '版权与备份'];
  }
  /** F06950 彩蛋：连续收藏 8 款解锁 */
  static easterUnlocked(favCount: number): boolean {
    return favCount >= 8;
  }
}

/* ============ 族0279 图标包生态（F06951~F06975） ============ */

export interface IconPackDef {
  id: string;
  name: string;
  author: string;
  version: string;
  rating: number;
  covers: string[]; // 支持的应用类别
  formats: ('png' | 'svg' | 'gif')[];
  dynamic: boolean;
  signature: string;
}

export class IconPackManager {
  private market: IconPackDef[] = [];
  private installed = new Map<string, IconPackDef>();
  private mappings = new Map<string, string>(); // app -> packId
  private active: string | null = null;

  /** F06951 市场列表 */
  publish(p: IconPackDef): boolean {
    if (this.market.some((m) => m.id === p.id)) return false;
    this.market.push(p);
    return true;
  }
  get marketList(): IconPackDef[] {
    return [...this.market];
  }
  /** F06952 应用前预览：抽 3 个映射样例 */
  preview(p: IconPackDef, apps: string[]): { app: string; icon: string }[] {
    return apps.slice(0, 3).map((a) => ({ app: a, icon: `${p.id}/${a}.svg` }));
  }
  /** F06953 一键全量应用 */
  applyAll(id: string): boolean {
    if (!this.installed.has(id)) return false;
    this.active = id;
    return true;
  }
  get activePack(): string | null {
    return this.active;
  }
  /** F06954 部分应用 / F06955 按类别 */
  applyCategory(id: string, cats: string[]): string[] {
    if (!this.installed.has(id)) return [];
    const pack = this.installed.get(id)!;
    return cats.filter((c) => pack.covers.includes(c));
  }
  /** F06956 自定义映射（允许先映射后装包，缺失由冲突检测报告） */
  map(app: string, packId: string): boolean {
    this.mappings.set(app, packId);
    return true;
  }
  iconOf(app: string): string | null {
    const p = this.mappings.get(app) ?? this.active;
    return p ? `${p}/${app}.svg` : null;
  }
  /** F06957 冲突检测：映射里缺失的包 */
  conflicts(): string[] {
    return [...this.mappings.entries()].filter(([, p]) => !this.installed.has(p)).map(([app]) => app);
  }
  /** F06958 更新 */
  update(id: string, version: string): boolean {
    const p = this.installed.get(id);
    if (!p || p.version === version) return false;
    p.version = version;
    return true;
  }
  /** F06959 安装（去重） */
  install(id: string): boolean {
    const p = this.market.find((m) => m.id === id);
    if (!p || this.installed.has(id)) return false;
    this.installed.set(id, { ...p });
    return true;
  }
  /** F06960 评分 */
  rate(id: string, stars: number): number {
    const p = this.market.find((m) => m.id === id);
    if (!p) return -1;
    p.rating = Math.max(0, Math.min(5, (p.rating + Math.max(0, Math.min(5, stars))) / 2));
    return p.rating;
  }
  /** F06961 动态图标：按时段 */
  static dynamicIcon(packId: string, hour: number): string {
    const phase = hour >= 6 && hour < 18 ? 'day' : 'night';
    return `${packId}/app.${phase}.svg`;
  }
  /** F06962 状态图标 */
  static statusIcon(packId: string, app: string, status: 'normal' | 'update' | 'running'): string {
    return status === 'normal' ? `${packId}/${app}.svg` : `${packId}/${app}.${status}.svg`;
  }
  /** F06963 SVG 支持 */
  static supportsSvg(p: IconPackDef): boolean {
    return p.formats.includes('svg');
  }
  /** F06964 动画图标 */
  static isAnimated(p: IconPackDef, file: string): boolean {
    return file.endsWith('.gif') && p.formats.includes('gif');
  }
  /** F06965 第三方导入：带签名即收 */
  importExternal(p: IconPackDef): boolean {
    if (!p.signature.startsWith('sig-')) return false;
    return this.publish(p);
  }
  /** F06966 导出 */
  exportPack(id: string): IconPackDef | null {
    return this.installed.get(id) ?? this.market.find((m) => m.id === id) ?? null;
  }
  /** F06967 教学 */
  static tutorial(): string[] {
    return ['逛市场', '预览试装', '全量/部分应用', '自定义映射', '备份与导出'];
  }
  /** F06968 彩蛋：装满 5 包解锁 */
  static easterUnlocked(installedCount: number): boolean {
    return installedCount >= 5;
  }
  /** F06969 图标 API */
  static apiSpec(): string {
    return 'variable.icons.resolve(app, status?, hour?) -> path';
  }
  /** F06970 包内预览 */
  inPackBrowse(id: string): string[] {
    const p = this.installed.get(id) ?? this.market.find((m) => m.id === id);
    return p ? p.covers.map((c) => `${id}/${c}/*`) : [];
  }
  /** F06971 按应用换装 */
  perApp(app: string, packId: string): boolean {
    return this.map(app, packId);
  }
  /** F06972 快捷方式图标 */
  static shortcutIcon(packId: string, target: string): string {
    return `${packId}/shortcuts/${target}.svg`;
  }
  /** F06973 文件夹图标库 */
  static folderIcons(packId: string): string[] {
    return [`${packId}/folder.open.svg`, `${packId}/folder.closed.svg`, `${packId}/folder.special.svg`];
  }
  /** F06974 备份：导出全部映射 */
  backupMappings(): { app: Record<string, string>; pack: string | null; active: string | null } {
    return { app: Object.fromEntries(this.mappings), pack: '', active: this.active };
  }
  /** F06975 收官：生态健康度 */
  ecosystemHealth(): { packs: number; installed: number; conflicts: number } {
    return { packs: this.market.length, installed: this.installed.size, conflicts: this.conflicts().length };
  }
}

/* ============ 族0280 触觉反馈（F06976~F07000） ============ */

export type HapticPattern = 'none' | 'tap' | 'double' | 'long' | 'success' | 'fail' | 'alarm' | 'urgent' | 'rhythm' | 'pulse';

export class HapticsManager {
  intensity = 0.7;
  enabled = true;
  foregroundOnly = true;
  private schemes = new Map<string, HapticPattern>();
  private played: HapticPattern[] = [];

  constructor() {
    this.schemes.set('default', 'tap');
  }
  /** F06993 总控 */
  setEnabled(on: boolean): void {
    this.enabled = on;
  }
  private canPlay(foreground: boolean): boolean {
    return this.enabled && (!this.foregroundOnly || foreground);
  }
  /** F06976 强度 */
  setIntensity(v: number): void {
    this.intensity = Math.max(0, Math.min(1, v));
  }
  /** F06977 按键振动 */
  keypress(foreground = true): HapticPattern {
    const p: HapticPattern = this.canPlay(foreground) ? 'tap' : 'none';
    this.played.push(p);
    return p;
  }
  /** F06978 通知样式（可区分） */
  notify(kind: 'message' | 'mail' | 'calendar'): HapticPattern {
    const p: HapticPattern = kind === 'message' ? 'double' : kind === 'mail' ? 'long' : 'pulse';
    if (this.canPlay(true)) this.played.push(p);
    return p;
  }
  /** F06979 成败振动 */
  outcome(ok: boolean): HapticPattern {
    const p: HapticPattern = ok ? 'success' : 'fail';
    if (this.canPlay(true)) this.played.push(p);
    return p;
  }
  /** F06980 闹钟模式 */
  alarm(): HapticPattern {
    if (this.canPlay(true)) this.played.push('alarm');
    return 'alarm';
  }
  /** F06981 紧急强提醒 */
  urgent(): HapticPattern {
    if (this.canPlay(true)) this.played.push('urgent');
    return 'urgent';
  }
  /** F06982 静音时仍振动 */
  silentModeVibrate(): HapticPattern {
    if (!this.enabled) return 'none';
    this.played.push('double');
    return 'double';
  }
  /** F06983 探索振动（触摸确认） */
  explore(): HapticPattern {
    const p: HapticPattern = this.canPlay(true) ? 'tap' : 'none';
    if (p !== 'none') this.played.push(p);
    return p;
  }
  /** F06984 长按 */
  longPress(): HapticPattern {
    const p: HapticPattern = this.canPlay(true) ? 'long' : 'none';
    if (p !== 'none') this.played.push(p);
    return p;
  }
  /** F06985 滑动手感 */
  scroll(): HapticPattern {
    const p: HapticPattern = this.canPlay(true) ? 'tap' : 'none';
    if (p !== 'none') this.played.push(p);
    return p;
  }
  /** F06986 打字节奏：每 4 键一记重振动 */
  typingRhythm(keys: number): HapticPattern {
    const p: HapticPattern = keys > 0 && keys % 4 === 0 ? 'rhythm' : 'tap';
    if (this.canPlay(true)) this.played.push(p);
    return p;
  }
  /** F06987 番茄提醒 */
  pomodoroBuzz(phase: 'work' | 'break'): HapticPattern {
    const p: HapticPattern = phase === 'work' ? 'pulse' : 'success';
    if (this.canPlay(true)) this.played.push(p);
    return p;
  }
  /** F06988 久坐提醒 */
  sedentaryBuzz(idleMin: number): boolean {
    if (idleMin >= 50 && this.canPlay(true)) {
      this.played.push('pulse');
      return true;
    }
    return false;
  }
  /** F06989 倒计时最后三秒 */
  countdownTick(secLeft: number): HapticPattern {
    const p: HapticPattern = secLeft <= 3 && secLeft > 0 ? 'pulse' : 'none';
    if (p !== 'none' && this.canPlay(true)) this.played.push(p);
    return p;
  }
  /** F06990 来电样式位（接口冻结） */
  static callPatternSlot(): HapticPattern {
    return 'double';
  }
  /** F06991 测试台 */
  testBench(): HapticPattern[] {
    return ['tap', 'double', 'long', 'success', 'fail', 'alarm', 'urgent'];
  }
  /** F06992 方案保存 */
  saveScheme(name: string, pattern: HapticPattern): boolean {
    if (this.schemes.has(name)) return false;
    this.schemes.set(name, pattern);
    return true;
  }
  get schemeList(): string[] {
    return [...this.schemes.keys()];
  }
  /** F06994 游戏位（预留） */
  static gamePatternSlot(): HapticPattern {
    return 'rhythm';
  }
  /** F06995 教学 */
  static tutorial(): string[] {
    return ['强度调节', '按键与滚动', '通知区分', '方案保存', '测试台自测'];
  }
  /** F06996 彩蛋：播放满 100 次解锁 */
  static easterUnlocked(playCount: number): boolean {
    return playCount >= 100;
  }
  /** F06997 与动效同步 */
  static syncWithMotion(motionMs: number): HapticPattern {
    return motionMs <= 150 ? 'tap' : 'long';
  }
  /** F06998 仅前台 */
  setForegroundOnly(on: boolean): void {
    this.foregroundOnly = on;
  }
  /** F06999 收官：自检 */
  selfCheck(): boolean {
    return this.enabled && this.intensity >= 0 && this.intensity <= 1 && this.schemeList.includes('default');
  }
  /** F07000 致谢 */
  static credits(): string {
    return '致谢：触觉反馈体验由无障碍共创小组与早期内测用户共同打磨';
  }
}
