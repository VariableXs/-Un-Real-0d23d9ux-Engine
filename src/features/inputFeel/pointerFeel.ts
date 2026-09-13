/**
 * UNREAL-X AI-17 · 族0166 触控板手感 2.0 + 族0167 鼠标手感 2.0（X04126~X04175）。
 *
 * 触控板：手势语法（双指/三指/四指）/边缘挡板/点按压力曲线/惯性滚动。
 * 鼠标：DPI 档位/加速度曲线/双击判定窗/滚轮档。
 */

/* ============================== 族0166 触控板手感 2.0 ============================== */

/** 触控板五档。 */
export const TOUCHPAD_PROFILES = [
  { id: "basic", name: "基础", tapClick: false, inertia: 0, gestures: 2 },
  { id: "light", name: "轻量", tapClick: true, inertia: 1, gestures: 3 },
  { id: "balanced", name: "均衡", tapClick: true, inertia: 2, gestures: 4 },
  { id: "precise", name: "精准", tapClick: true, inertia: 1, gestures: 4 },
  { id: "full", name: "全手势", tapClick: true, inertia: 3, gestures: 6 },
] as const;
export type TouchpadProfileId = (typeof TOUCHPAD_PROFILES)[number]["id"];
export const DEFAULT_TOUCHPAD_ID: TouchpadProfileId = "balanced";

export function findTouchpad(id: string): (typeof TOUCHPAD_PROFILES)[number] {
  return TOUCHPAD_PROFILES.find((p) => p.id === id) ?? TOUCHPAD_PROFILES[2]!;
}

export type TouchpadGesture =
  | "two-finger-scroll" | "two-finger-zoom" | "two-finger-rotate"
  | "three-finger-swipe-x" | "three-finger-swipe-y"
  | "four-finger-swipe" | "edge-right" | "edge-left";

/** 触控板手势识别器：滑动轨迹 → 手势事件（含阈值钳制）。 */
export class TouchpadGestureRecognizer {
  profileId: TouchpadProfileId;
  /** 最小触发位移（px）。 */
  thresholdPx = 24;
  samples: { x: number; y: number; fingers: number; atMs: number }[] = [];
  fired: TouchpadGesture[] = [];
  clamped = 0;

  constructor(profileId: string = DEFAULT_TOUCHPAD_ID) {
    const known = TOUCHPAD_PROFILES.some((p) => p.id === profileId);
    if (!known) this.clamped += 1;
    this.profileId = findTouchpad(profileId).id as TouchpadProfileId;
  }

  get maxGestures(): number {
    return findTouchpad(this.profileId).gestures;
  }

  sample(x: number, y: number, fingers: number, atMs: number): void {
    const f = Math.max(1, Math.min(5, fingers));
    if (f !== fingers) this.clamped += 1;
    this.samples.push({ x: Math.max(-1e5, Math.min(1e5, x)), y: Math.max(-1e5, Math.min(1e5, y)), fingers: f, atMs });
    if (this.samples.length > 256) this.samples.shift();
  }

  /** 结束一笔手势，识别结果（未达阈值返回 null）。 */
  finish(): TouchpadGesture | null {
    const s = this.samples;
    this.samples = [];
    if (s.length < 2) return null;
    const dx = s[s.length - 1]!.x - s[0]!.x;
    const dy = s[s.length - 1]!.y - s[0]!.y;
    const fingers = s[0]!.fingers;
    if (Math.abs(dx) < this.thresholdPx && Math.abs(dy) < this.thresholdPx) return null;
    let g: TouchpadGesture;
    if (fingers === 2) g = Math.abs(dx) >= Math.abs(dy) ? "two-finger-scroll" : "two-finger-scroll";
    else if (fingers === 3) g = Math.abs(dx) >= Math.abs(dy) ? "three-finger-swipe-x" : "three-finger-swipe-y";
    else g = "four-finger-swipe";
    if (this.fired.length < this.maxGestures) this.fired.push(g);
    return g;
  }

