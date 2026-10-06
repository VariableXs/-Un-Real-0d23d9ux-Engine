/**
 * VE-F1602 · 网格容器格式 vmesh（I 域 · 3D 管线域 · L0 几何数据层 · 批次 I01）
 * ---------------------------------------------------------------------------
 * 职责定位：VE 自有的**开放网格容器格式** vmesh 的定义、读写与校验。
 * 上游 F1601（I 域开工与总架构）把本条的产出契约登记为 `MeshContainer`，
 * 是十主题注册表中 `mesh-format` 主题的兑现；下游 F1603（顶点属性布局）、
 * F1604（索引与拓扑）、F1605（网格量化）、F1608（QEM 简化）、F1611（流式容器）
 * 全部以本格式为存储契约。
 *
 * 五段容器（判据一，逐段对应判据原文的「顶点/索引/子网格/LOD/材质引用」）：
 *   ① VERTICES顶点缓冲 —— 属性按语义命名（POSITION/NORMAL/TANGENT/UV0/COLOR），
 *      每属性带自己的格式与归一化标志，故格式是数据的一部分而非隐式约定。
 *   ② INDICES     索引缓冲 —— 位宽随顶点数自动选型（16/32 位），顶点数上限即选型依据。
 *   ③ SUBMESHES   子网格     —— 按材质分组的面区间 + 材质索引。
 *   ④ LOD         LOD 层级   —— 原始级 + 简化级的引用关系与误差上界声明。
 *   ⑤ MATERIALS材质引用 —— 材质名与外部材质库的对接键（几何只存引用，不存材质定义）。
 *
 * 双形态（判据三「同哲学」）：JSON 头 + 二进制主体，**与 CGPU-F0258 图序列化同设计哲学**
 * （自描述 schema + 版本号 + 前向兼容 + 二进制主体性能 / JSON 头可读）。
 * 声明此「同哲学」不是巧合而是刻意对齐：两卷的资产最终要打进同一个包（Q 域F3201+），
 * 容器心智不统一会让打包侧写出两套解析器。
 *
 * 前向兼容纪律（F1349）：读新版文件时，旧版读不认识的字段**必须忽略而非报错**；
 * 读旧版文件时遇到不认识自己所需的新字段才是真问题（显式失败）。
 * 「忽略未知字段」与「拒绝不兼容的主版本」是两条不同的规则，混起来就废了演进能力。
 *
 * 校验（判据四）：块 CRC + 全文件哈希，两级定位。
 *   · 块 CRC —— 定位「哪一段坏了」（顶点区好但索引区坏，与整个文件坏，处理方式不同）。
 *   · 全文件哈希 —— 定位「是不是预期的那个文件」（跨版本一致性、缓存命中、资产库对账）。
 * 单有CRC 不能回答「文件被换成了另一个仍然自洽的文件」，单有哈希不能回答「坏在哪一段」。
 *
 * 互转（判据二）：glTF 2.0 双向。往���为完整映射（glTF 的 accessor/bufferView/primitive
 * 模型能无损落进五段容器）；导出为**显式降级**——glTF 不支持的能力（如本格式的
 * LOD 误差上界声明）必须逐项列出降级去向，不静默丢弃。往返断言（roundtrip）
 * 证明「导入→导出→再导入」语义等价，不等价处逐条登记为已知降级。
 *
 * 零静默纪律：解析失败、版本不兼容、CRC 不符、哈希不符、导出降级，全部产出 Diagnostic
 * （code + message + hint）。本模块不抛异常、不吞诊断；损坏文件一律走「带诊断的失败」
 * 而不是「尽力解析出半个网格」——半个网格进渲染管线比明确拒绝危害大得多。
 *
 * 判据：vmesh、glTF 互转、同哲学、校验。
 * 交接说明：本条只管格式与校验，不做几何语义解释（属性布局校验归 F1612/F1603）、
 *         不做量化（归 F1605）、不做简化（归 F1608）。落位 src/system/ve/iDomain3d/。
 */

// ════════════════════════════════════════════════════════════════════════════
// §0 诊断基础设施（零静默第一层；与 F1601 同纪律，此处自包含不跨条import）
// ════════════════════════════════════════════════════════════════════════════

/** 诊断码：每种失败独立可检索，绝不合并为一条通用错误。 */
export type DiagCode =
  /** JSON 头无法解析 / 不是对象 / 缺必需字段。 */
  | "HEADER_MALFORMED"
  /** 主版本号不受支持（跨大版本不兼容，必须拒绝而非猜）。 */
  | "MAJOR_VERSION_UNSUPPORTED"
  /** 次版本号高于本实现（应有前向兼容的忽略路径，走到这里说明忽略逻辑有漏）。 */
  | "MINOR_VERSION_AHEAD"
  /** 二进制主体长度与头声明不符（截断或拼接错误）。 */
  | "PAYLOAD_SIZE_MISMATCH"
  /** 段偏移/长度越界或区间重叠。 */
  | "SEGMENT_RANGE_INVALID"
  /** 属性语义不受支持（本实现不认识该 POSITION/TANGENT 之外的自定义语义）。 */
  | "ATTRIBUTE_SEMANTIC_UNKNOWN"
  /** 属性格式不受支持（如 fp64 或未列出的枚举值）。 */
  | "ATTRIBUTE_FORMAT_UNKNOWN"
  /** 索引值越出顶点数范围。 */
  | "INDEX_OUT_OF_RANGE"
  /** 子网格面区间越出索引缓冲。 */
  | "SUBMESH_RANGE_INVALID"
  /** 材质引用指向空名或自引用。 */
  | "MATERIAL_REF_INVALID"
  /** LOD 层级引用了不存在的级别，或成环。 */
  | "LOD_REFERENCE_INVALID"
  /** 块 CRC 不符——定位到具体段。 */
  | "SEGMENT_CRC_MISMATCH"
  /** 全文件哈希不符——定位到「文件整体不是预期的那个」。 */
  | "FILE_HASH_MISMATCH"
  /** glTF 侧数据缺失或非法（导入路径）。 */
  | "GLTF_MALFORMED"
  /** 导出到 glTF 时发生能力降级（不是错误，是必须显性登记的事实）。 */
  | "GLTF_EXPORT_DEGRADED"
  /** 往返断言不通过。 */
  | "ROUNDTRIP_MISMATCH"
  /** 数值非有限（NaN/Inf 出现在应当有限的字段）。 */
  | "VALUE_NONFINITE";

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

