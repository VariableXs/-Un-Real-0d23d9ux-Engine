/**
 * F156 指针引擎深化 · 跟随物理 + 点击状态机 + 热点变换。
 *
 * 主册判据延伸：
 * - F156「指针轨迹跟随手感」「按下要有按压反馈」——自定义指针主题带
 *   跟随动效（拖尾/弹性）时，物理必须稳定可复现：临界阻尼弹簧无过冲，
 *   帧率无关（dt 归一）；
 * - 十四章「交互状态机」——指针按压是完整状态机：按下-移出取消-拖拽-
 *   松开触发，每条路径有明确出口（点按钮途中移出 = 取消不误触）；
 * - 「热点生效精确（1px 级取放对拍）」——缩放/旋转下热点变换是精确
 *   仿射数学，不是近似缩放。
 */

// ---------- 临界阻尼跟随（帧率无关） ----------

export interface Vec2 {
  x: number;
  y: number;
}

/**
 * SmoothDamp（Unity 规范公式）：临界阻尼弹簧，无过冲、帧率无关。
 * pointer trail / 缩放跟随共用手感——鼠标停住后指针精确落位（不漂移）。
 */
export function smoothDamp(current: Vec2, target: Vec2, velocity: Vec2, smoothTime: number, dt: number, maxSpeed = Infinity): { pos: Vec2; velocity: Vec2 } {
  const st = Math.max(0.0001, smoothTime);
  const omega = 2 / st;
  const x = omega * dt;
  // 稳定性夹紧（大 dt 步长——休眠唤醒后的第一帧不炸）。
  const exp = 1 / (1 + x + 0.48 * x * x + 0.235 * x * x * x);
  const axes = (cur: number, tgt: number, vel: number): { out: number; vel: number } => {
    let change = cur - tgt;
    const originalTarget = tgt;
    const maxChange = maxSpeed * st;
    change = Math.max(-maxChange, Math.min(maxChange, change));
    tgt = cur - change;
    const temp = (vel + omega * change) * dt;
    const newVel = (vel - omega * temp) * exp;
    let out = tgt + (change + temp) * exp;
    // 过冲钳制（到达目标即停——临界阻尼不越过）。
    if ((originalTarget - cur > 0) === out > originalTarget) {
      out = originalTarget;
      return { out, vel: (originalTarget - out) / dt };
    }
    return { out, vel: newVel };
  };
  const ax = axes(current.x, target.x, velocity.x);
  const ay = axes(current.y, target.y, velocity.y);
  return { pos: { x: ax.out, y: ay.out }, velocity: { x: ax.vel, y: ay.vel } };
}

/** 跟随是否已收敛（物理停机线——静止后零更新，空转清零 F049 同源纪律）。 */
export function trailSettled(pos: Vec2, target: Vec2, epsilonPx = 0.1): boolean {
  const dx = pos.x - target.x;
  const dy = pos.y - target.y;
  return dx * dx + dy * dy < epsilonPx * epsilonPx;
}

/** 拖尾采样带（trail buffer——超出长度最老的点被淘汰，内存恒定）。 */
export class TrailBuffer {
  private points: { x: number; y: number; t: number }[] = [];
  constructor(private readonly maxAgeMs: number, private readonly maxPoints: number) {}

  push(x: number, y: number, t: number): void {
    this.points.push({ x, y, t });
    if (this.points.length > this.maxPoints) this.points.splice(0, this.points.length - this.maxPoints);
  }

  /** 取 age 窗口内的有效点（过期点惰性清除——每帧 O(1) 摊销）。 */
  sample(now: number): { x: number; y: number; age: number }[] {
    const out: { x: number; y: number; age: number }[] = [];
    while (this.points.length > 0 && now - this.points[0]!.t > this.maxAgeMs) this.points.shift();
    for (const p of this.points) out.push({ x: p.x, y: p.y, age: now - p.t });
    return out;
  }

  get size(): number {
    return this.points.length;
  }

  clear(): void {
    this.points = [];
  }
}

// ---------- 点击状态机（十四章：每条路径有明确出口） ----------

export type ClickState = "idle" | "pressed" | "dragging" | "cancelled";

export interface ClickMachineConfig {
  /** 超过此像素位移转拖拽（普通点击容忍 4px 抖动）。 */
  dragThresholdPx: number;
  /** 长按阈值（进度感——十四章「长按要有进度感」）。 */
  longPressMs: number;
}

export const DEFAULT_CLICK_CONFIG: ClickMachineConfig = { dragThresholdPx: 4, longPressMs: 500 };

export type ClickEvent =
  | { type: "press"; x: number; y: number; t: number }
  | { type: "move"; x: number; y: number; t: number }
  | { type: "release"; x: number; y: number; t: number }
  | { type: "leave" }
  | { type: "cancel" };

export type ClickOutcome = "click" | "drag-start" | "drag-end" | "cancel" | "long-press" | null;

/** 指针点击状态机：纯逻辑（可测可回放）——渲染层消费 outcome。 */
export class ClickMachine {
  private state: ClickState = "idle";
  private downPos: Vec2 = { x: 0, y: 0 };
  private downT = 0;
  private longPressFired = false;
  private cfg: ClickMachineConfig;

  constructor(cfg: Partial<ClickMachineConfig> = {}) {
    this.cfg = { ...DEFAULT_CLICK_CONFIG, ...cfg };
  }

