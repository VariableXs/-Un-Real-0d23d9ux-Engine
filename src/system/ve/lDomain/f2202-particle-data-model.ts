/**
 * VE-F2202 · L 域粒子数据模型（L 域 · 粒子与物理域 · 批次 L01 第 2 项）
 * ---------------------------------------------------------------------------
 * 职责定位：粒子数据模型——**粒子池与属性**。它不发射粒子、不算一次求力、
 * 不碰一次显存，只把「一个粒子由哪些字节构成、这些字节怎么排、占多少内存」
 * 做成一份可机检的规格：
 *   1. 六类标准属性规格表（位置/速度/寿命/颜色/尺寸/自定义锚点）；
 *   2. 自定义属性扩展槽（名称/类型注册制——未注册即拒绝）；
 *   3. SoA 布局声明（结构数组、16 字节对齐——SIMD 与 GPU 友好，
 *      跨域布局范式与 F1526 同构，单源声明）；
 *   4. 池上限与内存预计算（CPU 池/GPU 池各自的容量上限；内存需求 O(1) 可预计算）；
 *   5. 布局版本化（属性布局变更即池格式升版——前向兼容约束预置）。
 *
 * 为什么数据模型必须排在发射器之前（域内立场）：
 *   粒子系统的第一个真实 bug 几乎从不是「算错了」，而是「这些字节到底怎么排」
 *   没定下来：有人按 AoS 写、有人按 SoA 写；有人把颜色塞进 float3、有人塞 float4；
 *   有人给自定义属性留 8 字节、实际写了 16 字节。于是池内存算不准、GPU 上传
 *   的 stride 与 CPU 侧不一致、SIMD 读到跨边界数据——而这些缺陷在粒子数为
 *   零时一个都不显现，等到十万粒子跑起来才炸，返工成本极高。
 *   故本条把布局定成**声明资产**：规格表是唯一事实源，其余条目（F2208 池实现、
 *   F2229 SIMD 求力、F2242 GPU 池）都从这一份表派生，不允许各自复述一份。
 *
 * 五条锚点契约（逐条对应判据）：
 *   1. 属性规格表 —— 六标准属性逐行声明：属性 id、类型、字节数、单位、默认值、
 *      钳制域、语义。规格表是**公开资产**：文档、编辑器 UI、GPU 上传、
 *      调试工具全部读同一份，不允许各写一份说明。
 *   2. 扩展槽 —— 自定义属性走注册制（名称 + 类型 + 字节数 + 对齐）。
 *      未注册的属性名一律拒绝并给出注册指引：属性名进入 SoA 布局后就是
 *      内存布局的一部分，让它随意出现等于让池的 stride 变成运行期才知道的值，
 *      O(1) 内存预计算随之失效。
 *   3. SoA 布局单源 —— 由规格表**推导**出布局（而非另写一份布局声明）：
 *      数组逐属性连续存放、起始偏移按 16 字节对齐、stride 由推导得出。
 *      推导式唯一 ⇒ 规格表改了布局自动跟着变，不存在「表与布局各说各话」。
 *   4. 内存预计算 —— 给定容量与布局，O(1) 算出池字节数（含对齐padding）。
 *      预算是配额的基数：容量 × stride，超预算即拒绝并给出建议容量，
 *      不接受「先跑起来再说」。
 *   5. 布局版本化 —— 布局指纹（由规格表与扩展槽推导）与池格式版本号绑定。
 *      池文件/网络传输的布局漂移靠指纹检出（与 F1201 的冻结指纹同纪律）：
 *      实现与规格表不符时对账钩子显性报错，而不是读到错位的字节后静默出画面。
 *
 * 错误路径与降级矩阵（逐条零静默）：
 *   - 自定义属性未注册 → 拒绝 + 给出注册方式（不静默丢弃该属性）。
 *   - 池容量超内存预计算上限 → 拒绝 + 建议降容量（附可容纳的最大容量）。
 *   - 布局漂移（实现上报的布局与规格表推导不符）→ 对账钩子显性失败。
 *   - stride 未按 16 字节对齐 → 拒绝（SIMD 对齐红线：未对齐的 SoA 会让
 *     向量加载退化成跨边界读，性能收益从「6x」变成负数，且行为随编译器版本漂移）。
 *   - 属性规格自相矛盾（字节数与类型不符 / 钳制域倒置）→ 规格表自身报错。
 *
 * 零静默纪律：所有拒绝路径一律产出 Diagnostic（code + message + hint），
 * 本模块不抛异常、不吞诊断、无静默分支。
 *
 * 判据：六属性 + 扩展、SoA 单源、内存预计算、对齐红线、布局版本化、判据。
 * 交接说明：本条是纯声明层，零 GPU 调用、零 DOM 依赖、零全局可变状态——
 *         可在任意宿主（浏览器/Worker/Node 校验脚本）中原样引入。
 *         下游（F2208 池实现 / F2229 SIMD 求力 / F2242 GPU 池）从本条的
 *         PARTICLE_ATTR_SPECS 与 deriveSoALayout 派生内存布局。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型（零静默的基础设施：与 F2201 同纪律，此处独立实现不跨域 import）
// ════════════════════════════════════════════════════════════════════════════

/** 诊断码：每种拒绝/越权/退化独立可检索，绝不合并成一条通用错误。 */
export type DiagCode =
  /** 自定义属性未注册：属性名不在扩展槽注册表中。 */
  | "CUSTOM_ATTR_UNREGISTERED"
  /** 自定义属性注册项自相矛盾（字节数与类型不符 / 对齐非 2 的幂 / 名称为空）。 */
  | "CUSTOM_ATTR_SPEC_INCONSISTENT"
  /** 标准属性规格表自身矛盾（类型与字节数不符 / 钳制域倒置 / 默认值越界）。 */
  | "ATTR_SPEC_INCONSISTENT"
  /** 属性 id 重复：两个属性声明了同一 id（SoA 偏移将互相覆盖）。 */
  | "ATTR_ID_DUPLICATE"
  /** 池容量非法（非正整数 / 超过路径规格表上限）。 */
  | "POOL_CAPACITY_INVALID"
  /** 池容量超出内存预算：按容量 × stride 计算的字节数超过声明预算。 */
  | "POOL_CAPACITY_EXCEEDS_BUDGET"
  /** SoA 布局未按对齐红线对齐（stride 或偏移非 16 字节倍数）。 */
  | "LAYOUT_ALIGNMENT_VIOLATION"
  /** 布局对账失败：实现上报的布局与规格表推导出的布局不一致。 */
  | "LAYOUT_DRIFT"
  /** 布局版本非法（非受支持的池格式位）。 */
  | "LAYOUT_VERSION_INVALID"
  /** 池类型非法（不在 CPU/GPU 两行之内）。 */
  | "POOL_KIND_INVALID";

/** 一条诊断：发生了什么（人话）、影响什么、下一步怎么办（可操作）。 */
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

/** 成功构造（diagnostics 允许携带非致命告警）。 */
export function ok<T>(value: T, diagnostics: readonly Diagnostic[] = []): Outcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造：失败路径必须给出可操作提示，不允许裸码。 */
export function fail<T>(code: DiagCode, message: string, hint: string): Outcome<T> {
  return { ok: false, code, message, hint, diagnostics: [{ code, message, hint }] };
}

/**
 * 诊断袋：跨函数聚合容器。
 *
 * 存在的理由：本条有「产出冲突清单但整体仍成功」的场景（规格表审计时
 * 逐行核查，收集全部问题比遇错即停更有用——开发者要一次看全所有矛盾）。
 * 用单个 Outcome 会迫使这些问题被丢弃或让函数失败，二者都错。
 */
export class DiagBag {
  private readonly items: Diagnostic[] = [];

  /** 追加一条诊断。 */
  push(code: DiagCode, message: string, hint: string): void {
    this.items.push({ code, message, hint });
  }

  /** 批量并入。 */
  pushAll(items: readonly Diagnostic[]): void {
    this.items.push(...items);
  }

  /** 只读视图。 */
  all(): readonly Diagnostic[] {
    return [...this.items];
  }

  /** 是否为空。 */
  get empty(): boolean {
    return this.items.length === 0;
  }

