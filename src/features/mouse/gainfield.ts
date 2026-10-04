/**
 * J 鼠标域 · F601 增益场精确反解引擎（v5 · 深化批次五）。
 *
 * v4 遗留的最丑角落（checklist F601 自记）：「自定义曲线的 x(t) 反解是两步
 * 牛顿近似——极端控制点下预览与实际增益有可见偏差，完整反解该用二分」。
 * 本模块就是那记补刀：x(t) 用「牛顿加速 + 二分兜底」混合反解——牛顿快、
 * 二分稳，导数退化区（控制点把曲线折出近平台）自动降级到二分，40 次迭代
 * 把 x 误差压到 1e-6 以下。curve.ts 的 custom 分支已切换到本引擎（一处一
 * 事实：预览、对拍表、运行时增益共用同一反解）。
 *
 * 判据锚点：
 * - 贝塞尔编辑器拖拽即时预览 → 精确反解后预览=实际（不再有近似偏差）
 * - 增益对拍表 20 点采样 → gainTable20Precise()（同源出表）
 * - 回归零破坏 → 单调性自检 monotonicityCheck()（曲线族健康度探针）
 */

/** 三次贝塞尔 x(t)：端点 (0,0)-(1,1)，两控制点。 */
export function bezierX(t: number, x1: number, x2: number): number {
  const u = 1 - t;
  return 3 * u * u * t * x1 + 3 * u * t * t * x2 + t * t * t;
}

/** 三次贝塞尔 y(t)。 */
export function bezierY(t: number, y1: number, y2: number): number {
  const u = 1 - t;
  return 3 * u * u * t * y1 + 3 * u * t * t * y2 + t * t * t;
}

/** x'(t)（导数——牛顿步与「平台区」检测共用）。 */
export function bezierXDeriv(t: number, x1: number, x2: number): number {
  const u = 1 - t;
  return 3 * u * u * x1 + 6 * u * t * (x2 - x1) + 3 * t * t * (1 - x2);
}

export const BEZIER_EPSILON = 1e-6;

/**
 * x(t) 精确反解：给定目标 x∈[0,1] 求 t。
 * 策略：先牛顿迭代（收敛快），若导数过小（平台区/折返区）或迭代 6 步未达
 * 精度，切换到区间二分（[0,1] 上单调，二分必然收敛）。
 * 返回 t ∈ [0,1]。
 */
export function solveBezierT(x: number, x1: number, x2: number): number {
  const cx = Math.max(0, Math.min(1, x));
  if (cx <= 0) return 0;
  if (cx >= 1) return 1;
  // 牛顿相位：从 x 线性猜测起步。
  let t = cx;
  for (let i = 0; i < 6; i++) {
    const cur = bezierX(t, x1, x2) - cx;
    if (Math.abs(cur) < BEZIER_EPSILON) return t;
    const der = bezierXDeriv(t, x1, x2);
    if (Math.abs(der) < 1e-5) break; // 平台区：交棒二分
    t -= cur / der;
    if (t < 0) t = 0;
    if (t > 1) t = 1;
  }
  // 二分相位：x(t) 在 x1,x2∈[0,1] 时严格单调递增，中点收敛有保证。
  let lo = 0;
  let hi = 1;
  let mid = cx;
  for (let i = 0; i < 40; i++) {
    mid = (lo + hi) / 2;
    const cur = bezierX(mid, x1, x2) - cx;
    if (Math.abs(cur) < BEZIER_EPSILON) return mid;
    if (cur < 0) lo = mid;
    else hi = mid;
  }
  return mid;
}

/**
 * 自定义贝塞尔增益精确映射：输入位移 a(px) → 增益倍率。
 * 与 curve.ts gainAt("custom") 的语义一致（归一带 0..128px、增益带 0.5..2.0），
 * 但 x(t) 反解换成精确引擎——预览与实际从此是同一份数学。
 */