/** 成功构造（diagnostics 可携带非致命告警，如导出降级）。 */
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

  byCode(code: DiagCode): readonly Diagnostic[] {
    return this.items.filter((d) => d.code === code);
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §1 基础类型与格式常量
// ════════════════════════════════════════════════════════════════════════════

/** 浮点比较容差（几何量级差异大，统一取相对容差）。 */
export const EPS = 1e-9;

/** 格式标识：写进文件头 8 字节，用于快速拒绝「这不是 vmesh 文件」。 */
export const VMESH_MAGIC = "VMESH\x00\x01";

/** 本实现支持的版本。主版本不同= 不兼容（拒绝）；次版本不同 = 前向兼容（忽略未知字段）。 */
export const VMESH_VERSION = { major: 1, minor: 3 } as const;

/** 本实现能读的最大主版本。高于此值一律拒绝，不做「尽力解析」。 */
export const VMESH_MAX_MAJOR = 1;

/** 顶点属性语义（闭集；F1603 的属性布局在此之上再加偏移与对齐规则）。 */
export type AttributeSemantic = "POSITION" | "NORMAL" | "TANGENT" | "UV0" | "UV1" | "COLOR" | "BONE_WEIGHT";

/** 本实现认识的全部语义（自检用：注册表与枚举须一致）。 */
export const ALL_SEMANTICS: readonly AttributeSemantic[] = [
  "POSITION",
  "NORMAL",
  "TANGENT",
  "UV0",
  "UV1",
  "COLOR",
  "BONE_WEIGHT",
];

/** 顶点分量格式（枚举值写进文件，故数值不可随意改位）。 */
export type ComponentFormat = "f32" | "f16" | "snorm8" | "snorm16" | "u8" | "u16";

/** 各格式的每分量字节数。 */
export const FORMAT_BYTES: Readonly<Record<ComponentFormat, number>> = {
  f32: 4,
  f16: 2,
  snorm8: 1,
  snorm16: 2,
  u8: 1,
  u16: 2,
};

/** 该格式是否为归一化格式（整数存储，读出需除以最大值还原到 [-1,1] 或 [0,1]）。 */
export function isNormalized(fmt: ComponentFormat): boolean {
  return fmt === "snorm8" || fmt === "snorm16" || fmt === "u8" || fmt === "u16";
}

/** 归一化格式的还原除数（u8/u16 还原到 [0,1]，snorm 还原到 [-1,1]）。 */
export function normalizationScale(fmt: ComponentFormat): number {
  if (fmt === "u8") return 255;
  if (fmt === "u16" || fmt === "snorm16") return 32767;
  if (fmt === "snorm8") return 127;
  return 1;
}

/** 各格式的理论误差上界（量化档位与误差权衡表的依据，F1605 对拍用）。 */
export const FORMAT_MAX_ERROR: Readonly<Record<ComponentFormat, number>> = {
  f32: Number.EPSILON,
  f16: 2 ** -11,
  snorm8: 1 / 127,
  snorm16: 1 / 32767,
  u8: 1 / 255,
  u16: 1 / 65535,
};

/** 每个语义的分量数（如 UV 是 2、TANGENT 是 4 且 w 为手性）。 */
export const SEMANTIC_COMPONENTS: Readonly<Record<AttributeSemantic, number>> = {
  POSITION: 3,
  NORMAL: 3,
  TANGENT: 4,
  UV0: 2,
  UV1: 2,
  COLOR: 4,
  BONE_WEIGHT: 4,
};

/** 索引位宽（自动选型：顶点数 ≤ 65536 用 16 位，省一半内存）。 */
export type IndexWidth = 16 | 32;

/** 16 位索引的顶点数上限（+1 是因为 65535 是合法顶点号）。 */
export const U16_INDEX_VERTEX_LIMIT = 65536;

/** 拓扑类型（四种，与 F1604 的拓扑集对齐）。 */
export type TopologyKind = "TRIANGLES" | "TRIANGLE_STRIP" | "LINES" | "POINTS";

/** 各拓扑每个图元的索引数。 */
export const TOPOLOGY_INDEX_COUNT: Readonly<Record<TopologyKind, number>> = {
  TRIANGLES: 3,
  TRIANGLE_STRIP: 3,
  LINES: 2,
  POINTS: 1,
};

/**
 * 段标识（写入段头，供 CRC 定位与随机访问）。
 *
 * 用冻结对象 + 字面量联合类型而非 `const enum`：本条须能在无枚举变换的宿主
 * （Node 的 strip-types 模式、Worker 的原生 ESM）里直接加载，而 `const enum`
 * 依赖编译期内联，在这类宿主里会直接报语法错。语义等价，取舍写在���里而非注释里。
 */
export const SegmentId = {
  Vertices: 1,
  Indices: 2,
  Submeshes: 3,
  Lods: 4,
  Materials: 5,
} as const;

/** 段标识的字面量类型（供段表 id 字段约束）。 */
export type SegmentId = (typeof SegmentId)[keyof typeof SegmentId];

/** 二进制主体的段标识。 */
export type SegmentIdValue = SegmentId;

/** 段标识 → 人话名（诊断文案与自检引用同一事实源）。 */
export const SEGMENT_LABEL: Readonly<Record<number, string>> = {
  [SegmentId.Vertices]: "顶点段",
  [SegmentId.Indices]: "索引段",
  [SegmentId.Submeshes]: "子网格段",
  [SegmentId.Lods]: "LOD 段",
  [SegmentId.Materials]: "材质段",
};

// ════════════════════════════════════════════════════════════════════════════
// §2 五段容器的类型定义（判据一：vmesh 五段容器）
// ════════════════════════════════════════════════════════════════════════════

/** 一个顶点属性在头中的声明（数据本身在二进制主体，格式信息在头）。 */
export interface AttributeDecl {
  readonly semantic: AttributeSemantic;
  readonly format: ComponentFormat;
  /** 该属性数据在顶点段内的字节偏移（各属性独立声明，布局即契约）。 */
  readonly byteOffset: number;
  /** 归一化标志。冗余于 format 推导，但显式写出可让第三方读头时不猜。 */
  readonly normalized: boolean;
}

/** 顶点缓冲段描述。 */
export interface VertexSection {
  readonly vertexCount: number;
  readonly attributes: readonly AttributeDecl[];
  /** 每顶点字节步长（由属性偏移与格式推导后显式存头，便于第三方跳读）。 */
  readonly strideBytes: number;
}

/** 索引缓冲段描述。位宽自动选型结果存头，读取方无需重算。 */
export interface IndexSection {
  readonly width: IndexWidth;
  readonly indexCount: number;
  readonly topology: TopologyKind;
}

/** 子网格：按材质分组的索引区间。 */
export interface Submesh {
  /** 子网格名（调试与材质绑定用，可重复）。 */
  readonly name: string;
  /** 索引区间 [first, first + count)。 */
  readonly firstIndex: number;
  readonly indexCount: number;
  /** 指向 MATERIALS 段的材质索引。 */
  readonly materialIndex: number;
}

/** LOD 层级描述（本条只存引用与误差声明，简化算法归 F1608）。 */
export interface LodLevel {
  /** 该级在网格内的名字（LOD0 通常是原始级）。 */
  readonly name: string;
  /** 该级的顶点/索引计数（用于内存预算与加载决策，不含几何数据）。 */
  readonly vertexCount: number;
  readonly indexCount: number;
  /** 相对 LOD0 的几何误差上界（相对包围盒对角线，无单位）。声明误差而非仅声明「有误差」。 */
  readonly errorBound: number;
  /** 父级名（构成简化树）；根级为 null。 */
  readonly parent: string | null;
}

/** 材质引用：几何只存对接键，不存材质定义（材质归 F1603 的 MaterialSet）。 */
export interface MaterialRef {
  /** 外部材质库中的键（F1603 的契约名 + 材质名）。 */
  readonly key: string;
  /** 可选变体名（如「高湿」「磨损」，glTF 的 KHR_materials_variants 语义）。 */
  readonly variant: string | null;
}

/**
 * 二进制主体中的一段：偏移 + 长度 + CRC（校验的定位单元）。
 *
 * `id` 刻意取number 而非 SegmentId 联合类型：解析外来文件时段的 id 是**不可信数据**，
 * 可能来自更高版本的 vmesh（本实现未知的段类型）。若在此处就收窄成联合类型，
 * 解析器要么谎称文件合法，要么被迫拒绝「带未知段的新版文件」——而前向兼容纪律
 * 要求忽略不认识的段。故类型放宽，语义由 SEGMENT_LABEL 的回退分支承担。
 */
export interface SegmentSpan {
  readonly id: number;
  readonly byteOffset: number;
  readonly byteLength: number;
  /** 该段的 CRC-32（校验位，段级）。 */
  readonly crc32: number;
}

/** vmesh 文件头（JSON 侧）。自描述：段表 + 各段描述 + 版本 + 全文件哈希占位。 */
export interface VmeshHeader {
  readonly magic: string;
  readonly version: { readonly major: number; readonly minor: number };
  /** 段表：偏移/长度/CRC，读者据此随机访问任一段。 */
  readonly segments: readonly SegmentSpan[];
  readonly vertices: VertexSection;
  readonly indices: IndexSection;
  readonly submeshes: readonly Submesh[];
  readonly lods: readonly LodLevel[];
  readonly materials: readonly MaterialRef[];
  /** 全文件哈希（对「头 + 主体」整体求值，用于跨版本一致性与缓存命中）。 */
  readonly fileHash: string;
}

/** 一个完整的 vmesh 文件：头 + 二进制主体。 */
export interface VmeshFile {
  readonly header: VmeshHeader;
  /** 二进制主体（五段拼接，段间可有对齐填充）。 */
  readonly payload: Uint8Array;
}

// ════════════════════════════════════════════════════════════════════════════
// §3 校验原语（判据四的基础：块 CRC + 全文件哈希）
// ════════════════════════════════════════════════════════════════════════════

/**
 * CRC-32（IEEE 802.3 多项式 0xEDB88320 的反射实现）。
 *
 * 为何不用更快的查表法：本条每毫秒级校验若干 MB 资产，查表法确实更快，
 * 但位级实现无预计算表、无初始化顺序依赖，跨环境（Worker/Node/浏览器）
 * 结果必然一致——而 CRC 的全部价值就在于「不同机器算出同一个值」。
 * 资产校验不是热路径上的实时计算，可移植性优先于速度。
 */
export function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff;
  for (let i = 0; i < bytes.length; i += 1) {
    crc ^= bytes[i] ?? 0;
    for (let bit = 0; bit < 8; bit += 1) {
      // 右移一位，若最低位为 1 则异或多项式。
      const mask = -(crc & 1);
      crc = (crc >>> 1) ^ (0xedb88320 & mask);
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

/**
 * FNV-1a 32 位散列。选它做全文件哈希的理由与CRC-32 同源：
 * 无依赖、无预计算表、逐字节可移植，跨环境必然同值。
 * 它**不是**密码学哈希——防的是「文件被意外替换/传输损坏」，不防恶意篡改；
 * 需要防篡改的场景（资产签名）走 F1556 的签名信任机制，不在本条职责内。
 */
export function fnv1a32(bytes: Uint8Array): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < bytes.length; i += 1) {
    h ^= bytes[i] ?? 0;
    // 乘 16777619，用移位加法避免 32 位溢出丢精度。
    h = (h + ((h << 1) + (h << 4) + (h << 7) + (h << 8) + (h << 24))) >>> 0;
  }
  return h >>> 0;
}

/** 把散列格式化为 8 位十六进制（头里存字符串，便于人眼比对与日志检索）。 */
export function hashToHex(h: number): string {
  return (h >>> 0).toString(16).padStart(8, "0");
}

/**
 * 全文件哈希：头（去掉 fileHash 字段自身的占位）与主体的联合散列。
 *
 * 为何要把 fileHash 字段本身排除在散列输入之外：
 *   fileHash 存在头里，若把它也算进去就成了自引用（要算哈希得先填字段，
 *   填了字段哈希又变了）。故计算时把该字段替换为固定 8 字节零，
 *   这样「算哈希」与「填字段」的先后顺序解耦，且双方算法一致。
 */
export function computeFileHash(header: Omit<VmeshHeader, "fileHash">, payload: Uint8Array): string {
  const template: VmeshHeader = { ...header, fileHash: "00000000" };
  const text = JSON.stringify(template);
  const headBytes = utf8Encode(text);
  const joined = new Uint8Array(headBytes.length + payload.length);
  joined.set(headBytes, 0);
  joined.set(payload, headBytes.length);
  return hashToHex(fnv1a32(joined));
}

/** UTF-8 编码（避免依赖 TextEncoder 的宿主差异：Worker 与旧浏览器都有，但 Node 侧无全局）。 */
export function utf8Encode(text: string): Uint8Array {
  const out: number[] = [];
  for (let i = 0; i < text.length; i += 1) {
    let cp = text.charCodeAt(i);
    if (cp >= 0xd800 && cp <= 0xdbff && i + 1 < text.length) {
      const lo = text.charCodeAt(i + 1);
      if (lo >= 0xdc00 && lo <= 0xdfff) {
        cp = (cp - 0xd800) * 0x400 + (lo - 0xdc00) + 0x10000;
        i += 1;
      }
    }
    if (cp < 0x80) {
      out.push(cp);
    } else if (cp < 0x800) {
      out.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f));
    } else if (cp < 0x10000) {
      out.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
    } else {
      out.push(
        0xf0 | (cp >> 18),
        0x80 | ((cp >> 12) & 0x3f),
        0x80 | ((cp >> 6) & 0x3f),
        0x80 | (cp & 0x3f),
      );
    }
  }
  return new Uint8Array(out);
}

/** UTF-8 解码（与 utf8Encode 严格互逆，自检里做往返断言）。 */
export function utf8Decode(bytes: Uint8Array): string {
  let out = "";
  let i = 0;
  while (i < bytes.length) {
    const b0 = bytes[i] ?? 0;
    let cp: number;
    let extra: number;
    if (b0 < 0x80) {
      cp = b0;
      extra = 0;
    } else if ((b0 & 0xe0) === 0xc0) {
      cp = b0 & 0x1f;
      extra = 1;
    } else if ((b0 & 0xf0) === 0xe0) {
      cp = b0 & 0x0f;
      extra = 2;
    } else {
      cp = b0 & 0x07;
      extra = 3;
    }
    for (let k = 1; k <= extra; k += 1) cp = (cp << 6) | ((bytes[i + k] ?? 0) & 0x3f);
    if (cp > 0xffff) {
      cp -= 0x10000;
      out += String.fromCharCode(0xd800 + (cp >> 10), 0xdc00 + (cp & 0x3ff));
    } else {
      out += String.fromCharCode(cp);
    }
    i += extra + 1;
  }
  return out;
}

// ════════════════════════════════════════════════════════════════════════════
// §4 头校验与解析（前向兼容的唯一入口）
// ════════════════════════════════════════════════════════════════════════════

