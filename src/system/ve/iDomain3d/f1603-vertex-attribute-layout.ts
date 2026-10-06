/**
 * VE-F1603 · 顶点属性布局系统（I 域 · 3D 管线域 · L0 几何数据层 · 批次 I01）
 * ---------------------------------------------------------------------------
 * 职责定位：顶点属性的**声明制布局** —— 语义、格式、偏移三者逐属性显式声明，
 *布局即契约。上游 F1601（I 域总架构）登记本条产出为 `VertexLayout`；
 * 下游 F1604（索引与拓扑）、F1605（网格量化）、F1606（法线切线）、
 * F1612（几何校验器）、F1615（几何 fuzz）全部以本布局为输入契约。
 *
 * 本条的四条判据与实现段落一一对应：
 *   判据一「声明制」   → §2 无隐式布局：属性必须自带偏移、stride 必须自带且与声明自洽；
 *                       布局经layoutHash 寻址，并可投影为 Vulkan/D3D11/D3D12/Metal
 *                       四后端的顶点输入描述 —— 同一份声明在四后端产出同序同偏移，
 *                       这是「跨后端一致」的可执行形态而非口号。
 *   判据二「打包权衡」 → §3 fp32 / fp16 / SNORM(8|16) / UNORM(8|16) 三类打包格式，
 *                       每「属性 × 格式」给出量化误差上界、绝对误差与相对 f32 的
 *                       内存比，构成一张**可查询的权衡表**（权衡透明）。
 *   判据三「三查」     → §4 语义重复 / 区间越界与重叠 / 对齐违规，三类独立查、
 *                       一次性报全部（不挤牙膏式「修一个报一个」）。
 *   判据四「自动建议」 → §5 按网格用途（静态道具/蒙皮角色/地形/粒子/UI 精灵）推荐
 *                       **最小合法布局**：按对齐降序紧凑排布使padding 最小，
 *                       并给出相对「全 f32 布局」的字节节省率（内存优化的量化）。
 *
 * 边界声明（不扩面，与 F1601 跨域边界表一致）：
 *   · 本条只管「顶点字节怎么排」，不管数据怎么来（顶点数据生成归 F1606）、
 *     不管怎么压（量化编解码归 F1605，本条只给格式与误差契约供其对拍）、
 *     不管怎么连（索引与拓扑归 F1604）、不管怎么检越界索引（恶意网格三查归 F1612）、
 *     不管怎么抽 LOD（归 F1608）、不碰着色器编译（C 域）。
 *   · 本条不 import F1602，而是自包含：语义名、格式名、分量数三处事实源与
 *     F1602逐字对齐并在 §1 显式声明该对齐关系。跨条运行时耦合会让「改一格式名
 *     要同时改两个文件」成为常态，而容器格式与布局规则的演进节奏本就不同。
 *
 * 零静默纪律：所有失败路径产出 Diagnostic（code + message + hint 三要素齐备），
 * 不抛异常、不吞诊断、不返回「尽力拼出的半个布局」。布局错误的后果是**静默的几何
 * 错乱**（读到相邻顶点的数据、法线翻面、UV 错位），比明确报错危害大得多。
 *
 * 判据：声明制、打包权衡、三查、自动建议、判据。
 * 落位：src/system/ve/iDomain3d/
 */

// ════════════════════════════════════════════════════════════════════════════
// §0 诊断基础设施（零静默第一层；与 F1602 同纪律，此处自包含不跨条import）
// ════════════════════════════════════════════════════════════════════════════

/** 诊断码：每种失败独立可检索，绝不合并为一条通用错误。 */
export type DiagCode =
  /** 语义不在闭集内（外来布局里出现本实现不认识的语义）。 */
  | "SEMANTIC_UNKNOWN"
  /** 同一语义被声明了两次（两条声明抢同一份顶点数据）。 */
  | "SEMANTIC_DUPLICATED"
  /** 格式不在闭集内（如 fp64、fp8 等本条未支持的格式）。 */
  | "FORMAT_UNKNOWN"
  /** 该格式与该语义不兼容（如 POSITION 用 SNORM8、权重用 SNORM）。 */
  | "FORMAT_NOT_ALLOWED_FOR_SEMANTIC"
  /** 偏移为负或非整数。 */
  | "OFFSET_INVALID"
  /** 属性区间越出每顶点步长（读到下一个顶点的数据）。 */
  | "LAYOUT_OUT_OF_BOUNDS"
  /** 两个属性的字节区间重叠（同一份字节被两个语义认领）。 */
  | "LAYOUT_OVERLAP"
  /** 属性偏移或步长违反对齐要求（GPU 按对齐地址取数，违反对齐读出垃圾值）。 */
  | "LAYOUT_ALIGNMENT_VIOLATION"
  /** stride 与「最大末尾偏移」不自洽（声明的步长装不下声明的属性）。 */
  | "STRIDE_MISMATCH"
  /** 布局未声明 POSITION（无位置不成网格）。 */
  | "LAYOUT_MISSING_POSITION"
  /** 归一化标志与格式不自洽（浮点格式标normalized，或整型格式漏标）。 */
  | "NORMALIZED_FLAG_INCONSISTENT"
  /** 用途档案引用了不存在的语义名。 */
  | "USAGE_PROFILE_INVALID"
  /** 量化往返超出了该格式声明的误差上界（打包实现与误差契约脱节）。 */
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

/** 成功构造（diagnostics 承载非致命告警，如精度档位偏激进）。 */
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
// §1 事实源常量（语义 / 格式 / 分量数 / 用途）与 F1602 的对齐声明
// ════════════════════════════════════════════════════════════════════════════

/**
 * 顶点属性语义闭集。
 *
 * 与 F1602 `ALL_SEMANTICS` 逐项对齐（POSITION/NORMAL/TANGENT/UV0/UV1/COLOR/BONE_WEIGHT），
 * 容器存头与布局消费必须用同一套名字——名字分叉会让「头里写的 UV0 在布局里找不到」。
 * 此处独立声明而非 import，是为了让两条可以各自演进（容器格式加了新语义而布局规则
 * 尚未跟进时，两边各自判断比互相阻塞更安全）；代价由§1 的对齐声明与自检兜底。
 */
export const ALL_SEMANTICS = [
  "POSITION",
  "NORMAL",
  "TANGENT",
  "UV0",
  "UV1",
  "COLOR",
  "BONE_WEIGHT",
] as const;

/** 顶点属性语义。 */
export type AttributeSemantic = (typeof ALL_SEMANTICS)[number];

/** 每个语义的分量数（TANGENT 的 w 是手性、BONE_WEIGHT 是 4 组骨骼权重）。 */
export const SEMANTIC_COMPONENTS: Readonly<Record<AttributeSemantic, number>> = {
  POSITION: 3,
  NORMAL: 3,
  TANGENT: 4,
  UV0: 2,
  UV1: 2,
  COLOR: 4,
  BONE_WEIGHT: 4,
};

/**
 * 语义用途分类（自动建议的输入：用途决定「需要哪些语义」）。
 *
 * 分类的工程意义：布局臃肿的根因往往不是「格式没压」，而是「带了用不上的属性」
 * （静态道具带骨骼权重 = 每顶点白付 8 字节× 百万顶点）。按用途裁剪属性集合
 * 比逐属性压格式的收益大得多。
 */
export const SEMANTIC_PURPOSE: Readonly<Record<AttributeSemantic, "GEOMETRY" | "SHADING" | "TEXTURE" | "COLOR" | "SKINNING">> = {
  POSITION: "GEOMETRY",
  NORMAL: "SHADING",
  TANGENT: "SHADING",
  UV0: "TEXTURE",
  UV1: "TEXTURE",
  COLOR: "COLOR",
  BONE_WEIGHT: "SKINNING",
};

/**
 * 分量格式闭集。
 *
 * f32/f16 为浮点（存实数值本身）；snorm8/snorm16 为有符号归一化（解码后落在 [-1,1]，
 * 法线/切线方向类属性的标准做法）；u8/u16 为无符号归一化（解码后落在 [0,1]，
 * UV/颜色/权重类属性的标准做法）。
 */
export const ALL_FORMATS = ["f32", "f16", "snorm8", "snorm16", "u8", "u16"] as const;

/** 分量格式。 */
export type ComponentFormat = (typeof ALL_FORMATS)[number];

/** 每分量字节数。 */
export const FORMAT_BYTES: Readonly<Record<ComponentFormat, number>> = {
  f32: 4,
  f16: 2,
  snorm8: 1,
  snorm16: 2,
  u8: 1,
  u16: 2,
};

/** 每分量自然对齐字节数（GPU 访存粒度；2字节格式要求 2 对齐，4字节要求 4 对齐）。 */
export const FORMAT_ALIGNMENT: Readonly<Record<ComponentFormat, number>> = {
  f32: 4,
  f16: 2,
  snorm8: 1,
  snorm16: 2,
  u8: 1,
  u16: 2,
};

/** 该格式是否为整数存储（读出时须按归一化规则还原，故 normalized 必为 true）。 */
export function isIntegerFormat(fmt: ComponentFormat): boolean {
  return fmt === "snorm8" || fmt === "snorm16" || fmt === "u8" || fmt === "u16";
}

/** 该格式解码后的值域半宽（量化误差换算成绝对误差的基数）。 */
export function valueRangeHalfWidth(fmt: ComponentFormat): number {
  if (fmt === "u8" || fmt === "u16") return 0.5;
  if (isIntegerFormat(fmt)) return 1;
  //浮点格式不在量化区间内，半宽记为 1（相对误差路径，不参与绝对误差换算）。
  return 1;
}

/** 该格式的归一化除数（u8/u16→[0,1]，snorm→[-1,1]，浮点为 1）。 */
export function normalizationScale(fmt: ComponentFormat): number {
  if (fmt === "u8") return 255;
  if (fmt === "u16") return 32767 * 2 + 1;
  if (fmt === "snorm8") return 127;
  if (fmt === "snorm16") return 32767;
  return 1;
}

/**
 * 各格式的量化误差上界（满值程内的最大偏差）。
 *
 * f32 记为 0 而非 Number.EPSILON：浮点路径不做量化，误差来自运算而非存储，
 * 用存储误差上界描述它会误导权衡表（让人以为 f32 也有 1e-7 级的存储误差）。
 * 真正的浮点误差由 F1605 的 fp32→fp16 转换路径单独度量。
 */
export const FORMAT_MAX_ERROR: Readonly<Record<ComponentFormat, number>> = {
  f32: 0,
  f16: 2 ** -11,
  snorm8: 1 / 127,
  snorm16: 1 / 32767,
  u8: 1 / 255,
  u16: 1 / 65535,
};

/** 格式族（打包权衡表的第一层分类）。 */
export type FormatFamily = "FLOAT32" | "FLOAT16" | "SNORM" | "UNORM";

/** 格式 → 格式族。 */
export const FORMAT_FAMILY: Readonly<Record<ComponentFormat, FormatFamily>> = {
  f32: "FLOAT32",
  f16: "FLOAT16",
  snorm8: "SNORM",
  snorm16: "SNORM",
  u8: "UNORM",
  u16: "UNORM",
};

/** 格式族的人话说明（权衡表的「格式」列展示用）。 */
export const FORMAT_FAMILY_LABEL: Readonly<Record<FormatFamily, string>> = {
  FLOAT32: "全精度浮点（无量化误差，4 字节/分量）",
  FLOAT16: "半精度浮点（相对误差 2^-11，2 字节/分量）",
  SNORM: "有符号归一化（解码落 [-1,1]，1~2 字节/分量）",
  UNORM: "无符号归一化（解码落 [0,1]，1~2 字节/分量）",
};