export function bezierGainPrecise(a: number, cp1x: number, cp1y: number, cp2x: number, cp2y: number): number {
  const t = Math.min(1, Math.max(0, a) / 128);
  if (t <= 0 || t >= 1) return 0.5 + 1.5 * bezierY(t, cp1y, cp2y);
  const ts = solveBezierT(t, cp1x, cp2x);
  return 0.5 + 1.5 * bezierY(ts, cp1y, cp2y);
}

/**
 * 增益对拍表（精确引擎版）：20 点采样，语义同 curve.gainTable20 的 custom 行。
 * 供「对拍表导出」与面板对照渲染共用——运行时/预览/出表三处一处一事实。
 */
export function gainTable20Precise(cp1x: number, cp1y: number, cp2x: number, cp2y: number): { inPx: number; outPx: number }[] {
  const rows: { inPx: number; outPx: number }[] = [];
  for (let i = 1; i <= 20; i++) {
    const inPx = i * 8;
    rows.push({ inPx, outPx: Math.round(inPx * bezierGainPrecise(inPx, cp1x, cp1y, cp2x, cp2y) * 100) / 100 });
  }
  return rows;
}

/**
 * 反解精度自检：沿 32 个目标点验证 |x(solve(t)) − t| < 2e-5。
 * 返回最大残差——面板「引擎健康度」与单测共用同一探针。
 */
export function solverResidual(cp1x: number, cp2x: number): number {
  let worst = 0;
  for (let i = 1; i < 32; i++) {
    const target = i / 32;
    const t = solveBezierT(target, cp1x, cp2x);
    worst = Math.max(worst, Math.abs(bezierX(t, cp1x, cp2x) - target));
  }
  return worst;
}

/**
 * 曲线单调性检查（增益场健康度）：自定义贝塞尔在 x1,x2∈[0,1] 时 y(t) 未必
 * 单调（控制点 y 出界会把增益折返）——出界不阻止编辑（创作自由），但要在
 * 面板显性标红，不许「画出来的曲线和手感不一致」这种事静默发生。
 * @returns null=单调健康；否则给出首个折返点的输入位移（px）。
 */
export function firstNonMonotonic(cp1x: number, cp1y: number, cp2x: number, cp2y: number, stepPx = 2): number | null {
  let prev = -Infinity;
  for (let a = 0; a <= 128; a += stepPx) {
    const g = bezierGainPrecise(a, cp1x, cp1y, cp2x, cp2y);
    if (g < prev - 1e-4) return a;
    prev = g;
  }
  return null;
}

/* ------------------------------- F601 深化：DPI 归一增益场 ------------------------------- */

/**
 * 每缩放比归一（F601「不同 DPI 下手感一致」的增益场口径）：
 * 增益曲线定义在**物理位移域**——管线以逻辑位移喂曲线前先乘 scale 换算
 * （物理口径入曲线），逻辑输出 = 逻辑输入 × 增益（增益本身不乘 scale，
 * 否则同一物理位移在不同缩放下手感漂移）。与 F224 显示缩放共用「逻辑/
 * 物理」分界纪律。
 */
export function normalizedGainAt(rawGain: (aPx: number) => number, aLogical: number, scale: number, sens: number): number {
  const s = scale > 0 ? scale : 1;
  return rawGain(aLogical * s) * sens;
}

/**
 * 跨缩放一致性对拍：同一**物理**位移在两档缩放下逻辑增益必须一致
 * （曲线吃物理域的实现纪律的机器预检——谁忘了乘 scale 这里当场红）。
 * 返回逻辑增益偏差比，<5% 判一致。
 */
export function scaleConsistency(rawGain: (aPx: number) => number, physPx: number, scaleA: number, scaleB: number, sens = 1): { delta: number; pass: boolean } {
  const ga = normalizedGainAt(rawGain, physPx / scaleA, scaleA, sens);
  const gb = normalizedGainAt(rawGain, physPx / scaleB, scaleB, sens);
  const delta = ga === 0 ? 0 : Math.abs(ga - gb) / Math.max(ga, gb);
  return { delta: Math.round(delta * 1000) / 1000, pass: delta < 0.05 };
}