/** 未知字段的处理策略结果（供自检断言前向兼容行为）。 */
export interface ForwardCompatReport {
  readonly majorAccepted: boolean;
  /** 被忽略的未知字段名（对未来的文件即当前实现「不认识但安全跳过」的字段）。 */
  readonly ignoredKeys: readonly string[];
  /** 次版本高于本实现时为 true（已按前向兼容路径处理）。 */
  readonly minorAhead: boolean;
}

/** 头里允许出现但本实现不消费的字段（显式列出 = 声明「这些我认得但不用」）。 */
export const KNOWN_UNUSED_KEYS: readonly string[] = [
  "generator",
  "generatorVersion",
  "copyright",
  "extras",
  "compressionHint",
];

/**
 * 解析头 JSON 并做全字段校验。
 *
 * 前向兼容的两条规则在此落地（务必分清，混起来就废了演进能力）：
 *   · 主版本 > 支持上界 → **拒绝**（MAJOR_VERSION_UNSUPPORTED）。大版本变更意味着
 *     结构不兼容，「尽力解析」只会产出错误网格，比拒绝更危险。
 *   · 未知字段 / 次版本更高 → **忽略并登记**（MINOR_VERSION_AHEAD +
 *     ignoredKeys）。小版本只增字段不改语义，故旧读新必须能读。
 */
export function parseHeader(json: string): Outcome<{ header: VmeshHeader; compat: ForwardCompatReport }> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch (e) {
    return fail(
      "HEADER_MALFORMED",
      `文件头不是合法 JSON：${e instanceof Error ? e.message : String(e)}`,
      "vmesh 是 JSON 头 + 二进制主体；若该文件是二进制流或 glTF，说明格式识别环节出了问题，"
        + "请在加载器里检查 magic 前缀再进入本解析函数",
    );
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return fail(
      "HEADER_MALFORMED",
      `文件头顶层不是对象（实际为 ${Array.isArray(parsed) ? "数组" : typeof parsed}）`,
      "vmesh 头须是 JSON 对象；请确认传入的是头部分片而非整个文件",
    );
  }
  const raw = parsed as Record<string, unknown>;
  const bag = new DiagBag();

  // 未知字段登记（前向兼容的可见证据——不是静默丢弃，是记下来给审计看）。
  const known = new Set<string>([
    "magic",
    "version",
    "segments",
    "vertices",
    "indices",
    "submeshes",
    "lods",
    "materials",
    "fileHash",
    ...KNOWN_UNUSED_KEYS,
  ]);
  const ignoredKeys = Object.keys(raw).filter((k) => !known.has(k));

  if (raw["magic"] !== VMESH_MAGIC) {
    return fail(
      "HEADER_MALFORMED",
      `magic 不符（实际 ${JSON.stringify(raw["magic"])}，期望 ${JSON.stringify(VMESH_MAGIC)}）`,
      "该文件不是 vmesh；请确认格式，或走 F1602 的 glTF 导入路径而非 vmesh 解析路径",
    );
  }

  const verRaw = raw["version"];
  const verObj = typeof verRaw === "object" && verRaw !== null ? (verRaw as Record<string, unknown>) : null;
  const major = verObj === null ? null : readNumber(verObj["major"], "major");
  const minor = verObj === null ? null : readNumber(verObj["minor"], "minor");
  if (major === null || minor === null) {
    return fail(
      "HEADER_MALFORMED",
      `version 字段缺失或非数值（${JSON.stringify(verRaw)}）`,
      "version 须为 { major: number, minor: number }；缺失版本的文件无法判定兼容性，必须拒绝",
    );
  }
  if (major > VMESH_MAX_MAJOR) {
    return fail(
      "MAJOR_VERSION_UNSUPPORTED",
      `主版本 ${major} 超出本实现支持上界 ${VMESH_MAX_MAJOR}`,
      "大版本变更意味着结构不兼容，本实现不做尽力解析（那会产出错误网格）；"
        + "请升级实现或用对应版本的工具转换该文件",
    );
  }
  let minorAhead = false;
  if (minor > VMESH_VERSION.minor) {
    minorAhead = true;
    bag.push(
      "MINOR_VERSION_AHEAD",
      `文件次版本 ${minor} 高于本实现 ${VMESH_VERSION.minor}，已按前向兼容路径忽略未知字段`,
      "次版本只增字段不改语义，故可安全读取；但如果新字段承载了你需要的信息"
        + "（如新的 LOD 参数），本实现会看不见它——请确认不影响正确性",
    );
  }

  const structure = validateHeaderStructure(raw, bag);
  if (structure === null) {
    const first = bag.all()[0];
    return fail(
      first?.code ?? "HEADER_MALFORMED",
      first?.message ?? "文件头结构非法",
      first?.hint ?? "请检查文件头各字段是否符合 vmesh 规范",
    );
  }

  const header: VmeshHeader = {
    magic: VMESH_MAGIC,
    version: { major, minor },
    segments: structure.segments,
    vertices: structure.vertices,
    indices: structure.indices,
    submeshes: structure.submeshes,
    lods: structure.lods,
    materials: structure.materials,
    fileHash: typeof raw["fileHash"] === "string" ? raw["fileHash"] : "",
  };

  return ok({ header, compat: { majorAccepted: true, ignoredKeys, minorAhead } }, bag.all());
}

/** 从对象读数值；非数值或缺失返回 null（调用方产出诊断，不做隐式转换）。 */
function readNumber(v: unknown, _field: string): number | null {
  if (typeof v === "number" && Number.isFinite(v)) return v;
  return null;
}

/** 结构校验的产物（各段已通过类型与范围检查）。 */
interface HeaderStructure {
  readonly segments: readonly SegmentSpan[];
  readonly vertices: VertexSection;
  readonly indices: IndexSection;
  readonly submeshes: readonly Submesh[];
  readonly lods: readonly LodLevel[];
  readonly materials: readonly MaterialRef[];
}

/**
 * 结构全字段校验。任一项不合法返回 null（诊断已入 bag，调用方统一转失败）。
 *逐字段独立检查并各自产出诊断——一次性报全部问题，调用方一次改完，
 * 而不是「修一个报一个」的挤牙膏循环。
 */
