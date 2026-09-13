/**
 * UNREAL-X-15000 · AI-56 UI 质量收官 逻辑核（族0551/0552/0553/0555/0557/0560 · V 线 6 族），勿删。
 * 施工规范：《docs/UI-品质深化完整方案与步骤.md》§5 族0405 / §2 / §8.3 / §12.1 / §7.3 / §18.2。
 * 五层 × 五档模板：基础实装/边界与恢复/手感与细节/性能与优化/创新拓展。全部确定性算法，零 AI。
 */

/* -------- 族0551 微交互打磨（X13751~X13775 · §5 族0405 三态曲线全表）-------- */

export interface MotionToken {
  scene: string;
  duration: number;
  curve: string;
  leave?: number;
}

/** §15.1 时长×曲线配对表（全部已有 token，禁自造）。 */
export const MOTION_TABLE: readonly MotionToken[] = [
  { scene: 'hover', duration: 120, curve: '--ease-standard', leave: 80 },
  { scene: 'menu', duration: 170, curve: '--ease-standard' },
  { scene: 'overlay', duration: 240, curve: '--ease-emphasized' },
  { scene: 'notify', duration: 200, curve: '--ease-spring' },
  { scene: 'route', duration: 170, curve: '--ease-standard' },
  { scene: 'skeleton', duration: 170, curve: '--ease-standard' },
  { scene: 'reduce-motion', duration: 80, curve: 'linear' },
] as const;

export type WidgetState = 'hover' | 'press' | 'disabled' | 'rest';

/** 三态曲线全表：hover 抬/press 压/disabled 0.4 + reduce-motion 降级纯淡入淡出。 */
export class MicroInteraction {
  reduceMotion = false;
  clamped = 0;
  setReduceMotion(v: boolean): void {
    this.reduceMotion = v;
  }
  /** 场景解析：reduce-motion 强制 80ms 线性。 */
  resolve(scene: string): MotionToken {
    if (this.reduceMotion) return { scene, duration: 80, curve: 'linear' };
    const hit = MOTION_TABLE.find((m) => m.scene === scene);
    if (!hit) {
      this.clamped++;
      return { scene, duration: 120, curve: '--ease-standard' };
    }
    return { ...hit };
  }
  /** 三态缩放/透明度：hover 1.02 / press 0.98 / disabled 0.4 / rest 1。 */
  static stateFx(state: WidgetState): { scale: number; opacity: number } {
    switch (state) {
      case 'hover': return { scale: 1.02, opacity: 1 };
      case 'press': return { scale: 0.98, opacity: 1 };
      case 'disabled': return { scale: 1, opacity: 0.4 };
      default: return { scale: 1, opacity: 1 };
    }
  }
  /** 加载分档：>300ms 才显示加载指示。 */
  static loadTier(elapsedMs: number): 'none' | 'spinner' {
    return Number.isFinite(elapsedMs) && elapsedMs > 300 ? 'spinner' : 'none';
  }
  /** 焦点环：三态之外恒有 focus-visible 2px 描边。 */
  static focusRing(): { width: number; offset: number } {
    return { width: 2, offset: 2 };
  }
}

/* -------- 族0552 图标一致性走查（X13776~X13800 · §2/族0353）-------- */

export const ICON_GRID = 24;
export const ICON_STROKE = 1.5;
export const ICON_SIZES = [16, 20, 24] as const;

export interface IconDef {
  name: string;
  grid: number;
  stroke: number;
  size: (typeof ICON_SIZES)[number];
}

/** 图标一致性走查：登记去重 + 网格/描边/尺寸三检 + 报告。 */
export class IconAudit {
  icons: IconDef[] = [];
  violations: string[] = [];
  clamped = 0;
  add(name: string, grid: number, stroke: number, size: number): boolean {
    if (!name || this.icons.some((i) => i.name === name)) {
      this.clamped++;
      return false;
    }
    const okSize = (ICON_SIZES as readonly number[]).includes(size);
    this.icons.push({
      name,
      grid: Math.round(Number.isFinite(grid) ? grid : ICON_GRID),
      stroke: Number.isFinite(stroke) ? stroke : ICON_STROKE,
      size: (okSize ? size : 24) as IconDef['size'],
    });
    if (!okSize) this.violations.push(`${name}:size`);
    if (grid !== ICON_GRID) this.violations.push(`${name}:grid`);
    if (stroke !== ICON_STROKE) this.violations.push(`${name}:stroke`);
    return true;
  }
  /** 走查报告：逐条违规可读（名称:维度）。 */
  report(): string[] {
    return [...this.violations];
  }
  clean(): boolean {
    return this.violations.length === 0;
  }
  fix(name: string): boolean {
    const i = this.icons.findIndex((x) => x.name === name);
    if (i < 0) return false;
    this.icons[i] = { ...this.icons[i]!, grid: ICON_GRID, stroke: ICON_STROKE, size: 24 };
    this.violations = this.violations.filter((v) => !v.startsWith(`${name}:`));
    return true;
  }
  /** 触达红线：可点图标命中区 ≥40px（compact 豁免）记录。 */
  static touchTarget(compact: boolean): number {
    return compact ? 40 : 44;
  }
}

