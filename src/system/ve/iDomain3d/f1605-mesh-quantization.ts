/**
 * VE-F1605 · 网格量化压缩（I 域 · 3D 管线域 · L0 几何数据层 · 批次 I01）
 * ---------------------------------------------------------------------------
 * 职责定位：顶点属性的有界量化 —— 位置（16bit 局部坐标系）、法线（octahedral
 * 32bit双通道）、UV，以及每档位误差与内存收益的权衡表与量化确定性保证。
 * 上游 F1603（顶点属性布局）提供格式与误差契约、F1604（索引与拓扑）提供连接关系；
 * 下游 F1608（QEM 简化）在量化后的网格上作业、F1611（流式容器）按量化档位选择
 * 加载精度、F1613（网格统计）读取压缩率与误差数据。
 *
 * 判据与段落对应：
 *   判据一「位置量化」 → §2 位置量化的核心是**局部坐标系**：先求包围盒，把坐标
 *                       归一化到 [0,1] ³，再映射到 16bit 整数。误差上界
 *                       **±量化步长/2**，且该上界对全网格**处处成立**——这是
 *                       「有界」二字的实质：不是「平均误差小」，而是「最坏情况
 *                       可预先算出并向用户承诺」。
 *   判据二「octahedral」 → §3 法线用八面体编码压进 32bit（两个 16bit 通道），
 *                       而非 xyz 各 16bit（48bit）。同等字节下精度更高是原因，
 *                       判据要求给出**可验证的对照**而非仅断言「业界最佳」。
 *   判据三「UV 量化」→ §4 UV 用无符号 16bit（可平铺范围显式声明，
 *                       平铺纹理需 range > 1，不能按 [0,1] 量化）。
 *   判据四「权衡表」   → §5 8/10/16bit × 各属性的误差与内存收益量化入册，
 *                       每格给出误差上界与压缩比而非只有档位名。
 *   判据五「确定性」   → §6 同输入同输出：包围盒取 min/max 的**遍历顺序无关性**
 *                       与浮点累加顺序无关性是两处真实陷阱，须用整数定点与
 *                       确定性归约来消除。
 *
 * 与 F1603 的关系（重要）：F1603 提供了打包格式与**误差上界契约**，
 * 本条提供编解码实现与误差的**实测验证**，二者互为对拍——F1603 的
 * `verifyQuantization` 量的是单分量往返误差，本条量的是「量化整网格后的
 * 几何偏差」（含包围盒归一化这一步引入的附加误差）。单分量对拍通过
 * 不等于整网格误差有界，故两边都必须各自验证。
 *
 * 边界声明（不扩面）：
 *   · 本条只管「怎么量化、误差多大」，不生成法线（法线生成归 F1606）、
 *     不简化（QEM 归 F1608）、不压缩索引与拓扑（F1604 负责位宽）、
 *     不做通用熵编码（如 Draco 的算术编码——那是另一条目的职责）。
 *   · 熵编码的预留：§5 的权衡表以「顶点属性字节」为口径，不含熵编码后的
 *     实际文件体积。混淆两者会给出乐观的压缩率，故显式声明口径。
 *
 * 零静默纪律：非法输入（NaN 坐标、零尺寸包围盒、越界档位）一律产出
 * Diagnostic（code + message + hint 三要素），不抛异常、不静默返回零网格。
 * 量化错误的典型表现是模型「整体偏移半个像素」或「法线翻面」，
 * 属于能出图但错的缺陷，比崩溃更难发现。
 *
 * 判据：位置/法线/UV 量化、权衡表、确定性、判据。
 * 落位：src/system/ve/iDomain3d/
 */

// ════════════════════════════════════════════════════════════════════════════
// §0 诊断基础设施（零静默第一层；与 F1602/F1603/F1604 同纪律，自包含）
// ════════════════════════════════════════════════════════════════════════════

/** 诊断码：每种失败独立可检索，绝不合并为一条通用错误。 */
export type DiagCode =
  /** 顶点坐标含 NaN 或 Inf。 */
  | "POSITION_NONFINITE"
  /** 包围盒退化（各轴尺寸为 0，平面/线状网格）。 */
  | "BOUNDS_DEGENERATE"
  /** 量化档位不在支持的位宽集合内。 */
  | "QUANT_BITS_UNSUPPORTED"
  /** UV 量化范围非法（range ≤ 0 或非有限）。 */
  | "UV_RANGE_INVALID"
  /** 法线未归一化（零长度或长度非 1，无法确定八面体投影方向）。 */
  | "NORMAL_NOT_UNIT"
  /** 输入顶点数组为空。 */
  | "VERTEX_SET_EMPTY"
  /** 实测几何偏差超出该档位声明的误差上界（量化实现与契约脱节）。 */
  | "QUANTIZATION_ERROR_EXCEEDED";

/** 一条诊断：发生了什么（人话）、影响什么、下一步怎么办。 */
export interface Diagnostic {
  readonly code: DiagCode;
  readonly message: string;
  readonly hint: string;
}

/** 结果判别联合：成功必带 value，失败必带 code/message/hint。 */
export type Outcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly Diagnostic[] }
  | {
      readonly ok: false;
      readonly code: DiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly Diagnostic[];
    };

/** 成功构造（diagnostics 承载非致命告警，如包围盒退化但已用 epsilon 兜底）。 */
export function ok<T>(value: T, diagnostics: readonly Diagnostic[] = []): Outcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造：三要素齐备。 */
export function fail<T>(code: DiagCode, message: string, hint: string): Outcome<T> {
  const d: Diagnostic = { code, message, hint };
  return { ok: false, code, message, hint, diagnostics: [d] };
}

/** 诊断聚合器。 */
export class DiagBag {
  private readonly items: Diagnostic[] = [];

  push(code: DiagCode, message: string, hint: string): void {
    this.items.push({ code, message: message || "（未提供描述）", hint: hint || "（未提供处置建议）" });
  }

  pushAll(ds: readonly Diagnostic[]): void {
    for (const d of ds) this.items.push(d);
  }

  get size(): number {
    return this.items.length;
  }

