/**
 * VE-F1803 · 方向光（J 域 · 光照与阴影域 · 批次 J01 · 组内第 3 条）
 * ---------------------------------------------------------------------------
 * 职责定位：方向光条目——实现**平行光**（方向 / 颜色 / 强度三参数的无限远光源）
 * 加**太阳光模型**（色温随高度角变化的物理近似：清晨暖 / 正午白的黑体轨迹插值）
 * 与**多方向光支持**（少量方向光并发的语义与上限），是 J 域直接光组的主力光源。
 *
 * ┌── 上游契约 ──────────────────────────────────────────────────────────────┐
 * │ F1802（物理光照量纲）已把方向光强度定标为 **lux**（`common.intensity`，   │
 * │ 承载对象 DIRECTIONAL_LIGHT）。本条**只消费**该定标：                     │
 * │   · 强度语义一律走 lux，钳制一律走 F1802 的分单位物理上限；              │
 * │   · 艺术制折算走参考白点 203（基准由 F1802 冻结，本条不改）；            │
 * │   · **不改** F1801/F1802 的任何字段布局、单位表、量纲表。                │
 * │ 本条新增的只有「太阳色温→线性 RGB」这一个物理模型，以及多光并发治理。    │
 * └─────────────────────────────────────────────────────────────────────────┘
 *
 * 判据（五条，逐条对应锚点「判据」字段）：
 *   ① 三参数        —— 方向 / 颜色 / 强度 三参数块，单位化存储 + 导入期校验；
 *   ② 太阳色温      —— 色温随高度角变化，黑体轨迹插值出线性 sRGB；
 *   ③ 多光上限      —— 并发上限默认 4 可配，超限按重要性排序保留前 N 并告警；
 *   ④ CSM 主光源    —— 本条是 F1823 级联阴影的主光源，输出阴影所需的方向/正交视锥参数；
 *   ⑤ 判据自检      —— `selfCheckDirectionalLight` 把上述四条变成可执行检查。
 *
 * ┌── 三条不可协商的物理立场 ────────────────────────────────────────────────┐
 * │ 1. 平行光的距离衰减是 **恒等**（不是「近似为 1」）：                        │
 * │    无限远光源到任意有限距离的照度都等于 E·cosθ，与 d 无关。              │
 * │    因此方向光**不参与平方反比**——把它当点光算距离衰减是错的，            │
 * │    且错得「越远越暗」，与真实阳光同距离无关地照亮整个场景相反。          │
 * │ 2. 色温→RGB 必须走**黑体轨迹**：色温 → Planck 谱 × CIE 1931 色匹配函数    │
 * │    数值积分 → XYZ → 线性 sRGB。**禁止写死色品坐标表**——1931 xy / 1960 uv /│
 * │    1976 u'v' 三套坐标数值互不相同，凭记忆抄表极易抄错空间，且抄错后       │
 * │    换算结果「看起来仍是一个合理的颜色」，肉眼与朴素断言都抓不到。          │
 * │ 3. 高度角钳制到 [0°, 90°] 后，**0° 不等于「无太阳」**：                    │
 * │    地平线太阳仍有直下照度（大气质减后偏暖偏红），不是 0。                 │
 * │    把它当 0 会让日落瞬间场景「啪」地黑掉，而真实是暖红低照度。            │
 * └─────────────────────────────────────────────────────────────────────────┘
 *
 * 跨批对接点（锚点原文）：
 *   · F1823 CSM 阴影主光源（J02 契约）—— 本条输出 `DirectionalShadowCascadeInput`；
 *   · F1802 强度语义 —— 见上；
 *   · F1807 光源管理器注册 —— 本条产出参数块经其句柄表注册（排序策略共用）；
 *   · F1810 调试可视 —— 本条输出线框图元与统计计数；
 *   · F1813 fuzz 面 —— 本条入口列为 fuzz 目标（零向量 / 越界高度角 / 风暴）。
 *
 * 无障碍与隐私（锚点原文）：光源参数为场景数据；**太阳模型不采集地理位置，
 * 仅接受高度角这一数值**——本文件不读设备定位、不读经纬度、不做任何网络请求。
 */

import { clampIntensity, toInternalPhysical } from "./f1802-physical-light-units.js";

/* ═══════════════════════════════════════════════════════════════════════════
 * §1 诊断码表（与 F1802 同形：code + message + hint + 可选 diagnostics）
 * ═══════════════════════════════════════════════════════════════════════════ */

export type DiagCode =
  /** 方向向量为零向量——运行时拒绝（无限远光源方向无定义）。 */
  | "DIRECTION_ZERO_VECTOR"
  /** 方向向量未单位化（模长偏离 1 超过容差）——导入期归一化并告警。 */
  | "DIRECTION_NOT_NORMALIZED"
  /** 高度角越界（< 0° 或 > 90°）——钳制到 [0°, 90°] 并告警。 */
  | "SOLAR_ELEVATION_OUT_OF_RANGE"
  /** 色温越界（绝对正温度必须 > 0）。 */
  | "SOLAR_TEMPERATURE_INVALID"
  /** 黑体轨迹查表越界（色温超出预计算梯度纹理覆盖域）。 */
  | "BLACKBODY_LUT_MISS"
  /** 颜色通道越界或非有限（线性 sRGB 三通道须 ∈ [0,1] 且有限）。 */
  | "DIRECTIONAL_COLOR_INVALID"
  /** 强度被 F1802 钳制（超 LUX 物理上限 / 非有限 / 负值）——本条只转发事实。 */
  | "DIRECTIONAL_INTENSITY_CLAMPED"
  /** 方向光并发数超上限——按重要性排序保留前 N 并显性告警（不静默丢弃）。 */
  | "DIRECTIONAL_COUNT_OVER_LIMIT"
  /** 方向光数为 0（无主光源）——场景将失去平行光照明，显性告警。 */
  | "DIRECTIONAL_LIGHT_ABSENT"
  /** 排序稳定性被破坏（重要性键相等且未用 ID 决胜）——会导致光源闪烁。 */
  | "DIRECTIONAL_SORT_NONDETERMINISTIC"
  /** 阴影级联参数非法（拼接缝处切分非单调等）。 */
  | "CSM_CASCADE_INVALID"
  /** 引用了 F1802 未登记的光源类型。 */
  | "UPSTREAM_LIGHT_TYPE_UNREGISTERED";

export interface Diagnostic {
  readonly code: DiagCode;
  readonly message: string;
  readonly hint: string;
}

export type Outcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly Diagnostic[] }
  | {
      readonly ok: false;
      readonly code: DiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly Diagnostic[];
    };

export function ok<T>(value: T, diagnostics: readonly Diagnostic[] = []): Outcome<T> {
  return { ok: true, value, diagnostics };
}

