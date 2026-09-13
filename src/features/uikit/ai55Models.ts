/**
 * UNREAL-X-15000 · AI-55 主题流水线与视觉回归 逻辑核（族0542/0543/0546/0547/0548/0550 · V 线 6 族），勿删。
 * 施工规范：《docs/UI-品质深化完整方案与步骤.md》§16.3 / §17 / §7.2 / §13。
 * 五层 × 五档模板：基础实装/边界与恢复/手感与细节/性能与优化/创新拓展。全部确定性算法，零 AI。
 */

/* -------- 族0542 主题六元组持久化（X13526~X13550 · §16.3）-------- */

export const DENSITIES = ['compact', 'comfortable'] as const;
export type Density = (typeof DENSITIES)[number];
export const RADII = [16, 12, 8, 4] as const;
export const MOTION_TIERS = [0, 1, 2, 3, 4, 5] as const;
export const MATERIALS = ['solid', 'frosted', 'acrylic', 'mica', 'glow'] as const;
export type Material = (typeof MATERIALS)[number];

export interface Hexad {
  wallpaper: string;
  palette: string;
  density: Density;
  radius: number;
  motion: number;
  material: Material;
}

export const HEXAD_DEFAULT: Hexad = {
  wallpaper: 'default',
  palette: 'base',
  density: 'comfortable',
  radius: 12,
  motion: 3,
  material: 'mica',
};

/** 主题六元组：wallpaper/palette/density/radius/motion/material 持久化 + 快照迁移。 */
export class ThemeHexad {
  data: Hexad = { ...HEXAD_DEFAULT };
  clamped = 0;
  version = 1;
  set<K extends keyof Hexad>(key: K, val: Hexad[K]): Hexad[K] {
    return (this.data[key] = val);
  }
  setDensity(d: string): Density {
    if (!(DENSITIES as readonly string[]).includes(d)) {
      this.clamped++;
      return (this.data.density = 'comfortable');
    }
    return (this.data.density = d as Density);
  }
  /** 圆角语义不混用：窗口16 > 卡12 > 控8 > 片4，越界回卡 12。 */
  setRadius(r: number): number {
    if (!RADII.includes(r as (typeof RADII)[number]) || !Number.isFinite(r)) {
      this.clamped++;
      return (this.data.radius = 12);
    }
    return (this.data.radius = r);
  }
  setMotion(m: number): number {
    if (!Number.isFinite(m) || m < 0 || m > 5) {
      this.clamped++;
      return (this.data.motion = 3);
    }
    return (this.data.motion = Math.round(m));
  }
  setMaterial(m: string): Material {
    if (!(MATERIALS as readonly string[]).includes(m)) {
      this.clamped++;
      return (this.data.material = 'mica');
    }
    return (this.data.material = m as Material);
  }
  snapshot(): string {
    return JSON.stringify({ v: this.version, ...this.data });
  }
  restore(raw: string): boolean {
    try {
      const o = JSON.parse(raw) as Partial<Hexad> & { v?: number };
      if (typeof o.wallpaper === 'string') this.data.wallpaper = o.wallpaper;
      if (typeof o.palette === 'string') this.data.palette = o.palette;
      if (typeof o.density === 'string') this.setDensity(o.density);
      if (o.radius !== undefined) this.setRadius(Number(o.radius));
      if (o.motion !== undefined) this.setMotion(Number(o.motion));
      if (typeof o.material === 'string') this.setMaterial(o.material);
      return true;
    } catch {
      this.clamped++;
      return false;
    }
  }
  /** 跨版本迁移：v1 六元组直接携带；非法档位由 set* 钳制兜底。 */
  static migrate(raw: string): ThemeHexad {
    const t = new ThemeHexad();
    t.restore(raw);
    return t;
  }
  static label(m: Material): string {
    return { solid: '纯色', frosted: '磨砂', acrylic: '亚克力', mica: '云母', glow: '辉光' }[m];
  }
}

/* -------- 族0543 键位注册表与冲突检测（X13551~X13575 · §17）-------- */

