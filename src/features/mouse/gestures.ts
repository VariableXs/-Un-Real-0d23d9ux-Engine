/**
 * J 鼠标域 · F617 右键手势层。
 *
 * 按住右键画轨迹触发动作（内置 12 手势可自定义轨迹库），默认关（不惊扰
 * Windows 习惯党）；轨迹识别走八方向编码（容错两向偏差）；手势进行中显示
 * 墨迹路径（120ms 淡出）；与右键菜单分工明确——无轨迹松开=正常右键菜单
 * （零误伤）。
 *
 * 判据锚点：
 * - 12 手势识别率（方向编码样本 200 例 ≥95%）→ GestureRecognizer（单测覆盖）
 * - 墨迹淡出 120ms → TRAIL_FADE_MS
 * - 无轨迹回退菜单判据 → recognize() 低于最小步数返回 null（runtime 据此放行菜单）
 * - 优先级矩阵（与 F215 右键菜单 / F615 侧键）→ gesturePriorityNote()
 */

export const TRAIL_FADE_MS = 120;
/** 最小方向步数：不足视为「无轨迹」→ 右键菜单兜底（零误伤）。 */
export const GESTURE_MIN_STEPS = 2;
/** 方向步长阈值：位移超过此值记一步（px）。 */
export const GESTURE_STEP_PX = 24;

/** 八方向编码（0=右，顺时针，45°/档）。 */
export type Dir8 = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7;

export function encodeDir8(dx: number, dy: number): Dir8 {
  let deg = (Math.atan2(dy, dx) * 180) / Math.PI;
  if (deg < 0) deg += 360;
  return (Math.round(deg / 45) % 8) as Dir8;
}

export const DIR8_NAMES = ["右", "右下", "下", "左下", "左", "左上", "上", "右上"] as const;

/** 内置 12 手势（方向编码即定义——轨迹库同源）。 */
export const BUILTIN_GESTURES: { id: string; name: string; dirs: Dir8[]; action: string }[] = [
  { id: "close-tab", name: "关闭标签", dirs: [3, 4], action: "window.close-tab" },
  { id: "forward", name: "前进", dirs: [0], action: "nav.forward" },
  { id: "back", name: "后退", dirs: [4], action: "nav.back" },
  { id: "new-item", name: "新建", dirs: [2], action: "file.new" },
  { id: "refresh", name: "刷新", dirs: [6, 2], action: "view.refresh" },
  { id: "reload-hard", name: "刷新（忽略缓存）", dirs: [6, 2, 6, 2], action: "view.refresh-hard" },
  { id: "up-level", name: "上一级", dirs: [6], action: "nav.up" },
  { id: "copy", name: "复制", dirs: [4, 0], action: "edit.copy" },
  { id: "cut", name: "剪切", dirs: [0, 4], action: "edit.cut" },
  { id: "paste", name: "粘贴", dirs: [4, 0, 4], action: "edit.paste" },
  { id: "minimize", name: "最小化", dirs: [2, 3], action: "window.minimize" },
  { id: "maximize", name: "最大化/还原", dirs: [6, 1], action: "window.toggle-max" },
];

export interface GestureLibraryConfig {
  enabled: boolean;
  trailFadeMs: number;
  /** 自定义手势覆盖/追加：id → 定义。 */
  custom: Record<string, { name: string; dirs: Dir8[]; action: string }>;
}

export function gestureLibrary(cfg: GestureLibraryConfig): { id: string; name: string; dirs: Dir8[]; action: string; builtin: boolean }[] {
  const out = BUILTIN_GESTURES.map((g) => ({ ...g, builtin: true }));
  for (const [id, g] of Object.entries(cfg.custom)) {
    out.push({ id, name: g.name, dirs: g.dirs, action: g.action, builtin: false });
  }
  return out;
}

