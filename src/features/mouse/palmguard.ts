/**
 * J 鼠标域 · F603 抬笔滤波 · 纵深引擎（批次七）。
 *
 * v3 的 LiftFilter 解决「抬笔落笔的位移毛刺」（时域滤波）。本引擎补
 * 分类器层——并非所有异常位移都是抬笔毛刺，还有两类敌人：
 *
 * 1. 手掌误触（palm rejection）——数位板/触屏上，手掌边缘搭在感应面
 *    上产生的宽接触大面积低速位移。特征签名：接触半径大 + 速度低 +
 *    压感浅（数位板）/ 接触面积大（触屏）。误判代价：真手写被掐断。
 *
 * 2. 落笔弹跳（touchdown bounce）——压感笔落笔瞬间 1-2 帧的压力过冲
 *    产生 3-8px 的弹跳位移。特征：发生在接触建立后 30ms 内 + 方向反转。
 *
 * 分类器输出三态：clean（正常移动）/ reject（丢弃事件）/ suspect
 * （可疑——交给上层的抬笔滤波进一步平滑）。三态而不是两态，因为
 * 「丢弃」和「平滑」是两种不同的处置，错处置比不处置更糟。
 *
 * 判据锚点：
 * - 宽接触低速签名 → classifyContact()
 * - 落笔 30ms 弹跳窗口 → BOUNCE_WINDOW_MS
 * - suspect 不静默丢弃（交给平滑层）→ 三态类型级成立
 * - 冷却期状态机（误判恢复需连续 3 帧 clean）→ PalmGuard.feed()
 */

/* ------------------------------- 接触签名 ------------------------------- */

/** 单帧接触采样（触屏/数位板事件归一化口；鼠标域可选提供）。 */
export interface ContactSample {
  /** 接触半径 px（触屏=触点等效圆；数位板=笔尖宽度；鼠标恒 0）。 */
  radiusPx: number;
  /** 速度 px/ms（帧间位移/帧时长）。 */
  speedPxMs: number;
  /** 压感 0-1（无压感设备恒 1）。 */
  pressure: number;
  /** 距接触建立 ms（落笔弹跳窗口判据用）。 */
  sinceTouchMs: number;
  /** 本帧位移方向与上帧夹角弧度（0=同向；π=反转）。 */
  angleDelta: number;
}

export type ContactVerdict = "clean" | "reject" | "suspect";

/** 落笔弹跳观察窗（ms）——窗内反转位移按弹跳处置。 */
export const BOUNCE_WINDOW_MS = 30;
/** 手掌签名：接触半径阈值（px）——超出即进入手掌签名评估。 */
export const PALM_RADIUS_PX = 14;
/** 手掌签名：速度上限（px/ms）——手掌拖动是低速的。 */
export const PALM_SPEED_MAX = 0.35;

/**
 * 单帧分类（纯函数）：
 * - 半径 ≥14px 且速度 ≤0.35px/ms 且压感 <0.5 → 手掌签名，reject；
 * - 半径 ≥14px 但速度高或压感实 → suspect（大接触但行为像真笔画——
 *   不敢直接丢，交给平滑）；
 * - 落笔 30ms 内方向反转 ≥120° → 弹跳，reject；
 * - 其余 clean。
 */
export function classifyContact(s: ContactSample): ContactVerdict {
  const palmSignature = s.radiusPx >= PALM_RADIUS_PX && s.speedPxMs <= PALM_SPEED_MAX && s.pressure < 0.5;
  if (palmSignature) return "reject";
  const bigButLively = s.radiusPx >= PALM_RADIUS_PX && (s.speedPxMs > PALM_SPEED_MAX || s.pressure >= 0.5);
  if (bigButLively) return "suspect";
  if (s.sinceTouchMs <= BOUNCE_WINDOW_MS && s.angleDelta >= (120 * Math.PI) / 180 && s.speedPxMs > 0.05) return "reject";
  return "clean";
}

/* ------------------------------- 带冷却的守门状态机 ------------------------------- */

/** 误判恢复：连续 CLEAN_STREAK 帧 clean 才解除 reject 态（防间歇抖动）。 */
export const CLEAN_STREAK = 3;

export interface GuardState {
  rejecting: boolean;
  cleanRun: number;
  /** 累计丢弃帧数（遥测口径——面板显示「手掌守门拦下 n 帧」）。 */
  rejectedFrames: number;
  /** 最近一帧判据（可观测性：被丢弃的帧在日志里留什么理由）。 */
  lastReason: string | null;
}

export const initialGuardState = (): GuardState => ({ rejecting: false, cleanRun: 0, rejectedFrames: 0, lastReason: null });

/**
 * 守门状态机推进：
 * - reject 态中一切事件照判：clean 计数、reject 续期；clean 连续
 *   CLEAN_STREAK 帧解除；
 * - 非 reject 态收到 reject → 进入 reject 态并记理由；
 * - suspect 永不改变状态机相位（它只是「交给滤波」的标记——状态机
 *   只对 reject 负责，一处一事实）。
 */
export function feedGuard(st: GuardState, s: ContactSample): { verdict: ContactVerdict; next: GuardState } {
  const v = classifyContact(s);
  if (v === "reject") {
    return {
      verdict: "reject",
      next: {
        rejecting: true,
        cleanRun: 0,
        rejectedFrames: st.rejectedFrames + 1,
        lastReason: s.radiusPx >= PALM_RADIUS_PX ? "palm-signature" : "touchdown-bounce",
      },
    };
  }
  if (v === "suspect") return { verdict: "suspect", next: { ...st, lastReason: "suspect→smooth" } };
  // clean
  if (st.rejecting) {
    const run = st.cleanRun + 1;
    if (run >= CLEAN_STREAK) {
      return { verdict: "clean", next: { rejecting: false, cleanRun: 0, rejectedFrames: st.rejectedFrames, lastReason: "recovered" } };
    }
    return { verdict: "clean", next: { ...st, cleanRun: run, lastReason: `recovering ${run}/${CLEAN_STREAK}` } };
  }
  return { verdict: "clean", next: { ...st, cleanRun: 0, lastReason: null } };
}

/** 面板摘要（遥测字段同一来源——对账同源纪律）。 */
export function guardSummary(st: GuardState): string {
  if (st.rejecting) return `守门中（${st.lastReason} · 已拦 ${st.rejectedFrames} 帧）`;
  if (st.rejectedFrames > 0) return `空闲（历史拦截 ${st.rejectedFrames} 帧 · 最近 ${st.lastReason ?? "—"}）`;
  return "空闲（无拦截记录）";
}