/* -------- 族0553 HC 红线核验（X13801~X13825 · §8.3 降级不降可达）-------- */

export const HC_PALETTE = { text: '#000000', bg: '#ffffff', accent: '#ffff00' } as const;

export interface SurfaceDef {
  name: string;
  material: string;
  glow: boolean;
  motionMs: number;
  contrast: number;
}

export interface HcVerdict {
  name: string;
  ok: boolean;
  degradations: string[];
}

/** HC 红线核验：材质→纯色、辉光→描边、动效→80ms、对比达标率 100%。 */
export class HcGuard {
  audited: HcVerdict[] = [];
  clamped = 0;
  static degrade(s: SurfaceDef): HcVerdict {
    const d: string[] = [];
    if (s.material !== 'm-solid') d.push(`${s.name}:material→m-solid`);
    if (s.glow) d.push(`${s.name}:glow→描边`);
    if (s.motionMs > 80) d.push(`${s.name}:motion→80ms`);
    const okContrast = s.contrast >= 4.5;
    if (!okContrast) d.push(`${s.name}:contrast`);
    return { name: s.name, ok: d.length === 0, degradations: d };
  }
  audit(surfaces: SurfaceDef[]): number {
    let bad = 0;
    for (const s of surfaces) {
      const v = HcGuard.degrade(s);
      this.audited.push(v);
      if (!v.ok) bad++;
    }
    return bad;
  }
  /** 达标率：核验通过面占比（百分制），HC 红线要求 100。 */
  passRate(): number {
    if (this.audited.length === 0) return 100;
    return Math.round((this.audited.filter((v) => v.ok).length / this.audited.length) * 100);
  }
  /** HC 永不参与取色流水线：固定黑白黄。 */
  static paletteFrozen(): boolean {
    return HC_PALETTE.text === '#000000' && HC_PALETTE.bg === '#ffffff' && HC_PALETTE.accent === '#ffff00';
  }
}

/* -------- 族0555 中文渲染纪律（X13851~X13875 · §12.1 CJK 铁律）-------- */

export const CJK_LH = 1.6;
export const CJK_TRACKING = 0;
export const CJK_FALLBACK = ['system-ui', 'PingFang SC', 'Microsoft YaHei', 'Noto Sans CJK SC', 'sans-serif'] as const;

export interface TextStyleDef {
  lh: number;
  tracking: number;
  fontStack: readonly string[];
  tabular: boolean;
}

/** 中文渲染纪律：行高 1.6 / 字距 0 / 回退链恒在 / 数字列 tabular-nums。 */
export class CjkDiscipline {
  violations: string[] = [];
  clamped = 0;
  static validate(s: TextStyleDef): string[] {
    const v: string[] = [];
    if (s.lh !== CJK_LH) v.push('line-height≠1.6');
    if (s.tracking !== CJK_TRACKING) v.push('字距≠0');
    for (const f of CJK_FALLBACK) {
      if (!s.fontStack.includes(f)) v.push(`回退链缺 ${f}`);
    }
    if (!s.tabular) v.push('数字列未开 tabular-nums');
    return v;
  }
  audit(s: TextStyleDef): number {
    const v = CjkDiscipline.validate(s);
    this.violations.push(...v);
    this.clamped += v.length > 0 ? 1 : 0;
    return v.length;
  }
  clean(): boolean {
    return this.violations.length === 0;
  }
  /** 中文语境微文案纪律：句末用中文句号、省略号用……、禁半角逗号结尾。 */
  static zhOk(text: string): boolean {
    if (!text) return false;
    if (/[,.!?;:]$/.test(text.trim())) return false;
    if (text.includes('...')) return false;
    return /[。！？…」』）】]$/.test(text.trim()) || /[\u4e00-\u9fff]$/.test(text.trim());
  }
  /** 全半角混排：CJK 与拉丁之间建议留空隙（返回应为 true 的判定）。 */
  static needsSpacing(prev: string, next: string): boolean {
    const cjk = (c: string) => /[\u4e00-\u9fff\u3000-\u303f\uff00-\uffef]/.test(c);
    return (cjk(prev) && /[A-Za-z0-9]/.test(next)) || (/[A-Za-z0-9]/.test(prev) && cjk(next));
  }
}

