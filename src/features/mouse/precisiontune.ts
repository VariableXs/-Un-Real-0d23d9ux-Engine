/**
 * J 鼠标域 · F602 慢速微调模式 · 纵深引擎（批次七）。
 *
 * v4 已有「激活即精修徽标跟指针」的 HUD 面。本引擎补三块真实纵深：
 *
 * 1. 减速坡道——精密模式不是开关式的瞬降（瞬降 = 指针原地跳一截），
 *    进出都走 120ms 指数坡道：进入时灵敏度从 1x 平滑降到精密系数，
 *    退出平滑升回。坡道期间 HUD 徽标透明度同步渐变（视觉与手感同一节奏）。
 *
 * 2. 键盘微调——精密模式激活时方向键 = 1px 步进、Shift+方向 = 5px、
 *    Home/End = 吸附到水平/垂直轴（像素级对齐的真用途：截图框选、
 *    矢量锚点）。键盘步进与指针移动共用同一坐标信道（对等性纪律：
 *    鼠标能到的键盘也能到）。
 *
 * 3. 粘滞精密——状态机：pending → active → cooldown。三连击精密键进入
 *    粘滞（释放按键不退出，再按一次退出）；cooldown 期内忽略重触发
 *    （防抖：双击精密键的第二次不产生「进-出-进」三连跳变）。
 *
 * 判据锚点：
 * - 减速坡道 120ms 端点精确（t=0 → 1x，t≥120ms → 精密系数）→ rampFactor()
 * - 键盘步进三档（1px/5px/轴吸附）→ nudgeStep() / nudge()
 * - 粘滞状态机与 cooldown 防抖 → PrecisionTune.feed()
 * - 与 F601 增益曲线正交（精密系数是乘法末级）→ composedGain()
 */

/* ------------------------------- 减速坡道 ------------------------------- */

/** 坡道时长（ms）——进出共用（对称性：退出不应比进入更突兀）。 */
export const PRECISION_RAMP_MS = 120;

/**
 * 坡道系数：tMs ∈ [0, RAMP] 内插 1x → precision，指数缓动
 * （easeOutCubic 口径——与 v5 physics 缓动族同一格律）。
 * 进入方向 enter=true：1 → precision；退出反向。
 */
export function rampFactor(tMs: number, precision: number, enter: boolean): number {
  const p = Math.max(0.02, Math.min(1, precision)); // 精密系数下限 2%（再低不可用）
  const t = Math.min(1, Math.max(0, tMs) / PRECISION_RAMP_MS);
  const eased = 1 - (1 - t) ** 3;
  return enter ? 1 + (p - 1) * eased : p + (1 - p) * eased;
}

/**
 * 精密系数档位表（四档对拍 F610 旋钮格律）。
 * 0.05 = 5% —— 20px 手腕位移出 1px 屏上位移，矢量锚点级。
 */
export const PRECISION_LEVELS = [0.05, 0.1, 0.2, 0.35] as const;
export type PrecisionLevel = (typeof PRECISION_LEVELS)[number];

/* ------------------------------- 键盘微调 ------------------------------- */

/**
 * 步进档：裸方向键 1px、Shift 5px、Ctrl 10px（三档够用，不做连续变档——
 * 步进档位必须可预期，这是键盘微调与指针滑动的本质区别）。
 */
export function nudgeStep(mod: { shift?: boolean; ctrl?: boolean }): number {
  if (mod.ctrl) return 10;
  if (mod.shift) return 5;
  return 1;
}

export type NudgeKey = "up" | "down" | "left" | "right" | "home" | "end";

/**
 * 键盘微调执行器：当前坐标 + 键位 → 新坐标。
 * Home 吸附垂直轴（x=0 对齐）、End 吸附水平轴（y=0 对齐）——吸附语义是
 * 「对齐到轴」而非「跳到原点」；步进钳制非负（屏幕坐标不越负）。
 */
export function nudge(cur: { x: number; y: number }, key: NudgeKey, mod: { shift?: boolean; ctrl?: boolean }): { x: number; y: number } {
  const s = nudgeStep(mod);
  switch (key) {
    case "left":
      return { x: Math.max(0, cur.x - s), y: cur.y };
    case "right":
      return { x: cur.x + s, y: cur.y };
    case "up":
      return { x: cur.x, y: Math.max(0, cur.y - s) };
    case "down":
      return { x: cur.x, y: cur.y + s };
    case "home":
      return { x: 0, y: cur.y };
    case "end":
      return { x: cur.x, y: 0 };
  }
}

/* ------------------------------- 粘滞精密状态机 ------------------------------- */

export type PrecisionPhase = "idle" | "active" | "sticky" | "cooldown";

/** cooldown 时长（ms）——防抖窗：双击精密键不产生三连跳变。 */
export const PRECISION_COOLDOWN_MS = 260;

export interface PrecisionEvent {
  kind: "tap" | "hold" | "release" | "tripleTap";
  atMs: number;
}

export interface PrecisionState {
  phase: PrecisionPhase;
  /** 进入 active/sticky 的时刻（坡道 t 计算与 HUD 渐变共用）。 */
  sinceMs: number | null;
  /** 粘滞激活计数（面板显示「已粘滞 n 次」）。 */
  stickyCount: number;
}

/**
 * 精密模式状态机（纯函数核，宿主侧持状态）。
 *
 * 转移表：
 * - idle + tap → active（普通按压触发）
 * - active + release → cooldown（松开退出，走坡道）
 * - active + tripleTap → sticky（三连击粘滞）
 * - sticky + tap → cooldown（粘滞中再按一次退出）
 * - cooldown + (t < cooldown) 任意 → 忽略（防抖）
 * - cooldown + (t ≥ cooldown) + tap → active
 *
 * hold 事件在 active 中是维持（无转移）；release 在 sticky 中无转移
 * （粘滞的定义就是无视 release）。
 */
export function transitionPrecision(st: PrecisionState, ev: PrecisionEvent): PrecisionState {
  const now = ev.atMs;
  switch (st.phase) {
    case "idle":
      if (ev.kind === "tap" || ev.kind === "hold") return { phase: "active", sinceMs: now, stickyCount: st.stickyCount };
      return st;
    case "active":
      if (ev.kind === "tripleTap") return { phase: "sticky", sinceMs: now, stickyCount: st.stickyCount + 1 };
      if (ev.kind === "release") return { phase: "cooldown", sinceMs: now, stickyCount: st.stickyCount };
      return st;
    case "sticky":
      if (ev.kind === "tap") return { phase: "cooldown", sinceMs: now, stickyCount: st.stickyCount };
      return st; // release 被粘滞吞掉——定义如此
    case "cooldown":
      if (now - (st.sinceMs ?? now) < PRECISION_COOLDOWN_MS) return st;
      if (ev.kind === "tap" || ev.kind === "hold") return { phase: "active", sinceMs: now, stickyCount: st.stickyCount };
      return { phase: "idle", sinceMs: null, stickyCount: st.stickyCount };
  }
}

/** 粘滞期是否吃 release（HUD 提示「已粘滞——再按一次退出」的依据）。 */
export function swallowsRelease(phase: PrecisionPhase): boolean {
  return phase === "sticky";
}

/* ------------------------------- 与 F601 正交合成 ------------------------------- */

/**
 * 合成增益：F601 曲线增益 × 精密坡道系数。
 * 正交性承诺：precision=1 时逐位等于原曲线（乘法幺元）；曲线更换不感知
 * 精密层（两层各自独立测试——二维正交矩阵纪律，与 F614/F616 同格律）。
 */
export function composedGain(baseGain: number, ramp: number): number {
  return baseGain * ramp;
}