/**
 * 语义×格式 合法矩阵：哪些格式对该语义在**语义上**成立。
 *
 * 这不是「GPU 能不能读」的硬件问题，而是「读出来有没有意义」的问题：
 *   · POSITION禁用 SNORM8 ——位置值域由模型决定（可达1e3），归一化到 [-1,1]
 *     的误差 1/127 在 1e3 量级下是 7.9 个单位，几何直接崩。半精度 fp16 可以
 *     用，但前提是位置已归一化到局部坐标系（F1605 的前提，本条不负责归一化）。
 *   · BONE_WEIGHT 禁用 SNORM —— 权重非负，SNORM 的负半区永远用不上，
 *     却要付「负权重可表达」的错误语义（负权重会让蒙皮把顶点拉向反方向）。
 *   · BONE_WEIGHT 禁用 u8 —— 4 组权重求和须为 1，每组 1/255≈0.0039 的步长在
 *     权重量级上偏粗（骨骼数多时权重分布误差可见），故只留 f32/f16/u16。
 */
export const ALLOWED_FORMATS: Readonly<Record<AttributeSemantic, readonly ComponentFormat[]>> = {
  POSITION: ["f32", "f16"],
  NORMAL: ["f32", "f16", "snorm16", "snorm8"],
  TANGENT: ["f32", "f16", "snorm16", "snorm8"],
  UV0: ["f32", "f16", "u16", "u8"],
  UV1: ["f32", "f16", "u16", "u8"],
  COLOR: ["f32", "f16", "snorm16", "snorm8", "u16", "u8"],
  BONE_WEIGHT: ["f32", "f16", "u16"],
};

/** 该格式对该语义是否合法（合法矩阵查询）。 */
export function isFormatAllowed(sem: AttributeSemantic, fmt: ComponentFormat): boolean {
  const allowed = ALLOWED_FORMATS[sem];
  return allowed !== undefined && allowed.includes(fmt);
}

/**
 * 语义值域声明（绝对误差换算的基数）。
 *
 * POSITION 记半宽 1：其语义前提是「已归一化到局部坐标系」——这正是 F1605 的
 * 量化前提。本条不校验归一化是否真做了（那是 F1605/F1612 的职责），但布局层必须
 * 声明自己在按「归一化值域」计算误差，否则权衡表会给出乐观到有害的结论。
 */
export const SEMANTIC_RANGE_HALF_WIDTH: Readonly<Record<AttributeSemantic, number>> = {
  POSITION: 1,
  NORMAL: 1,
  TANGENT: 1,
  UV0: 0.5,
  UV1: 0.5,
  COLOR: 0.5,
  BONE_WEIGHT: 0.5,
};

/** 语义人话名（诊断与权衡表展示用，与语义名同源，避免两套中文对不上）。 */
export const SEMANTIC_LABEL: Readonly<Record<AttributeSemantic, string>> = {
  POSITION: "位置",
  NORMAL: "法线",
  TANGENT: "切线（含手性w）",
  UV0: "主UV 坐标",
  UV1: "第二套 UV 坐标",
  COLOR: "顶点色",
  BONE_WEIGHT: "骨骼权重",
};

// ════════════════════════════════════════════════════════════════════════════
// §2 声明制模型（判据一：无隐式布局 + 声明即契约 + 跨后端一致）
// ════════════════════════════════════════════════════════════════════════════

/** 一条属性声明：三元组「语义 + 格式 + 偏移」齐备，另带归一化标志。 */
export interface AttributeDecl {
  readonly semantic: AttributeSemantic;
  readonly format: ComponentFormat;
  /** 该属性首字节在单个顶点内的偏移。显式必填——本条不接受「按顺序自动排」。 */
  readonly byteOffset: number;
  /** 归一化标志。整数格式必须为 true，浮点格式必须为 false。 */
  readonly normalized: boolean;
}

/**
 * 顶点布局：一份**自描述**的顶点字节契约。
 *
 * 「声明即契约」的具体含义：stride 与每个属性的偏移都是显式字段，本模块在消费时
 * 只做校验与投影，不做补全。任何「读不出来就按默认排」的兜底逻辑都会把布局错误
 * 变成静默的几何错乱，故本模块不存在这种路径。
 */
export interface VertexLayout {
  /** 每顶点字节步长。显式必填且必须与属性声明自洽（见 §4 STRIDE_MISMATCH）。 */
  readonly strideBytes: number;
  readonly attributes: readonly AttributeDecl[];
  /** 布局指纹：布局内容的稳定摘要，用作 PSO 缓存键与跨版本一致性凭据。 */
  readonly layoutHash: string;
}

/** 单个属性在布局中的解析结果（校验通过后的展开视图，含字节宽度）。 */
export interface ResolvedAttribute {
  readonly semantic: AttributeSemantic;
  readonly format: ComponentFormat;
  readonly byteOffset: number;
  /** 该属性总字节宽（分量数 × 每分量字节）。 */
  readonly byteLength: number;
  /** 该属性要求的最小对齐。 */
  readonly alignment: number;
  /** 声明序号（跨后端投影时作为 location 序号，保持四后端同序）。 */
  readonly location: number;
}

/** 从声明推导的完整布局视图。 */
export interface LayoutView {
  readonly layout: VertexLayout;
  readonly resolved: readonly ResolvedAttribute[];
  /** 全部属性的最大末尾偏移（stride 的下界）。 */
  readonly extentBytes: number;
  /** stride - extentBytes：尾部填充字节数。 */
  readonly tailPaddingBytes: number;
}

/** 取属性的总字节宽。 */
export function attributeByteLength(a: AttributeDecl): number {
  return FORMAT_BYTES[a.format] * SEMANTIC_COMPONENTS[a.semantic];
}

/** 取属性的最小对齐。 */
export function attributeAlignment(a: AttributeDecl): number {
  return FORMAT_ALIGNMENT[a.format];
}

/**
 * 声明布局：**不做任何隐式补全**。
 *
 * 三条硬规则：
 *   ① 属性偏移必须自带且为非负整数 —— 不按声明顺序自动排布（自动排布就是隐式布局，
 *      它让「改一个属性的顺序就得重算所有偏移」，且第三方无法独立解析）。
 *   ② stride 必须自带 —— stride 是「下一个顶点从哪开始」的唯一事实，漏了它
 *      任何消费方都得自己猜，而不同消费方猜法可能不同（有人补到 4、有人补到 16）。
 *   ③ stride 必须 ≥ 所有属性的最大末尾偏移 —— 由 §4 校验，此处只做结构完整性检查。
 */
export function declareLayout(strideBytes: number, attributes: readonly AttributeDecl[]): Outcome<LayoutView> {
  if (attributes.length === 0) {
    return fail(
      "LAYOUT_MISSING_POSITION",
      "布局未声明任何属性",
      "顶点布局至少须含 POSITION；空布局无法表达任何几何，请检查布局构造路径",
    );
  }
  if (!Number.isInteger(strideBytes) || strideBytes <= 0) {
    return fail(
      "STRIDE_MISMATCH",
      `strideBytes 非法（${String(strideBytes)}）`,
      "步长须为正整数；0或负值会让所有属性偏移越界",
    );
  }
  const view = buildView(strideBytes, attributes);
  return ok(view);
}

/** 由声明组装 LayoutView（不做语义级校验——校验是 §4 的职责，两步分离便于定位）。 */
function buildView(strideBytes: number, attributes: readonly AttributeDecl[]): LayoutView {
  const resolved: ResolvedAttribute[] = [];
  let extent = 0;
  for (let i = 0; i < attributes.length; i += 1) {
    const a = attributes[i];
    if (a === undefined) continue;
    const byteLength = attributeByteLength(a);
    extent = Math.max(extent, a.byteOffset + byteLength);
    resolved.push({
      semantic: a.semantic,
      format: a.format,
      byteOffset: a.byteOffset,
      byteLength,
      alignment: attributeAlignment(a),
      location: resolved.length,
    });
  }
  const layout: VertexLayout = {
    strideBytes,
    attributes,
    layoutHash: computeLayoutHash(strideBytes, attributes),
  };
  return { layout, resolved, extentBytes: extent, tailPaddingBytes: Math.max(0, strideBytes - extent) };
}

/**
 * 布局指纹（FNV-1a 32 位）：布局内容的稳定摘要。
 *
 * 用途不是安全（不防篡改），而是**寻址**：PSO 缓存按布局命中、资产版本对账按布局
 * 区分、跨进程传递布局只传 8 个字符而非整张声明表。
 * 输入取「语义+格式+偏移+stride」的规范化串，故属性**声明顺序不影响指纹**——
 * 这是刻意的：同序无关意味着「仅重排属性」不会被误判为布局变更。
 */
export function computeLayoutHash(strideBytes: number, attributes: readonly AttributeDecl[]): string {
  const canonical = [...attributes]
    .map((a) => `${a.semantic}:${a.format}:${a.byteOffset}:${a.normalized ? 1 : 0}`)
    .sort()
    .join("|");
  let h = 0x811c9dc5;
  const text = `${strideBytes}#${canonical}`;
  for (let i = 0; i < text.length; i += 1) {
    h ^= text.charCodeAt(i);
    h = (h + ((h << 1) + (h << 4) + (h << 7) + (h << 8) + (h << 24))) >>> 0;
  }
  return (h >>> 0).toString(16).padStart(8, "0");
}

/** 支持投影的后端。 */
export type BackendId = "VULKAN" | "D3D11" | "D3D12" | "METAL" | "OPENGL";

/** 一个后端的一条顶点输入描述。 */
export interface BackendSlot {
  /** location / attrib 序号：与声明序号一致（判据一的「跨后端一致」锚点）。 */
  readonly location: number;
  /** 后端原生语义名（D3D11/OpenGL 用名字，Vulkan/Metal 用 location）。 */
  readonly semanticName: string;
  /** 后端原生格式串（如 DXGI_FORMAT_R32G32B32_FLOAT）。 */
  readonly formatToken: string;
  readonly byteOffset: number;
}

/** 后端顶点输入描述的完整投影结果。 */
export interface BackendVertexInput {
  readonly backend: BackendId;
  /** 输入槽（D3D11 的 InputSlot；其余后端为 0）。 */
  readonly inputSlot: number;
  readonly slots: readonly BackendSlot[];
  readonly strideBytes: number;
  readonly layoutHash: string;
}

/** D3D11 / OpenGL 风格的语义名（以 D3D 为蓝本，OpenGL 与之同名）。 */
const BACKEND_SEMANTIC_NAME: Readonly<Record<AttributeSemantic, string>> = {
  POSITION: "POSITION",
  NORMAL: "NORMAL",
  TANGENT: "TANGENT",
  UV0: "TEXCOORD",
  UV1: "TEXCOORD",
  COLOR: "COLOR",
  BONE_WEIGHT: "BLENDWEIGHT",
};

/** OpenGL 用语义名做区分时附加的序号（D3D 用 TEXCOORD0/TEXCOORD1 的下标后缀）。 */
const SEMANTIC_NAME_INDEX: Readonly<Record<AttributeSemantic, number>> = {
  POSITION: 0,
  NORMAL: 0,
  TANGENT: 0,
  UV0: 0,
  UV1: 1,
  COLOR: 0,
  BONE_WEIGHT: 0,
};

