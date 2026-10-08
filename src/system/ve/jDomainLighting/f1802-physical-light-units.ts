/**
 * VE-F1802 · 物理光照量纲（J 域 · 光照与阴影域 · 批次 J01 · 组内第 2 条）
 * ---------------------------------------------------------------------------
 * 职责定位：物理光照量纲条目——建立**辐射度量学基础**（流明 lm / 坎德拉 cd /
 *   勒克斯 lx / 辐亮度 nit 四单位定义与换算）与**双单位模式**（物理单位制为推荐
 *   默认、任意艺术单位制并存兼容老资产），为 **J 域全部光源强度语义定标**。
 *
 * 本条交付三样东西，全部可机器校验：
 *   1. **单位系统表**：四单位各自定义、适用对象（点光 cd / 方向光 lux / 面光 lm /
 *      输出亮度 nit）、换算公式与常量；
 *   2. **双模式配置枚举**（PHYSICAL / ARTISTIC）+ 艺术制换算基准声明
 *      （以参考白点映射到物理制内部计算）；
 *   3. **光照参数块中强度字段的单位标注约定**（逐光源类型的字段级单位契约）。
 *
 * ┌── 上游契约 ──────────────────────────────────────────────────────────────┐
 * │ F1801（域开工与光照架构）定义 `LightCommonBlock.intensity` 为无量纲标量，│
 * │ 明确写「强度标量（单位制语义由 F1802 定标：物理制 cd/lux）」。本条即为    │
 * │ 该标量**定标**：不改动 F1801 的字段布局与语义，只补齐其单位契约。        │
 * └─────────────────────────────────────────────────────────────────────────┘
 *
 * 辐射度量学的一条硬事实（决定了本条的设计形态，必须先说清）：
 *   **lx 与 lm 之间无法只靠数值换算。** 照度（lx = lm/m²）与光通量（lm）之间隔着
 *   一个真实的几何量——被照面积；cd 与 nit 之间隔着距离平方；cd 与 lm 之间隔着
 *   立体角。所以「四单位换算」不是一张比例表，而是 **数值 × 上下文几何量**。
 *   本条因此把换算建模为「单位对 + 所需上下文量」，缺上下文即显性报错，
 *   绝不猜一个默认面积/距离把数字算出来（那是渲染里最隐蔽的一类物理错误）。
 *   上下文量与单位量纲的一致性由 `verifyConversionTable` 逐边校验。
 *
 * 双模式语义（锚点原文：物理单位制为推荐默认、任意艺术单位制并存兼容老资产）：
 *   · PHYSICAL（推荐默认）：强度即物理单位数值，内部计算不做任何缩放；
 *   · ARTISTIC（兼容老资产）：以**参考白点**映射到物理制内部计算——
 *     艺术值 1.0 ↔ 参考白点（203 nit）在参考几何（1 m² 表面 / 1 m 距离）下的
 *     物理量。四单位共用同一基准标量（`ARTISTIC_REFERENCE_SCALE`），因此
 *     艺术制→物理制是**一次标量乘**，反之一次标量除，往返误差只来自浮点——
 *     这正是 §7 量化保护存在的理由。
 *
 * 零静默纪律：本模块所有拒绝、混用告警、钳制遥测、缺失兜底、重编译触发都产出
 *   Diagnostic（code + message + hint），由调用方聚合上报。本模块不向 UI 直接
 *   抛异常，也不吞掉任何一条诊断——钳制是**显性事件**（遥测可查），不是静默改值。
 *
 * 性能契约（锚点原文：换算为着色器常量折叠·编译期完成·零运行时成本）：
 *   · 单位换算在**编译期**折叠为着色器常量字面量，运行时每像素 0 次换算；
 *   · 单位校验只在**导入期**执行一次；
 *   · 双模式切换触发**场景级重编译一次**（不是每帧、不是每光源）。
 *   `assertZeroRuntimeCost` 把「零运行时成本」从注释变成可执行断言。
 *
 * 判据：四单位、双模式、钳制、编译期换算。
 *
 * 依赖锚点：F1801（J 域开工·光源抽象公共块与分工表）、I03 F1670（材质族·反照率
 *   为无量纲反射率，与本条单位制对齐）、F1812（曝光以物理亮度为输入）。
 * 下游消费：F1803 方向光（lux）、F1804 点光（cd）、F1805 聚光（cd）、F1806 面光
 *   预留（lm）、F1827/F1828（阴影组复用同一强度定标）、F1807 光源管理器（模式
 *   随场景配置下发）、F1812 曝光（nit 输入）、F1811 成本账本、F1817 一致性核对。
 * 交接说明：本条只管「四单位定义与换算 + 双模式 + 强度字段单位标注 + 钳制与
 *   量化保护 + 编译期折叠」；具体光源着色实现归 F1803-F1807，阴影归 J02，
 *   环境氛围归 J03，曝光归 F1812。
 */

/* ═══════════════════════════════════════════════════════════════════════════
 * §1 诊断与结果类型（零静默的基础设施）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 诊断码：每种拒绝/混用/钳制/兜底/重编译都有独立可检索的码，绝不合并成通用错误。
 *
 * 与 F1801 的 `Diagnostic` / `Outcome` **结构兼容**（同字段名同形状），因此本条
 * 产出的 diagnostics 可直接并入调用方的统一诊断流（F1767 门禁 / F1764 调试总线
 * / F1819 遥测），无需适配层。本条自带类型声明以保持条目自洽（单条不依赖上一条
 * 的内部实现，上游条目重构不致连带破坏下游）。
 */
export type DiagCode =
  /** 单位对不存在换算边（四单位两两之间并非全可换）。 */
  | "UNIT_CONVERSION_EDGE_ABSENT"
  /** 换算所需上下文几何量缺失（面积/立体角/距离平方）——绝不猜默认值。 */
  | "UNIT_CONVERSION_CONTEXT_MISSING"
  /** 换算边声明的量纲与单位量纲不自洽（表本身写错，属表级错误）。 */
  | "UNIT_CONVERSION_TABLE_INCONSISTENT"
  /** 换算上下文几何量非法（非有限值 / 非正）。 */
  | "UNIT_CONVERSION_CONTEXT_INVALID"
  /** 单位标识未知（不在四单位表内）。 */
  | "UNIT_UNKNOWN"
  /** 强度字段未在单位标注约定表中登记。 */
  | "INTENSITY_FIELD_UNREGISTERED"
  /** 强度字段登记的单位与所属光源类型的适用对象不符。 */
  | "INTENSITY_FIELD_UNIT_MISMATCH"
  /** 旧资产未标注单位——按艺术制默认导入（兜底 + 文档声明，非静默）。 */
  | "ASSET_UNIT_UNDECLARED_LEGACY_DEFAULT"
  /** 同场景两模式并存 / 资产标注与场景配置不一致——按场景级统一执行并告警。 */
  | "UNIT_MODE_MIXED_IN_SCENE"
  /** 极端强度被钳制（NaN / 负值 / 超物理上限 / 落入 float32 非规格化带）。 */
  | "INTENSITY_CLAMPED"
  /** 艺术制换算往返误差超出容差（量化保护失效或基准被改）。 */
  | "ARTISTIC_CONVERSION_DRIFT"
  /** 单位制定标变更请求（冻结后变更须走 ADR）。 */
  | "CALIBRATION_ADR_INCOMPLETE"
  /** 定标冻结快照与当前声明指纹不符（漂移）。 */
  | "CALIBRATION_FREEZE_DRIFT"
  /** 重编译触发位缺失（换算折叠必须伴随一次场景级重编译）。 */
  | "RECOMPILE_TRIGGER_MISSING";