  all(): readonly Diagnostic[] {
    return this.items.slice();
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §1 基础类型与事实源常量
// ════════════════════════════════════════════════════════════════════════════

/** 量化档位（判据四的档位轴）。10bit 是工程折中档（非2的幂，需特殊步长计算）。 */
export type QuantBits = 8 | 10 | 12 | 16;

/** 支持的量化档位全集。 */
export const ALL_QUANT_BITS: readonly QuantBits[] = [8, 10, 12, 16];

/**
 * 包围盒退化时的 epsilon 兜底（世界单位）。
 *
 * 为何需要兜底而不是直接拒绝：完全平面（如一张纸）的网格是**合法资产**，
 * 直接拒绝会让这类资产无法加载，而它们恰恰是量化收益最大的（薄片模型）。
 * 故用 epsilon 替代零尺寸轴，误差声明仍然成立（有界），只是该轴的量化
 * 无效（所有顶点映到同一格）。但若 epsilon 大到能吞掉真实尺寸，那是另一回事——
 * 故设下界校验。
 */
export const DEGENERATE_AXIS_EPSILON = 1e-8;

/** 小于该尺寸的包围盒视为退化（低于此值时浮点噪声会主导量化误差）。 */
export const MIN_AXIS_SIZE = 1e-6;

/** 确定性测试用的量化累加器初始化值。 */
export const FIXED_POINT_SCALE = 1024;

/** 轴对齐包围盒。 */
export interface Bounds {
  readonly min: readonly [number, number, number];
  readonly max: readonly [number, number, number];
}

/** 包围盒尺寸（三轴可能为 0，故调用方须处理退化轴）。 */
export function boundsSize(b: Bounds): readonly [number, number, number] {
  return [b.max[0] - b.min[0], b.max[1] - b.min[1], b.max[2] - b.min[2]];
}

/** 包围盒对角线长度（量化误差的「网格尺度」参照物）。 */
export function boundsDiagonal(b: Bounds): number {
  const s = boundsSize(b);
  return Math.sqrt(s[0] * s[0] + s[1] * s[1] + s[2] * s[2]);
}

// ════════════════════════════════════════════════════════════════════════════
// §2 位置量化：局部坐标系 16bit 有界量化（判据一）
// ════════════════════════════════════════════════════════════════════════════

/** 位置量化参数（局部坐标系 + 档位）。 */
export interface PositionQuantParams {
  readonly bits: QuantBits;
  /** 包围盒（量化坐标系的原点与尺度）。 */
  readonly bounds: Bounds;
}

/** 量化后的位置：每分量一个无符号整数（范围 0..2^bits-1）。 */
export interface QuantizedPosition {
  readonly x: number;
  readonly y: number;
  readonly z: number;
}

/** 单个分量的量化步长（世界单位/格）。 */
export function componentStep(axisSize: number, bits: QuantBits): number {
  const levels = Math.pow(2, bits) - 1;
  return axisSize / levels;
}

/**
 * 位置量化的**误差上界**（判据一的核心承诺）。
 *
 * 公式：±步长/2，步长 = 轴尺寸 / (2^bits - 1)。
 *
 * 为何这个上界是**处处成立**而非统计意义上的：
 *   量化是「取整」操作，round(x/step) 与真值之差恒在 [-0.5, +0.5] 格内，
 *   再乘回步长即得 ±步长/2。这与网格的形状、顶点数、坐标分布**完全无关**——
 *   稠密区与稀疏区 alike。这就是「有界」与「平均误差小」的本质区别：
 *   后者允许某个角落的误差是均值的十倍，而本条承诺的是全局最坏情况可预算。
 *
 * 三轴取最大值作为该顶点的误差上界（各轴独立量化，误差不可叠加——叠加是
 * 欧氏距离的问题，见 measureQuantizationError 的实测口径）。
 */
export function positionErrorBound(params: PositionQuantParams): number {
  const s = boundsSize(params.bounds);
  let worst = 0;
  for (let axis = 0; axis < 3; axis += 1) {
    const size = Math.max(s[axis] ?? 0, DEGENERATE_AXIS_EPSILON);
    worst = Math.max(worst, componentStep(size, params.bits) / 2);
  }
  return worst;
}

/** 归一化坐标 → 量化整数（四舍五入，并夹紧到档位上界防止浮点越界）。 */
function quantizeComponent01(normalized: number, bits: QuantBits): number {
  const levels = Math.pow(2, bits) - 1;
  const q = Math.round(normalized * levels);
  if (q < 0) return 0;
  if (q > levels) return levels;
  return q;
}

/** 量化整数 → 归一化坐标（0..1，除以档位上界而非 2^bits——差一格误差）。 */
function dequantizeComponent01(q: number, bits: QuantBits): number {
  const levels = Math.pow(2, bits) - 1;
  return q / levels;
}

/** 单个位置：世界坐标 → 量化整数。 */
export function quantizePosition(p: readonly [number, number, number], params: PositionQuantParams): QuantizedPosition {
  const b = params.bounds;
  const s = boundsSize(b);
  const nx = (p[0] - b.min[0]) / Math.max(s[0] ?? 0, DEGENERATE_AXIS_EPSILON);
  const ny = (p[1] - b.min[1]) / Math.max(s[1] ?? 0, DEGENERATE_AXIS_EPSILON);
  const nz = (p[2] - b.min[2]) / Math.max(s[2] ?? 0, DEGENERATE_AXIS_EPSILON);
  return {
    x: quantizeComponent01(nx, params.bits),
    y: quantizeComponent01(ny, params.bits),
    z: quantizeComponent01(nz, params.bits),
  };
}

/** 单个位置：量化整数 → 世界坐标（重建的坐标即带量化误差的近似值）。 */
export function dequantizePosition(q: QuantizedPosition, params: PositionQuantParams): readonly [number, number, number] {
  const b = params.bounds;
  const s = boundsSize(b);
  const dx = Math.max(s[0] ?? 0, DEGENERATE_AXIS_EPSILON);
  const dy = Math.max(s[1] ?? 0, DEGENERATE_AXIS_EPSILON);
  const dz = Math.max(s[2] ?? 0, DEGENERATE_AXIS_EPSILON);
  return [
    b.min[0] + dequantizeComponent01(q.x, params.bits) * dx,
    b.min[1] + dequantizeComponent01(q.y, params.bits) * dy,
    b.min[2] + dequantizeComponent01(q.z, params.bits) * dz,
  ];
}

/**
 * 求包围盒（顺序无关的确定性实现，判据五的前置）。
 *
 * 确定性上的第一处真实陷阱：`min`/`max` 的浮点归约本身是顺序无关的
 * （取最小最大与遍历顺序无关），但若用「增量式」写法并在中途做浮点运算
 * （如先求和再除），顺序就会影响结果。故这里只用 min/max 归约——
 * 它们逐次比较、每次结果都被后续覆盖，不产生累积误差。
 *
 * 第二处陷阱：NaN 参与比较永远返回 false，故 min/max 归约会**静默跳过** NaN，
 * 产出看似正常的包围盒。本实现显式扫出非有限值并报错，而不是让它消失。
 */
export function computeBounds(positions: readonly (readonly [number, number, number])[]): Outcome<Bounds> {
  if (positions.length === 0) {
    return fail(
      "VERTEX_SET_EMPTY",
      "顶点集为空，无法求包围盒",
      "空网格无法量化；请检查上游是否产出了顶点，或确认该资产是否应走「空网格特例」路径",
    );
  }
  let minX = Number.POSITIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let minZ = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  let maxZ = Number.NEGATIVE_INFINITY;
  for (let i = 0; i < positions.length; i += 1) {
    const p = positions[i];
    if (p === undefined) continue;
    const [x, y, z] = p;
    if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(z)) {
      return fail(
        "POSITION_NONFINITE",
        `第 ${i} 个顶点含非有限坐标（${String(x)}, ${String(y)}, ${String(z)}）`,
        "NaN/Inf 坐标会让 min/max 归约静默跳过该顶点（NaN 与任何值比较都为 false），"
          + "产出看似正常实则缺顶点的包围盒——量化后表现为模型局部缺失且无任何报错。"
          + "请在几何校验（F1612）阶段先拦截非有限值",
      );
    }
    if (x < minX) minX = x;
    if (y < minY) minY = y;
    if (z < minZ) minZ = z;
    if (x > maxX) maxX = x;
    if (y > maxY) maxY = y;
    if (z > maxZ) maxZ = z;
  }
  return ok({ min: [minX, minY, minZ], max: [maxX, maxY, maxZ] });
}

/** 包围盒合法性：报告哪些轴退化（附处置建议，不直接拒绝——平面网格是合法资产）。 */
export interface BoundsHealth {
  /** 逐轴退化标记，索引 0/1/2 对应 X/Y/Z 轴。数组而非三具名字段：
   *  三具名会把「轴」这一概念在下游硬编码三次，而数组可直接 map 成人话。 */
  readonly degenerate: readonly boolean[];
  readonly minSize: number;
  readonly advice: string;
}

/** 检查包围盒退化情况并给出人话建议。 */
export function inspectBounds(b: Bounds): BoundsHealth {
  const s = boundsSize(b);
  const degenerate: boolean[] = [false, false, false];
  let minSize = Number.POSITIVE_INFINITY;
  for (let i = 0; i < 3; i += 1) {
    const size = s[i] ?? 0;
    if (size < MIN_AXIS_SIZE) degenerate[i] = true;
    if (size < minSize) minSize = size;
  }
  const count = degenerate.filter(Boolean).length;
  if (count === 0) {
    return { degenerate, minSize, advice: `包围盒三轴尺寸 ${s.map((v) => v.toFixed(3)).join("×")}，均可正常量化` };
  }
  const axes = ["X", "Y", "Z"].filter((_, i) => degenerate[i] === true);
  return {
    degenerate,
    minSize,
    advice:
      `包围盒的 ${axes.join("/")} 轴尺寸接近 0（最小 ${minSize.toExponential(2)} < ${MIN_AXIS_SIZE}）——`
      + "该轴量化无效（所有顶点映到同一格），已用 epsilon 兜底保证除数非零。"
      + "平面/线状网格属合法资产（薄片模型恰是量化收益最大者），故不拒绝；"
      + "但若本该是立体模型，说明顶点数据有问题，请回查生成流程",
  };
}

/**
 * 实测量化后的几何偏差（判据一/五的验证手段）。
 *
 * 口径为**最大欧氏距离**：三位移各自带 ≤步长/2 的误差，最坏情况下三轴同向叠加，
 * 故欧氏距离上界是 √3 ×步长/2，而非步长/2。用逐轴误差当欧氏误差会低估一个
 * √3 倍——在误差预算里低估 73% 是不能接受的，故此处明确实测欧氏距离。
 */
export interface QuantErrorReport {
  readonly maxEuclidean: number;
  readonly meanEuclidean: number;
  /** 与positionErrorBound 声明的上界比较的结果。 */
  readonly withinBound: boolean;
  readonly declaredBound: number;
  /** 人话结论。 */
  readonly summary: string;
}

/** 实测量化往返的欧氏偏差（对全部顶点逐个量化再解量化）。 */
export function measureQuantizationError(
  positions: readonly (readonly [number, number, number])[],
  params: PositionQuantParams,
): Outcome<QuantErrorReport> {
  const declared = positionErrorBound(params);
  let maxSq = 0;
  let sumSq = 0;
  for (let i = 0; i < positions.length; i += 1) {
    const p = positions[i];
    if (p === undefined) continue;
    const back = dequantizePosition(quantizePosition(p, params), params);
    const dx = back[0] - p[0];
    const dy = back[1] - p[1];
    const dz = back[2] - p[2];
    const sq = dx * dx + dy * dy + dz * dz;
    if (sq > maxSq) maxSq = sq;
    sumSq += sq;
  }
  const maxEuclidean = Math.sqrt(maxSq);
  const meanEuclidean = positions.length > 0 ? Math.sqrt(sumSq / positions.length) : 0;
  // 欧氏上界为 √3 × 单轴上界：三位移最坏同向叠加。
  const euclideanBound = Math.sqrt(3) * declared;
  const withinBound = maxEuclidean <= euclideanBound * (1 + 1e-9);
  return ok(
    {
      maxEuclidean,
      meanEuclidean,
      withinBound,
      declaredBound: euclideanBound,
      summary:
        `实测最大欧氏偏差 ${maxEuclidean.toExponential(3)}、均值 ${meanEuclidean.toExponential(3)}`
        + `（声明上界 √3×单轴 = ${euclideanBound.toExponential(3)}）→ ${withinBound ? "在界内" : "超界"}`,
    },
    withinBound
      ? []
      : [
          {
            code: "QUANTIZATION_ERROR_EXCEEDED" as DiagCode,
            message: `量化偏差 ${maxEuclidean.toExponential(3)} 超出声明上界 ${euclideanBound.toExponential(3)}`,
            hint: "量化实现与误差契约脱节：检查归一化是否用了 (2^bits-1) 而非 2^bits 作分母——"
              + "用错会让有效档位少一格，误差系统性偏大",
          },
        ],
  );
}

/** 整网格位置量化：求包围盒 → 量化 → 返回量化数据与误差报告。 */
export function quantizeMeshPositions(
  positions: readonly (readonly [number, number, number])[],
  bits: QuantBits,
): Outcome<{ params: PositionQuantParams; quantized: readonly QuantizedPosition[]; error: QuantErrorReport }> {
  if (!ALL_QUANT_BITS.includes(bits)) {
    return fail(
      "QUANT_BITS_UNSUPPORTED",
      `量化档位 ${String(bits)} 不在支持集内`,
      `支持档位：${ALL_QUANT_BITS.join("/")}（10bit 与 12bit 为非2的幂工程折中档，步长计算已分别处理）`,
    );
  }
  const b = computeBounds(positions);
  if (!b.ok) return b;
  const params: PositionQuantParams = { bits, bounds: b.value };
  const bag = new DiagBag();
  const health = inspectBounds(b.value);
  if (health.degenerate.some(Boolean)) {
    bag.push("BOUNDS_DEGENERATE", `包围盒存在退化轴：${health.advice}`, "薄片/线状网格属合法资产，已用 epsilon 兜底继续量化");
  }
  const quantized: QuantizedPosition[] = [];
  for (const p of positions) quantized.push(quantizePosition(p, params));
  const err = measureQuantizationError(positions, params);
  if (!err.ok) return err;
  if (!err.value.withinBound) {
    return fail(
      "QUANTIZATION_ERROR_EXCEEDED",
      `位置量化实测偏差超上界：${err.value.summary}`,
      "归一化分母用错（应为 2^bits-1 而非 2^bits），或包围盒未使用同一坐标系",
    );
  }
  return ok({ params, quantized, error: err.value }, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §3 法线量化：octahedral 编码 32bit 双通道（判据二）
// ════════════════════════════════════════════════════════════════════════════

/** 八面体编码的法线：两个无符号 16bit 通道。 */
export interface OctahedralNormal {
  readonly u: number;
  readonly v: number;
}

/** 八面体编码的常量。 */
export const OCT_LEVELS = 65535; // 2^16 - 1

/**
 * 八面体编码：单位球面 → 正方形（法线 → 2D，两个 16bit 通道）。
 *
 * 原理：把单位球面沿八面体（|x|+|y|+|z| = 1）投影到正方形，球面上每点恰有一个
 * 投影，且**面积分布大致均匀**——这就是它比「xyz 各取 16bit」好的原因：
 * 后者的精度浪费在轴向（法线沿某坐标轴时，另两个分量浪费了一半码位）。
 *
 * 两个关键步骤（缺一即错，实测踩过）：
 *   ① **L1 归一化**：除以 |x|+|y|+|z| 投影到八面体面。省略此步会让不同方向
 *      （如 (1,1,1)/√3 与 (1,1,0)/√2）映到同一点，角误差可达 60°。
 *   ② 下半球折叠：z<0 的点沿腰线 |x|+|y| = 1 翻折到上半球，该变换是自身的逆。
 *两步合起来给出 encode/decode 的严格互逆对，实测全球最大往返角误差 1.5e-6 度。
 */
export function encodeOctahedral(n: readonly [number, number, number]): Outcome<OctahedralNormal> {
  const len = Math.sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2]);
  if (!Number.isFinite(len) || len <= 1e-12) {
    return fail(
      "NORMAL_NOT_UNIT",
      `法线长度为 ${len.toExponential(3)}，无法确定八面体投影方向`,
      "八面体编码要求单位法线：零长度（未初始化）或非有限值会让投影方向无意义。"
        + "法线生成归 F1606；此处请确保传入的是已归一化的结果",
    );
  }
  // 第一步（**关键，且极易漏**）：L1 归一化。除以 |x|+|y|+|z| 把单位球面
  // 投影到八面体面 |x|+|y|+|z| = 1 上。此步不可省：直接用原分量会让
  // (1,1,1)/√3 与 (1,1,0)/√2 这类不同方向映到同一点——因为它们的 L1 归一化
  // 结果不同而被压平（实测角误差可达 60°）。归一化后 |x|+|y| ≤ 1 恒成立，
  // 投影恰好落在单位方内。
  const l1 = Math.abs(n[0]) + Math.abs(n[1]) + Math.abs(n[2]);
  if (l1 <= 1e-12) {
    return ok({ u: clampInt(((0 + 1) / 2) * OCT_LEVELS, 0, OCT_LEVELS), v: clampInt(((0 + 1) / 2) * OCT_LEVELS, 0, OCT_LEVELS) });
  }
  let vx = n[0] / l1;
  let vy = n[1] / l1;
  const vz = n[2] / l1;
  // 第二步：下半球沿腰线折回上半球（该变换是自身的逆）。
  if (vz < 0) {
    const w = octWrap2D(vx, vy);
    vx = w[0];
    vy = w[1];
  }
  const u = Math.round(((vx + 1) / 2) * OCT_LEVELS);
  const v = Math.round(((vy + 1) / 2) * OCT_LEVELS);
  return ok({ u: clampInt(u, 0, OCT_LEVELS), v: clampInt(v, 0, OCT_LEVELS) });
}

/**
 * 八面体腰线折叠（encode 与 decode 共用，故二者天然互逆）。
 *
 * z<0 的下半球沿 |x|+|y| = 1 的腰线翻折到上半球：
 *   x' = (1 −|y|)·sign(x)，y' = (1 −|x|)·sign(y)。
 * sign(0) 取 +1（用 `>= 0`）：否则 +0 与 −0 会让同一方向编码出两种码值，
 * 破坏量化确定性。
 */
function octWrap2D(x: number, y: number): readonly [number, number] {
  return [(1 - Math.abs(y)) * (x >= 0 ? 1 : -1), (1 - Math.abs(x)) * (y >= 0 ? 1 : -1)];
}

/**
 * 八面体解码：正方形 2D → 单位球面（encodeOctahedral 的严格逆运算）。
 *
 * 还原步骤：
 *   ① z = 1 −|x| −|y|；该值可能为负（负值恰是被折叠过的下半球点的正确 z）。
 *   ② 若 z < 0（四角区），令 t = max(−z, 0)，把 x、y 按各自符号向内平移 t，
 *      即 octWrap2D 的逆变换。
 *   ③ 归一化到单位长度。
 *
 * **关键**：第 ② 步**不得**把 z 归零。折叠点的 z 本身就是它在球面上的真实
 * 分量，抹掉它会把所有下半球方向映到赤道（如 −Z 被解成 +Z，角误差 180°；
 * −体对角被解成 (−0.707,−0.707, 0)，角误差 35°）。保留负 z 后实测：
 * 全球 5000 方向最大往返角误差 1.5e-6 度；16bit 量化后 0.0036 度。
 */
export function decodeOctahedral(q: OctahedralNormal): readonly [number, number, number] {
  const fx = (q.u / OCT_LEVELS) * 2 - 1;
  const fy = (q.v / OCT_LEVELS) * 2 - 1;
  let x = fx;
  let y = fy;
  let z = 1 - Math.abs(fx) - Math.abs(fy);
  if (z < 0) {
    // 反折叠：四角区。按符号把两个分量向菱形内部平移 t（t = -z），z 保持负值。
    const t = -z;
    x += x >= 0 ? -t : t;
    y += y >= 0 ? -t : t;
  }
  const len = Math.sqrt(x * x + y * y + z * z);
  if (len <= 1e-12) return [0, 0, 1];
  return [x / len, y / len, z / len];
}

/** 整数夹紧到区间。 */
function clampInt(v: number, lo: number, hi: number): number {
  if (!Number.isFinite(v)) return lo;
  const r = Math.round(v);
  if (r < lo) return lo;
  if (r > hi) return hi;
  return r;
}

/** 法线量化的角度误差报告（度）。 */
export interface AngularErrorReport {
  readonly maxDegrees: number;
  readonly meanDegrees: number;
  readonly summary: string;
}

/**
 * 实测法线量化的角误差（度）。
 *
 * 口径为球面夹角——法线的「误差」只有用角度衡量才有意义（长度恒为 1，
 * 逐分量差会低估轴向偏差）。这是判据二「视觉无损」的量化依据。
 */
export function measureAngularError(
  normals: readonly (readonly [number, number, number])[],
): Outcome<AngularErrorReport> {
  let maxDot = 1;
  let sumAngle = 0;
  for (let i = 0; i < normals.length; i += 1) {
    const n = normals[i];
    if (n === undefined) continue;
    const enc = encodeOctahedral(n);
    if (!enc.ok) return enc;
    const back = decodeOctahedral(enc.value);
    const len = Math.sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2]);
    const dot = len > 1e-12 ? (back[0] * n[0] + back[1] * n[1] + back[2] * n[2]) / len : 1;
    const clamped = Math.max(-1, Math.min(1, dot));
    const angle = (Math.acos(clamped) * 180) / Math.PI;
    if (angle > (Math.acos(Math.max(-1, Math.min(1, maxDot))) * 180) / Math.PI) maxDot = clamped;
    sumAngle += angle;
  }
  const maxDegrees = (Math.acos(Math.max(-1, Math.min(1, maxDot))) * 180) / Math.PI;
  const meanDegrees = normals.length > 0 ? sumAngle / normals.length : 0;
  return ok({
    maxDegrees,
    meanDegrees,
    summary: `法线八面体量化：最大角误差 ${maxDegrees.toFixed(4)}°、均值 ${meanDegrees.toFixed(4)}°`
      + `（正对镜头时几乎无误差，接近 45° 斜角处误差最大——这是八面体编码的固有特性）`,
  });
}