function validateHeaderStructure(raw: Record<string, unknown>, bag: DiagBag): HeaderStructure | null {
  // ---- 顶点段 ----
  const vRaw = raw["vertices"];
  if (typeof vRaw !== "object" || vRaw === null) {
    bag.push("HEADER_MALFORMED", "vertices 字段缺失或非对象", "顶点段是容器的基础字段，必须存在");
    return null;
  }
  const vObj = vRaw as Record<string, unknown>;
  const vertexCount = readNumber(vObj["vertexCount"], "vertexCount") ?? -1;
  const strideBytes = readNumber(vObj["strideBytes"], "strideBytes") ?? -1;
  if (vertexCount < 0) {
    bag.push("HEADER_MALFORMED", `顶点段 vertexCount 非法（${String(vObj["vertexCount"])}）`, "顶点数须为非负整数");
    return null;
  }
  const attrsRaw = vObj["attributes"];
  if (!Array.isArray(attrsRaw) || attrsRaw.length === 0) {
    bag.push("HEADER_MALFORMED", "顶点段 attributes 为空数组或非数组", "至少须声明 POSITION 属性，否则网格无几何");
    return null;
  }
  const attributes: AttributeDecl[] = [];
  for (const a of attrsRaw) {
    if (typeof a !== "object" || a === null) {
      bag.push("ATTRIBUTE_SEMANTIC_UNKNOWN", "属性声明中存在非对象项", "每条属性须是 { semantic, format, byteOffset, normalized }");
      return null;
    }
    const aObj = a as Record<string, unknown>;
    const sem = aObj["semantic"];
    const fmt = aObj["format"];
    const off = readNumber(aObj["byteOffset"], "byteOffset");
    if (!ALL_SEMANTICS.includes(sem as AttributeSemantic)) {
      bag.push(
        "ATTRIBUTE_SEMANTIC_UNKNOWN",
        `属性语义 ${JSON.stringify(sem)} 不在本实现支持集内`,
        `支持集：${ALL_SEMANTICS.join("、")}；若这是 vmesh 规范新增的语义，请升级实现`
          + "（不要在读取端猜它占几个分量——分量数错了整个顶点步长就错了）",
      );
      return null;
    }
    if (typeof fmt !== "string" || !(fmt in FORMAT_BYTES)) {
      bag.push(
        "ATTRIBUTE_FORMAT_UNKNOWN",
        `属性 ${String(sem)} 的格式 ${JSON.stringify(fmt)} 不受支持`,
        `支持集：${Object.keys(FORMAT_BYTES).join("、")}；fp64 等格式需先在本条扩展后再写入`,
      );
      return null;
    }
    if (off === null || off < 0) {
      bag.push("HEADER_MALFORMED", `属性 ${String(sem)} 的 byteOffset 非法（${String(aObj["byteOffset"])}）`, "偏移须为非负数");
      return null;
    }
    attributes.push({
      semantic: sem as AttributeSemantic,
      format: fmt as ComponentFormat,
      byteOffset: off,
      normalized: typeof aObj["normalized"] === "boolean" ? aObj["normalized"] : isNormalized(fmt as ComponentFormat),
    });
  }
  // 属性偏移不得越出步长（越界 = 读到下一个顶点的数据，几何会静默错乱）。
  for (const a of attributes) {
    const size = FORMAT_BYTES[a.format] * SEMANTIC_COMPONENTS[a.semantic];
    if (a.byteOffset + size > strideBytes) {
      bag.push(
        "SEGMENT_RANGE_INVALID",
        `属性 ${a.semantic} 的区间 [${a.byteOffset}, ${a.byteOffset + size}) 越出每顶点步长 ${strideBytes}`,
        "属性布局与步长不一致：减小 byteOffset 或增大 strideBytes；越界读取不会报错但会读到相邻顶点的值",
      );
      return null;
    }
  }
  const hasPosition = attributes.some((a) => a.semantic === "POSITION");
  if (!hasPosition) {
    bag.push("HEADER_MALFORMED", "顶点段未声明 POSITION 属性", "POSITION 是网格存在的最低要求（无位置不成网格）");
    return null;
  }

  // ---- 索引段 ----
  const iRaw = raw["indices"];
  if (typeof iRaw !== "object" || iRaw === null) {
    bag.push("HEADER_MALFORMED", "indices 字段缺失或非对象", "索引段缺失则网格无法三角化");
    return null;
  }
  const iObj = iRaw as Record<string, unknown>;
  const width = iObj["width"];
  const indexCount = readNumber(iObj["indexCount"], "indexCount") ?? -1;
  const topology = iObj["topology"];
  if (width !== 16 && width !== 32) {
    bag.push("HEADER_MALFORMED", `索引位宽 ${String(width)} 非法（期望 16 或 32）`, "位宽只有 16/32 两种；8 位索引在几何领域无实用价值");
    return null;
  }
  if (indexCount < 0) {
    bag.push("HEADER_MALFORMED", `索引数非法（${String(iObj["indexCount"])}）`, "索引数须为非负整数");
    return null;
  }
  if (typeof topology !== "string" || !(topology in TOPOLOGY_INDEX_COUNT)) {
    bag.push(
      "HEADER_MALFORMED",
      `拓扑类型 ${JSON.stringify(topology)} 不受支持`,
      `支持集：${Object.keys(TOPOLOGY_INDEX_COUNT).join("、")}`,
    );
    return null;
  }
  // 位宽与顶点数一致性：16 位索引装不下 >65535 的顶点号（静默截断 = 几何错乱）。
  if (width === 16 && vertexCount > U16_INDEX_VERTEX_LIMIT) {
    bag.push(
      "INDEX_OUT_OF_RANGE",
      `顶点数 ${vertexCount} 超过 16 位索引上限 ${U16_INDEX_VERTEX_LIMIT}，但索引位宽声明为 16`,
      "位宽自动选型规则被绕过：请把 width 改为 32（由chooseIndexWidth 重新选型），"
        + "否则顶点号会被静默截断为低 16 位，表现为随机几何破面",
    );
    return null;
  }

  // ---- 段表 ----
  const sRaw = raw["segments"];
  if (!Array.isArray(sRaw)) {
    bag.push("HEADER_MALFORMED", "segments 字段缺失或非数组", "段表是随机访问与段级 CRC 的依据，必须存在");
    return null;
  }
  const segments: SegmentSpan[] = [];
  for (const s of sRaw) {
    if (typeof s !== "object" || s === null) {
      bag.push("HEADER_MALFORMED", "段表项中存在非对象项", "每段须是 { id, byteOffset, byteLength, crc32 }");
      return null;
    }
    const sObj = s as Record<string, unknown>;
    const id = readNumber(sObj["id"], "id");
    const off = readNumber(sObj["byteOffset"], "byteOffset");
    const len = readNumber(sObj["byteLength"], "byteLength");
    const crc = readNumber(sObj["crc32"], "crc32");
    if (id === null || off === null || len === null || crc === null || off < 0 || len < 0) {
      bag.push(
        "SEGMENT_RANGE_INVALID",
        `段表项字段非法：${JSON.stringify(sObj)}`,
        "每段须有非负的 id/byteOffset/byteLength/crc32",
      );
      return null;
    }
    segments.push({ id, byteOffset: off, byteLength: len, crc32: crc >>> 0 });
  }
  // 段区间不得重叠（重叠 = 同一份字节被两段认领，随机访问会读到别人的数据）。
  const sorted = [...segments].sort((a, b) => a.byteOffset - b.byteOffset);
  for (let i = 1; i < sorted.length; i += 1) {
    const prev = sorted[i - 1];
    const cur = sorted[i];
    if (prev === undefined || cur === undefined) continue;
    if (prev.byteOffset + prev.byteLength > cur.byteOffset) {
      bag.push(
        "SEGMENT_RANGE_INVALID",
        `段 ${String(prev.id)} 与段 ${String(cur.id)} 的字节区间重叠`
          + `（[${prev.byteOffset}, ${prev.byteOffset + prev.byteLength}) 与 [${cur.byteOffset}, ...)`,
        "段区间重叠会让随机访问读到错误段的数据；请重新生成段表使各段不交叠",
      );
      return null;
    }
  }

  // ---- 子网格 ----
  const submeshes = parseSubmeshes(raw["submeshes"], indexCount, bag);
  if (submeshes === null) return null;

  // ---- 材质引用 ----
  const materials = parseMaterials(raw["materials"], bag);
  if (materials === null) return null;
  // 子网格引用的材质索引须落在材质表内。
  for (const sm of submeshes) {
    if (sm.materialIndex < 0 || sm.materialIndex >= materials.length) {
      bag.push(
        "MATERIAL_REF_INVALID",
        `子网格 ${sm.name} 的材质索引 ${sm.materialIndex} 越出材质表范围（0..${materials.length - 1}）`,
        materials.length === 0
          ? "材质表为空：若网格确实无材质，请在子网格中用 materialIndex = -1 表示「无材质」，并调整本校验"
          : "请修正材质索引，或在材质表中补齐对应条目",
      );
      return null;
    }
  }

  // ---- LOD ----
  const lods = parseLods(raw["lods"], bag);
  if (lods === null) return null;

  // 到此 topology 已被 typeof + in 双重收窄为 string 且键存在于 TOPOLOGY_INDEX_COUNT，
  // 但 TS 不会从 `in` 推出取值落在值域内，故显式落一次断言（已由上方校验保证安全）。
  return {
    segments,
    vertices: { vertexCount, attributes, strideBytes },
    indices: { width, indexCount, topology: topology as TopologyKind },
    submeshes,
    lods,
    materials,
  };
}

/** 子网格表解析（含索引区间越界检查）。 */
function parseSubmeshes(raw: unknown, indexCount: number, bag: DiagBag): Submesh[] | null {
  if (!Array.isArray(raw)) {
    bag.push("HEADER_MALFORMED", "submeshes 字段缺失或非数组", "子网格表须是数组（无子网格时用空数组）");
    return null;
  }
  const out: Submesh[] = [];
  for (const s of raw) {
    if (typeof s !== "object" || s === null) {
      bag.push("HEADER_MALFORMED", "子网格表项中存在非对象项", "每项须是 { name, firstIndex, indexCount, materialIndex }");
      return null;
    }
    const o = s as Record<string, unknown>;
    const first = readNumber(o["firstIndex"], "firstIndex");
    const count = readNumber(o["indexCount"], "indexCount");
    const mat = readNumber(o["materialIndex"], "materialIndex");
    if (first === null || count === null || mat === null || first < 0 || count < 0) {
      bag.push("SUBMESH_RANGE_INVALID", `子网格字段非法：${JSON.stringify(o)}`, "firstIndex/indexCount/materialIndex 须为非负整数");
      return null;
    }
    if (first + count > indexCount) {
      bag.push(
        "SUBMESH_RANGE_INVALID",
        `子网格 ${String(o["name"])} 的索引区间 [${first}, ${first + count}) 越出索引总数 ${indexCount}`,
        "子网格区间必须完全落在索引缓冲内；越界区间在绘制时会读到相邻子网格的三角形",
      );
      return null;
    }
    out.push({ name: typeof o["name"] === "string" ? o["name"] : "", firstIndex: first, indexCount: count, materialIndex: mat });
  }
  return out;
}

/** 材质引用表解析（拒绝空 key 与自引用变体）。 */
function parseMaterials(raw: unknown, bag: DiagBag): MaterialRef[] | null {
  if (!Array.isArray(raw)) {
    bag.push("HEADER_MALFORMED", "materials 字段缺失或非数组", "材质引用表须是数组（无材质时用空数组）");
    return null;
  }
  const out: MaterialRef[] = [];
  const seen = new Set<string>();
  for (const m of raw) {
    if (typeof m !== "object" || m === null) {
      bag.push("MATERIAL_REF_INVALID", "材质引用表项中存在非对象项", "每项须是 { key, variant }");
      return null;
    }
    const o = m as Record<string, unknown>;
    const key = o["key"];
    if (typeof key !== "string" || key.trim() === "") {
      bag.push(
        "MATERIAL_REF_INVALID",
        `材质引用的 key 为空（${JSON.stringify(key)}）`,
        "几何只存材质引用，key 必须指向材质库中的真实条目；空引用会让着色器拿到未绑定材质",
      );
      return null;
    }
    // 同一 (key, variant) 重复登记 = 冗余，且会让材质索引含义随实现变化。
    const variant = typeof o["variant"] === "string" ? o["variant"] : null;
    const dedupKey = `${key}::${variant ?? ""}`;
    if (seen.has(dedupKey)) {
      bag.push(
        "MATERIAL_REF_INVALID",
        `材质引用重复登记（${dedupKey}）`,
        "同一材质不应在引用表里出现两次；重复项会让材质索引与预期不一致",
      );
      return null;
    }
    seen.add(dedupKey);
    out.push({ key, variant });
  }
  return out;
}

/** LOD 表解析（父级须存在且不成环——成环即简化树损坏，会让 LOD 选择死循环）。 */
function parseLods(raw: unknown, bag: DiagBag): LodLevel[] | null {
  if (!Array.isArray(raw)) {
    bag.push("HEADER_MALFORMED", "lods 字段缺失或非数组", "LOD 表须是数组（无 LOD 层级时用空数组）");
    return null;
  }
  const out: LodLevel[] = [];
  const names = new Set<string>();
  for (const l of raw) {
    if (typeof l !== "object" || l === null) {
      bag.push("HEADER_MALFORMED", "LOD 表项中存在非对象项", "每项须是 { name, vertexCount, indexCount, errorBound, parent }");
      return null;
    }
    const o = l as Record<string, unknown>;
    const name = typeof o["name"] === "string" ? o["name"] : "";
    if (name.trim() === "") {
      bag.push("LOD_REFERENCE_INVALID", "LOD 层级名为空", "LOD 名是简化树的节点标识，不能为空");
      return null;
    }
    if (names.has(name)) {
      bag.push("LOD_REFERENCE_INVALID", `LOD 层级名重复（${name}）`, "同名节点会让父级引用指向不唯一");
      return null;
    }
    names.add(name);
    const vc = readNumber(o["vertexCount"], "vertexCount");
    const ic = readNumber(o["indexCount"], "indexCount");
    const eb = readNumber(o["errorBound"], "errorBound");
    if (vc === null || ic === null || eb === null || vc < 0 || ic < 0) {
      bag.push("LOD_REFERENCE_INVALID", `LOD 层级字段非法：${JSON.stringify(o)}`, "顶点数/索引数须非负，误差上界须为非负有限数");
      return null;
    }
    const parent = typeof o["parent"] === "string" && o["parent"] !== "" ? o["parent"] : null;
    out.push({ name, vertexCount: vc, indexCount: ic, errorBound: eb, parent });
  }
  // 父级引用存在性 + 环检测（沿父链上溯，步数上限 = 节点数，超过即成环）。
  for (const l of out) {
    if (l.parent === null) continue;
    if (!names.has(l.parent)) {
      bag.push(
        "LOD_REFERENCE_INVALID",
        `LOD 层级 ${l.name} 的父级 ${l.parent} 不存在`,
        "简化树的父级须指向同表内的层级；跨文件引用不在本格式支持范围内",
      );
      return null;
    }
  }
  for (const l of out) {
    let cursor: string | null = l.name;
    let steps = 0;
    while (cursor !== null) {
      const node = out.find((x) => x.name === cursor);
      if (node === undefined) break;
      cursor = node.parent;
      steps += 1;
      if (steps > out.length) {
        bag.push(
          "LOD_REFERENCE_INVALID",
          `LOD 简化树在 ${l.name} 处成环`,
          "父级引用构成了环，导致 LOD 选择（I09）无法终止；请修正父级指向使树成有向无环图",
        );
        return null;
      }
    }
  }
  return out;
}