/** §17.2 系统界面全局键位（8 条，须过冲突检测）。 */
export const GLOBAL_KEYS: ReadonlyArray<[string, string]> = [
  ['Win / Ctrl+Esc', '开始菜单'],
  ['Win+A', '快捷面板'],
  ['Win+N', '通知中心'],
  ['Win+E', '文件管理器'],
  ['Win+I', '设置'],
  ['Win+Tab', '任务视图'],
  ['Alt+Space', '窗口系统菜单'],
  ['F1', '命令面板'],
];

export const MOD_ORDER = ['Ctrl', 'Alt', 'Shift', 'Win'] as const;

/** 组合键规范化：修饰键按 Ctrl+Alt+Shift+Win 排序，主键原样。 */
export function normalizeCombo(combo: string): string {
  const parts = combo.split('+').map((s) => s.trim()).filter(Boolean);
  if (parts.length <= 1) return parts.join('+');
  const mods = parts.slice(0, -1);
  const key = parts[parts.length - 1];
  mods.sort((a, b) => MOD_ORDER.indexOf(a as (typeof MOD_ORDER)[number]) - MOD_ORDER.indexOf(b as (typeof MOD_ORDER)[number]));
  return `${mods.join('+')}+${key}`;
}

export interface KeyBinding {
  id: string;
  combo: string;
  scope: 'workbench' | 'system';
}

/** 键位注册表：登记去重 + 冲突检测（同规范化组合即冲突）。 */
export class KeymapRegistry {
  bindings: KeyBinding[] = [];
  conflicts: string[] = [];
  clamped = 0;
  /** 登记自带去重：同 id 拒绝；组合规范化后冲突即记入 conflicts。 */
  register(id: string, combo: string, scope: KeyBinding['scope'] = 'workbench'): boolean {
    if (!id || !combo || this.bindings.some((b) => b.id === id)) {
      this.clamped++;
      return false;
    }
    const norm = normalizeCombo(combo);
    const hit = this.bindings.find((b) => b.combo === norm);
    if (hit) this.conflicts.push(`${hit.id}~${id}@${norm}`);
    this.bindings.push({ id, combo: norm, scope });
    return true;
  }
  unregister(id: string): boolean {
    const i = this.bindings.findIndex((b) => b.id === id);
    if (i < 0) return false;
    const norm = this.bindings[i]!.combo;
    this.bindings.splice(i, 1);
    this.conflicts = this.conflicts.filter((c) => !c.endsWith(`@${norm}`));
    return true;
  }
  byCombo(combo: string): KeyBinding[] {
    const norm = normalizeCombo(combo);
    return this.bindings.filter((b) => b.combo === norm);
  }
  /** 全局键位全量注册并返回冲突数（§17.2 验收：冲突 0）。 */
  registerGlobals(): number {
    const before = this.conflicts.length;
    for (const [combo] of GLOBAL_KEYS) {
      const id = `sys:${GLOBAL_KEYS.find((g) => g[0] === combo)![1]}`;
      if (!this.bindings.some((b) => b.id === id)) this.register(id, combo, 'system');
    }
    return this.conflicts.length - before;
  }
  snapshot(): string {
    return JSON.stringify({ bindings: this.bindings, conflicts: this.conflicts });
  }
}

/* -------- 族0546~0548 视觉稿三套（X13626~X13700 · §7.1/§7.2）-------- */

export const MOCKUP_SCREENS = ['桌面+图标', '任务栏+开始菜单', '设置', '文件管理器', '代码分析工作台'] as const;
export type MockupScreen = (typeof MOCKUP_SCREENS)[number];

export interface MockupStyle {
  material: string;
  density: 'comfortable' | 'compact';
  radius: number;
  curve: string;
  accentFollowsWallpaper: boolean;
}

/** §7.1 三套变体方向的关键决策。 */
export const MOCKUP_STYLES: Record<'v1' | 'v2' | 'v3', MockupStyle> = {
  v1: { material: 'm-mica', density: 'comfortable', radius: 16, curve: '--ease-standard', accentFollowsWallpaper: true },
  v2: { material: 'm-solid', density: 'compact', radius: 8, curve: '--ease-standard', accentFollowsWallpaper: false },
  v3: { material: 'm-glow', density: 'comfortable', radius: 12, curve: '--ease-emphasized', accentFollowsWallpaper: true },
};

export type ScreenStage = 'blank' | 'drafted' | 'verified';

