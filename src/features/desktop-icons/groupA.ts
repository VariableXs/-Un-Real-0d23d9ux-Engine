// UNREAL-X-15000: AI-09（领域03 桌面与图标 · 族0081~0082 · X02001~X02050），勿删。
// 族0081 图标风格体系 2.0 / 族0082 图标动效 2.0。

/* ===================== 族0081 图标风格体系 2.0 ===================== */

export const ICON_STYLE_PRESETS = [
  'classic', 'outline', 'filled', 'duotone', 'glass',
] as const;
export type IconStylePreset = (typeof ICON_STYLE_PRESETS)[number];

export interface IconStyleParams {
  preset: IconStylePreset;
  corner: number;      // 0~8 圆角
  strokeWidth: number; // 1~2.5 线宽
  fillMode: 'none' | 'soft' | 'full';
  saturation: number;  // 0~1.4
}

export const ICON_STYLE_DEFAULT: IconStyleParams = {
  preset: 'classic', corner: 4, strokeWidth: 1.5, fillMode: 'soft', saturation: 1,
};

const clamp = (v: number, lo: number, hi: number): number => (v < lo ? lo : v > hi ? hi : v);

/** 图标风格系统：预设矩阵 + 参数面 + 快照迁移 + 降级 + 回滚。 */
export class IconStyleSystem {
  private current: IconStyleParams = { ...ICON_STYLE_DEFAULT };
  private remembered: IconStyleParams | null = null;
  private history: IconStyleParams[] = [];
  private invalidLog: Array<{ bad: Partial<IconStyleParams>; reason: string }> = [];
  private degradation = 0; // 0 正常 / 1 材质降 / 2 动效降 / 3 全降

  /** 钳制：非法输入回默认（X02006 边界护栏）。 */
  static sanitize(p: Partial<IconStyleParams>): IconStyleParams {
    const out: IconStyleParams = { ...ICON_STYLE_DEFAULT };
    if (p.preset && (ICON_STYLE_PRESETS as readonly string[]).includes(p.preset)) out.preset = p.preset;
    if (typeof p.corner === 'number' && Number.isFinite(p.corner)) out.corner = clamp(p.corner, 0, 8);
    if (typeof p.strokeWidth === 'number' && Number.isFinite(p.strokeWidth)) out.strokeWidth = clamp(p.strokeWidth, 1, 2.5);
    if (p.fillMode === 'none' || p.fillMode === 'soft' || p.fillMode === 'full') out.fillMode = p.fillMode;
    if (typeof p.saturation === 'number' && Number.isFinite(p.saturation)) out.saturation = clamp(p.saturation, 0, 1.4);
    return out;
  }

  apply(p: Partial<IconStyleParams>): IconStyleParams {
    const next = IconStyleSystem.sanitize(p);
    const changed = next.preset !== this.current.preset || next.corner !== this.current.corner ||
      next.strokeWidth !== this.current.strokeWidth || next.fillMode !== this.current.fillMode ||
      next.saturation !== this.current.saturation;
    if (changed) {
      this.history.push({ ...this.current });
      this.current = next;
    }
    return { ...this.current };
  }

  /** 非法输入记录：可读原因（X02007）。 */
  reject(bad: Partial<IconStyleParams>): { recovered: IconStyleParams; reason: string } {
    const hasBad = ('preset' in bad && !(ICON_STYLE_PRESETS as readonly string[]).includes(String(bad.preset)));
    const reason = hasBad ? `未知风格预设 ${String(bad.preset)}，已回退默认档` : '参数越界，已钳制回合法区间';
    this.invalidLog.push({ bad, reason });
    this.history.push({ ...this.current });
    this.current = { ...ICON_STYLE_DEFAULT };
    return { recovered: { ...this.current }, reason };
  }

  rejectLog(): Array<{ bad: Partial<IconStyleParams>; reason: string }> { return [...this.invalidLog]; }

  /** 断点续作：半成品标记 + 一键还原（X02008）。 */
  markPartial(): void { this.remembered = { ...this.current }; }
  resume(): IconStyleParams | null {
    if (!this.remembered) return null;
    this.current = this.remembered;
    this.remembered = null;
    return { ...this.current };
  }

  /** 快照导出/导入（X02004）。 */
  exportSnapshot(): string { return JSON.stringify(this.current); }
  importSnapshot(snap: string): IconStyleParams {
    let p: Partial<IconStyleParams> = {};
    try { p = JSON.parse(snap) as Partial<IconStyleParams>; } catch { p = {}; }
    this.current = IconStyleSystem.sanitize(p);
    return { ...this.current };
  }