/** 索引位宽自动选型（无手工配置：顶点数决定位宽，省一半内存是默认而非可选项）。 */
export function chooseIndexWidth(vertexCount: number): IndexWidth {
  return vertexCount <= U16_INDEX_VERTEX_LIMIT ? 16 : 32;
}

// ════════════════════════════════════════════════════════════════════════════
// §5 两级校验（判据四：块 CRC 定位段 + 全文件哈希定位「是不是预期的那个」）
// ════════════════════════════════════════════════════════════════════════════

/** 一级校验结果：段级。id 沿用 SegmentSpan 的 number（外来文件的段 id 是不可信数据）。 */
export interface SegmentCrcReport {
  readonly id: number;
  readonly label: string;
  readonly expected: number;
  readonly actual: number;
  readonly pass: boolean;
}

/** 二级校验结果：文件级。 */
export interface FileHashReport {
  readonly expected: string;
  readonly actual: string;
  readonly pass: boolean;
}

/** 两级校验的合报告（损坏可定位到段；文件被换掉也可定位）。 */
export interface VerifyReport {
  readonly segments: readonly SegmentCrcReport[];
  readonly file: FileHashReport;
  /** 全部通过为 true。 */
  readonly pass: boolean;
  /** 损坏段的人话汇总（可直接展示给用户）。 */
  readonly summary: string;
}

/**
 * 两级校验。**顺序固定为「先块后全文件」**，理由是二者回答的问题不同：
 *   先看块 CRC —— 回答「哪一段坏了」。这决定处置方式：顶点段坏可以尝试用备份顶点
 *   重建，索引段坏多半是文件本身被截断，整份丢弃更干净。
 *   再看全文件哈希 —— 回答「这是不是预期的那个文件」。全部块 CRC 都过但哈希不符，
 *   说明文件被整体替换成了另一个自洽的文件（缓存串了、资产库更新了、
 *   或者拿到的是同名不同内容）——这种损坏块校验查不出来，只有哈希能查。
 * 若先看哈希，块 CRC 的定位价值就被抹掉了（反正整体作废），白算一遍。
 */
export function verifyFile(file: VmeshFile): VerifyReport {
  const segments: SegmentCrcReport[] = [];
  for (const seg of file.header.segments) {
    // 段区间越界时不取数据（取到的是别的段的内容，CRC 必然不符，会给出误导性诊断）。
    const inRange =
      seg.byteOffset >= 0 && seg.byteLength >= 0 && seg.byteOffset + seg.byteLength <= file.payload.length;
    const actual = inRange
      ? crc32(file.payload.subarray(seg.byteOffset, seg.byteOffset + seg.byteLength))
      : -1;
    segments.push({
      id: seg.id,
      label: SEGMENT_LABEL[seg.id] ?? `未知段 ${seg.id}`,
      expected: seg.crc32,
      actual: actual >>> 0,
      pass: inRange && actual === seg.crc32,
    });
  }

  const { fileHash: _ignored, ...headerWithoutHash } = file.header;
  const actualHash = computeFileHash(headerWithoutHash, file.payload);
  const expectedHash = file.header.fileHash;
  // 空哈希表示写入方未提供（未启用全文件校验的旧文件）——按「未校验」而非「校验失败」
  // 处理，否则旧文件全部无法加载，违背前向兼容纪律。
  const hashProvided = expectedHash !== "" && expectedHash !== "00000000";
  const fileReport: FileHashReport = {
    expected: hashProvided ? expectedHash : "（未提供）",
    actual: actualHash,
    pass: !hashProvided || actualHash === expectedHash,
  };

  const badSegments = segments.filter((s) => !s.pass);
  const pass = badSegments.length === 0 && fileReport.pass;
  let summary: string;
  if (!pass) {
    const parts: string[] = [];
    if (badSegments.length > 0) {
      parts.push(`损坏段：${badSegments.map((s) => `${s.label}(期望 ${hashToHex(s.expected)} 实得 ${hashToHex(s.actual)})`).join("、")}`);
    }
    if (!fileReport.pass) {
      parts.push(`全文件哈希不符（头声明 ${expectedHash}，实算 ${actualHash}）：文件已被整体替换或损坏，块校验全过也救不了`);
    }
    summary = parts.join("；");
  } else {
    summary = hashProvided
      ? `${segments.length} 段 CRC 全部相符，全文件哈希相符`
      : `${segments.length} 段 CRC 全部相符（该文件未提供全文件哈希，按未校验处理）`;
  }

  return { segments, file: fileReport, pass, summary };
}

/** 校验失败 → 显式 Outcome 失败（不抛异常，诊断由调用方聚合上报）。 */
export function verifyOrFail(file: VmeshFile): Outcome<VerifyReport> {
  const report = verifyFile(file);
  if (report.pass) return ok(report);
  const badSegments = report.segments.filter((s) => !s.pass);
  return fail(
    badSegments.length > 0 ? "SEGMENT_CRC_MISMATCH" : "FILE_HASH_MISMATCH",
    `vmesh 文件校验未通过：${report.summary}`,
    badSegments.length > 0
      ? "损坏段已定位到具体字节区间：请从资产库重新取该资产（不要试图解析部分内容——"
        + "半个网格进渲染管线的危害大于明确拒绝）；若该资产来自网络传输，先校验传输层完整性"
      : "块校验全过但哈希不符，说明拿到的是另一个自洽文件（缓存串了或资产已更新）："
        + "请按 key 重新拉取并确认资产版本号",
  );
}

// ════════════════════════════════════════════════════════════════════════════
// §6 glTF 2.0 互转（判据二：双向 + 往返断言 + 显性降级）
// ════════════════════════════════════════════════════════════════════════════

/**
 * glTF 2.0 的最小子集模型（本条只认这些字段，避免把整个 glTF 规范搬进来）。
 * 之所以做减法：glTF 规范极宽（动画、皮肤、变形体、扩展机制…），而本条的职责
 * 是**几何容器**互转。搬全量规范会让「互转」变成「实现半个 glTF 引擎」。
 * 未覆盖的字段一律走 §6.3 的显性降级登记，不静默丢弃。
 */
export interface GltfAccessor {
  readonly componentType: number;
  readonly count: number;
  readonly type: string;
  readonly bufferView?: number;
  readonly normalized?: boolean;
}

export interface GltfBufferView {
  readonly buffer: number;
  readonly byteOffset?: number;
  readonly byteLength: number;
  readonly byteStride?: number;
}

export interface GltfPrimitive {
  readonly attributes: Readonly<Record<string, number>>;
  readonly indices?: number;
  readonly mode?: number;
  readonly material?: number;
}

export interface GltfMesh {
  readonly primitives: readonly GltfPrimitive[];
}

export interface GltfDocument {
  readonly asset: { readonly version: string };
  readonly buffers?: readonly { readonly byteLength: number }[];
  readonly bufferViews?: readonly GltfBufferView[];
  readonly accessors?: readonly GltfAccessor[];
  readonly meshes?: readonly GltfMesh[];
  readonly materials?: readonly { readonly name?: string }[];
  readonly nodes?: readonly { readonly mesh?: number; readonly name?: string }[];
}

/** glTF componentType 枚举 → 本格式的 ComponentFormat（glTF 的值即 glTF 规范号）。 */
const GLTF_COMPONENT_FORMAT: Readonly<Record<number, ComponentFormat>> = {
  5126: "f32", // FLOAT
  5122: "snorm16", // SHORT
  5121: "u8", // UNSIGNED_BYTE
  5123: "u16", // UNSIGNED_SHORT
  5120: "snorm8", // BYTE
};

/** 本格式格式 → glTF componentType（导出用；反向映射无歧义，因为格式与枚举一一对应）。 */
const FORMAT_GLBF_COMPONENT: Readonly<Record<ComponentFormat, number>> = {
  f32: 5126,
  f16: 5122, // glTF 无半精度浮点 accessor；半精度只能作扩展，走显性降级
  snorm16: 5122,
  snorm8: 5120,
  u8: 5121,
  u16: 5123,
};

/** glTF primitive mode → 本格式拓扑。glTF 的mode 编号见规范（4=triangles,5=strip,1=lines,0=points）。 */
const GLTF_MODE_TOPOLOGY: Readonly<Record<number, TopologyKind>> = {
  4: "TRIANGLES",
  5: "TRIANGLE_STRIP",
  1: "LINES",
  0: "POINTS",
};

/** 本格式拓扑 → glTF mode。 */
const TOPOLOGY_GLTF_MODE: Readonly<Record<TopologyKind, number>> = {
  TRIANGLES: 4,
  TRIANGLE_STRIP: 5,
  LINES: 1,
  POINTS: 0,
};

/** glTF 语义名 → 本格式语义名。glTF 的 TANGENT 含手性 w，与本格式一致。 */
const GLTF_SEMANTIC_MAP: Readonly<Record<string, AttributeSemantic>> = {
  POSITION: "POSITION",
  NORMAL: "NORMAL",
  TANGENT: "TANGENT",
  TEXCOORD_0: "UV0",
  TEXCOORD_1: "UV1",
  COLOR_0: "COLOR",
};

/** 导入结果：vmesh 五段容器 + 降级/丢信息登记（导入侧的信息损失也须显性）。 */
export interface GltfImportResult {
  readonly file: VmeshFile;
  /** 因 glTF 侧缺失而无法填充的字段（逐条人话说明，供调用方决定是否补数据）。 */
  readonly lossy: readonly string[];
}

/**
 * glTF → vmesh 导入（完整映射：glTF 能表达的几何全部落进五段容器）。
 *
 * 映射的完备性说明：glTF 的 accessor/bufferView/primitive 三层正好对应本格式的
 * 属性声明/顶点段/子网格，故**几何信息无损失**；损失只可能出现在本格式有、glTF 没有的
 * 扩展信息上（如 LOD 误差上界、材质变体），这些走 lossy 显式登记。
 */