  /** 条数。 */
  get size(): number {
    return this.items.length;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 值域：类型与钳制域
// ════════════════════════════════════════════════════════════════════════════

/**
 * 属性标量类型。
 *
 * 为何只有这四种（定标理由，不是随手枚举）：
 *   float  —— 单标量（寿命、尺寸、半径）。32 位足够：寿命以秒计，
 *             16.7s@60fps 下 32 位尾数给出的分辨率远超人眼可辨。
 *   float3 —— 三维向量（位置、速度）。**不用 float4 存向量**是刻意的：
 *             向量补齐到 16 字节会让每粒子多占 4 字节，十万粒子即多 400KB，
 *             而 SoA 的向量数组靠「数组起始偏移对齐」已足够 SIMD 化，
 *             元素内部补齐是AoS 时代的习惯（见 §4 对齐纪律）。
 *   float4 —— 需要整体读写且必须 16 字节对齐者（颜色 RGBA）。
 *             颜色刻意补齐：纹理采样端普遍按 vec4 取，跨边界读会掉性能；
 *             且后续可能挂 alpha 之外的语义（如HDR 强度），预留位省一次布局变更。
 *   uint32  —— 位域与句柄（自定义扩展槽的常见形态：材质 id、发射器索引）。
 *             用 uint 而非 float 是为了让「id 类语义」与「物理量语义」
 *             在类型层面就不可互换。
 */
export type AttrScalarType = "float" | "float3" | "float4" | "uint32";

/** 各标量类型的字节数（唯一定义源，布局推导与内存预计算都读它）。 */
export const SCALAR_BYTES: Readonly<Record<AttrScalarType, number>> = {
  float: 4,
  float3: 12,
  uint32: 4,
  float4: 16,
};

/** 各标量类型的分量数（自定义属性的元数校验读它）。 */
export const SCALAR_COMPONENTS: Readonly<Record<AttrScalarType, number>> = {
  float: 1,
  float3: 3,
  uint32: 1,
  float4: 4,
};

/** 各标量类型的自然对齐字节数（SoA 偏移推导读它）。 */
export const SCALAR_ALIGN_BYTES: Readonly<Record<AttrScalarType, number>> = {
  float: 4,
  float3: 4,
  uint32: 4,
  float4: 16,
};

/**
 * 对齐红线：SoA 布局中每个数组的起始偏移与总 stride 都必须是本值的倍数。
 *
 * 数值取16 的理由（定标，非随手）：
 *   16 字节是 AVX（256 位）单指令的向量宽度，也是主流 GPU 结构体布局的
 *   基本对齐单位。取 4 或 8 只能喂饱 SSE，取 32 则为 AVX-512 机器对齐而在
 *   只跑 AVX 的机器上白占空间。粒子池每帧全量读写，对齐收益直接乘以粒子数：
 *   十万粒子错对齐一次就是每帧多搬一整趟缓存行。
 */
export const SOA_ALIGN_BYTES = 16;

/** 钳制域：属性值在运行期被允许的范围（null 表示不钳制）。 */
export interface ClampDomain {
  readonly lo: number | null;
  readonly hi: number | null;
}

/** 无钳制（开放域）。 */
export const NO_CLAMP: ClampDomain = { lo: null, hi: null };

// ════════════════════════════════════════════════════════════════════════════
// §3 属性规格表（六类标准属性 + 自定义扩展槽）
// ════════════════════════════════════════════════════════════════════════════

/** 标准属性 id（六类标准属性的稳定标识；扩展槽属性不得与之一致）。 */
export type StandardAttrId =
  /** 位置：世界坐标。 */
  | "position"
  /** 速度：世界单位每秒。 */
  | "velocity"
  /** 寿命：已存活时长。 */
  | "age"
  /** 颜色：线性 RGBA。 */
  | "color"
  /** 尺寸：等效半径。 */
  | "size"
  /** 自定义锚点：预留的标准扩展位（发射器索引等）。 */
  | "anchor";

/** 单条属性规格（一行规格表）。 */
export interface AttrSpec {
  /** 属性 id：标准属性取标准名，自定义属性取注册名。 */
  readonly id: string;
  /** 属性中文名（公开资产：编辑器 UI 与文档读它）。 */
  readonly name: string;
  /** 标量类型。 */
  readonly type: AttrScalarType;
  /** 字节数（须与type 一致；单独列出是为了让布局推导不必再查表）。 */
  readonly bytes: number;
  /** 单位（无单位者写「无量纲」，不留空——空单位在文档里会被渲染成空白格）。 */
  readonly unit: string;
  /** 默认值（新建粒子的初始值；向量类给出全部分量）。 */
  readonly defaultValue: readonly number[];
  /** 钳制域（null 分量表示该侧不钳制）。 */
  readonly clamp: ClampDomain;
  /** 语义一句话（公开资产）。 */
  readonly semantic: string;
  /** 是否为标准属性（自定义扩展槽为 false）。 */
  readonly standard: boolean;
}

/**
 * 六类标准属性规格表（判据一的核心资产）。
 *
 * 逐项定标理由（不逐项说明就会出现「凭感觉填的默认值」）：
 *   position —— float3/世界单位/默认原点/不钳制。位置无界是场景决定的，
 *     钳到固定域反而会让大场景的粒子被拽回来。
 *   velocity —— float3/世界单位每秒/默认零/不钳制。速度可能为负（方向），
 *     且回弹、风场累加都可使其瞬时很大，钳制会引入能量不守恒的假象。
 *   age—— float/秒/默认 0/钳制 [0, +∞)。下界 0 是硬约束：负寿命会让
 *     「是否该回收」的判定（age ≥ life）永远为真，全池瞬间回收；
 *     上界开放，因为具体寿命由发射器参数决定，不在属性层设限。
 *   color —— float4/线性 0~1/默认不透明白/钳制 [0,1]。颜色是本表唯一
 *     默认值非零的属性（白=不透明、不染色），因为「新建粒子是黑色」
 *     在多数发射器里表现为「凭空出现一团黑」，观感上像 bug。
 *     钳制 [0,1] 而非 HDR：HDR 颜色值 >1 会让加性混合溢出，
 *     超出本条职责（若将来要 HDR，须走 ADR 升布局版本而不是悄悄放开钳制）。
 *   size  —— float/世界单位/默认 1.0/钳制 (0, +∞)。下界**开区间**：
 *     尺寸为 0 的粒子在光栅化阶段不产生任何片元，却仍占满池容量与
 *     显存带宽——零尺寸粒子是纯浪费，必须在数据层就拒绝。
 *   anchor —— uint32/无量纲/默认 0/不钳制。发射器索引，供发射器回收时
 *     归还配额（F2203 联动）。取 uint32 是因为它是 id 不是物理量。
 */
export const STANDARD_ATTR_SPECS: readonly AttrSpec[] = [
  {
    id: "position",
    name: "位置",
    type: "float3",
    bytes: 12,
    unit: "世界单位",
    defaultValue: [0, 0, 0],
    clamp: NO_CLAMP,
    semantic: "粒子在世界空间的位置；渲染与碰撞的共同基准",
    standard: true,
  },
  {
    id: "velocity",
    name: "速度",
    type: "float3",
    bytes: 12,
    unit: "世界单位/秒",
    defaultValue: [0, 0, 0],
    clamp: NO_CLAMP,
    semantic: "粒子速度；积分与运动场的输入量",
    standard: true,
  },
  {
    id: "age",
    name: "寿命",
    type: "float",
    bytes: 4,
    unit: "秒",
    defaultValue: [0],
    clamp: { lo: 0, hi: null },
    semantic: "已存活时长；与发射器设定的寿命比较得出是否回收",
    standard: true,
  },
  {
    id: "color",
    name: "颜色",
    type: "float4",
    bytes: 16,
    unit: "线性 0~1",
    defaultValue: [1, 1, 1, 1],
    clamp: { lo: 0, hi: 1 },
    semantic: "线性 RGBA；渲染着色的唯一来源",
    standard: true,
  },
  {
    id: "size",
    name: "尺寸",
    type: "float",
    bytes: 4,
    unit: "世界单位",
    defaultValue: [1],
    clamp: { lo: 0, hi: null },
    semantic: "等效半径；光栅化尺寸与碰撞体尺寸同源",
    standard: true,
  },
  {
    id: "anchor",
    name: "自定义锚点",
    type: "uint32",
    bytes: 4,
    unit: "无量纲",
    defaultValue: [0],
    clamp: NO_CLAMP,
    semantic: "发射器索引等锚点 id；供发射器回收时归还配额",
    standard: true,
  },
];

/** 标准属性 id 全集（遍历与无缝隙校验用）。 */
export const STANDARD_ATTR_IDS: readonly StandardAttrId[] = STANDARD_ATTR_SPECS.map((s) => s.id) as StandardAttrId[];

// ════════════════════════════════════════════════════════════════════════════
// §4 自定义属性扩展槽（注册制）
// ════════════════════════════════════════════════════════════════════════════

/** 自定义属性的注册项。 */
export interface CustomAttrRegistration {
  /** 注册名（进布局后不可改：改名等于换布局，须走布局升版）。 */
  readonly name: string;
  /** 标量类型。 */
  readonly type: AttrScalarType;
  /** 语义一句话（注册时必须写：无名属性会在三个月后无人敢动）。 */
  readonly semantic: string;
  /** 单位。 */
  readonly unit: string;
}

/**
 * 自定义属性注册表（判据二的核心资产）。
 *
 * 为何是注册制而非「随便加字段」（这是本条最容易被误解的约束）：
 *   粒子属性名一旦进入 SoA 布局，它就是**内存布局的一部分**——偏移量、
 *   stride、GPU 上传缓冲区的解释方式全都由它决定。若允许运行期随意添加，
 *   则池的字节数只能在填充完成之后才知道，O(1) 内存预计算当场失效，
 *   「容量 × stride」这个预算基数也就不成立了。
 *   注册制把这个决定提前到编译期：布局在池创建前就是确定的常量。
 *
 * 初始三条注册项为何是这三类（覆盖真实发射器的高频需求，避免下游一上来就撞未注册）：
 *   material  —— 材质 id（uint32）：同一发射器混多种材质是常见需求。
 *   turbulence—— 湍流强度（float）：per-particle湍流权重，风场（力场域）需要。
 *   seed      —— 随机种子（uint32）：per-particle 确定性随机，F2215
 *     双跑逐位复现断言依赖它——没有 per-particle 种子，同种子双跑会因
 *     发射顺序变化而分叉。这条是 F2215 的地基，不是可选装饰。
 */
export const CUSTOM_ATTR_REGISTRY: readonly CustomAttrRegistration[] = [
  { name: "material", type: "uint32", unit: "无量纲", semantic: "材质 id；同发射器混材质的分派键" },
  { name: "turbulence", type: "float", unit: "无量纲", semantic: "湍流权重；力场采样时的 per-particle 强度" },
  { name: "seed", type: "uint32", unit: "无量纲", semantic: "per-particle 随机种子；双跑逐位复现的地基" },
];

/**
 * 查找自定义属性注册项。未注册返回 null（调用方据此产出可操作拒绝）。
 *
 * 名称比对刻意**大小写敏感且不做 trim**：属性名是布局键，
 * "Seed" 与 "seed" 在内存里是两个不同偏移，静默归一等于允许两个不同的
 * 布局共用一个名字。
 */
export function lookupCustomAttr(name: string): CustomAttrRegistration | null {
  for (const reg of CUSTOM_ATTR_REGISTRY) {
    if (reg.name === name) return reg;
  }
  return null;
}

/** 自定义属性名全集（布局推导按注册顺序展开）。 */
export const CUSTOM_ATTR_NAMES: readonly string[] = CUSTOM_ATTR_REGISTRY.map((r) => r.name);

// ════════════════════════════════════════════════════════════════════════════
// §5 SoA 布局（判据三：由规格表推导，不另写声明）
// ════════════════════════════════════════════════════════════════════════════

/** 布局中的一个数组（SoA 的一维）。 */
export interface SoAArray {
  /** 属性 id（标准属性名或自定义注册名）。 */
  readonly attrId: string;
  /** 标量类型。 */
  readonly type: AttrScalarType;
  /** 每元素字节数。 */
  readonly bytesPerElement: number;
  /** 数组起始偏移（相对池基址；必为 SOA_ALIGN_BYTES 的倍数）。 */
  readonly offset: Bytes;
  /** 是否为标准属性。 */
  readonly standard: boolean;
}

/** 字节数（标称名，避免与 `number` 混读时看不出单位）。 */
export type Bytes = number;

/** 池布局（一粒子所占字节的完整声明）。 */
export interface SoALayout {
  /** 逐数组声明（顺序即布局顺序；标准属性在前、自定义在后）。 */
  readonly arrays: readonly SoAArray[];
  /** 单粒子总stride（必为 SOA_ALIGN_BYTES 的倍数）。 */
  readonly stride: Bytes;
  /** 布局版本号（池格式位；布局内容变更即升版）。 */
  readonly layoutVersion: number;
}

/** 池格式版本（v1 = 六标准属性 + 对齐纪律；语义破坏性变更须升版）。 */
export const POOL_FORMAT_VERSION = 1;

/** 对齐辅助：向上取整到 alignment 的倍数。 */
export function alignUp(value: number, alignment: number): number {
  const rem = value % alignment;
  return rem === 0 ? value : value + (alignment - rem);
}

/**
 * 由规格表推导SoA 布局（判据三的单源保证）。
 *
 * 推导规则（唯一，不接受第二份手写布局声明）：
 *   1. 顺序 = 标准属性表顺序，随后是自定义扩展槽的**注册顺序**；
 *   2. 每数组起始偏移 = alignUp(前一项结束偏移, max(SOA_ALIGN_BYTES, 类型自然对齐))；
 *   3. 总 stride = alignUp(末项结束偏移, SOA_ALIGN_BYTES)。
 *
 * 规则 2 里的 `max` 不可省：float4 的自然对齐是 16，float 是 4。
 * 只按 16 对齐就够（16 是 4 的倍数），但若将来出现自然对齐大于16 的类型
 * （如 future 的 32 字节向量），写死 16 会让该数组起始错位。
 * 把类型自然对齐取入max 是让规则对未知类型也成立，而不是只对今天这四种成立。
 *
 * 拒绝的情形（产出诊断，由调用方决定是否阻断）：
 *   - 自定义属性未注册（offset 无从推导）；
 *   - 推导出的 stride 未按 SOA_ALIGN_BYTES 对齐（对齐红线）；
 *   - 属性 id 重复（两个数组会争同一偏移）。
 */
export function deriveSoALayout(customAttrs: readonly string[] = []): Outcome<SoALayout> {
  const bag = new DiagBag();
  const arrays: SoAArray[] = [];
  const seen = new Set<string>();
  let cursor: Bytes = 0;

  const push = (attrId: string, type: AttrScalarType, standard: boolean): void => {
    if (seen.has(attrId)) {
      bag.push(
        "ATTR_ID_DUPLICATE",
        `属性 id ${attrId} 在布局中出现两次。`,
        "两个数组会争同一段偏移，池内存将互相覆盖；改名或去掉重复声明。",
      );
      return;
    }
    seen.add(attrId);
    const bytesPerElement = SCALAR_BYTES[type];
    const offset = alignUp(cursor, Math.max(SOA_ALIGN_BYTES, SCALAR_ALIGN_BYTES[type]));
    arrays.push({ attrId, type, bytesPerElement, offset, standard });
    cursor = offset + bytesPerElement;
  };

  for (const spec of STANDARD_ATTR_SPECS) {
    push(spec.id, spec.type, true);
  }

  for (const name of customAttrs) {
    // 撞名先行判定：与标准属性同名是一种**具体的**错误（两种类型抢同一偏移），
    // 若并入「未注册」分支，处方会退化成「去注册」，而注册它恰恰会制造冲突——
    // 处方与病因相反是零静默纪律里最伤的一种。
    if (STANDARD_ATTR_IDS.includes(name as StandardAttrId)) {
      bag.push(
        "CUSTOM_ATTR_SPEC_INCONSISTENT",
        `自定义属性 ${name} 与标准属性同名。`,
        `改名，或直接使用标准属性 ${name}（它已在布局中）；把标准属性名注册成扩展属性会让两者争同一段偏移，读取方无法区分语义。`,
      );
      continue;
    }
    const reg = lookupCustomAttr(name);
    if (reg === null) {
      bag.push(
        "CUSTOM_ATTR_UNREGISTERED",
        `自定义属性 ${name} 未在扩展槽注册表中登记。`,
        `先在 CUSTOM_ATTR_REGISTRY 注册（名称/类型/单位/语义四样齐备）再引用；` +
          `已注册的有：${CUSTOM_ATTR_NAMES.join("、")}。属性名决定内存偏移，运行期临时添加会让 stride 无法预计算。`,
      );
      continue;
    }
    push(reg.name, reg.type, false);
  }

  const stride = alignUp(cursor, SOA_ALIGN_BYTES);

  // 对齐红线：逐数组偏移与总 stride 都必须守住 16 字节（SIMD 向量加载的边界）。
  for (const a of arrays) {
    if (a.offset % SOA_ALIGN_BYTES !== 0) {
      bag.push(
        "LAYOUT_ALIGNMENT_VIOLATION",
        `数组 ${a.attrId} 的起始偏移 ${a.offset} 不是 ${SOA_ALIGN_BYTES} 的倍数。`,
        `未对齐的 SoA 会让向量加载退化为跨缓存行读，SIMD 收益从数倍变成负数；` +
          `检查推导规则或类型自然对齐（当前 ${a.type} 的自然对齐为 ${SCALAR_ALIGN_BYTES[a.type]}）。`,
      );
    }
  }
  if (stride % SOA_ALIGN_BYTES !== 0) {
    bag.push(
      "LAYOUT_ALIGNMENT_VIOLATION",
      `推导出的总 stride ${stride} 不是 ${SOA_ALIGN_BYTES} 的倍数。`,
      "stride 未对齐会让逐粒子步进跨越向量边界，SIMD 求力（F2229）无法按整粒子批量处理。",
    );
  }

  if (!bag.empty) {
    const first = bag.all()[0];
    if (first !== undefined) return fail(first.code, first.message, first.hint);
  }

  return ok({ arrays, stride, layoutVersion: POOL_FORMAT_VERSION }, bag.all());
}

/**
 * 基线布局（六标准属性 + 注册表全部扩展），O(1) 可复算。
 *
 * 存在的理由：GPU 池（F2242）与 SIMD 求力（F2229）都需要一份确定的布局，
 * 但它们不该各自写死一份声明（那就是布局漂移的源头）。此函数是「不传自定义
 * 属性」时的规范布局，一切默认假设都应引用它。
 */
export function baselineLayout(): Outcome<SoALayout> {
  return deriveSoALayout(CUSTOM_ATTR_NAMES);
}

// ════════════════════════════════════════════════════════════════════════════
// §6 池上限与内存预计算（判据四）
// ════════════════════════════════════════════════════════════════════════════

/** 池种类（CPU 池/GPU 池两行，各自容量上限不同）。 */
export type PoolKind = "cpu" | "gpu";

/** 池容量与内存预算的行规格。 */
export interface PoolKindSpec {
  readonly kind: PoolKind;
  readonly kindName: string;
  /** 默认容量。 */
  readonly defaultCapacity: number;
  /** 容量上限（硬顶；超过即拒绝，与预算闸门是两道独立的门）。 */
  readonly maxCapacity: number;
  /** 默认内存预算字节数（0 表示不限，仅受maxCapacity 约束）。 */
  readonly defaultBudgetBytes: Bytes;
  readonly duty: string;
}

/**
 * 两行池规格。
 *
 * 数值定标（不是随手填的默认）：
 *   CPU 池默认 10 万/ 硬顶 100 万 —— 与 F2201 的 PATH_TABLE 呼应：
 *     CPU 路径的目标规模上界即 10 万（超过则每帧撞帧预算，F2210 预算条目），
 *     默认值取路径上界意味着「按默认配置跑」恰好是 CPU 路径的最优工况。
 *     硬顶放宽到 100 万是为了容纳离线渲染/预热场景（可超帧预算，只是不实时）。
 *   GPU 池默认 100 万 / 硬顶 400 万 —— 同样是 PATH_TABLE 的量级：
 *     GPU 路径区间是 10 万~400 万，默认取下界之上一个数量级（100 万）
 *     是「视觉等效优先且实时」的最佳区间中点；硬顶取路径上界 400 万，
 *     超过此值 PATH_TABLE 已无成本优势声明。
 *   两池默认容量不同不是笔误：容量是配额的基数，同一内存预算下
 *     GPU 池必须开更大容量才可能与 CPU 池承载同等内容。
 *
 * 默认预算与硬顶的关系（这条约束是被对抗验证逼出来的，不是事后补的注释）：
 *   **默认预算必须容得下硬顶容量**，即 budgetBytes ≥ maxCapacity × stride。
 *   两条数字若互相矛盾，硬顶就是一句空话：调用方按硬顶申请容量，却在预算门
 *   被拒，报错还会指向「降容量」——而它要的正是硬顶允许的量，于是形成
 *   「按文档配置必然被拒」的死结。
 *   按stride=144（本条基线布局）核算：
 *     CPU 硬顶 100 万 × 144 = 137.3MB → 预算取 160MB；
 *     GPU 硬顶 400 万 × 144 = 549.3MB → 预算取 640MB。
 *   取整到 160/640MB 是为了给后续属性扩展留余量：stride 增长时硬顶仍在预算内。
 *   注意这两个预算是**允许的上界**而非目标占用——真实占用由实际容量决定，
 *   预算是「允许开多大的池」，不是「必须占多少内存」。
 */
export const POOL_KIND_SPECS: Readonly<Record<PoolKind, PoolKindSpec>> = {
  cpu: {
    kind: "cpu",
    kindName: "CPU 池",
    defaultCapacity: 100_000,
    maxCapacity: 1_000_000,
    defaultBudgetBytes: 160 * 1024 * 1024,
    duty: "小规模高精度模拟；逐位确定，池常驻内存供 CPU 顺序访问",
  },
  gpu: {
    kind: "gpu",
    kindName: "GPU 池",
    defaultCapacity: 1_000_000,
    maxCapacity: 4_000_000,
    defaultBudgetBytes: 640 * 1024 * 1024,
    duty: "百万级大规模模拟；显存驻留，按布局上传到 compute 缓冲区",
  },
};

/** 池种类全集。 */
export const POOL_KINDS: readonly PoolKind[] = ["cpu", "gpu"];

/** 池实例的完整声明（容量 + 布局 + 内存预算三者绑定）。 */
export interface PoolDeclaration {
  readonly kind: PoolKind;
  /** 粒子容量（活跃槽位数；池按此容量一次性预留，不随活跃数增长）。 */
  readonly capacity: number;
  /** 布局（决定每粒子字节数）。 */
  readonly layout: SoALayout;
  /** 内存预算字节数（0 = 不限，仅受硬顶约束）。 */
  readonly budgetBytes: Bytes;
  /** 池总字节数 = capacity × stride（O(1)，不做循环）。 */
  readonly totalBytes: Bytes;
}

/**
 * 内存预计算：O(1) 给出池字节数。
 *
 * 公式为什么就这么简单（而不是「加上传缓冲、对齐余量、备用槽」）：
 *   SoA 布局下**所有**对齐余量都已经被吸收进 stride——推导规则 3 的最后
 *   一步就是 alignUp(末项结束偏移, 16)。所以 capacity × stride 已经是精确值，
 *   不存在「再加一点保险」的必要。加保险反而会让预算与实际占用脱节，
 *   而预算与实际脱节正是本条要防的那类静默失真。
 *   GPU 上传缓冲（F2242）是运行期按帧分配的，不属于池常驻内存，故不在此公式内。
 */
export function estimatePoolBytes(capacity: number, stride: Bytes): Bytes {
  return capacity * stride;
}

/**
 * 预算内可容纳的最大容量（向下取整；预算不足一粒子时返回 0）。
 *
 * 存在的理由：拒绝超预算容量时必须给出**可执行的建议**（「降到多少」），
 * 只说「超预算了」等于把算术题推给调用方。
 */
export function maxCapacityWithinBudget(budgetBytes: Bytes, stride: Bytes): number {
  if (budgetBytes <= 0 || stride <= 0) return 0;
  return Math.floor(budgetBytes / stride);
}

/**
 * 声明一个池（容量/预算/硬顶三道门一次校验）。
 *
 * 三道门的分工（缺任何一道都让某类失真静默通过）：
 *   ① 容量合法——非正整数或超过硬顶即拒（硬顶是路径规格的无成本边界）；
 *   ② 预算充足——按容量 × stride 算出的字节数超预算即拒（预算是配额基数）；
 *   ③ 布局对齐——布局 drift 或未对齐即拒（对齐红线，见 reconcileLayout）。
 *
 * 错误路径与降级矩阵对应：
 *   - 容量超硬顶 → 拒绝 + 建议降到 maxCapacity；
 *   - 容量超预算 → 拒绝 + 给出预算内最大容量（可执行的降级值）。
 */
export function declarePool(
  kind: PoolKind,
  capacity: number,
  layout: SoALayout,
  budgetBytes: Bytes,
): Outcome<PoolDeclaration> {
  const spec = POOL_KIND_SPECS[kind];
  if (spec === undefined) {
    return fail(
      "POOL_KIND_INVALID",
      `池种类 ${String(kind)} 不在册。`,
      `池种类只有 ${POOL_KINDS.join(" 与 ")} 两行；新增种类须先在POOL_KIND_SPECS 立项并声明容量与预算口径。`,
    );
  }

  if (!Number.isInteger(capacity) || capacity <= 0) {
    return fail(
      "POOL_CAPACITY_INVALID",
      `池容量 ${capacity} 非法（须为正整数）。`,
      "容量是按槽位分配的整数；小数或非正容量说明调用侧单位换算错误（比例↔计数）。",
    );
  }

  if (capacity > spec.maxCapacity) {
    return fail(
      "POOL_CAPACITY_INVALID",
      `${spec.kindName}容量 ${capacity} 超过硬顶 ${spec.maxCapacity}。`,
      `降到 ${spec.maxCapacity} 或以下；超过硬顶后${kind === "gpu" ? "GPU 路径（F2201 PATH_TABLE）已无成本优势声明，再加容量只增显存不增吞吐" : "CPU 路径每帧必撞帧预算（F2210）"}，` +
        `需要更大规模请改走另一条路径。`,
    );
  }

  if (layout.stride % SOA_ALIGN_BYTES !== 0) {
    return fail(
      "LAYOUT_ALIGNMENT_VIOLATION",
      `布局 stride ${layout.stride} 未按 ${SOA_ALIGN_BYTES} 字节对齐。`,
      "未对齐布局不得建池：SIMD 求力（F2229）会退化为跨边界读。先修布局推导，不要靠调容量绕过。",
    );
  }

  const totalBytes = estimatePoolBytes(capacity, layout.stride);

  if (budgetBytes > 0 && totalBytes > budgetBytes) {
    const affordable = maxCapacityWithinBudget(budgetBytes, layout.stride);
    // 硬顶与预算互相矛盾的显式提示：若要申请的容量本就等于硬顶却被预算拒了，
    // 只说「降容量」会把调用方引向死结（降到硬顶以下却仍要那么多粒子）。
    // 这类配置错误必须当场点名，否则会被误诊为容量填错。
    const hitsHardcap = capacity === spec.maxCapacity;
    const budgetBelowHardcap = budgetBytes < spec.maxCapacity * layout.stride;
    const paradox =
      hitsHardcap && budgetBelowHardcap
        ? `注意：要申请的 ${capacity} 恰是${spec.kindName}硬顶，而预算 ${budgetBytes} 低于硬顶所需 ${spec.maxCapacity * layout.stride} 字节——` +
          "这是规格侧的自相矛盾（按硬顶配置必然被拒），须先抬预算或调低硬顶，不应改容量。"
        : "";
    return fail(
      "POOL_CAPACITY_EXCEEDS_BUDGET",
      `${spec.kindName}需 ${totalBytes} 字节（容量 ${capacity} × stride ${layout.stride}），超过预算 ${budgetBytes} 字节。`,
      `预算内最大容量为 ${affordable}（${budgetBytes} ÷ stride ${layout.stride} 向下取整）；` +
        `要保容量就抬预算，要保预算就降容量——两者不可兼得时请先定标哪一个是硬约束。${paradox}`,
    );
  }

  return ok({ kind, capacity, layout, budgetBytes, totalBytes });
}

/** 按池种类的默认容量与默认预算建池（便捷入口；参数错由 declarePool 兜底）。 */
export function declareDefaultPool(kind: PoolKind, layout: SoALayout): Outcome<PoolDeclaration> {
  const spec = POOL_KIND_SPECS[kind];
  if (spec === undefined) {
    return fail(
      "POOL_KIND_INVALID",
      `池种类 ${String(kind)} 不在册。`,
      `可选：${POOL_KINDS.join(" / ")}。`,
    );
  }
  return declarePool(kind, spec.defaultCapacity, layout, spec.defaultBudgetBytes);
}

// ════════════════════════════════════════════════════════════════════════════
// §7 布局版本化与对账（判据五）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 布局指纹：布局内容的稳定摘要（与 F1201 冻结指纹同纪律）。
 *
 * 摘要哪些字段（以及为什么是这几个）：
 *   -逐数组的 (attrId, type, offset)：决定内存解释方式，缺一不可；
 *   - stride：决定步进；
 *   - layoutVersion：决定解释规则本身。
 * 不摘要属性语义/单位/默认值：它们是**文档层**信息，改文案不应触发
 * 池格式升版（否则每次润色注释都要重建全部池）。
 *
 * 哈希用 FNV-1a 32位：用途是「变了没有」的快速比对，不是抗碰撞的密码学。
 * 碰撞概率对本场景可接受（布局变更是低频事件，且真出问题时对账钩子还会
 * 逐字段比对，指纹只是快速筛）。
 */
export function layoutFingerprint(layout: SoALayout): string {
  const parts: string[] = [
    `v${layout.layoutVersion}`,
    `s${layout.stride}`,
    ...layout.arrays.map((a) => `${a.attrId}:${a.type}@${a.offset}`),
  ];
  const text = parts.join("|");
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, "0");
}

/**
 * 布局对账钩子：实现上报的布局 vs 规格表推导的布局。
 *
 * 为什么对账必须**逐字段**比而不是只比指纹：
 *   指纹是 32 位摘要，理论上可能碰撞；而布局错位的后果是读到错位的字节
 *   后画面出现莫名其妙的花屏——这种问题极难回溯到「池布局版本没对上」。
 *   指纹用于快速筛（绝大多数情况下一眼比对），逐字段用于确诊。
 *
 * 报错时给出**第一处**差异即可：布局差异往往成片，逐条列全反而淹没根因。
 */
export function reconcileLayout(expected: SoALayout, reported: SoALayout): Outcome<SoALayout> {
  if (reported.layoutVersion !== POOL_FORMAT_VERSION) {
    return fail(
      "LAYOUT_VERSION_INVALID",
      `实现上报的布局版本 ${reported.layoutVersion}，受支持位为 ${POOL_FORMAT_VERSION}。`,
      "池格式版本是前向兼容的唯一依据；未知版本不得静默按 v1 解释（字段含义可能已变）。",
    );
  }

  if (expected.layoutVersion !== reported.layoutVersion) {
    return fail(
      "LAYOUT_VERSION_INVALID",
      `规格表版本 ${expected.layoutVersion} 与实现上报版本 ${reported.layoutVersion} 不一致。`,
      "两侧必须同版本；跨版本读取前需走布局升版流程（含数据迁移），不可直接解释。",
    );
  }

  if (expected.stride !== reported.stride) {
    return fail(
      "LAYOUT_DRIFT",
      `stride 漂移：规格表为 ${expected.stride}，实现上报 ${reported.stride}。`,
      `以规格表为准重建实现侧布局；若实现确实需要不同 stride，说明属性集已变，` +
        "应升布局版本而不是让两侧各持一份。",
    );
  }

  const n = Math.max(expected.arrays.length, reported.arrays.length);
  for (let i = 0; i < n; i++) {
    const e = expected.arrays[i];
    const r = reported.arrays[i];
    if (e === undefined) {
      return fail(
        "LAYOUT_DRIFT",
        `实现侧多出数组 ${r?.attrId ?? "?"}（第 ${i + 1} 项）。`,
        "实现不得私自扩充布局；确需新属性须走注册表 + 布局升版。",
      );
    }
    if (r === undefined) {
      return fail(
        "LAYOUT_DRIFT",
        `实现侧缺少数组 ${e.attrId}（第 ${i + 1} 项）。`,
        "实现必须完整对齐规格表布局；缺项会让该属性读到相邻数组的字节。",
      );
    }
    if (e.attrId !== r.attrId || e.type !== r.type || e.offset !== r.offset) {
      return fail(
        "LAYOUT_DRIFT",
        `第 ${i + 1} 项布局不一致：规格表 ${e.attrId}/${e.type}@${e.offset}，实现 ${r.attrId}/${r.type}@${r.offset}。`,
        "以规格表推导结果为准修正实现侧；逐字段对账是为了在花屏发生前定位到这一项。",
      );
    }
  }

  return ok(expected);
}

/**
 * 布局兼容性裁决：给定两份布局，判断能否按同一份池内存互读。
 *
 * 三档裁决（前向兼容约束的预置，判据五）：
 *   compatible   —— 指纹一致，可直接共用同一池内存（F2208 池复用/F2242 上传的前提）。
 *   needs-migration —— 版本相同但内容不同：同版本不该有不同内容，判为漂移而非兼容。
 *   incompatible —— 版本不同：语义破坏性变更已发生，禁止按旧版解释。
 *
 * 为何「同版本不同内容」判为漂移而不是兼容：布局版本是布局内容的承诺，
 * 同版本出现两种内容说明有一方没遵守承诺（通常是手写了一份布局声明）。
 * 把它放行为「兼容」会让两套解释同时存在于同一池内存——正是本条要防的静默错位。
 */
export function judgeLayoutCompatibility(
  left: SoALayout,
  right: SoALayout,
): "compatible" | "needs-migration" | "incompatible" {
  if (left.layoutVersion !== right.layoutVersion) return "incompatible";
  return layoutFingerprint(left) === layoutFingerprint(right) ? "compatible" : "needs-migration";
}

// ════════════════════════════════════════════════════════════════════════════
// §8 规格表自审（公开资产必须能自己证明自己一致）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 属性规格表自审：逐行核查类型/字节/分量/默认值/钳制域的自洽性。
 *
 * 为什么需要它（不是「写对了就够」）：规格表是公开资产，编辑器 UI、
 * 文档生成、GPU 上传三处都读它。任何一处行数据写错都会同时污染三处，
 * 而三处各自的测试可能都「通过」（它们照抄了同一份错数据）。
 * 只有**独立于数据**的规则核查才能拦住这种一致性错误。
 */
export function auditAttrSpecs(bag: DiagBag): Outcome<readonly AttrSpec[]> {
  const seen = new Set<string>();

  for (const spec of STANDARD_ATTR_SPECS) {
    if (seen.has(spec.id)) {
      bag.push(
        "ATTR_ID_DUPLICATE",
        `标准属性 ${spec.id} 重复声明。`,
        "重复 id 会让 SoA 推导产生两个同偏移数组；标准属性表是单源，不允许重复。",
      );
    }
    seen.add(spec.id);

    const expectedBytes = SCALAR_BYTES[spec.type];
    if (spec.bytes !== expectedBytes) {
      bag.push(
        "ATTR_SPEC_INCONSISTENT",
        `属性 ${spec.id} 声明字节数 ${spec.bytes}，但类型 ${spec.type} 应为 ${expectedBytes} 字节。`,
        `以类型为准修正字节数；字节数参与 stride 推导，两处不一致会让内存预计算与实际占用脱节。`,
      );
    }

    const components = SCALAR_COMPONENTS[spec.type];
    if (spec.defaultValue.length !== components) {
      bag.push(
        "ATTR_SPEC_INCONSISTENT",
        `属性 ${spec.id}（${spec.type}）默认值给了 ${spec.defaultValue.length} 个分量，应为 ${components} 个。`,
        "分量数与类型不符会让初始化按错误步长写入；float3 给 4 个分量会多写越界一个 float。",
      );
    }

    const { lo, hi } = spec.clamp;
    if (lo !== null && hi !== null && lo > hi) {
      bag.push(
        "ATTR_SPEC_INCONSISTENT",
        `属性 ${spec.id} 的钳制域倒置：lo=${lo} > hi=${hi}。`,
        "钳制域下界不得大于上界；倒置域会让任何值都被判越界。",
      );
    }
    if (lo !== null && hi !== null && lo === hi) {
      // 不是错误（退化钳制在数学上合法），但对连续量几乎必是笔误，显式告知。
      bag.push(
        "ATTR_SPEC_INCONSISTENT",
        `属性 ${spec.id} 的钳制域退化为单点 ${lo}（lo=hi）。`,
        "若本意为「不钳制」，请写 NO_CLAMP（两侧 null）；单点域会让该属性恒为常量。",
      );
    }

    for (const v of spec.defaultValue) {
      const belowLo = lo !== null && v < lo;
      const aboveHi = hi !== null && v > hi;
      if (belowLo || aboveHi) {
        bag.push(
          "ATTR_SPEC_INCONSISTENT",
          `属性 ${spec.id} 的默认值 ${v} 越出钳制域 [${String(lo)}, ${String(hi)}]。`,
          "新建粒子一出生就带着越界值，下一帧钳制会把它改写——这既是视觉抖动也是脏数据来源。",
        );
      }
    }

    if (spec.unit.length === 0) {
      bag.push(
        "ATTR_SPEC_INCONSISTENT",
        `属性 ${spec.id} 的单位为空。`,
        "无单位请显式写「无量纲」；空单位在文档与 UI 里会渲染成空白格。",
      );
    }
  }

  // 自定义属性名不得与标准属性撞名（撞名等于两种类型抢同一偏移）。
  for (const reg of CUSTOM_ATTR_REGISTRY) {
    if (seen.has(reg.name)) {
      bag.push(
        "CUSTOM_ATTR_SPEC_INCONSISTENT",
        `自定义属性 ${reg.name} 与标准属性同名。`,
        "改名；同名会让 SoA 偏移冲突，且读取方无法区分读到的是标准语义还是扩展语义。",
      );
    }
    if (reg.name.trim().length === 0) {
      bag.push(
        "CUSTOM_ATTR_SPEC_INCONSISTENT",
        `自定义属性的名称为空或全为空白。`,
        "属性名是内存布局的键，不可为空；空白名在布局字符串里不可区分。",
      );
    }
    if (reg.semantic.trim().length === 0) {
      bag.push(
        "CUSTOM_ATTR_SPEC_INCONSISTENT",
        `自定义属性 ${reg.name} 未写语义。`,
        "注册时必须写语义：无名属性三个月后无人敢动，届时会成为删不掉的兼容负担。",
      );
    }
    if (reg.type !== "float" && reg.type !== "float3" && reg.type !== "float4" && reg.type !== "uint32") {
      bag.push(
        "CUSTOM_ATTR_SPEC_INCONSISTENT",
        `自定义属性 ${reg.name} 的类型 ${String(reg.type)} 不在四种标量类型内。`,
        "先在AttrScalarType 上加类型并同步 SCALAR_BYTES/SCALAR_ALIGN_BYTES/SCALAR_COMPONENTS 三张表，不可在注册侧私造。",
      );
    }
  }

  if (!bag.empty) {
    const first = bag.all()[0];
    if (first !== undefined) return fail(first.code, first.message, first.hint);
  }
  return ok(STANDARD_ATTR_SPECS, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §9 判据自检（不靠人读代码确认，靠断言输出）
// ════════════════════════════════════════════════════════════════════════════

/** 自检结果项：每项对应判据的一条，独立可定位。 */
export interface SelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 判据一：六属性 + 扩展槽齐备。 */
export function selfCheckAttrSpecs(): SelfCheck[] {
  const out: SelfCheck[] = [];

  out.push({
    name: "spec-six-standard-attrs",
    pass: STANDARD_ATTR_SPECS.length === 6,
    detail: `标准属性数 = ${STANDARD_ATTR_SPECS.length}（期望 6：位置/速度/寿命/颜色/尺寸/自定义锚点）`,
  });

  const expected = ["position", "velocity", "age", "color", "size", "anchor"];
  const ids = STANDARD_ATTR_IDS.join(",");
  out.push({
    name: "spec-anchor-coverage",
    pass: expected.every((e) => ids.includes(e)),
    detail: `六属性 id 覆盖：${ids}`,
  });

  // 每行类型/字节/分量/单位/默认值/钳制域六项齐备（缺项即为规格表残缺）。
  let rowsComplete = true;
  const incomplete: string[] = [];
  for (const s of STANDARD_ATTR_SPECS) {
    const components = SCALAR_COMPONENTS[s.type];
    const complete =
      s.id.length > 0 &&
      s.name.length > 0 &&
      s.bytes === SCALAR_BYTES[s.type] &&
      s.unit.length > 0 &&
      s.semantic.length > 0 &&
      s.defaultValue.length === components &&
      s.clamp !== undefined;
    if (!complete) {
      rowsComplete = false;
      incomplete.push(s.id);
    }
  }
  out.push({
    name: "spec-rows-complete",
    pass: rowsComplete,
    detail: rowsComplete ? `六行规格六项齐备（id/名/字节/单位/语义/默认值/钳制域）` : `规格行残缺：${incomplete.join("、")}`,
  });

  const bag = new DiagBag();
  const audit = auditAttrSpecs(bag);
  out.push({
    name: "spec-table-self-consistent",
    pass: audit.ok,
    detail: audit.ok ? "规格表逐行自审通过" : `规格表自审失败（${audit.code}：${audit.message}）`,
  });

  // 扩展槽注册项四样齐备 + 未注册即拒。
  out.push({
    name: "custom-attr-registry-nonempty",
    pass: CUSTOM_ATTR_REGISTRY.length >= 1,
    detail: `注册扩展槽 ${CUSTOM_ATTR_REGISTRY.length} 项：${CUSTOM_ATTR_NAMES.join("、")}`,
  });

  const unregistered = deriveSoALayout(["no-such-attr"]);
  out.push({
    name: "custom-attr-unregistered-rejected",
    pass: !unregistered.ok && unregistered.code === "CUSTOM_ATTR_UNREGISTERED",
    detail: unregistered.ok ? "未注册属性名未被拒绝（stride 将不可预计算）" : `未注册属性被拒（${unregistered.code}）`,
  });

  // 正例：已注册属性须放行。
  const registered = deriveSoALayout(["seed"]);
  out.push({
    name: "custom-attr-registered-accepted",
    pass: registered.ok,
    detail: registered.ok ? "已注册属性 seed 放行" : `已注册属性被误拒（${registered.code}）`,
  });

  return out;
}

/** 判据二：SoA 单源 + 对齐红线。 */
export function selfCheckLayout(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const layout = baselineLayout();

  if (!layout.ok) {
    out.push({
      name: "layout-baseline-derivable",
      pass: false,
      detail: `基线布局推导失败（${layout.code}：${layout.message}）`,
    });
    return out;
  }
  const L = layout.value;

  out.push({
    name: "layout-baseline-derivable",
    pass: true,
    detail: `基线布局推导成功：${L.arrays.length} 个数组，stride=${L.stride} 字节`,
  });

  out.push({
    name: "layout-stride-aligned",
    pass: L.stride % SOA_ALIGN_BYTES === 0,
    detail: `stride=${L.stride}，${SOA_ALIGN_BYTES} 的倍数=${L.stride % SOA_ALIGN_BYTES === 0}`,
  });

  let allOffsetsAligned = true;
  for (const a of L.arrays) {
    if (a.offset % SOA_ALIGN_BYTES !== 0) allOffsetsAligned = false;
  }
  out.push({
    name: "layout-offsets-aligned",
    pass: allOffsetsAligned,
    detail: `全部 ${L.arrays.length} 个数组起始偏移守住 ${SOA_ALIGN_BYTES} 字节红线`,
  });

  // 单源保证：同输入必得同布局（推导是纯函数，无隐藏状态）。
  const again = baselineLayout();
  const deterministic = again.ok && layoutFingerprint(again.value) === layoutFingerprint(L);
  out.push({
    name: "layout-single-source-deterministic",
    pass: deterministic,
    detail: deterministic ? "两次推导指纹一致（布局确由规格表单源导出）" : "两次推导结果不同（存在隐藏状态或第二份布局声明）",
  });

  // 偏移必须单调不重叠（否则两个数组互相覆盖）。
  let monotone = true;
  for (let i = 1; i < L.arrays.length; i++) {
    const prev = L.arrays[i - 1];
    const cur = L.arrays[i];
    if (prev !== undefined && cur !== undefined && prev.offset + prev.bytesPerElement > cur.offset) {
      monotone = false;
    }
  }
  out.push({
    name: "layout-offsets-monotone-no-overlap",
    pass: monotone,
    detail: monotone ? "各数组字节区间首尾相接不重叠" : "存在数组区间重叠（属性会互相覆盖）",
  });

  // 负例：人 constructs一个未对齐布局须被对账钩子拦下。
  const misaligned: SoALayout = { ...L, stride: L.stride + 4 };
  const drift = reconcileLayout(L, misaligned);
  out.push({
    name: "layout-misaligned-rejected-by-reconcile",
    pass: !drift.ok,
    detail: drift.ok ? "未对齐布局被对账放行（SIMD 红线失守）" : `未对齐布局被拒（${drift.code}）`,
  });

  // 自定义属性改变 stride（证明扩展槽真的进入布局，不是摆设）。
  //
  // 比较基准取「零扩展」布局而非 baselineLayout()——后者按定义已含注册表全部
  // 扩展项，拿它当基准去比一个「只加两项」的布局，宽度必然更小，断言恒假。
  // 这里要证明的是：扩展槽进入布局 ⇒ stride 变宽。
  const bare = deriveSoALayout([]);
  const withExtra = deriveSoALayout(["material", "seed"]);
  const grows = bare.ok && withExtra.ok && withExtra.value.stride > bare.value.stride;
  out.push({
    name: "layout-custom-attrs-affect-stride",
    pass: grows,
    detail: bare.ok && withExtra.ok
      ? `零扩展 stride ${bare.value.stride} → 加两项扩展 ${withExtra.value.stride}（扩展槽确实进入布局）`
      : "加扩展属性后布局推导失败",
  });

  // 基线布局必须等于「注册表全部扩展」的推导结果（证明 baselineLayout 没有偷工减料）。
  const allRegistered = deriveSoALayout(CUSTOM_ATTR_NAMES);
  const baselineIsFull = allRegistered.ok && layoutFingerprint(allRegistered.value) === layoutFingerprint(L);
  out.push({
    name: "layout-baseline-covers-full-registry",
    pass: baselineIsFull,
    detail: baselineIsFull
      ? `基线布局 = 注册表全展开（${L.arrays.length} 数组 / stride ${L.stride}）`
      : "基线布局与注册表全展开不一致（基线漏了扩展项）",
  });

  return out;
}

/** 判据三：内存预计算 O(1) + 三道门。 */
export function selfCheckPoolBudget(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const layout = baselineLayout();
  if (!layout.ok) {
    out.push({ name: "pool-baseline-available", pass: false, detail: `基线布局不可用（${layout.code}）` });
    return out;
  }
  const L = layout.value;

  // O(1) 预计算与乘法一致（不做循环正是本条的工程量声明）。
  const cap = 100_000;
  const expect = cap * L.stride;
  const got = estimatePoolBytes(cap, L.stride);
  out.push({
    name: "pool-bytes-o1-exact",
    pass: got === expect,
    detail: `容量 ${cap} × stride ${L.stride} = ${expect} 字节（O(1) 乘法，非循环累加）`,
  });

  // 预算内最大容量必须恰好装下（边界：不多不少）。
  const affordable = maxCapacityWithinBudget(expect, L.stride);
  out.push({
    name: "pool-max-capacity-exact-boundary",
    pass: affordable === cap,
    detail: `按 ${expect} 字节预算 ÷ stride ${L.stride} 得最大容量 ${affordable}（应为 ${cap}）`,
  });

  // 正例：默认池声明成功。
  const cpu = declareDefaultPool("cpu", L);
  out.push({
    name: "pool-default-cpu-declared",
    pass: cpu.ok,
    detail: cpu.ok
      ? `CPU 池默认 ${POOL_KIND_SPECS.cpu.defaultCapacity} 容量 → ${cpu.value.totalBytes} 字节`
      : `CPU 默认池声明失败（${cpu.code}）`,
  });

  const gpu = declareDefaultPool("gpu", L);
  out.push({
    name: "pool-default-gpu-declared",
    pass: gpu.ok,
    detail: gpu.ok
      ? `GPU 池默认 ${POOL_KIND_SPECS.gpu.defaultCapacity} 容量 → ${gpu.value.totalBytes} 字节`
      : `GPU 默认池声明失败（${gpu.code}）`,
  });

  // 负例一：超硬顶。
  const overHard = declarePool("cpu", POOL_KIND_SPECS.cpu.maxCapacity + 1, L, 0);
  out.push({
    name: "pool-over-hardcap-rejected",
    pass: !overHard.ok && overHard.code === "POOL_CAPACITY_INVALID",
    detail: overHard.ok ? "超硬顶容量未被拒" : `超硬顶被拒（${overHard.code}）`,
  });

  // 负例二：超预算（给足硬顶内的容量，但预算很小）。
  const tightBudget = L.stride * 1000;
  const overBudget = declarePool("cpu", 100_000, L, tightBudget);
  out.push({
    name: "pool-over-budget-rejected",
    pass: !overBudget.ok && overBudget.code === "POOL_CAPACITY_EXCEEDS_BUDGET",
    detail: overBudget.ok ? "超预算容量未被拒" : `超预算被拒（${overBudget.code}）`,
  });

  // 拒绝时必须给出可执行建议（预算内最大容量出现在 hint 里）。
  const hintHasSuggestion = !overBudget.ok && overBudget.hint.includes(String(maxCapacityWithinBudget(tightBudget, L.stride)));
  out.push({
    name: "pool-budget-rejection-gives-actionable-value",
    pass: hintHasSuggestion,
    detail: hintHasSuggestion
      ? `超预算拒绝的 hint 含可执行降级值 ${maxCapacityWithinBudget(tightBudget, L.stride)}`
      : "超预算拒绝未给出可执行的降容量建议",
  });

  // 负例三：非法容量。
  const badCap = declarePool("cpu", 0, L, 0);
  out.push({
    name: "pool-nonpositive-capacity-rejected",
    pass: !badCap.ok && badCap.code === "POOL_CAPACITY_INVALID",
    detail: badCap.ok ? "零容量未被拒" : `零容量被拒（${badCap.code}）`,
  });

  const fracCap = declarePool("cpu", 1000.5, L, 0);
  out.push({
    name: "pool-fractional-capacity-rejected",
    pass: !fracCap.ok && fracCap.code === "POOL_CAPACITY_INVALID",
    detail: fracCap.ok ? "小数容量未被拒" : `小数容量被拒（${fracCap.code}）`,
  });

  // 负例四：未对齐布局不得建池。
  const unaligned: SoALayout = { ...L, stride: L.stride + 4 };
  const poolOnUnaligned = declarePool("cpu", 1000, unaligned, 0);
  out.push({
    name: "pool-on-unaligned-layout-rejected",
    pass: !poolOnUnaligned.ok && poolOnUnaligned.code === "LAYOUT_ALIGNMENT_VIOLATION",
    detail: poolOnUnaligned.ok ? "未对齐布局建池被放行" : `未对齐布局建池被拒（${poolOnUnaligned.code}）`,
  });

  return out;
}

/** 判据四：布局版本化与对账。 */
export function selfCheckVersioning(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const layout = baselineLayout();
  if (!layout.ok) {
    out.push({ name: "versioning-baseline-available", pass: false, detail: `基线布局不可用（${layout.code}）` });
    return out;
  }
  const L = layout.value;

  // 指纹确定性（同内容必同指纹，不同内容必不同指纹）。
  const same = layoutFingerprint(L) === layoutFingerprint({ ...L, arrays: [...L.arrays] });
  const diff = layoutFingerprint(L) !== layoutFingerprint({ ...L, stride: L.stride + 16 });
  out.push({
    name: "versioning-fingerprint-deterministic",
    pass: same,
    detail: same ? `指纹稳定（${layoutFingerprint(L)}）` : "同内容两次指纹不同",
  });

  out.push({
    name: "versioning-fingerprint-sensitive",
    pass: diff,
    detail: diff ? "stride 变更被指纹捕获" : "stride 变更未改变指纹（对账失效）",
  });

  // 自比对不得误报（对账钩子最容易被「过度敏感」坑死）。
  const self = reconcileLayout(L, { ...L });
  out.push({
    name: "versioning-no-false-positive",
    pass: self.ok,
    detail: self.ok ? "同源布局自比对无误报" : `同源布局被误拒（${self.code}）`,
  });

  // 版本漂移必被检出。
  const badVersion = reconcileLayout(L, { ...L, layoutVersion: L.layoutVersion + 1 });
  out.push({
    name: "versioning-drift-detected",
    pass: !badVersion.ok,
    detail: badVersion.ok ? "版本漂移未被检出" : `版本漂移被检出（${badVersion.code}）`,
  });

  // 内容漂移（同版本不同内容）必被判为 needs-migration，不得判兼容。
  const compat = judgeLayoutCompatibility(L, { ...L, stride: L.stride + 16, layoutVersion: L.layoutVersion });
  out.push({
    name: "versioning-same-version-different-content-not-compatible",
    pass: compat === "needs-migration",
    detail: `同版本不同内容裁决 = ${compat}（须为 needs-migration，不得为 compatible）`,
  });

  const incompat = judgeLayoutCompatibility(L, { ...L, layoutVersion: L.layoutVersion + 1 });
  out.push({
    name: "versioning-cross-version-incompatible",
    pass: incompat === "incompatible",
    detail: `跨版本裁决 = ${incompat}`,
  });

  const sameCompat = judgeLayoutCompatibility(L, { ...L });
  out.push({
    name: "versioning-same-content-compatible",
    pass: sameCompat === "compatible",
    detail: `同内容裁决 = ${sameCompat}`,
  });

  // 逐字段对账：抽掉中间某个数组须被确诊（不只是指纹变化）。
  const missingArray: SoALayout = {
    ...L,
    arrays: [...L.arrays.slice(0, 2), ...L.arrays.slice(3)],
  };
  const drift = reconcileLayout(L, missingArray);
  out.push({
    name: "versioning-field-level-reconcile-catches-dropped-array",
    pass: !drift.ok && drift.code === "LAYOUT_DRIFT",
    detail: drift.ok ? "实现侧少一个数组未被确诊" : `少数组被确诊（${drift.code}）`,
  });

  // 多出数组同样须被拒。
  const extraArray: SoALayout = { ...L, arrays: [...L.arrays, { attrId: "rogue", type: "float", bytesPerElement: 4, offset: L.stride, standard: false }] };
  const extra = reconcileLayout(L, extraArray);
  out.push({
    name: "versioning-field-level-reconcile-catches-extra-array",
    pass: !extra.ok && extra.code === "LAYOUT_DRIFT",
    detail: extra.ok ? "实现侧多一个数组未被确诊" : `多数组被确诊（${extra.code}）`,
  });

  return out;
}

/**
 * 判据五：回归护栏——本条历次修复留下的常驻断言。
 *
 * 存在的理由：修复若只落在实现里而无断言，下次重构就会把它悄悄改回去。
 * 每项写死「必须被拒 + 必须报某码」，任何回归立即可见。
 */
export function selfCheckRegressions(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const layout = baselineLayout();
  if (!layout.ok) {
    out.push({ name: "regression-baseline-available", pass: false, detail: `基线布局不可用（${layout.code}）` });
    return out;
  }
  const L = layout.value;

  // R1 重复属性名必须被拒（两个数组争同一偏移会让池内存互相覆盖）。
  const dup = deriveSoALayout(["seed", "seed"]);
  out.push({
    name: "regression-duplicate-attr-rejected",
    pass: !dup.ok && dup.code === "ATTR_ID_DUPLICATE",
    detail: dup.ok ? "重复属性名被放行（偏移互相覆盖）" : `重复属性名被拒（${dup.code}）`,
  });

  // R2 自定义属性撞标准属性名必须被拒，且报**专属**码而非泛化的未注册
  //（报未注册会让处方退化成「去注册」，而注册它恰恰会制造冲突）。
  const collide = deriveSoALayout(["position"]);
  out.push({
    name: "regression-custom-collides-standard-rejected",
    pass: !collide.ok && collide.code === "CUSTOM_ATTR_SPEC_INCONSISTENT",
    detail: collide.ok
      ? "扩展属性与标准属性同名被放行"
      : `同名撞车被精确拒（${collide.code}${collide.code === "CUSTOM_ATTR_UNREGISTERED" ? " ← 码退化为未注册，处方会反向引导去注册" : ""}）`,
  });

  // R3 预算门必须真的生效：预算=0 表示不限，须与「预算充足」区分清楚。
  const zeroBudget = declarePool("cpu", POOL_KIND_SPECS.cpu.maxCapacity, L, 0);
  out.push({
    name: "regression-zero-budget-means-unlimited",
    pass: zeroBudget.ok,
    detail: zeroBudget.ok
      ? `预算 0 视为不限，硬顶内最大容量 ${POOL_KIND_SPECS.cpu.maxCapacity} 放行`
      : `预算 0 被误判为超预算（${zeroBudget.code}）`,
  });

  // R4 两池默认容量必须不同（容量是配额基数，同预算下 GPU 需更大容量）。
  out.push({
    name: "regression-two-pools-differ-in-default-capacity",
    pass: POOL_KIND_SPECS.cpu.defaultCapacity !== POOL_KIND_SPECS.gpu.defaultCapacity,
    detail: `CPU 默认 ${POOL_KIND_SPECS.cpu.defaultCapacity} ≠ GPU 默认 ${POOL_KIND_SPECS.gpu.defaultCapacity}`,
  });

  // R5 非法池种类必须被拒（不静默回落到某一行）。
  const badKind = declarePool("tpu" as never, 1000, L, 0);
  out.push({
    name: "regression-unknown-pool-kind-rejected",
    pass: !badKind.ok && badKind.code === "POOL_KIND_INVALID",
    detail: badKind.ok ? "未注册池种类被放行" : `未注册池种类被拒（${badKind.code}）`,
  });

  // R6 边界容量须放行（恰好等于硬顶 / 恰好等于预算内最大容量）。
  const atHard = declarePool("cpu", POOL_KIND_SPECS.cpu.maxCapacity, L, 0);
  const atBudget = declarePool("cpu", maxCapacityWithinBudget(L.stride * 7777, L.stride), L, L.stride * 7777);
  out.push({
    name: "regression-boundary-values-accepted",
    pass: atHard.ok && atBudget.ok,
    detail: `硬顶边界=${atHard.ok ? "放行" : `拒(${atHard.code})`}；预算边界=${atBudget.ok ? "放行" : `拒(${atBudget.code})`}`,
  });

  // R6b 默认预算必须容得下硬顶容量。
  //
  // 这条是被对抗验证逼出来的：两行规格的maxCapacity 与 defaultBudgetBytes
  // 曾互相矛盾（CPU 硬顶需 137MB 而预算只给 64MB），于是硬顶成了一句空话——
  // 按硬顶申请容量却在预算门被拒，报错还指向「降容量」，而它要的正是硬顶
  // 允许的量，形成「按文档配置必然被拒」的死结。故固化为断言。
  const budgetCoversHardcap: string[] = [];
  let budgetOk = true;
  for (const kind of ["cpu", "gpu"] as const) {
    const spec = POOL_KIND_SPECS[kind];
    if (spec.defaultBudgetBytes < spec.maxCapacity * L.stride) {
      budgetOk = false;
      budgetCoversHardcap.push(
        `${spec.kindName}: 硬顶需 ${spec.maxCapacity * L.stride} 字节 > 预算 ${spec.defaultBudgetBytes} 字节`,
      );
    }
  }
  out.push({
    name: "regression-budget-covers-hardcap",
    pass: budgetOk,
    detail: budgetOk
      ? `两池默认预算均容得下硬顶容量（CPU ${POOL_KIND_SPECS.cpu.maxCapacity * L.stride}≤${POOL_KIND_SPECS.cpu.defaultBudgetBytes}；GPU ${POOL_KIND_SPECS.gpu.maxCapacity * L.stride}≤${POOL_KIND_SPECS.gpu.defaultBudgetBytes}）`
      : budgetCoversHardcap.join("；"),
  });

  // R6c 默认容量必须在默认预算内（否则「按默认配置建池」直接失败）。
  const defaultFits: string[] = [];
  let defaultOk = true;
  for (const kind of ["cpu", "gpu"] as const) {
    const spec = POOL_KIND_SPECS[kind];
    if (spec.defaultCapacity * L.stride > spec.defaultBudgetBytes) {
      defaultOk = false;
      defaultFits.push(`${spec.kindName}: 默认 ${spec.defaultCapacity * L.stride} > 预算 ${spec.defaultBudgetBytes}`);
    }
  }
  out.push({
    name: "regression-default-capacity-within-default-budget",
    pass: defaultOk,
    detail: defaultOk ? "两池默认容量均在默认预算内" : defaultFits.join("；"),
  });

  // R7 对齐辅助函数本身正确（含「已对齐则原样返回」这条容易被写成多加了16 的分支）。
  out.push({
    name: "regression-align-up-correct",
    pass: alignUp(0, 16) === 0 && alignUp(1, 16) === 16 && alignUp(16, 16) === 16 && alignUp(17, 16) === 32,
    detail: `alignUp: 0→${alignUp(0, 16)} 1→${alignUp(1, 16)} 16→${alignUp(16, 16)} 17→${alignUp(17, 16)}`,
  });

  // R8 内存预计算不得溢出为负（大容量 × stride 须仍为有限正数）。
  const big = estimatePoolBytes(4_000_000, L.stride);
  out.push({
    name: "regression-estimate-no-overflow",
    pass: Number.isFinite(big) && big > 0,
    detail: `最大池 ${estimatePoolBytes(4_000_000, L.stride)} 字节为有限正数`,
  });

  return out;
}

/** 汇总全部判据自检。 */
export function runSelfCheck(): { readonly checks: readonly SelfCheck[]; readonly allPass: boolean } {
  const checks: SelfCheck[] = [
    ...selfCheckAttrSpecs(),
    ...selfCheckLayout(),
    ...selfCheckPoolBudget(),
    ...selfCheckVersioning(),
    ...selfCheckRegressions(),
  ];
  const allPass = checks.every((c) => c.pass);
  return { checks, allPass };
}