/** Vulkan 格式串（每分量类型 × 分量数）。 */
const VULKAN_FORMAT: Readonly<Record<ComponentFormat, readonly string[]>> = {
  f32: ["R32_SFLOAT", "R32G32_SFLOAT", "R32G32B32_SFLOAT", "R32G32B32A32_SFLOAT"],
  f16: ["R16_SFLOAT", "R16G16_SFLOAT", "R16G16B16_SFLOAT", "R16G16B16A16_SFLOAT"],
  snorm8: ["R8_SNORM", "R8G8_SNORM", "R8G8B8_SNORM", "R8G8B8A8_SNORM"],
  snorm16: ["R16_SNORM", "R16G16_SNORM", "R16G16B16_SNORM", "R16G16B16A16_SNORM"],
  u8: ["R8_UNORM", "R8G8_UNORM", "R8G8B8_UNORM", "R8G8B8A8_UNORM"],
  u16: ["R16_UNORM", "R16G16_UNORM", "R16G16B16_UNORM", "R16G16B16A16_UNORM"],
};

/** D3D11 DXGI 格式串。 */
const DXGI_FORMAT: Readonly<Record<ComponentFormat, readonly string[]>> = {
  f32: ["DXGI_FORMAT_R32_FLOAT", "DXGI_FORMAT_R32G32_FLOAT", "DXGI_FORMAT_R32G32B32_FLOAT", "DXGI_FORMAT_R32G32B32A32_FLOAT"],
  f16: ["DXGI_FORMAT_R16_FLOAT", "DXGI_FORMAT_R16G16_FLOAT", "DXGI_FORMAT_R16G16B16_FLOAT", "DXGI_FORMAT_R16G16B16A16_FLOAT"],
  snorm8: ["DXGI_FORMAT_R8_SNORM", "DXGI_FORMAT_R8G8_SNORM", "DXGI_FORMAT_R8G8B8_SNORM", "DXGI_FORMAT_R8G8B8A8_SNORM"],
  snorm16: ["DXGI_FORMAT_R16_SNORM", "DXGI_FORMAT_R16G16_SNORM", "DXGI_FORMAT_R16G16B16_SNORM", "DXGI_FORMAT_R16G16B16A16_SNORM"],
  u8: ["DXGI_FORMAT_R8_UNORM", "DXGI_FORMAT_R8G8_UNORM", "DXGI_FORMAT_R8G8B8_UNORM", "DXGI_FORMAT_R8G8B8A8_UNORM"],
  u16: ["DXGI_FORMAT_R16_UNORM", "DXGI_FORMAT_R16G16_UNORM", "DXGI_FORMAT_R16G16B16_UNORM", "DXGI_FORMAT_R16G16B16A16_UNORM"],
};

/** Metal 顶点格式串。 */
const METAL_FORMAT: Readonly<Record<ComponentFormat, readonly string[]>> = {
  f32: ["MTLVertexFormatFloat", "MTLVertexFormatFloat2", "MTLVertexFormatFloat3", "MTLVertexFormatFloat4"],
  f16: ["MTLVertexFormatHalf", "MTLVertexFormatHalf2", "MTLVertexFormatHalf3", "MTLVertexFormatHalf4"],
  snorm8: ["MTLVertexFormatChar", "MTLVertexFormatChar2", "MTLVertexFormatChar3", "MTLVertexFormatChar4"],
  snorm16: ["MTLVertexFormatShort", "MTLVertexFormatShort2", "MTLVertexFormatShort3", "MTLVertexFormatShort4"],
  u8: ["MTLVertexFormatUChar", "MTLVertexFormatUChar2", "MTLVertexFormatUChar3", "MTLVertexFormatUChar4"],
  u16: ["MTLVertexFormatUShort", "MTLVertexFormatUShort2", "MTLVertexFormatUShort3", "MTLVertexFormatUShort4"],
};

/** OpenGL 顶点格式 token（顶点attrib 指针的类型枚举名）。 */
const GL_FORMAT: Readonly<Record<ComponentFormat, readonly string[]>> = {
  f32: ["GL_FLOAT", "GL_FLOAT_VEC2", "GL_FLOAT_VEC3", "GL_FLOAT_VEC4"],
  f16: ["GL_HALF_FLOAT", "GL_HALF_FLOAT_VEC2", "GL_HALF_FLOAT_VEC3", "GL_HALF_FLOAT_VEC4"],
  snorm8: ["GL_BYTE", "GL_BYTE_VEC2", "GL_BYTE_VEC3", "GL_BYTE_VEC4"],
  snorm16: ["GL_SHORT", "GL_SHORT_VEC2", "GL_SHORT_VEC3", "GL_SHORT_VEC4"],
  u8: ["GL_UNSIGNED_BYTE", "GL_UNSIGNED_BYTE_VEC2", "GL_UNSIGNED_BYTE_VEC3", "GL_UNSIGNED_BYTE_VEC4"],
  u16: ["GL_UNSIGNED_SHORT", "GL_UNSIGNED_SHORT_VEC2", "GL_UNSIGNED_SHORT_VEC3", "GL_UNSIGNED_SHORT_VEC4"],
};

/** 取某后端的格式串表（未知后端返回空表，交由调用方诊断而非硬崩）。 */
function formatTableFor(backend: BackendId): Readonly<Record<ComponentFormat, readonly string[]>> | null {
  if (backend === "VULKAN") return VULKAN_FORMAT;
  if (backend === "D3D11" || backend === "D3D12") return DXGI_FORMAT;
  if (backend === "METAL") return METAL_FORMAT;
  if (backend === "OPENGL") return GL_FORMAT;
  return null;
}

/**
 * 投影为某后端的顶点输入描述——「跨后端一致」的可执行形态。
 *
 * 一致性由三点保证，且都能被断言检验：
 *   ① location 序号 = 声明序号（四后端共用同一顺序，不做后端特有的重排）；
 *   ② 字节偏移**原样透传**，投影层绝不重算——重算就等于把布局规则复制成四份，
 *      四份迟早会不一致（这正是「隐式布局跨后端失效」的经典成因）；
 *   ③ 格式串由后端枚举表查得，不做等价格式替换（如不把 f16 悄悄升为 f32）。
 *
 * D3D11/OpenGL 额外给语义名，因为这两个后端的着色器按名字绑定输入；Vulkan/D3D12/Metal
 * 按 location 绑定，语义名仅作调试可读性保留。
 */
export function projectToBackend(view: LayoutView, backend: BackendId): Outcome<BackendVertexInput> {
  const table = formatTableFor(backend);
  if (table === null) {
    return fail(
      "FORMAT_UNKNOWN",
      `后端 ${String(backend)} 无格式映射表`,
      `支持后端：VULKAN、D3D11、D3D12、METAL、OPENGL；新增后端时须同时补齐其原生格式枚举表，`
        + "不能直接复用别后端的格式串（枚举值不同会静默读到错误数据）",
    );
  }
  const usesSemanticName = backend === "D3D11" || backend === "OPENGL";
  const slots: BackendSlot[] = [];
  for (const r of view.resolved) {
    const tokens = table[r.format];
    const token = tokens[SEMANTIC_COMPONENTS[r.semantic] - 1];
    if (token === undefined) {
      return fail(
        "FORMAT_UNKNOWN",
        `格式 ${r.format} 缺少 ${SEMANTIC_COMPONENTS[r.semantic]} 分量的后端格式串`,
        "后端格式表缺项；请补齐该格式该分量数的枚举串，不要退回单分量格式"
          + "（单分量会让 GPU 按错误宽度取数，且不报错）",
      );
    }
    const base = BACKEND_SEMANTIC_NAME[r.semantic];
    slots.push({
      location: r.location,
      semanticName: usesSemanticName ? (SEMANTIC_NAME_INDEX[r.semantic] > 0 ? `${base}${SEMANTIC_NAME_INDEX[r.semantic]}` : base) : base,
      formatToken: token,
      byteOffset: r.byteOffset,
    });
  }
  return ok({
    backend,
    // 输入槽固定为 0：本条只描述「单个顶点缓冲的顶点输入」这一种情形，
    // 多槽（instance buffer）属实例化语义，归 I08 批次而非布局层，故此处不开放该维度。
    inputSlot: 0,
    slots,
    strideBytes: view.layout.strideBytes,
    layoutHash: view.layout.layoutHash,
  });
}

/** 投影到全部五个后端（一致性断言的入口：任一后端投影失败即整体失败）。 */
export function projectToAllBackends(view: LayoutView): Outcome<readonly BackendVertexInput[]> {
  const ids: readonly BackendId[] = ["VULKAN", "D3D11", "D3D12", "METAL", "OPENGL"];
  const out: BackendVertexInput[] = [];
  const diags: Diagnostic[] = [];
  for (const id of ids) {
    const r = projectToBackend(view, id);
    if (r.ok) {
      out.push(r.value);
      diags.push(...r.diagnostics);
    } else {
      return fail(r.code, r.message, r.hint);
    }
  }
  return ok(out, diags);
}

// ════════════════════════════════════════════════════════════════════════════
// §3 打包格式与精度-内存权衡表（判据二：三格式可配 + 每属性每格式误差量化）
// ════════════════════════════════════════════════════════════════════════════

/** 一个「语义 × 格式」组合的权衡条目。 */
export interface PrecisionMemoryTradeoff {
  readonly semantic: AttributeSemantic;
  readonly format: ComponentFormat;
  readonly family: FormatFamily;
  /** 该属性在该格式下的字节宽。 */
  readonly bytes: number;
  /** 相对同语义 f32 的内存比（0.25 = 只用四分之一字节）。 */
  readonly memoryRatio: number;
  /** 满值程内的相对量化误差上界。 */
  readonly relativeError: number;
  /** 按语义值域换算的绝对误差上界（可直接与世界尺度比较，如「米」）。 */
  readonly absoluteError: number;
  /** 该组合是否合法（合法矩阵查询结果）。 */
  readonly allowed: boolean;
  /** 裁决：推荐 / 可接受 / 非法。 */
  readonly verdict: "RECOMMENDED" | "ACCEPTABLE" | "REJECTED";
  /** 人话理由（裁决的依据，展示给使用方而非只给一个标签）。 */
  readonly rationale: string;
}

/** 误差判优阈值：绝对误差大于该值（占语义值域的比例）即降级为「可接受」。 */
export const ERROR_WARN_RATIO = 0.01;

/**
 * 权衡表查询：某语义在某格式下的量化代价。
 *
 * 「权衡透明」的落地：不用「高精度/低精度」这种无法据以决策的形容词，而给三个
 * 可比数字——**绝对误差**（可与世界尺度比）、**内存比**（可与显存预算比）、
 * **合法性**（不可用时明确拒之）。三者齐备，调用方能自行决策而非依赖本条的推荐。
 */
export function tradeoffFor(sem: AttributeSemantic, fmt: ComponentFormat): PrecisionMemoryTradeoff {
  const comps = SEMANTIC_COMPONENTS[sem];
  const bytes = FORMAT_BYTES[fmt] * comps;
  const f32Bytes = FORMAT_BYTES.f32 * comps;
  const allowed = isFormatAllowed(sem, fmt);
  const half = SEMANTIC_RANGE_HALF_WIDTH[sem];
  const absoluteError = FORMAT_MAX_ERROR[fmt] * half;
  const relativeError = FORMAT_MAX_ERROR[fmt];
  const memoryRatio = bytes / f32Bytes;
  const ratio = absoluteError / half;

  let verdict: PrecisionMemoryTradeoff["verdict"];
  let rationale: string;
  if (!allowed) {
    verdict = "REJECTED";
    rationale = `${SEMANTIC_LABEL[sem]}不使用 ${FORMAT_FAMILY_LABEL[FORMAT_FAMILY[fmt]]}：语义值域与该格式的解码区间不匹配，写进去读出来就是错值`;
  } else if (ratio <= ERROR_WARN_RATIO) {
    verdict = "RECOMMENDED";
    rationale = `绝对误差 ${absoluteError.toExponential(3)}（占值域 ${(ratio * 100).toFixed(4)}%），落在人眼不可辨区间，可放心用于该属性`;
  } else {
    verdict = "ACCEPTABLE";
    rationale = `绝对误差 ${absoluteError.toExponential(3)}（占值域 ${(ratio * 100).toFixed(3)}%）已超${ERROR_WARN_RATIO * 100}%，`
      + "近景可见；仅在对精度不敏感的场景（远景LOD、粒子）使用，并配断言监控";
  }
  return { semantic: sem, format: fmt, family: FORMAT_FAMILY[fmt], bytes, memoryRatio, relativeError, absoluteError, allowed, verdict, rationale };
}

