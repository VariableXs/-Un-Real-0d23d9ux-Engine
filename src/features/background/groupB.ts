// UNREAL-X-15000: AI-10（领域03 桌面与图标 · 族0095 动态物理 · X02351~X02375），勿删。
// 壁纸动态物理：粒子/视差响应 + 强度档 + 低配降级。

export const PHYSICS_TIERS = ['off', 'gentle', 'standard', 'lively', 'storm'] as const;
export type PhysicsTier = (typeof PHYSICS_TIERS)[number];

const TIER_PARAMS: Record<PhysicsTier, { particles: number; parallax: number; drag: number }> = {
  off: { particles: 0, parallax: 0, drag: 0 },
  gentle: { particles: 40, parallax: 0.02, drag: 0.9 },
  standard: { particles: 90, parallax: 0.05, drag: 0.82 },
  lively: { particles: 160, parallax: 0.08, drag: 0.74 },
  storm: { particles: 260, parallax: 0.12, drag: 0.6 },
};

export interface PointerState { x: number; y: number; down: boolean }

/** 动态物理系统：指针视差 + 粒子场 + 档位降级。 */
export class WallpaperPhysics {
  private tier: PhysicsTier = 'standard';
  private lowPower = false;
  private pointer: PointerState = { x: 0.5, y: 0.5, down: false };
  private ticks = 0;

  setTier(t: PhysicsTier): void { this.tier = (PHYSICS_TIERS as readonly string[]).includes(t) ? t : 'standard'; }
  tierOf(): PhysicsTier { return this.tier; }
  tierCount(): number { return PHYSICS_TIERS.length; }
  setLowPower(on: boolean): void { this.lowPower = on; }
  setPointer(x: number, y: number, down = false): void {
    this.pointer = { x: Math.min(1, Math.max(0, x)), y: Math.min(1, Math.max(0, y)), down };
  }

  params(): { particles: number; parallax: number; drag: number } {
    if (this.lowPower) {
      const base = TIER_PARAMS[this.tier];
      return { particles: Math.floor(base.particles / 4), parallax: base.parallax / 2, drag: base.drag };
    }
    return { ...TIER_PARAMS[this.tier] };
  }

  /** 视差偏移（X02351 核心链路）：返回归一化偏移。 */
  parallaxOffset(): { dx: number; dy: number } {
    const p = this.params();
    return { dx: (this.pointer.x - 0.5) * 2 * p.parallax, dy: (this.pointer.y - 0.5) * 2 * p.parallax };
  }

  /** 粒子推进一帧（X02352）。 */
  step(dtMs: number): number {
    const p = this.params();
    if (p.particles === 0) return 0;
    this.ticks++;
    const pressBoost = this.pointer.down ? 1.5 : 1;
    return Math.floor(p.particles * Math.min(2, dtMs / 16) * pressBoost);
  }
  tickCount(): number { return this.ticks; }

  /** 非法档位钳制（X02356）。 */
  static safeTier(t: string): PhysicsTier {
    return (PHYSICS_TIERS as readonly string[]).includes(t) ? (t as PhysicsTier) : 'standard';
  }

  /** 低配降级链（X02369）：storm→standard→gentle→off。 */
  static degradeChain(t: PhysicsTier): PhysicsTier[] {
    const order: PhysicsTier[] = ['storm', 'lively', 'standard', 'gentle', 'off'];
    const i = order.indexOf(t);
    return i < 0 ? ['standard'] : order.slice(i);
  }
}