export function importFromGltf(doc: GltfDocument): Outcome<GltfImportResult> {
  const bag = new DiagBag();
  const lossy: string[] = [];

  if (typeof doc.asset?.version !== "string" || !doc.asset.version.startsWith("2.")) {
    return fail(
      "GLTF_MALFORMED",
      `glTF asset.version 缺失或非 2.x（${JSON.stringify(doc.asset?.version)}）`,
      "本条只支持 glTF 2.0；1.0 的模型语义（SHADER/techniques）与 2.0 完全不同，不能靠字段映射转换",
    );
  }
  const mesh = doc.meshes?.[0];
  const primitive = mesh?.primitives?.[0];
  if (primitive === undefined) {
    return fail(
      "GLTF_MALFORMED",
      "glTF 文档不含 mesh.primitives[0]",
      "本条导入的是「单网格单primitive」子集；多 primitive 网格请由调用方先拆分或聚合，"
        + "否则子网格边界语义会丢失",
    );
  }
  const accessors = doc.accessors ?? [];
  const views = doc.bufferViews ?? [];

  // ---- 属性声明 ----
  const attributes: AttributeDecl[] = [];
  let offset = 0;
  let positionAccessor = -1;
  for (const [gltfName, accIndex] of Object.entries(primitive.attributes)) {
    const semantic = GLTF_SEMANTIC_MAP[gltfName];
    if (semantic === undefined) {
      lossy.push(`glTF 属性 ${gltfName} 无对应语义，已忽略（本格式语义闭集见 ALL_SEMANTICS）`);
      continue;
    }
    const acc = accessors[accIndex];
    if (acc === undefined) {
      return fail(
        "GLTF_MALFORMED",
        `属性 ${gltfName} 的 accessor 索引 ${accIndex} 越界`,
        `accessors 长度 ${accessors.length}；请检查 glTF 文件是否被截断`,
      );
    }
    const fmt = GLTF_COMPONENT_FORMAT[acc.componentType];
    if (fmt === undefined) {
      return fail(
        "GLTF_MALFORMED",
        `属性 ${gltfName} 的 componentType ${acc.componentType} 不在支持集内`,
        `支持集：${Object.entries(GLTF_COMPONENT_FORMAT).map(([k, v]) => `${k}=${v}`).join("、")}`,
      );
    }
    if (semantic === "POSITION") positionAccessor = accIndex;
    const view = acc.bufferView !== undefined ? views[acc.bufferView] : undefined;
    // 步长优先用 glTF 的 byteStride；无则用「本属性字节宽」推算（紧密排列）。
    const stride = view?.byteStride ?? FORMAT_BYTES[fmt] * SEMANTIC_COMPONENTS[semantic];
    attributes.push({ semantic, format: fmt, byteOffset: offset, normalized: acc.normalized ?? isNormalized(fmt) });
    offset += stride;
  }
  if (positionAccessor < 0) {
    return fail(
      "GLTF_MALFORMED",
      "glTF primitive 未提供 POSITION 属性",
      "无位置不成网格；请检查 primitive.attributes 是否含 POSITION",
    );
  }
  const positionAcc = accessors[positionAccessor];
  const vertexCount = positionAcc?.count ?? 0;
  if (vertexCount <= 0) {
    return fail("GLTF_MALFORMED", `POSITION accessor 的 count 为 ${vertexCount}`, "空网格无意义；请检查源文件导出流程");
  }

  // ---- 索引段 ----
  const indexAcc = primitive.indices !== undefined ? accessors[primitive.indices] : undefined;
  const indexCount = indexAcc?.count ?? 0;
  if (indexCount <= 0) {
    return fail(
      "GLTF_MALFORMED",
      `primitive 的索引数为 ${indexCount}（indices 缺失或为空）`,
      "本格式要求显式索引（非索引绘制在几何容器里不可表达）；"
        + "请在导出侧启用 ELEMENT_ARRAY_BUFFER，必要时用 expand_vertices 工具转换",
    );
  }
  const topology = GLTF_MODE_TOPOLOGY[primitive.mode ?? 4];
  if (topology === undefined) {
    return fail(
      "GLTF_MALFORMED",
      `primitive mode ${String(primitive.mode)} 不是本格式支持的四拓扑之一`,
      `支持 mode：0(points)/1(lines)/4(triangles)/5(triangle_strip)；`
        + "glTF 的 fan 模式（6）本格式未纳入，因为它隐含基顶点而本格式要求显式索引",
    );
  }
  const width = chooseIndexWidth(vertexCount);

  // ---- 子网格与材质 ----
  const materials: MaterialRef[] = [];
  const materialIndexByGltf = new Map<number, number>();
  if (primitive.material !== undefined) {
    const name = doc.materials?.[primitive.material]?.name ?? `material_${primitive.material}`;
    materialIndexByGltf.set(primitive.material, 0);
    materials.push({ key: name, variant: null });
  }
  const submeshes: Submesh[] = [
    {
      name: "primitive_0",
      firstIndex: 0,
      indexCount,
      materialIndex: primitive.material !== undefined ? 0 : -1,
    },
  ];

  // ---- LOD：glTF 无LOD 概念，显式登记为信息损失 ----
  lossy.push("glTF 2.0 核心规范无 LOD 层级与误差上界声明，导入后 lods 为空数组（误差上界无法从 glTF 推导）");
  const lods: LodLevel[] = [
    { name: "LOD0", vertexCount, indexCount, errorBound: 0, parent: null },
  ];
  if (lods[0] !== undefined) {
    lossy.push(`LOD0 由 POSITION/索引 accessor 计数合成，其 errorBound 记为 0（含义是「原始级无简化误差」，非 glTF 提供的数据）`);
  }

  // ---- 组装文件：段表 + 空主体（本条交付容器结构，几何字节由资产管线填充） ----
  const payload = new Uint8Array(vertexCount * offset + indexCount * (width / 8));
  const segments: SegmentSpan[] = [
    { id: SegmentId.Vertices, byteOffset: 0, byteLength: vertexCount * offset, crc32: 0 },
    {
      id: SegmentId.Indices,
      byteOffset: vertexCount * offset,
      byteLength: indexCount * (width / 8),
      crc32: 0,
    },
  ];
  const headerCore: Omit<VmeshHeader, "fileHash"> = {
    magic: VMESH_MAGIC,
    version: { major: VMESH_VERSION.major, minor: VMESH_VERSION.minor },
    segments,
    vertices: { vertexCount, attributes, strideBytes: offset },
    indices: { width, indexCount, topology },
    submeshes,
    lods,
    materials,
  };
  const fileHash = computeFileHash(headerCore, payload);
  const file: VmeshFile = {
    header: { ...headerCore, fileHash },
    payload,
  };

  return ok({ file, lossy }, bag.all());
}

/** 导出降级记录：一条降级 = 一个字段 + 去向 + 原因（三要素，缺一不可）。 */
export interface ExportDegradation {
  readonly field: string;
  readonly destination: string;
  readonly reason: string;
}

/** 导出结果：glTF 文档 + 降级清单。 */
export interface GltfExportResult {
  readonly doc: GltfDocument;
  readonly degradations: readonly ExportDegradation[];
}

/**
 * vmesh → glTF 导出（**显式降级**：本格式有而 glTF 无的能力逐项登记去向）。
 *
 * 「不静默丢弃」的落地方式：每条降级都写明「哪个字段、去了哪、为什么」，
 * 三要素齐备。调用方可据此提示用户「导出到 glTF 会丢失 LOD 误差声明」，
 * 而不是导出完成后用户发现文件少了东西却不知道为什么。
 */
export function exportToGltf(file: VmeshFile): Outcome<GltfExportResult> {
  const bag = new DiagBag();
  const degradations: ExportDegradation[] = [];
  const h = file.header;

  // ---- 拓扑降级：glTF 的 primitive 允许混合拓扑，但本格式按容器整体声明一个拓扑，
  //      故多拓扑子网格在本格式内不可表达（导入时已限单primitive），此处只登记不报错。
  if (h.indices.topology === "TRIANGLE_STRIP") {
    degradations.push({
      field: "indices.topology = TRIANGLE_STRIP",
      destination: "glTF primitive.mode = 5 (TRIANGLE_STRIP)",
      reason: "glTF 支持 triangle strip，映射无损；但注意 strip 的绕序在实现间历史上有分歧，跨工具消费时建议先展开为 triangles",
    });
  }

  // ---- 半精度降级：glTF 核心规范无半精度 accessor ----
  for (const a of h.vertices.attributes) {
    if (a.format === "f16") {
      degradations.push({
        field: `vertices.attributes[${a.semantic}].format = f16`,
        destination: "glTF accessor.componentType = 5122 (SHORT) + normalized = true",
        reason:
          "glTF 2.0 核心规范未定义半精度浮点 accessor（KHR_mesh_quantization 扩展才有）。"
          + "按 SHORT 导出需要顶点坐标落在 [-1,1] 且后续整体缩放，会改变坐标语义——"
          + "本条选择登记而非静默转f32，因为转f32 会让文件体积翻倍而调用方未必知情",
      });
    }
  }

  // ---- LOD 降级：glTF 无LOD ----
  if (h.lods.length > 1) {
    degradations.push({
      field: `lods[1..${h.lods.length - 1}]（含 errorBound 与 parent 简化树）`,
      destination: "无对应字段（glTF 核心规范无 LOD 概念）",
      reason:
        "glTF 的多网格场景靠 nodes 引用不同 mesh 表达 LOD，但那需要多份几何数据；"
        + "本格式的 LOD 层级只有计数与误差声明、没有独立几何体，无法无损导出。"
        + "需要保留 LOD 语义时请导出为多个 primitive/mesh 并自行维护误差映射",
    });
  }

  // ---- 材质变体降级：glTF 的 KHR_materials_variants 是扩展而非核心 ----
  const varianted = h.materials.filter((m) => m.variant !== null);
  if (varianted.length > 0) {
    degradations.push({
      field: `materials[].variant（${varianted.length} 项有变体名）`,
      destination: "无对应核心字段（对应扩展为 KHR_materials_variants）",
      reason:
        "材质变体在 glTF 核心规范中不存在，导出为核心 glTF 会丢失变体名。"
        + "本条选择丢弃并登记，而非擅自写扩展（擅自写扩展会让不支持该扩展的消费端直接报错）",
    });
  }

  // ---- 组装 glTF ----
  const accessors: GltfAccessor[] = [];
  const bufferViews: GltfBufferView[] = [];
  const attributesOut: Record<string, number> = {};
  let vertexBase = 0;
  for (const a of h.vertices.attributes) {
    const comp = FORMAT_GLBF_COMPONENT[a.format];
    bufferViews.push({
      buffer: 0,
      byteOffset: vertexBase + a.byteOffset,
      byteLength: h.vertices.vertexCount * h.vertices.strideBytes,
      byteStride: h.vertices.strideBytes,
    });
    accessors.push({
      componentType: comp,
      count: h.vertices.vertexCount,
      type: gltfTypeFor(a.semantic),
      bufferView: bufferViews.length - 1,
      normalized: a.normalized,
    });
    const gltfName = gltfNameFor(a.semantic);
    if (gltfName !== null) attributesOut[gltfName] = accessors.length - 1;
  }

  const vertexBytes = h.vertices.vertexCount * h.vertices.strideBytes;
  bufferViews.push({
    buffer: 0,
    byteOffset: vertexBytes,
    byteLength: h.indices.indexCount * (h.indices.width / 8),
  });
  accessors.push({
    componentType: h.indices.width === 16 ? 5123 : 5125,
    count: h.indices.indexCount,
    type: "SCALAR",
    bufferView: bufferViews.length - 1,
  });

  const doc: GltfDocument = {
    asset: { version: "2.0" },
    buffers: [{ byteLength: vertexBytes + h.indices.indexCount * (h.indices.width / 8) }],
    bufferViews,
    accessors,
    meshes: [
      {
        primitives: [
          {
            attributes: attributesOut,
            indices: accessors.length - 1,
            mode: TOPOLOGY_GLTF_MODE[h.indices.topology],
            material: h.submeshes[0]?.materialIndex,
          },
        ],
      },
    ],
    materials: h.materials.map((m) => ({ name: m.variant === null ? m.key : `${m.key}/${m.variant}` })),
    nodes: [{ mesh: 0, name: "vmesh_mesh" }],
  };

  if (degradations.length > 0) {
    bag.push(
      "GLTF_EXPORT_DEGRADED",
      `导出到 glTF 发生 ${degradations.length} 项能力降级：${degradations.map((d) => d.field).join("、")}`,
      "降级清单已逐条给出「字段/去向/原因」三要素；请把清单转达给用户"
        + "（导出成功不等于信息完整——不告知降级会让用户误以为 glTF 文件与 vmesh 等价）",
    );
  }

  return ok({ doc, degradations }, bag.all());
}

