/**
 * J 鼠标域 · F606 倾斜滚轮支持 · 纵深引擎（批次七）。
 *
 * v4 有横向惯性通道（F204 余韵共享）。本引擎补倾斜滚轮的本体——
 * 真倾斜滚轮（MX Master 类）是模拟量：左右倾斜是连续角度（±10°），
 * 不是两枚离散键。离散键口径浪费了模拟量的表达力（缓滚/急滚/回中
 * 的手感全部丢失）。三块纵深：
 *
 * 1. 角度→速率映射：倾斜角线性映射横滚速率（5° = 200px/s 起步、
 *    10° = 1200px/s 封顶），带死区（±1.5°——回中不漂移）。
 *
 * 2. 动量余韵：倾斜松开（回中）后横滚带 300ms 指数余韵——真实滚轮
 *    有物理惯性，瞬停是「电子感」。与 F204 横向惯性共享余韵常数
 *    （同一格律，不另造一套）。
 *
 * 3. 倾斜按压（tilt-press）：滚轮下按是中键语义（F604 接管），但
 *    「按住 + 倾斜」是缩放语义（时间线/画布类应用的真实工作流）。
 *    与 F604 互斥仲裁：按压先于倾斜 → 自动滚接管；倾斜先于按压 →
 *    缩放接管。先到先得，一次会话内不换手（防抖红线）。
 *
 * 判据锚点：
 * - 死区与线性映射端点 → tiltRate()
 * - 余韵与 F204 同常数 → TILT_COAST_MS / tiltCoast()
 * - 先到先得仲裁 → arbiterTiltPress()
 * - 模拟量离散化无损档（设备只报离散键时的降级口径）→ discreteTiltRate()
 */

/* ------------------------------- 角度→速率 ------------------------------- */

/** 死区（°）——回中防漂移。 */
export const TILT_DEADZONE_DEG = 1.5;
/** 满偏角（°）——物理滚轮极限口径。 */
export const TILT_FULL_DEG = 10;
/** 起步速率（px/s，刚出死区）。 */
export const TILT_RATE_MIN = 200;
/** 封顶速率（px/s，满偏）。 */
export const TILT_RATE_MAX = 1200;

/**
 * 倾斜角（-10..10°）→ 横滚速率（px/s，负=左）。
 * 死区内严格 0（不漂移判据）；死区外线性到满偏封顶。
 */
export function tiltRate(angleDeg: number): number {
  const a = Math.max(-TILT_FULL_DEG, Math.min(TILT_FULL_DEG, angleDeg));
  const mag = Math.abs(a);
  if (mag <= TILT_DEADZONE_DEG) return 0;
  const t = (mag - TILT_DEADZONE_DEG) / (TILT_FULL_DEG - TILT_DEADZONE_DEG);
  return Math.sign(a) * (TILT_RATE_MIN + (TILT_RATE_MAX - TILT_RATE_MIN) * t);
}

/**
 * 离散降级：设备只报「左倾/右倾/回中」三态时，用固定角 6° 等效
 * （中段速率——降级不是阉割，是明确档位的诚实口径）。
 */
export function discreteTiltRate(dir: "left" | "right" | "center"): number {
  if (dir === "center") return 0;
  return dir === "left" ? -tiltRate(6) : tiltRate(6);
}

/* ------------------------------- 动量余韵 ------------------------------- */

/** 余韵时长（ms）——与 F204 横向惯性同一常数（同一格律纪律）。 */
export const TILT_COAST_MS = 300;

/**
 * 余韵系数：松开后 tMs 时刻的速率保留比例（指数衰减，t≥TILT_COAST_MS → 0）。
 * 与惯性引擎共享常数但各自独立调用（正交，不共享状态）。
 */
export function tiltCoast(tMs: number): number {
  if (tMs >= TILT_COAST_MS) return 0;
  return Math.exp((-3 * tMs) / TILT_COAST_MS); // 3 倍时间常数——300ms 处降到 5% 以下
}