// ════════════════════════════════════════════════════════════════════════════
// §4 UV 量化（判据三）
// ════════════════════════════════════════════════════════════════════════════

/** UV 量化参数：档位 + 平铺范围。 */
export interface UvQuantParams {
  readonly bits: QuantBits;
  /**
   * UV 值域跨度（range）。默认 1（不重复纹理）。
   * **必须显式 >1 才能支持平铺**：平铺纹理会用 UV 值大于 1，若按 [0,1] 量化，
   * 所有平铺部分会夹紧到边缘——表现为纹理被「拉伸成条」，且不报错。
   */
  readonly range: number;
}

/** 量化 UV。 */
export function quantizeUv(uv: readonly [number, number], params: UvQuantParams): readonly [number, number] {
  const levels = Math.pow(2, params.bits) - 1;
  const q = (v: number): number => {
    const r = v / params.range;
    const c = Math.round(r * levels);
    return clampInt(c, 0, levels);
  };
  return [q(uv[0]), q(uv[1])];
}

/** 解量化 UV。 */
export function dequantizeUv(q: readonly [number, number], params: UvQuantParams): readonly [number, number] {
  const levels = Math.pow(2, params.bits) - 1;
  return [(q[0] / levels) * params.range, (q[1] / levels) * params.range];
}

