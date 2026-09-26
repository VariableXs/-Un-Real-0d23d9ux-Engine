/**
 * J 鼠标域 · F619 长按时长统一旋钮 · 仲裁器引擎（批次八）。
 *
 * v3 的 longPress registry 只回答「各功能时长 = 注册值 × 全局 scale」。
 * 真实工作流还差三块：
 *
 * 1. 多功能竞争同一按住——左键长按可能同时被「长按菜单」「拖拽预备」
 *    声明。谁赢？规则：**最短时长者先到先得**（用户等待成本最小化；
 *    长时长功能在被短路时记一次体验日志，对账可见）；
 *
 * 2. 移动取消——按住期间指针移出取消半径（12px）即取消全部声明
 *    （用户改主意 = 拖拽/滑动意图，长按不该再弹菜单）；
 *
 * 3. 进度可观测——返回 0..1 进度与剩余毫秒（宿主画进度环；
 *    「长按要有进度感而不是莫名卡住」章五）。
 *
 * 判据锚点：
 * - 最短先得 → arbiter 声明排序
 * - 移动取消半径 → HOLD_CANCEL_RADIUS_PX
 * - 进度环 → progress()
 */

/** 移动取消半径（px）——按住期间位移超过即取消全部长按声明。 */
export const HOLD_CANCEL_RADIUS_PX = 12;

/** 单个长按声明（功能 id + 生效时长）。 */
export interface HoldClaim {
  /** 功能 id（longPress.registry 的键——动作路由同源）。 */
  id: string;
  /** 生效时长（ms，已含全局 scale——本仲裁器不做二次缩放）。 */
  durationMs: number;
}

export interface HoldProgress {
  /** 将最先到期的声明 id。 */
  id: string;
  /** 该声明的进度（0..1）。 */
  progress: number;
  /** 距触发剩余毫秒。 */
  remainingMs: number;
}

/** 完成形态：winner=命中声明；short-circuited=被更短声明短路的功能（对账用）。 */
export type HoldOutcome =
  | { kind: "winner"; id: string; atMs: number }
  | { kind: "none"; reason: "released-early" | "move-cancelled" | "no-claims" };

export class HoldArbiter {
  private claims: HoldClaim[] = [];
  private startedAt: number | null = null;
  private moved = false;
  private origin = { x: 0, y: 0 };
  /** 被短路声明的 id（win 后剩余未到期者——体验日志对账面）。 */
  private shortCircuited: string[] = [];
  /** 已胜出的声明（win 后锁定——后续 finish 不重复触发）。 */
  private winnerId: string | null = null;
  private winnerAtMs = 0;

  /**
   * 按下即登记全部声明（durationMs 升序排——最短先得）。
   * 空/非正时长声明直接丢弃（仲裁器不猜）。
   */
  begin(claims: HoldClaim[], atMs: number, px: number, py: number): void {
    this.claims = claims
      .filter((c) => c.durationMs > 0)
      .sort((a, b) => a.durationMs - b.durationMs);
    this.startedAt = this.claims.length > 0 ? atMs : null;
    this.moved = false;
    this.origin = { x: px, y: py };
    this.shortCircuited = [];
    this.winnerId = null;
  }

  get active(): boolean {
    return this.startedAt !== null && this.winnerId === null && !this.moved;
  }

  /** 按住期间指针移动：超出取消半径 = 全部取消（拖拽意图优先）。 */
  move(px: number, py: number): "tracking" | "cancelled" {
    if (!this.active) return "tracking";
    if (Math.hypot(px - this.origin.x, py - this.origin.y) > HOLD_CANCEL_RADIUS_PX) {
      this.moved = true;
      return "cancelled";
    }
    return "tracking";
  }

  /** 当前进度（最短到期声明口径——进度环显示的就是「再按多久触发」）。 */
  progress(atMs: number): HoldProgress | null {
    if (!this.active || this.startedAt === null) return null;
    const first = this.claims[0];
    if (!first) return null;
    const elapsed = atMs - this.startedAt;
    const progress = Math.min(1, elapsed / first.durationMs);
    return { id: first.id, progress, remainingMs: Math.max(0, Math.round(first.durationMs - elapsed)) };
  }

  /**
   * 周期检查（宿主 RAF 调）：返回最先到期的声明（若已到期）。
   * 触发后锁定 winnerId——同一按住只触发一次；剩余声明记入短路账。
   */
  tick(atMs: number): { fired: string; shortCircuited: string[] } | null {
    if (!this.active || this.startedAt === null) return null;
    const due = this.claims.find((c) => atMs - this.startedAt! >= c.durationMs);
    if (!due) return null;
    this.winnerId = due.id;
    this.winnerAtMs = atMs;
    this.shortCircuited = this.claims.filter((c) => c.id !== due.id).map((c) => c.id);
    return { fired: due.id, shortCircuited: [...this.shortCircuited] };
  }

  /** 松手：未触发时按取消/早释归因（触发后 = 返回胜者与真实触发时刻）。 */
  finish(): HoldOutcome {
    if (this.winnerId !== null) {
      const out: HoldOutcome = { kind: "winner", id: this.winnerId, atMs: this.winnerAtMs };
      this.reset();
      return out;
    }
    if (this.startedAt === null) {
      this.reset();
      return { kind: "none", reason: "no-claims" };
    }
    const reason: "released-early" | "move-cancelled" = this.moved ? "move-cancelled" : "released-early";
    this.reset();
    return { kind: "none", reason };
  }

  reset(): void {
    this.claims = [];
    this.startedAt = null;
    this.moved = false;
    this.shortCircuited = [];
    this.winnerId = null;
    this.winnerAtMs = 0;
  }
}

/**
 * registry + 全局 scale → 声明清单（运行时装配口径——与 F610 悬停
 * 时序同一缩放纪律：registry 存基准值，scale 在消费端乘）。
 */
export function holdClaimsFromRegistry(registry: Record<string, number>, scale: number): HoldClaim[] {
  const s = scale > 0 ? scale : 1;
  return Object.entries(registry).map(([id, baseMs]) => ({ id, durationMs: Math.round(baseMs * s) }));
}
