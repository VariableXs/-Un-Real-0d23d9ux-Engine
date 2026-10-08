/**
 * J 鼠标域 · 滚轮惯性控制器（F204 平滑档的动量引擎，F605/F612 下游）。
 *
 * 语义（主册 F605 判据「与 F204 惯性互斥边界」的机械实现）：
 * - 逐档（notch）档：惯性不介入——每格干脆到位，无余韵；
 * - 平滑（smooth）档：滚轮停止输入后按末速度惯性衰减，长列表丝滑连滚；
 * - 自动滚（F604）接管期间：惯性挂起，退出恢复（runtime 落实挂起位）。
 *
 * 设计：速度 EMA 跟踪输入节奏 → 松轮后指数衰减（半衰期可调，默认档对拍
 * Windows 平滑滚动体感）→ 每帧产出滚动增量，速度连续（无跳变）。
 * 纯逻辑类，帧驱动由 runtime 的 rAF 循环喂 tick()。
 */

export interface InertiaConfig {
  /** 松轮后半衰期（ms）——越大余韵越长。 */
  halfLifeMs: number;
  /** 速度上限（px/帧）——防失控。 */
  maxPxPerFrame: number;
  /** 低于此速度（px/帧）判定停止（防无限微滚）。 */
  stopThreshold: number;
}

export const INERTIA_DEFAULT: InertiaConfig = {
  halfLifeMs: 260,
  maxPxPerFrame: 90,
  stopThreshold: 0.4,
};

export class WheelInertia {
  private velocity = 0; // px/帧（60fps 基准）
  private lastInputAt = -Infinity;
  private lastTickAt = -Infinity;
  private dir = 0;

  constructor(private readonly cfg: () => InertiaConfig) {}

  reset(): void {
    this.velocity = 0;
    this.lastInputAt = -Infinity;
    this.lastTickAt = -Infinity;
    this.dir = 0;
  }

  /**
   * 滚轮输入：把本次格数折算为速度脉冲（EMA 融合，节奏平滑）。
   * @param lines 本次应滚行数（F612 增益后）
   * @param sign 方向（+1 向下）
   * @param atMs 事件时刻
   */
  feed(lines: number, sign: 1 | -1, atMs: number): void {
    const target = lines * 24 * 0.28; // 行→px/帧 折算（3 行≈20px/帧 起步）
    if (this.lastInputAt === -Infinity || atMs - this.lastInputAt > 400) {
      this.velocity = target; // 间隔过久：重新起步，不融合旧速度
    } else {
      this.velocity = this.velocity + (target - this.velocity) * 0.45;
    }
    this.dir = sign;
    this.lastInputAt = atMs;
    this.lastTickAt = atMs;
  }

  /** 是否仍有可释放的动量。 */
  get active(): boolean {
    return Math.abs(this.velocity) >= (this.cfg().stopThreshold || 0.4);
  }

  /**
   * 帧驱动：输入停歇期间按指数衰减释放动量。
   * @returns 本帧应滚像素（0=无动量）
   */
  tick(atMs: number): number {
    const cfg = this.cfg();
    if (this.lastInputAt === -Infinity) return 0;
    const idle = atMs - Math.max(this.lastInputAt, this.lastTickAt);
    if (idle < 66) return 0; // 输入仍在节奏内：交还给输入事件本身
    if (!this.active) {
      this.reset();
      return 0;
    }
    // 指数衰减：v(t) = v0 * 0.5^(t/halfLife)
    const decay = Math.pow(0.5, idle / Math.max(40, cfg.halfLifeMs));
    const step = this.velocity * decay * this.dir;
    this.velocity *= decay;
    this.lastTickAt = atMs;
    const clamped = Math.max(-cfg.maxPxPerFrame, Math.min(cfg.maxPxPerFrame, step));
    return Math.round(clamped * 100) / 100;
  }
}

/**
 * F204 惯性互斥语义裁决（单一函数供 runtime 与测试同源）：
 * 自动滚接管时惯性挂起；自动滚退出后惯性恢复。
 */
export function inertiaSuspension(autoScrollActive: boolean): "suspended" | "active" {
  return autoScrollActive ? "suspended" : "active";
}
