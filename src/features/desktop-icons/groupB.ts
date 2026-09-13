// UNREAL-X-15000: AI-09（领域03 桌面与图标 · 族0083~0084 · X02051~X02100），勿删。
// 族0083 图标栅格密度 2.0 / 族0084 图标语义色 2.0。

/* ===================== 族0083 图标栅格密度 2.0 ===================== */

export interface GridSpec { cell: number; gap: number; icon: number; label: boolean }
export const GRID_DENSITY: Record<string, GridSpec> = {
  compact: { cell: 72, gap: 8, icon: 40, label: true },
  snug: { cell: 84, gap: 10, icon: 44, label: true },
  standard: { cell: 96, gap: 12, icon: 48, label: true },
  relaxed: { cell: 112, gap: 16, icon: 56, label: true },
  airy: { cell: 128, gap: 20, icon: 64, label: true },
};
export type GridDensity = keyof typeof GRID_DENSITY;

/** 栅格系统：五档密度 + 吸附 + 边界钳制 + 持久化。 */
export class IconGrid {
  private density: GridDensity = 'standard';
  private snap = true;

  setDensity(d: GridDensity): GridSpec {
    this.density = (d in GRID_DENSITY) ? d : 'standard';
    return { ...GRID_DENSITY[this.density]! };
  }
  spec(): GridSpec { return { ...GRID_DENSITY[this.density]! }; }
  densityOf(): GridDensity { return this.density; }
  tierCount(): number { return Object.keys(GRID_DENSITY).length; }
  setSnap(on: boolean): void { this.snap = on; }

  /** 吸附到栅格（X02051 核心链路）；snap 关闭时原样返回。 */
  snapPoint(x: number, y: number): { x: number; y: number; snapped: boolean } {
    const s = this.spec();
    if (!this.snap) return { x, y, snapped: false };
    return { x: Math.round(x / s.cell) * s.cell, y: Math.round(y / s.cell) * s.cell, snapped: true };
  }

  /** 屏幕边界钳制（X02056）。 */
  clampToScreen(x: number, y: number, screenW: number, screenH: number): { x: number; y: number } {
    const s = this.spec();
    const maxX = Math.max(0, screenW - s.cell);
    const maxY = Math.max(0, screenH - s.cell);
    return { x: Math.min(Math.max(0, x), maxX), y: Math.min(Math.max(0, y), maxY) };
  }

  /** 栅格容量估算（性能预算 X02066）。 */
  capacity(screenW: number, screenH: number): number {
    const s = this.spec();
    return Math.floor(screenW / s.cell) * Math.floor(screenH / s.cell);
  }

  /** 批量重排入格（X02072）。 */
  static layout(count: number, cell: number, cols: number): Array<{ x: number; y: number }> {
    return Array.from({ length: count }, (_, i) => ({ x: (i % cols) * cell, y: Math.floor(i / cols) * cell }));
  }

  exportSpec(): string { return JSON.stringify({ density: this.density, snap: this.snap }); }
  importSpec(json: string): boolean {
    try {
      const o = JSON.parse(json) as { density?: string; snap?: boolean };
      if (o.density && o.density in GRID_DENSITY) this.density = o.density as GridDensity;
      this.snap = !!o.snap;
      return true;
    } catch { return false; }
  }
}

/* ===================== 族0084 图标语义色 2.0 ===================== */

export const SEMANTIC_CATEGORIES = [
  'system', 'work', 'media', 'network', 'security', 'game', 'tool', 'social',
  'dev', 'file', 'trash', 'settings',
] as const;
export type SemanticCategory = (typeof SEMANTIC_CATEGORIES)[number];

export interface SemanticColor { hue: number; chroma: number; tone: number }

const BASE_SEMANTIC: Record<SemanticCategory, SemanticColor> = {
  system: { hue: 262, chroma: 0.09, tone: 0.68 },
  work: { hue: 230, chroma: 0.08, tone: 0.66 },
  media: { hue: 320, chroma: 0.11, tone: 0.7 },
  network: { hue: 200, chroma: 0.1, tone: 0.68 },
  security: { hue: 150, chroma: 0.1, tone: 0.66 },
  game: { hue: 20, chroma: 0.12, tone: 0.7 },
  tool: { hue: 85, chroma: 0.09, tone: 0.68 },
  social: { hue: 340, chroma: 0.1, tone: 0.7 },
  dev: { hue: 265, chroma: 0.1, tone: 0.64 },
  file: { hue: 60, chroma: 0.06, tone: 0.7 },
  trash: { hue: 0, chroma: 0.02, tone: 0.55 },
  settings: { hue: 250, chroma: 0.05, tone: 0.6 },
};

/** 语义色系统：五档色调矩阵 + HC 降级 + 对比门禁 + 跟随强调色。 */
export class SemanticColorSystem {
  private tier: 1 | 2 | 3 | 4 | 5 = 3;
  private highContrast = false;
  private accentFollow = false;
  private accentHue = 262;

  setTier(t: 1 | 2 | 3 | 4 | 5): void { if ([1, 2, 3, 4, 5].includes(t)) this.tier = t; }
  tierOf(): number { return this.tier; }
  setHighContrast(on: boolean): void { this.highContrast = on; }
  setAccentFollow(on: boolean, hue = 262): void { this.accentFollow = on; this.accentHue = hue; }

  colorOf(cat: SemanticCategory): SemanticColor {
    const base = BASE_SEMANTIC[cat];
    if (this.highContrast) return { hue: 0, chroma: 0, tone: 1 }; // HC：纯白描边语义
    if (this.accentFollow) return { hue: this.accentHue, chroma: base.chroma, tone: base.tone };
    // 档位控制色散强度：低档收敛、高档张扬
    const spread = [0.4, 0.6, 0.8, 1.0, 1.2][this.tier - 1]!;
    const hue = 262 + ((base.hue - 262) * spread + 360) % 360;
    return { hue: Math.round(hue) % 360, chroma: Math.min(0.13, base.chroma * spread), tone: base.tone };
  }

  allColors(): Array<{ cat: SemanticCategory; c: SemanticColor }> {
    return SEMANTIC_CATEGORIES.map((cat) => ({ cat, c: this.colorOf(cat) }));
  }

  /** 对比门禁（X02090）：tone 上叠深字自动择优。 */
  static contrastText(tone: number): 'dark' | 'light' {
    return tone >= 0.62 ? 'dark' : 'light';
  }

  /** 钳制（X02081）：C ≤0.13、tone 0.4~0.8。 */
  static clamp(c: SemanticColor): SemanticColor {
    return {
      hue: ((Math.round(c.hue) % 360) + 360) % 360,
      chroma: Math.min(0.13, Math.max(0, c.chroma)),
      tone: Math.min(0.8, Math.max(0.4, c.tone)),
    };
  }

  /** 批量导出（X02079 快照通道）。 */
  exportAll(): string { return JSON.stringify({ tier: this.tier, hc: this.highContrast }); }
  importAll(json: string): boolean {
    try {
      const o = JSON.parse(json) as { tier?: number; hc?: boolean };
      if ([1, 2, 3, 4, 5].includes(o.tier as number)) this.tier = o.tier as 1 | 2 | 3 | 4 | 5;
      this.highContrast = !!o.hc;
      return true;
    } catch { return false; }
  }
}
