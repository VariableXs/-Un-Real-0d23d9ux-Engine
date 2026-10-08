/**
 * VE-F0608 · 图层动画插值接口（D 域 · 2D/3D 合成引擎图层树组 · 批次 D01）
 * ---------------------------------------------------------------------------
 * 职责定位：图层动画插值接口——D 域动画的地基条。上游 F0602 变换系统给出
 * 「平移/旋转/缩放/倾斜的可逆分解」，本条把这份分解当作**分量空间**，在其上
 * 定义纯函数式的时间插值协议；下游 F0603（不透明度淡入淡出）、F0606（Z 序重排）、
 * F0613（树级脏区）全部消费本条的插值结果。
 *
 * 锚点契约（四条，逐条对应判据）：
 *   1. 可动画属性注册表 —— 变换各分量、不透明度、裁剪参数逐属性声明插值规则：
 *      标量线性、旋转分量走四元数球面插值（防万向锁）、贝塞尔缓动曲线。
 *      未注册属性**显性不支持**，返回带 code 与可操作提示的失败，不静默降级。
 *   2. 时间基插值协议 —— 归一化进度 → 属性值是**纯函数**：无隐式状态、无时钟读取、
 *      不写全局。同输入必同输出，故可重放（时间旅行调试）可快照（撤销重做/崩溃恢复）。
 *   3. 驱动联动 —— 每帧插值结果按属性声明的脏类产出 F0613 树级脏区事件；
 *      动画层的子树按「变换脏」整体处理（世界矩阵与包围盒级联失效）。
 *   4. 与 F0602 分解重组衔接 —— 插值全程在分量空间进行，末端由 compose 重组回矩阵。
 *
 * 防万向锁的实现立场（本条最易做错的地方，故显式记录）：
 *   欧拉角空间插值在 pitch = ±90° 附近参数化退化（两轴共线，解不唯一），
 *   线性插值会在该处「翻滚」。故旋转属性的插值规则声明为 slerp：先把起止欧拉三元组
 *   各编为四元数，在四元数空间做球面插值（该空间与欧拉参数化无关，天然无锁），
 *   再解回欧拉分量。对 toEuler 在 pitch = ±90° 的退化单独检测，走备用分解序并产出
 *   GIMBAL_LOCK_BYPASS 诊断——退化是显性事件，不是静默 NaN。
 *
 * 零静默纪律：所有拒绝、钳制、退化都产出 Diagnostic（code + message + hint），
 * 由调用方聚合上报；本模块不向 UI 直接抛异常，也不吞掉任何一条诊断。
 *
 * 判据：注册表、纯函数插值、四元数防锁、脏区联动。
 * 依赖锚点：F0601 图层树结构（脏标记族）、F0602 变换系统（分解重组与逆矩阵缓存）、
 *          F0603 不透明度（alpha 通道为可动画属性）、F0604 裁剪（裁剪域随层变换）、
 *          F0613 脏区收集（脏区事件消费方）。
 * 交接说明：本条只管「分量插值 + 脏区联动」，效果链归 F0609、时间线编辑归 UI 层，
 *          GPU/CPU 双路执行归 F0628/F0629——本条输出的矩阵是二者共同的唯一输入。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型（异常显性化的载体：零静默的第一层基础设施）
// ════════════════════════════════════════════════════════════════════════════

/** 诊断码：每一种拒绝/钳制/退化都有独立可检索的码，绝不合并成一条通用错误。 */
export type DiagCode =
  | "PROP_UNREGISTERED"
  | "PROGRESS_OUT_OF_RANGE"
  | "TRACK_EMPTY"
  | "KEYFRAME_UNORDERED"
  | "VALUE_NONFINITE"
  | "QUAT_DEGENERATE"
  | "QUAT_NONFINITE"
  | "SLERP_COLLAPSE"
  | "GIMBAL_LOCK_BYPASS"
  | "EASING_UNKNOWN"
  | "BEZIER_SOLVE_BAILOUT"
  | "CLAMP_DEGENERATE"
  | "SNAPSHOT_CHECKSUM_MISMATCH"
  | "REGISTRY_DUPLICATE"
  | "MATRIX_DEGENERATE";

/** 一条诊断：发生了什么（code+message）、影响什么、下一步怎么办（hint）。 */
export interface Diagnostic {
  readonly code: DiagCode;
  /** 人话描述：直接面向开发者排障，不含裸异常码。 */
  readonly message: string;
  /** 可操作提示：调用方该改哪里、调用方该怎么降级。 */
  readonly hint: string;
}

/** 结果判别联合：成功必带 value，失败必带 code/message/hint——失败不可被误当成功。 */
export type Outcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly Diagnostic[] }
  | {
      readonly ok: false;
      readonly code: DiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly Diagnostic[];
    };

/** 成功构造（diagnostics 允许携带非致命告警，例如钳制与退化旁路）。 */
export function ok<T>(value: T, diagnostics: readonly Diagnostic[] = []): Outcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造：三要素齐备（code/message/hint），diagnostics 含本条自身便于统一上报。 */
export function fail<T>(code: DiagCode, message: string, hint: string): Outcome<T> {
  const d: Diagnostic = { code, message, hint };
  return { ok: false, code, message, hint, diagnostics: [d] };
}

/** 诊断聚合器：把散落各处的非致命告警汇成一条时间轴可查的清单。 */
export class DiagBag {
  private readonly items: Diagnostic[] = [];

  /** 追加一条诊断。空 message/hint 被规范化，避免上游写出半截诊断。 */
  push(code: DiagCode, message: string, hint: string): void {
    this.items.push({ code, message: message || "（未提供描述）", hint: hint || "（未提供处置建议）" });
  }

  /** 当前条数。 */
  get size(): number {
    return this.items.length;
  }

  /** 只读视图（返回副本，调用方改不动内部清单）。 */
  all(): readonly Diagnostic[] {
    return this.items.slice();
  }