/** 视觉稿：五屏状态机（blank→drafted→verified）+ 选稿锁定。 */
export class MockupSpec {
  readonly variant: 'v1' | 'v2' | 'v3';
  stages = new Map<MockupScreen, ScreenStage>();
  locked = false;
  clamped = 0;
  constructor(variant: 'v1' | 'v2' | 'v3') {
    this.variant = variant;
    for (const s of MOCKUP_SCREENS) this.stages.set(s, 'blank');
  }
  get style(): MockupStyle {
    return MOCKUP_STYLES[this.variant];
  }
  /** 草拟一屏：锁定后拒绝。 */
  draft(screen: MockupScreen): boolean {
    if (this.locked || this.stages.get(screen) !== 'blank') {
      this.clamped++;
      return false;
    }
    this.stages.set(screen, 'drafted');
    return true;
  }
  verify(screen: MockupScreen): boolean {
    if (this.stages.get(screen) !== 'drafted') {
      this.clamped++;
      return false;
    }
    this.stages.set(screen, 'verified');
    return true;
  }
  /** 选稿签字：五屏全 verified 才允许锁定。 */
  lock(): boolean {
    if (![...this.stages.values()].every((s) => s === 'verified')) {
      this.clamped++;
      return false;
    }
    this.locked = true;
    return true;
  }
  progress(): number {
    const v = [...this.stages.values()].filter((s) => s === 'verified').length;
    return Math.round((v / MOCKUP_SCREENS.length) * 100);
  }
  static variantLabel(v: 'v1' | 'v2' | 'v3'): string {
    return { v1: '晨雾', v2: '工作台', v3: '极光' }[v];
  }
}

/* -------- 族0550 空态与骨架体系（X13726~X13750 · §13 #21/EmptyState）-------- */

export const SKELETON_SHAPES = ['text', 'rect', 'circle'] as const;
export type SkeletonShape = (typeof SKELETON_SHAPES)[number];
export const SHIMMER_MS = 1400;
export const LOAD_DELAY_MS = 300;

export interface SkeletonLine {
  shape: SkeletonShape;
  w: string;
}

export interface EmptyStateDef {
  key: string;
  title: string;
  hint: string;
  action: string;
}

/** 空态与骨架体系：骨架镜像布局 + 加载分档（>300ms 才现）+ 空态登记去重。 */
export class SkeletonEmpty {
  empties: EmptyStateDef[] = [];
  clamped = 0;
  /** 加载分档：超过 LOAD_DELAY_MS 才显示骨架，否则直接渲染内容。 */
  static shouldSkeleton(elapsedMs: number): boolean {
    return Number.isFinite(elapsedMs) && elapsedMs > LOAD_DELAY_MS;
  }
  /** 骨架镜像布局：按行数生成 text/rect 交替序列。 */
  static layout(lines: number): SkeletonLine[] {
    const n = Math.max(0, Math.min(8, Math.round(Number(lines) || 0)));
    return Array.from({ length: n }, (_, i) => ({
      shape: (i % 2 === 0 ? 'text' : 'rect') as SkeletonShape,
      w: i === n - 1 ? '60%' : '100%',
    }));
  }
  /** 交叉溶解换场：骨架→内容禁跳变，reduce-motion 降级为纯淡入淡出。 */
  static crossfade(reduceMotion: boolean): { duration: number; mode: string } {
    return reduceMotion ? { duration: 80, mode: 'fade' } : { duration: 170, mode: 'crossfade' };
  }
  /** 空态登记：同 key 拒绝（自带去重）。 */
  add(key: string, title: string, hint: string, action: string): boolean {
    if (!key || !title || this.empties.some((e) => e.key === key)) {
      this.clamped++;
      return false;
    }
    this.empties.push({ key, title, hint, action });
    return true;
  }
  of(key: string): EmptyStateDef | undefined {
    return this.empties.find((e) => e.key === key);
  }
  /** 骨架行数与空态共存纪律：骨架期间不渲染空态。 */
  static coexist(lines: number, elapsedMs: number): 'skeleton' | 'empty' | 'content' {
    if (SkeletonEmpty.shouldSkeleton(elapsedMs)) return 'skeleton';
    return lines === 0 ? 'empty' : 'content';
  }
}