/** 某语义的全部格式权衡（按字节升序——内存最小的排在最前，便于选型）。 */
export function tradeoffTable(sem: AttributeSemantic): readonly PrecisionMemoryTradeoff[] {
  return ALL_FORMATS.map((f) => tradeoffFor(sem, f)).sort((a, b) => a.bytes - b.bytes || a.format.localeCompare(b.format));
}

/** 全表（语义 × 格式的完整矩阵，导出为文档/报告用）。 */
export function fullTradeoffMatrix(): readonly PrecisionMemoryTradeoff[] {
  const out: PrecisionMemoryTradeoff[] = [];
  for (const s of ALL_SEMANTICS) out.push(...tradeoffTable(s));
  return out;
}

// ── 量化编解码原语（供F1605 量化实现对拍，本条提供契约与可执行参考） ────────

/** 单分量打包结果：浮点格式存IEEE-754 位型，整数格式存无符号位型。 */
export type PackedComponent = number;

/** IEEE-754 单精度 → 半精度位型（round-to-nearest-even，含次正规与溢出处理）。 */
export function float32ToFloat16Bits(value: number): number {
  if (Number.isNaN(value)) return 0x7e00;
  const sign = value < 0 || Object.is(value, -0) ? 0x8000 : 0;
  const v = Math.abs(value);
  if (!Number.isFinite(v)) return sign | 0x7c00;
  // 次正规下界：最小可表示半精度是 2^-24。
  if (v < 2 ** -24) return sign;
  // 溢出上界：最大可表示半精度略小于 65536，取 65520 以上的值统一饱和为 Inf。
  if (v >= 65520) return sign | 0x7c00;
  let e = Math.floor(Math.log2(v));
  // log2 在 2 的幂附近可能给出边界误差，修正一次即可（|v/2^e - 1| >= 1 说明 e 大了）。
  if (v / 2 ** e >= 2) e += 1;
  else if (v / 2 ** e < 1) e -= 1;
  let mant = Math.round((v / 2 ** e - 1) * 1024);
  if (mant === 1024) {
    mant = 0;
    e += 1;
  }
  if (e > 15) return sign | 0x7c00;
  if (e >= -14) return sign | ((e + 15) << 10) | mant;
  // 次正规区：直接按最小单位（2^-24）折算，量级很小故精度损失可接受。
  return sign | Math.round(v / 2 ** -24);
}

/** 半精度位型 → IEEE-754 单精度（float32ToFloat16Bits 的严格逆运算，除溢出饱和外）。 */
export function float16BitsToFloat32(bits: number): number {
  const h = bits & 0xffff;
  const sign = (h & 0x8000) !== 0 ? -1 : 1;
  const exp = (h & 0x7c00) >> 10;
  const mant = h & 0x03ff;
  if (exp === 0) return sign * 2 ** -14 * (mant / 1024);
  if (exp === 0x1f) return mant !== 0 ? Number.NaN : sign * Number.POSITIVE_INFINITY;
  return sign * 2 ** (exp - 15) * (1 + mant / 1024);
}

/** 打包单分量：把实数值按格式压成存储位型。 */
export function packComponent(value: number, fmt: ComponentFormat): PackedComponent {
  if (fmt === "f32") {
    const buf = new ArrayBuffer(4);
    new DataView(buf).setFloat32(0, value, true);
    return new DataView(buf).getUint32(0, true);
  }
  if (fmt === "f16") return float32ToFloat16Bits(value);
  if (fmt === "snorm8") return Math.round(clamp(value, -1, 1) * 127) & 0xff;
  if (fmt === "snorm16") return Math.round(clamp(value, -1, 1) * 32767) & 0xffff;
  if (fmt === "u8") return Math.round(clamp(value, 0, 1) * 255);
  return Math.round(clamp(value, 0, 1) * 65535);
}

/** 解包单分量：把存储位型还原成实数值。 */
export function unpackComponent(packed: PackedComponent, fmt: ComponentFormat): number {
  if (fmt === "f32") {
    const buf = new ArrayBuffer(4);
    new DataView(buf).setUint32(0, packed >>> 0, true);
    return new DataView(buf).getFloat32(0, true);
  }
  if (fmt === "f16") return float16BitsToFloat32(packed);
  if (fmt === "snorm8") {
    let v = packed & 0xff;
    if (v > 127) v -= 256;
    return v / 127;
  }
  if (fmt === "snorm16") {
    let v = packed & 0xffff;
    if (v > 32767) v -= 65536;
    return v / 32767;
  }
  if (fmt === "u8") return (packed & 0xff) / 255;
  return (packed & 0xffff) / 65535;
}

/** 数值截断到区间（NaN 归到下界——NaN 进量化必然产出垃圾值，不如显式压到边界）。 */
function clamp(v: number, lo: number, hi: number): number {
  if (Number.isNaN(v)) return lo;
  return v < lo ? lo : v > hi ? hi : v;
}

/**
 * 量化往返断言：实测「打包→解包」的误差是否落在声明的误差上界内。
 *
 * 这是权衡表的**自证机制**：权衡表宣称「snorm16 的绝对误差 ≤ 1/32767× 半宽」，
 * 那就必须能用同一套编解码路径实测出这个结论。若某格式的实测误差超出声明上界，
 * 说明量化实现与契约脱节——这正是 F1605 落地量化时最容易出错的地方（声明写 16 位
 * 精度，实际按 8 位算）。失败产出 QUANTIZATION_ERROR_EXCEEDED 而非静默放行。
 */
export function verifyQuantization(sem: AttributeSemantic, fmt: ComponentFormat): Outcome<PrecisionMemoryTradeoff> {
  const table = tradeoffFor(sem, fmt);
  if (!table.allowed) {
    return fail(
      "FORMAT_NOT_ALLOWED_FOR_SEMANTIC",
      `${SEMANTIC_LABEL[sem]} 与 ${fmt} 的组合非法，无法做量化往返`,
      `合法格式：${(ALLOWED_FORMATS[sem] ?? []).join("、")}；非法组合须先改布局设计，不要靠「先写着以后再说」引入错误值`,
    );
  }
  const half = SEMANTIC_RANGE_HALF_WIDTH[sem];
  const probes = buildProbeValues(fmt);
  let worst = 0;
  let worstAt = 0;
  for (const v of probes) {
    const round = unpackComponent(packComponent(v, fmt), fmt);
    // 浮点格式的误差来自转换本身（半精度才有），f32 路径恒等故误差为 0。
    const err = Math.abs(round - v);
    if (err > worst) {
      worst = err;
      worstAt = v;
    }
  }
  // 上界加一个相对松弛项：浮点舍入的边界情形可能落在上界的下一个可表示值上。
  const slack = fmt === "f32" || fmt === "f16" ? half * 2 ** -11 : half * FORMAT_MAX_ERROR[fmt];
  if (worst > slack) {
    return fail(
      "QUANTIZATION_ERROR_EXCEEDED",
      `${SEMANTIC_LABEL[sem]}@${fmt} 的实测最大往返误差 ${worst.toExponential(3)}（探针值 ${worstAt}）超出声明上界 ${slack.toExponential(3)}`,
      "量化编解码与误差契约脱节：请核对除数（snorm8 应为 127 而非 128、u8 应为 255 而非 256）"
        + "与饱和处理；这类偏差在小网格上不可见，到百万顶点级才显形",
    );
  }
  return ok({ ...table, absoluteError: worst });
}

/**
 * 误差探针集：取样于**格式自身的解码值域**而非语义值域。
 *
 * 这一区分是本函数存在的全部理由：u8/u16 是无符号归一化，解码值域是 [0,1]；
 * snorm8/snorm16 是 [-1,1]；f16/f32 是实数轴。若按语义半宽取样（UV/顶点色半宽为
 * 0.5）却不管格式是无符号，就会把 -0.5 喂给 u8——打包时被clamp 到 0，解包得 0，
 * 往返误差 0.5，于是每个UNORM 格式都被误判为「误差超标」。那是探针的错，不是
 * 量化器的错——真实数据里UV 不会是负数。
 *
 * 取样点覆盖：两端点、0、量化刻度上的点与刻度中点（最坏情形通常落在中点）。
 */
function buildProbeValues(fmt: ComponentFormat): readonly number[] {
  const unsigned = fmt === "u8" || fmt === "u16";
  const lo = unsigned ? 0 : -1;
  const hi = 1;
  // 固定样点也须按格式过滤：无符号格式不喂负值（那不是它的合法输入区间）。
  const fixed: number[] = unsigned ? [0, 0.25, 0.5, 0.75, 1] : [-1, -0.5, 0, 0.5, 1];
  const out: number[] = [...fixed];
  const steps = 64;
  for (let i = 0; i <= steps; i += 1) {
    out.push(lo + ((hi - lo) * i) / steps);
  }
  if (fmt === "f16") {
    // 半精度必须额外喂「不可精确表示」的点，否则整批样点都落在 f16 的精确区间内
    // （f16 有 11 位有效位，1/64 网格全可精确表示），实测误差恒为 0 ——
    // 那等于没验。1/3、0.1、π/8 这类无穷尾数才真正逼出舍入误差。
    out.push(1 / 3, -1 / 3, 0.1, -0.1, Math.PI / 8, Math.E / 10, 1 / 7, -1 / 9);
  }
  return out;
}

// ════════════════════════════════════════════════════════════════════════════
// §4 布局校验三查（判据三：语义重复 / 越界 / 对齐违规）
// ════════════════════════════════════════════════════════════════════════════

/** 一条校验结论。 */
export interface LayoutIssue {
  readonly code: DiagCode;
  readonly message: string;
  readonly hint: string;
  /** 涉事的属性序号（无关联属性时为 -1）。 */
  readonly attributeIndex: number;
}

/** 三查报告：按查分类聚合，一次报全部。 */
export interface LayoutValidationReport {
  /** 第一查：语义重复。 */
  readonly semanticDuplicates: readonly LayoutIssue[];
  /** 第二查：区间越界与重叠。 */
  readonly boundsIssues: readonly LayoutIssue[];
  /** 第三查：对齐违规。 */
  readonly alignmentIssues: readonly LayoutIssue[];
  /** 附加查：stride 自洽、POSITION 存在性、normalized 自洽（与三查同批报出）。 */
  readonly structuralIssues: readonly LayoutIssue[];
  readonly pass: boolean;
  /** 全部问题的人话汇总。 */
  readonly summary: string;
}

/** 声明的外部输入（校验入口）：允许外来布局带「本实现不认识的语义」，校验须逐项识别。 */
export interface LayoutCandidate {
  readonly strideBytes: number;
  readonly attributes: readonly AttributeDecl[];
}

/**
 * 布局校验三查。
 *
 * 为何是三查而不是「一个validate」：三类问题的**归属方不同**，混报会让定位失效——
 *   · 语义重复 → 布局构造方（同一语义声明两次，多半是漏了 location 概念）；
 *   · 越界/重叠 → 布局构造方与容器打包方（偏移算错或打包时stride 与头不符）；
 *   · 对齐违规 → 布局构造方（GPU 按对齐地址取数，违规静默读出垃圾）。
 * 分桶报告让每类问题都能直接路由到该修的地方。
 *
 * 一次性报全部而非「遇错即返」：布局错误常有连带（多属性同偏移= 重叠+可能对齐违规），
 * 逐个报会让人陷入「修一个冒一个」的循环。
 */
