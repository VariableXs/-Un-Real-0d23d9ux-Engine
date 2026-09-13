// UNREAL-X-15000: AI-10（领域03 桌面与图标 · 族0099~0100 · X02451~X02500），勿删。
// 族0099 锁屏一体化 2.0 / 族0100 桌面时空感。

/* ===================== 族0099 锁屏一体化 2.0 ===================== */

export interface LockscreenConfig { wallpaperId: string; blur: number; dim: number; widgets: string[]; clockVisible: boolean }

/** 锁屏一体化：与壁纸联动 + 模糊/压暗档 + 锁屏微件。 */
export class LockscreenIntegration {
  private config: LockscreenConfig = { wallpaperId: 'default', blur: 12, dim: 0.35, widgets: ['clock'], clockVisible: true };
  private locked = false;
  private events: Array<{ at: number; type: 'lock' | 'unlock' }> = [];

  syncWallpaper(id: string): void { if (id) this.config.wallpaperId = id; }
  configOf(): LockscreenConfig { return { ...this.config, widgets: [...this.config.widgets] }; }

  setBlur(level: 1 | 2 | 3 | 4 | 5): void { this.config.blur = [0, 6, 12, 20, 28][level] ?? 12; }
  setDim(level: 1 | 2 | 3 | 4 | 5): void { this.config.dim = [0, 0.15, 0.35, 0.5, 0.7][level] ?? 0.35; }

  /** 锁屏微件登记去重（X02471）。 */
  addWidget(id: string): 'new' | 'dup' {
    if (this.config.widgets.includes(id)) return 'dup';
    this.config.widgets.push(id);
    return 'new';
  }
  removeWidget(id: string): boolean {
    const i = this.config.widgets.indexOf(id);
    if (i < 0) return false;
    this.config.widgets.splice(i, 1);
    return true;
  }

  lock(at: number): void { this.locked = true; this.events.push({ at, type: 'lock' }); }
  unlock(at: number): void { this.locked = false; this.events.push({ at, type: 'unlock' }); }
  isLocked(): boolean { return this.locked; }
  eventLog(): number { return this.events.length; }

  /** 非法输入护栏（X02476）。 */
  static sanitize(c: Partial<LockscreenConfig>): LockscreenConfig {
    return {
      wallpaperId: typeof c.wallpaperId === 'string' && c.wallpaperId ? c.wallpaperId : 'default',
      blur: typeof c.blur === 'number' && Number.isFinite(c.blur) ? Math.min(28, Math.max(0, c.blur)) : 12,
      dim: typeof c.dim === 'number' && Number.isFinite(c.dim) ? Math.min(0.7, Math.max(0, c.dim)) : 0.35,
      widgets: Array.isArray(c.widgets) ? c.widgets.filter((w): w is string => typeof w === 'string') : ['clock'],
      clockVisible: c.clockVisible !== false,
    };
  }
}

/* ===================== 族0100 桌面时空感 ===================== */

export const TIME_PHASES = ['dawn', 'morning', 'noon', 'afternoon', 'dusk', 'night'] as const;
export type TimePhase = (typeof TIME_PHASES)[number];

/** 时空感：时辰→氛围映射 + 季节调色 + 夜间色温。 */
export class DeskTimeAmbience {
  private seasonal = true;
  private nightShift = false;

  setSeasonal(on: boolean): void { this.seasonal = on; }
  setNightShift(on: boolean): void { this.nightShift = on; }

  /** 小时 → 时辰档（X02476 核心链路，6 档）。 */
  static phaseOf(hour: number): TimePhase {
    const h = ((Math.round(hour) % 24) + 24) % 24;
    if (h < 6) return 'night';
    if (h < 9) return 'dawn';
    if (h < 12) return 'morning';
    if (h < 15) return 'noon';
    if (h < 19) return 'afternoon';
    if (h < 22) return 'dusk';
    return 'night';
  }

  /** 时辰 → 色温 K（X02477）。 */
  static kelvinOf(phase: TimePhase): number {
    const map: Record<TimePhase, number> = {
      dawn: 3400, morning: 5000, noon: 6200, afternoon: 5400, dusk: 3000, night: 2200,
    };
    return map[phase];
  }

  /** 季节偏移（X02478）：春夏秋冬色相偏移。 */
  seasonalHue(baseHue: number, season: 'spring' | 'summer' | 'autumn' | 'winter'): number {
    if (!this.seasonal) return baseHue;
    const shift = { spring: -20, summer: 10, autumn: 35, winter: -45 }[season];
    return ((baseHue + shift) % 360 + 360) % 360;
  }

  /** 夜间模式（X02479）：色温压暗到 1800K。 */
  effectiveKelvin(hour: number): number {
    const phase = DeskTimeAmbience.phaseOf(hour);
    const k = DeskTimeAmbience.kelvinOf(phase);
    return this.nightShift && (phase === 'night' || phase === 'dusk') ? Math.min(k, 1800) : k;
  }

  /** 全天编排（X02497 批处理口径）。 */
  static dayPlan(): Array<{ hour: number; phase: TimePhase; kelvin: number }> {
    return Array.from({ length: 24 }, (_, hour) => {
      const phase = DeskTimeAmbience.phaseOf(hour);
      return { hour, phase, kelvin: DeskTimeAmbience.kelvinOf(phase) };
    });
  }
}