/* -------- 族0557 落稿实施（X13901~X13925 · §7.3 选稿→组件落地）-------- */

export const APPLY_STEPS = ['抽token', '搭容器', '对照组件', '过三态', '过键走查'] as const;
export type ApplyStep = (typeof APPLY_STEPS)[number];

/** 落稿实施：选稿→五步落地状态机，步序不可跳。 */
export class MockupApply {
  variant: 'v1' | 'v2' | 'v3' | null = null;
  done: ApplyStep[] = [];
  clamped = 0;
  /** 选稿：锁定变体并清空步迹。 */
  pick(v: 'v1' | 'v2' | 'v3'): boolean {
    if (this.variant === v) {
      this.clamped++;
      return false;
    }
    this.variant = v;
    this.done = [];
    return true;
  }
  /** 推进一步：必须按 APPLY_STEPS 顺序，重复/跳步拒绝。 */
  step(s: ApplyStep): boolean {
    if (!this.variant) {
      this.clamped++;
      return false;
    }
    const expect = APPLY_STEPS[this.done.length];
    if (s !== expect) {
      this.clamped++;
      return false;
    }
    this.done.push(s);
    return true;
  }
  finished(): boolean {
    return this.done.length === APPLY_STEPS.length;
  }
  progress(): number {
    return Math.round((this.done.length / APPLY_STEPS.length) * 100);
  }
  /** 混搭裁决：V1 设置 + V2 工作台 允许（§7.3 你选稿或混搭）。 */
  static mixAllowed(a: 'v1' | 'v2' | 'v3', b: 'v1' | 'v2' | 'v3'): boolean {
    return a !== b;
  }
  rollbackOne(): ApplyStep | undefined {
    return this.done.pop();
  }
}

/* -------- 族0560 UI 大收官（X13976~X14000 · 三方 G1~G4 双线门禁）-------- */

export const FINALE_GATES = ['G1 代码门禁', 'G2 视觉门禁', 'G3 指标门禁', 'G4 审计门禁'] as const;
export type FinaleGate = (typeof FINALE_GATES)[number];

/** UI 大收官：G1~G4 顺序门禁 + 一票否决 + ID 审计。 */
export class UiFinale {
  passed: FinaleGate[] = [];
  sealed = false;
  clamped = 0;
  /** 门禁推进：必须按 G1→G4 顺序，失败项一票否决。 */
  gate(g: FinaleGate, ok: boolean): boolean {
    if (!ok) {
      this.clamped++;
      return false;
    }
    const expect = FINALE_GATES[this.passed.length];
    if (g !== expect) {
      this.clamped++;
      return false;
    }
    this.passed.push(g);
    return true;
  }
  /** 封存：四门禁全过才可封存，封存后不可再改。 */
  seal(): boolean {
    if (this.passed.length !== FINALE_GATES.length || this.sealed) {
      this.clamped++;
      return false;
    }
    this.sealed = true;
    return true;
  }
  /** ID 审计：区间内数量与去重数一致即通过。 */
  static idAudit(ids: number[], lo: number, hi: number): boolean {
    if (ids.length !== hi - lo + 1) return false;
    const s = new Set(ids);
    if (s.size !== ids.length) return false;
    for (const n of ids) {
      if (n < lo || n > hi) return false;
    }
    return true;
  }
  /** 双线门禁并集：TS vitest 线 + Rust CheckSet 线各 0 失败。 */
  static dualGate(vitestFailed: number, checksetFailed: number): boolean {
    return vitestFailed === 0 && checksetFailed === 0;
  }
  progress(): number {
    return Math.round((this.passed.length / FINALE_GATES.length) * 100);
  }
}