export function validateLayout(candidate: LayoutCandidate): LayoutValidationReport {
  const bag = new DiagBag();
  const attrs = candidate.attributes;
  const stride = candidate.strideBytes;
  const semanticDuplicates: LayoutIssue[] = [];
  const boundsIssues: LayoutIssue[] = [];
  const alignmentIssues: LayoutIssue[] = [];
  const structuralIssues: LayoutIssue[] = [];

  // ---- 结构先决条件 ----
  if (!Number.isInteger(stride) || stride <= 0) {
    structuralIssues.push({
      code: "STRIDE_MISMATCH",
      message: `strideBytes 非法（${String(stride)}）`,
      hint: "步长须为正整数；声明式布局不提供「未声明则按最紧排列」的兜底，请显式给出步长",
      attributeIndex: -1,
    });
  }

  // ---- 第一查：语义重复 ----
  const firstIndexOf = new Map<string, number>();
  for (let i = 0; i < attrs.length; i += 1) {
    const a = attrs[i];
    if (a === undefined) continue;
    if (!ALL_SEMANTICS.includes(a.semantic)) {
      structuralIssues.push({
        code: "SEMANTIC_UNKNOWN",
        message: `第 ${i} 条属性声明的语义 ${JSON.stringify(a.semantic)} 不在闭集内`,
        hint: `支持集：${ALL_SEMANTICS.join("、")}；外来布局出现未知语义时不要猜它占几个分量——`
          + "猜错会让整张布局的步长连锁出错，且不会有任何报错",
        attributeIndex: i,
      });
      continue;
    }
    const seen = firstIndexOf.get(a.semantic);
    if (seen !== undefined) {
      semanticDuplicates.push({
        code: "SEMANTIC_DUPLICATED",
        message: `语义 ${a.semantic}（${SEMANTIC_LABEL[a.semantic]}）被声明了两次：第 ${seen} 条与第 ${i} 条`,
        hint: "同一语义在一个顶点里只能出现一次——顶点着色器按语义绑定输入，"
          + "重复声明会让「取法线」读到不确定的那一份（取决于驱动实现），表现为法线随机翻转",
        attributeIndex: i,
      });
    } else {
      firstIndexOf.set(a.semantic, i);
    }
  }

  // ---- 第二查：区间越界与重叠 ----
  const spans: { index: number; start: number; end: number; a: AttributeDecl }[] = [];
  for (let i = 0; i < attrs.length; i += 1) {
    const a = attrs[i];
    if (a === undefined) continue;
    if (!Number.isInteger(a.byteOffset) || a.byteOffset < 0) {
      boundsIssues.push({
        code: "OFFSET_INVALID",
        message: `第 ${i} 条属性 ${a.semantic} 的偏移非法（${String(a.byteOffset)}）`,
        hint: "偏移须为非负整数；声明式布局不接受负偏移或小数偏移",
        attributeIndex: i,
      });
      continue;
    }
    if (!(a.format in FORMAT_BYTES)) {
      structuralIssues.push({
        code: "FORMAT_UNKNOWN",
        message: `第 ${i} 条属性 ${a.semantic} 的格式 ${JSON.stringify(a.format)} 不在闭集内`,
        hint: `支持集：${ALL_FORMATS.join("、")}；fp64/fp8 等格式需先在本条扩展后再写入布局`,
        attributeIndex: i,
      });
      continue;
    }
    const end = a.byteOffset + attributeByteLength(a);
    if (end > stride) {
      boundsIssues.push({
        code: "LAYOUT_OUT_OF_BOUNDS",
        message: `第 ${i} 条属性 ${a.semantic} 的区间 [${a.byteOffset}, ${end}) 越出每顶点步长 ${stride}`,
        hint: "越界读取**不会报错**，只会静默读到下一个顶点的数据（表现为顶点整体错位一格）；"
          + "请减小 byteOffset 或增大 strideBytes",
        attributeIndex: i,
      });
    }
    spans.push({ index: i, start: a.byteOffset, end, a });
  }
  // 重叠：按起点排序后两两相邻判交（同一份字节被两个语义认领 = 必然有一个是错的）。
  const sorted = [...spans].sort((x, y) => x.start - y.start || x.index - y.index);
  for (let i = 1; i < sorted.length; i += 1) {
    const prev = sorted[i - 1];
    const cur = sorted[i];
    if (prev === undefined || cur === undefined) continue;
    if (prev.end > cur.start) {
      boundsIssues.push({
        code: "LAYOUT_OVERLAP",
        message: `第 ${prev.index} 条 ${prev.a.semantic} [${prev.start}, ${prev.end}) 与第 ${cur.index} 条 ${cur.a.semantic} [${cur.start}, ${cur.end}) 字节区间重叠`,
        hint: "两份声明认领同一段字节：至少有一条偏移是错的。"
          + "请注意「按顺序紧密累加」在有对齐要求时会算错——对齐空洞须显式计入偏移",
        attributeIndex: cur.index,
      });
    }
  }

  // ---- 第三查：对齐违规 ----
  for (const s of spans) {
    const align = attributeAlignment(s.a);
    if (s.start % align !== 0) {
      alignmentIssues.push({
        code: "LAYOUT_ALIGNMENT_VIOLATION",
        message: `第 ${s.index} 条属性 ${s.a.semantic}（${s.a.format}，需 ${align} 字节对齐）偏移 ${s.start} 不是 ${align} 的倍数`,
        hint: `GPU 以${align} 字节粒度取数，未对齐地址的读出值不可信且不报错；`
          + "请把偏移向上取整到对齐边界（代价是 0~align-1 字节填充）",
        attributeIndex: s.index,
      });
    }
  }
  // stride 也须满足最大对齐（否则第二个顶点的首属性必然未对齐——对齐只在首顶点成立是错觉）。
  if (spans.length > 0) {
    const maxAlign = spans.reduce((m, s) => Math.max(m, attributeAlignment(s.a)), 1);
    if (Number.isInteger(stride) && stride > 0 && stride % maxAlign !== 0) {
      alignmentIssues.push({
        code: "LAYOUT_ALIGNMENT_VIOLATION",
        message: `strideBytes ${stride} 不是布局最大对齐 ${maxAlign} 的倍数`,
        hint: "步长未对齐会让第 2 个及以后的顶点整体错位——首顶点对齐而后续不对齐，"
          + "是最难排查的布局缺陷（表现为网格随索引号渐进扭曲）。请把步长补齐到最大对齐",
        attributeIndex: -1,
      });
    }
  }

  // ---- 附加结构查 ----
  const extent = spans.reduce((m, s) => Math.max(m, s.end), 0);
  if (Number.isInteger(stride) && stride > 0 && extent > stride) {
    structuralIssues.push({
      code: "STRIDE_MISMATCH",
      message: `属性最大末尾偏移 ${extent} 超过声明的步长 ${stride}`,
      hint: "步长装不下声明的全部属性：声明与布局互相矛盾，须先确定步长再定偏移",
      attributeIndex: -1,
    });
  }
  if (!firstIndexOf.has("POSITION")) {
    structuralIssues.push({
      code: "LAYOUT_MISSING_POSITION",
      message: "布局未声明 POSITION 属性",
      hint: "POSITION 是顶点数据的最低要求；无位置不成网格（深度/裁剪也无从计算）",
      attributeIndex: -1,
    });
  }
  for (let i = 0; i < attrs.length; i += 1) {
    const a = attrs[i];
    if (a === undefined) continue;
    if (!(a.format in FORMAT_BYTES)) continue;
    const mustBeNormalized = isIntegerFormat(a.format);
    if (a.normalized !== mustBeNormalized) {
      structuralIssues.push({
        code: "NORMALIZED_FLAG_INCONSISTENT",
        message: `第 ${i} 条属性 ${a.semantic} 的 normalized=${String(a.normalized)}，但格式 ${a.format} 要求 ${String(mustBeNormalized)}`,
        hint: mustBeNormalized
          ? "整数格式必须标 normalized=true，否则消费方按原始整数取值（如 255 当成 255.0），数值直接爆表"
          : "浮点格式不应标 normalized，消费方可能按归一化路径再除一次，数值被缩到零附近",
        attributeIndex: i,
      });
    }
    if (ALL_SEMANTICS.includes(a.semantic) && !isFormatAllowed(a.semantic, a.format)) {
      structuralIssues.push({
        code: "FORMAT_NOT_ALLOWED_FOR_SEMANTIC",
        message: `第 ${i} 条属性 ${a.semantic}（${SEMANTIC_LABEL[a.semantic]}）使用非法格式 ${a.format}`,
        hint: `${SEMANTIC_LABEL[a.semantic]}的合法格式：${(ALLOWED_FORMATS[a.semantic] ?? []).join("、")}；`
          + "非法组合写入的数据解码后即错值，且校验不报错",
        attributeIndex: i,
      });
    }
  }

  for (const group of [semanticDuplicates, boundsIssues, alignmentIssues, structuralIssues]) {
    for (const it of group) bag.push(it.code, it.message, it.hint);
  }
  const total = semanticDuplicates.length + boundsIssues.length + alignmentIssues.length + structuralIssues.length;
  const pass = total === 0;
  const summary = pass
    ? `布局校验通过：${attrs.length} 条属性、步长 ${stride} 字节，指纹 ${computeLayoutHash(stride, attrs)}`
    : [
        semanticDuplicates.length > 0 ? `语义重复 ${semanticDuplicates.length} 处` : "",
        boundsIssues.length > 0 ? `越界/重叠 ${boundsIssues.length} 处` : "",
        alignmentIssues.length > 0 ? `对齐违规 ${alignmentIssues.length} 处` : "",
        structuralIssues.length > 0 ? `结构问题 ${structuralIssues.length} 处` : "",
      ]
        .filter((s) => s !== "")
        .join("，");

  return { semanticDuplicates, boundsIssues, alignmentIssues, structuralIssues, pass, summary };
}

/** 校验失败 → 显式 Outcome 失败（不抛异常，诊断由调用方聚合上报）。 */
export function validateOrFail(candidate: LayoutCandidate): Outcome<LayoutValidationReport> {
  const report = validateLayout(candidate);
  if (report.pass) return ok(report);
  const first =
    report.semanticDuplicates[0] ?? report.boundsIssues[0] ?? report.alignmentIssues[0] ?? report.structuralIssues[0];
  if (first === undefined) {
    return fail("STRIDE_MISMATCH", "布局校验未通过但未定位到具体问题（缺陷）", "请检查 validateLayout 的分桶逻辑是否漏项");
  }
  return fail(first.code, `顶点布局校验未通过：${report.summary}`, first.hint);
}

// ════════════════════════════════════════════════════════════════════════════
// §5 自动布局建议（判据四：按用途推荐最小布局·内存优化）
// ════════════════════════════════════════════════════════════════════════════

/** 网格用途（决定需要哪些语义）。 */
export type MeshUsage = "STATIC_PROP" | "SKINNED_CHARACTER" | "TERRAIN" | "PARTICLE" | "UI_SPRITE";

/** 精度档位（在合法格式内进一步选宽窄）。 */
export type PrecisionTier = "HIGH" | "BALANCED" | "MINIMAL";