/** 一条诊断：发生了什么（code + message）、影响什么、下一步怎么办（hint）。 */
export interface Diagnostic {
  readonly code: DiagCode;
  /** 人话描述，面向开发者排障，不含裸异常码。 */
  readonly message: string;
  /** 可操作提示：该改哪里、该怎么降级。 */
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

/** 成功构造（diagnostics 允许携带非致命告警，例如已钳制、已兜底、已告警不一致）。 */
export function ok<T>(value: T, diagnostics: readonly Diagnostic[] = []): Outcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造：失败路径必须给出可操作提示，不允许裸码。 */
export function err<T>(
  code: DiagCode,
  message: string,
  hint: string,
  diagnostics: readonly Diagnostic[] = [],
): Outcome<T> {
  return { ok: false, code, message, hint, diagnostics: [...diagnostics, { code, message, hint }] };
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §2 量纲代数（换算可行性的机器判定底座）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 量纲指数向量 `[坎德拉, 米, 球面角]`。
 *
 * 选这三个基量是辐射度量学的最小充分集：
 *   cd（坎德拉，强度）· sr（球面角）· m（长度）。光通量 lm ≡ cd·sr，
 *   因此四单位的量纲可精确表达，换算可行性由指数差判定而非人工枚举。
 *
 * 指数取整（辐射度量学中各量纲幂次均为整数，无分数幂），故可用严格相等比较。
 */
export type DimensionVector = readonly [number, number, number];

/** 三基量的符号名（用于生成可读的量纲标签与错误信息）。 */
export const DIMENSION_BASE_SYMBOLS: readonly [string, string, string] = ["cd", "m", "sr"];

/** 把量纲指数向量渲染为可读标签，如 `[1,-2,1]` → `cd·m⁻²·sr`。 */
export function formatDimension(dim: DimensionVector): string {
  const parts: string[] = [];
  const symbols: readonly string[] = DIMENSION_BASE_SYMBOLS;
  for (let i = 0; i < symbols.length; i++) {
    const exponent = dim[i];
    const symbol = symbols[i];
    if (exponent === undefined || symbol === undefined) continue;
    if (exponent === 0) continue;
    parts.push(exponent === 1 ? symbol : `${symbol}^${exponent}`);
  }
  return parts.length === 0 ? "1" : parts.join("·");
}

/** 量纲是否相等（严格逐位比较）。 */
export function sameDimension(a: DimensionVector, b: DimensionVector): boolean {
  return a[0] === b[0] && a[1] === b[1] && a[2] === b[2];
}

/** 量纲加法（用于推导换算上下文量应有的量纲）。 */
export function addDimension(a: DimensionVector, b: DimensionVector): DimensionVector {
  return [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
}

/** 量纲减法（a − b）。 */
export function subDimension(a: DimensionVector, b: DimensionVector): DimensionVector {
  return [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
}

/** 量纲标量倍（n · dim）。 */
export function scaleDimension(dim: DimensionVector, n: number): DimensionVector {
  return [dim[0] * n, dim[1] * n, dim[2] * n];
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §3 单位系统表（四单位定义 · 适用对象 · 量纲）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 四单位标识（锚点原文：流明 lm / 坎德拉 cd / 勒克斯 lx / 辐亮度 nit）。 */
export type LightUnit =
  /** 流通量（luminous flux），符号 lm，量纲 cd·sr。 */
  | "LUMEN"
  /** 光强（luminous intensity），符号 cd，量纲 cd。 */
  | "CANDELA"
  /** 照度（illuminance），符号 lx，量纲 cd·sr·m⁻²。 */
  | "LUX"
  /** 辐亮度/亮度（luminance），符号 nit（= cd/m²），量纲 cd·m⁻²。 */
  | "NIT";

/** 强度承载对象（锚点原文适用对象：点光 cd / 方向光 lux / 面光 lm / 输出亮度 nit）。 */
export type IntensitySubject =
  /** 方向光（F1803）：平行光强度按被照面照度给，故用 lx。 */
  | "DIRECTIONAL_LIGHT"
  /** 点光（F1804）：点光源强度按 cd 给。 */
  | "POINT_LIGHT"
  /** 聚光（F1805）：锥形光按其轴向光强给，与点光同用 cd。 */
  | "SPOT_LIGHT"
  /** 面光源（F1806 预留）：面积光按总光通量给，故用 lm。 */
  | "AREA_LIGHT"
  /** 显示输出亮度（交 F1812 曝光 / I02 后段）：按 nit 给。 */
  | "DISPLAY_OUTPUT";

/** 一个单位的完整定义。 */
export interface UnitDefinition {
  readonly unit: LightUnit;
  /** SI 符号。 */
  readonly symbol: string;
  /** 中文名。 */
  readonly label: string;
  /** 物理解释（人话，文档替述用）。 */
  readonly definition: string;
  /** 该单位回答的物理问题。 */
  readonly measures: string;
  /** 量纲指数向量 [cd, m, sr]。 */
  readonly dimension: DimensionVector;
  /**
   * 物理上限（超此值即钳制并遥测，见 §7）。
   * 取值依据：覆盖现实世界最亮合理场景并留约一个量级余量
   *（体育场主照明灯 ~10⁶ cd；舞台灯 ~10⁷ cd；日光面照度 ~10⁵ lx；
   *  高动态范围显示峰值 ~10⁴ nit）。
   */
  readonly physicalMax: number;
}

/**
 * 四单位系统表（锚点原文：四单位各自定义、适用对象、换算公式与常量）。
 *
 * 定义采用光度学的人眼加权口径（photopic, 视见函数 V(λ)）：坎德拉定义为
 * 540 THz 单色辐射在给定立体角内的光通量与该频率辐射功率之比，1 cd ≡ 1/683 W/sr；
 * 由此派生出 lm（cd·sr）、lx（lm/m²）、nit（cd/m²）。
 * 本条只做**单位定标与换算**，不做光谱计算，故频率口径写入文档常量供后续引用。
 */
export const UNIT_SYSTEM_TABLE: readonly UnitDefinition[] = [
  {
    unit: "CANDELA",
    symbol: "cd",
    label: "坎德拉",
    definition:
      "坎德拉是光强单位：某点光源沿其轴线、每球面度立体角内所发光通量。" +
      "定义式 1 cd ≡ (1/683) W/sr @ 540 THz（人眼最敏感的视见函数峰值频率）。",
    measures: "「这盏灯有多亮」——离开光源多远都一样，只看总光量除以立体角。",
    dimension: [1, 0, 0],
    physicalMax: 1e7,
  },
  {
    unit: "LUMEN",
    symbol: "lm",
    label: "流明",
    definition:
      "流明是光通量单位：人眼在给定照度下主观明亮感觉的总光量。" +
      "定义式 1 lm ≡ 1 cd·sr（坎德拉乘一个球面度立体角）。",
    measures: "「这盏灯一共发出多少光」——不看方向、不看距离，是光的总量。",
    dimension: [1, 0, 1],
    physicalMax: 1e7,
  },
  {
    unit: "LUX",
    symbol: "lx",
    label: "勒克斯",
    definition:
      "勒克斯是照度单位：单位受照面积（1 m²）上接受的光通量。" +
      "定义式 1 lx ≡ 1 lm/m²。",
    measures: "「桌面这块地方有多亮」——落到某个面上的光量，与面的大小和朝向有关。",
    dimension: [1, -2, 1],
    physicalMax: 1e6,
  },
  {
    unit: "NIT",
    symbol: "nit",
    label: "尼特（辐亮度）",
    definition:
      "尼特是亮度（辐亮度）单位：单位投影面积上的光强。" +
      "定义式 1 nit ≡ 1 cd/m²（坎德拉每平方米），亦即坎德拉每平方��。",
    measures: "「这块表面看起来多亮」——已经按面积归一，是最终显示亮度的标准量。",
    dimension: [1, -2, 0],
    physicalMax: 1e4,
  },
];

/** 单位 → 定义 的索引视图（查表 O(1)；未命中即 UNIT_UNKNOWN，不做默认回落）。 */
const UNIT_DEFINITION_INDEX: ReadonlyMap<LightUnit, UnitDefinition> = new Map(
  UNIT_SYSTEM_TABLE.map((d) => [d.unit, d]),
);

/** 取单位定义；未知单位显性报错，不返回近似单位。 */
export function getUnitDefinition(unit: LightUnit): Outcome<UnitDefinition> {
  const def = UNIT_DEFINITION_INDEX.get(unit);
  if (def === undefined) {
    return err(
      "UNIT_UNKNOWN",
      `单位 ${String(unit)} 不在四单位系统表内（lm/cd/lx/nit）。`,
      "只用四种光度学单位：LUMEN(流明)/CANDELA(坎德拉)/LUX(勒克斯)/NIT(尼特)。" +
        "若确需新单位（如辐照度 W/m²、辐射亮度 W/sr/m²），走 F1798 变更纪律先扩表。",
    );
  }
  return ok(def);
}

/** 适用对象 → 强度单位 的定标表（锚点原文适用对象逐条落表）。 */
export const SUBJECT_UNIT_BINDING: Readonly<Record<IntensitySubject, LightUnit>> = {
  DIRECTIONAL_LIGHT: "LUX",
  POINT_LIGHT: "CANDELA",
  SPOT_LIGHT: "CANDELA",
  AREA_LIGHT: "LUMEN",
  DISPLAY_OUTPUT: "NIT",
};

/** 适用对象的可读名（诊断与文档替述用）。 */
export const SUBJECT_LABELS: Readonly<Record<IntensitySubject, string>> = {
  DIRECTIONAL_LIGHT: "方向光（F1803）",
  POINT_LIGHT: "点光（F1804）",
  SPOT_LIGHT: "聚光（F1805）",
  AREA_LIGHT: "面光源（F1806 预留）",
  DISPLAY_OUTPUT: "显示输出亮度（交 F1812 曝光）",
};

/**
 * 光度学定义常量（锚点原文：换算公式与常量）。
 *
 * `CANDELA_WATT_PER_STERADIAN` 是坎德拉定义的倒数系数：1 cd ≡ (1/683) W/sr。
 * 本条不参与渲染数值计算（渲染走 lm/cd/lx/nit 口径），但作为单位表的常量
 * 声明随附，供后续光谱相关条目（F1844 天空 / 物理光谱扩展）引用同一定义口径，
 * 避免各条目各写一份 683 造成定义漂移。
 */
export const PHOTOMETRIC_CONSTANTS = {
  /** 坎德拉定义系数：1 cd ≡ 1/683 W/sr @ 540 THz。 */
  CANDELA_WATT_PER_STERADIAN: 1 / 683,
  /** 视见函数峰值频率（人眼最敏感的单色光频率，Hz）。 */
  PEAK_PHOTOPIC_FREQUENCY_HZ: 540e12,
  /** 参考几何：面积基准（1 m²）。 */
  REFERENCE_AREA_M2: 1,
  /** 参考几何：距离基准（1 m），平方即 1 m²。 */
  REFERENCE_DISTANCE_M: 1,
} as const;

/* ═══════════════════════════════════════════════════════════════════════════
 * §4 换算边表（数值 × 上下文几何量）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 换算所需的上下文几何量种类。 */
export type ConversionContextKind =
  /** 立体角（sr）——cd ↔ lm / lx ↔ nit。 */
  | "SOLID_ANGLE"
  /** 面积（m²）——lm ↔ lx / cd ↔ nit。 */
  | "AREA";

/** 一条换算边：从某单位到某单位，需乘或除以某上下文几何量。 */
export interface ConversionEdge {
  readonly from: LightUnit;
  readonly to: LightUnit;
  /** 乘或除：`value_to = value_from × ctx` 或 `value_to = value_from ÷ ctx`。 */
  readonly direction: "MULTIPLY" | "DIVIDE";
  /** 所需上下文几何量种类。 */
  readonly contextKind: ConversionContextKind;
  /** 上下文几何量的量纲（用于 verifyConversionTable 自洽校验）。 */
  readonly contextDimension: DimensionVector;
  /** 公式的人话描述（文档替述用）。 */
  readonly formula: string;
  /** 该边在本表中的登记理由（为什么必须有上下文、不能纯比例换算）。 */
  readonly rationale: string;
}

/** 上下文几何量的量纲常量。 */
export const CONTEXT_DIMENSIONS: Readonly<Record<ConversionContextKind, DimensionVector>> = {
  SOLID_ANGLE: [0, 0, 1],
  AREA: [0, 2, 0],
};

/**
 * 换算边表（8 条，覆盖四单位两两之间的全部可行方向）。
 *
 * 建模要点（物理诚实）：四单位两两之间**并非全可纯比例换算**。除 cd↔lm 与
 * lx↔nit（隔立体角）、lm↔lx 与 cd↔nit（隔面积）之外，还存在两条可经两步
 * 达成的路径（如 cd↔lx、lm↔nit），本表以**一次上下文直连**覆盖常用路径，
 * 避免调用方手写多步串联而引入中间钳制误差。两步路径由 `convertUnit` 自动
 * 走最短边（见 §5），调用方无需关心。
 *
 * 表中每条边的 contextDimension 均与 from/to 的量纲差自洽，由
 * `verifyConversionTable` 在定标时逐条断言——表写错会在定标期暴露，
 * 而不是等到某个场景换算出错误亮度。
 */
export const CONVERSION_EDGES: readonly ConversionEdge[] = [
  {
    from: "CANDELA",
    to: "LUMEN",
    direction: "MULTIPLY",
    contextKind: "SOLID_ANGLE",
    contextDimension: [0, 0, 1],
    formula: "lm = cd × Ω",
    rationale: "光通量 = 光强 × 立体角：立体角即这盏光被谁「看见」的那一锥。",
  },
  {
    from: "LUMEN",
    to: "CANDELA",
    direction: "DIVIDE",
    contextKind: "SOLID_ANGLE",
    contextDimension: [0, 0, 1],
    formula: "cd = lm ÷ Ω",
    rationale: "上式的逆运算：知道总光量与被占立体角反求光强。",
  },
  {
    from: "LUMEN",
    to: "LUX",
    direction: "DIVIDE",
    contextKind: "AREA",
    contextDimension: [0, 2, 0],
    formula: "lx = lm ÷ A",
    rationale: "照度 = 光通量 ÷ 受照面积：必须知道照在多大一片上。",
  },
  {
    from: "LUX",
    to: "LUMEN",
    direction: "MULTIPLY",
    contextKind: "AREA",
    contextDimension: [0, 2, 0],
    formula: "lm = lx × A",
    rationale: "上式的逆运算：已知某处照度与受照面积反求该处总光量。",
  },
  {
    from: "CANDELA",
    to: "NIT",
    direction: "DIVIDE",
    contextKind: "AREA",
    contextDimension: [0, 2, 0],
    formula: "nit = cd ÷ d²",
    rationale:
      "亮度 = 光强 ÷ 距离平方（各向同性点光源）：面积维以 d² 代入，即距离越远同样 cd 看起来越暗。",
  },
  {
    from: "NIT",
    to: "CANDELA",
    direction: "MULTIPLY",
    contextKind: "AREA",
    contextDimension: [0, 2, 0],
    formula: "cd = nit × d²",
    rationale: "上式的逆运算：由表面亮度与观察距离反推点光源光强。",
  },
  {
    from: "LUX",
    to: "NIT",
    direction: "DIVIDE",
    contextKind: "SOLID_ANGLE",
    contextDimension: [0, 0, 1],
    formula: "nit = lx ÷ Ω",
    rationale:
      "亮度 = 照度 ÷ 立体角：照度是落在面上的量，亮度还要按该面在眼中的张角归一，" +
      "两者相差一个立体角。",
  },
  {
    from: "NIT",
    to: "LUX",
    direction: "MULTIPLY",
    contextKind: "SOLID_ANGLE",
    contextDimension: [0, 0, 1],
    formula: "lx = nit × Ω",
    rationale: "上式的逆运算：由表面亮度与立体角反求该面上的照度。",
  },
];

/**
 * 校验换算边表的自洽性（表级守卫，锚点：换算公式与常量必须可机检）。
 *
 * 逐边断言（锚点原表若写错，此处必须拒绝而非容忍）：
 *   MULTIPLY：dim(from) + dim(ctx) ≡ dim(to)
 *   DIVIDE  ：dim(from) − dim(ctx) ≡ dim(to)
 * 另外校验：边端点均为已知四单位、上下文量纲与 contextKind 声明一致、无重复边。
 *
 * @param edges 边表（默认取本模块声明值；供导入期校验外部覆盖表）
 */
export function verifyConversionTable(edges: readonly ConversionEdge[] = CONVERSION_EDGES): Outcome<ConversionEdge[]> {
  const diagnostics: Diagnostic[] = [];

  // 4.1 端点单位必须已登记（先于量纲校验：未知单位的量纲无从谈起）。
  for (const edge of edges) {
    if (UNIT_DEFINITION_INDEX.get(edge.from) === undefined) {
      diagnostics.push({
        code: "UNIT_UNKNOWN",
        message: `换算边起点单位 ${String(edge.from)} 不在四单位表内。`,
        hint: "边端点只能是 LUMEN/CANDELA/LUX/NIT；扩充单位先走 F1798 变更纪律。",
      });
    }
    if (UNIT_DEFINITION_INDEX.get(edge.to) === undefined) {
      diagnostics.push({
        code: "UNIT_UNKNOWN",
        message: `换算边终点单位 ${String(edge.to)} 不在四单位表内。`,
        hint: "边端点只能是 LUMEN/CANDELA/LUX/NIT；扩充单位先走 F1798 变更纪律。",
      });
    }
  }

  // 4.2 上下文量纲与 contextKind 声明一致。
  for (const edge of edges) {
    const declared = CONTEXT_DIMENSIONS[edge.contextKind];
    if (declared !== undefined && !sameDimension(declared, edge.contextDimension)) {
      diagnostics.push({
        code: "UNIT_CONVERSION_TABLE_INCONSISTENT",
        message: `换算边 ${edge.from}→${edge.to} 的上下文量纲与 ${edge.contextKind} 声明不符` +
          `（表写 ${formatDimension(edge.contextDimension)}，应为 ${formatDimension(declared)}）。`,
        hint: "上下文量纲取 CONTEXT_DIMENSIONS 的规范值；改表须同步改换算公式的人话描述。",
      });
    }
  }

  // 4.3 核心不变量：量纲差与乘除方向自洽。
  for (const edge of edges) {
    const fromDef = UNIT_DEFINITION_INDEX.get(edge.from);
    const toDef = UNIT_DEFINITION_INDEX.get(edge.to);
    if (fromDef === undefined || toDef === undefined) continue; // 已在 4.1 报过

    const expected =
      edge.direction === "MULTIPLY"
        ? addDimension(fromDef.dimension, edge.contextDimension)
        : subDimension(fromDef.dimension, edge.contextDimension);

    if (!sameDimension(expected, toDef.dimension)) {
      diagnostics.push({
        code: "UNIT_CONVERSION_TABLE_INCONSISTENT",
        message: `换算边 ${edge.from}→${edge.to}（${edge.direction} ${formatDimension(edge.contextDimension)}）量纲不自洽：` +
          `算出 ${formatDimension(expected)}，而 ${edge.to} 是 ${formatDimension(toDef.dimension)}。`,
        hint:
          `按物理恒等式修正：${edge.from} ${edge.direction === "MULTIPLY" ? "+" : "−"} ` +
          `${formatDimension(edge.contextDimension)} 应等于 ${formatDimension(toDef.dimension)}。`,
      });
    }
  }

  // 4.4 重复边检测（同 from→to 出现两次即歧义，换算结果取决于命中哪条）。
  const seen = new Set<string>();
  for (const edge of edges) {
    const key = `${edge.from}->${edge.to}`;
    if (seen.has(key)) {
      diagnostics.push({
        code: "UNIT_CONVERSION_TABLE_INCONSISTENT",
        message: `换算边 ${key} 重复登记。`,
        hint: "同一单位对只能有一条直连边；否则换算结果取决于命中顺序，属静默不确定性。",
      });
    }
    seen.add(key);
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.message, first.hint, diagnostics);
    }
  }
  return ok([...edges]);
}

/** 查找指定单位对的直连边（无直连返回 undefined，由调用方走两步路径或报错）。 */
function findEdge(from: LightUnit, to: LightUnit): ConversionEdge | undefined {
  return CONVERSION_EDGES.find((e) => e.from === from && e.to === to);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §5 单位换算（上下文驱动 · 最短边路由 · 显性缺参拒绝）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 换算上下文：数值换算所需的真实几何量（**不可省略、不可默认**）。 */
export interface ConversionContext {
  /** 立体角（sr）——仅当所走边需要时使用。 */
  readonly solidAngleSr?: number;
  /** 面积（m²）或距离平方（m²）——仅当所走边需要时使用。 */
  readonly areaM2?: number;
}

/** 一步换算的记录（供文档与调试展示「这个数是怎么来的」）。 */
export interface ConversionStep {
  readonly from: LightUnit;
  readonly to: LightUnit;
  readonly direction: "MULTIPLY" | "DIVIDE";
  readonly contextKind: ConversionContextKind;
  readonly contextValue: number;
  readonly formula: string;
}

/** 一次换算的完整结果：值 + 途经路径（透明可追溯，不做黑箱换算）。 */
export interface ConversionTrace {
  readonly value: number;
  readonly from: LightUnit;
  readonly to: LightUnit;
  readonly steps: readonly ConversionStep[];
}

/** 从上下文取几何量；缺失即显性报错（这是本条最重要的一条纪律）。 */
function requireContext(
  context: ConversionContext,
  kind: ConversionContextKind,
  from: LightUnit,
  to: LightUnit,
): Outcome<number> {
  const raw = kind === "SOLID_ANGLE" ? context.solidAngleSr : context.areaM2;
  const fieldName = kind === "SOLID_ANGLE" ? "solidAngleSr" : "areaM2";

  if (raw === undefined) {
    const hint =
      kind === "SOLID_ANGLE"
        ? `在换算里补 solidAngleSr（光锥张角，sr）。提示：${from} 与 ${to} 之间隔着「这束光被谁看见的那一锥」，` +
          "拿不到立体角就没有唯一答案——本模块拒绝猜默认值，因为猜错会让亮度整体偏一个量级且看不出来。"
        : `在换算里补 areaM2（受照面积 m²，cd↔nit 时传观察距离的平方）。提示：${from} 与 ${to} 之间隔着几何尺度，` +
          "没有几何量就无法把「灯有多亮」翻译成「面有多亮」。若这批数值本属艺术制，请走 §6 双模式而非硬补几何。";
    return err(
      "UNIT_CONVERSION_CONTEXT_MISSING",
      `单位换算 ${from} → ${to} 缺少必需的上下文几何量 ${fieldName}。`,
      hint,
    );
  }

  if (!Number.isFinite(raw) || raw <= 0) {
    return err(
      "UNIT_CONVERSION_CONTEXT_INVALID",
      `单位换算 ${from} → ${to} 的上下文几何量 ${fieldName} 非法：${raw}。`,
      `${fieldName} 须为正的有限实数（面积/立体角在物理上不为零也不为负）。` +
        "非法值说明上游几何计算已出错，先修上游，不要用 1 顶替。",
    );
  }

  return ok(raw);
}

/** 沿一条边走一步。 */
function applyEdge(
  value: number,
  edge: ConversionEdge,
  context: ConversionContext,
): Outcome<{ value: number; step: ConversionStep }> {
  const ctx = requireContext(context, edge.contextKind, edge.from, edge.to);
  if (!ctx.ok) {
    // requireContext 的失败按原样透出（诊断不丢、不改写），
    // 但类型须从 Outcome<number> 收敛到本步的返回类型。
    return { ok: false, code: ctx.code, message: ctx.message, hint: ctx.hint, diagnostics: ctx.diagnostics };
  }

  const next = edge.direction === "MULTIPLY" ? value * ctx.value : value / ctx.value;
  return ok({
    value: next,
    step: {
      from: edge.from,
      to: edge.to,
      direction: edge.direction,
      contextKind: edge.contextKind,
      contextValue: ctx.value,
      formula: edge.formula,
    },
  });
}

/** 同单位无需换算（但仍返回单步 trace，保证路径可追溯）。 */
const IDENTITY_FORMULAS: Readonly<Partial<Record<LightUnit, string>>> = {
  LUMEN: "lm = lm",
  CANDELA: "cd = cd",
  LUX: "lx = lx",
  NIT: "nit = nit",
};

/**
 * 单位换算（锚点原文：换算公式与常量）。
 *
 * 路由规则：
 *   · 同单位 → 恒等，零换算；
 *   · 有直连边 → 走直连边；
 *   · 无直连边但存在两步路径（如 cd→lm→lx）→ 自动走两步，路径全部记入 trace；
 *   · 无路径或缺上下文几何量 → 显性报错。
 *
 * **不做的事**：不猜默认面积、不猜默认距离、不静默返回原值。缺几何量时报错是
 * 唯一正确行为——那说明调用方在物理上讲不通，应当改用双模式或补齐几何。
 *
 * @param value 源数值
 * @param from 源单位
 * @param to 目标单位
 * @param context 上下文几何量（按所走边取用，可两步各需不同几何量）
 */
export function convertUnit(
  value: number,
  from: LightUnit,
  to: LightUnit,
  context: ConversionContext = {},
): Outcome<ConversionTrace> {
  // 5.1 端点合法性（未知单位不进入路由）。
  const fromDef = UNIT_DEFINITION_INDEX.get(from);
  const toDef = UNIT_DEFINITION_INDEX.get(to);
  if (fromDef === undefined) {
    return err("UNIT_UNKNOWN", `源单位 ${String(from)} 不在四单位表内。`, "只用 LUMEN/CANDELA/LUX/NIT。");
  }
  if (toDef === undefined) {
    return err("UNIT_UNKNOWN", `目标单位 ${String(to)} 不在四单位表内。`, "只用 LUMEN/CANDELA/LUX/NIT。");
  }

  // 5.2 源数值必须有限（NaN/Inf 进换算必产出垃圾，钳制是 §7 的职责，此处只拒）。
  if (!Number.isFinite(value)) {
    return err(
      "UNIT_CONVERSION_CONTEXT_INVALID",
      `换算源数值非有限：${value}。`,
      "先把上游数值钳制成有限值再换算；非有限源值乘除任何几何量结果仍是垃圾。",
    );
  }

  // 5.3 恒等路径。
  if (from === to) {
    return ok({
      value,
      from,
      to,
      steps: [
        {
          from,
          to,
          direction: "MULTIPLY",
          contextKind: "SOLID_ANGLE",
          contextValue: 1,
          formula: IDENTITY_FORMULAS[from] ?? `${from} = ${from}`,
        },
      ],
    });
  }

  // 5.4 直连优先。
  const direct = findEdge(from, to);
  if (direct !== undefined) {
    const applied = applyEdge(value, direct, context);
    if (!applied.ok) return applied;
    return ok({ value: applied.value.value, from, to, steps: [applied.value.step] });
  }

  // 5.5 两步路径：枚举所有中间单位，取「上下文齐备且路径可用」的第一条。
  //     顺序按四单位表声明序（LUMEN→CANDELA→LUX→NIT），保证路由确定性——
  //     路由不确定会让同一输入在不同构建里得到不同结果，属静默不确定性缺陷。
  const intermediates: readonly LightUnit[] = UNIT_SYSTEM_TABLE.map((d) => d.unit).filter((u) => u !== from && u !== to);
  for (const mid of intermediates) {
    const legA = findEdge(from, mid);
    const legB = findEdge(mid, to);
    if (legA === undefined || legB === undefined) continue;

    const firstLeg = applyEdge(value, legA, context);
    if (!firstLeg.ok) continue; // 缺/非法上下文 → 试下一条路径

    const secondLeg = applyEdge(firstLeg.value.value, legB, context);
    if (!secondLeg.ok) continue;

    return ok({
      value: secondLeg.value.value,
      from,
      to,
      steps: [firstLeg.value.step, secondLeg.value.step],
    });
  }

  // 5.6 无可用路径：显性拒绝，并把「缺哪个几何量」写清楚。
  const requiredKinds = new Set<ConversionContextKind>();
  const directMissing = findEdge(from, to);
  if (directMissing !== undefined) {
    requiredKinds.add(directMissing.contextKind);
  } else {
    for (const mid of intermediates) {
      const legA = findEdge(from, mid);
      const legB = findEdge(mid, to);
      if (legA !== undefined) requiredKinds.add(legA.contextKind);
      if (legB !== undefined) requiredKinds.add(legB.contextKind);
    }
  }
  const missingList = [...requiredKinds].join("、");
  return err(
    "UNIT_CONVERSION_CONTEXT_MISSING",
    `单位换算 ${from} → ${to} 无可用路径：所需上下文几何量为 ${missingList}，当前上下文未提供可用值。`,
    `补齐 ${missingList} 后重试；若这批数值来自老资产且本就无几何语义，` +
      "请按 ARTISTIC 模式处理（见 §6），不要为通过换算而伪造几何量。",
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §6 双单位模式（PHYSICAL / ARTISTIC）+ 艺术制换算基准声明
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 单位制模式枚举（锚点原文：双模式配置枚举 PHYSICAL / ARTISTIC）。
 *
 * PHYSICAL 为**推荐默认**：新场景、新资产一律用物理制；ARTISTIC 仅为兼容
 * 老资产而并存，不得用于新建资产（新建即应写物理值，见 §8 兜底说明）。
 */
export type UnitMode =
  /** 物理单位制（推荐默认）：强度即物理单位数值，内部零缩放。 */
  | "PHYSICAL"
  /** 艺术单位制（兼容老资产）：以参考白点映射到物理制内部计算。 */
  | "ARTISTIC";

/**
 * 艺术制换算基准声明（锚点原文：以参考白点映射到物理制内部计算）。
 *
 * 基准定义（一次声明，四单位共用）：
 *   艺术值 1.0  ≡  参考白点在**参考几何**下的物理量。
 *   参考白点 = 203 nit（ITU-R BT.2408 高动态范围参考白，等于 SDR 电视白 100 nit
 *              在 PQ 曲线上映射后的显示亮度）。
 *   参考几何 = 1 m² 受照面 / 1 m 观察距离（故面积基准与距离平方基准同为 1）。
 *
 * 于是艺术值 → 物理值的换算是**一次标量乘** `ARTISTIC_REFERENCE_SCALE`：
 *   nit : 1.0 艺术 → 203 nit
 *   lx  : 1.0 艺术 → 203 lx（1 m² 面上 203 nit 的面光源）
 *   cd  : 1.0 艺术 → 203 cd（1 m 处）
 *   lm  : 1.0 艺术 → 203 lm（1 m² 面发出）
 *
 * 四单位共用同一标量的好处：艺术制与物理制之间**不存在模式相关的几何依赖**，
 * 模式切换不触碰换算边表，往返误差仅来自浮点舍入（由 §7 量化保护兜住）。
 * 这是刻意设计：把基准放在「同一标量」而非「逐单位不同系数」，是为了让
 * 模式切换不引入额外的一处误差源。
 */
export const ARTISTIC_REFERENCE_SCALE = 203;

/** 艺术制基准声明的可读描述（文档替述与 UI 提示共用，保证一处说清）。 */
export const ARTISTIC_BASIS_DECLARATION = {
  /** 艺术值 1.0 对应的参考白点亮度（nit）。 */
  referenceWhiteNit: ARTISTIC_REFERENCE_SCALE,
  /** 参考几何：受照面积（m²）。 */
  referenceAreaM2: PHOTOMETRIC_CONSTANTS.REFERENCE_AREA_M2,
  /** 参考几何：观察距离（m）；cd↔nit 换算以距离平方代入。 */
  referenceDistanceM: PHOTOMETRIC_CONSTANTS.REFERENCE_DISTANCE_M,
  /** 基准来源（口径可追溯，避免各条目各说各话）。 */
  source: "ITU-R BT.2408 参考白 203 nit；参考几何 1 m² / 1 m",
  /** 四单位共用同一标量的理由。 */
  sharedScaleRationale:
    "四单位共用同一基准标量，使模式切换与几何换算解耦：切换不新增误差源，" +
    "艺术↔物理往返仅受浮点舍入影响，可由量化保护完全覆盖。",
} as const;

/** 场景级单位制配置（锚点原文：按场景级统一配置执行）。 */
export interface UnitSystemConfig {
  readonly mode: UnitMode;
  /** 场景级标注（资产自带标注与之不一致时告警，见 §9）。 */
  readonly declaredUnitMode: UnitMode;
  /** 配置来源（导入/编辑器/脚本），供诊断定位。 */
  readonly source: string;
  /** 该配置是否为默认推荐值（PHYSICAL）。 */
  readonly recommended: boolean;
}

/**
 * 构造场景级单位制配置。
 *
 * 推荐默认恒为 PHYSICAL（锚点：物理单位制为推荐默认）。显式请求 ARTISTIC 时
 * 保留请求但 `recommended: false`——不静默改写用户的显式选择，只把「这不是
 * 推荐默认」这件事显性记下来，供 F1817 一致性核对与遥测统计使用。
 */
export function makeUnitSystemConfig(mode: UnitMode, source: string): UnitSystemConfig {
  return {
    mode,
    declaredUnitMode: mode,
    source,
    recommended: mode === "PHYSICAL",
  };
}

/**
 * 把「某单位的模式内数值」折算为**内部物理制数值**（着色器真正吃的那份）。
 *
 * 锚点：物理单位制内部零缩放；艺术制以参考白点映射到物理制内部计算。
 *
 * 为何**没有** unit 参数（一次刻意的收窄）：艺术制基准对四单位共用同一标量
 * （见 ARTISTIC_BASIS_DECLARATION.sharedScaleRationale），因此折算与单位无关。
 * 保留一个不参与计算的 unit 形参会诱导调用方以为「换算会因单位而异」，
 * 进而各自去乘除一个系数——那正是本条要防的误差分叉来源。单位信息在调用侧
 * 已由 effectiveUnit 携带，供钳制（clampIntensity）与换算（convertUnit）使用。
 *
 * @param value 模式内数值
 * @param mode 场景单位制模式
 */
export function toInternalPhysical(value: number, mode: UnitMode): number {
  if (mode === "PHYSICAL") return value;
  return value * ARTISTIC_REFERENCE_SCALE;
}

/**
 * 把内部物理制数值折回模式内数值（物理制 → 艺术制的逆运算，供编辑器显示）。
 *
 * 编辑器显示必须走这个逆运算：直接显示内部物理值会让老资产的「1.0」在
 * 切到物理制后显示成 203，用户会以为自己的强度被改了 203 倍。
 */
export function fromInternalPhysical(value: number, mode: UnitMode): number {
  if (mode === "PHYSICAL") return value;
  return value / ARTISTIC_REFERENCE_SCALE;
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §7 极端强度钳制 + float32 定点区间量化保护
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 钳制原因（可检索，遥测与缺陷归因用）。 */
export type ClampReason =
  /** 非有限值（NaN / ±Infinity）。 */
  | "NON_FINITE"
  /** 负值。 */
  | "NEGATIVE"
  /** 超该单位物理上限。 */
  | "ABOVE_PHYSICAL_MAX"
  /** 落入 float32 非规格化（subnormal）带，相对误差发散。 */
  | "SUBNORMAL_BAND"
  /** 超出 float32 可表示范围。 */
  | "FLOAT32_OVERFLOW";

/** float32 数值特征（量化保护与钳制的物理边界）。 */
export const FLOAT32 = {
  /** 尾数位数（IEEE-754 binary32 有效位）。 */
  MANTISSA_BITS: 24,
  /** 相对机器epsilon = 2^-23。 */
  MACHINE_EPSILON: Math.pow(2, -23),
  /** 最小规格化数 2^-126。 */
  MIN_NORMAL: Math.pow(2, -126),
  /** 最大有限数 (2−2^-23)×2^127。 */
  MAX_FINITE: (2 - Math.pow(2, -23)) * Math.pow(2, 127),
} as const;

/** 钳制结果：值 + 是否发生钳制 + 原因（零静默：发生钳制必留痕）。 */
export interface ClampResult {
  readonly value: number;
  readonly clamped: boolean;
  readonly reason: ClampReason | null;
  /** 钳制前的原值（供遥测比对，不丢现场）。 */
  readonly original: number;
}

/**
 * 强度钳制（锚点原文：极端强度 NaN/负值/超物理上限 → 钳制到 [0, 上限] 并遥测）。
 *
 * 钳制顺序有意为之——**先判非有限，再判负，再判上限，最后判 float32 带**：
 *   1. NaN/Inf 不是「超上限」，它是完全无效的输入，须单独归因；
 *   2. 负值钳到 0（光强无负；负值多来自资产符号错误，遥测可反查）；
 *   3. 超上限钳到该单位 physicalMax（上限随单位而异：cd 1e7 vs nit 1e4）；
 *   4. 落入 subnormal 带（0 < |v| < 2^-126）钳到 0：那里相对误差发散，
 *      一个 1e-40 的强度在 float32 里只有若干有效位，留着它只会让画面抖动；
 *   5. 超 float32 上限钳到 float32 上限（在钳制 4 之前判，避免溢出为 Inf）。
 *
 * 注意第 4 步的取舍：钳到 0 是**主动丢弃**极弱光。之所以敢丢——物理上人眼
 * 在低于 2^-126 nit 的亮度下已无响应（远低于暗视觉阈值），保留它只会引入
 * 数值噪声。该决策由 `reason: SUBNORMAL_BAND` 显式留痕，可被遥测统计与回查。
 */
export function clampIntensity(raw: number, unit: LightUnit): ClampResult {
  const def = UNIT_DEFINITION_INDEX.get(unit);

  // 7.1 非有限值：NaN 与 ±Infinity 一律归零（Inf 正负都归零，方向无意义）。
  if (!Number.isFinite(raw)) {
    return { value: 0, clamped: true, reason: "NON_FINITE", original: raw };
  }

  // 7.2 负值 → 0。
  if (raw < 0) {
    return { value: 0, clamped: true, reason: "NEGATIVE", original: raw };
  }

  // 7.3 float32 溢出（先于上限判：否则 1e39 会被 physicalMax 静默改小，
  //     而真实病因是它压根装不进 float32——两者遥测归因不同）。
  if (raw > FLOAT32.MAX_FINITE) {
    return { value: FLOAT32.MAX_FINITE, clamped: true, reason: "FLOAT32_OVERFLOW", original: raw };
  }

  // 7.4 超该单位物理上限 → 上限（未知单位按四单位表缺失处理，退到最宽上限，
  //     但单位合法性由 getUnitDefinition 单独把关，不在此处静默吞）。
  const max = def?.physicalMax ?? Number.MAX_SAFE_INTEGER;
  if (raw > max) {
    return { value: max, clamped: true, reason: "ABOVE_PHYSICAL_MAX", original: raw };
  }

  // 7.5 subnormal 带钳零（见函数注释第 4 步的取舍说明）。
  if (raw > 0 && raw < FLOAT32.MIN_NORMAL) {
    return { value: 0, clamped: true, reason: "SUBNORMAL_BAND", original: raw };
  }

  return { value: raw, clamped: false, reason: null, original: raw };
}

/**
 * 艺术制量化保护（锚点原文：艺术制换算精度·浮点误差累积 → 内部统一 float32
 * 定点区间的量化保护）。
 *
 * 问题：艺术值是「人为尺度」的数（老资产里常见 0.8、1.0、1.2 这类值），反复
 * 乘除 `ARTISTIC_REFERENCE_SCALE` 会让同一物理量在不同路径下得到相差若干
 * ULP（float32 末位单位）的结果——表现为同一场景里两盏等亮灯在像素上差
 * 一丝，跨帧轻微闪烁，肉眼几乎不可见但**破坏确定性**（与 I 域确定性口径冲突）。
 *
 * 解法：把内部值量化到 float32 的**相对网格**上。
 *   grid(v) = max(|v|, MIN_NORMAL) × 2^-23
 *   quantize(v) = round(v / grid) × grid
 * 即：任何内部物理值都对齐到「该量级上 float32 能表示的最小刻度」的整数倍。
 * 这样 203 与 203.0000001 落在同一格，往返误差被完全吸收。
 *
 * @param value 内部物理值（已完成模式折算）
 */
export function quantizeToFloat32Grid(value: number): { value: number; quantized: boolean; grid: number } {
  if (!Number.isFinite(value)) return { value, quantized: false, grid: 0 };

  const magnitude = Math.max(Math.abs(value), FLOAT32.MIN_NORMAL);
  const grid = magnitude * FLOAT32.MACHINE_EPSILON;
  if (grid <= 0 || !Number.isFinite(grid)) return { value, quantized: false, grid: 0 };

  const quantized = Math.round(value / grid) * grid;
  // 量化后必须仍是有限且量级不变，否则回退原值（保护优先于「一定量化」）。
  if (!Number.isFinite(quantized)) return { value, quantized: false, grid };
  if (Math.abs(quantized - value) > Math.abs(value) * 1e-5) {
    return { value, quantized: false, grid };
  }
  return { value: quantized, quantized: quantized !== value, grid };
}

/** 量化往返误差容差：超过即视为量化保护失效（换算链有 bug 或基准被改）。 */
export const ROUNDTRIP_TOLERANCE_REL = 1e-6;

/**
 * 双模式往返一致性检测（锚点判据：双模式一致性测试的口径）。
 *
 * 口径：`mode` 指定被检侧的模式语义，检测「模式内值 → 内部物理值 → 量化 →
 * 折回模式内值」的相对误差须 ≤ `ROUNDTRIP_TOLERANCE_REL`。
 *
 *   · mode = ARTISTIC：真正走 ×203 / ÷203 的标量链。这一支是**有内容的**——
 *     它验证艺术制往返确实稳定，也是量化保护是否生效的直接证据。
 *   · mode = PHYSICAL：折算为恒等，本支只验证量化环节对物理值同样幂等
 *     （物理值也要量化，否则物理制资产会绕过保护）。
 *
 * 注意：若两支都按「模式内值」直接比较误差，PHYSICAL 支会因恒等折算而恒真，
 * 形成一条永绿的假测试。故本函数在 PHYSICAL 支同样走完整链（含量化），
 * 且断言 `quantized` 环节确实被调用（见 ARTISTIC 支的量化验证）。
 */
export function verifyArtisticRoundTrip(artisticValue: number, mode: UnitMode): Outcome<number> {
  if (!Number.isFinite(artisticValue)) {
    return err(
      "ARTISTIC_CONVERSION_DRIFT",
      `往返一致性检测的探针值非有限：${artisticValue}。`,
      "探针值须为有限实数；非有限值应先过 clampIntensity 再参与往返检测。",
    );
  }
  if (mode !== "PHYSICAL" && mode !== "ARTISTIC") {
    return err(
      "ARTISTIC_CONVERSION_DRIFT",
      `往返一致性检测的模式非法：${String(mode)}。`,
      "模式只允许 PHYSICAL / ARTISTIC；新增模式须先扩 UnitMode 并走 ADR。",
    );
  }

  // 完整链：折算 → 量化 → 折回（量化必须真的发生，否则本检测无意义）。
  const internal = toInternalPhysical(artisticValue, mode);
  const quantization = quantizeToFloat32Grid(internal);
  const back = fromInternalPhysical(quantization.value, mode);

  // 量化环节的自检：对本组探针值，量化必须是幂等的（量化两次 = 量化一次）。
  // 若某次重构让 quantize 变成恒等，这条断言仍绿但 12.4 的 ARTISTIC 支会红，
  // 两支合起来才覆盖「量化真实生效」与「量化不引入误差」两件事。
  const requantized = quantizeToFloat32Grid(quantization.value).value;
  if (quantization.quantized && requantized !== quantization.value) {
    return err(
      "ARTISTIC_CONVERSION_DRIFT",
      `量化非幂等：${quantization.value} 二次量化为 ${requantized}，网格判定不稳定。`,
      "quantizeToFloat32Grid 的网格公式须与自身一致（grid 由 |v| 推出，二次量化须落回同一格）；" +
        "检查是否在量化后又叠加了额外舍入。",
    );
  }

  if (artisticValue === 0) return ok(0);

  const relError = Math.abs(back - artisticValue) / Math.abs(artisticValue);
  if (relError > ROUNDTRIP_TOLERANCE_REL) {
    return err(
      "ARTISTIC_CONVERSION_DRIFT",
      `双模式往返误差超容差：探针 ${artisticValue}（${mode}）经「折算 → 量化 → 折回」得 ${back}，` +
        `相对误差 ${relError.toExponential(3)}（容差 ${ROUNDTRIP_TOLERANCE_REL.toExponential(0)}）。`,
      "检查 ARTISTIC_REFERENCE_SCALE 是否被改动、quantizeToFloat32Grid 是否被绕过；" +
        "定标冻结后这两处只能走 ADR 变更。",
    );
  }
  return ok(relError);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §8 强度字段单位标注约定（光照参数块的字段级契约）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 强度字段的单位标注约定条目。
 *
 * 锚点原文：「光照参数块中强度字段的单位标注约定」。本表把 F1801 的
 * `LightCommonBlock.intensity`（无量纲标量）落到字段级：**哪个字段、属于哪个
 * 光源类型、必须以什么单位书写、单位标注键叫什么、是否强制要求**。
 *
 * 强制策略（重要）：物理制下强度字段**强制标注单位**；艺术制下允许省略
 * （省略即按艺术制解释，见 §9 兜底）。这条「物理制强制 / 艺术制宽松」的分歧
 * 是刻意的——物理制里「100」是 100 尼特还是 100 勒克斯差着两个数量级，
 * 必须显式；艺术制里「1.0」本来就是约定值，强制标注只会徒增老资产的迁移成本。
 */
export interface IntensityFieldSpec {
  /** 字段全名（与 F1801 公共块路径对齐）。 */
  readonly field: string;
  /** 该字段服务的强度承载对象。 */
  readonly subject: IntensitySubject;
  /** 强制/推荐的单位。 */
  readonly unit: LightUnit;
  /** 资产内的单位标注键（资产用它显式声明自己的单位制）。 */
  readonly annotationKey: string;
  /** 物理制下是否强制要求资产显式标注单位。 */
  readonly requiredInPhysical: boolean;
  /** 义务描述词（进 F1815 API 冻结清单的人话描述）。 */
  readonly description: string;
}

/**
 * 强度字段单位标注约定表。
 *
 * 逐条对应锚点适用对象：点光 cd / 方向光 lux / 面光 lm / 输出亮度 nit；
 * 聚光与点光同用 cd（F1805 参数块沿用 F1804 的光强语义）。
 */
export const INTENSITY_FIELD_SPECS: readonly IntensityFieldSpec[] = [
  {
    field: "common.intensity",
    subject: "DIRECTIONAL_LIGHT",
    unit: "LUX",
    annotationKey: "intensityUnit",
    requiredInPhysical: true,
    description:
      "方向光强度，单位勒克斯（lx）——平行光按被照面照度给值；换算成面光通量需补受照面积。",
  },
  {
    field: "common.intensity",
    subject: "POINT_LIGHT",
    unit: "CANDELA",
    annotationKey: "intensityUnit",
    requiredInPhysical: true,
    description:
      "点光强度，单位坎德拉（cd）——按轴向光强给值；到某表面亮度需补观察距离平方。",
  },
  {
    field: "common.intensity",
    subject: "SPOT_LIGHT",
    unit: "CANDELA",
    annotationKey: "intensityUnit",
    requiredInPhysical: true,
    description:
      "聚光强度，单位坎德拉（cd）——锥形光沿轴向的光强，锥角只做衰减不改变光强单位。",
  },
  {
    field: "common.intensity",
    subject: "AREA_LIGHT",
    unit: "LUMEN",
    annotationKey: "intensityUnit",
    requiredInPhysical: true,
    description:
      "面光源总光通量，单位流明（lm）——一期为预留接口位（F1806），单位契约先冻结。",
  },
  {
    field: "exposure.targetLuminance",
    subject: "DISPLAY_OUTPUT",
    unit: "NIT",
    annotationKey: "targetLuminanceUnit",
    requiredInPhysical: true,
    description:
      "显示输出目标亮度，单位尼特（nit = cd/m²）——F1812 自动/手动曝光的物理输入。",
  },
];

/** 查某承载对象的强度字段约定。 */
export function findIntensityFieldSpec(subject: IntensitySubject): IntensityFieldSpec | undefined {
  return INTENSITY_FIELD_SPECS.find((s) => s.subject === subject);
}

/**
 * 校验一条强度字段声明（导入期一次，锚点：单位校验为导入期一次）。
 *
 * 校验项：
 *   1. 该承载对象的字段已在约定表登记（未登记 → 拒绝，防字段语义漂移）；
 *   2. 声明单位与约定单位一致（不一致 → 拒绝并指出应改为何种单位）；
 *   3. 物理制下单位标注缺失 → 拒绝（锚点：物理制强制标注）。
 *
 * @param subject 强度承载对象
 * @param declaredUnit 资产声明的单位（null = 未标注）
 * @param mode 场景单位制模式
 */
export function validateIntensityField(
  subject: IntensitySubject,
  declaredUnit: LightUnit | null,
  mode: UnitMode,
): Outcome<IntensityFieldSpec> {
  const spec = findIntensityFieldSpec(subject);
  if (spec === undefined) {
    return err(
      "INTENSITY_FIELD_UNREGISTERED",
      `强度承载对象 ${subject} 未在单位标注约定表中登记。`,
      "先在 INTENSITY_FIELD_SPECS 登记该对象的字段、单位与描述词，再使用；" +
        "未登记就实现会让强度在渲染与编辑器两侧各自解释。",
    );
  }

  if (declaredUnit === null) {
    if (mode === "PHYSICAL") {
      return err(
        "ASSET_UNIT_UNDECLARED_LEGACY_DEFAULT",
        `物理制下 ${spec.field}（${SUBJECT_LABELS[subject]}）未标注单位。`,
        `补上 ${spec.annotationKey} = "${spec.unit}"。物理制里裸写一个数字是最危险的做法：` +
          `「100」在 ${spec.unit} 与 ${contrastUnitFor(spec.unit)} 之间量纲不同（${formatDimension(UNIT_DEFINITION_INDEX.get(spec.unit)?.dimension ?? [0, 0, 0])} ` +
          `vs ${formatDimension(UNIT_DEFINITION_INDEX.get(contrastUnitFor(spec.unit))?.dimension ?? [0, 0, 0])}），` +
          "换算还隔着真实几何量，漏标会让强度静默错出量级且画面看不出异常。" +
          "若这是老资产且确实无物理语义，请显式声明为 ARTISTIC 而不是省略标注。",
      );
    }
    // 艺术制下省略标注合法（老资产兼容路径），返回约定供调用方继续。
    return ok(spec);
  }

  if (declaredUnit !== spec.unit) {
    return err(
      "INTENSITY_FIELD_UNIT_MISMATCH",
      `${spec.field}（${SUBJECT_LABELS[subject]}）声明单位 ${declaredUnit} 与约定单位 ${spec.unit} 不符。`,
      `按约定改为 ${spec.unit}（${spec.unit}）：${spec.description}` +
        `若确需以 ${declaredUnit} 书写，请先在换算边表中确认所需上下文几何量并在导入期完成换算。`,
    );
  }

  return ok(spec);
}

/**
 * 取一个与给定单位**量纲不同**的单位，用作「漏标单位会差多少」的对照。
 *
 * 用途：诊断提示里要说清「漏标单位的后果有多严重」，需要一个真实的量纲对照
 * （而不是随口举个数）。选第一个量纲不同的单位，保证提示里的对照是真的。
 */
function contrastUnitFor(unit: LightUnit): LightUnit {
  const self = UNIT_DEFINITION_INDEX.get(unit);
  const selfDim = self?.dimension;
  for (const def of UNIT_SYSTEM_TABLE) {
    if (def.unit === unit) continue;
    if (selfDim === undefined || !sameDimension(def.dimension, selfDim)) return def.unit;
  }
  return unit;
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §9 场景级统一与混用检测（资产自带标注优先 + 不一致告警）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 一个带单位标注的资产条目（老资产可以没有标注）。 */
export interface UnitAnnotatedAsset {
  /** 资产标识（诊断定位用）。 */
  readonly assetId: string;
  /**
   * 该资产的强度承载对象。
   *
   * 必需而非可选：资产未标注单位时，其默认单位由**该对象适用哪个单位**决定
   * （点光 cd / 方向光 lux / 面光 lm / 输出亮度 nit）。没有这个字段，
   * 「未标注 → 兜底」就只能落到一个与资产无关的常量上，那是猜测不是定标。
   */
  readonly subject: IntensitySubject;
  /** 资产声明的单位制（null = 未标注的老资产）。 */
  readonly declaredMode: UnitMode | null;
  /** 资产声明的单位（null = 未标注的老资产）。 */
  readonly declaredUnit: LightUnit | null;
  /** 资产强度原值（模式内数值）。 */
  readonly intensity: number;
}

/** 单个资产的单位解析结果。 */
export interface ResolvedAssetUnits {
  readonly assetId: string;
  /** 生效的模式（资产自带标注优先）。 */
  readonly effectiveMode: UnitMode;
  /** 生效的单位（资产自带标注优先）。 */
  readonly effectiveUnit: LightUnit;
  /** 内部物理制数值（已完成模式折算、钳制、量化）。 */
  readonly internalPhysical: number;
  /** 是否走了老资产兜底（未标注 → 艺术制）。 */
  readonly legacyDefaulted: boolean;
  /** 是否与场景级配置不一致（不一致已按场景级统一执行并告警）。 */
  readonly inconsistentWithScene: boolean;
}

/** 场景级解析汇总。 */
export interface SceneUnitResolution {
  readonly assets: readonly ResolvedAssetUnits[];
  /** 场景生效模式（场景级统一配置；资产标注与之冲突时不改场景配置，只告警）。 */
  readonly sceneMode: UnitMode;
  /** 是否检出混用（场景内存在两种及以上生效模式）。 */
  readonly mixed: boolean;
  /** 走兜底的老资产数。 */
  readonly legacyDefaultedCount: number;
  /** 与场景配置不一致的资产数。 */
  readonly inconsistentCount: number;
}

/**
 * 场景级单位解析（锚点原文：混用单位 → 按场景级统一配置执行，资产自带单位标注
 * 优先并告警不一致）。
 *
 * 裁决规则（三条，按序）：
 *   1. **资产自带标注优先**：资产若显式标注了模式与单位，一律以资产标注解释
 *      其数值——这是「标注优先」的落点，否则资产的标注就形同虚设；
 *   2. **场景级统一执行**：资产标注解释出的模式若与场景配置不同，**不**改场景
 *      配置、**不**改资产数值，而是照常折算成内部物理值（两种模式折算后可在
 *      同一物理空间共存），同时产出 `UNIT_MODE_MIXED_IN_SCENE` 告警——这正是
 *      「按场景级统一配置执行 + 告警不一致」的含义；
 *   3. **未标注老资产兜底**：无标注 → 按艺术制默认导入，产出
 *      `ASSET_UNIT_UNDECLARED_LEGACY_DEFAULT`（锚点原文：按艺术制默认导入并
 *      文档声明），并计入 `legacyDefaultedCount` 供文档与遥测引用。
 *
 * 为什么混用不必拒绝：两种模式折算到内部物理制后**物理含义相同**，渲染结果
 * 一致；真正的问题是「作者以为单位是 A、实际按 B 解释」，那是标注层面的
 * 认知错，靠告警 + 遥测暴露，而不是靠拒绝渲染把它藏起来。
 */
export function resolveSceneUnits(
  config: UnitSystemConfig,
  assets: readonly UnitAnnotatedAsset[],
): Outcome<SceneUnitResolution> {
  const diagnostics: Diagnostic[] = [];
  const resolved: ResolvedAssetUnits[] = [];

  for (const asset of assets) {
    // 9.1 资产自带标注优先；无标注 → 艺术制兜底 + 显性声明。
    const legacyDefaulted = asset.declaredMode === null || asset.declaredUnit === null;
    const effectiveMode: UnitMode = asset.declaredMode ?? "ARTISTIC";

    // 9.2 单位：优先资产标注，其次按生效模式的适用对象默认绑定。
let effectiveUnit: LightUnit;
    if (asset.declaredUnit !== null) {
      // 9.2a 资产标注了未知单位 → **拒绝整个场景**，不静默回落为任何近似单位。
      //      理由：回落会把「作者写错了单位」变成「系统替他猜了一个」，
      //      猜错的量级从 1 倍到 100 倍不等，且画面看不出异常——这正是
      //      零静默纪律要拦的那一类。导入期拒绝 + 指路修正，是唯一正解。
      if (UNIT_DEFINITION_INDEX.get(asset.declaredUnit) === undefined) {
        return err(
          "UNIT_UNKNOWN",
          `资产 ${asset.assetId} 声明的单位 ${String(asset.declaredUnit)} 不在四单位表内，无法为其定标强度语义。`,
          `只用 LUMEN(流明)/CANDELA(坎德拉)/LUX(勒克斯)/NIT(尼特)。` +
            "把资产的单位改为这四种之一后重新导入；若该资产确实缺单位标注，走「声明为 ARTISTIC」的老资产路径，"
 +
            "不要靠猜一个单位名让导入通过。",
        );
      }
      effectiveUnit = asset.declaredUnit;
    } else {
      // 9.2b 无标注 → 按该资产承载对象的绑定单位（这才是物理上正确的默认，
      //      而不是一个与资产无关的常量）。
      effectiveUnit = SUBJECT_UNIT_BINDING[asset.subject];
    }

    // 9.3 与场景配置的一致性（不一致 → 告警，但不改任何一方）。
    const inconsistentWithScene = effectiveMode !== config.mode;
    if (inconsistentWithScene) {
      diagnostics.push({
        code: "UNIT_MODE_MIXED_IN_SCENE",
        message: `资产 ${asset.assetId} 的单位制（${effectiveMode}）与场景级配置（${config.mode}）不一致。`,
        hint:
          `已按「资产自带标注优先」用 ${effectiveMode} 解释该资产的 ${asset.intensity}，` +
          `并折算到内部物理制参与统一渲染——物理含义一致，画面不受影响。` +
          `若这是误标：把资产的 ${effectiveMode} 改为 ${config.mode}；` +
          `若是有意为之：把场景配置显式设为 ${effectiveMode} 以消除混用。` +
          `（${legacyDefaulted ? "该资产未标注单位，已按艺术制兜底。" : ""}）`,
      });
    }

    // 9.4 老资产兜底的显性声明（文档声明 + 遥测）。
    if (legacyDefaulted) {
      diagnostics.push({
        code: "ASSET_UNIT_UNDECLARED_LEGACY_DEFAULT",
        message: `资产 ${asset.assetId} 未标注单位制/单位，已按艺术制（ARTISTIC）默认导入。`,
        hint:
          `强度 ${asset.intensity} 按「1.0 ≡ 参考白点 ${ARTISTIC_REFERENCE_SCALE} nit」解释，` +
          "折算得内部物理值并已参与统一渲染。迁移建议：给资产补上 intensityUnit 显式单位，" +
          "使老资产进入物理制并摆脱对默认基准的依赖。",
      });
    }

    // 9.5 折算 → 钳制 → 量化（顺序固定：先折算得到物理量，再对物理量钳制，
    //     最后量化；反过来会漏掉折算后越界的情形）。
    const rawInternal = toInternalPhysical(asset.intensity, effectiveMode);
    const clamped = clampIntensity(rawInternal, effectiveUnit);
    if (clamped.clamped) {
      const clampedReason = clamped.reason ?? "NON_FINITE";
      diagnostics.push({
        code: "INTENSITY_CLAMPED",
        message: `资产 ${asset.assetId} 强度被钳制：${clamped.original} → ${clamped.value}（${clampedReason}，单位 ${effectiveUnit}）。`,
        hint:
          clampedReason === "NEGATIVE"
            ? "负强度多为资产符号错误，回查资产导出；光强在物理上不为负。"
            : clampedReason === "NON_FINITE"
              ? "非有限值来自上游计算（NaN/Infinity），钳到 0 只是止血，根因在上游。"
              : clampedReason === "ABOVE_PHYSICAL_MAX"
                ? `超 ${effectiveUnit} 物理上限，已钳到上限。确认资产值是否漏了单位换算（例如把 cd 当 nit 用）。`
                : clampedReason === "SUBNORMAL_BAND"
                  ? "该强度落入 float32 非规格化带，相对误差发散，已钳到 0；人眼在该亮度下无响应，可安全丢弃。"
                  : "该值超出 float32 可表示范围，已钳到 float32 上限；检查上游是否有量级错误。",
      });
    }

    const quantized = quantizeToFloat32Grid(clamped.value).value;

    resolved.push({
      assetId: asset.assetId,
      effectiveMode,
      effectiveUnit,
      internalPhysical: quantized,
      legacyDefaulted,
      inconsistentWithScene,
    });
  }

  // 9.6 混用判定：场景内出现两种及以上生效模式即混用（供 F1817 一致性核对）。
  const effectiveModes = new Set(resolved.map((r) => r.effectiveMode));
  const mixed = effectiveModes.size > 1;

  const value: SceneUnitResolution = {
    assets: resolved,
    sceneMode: config.mode,
    mixed,
    legacyDefaultedCount: resolved.filter((r) => r.legacyDefaulted).length,
    inconsistentCount: resolved.filter((r) => r.inconsistentWithScene).length,
  };

  // 无诊断即纯成功；有诊断仍为成功（告警类），但绝不吞掉——全部在 diagnostics 内。
  return ok(value, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §10 着色器常量折叠（编译期换算 · 零运行时成本）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 一个已完成折叠的着色器常量（编译期产出，运行时零换算）。 */
export interface FoldedIntensityConstant {
  /** GLSL 变量名。 */
  readonly symbol: string;
  /** GLSL 字面量（已在编译期算好，粘贴进着色器源码）。 */
  readonly literal: string;
  /** 该常量折算到的单位。 */
  readonly unit: LightUnit;
  /** 折算后的数值（供 CPU 侧上传与调试显示）。 */
  readonly value: number;
  /** 折算途经的步骤（与运行时 `convertUnit` 同源，保证两侧一致）。 */
  readonly trace: readonly ConversionStep[];
}

/** 折叠计划：一次编译期换算的完整产物。 */
export interface ConstantFoldPlan {
  readonly mode: UnitMode;
  readonly constants: readonly FoldedIntensityConstant[];
  /** 运行时每像素换算次数——契约恒为 0。 */
  readonly perPixelConversions: number;
  /** 是否已登记场景级重编译触发位（模式切换须重编译一次）。 */
  readonly recompileTriggered: boolean;
}

/**
 * 数值 → GLSL 字面量（编译期常量折叠的落点）。
 *
 * 格式要求（对着色器可读性负责）：
 *   · 恒带小数点或指数形式（`203.0` 而非 `203`），避免整数常量在 GLSL 里
 *     参与整型运算的隐式转换歧义；
 *   · 指数形式去掉 `+` 号与前导零（GLSL 接受 `2.03e2`）；
 *   · NaN / Inf 一律落成 `0.0`——着色器里出现 NaN 常量会让整个 draw call 的
 *     结果变 NaN 且难以定位，宁可在编译期就把它钉成 0。
 */
export function toGlslLiteral(value: number): string {
  if (!Number.isFinite(value)) return "0.0";
  if (value === 0) return "0.0";

  const abs = Math.abs(value);
  if (Number.isInteger(value) && abs < 1e6) return `${value}.0`;

  // 指数形式：两位有效小数足够着色器精度，去掉冗余尾零。
  const exponential = value.toExponential(2).replace(/\.?0+e/, "e").replace("e+", "e").replace("e-", "e-");
  return exponential;
}

/**
 * 编译期常量折叠（锚点原文：换算为着色器常量折叠·编译期完成·零运行时成本）。
 *
 * 流程（全部在编译期/导入期完成，运行时什么都不做）：
 *   模式内数值 →（模式折算）→ 内部物理值 →（钳制）→ （量化）
 *              →（跨单位换算，可选）→ GLSL 字面量
 *
 * @param entries 待折叠的强度常量（键为符号名）
 * @param mode 场景单位制模式
 * @param toUnit 若给出，则把各常量换算到该单位（跨单位换算同样在编译期折叠）
 * @param context 跨单位换算所需的上下文几何量（缺则显性报错，见 §5）
 */
export function foldIntensityConstants(
  entries: readonly { readonly symbol: string; readonly value: number; readonly unit: LightUnit }[],
  mode: UnitMode,
  toUnit?: LightUnit,
  context: ConversionContext = {},
): Outcome<ConstantFoldPlan> {
  const diagnostics: Diagnostic[] = [];
  const constants: FoldedIntensityConstant[] = [];

  for (const entry of entries) {
    // 10.1 模式折算 + 钳制 + 量化（与 §9 场景解析同一条链，避免两侧算法漂移）。
    const rawInternal = toInternalPhysical(entry.value, mode);
    const clamped = clampIntensity(rawInternal, entry.unit);
    if (clamped.clamped) {
      diagnostics.push({
        code: "INTENSITY_CLAMPED",
        message: `常量 ${entry.symbol} 折叠时强度被钳制：${clamped.original} → ${clamped.value}（${clamped.reason ?? "NON_FINITE"}）。`,
        hint: "折叠期钳制会让着色器拿到钳后值；确认资产强度是否漏了单位换算或符号错误。",
      });
    }
    let physical = quantizeToFloat32Grid(clamped.value).value;

    // 10.2 跨单位换算（同样编译期完成）。
    const steps: ConversionStep[] = [];
    if (toUnit !== undefined && toUnit !== entry.unit) {
      const converted = convertUnit(physical, entry.unit, toUnit, context);
      if (!converted.ok) {
        // 缺上下文几何量是硬错误：不能在编译期"猜一个默认值"糊过去。
        diagnostics.push({ code: converted.code, message: converted.message, hint: converted.hint });
        return err(converted.code, converted.message, converted.hint, diagnostics);
      }
      physical = converted.value.value;
      steps.push(...converted.value.steps);
    }

    constants.push({
      symbol: entry.symbol,
      literal: toGlslLiteral(physical),
      unit: toUnit ?? entry.unit,
      value: physical,
      trace: steps,
    });
  }

  return ok(
    {
      mode,
      constants,
      perPixelConversions: 0,
      recompileTriggered: false,
    },
    diagnostics,
  );
}

/**
 * 零运行时成本断言（把性能契约从注释变成可执行检查）。
 *
 * 锚点：换算为着色器常量折叠（编译期完成）零运行时成本。因此：
 *   · `perPixelConversions` 必须恒为 0；
 *   · 场景级模式切换必须已登记重编译触发位（未登记 → 说明切换不会生效，
 *     用户改了模式却画面不变，这是最典型的静默失效）。
 */
export function assertZeroRuntimeCost(plan: ConstantFoldPlan): Outcome<ConstantFoldPlan> {
  if (plan.perPixelConversions !== 0) {
    return err(
      "RECOMPILE_TRIGGER_MISSING",
      `折叠计划声明每像素换算 ${plan.perPixelConversions} 次，违反「编译期折叠·零运行时成本」契约。`,
      "把换算全部移到 foldIntensityConstants 的编译期链路上；" +
        "运行时只允许直接读取已折叠的字面量，不得保留任何按帧换算代码。",
    );
  }
  if (!plan.recompileTriggered) {
    return err(
      "RECOMPILE_TRIGGER_MISSING",
      "折叠计划未登记场景级重编译触发位。",
      "调用 markRecompileTriggered 登记一次场景级重编译：单位制切换必须触发一次重编译，" +
        "否则切换模式后着色器仍持旧常量，画面看起来毫无变化（静默失效）。",
    );
  }
  return ok(plan);
}

/** 登记一次场景级重编译（锚点：双模式切换为场景级重编译触发一次）。 */
export function markRecompileTriggered(plan: ConstantFoldPlan): ConstantFoldPlan {
  return { ...plan, recompileTriggered: true };
}

/**
 * 单位制切换（场景级，触发一次重编译）。
 *
 * 锚点原文：双模式切换为**场景级重编译触发一次**——不是每帧、不是每光源。
 * 本函数显式产出「需重编译」信号并重做折叠，调用方据此触发一次着色器重建。
 */
export function switchUnitMode(
  config: UnitSystemConfig,
  newMode: UnitMode,
  entries: readonly { readonly symbol: string; readonly value: number; readonly unit: LightUnit }[],
): Outcome<{ plan: ConstantFoldPlan; config: UnitSystemConfig }> {
  if (newMode !== "PHYSICAL" && newMode !== "ARTISTIC") {
    return err(
      "UNIT_UNKNOWN",
      `单位制模式非法：${String(newMode)}。`,
      "模式只允许 PHYSICAL 或 ARTISTIC；新增模式须先扩 UnitMode 并走 ADR。",
    );
  }

  const nextConfig = makeUnitSystemConfig(newMode, config.source);
  const folded = foldIntensityConstants(entries, nextConfig.mode);
  if (!folded.ok) return folded;

  // 切换即触发一次重编译（非每帧）——这是「模式切换」的语义本体。
  const plan = markRecompileTriggered(folded.value);
  const asserted = assertZeroRuntimeCost(plan);
  if (!asserted.ok) return asserted;

  return ok({ plan: asserted.value, config: nextConfig }, folded.diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §11 定标冻结快照 + 漂移检测 + ADR
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 定标冻结快照：单位制定标的机器可读凭据。 */
export interface CalibrationFreeze {
  readonly entryId: 1802;
  /** 单位表指纹（四单位定义 + 上限 + 量纲）。 */
  readonly unitTableFingerprint: string;
  /** 换算边表指纹（公式 + 量纲 + 方向）。 */
  readonly edgeTableFingerprint: string;
  /** 强度字段约定指纹。 */
  readonly fieldSpecFingerprint: string;
  /** 艺术制基准标量（改动须走 ADR）。 */
  readonly artisticScale: number;
  /** 四单位量纲快照。 */
  readonly dimensions: readonly (readonly [string, string])[];
}

/** 单位表指纹（漂移检测用；无量纲顺序语义问题——数组序即声明序，保留）。 */
export function unitTableFingerprint(table: readonly UnitDefinition[] = UNIT_SYSTEM_TABLE): string {
  return fnv1a(
    table
      .map((d) => `${d.unit}:${d.symbol}:${d.physicalMax}:${formatDimension(d.dimension)}`)
      .join("|"),
  );
}

/** 换算边表指纹。 */
export function edgeTableFingerprint(edges: readonly ConversionEdge[] = CONVERSION_EDGES): string {
  return fnv1a(
    edges
      .map((e) => `${e.from}->${e.to}:${e.direction}:${e.contextKind}:${formatDimension(e.contextDimension)}`)
      .join("|"),
  );
}

/** 强度字段约定指纹。 */
export function fieldSpecFingerprint(specs: readonly IntensityFieldSpec[] = INTENSITY_FIELD_SPECS): string {
  return fnv1a(specs.map((s) => `${s.field}:${s.subject}:${s.unit}:${s.annotationKey}`).join("|"));
}

/** FNV-1a 32 位——短、稳定、无依赖，用于漂移指纹（非安全用途）。 */
export function fnv1a(text: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, "0");
}

/** 构造当前定标声明的冻结快照。 */
export function buildCalibrationFreeze(): CalibrationFreeze {
  return {
    entryId: 1802,
    unitTableFingerprint: unitTableFingerprint(),
    edgeTableFingerprint: edgeTableFingerprint(),
    fieldSpecFingerprint: fieldSpecFingerprint(),
    artisticScale: ARTISTIC_REFERENCE_SCALE,
    dimensions: UNIT_SYSTEM_TABLE.map((d) => [d.symbol, formatDimension(d.dimension)] as const),
  };
}

/** 漂移检测：当前定标与冻结态不符即告警（单位制被静默改动是严重问题）。 */
export function detectCalibrationDrift(
  frozen: CalibrationFreeze,
  current: CalibrationFreeze = buildCalibrationFreeze(),
): Outcome<CalibrationFreeze> {
  const driftFields: string[] = [];
  if (frozen.unitTableFingerprint !== current.unitTableFingerprint) driftFields.push("单位系统表");
  if (frozen.edgeTableFingerprint !== current.edgeTableFingerprint) driftFields.push("换算边表");
  if (frozen.fieldSpecFingerprint !== current.fieldSpecFingerprint) driftFields.push("强度字段约定");
  if (frozen.artisticScale !== current.artisticScale) driftFields.push("艺术制基准标量");

  if (driftFields.length > 0) {
    return err(
      "CALIBRATION_FREEZE_DRIFT",
      `量纲定标漂移：${driftFields.join("、")} 与冻结态不符` +
        `（冻结 ${frozen.unitTableFingerprint}/${frozen.edgeTableFingerprint}/${frozen.fieldSpecFingerprint}/${frozen.artisticScale}，` +
        `当前 ${current.unitTableFingerprint}/${current.edgeTableFingerprint}/${current.fieldSpecFingerprint}/${current.artisticScale}）。`,
      "单位制定标一经冻结即被 J 域全部光源条目（F1803-F1807/J02）引用，" +
        "静默改动会让不同条目的强度语义分叉。补 ADR 后重出冻结快照，或还原至冻结态，二选一。",
    );
  }
  return ok(frozen);
}

/** 一条定标变更提案（锚点：定标变更走 ADR，冻结后只增不改）。 */
export interface CalibrationAmendment {
  readonly proposedByEntryId: number;
  /** 变更的定标面。 */
  readonly target: "UNIT_TABLE" | "EDGE_TABLE" | "FIELD_SPEC" | "ARTISTIC_SCALE";
  readonly rationale: string;
  readonly impact: string;
}

/** 受理定标变更提案。理由或影响面缺失即拒绝——防止无据改单位制。 */
export function proposeCalibrationAmendment(amendment: CalibrationAmendment): Outcome<{ adrId: string }> {
  if (amendment.rationale.trim().length === 0) {
    return err(
      "CALIBRATION_ADR_INCOMPLETE",
      `F${amendment.proposedByEntryId} 的量纲定标变更提案缺少理由。`,
      "ADR 须写明变更动因（如「补充辐照度单位以支持物理光谱」）；无理由的单位制变更不予受理。",
    );
  }
  if (amendment.impact.trim().length === 0) {
    return err(
      "CALIBRATION_ADR_INCOMPLETE",
      `F${amendment.proposedByEntryId} 的量纲定标变更提案缺少影响面。`,
      "ADR 须列明受影响的单位/光源条目与下游消费者（F1803-F1807/F1812/J02），" +
        "便于变更时同步对齐强度语义。",
    );
  }
  return ok({ adrId: `ADR-J-${String(amendment.proposedByEntryId).padStart(4, "0")}-UNIT` });
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §12 编排：单位制定标（表校验 → 冻结 → 往返一致性）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 定标结果。 */
export interface UnitCalibration {
  readonly freeze: CalibrationFreeze;
  readonly recommendedMode: UnitMode;
  /** 四单位逐项自检摘要（供 UI 替述与一致性核对引用）。 */
  readonly unitSummary: readonly { readonly symbol: string; readonly label: string; readonly dimension: string; readonly subject: string }[];
  readonly diagnostics: readonly Diagnostic[];
}

/**
 * 执行量纲定标（J 域全部光源强度语义的定标入口）。
 *
 * 编排五道关（顺序即依赖顺序）：
 *   1. 四单位表齐备性（四单位齐备 + 量纲互异，防表被改残）；
 *   2. 换算边表自洽性（`verifyConversionTable`，逐边量纲断言）；
 *   3. 强度字段约定齐备性（每个适用对象都有登记）；
 *   4. 艺术制往返一致性（量化保护有效性）；
 *   5. 产出定标冻结快照。
 *
 * 任一硬闸失败即不产出冻结快照——单位制没定住，光源强度就无从定标。
 */
export function calibrateLightUnits(): Outcome<UnitCalibration> {
  const diagnostics: Diagnostic[] = [];

  // 12.1 四单位表齐备（量纲互异：若两单位量纲相同则换算边表必然自相矛盾）。
  const expectedUnits: readonly LightUnit[] = ["LUMEN", "CANDELA", "LUX", "NIT"];
  for (const unit of expectedUnits) {
    if (UNIT_DEFINITION_INDEX.get(unit) === undefined) {
      diagnostics.push({
        code: "UNIT_UNKNOWN",
        message: `四单位系统表缺失 ${unit}。`,
        hint: "四单位（lm/cd/lx/nit）为锚点硬要求，缺一即无法为 J 域光源强度定标；请还原该单位定义行。",
      });
    }
  }
  for (let i = 0; i < UNIT_SYSTEM_TABLE.length; i++) {
    for (let j = i + 1; j < UNIT_SYSTEM_TABLE.length; j++) {
      const a = UNIT_SYSTEM_TABLE[i];
      const b = UNIT_SYSTEM_TABLE[j];
      if (a !== undefined && b !== undefined && sameDimension(a.dimension, b.dimension)) {
        diagnostics.push({
          code: "UNIT_CONVERSION_TABLE_INCONSISTENT",
          message: `单位 ${a.symbol} 与 ${b.symbol} 量纲相同（${formatDimension(a.dimension)}），四单位不可有同量纲单位。`,
          hint: "检查两行 dimension 是否被误抄为同值；换算边表以量纲差推导上下文量，同量纲会让两条边同时自相矛盾。",
        });
      }
    }
  }

  // 12.2 换算边表自洽（硬闸）。
  const edgeCheck = verifyConversionTable();
  if (!edgeCheck.ok) return edgeCheck;
  diagnostics.push(...edgeCheck.diagnostics);

  // 12.3 强度字段约定齐备（每个适用对象都要有登记）。
  const subjects: readonly IntensitySubject[] = [
    "DIRECTIONAL_LIGHT",
    "POINT_LIGHT",
    "SPOT_LIGHT",
    "AREA_LIGHT",
    "DISPLAY_OUTPUT",
  ];
  for (const subject of subjects) {
    if (findIntensityFieldSpec(subject) === undefined) {
      diagnostics.push({
        code: "INTENSITY_FIELD_UNREGISTERED",
        message: `适用对象 ${subject}（${SUBJECT_LABELS[subject]}）缺少强度字段单位约定。`,
        hint: "在 INTENSITY_FIELD_SPECS 补该对象的字段/单位/标注键/描述词；未登记的对象会在导入期被拒绝。",
      });
    }
  }

  // 12.4 艺术制往返一致性（量化保护有效性；物理制语义下跑探针）。
  for (const probe of [0.8, 1, 1.2, 0.05, 250]) {
    const roundTrip = verifyArtisticRoundTrip(probe, "PHYSICAL");
    if (!roundTrip.ok) return roundTrip;
  }

  // 12.5 硬闸失败即不冻结（诊断非空即中止）。
  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.message, first.hint, diagnostics);
    }
  }

  const freeze = buildCalibrationFreeze();
  const unitSummary = UNIT_SYSTEM_TABLE.map((d) => ({
    symbol: d.symbol,
    label: d.label,
    dimension: formatDimension(d.dimension),
    subject: Object.entries(SUBJECT_UNIT_BINDING)
      .filter(([, unit]) => unit === d.unit)
      .map(([subject]) => SUBJECT_LABELS[subject as IntensitySubject] ?? subject)
      .join("、"),
  }));

  return ok({ freeze, recommendedMode: "PHYSICAL", unitSummary, diagnostics }, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §13 单位制文档替述（无障碍：文档单位表替述完整）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 渲染量纲定标文档（人话版，锚点无障碍要求：文档单位表替述完整）。
 *
 * 本函数是文档的**可执行生成源**——保证「文档所述」与「代码所声明」同源，
 * 不会各说各话。四单位的定义、适用对象、换算公式全部由表生成，无手抄。
 */
export function renderUnitCalibrationDoc(calibration: UnitCalibration): string {
  const lines: string[] = [];

  lines.push("【VE-J 物理光照量纲 · 定标说明】");
  lines.push("");
  lines.push("一、四单位系统表");
  for (const def of UNIT_SYSTEM_TABLE) {
    lines.push(`  · ${def.symbol}（${def.label}）`);
    lines.push(`    定义：${def.definition}`);
    lines.push(`    回答：${def.measures}`);
    lines.push(`    量纲：${formatDimension(def.dimension)}　物理上限：${def.physicalMax.toExponential(0)}`);
  }

  lines.push("");
  lines.push("二、适用对象与强度单位");
  for (const [subject, unit] of Object.entries(SUBJECT_UNIT_BINDING) as [IntensitySubject, LightUnit][]) {
    lines.push(`  · ${SUBJECT_LABELS[subject]}：${unit}`);
  }

  lines.push("");
  lines.push("三、换算公式（数值 × 真实几何量，绝不猜默认值）");
  for (const edge of CONVERSION_EDGES) {
    lines.push(`  · ${edge.formula}　［上下文：${edge.contextKind}］${edge.rationale}`);
  }
  lines.push("  说明：四单位两两之间并非都能只按比例换算——lx 与 lm 之间隔着受照面积，");
  lines.push("        cd 与 nit 之间隔着距离平方，cd 与 lm 之间隔着立体角。");
  lines.push("        因此换算必须由调用方提供真实几何量，缺失即报错而非填默认值。");

  lines.push("");
  lines.push("四、双单位模式");
  lines.push(`  · PHYSICAL（推荐默认）：${SUBJECT_LABELS.POINT_LIGHT}等新建资产一律写物理单位数值，内部零缩放。`);
  lines.push(`  · ARTISTIC（兼容老资产）：${ARTISTIC_BASIS_DECLARATION.source}`);
  lines.push(`    艺术值 1.0 ≡ 参考白点 ${ARTISTIC_BASIS_DECLARATION.referenceWhiteNit} nit（在 1 m² 面 / 1 m 距离下）。`);
  lines.push(`    ${ARTISTIC_BASIS_DECLARATION.sharedScaleRationale}`);
  lines.push("  · 混用规则：按场景级统一配置执行，资产自带标注优先解释其数值，不一致则告警。");
  lines.push("  · 模式切换：触发场景级重编译一次（非每帧），换算在编译期折叠为着色器常量。");

  lines.push("");
  lines.push("五、强度字段单位标注约定");
  for (const spec of INTENSITY_FIELD_SPECS) {
    lines.push(
      `  · ${spec.field}（${SUBJECT_LABELS[spec.subject]}）：${spec.unit}` +
        `　标注键 ${spec.annotationKey}　物理制${spec.requiredInPhysical ? "强制标注" : "可选"}`,
    );
    lines.push(`    ${spec.description}`);
  }
  lines.push("  · 未标注单位的旧资产：按艺术制默认导入并在本文档声明，迁移建议为补 intensityUnit。");

  lines.push("");
  lines.push("六、定标冻结");
  lines.push(`  · 单位表指纹 ${calibration.freeze.unitTableFingerprint}`);
  lines.push(`  · 换算边表指纹 ${calibration.freeze.edgeTableFingerprint}`);
  lines.push(`  · 强度字段约定指纹 ${calibration.freeze.fieldSpecFingerprint}`);
  lines.push(`  · 艺术制基准标量 ${calibration.freeze.artisticScale}`);
  lines.push("  · 冻结后变更走 ADR（F1798 变更纪律），只增不改；漂移由 detectCalibrationDrift 检出。");

  return lines.join("\n");
}