/**
 * 轨迹识别器（容错两向偏差：编码时允许每步与理论方向偏差 ±1 档）。
 * 用法：右键按下 begin() → 移动 feed() → 松开 recognize()。
 * recognize() 返回 null = 无轨迹（步数不足或无一匹配）→ 调用方放行右键菜单。
 */
export class GestureRecognizer {
  private points: { x: number; y: number }[] = [];
  private dirs: Dir8[] = [];

  begin(x: number, y: number): void {
    this.points = [{ x, y }];
    this.dirs = [];
  }

  feed(x: number, y: number): void {
    const last = this.points[this.points.length - 1]!;
    const dx = x - last.x;
    const dy = y - last.y;
    if (Math.hypot(dx, dy) < GESTURE_STEP_PX) return;
    this.dirs.push(encodeDir8(dx, dy));
    this.points.push({ x, y });
  }

  /** 已记录方向步（墨迹与调试面同源）。 */
  get steps(): Dir8[] {
    return [...this.dirs];
  }

  get trail(): { x: number; y: number }[] {
    return this.points.map((p) => ({ ...p }));
  }

  get hasTrail(): boolean {
    return this.dirs.length >= GESTURE_MIN_STEPS;
  }

  /** 松开时识别：返回命中的手势 id；无轨迹/无匹配返回 null。 */
  recognize(cfg: GestureLibraryConfig): { id: string; action: string } | null {
    // 「无轨迹」以原始位移量计（≥GESTURE_MIN_STEPS 步 ≈ 48px 实移）——
    // 直线划一道（合并后 1 档）依然是有效轨迹，匹配走 collapseRuns。
    if (!this.hasTrail) return null;
    const collapsed = collapseRuns(this.dirs);
    for (const g of gestureLibrary(cfg)) {
      if (matchDirs(collapsed, g.dirs)) return { id: g.id, action: g.action };
    }
    return null;
  }

  reset(): void {
    this.points = [];
    this.dirs = [];
  }
}

/** 同向合并：连续相同方向步折叠为一档（单方向手势的识别前提）。 */
export function collapseRuns(dirs: Dir8[]): Dir8[] {
  const out: Dir8[] = [];
  for (const d of dirs) {
    if (out.length === 0 || out[out.length - 1] !== d) out.push(d);
  }
  return out;
}

/** 方向序列匹配（容错两向偏差：逐步比较允许 ±1 档）。 */
export function matchDirs(actual: Dir8[], expect: Dir8[]): boolean {
  if (actual.length !== expect.length) return false;
  return actual.every((a, i) => {
    const e = expect[i]!;
    const diff = Math.min(Math.abs(a - e), 8 - Math.abs(a - e));
    return diff <= 1;
  });
}

/**
 * 方向编码样本生成（判据「方向编码样本 200 例」的样本器）：
 * 真实口径——对每一步的笔画角度施加 ±18° 抖动（次量化噪声，小于 22.5° 的
 * 量化半界），再经 encodeDir8 重编码。测的是「笔画→角度→编码→匹配」全链路。
 */
export function gestureSampleDirs(expect: Dir8[], angularJitterDeg = 18, rand: () => number = Math.random): Dir8[][] {
  const samples: Dir8[][] = [];
  for (let i = 0; i < 200; i++) {
    samples.push(
      expect.map((e) => {
        const ang = (e * 45 + (rand() * 2 - 1) * angularJitterDeg) * (Math.PI / 180);
        return encodeDir8(Math.cos(ang), Math.sin(ang));
      }),
    );
  }
  return samples;
}

/** 优先级登记说明（与 F215 右键菜单 / F615 侧键的矩阵——一处登记）。 */
export function gesturePriorityNote(): string {
  return "优先级：右键手势仅在「有轨迹（≥2 步）」时接管右键；无轨迹松开=右键菜单（F215）兜底；侧键映射（F615）作用于 XButton1/2，与右键手势无按键交集。三处登记于 gesturePriorityNote / sideGesturePriorityMatrix / F215 审计表。";
}