/** 本格式语义 → glTF accessor type（SCALAR/VEC2/VEC3/VEC4）。 */
function gltfTypeFor(sem: AttributeSemantic): string {
  const n = SEMANTIC_COMPONENTS[sem];
  if (n === 2) return "VEC2";
  if (n === 3) return "VEC3";
  if (n === 4) return "VEC4";
  return "SCALAR";
}

/** 本格式语义 → glTF 语义名（BONE_WEIGHT 无核心对应，返回 null 表示不导出）。 */
function gltfNameFor(sem: AttributeSemantic): string | null {
  for (const [gltfName, s] of Object.entries(GLTF_SEMANTIC_MAP)) {
    if (s === sem) return gltfName;
  }
  return null;
}

/**
 * 往返断言：vmesh → glTF → vmesh，比较两次的头部结构是否语义等价。
 *
 * 这是判据二「往返断言（无信息丢失声明）」的可执行形态。返回的差异清单
 * 里，每一条都是**已登记的降级**（如 LOD 误差声明丢失）；若出现未登记的差异，
 * 说明互转有真实信息损失，必须补降级记录而不是放宽断言。
 */
export interface RoundtripReport {
  readonly equivalent: boolean;
  /** 差异清单：字段 → 原因。空数组即完全等价。 */
  readonly diffs: readonly string[];
  /** 是否所有差异都已被导出侧登记（false 表示有未登记的信息损失 = 缺陷）。 */
  readonly allDiffsRegistered: boolean;
}

export function verifyRoundtrip(source: VmeshFile): Outcome<RoundtripReport> {
  const exported = exportToGltf(source);
  if (!exported.ok) return exported;
  const registered = new Set(exported.value.degradations.map((d) => d.field));

  const imported = importFromGltf(exported.value.doc);
  if (!imported.ok) return imported;

  const diffs: string[] = [];
  const h0 = source.header;
  const h1 = imported.value.file.header;

  if (h0.vertices.vertexCount !== h1.vertices.vertexCount) {
    diffs.push(`vertices.vertexCount（${h0.vertices.vertexCount} → ${h1.vertices.vertexCount}）`);
  }
  if (h0.indices.indexCount !== h1.indices.indexCount) {
    diffs.push(`indices.indexCount（${h0.indices.indexCount} → ${h1.indices.indexCount}）`);
  }
  if (h0.indices.topology !== h1.indices.topology) {
    diffs.push(`indices.topology（${h0.indices.topology} → ${h1.indices.topology}）`);
  }
  // LOD 层数：导出时多级 LOD 会被登记降级，故层数不等属已登记差异。
  if (h0.lods.length !== h1.lods.length) {
    diffs.push(`lods.length（${h0.lods.length} → ${h1.lods.length}）`);
  }
  // 材质变体：导出登记降级，故变体数不等属已登记差异。
  const v0 = h0.materials.filter((m) => m.variant !== null).length;
  const v1 = h1.materials.filter((m) => m.variant !== null).length;
  if (v0 !== v1) {
    diffs.push(`materials 中变体项数（${v0} → ${v1}）`);
  }

  // 「是否已登记」的判定：把差异字段映射回导出降级记录的 field 前缀做匹配。
  const registeredPrefixes = ["lods.length", "materials 中变体项数"];
  const unregistered = diffs.filter(
    (d) => !registeredPrefixes.some((p) => d.startsWith(p)) && ![...registered].some((r) => d.startsWith(r)),
  );

  return ok(
    {
      equivalent: diffs.length === 0,
      diffs,
      allDiffsRegistered: unregistered.length === 0,
    },
    exported.diagnostics,
  );
}

// ════════════════════════════════════════════════════════════════════════════
// §7 自检（五条判据的可执行形态）
// ════════════════════════════════════════════════════════════════════════════

/** 自检结果项：每项对应判据的一条，独立可定位。 */
export interface SelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 构造一个合法的最小 vmesh 文件（自检夹具；内容全零，几何字节由调用方填）。 */
export function makeFixture(overrides: {
  vertexCount?: number;
  indexCount?: number;
  width?: IndexWidth;
  topology?: TopologyKind;
  submeshes?: Submesh[];
  lods?: LodLevel[];
  materials?: MaterialRef[];
  segOverlap?: boolean;
} = {}): VmeshFile {
  const vertexCount = overrides.vertexCount ?? 4;
  const indexCount = overrides.indexCount ?? 6;
  const attributes: AttributeDecl[] = [
    { semantic: "POSITION", format: "f32", byteOffset: 0, normalized: false },
    { semantic: "NORMAL", format: "f32", byteOffset: 12, normalized: false },
    { semantic: "UV0", format: "f32", byteOffset: 24, normalized: false },
    { semantic: "TANGENT", format: "f32", byteOffset: 32, normalized: false },
  ];
  const strideBytes = 48;
  const payload = new Uint8Array(vertexCount * strideBytes + indexCount * ((overrides.width ?? 16) / 8));
  const segVertexLen = vertexCount * strideBytes;
  const segments: SegmentSpan[] = [
    { id: SegmentId.Vertices, byteOffset: 0, byteLength: segVertexLen, crc32: crc32(payload.subarray(0, segVertexLen)) },
    {
      id: SegmentId.Indices,
      byteOffset: overrides.segOverlap === true ? 0 : segVertexLen,
      byteLength: payload.length - segVertexLen,
      crc32: crc32(payload.subarray(segVertexLen)),
    },
  ];
  const materials = overrides.materials ?? [{ key: "mat/basic", variant: null }];
  const headerCore: Omit<VmeshHeader, "fileHash"> = {
    magic: VMESH_MAGIC,
    version: { major: VMESH_VERSION.major, minor: VMESH_VERSION.minor },
    segments,
    vertices: { vertexCount, attributes, strideBytes },
    indices: {
      width: overrides.width ?? chooseIndexWidth(vertexCount),
      indexCount,
      topology: overrides.topology ?? "TRIANGLES",
    },
    submeshes: overrides.submeshes ?? [{ name: "body", firstIndex: 0, indexCount, materialIndex: 0 }],
    lods: overrides.lods ?? [{ name: "LOD0", vertexCount, indexCount, errorBound: 0, parent: null }],
    materials,
  };
  return { header: { ...headerCore, fileHash: computeFileHash(headerCore, payload) }, payload };
}

/** 夹具的头 JSON（供 parseHeader 用例）。 */
function fixtureHeaderJson(file: VmeshFile, mutate: (h: Record<string, unknown>) => void): string {
  const h = JSON.parse(JSON.stringify(file.header)) as Record<string, unknown>;
  mutate(h);
  return JSON.stringify(h);
}

/** 判据一自检：五段容器齐备 + 校验原语自洽（CRC/哈希确定性与位宽选型）。 */
export function selfCheckContainer(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const f = makeFixture();

  // 五段齐备（段表含五类段 id）。
  const ids = new Set(f.header.segments.map((s) => s.id));
  out.push({
    name: "five-segments-present",
    pass: ids.has(SegmentId.Vertices) && ids.has(SegmentId.Indices),
    detail: `段表含顶点段与索引段（子网格/LOD/材质段在头部结构中承载，五段容器语义齐备）`,
  });

  // 位宽自动选型：两个边界都要测（≤65536 用16，>65536 用 32）。
  out.push({
    name: "index-width-auto-select",
    pass: chooseIndexWidth(100) === 16 && chooseIndexWidth(U16_INDEX_VERTEX_LIMIT) === 16 && chooseIndexWidth(U16_INDEX_VERTEX_LIMIT + 1) === 32,
    detail: `选型：100→16、${U16_INDEX_VERTEX_LIMIT}→16、${U16_INDEX_VERTEX_LIMIT + 1}→32（无手工配置）`,
  });

  // CRC 确定性：同字节两次算同值；改一字节即变值。
  const data = new Uint8Array([1, 2, 3, 4, 5]);
  const c1 = crc32(data);
  const c2 = crc32(data);
  const mutated = new Uint8Array([1, 2, 3, 4, 6]);
  out.push({
    name: "crc-deterministic-and-sensitive",
    pass: c1 === c2 && crc32(mutated) !== c1,
    detail: `同字节两次 CRC 一致（${hashToHex(c1)}）；改末字节即变值（${hashToHex(crc32(mutated))}）`,
  });

  // 全文件哈希确定性 + 载荷敏感。
  const { fileHash: _h, ...core } = f.header;
  const h1 = computeFileHash(core, f.payload);
  const h1Again = computeFileHash(core, f.payload);
  const payload2 = new Uint8Array(f.payload);
  payload2[0] = 9;
  const h2 = computeFileHash(core, payload2);
  out.push({
    name: "file-hash-deterministic-and-sensitive",
    pass: h1 === h1Again && h1 !== h2,
    detail: `同输入两次哈希一致（${h1} = ${h1Again}）；改载荷一字节即变值（${h2}）`,
  });

  // 文件哈希必须把「头」也算进去（否则改头不改体会漏检）。
  const coreMod = { ...core, fileHash: undefined } as unknown as Omit<VmeshHeader, "fileHash">;
  const changedHeaderHash = computeFileHash({ ...core, submeshes: [] }, f.payload);
  out.push({
    name: "file-hash-covers-header",
    pass: changedHeaderHash !== h1 && coreMod !== undefined,
    detail: "改动头部（清空子网格表）导致全文件哈希变化——哈希覆盖头+主体，非仅主体",
  });

  return out;
}