  get current(): ClickState {
    return this.state;
  }

  /** 当前按压时长（长按进度条数据源——0..1）。 */
  progress(t: number): number {
    if (this.state !== "pressed") return 0;
    return Math.min(1, Math.max(0, (t - this.downT) / this.cfg.longPressMs));
  }

  feed(ev: ClickEvent): ClickOutcome {
    switch (ev.type) {
      case "press": {
        if (this.state !== "idle") return null; // 连点去抖：非 idle 态的 press 忽略（双份动作防抖）。
        this.state = "pressed";
        this.downPos = { x: ev.x, y: ev.y };
        this.downT = ev.t;
        this.longPressFired = false;
        return null;
      }
      case "move": {
        if (this.state === "pressed") {
          const dx = ev.x - this.downPos.x;
          const dy = ev.y - this.downPos.y;
          if (dx * dx + dy * dy > this.cfg.dragThresholdPx * this.cfg.dragThresholdPx) {
            this.state = "dragging";
            return "drag-start";
          }
          return null;
        }
        if (this.state === "dragging") return null;
        return null;
      }
      case "release": {
        if (this.state === "pressed") {
          // 长按已触发过 → 松开不再给 click（一次按压一个结果——不重复提交）。
          if (this.longPressFired) {
            this.state = "idle";
            return null;
          }
          this.state = "idle";
          return "click";
        }
        if (this.state === "dragging") {
          this.state = "idle";
          return "drag-end";
        }
        return null;
      }
      case "leave": {
        // 按压途中移出按钮范围 = 取消不误触（十四章手感纪律）。
        if (this.state === "pressed" || this.state === "dragging") {
          this.state = "cancelled";
          return "cancel";
        }
        return null;
      }
      case "cancel": {
        if (this.state !== "idle") {
          this.state = "idle";
          return "cancel";
        }
        return null;
      }
    }
  }

  /** 长按到点（由外部计时器在 progress>=1 时调用——一次有效）。 */
  fireLongPress(): ClickOutcome {
    if (this.state === "pressed" && !this.longPressFired) {
      this.longPressFired = true;
      return "long-press";
    }
    return null;
  }
}

// ---------- 热点精确变换（1px 级取放对拍的数学面） ----------

export interface Hotspot {
  x: number;
  y: number;
}

/**
 * 热点在缩放下的精确映射：缩放后取整采用「半上」规则（0.5 上取——
 * 光标落点系统性偏差向右下 0.5px 优于左上，实测对拍口径）。
 */
export function transformHotspot(h: Hotspot, scale: number): Hotspot {
  if (!Number.isFinite(scale) || scale <= 0) throw new Error("scale 必须为正有限数");
  const roundHalfUp = (v: number) => Math.floor(v + 0.5);
  return { x: roundHalfUp(h.x * scale), y: roundHalfUp(h.y * scale) };
}

/** 热点在裁剪（帧内子矩形）下的映射——.cur 多帧裁剪共用。 */
export function transformHotspotCrop(h: Hotspot, crop: { x: number; y: number }): Hotspot {
  return { x: h.x - crop.x, y: h.y - crop.y };
}

/** 双倍率 sprite 选档（与 icon-atlas.pickIconSize 同源纪律——指针侧口径）。 */
export function pickPointerSpriteTier(displayPx: number, dpr: number): 1 | 2 {
  return displayPx * dpr > 32 ? 2 : 1;
}

// ---------- 播放头调度（.ani 动画指针的帧推进） ----------

export interface AnimFrame {
  /** 帧持续（.ani 的 jif 单位 = 1/60s）。 */
  jif: number;
}

/**
 * 动画指针播放头：按帧持续时间推进、循环；
 * 系统忙时（帧间隔 > 4 倍目标）跳帧不追帧——指针动画落后于真实时间毫无意义。
 */
export class AnimCursorClock {
  private frame = 0;
  private elapsedInFrame = 0;

  constructor(private readonly frames: AnimFrame[]) {}

  get current(): number {
    return this.frame;
  }

  /** 喂入真实 dt，返回（可能的）新帧号。 */
  tick(dtMs: number): number {
    if (this.frames.length === 0) return 0;
    this.elapsedInFrame += dtMs;
    // 跳帧不追帧：单次 tick 最多推进 3 帧（卡顿后指针动画直接到当前帧）。
    let hops = 0;
    while (hops < 3) {
      const f = this.frames[this.frame]!;
      const dur = Math.max(1, (f.jif * 1000) / 60);
      if (this.elapsedInFrame < dur) break;
      this.elapsedInFrame -= dur;
      this.frame = (this.frame + 1) % this.frames.length;
      hops++;
    }
    if (hops >= 3) this.elapsedInFrame = 0;
    return this.frame;
  }

  reset(): void {
    this.frame = 0;
    this.elapsedInFrame = 0;
  }
}

// ---------- DPR 吸附（多显示器拖拽指针的像素对齐） ----------

/** 指针坐标吸附到物理像素（1/dpr 网格）——跨 DPI 拖动时指针不糊。 */
export function snapToPhysicalPixel(x: number, y: number, dpr: number): Vec2 {
  if (dpr <= 0) throw new Error("dpr 必须为正");
  const g = 1 / dpr;
  return { x: Math.round(x / g) * g, y: Math.round(y / g) * g };
}