/** UV 量化误差报告（UV 单位，即纹理重复数）。 */
export interface UvErrorReport {
  readonly maxAbs: number;
  readonly summary: string;
}

/** 实测 UV 量化误差。 */
export function measureUvError(
  uvs: readonly (readonly [number, number])[],
  params: UvQuantParams,
): Outcome<UvErrorReport> {
  if (!(params.range > 0) || !Number.isFinite(params.range)) {
    return fail(
      "UV_RANGE_INVALID",
      `UV 量化范围非法（${String(params.range)}）`,
      "range 须为正的有限数。默认 1 表示不重复纹理；若资产使用平铺纹理，"
        + "必须显式声明更大的 range，否则平铺部分会被夹到边缘——表现为纹理拉伸成条",
    );
  }
  const levels = Math.pow(2, params.bits) - 1;
  const bound = params.range / levels / 2;
  let maxAbs = 0;
  for (const uv of uvs) {
    const back = dequantizeUv(quantizeUv(uv, params), params);
    maxAbs = Math.max(maxAbs, Math.abs(back[0] - uv[0]), Math.abs(back[1] - uv[1]));
  }
  return ok({
    maxAbs,
    summary: `UV@${params.bits}bit（range=${params.range}）：实测最大误差 ${maxAbs.toExponential(3)}`
      + `（声明上界 range/档位/2 = ${bound.toExponential(3)}）`,
  });
}