/** 用途档案：语义集合 + 逐语义默认格式（按精度档位取用）。 */
export interface UsageProfile {
  readonly usage: MeshUsage;
  readonly label: string;
  /** 该用途**必需**的语义（缺任一项则该用途无法正确渲染）。 */
  readonly required: readonly AttributeSemantic[];
  /** 该用途**可选**的语义（材质用到才有；建议器按调用方声明择装）。 */
  readonly optional: readonly AttributeSemantic[];
  /** 各语义在 BALANCED 档的默认格式。 */
  readonly balanced: Readonly<Partial<Record<AttributeSemantic, ComponentFormat>>>;
  /** 各语义在 MINIMAL 档的默认格式（内存最小但误差逼近可辨阈值）。 */
  readonly minimal: Readonly<Partial<Record<AttributeSemantic, ComponentFormat>>>;
  /** 该用途的取舍说明（为什么是这组属性/格式）。 */
  readonly tradeoff: string;
}

/**
 * 五个用途档案。
 *
 * 设计的核心判断：布局臃肿的主要来源是**属性集冗余**而非格式不够省，故每个档案
 * 严格区分 required 与 optional——静态道具的 optional 为空（它永远不需要骨骼权重
 * 与顶点色，除非材质明确用到），而 optional 只在调用方显式声明需要时才装上。
 */
export const USAGE_PROFILES: Readonly<Record<MeshUsage, UsageProfile>> = {
  STATIC_PROP: {
    usage: "STATIC_PROP",
    label: "静态道具（不蒙皮、单一材质为主）",
    required: ["POSITION", "NORMAL", "TANGENT", "UV0"],
    optional: ["COLOR"],
    balanced: { POSITION: "f32", NORMAL: "snorm16", TANGENT: "snorm16", UV0: "u16", COLOR: "u8" },
    minimal: { POSITION: "f16", NORMAL: "snorm8", TANGENT: "snorm16", UV0: "u8", COLOR: "u8" },
    tradeoff: "法线与切线走 snorm16（误差 3e-5，金属高光边缘不抖），UV 走 u16（支持 65K 级平铺且误差 1.5e-5）；"
      + "位置保 f32——静态道具常直接复用美术原坐标（未归一化），半精度会在 1e3 量级丢毫米级精度",
  },
  SKINNED_CHARACTER: {
    usage: "SKINNED_CHARACTER",
    label: "蒙皮角色（含骨骼权重与层级动画）",
    required: ["POSITION", "NORMAL", "TANGENT", "UV0", "BONE_WEIGHT"],
    optional: ["COLOR", "UV1"],
    balanced: { POSITION: "f32", NORMAL: "snorm16", TANGENT: "snorm16", UV0: "u16", BONE_WEIGHT: "f32", COLOR: "u8", UV1: "u16" },
    minimal: { POSITION: "f16", NORMAL: "snorm8", TANGENT: "snorm8", UV0: "u8", BONE_WEIGHT: "u16", COLOR: "u8", UV1: "u8" },
    tradeoff: "骨骼权重保 f32：权重和须为 1，4 组权重用低精度会在关节处产生可见的肢体收缩；"
      + "角色是近景高频资产，权重省下的 8 字节相对整体收益小而形变风险大",
  },
  TERRAIN: {
    usage: "TERRAIN",
    label: "地形（大范围、高程敏感、无切线空间需求）",
    required: ["POSITION", "NORMAL", "UV0"],
    optional: ["COLOR"],
    balanced: { POSITION: "f32", NORMAL: "snorm16", UV0: "u16", COLOR: "u8" },
    minimal: { POSITION: "f16", NORMAL: "snorm8", UV0: "u16", COLOR: "u8" },
    tradeoff: "不装 TANGENT——地形用几何法线做光照，不做切线空间法线贴图，省下 6~8 字节/顶点；"
      + "位置保 f32：地形高程差可达万米量级，且 LOD 简化对高程误差极敏感",
  },
  PARTICLE: {
    usage: "PARTICLE",
    label: "粒子（海量小图元、顶点色主导）",
    required: ["POSITION", "COLOR"],
    optional: ["UV0"],
    balanced: { POSITION: "f32", COLOR: "u8", UV0: "u8" },
    minimal: { POSITION: "f16", COLOR: "u8", UV0: "u8" },
    tradeoff: "只留位置与顶点色——粒子通常是十万级图元，每省 1 字节就是数百 KB 显存；"
      + "不装法线/切线（粒子光照恒为环境光，不参与 PBR 法线计算）",
  },
  UI_SPRITE: {
    usage: "UI_SPRITE",
    label: "UI 精灵（2D 公告板、图标、文本）",
    required: ["POSITION", "UV0", "COLOR"],
    optional: [],
    balanced: { POSITION: "f16", UV0: "u8", COLOR: "u8" },
    minimal: { POSITION: "f16", UV0: "u8", COLOR: "u8" },
    tradeoff: "位置走半精度 f16：UI 坐标经 UI 系统归一化到 [-1,1] 后仍精确到 5e-4（约 0.05 像素@1080p），"
      + "而这是 POSITION 在合法矩阵内唯一可省的档（snorm8 被拒——它是为方向向量设计的，"
      + "值域 1/127 的误差在 [-1,1] 区间约 8e-3，对 UI 像素已可见）；UV 与顶点色走 u8 足够",
  },
};

/** 自动建议的产物。 */
export interface LayoutSuggestion {
  readonly usage: MeshUsage;
  readonly profileLabel: string;
  readonly tier: PrecisionTier;
  /** 推荐的布局（已按紧凑排布生成显式偏移，须通过校验）。 */
  readonly view: LayoutView;
  /** 装上的属性语义列表。 */
  readonly semantics: readonly AttributeSemantic[];
  /** 各属性的权衡条目（逐属性给误差与字节，让「为什么这么配」可查）。 */
  readonly perAttribute: readonly PrecisionMemoryTradeoff[];
  /** 相对同属性集「全 f32 紧凑布局」的字节节省率（0~1，内存优化的量化）。 */
  readonly savedRatio: number;
  /** 总填充字节（紧凑排布后的不可避免空洞）。 */
  readonly paddingBytes: number;
  /** 人话建议（属性集取舍 + 格式取舍 + 误差声明）。 */
  readonly advice: string;
}

/** 建议器入参。 */
export interface SuggestOptions {
  /** 调用方额外需要的可选语义（如材质用到第二套 UV）。 */
  readonly include?: readonly AttributeSemantic[];
  /** 精度档位，缺省 BALANCED。 */
  readonly tier?: PrecisionTier;
}

/**
 * 紧凑排布：按对齐降序放置，使padding 最小。
 *
 * 为何「对齐降序」而非「声明顺序」：2 字节属性放在 4 字节属性之前不会浪费，
 * 反之则会在每个 4 字节属性前留下1~3 字节空洞。按对齐降序排完后空洞只出现在
 * 末尾，再把步长补到最大对齐即得该属性集下的**最小步长**。
 * 声明顺序只影响 location 序号（跨后端一致性的锚点），不影响字节布局——二者解耦
 * 是刻意的：让「改 location 顺序」与「改字节布局」互不牵连。
 */
export function packAttributes(
  entries: readonly { semantic: AttributeSemantic; format: ComponentFormat }[],
): readonly AttributeDecl[] {
  // 同对齐内按语义字典序定序，保证同输入同输出（确定性是资产可复现的前提）。
  const ordered = [...entries].sort((a, b) => {
    const da = FORMAT_ALIGNMENT[a.format];
    const db = FORMAT_ALIGNMENT[b.format];
    if (db !== da) return db - da;
    return a.semantic.localeCompare(b.semantic);
  });
  const decls: AttributeDecl[] = [];
  let cursor = 0;
  for (const e of ordered) {
    const align = FORMAT_ALIGNMENT[e.format];
    const aligned = alignUp(cursor, align);
    decls.push({ semantic: e.semantic, format: e.format, byteOffset: aligned, normalized: isIntegerFormat(e.format) });
    cursor = aligned + FORMAT_BYTES[e.format] * SEMANTIC_COMPONENTS[e.semantic];
  }
  // 步长不在此处返回：由 naturalStride 依同一算法从 decls 复算，调用方统一走那一个
  // 事实源。两处各算一次会在「重排后重算」这类改动里产生两份可能不一致的步长。
  return decls;
}

/** 向上取整到align 的倍数。 */
function alignUp(value: number, align: number): number {
  const rem = value % align;
  return rem === 0 ? value : value + (align - rem);
}

/** 由一组属性声明求步长（紧凑排布后的自然步长 = 末尾补到最大对齐）。 */
function naturalStride(decls: readonly AttributeDecl[]): number {
  let cursor = 0;
  let maxAlign = 1;
  for (const d of decls) {
    const align = FORMAT_ALIGNMENT[d.format];
    maxAlign = Math.max(maxAlign, align);
    cursor = Math.max(cursor, d.byteOffset + attributeByteLength(d));
  }
  return alignUp(cursor, maxAlign);
}

/** 语义 → location 序号（跨后端投影的锚点：按语义字典序定序，全局稳定）。 */
function locationOrder(semantics: readonly AttributeSemantic[]): ReadonlyMap<AttributeSemantic, number> {
  const sorted = [...semantics].sort((a, b) => a.localeCompare(b));
  const m = new Map<AttributeSemantic, number>();
  for (let i = 0; i < sorted.length; i += 1) {
    const s = sorted[i];
    if (s !== undefined) m.set(s, i);
  }
  return m;
}

/**
 * 自动布局建议：按用途推荐**最小合法布局**。
 *
 * 「最小」的含义是三维的，不是单纯把字节压到最小：
 *   ① 属性集最小——只装该用途必需 + 调用方显式索要的语义（臃肿主因是属性冗余）；
 *   ② 格式在合法矩阵内按精度档位取（非法组合一律不推荐，宁可给较宽格式）；
 *   ③ 字节排布最小——紧凑对齐排布，padding 只落在末尾。
 * 三者合起来才是「最小合法布局」：单看①会牺牲②，单看③会装无用属性。
 */
