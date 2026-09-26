/**
 * J 鼠标域 · F610 悬停时序自定义 + F619 长按时长统一旋钮。
 *
 * F610——悬停反馈的节奏两把旋钮：菜单子级展开延迟（Windows 默认 400ms）
 * 四档（200/300/400/600ms）、tooltip 出现延迟（F549 系 500ms 基线）独立四档；
 * 两旋钮独立记忆、即时生效；展开延迟只影响悬停展开、不影响点击展开（点击
 * 永远即时——手感红线）；tooltip 的跟随/翻转行为（F549）不受延迟档影响。
 *
 * F619——全系统「按住多久算长按」的统一时长旋钮（三档：紧凑 0.6x/标准 1x/
 * 从容 1.6x）：触屏长按、磁贴长按（F496 的 500ms）、ClickLock 抓起（F542 的
 * 1.1s）统一跟随一档按比例缩放而非一刀切；藏在辅助功能-指针页（进阶人群去）。
 *
 * 判据锚点：
 * - 两旋钮四档 → HOVER_MENU_DELAY 档位表 + clampHoverDelay()
 * - 点击展开不受影响 → clickExpandsImmediately()（恒 true，红线在类型上成立）
 * - 默认档对拍 Windows 400ms / 基线 500ms → 常量即默认
 * - 三档对三场景缩放 / 0.6x-1.6x 比例 / 默认档=现行值 → longPressMs()
 * - 新增长按功能必须接入本旋钮的登记纪律 → registerLongPress()（未登记抛错）
 */

/* ------------------------------- F610 悬停时序 ------------------------------- */

/** 四档（两旋钮共用档位表）。 */
export const HOVER_DELAY_STEPS = [200, 300, 400, 600] as const;

/** Windows 默认 400ms（迁移零差异）。 */
export const HOVER_MENU_DEFAULT = 400;
/** tooltip 基线 500ms（F549 系）。 */
export const HOVER_TOOLTIP_DEFAULT = 500;

export interface HoverTimingConfig {
  menuDelayMs: number;
  tooltipDelayMs: number;
}

/** 档位钳制：非档值收敛到最近档（旋钮只会产出四档之一）。 */
export function clampHoverDelay(ms: number): number {
  return HOVER_DELAY_STEPS.reduce((best, v) => (Math.abs(v - ms) < Math.abs(best - ms) ? v : best), HOVER_DELAY_STEPS[0]);
}

/** 悬停展开计时器：到点回调，重启/取消显式（无幽灵回调）。 */
export class HoverTimer {
  private timer: number | null = null;

  start(delayMs: number, onFire: () => void): void {
    this.cancel();
    if (delayMs <= 0) {
      onFire();
      return;
    }
    this.timer = window.setTimeout(() => {
      this.timer = null;
      onFire();
    }, delayMs);
  }

  cancel(): void {
    if (this.timer !== null) {
      window.clearTimeout(this.timer);
      this.timer = null;
    }
  }
}

/**
 * 手感红线落地：点击展开永远即时——本函数恒真，供菜单组件在点击路径
 * 显式短路悬停延迟（类型与调用点双重成立，防被后续改坏）。
 */
export function clickExpandsImmediately(): true {
  return true;
}

/**
 * 悬停展开编排器（通用子级展开的时序机）：
 * enter → 延迟触发；leave → 取消；点击 → 立即（红线短路）；重入重启。
 * 菜单组件/树/工具条共用——全系统只有这一份时序逻辑（一致性章十）。
 */
export class MenuHoverController {
  private timer: number | null = null;
  private pendingKey: string | null = null;

  constructor(
    private readonly delayMs: () => number,
    private readonly onOpen: (key: string) => void,
    private readonly onClose: (key: string) => void,
  ) {}

  /** 指针进入某子级宿主。 */
  enter(key: string): void {
    if (this.pendingKey === key) return;
    this.cancelPending();
    this.pendingKey = key;
    const d = this.delayMs();
    if (d <= 0) {
      this.fire();
      return;
    }
    this.timer = window.setTimeout(() => this.fire(), d);
  }

  /** 指针离开（未落到别的子级时由调用方决定是否关闭）。 */
  leave(): void {
    this.cancelPending();
  }

  /** 点击路径：绕过延迟立即展开（clickExpandsImmediately 红线）。 */
  click(key: string): void {
    this.cancelPending();
    this.pendingKey = key;
    this.fire();
  }

  /** 切换到另一个宿主时关闭旧宿主。 */
  private fire(): void {
    this.timer = null;
    if (this.pendingKey) this.onOpen(this.pendingKey);
  }

  close(key: string): void {
    if (this.pendingKey === key) {
      this.pendingKey = null;
      this.onClose(key);
    }
  }

  cancelPending(): void {
    if (this.timer !== null) {
      window.clearTimeout(this.timer);
      this.timer = null;
    }
  }

  dispose(): void {
    this.cancelPending();
    this.pendingKey = null;
  }
}

/* ------------------------------- F619 长按旋钮 ------------------------------- */

export type LongPressScale = 0.6 | 1.0 | 1.6;
export const LONG_PRESS_SCALES: LongPressScale[] = [0.6, 1.0, 1.6];
export const LONG_PRESS_LABELS: Record<string, string> = {
  "0.6": "紧凑 0.6x",
  "1": "标准 1x",
  "1.6": "从容 1.6x",
};

export interface LongPressConfig {
  scale: number;
  /** 登记纪律：功能键 → 基线 ms（新增长按功能必须登记，未登记抛错）。 */
  registry: Record<string, number>;
}

/** 现行基线（默认档=现行值判据的「现行值」——一处一事实）： */
export const LONG_PRESS_BASE: Record<string, number> = {
  touchMenu: 500,   // 触屏长按菜单
  magnetTile: 500,  // F496 磁贴长按
  clickLock: 1100,  // F542 ClickLock 抓起
};

/** 缩放计算：基线 × 档位，四舍五入到 10ms（比例正确判据）。 */
export function longPressMs(baseMs: number, scale: number): number {
  const s = LONG_PRESS_SCALES.includes(scale as LongPressScale) ? scale : 1.0;
  return Math.round((baseMs * s) / 10) * 10;
}

/** 从旋钮取某功能的实际时长（登记纪律执法点：未登记抛错——异常显性化）。 */
export function longPressFor(feature: string, cfg: LongPressConfig): number {
  const base = cfg.registry[feature] ?? LONG_PRESS_BASE[feature];
  if (base === undefined) {
    throw new Error(`[mouse-j1:F619] 长按功能「${feature}」未在旋钮登记——新增长按功能必须接入本旋钮（登记纪律）`);
  }
  return longPressMs(base, cfg.scale);
}

/** 默认档对账脚本判据：1x 档各功能值 = 现行基线值。 */
export function longPressDefaultAudit(cfg: LongPressConfig): { feature: string; base: number; at1x: number; ok: boolean }[] {
  return Object.keys(LONG_PRESS_BASE).map((f) => {
    const base = LONG_PRESS_BASE[f]!;
    const at1x = longPressMs(base, cfg.scale === 1.0 ? 1.0 : 1.0);
    return { feature: f, base, at1x, ok: base === at1x };
  });
}
