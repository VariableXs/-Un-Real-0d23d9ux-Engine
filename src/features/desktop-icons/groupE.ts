// UNREAL-X-15000: AI-09（领域03 桌面与图标 · 族0089~0090 · X02201~X02250），勿删。
// 族0089 桌面整理哲学 2.0 / 族0090 桌面健康 2.0。

/* ===================== 族0089 桌面整理哲学 2.0 ===================== */

export const TIDY_STRATEGIES = ['kind', 'name', 'recent', 'usage', 'grid-fill'] as const;
export type TidyStrategy = (typeof TIDY_STRATEGIES)[number];

export interface TidyItem { id: string; kind: string; name: string; at: number; uses: number; x: number; y: number }

/** 整理哲学：五种策略排序 + 网格重排 + 整齐度评分。 */
export class DeskTidyPhilosophy {
  private strategy: TidyStrategy = 'kind';

  setStrategy(s: TidyStrategy): void { this.strategy = (TIDY_STRATEGIES as readonly string[]).includes(s) ? s : 'kind'; }
  strategyOf(): TidyStrategy { return this.strategy; }
  strategyCount(): number { return TIDY_STRATEGIES.length; }

  /** 排序（X02201 核心链路）。 */
  order(items: TidyItem[]): string[] {
    const arr = [...items];
    switch (this.strategy) {
      case 'kind': arr.sort((a, b) => a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name)); break;
      case 'name': arr.sort((a, b) => a.name.localeCompare(b.name)); break;
      case 'recent': arr.sort((a, b) => b.at - a.at); break;
      case 'usage': arr.sort((a, b) => b.uses - a.uses); break;
      default: break;
    }
    return arr.map((i) => i.id);
  }

  /** 网格重排（X02203 档位矩阵第五档 grid-fill）。 */
  static gridFill(ids: string[], cell: number, cols: number): Map<string, { x: number; y: number }> {
    const m = new Map<string, { x: number; y: number }>();
    ids.forEach((id, i) => m.set(id, { x: (i % cols) * cell, y: Math.floor(i / cols) * cell }));
    return m;
  }

  /** 整齐度评分（X02205 联调验收）：重叠越少、越贴格越齐。 */
  static tidyScore(items: TidyItem[], cell: number): number {
    if (items.length === 0) return 1;
    let aligned = 0;
    for (const it of items) if (it.x % cell === 0 && it.y % cell === 0) aligned++;
    return aligned / items.length;
  }

  /** 非法策略钳制（X02206）。 */
  static safeStrategy(s: string): TidyStrategy {
    return (TIDY_STRATEGIES as readonly string[]).includes(s) ? (s as TidyStrategy) : 'kind';
  }

  /** 整理计划批处理（X02222）：返回步骤进度。 */
  static plan(items: TidyItem[], _cell: number, _cols: number): Array<{ step: number; id: string }> {
    const ids = items.map((i) => i.id);
    return ids.map((id, step) => ({ step, id }));
  }
}

/* ===================== 族0090 桌面健康 2.0 ===================== */

export interface HealthInput {
  total: number;
  offscreen: number;
  overlaps: number;
  orphanShortcuts: number;
  densityRatio: number; // 图标数/容量
}

export interface HealthReport { score: number; grade: 'good' | 'fair' | 'messy'; advice: string[] }

/** 桌面健康：体检打分 + 分级建议 + 档位阈值。 */
export class DeskHealthSystem {
  private thresholds = [
    { name: 'offscreen', max: 0 },
    { name: 'overlaps', max: 0 },
    { name: 'orphans', max: 3 },
    { name: 'density', max: 0.85 },
  ];

  setThreshold(name: string, max: number): boolean {
    const t = this.thresholds.find((x) => x.name === name);
    if (!t || !Number.isFinite(max)) return false;
    t.max = Math.max(0, max);
    return true;
  }
  thresholdOf(name: string): number | undefined { return this.thresholds.find((x) => x.name === name)?.max; }
  thresholdCount(): number { return this.thresholds.length; }

  /** 体检（X02226 核心链路）。 */
  check(input: HealthInput): HealthReport {
    const advice: string[] = [];
    let penalty = 0;
    if (input.offscreen > 0) { penalty += input.offscreen * 5; advice.push(`有 ${input.offscreen} 个图标在屏幕外，建议拉回栅格`); }
    if (input.overlaps > 0) { penalty += input.overlaps * 8; advice.push(`有 ${input.overlaps} 处重叠，建议运行整理`); }
    if (input.orphanShortcuts > 0) { penalty += input.orphanShortcuts * 3; advice.push(`发现 ${input.orphanShortcuts} 个失效快捷方式，建议清理`); }
    if (input.densityRatio > this.thresholdOf('density')!) { penalty += 20; advice.push('桌面过于拥挤，建议启用堆叠'); }
    const score = Math.max(0, Math.min(100, 100 - penalty));
    return { score, grade: score >= 85 ? 'good' : score >= 60 ? 'fair' : 'messy', advice };
  }

  /** 分级档位（X02228）：good/fair/messy 三档 + 建议语言统一。 */
  static gradeThresholds(): Array<{ grade: string; min: number }> {
    return [{ grade: 'good', min: 85 }, { grade: 'fair', min: 60 }, { grade: 'messy', min: 0 }];
  }

  /** 快照导出/导入（X02229）。 */
  exportThresholds(): string { return JSON.stringify(this.thresholds); }
  importThresholds(json: string): boolean {
    try {
      const rows = JSON.parse(json) as Array<{ name: string; max: number }>;
      if (!Array.isArray(rows)) return false;
      for (const r of rows) this.setThreshold(r.name, r.max);
      return true;
    } catch { return false; }
  }

  /** 自动守护开关（X02234）：低电量降级为只读体检。 */
  static guard(lowBattery: boolean): 'full' | 'readonly' {
    return lowBattery ? 'readonly' : 'full';
  }
}