export function suggestLayout(usage: MeshUsage, options: SuggestOptions = {}): Outcome<LayoutSuggestion> {
  const profile = USAGE_PROFILES[usage];
  if (profile === undefined) {
    return fail(
      "USAGE_PROFILE_INVALID",
      `未知网格用途 ${JSON.stringify(usage)}`,
      `支持用途：${Object.keys(USAGE_PROFILES).join("、")}；新增用途须同时给出属性集与各档默认格式，`
        + "否则建议器只能靠猜——猜出来的布局会静默装错属性集",
    );
  }
  const tier = options.tier ?? "BALANCED";
  if (tier !== "HIGH" && tier !== "BALANCED" && tier !== "MINIMAL") {
    return fail(
      "USAGE_PROFILE_INVALID",
      `未知精度档位 ${JSON.stringify(tier)}`,
      "支持档位：HIGH、BALANCED、MINIMAL；档位决定在合法格式内取多宽的格式",
    );
  }
  const wanted = [...profile.required];
  for (const extra of options.include ?? []) {
    if (!ALL_SEMANTICS.includes(extra)) {
      return fail(
        "SEMANTIC_UNKNOWN",
        `include 中的语义 ${JSON.stringify(extra)} 不在闭集内`,
        `支持集：${ALL_SEMANTICS.join("、")}`,
      );
    }
    if (!wanted.includes(extra)) wanted.push(extra);
  }

  const bag = new DiagBag();
  const entries: { semantic: AttributeSemantic; format: ComponentFormat }[] = [];
  const perAttribute: PrecisionMemoryTradeoff[] = [];
  const fallbackNotes: string[] = [];

  for (const sem of wanted) {
    const chosen = chooseFormatFor(profile, sem, tier);
    if (chosen === null) {
      // 档案未给该语义配格式：按语义值域退到该语义最宽的合法格式（f32 恒合法），
      // 登记为告警而不是静默跳过——跳过会让属性消失，而属性消失是静默的质量降级。
      const fallback: ComponentFormat = "f32";
      entries.push({ semantic: sem, format: fallback });
      perAttribute.push(tradeoffFor(sem, fallback));
      fallbackNotes.push(`${SEMANTIC_LABEL[sem]} 在 ${usage} 的${tier} 档未配格式，已退到全精度 f32（安全但占内存）`);
      continue;
    }
    entries.push({ semantic: sem, format: chosen });
    const t = tradeoffFor(sem, chosen);
    perAttribute.push(t);
    if (t.verdict === "ACCEPTABLE") {
      fallbackNotes.push(
        `${SEMANTIC_LABEL[sem]} 采用 ${chosen} 时绝对误差 ${t.absoluteError.toExponential(2)}已超${ERROR_WARN_RATIO * 100}%：`
          + "建议仅用于远景/粒子，近景资产改用 HIGH 档",
      );
    }
  }

  const packed = packAttributes(entries);
  const stride = naturalStride(packed);
  // 按语义字典序重排声明，使 location 序号全局稳定（与跨后端投影的一致性锚点对齐）。
  const locs = locationOrder(wanted);
  const ordered: AttributeDecl[] = [...packed].sort((a, b) => {
    const la = locs.get(a.semantic) ?? 0;
    const lb = locs.get(b.semantic) ?? 0;
    return la - lb;
  });
  const view = buildView(stride, ordered);

  // 自证：推荐结果必须自身通过三查。推荐器输出未经验证的布局 = 把错误从工具
  // 转移到了调用方，故此处直接校验并把问题转为诊断（不静默输出坏布局）。
  const report = validateLayout({ strideBytes: view.layout.strideBytes, attributes: view.layout.attributes });
  if (!report.pass) {
    bag.push(
      "STRIDE_MISMATCH",
      `自动建议产出的布局未通过自身校验：${report.summary}`,
      "这是建议器内部的缺陷（排布或格式选型有误），不是调用方的输入问题；"
        + "请按报告指出的类别修正 packAttributes / chooseFormatFor 后重试",
    );
    for (const it of [...report.semanticDuplicates, ...report.boundsIssues, ...report.alignmentIssues, ...report.structuralIssues]) {
      bag.push(it.code, it.message, it.hint);
    }
  }

  // 节省率：同属性集全 f32 紧凑布局 vs 推荐布局（同排布算法，故差异只来自格式）。
  const allF32 = packAttributes(wanted.map((s) => ({ semantic: s, format: "f32" as const })));
  const f32Stride = naturalStride(allF32);
  const savedRatio = f32Stride > 0 ? (f32Stride - stride) / f32Stride : 0;

  const advice = [
    `用途「${profile.label}」→ 属性集：${wanted.map((s) => SEMANTIC_LABEL[s]).join("、")}。${profile.tradeoff}`,
    `紧凑排布后每顶点 ${stride} 字节（全 f32 同属性集为 ${f32Stride} 字节，省 ${(savedRatio * 100).toFixed(1)}%），`
      + `末尾填充 ${view.tailPaddingBytes} 字节。`,
    ...fallbackNotes,
  ].join("\n");

  return ok(
    {
      usage,
      profileLabel: profile.label,
      tier,
      view,
      semantics: wanted,
      perAttribute,
      savedRatio,
      paddingBytes: view.tailPaddingBytes,
      advice,
    },
    bag.all(),
  );
}

/** 档位 → 档案格式表的查法（HIGH 档统一取最宽合法格式）。 */
function chooseFormatFor(profile: UsageProfile, sem: AttributeSemantic, tier: PrecisionTier): ComponentFormat | null {
  const allowed = ALLOWED_FORMATS[sem] ?? [];
  if (tier === "HIGH") {
    // 最宽合法格式：f32 恒在合法集内，故 HIGH 档等价于 f32，但仍走合法性检查兜底。
    for (const f of ["f32", "f16", "snorm16", "u16"] as const) {
      if (allowed.includes(f)) return f;
    }
    return allowed.length > 0 ? allowed[0] ?? null : null;
  }
  const table = tier === "MINIMAL" ? profile.minimal : profile.balanced;
  const want = table[sem];
  if (want === undefined) return null;
  if (!allowed.includes(want)) {
    // 档案配了非法格式（档案维护者的错）：退到最宽合法格式，由调用方通过诊断知悉。
    for (const f of ["f32", "f16", "snorm16", "u16"] as const) {
      if (allowed.includes(f)) return f;
    }
    return allowed[0] ?? null;
  }
  return want;
}

// ════════════════════════════════════════════════════════════════════════════
// §6 自检（四条判据的可执行形态）
// ════════════════════════════════════════════════════════════════════════════

/** 自检结果项：每项对应判据的一条，独立可定位。 */
export interface SelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 构造一份声明式布局夹具（全部属性自带偏移、stride 自带——无隐式补全）。 */
function makeFixtureDecls(): readonly AttributeDecl[] {
  // 手算紧凑排布：f32 对齐 4、2 字节对齐 2，故POSITION(4B) → NORMAL(4B) →
  // TANGENT(4B) → UV0(4B) → COLOR(4B)，末尾恰好 52，补到 4 的倍数仍为 52。
  return [
    { semantic: "POSITION", format: "f32", byteOffset: 0, normalized: false },
    { semantic: "NORMAL", format: "f32", byteOffset: 12, normalized: false },
    { semantic: "TANGENT", format: "f32", byteOffset: 24, normalized: false },
    { semantic: "UV0", format: "f32", byteOffset: 40, normalized: false },
    { semantic: "COLOR", format: "u8", byteOffset: 48, normalized: true },
  ];
}

/** 判据一自检：声明制（无隐式补全 + 偏移显式）+ 跨后端一致（投影同序同偏移）。 */
export function selfCheckDeclarative(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const decls = makeFixtureDecls();
  const stride = naturalStride(decls);
  const declared = declareLayout(stride, decls);
  out.push({
    name: "declare-layout-explicit-offsets-and-stride",
    pass: declared.ok,
    detail: declared.ok
      ? `声明制布局成立：${declared.value.layout.attributes.length} 条属性全部自带偏移，步长显式为 ${stride} 字节，指纹 ${declared.value.layout.layoutHash}`
      : `声明失败：${declared.message}`,
  });

  // 空布局必须被拒（无隐式补全的极端体现：不会给你造一个「只有隐式默认」的空壳）。
  const empty = declareLayout(16, []);
  out.push({
    name: "empty-layout-rejected-not-defaulted",
    pass: !empty.ok && empty.code === "LAYOUT_MISSING_POSITION",
    detail: "空属性集被显式拒绝，而不是补一个隐式默认布局——补默认值就是隐式布局的起点",
  });

  // 跨后端一致：五后端投影出的 location 序列与字节偏移序列必须逐项相同。
  const view = declared.ok ? declared.value : null;
  const all = view === null ? null : projectToAllBackends(view);
  let consistent = false;
  let detail = "投影失败";
  if (all !== null && all.ok) {
    const base = all.value[0];
    consistent =
      base !== undefined &&
      all.value.every(
        (b) =>
          b.slots.length === base.slots.length &&
          b.slots.every((s, i) => {
            const ref = base.slots[i];
            return ref !== undefined && s.location === ref.location && s.byteOffset === ref.byteOffset;
          }) &&
          b.strideBytes === base.strideBytes &&
          b.layoutHash === base.layoutHash,
      );
    detail = `五后端（Vulkan/D3D11/D3D12/Metal/OpenGL）投影：各 ${String(base?.slots.length)}槽，location 序列与偏移序列逐项一致，步长与指纹全同（${String(base?.layoutHash)}）`;
  }
  out.push({ name: "cross-backend-projection-consistent", pass: consistent, detail });

  // 后端格式串必须各后端原生（不是把 Vulkan 串塞给 Metal）。
  const vk = view === null ? null : projectToBackend(view, "VULKAN");
  const mtl = view === null ? null : projectToBackend(view, "METAL");
  const nativeOk =
    vk !== null &&
    mtl !== null &&
    vk.ok &&
    mtl.ok &&
    vk.value.slots[0]?.formatToken.startsWith("R32G32B32_") === true &&
    mtl.value.slots[0]?.formatToken.startsWith("MTLVertexFormat") === true;
  out.push({
    name: "backend-format-tokens-native",
    pass: nativeOk,
    detail: "各后端格式串取自各自原生枚举表（Vulkan R32G32B32_SFLOAT / Metal MTLVertexFormatFloat3），非跨后端复用",
  });

  // 指纹确定性 + 顺序无关性（仅重排属性不算布局变更）。
  const h1 = computeLayoutHash(stride, decls);
  const h2 = computeLayoutHash(stride, decls);
  const shuffled = [decls[3], decls[0], decls[4], decls[1], decls[2]].filter((d): d is AttributeDecl => d !== undefined);
  const h3 = computeLayoutHash(stride, shuffled);
  out.push({
    name: "layout-hash-deterministic-and-order-free",
    pass: h1 === h2 && h1 === h3,
    detail: `同布局两次指纹一致（${h1} = ${h2}）；仅重排属性顺序指纹不变（${h3}）——顺序不参与布局身份`,
  });

  return out;
}

/** 判据二自检：打包权衡（表可查 + 误差实测落在声明上界内）。 */
export function selfCheckTradeoff(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const matrix = fullTradeoffMatrix();
  out.push({
    name: "tradeoff-matrix-complete",
    pass: matrix.length === ALL_SEMANTICS.length * ALL_FORMATS.length,
    detail: `权衡表覆盖 ${ALL_SEMANTICS.length} 语义 × ${ALL_FORMATS.length} 格式 = ${matrix.length} 条，无遗漏格`,
  });

  // 内存比正确：NORMAL@snorm8 = 3 字节 vs f32 = 12 字节 → 0.25。
  const t = tradeoffFor("NORMAL", "snorm8");
  out.push({
    name: "memory-ratio-and-error-declared",
    pass: t.bytes === 3 && Math.abs(t.memoryRatio - 0.25) < 1e-12 && Math.abs(t.absoluteError - 1 / 127) < 1e-12,
    detail: `法线@snorm8：${t.bytes} 字节、内存比 ${t.memoryRatio}、绝对误差 ${t.absoluteError.toExponential(3)}（=1/127×值域半宽 1）`,
  });

  // 非法组合必须被拒（权衡透明：不可用就说不可用，不给「大概可以」）。
  const bad = tradeoffFor("POSITION", "snorm8");
  const badWeight = tradeoffFor("BONE_WEIGHT", "snorm8");
  out.push({
    name: "illegal-combinations-rejected",
    pass: bad.verdict === "REJECTED" && badWeight.verdict === "REJECTED",
    detail: `POSITION@snorm8 与 BONE_WEIGHT@snorm8 均判REJECTED（位置值域不受控；权重非负用 SNORM 会允许负权重）`,
  });

  // 量化往返实测：全部「合法」组合都验一遍，误差须落在声明上界内。
  const failures: string[] = [];
  let checked = 0;
  for (const sem of ALL_SEMANTICS) {
    for (const fmt of ALL_FORMATS) {
      if (!isFormatAllowed(sem, fmt)) continue;
      checked += 1;
      const r = verifyQuantization(sem, fmt);
      if (!r.ok) failures.push(`${sem}@${fmt}: ${r.message}`);
    }
  }
  out.push({
    name: "quantization-roundtrip-within-declared-bound",
    pass: failures.length === 0,
    detail:
      failures.length === 0
        ? `${checked} 个合法「语义×格式」组合的打包→解包实测误差全部落在声明上界内（探针取满值程 64 等分 + 端点）`
        : `以下组合实测误差超上界：${failures.join("；")}`,
  });

  // 半精度转换的往返：f16 位型→f32→位型应幂等（幂等是量化确定性的可测形式）。
  const halfRound = (v: number): number => float32ToFloat16Bits(float16BitsToFloat32(float32ToFloat16Bits(v)));
  const probes = [0, 1, -1, 0.5, 3.14159, 1e-4, 1023.5, -2048];
  const idempotent = probes.every((v) => halfRound(v) === float32ToFloat16Bits(v));
  out.push({
    name: "fp16-conversion-idempotent",
    pass: idempotent,
    detail: `半精度位型转换幂等（f32→f16→f32→f16 同值），探针 ${probes.length} 个含次正规 1e-4 与边界 1023.5`,
  });

  return out;
}