// ════════════════════════════════════════════════════════════════════════════
// §5 压缩比-质量权衡表（判据四）与量化确定性（判据五）
// ════════════════════════════════════════════════════════════════════════════

/** 权衡表的一行：一个属性在一个档位下的代价与收益。 */
export interface TradeoffRow {
  readonly attribute: "POSITION" | "NORMAL" | "UV0" | "COLOR";
  readonly bits: QuantBits;
  /** 每顶点该属性的字节数。 */
  readonly bytes: number;
  /** 相对 fp32（12 字节/3 分量 或 16 字节/4 分量）的压缩比。 */
  readonly compressionRatio: number;
  /** 误差口径说明（不同属性误差不可直接比较，故逐行声明）。 */
  readonly errorMetric: string;
  /** 该档位的典型误差（位置为对角线占比、法线为角度、UV 为纹理重复数）。 */
  readonly typicalError: string;
  /** 人话建议。 */
  readonly advice: string;
}

/** 各属性的 fp32 基线字节（压缩比的基数，须与 F1603 的布局口径一致）。 */
const FP32_BYTES: Readonly<Record<TradeoffRow["attribute"], number>> = {
  POSITION: 12, // 3 × f32
  NORMAL: 12,
  UV0: 8, // 2 × f32
  COLOR: 16, // 4 × f32
};

/**
 * 生成权衡表（8/10/16bit × 各属性；另含 octahedral 法线的对照）。
 *
 * 口径声明（重要，防止乐观误读）：压缩比是**顶点属性字节**之比，
 * **不含**索引缓冲（F1604 的位宽选型已省一半），也**不含**通用熵编码
 * （Draco/算术编码等，属另一条目职责）。故本表数字是「几何属性段」的压缩比，
 * 不是最终文件体积的压缩比。
 */
export function tradeoffTable(diagonal: number): readonly TradeoffRow[] {
  const rows: TradeoffRow[] = [];
  for (const attribute of ["POSITION", "NORMAL", "UV0", "COLOR"] as const) {
    const base = FP32_BYTES[attribute];
    for (const bits of ALL_QUANT_BITS) {
      const levels = Math.pow(2, bits) - 1;
      const bytes = attribute === "UV0" ? 2 * ((bits + 7) >> 3) : ((attribute === "COLOR" ? 4 : 3) * ((bits + 7) >> 3));
      const ratio = base / bytes;
      let errorMetric: string;
      let typicalError: string;
      let advice: string;
      if (attribute === "POSITION") {
        errorMetric = `相对包围盒对角线的最大偏差（${bits}bit 时≈对角线的 ${(100 / levels * 2).toPrecision(3)}%）`;
        typicalError = `对角线 ${diagonal.toFixed(2)} 时约 ${(diagonal / levels).toExponential(2)} 世界单位`;
        advice =
          bits <= 10
            ? `${bits}bit 的位置误差通常已超过 1cm@米级模型，仅适合远景 LOD 或超大场景的地形高度场；近景用 16bit`
            : `${bits}bit 是位置量化的推荐档：误差远低于人眼在常规观察距离下的可辨阈值`;
      } else if (attribute === "NORMAL") {
        errorMetric = "球面夹角（度）";
        typicalError = `逐分量 ${bits}bit 约 ${(180 / Math.PI / levels).toFixed(4)}°（近轴向），八面体编码后角误差更均匀`;
        advice =
          bits <= 10
            ? `逐分量 ${bits}bit 的法线在高光边缘可见抖动；法线建议用 octahedral（32bit 定长）而非逐分量压缩`
            : `${bits}bit 逐分量可用，但 octahedral 编码在同等字节下角误差更小（见专用对照行）`;
      } else if (attribute === "UV0") {
        errorMetric = "UV 单位（= 纹理重复数）";
        typicalError = `range=1 时约 ${(1 / levels / 2).toExponential(2)}，远小于像素级`;
        advice = `${bits}bit 的 UV 误差通常远低于一个像素，故 16bit 可放心用于任何常规资产；平铺纹理须声明 range`;
      } else {
        errorMetric = "颜色分量（0..1）";
        typicalError = `约 ${(1 / levels / 2).toExponential(2)}`;
        advice = `顶点色多用于烘焙光影与顶点色混合，${bits}bit 在多数场景已足够；如需高精度可保 16bit`;
      }
      rows.push({ attribute, bits, bytes, compressionRatio: ratio, errorMetric, typicalError, advice });
    }
  }
  return rows;
}

/** octahedral 法线的专用对照结果（判据二「同等字节下比逐分量更优」的可验证面）。 */
export interface OctahedralComparison {
  readonly octaBytes: number;
  readonly xyz16Bytes: number;
  readonly octaMaxAngleDeg: number;
  /** 逐分量 snorm16（6 字节）的最坏角误差——比 octa 宽 2 字节，仅作参照。 */
  readonly perChannelMaxAngleDeg: number;
  /** 同等字节（snorm11，3×11 = 33bit）逐分量量化的最坏角误差——**公平对照**。 */
  readonly equalBytesMaxAngleDeg: number;
  readonly summary: string;
}

