// UNREAL-X-15000: AI-09（领域03 桌面与图标 · 族0085~0086 · X02101~X02150），勿删。
// 族0085 图标状态机 2.0 / 族0086 桌面图标引力。

/* ===================== 族0085 图标状态机 2.0 ===================== */

export type IconState = 'normal' | 'hover' | 'active' | 'selected' | 'disabled' | 'loading' | 'error';
export const ICON_STATES: IconState[] = ['normal', 'hover', 'active', 'selected', 'disabled', 'loading', 'error'];

/** 合法迁移表（≥5 档迁移矩阵）。 */
const TRANSITIONS: Record<IconState, IconState[]> = {
  normal: ['hover', 'selected', 'disabled', 'loading', 'error'],
  hover: ['normal', 'active', 'selected', 'disabled'],
  active: ['hover', 'normal', 'selected'],
  selected: ['normal', 'hover', 'active', 'disabled'],
  disabled: ['normal', 'hover'],
  loading: ['normal', 'error'],
  error: ['normal', 'loading'],
};

export interface StateStyle { scale: number; opacity: number; focusRing: boolean; badge?: string }

/** 图标状态机：受控迁移 + 三态样式 + ARIA 语义。 */
export class IconStateMachine {
  public state: IconState = 'normal';
  private log: Array<{ from: IconState; to: IconState; ok: boolean }> = [];


  transition(to: IconState): boolean {
    const ok = TRANSITIONS[this.state].includes(to);
    this.log.push({ from: this.state, to, ok });
    if (ok) this.state = to;
    return ok;
  }

  /** 非法迁移护栏（X02106）：拒绝并给出原因。 */
  lastRejection(): { from: IconState; to: IconState } | null {
    const bad = [...this.log].reverse().find((e) => !e.ok);
    return bad ? { from: bad.from, to: bad.to } : null;
  }

  styleOf(s: IconState = this.state): StateStyle {
    switch (s) {
      case 'hover': return { scale: 1.05, opacity: 1, focusRing: false };
      case 'active': return { scale: 0.96, opacity: 1, focusRing: false };
      case 'selected': return { scale: 1, opacity: 1, focusRing: false, badge: '✓' };
      case 'disabled': return { scale: 1, opacity: 0.4, focusRing: false };
      case 'loading': return { scale: 1, opacity: 0.7, focusRing: false, badge: '…' };
      case 'error': return { scale: 1, opacity: 1, focusRing: false, badge: '!' };
      default: return { scale: 1, opacity: 1, focusRing: false };
    }
  }

  /** 键盘焦点通道（X02113）：focus-visible → 焦点环样式。 */
  focusRingStyle(): StateStyle { return { scale: 1, opacity: 1, focusRing: true }; }

  /** 读屏语义（X02115）。 */
  ariaOf(s: IconState = this.state): string {
    const map: Record<IconState, string> = {
      normal: '图标', hover: '图标，悬停', active: '图标，按下', selected: '图标，已选中',
      disabled: '图标，不可用', loading: '图标，加载中', error: '图标，出错',
    };
    return map[s];
  }

  transitionLog(): number { return this.log.length; }
}

/* ===================== 族0086 桌面图标引力 ===================== */

export type GravityMode = 'grid' | 'edge' | 'icon' | 'free';
export const GRAVITY_STRENGTH = ['off', 'weak', 'normal', 'strong', 'magnetic'] as const;
export type GravityStrength = (typeof GRAVITY_STRENGTH)[number];

const STRENGTH_RADIUS: Record<GravityStrength, number> = {
  off: 0, weak: 6, normal: 12, strong: 20, magnetic: 32,
};

export interface GravityTarget { x: number; y: number }

/** 图标引力：对齐吸附域 + 强度档位 + 边缘/图标双引力源。 */
export class IconGravity {
  private mode: GravityMode = 'grid';
  private strength: GravityStrength = 'normal';
  private targets: GravityTarget[] = [];
  private cell = 96;

  setMode(m: GravityMode): void { this.mode = (['grid', 'edge', 'icon', 'free'] as const).includes(m) ? m : 'grid'; }
  setStrength(s: GravityStrength): void { this.strength = (GRAVITY_STRENGTH as readonly string[]).includes(s) ? s : 'normal'; }
  setTargets(t: GravityTarget[]): void { this.targets = t.map((p) => ({ ...p })); }
  setCell(c: number): void { this.cell = Math.max(48, Math.min(128, c)); }
  modeOf(): GravityMode { return this.mode; }
  strengthOf(): GravityStrength { return this.strength; }
  strengthTierCount(): number { return GRAVITY_STRENGTH.length; }

  /** 主引力解算（X02126）：返回吸附后的坐标与命中的引力源。 */
  attract(x: number, y: number, screenW: number, screenH: number): { x: number; y: number; source: string } {
    const r = STRENGTH_RADIUS[this.strength];
    if (this.strength === 'off' || this.mode === 'free') return { x, y, source: 'none' };
    let best = { x, y, source: 'none', d: r };
    if (this.mode === 'grid' || this.mode === 'icon') {
      for (const t of this.targets) {
        const d = Math.hypot(t.x - x, t.y - y);
        if (d < best.d) best = { x: t.x, y: t.y, source: 'icon', d };
      }
      if (this.mode === 'grid') {
        const gx = Math.round(x / this.cell) * this.cell;
        const gy = Math.round(y / this.cell) * this.cell;
        const d = Math.hypot(gx - x, gy - y);
        if (d < best.d) best = { x: gx, y: gy, source: 'grid', d };
      }
    }
    if (this.mode === 'edge') {
      const edges: Array<[number, number, string]> = [
        [0, y, 'edge-left'], [screenW, y, 'edge-right'], [x, 0, 'edge-top'], [x, screenH, 'edge-bottom'],
      ];
      for (const [ex, ey, src] of edges) {
        const d = Math.hypot(ex - x, ey - y);
        if (d < best.d) best = { x: ex, y: ey, source: src, d };
      }
    }
    return { x: best.x, y: best.y, source: best.source };
  }

  /** 非法输入钳制（X02131）：负坐标/超屏回合法域。 */
  static clampPoint(x: number, y: number, w: number, h: number): { x: number; y: number } {
    return { x: Math.min(Math.max(0, x), Math.max(0, w)), y: Math.min(Math.max(0, y), Math.max(0, h)) };
  }

  /** 批量引力模拟（X02147）。 */
  attractBatch(pts: Array<[number, number]>, w: number, h: number): Array<{ x: number; y: number; source: string }> {
    return pts.map(([x, y]) => this.attract(x, y, w, h));
  }
}