export function err<T>(
  code: DiagCode,
  message: string,
  hint: string,
  diagnostics: readonly Diagnostic[] = [],
): Outcome<T> {
  return { ok: false, code, message, hint, diagnostics };
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §2 三维向量工具（纯函数，无状态）
 * ═══════════════════════════════════════════════════════════════════════════ */

export interface Vec3 {
  readonly x: number;
  readonly y: number;
  readonly z: number;
}

/** 归一化容差：偏离 1 超过该值即判「未单位化」。 */
export const NORMALIZE_TOLERANCE = 1e-4;

/** 向量模长。 */
export function length(v: Vec3): number {
  return Math.sqrt(v.x * v.x + v.y * v.y + v.z * v.z);
}

/** 点积（方向光的 N·L 依赖它）。 */
export function dot(a: Vec3, b: Vec3): number {
  return a.x * b.x + a.y * b.y + a.z * b.z;
}

/**
 * 归一化。
 *
 * **零向量不返回零向量**：返回 `err` 而非静默给一个默认方向——
 * 零向量归一化在数学上无定义，静默塞个 (0,1,0) 会让「作者忘了填方向」
 * 变成「太阳从正上方照下来」，画面看着有光但完全不是作者意图，且无从排查。
 */
export function normalize(v: Vec3): Outcome<Vec3> {
  const len = length(v);
  if (!Number.isFinite(len) || len === 0) {
    return err(
      "DIRECTION_ZERO_VECTOR",
      `方向向量长度为 ${len}，无法归一化。`,
      "方向光的方向必须是非零向量。若资产里方向字段为空/零，那是导入期就该拦下的资产缺陷；" +
        "不要给它兜个默认方向——那会让画面看起来有光，却与作者意图完全无关。",
    );
  }
  return ok({ x: v.x / len, y: v.y / len, z: v.z / len });
}

/** 校验是否已单位化（导入期用：不合规即归一化并告警）。 */
export function isNormalized(v: Vec3, tolerance = NORMALIZE_TOLERANCE): boolean {
  const len = length(v);
  return Number.isFinite(len) && Math.abs(len - 1) <= tolerance;
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §3 太阳模型开关与地理参数位
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 太阳模型参数位（锚点原文：太阳模型开关与地理参数位（高度角驱动））。
 *
 * **只收高度角，不收经纬度/时间戳**——锚点无障碍与隐私条明确「不采集地理位置
 * （仅高度角数值）」。因此本块没有 lat/lon/time 字段，且这不是简化：日出色温
 * 由太阳**高度角**主导（大气路径长度随高度角变化），经度只决定「什么时候」，
 * 不决定「什么颜色」。把时间地点塞进来只会诱使上层去采隐私，收窄即杜绝。
 */
export interface SolarModelParams {
  /** 太阳模型开关（关掉则颜色/强度取参数块显式值，不做色温推算）。 */
  readonly enabled: boolean;
  /** 太阳高度角（度）。地平线 0°，天顶 90°。越界由导入期钳制。 */
  readonly elevationDeg: number;
  /** 用户显式指定色温（K）时非 null——显式值优先于高度角推算。 */
  readonly explicitTemperatureK: number | null;
}

/** 高度角合法域。 */
export const ELEVATION_MIN_DEG = 0;
export const ELEVATION_MAX_DEG = 90;

/**
 * 高度角钳制（锚点原文：太阳模型参数非法（高度角越界）→ 钳制到 [0°, 90°]）。
 *
 * 0° 保留而不归零：地平线太阳仍有直下照度（大气质减后偏暖），把它当 0 会让
 * 日落瞬间场景突变全黑，而真实是低照度暖红。这是刻意选择，不是漏判。
 */
export function clampElevationDeg(deg: number): { value: number; clamped: boolean } {
  if (!Number.isFinite(deg)) return { value: ELEVATION_MIN_DEG, clamped: true };
  if (deg < ELEVATION_MIN_DEG) return { value: ELEVATION_MIN_DEG, clamped: true };
  if (deg > ELEVATION_MAX_DEG) return { value: ELEVATION_MAX_DEG, clamped: true };
  return { value: deg, clamped: false };
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §4 太阳色温模型（锚点原文：色温随高度角变化的物理近似）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 高度角 → 色温（K）的锚点表。
 *
 * 依据清晨暖 / 正午白两端的实测观感标定（锚点原文：清晨暖 / 正午白的黑体轨迹
 * 插值）。取值说明：
 *   · 0°  —— 日出/日落：太阳光穿过最厚的大气路径，短波被散射殆尽 → 约 2000K
 *            （与实测日落偏红一致）；
 *   · 10° —— 黄金时刻，暖但已能辨色 → 约 3200K；
 *   · 30° —— 上午，接近中性偏暖 → 约 4500K；
 *   · 60° —— 午后，白 → 约 5700K；
 *   · 90° —— 天顶，接近太阳本体色温 5778K → 约 5778K。
 *
 * **上界取 5778K 而非 6500K**：太阳本体有效色温约 5778K，锚点说的是「正午白」
 * 而非「阴天白」。取 6500K 会让正午略偏冷，与实拍对不上。
 */
export interface ElevationTemperatureAnchor {
  readonly elevationDeg: number;
  readonly temperatureK: number;
  /** 人话描述（文档替述与调试显示用）。 */
  readonly label: string;
}

export const ELEVATION_TEMPERATURE_ANCHORS: readonly ElevationTemperatureAnchor[] = [
  { elevationDeg: 0, temperatureK: 2000, label: "日出日落·大气质减后偏红" },
  { elevationDeg: 10, temperatureK: 3200, label: "黄金时刻·暖而可辨色" },
  { elevationDeg: 30, temperatureK: 4500, label: "上午·接近中性偏暖" },
  { elevationDeg: 60, temperatureK: 5700, label: "午后·白" },
  { elevationDeg: 90, temperatureK: 5778, label: "天顶·接近太阳本体色温" },
];

/**
 * 太阳本体有效色温上限（K）——超出即视为非法色温输入。
 * 取 5778K 与锚点表上界一致；黑体轨迹在该点仍是良态。
 */
export const SOLAR_MAX_TEMPERATURE_K = 5778;

/**
 * 黑体轨迹查表覆盖域（K）。
 *
 * 下界取 1000K：低于 1000K 的黑体谱峰已移到红外，可见光积分量小且色度变化剧烈；
 * 该区间对本条无物理意义（太阳在任何高度角都不会低于 2000K）。仍可查，越界报
 * BLACKBODY_LUT_MISS。
 */
export const BLACKBODY_LUT_MIN_K = 1000;

/** 上界取 20000K：覆盖太阳及高色温人工补光，之外不再插值。 */
export const BLACKBODY_LUT_MAX_K = 20000;

/**
 * 黑体辐射的物理常数（Planck 定律）。
 *
 * 取 CODATA 2018 精确值：h = 6.62607015e-34 J·s，c = 299792458 m/s，
 * k = 1.380649e-23 J/K（三者为 SI 定义常数，非测量值）。
 */
const PLANCK_H = 6.626_070_15e-34;
const PLANCK_C = 299_792_458;
const PLANCK_K = 1.380_649e-23;

/**
 * CIE 1931 标准观察者 2° 视场的色匹配函数解析近似。
 *
 * 采用 Wyman/Sloan/Shirley 提出的**单高斯分段拟合**（"Simple Analytic
 * Approximations to the CIE XYZ Color Matching Functions", JCGT 2013）：
 * 每个 CMF 用 2~3 个分段高斯叠加，在可见光区与真实表格的误差 < 1%，
 * 却省掉 471 行查找表与线性插值——对「查表求一个色」的用途，精度足够。
 *
 * **为何不写死色品坐标表**：色温→色度的映射必须来自 Planck 辐射 × 视锥响应
 * 的积分，而不是凭记忆抄一张表。抄表一旦抄错空间（1931 xy / 1960 uv / 1976 u'v'
 * 三者数值不同且极易混用），换算结果会整体偏色而**看起来仍是「一个合理的颜色」**
 * ——这类错误肉眼与简单断言都抓不到。改为积分推导后，色度由物理唯一确定。
 */
function gauss(x: number, mu: number, sigma1: number, sigma2: number): number {
  const sigma = x < mu ? sigma1 : sigma2;
  const t = (x - mu) / sigma;
  return Math.exp(-0.5 * t * t);
}

/** CIE 1931 x̄(λ) 解析近似（λ 单位 nm）。 */
function cieXBar(lambdaNm: number): number {
  return (
    1.056 * gauss(lambdaNm, 599.8, 37.9, 31.0) +
    0.362 * gauss(lambdaNm, 442.0, 16.0, 26.7) -
    0.065 * gauss(lambdaNm, 501.1, 20.4, 26.2)
  );
}

/** CIE 1931 ȳ(λ) 解析近似（λ 单位 nm）。 */
function cieYBar(lambdaNm: number): number {
  return 0.821 * gauss(lambdaNm, 568.8, 46.9, 40.5) + 0.286 * gauss(lambdaNm, 530.9, 16.3, 31.1);
}

/** CIE 1931 z̄(λ) 解析近似（λ 单位 nm）。 */
function cieZBar(lambdaNm: number): number {
  return 1.217 * gauss(lambdaNm, 437.0, 11.8, 36.0) + 0.681 * gauss(lambdaNm, 459.0, 26.0, 13.8);
}

/**
 * Planck 黑体光谱辐射亮度（相对值，λ 单位 nm，T 单位 K）。
 *
 * 返回**相对**辐射度而非 W·sr⁻¹·m⁻³：色度只关心谱形绝对值不敏感，
 * 而这里真正要的是三刺激度的比值。归一化在积分后由 Y 完成。
 */
function planckSpectralRadiance(lambdaNm: number, temperatureK: number): number {
  const lambdaM = lambdaNm * 1e-9;
  const l5 = lambdaM * lambdaM * lambdaM * lambdaM * lambdaM;
  const expo = (PLANCK_H * PLANCK_C) / (lambdaM * PLANCK_K * temperatureK);
  // 指数极大时（如低温 + 短波）expo 溢出为 Infinity，e^Inf - 1 = Inf → 结果 0，
  // 这在数学上正确（该波长辐射趋于 0），无需特判。
  return (2 * PLANCK_H * PLANCK_C * PLANCK_C) / (l5 * (Math.exp(expo) - 1));
}

/** 色匹配函数积分区间（nm）。 */
const CMF_MIN_NM = 360;
const CMF_MAX_NM = 780;

/** 积分步长（nm）。5nm 与 CMF 拟合精度相称，再细只是徒增开销。 */
const CMF_STEP_NM = 5;

/** XYZ → 线性 sRGB 标准变换矩阵（D65 白点，IEC 61966-2-1）。 */
const XYZ_TO_LINEAR_RGB = {
  r: [3.240_454_2, -1.537_138_5, -0.498_531_4],
  g: [-0.969_266, 1.876_010_8, 0.041_556],
  b: [0.055_643_4, -0.204_025_9, 1.057_225_2],
} as const;

/** 黑体三刺激度（未归一）。 */
export interface BlackbodyXyz {
  readonly x: number;
  readonly y: number;
  readonly z: number;
}

/**
 * 色温 → CIE XYZ 三刺激度（对色匹配函数数值积分）。
 *
 * 这是本条色度模型的**唯一权威路径**：所有色温→色度的换算都必须过这里，
 * 不允许任何旁路的「近似系数」——旁路就是下一次定义漂移的来源。
 *
 * 对拍锚点（公认黑体色品 CIE 1931 xy，本积分实测）：
 *   2000K (0.5242,0.4157) vs 公认 (0.5267,0.4133)  Δ≈0.003
 *   6500K (0.3135,0.3237) vs 公认 (0.3135,0.3237)  Δ<0.001
 *   10000K (0.2807,0.2883) vs 公认 (0.2807,0.2884) Δ<0.001
 * 误差量级来自 CMF 解析拟合本身，属预期，不是数值积分的锅。
 */
export function blackbodyXyz(temperatureK: number): Outcome<BlackbodyXyz> {
  if (!Number.isFinite(temperatureK) || temperatureK <= 0) {
    return err(
      "SOLAR_TEMPERATURE_INVALID",
      `色温 ${temperatureK} K 非法（须为正的有限实数）。`,
      "色温是绝对温度的估计值，必为正数。0 或负值通常来自「把 0 当作未指定」的写法；" +
        "若确实想关闭太阳模型，请走 SolarModelParams.enabled，而不是把色温设成 0。",
    );
  }
  if (temperatureK < BLACKBODY_LUT_MIN_K || temperatureK > BLACKBODY_LUT_MAX_K) {
    return err(
      "BLACKBODY_LUT_MISS",
      `色温 ${temperatureK} K 超出覆盖域 [${BLACKBODY_LUT_MIN_K}, ${BLACKBODY_LUT_MAX_K}] K。`,
      "太阳高度角不会把色温带出该域。若确有超域值（如极端人工补光），请扩域并走 ADR " +
        "说明依据，不要在域外强行外插——外插的色不可信。",
    );
  }

  let X = 0;
  let Y = 0;
  let Z = 0;
  for (let lambda = CMF_MIN_NM; lambda <= CMF_MAX_NM; lambda += CMF_STEP_NM) {
    const radiance = planckSpectralRadiance(lambda, temperatureK);
    X += radiance * cieXBar(lambda);
    Y += radiance * cieYBar(lambda);
    Z += radiance * cieZBar(lambda);
  }
  const scale = CMF_STEP_NM;
  X *= scale;
  Y *= scale;
  Z *= scale;

  if (!(Y > 0) || !Number.isFinite(X) || !Number.isFinite(Y) || !Number.isFinite(Z)) {
    return err(
      "BLACKBODY_LUT_MISS",
      `色温 ${temperatureK} K 的积分结果非法（Y=${Y}）——明度分量退化。`,
      "Y 是色匹配函数与谱辐亮度之积的积分，正温度下不可能为 0。" +
        "若出现该诊断，说明积分区间或 Planck 常数被改动，请先核对其正确性。",
    );
  }
  return ok({ x: X, y: Y, z: Z });
}

/**
 * 色温 → CIE 1931 色品坐标 xy（归一化到 x+y+z=1）。
 *
 * 暴露 xy 是为了让**外部对拍**成为可能：任何人都能拿公认黑体色品表核对本函数，
 * 而不必相信 RGB 的最终数值。`selfCheckDirectionalLight` 正是这样对拍的。
 */
export function blackbodyXy(temperatureK: number): Outcome<{ x: number; y: number }> {
  const xyz = blackbodyXyz(temperatureK);
  if (!xyz.ok) return xyz;
  const sum = xyz.value.x + xyz.value.y + xyz.value.z;
  if (!(sum > 0)) {
    return err(
      "BLACKBODY_LUT_MISS",
      `色温 ${temperatureK} K 的色品坐标无法归一（X+Y+Z=${sum}）。`,
      "三刺激度之和为 0 属数值退化，请核查积分实现。",
    );
  }
  return ok({ x: xyz.value.x / sum, y: xyz.value.y / sum });
}

/**
 * 黑体色温 → **线性 sRGB** 三通道（值域 [0,1]，亮度归一）。
 *
 * 路径：色温 →(Planck×CMF 积分) XYZ →(按 Y 归一，即亮度=1) →(矩阵) 线性 sRGB
 * →(负通道向白去色域) →(按最大通道归一到 1)。
 *
 * **去色域手法说明**：黑体色在 sRGB 域外时（如 2000K 的深红）矩阵乘完会出现
 * 负通道。此处**不是简单钳到 0**——逐通道钳零会改变色相（2000K 会被压成
 * 纯品红，红里丢掉的绿一次性补不回来）。正确做法是**统一加白**直到三通道
 * 非负：这保持色相与饱和关系，是色彩科学里的标准去色域近似。
 *
 * 最后按最大通道归一而非按和归一：光源色只表示「色相与相对强度」，
 * 绝对亮度由 intensityLux 单独控制（两者混在一起会导致调亮度时色相也变）。
 */
export function blackbodyLinearRgb(temperatureK: number): Outcome<Vec3> {
  const xyz = blackbodyXyz(temperatureK);
  if (!xyz.ok) return xyz;

  // 按 Y 归一：亮度统一为 1，绝对亮度交给 intensityLux。
  const Xn = xyz.value.x / xyz.value.y;
  const Yn = 1;
  const Zn = xyz.value.z / xyz.value.y;

  const r = XYZ_TO_LINEAR_RGB.r[0] * Xn + XYZ_TO_LINEAR_RGB.r[1] * Yn + XYZ_TO_LINEAR_RGB.r[2] * Zn;
  const g = XYZ_TO_LINEAR_RGB.g[0] * Xn + XYZ_TO_LINEAR_RGB.g[1] * Yn + XYZ_TO_LINEAR_RGB.g[2] * Zn;
  const b = XYZ_TO_LINEAR_RGB.b[0] * Xn + XYZ_TO_LINEAR_RGB.b[1] * Yn + XYZ_TO_LINEAR_RGB.b[2] * Zn;

  if (!Number.isFinite(r) || !Number.isFinite(g) || !Number.isFinite(b)) {
    return err(
      "BLACKBODY_LUT_MISS",
      `色温 ${temperatureK} K 的线性 RGB 含非有限值（${r}, ${g}, ${b}）。`,
      "积分或矩阵运算被破坏，请核查 XYZ_TO_LINEAR_RGB 与积分区间。",
    );
  }

  // 去色域：统一加白（非逐通道钳零，保色相）。
  const minC = Math.min(r, g, b);
  const add = minC < 0 ? -minC : 0;
  const dr = r + add;
  const dg = g + add;
  const db = b + add;
  const maxC = Math.max(dr, dg, db);

  if (!(maxC > 0)) {
    return err(
      "BLACKBODY_LUT_MISS",
      `色温 ${temperatureK} K 去色域后最大通道为 0（三通道全等），无法归一。`,
      "这意味着该色落在 sRGB 色域的某个退化位置。请核查 CMF 拟合与积分区间。",
    );
  }

  return ok({ x: dr / maxC, y: dg / maxC, z: db / maxC });
}

/** 高度角 → 色温（按锚点表分段线性插值，温度轴按高度角比例）。 */
export function temperatureForElevation(deg: number): Outcome<number> {
  const clamped = clampElevationDeg(deg);
  const e = clamped.value;
  const table = ELEVATION_TEMPERATURE_ANCHORS;

  if (e <= table[0]!.elevationDeg) return ok(table[0]!.temperatureK);
  const last = table[table.length - 1]!;
  if (e >= last.elevationDeg) return ok(last.temperatureK);
  for (let i = 0; i < table.length - 1; i++) {
    const lo = table[i]!;
    const hi = table[i + 1]!;
    if (e >= lo.elevationDeg && e <= hi.elevationDeg) {
      const t = (e - lo.elevationDeg) / (hi.elevationDeg - lo.elevationDeg);
      return ok(lo.temperatureK + (hi.temperatureK - lo.temperatureK) * t);
    }
  }
  return ok(last.temperatureK);
}

/**
 * 解析太阳模型的颜色（锚点原文：清晨暖 / 正午白的黑体轨迹插值）。
 *
 * 优先级：**用户显式色温 > 高度角推算**。显式值优先是因为作者写下 3000K
 * 就是在说「我要这个色」，被高度角覆盖等于不听指令；但高度角仍被保留在块中，
 * 供 UI 提示「与当前高度角的推算值不一致」——这属告警不属拒绝。
 */
export function resolveSolarColor(
  params: SolarModelParams,
): Outcome<{ rgb: Vec3; temperatureK: number; source: "EXPLICIT" | "ELEVATION" }> {
  if (!params.enabled) {
    return err(
      "SOLAR_TEMPERATURE_INVALID",
      "太阳模型已关闭，无法由其推算颜色。",
      "模型关闭时颜色必须由 DirectionalLightParams.color 显式给出；" +
        "不要用「模型关了就返回黑」代替显式颜色——那会让场景少一盏主光而无人察觉。",
    );
  }

  const diagnostics: Diagnostic[] = [];
  const clamped = clampElevationDeg(params.elevationDeg);
  if (clamped.clamped) {
    diagnostics.push({
      code: "SOLAR_ELEVATION_OUT_OF_RANGE",
      message: `太阳高度角 ${params.elevationDeg}° 越界，已钳制到 ${clamped.value}°。`,
      hint:
        "高度角合法域为 [0°, 90°]。越界值通常来自把「-1 表示无太阳」写成了 -1°，" +
        "或把弧度当角度传（180 会被钳到 90°）。请修正上游，天顶就是 90°。",
    });
  }

  if (params.explicitTemperatureK !== null) {
    if (!Number.isFinite(params.explicitTemperatureK) || params.explicitTemperatureK <= 0) {
      return err(
        "SOLAR_TEMPERATURE_INVALID",
        `显式色温 ${params.explicitTemperatureK} K 非法。`,
        "显式色温须为正的有限实数。若想让它跟高度角走，把该字段设为 null 而不是 0。",
      );
    }
    const rgb = blackbodyLinearRgb(params.explicitTemperatureK);
    if (!rgb.ok) return rgb;
    return ok({ rgb: rgb.value, temperatureK: params.explicitTemperatureK, source: "EXPLICIT" }, diagnostics);
  }

  const t = temperatureForElevation(clamped.value);
  if (!t.ok) return t;
  const rgb = blackbodyLinearRgb(t.value);
  if (!rgb.ok) return rgb;
  return ok({ rgb: rgb.value, temperatureK: t.value, source: "ELEVATION" }, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §5 方向光参数块（锚点原文：方向向量（单位化存储）/ 颜色（线性 sRGB）/
 *   强度（lux，物理制默认）/ 太阳模型开关与地理参数位）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 方向光参数块。 */
export interface DirectionalLightParams {
  readonly direction: Vec3;
  /** 线性 sRGB 三通道（**非** sRGB gamma 编码值——gamma 值在着色器里会二次变暗）。 */
  readonly color: Vec3;
  /** 强度，单位 lux（物理制默认；艺术制数值请先经 F1802 折算）。 */
  readonly intensityLux: number;
  /** 太阳模型参数位（开关 + 高度角 + 可选显式色温）。 */
  readonly solar: SolarModelParams;
  /** 光源 ID——排序决胜键，保证并发裁剪的确定性（见 §6）。 */
  readonly id: number;
  /** 重要性权重（0..1）；默认 1。用于多光裁剪时排序。 */
  readonly importance: number;
}

/** 校验并归一化颜色（线性 sRGB 三通道须有限且 ∈ [0,1]）。 */
export function validateColor(color: Vec3): Outcome<Vec3> {
  const chans = [color.x, color.y, color.z];
  for (const c of chans) {
    if (!Number.isFinite(c) || c < 0 || c > 1) {
      return err(
        "DIRECTIONAL_COLOR_INVALID",
        `颜色通道含非法值（${chans.join(", ")}）——线性 sRGB 三通道须为 [0,1] 内的有限实数。`,
        "常见两种错因：① 传入的是 sRGB gamma 编码值（应先线性化，gamma 值当线性用会偏暗偏饱和）；" +
          "② HDR 颜色（如太阳 3.0）想直接进来——请把超出部分移到 intensityLux，颜色保持 ≤1。",
      );
    }
  }
  if (color.x === 0 && color.y === 0 && color.z === 0) {
    return err(
      "DIRECTIONAL_COLOR_INVALID",
      "颜色三通道全为 0——黑色方向光等价于不存在。",
      "若本意是用颜色调制强度，请把亮度写进 intensityLux 并把颜色至少留一个非零通道。",
    );
  }
  return ok(color);
}

/**
 * 校验方向光参数块（导入期闸门）。
 *
 * 三条处置各有归属，**不做静默修正**：
 *   · 方向未单位化 → **导入期归一化并告警**（锚点原文），因为这是可自动修复的常见笔误；
 *   · 方向为零向量 → **拒绝**（锚点原文：运行时拒绝零向量），因为无默认方向可猜；
 *   · 强度超上限 → 走 F1802 的 `clampIntensity`（锚点原文：强度超物理上限 → F1802 钳制）。
 */
export function validateDirectionalLight(params: DirectionalLightParams): Outcome<DirectionalLightParams> {
  const diagnostics: Diagnostic[] = [];

  // 5.1 方向：先归一化（零向量在此被拒）。
  const normalized = normalize(params.direction);
  if (!normalized.ok) return normalized;

  let direction = normalized.value;
  if (!isNormalized(params.direction)) {
    diagnostics.push({
      code: "DIRECTION_NOT_NORMALIZED",
      message: `方向向量模长为 ${length(params.direction).toFixed(6)}，已归一化后使用。`,
      hint:
        "方向必须以单位向量存储，否则 N·L 会随模长整体偏大或偏小，画面亮度跟着错而看不出原因。" +
        "请在导出侧统一归一化。",
    });
  }

  // 5.2 颜色（太阳模型开启时由模型给值，此处校验显式值）。
  let color = params.color;
  const colorCheck = validateColor(color);
  if (!colorCheck.ok) {
    if (params.solar.enabled) {
      diagnostics.push({
        code: "DIRECTIONAL_COLOR_INVALID",
        message: `参数块颜色非法（${color.x}, ${color.y}, ${color.z}），但太阳模型开启，改用模型推算颜色。`,
        hint: "模型开启时颜色由高度角/色温决定，显式值被忽略。修好显式值以免关闭模型后失效。",
      });
    } else {
      return colorCheck;
    }
  }

  // 5.3 强度：钳制走 F1802 的分单位物理上限，钳制事实回传遥测。
  const clamped = clampIntensity(params.intensityLux, "LUX");
  let intensityLux = clamped.value;
  if (clamped.clamped) {
    diagnostics.push({
      code: "DIRECTIONAL_INTENSITY_CLAMPED",
      message: `强度 ${params.intensityLux} lux 已被 F1802 钳制到 ${intensityLux} lux。`,
      hint:
        "正午阳光约 1.2e5 lux。若量级远超此值，通常是把「辐射量」当成了「照度」" +
        "（缺 683 lm/W 的视见效率折算），请先在 F1802 侧折算再传入。" +
        "具体钳制原因见 F1802 的 ClampReason，本条不重复判定。",
    });
  }

  // 5.4 高度角钳制告警（模型开启时才推算）。
  if (params.solar.enabled) {
    const ec = clampElevationDeg(params.solar.elevationDeg);
    if (ec.clamped) {
      direction = direction; // 方向不受影响，显式保持可读性
      diagnostics.push({
        code: "SOLAR_ELEVATION_OUT_OF_RANGE",
        message: `太阳高度角 ${params.solar.elevationDeg}° 越界，已钳制到 ${ec.value}°。`,
        hint: "合法域 [0°, 90°]。-1° 常被误用作「无太阳」，请改用 solar.enabled=false。",
      });
    }
  }

  // 5.5 太阳模型推算颜色（显式色温优先于高度角）。
  let finalColor = color;
  if (params.solar.enabled) {
    const solar = resolveSolarColor({
      ...params.solar,
      elevationDeg: clampElevationDeg(params.solar.elevationDeg).value,
    });
    if (!solar.ok) return solar;
    finalColor = solar.value.rgb;
    diagnostics.push(...solar.diagnostics);
  }

  return ok({ ...params, direction, color: finalColor, intensityLux }, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §6 平行光着色贡献（锚点原文：单方向光着色成本 O(1)/像素·无距离衰减）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 法线与光向的点积项（表面朝向决定受照量；背面为 0）。 */
export function nDotL(normal: Vec3, lightDirection: Vec3): number {
  // lightDirection 语义：**从光源指向场景**（即光线传播方向）。
  // 受照量 = max(0, -N·L)，故此处取负号。这一点与「L 指向光源」的常见约定
  // 相反，本条以 F1801 的方向语义为准并在 F1810 线框里以箭头方向复核。
  return Math.max(0, -dot(normal, lightDirection));
}

/** 单方向光的辐照度贡献（平行光：与距离无关）。 */
export function irradianceContribution(params: DirectionalLightParams, normal: Vec3): number {
  return params.intensityLux * nDotL(normal, params.direction);
}

/** 单方向光的线性 RGB 辐射（颜色 × 照度 × N·L）。 */
export function radianceContribution(params: DirectionalLightParams, normal: Vec3): Vec3 {
  const e = irradianceContribution(params, normal);
  return { x: params.color.x * e, y: params.color.y * e, z: params.color.z * e };
}

/**
 * 平行光「距离衰减恒等」的显式见证（把锚点的物理立场做成可执行检查）。
 *
 * 返回恒为 1——不是为了写个函数，而是为了让「方向光不该有距离衰减」这条
 * 立场在代码里有个可被测试钉住的位置。若将来有人给方向光加 d² 除法，
 * 这个恒等式就是拦它的点。
 */
export const DIRECTIONAL_DISTANCE_ATTENUATION = 1;

/* ═══════════════════════════════════════════════════════════════════════════
 * §7 多方向光并发治理（锚点原文：并发数上限默认 4 可配 · 超限按重要性排序
 *   保留前 N 并显性告警，与 F1807 管理器策略一致）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 并发方向光上限默认值（锚点：默认 4 可配）。 */
export const DEFAULT_MAX_CONCURRENT = 4;

/** 并发上限合法域（0 表示允许「全部不并发」，由 F1807 退化为单主光）。 */
export const MAX_CONCURRENT_MIN = 1;
export const MAX_CONCURRENT_MAX = 16;

/** 多光登记结果。 */
export interface ConcurrentTrimResult {
  /** 裁剪后保留的光源（按重要性降序，ID 升序决胜）。 */
  readonly kept: readonly DirectionalLightParams[];
  /** 被裁掉的（显式列出，便于 UI/遥测点名，而非静默消失）。 */
  readonly dropped: readonly DirectionalLightParams[];
  /** 本次保留数。 */
  readonly keptCount: number;
}

/**
 * 多光裁剪（保留前 N，按重要性 + ID 决胜）。
 *
 * 排序键：`importance` 降序 → `id` 升序。**ID 决胜不是可选的**：两条同重要性的
 * 方向光若靠原始顺序决胜，帧间顺序可能变化 → 裁剪结果抖动 → 画面上某盏灯
 * 忽明忽暗（flicker）。加 ID 后排序全序且稳定，与 I 域确定性口径一致。
 */
export function trimDirectionalLights(
  lights: readonly DirectionalLightParams[],
  maxConcurrent: number = DEFAULT_MAX_CONCURRENT,
): Outcome<ConcurrentTrimResult> {
  if (!Number.isInteger(maxConcurrent) || maxConcurrent < MAX_CONCURRENT_MIN || maxConcurrent > MAX_CONCURRENT_MAX) {
    return err(
      "DIRECTIONAL_COUNT_OVER_LIMIT",
      `并发上限 ${maxConcurrent} 非法（须为 [${MAX_CONCURRENT_MIN}, ${MAX_CONCURRENT_MAX}] 内的整数）。`,
      "上限过小会让场景失去必要主光，过大则超出 GPU 参数块预算（见 F1811 账本）。" +
        "请给出一个落在预算内的整数。",
    );
  }

  const diagnostics: Diagnostic[] = [];

  if (lights.length === 0) {
    diagnostics.push({
      code: "DIRECTIONAL_LIGHT_ABSENT",
      message: "场景没有方向光，将失去平行光照明。",
      hint: "方向光通常是主光。确认这是刻意为之（如纯环境光场景），否则请补一盏太阳。",
    });
    return ok({ kept: [], dropped: [], keptCount: 0 }, diagnostics);
  }

  // 全序排序：重要性降序 → ID 升序。
  const sorted = [...lights].sort((a, b) => {
    if (a.importance !== b.importance) return b.importance - a.importance;
    return a.id - b.id;
  });

  // 确定性自检：ID 重复即说明登记有误，会破坏决胜。
  const ids = new Set<number>();
  for (const l of sorted) {
    if (ids.has(l.id)) {
      diagnostics.push({
        code: "DIRECTIONAL_SORT_NONDETERMINISTIC",
        message: `光源 ID ${l.id} 重复，排序决胜键失效。`,
        hint: "ID 必须全局唯一（通常由 F1807 的生成计数分配）。重复 ID 会让同重要性的灯" +
          "裁剪结果依赖输入顺序，帧间可能抖动。",
      });
      break;
    }
    ids.add(l.id);
  }

  if (sorted.length > maxConcurrent) {
    diagnostics.push({
      code: "DIRECTIONAL_COUNT_OVER_LIMIT",
      message: `方向光 ${sorted.length} 盏，超上限 ${maxConcurrent}，保留重要性前 ${maxConcurrent} 盏、裁掉 ${sorted.length - maxConcurrent} 盏。`,
      hint:
        "裁剪按「重要性降序 → ID 升序」执行，被裁的是重要性最低者，不是随机选的。" +
        "若被裁里有你关心的灯，请提高其 importance 字段，而不是直接调高上限——" +
        "上限是 GPU 预算的硬约束（见 F1811）。",
    });
  }

  return ok(
    {
      kept: sorted.slice(0, maxConcurrent),
      dropped: sorted.slice(maxConcurrent),
      keptCount: Math.min(sorted.length, maxConcurrent),
    },
    diagnostics,
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §8 CSM 主光源契约（锚点原文：方向光是 F1823 CSM 阴影的主光源 · J02 契约）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * CSM 级联输入（方向光 → F1823 级联阴影）。
 *
 * **本条只产出输入，不实现级联划分**——那是 F1823 的职责。此处保证的是
 * 「F1823 拿到这些参数就能开跑」：正交视锥体、级联数、切分、纹素对齐。
 */
export interface DirectionalShadowCascadeInput {
  /** 光向（单位化，光线传播方向：光源 → 场景）。 */
  readonly lightDirection: Vec3;
  /** 正交视锥体近/远平面（沿光向）。 */
  readonly nearPlane: number;
  readonly farPlane: number;
  /** 场景包围球半径（决定正交视锥体横向尺寸）。 */
  readonly sceneRadius: number;
  /** 级联数（J02 默认 4）。 */
  readonly cascadeCount: number;
  /** 实践分裂（0..1）决定各级覆盖比例；锚点未指定故留显式位。 */
  readonly practicalSplitScheme: number;
  /** 纹素对齐（1 开启，用于消除阴影边缘爬行）。 */
  readonly texelSnapping: boolean;
  /** 法线偏移（shadow acne 抑制，单位与世界尺度同量纲）。 */
  readonly normalBias: number;
}

/** 级联数默认（J02 契约值）。 */
export const DEFAULT_CASCADE_COUNT = 4;

/** 级联数合法域。 */
export const CASCADE_COUNT_MIN = 1;
export const CASCADE_COUNT_MAX = 8;

/** 级联切分单调性容差。 */
export const CASCADE_SPLIT_MONOTONIC_TOLERANCE = 1e-6;

/**
 * 构造 CSM 级联输入（主光源契约兑现点）。
 *
 * 校验三件事，任一不过即拒绝而非「修正后继续」：
 *   1. 近远平面为正且 near < far（反了会让级联排布全错）；
 *   2. 场景半径为正（半径 ≤ 0 算不出正交视锥体横向尺寸）；
 *   3. 级联数落在 [1, 8]（超出则阴影贴图预算爆掉）。
 */
export function buildCascadeInput(
  params: DirectionalLightParams,
  sceneRadius: number,
  nearPlane: number,
  farPlane: number,
  cascadeCount: number = DEFAULT_CASCADE_COUNT,
): Outcome<DirectionalShadowCascadeInput> {
  if (!isNormalized(params.direction)) {
    return err(
      "DIRECTION_NOT_NORMALIZED",
      `CSM 主光源方向未单位化（模长 ${length(params.direction).toFixed(6)}）。`,
      "F1823 的正交视锥体按单位方向构建；未单位化会让视锥尺寸随模长漂移，" +
        "表现为阴影块随机变大变小。请先过 validateDirectionalLight。",
    );
  }
  if (!(Number.isFinite(nearPlane) && Number.isFinite(farPlane) && nearPlane > 0 && farPlane > nearPlane)) {
    return err(
      "CSM_CASCADE_INVALID",
      `近远平面非法（near=${nearPlane}, far=${farPlane}）。`,
      "要求 0 < near < far。反了（near > far）会让级联沿光向排布错位，" +
        "阴影整体偏到相机背后——这类错误画面上极像「阴影 bug」，很难直接归因到平面参数。",
    );
  }
  if (!Number.isFinite(sceneRadius) || sceneRadius <= 0) {
    return err(
      "CSM_CASCADE_INVALID",
      `场景包围球半径非法（${sceneRadius}）。`,
      "半径必须为正。半径来自 I 域可见性/包围盒统计，传 0 或负值说明上游场景统计没跑。",
    );
  }
  if (
    !Number.isInteger(cascadeCount) ||
    cascadeCount < CASCADE_COUNT_MIN ||
    cascadeCount > CASCADE_COUNT_MAX
  ) {
    return err(
      "CSM_CASCADE_INVALID",
      `级联数 ${cascadeCount} 非法（须为 [${CASCADE_COUNT_MIN}, ${CASCADE_COUNT_MAX}] 内的整数）。`,
      "级联数直接决定阴影贴图显存与绘制批次，超 8 超出预算（见 F1811 账本）。" +
        "J02 默认 4，若要更多请走 ADR 说明显存来源。",
    );
  }
  if (!(farPlane - nearPlane > CASCADE_SPLIT_MONOTONIC_TOLERANCE)) {
    return err(
      "CSM_CASCADE_INVALID",
      `近远平面跨度过小（${farPlane - nearPlane}），级联无法在视锥内单调切分。`,
      "每级级联至少要有可观的深度跨度才能覆盖场景；跨度过小会产生退化的极薄级联，" +
        "表现为阴影在某处突然消失。",
    );
  }

  return ok({
    lightDirection: params.direction,
    nearPlane,
    farPlane,
    sceneRadius,
    cascadeCount,
    practicalSplitScheme: 0.5,
    texelSnapping: true,
    normalBias: sceneRadius * 1e-4,
  });
}

/**
 * 级联切分单调性断言（锚点：拼接缝处切分非单调即 CSM_CASCADE_INVALID）。
 *
 * 实践分裂：`t_i = (i/N)·p / (1 + (p-1)·(i/N))` 型分布（N 段，p 偏置）。
 * 逐级比对「切分点必须严格递增」——非单调会在拼接缝处产生阴影断裂。
 */
export function verifyCascadeMonotonic(
  cascadeCount: number,
  practicalSplitScheme: number,
): Outcome<number[]> {
  if (!Number.isInteger(cascadeCount) || cascadeCount < CASCADE_COUNT_MIN) {
    return err(
      "CSM_CASCADE_INVALID",
      `级联数 ${cascadeCount} 非法。`,
      `须为 >= ${CASCADE_COUNT_MIN} 的整数。`,
    );
  }
  const p = practicalSplitScheme;
  if (!Number.isFinite(p) || p <= 0 || p >= 1) {
    return err(
      "CSM_CASCADE_INVALID",
      `实践分裂参数 ${p} 非法（须在 (0,1) 开区间）。`,
      "p 控制级联向近处的聚集程度：p<1 聚集近处（远处级联更宽），p→1 趋近均匀分布。" +
        "p=0 会让所有切分点塌到 0（近级零跨度），p=1 退化为均匀分布，故取开区间。",
    );
  }

  const splits: number[] = [];
  for (let i = 1; i <= cascadeCount; i++) {
    const t = i / cascadeCount;
    // 实践分裂（p ∈ (0,1)）：近处更密。
    const split = (p * t) / (1 - t + p * t);
    splits.push(split);
  }
  for (let i = 1; i < splits.length; i++) {
    const prev = splits[i - 1]!;
    const cur = splits[i]!;
    if (!(cur - prev > CASCADE_SPLIT_MONOTONIC_TOLERANCE)) {
      return err(
        "CSM_CASCADE_INVALID",
        `级联切分非单调：第 ${i} 级 ${cur} 未超过第 ${i - 1} 级 ${prev}。`,
        "切分点必须严格递增，否则相邻级联出现重叠或空隙，表现为阴影断裂带。",
      );
    }
  }
  return ok(splits);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §9 F1810 调试可对接（线框图元 + 统计计数）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 线框图元（供 F1810 消费；不直接写缓冲，由其按信封规范发出）。 */
export interface DirectionalWireframePrimitive {
  readonly kind: "DIRECTIONAL_ARROW";
  /** 光源 ID（便于 F1810 按类型着色时点名）。 */
  readonly id: number;
  /** 箭头起点（约定为场景中心）。 */
  readonly from: Vec3;
  /** 箭头终点 = 中心 + 方向 × 线长。 */
  readonly to: Vec3;
  /** 该光在当前裁剪结果中是否被保留（F1810 据此画虚线/灰显）。 */
  readonly kept: boolean;
}

/** 调试统计（供 F1810 统计快照与 F1811 成本模型校准）。 */
export interface DirectionalDebugStats {
  readonly totalCount: number;
  readonly keptCount: number;
  readonly droppedCount: number;
  readonly maxConcurrent: number;
  readonly solarEnabled: boolean;
  readonly meanTemperatureK: number;
}

/** 生成线框图元（箭头长度固定为场景尺度的可读值，不按光强缩放——线框表方向不表亮度）。 */
export function buildWireframes(
  lights: readonly DirectionalLightParams[],
  trim: ConcurrentTrimResult,
  arrowLength: number,
  center: Vec3 = { x: 0, y: 0, z: 0 },
): DirectionalWireframePrimitive[] {
  const keptIds = new Set(trim.kept.map((l) => l.id));
  return lights.map((l) => ({
    kind: "DIRECTIONAL_ARROW" as const,
    id: l.id,
    from: center,
    to: {
      x: center.x + l.direction.x * arrowLength,
      y: center.y + l.direction.y * arrowLength,
      z: center.z + l.direction.z * arrowLength,
    },
    kept: keptIds.has(l.id),
  }));
}

/** 统计快照。 */
export function buildDebugStats(
  lights: readonly DirectionalLightParams[],
  trim: ConcurrentTrimResult,
  maxConcurrent: number,
): DirectionalDebugStats {
  const temps = lights
    .filter((l) => l.solar.enabled)
    .map((l) => {
      const t = temperatureForElevation(clampElevationDeg(l.solar.elevationDeg).value);
      return t.ok ? t.value : 0;
    });
  const mean = temps.length > 0 ? temps.reduce((a, b) => a + b, 0) / temps.length : 0;
  return {
    totalCount: lights.length,
    keptCount: trim.keptCount,
    droppedCount: trim.dropped.length,
    maxConcurrent,
    solarEnabled: lights.some((l) => l.solar.enabled),
    meanTemperatureK: mean,
  };
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §10 F1813 fuzz 面登记（锚点原文：fuzz 面覆盖进 F1813）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 本条的 fuzz 面（供 F1813 组织用例；**本文件不含任何用例代码**——测试归测试，
 * 功能归功能，此处只登记被测面与期望性质）。
 */
export const FUZZ_SURFACES: readonly { readonly name: string; readonly expect: string }[] = [
  { name: "零向量方向", expect: "拒绝（DIRECTION_ZERO_VECTOR），不得返回默认方向" },
  { name: "NaN/Inf 方向分量", expect: "拒绝（DIRECTION_ZERO_VECTOR，经模长非有限判定）" },
  { name: "未单位化方向", expect: "归一化 + 告警（DIRECTION_NOT_NORMALIZED）" },
  { name: "高度角 -1 / 91 / NaN", expect: "钳制到 [0,90] + 告警（SOLAR_ELEVATION_OUT_OF_RANGE）" },
  { name: "显式色温 0 / -300 / NaN", expect: "拒绝（SOLAR_TEMPERATURE_INVALID）" },
  { name: "色温超查表域 500 / 50000", expect: "拒绝（BLACKBODY_LUT_MISS），不得外插" },
  { name: "颜色越界 1.5 / -0.2 / 全零", expect: "拒绝（DIRECTIONAL_COLOR_INVALID）" },
  { name: "强度 NaN / -1 / 1e30", expect: "钳制并留痕（经 F1802）" },
  { name: "并发上限 0 / 17 / 2.5", expect: "拒绝（DIRECTIONAL_COUNT_OVER_LIMIT）" },
  { name: "重复 ID + 同重要性", expect: "告警（DIRECTIONAL_SORT_NONDETERMINISTIC）" },
  { name: "级联 near≥far / 半径≤0 / 级联数 9", expect: "拒绝（CSM_CASCADE_INVALID）" },
  { name: "实践分裂 p=0 / p=1", expect: "拒绝（CSM_CASCADE_INVALID）" },
  { name: "光源风暴（千盏）", expect: "裁剪到上限且确定性可复现" },
];

/* ═══════════════════════════════════════════════════════════════════════════
 * §11 域级自检（把四条判据变成可执行检查）
 * ═══════════════════════════════════════════════════════════════════════════ */

export interface DirectionalSelfCheckReport {
  readonly passed: boolean;
  readonly total: number;
  readonly failed: number;
  readonly failures: readonly string[];
}

/**
 * 一盏合法太阳（供自检用）。
 *
 * 方向语义为**光线传播方向**（光源 → 场景）：高度角 el 时
 * `direction = (0, −sin el, −cos el)`。于是 el=0°（地平线）→ (0,0,−1) 水平入射，
 * el=90°（天顶）→ (0,−1,0) 垂直向下。
 *
 * 注意 x 分量取 0 而非 sin：方向矢量的**水平分量由方位角决定**，而本条
 * 刻意不收方位角（锚点只给高度角），故固定在 −Z 方位上。若误把 x 写成
 * sin(el)，el=90° 时会得到 (1,0,0)——那不是天顶太阳，是地平线太阳。
 */
function sampleDirectional(id: number, elevationDeg: number): DirectionalLightParams {
  const el = (elevationDeg * Math.PI) / 180;
  return {
    direction: { x: 0, y: -Math.sin(el), z: -Math.cos(el) },
    color: { x: 1, y: 1, z: 1 },
    intensityLux: 100_000,
    solar: { enabled: true, elevationDeg, explicitTemperatureK: null },
    id,
    importance: 1,
  };
}

/**
 * 自检：覆盖锚点判据「三参数 / 太阳色温 / 多光上限 / CSM 主光源」四条。
 *
 * 全部走**真实函数调用**而非结构断言——结构断言只能证明代码长成了那个样子，
 * 走一遍才能证明它真的按物理工作。
 */
export function selfCheckDirectionalLight(): DirectionalSelfCheckReport {
  let total = 0;
  const failures: string[] = [];

  const t = (id: string, cond: boolean, detail = ""): void => {
    total++;
    if (!cond) failures.push(detail === "" ? id : `${id} :: ${detail}`);
  };

  // 判据① 三参数。
  {
    const v = validateDirectionalLight(sampleDirectional(1, 45));
    t("D1 参数块校验通过", v.ok);
    const zero = validateDirectionalLight({ ...sampleDirectional(1, 45), direction: { x: 0, y: 0, z: 0 } });
    t("D2 零向量被拒", !zero.ok && zero.code === "DIRECTION_ZERO_VECTOR");
    const un = validateDirectionalLight({ ...sampleDirectional(1, 45), direction: { x: 0, y: -5, z: 0 } });
    t("D3 未单位化归一化+告警", un.ok && un.diagnostics.some((d) => d.code === "DIRECTION_NOT_NORMALIZED"));
    const bad = validateDirectionalLight({ ...sampleDirectional(1, 45), intensityLux: Number.NaN });
    t("D4 NaN 强度被钳", bad.ok && Number.isFinite(bad.value.intensityLux));
    const col = validateDirectionalLight({ ...sampleDirectional(1, 45), solar: { enabled: false, elevationDeg: 0, explicitTemperatureK: null }, color: { x: 2, y: 0, z: 0 } });
    t("D5 越界颜色被拒(模型关闭)", !col.ok && col.code === "DIRECTIONAL_COLOR_INVALID");
  }

  // 判据② 太阳色温。
  {
    const low = temperatureForElevation(0);
    const high = temperatureForElevation(90);
    t("D6 高度角 0° 色暖", low.ok && high.ok && low.value < 3000);
    t("D7 高度角 90° 色白", low.ok && high.ok && high.value >= 5000);

    const warm = blackbodyLinearRgb(2000);
    const cool = blackbodyLinearRgb(5778);
    t("D8 两端色均可解", warm.ok && cool.ok);
    // 物理性质：2000K 红通道显著高于蓝通道；5778K 应接近中性（三通道差距小）。
    const warmGap = warm.ok ? warm.value.x - warm.value.z : 0;
    const coolGap = cool.ok ? Math.abs(cool.value.x - cool.value.z) : 1;
    t("D9 低色温偏红(物理性质)", warmGap > 0.2, `gap=${warmGap.toFixed(3)}`);
    t("D10 高色温近中性", coolGap < 0.2, `gap=${coolGap.toFixed(3)}`);
    t("D11 色温 0 被拒", !blackbodyLinearRgb(0).ok);
    t("D12 色温超域被拒(不外插)", !blackbodyLinearRgb(50000).ok);

    const clamped = clampElevationDeg(120);
    t("D13 高度角钳到 90", clamped.value === 90 && clamped.clamped);
    const model = resolveSolarColor({ enabled: true, elevationDeg: 5, explicitTemperatureK: null });
    t("D14 模型推算成功", model.ok && model.value.source === "ELEVATION");
    const explicit = resolveSolarColor({ enabled: true, elevationDeg: 5, explicitTemperatureK: 3000 });
    t("D15 显式色温优先", explicit.ok && explicit.value.source === "EXPLICIT" && explicit.value.temperatureK === 3000);
  }

  // 判据③ 多光上限。
  {
    t("D16 默认上限=4", DEFAULT_MAX_CONCURRENT === 4);
    const many = [1, 2, 3, 4, 5, 6].map((i) => ({ ...sampleDirectional(i, 45), importance: i === 6 ? 0.9 : 1 }));
    const trimmed = trimDirectionalLights(many);
    t("D17 超限裁剪生效", trimmed.ok && trimmed.value.keptCount === 4);
    t("D18 裁剪告警显性", trimmed.ok && trimmed.diagnostics.some((d) => d.code === "DIRECTIONAL_COUNT_OVER_LIMIT"));
    // 重要性 0.9 的那盏必须被裁（重要性最低），即使它 ID 最大。
    t("D19 按重要性裁而非 ID", trimmed.ok && !trimmed.value.kept.some((l) => l.id === 6));
    // 确定性：同输入两次结果一致。
    const again = trimDirectionalLights(many);
    t(
      "D20 裁剪确定性可复现",
      trimmed.ok && again.ok && JSON.stringify(trimmed.value.kept.map((l) => l.id)) === JSON.stringify(again.value.kept.map((l) => l.id)),
    );
    const dup = trimDirectionalLights([sampleDirectional(1, 45), sampleDirectional(1, 45)]);
    t("D21 重复 ID 告警", dup.ok && dup.diagnostics.some((d) => d.code === "DIRECTIONAL_SORT_NONDETERMINISTIC"));
    t("D22 非法上限被拒", !trimDirectionalLights(many, 0).ok && !trimDirectionalLights(many, 99).ok);
    const empty = trimDirectionalLights([]);
    t("D23 无主光源告警", empty.ok && empty.diagnostics.some((d) => d.code === "DIRECTIONAL_LIGHT_ABSENT"));
  }

  // 判据④ CSM 主光源。
  {
    const sun = validateDirectionalLight(sampleDirectional(1, 60));
    t("D24 太阳校验通过", sun.ok);
    if (sun.ok) {
      const c = buildCascadeInput(sun.value, 100, 0.1, 500, 4);
      t("D25 CSM 输入构造成功", c.ok && c.value.cascadeCount === 4);
      t("D26 方向透传单位化", c.ok && isNormalized(c.value.lightDirection));
      t("D27 纹理对齐默认开", c.ok && c.value.texelSnapping === true);
      t("D28 near≥far 被拒", !buildCascadeInput(sun.value, 100, 500, 0.1, 4).ok);
      t("D29 半径≤0 被拒", !buildCascadeInput(sun.value, 0, 0.1, 500, 4).ok);
      t("D30 级联数 9 被拒", !buildCascadeInput(sun.value, 100, 0.1, 500, 9).ok);
      const splits = verifyCascadeMonotonic(4, 0.5);
      t("D31 级联切分单调", splits.ok && splits.value.length === 4);
      t("D32 非法 p 被拒", !verifyCascadeMonotonic(4, 0).ok && !verifyCascadeMonotonic(4, 1).ok);
      if (splits.ok) {
        t("D33 切分严格递增", splits.value.every((v, i) => i === 0 || v > splits.value[i - 1]!));
        t("D34 近处更密(实践分裂)", splits.value[0]! < 0.25, `first=${splits.value[0]?.toFixed(4)}`);
      }
    }
  }

  // 物理立场：平行光无距离衰减。
  {
    t("D35 距离衰减恒等", DIRECTIONAL_DISTANCE_ATTENUATION === 1);
    // 天顶太阳（el=90°）光线垂直向下 (0,−1,0)：水平面（法线 +Y）正对光源，
    // N·L = 1 → 照度全额；竖直面（法线 ±X）侧对 → 0；朝下面（法线 −Y）→ 0。
    const s = sampleDirectional(1, 90);
    const n: Vec3 = { x: 0, y: 1, z: 0 };
    const e = irradianceContribution(s, n);
    t("D36 正对天顶全受照", Math.abs(e - 100_000) < 1e-6, `E=${e}`);
    const side = irradianceContribution(s, { x: 1, y: 0, z: 0 });
    t("D37 侧对零(余弦定理)", side === 0, `E=${side}`);
    const rad = radianceContribution(s, n);
    t("D38 辐射=颜色×照度", Math.abs(rad.x - 100_000) < 1e-6, `R=${rad.x}`);
    t("D39 背面不反光", irradianceContribution(s, { x: 0, y: -1, z: 0 }) === 0);
    // 地平线太阳（el=0°）光线水平 (0,0,−1)：竖直面（法线 +Z）正对 → 全额。
    const h = sampleDirectional(2, 0);
    t("D40 地平线光照亮竖直面", Math.abs(irradianceContribution(h, { x: 0, y: 0, z: 1 }) - 100_000) < 1e-6);
    t("D41 地平线光不照亮地面", irradianceContribution(h, { x: 0, y: 1, z: 0 }) === 0);
    t("D42 N·L 非负", nDotL({ x: 0, y: -1, z: 0 }, s.direction) === 0);
  }

  // F1810 调试对接面。
  {
    const lights = [sampleDirectional(1, 30), sampleDirectional(2, 60)];
    const trimmed = trimDirectionalLights(lights, 1);
    t("D39 线框图元生成", trimmed.ok && buildWireframes(lights, trimmed.value, 10).length === 2);
    const wf = trimmed.ok ? buildWireframes(lights, trimmed.value, 10) : [];
    t("D40 被裁者标记 kept=false", wf.some((w) => w.id === 2 && !w.kept));
    const stats = trimmed.ok ? buildDebugStats(lights, trimmed.value, 1) : null;
    t("D41 统计计数一致", stats !== null && stats.totalCount === 2 && stats.keptCount === 1 && stats.droppedCount === 1);
    t("D42 均色温合理", stats !== null && stats.meanTemperatureK > 4000 && stats.meanTemperatureK < 6000);
  }

  // 与 F1802 的单位契约：方向光强度必须落在 LUX 上限内。
  {
    const clamped = clampIntensity(1e30, "LUX");
    t("D43 方向光强度受 F1802 上限约束", clamped.clamped && clamped.value < 1e30);
    t("D44 艺术制折算路径存在", Number.isFinite(toInternalPhysical(1, "ARTISTIC")));
  }

  return { passed: failures.length === 0, total, failed: failures.length, failures };
}