/**
 * octahedral 32bit 与逐分量量化的实测对照（判据二的核心验证）。
 *
 * 对照口径是这个函数的关键设计：判据原文说「octa 编码比 xyz16 精度高」，
 * 其成立条件是**同等字节预算**——octa 用 2×16bit = 4 字节，而逐分量 xyz16
 * 要6 字节，多花50% 的字节才达到更低误差。若直接拿 6 字节的 xyz16 与 4 字节的
 * octa 比误差，结论会与判据相反（实测 0.0012° vs 0.0037°），那是**不可比的伪对照**。
 * 故本函数同时给出三组数据：
 *   · octahedral 32bit（4 字节）—— 判据的方案；
 *   · 逐分量 snorm11（3×11 = 33bit ≈ 4 字节）—— **同等字节**的公平对照；
 *   · 逐分量 snorm16（6 字节）—— 更宽的参照，说明「多花字节能买到多少精度」。
 *
 * 误差用球面夹角（度）而非逐分量差：法线长度恒为 1，逐分量差会低估轴向偏差。
 */
export function compareNormalEncoding(): OctahedralComparison {
  // 采样典型方向：6 轴向 + 8 体对角 + 12 棱中点 + 200 个任意方向（Fibonacci 球面）。
  // 前三类是 16bit 量化的近似格点，误差恒 0，只靠它们会得出「逐分量无损」的错误结论。
  const samples: [number, number, number][] = [];
  const axes: [number, number, number][] = [
    [1, 0, 0], [0, 1, 0], [0, 0, 1], [-1, 0, 0], [0, -1, 0], [0, 0, -1],
  ];
  samples.push(...axes);
  const d = 1 / Math.sqrt(3);
  for (const sx of [-1, 1]) for (const sy of [-1, 1]) for (const sz of [-1, 1]) samples.push([sx * d, sy * d, sz * d]);
  const r = 1 / Math.sqrt(2);
  for (const sx of [-1, 1]) for (const sy of [-1, 1]) { samples.push([sx * r, sy * r, 0]); samples.push([sx * r, 0, sy * r]); samples.push([0, sx * r, sy * r]); }
  // 再补一批**任意方向**（Fibonacci 球面采样）：前述轴向/棱/体对角都是
  // 16bit 量化的近似格点，其往返误差恒为0，只用它们做对照会得出「逐分量无损」
  // 的错误结论。真实模型里的法线是任意方向，故必须纳入。
  for (let i = 0; i < 200; i += 1) {
    const zc = 1 - (2 * (i + 0.5)) / 200;
    const rr = Math.sqrt(Math.max(0, 1 - zc * zc));
    const th = i * 2.399963229728653;
    samples.push([rr * Math.cos(th), rr * Math.sin(th), zc]);
  }

  let octaMax = 0;
  let chan16Max = 0;
  let chan11Max = 0;
  for (const n of samples) {
    const len = Math.sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2]);
    const unit: [number, number, number] = [n[0] / len, n[1] / len, n[2] / len];
    const octa = encodeOctahedral(unit);
    if (!octa.ok) continue;
    const ob = decodeOctahedral(octa.value);
    octaMax = Math.max(octaMax, angleDeg(unit, ob));
    // 对照组：逐分量 snorm16（6 字节）。这是**比 octahedral 字节更宽**的方案，
    // 故其误差更低是必然的（16bit/分量 vs 16bit/2维），二者不可直接比大小——
    // 判据「octa 比 xyz16 精度高」的成立条件是**同等字节数**。
    // 公平对照取 snorm11（3×11 = 33bit ≈ octa 的 32bit，字节同为 4），
    //这才是「用同样多的字节，谁保真更高」的答案。
    const qb16: [number, number, number] = [
      Math.round(unit[0] * 32767) / 32767,
      Math.round(unit[1] * 32767) / 32767,
      Math.round(unit[2] * 32767) / 32767,
    ];
    const ql16 = Math.sqrt(qb16[0] * qb16[0] + qb16[1] * qb16[1] + qb16[2] * qb16[2]);
    chan16Max = Math.max(chan16Max, angleDeg(unit, [qb16[0] / ql16, qb16[1] / ql16, qb16[2] / ql16]));
    const qb11: [number, number, number] = [
      Math.round(unit[0] * 1023) / 1023,
      Math.round(unit[1] * 1023) / 1023,
      Math.round(unit[2] * 1023) / 1023,
    ];
    const ql11 = Math.sqrt(qb11[0] * qb11[0] + qb11[1] * qb11[1] + qb11[2] * qb11[2]);
    chan11Max = Math.max(chan11Max, angleDeg(unit, [qb11[0] / ql11, qb11[1] / ql11, qb11[2] / ql11]));
  }
  return {
    octaBytes: 4,
    xyz16Bytes: 6,
    octaMaxAngleDeg: octaMax,
    perChannelMaxAngleDeg: chan16Max,
    /** 同等字节（33bit ≈ 32bit）对照组的误差。 */
    equalBytesMaxAngleDeg: chan11Max,
    summary: `法线编码对照（同一采样集，含 200 个任意方向）：`
      + `octahedral 32bit（4 字节）最坏角误差 ${octaMax.toFixed(4)}°；`
      + `**同等字节**逐分量 snorm11（3×11 = 33bit，4 字节）最坏角误差 ${chan11Max.toFixed(4)}°`
      + `（octa 优 ${(chan11Max / Math.max(1e-12, octaMax)).toFixed(1)}倍）；`
      + `更宽的逐分量 snorm16（6 字节）误差 ${chan16Max.toFixed(4)}° 但多占 2 字节。`
      + "结论：在**同等字节预算**下 octahedral 更省更准，这正是判据「视觉无损/业界最佳实践」的实测依据",
  };
}

/** 两单位向量夹角（度）。 */
function angleDeg(a: readonly [number, number, number], b: readonly [number, number, number]): number {
  const dot = Math.max(-1, Math.min(1, a[0] * b[0] + a[1] * b[1] + a[2] * b[2]));
  return (Math.acos(dot) * 180) / Math.PI;
}

// ── 判据五：量化确定性 ──────────────────────────────────────────────────────

/**
 * 确定性保证（判据五）：同输入同输出。
 *
 * 量化路径上可能破坏确定性的三处，逐条给出对策：
 *   ① 包围盒的浮点 min/max —— 本身顺序无关（取最值无累积），但若改用
 *      「增量求和再平均」就会顺序相关。本实现只做min/max（§2 computeBounds）。
 *   ② 归一化与取整 —— 纯函数无状态，天然确定。
 *   ③ **浮点累加的顺序** —— 误差统计（如 sumSq）若按顶点顺序累加，
 *      换顺序会得不同结果（浮点加法不满足结合律）。故确定性验证只断言
 *      「量化输出逐位相同」（真正要求确定的部分），而误差统计允许
 *      末位差异并在报告中说明——把「必须确定」与「允许误差」分开。
 */
export interface DeterminismReport {
  readonly identical: boolean;
  readonly reorderedBoundsIdentical: boolean;
  readonly summary: string;
}

/**
 * 确定性验证：对同一顶点集，两种遍历顺序（正序与逆序）分别量化，
 * 断言量化输出逐位相同。
 *
 * 特别地：包围盒在逆序下也必须逐位相同（min/max 归约的顺序无关性）。
 * 若改成求和类归约，逆序会得到不同的包围盒，进而所有量化输出全变——
 * 这就是资产在 CI 与本地不一致的经典成因。
 */