/* ------------------------------- 倾斜按压仲裁 ------------------------------- */

export type TiltPressWinner = "autoscroll" | "zoom" | "none";

/**
 * 先到先得仲裁（一次按压会话内锁定）：
 * - 都未发生 → none；
 * - 按压时刻 ≤ 倾斜时刻 → autoscroll（F604 接管，倾斜只在其内改横滚）；
 * - 倾斜先于按压 → zoom（倾斜语义在先，按压只确认）。
 * 防抖承诺：胜者一经返回由宿主锁定到会话结束——本函数是纯仲裁，
 * 不持状态（状态在宿主，一处一事实）。
 */
export function arbiterTiltPress(pressAtMs: number | null, tiltAtMs: number | null): TiltPressWinner {
  if (pressAtMs === null && tiltAtMs === null) return "none";
  if (pressAtMs !== null && (tiltAtMs === null || pressAtMs <= tiltAtMs)) return "autoscroll";
  return "zoom";
}

/* ------------------------------- 缩放通道 ------------------------------- */

/**
 * 缩放通道：倾斜角 → 缩放步进速率（步/秒）。与横滚通道物理隔离——
 * 缩放是「档位感」（每步明确的 1.1x 倍率链），不是连续像素流；
 * 端点档（±10°）步频封顶 12 步/s（人手能数清的上限）。
 */
export function tiltZoomStepsPerSec(angleDeg: number): number {
  const rate = Math.abs(tiltRate(angleDeg));
  if (rate === 0) return 0;
  return Math.round((rate / TILT_RATE_MAX) * 12 * 10) / 10;
}

/* ------------------------------- 模拟量运行时通道（批次八接线件） ------------------------------- */

/**
 * 模拟量倾斜通道：连续角度输入 → 本帧横滚像素。
 * 与 onWheel 档位路径的分工（一处一事实）：
 * - 档位路径（deltaX ≥ 1 整档）：tiltFromWheelEvent 档语义；
 * - 模拟量路径（deltaX 为亚档粒度——触控板/高端模拟倾斜轮）：
 *   角度映射 → 一阶趋近平滑（80ms 时间常数，角度突变不瞬跳）→
 *   松开后 tiltCoast 余韵衰减（与 F204 惯性同格律）。
 */
export class TiltAnalogChannel {
  private vel = 0; // px/s（带符号）

  /**
   * 喂一帧模拟量角度。
   * @param dtMs 距上帧毫秒（事件时间戳实测）
   * @returns 本帧应滚像素（带符号；0 = 死区内）
   */
  feed(angleDeg: number, dtMs: number): number {
    const target = tiltRate(angleDeg);
    const k = Math.min(1, Math.max(1, dtMs) / 80);
    this.vel += (target - this.vel) * k;
    return this.framePx(dtMs);
  }

  /** 松开倾斜后的余韵帧（指数衰减到 <1px/s 归零——零静默漂移）。 */
  coast(dtMs: number): number {
    this.vel *= Math.exp(-Math.max(1, dtMs) / TILT_COAST_MS);
    if (Math.abs(this.vel) < 1) this.vel = 0;
    return this.framePx(dtMs);
  }

  private framePx(dtMs: number): number {
    const px = (this.vel * Math.max(1, dtMs)) / 1000;
    return Math.abs(px) < 0.01 ? 0 : Math.round(px * 100) / 100;
  }

  get velocityPxPerSec(): number {
    return Math.round(this.vel * 10) / 10;
  }

  reset(): void {
    this.vel = 0;
  }
}

/**
 * 亚档粒度判据：|deltaX| 在 (0, 0.9) 视为模拟量输入（整档 ≥1）。
 * 阈值 0.9 而非 1——部分设备发 0.95 的量化档，归模拟侧（宁慢勿跳）。
 */
export function isAnalogTilt(deltaX: number): boolean {
  return deltaX !== 0 && Math.abs(deltaX) < 0.9;
}