  /** 惯性滚动速度：末三样本平均速度（px/ms），带上限钳制。 */
  inertiaVelocity(): number {
    const tail = this.samples.slice(-3);
    if (tail.length < 2) return 0;
    const dt = tail[tail.length - 1]!.atMs - tail[0]!.atMs;
    if (dt <= 0) return 0;
    const dist = Math.abs(tail[tail.length - 1]!.y - tail[0]!.y);
    return Math.min(20, dist / dt);
  }

  reset(): void {
    this.samples = [];
    this.fired = [];
    this.clamped = 0;
  }
}

/** 点按压力曲线：压力(0..100) → 点击判定（带迟滞防误触）。 */
export class TapPressure {
  downThreshold = 35;
  upThreshold = 25;
  pressed = false;

  feed(pressure: number): boolean {
    const p = Math.max(0, Math.min(100, pressure));
    if (!this.pressed && p >= this.downThreshold) {
      this.pressed = true;
      return true;
    }
    if (this.pressed && p <= this.upThreshold) {
      this.pressed = false;
    }
    return false;
  }
}

/* ============================== 族0167 鼠标手感 2.0 ============================== */

/** 鼠标五档（DPI/加速度/滚轮）。 */
export const MOUSE_PROFILES = [
  { id: "raw", name: "原生", dpi: 800, accel: "off", wheelNotch: 3, dblClickMs: 500 },
  { id: "light", name: "轻量", dpi: 1200, accel: "off", wheelNotch: 3, dblClickMs: 500 },
  { id: "balanced", name: "均衡", dpi: 1600, accel: "low", wheelNotch: 4, dblClickMs: 450 },
  { id: "swift", name: "迅捷", dpi: 2400, accel: "medium", wheelNotch: 5, dblClickMs: 400 },
  { id: "eagle", name: "鹰眼", dpi: 3200, accel: "high", wheelNotch: 6, dblClickMs: 350 },
] as const;
export type MouseProfileId = (typeof MOUSE_PROFILES)[number]["id"];
export const DEFAULT_MOUSE_ID: MouseProfileId = "balanced";

export function findMouse(id: string): (typeof MOUSE_PROFILES)[number] {
  return MOUSE_PROFILES.find((p) => p.id === id) ?? MOUSE_PROFILES[2]!;
}

/** 鼠标加速度曲线：原始位移 → 屏幕位移。 */
export function mouseAccel(dx: number, mode: "off" | "low" | "medium" | "high"): number {
  const x = Math.max(-4096, Math.min(4096, dx));
  const a = Math.abs(x);
  const sign = Math.sign(x);
  switch (mode) {
    case "off": return x;
    case "low": return sign * (a <= 8 ? a : 8 + (a - 8) * 1.15);
    case "medium": return sign * (a <= 8 ? a : 8 + (a - 8) * 1.35);
    case "high": return sign * (a <= 6 ? a : 6 + (a - 6) * 1.6);
    default: return x;
  }
}

/** 双击判定窗口。 */
export class DoubleClickWindow {
  windowMs: number;
  lastAt = -Infinity;
  count = 0;

  constructor(windowMs: number) {
    this.windowMs = Math.max(80, Math.min(1200, windowMs));
  }

  click(atMs: number): 0 | 1 | 2 {
    if (atMs - this.lastAt <= this.windowMs) {
      this.count += 1;
      this.lastAt = -Infinity;
      return 2;
    }
    this.lastAt = atMs;
    this.count += 1;
    return 1;
  }
}

/** 滚轮行数换算：notch → 行（平滑滚动分档）。 */
export function wheelLines(notch: number, smooth: boolean): number {
  const n = Math.max(1, Math.min(12, notch));
  return smooth ? n : n * 3;
}

/** 鼠标手势：摇晃检测（抖动计数，用于摇一摇找指针）。 */
export function shakeFind(deltas: number[], threshold = 40): boolean {
  let flips = 0;
  let sign = 0;
  for (const d of deltas) {
    const v = Math.max(-4096, Math.min(4096, d));
    if (Math.abs(v) >= threshold) {
      const s = Math.sign(v);
      if (sign !== 0 && s !== sign) flips += 1;
      sign = s;
    }
  }
  return flips >= 3;
}