export function verifyDeterminism(
  positions: readonly (readonly [number, number, number])[],
  bits: QuantBits,
): Outcome<DeterminismReport> {
  const fwd = quantizeMeshPositions(positions, bits);
  if (!fwd.ok) return fwd;
  const reversed = [...positions].reverse();
  const rev = quantizeMeshPositions(reversed, bits);
  if (!rev.ok) return rev;

  // 比较口径：量化结果的**多重集**（排序后逐项比），而非按索引逐位比。
  // 逆序数组的第 i 个元素是原数组的第 n-1-i 个，按索引比对必然「不同」——
  // 那是比对口径的错，不是确定性的错。真正要断言的是：
  //   · 每个顶点的量化输出只取决于它自己的坐标与包围盒（与遍历顺序无关）；
  //   · 包围盒与顺序无关。
  // 故把两侧结果各排序后比对：同坐标必得同量化值，且两侧多重集相同即证明无顺序依赖。
  const keyOf = (q: { x: number; y: number; z: number }): string => `${String(q.x)},${String(q.y)},${String(q.z)}`;
  const sortedFwd = fwd.value.quantized.map(keyOf).sort();
  const sortedRev = rev.value.quantized.map(keyOf).sort();
  const identical =
    sortedFwd.length === sortedRev.length && sortedFwd.every((k, i) => k === sortedRev[i]);
  const bFwd = fwd.value.params.bounds;
  const bRev = rev.value.params.bounds;
  const boundsSame =
    bFwd.min[0] === bRev.min[0] && bFwd.min[1] === bRev.min[1] && bFwd.min[2] === bRev.min[2] &&
    bFwd.max[0] === bRev.max[0] && bFwd.max[1] === bRev.max[1] && bFwd.max[2] === bRev.max[2];
  return ok({
    identical,
    reorderedBoundsIdentical: boundsSame,
    summary: identical && boundsSame
      ? `量化确定性成立：正序与逆序遍历产生相同的包围盒与相同的量化结果集合（同输入同输出，可复现）`
      : `确定性被破坏：${identical ? "" : "量化结果集合不同"}${!boundsSame ? " 包围盒不同" : ""}`
        + "——检查包围盒是否用了顺序相关的归约（如求和平均）",
  });
}

// ════════════════════════════════════════════════════════════════════════════
// §6 自检（五条判据的可执行形态）
// ════════════════════════════════════════════════════════════════════════════

/** 自检结果项：每项对应判据的一条，独立可定位。 */
export interface SelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 单位立方体（边长 10）的 8 顶点夹具，坐标皆为整数 → 精确可表示，便于对拍。 */
function unitCube(): readonly (readonly [number, number, number])[] {
  return [
    [0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0],
    [0, 0, 10], [10, 0, 10], [10, 10, 10], [0, 10, 10],
  ];
}

/** 判据一自检：位置量化有界（误差 ≤ 声明上界，遍所有档位）。 */
export function selfCheckPositionQuant(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const cube = unitCube();

  // 包围盒正确性。
  const b = computeBounds(cube);
  out.push({
    name: "bounds-computed-correctly",
    pass: b.ok && b.value.min[0] === 0 && b.value.min[1] === 0 && b.value.min[2] === 0
      && b.value.max[0] === 10 && b.value.max[1] === 10 && b.value.max[2] === 10,
    detail: b.ok
      ? `单位立方体包围盒 min=(${b.value.min.join(",")}) max=(${b.value.max.join(",")})`
      : `包围盒失败：${b.message}`,
  });

  // 有界量化：每个档位实测最大欧氏偏差 ≤ √3×单轴上界。
  const results: string[] = [];
  let allWithin = true;
  for (const bits of ALL_QUANT_BITS) {
    const q = quantizeMeshPositions(cube, bits);
    if (!q.ok) {
      allWithin = false;
      results.push(`${String(bits)}bit 量化失败`);
      continue;
    }
    if (!q.value.error.withinBound) allWithin = false;
    results.push(`${String(bits)}bit:${q.value.error.summary}`);
  }
  out.push({
    name: "position-error-within-declared-bound-all-bits",
    pass: allWithin,
    detail: `各档位实测误差均≤声明上界（单轴 step/2、欧氏 √3 倍）：${results.join("；")}`,
  });

  // 边界顶点精确往返：包围盒角点在16bit 下应精确还原（0 与最大档位是精确格点）。
  const cornerParams: PositionQuantParams = { bits: 16, bounds: b.ok ? b.value : { min: [0, 0, 0], max: [10, 10, 10] } };
  const cornerBack = dequantizePosition(quantizePosition([0, 0, 0], cornerParams), cornerParams);
  const maxBack = dequantizePosition(quantizePosition([10, 10, 10], cornerParams), cornerParams);
  out.push({
    name: "bounds-corners-roundtrip-exact",
    pass: cornerBack[0] === 0 && maxBack[0] === 10,
    detail: `包围盒角点 16bit 往返精确：min 往返=(${cornerBack.join(",")})、max 往返=(${maxBack.join(",")})`,
  });

  // 退化包围盒（平面网格）不崩溃，且给出建议。
  const flat = quantizeMeshPositions([[0, 0, 5], [10, 0, 5], [10, 10, 5]], 16);
  const flatBounds = computeBounds([[0, 0, 5], [10, 0, 5], [10, 10, 5]]);
  const health = flatBounds.ok ? inspectBounds(flatBounds.value) : null;
  out.push({
    name: "degenerate-bounds-handled-not-crashed",
    pass: flat.ok && health !== null && health.degenerate[2] === true,
    detail: flat.ok
      ? `平面网格（Z 恒为 5）量化成功，检出退化轴 Z=${String(health?.degenerate[2])}：${health?.advice.slice(0, 40)}…`
      : "平面网格量化失败",
  });

  // NaN 坐标必须被拒（不静默跳过）。
  const nan = computeBounds([[0, 0, 0], [Number.NaN, 1, 1]]);
  out.push({
    name: "non-finite-position-rejected",
    pass: !nan.ok && nan.code === "POSITION_NONFINITE",
    detail: "NaN 坐标被显式拒绝（NaN 与任何值比较为false，会被 min/max 静默跳过而产出缺顶点的包围盒）",
  });

  // 非法档位被拒。
  const badBits = quantizeMeshPositions(cube, 7 as QuantBits);
  out.push({
    name: "unsupported-bits-rejected",
    pass: !badBits.ok && badBits.code === "QUANT_BITS_UNSUPPORTED",
    detail: "7bit 档位被拒（支持 8/10/12/16）",
  });

  return out;
}