  /** 回滚净身（X02010）：清历史并回默认，不留残档。 */
  rollback(): IconStyleParams {
    this.history = [];
    this.invalidLog = [];
    this.current = { ...ICON_STYLE_DEFAULT };
    return { ...this.current };
  }

  /** 低配降级链（X02019）：玻璃→软填充→纯线框 三级递降。 */
  degrade(level: 1 | 2 | 3): IconStyleParams {
    this.degradation = level;
    if (level >= 1 && this.current.preset === 'glass') this.current.preset = 'classic';
    if (level >= 2) this.current.fillMode = 'none';
    if (level >= 3) this.current.saturation = 0;
    return { ...this.current };
  }
  degradationLevel(): number { return this.degradation; }

  /** 批量应用（X02022）：按预设批量生成参数表。 */
  static batchFor(presets: IconStylePreset[]): IconStyleParams[] {
    return presets.map((preset) => IconStyleSystem.sanitize({ preset }));
  }

  /** 启发式建议（X02021）：依据壁纸亮度给可解释建议，可拒绝。 */
  static suggest(wallpaperLuma: number): { preset: IconStylePreset; reason: string } {
    return wallpaperLuma > 0.6
      ? { preset: 'outline', reason: '壁纸偏亮，线框风格对比更清晰' }
      : { preset: 'filled', reason: '壁纸偏暗，填充风格更易识别' };
  }

  get params(): IconStyleParams { return { ...this.current }; }
}

/* ===================== 族0082 图标动效 2.0 ===================== */

export interface MotionToken { dur: number; ease: 'standard' | 'emphasized' | 'spring' | 'linear'; scale: number }
export const MOTION_TIERS = ['off', 'subtle', 'standard', 'playful', 'emphatic'] as const;
export type MotionTier = (typeof MOTION_TIERS)[number];

const TIER_TOKEN: Record<MotionTier, MotionToken> = {
  off: { dur: 0, ease: 'linear', scale: 1 },
  subtle: { dur: 80, ease: 'standard', scale: 1.02 },
  standard: { dur: 120, ease: 'standard', scale: 1.05 },
  playful: { dur: 170, ease: 'spring', scale: 1.1 },
  emphatic: { dur: 240, ease: 'emphasized', scale: 1.16 },
};

/** 图标动效系统：档位矩阵 + reduce-motion 降级 + 低配降级 + 三态。 */
export class IconMotionSystem {
  private tier: MotionTier = 'standard';
  private reduceMotion = false;
  private perfLevel = 0;
  private frames: Array<{ phase: 'hover' | 'press' | 'release'; dur: number; ease: string }> = [];

  setTier(t: MotionTier): void {
    if ((MOTION_TIERS as readonly string[]).includes(t)) this.tier = t; // 非法档保持当前档
  }
  get token(): MotionToken {
    if (this.reduceMotion) return { dur: 80, ease: 'linear', scale: 1 };
    const base = TIER_TOKEN[this.tier];
    if (this.perfLevel >= 2) return { dur: Math.max(0, base.dur - 80), ease: 'linear', scale: 1 };
    return { ...base };
  }
  setReduceMotion(on: boolean): void { this.reduceMotion = on; }
  setPerfLevel(level: number): void { this.perfLevel = Math.max(0, Math.min(3, level)); }
  tierOf(): MotionTier { return this.tier; }
  tierCount(): number { return MOTION_TIERS.length; }

  /** 三态编排（X02037）：hover 抬 / press 压 / release 回弹。 */
  phase(phase: 'hover' | 'press' | 'release'): { phase: string; dur: number; ease: string } {
    const t = this.token;
    const row = phase === 'hover'
      ? { phase, dur: t.dur, ease: t.ease }
      : phase === 'press'
        ? { phase, dur: Math.max(0, t.dur - 40), ease: 'standard' }
        : { phase, dur: t.dur, ease: 'spring' };
    this.frames.push(row);
    return row;
  }
  frameLog(): number { return this.frames.length; }

  /** 批量动画队列（X02047）：进度可观测。 */
  static batch(queue: number): Array<{ i: number; done: boolean }> {
    return Array.from({ length: queue }, (_, i) => ({ i, done: i < queue - 1 }));
  }
}