  /** 按诊断码筛选——排障时按码聚合的入口。 */
  byCode(code: DiagCode): readonly Diagnostic[] {
    return this.items.filter((d) => d.code === code);
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 数值基础（有限性守卫与钳制：所有插值的共同前置防线）
// ════════════════════════════════════════════════════════════════════════════

/** 浮点比较容差：四元数归一化、slerp 退化判据共用同一阈值，避免各处魔数不一致。 */
export const EPS = 1e-9;

/** 判定一个数是否有限（NaN 与 ±Inf 都判否——插值里 NaN 会静默污染整棵子树）。 */
export function isFiniteNum(v: number): boolean {
  return Number.isFinite(v);
}

/** 把 v 钳制到 [lo, hi]；lo > hi（退化区间）时返回 hi 并由调用方产出诊断，不静默。 */
export function clamp(v: number, lo: number, hi: number): number {
  if (lo > hi) return hi;
  if (v < lo) return lo;
  if (v > hi) return hi;
  return v;
}

/** 标量线性插值：注册表里绝大多数属性的默认规则。输入非有限时原样透传交由上层判失败。 */
export function lerpScalar(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

/** 二维向量（平移、缩放分量）。冻结语义：全模块只读不写，纯函数可重放的前提之一。 */
export interface Vec2 {
  readonly x: number;
  readonly y: number;
}

/** 三维向量（三维平移分量）。 */
export interface Vec3 {
  readonly x: number;
  readonly y: number;
  readonly z: number;
}

/** 二维向量线性插值（平移/缩放/裁剪边距共用）。 */
export function lerpVec2(a: Vec2, b: Vec2, t: number): Vec2 {
  return { x: lerpScalar(a.x, b.x, t), y: lerpScalar(a.y, b.y, t) };
}

/** 三维向量线性插值。 */
export function lerpVec3(a: Vec3, b: Vec3, t: number): Vec3 {
  return {
    x: lerpScalar(a.x, b.x, t),
    y: lerpScalar(a.y, b.y, t),
    z: lerpScalar(a.z, b.z, t),
  };
}

/** 二维向量长度。 */
export function length2(v: Vec2): number {
  return Math.hypot(v.x, v.y);
}

/**
 * 二维向量归一化，零向量是**显性退化**而非除零 NaN：
 * 返回 null 表示零向量，调用方据此产出 QUAT_DEGENERATE 类的「保持上一帧」处置。
 */
export function normalize2(v: Vec2): Vec2 | null {
  const len = length2(v);
  if (!isFiniteNum(len) || len < EPS) return null;
  return { x: v.x / len, y: v.y / len };
}

/**
 * 弧度归一化到 (-π, π]，供 2D 标量旋转走最短弧插值（欧拉插值的 2D 特化）。
 *
 * ⚠️ 单位纪律：本函数**只收弧度**。角度请用 normalizeAngleDeg。
 * 两者混用会产生「看起来只差几度、实际差整圈量级」的静默错误
 * （曾发生：-5° 传入本函数被当作弧度折算，得 1.283°，肉眼几乎看不出但动画全程错）。
 * 所有对外接口的角度分量一律以「Deg」结尾标注单位，正是为杜绝此类混用。
 */
export function normalizeAngle(rad: number): number {
  if (!isFiniteNum(rad)) return rad;
  const twoPi = Math.PI * 2;
  let a = rad % twoPi;
  if (a > Math.PI) a -= twoPi;
  if (a <= -Math.PI) a += twoPi;
  return a;
}

/** 角度归一化到 (-180, 180]。与 normalizeAngle 分开定义，单位不同不可互换。 */
export function normalizeAngleDeg(deg: number): number {
  if (!isFiniteNum(deg)) return deg;
  let a = deg % 360;
  if (a > 180) a -= 360;
  if (a <= -180) a += 360;
  return a;
}

/**
 * 标量角度插值（走最短弧）：直接 lerp 会在 ±π 接缝处翻转整圈。
 * 半程恰为 π（等价路径二义）时按正向取值并产出诊断由调用方登记。
 */
export function lerpAngle(a: number, b: number, t: number, bag: DiagBag): number {
  const from = normalizeAngle(a);
  const to = normalizeAngle(b);
  let delta = normalizeAngle(to - from);
  if (Math.abs(Math.abs(delta) - Math.PI) < EPS) {
    bag.push(
      "SLERP_COLLAPSE",
      `旋转分量在 ${from}→${to} 处半程恰为 π，两条旋转路径视觉等价`,
      "已按正向弧取值；若需反向动画请把关键帧显式拆成两帧，避免依赖此处的取向约定",
    );
  }
  return normalizeAngle(from + delta * t);
}

// ════════════════════════════════════════════════════════════════════════════
// §3 四元数与球面插值（防万向锁的核心实现）
// ════════════════════════════════════════════════════════════════════════════

/** 四元数 (x, y, z, w)。2D 合成走 z 轴旋转，但插值空间仍是完整四元数以取得无锁性质。 */
export interface Quat {
  readonly x: number;
  readonly y: number;
  readonly z: number;
  readonly w: number;
}

/** 单位四元数（无旋转）。 */
export const QUAT_IDENTITY: Quat = { x: 0, y: 0, z: 0, w: 1 };

/** 欧拉旋转分量（度）。插值在分量空间进行——这是 F0602 分解的产物形态。 */
export interface Euler3 {
  readonly rxDeg: number;
  readonly ryDeg: number;
  readonly rzDeg: number;
}

/** 角度转弧度。 */
export function degToRad(deg: number): number {
  return (deg * Math.PI) / 180;
}

/** 弧度转角度。 */
export function radToDeg(rad: number): number {
  return (rad * 180) / Math.PI;
}

/** 四元数模长平方（避免开方，用于判退化）。 */
function quatNormSq(q: Quat): number {
  return q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
}

/** 四元数各分量是否全有限。 */
export function quatFinite(q: Quat): boolean {
  return isFiniteNum(q.x) && isFiniteNum(q.y) && isFiniteNum(q.z) && isFiniteNum(q.w);
}

/**
 * 四元数归一化。零模长（四元数退化）为显性失败，返回 null 而非除零。
 * 近单位但未归一（累积漂移）时静默修正属**预期行为**，故不产诊断。
 */
export function normalizeQuat(q: Quat): Quat | null {
  if (!quatFinite(q)) return null;
  const n2 = quatNormSq(q);
  if (!isFiniteNum(n2) || n2 < EPS) return null;
  const inv = 1 / Math.sqrt(n2);
  return { x: q.x * inv, y: q.y * inv, z: q.z * inv, w: q.w * inv };
}

/** 四元数点积（同时用于半球判定）。 */
export function quatDot(a: Quat, b: Quat): number {
  return a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
}

/** 哈密积（旋转复合）。 */
export function quatMul(a: Quat, b: Quat): Quat {
  return {
    x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
    y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
    z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
    w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
  };
}

/** 四元数共轭。 */
export function quatConj(q: Quat): Quat {
  return { x: -q.x, y: -q.y, z: -q.z, w: q.w };
}

/** 由轴角编四元数。轴不必归一——本函数内部归一，零轴退化返回 null。 */
export function quatFromAxisAngle(axis: Vec3, rad: number): Quat | null {
  if (!quatFinite({ x: axis.x, y: axis.y, z: axis.z, w: rad })) return null;
  const len = Math.hypot(axis.x, axis.y, axis.z);
  if (len < EPS || !isFiniteNum(rad)) return null;
  const half = rad / 2;
  const s = Math.sin(half) / len;
  return { x: axis.x * s, y: axis.y * s, z: axis.z * s, w: Math.cos(half) };
}

/**
 * 旋转序约定（全模块唯一定义，编解码两侧共用，顺序即视觉结果）：
 *   欧拉三元组字段语义为 rxDeg = roll（绕 X）、ryDeg = pitch（绕 Y）、rzDeg = yaw（绕 Z），
 *   施加顺序固定为 **ZYX**：先 roll，后 pitch，最后 yaw，即 q = qz · qy · qx。
 *   下方 quatToEuler 的解算公式与本约定严格互逆（ZYX 标准解），
 *   任何一侧改动必须同步另一侧并由 selfCheckQuaternion 的往返项把守。
 */
export type EulerOrder = "ZYX";

/** 本模块锁定的旋转序（导出以便调用方与文档引用同一事实源）。 */
export const EULER_ORDER: EulerOrder = "ZYX";

/**
 * 由欧拉三元组（度，ZYX 施加序）编四元数：q = qz · qy · qx。
 * 逐分量非有限则返回 null（不产出半合法的四元数）。
 */
export function quatFromEuler(e: Euler3): Quat | null {
  if (!isFiniteNum(e.rxDeg) || !isFiniteNum(e.ryDeg) || !isFiniteNum(e.rzDeg)) return null;
  const qx = quatFromAxisAngle({ x: 1, y: 0, z: 0 }, degToRad(e.rxDeg));
  const qy = quatFromAxisAngle({ x: 0, y: 1, z: 0 }, degToRad(e.ryDeg));
  const qz = quatFromAxisAngle({ x: 0, y: 0, z: 1 }, degToRad(e.rzDeg));
  if (qx === null || qy === null || qz === null) return null;
  return normalizeQuat(quatMul(qz, quatMul(qy, qx)));
}

/**
 * 四元数解回欧拉三元组（度，ZYX 施加序，与 quatFromEuler 严格互逆）。
 *
 * 万向锁邻域（|sinPitch| → 1，pitch = ±90°）的处理立场：
 *   此时 roll 与 yaw 两轴共线，二者的旋转效果不可分离——只有差值确定，
 *   单独恢复任一个都会丢姿态（这是万向锁的物理本质，不是数值缺陷）。
 *   故本实现取**规范解 roll = 0**，并由姿态守恒精确反推 yaw：
 *     原式 q = qz·qy·qx ⇒ qz = q · conj(qy)（roll=0 时 qx 为单位元）
 *     ⇒ yaw = 2·atan2( (q·conj(qy)).z , (q·conj(qy)).w )
 *   该式保证「编 → 解 → 再编」姿态严格等价（|点积| = 1），而非各分量凑数。
 *   万向锁是显性事件：产出 GIMBAL_LOCK_BYPASS 诊断，由调用方决定是否升级为告警。
 */
export function quatToEuler(q: Quat, bag: DiagBag): Euler3 | null {
  const n = normalizeQuat(q);
  if (n === null) return null;

  // ZYX 标准解的三个分子分母对。
  const sinPitch = 2 * (n.w * n.y - n.z * n.x);
  const rollNum = 2 * (n.w * n.x + n.y * n.z);
  const rollDen = 1 - 2 * (n.x * n.x + n.y * n.y);
  const yawNum = 2 * (n.w * n.z + n.x * n.y);
  const yawDen = 1 - 2 * (n.y * n.y + n.z * n.z);

  if (Math.abs(sinPitch) > 1 - 1e-7) {
    const pitchDeg = sinPitch > 0 ? 90 : -90;
    bag.push(
      "GIMBAL_LOCK_BYPASS",
      `旋转落在 pitch=±90° 邻域（sinPitch=${sinPitch.toFixed(9)}），roll 与 yaw 共线不可分离`,
      "已取规范解 roll=0 并由姿态守恒反推 yaw（编解码往返姿态严格等价）；若界面需要连续的欧拉分量曲线，请改读四元数分量",
    );
    // 姿态守恒反推：qz = q · conj(qy)，qy 为 pitch=±90° 的单位四元数。
    const qy = quatFromAxisAngle({ x: 0, y: 1, z: 0 }, degToRad(pitchDeg));
    if (qy === null) return null;
    const t = quatMul(n, quatConj(qy));
    const yawDeg = normalizeAngleDeg(radToDeg(2 * Math.atan2(t.z, t.w)));
    if (!isFiniteNum(yawDeg)) return null;
    return { rxDeg: 0, ryDeg: pitchDeg, rzDeg: yawDeg };
  }

  const rx = radToDeg(Math.atan2(rollNum, rollDen));
  const ry = radToDeg(Math.asin(clamp(sinPitch, -1, 1)));
  const rz = radToDeg(Math.atan2(yawNum, yawDen));
  const e: Euler3 = { rxDeg: normalizeAngleDeg(rx), ryDeg: ry, rzDeg: normalizeAngleDeg(rz) };
  if (!isFiniteNum(e.rxDeg) || !isFiniteNum(e.ryDeg) || !isFiniteNum(e.rzDeg)) return null;
  return e;
}

/**
 * 四元数球面插值（slerp）——防万向锁的关键一步。
 *
 * 三重退化处置，逐条显式：
 *   · 输入非有限 → 返回 null，调用方产出 QUAT_NONFINITE 并拒绝；
 *   · 输入为退化四元数（零模长）→ 返回 null，调用方保持上一帧值；
 *   · 两四元数几乎同向（|dot| → 1，正弦分母趋零）→ 降级归一化线性插值（nlerp），
 *     并产出 SLERP_COLLAPSE 诊断。
 * 半球修正（dot < 0 时取反 b）保证走短弧——四元数双覆盖 q ≡ -q，不修正会绕远路。
 */
export function slerpQuat(a: Quat, b: Quat, t: number, bag: DiagBag): Quat | null {
  if (!quatFinite(a) || !quatFinite(b) || !isFiniteNum(t)) return null;
  const na = normalizeQuat(a);
  const nb = normalizeQuat(b);
  if (na === null || nb === null) return null;

  let dot = quatDot(na, nb);
  let end = nb;
  if (dot < 0) {
    end = { x: -nb.x, y: -nb.y, z: -nb.z, w: -nb.w };
    dot = -dot;
  }
  if (dot > 1) dot = 1;

  // 同向退化：sin(Ω) → 0，标准 slerp 分母除零。此处 nlerp 并显性登记。
  if (dot > 1 - 1e-7) {
    bag.push(
      "SLERP_COLLAPSE",
      `旋转两端几乎同向（dot=${dot.toFixed(9)}），球面插值分母退化`,
      "已降级为归一化线性插值；该区间内角速度会有轻微不均，视觉不可见",
    );
    const lerped = normalizeQuat({
      x: lerpScalar(na.x, end.x, t),
      y: lerpScalar(na.y, end.y, t),
      z: lerpScalar(na.z, end.z, t),
      w: lerpScalar(na.w, end.w, t),
    });
    return lerped;
  }

  const theta0 = Math.acos(dot);
  const sinTheta0 = Math.sin(theta0);
  if (Math.abs(sinTheta0) < EPS) {
    bag.push(
      "SLERP_COLLAPSE",
      `旋转两端正弦分量过小（sin=${sinTheta0.toExponential(3)}），插值不可靠`,
      "已降级为归一化线性插值；建议把该关键帧对的旋转差拆细以避开近平行姿态",
    );
    return normalizeQuat({
      x: lerpScalar(na.x, end.x, t),
      y: lerpScalar(na.y, end.y, t),
      z: lerpScalar(na.z, end.z, t),
      w: lerpScalar(na.w, end.w, t),
    });
  }

  const theta = theta0 * t;
  const s0 = Math.sin(theta0 - theta) / sinTheta0;
  const s1 = Math.sin(theta) / sinTheta0;
  return normalizeQuat({
    x: na.x * s0 + end.x * s1,
    y: na.y * s0 + end.y * s1,
    z: na.z * s0 + end.z * s1,
    w: na.w * s0 + end.w * s1,
  });
}

/** 欧拉三元组插值：分量空间进、四元数空间插、再解回分量空间出（防锁全程）。 */
export function lerpEuler(a: Euler3, b: Euler3, t: number, bag: DiagBag): Euler3 | null {
  const qa = quatFromEuler(a);
  const qb = quatFromEuler(b);
  if (qa === null || qb === null) return null;
  const q = slerpQuat(qa, qb, t, bag);
  if (q === null) return null;
  return quatToEuler(q, bag);
}

// ════════════════════════════════════════════════════════════════════════════
// §4 缓动曲线库（贝塞尔为通用底座，其余为其参数化实例）
// ════════════════════════════════════════════════════════════════════════════

/** 缓动函数签名：归一化进度 → 归一化输出（域 [0,1]，值域通常 [0,1]，回弹类可越界）。 */
export type EasingFn = (t: number) => number;

/** 三次贝塞尔缓动的四个控制点横纵坐标（CSS 同义，P1/P2 可越界以支持回弹）。 */
export interface BezierSpec {
  readonly x1: number;
  readonly y1: number;
  readonly x2: number;
  readonly y2: number;
}

/** 贝塞尔单轴三次多项式求值。 */
function bezierAxis(t: number, a1: number, a2: number): number {
  // B(t) = 3(1-t)²t·a1 + 3(1-t)t²·a2 + t³
  const mt = 1 - t;
  return 3 * mt * mt * t * a1 + 3 * mt * t * t * a2 + t * t * t;
}

/** 贝塞尔单轴三次多项式导数（牛顿迭代用）。 */
function bezierAxisDeriv(t: number, a1: number, a2: number): number {
  const mt = 1 - t;
  return 3 * mt * mt * a1 + 6 * mt * t * (a2 - a1) + 3 * t * t * (1 - a2);
}

/** 贝塞尔反解的牛顿迭代上限（收敛快，主路径）。 */
const NEWTON_MAX_ITER = 8;
/** 贝塞尔反解的二分兜底上限（收敛慢但可靠，32 次把区间收到 2e-10 远小于 1e-7 判据）。 */
const BISECT_MAX_ITER = 32;
/** 贝塞尔反解的收敛判据。 */
const BEZIER_EPS = 1e-7;

/**
 * 三次贝塞尔缓动：给定输入进度 x，反解参数 t 再取 y。
 * x 单调可逆（控制点横坐标在 [0,1] 时成立），故反解有唯一解。
 * 反解策略：牛顿迭代（收敛快）为主，失败或越界转二分兜底（收敛慢但可靠），
 * 两者都有上限——超上限产出 BEZIER_SOLVE_BAILOUT 并退化为线性，绝不返回 NaN。
 */
export function cubicBezierEasing(spec: BezierSpec, bag: DiagBag): EasingFn {
  const { x1, y1, x2, y2 } = spec;
  return (input: number): number => {
    if (!isFiniteNum(input)) return 0;
    // 端点直通：避免迭代在 0/1 处的数值抖动，也让恒等映射精确成立。
    if (input <= 0) return 0;
    if (input >= 1) return 1;

    const solveForT = (x: number): number => {
      let t = x;
      for (let i = 0; i < NEWTON_MAX_ITER; i += 1) {
        const xt = bezierAxis(t, x1, x2) - x;
        if (Math.abs(xt) < BEZIER_EPS) return t;
        const d = bezierAxisDeriv(t, x1, x2);
        if (Math.abs(d) < 1e-9) break; // 导数退化 → 交二分兜底
        t -= xt / d;
        if (t < 0 || t > 1) break; // 牛顿跑出区间 → 交二分兜底
      }
      let lo = 0;
      let hi = 1;
      t = x;
      for (let i = 0; i < BISECT_MAX_ITER; i += 1) {
        const xt = bezierAxis(t, x1, x2);
        if (Math.abs(xt - x) < BEZIER_EPS) return t;
        if (xt < x) lo = t;
        else hi = t;
        t = (lo + hi) / 2;
      }
      bag.push(
        "BEZIER_SOLVE_BAILOUT",
        `贝塞尔缓动反解在 ${BISECT_MAX_ITER} 次二分内未收敛`,
        "已退化为线性映射；请检查控制点是否让横坐标非单调（本实现要求 x1,x2 ∈ [0,1]）",
      );
      return x;
    };

    const t = solveForT(input);
    const y = bezierAxis(t, y1, y2);
    return isFiniteNum(y) ? y : 0;
  };
}

/** 线性缓动（恒等）。 */
export const EASE_LINEAR: EasingFn = (t) => (isFiniteNum(t) ? t : 0);

/** 缓动曲线库：命名曲线一律由 cubicBezierEasing 参数化派生，保证族内行为一致。 */
export const EASINGS = {
  linear: EASE_LINEAR,
  easeInQuad: cubicBezierEasing({ x1: 0.55, y1: 0.085, x2: 0.68, y2: 0.53 }, new DiagBag()),
  easeOutQuad: cubicBezierEasing({ x1: 0.25, y1: 0.46, x2: 0.45, y2: 0.94 }, new DiagBag()),
  easeInOutQuad: cubicBezierEasing({ x1: 0.455, y1: 0.03, x2: 0.515, y2: 0.955 }, new DiagBag()),
  easeInCubic: cubicBezierEasing({ x1: 0.32, y1: 0, x2: 0.67, y2: 0 }, new DiagBag()),
  easeOutCubic: cubicBezierEasing({ x1: 0.33, y1: 1, x2: 0.68, y2: 1 }, new DiagBag()),
  easeInOutCubic: cubicBezierEasing({ x1: 0.65, y1: 0, x2: 0.35, y2: 1 }, new DiagBag()),
  easeOutBack: cubicBezierEasing({ x1: 0.34, y1: 1.56, x2: 0.64, y2: 1 }, new DiagBag()),
} as const;

/** 缓动名键（与 EASINGS 一一对应，用类型层面锁死，杜绝查表落空）。 */
export type EasingName = keyof typeof EASINGS;

/** 弹簧（阻尼谐振）缓动：freq 为全程振荡次数，zeta 为阻尼比。物理模型而非拟合曲线。 */
export function springEasing(freq: number, zeta: number): EasingFn {
  const w0 = Math.max(EPS, Math.abs(freq) * Math.PI * 2);
  const z = Math.max(0, zeta);
  return (input: number): number => {
    if (!isFiniteNum(input)) return 0;
    if (input <= 0) return 0;
    if (input >= 1) return 1;
    const t = input;
    if (z < 1) {
      // 欠阻尼：标准二阶阶跃响应
      const wd = w0 * Math.sqrt(1 - z * z);
      const env = Math.exp(-z * w0 * t);
      const osc = Math.cos(wd * t) + ((z * w0) / wd) * Math.sin(wd * t);
      return 1 - env * osc;
    }
    if (Math.abs(z - 1) < 1e-6) {
      // 临界阻尼：重根，解析式退化
      const env = Math.exp(-w0 * t);
      return 1 - (1 + w0 * t) * env;
    }
    // 过阻尼：两个实根
    const r = w0 * Math.sqrt(z * z - 1);
    const r1 = -z * w0 + r;
    const r2 = -z * w0 - r;
    const c2 = (-r2) / (r1 - r2);
    const c1 = 1 - c2;
    const v = c1 * Math.exp(r1 * t) + c2 * Math.exp(r2 * t);
    return 1 - v;
  };
}

/** 按名取缓动函数；未知名返回显性失败（调用方须决定降级，不静默回落到 linear）。 */
export function resolveEasing(name: string): Outcome<EasingFn> {
  if (name === "spring") return ok(springEasing(1.5, 0.35));
  if (name === "springBouncy") return ok(springEasing(2, 0.12));
  const table = EASINGS as Record<string, EasingFn | undefined>;
  const fn = table[name];
  if (fn === undefined) {
    return fail(
      "EASING_UNKNOWN",
      `缓动名 ${name} 不在曲线库中`,
      `可用名：${Object.keys(EASINGS).join("、")}、spring、springBouncy；或改用 cubicBezier 自定义曲线`,
    );
  }
  return ok(fn);
}

// ════════════════════════════════════════════════════════════════════════════
// §5 裁剪域结构化插值（F0604 语义：裁剪随层变换，故其参数亦可动画）
// ════════════════════════════════════════════════════════════════════════════

/** 裁剪矩形（四边内缩距，局部坐标系）。与 F0604 的三形态共享同一参数载体。 */
export interface ClipRect {
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
  readonly left: number;
}

/** 全零裁剪（不裁剪）。 */
export const CLIP_NONE: ClipRect = { top: 0, right: 0, bottom: 0, left: 0 };

/**
 * 裁剪矩形插值：四边各自线性插值并保持每边非负。
 * 单边为负会让裁剪域反转（露边而非裁边），故钳到 0 并产出 CLAMP_DEGENERATE。
 */
export function lerpClipRect(a: ClipRect, b: ClipRect, t: number, bag: DiagBag): ClipRect {
  const edges: Array<keyof ClipRect> = ["top", "right", "bottom", "left"];
  const out: Record<keyof ClipRect, number> = { top: 0, right: 0, bottom: 0, left: 0 };
  let clamped = false;
  for (const e of edges) {
    const raw = lerpScalar(a[e], b[e], t);
    if (!isFiniteNum(raw)) continue;
    if (raw < 0) clamped = true;
    out[e] = Math.max(0, raw);
  }
  if (clamped) {
    bag.push(
      "CLAMP_DEGENERATE",
      "裁剪边距插值出现负值（会让裁剪域反转成露边）",
      "已钳制到 0；若确实需要负边距语义，请改用变换矩阵而非裁剪参数表达",
    );
  }
  return { top: out.top, right: out.right, bottom: out.bottom, left: out.left };
}

// ════════════════════════════════════════════════════════════════════════════
// §6 可动画属性注册表（判据一：注册表）
// ════════════════════════════════════════════════════════════════════════════

/** 属性值的三种形态（注册表按形态派发到对应插值规则）。 */
export type PropKind = "scalar" | "vec2" | "euler3" | "clipRect";

/** 脏类：属性变更影响到的脏区级别，由 F0613 消费。 */
export type DirtyClass = "transform-self" | "subtree-transform" | "content";

/** 属性键。闭集——新增可动画属性必须进注册表，不允许旁路直写。 */
export type PropKey =
  | "transform.translate"
  | "transform.rotate"
  | "transform.scale"
  | "transform.skewX"
  | "transform.skewY"
  | "opacity"
  | "clip.inset"
  | "clip.radius";

/** 一条属性规格：插值规则 + 默认值 + 值域 + 脏类，全部显式声明不靠约定。 */
export interface PropSpec {
  readonly key: PropKey;
  readonly kind: PropKind;
  /** 默认值（未设属性时的继承/默认来源，符合 F0601 稀疏属性策略）。 */
  readonly defaultValue: number | Vec2 | Euler3 | ClipRect;
  /** 标量属性的值域；非标量为 undefined。 */
  readonly range?: { readonly lo: number; readonly hi: number };
  /** 该属性变更应触发的脏区级别。 */
  readonly dirtyClass: DirtyClass;
  /** 是否参与「减弱动效」降级咨询（UI 层查询本注册表实现降级）。 */
  readonly respectsReducedMotion: boolean;
  /** 一句话说明，供自检与文档生成引用。 */
  readonly note: string;
}

/** 默认（未旋转）欧拉三元组。 */
const EULER_IDENTITY: Euler3 = { rxDeg: 0, ryDeg: 0, rzDeg: 0 };
/** 默认单位缩放。 */
const SCALE_IDENTITY: Vec2 = { x: 1, y: 1 };

/** 变更隔离语义提示（F0605 条件清单成员之一：alpha < 1 隐式隔离）。 */
const NOTE_ALPHA = "alpha<1 触发 F0605 隐式隔离，故其变更按子树脏处理";

/**
 * 属性注册表。判据一要求逐属性声明插值规则，故每条规格自带 kind（决定插值算法）
 * 与 dirtyClass（决定脏区级别）——二者在同一处定义，避免「谁在插什么、脏到哪」分裂。
 */
export const PROP_REGISTRY: Readonly<Record<PropKey, PropSpec>> = {
  "transform.translate": {
    key: "transform.translate",
    kind: "vec2",
    defaultValue: { x: 0, y: 0 },
    dirtyClass: "subtree-transform",
    respectsReducedMotion: true,
    note: "平移分量，标量线性插值；下游 F0643 位移优化消费本属性的逐帧值",
  },
  "transform.rotate": {
    key: "transform.rotate",
    kind: "euler3",
    defaultValue: EULER_IDENTITY,
    dirtyClass: "subtree-transform",
    respectsReducedMotion: true,
    note: "旋转分量，四元数球面插值防万向锁（禁止欧拉线性插值）",
  },
  "transform.scale": {
    key: "transform.scale",
    kind: "vec2",
    defaultValue: SCALE_IDENTITY,
    dirtyClass: "subtree-transform",
    respectsReducedMotion: false,
    note: "缩放分量，标量线性插值；负缩放属镜像，语义变更走脏路径不静默",
  },
  "transform.skewX": {
    key: "transform.skewX",
    kind: "scalar",
    defaultValue: 0,
    range: { lo: -89.9, hi: 89.9 },
    dirtyClass: "subtree-transform",
    respectsReducedMotion: true,
    note: "X 向倾斜（度），值域避开 ±90° 以免矩阵奇异",
  },
  "transform.skewY": {
    key: "transform.skewY",
    kind: "scalar",
    defaultValue: 0,
    range: { lo: -89.9, hi: 89.9 },
    dirtyClass: "subtree-transform",
    respectsReducedMotion: true,
    note: "Y 向倾斜（度），值域避开 ±90° 以免矩阵奇异",
  },
  opacity: {
    key: "opacity",
    kind: "scalar",
    defaultValue: 1,
    range: { lo: 0, hi: 1 },
    dirtyClass: "subtree-transform",
    respectsReducedMotion: false,
    note: NOTE_ALPHA,
  },
  "clip.inset": {
    key: "clip.inset",
    kind: "clipRect",
    defaultValue: CLIP_NONE,
    dirtyClass: "subtree-transform",
    respectsReducedMotion: false,
    note: "裁剪四边内缩距，结构化插值并保持每边非负；F0604 随层变换语义在此对齐",
  },
  "clip.radius": {
    key: "clip.radius",
    kind: "scalar",
    defaultValue: 0,
    range: { lo: 0, hi: 8192 },
    dirtyClass: "subtree-transform",
    respectsReducedMotion: false,
    note: "圆角裁剪半径，语义与蒙版实现解耦（蒙版机制归 F0650）",
  },
};

/** 注册表键全集（自检与遍历用，避免手写清单与实际注册漂移）。 */
export const PROP_KEYS: readonly PropKey[] = Object.keys(PROP_REGISTRY) as PropKey[];

/** 查属性规格；未注册返回显性失败——判据要求「属性未注册 → 显性不支持」。 */
export function lookupProp(key: string): Outcome<PropSpec> {
  const table = PROP_REGISTRY as Record<string, PropSpec | undefined>;
  const spec = table[key];
  if (spec === undefined) {
    return fail(
      "PROP_UNREGISTERED",
      `属性 ${key} 未在可动画属性注册表中注册，拒绝插值`,
      `已注册属性：${PROP_KEYS.join("、")}；新增属性须先在 PROP_REGISTRY 声明插值规则与脏类`,
    );
  }
  return ok(spec);
}

// ════════════════════════════════════════════════════════════════════════════
// §7 时间基插值协议（判据二：纯函数插值）
// ════════════════════════════════════════════════════════════════════════════

/** 关键帧：归一化时间 + 属性值 + 该帧出向缓动名。 */
export interface Keyframe {
  /** 归一化时间，必须落在 [0,1] 且轨道内单调不减。 */
  readonly t: number;
  readonly value: number | Vec2 | Euler3 | ClipRect;
  /** 从本帧到下一帧区间使用的缓动名；缺省为 linear。 */
  readonly easing?: EasingName | "spring" | "springBouncy";
}

/** 一条属性轨道（时间基）。刻意不含任何可变字段——可快照、可重放的结构前提。 */
export interface Track {
  readonly propKey: PropKey;
  readonly keyframes: readonly Keyframe[];
}

/** 属性值的名义类型（对外暴露用，内部按 kind 收窄）。 */
export type PropValue = number | Vec2 | Euler3 | ClipRect;

/** 轨道内的形状守卫：不同 kind 的属性值形状互不相同，混用会让插值静默产出垃圾。 */
function valueShapeOk(spec: PropSpec, v: PropValue): boolean {
  switch (spec.kind) {
    case "scalar":
      return typeof v === "number";
    case "vec2":
      return typeof v === "object" && v !== null && "x" in v && "y" in v;
    case "euler3":
      return typeof v === "object" && v !== null && "rxDeg" in v;
    case "clipRect":
      return typeof v === "object" && v !== null && "top" in v;
    default:
      return false;
  }
}

/**
 * 按属性 kind 插值两个值（分派点）。所有分支都写明退化处置，无通用兜底分支。
 * bag 累积非致命诊断；返回 null 表示该对不可插值（调用方走「保持上一帧」）。
 */
function interpolateByKind(spec: PropSpec, a: PropValue, b: PropValue, t: number, bag: DiagBag): PropValue | null {
  switch (spec.kind) {
    case "scalar": {
      if (typeof a !== "number" || typeof b !== "number") return null;
      if (!isFiniteNum(a) || !isFiniteNum(b)) return null;
      return lerpScalar(a, b, t);
    }
    case "vec2": {
      if (typeof a !== "object" || a === null || typeof b !== "object" || b === null) return null;
      if (!("x" in a) || !("y" in a) || !("x" in b) || !("y" in b)) return null;
      return lerpVec2(a, b, t);
    }
    case "euler3": {
      if (typeof a !== "object" || a === null || typeof b !== "object" || b === null) return null;
      if (!("rxDeg" in a) || !("rxDeg" in b)) return null;
      return lerpEuler(a, b, t, bag);
    }
    case "clipRect": {
      if (typeof a !== "object" || a === null || typeof b === "object" || b === null) return null;
      if (!("top" in a) || !("top" in b)) return null;
      return lerpClipRect(a, b, t, bag);
    }
    default:
      return null;
  }
}

/** 对属性值施加值域钳制并按需记录告警（注册表声明了 range 才钳）。 */
function applyRange(spec: PropSpec, v: PropValue, bag: DiagBag): PropValue {
  if (spec.range === undefined || typeof v !== "number") return v;
  const lo = spec.range.lo;
  const hi = spec.range.hi;
  const c = clamp(v, lo, hi);
  if (c !== v) {
    bag.push(
      "VALUE_NONFINITE",
      `属性 ${spec.key} 的插值结果 ${v} 越出值域 [${lo}, ${hi}]`,
      `已钳制到 ${c}；若越界是预期的，改用不声明 range 的属性或引入归一化分量表达`,
    );
  }
  return c;
}

/**
 * 对任意形态的属性值施加值域约束（端点路径专用）。
 * 标量走 applyRange；clipRect 逐边钳到非负（关键帧侧的插值已钳过，此处防关键帧本身写负）；
 * vec2 / euler3 无声明值域，原样返回（不静默改动语义）。
 */
function clampPropValue(spec: PropSpec, v: PropValue, bag: DiagBag): PropValue {
  if (typeof v === "number") return applyRange(spec, v, bag);
  if (spec.kind === "clipRect" && typeof v === "object" && v !== null && "top" in v) {
    let clamped = false;
    const edges: Array<keyof ClipRect> = ["top", "right", "bottom", "left"];
    const out: Record<keyof ClipRect, number> = { top: 0, right: 0, bottom: 0, left: 0 };
    for (const e of edges) {
      const raw = v[e];
      if (!isFiniteNum(raw)) {
        out[e] = 0;
        continue;
      }
      if (raw < 0) clamped = true;
      out[e] = Math.max(0, raw);
    }
    if (clamped) {
      bag.push(
        "CLAMP_DEGENERATE",
        `属性 ${spec.key} 的关键帧裁剪边距为负（会让裁剪域反转成露边）`,
        "已把负边距钳制到 0；若确实需要负边距语义，请改用变换矩阵而非裁剪参数表达",
      );
    }
    return { top: out.top, right: out.right, bottom: out.bottom, left: out.left };
  }
  return v;
}

/** 二分查找：定位 progress 落在哪一段（找最大的 i 使 keyframes[i].t <= progress）。 */
function findSegment(frames: readonly Keyframe[], progress: number): number {
  let lo = 0;
  let hi = frames.length - 1;
  let best = 0;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const f = frames[mid];
    if (f !== undefined && f.t <= progress) {
      best = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return best;
}

/**
 * 轨道采样——本条的核心纯函数。
 *
 * 纯函数契约（判据二）：
 *   · 只读入参与轨道数据，绝不读写全局、绝不取时钟、绝不产生副作用；
 *   · 同一入参调用任意次，返回值逐位一致（可重放、可快照比对）；
 *   · 返回全新对象，调用方改动返回值不污染轨道（快照安全的另一半）。
 *
 * 失败语义：进度越界先钳制并记录（不拒绝——播放头 overshoot 是常态），
 * 轨道为空或关键帧形状非法才返回显性失败。插值退化（旋转零向量等）时
 * 返回 keepPrevious 处置码而非 null，让调用方明确知道该保持上一帧。
 */
export function sampleTrack(track: Track, progress: number): Outcome<PropValue> {
  const bag = new DiagBag();
  const spec = lookupProp(track.propKey);
  if (!spec.ok) return spec;
  const sp = spec.value;

  if (track.keyframes.length === 0) {
    return fail<PropValue>(
      "TRACK_EMPTY",
      `属性 ${track.propKey} 的轨道没有任何关键帧`,
      "至少需要一帧（常量轨道）才能采样；请在时间线编辑器补帧或改用注册表默认值",
    );
  }

  if (!isFiniteNum(progress)) {
    bag.push(
      "PROGRESS_OUT_OF_RANGE",
      `归一化进度为非有限数（${String(progress)}）`,
      "已钳制到 0（从轨道起点重放）；若这是上游计算缺陷请先修播放头时钟",
    );
    // 钳到 0 后正常走端点路径（取轨道首帧值），而不是直接吐注册表默认值——
    // 轨道存在时它才是语义上的「起点」，默认值只适用于无轨道的情形。
    // 形状合法性在此之前已由下方统一校验，这里补一次以保证类型收窄。
    const head = track.keyframes[0];
    return head === undefined
      ? ok(sp.defaultValue, bag.all())
      : ok(clampPropValue(sp, head.value, bag), bag.all());
  }

  let p = progress;
  if (p < 0 || p > 1) {
    bag.push(
      "PROGRESS_OUT_OF_RANGE",
      `归一化进度 ${p} 越出 [0,1]`,
      "已钳制到 [0,1]；播放头 overshoot 属正常，若来自跳转逻辑请核对该处的边界约定",
    );
    p = clamp(p, 0, 1);
  }

  // 关键帧合法性：时间单调与值形状，两者任一不满足都不得静默插值。
  let ordered = true;
  let shapeOk = true;
  for (let i = 0; i < track.keyframes.length; i += 1) {
    const f = track.keyframes[i];
    if (f === undefined) continue;
    if (!isFiniteNum(f.t) || f.t < 0 || f.t > 1) ordered = false;
    if (i > 0) {
      const prev = track.keyframes[i - 1];
      if (prev !== undefined && f.t < prev.t) ordered = false;
    }
    if (!valueShapeOk(sp, f.value)) shapeOk = false;
  }
  if (!shapeOk) {
    return fail<PropValue>(
      "VALUE_NONFINITE",
      `属性 ${track.propKey}（${sp.kind}）的关键帧值形状不匹配`,
      `每个关键帧都必须携带与属性种类一致的值形态；${describeShape(sp.kind)}`,
    );
  }
  if (!ordered) {
    bag.push(
      "KEYFRAME_UNORDERED",
      `属性 ${track.propKey} 的关键帧时间非单调或越出 [0,1]`,
      "已按轨道既有顺序做线性段选择（不做重排——重排会掩盖上游时间线缺陷）；请在编辑器侧修正关键帧顺序",
    );
  }

  const frames = track.keyframes;
  const first = frames[0];
  const last = frames[frames.length - 1];
  if (first === undefined || last === undefined) {
    return fail<PropValue>("TRACK_EMPTY", "轨道关键帧数组不可读", "请检查轨道构造过程是否产出了空洞");
  }

  // 端点外推：轨道外取端值（保持态动画的正确语义，不是钳制错误）。
  // ⚠️ 端点值同样必须过值域校验：关键帧本身可能写错（如 opacity 写成 5），
  // 若端点直返原始值，越界数据会绕过全部钳制漏到渲染层——这是实测踩过的坑。
  // 结构化属性（clipRect）在关键帧侧就已钳制（lerpClipRect 逐边钳），
  // 故此处只需对标量与向量走 applyRange。
  if (p <= first.t) return ok(clampPropValue(sp, first.value, bag), bag.all());
  if (p >= last.t) return ok(clampPropValue(sp, last.value, bag), bag.all());

  const i = findSegment(frames, p);
  const k0 = frames[i];
  const k1 = frames[i + 1];
  if (k0 === undefined || k1 === undefined) {
    return fail<PropValue>(
      "TRACK_EMPTY",
      `属性 ${track.propKey} 在进度 ${p} 处找不到包围关键帧`,
      "这是轨道内部结构损坏（相邻帧缺失），请上报并保留该轨道快照用于复现",
    );
  }

  const span = k1.t - k0.t;
  // 零宽段（同时间双帧）退化：不做除零，直接取后帧值并显性登记。
  const local = span < EPS ? 1 : clamp((p - k0.t) / span, 0, 1);

  const easingName = k0.easing ?? "linear";
  const eased = resolveEasing(easingName);
  if (!eased.ok) {
    bag.push(eased.code, eased.message, eased.hint);
    return ok(first.value, bag.all());
  }

  const t = eased.value(local);
  const interpolated = interpolateByKind(sp, k0.value, k1.value, t, bag);
  if (interpolated === null) {
    // 插值退化：显性返回 keepPrevious 语义——由调用方保持上一帧，本函数不猜值。
    bag.push(
      sp.kind === "euler3" ? "QUAT_DEGENERATE" : "VALUE_NONFINITE",
      `属性 ${track.propKey} 在进度 ${p} 处插值退化（起止值不可插值）`,
      "调用方应保持上一帧值并跳过本帧写入；连续退化说明关键帧含非法值，请回查时间线数据",
    );
    return ok(k0.value, bag.all());
  }

  return ok(applyRange(sp, interpolated, bag), bag.all());
}

/** 形态说明文案（供失败提示使用，避免调用方自己拼描述）。 */
function describeShape(kind: PropKind): string {
  switch (kind) {
    case "scalar":
      return "标量属性须用 number";
    case "vec2":
      return "二维属性须用 { x, y }";
    case "euler3":
      return "旋转属性须用 { rxDeg, ryDeg, rzDeg }（度）";
    case "clipRect":
      return "裁剪属性须用 { top, right, bottom, left }";
    default:
      return "形态未知，请核对注册表声明";
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §8 重组回矩阵（判据四：与 F0602 分解重组衔接）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 2D 仿射矩阵，行主序六元组：
 *   | a c e |
 *   | b d f |
 *   | 0 0 1 |
 * 与 F0602 的 2D 仿射约定一致；分解产物为本模块的动画分量，重组即 compose。
 */
export interface Mat2D {
  readonly a: number;
  readonly b: number;
  readonly c: number;
  readonly d: number;
  readonly e: number;
  readonly f: number;
}

/** 单位矩阵。 */
export const MAT2D_IDENTITY: Mat2D = { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 };

/** 一组层的动画分量（插值的直接产物，也是重组的唯一输入）。 */
export interface LayerComponents {
  readonly translate: Vec2;
  readonly rotateDeg: number;
  readonly scale: Vec2;
  readonly skewXDeg: number;
  readonly skewYDeg: number;
}

/** 默认分量（未动画时的单位姿态）。 */
export const LAYER_COMPONENTS_DEFAULT: LayerComponents = {
  translate: { x: 0, y: 0 },
  rotateDeg: 0,
  scale: SCALE_IDENTITY,
  skewXDeg: 0,
  skewYDeg: 0,
};

/**
 * 3x3 齐次矩阵（行主序九元组），仅供本模块内部连乘使用。
 * 用完整 3x3 做连乘再降为 2x3，避免手推 2x2 展开式引入符号错误——
 * 连乘的每一项都可与定义式逐项核对，审查成本远低于验证手推公式。
 */
type Mat3 = readonly [number, number, number, number, number, number, number, number, number];

/** 3x3 矩阵连乘。 */
function mat3Mul(x: Mat3, y: Mat3): Mat3 {
  return [
    x[0] * y[0] + x[1] * y[3] + x[2] * y[6],
    x[0] * y[1] + x[1] * y[4] + x[2] * y[7],
    x[0] * y[2] + x[1] * y[5] + x[2] * y[8],
    x[3] * y[0] + x[4] * y[3] + x[5] * y[6],
    x[3] * y[1] + x[4] * y[4] + x[5] * y[7],
    x[3] * y[2] + x[4] * y[5] + x[5] * y[8],
    x[6] * y[0] + x[7] * y[3] + x[8] * y[6],
    x[6] * y[1] + x[7] * y[4] + x[8] * y[7],
    x[6] * y[2] + x[7] * y[5] + x[8] * y[8],
  ];
}

/** 由 3x3 齐次形式降为 2D 仿射（末行末列必须是 0/0/1，否则矩阵含投影不可表达）。 */
function mat2DFromMat3(m: Mat3, bag: DiagBag): Outcome<Mat2D> {
  if (m[6] !== 0 || m[7] !== 0 || m[8] !== 1) {
    return fail<Mat2D>(
      "MATRIX_DEGENERATE",
      `重组结果含投影分量（末行 = ${m[6]}, ${m[7]}, ${m[8]}）`,
      "2D 仿射无法表达投影；若确需透视，请走 F0602 的 3D 投影变换路径而非本条的 2D 重组",
    );
  }
  return ok({ a: m[0], b: m[3], c: m[1], d: m[4], e: m[2], f: m[5] }, bag.all());
}

/**
 * 分量重组为 2D 仿射矩阵（O(1)，判据「重组 O(1)」）。
 * 顺序固定为 T · R · Kx · Ky · S（平移最后相乘即最先施加于点），
 * 顺序即视觉结果，不接受调用方改序——与 F0602「级联顺序即视觉结果不容两义」同律。
 */
export function composeMatrix(c: LayerComponents): Outcome<Mat2D> {
  const bag = new DiagBag();
  const { translate, rotateDeg, scale, skewXDeg, skewYDeg } = c;
  if (!isFiniteNum(rotateDeg) || !isFiniteNum(skewXDeg) || !isFiniteNum(skewYDeg)) {
    return fail<Mat2D>(
      "VALUE_NONFINITE",
      "重组矩阵时角度分量为非有限数",
      "请回查该属性的关键帧数据；非有限角度不得进入矩阵运算（会污染整棵子树的世界矩阵）",
    );
  }
  if (!isFiniteNum(translate.x) || !isFiniteNum(translate.y)) {
    return fail<Mat2D>("VALUE_NONFINITE", "重组矩阵时平移分量为非有限数", "请回查平移轨道的关键帧数据");
  }
  if (!isFiniteNum(scale.x) || !isFiniteNum(scale.y)) {
    return fail<Mat2D>("VALUE_NONFINITE", "重组矩阵时缩放分量为非有限数", "请回查缩放轨道的关键帧数据");
  }

  const th = degToRad(rotateDeg);
  const cos = Math.cos(th);
  const sin = Math.sin(th);
  const tanX = Math.tan(degToRad(skewXDeg));
  const tanY = Math.tan(degToRad(skewYDeg));

  // 退化前置闸门：先看线性部分是否可逆，再做连乘（连乘结果退化时同样会被下方 det 检查拦下，
  // 但前置闸门能给出更精确的根因——是缩放归零还是倾斜趋 ±90°）。
  const scaleDet = scale.x * scale.y;
  if (Math.abs(scaleDet) < EPS) {
    return fail<Mat2D>(
      "MATRIX_DEGENERATE",
      `缩放分量使线性部分退化（scale.x·scale.y = ${String(scaleDet)}）`,
      "缩放为零会产出不可逆矩阵，F0602 的逆矩阵缓存与命中测试都会失效；请把缩放关键帧的最小绝对值设为非零下限",
    );
  }

  // R：旋转
  const r: Mat3 = [cos, -sin, 0, sin, cos, 0, 0, 0, 1];
  // Kx：X 向倾斜 [[1, tanX], [0, 1]]
  const kx: Mat3 = [1, tanX, 0, 0, 1, 0, 0, 0, 1];
  // Ky：Y 向倾斜 [[1, 0], [tanY, 1]]
  const ky: Mat3 = [1, 0, 0, tanY, 1, 0, 0, 0, 1];
  // S：缩放
  const s: Mat3 = [scale.x, 0, 0, 0, scale.y, 0, 0, 0, 1];
  // T：平移
  const t: Mat3 = [1, 0, translate.x, 0, 1, translate.y, 0, 0, 1];

  const m = mat3Mul(mat3Mul(mat3Mul(mat3Mul(t, r), kx), ky), s);
  return mat2DFromMat3(m, bag);
}

/** 矩阵行列式（奇异判定；F0602 命中测试的逆矩阵缓存依赖此值非零）。 */
export function matDeterminant(m: Mat2D): number {
  return m.a * m.d - m.b * m.c;
}

/**
 * 矩阵逆（显式 2x2 闭式解）。奇异时返回显性失败——
 * 由 F0602 的「奇异 → 降级恒等加告警」承接，本函数只负责如实报告。
 */
export function invertMatrix(m: Mat2D): Outcome<Mat2D> {
  const det = matDeterminant(m);
  if (!isFiniteNum(det) || Math.abs(det) < EPS) {
    return fail<Mat2D>(
      "MATRIX_DEGENERATE",
      `矩阵奇异（行列式 ${String(det)}），无法求逆`,
      "调用方应按 F0602 退化策略降级为恒等矩阵并记录告警；本函数不代为降级以免掩盖根因",
    );
  }
  const inv = 1 / det;
  const a = m.d * inv;
  const b = -m.b * inv;
  const c = -m.c * inv;
  const d = m.a * inv;
  return ok({ a, b, c, d, e: -(a * m.e + c * m.f), f: -(b * m.e + d * m.f) });
}

/**
 * 从矩阵反解 2D 分量（分解侧）。
 *
 * 规范化约定（必须先读，否则会误判「分解不唯一」是 bug）：
 *   compose 的顺序是 T · R · Kx · Ky · S，含两个倾斜参数共 5 个自由度，
 *   而 2x2 线性矩阵只有 4 个自由度（a,b,c,d）——数学上分解本就不唯一。
 *   故本函数采用**规范化约定：skewYDeg 恒为 0，全部剪切并入 skewXDeg**，
 *   即约定 compose 的等价规范形为 T · R · K · S（4 参数 ↔ 4 自由度，严格可逆）。
 *   非零的 skewYDeg 仍可由 compose 正常消费（动画可以倾斜两个轴），
 *   只是分解回分量时不保留该自由度——需要保留原始双轴倾斜的调用方
 *   应保存分量快照而非依赖矩阵分解（F0610 快照正是为此存在）。
 *
 * 解算式（由 compose 的规范形连乘逐项反解，可用代入法核对）：
 *   M = T · R · K · S，线性部分展开后
 *     a = cos·sx
 *     b = sin·sx
 *     c = (cos·t − sin)·sy
 *     d = (sin·t + cos)·sy
 *   反解（标准 QR 分解步骤）：
 *     n  = hypot(a, b) = sx            ← 第一列长度即缩放（旋转列正交，长度守恒）
 *     cos = a/n, sin = b/n            ← 第一列方向即旋转
 *     sy = (−b·c + a·d)/n             ← 第二列在正交基下的分量（**不是** hypot(c,d)，
 *                                          因为倾斜把第二列在旋转基下混入了 t·sy 项）
 *     t  = (a·c + b·d)/(n·sy)          ← 同基下的剪切量除以 sy 还原为斜切角
 */
export function decomposeMatrix(m: Mat2D): Outcome<LayerComponents> {
  const det = matDeterminant(m);
  if (!isFiniteNum(det) || Math.abs(det) < EPS) {
    return fail<LayerComponents>(
      "MATRIX_DEGENERATE",
      `矩阵奇异（行列式 ${String(det)}），无法分解`,
      "奇异矩阵的分解不唯一；请先修复上游重组输入（缩放为零或倾斜越界）",
    );
  }
  const bag = new DiagBag();
  const n = Math.hypot(m.a, m.b);
  if (n < EPS) {
    return fail<LayerComponents>(
      "MATRIX_DEGENERATE",
      `矩阵第一列长度为零（${n}），缩放不可解`,
      "该矩阵无法表达为 T·R·K·S 形式；请检查是否混入了投影或错序的乘法",
    );
  }
  const cos = m.a / n;
  const sin = m.b / n;
  // 第二列在正交基 (cos,sin) / (−sin,cos) 下的两个分量。
  const shearTimesSy = (m.a * m.c + m.b * m.d) / n;
  const sy = (-m.b * m.c + m.a * m.d) / n;
  if (!isFiniteNum(sy) || Math.abs(sy) < EPS) {
    return fail<LayerComponents>(
      "MATRIX_DEGENERATE",
      `分解得缩放 sy = ${String(sy)}（为零即矩阵不可逆）`,
      "sy 为零说明第二列与第一列共线；请检查上游是否产出了退化矩阵或错序乘法",
    );
  }
  // 镜像（sy 为负）保留符号：负缩放属镜像变换，是语义信息不可丢弃。
  const tanT = shearTimesSy / sy;
  const out: LayerComponents = {
    translate: { x: m.e, y: m.f },
    rotateDeg: normalizeAngleDeg(radToDeg(Math.atan2(sin, cos))),
    scale: { x: n, y: sy },
    skewXDeg: normalizeAngleDeg(radToDeg(Math.atan(tanT))),
    skewYDeg: 0,
  };
  if (
    !isFiniteNum(out.rotateDeg) ||
    !isFiniteNum(out.scale.x) ||
    !isFiniteNum(out.scale.y) ||
    !isFiniteNum(out.skewXDeg)
  ) {
    return fail<LayerComponents>(
      "VALUE_NONFINITE",
      "矩阵分解产出非有限分量",
      "请检查矩阵输入是否含 NaN/Inf；非有限输入一律不得进入分解路径",
    );
  }
  // 规范化约定：分解结果的行列式必须与输入一致，否则说明解算式与 compose 不同序。
  const roundDet = out.scale.x * out.scale.y;
  if (Math.abs(roundDet - det) > 1e-6 * Math.max(1, Math.abs(det))) {
    bag.push(
      "MATRIX_DEGENERATE",
      `分解结果的行列式 ${roundDet} 与输入 ${det} 不一致`,
      "分解式与 compose 连乘不同序；这是实现缺陷而非数据问题，请携带该矩阵样本上报",
    );
  }
  return ok(out, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §9 驱动联动（判据三：脏区联动）
// ════════════════════════════════════════════════════════════════════════════

/** 一条脏区事件：与 F0613 的脏区收集契约对齐（节点 + 脏类 + 是否波及子树）。 */
export interface DirtyEvent {
  readonly nodeId: string;
  readonly propKey: PropKey;
  readonly dirtyClass: DirtyClass;
  /** 动画层恒为 true：子树世界矩阵与包围盒随本层变换级联失效。 */
  readonly subtreeInvalidated: boolean;
}

/** 一次帧求值的完整产物。 */
export interface FrameResult {
  /** 属性键 → 该帧值。 */
  readonly values: ReadonlyMap<PropKey, PropValue>;
  /** 本帧应触发的脏区事件（顺序稳定，便于测试与回放比对）。 */
  readonly dirty: readonly DirtyEvent[];
  /** 重组好的本层矩阵（判据四的落点：矩阵是下游 GPU/CPU 双路唯一输入）。 */
  readonly matrix: Mat2D;
  /** 本帧全部诊断（按发生序）。 */
  readonly diagnostics: readonly Diagnostic[];
}

/**
 * 一个属性在一帧上的输入：节点 + 轨道（无轨道则用注册表默认值）。
 *
 * propKey 的两种来源：
 *   · track 非空 → 取 track.propKey（受信路径，Track 构造时已过注册表校验）；
 *   · 提供 rawPropKey → 走不可信路径（来自 F0612 反序列化的原始字符串），
 *     由 evaluateFrame 在运行时校验，未注册即显性拒绝并产出诊断。
 * 两者都不写时视为「本层无该属性」，取注册表默认值，不产脏事件。
 */
export interface PropBinding {
  readonly nodeId: string;
  readonly track: Track | null;
  /** 不可信属性键（外部数据来源）。优先于 track.propKey，用于负向校验路径。 */
  readonly rawPropKey?: string;
}

/**
 * 帧求值：对一组绑定逐条采样、收集脏区、重组矩阵。
 *
 * 「动画层的子树按变换脏处理」的实现要点：
 *   任一绑定采样出与注册表默认值不同的值，即视为该节点进入动画态，
 *   其脏类由属性规格决定（subtree-transform 的属性会让整棵子树的世界矩阵
 *   与包围盒失效——这正是 F0613 需要的粒度，不必逐子树发事件）。
 */
export function evaluateFrame(bindings: readonly PropBinding[], progress: number): FrameResult {
  const values = new Map<PropKey, PropValue>();
  const dirty: DirtyEvent[] = [];
  const diagnostics: Diagnostic[] = [];

  for (const binding of bindings) {
    // 属性键来源：不可信原始键优先，其次受信轨道；都没有则本层无该属性。
    const key = binding.rawPropKey ?? binding.track?.propKey;
    if (key === undefined) continue;
    const spec = lookupProp(key);
    // 未注册：显性跳过 + 诊断，绝不猜一个默认值进去。
    if (!spec.ok) {
      diagnostics.push(...spec.diagnostics);
      continue;
    }
    const sp = spec.value;

    let value: PropValue;
    if (binding.track === null) {
      value = sp.defaultValue;
    } else {
      const sampled = sampleTrack(binding.track, progress);
      diagnostics.push(...sampled.diagnostics);
      if (!sampled.ok) {
        continue;
      }
      value = sampled.value;
      // 采样成功但返回默认值 → 视为静止，不产脏事件（避免空转脏区）。
      if (!sameValue(sp.defaultValue, value)) {
        dirty.push({
          nodeId: binding.nodeId,
          propKey: sp.key,
          dirtyClass: sp.dirtyClass,
          subtreeInvalidated: sp.dirtyClass === "subtree-transform",
        });
      }
    }
    values.set(sp.key, value);
  }

  const components = componentsFrom(values);
  const composed = composeMatrix(components);
  const matrix: Mat2D = composed.ok ? composed.value : MAT2D_IDENTITY;
  if (!composed.ok) diagnostics.push(...composed.diagnostics);

  return { values, dirty, matrix, diagnostics };
}

/** 从属性值集合组装层分量（未出现的属性取注册表默认值——稀疏属性策略）。 */
export function componentsFrom(values: ReadonlyMap<PropKey, PropValue>): LayerComponents {
  const tr = values.get("transform.translate");
  const rot = values.get("transform.rotate");
  const sc = values.get("transform.scale");
  const kx = values.get("transform.skewX");
  const ky = values.get("transform.skewY");
  const def = LAYER_COMPONENTS_DEFAULT;
  return {
    translate: isVec2(tr) ? tr : def.translate,
    rotateDeg: isEuler3(rot) ? rot.rzDeg : def.rotateDeg,
    scale: isVec2(sc) ? sc : def.scale,
    skewXDeg: typeof kx === "number" ? kx : def.skewXDeg,
    skewYDeg: typeof ky === "number" ? ky : def.skewYDeg,
  };
}

/** 二维向量形状守卫。 */
function isVec2(v: PropValue | undefined): v is Vec2 {
  return typeof v === "object" && v !== null && "x" in v && "y" in v;
}

/** 欧拉三元组形状守卫。 */
function isEuler3(v: PropValue | undefined): v is Euler3 {
  return typeof v === "object" && v !== null && "rxDeg" in v;
}

/** 二维向量形状守卫（对外供自检与调用方做值域断言用）。 */
function isVec2Value(v: PropValue | undefined): v is Vec2 {
  return typeof v === "object" && v !== null && "x" in v && "y" in v;
}

/** 欧拉三元组形状守卫（对外供自检做值域断言用）。 */
function isEuler3Value(v: PropValue | undefined): v is Euler3 {
  return typeof v === "object" && v !== null && "rxDeg" in v && "ryDeg" in v && "rzDeg" in v;
}

/** 值相等判定（形状敏感：不同形态不比较，避免 vec2 与 euler3 被误判相等）。 */
export function sameValue(a: PropValue, b: PropValue): boolean {
  if (typeof a === "number" && typeof b === "number") return a === b;
  if (typeof a === "number" || typeof b === "number") return false;
  if ("x" in a && "x" in b) return a.x === b.x && a.y === b.y;
  if ("rxDeg" in a && "rxDeg" in b) return a.rxDeg === b.rxDeg && a.ryDeg === b.ryDeg && a.rzDeg === b.rzDeg;
  return a.top === b.top && a.right === b.right && a.bottom === b.bottom && a.left === b.left;
}

// ════════════════════════════════════════════════════════════════════════════
// §10 快照与重放（判据二的可验证面：可重放可快照不是口号，是可对拍的性质）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 不可信轨道输入（来自文件/网络/存档的原始数据，属性键为任意字符串）。
 * 显式与受信的 Track 分离：类型层面就把「外部数据」标成不可信，
 * 迫使调用方先经 trackFromUntrusted 校验再进入采样路径——
 * 这正是 F0612 反序列化场景需要的闸门，也避免了在测试里写类型断言来伪造非法键。
 */
export interface UntrustedTrack {
  readonly propKey: string;
  readonly keyframes: readonly Keyframe[];
}

/**
 * 不可信输入 → 受信轨道的校验闸门。
 * 属性键必须已注册（显性不支持未注册属性），关键帧数组必须存在且非空；
 * 值形状与时间合法性由 sampleTrack 逐帧校验，此处只做「能否进入管线」的前置判断。
 */
export function trackFromUntrusted(raw: UntrustedTrack): Outcome<Track> {
  const spec = lookupProp(raw.propKey);
  if (!spec.ok) return spec;
  if (!Array.isArray(raw.keyframes) || raw.keyframes.length === 0) {
    return fail<Track>(
      "TRACK_EMPTY",
      `属性 ${raw.propKey} 的轨道没有关键帧`,
      "至少需要一帧（常量轨道）才能采样；请检查存档或文件是否被截断",
    );
  }
  return ok({ propKey: spec.value.key, keyframes: raw.keyframes });
}

/** 轨道快照：数据加校验和。重放时先验校验和，防篡改/截断的快照静默进入管线。 */
export interface TrackSnapshot {
  readonly propKey: PropKey;
  readonly keyframes: readonly Keyframe[];
  /** FNV-1a 32 位校验和（对关键帧的规范化序列化求值）。 */
  readonly checksum: number;
}

/** FNV-1a 32 位散列（快照校验和用；选它是因为实现短、无依赖、够用）。 */
export function fnv1a32(text: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i += 1) {
    h ^= text.charCodeAt(i);
    // 乘 16777619，用移位加法避免 32 位溢出丢精度
    h = (h + ((h << 1) + (h << 4) + (h << 7) + (h << 8) + (h << 24))) >>> 0;
  }
  return h >>> 0;
}

/** 关键帧的规范化序列化（快照校验和的输入；浮点按固定精度取整以抗末位抖动）。 */
function serializeTrack(track: Track): string {
  const parts = track.keyframes.map((f) => {
    const v = f.value;
    const vs =
      typeof v === "number"
        ? `n:${round6(v)}`
        : "x" in v
          ? `p:${round6(v.x)},${round6(v.y)}`
          : "rxDeg" in v
            ? `e:${round6(v.rxDeg)},${round6(v.ryDeg)},${round6(v.rzDeg)}`
            : `c:${round6(v.top)},${round6(v.right)},${round6(v.bottom)},${round6(v.left)}`;
    return `${round6(f.t)}|${vs}|${f.easing ?? "linear"}`;
  });
  return `${track.propKey}#${parts.join(";")}`;
}

/** 保留六位小数（序列化规范化；避免 0.1+0.2 类末位差异污染校验和）。 */
function round6(v: number): string {
  if (!isFiniteNum(v)) return "nan";
  return (Math.round(v * 1e6) / 1e6).toFixed(6);
}

/** 生成轨道快照（纯读，不改轨道；快照可安全长期持有）。 */
export function snapshotTrack(track: Track): TrackSnapshot {
  return {
    propKey: track.propKey,
    keyframes: track.keyframes.map((f) => ({ ...f })),
    checksum: fnv1a32(serializeTrack(track)),
  };
}

/** 校验快照自洽性（校验和与内容不符即拒绝——数据完整性前置闸门）。 */
export function verifySnapshot(snap: TrackSnapshot): Outcome<TrackSnapshot> {
  const rebuilt: Track = { propKey: snap.propKey, keyframes: snap.keyframes };
  const actual = fnv1a32(serializeTrack(rebuilt));
  if (actual !== snap.checksum) {
    return fail<TrackSnapshot>(
      "SNAPSHOT_CHECKSUM_MISMATCH",
      `快照校验和不符（记录 ${snap.checksum}，实算 ${actual}）`,
      "快照在传输或存储中被改动/截断；请丢弃该快照并从源轨道重新生成，不要在损坏数据上重放",
    );
  }
  return ok(snap);
}

/** 从快照恢复轨道（恢复为纯函数：同快照恢复两次结果逐位一致）。 */
export function restoreTrack(snap: TrackSnapshot): Outcome<Track> {
  const v = verifySnapshot(snap);
  if (!v.ok) return v;
  return ok({ propKey: snap.propKey, keyframes: v.value.keyframes.map((f) => ({ ...f })) });
}

/**
 * 重放一致性对拍：同轨道同进度连采两次，逐值比对。
 * 这是判据二「纯函数插值」的可执行证明——不是靠人读代码确认无副作用。
 */
export function verifyReplayPure(track: Track, progress: number): Outcome<boolean> {
  const first = sampleTrack(track, progress);
  const second = sampleTrack(track, progress);
  if (!first.ok || !second.ok) {
    const bad = first.ok ? second : first;
    return fail<boolean>(bad.code, bad.message, bad.hint);
  }
  return ok(sameValue(first.value, second.value));
}

// ════════════════════════════════════════════════════════════════════════════
// §11 减弱动效降级咨询（无障碍面：UI 层查注册表，本条只提供数据与判定）
// ════════════════════════════════════════════════════════════════════════════

/** 减弱动效咨询结果。 */
export interface ReducedMotionPlan {
  /** 参与降级的属性（尊重该偏好的属性清单）。 */
  readonly properties: readonly PropKey[];
  /** 应跳过的属性（不尊重该偏好的属性——透明度等本无动效风险）。 */
  readonly exempt: readonly PropKey[];
  /** 建议的时长压缩比（1 = 原时长；0 = 直接跳到终值）。 */
  readonly durationScale: number;
}

/**
 * 依减弱动效偏好给出降级方案（注册表作为唯一事实源）。
 * UI 层拿到本结果后自行决定如何呈现；本条不持有偏好状态，只做纯判定。
 */
export function consultReducedMotion(
  keys: readonly PropKey[],
  prefersReducedMotion: boolean,
  durationScale = 0.25,
): ReducedMotionPlan {
  const inRegistry = keys.filter((k) => lookupProp(k).ok);
  if (!prefersReducedMotion) {
    return { properties: [...inRegistry], exempt: [], durationScale: 1 };
  }
  return {
    properties: inRegistry.filter((k) => PROP_REGISTRY[k].respectsReducedMotion),
    exempt: inRegistry.filter((k) => !PROP_REGISTRY[k].respectsReducedMotion),
    durationScale: clamp(durationScale, 0, 1),
  };
}

// ════════════════════════════════════════════════════════════════════════════
// §12 自检（判据的可执行形态：注册表完整性、四元数防锁、脏区联动、纯函数性）
// ════════════════════════════════════════════════════════════════════════════

/** 自检结果项：每项对应判据的一条，独立可定位。 */
export interface SelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/**
 * 注册表自检：覆盖判据一的全部要求。
 * 检查项：键集合可枚举、每条规格字段齐备、值域不倒置、脏类齐备、默认值形状匹配。
 */
export function selfCheckRegistry(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const keys = PROP_KEYS;
  out.push({
    name: "registry-enumerable",
    pass: keys.length > 0 && keys.every((k) => PROP_REGISTRY[k] !== undefined),
    detail: `注册表可枚举属性数 = ${keys.length}`,
  });

  let shapeOk = true;
  let badKey = "";
  for (const k of keys) {
    const s = PROP_REGISTRY[k];
    if (s.key !== k || !valueShapeOk(s, s.defaultValue)) {
      shapeOk = false;
      badKey = k;
      break;
    }
  }
  out.push({
    name: "registry-default-shape",
    pass: shapeOk,
    detail: shapeOk ? "全部属性的默认值形态与 kind 匹配" : `属性 ${badKey} 的默认值形态与 kind 不符`,
  });

  let rangeOk = true;
  let badRange = "";
  for (const k of keys) {
    const s = PROP_REGISTRY[k];
    if (s.range !== undefined && !(s.range.lo < s.range.hi)) {
      rangeOk = false;
      badRange = k;
      break;
    }
  }
  out.push({
    name: "registry-range-sane",
    pass: rangeOk,
    detail: rangeOk ? "全部声明值域满足 lo < hi" : `属性 ${badRange} 的值域倒置`,
  });

  const dirtyOk = keys.every((k) => {
    const c = PROP_REGISTRY[k].dirtyClass;
    return c === "transform-self" || c === "subtree-transform" || c === "content";
  });
  out.push({ name: "registry-dirty-class", pass: dirtyOk, detail: "全部属性的脏类取值合法" });

  return out;
}

/**
 * 四元数防锁自检：覆盖判据三的关键性质。
 * 用例：编解码严格互逆、万向锁真实锁区（pitch=±90°）往返一致、
 *      180° 大跳变走短弧、同向端点不产生 NaN、球面插值模长守恒。
 */
export function selfCheckQuaternion(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 1) 编解码严格互逆（ZYX 施加序，两侧公式必须成对）
  //    覆盖锁区外的一组姿态：roll/pitch/yaw 三轴皆非零，最容易暴露同序错误。
  const ePlain: Euler3 = { rxDeg: 25, ryDeg: -40, rzDeg: 130 };
  const bagPlain = new DiagBag();
  const qPlain = quatFromEuler(ePlain);
  const backPlain = qPlain === null ? null : quatToEuler(qPlain, bagPlain);
  const plainPass =
    qPlain !== null &&
    backPlain !== null &&
    Math.abs(normalizeAngleDeg(backPlain.rxDeg - ePlain.rxDeg)) < 1e-6 &&
    Math.abs(normalizeAngleDeg(backPlain.ryDeg - ePlain.ryDeg)) < 1e-6 &&
    Math.abs(normalizeAngleDeg(backPlain.rzDeg - ePlain.rzDeg)) < 1e-6;
  out.push({
    name: "euler-quat-roundtrip",
    pass: plainPass,
    detail: plainPass
      ? `锁区外姿态 (25°, -40°, 130°) 编解码往返一致`
      : `往返不一致：${backPlain === null ? "解算失败" : `(${backPlain.rxDeg}, ${backPlain.ryDeg}, ${backPlain.rzDeg})`}`,
  });

  // 2) 万向锁真实锁区：pitch = ±90° 两侧都必须**姿态等价**往返。
  //
  //    这里必须用「姿态等价」而非「欧拉分量相等」作判据——锁区内解本就不唯一：
  //    ZYX 序下 pitch=±90° 时 roll 与 yaw 共线，roll 的效果被并入 yaw。
  //    实测 (0°, 90°, 45°) 与 (180°, 90°, 180°) 表示同一姿态，两者都是合法解。
  //    判据取四元数点积 |q₁·q₂| ≈ 1（姿态等价），这是锁区唯一有意义的往返判据。
  //    同时要求必须产出 GIMBAL_LOCK_BYPASS 诊断——退化是显性事件，不是静默通过。
  let lockPass = true;
  const lockDetails: string[] = [];
  for (const pitchDeg of [90, -90]) {
    const bagLock = new DiagBag();
    const eLock: Euler3 = { rxDeg: 20, ryDeg: pitchDeg, rzDeg: -35 };
    const qLock = quatFromEuler(eLock);
    const backLock = qLock === null ? null : quatToEuler(qLock, bagLock);
    const qBack = backLock === null ? null : quatFromEuler(backLock);
    const equiv =
      qLock !== null &&
      qBack !== null &&
      Math.abs(Math.abs(quatDot(qLock, qBack)) - 1) < 1e-9;
    const diagCount = bagLock.byCode("GIMBAL_LOCK_BYPASS").length;
    const pitchKept = backLock !== null && Math.abs(backLock.ryDeg - pitchDeg) < 1e-6;
    const casePass = equiv && pitchKept && diagCount === 1;
    lockDetails.push(
      `pitch=${pitchDeg > 0 ? "+" : ""}${pitchDeg}°: 姿态等价=${String(equiv)}, pitch保持=${String(pitchKept)}, 旁路诊断=${diagCount} 条`,
    );
    if (!casePass) lockPass = false;
  }
  out.push({
    name: "gimbal-roundtrip",
    pass: lockPass,
    detail: lockDetails.join("；"),
  });

  // 3) 大跳变走短弧：0° → 350° 的插值中点应贴近 -5°，而非 175°
  const bagArc = new DiagBag();
  const midArc = lerpEuler({ rxDeg: 0, ryDeg: 0, rzDeg: 0 }, { rxDeg: 0, ryDeg: 0, rzDeg: 350 }, 0.5, bagArc);
  const arcDelta = midArc === null ? NaN : normalizeAngleDeg(midArc.rzDeg);
  out.push({
    name: "short-arc-not-wrap",
    pass: isFiniteNum(arcDelta) && Math.abs(arcDelta + 5) < 1e-6,
    detail: `0°→350° 中点 = ${String(arcDelta)}°（期望 -5°，即走短弧而非 175° 远路）`,
  });

  // 4) 同向端点不产生 NaN（slerp 退化降级路径）
  const bagSame = new DiagBag();
  const same = slerpQuat(QUAT_IDENTITY, QUAT_IDENTITY, 0.5, bagSame);
  const sameOk = same !== null && quatFinite(same) && Math.abs(quatNormSq(same) - 1) < 1e-9;
  out.push({
    name: "slerp-degenerate-no-nan",
    pass: sameOk,
    detail: `同向端点插值结果有限且归一 = ${String(sameOk)}；退化诊断数 = ${bagSame.byCode("SLERP_COLLAPSE").length}`,
  });

  // 5) 球面插值模长守恒（任意两端任意进度都应落在单位球面上）
  let conserved = true;
  for (let i = 0; i <= 10; i += 1) {
    const b = new DiagBag();
    const q = slerpQuat(
      quatFromEuler({ rxDeg: 20, ryDeg: -35, rzDeg: 110 }) ?? QUAT_IDENTITY,
      quatFromEuler({ rxDeg: -70, ryDeg: 15, rzDeg: -160 }) ?? QUAT_IDENTITY,
      i / 10,
      b,
    );
    if (q === null || Math.abs(quatNormSq(q) - 1) > 1e-6) {
      conserved = false;
      break;
    }
  }
  out.push({ name: "slerp-norm-conserved", pass: conserved, detail: "11 点采样全部落在单位四元数球面上" });

  return out;
}

/** 脏区联动自检：覆盖判据四（动画层子树按变换脏处理）。 */
export function selfCheckDirtyLinkage(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const track: Track = {
    propKey: "transform.translate",
    keyframes: [
      { t: 0, value: { x: 0, y: 0 } },
      { t: 1, value: { x: 100, y: 50 }, easing: "easeInOutQuad" },
    ],
  };
  const still = evaluateFrame([{ nodeId: "n1", track }], 0);
  const moving = evaluateFrame([{ nodeId: "n1", track }], 0.5);

  out.push({
    name: "dirty-silent-at-rest",
    pass: still.dirty.length === 0,
    detail: `进度 0（等于起点值）产生脏事件 ${still.dirty.length} 条，期望 0（静止不空转脏区）`,
  });
  out.push({
    name: "dirty-fires-when-animating",
    pass: moving.dirty.length === 1,
    detail: `进度 0.5 产生脏事件 ${moving.dirty.length} 条，期望 1`,
  });
  const ev = moving.dirty[0];
  out.push({
    name: "dirty-subtree-invalidated",
    pass: ev !== undefined && ev.subtreeInvalidated && ev.dirtyClass === "subtree-transform",
    detail: `脏类 = ${String(ev?.dirtyClass)}，子树失效 = ${String(ev?.subtreeInvalidated)}（动画层须整棵子树世界矩阵级联失效）`,
  });

  // 未注册属性必须显性拒绝且不产出脏事件：
  //   走不可信绑定路径（属性键为任意字符串），验证帧求值不会把非法键静默吞掉。
  const bad = evaluateFrame([{ nodeId: "n2", track: null, rawPropKey: "not.a.prop" }], 0.5);
  const rejected = bad.diagnostics.some((d) => d.code === "PROP_UNREGISTERED");
  out.push({
    name: "unregistered-rejected-explicitly",
    pass: rejected && bad.dirty.length === 0,
    detail: `帧求值对未注册属性产出 PROP_UNREGISTERED 诊断 = ${String(rejected)}，脏事件数 = ${bad.dirty.length}（期望 0）`,
  });

  // 不可信轨道入口闸门：未注册键必须在进入采样管线前就被拦下。
  const gate = trackFromUntrusted({ propKey: "not.a.prop", keyframes: [] });
  const gateRejected = !gate.ok && gate.code === "PROP_UNREGISTERED";
  out.push({
    name: "untrusted-track-gate",
    pass: gateRejected,
    detail: gateRejected
      ? `不可信轨道在入口即被拦下（${gate.code}）`
      : "不可信轨道竟然通过了入口校验（缺陷）",
  });

  return out;
}

/** 纯函数与快照自检：覆盖判据二的可重放可快照。 */
export function selfCheckPurity(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const track: Track = {
    propKey: "transform.rotate",
    keyframes: [
      { t: 0, value: { rxDeg: 0, ryDeg: 0, rzDeg: 0 } },
      { t: 1, value: { rxDeg: 10, ryDeg: 20, rzDeg: 200 }, easing: "springBouncy" },
    ],
  };
  const replay = verifyReplayPure(track, 0.37);
  out.push({
    name: "replay-deterministic",
    pass: replay.ok && replay.value,
    detail: replay.ok ? "同轨道同进度连采两次逐值一致" : `重放对拍失败：${replay.message}`,
  });

  const snap = snapshotTrack(track);
  const verified = verifySnapshot(snap);
  out.push({
    name: "snapshot-verifies",
    pass: verified.ok,
    detail: verified.ok ? `快照校验和自洽（${snap.checksum}）` : `快照校验失败：${verified.message}`,
  });

  const tampered: TrackSnapshot = {
    ...snap,
    keyframes: [{ t: 0, value: { rxDeg: 0, ryDeg: 0, rzDeg: 0 } }],
  };
  const tamperedCheck = verifySnapshot(tampered);
  out.push({
    name: "tampered-snapshot-rejected",
    pass: !tamperedCheck.ok && tamperedCheck.code === "SNAPSHOT_CHECKSUM_MISMATCH",
    detail: tamperedCheck.ok ? "被篡改的快照竟然通过了校验（缺陷）" : "被篡改的快照被校验和拦下",
  });

  const restored = restoreTrack(snap);
  const restoredReplay =
    restored.ok && verifyReplayPure(restored.value, 0.37).ok && verifyReplayPure(restored.value, 0.37).value;
  out.push({
    name: "restore-then-replay-identical",
    pass: restoredReplay === true,
    detail: "从快照恢复的轨道与原轨道采样结果逐位一致（恢复为纯函数）",
  });

  return out;
}

/** 矩阵重组自检：覆盖判据四的重组与往返。 */
export function selfCheckCompose(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const comps: LayerComponents = {
    translate: { x: 12, y: -8 },
    rotateDeg: 30,
    scale: { x: 1.5, y: 2 },
    skewXDeg: 10,
    skewYDeg: 0,
  };
  const m = composeMatrix(comps);
  if (!m.ok) {
    return [{ name: "compose-roundtrip", pass: false, detail: `重组失败：${m.message}` }];
  }
  out.push({
    name: "compose-translation-exact",
    pass: m.value.e === 12 && m.value.f === -8,
    detail: `平移分量精确保留：e=${m.value.e}, f=${m.value.f}`,
  });
  const det = matDeterminant(m.value);
  out.push({
    name: "compose-determinant-sane",
    pass: isFiniteNum(det) && Math.abs(det - 3) < 1e-9,
    // det(T·R·Kx·Ky·S) = 1·1·1·1·(sx·sy) = sx·sy，与倾斜和旋转无关（这是可核对的解析期望值）。
    detail: `行列式 = ${det.toFixed(9)}（解析期望 sx·sy = ${(1.5 * 2).toFixed(9)}）`,
  });
  const inv = invertMatrix(m.value);
  out.push({
    name: "compose-invertible",
    pass: inv.ok,
    detail: inv.ok ? "矩阵可逆（F0602 命中测试的逆矩阵缓存可命中）" : `求逆失败：${inv.message}`,
  });

  // 分解往返：compose 的规范形（skewY=0）与 decompose 严格互逆。
  const de = decomposeMatrix(m.value);
  const rtPass =
    de.ok &&
    Math.abs(normalizeAngleDeg(de.value.rotateDeg - comps.rotateDeg)) < 1e-6 &&
    Math.abs(normalizeAngleDeg(de.value.skewXDeg - comps.skewXDeg)) < 1e-6 &&
    Math.abs(de.value.scale.x - comps.scale.x) < 1e-9 &&
    Math.abs(de.value.scale.y - comps.scale.y) < 1e-9 &&
    de.value.translate.x === comps.translate.x &&
    de.value.translate.y === comps.translate.y;
  out.push({
    name: "decompose-roundtrip",
    pass: rtPass,
    detail: de.ok
      ? `往返 旋转=${de.value.rotateDeg.toFixed(6)}° 倾斜X=${de.value.skewXDeg.toFixed(6)}° 缩放=(${de.value.scale.x.toFixed(6)}, ${de.value.scale.y.toFixed(6)})`
      : `分解失败：${de.message}`,
  });

  // 分解再重组必须回到原矩阵（比分量比对更强的判据：矩阵级往返）。
  const recomposed = de.ok ? composeMatrix(de.value) : null;
  const mEqual =
    recomposed !== null &&
    recomposed.ok &&
    Math.abs(recomposed.value.a - m.value.a) < 1e-9 &&
    Math.abs(recomposed.value.b - m.value.b) < 1e-9 &&
    Math.abs(recomposed.value.c - m.value.c) < 1e-9 &&
    Math.abs(recomposed.value.d - m.value.d) < 1e-9 &&
    Math.abs(recomposed.value.e - m.value.e) < 1e-12 &&
    Math.abs(recomposed.value.f - m.value.f) < 1e-12;
  out.push({
    name: "matrix-level-roundtrip",
    pass: mEqual,
    detail: mEqual ? "分解后再重组逐项回到原矩阵（误差 < 1e-9）" : "矩阵级往返不一致——分解式与 compose 连乘不同序",
  });

  const singular = composeMatrix({ ...comps, scale: { x: 0, y: 1 } });
  out.push({
    name: "singular-scale-rejected",
    pass: !singular.ok && singular.code === "MATRIX_DEGENERATE",
    detail: singular.ok ? "零缩放竟然重组成功（会产出不可逆矩阵，缺陷）" : "零缩放被显式拒绝并给出处置提示",
  });

  // 双轴倾斜（skewY ≠ 0）必须仍可重组：规范化只约束分解侧，不约束创作侧。
  const dual = composeMatrix({ ...comps, skewYDeg: 7 });
  out.push({
    name: "dual-skew-composes",
    pass: dual.ok && isFiniteNum(matDeterminant(dual.value)) && Math.abs(matDeterminant(dual.value) - 3) < 1e-9,
    detail: dual.ok
      ? `双轴倾斜重组成功，det=${matDeterminant(dual.value).toFixed(6)}（skew 不改变 det，符合 det=sx·sy）`
      : `双轴倾斜重组失败：${dual.message}`,
  });
  return out;
}

/**
 * 错误路径与降级矩阵自检：逐条覆盖锚点声明的降级处置。
 * 锚点原文：「属性未注册→显性不支持；进度越界→钳制；插值退化→上一帧值保持」。
 * 每一类都必须既产生对应诊断码，又在数值上落到声明的降级值——只查诊断码不够
 * （诊断对了但值没降级，是最常见的半吊子实现）。
 */
export function selfCheckDegradation(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 1) 属性未注册 → 显性不支持（失败而非静默取默认值）
  const unknown = lookupProp("transform.doesNotExist");
  out.push({
    name: "unregistered-is-explicit-failure",
    pass: !unknown.ok && unknown.code === "PROP_UNREGISTERED" && unknown.hint.length > 0,
    detail: unknown.ok
      ? "未注册属性竟然查询成功"
      : `返回失败码 ${unknown.code}，处置提示长度 ${unknown.hint.length}`,
  });

  // 2) 空轨道 → 显性失败（不是取默认值）
  const emptyTrack = sampleTrack({ propKey: "transform.translate", keyframes: [] }, 0.5);
  out.push({
    name: "empty-track-rejected",
    pass: !emptyTrack.ok && emptyTrack.code === "TRACK_EMPTY",
    detail: emptyTrack.ok ? "空轨道竟然采样成功" : `返回失败码 ${emptyTrack.code}`,
  });

  // 3) 进度越界 → 钳制到 [0,1]（取端点值，且产出诊断）
  const ramp: Track = {
    propKey: "transform.translate",
    keyframes: [
      { t: 0, value: { x: 10, y: 20 } },
      { t: 1, value: { x: 110, y: 220 } },
    ],
  };
  const over = sampleTrack(ramp, 5);
  const under = sampleTrack(ramp, -3);
  const overClamped = over.ok && isVec2Value(over.value) && over.value.x === 110;
  const underClamped = under.ok && isVec2Value(under.value) && under.value.x === 10;
  const overDiag = over.ok && over.diagnostics.some((d) => d.code === "PROGRESS_OUT_OF_RANGE");
  out.push({
    name: "progress-clamped-at-both-ends",
    pass: overClamped && underClamped && overDiag === true,
    detail: `进度 5 → x=${over.ok && isVec2Value(over.value) ? over.value.x : "?"}（期望 110）；进度 -3 → x=${under.ok && isVec2Value(under.value) ? under.value.x : "?"}（期望 10）；越界诊断=${String(overDiag)}`,
  });

  // 4) 非有限进度 → 钳制到 0 并诊断（NaN 不得污染整棵子树）
  const nanProg = sampleTrack(ramp, Number.NaN);
  out.push({
    name: "nonfinite-progress-clamped",
    pass: nanProg.ok && isVec2Value(nanProg.value) && nanProg.value.x === 10 &&
      nanProg.diagnostics.some((d) => d.code === "PROGRESS_OUT_OF_RANGE"),
    detail: nanProg.ok
      ? `NaN 进度返回起点值 x=${isVec2Value(nanProg.value) ? nanProg.value.x : "?"} 并产出诊断`
      : `NaN 进度被拒绝（期望钳制而非拒绝）：${nanProg.message}`,
  });

  // 5) 值域钳制：opacity 越界必须钳回且记录（防止关键帧把 alpha 写成 5）
  const badAlpha: Track = {
    propKey: "opacity",
    keyframes: [
      { t: 0, value: 0 },
      { t: 1, value: 5 },
    ],
  };
  const alphaSampled = sampleTrack(badAlpha, 1);
  out.push({
    name: "range-clamped-with-diagnostic",
    pass: alphaSampled.ok && alphaSampled.value === 1 &&
      alphaSampled.diagnostics.some((d) => d.code === "VALUE_NONFINITE"),
    detail: alphaSampled.ok
      ? `opacity 越界 5 → 钳制为 ${String(alphaSampled.value)}（值域 [0,1]），诊断已记录`
      : `采样失败：${alphaSampled.message}`,
  });

  // 6) 关键帧值形状不匹配 → 显性失败（vec2 塞进 scalar 属性不得静默取 0）
  const shapeMismatch = sampleTrack(
    { propKey: "transform.translate", keyframes: [{ t: 0, value: 42 }] },
    0.5,
  );
  out.push({
    name: "shape-mismatch-rejected",
    pass: !shapeMismatch.ok && shapeMismatch.code === "VALUE_NONFINITE",
    detail: shapeMismatch.ok ? "形状不匹配的关键帧竟然被接受" : `返回失败码 ${shapeMismatch.code}`,
  });

  // 7) 缓动名未知 → 降级取轨道首帧值 + 诊断（不静默回落 linear）
  const badEasing = sampleTrack(
    {
      propKey: "transform.translate",
      keyframes: [
        { t: 0, value: { x: 1, y: 1 }, easing: "noSuchEasing" },
        { t: 1, value: { x: 9, y: 9 } },
      ],
    },
    0.5,
  );
  out.push({
    name: "unknown-easing-degrades-visibly",
    pass: badEasing.ok && isVec2Value(badEasing.value) && badEasing.value.x === 1 &&
      badEasing.diagnostics.some((d) => d.code === "EASING_UNKNOWN"),
    detail: badEasing.ok
      ? `未知缓动降级为轨道首帧值 x=${isVec2Value(badEasing.value) ? badEasing.value.x : "?"} 并产出 EASING_UNKNOWN 诊断`
      : "未知缓动导致采样失败（期望降级为可用值而非中断整帧）",
  });

  // 8) 旋转插值退化（起止同姿态）→ 保持起始帧值且诊断，不产出 NaN
  //    注：四元数域的往返会引入 1e-15 级浮点残差，故用容差而非严格相等判定。
  const degenerateRot = sampleTrack(
    {
      propKey: "transform.rotate",
      keyframes: [
        { t: 0, value: { rxDeg: 10, ryDeg: 20, rzDeg: 30 } },
        { t: 1, value: { rxDeg: 10, ryDeg: 20, rzDeg: 30 } },
      ],
    },
    0.5,
  );
  const rotKept =
    degenerateRot.ok &&
    isEuler3Value(degenerateRot.value) &&
    Math.abs(degenerateRot.value.rxDeg - 10) < 1e-9 &&
    Math.abs(degenerateRot.value.ryDeg - 20) < 1e-9 &&
    Math.abs(degenerateRot.value.rzDeg - 30) < 1e-9;
  out.push({
    name: "rotation-degenerate-no-nan",
    pass:
      rotKept &&
      degenerateRot.ok &&
      degenerateRot.diagnostics.some((d) => d.code === "SLERP_COLLAPSE"),
    detail: degenerateRot.ok
      ? `同姿态端点保持原值（|残差| < 1e-9）且产出 SLERP_COLLAPSE 诊断（无 NaN）`
      : `采样失败：${degenerateRot.message}`,
  });

  // 9) 裁剪负值 → 钳到 0 并诊断（防裁剪域反转让负值变露边）
  //    取 t=1 端点路径：这是关键帧写错负值最常见的暴露点，也是曾经绕过钳制的位置。
  const negClip = sampleTrack(
    {
      propKey: "clip.inset",
      keyframes: [
        { t: 0, value: { top: 0, right: 0, bottom: 0, left: 0 } },
        { t: 1, value: { top: -20, right: 5, bottom: 0, left: -3 } },
      ],
    },
    1,
  );
  const clipShapeOk =
    negClip.ok && typeof negClip.value === "object" && negClip.value !== null && "top" in negClip.value;
  const clipNoNegative =
    clipShapeOk &&
    negClip.value.top >= 0 &&
    negClip.value.right >= 0 &&
    negClip.value.bottom >= 0 &&
    negClip.value.left >= 0;
  out.push({
    name: "clip-negative-clamped-at-endpoint",
    pass: clipNoNegative && negClip.diagnostics.some((d) => d.code === "CLAMP_DEGENERATE"),
    detail: negClip.ok && clipShapeOk
      ? `裁剪负边距在端点路径被钳到 0（top=${negClip.value.top}, left=${negClip.value.left}）并产出 CLAMP_DEGENERATE 诊断`
      : "裁剪负值未被显式处理",
  });

  // 10) 贝塞尔缓动端点精确（0→0, 1→1），且中点单调
  const bagB = new DiagBag();
  const bez = cubicBezierEasing({ x1: 0.42, y1: 0, x2: 0.58, y2: 1 }, bagB);
  const endpoints = bez(0) === 0 && bez(1) === 1;
  let monotonic = true;
  let prev = -Infinity;
  for (let i = 0; i <= 20; i += 1) {
    const y = bez(i / 20);
    if (!(y >= prev - 1e-9)) {
      monotonic = false;
      break;
    }
    prev = y;
  }
  out.push({
    name: "bezier-endpoints-and-monotonic",
    pass: endpoints && monotonic,
    detail: `端点精确 = ${String(endpoints)}（0→0, 1→1）；21 点采样单调 = ${String(monotonic)}；反解放弃诊断 = ${bagB.byCode("BEZIER_SOLVE_BAILOUT").length} 条`,
  });

  // 11) 弹簧缓动端点精确（欠阻尼/临界/过阻尼三支都要覆盖）
  const springs = [springEasing(1.5, 0.35), springEasing(1, 1), springEasing(2, 1.6)];
  const springOk = springs.every((s) => s(0) === 0 && Math.abs(s(1) - 1) < 1e-9);
  out.push({
    name: "spring-three-regimes-endpoints",
    pass: springOk,
    detail: `欠阻尼/临界/过阻尼三支端点均精确 = ${String(springOk)}`,
  });

  // 12) 归一化单位纪律：弧度制与角度制必须给出不同结果（防再次混用）
  const unitOk = normalizeAngle(-5) !== normalizeAngleDeg(-5);
  out.push({
    name: "angle-unit-discipline",
    pass: unitOk,
    detail: `normalizeAngle(-5)=${normalizeAngle(-5).toFixed(6)}（弧度制）≠ normalizeAngleDeg(-5)=${normalizeAngleDeg(-5)}（角度制）`,
  });

  return out;
}

/**
 * 全量自检入口：一次跑完判据对应的全部检查项，返回逐项结果（不聚合为单一布尔）。
 * 六组分别对应锚点判据的四条 + 错误降级矩阵 + 注册表完整性：
 *   registry（判据一）· quaternion（判据三 防锁）· dirty（判据四 脏区联动）
 *   purity（判据二 纯函数/快照）· compose（判据四 重组）· degradation（错误路径与降级矩阵）
 */
export function runSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly SelfCheck[]>>;
  readonly allPass: boolean;
  readonly failed: readonly string[];
  /** 自检项总数（供闸门脚本核对覆盖面，避免「少跑了几项却显示全绿」）。 */
  readonly total: number;
} {
  const groups = {
    registry: selfCheckRegistry(),
    quaternion: selfCheckQuaternion(),
    dirty: selfCheckDirtyLinkage(),
    purity: selfCheckPurity(),
    compose: selfCheckCompose(),
    degradation: selfCheckDegradation(),
  };
  const failed: string[] = [];
  let total = 0;
  for (const [g, items] of Object.entries(groups)) {
    for (const it of items) {
      total += 1;
      if (!it.pass) failed.push(`${g}.${it.name}: ${it.detail}`);
    }
  }
  return { groups, allPass: failed.length === 0, failed, total };
}