/** 判据三自检：三查各自能触发，且一次性报全部（不挤牙膏）。 */
export function selfCheckThreeAudits(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 干净布局全绿。
  const good = validateLayout({ strideBytes: naturalStride(makeFixtureDecls()), attributes: makeFixtureDecls() });
  out.push({
    name: "clean-layout-passes",
    pass: good.pass,
    detail: `合法布局：${good.summary}`,
  });

  // 第一查：语义重复。
  const dup = validateLayout({
    strideBytes: 64,
    attributes: [
      { semantic: "POSITION", format: "f32", byteOffset: 0, normalized: false },
      { semantic: "NORMAL", format: "f32", byteOffset: 12, normalized: false },
      { semantic: "NORMAL", format: "f32", byteOffset: 24, normalized: false },
    ],
  });
  out.push({
    name: "audit-1-semantic-duplicate-detected",
    pass: !dup.pass && dup.semanticDuplicates.length === 1,
    detail: `重复声明 NORMAL 被归入第一查：${dup.semanticDuplicates[0]?.message ?? "未检出"}`,
  });

  // 第二查：越界 + 重叠（两类同属第二查，一次报出）。
  const oob = validateLayout({
    strideBytes: 24,
    attributes: [
      { semantic: "POSITION", format: "f32", byteOffset: 0, normalized: false },
      { semantic: "TANGENT", format: "f32", byteOffset: 20, normalized: false },
    ],
  });
  const overlap = validateLayout({
    strideBytes: 64,
    attributes: [
      { semantic: "POSITION", format: "f32", byteOffset: 0, normalized: false },
      { semantic: "NORMAL", format: "snorm16", byteOffset: 6, normalized: true },
    ],
  });
  out.push({
    name: "audit-2-bounds-and-overlap-detected",
    pass:
      oob.boundsIssues.some((i) => i.code === "LAYOUT_OUT_OF_BOUNDS") &&
      overlap.boundsIssues.some((i) => i.code === "LAYOUT_OVERLAP"),
    detail: `越界检出：${oob.boundsIssues[0]?.code ?? "无"}；重叠检出：${overlap.boundsIssues[0]?.code ?? "无"}（二者同属第二查）`,
  });

  // 第三查：对齐违规（属性偏移 + 步长双面）。
  const misalign = validateLayout({
    strideBytes: 50,
    attributes: [
      { semantic: "POSITION", format: "f32", byteOffset: 2, normalized: false },
      { semantic: "NORMAL", format: "f32", byteOffset: 14, normalized: false },
    ],
  });
  const strideMisalign = validateLayout({
    strideBytes: 50,
    attributes: [
      { semantic: "POSITION", format: "f32", byteOffset: 0, normalized: false },
      { semantic: "NORMAL", format: "f32", byteOffset: 12, normalized: false },
    ],
  });
  const propMis = misalign.alignmentIssues.filter((i) => i.attributeIndex >= 0).length;
  const strideMis = strideMisalign.alignmentIssues.filter((i) => i.attributeIndex < 0).length;
  out.push({
    name: "audit-3-alignment-detected",
    pass: propMis === 2 && strideMis === 1,
    detail: `属性偏移未对齐检出 ${propMis} 处（偏移 2 与 14 对 f32 的 4 字节对齐）；步长未对齐检出 ${strideMis} 处（stride 50 对 4）`,
  });

  // 一次报全部：三类查各自真有命中的布局，每类都被报出而非「遇错即返」。
  // 构造：POSITION 与首条 NORMAL 同偏移 2（→ 重叠 + 双双未对齐），第二条 NORMAL 重复语义
  // 且偏移 18 未对齐，步长 50 亦未对齐。三个桶同时非空即证明报告是聚合式的。
  const multi = validateLayout({
    strideBytes: 50,
    attributes: [
      { semantic: "POSITION", format: "f32", byteOffset: 2, normalized: false },
      { semantic: "NORMAL", format: "f32", byteOffset: 2, normalized: false },
      { semantic: "NORMAL", format: "f32", byteOffset: 18, normalized: false },
    ],
  });
  const buckets = multi.semanticDuplicates.length + multi.boundsIssues.length + multi.alignmentIssues.length + multi.structuralIssues.length;
  out.push({
    name: "all-issues-reported-at-once",
    pass:
      !multi.pass &&
      multi.semanticDuplicates.length > 0 &&
      multi.boundsIssues.length > 0 &&
      multi.alignmentIssues.length > 0,
    detail: `同偏移 2 + 重复 NORMAL + 步长 50 未对齐：一次报出第一查 ${multi.semanticDuplicates.length} 条、第二查 ${multi.boundsIssues.length} 条、第三查 ${multi.alignmentIssues.length} 条，合计 ${buckets} 条（三桶同时非空，证明非「遇错即返」）`,
  });

  // 附加查：normalized 自洽 + 非法格式组合。
  const flag = validateLayout({
    strideBytes: 12,
    attributes: [{ semantic: "POSITION", format: "f32", byteOffset: 0, normalized: true }],
  });
  const badFmt = validateLayout({
    strideBytes: 4,
    attributes: [{ semantic: "POSITION", format: "snorm8", byteOffset: 0, normalized: true }],
  });
  out.push({
    name: "normalized-flag-and-format-legality-checked",
    pass:
      flag.structuralIssues.some((i) => i.code === "NORMALIZED_FLAG_INCONSISTENT") &&
      badFmt.structuralIssues.some((i) => i.code === "FORMAT_NOT_ALLOWED_FOR_SEMANTIC"),
    detail: "浮点格式误标 normalized 与 POSITION 用 snorm8 均被结构查拦下",
  });

  // 校验失败转 Outcome 且带三要素。
  const orFail = validateOrFail({ strideBytes: 24, attributes: [{ semantic: "POSITION", format: "f32", byteOffset: 0, normalized: false }, { semantic: "TANGENT", format: "f32", byteOffset: 20, normalized: false }] });
  out.push({
    name: "validate-failure-is-explicit-outcome",
    pass: !orFail.ok && orFail.message.length > 0 && orFail.hint.length > 0,
    detail: orFail.ok ? "坏布局竟被放行（缺陷）" : `校验失败转为 Outcome失败：${orFail.code}`,
  });

  return out;
}

/** 判据四自检：自动建议（推荐结果自身合规 + 确实更省 + 按用途裁属性）。 */
export function selfCheckSuggest(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const usages = Object.keys(USAGE_PROFILES) as MeshUsage[];

  // 全部用途的建议结果必须自身通过三查。
  const bad: string[] = [];
  const rows: string[] = [];
  for (const u of usages) {
    const s = suggestLayout(u);
    if (!s.ok) {
      bad.push(`${u}: ${s.message}`);
      continue;
    }
    const r = validateLayout({
      strideBytes: s.value.view.layout.strideBytes,
      attributes: s.value.view.layout.attributes,
    });
    if (!r.pass) bad.push(`${u}: 建议布局未过校验（${r.summary}）`);
    rows.push(
      `${u} ${s.value.view.layout.strideBytes}B/${s.value.view.layout.attributes.length}属性/省${(s.value.savedRatio * 100).toFixed(1)}%`,
    );
  }
  out.push({
    name: "suggestions-are-self-valid",
    pass: bad.length === 0,
    detail: bad.length === 0 ? `五用途建议全部通过三查：${rows.join("，")}` : `不合规建议：${bad.join("；")}`,
  });

  // 属性集按用途裁剪：静态道具不得带骨骼权重；蒙皮角色必须带。
  const prop = suggestLayout("STATIC_PROP");
  const skin = suggestLayout("SKINNED_CHARACTER");
  const terrain = suggestLayout("TERRAIN");
  const propOk = prop.ok && !prop.value.semantics.includes("BONE_WEIGHT");
  const skinOk = skin.ok && skin.value.semantics.includes("BONE_WEIGHT");
  const terrainOk = terrain.ok && !terrain.value.semantics.includes("TANGENT");
  out.push({
    name: "attribute-set-trimmed-per-usage",
    pass: propOk && skinOk && terrainOk,
    detail: `静态道具无骨骼权重（${propOk}）、蒙皮角色带权重（${skinOk}）、地形无切线（${terrainOk}）——臃肿主因是属性冗余而非格式`,
  });

  // 可选属性按需加装：显式 include 才装COLOR。
  const withColor = suggestLayout("STATIC_PROP", { include: ["COLOR"] });
  out.push({
    name: "optional-attributes-added-on-demand",
    pass: withColor.ok && withColor.value.semantics.includes("COLOR"),
    detail: withColor.ok
      ? `静态道具显式 include COLOR 后步长从 ${String(prop.ok ? prop.value.view.layout.strideBytes : "?")} 增至 ${withColor.value.view.layout.strideBytes} 字节（可选属性按需而非默认全装）`
      : "include 失败",
  });

  // 确实更省：相对同属性集全 f32 布局有节省，且 MINIMAL 档不比 BALANCED 宽。
  const bal = suggestLayout("STATIC_PROP", { tier: "BALANCED" });
  const min = suggestLayout("STATIC_PROP", { tier: "MINIMAL" });
  const hi = suggestLayout("STATIC_PROP", { tier: "HIGH" });
  const savingOk = bal.ok && min.ok && hi.ok && bal.value.savedRatio > 0 && min.value.view.layout.strideBytes <= bal.value.view.layout.strideBytes;
  out.push({
    name: "suggested-layout-saves-memory",
    pass: savingOk,
    detail:
      bal.ok && min.ok && hi.ok
        ? `静态道具三档步长：HIGH ${hi.value.view.layout.strideBytes}B / BALANCED ${bal.value.view.layout.strideBytes}B（省 ${(bal.value.savedRatio * 100).toFixed(1)}%）/ MINIMAL ${min.value.view.layout.strideBytes}B`
        : "档位建议失败",
  });

  // 紧凑排布的padding 最小性：同属性集下末尾填充不超过 (maxAlign-1)。
  const padOk = bal.ok && bal.value.paddingBytes < 4;
  out.push({
    name: "compact-packing-minimizes-padding",
    pass: padOk,
    detail: bal.ok ? `紧凑排布后末尾填充 ${bal.value.paddingBytes} 字节（最大对齐 4，故上界为 3）` : "建议失败",
  });

  // 未知用途必须显式失败（不猜布局）。
  const unknown = suggestLayout("UNKNOWN_USAGE" as MeshUsage);
  out.push({
    name: "unknown-usage-rejected",
    pass: !unknown.ok && unknown.code === "USAGE_PROFILE_INVALID",
    detail: "未知用途被显式拒绝（猜布局=静默装错属性集，故必须拒绝）",
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
    declarative: selfCheckDeclarative(),
    tradeoff: selfCheckTradeoff(),
    threeAudits: selfCheckThreeAudits(),
    suggest: selfCheckSuggest(),
  };
  const failed: string[] = [];
  for (const [g, items] of Object.entries(groups)) {
    for (const it of items) {
      if (!it.pass) failed.push(`${g}.${it.name}: ${it.detail}`);
    }
  }
  return { groups, allPass: failed.length === 0, failed };
}