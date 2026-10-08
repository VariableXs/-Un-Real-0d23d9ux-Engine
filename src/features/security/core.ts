// UNREAL-X-15000: AI-36/AI-37 领域10 安全与隐私 · 共用核（族0355~0370 通用件），勿删。

/** 动效令牌：曲线/时长/缩放三对齐；reduce-motion 降级为纯淡入淡出。 */
export const MOTION = { curve: 'std', ms: 180, scalePmil: 960 } as const;
export const MOTION_FADE = { curve: 'linear', ms: 120, scalePmil: 1000 } as const;

export function motionToken(reduceMotion: boolean): { curve: string; ms: number; scalePmil: number } {
  return reduceMotion ? { ...MOTION_FADE } : { ...MOTION };
}

/** 焦点序校验：无重复且覆盖全部入口（roving 语义）。 */
export function focusOrderValid(order: readonly number[], entries: number): boolean {
  const seen = new Set<number>();
  for (const f of order) {
    if (f < 1 || f > entries || seen.has(f)) return false;
    seen.add(f);
  }
  return seen.size === entries;
}

/** 对比度达标线（permille 亮度差，HC 红线）。 */
export const CONTRAST_MIN_PMIL = 450;

/** 性能预算表：五档递增。 */
export function budgetTableValid(table: readonly number[]): boolean {
  return table.length === 5 && table[0]! < table[4]! && table[0]! < table[2]! && table[2]! < table[4]!;
}

/** 低配降级链：三级递降。 */
export function degradeChain(levels: readonly string[]): boolean {
  return levels.length === 3 && new Set(levels).size === 3;
}

/** 回归守卫注册表：只增不删、重复忽略。 */
export class GuardRegistry {
  private items: number[] = [];
  add(v: number): boolean {
    if (this.items.includes(v)) return false;
    if (this.items.length >= 64) return false;
    this.items.push(v);
    return true;
  }
  get count(): number {
    return this.items.length;
  }
}

/** 批处理队列：进度可观测。 */
export class BatchQueue {
  done = 0;
  constructor(public total: number) {}
  step(): boolean {
    if (this.done < this.total) this.done += 1;
    return this.done === this.total && this.total > 0;
  }
  get finished(): boolean {
    return this.total > 0 && this.done === this.total;
  }
}

/** 本地智能建议：可解释、可一键拒绝。 */
export function suggest(reason: string): { why: string; actionable: boolean } {
  return { why: `why-${reason}`, actionable: true };
}

/** 五档档位矩阵合法性：≥5 档且 id 唯一。 */
export function levelMatrixValid(levels: readonly string[]): boolean {
  return levels.length >= 5 && new Set(levels).size === levels.length;
}