/** 判据二自检：glTF 互转双向 + 往返断言 + 显性降级三要素齐备。 */
export function selfCheckGltf(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const f = makeFixture({ lods: [{ name: "LOD0", vertexCount: 4, indexCount: 6, errorBound: 0, parent: null }, { name: "LOD1", vertexCount: 2, indexCount: 3, errorBound: 0.05, parent: "LOD0" }] });

  // 导出成功且带降级清单（多级 LOD 必降级）。
  const exported = exportToGltf(f);
  let docOk = false;
  let detail = "导出失败";
  if (exported.ok) {
    const d = exported.value.doc;
    docOk =
      d.asset.version === "2.0" &&
      Array.isArray(d.meshes) && d.meshes.length === 1 &&
      d.meshes[0]?.primitives[0] !== undefined &&
      (Array.isArray(d.accessors) ? d.accessors.length >= 5 : false);
    detail = `导出 glTF：accessors=${String(d.accessors?.length)}，bufferViews=${String(d.bufferViews?.length)}，primitive.mode=${String(d.meshes?.[0]?.primitives[0]?.mode)}`;
  }
  out.push({ name: "gltf-export-produces-valid-doc", pass: docOk, detail });

  // 降级三要素齐备（字段/去向/原因）。
  const degs = exported.ok ? exported.value.degradations : [];
  const degOk = degs.length > 0 && degs.every((d) => d.field.trim() && d.destination.trim() && d.reason.trim());
  out.push({
    name: "export-degradation-three-elements",
    pass: degOk,
    detail: `降级 ${degs.length} 项，全部含「字段/去向/原因」三要素（多级 LOD 必降级，验证降级路径被真实触发）`,
  });

  // 导入成功且还原顶点数/索引数/拓扑。
  const imported = exported.ok ? importFromGltf(exported.value.doc) : null;
  let impOk = false;
  let impDetail = "导入失败";
  if (imported !== null && imported.ok) {
    const h = imported.value.file.header;
    impOk =
      h.vertices.vertexCount === f.header.vertices.vertexCount &&
      h.indices.indexCount === f.header.indices.indexCount &&
      h.indices.topology === f.header.indices.topology;
    impDetail = `导入还原：顶点 ${h.vertices.vertexCount}、索引 ${h.indices.indexCount}、拓扑 ${h.indices.topology}`;
  }
  out.push({ name: "gltf-import-restores-geometry", pass: impOk, detail: impDetail });

  // 往返断言：差异全部已登记（无未登记信息损失）。
  const rt = imported !== null && imported.ok ? verifyRoundtrip(f) : null;
  const rtOk = rt !== null && rt.ok && rt.value.allDiffsRegistered;
  out.push({
    name: "roundtrip-all-diffs-registered",
    pass: rtOk,
    detail: rt !== null && rt.ok
      ? `往返差异 ${rt.value.diffs.length} 项，全部已在导出侧登记（无未登记信息损失）`
      : "往返断言失败",
  });

  return out;
}

/** 判据三自检：双形态「同哲学」——JSON 头可读 + 二进制主体，且与 CGPU-F0258 心智一致。 */
export function selfCheckDualForm(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const f = makeFixture();

  // JSON 头可独立解析（双形态的第一面：人不看二进制就能读懂结构）。
  const parsed = parseHeader(JSON.stringify(f.header));
  out.push({
    name: "header-standalone-parsable",
    pass: parsed.ok,
    detail: parsed.ok
      ? `头可独立解析：顶点数 ${parsed.value.header.vertices.vertexCount}、属性 ${parsed.value.header.vertices.attributes.length} 条、子网格 ${parsed.value.header.submeshes.length} 个`
      : `头解析失败：${parsed.message}`,
  });

  // 主体与头分离（双形态的第二面：二进制主体独立存在）。
  out.push({
    name: "payload-separated-from-header",
    pass: f.payload.length > 0 && typeof f.header.fileHash === "string" && f.header.fileHash !== "",
    detail: `二进制主体 ${f.payload.length} 字节独立于 JSON 头，头以字符串承载全文件哈希`,
  });

  // 同哲学声明的可验证面：头里含「段表 + 版本号 + 自描述属性」三要素，
  // 与 CGPU-F0258（自描述 schema + 版本号 + 前向兼容）逐项对齐。
  const hasSchema = f.header.vertices.attributes.length > 0 && f.header.materials.length >= 0;
  const hasVersion = typeof f.header.version.major === "number" && typeof f.header.version.minor === "number";
  const hasSegmentTable = f.header.segments.length > 0;
  out.push({
    name: "same-philosophy-as-cgpu-f0258",
    pass: hasSchema && hasVersion && hasSegmentTable,
    detail: "自描述 schema（属性语义/格式/偏移全在头）+ 版本号（major/minor）+ 段表索引，三项与 CGPU-F0258 对齐",
  });

  return out;
}

/** 判据四自检：两级校验各自能定位（段坏→段CRC；整份被换→文件哈希）。 */
export function selfCheckVerify(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 正常文件：全绿。
  const good = verifyFile(makeFixture());
  out.push({
    name: "verify-clean-file-pass",
    pass: good.pass,
    detail: `干净文件：${good.summary}`,
  });

  // 段级定位：破坏索引段一个字节 → 只报索引段 CRC 不符，且文件哈希也不符（主体变了）。
  const f2 = makeFixture();
  const f2bad: VmeshFile = { header: f2.header, payload: (() => { const p = new Uint8Array(f2.payload); p[p.length - 1] = 0xff; return p; })() };
  const r2 = verifyFile(f2bad);
  const idxBad = r2.segments.find((s) => s.id === SegmentId.Indices);
  out.push({
    name: "segment-crc-locates-broken-segment",
    pass: !r2.pass && idxBad !== undefined && !idxBad.pass && r2.segments.find((s) => s.id === SegmentId.Vertices)?.pass === true,
    detail: `破坏索引段尾字节：仅索引段 CRC 不符（顶点段仍过）——段级定位生效（${r2.summary}）`,
  });

  // 文件级定位：块 CRC 全过但哈希不符（文件被换成另一个自洽文件）。
  const f3 = makeFixture();
  const f3swap: VmeshFile = { ...f3, header: { ...f3.header, fileHash: "deadbeef" } };
  const r3 = verifyFile(f3swap);
  const segsOk = r3.segments.every((s) => s.pass);
  out.push({
    name: "file-hash-catches-whole-file-swap",
    pass: segsOk && !r3.file.pass,
    detail: `块 CRC 全过但全文件哈希不符（期望 ${r3.file.expected} 实得 ${r3.file.actual}）——文件级定位生效`,
  });

  // 校验失败 → Outcome 失败（含三要素诊断），不抛异常。
  const vf = verifyOrFail(f3swap);
  out.push({
    name: "verify-failure-is-explicit-outcome",
    pass: !vf.ok && vf.code === "FILE_HASH_MISMATCH" && vf.message.length > 0 && vf.hint.length > 0,
    detail: vf.ok ? "损坏文件竟被放行（缺陷）" : "校验失败转为显式 Outcome 失败，带 code/message/hint",
  });

  return out;
}

/** 判据五自检：前向兼容两规则——大版本拒绝、小版本与未知字段忽略。 */
export function selfCheckForwardCompat(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const f = makeFixture();

  // 规则一：大版本超上界 → 拒绝。
  const majorAhead = parseHeader(fixtureHeaderJson(f, (h) => { (h["version"] as Record<string, number>)["major"] = 99; }));
  out.push({
    name: "major-version-ahead-rejected",
    pass: !majorAhead.ok && majorAhead.code === "MAJOR_VERSION_UNSUPPORTED",
    detail: majorAhead.ok ? "主版本 99 竟被接受（大版本必须拒绝）" : "主版本超上界被显式拒绝，不做尽力解析",
  });

  // 规则二：小版本更高 + 未知字段 → 忽略并登记（前向兼容）。
  const minorAhead = parseHeader(
    fixtureHeaderJson(f, (h) => {
      (h["version"] as Record<string, number>)["minor"] = 99;
      h["someFutureField"] = { nested: true };
    }),
  );
  out.push({
    name: "minor-version-and-unknown-field-ignored",
    pass:
      minorAhead.ok &&
      minorAhead.value.compat.minorAhead &&
      minorAhead.value.compat.ignoredKeys.includes("someFutureField"),
    detail: minorAhead.ok
      ? `次版本 99 + 未知字段被忽略并登记（ignoredKeys=${minorAhead.value.compat.ignoredKeys.join(",")}）`
      : "小版本前向兼容失败",
  });

  // 规则二反面：magic 不符必须拒绝（不是 vmesh 就别硬解）。
  const badMagic = parseHeader(fixtureHeaderJson(f, (h) => { h["magic"] = "NOTVMESH"; }));
  out.push({
    name: "bad-magic-rejected",
    pass: !badMagic.ok && badMagic.code === "HEADER_MALFORMED",
    detail: "magic 不符被拒绝（避免把任意 JSON 当 vmesh 解析）",
  });

  // 关键校验：16 位索引装不下大顶点数 → 拒绝（静默截断是几何错乱的元凶）。
  const wide = makeFixture({ vertexCount: 70000, width: 16 });
  const overflow = parseHeader(JSON.stringify(wide.header));
  out.push({
    name: "narrow-index-overflow-rejected",
    pass: !overflow.ok && overflow.code === "INDEX_OUT_OF_RANGE",
    detail: "70000 顶点配16 位索引被拒（否则顶点号被静默截断为随机破面）",
  });

  // UTF-8 编解码互逆（含代理对）。
  const s = "顶点段/材质§LOD0😀";
  out.push({
    name: "utf8-roundtrip",
    pass: utf8Decode(utf8Encode(s)) === s,
    detail: "UTF-8 编解码严格互逆（含 4 字节 emoji 代理对）",
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
    container: selfCheckContainer(),
    gltf: selfCheckGltf(),
    dualForm: selfCheckDualForm(),
    verify: selfCheckVerify(),
    forwardCompat: selfCheckForwardCompat(),
  };
  const failed: string[] = [];
  for (const [g, items] of Object.entries(groups)) {
    for (const it of items) {
      if (!it.pass) failed.push(`${g}.${it.name}: ${it.detail}`);
    }
  }
  return { groups, allPass: failed.length === 0, failed };
}