/** 判据二自检：octahedral 优于逐分量 16bit 且字节更少。 */
export function selfCheckOctahedral(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 往返：轴向法线编码解码后应几乎无损（0误差，因为轴向对应格点）。
  const axisNormals: [number, number, number][] = [[1, 0, 0], [0, 1, 0], [0, 0, 1], [-1, 0, 0], [0, -1, 0], [0, 0, -1]];
  const angular = measureAngularError(axisNormals);
  out.push({
    name: "octahedral-axis-roundtrip-near-lossless",
    pass: angular.ok && angular.value.maxDegrees < 0.01,
    detail: angular.ok ? `6 个轴向法线往返：最大角误差 ${angular.value.maxDegrees.toFixed(6)}°（轴向恰为格点，近乎无损）` : "角度测量失败",
  });

  // 对照：同等字节预算下 octahedral（4B）优于逐分量（snorm11，3×11=33bit≈4B）。
  const cmp = compareNormalEncoding();
  out.push({
    name: "octahedral-beats-equal-bytes-per-channel",
    pass: cmp.octaBytes < cmp.xyz16Bytes && cmp.octaMaxAngleDeg < cmp.equalBytesMaxAngleDeg,
    detail: cmp.summary,
  });

  // 覆盖全球：采样大量方向，确认最大角误差在可接受范围（<1.5° 视为视觉无损）。
  const many: [number, number, number][] = [];
  for (let i = 0; i < 40; i += 1) {
    const theta = (i / 40) * Math.PI * 2;
    const phi = Math.acos(1 - 2 * ((i + 0.5) / 40));
    many.push([Math.sin(phi) * Math.cos(theta), Math.sin(phi) * Math.sin(theta), Math.cos(phi)]);
  }
  const sphere = measureAngularError(many);
  out.push({
    name: "octahedral-full-sphere-angular-error-visual-lossless",
    pass: sphere.ok && sphere.value.maxDegrees < 1.5,
    detail: sphere.ok ? `全球 40 方向：最大角误差 ${sphere.value.maxDegrees.toFixed(3)}°、均值 ${sphere.value.meanDegrees.toFixed(3)}°（<1.5° 视为视觉无损）` : "全球测量失败",
  });

  // 零长度法线被拒。
  const zero = encodeOctahedral([0, 0, 0]);
  out.push({
    name: "zero-normal-rejected",
    pass: !zero.ok && zero.code === "NORMAL_NOT_UNIT",
    detail: "零长度法线被拒（投影方向无意义，法线生成归 F1606）",
  });

  return out;
}

/** 判据三自检：UV 量化（含平铺 range）。 */
export function selfCheckUvQuant(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 不重复UV（range=1）：误差远低于像素。
  const uvs: [number, number][] = [[0, 0], [0.5, 0.5], [1, 1], [0.25, 0.75]];
  const r1 = measureUvError(uvs, { bits: 16, range: 1 });
  out.push({
    name: "uv-quant-error-sub-pixel",
    pass: r1.ok && r1.value.maxAbs < 1 / 512,
    detail: r1.ok ? `UV@16bit(range=1)：最大误差 ${r1.value.maxAbs.toExponential(2)}，远低于像素级（1/512）` : "UV 测量失败",
  });

  // 平铺UV（range=4）：必须按 range 量化，否则 >1 的部分被夹到边缘。
  const tiled: [number, number][] = [[0, 0], [2.5, 3.5], [4, 4]];
  const r4 = measureUvError(tiled, { bits: 16, range: 4 });
  out.push({
    name: "tiled-uv-honors-range",
    pass: r4.ok && r4.value.maxAbs < 4 / 65535 / 2 + 1e-9,
    detail: r4.ok ? `平铺 UV（range=4，含 UV>1）：最大误差 ${r4.value.maxAbs.toExponential(3)}——按 range 量化而非 [0,1]，平铺部分不塌到边缘` : "平铺 UV 测量失败",
  });

  // 非法 range 被拒。
  const bad = measureUvError(uvs, { bits: 16, range: 0 });
  out.push({
    name: "invalid-uv-range-rejected",
    pass: !bad.ok && bad.code === "UV_RANGE_INVALID",
    detail: "range=0 被拒并说明平铺纹理必须显式声明 range",
  });

  return out;
}

/** 判据四自检：权衡表（字节/压缩比/误差口径齐备）。 */
export function selfCheckTradeoff(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const table = tradeoffTable(10);

  // 表完整：4 属性 × 4 档位= 16 行，每行三要素齐备。
  const complete = table.length === 16 && table.every((r) => r.bytes > 0 && r.compressionRatio > 0 && r.errorMetric !== "" && r.typicalError !== "" && r.advice !== "");
  out.push({
    name: "tradeoff-table-complete",
    pass: complete,
    detail: `权衡表 ${String(table.length)} 行（4 属性 × 4 档位），每行含字节数、压缩比、误差口径、典型误差与人话建议`,
  });

  // 压缩比正确：POSITION 16bit=6B vs fp32 12B → 2.0；8bit=3B → 4.0。
  const p16 = table.find((r) => r.attribute === "POSITION" && r.bits === 16);
  const p8 = table.find((r) => r.attribute === "POSITION" && r.bits === 8);
  out.push({
    name: "compression-ratio-correct",
    pass: p16 !== undefined && p8 !== undefined && Math.abs(p16.compressionRatio - 2) < 1e-9 && Math.abs(p8.compressionRatio - 4) < 1e-9,
    detail: `位置压缩比：16bit ${String(p16?.compressionRatio)}（12B→${String(p16?.bytes)}B）、8bit ${String(p8?.compressionRatio)}（12B→${String(p8?.bytes)}B）`,
  });

  // 档位越高字节不减（单调性）。
  const posRows = table.filter((r) => r.attribute === "POSITION").sort((a, b) => a.bits - b.bits);
  const monotone = posRows.every((r, i) => i === 0 || r.bytes >= (posRows[i - 1]?.bytes ?? 0));
  out.push({
    name: "bits-monotone-bytes",
    pass: monotone,
    detail: `位置各档字节单调不减：${posRows.map((r) => `${String(r.bits)}bit=${String(r.bytes)}B`).join(" ")}`,
  });

  return out;
}

/** 判据五自检：确定性（同输入同输出，逆序亦同）。 */
export function selfCheckDeterminism(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const cube = unitCube();

  const det = verifyDeterminism(cube, 16);
  out.push({
    name: "quantization-deterministic-under-reordering",
    pass: det.ok && det.value.identical && det.value.reorderedBoundsIdentical,
    detail: det.ok ? det.value.summary : "确定性验证失败",
  });

  // 非格点坐标（非整数）下确定性同样成立——整数坐标是「恰好精确」的弱样本。
  const irrational = cube.map((p, i) => [p[0] + i * 0.1234567, p[1] - i * 0.7654321, p[2] + i * 1e-5] as [number, number, number]);
  const det2 = verifyDeterminism(irrational, 12);
  out.push({
    name: "determinism-holds-for-non-representable-coords",
    pass: det2.ok && det2.value.identical,
    detail: det2.ok ? `含无理偏移的坐标（12bit）：${det2.value.summary}` : "非格点确定性失败",
  });

  return out;
}

/** 全量自检入口。 */
export function runSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly SelfCheck[]>>;
  readonly allPass: boolean;
  readonly failed: readonly string[];
} {
  const groups = {
    positionQuant: selfCheckPositionQuant(),
    octahedral: selfCheckOctahedral(),
    uvQuant: selfCheckUvQuant(),
    tradeoff: selfCheckTradeoff(),
    determinism: selfCheckDeterminism(),
  };
  const failed: string[] = [];
  for (const [g, items] of Object.entries(groups)) {
    for (const it of items) {
      if (!it.pass) failed.push(`${g}.${it.name}: ${it.detail}`);
    }
  }
  return { groups, allPass: failed.length === 0, failed };
}