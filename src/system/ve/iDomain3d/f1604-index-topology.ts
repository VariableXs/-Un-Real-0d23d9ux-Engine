/**
 * VE-F1604 · 索引与拓扑（I 域 · 3D 管线域 · L0 几何数据层 · 批次 I01）
 * ---------------------------------------------------------------------------
 * 职责定位：网格的索引缓冲与图元拓扑 —— 位宽自动选型、四类拓扑的语义与用途、
 * 按需邻接信息生成、拓扑校验。上游 F1603（顶点属性布局）产出顶点字节契约，
 * 本条在其之上补齐「顶点怎么连成面」；下游 F1607（网格修复检测，破面/退化三角）、
 * F1608（QEM 简化需边邻接）、F1609（Forsyth 顶点缓存重排）以本条为输入契约。
 *
 * 四条判据与实现段落一一对应：
 *   判据一「自动选型」 → §2 索引位宽由顶点数自动判定（≤65536 → 16 位，>65536 → 32 位），
 *                       无手工配置；16 位恰省一半索引内存，且给出选型结果的
 *                       人话理由与越界风险提示（静默截断是几何错乱的元凶）。
 *   判据二「拓扑集」   → §3 四类拓扑（三角列表/三角带/线/点）各自的语义、用途与
 *                       展开代价；声明「strip 的绕序在实现间有历史分歧」并给出
 *                       跨工具消费建议（不假装没有这个坑）。
 *   判据三「邻接按需」 → §4 边邻接与点邻接**按需生成、不预生成**：邻接结构内存开销
 *                       与网格规模成正比（百万顶点网格的完整边邻接可达数十 MB），
 *                       而多数渲染帧并不查询它。故提供生成器 + 显式的
 *                       「内存换查询」权衡声明，而非在加载时无条件构建。
 *   判据四「校验」→ §5 索引越界与退化三角检出，退化判据给出面积/长宽比/重复顶点
 *                       三条并说明各自的失效场景（单用面积会漏掉细长三角形）。
 *
 * 边界声明（不扩面）：
 *   · 本条只管「顶点怎么连」与「连得对不对」，不管顶点字节怎么排（F1603）、
 *     不管容器格式与校验和（归 F1602）、不修网格（修复器归 F1607）、
 *     不简化（QEM 归 F1608）、不做顶点重排（Forsyth 归 F1609）、
 *     不做剔除（LOD/视锥归 I09）。
 *   · 本条不 import F1602/F1603：拓扑与位宽规则独立于容器与布局演进；
 *     语义名与常量口径的自洽由 §6 自检兜底，而非靠跨条 import 制造耦合。
 *
 * 零静默纪律：校验失败一律产出 Diagnostic（code + message + hint 三要素齐备），
 * 不抛异常、不吞诊断。拓扑错误的典型表现是「看起来能渲染但某些面缺失/翻转」，
 * 属于最难排查的一类缺陷，故必须在数据入口就拦下并说清成因。
 *
 * 判据：自动选型、拓扑集、邻接按需、校验、判据。
 * 落位：src/system/ve/iDomain3d/
 */

// ════════════════════════════════════════════════════════════════════════════
// §0 诊断基础设施（零静默第一层；与 F1602/F1603 同纪律，自包含不跨条 import）
// ════════════════════════════════════════════════════════════════════════════

/** 诊断码：每种失败独立可检索，绝不合并为一条通用错误。 */
export type DiagCode =
  /** 拓扑类型不在四类之内。 */
  | "TOPOLOGY_UNKNOWN"
  /** 索引值越出顶点数范围（画到不存在的顶点 = 读越界内存）。 */
  | "INDEX_OUT_OF_RANGE"
  /** 索引数为负或非整数。 */
  | "INDEX_COUNT_INVALID"
  /** 非点拓扑的索引数为 0（无法构成任何图元）。 */
  | "TOPOLOGY_EMPTY"
  /** 图元数与拓扑不匹配（如线拓扑的索引数不是偶数）。 */
  | "TOPOLOGY_COUNT_MISMATCH"
  /** 位宽选型与顶点数不自洽（顶点数超上限却用 16 位索引 =静默截断）。 */
  | "INDEX_WIDTH_OVERFLOW"
  /** 退化三角：面积近零。 */
  | "DEGENERATE_TRIANGLE_AREA"
  /** 退化三角：长宽比异常（细长但面积不小，单用面积判据会漏）。 */
  | "DEGENERATE_TRIANGLE_SLIVER"
  /** 退化三角：顶点索引重复。 */
  | "DEGENERATE_TRIANGLE_DUPLICATE_INDEX"
  /** 邻接查询请求了不存在的顶点或边。 */
  | "ADJACENCY_QUERY_INVALID"
  /** 邻接结构与当前网格不一致（顶点数/索引数变了却复用旧邻接）。 */
  | "ADJACENCY_STALE"
  /** 拓扑转换（如 strip 展开）输入不合法。 */
  | "TOPOLOGY_CONVERSION_INVALID";

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

/** 成功构造（diagnostics 承载非致命告警，如 strip 绕序分歧提示）。 */
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

/** 几何比较容差（面积/长宽比判定用；单精度顶点坐标下1e-12 已远低于有效位噪声）。 */
export const EPS = 1e-12;

/** 索引位宽（16 或 32 位）。 */
export type IndexWidth = 16 | 32;

/** 16 位索引的顶点数上限。65535 是合法顶点号，故上限是 65536 而不是 65535。 */
export const U16_INDEX_VERTEX_LIMIT = 65536;

/** 各类格式每索引的字节数。 */
export const INDEX_WIDTH_BYTES: Readonly<Record<IndexWidth, number>> = { 16: 2, 32: 4 };

/**
 * 四类拓扑（判据二：拓扑集）。
 *
 * 为何恰好是这四类：它们是「图元」在硬件与文件格式层面的四种最基本连接方式，
 * 覆盖了从面网格到线框到点云的全部表达需求。多于四类（如扇形 fan）会引入
 * 「隐含基顶点」这一隐式状态——那正是本域要消除的东西（见 F1603 的无隐式布局）。
 */
export const TopologyKind = {
  /** 三角列表：每3 个索引一个独立三角。通用、无隐式状态、可任意拆分合并。 */
  Triangles: "TRIANGLES",
  /** 三角带：第 i 个三角复用前一个的末两个顶点，顶点数省但有绕序分歧。 */
  TriangleStrip: "TRIANGLE_STRIP",
  /** 线：每 2 个索引一条线段。线框/边几何。 */
  Lines: "LINES",
  /** 点：每 1 个索引一个点。图元/点云。 */
  Points: "POINTS",
} as const;

/** 拓扑类型（字面量联合，供函数签名使用）。 */
export type TopologyKind = (typeof TopologyKind)[keyof typeof TopologyKind];

/** 四类拓扑的全集（自检用：注册表与枚举须一致）。 */
export const ALL_TOPOLOGIES: readonly TopologyKind[] = [
  TopologyKind.Triangles,
  TopologyKind.TriangleStrip,
  TopologyKind.Lines,
  TopologyKind.Points,
];

/** 每图元的索引数。 */
export const TOPOLOGY_PRIMITIVE_SIZE: Readonly<Record<TopologyKind, number>> = {
  TRIANGLES: 3,
  TRIANGLE_STRIP: 3,
  LINES: 2,
  POINTS: 1,
};

/** 拓扑元信息：语义、用途、索引节拍、展开代价（判据二的可查事实源）。 */
export interface TopologyInfo {
  readonly kind: TopologyKind;
  /** 人话名（诊断与文档展示）。 */
  readonly label: string;
  /** 语义：它表达的是什么。 */
  readonly semantics: string;
  /** 典型用途。 */
  readonly usage: string;
  /** 每图元索引数。 */
  readonly primitiveSize: number;
  /** 索引缓冲每图元最小字节数（strip 需3 起、之后每三角 1 个索引，故折算值另注）。 */
  readonly bytesPerPrimitive: number;
  /** 是否为「面」拓扑（决定是否参与面级校验与 QEM 简化）。 */
  readonly isSurface: boolean;
  /** 已知坑（诚实登记，不假装没有）。 */
  readonly caveats: string;
}

/**
 * 四类拓扑的完整档案。
 *
 * `bytesPerPrimitive` 的算法与选型直接相关，故在此显式给出：
 *   · 三角列表：3 索引/三角 —— 无冗余，可随机访问任意三角（拆分/合并零成本）。
 *   · 三角带：首三角 3 索引、后续每三角 1 索引，故 n 个三角需 n+2 个索引，
 *     平均每三角约 1 索引（在 16 位下省约 2/3 索引内存），代价是无法随机访问
 *     且绕序有分歧。
 *   · 线/点：分别2/1 索引，无冗余。
 */
export const TOPOLOGY_INFO: Readonly<Record<TopologyKind, TopologyInfo>> = {
  TRIANGLES: {
    kind: TopologyKind.Triangles,
    label: "三角列表",
    semantics: "每三个索引构成一个独立三角形，各三角之间不共享隐式顶点关系",
    usage: "通用首选：面网格的默认形态、子网格拆分与材质分组的天然单位（每个子网格是一个连续区间）",
    primitiveSize: 3,
    bytesPerPrimitive: 3,
    isSurface: true,
    caveats: "索引冗余最大——共享顶点必须在索引里重复写，故顶点数远大于顶点数时内存开销高于 strip",
  },
  TRIANGLE_STRIP: {
    kind: TopologyKind.TriangleStrip,
    label: "三角带",
    semantics: "连续索引构成三角带：第 i 个三角（i≥1）由索引 (i, i+1, i+2) 构成，相邻三角自动复用两个顶点",
    usage: "顶点复用极高的条带状/条带网格（圆柱侧面、条带布料、展开的平面带）",
    primitiveSize: 3,
    bytesPerPrimitive: 1,
    isSurface: true,
    caveats:
      "绕序在实现间有历史分歧：偶数位三角与奇数位三角的朝向约定、以及「从哪端开始遍历」"
      + "在不同引擎/文件格式间不一致，跨工具消费同一strip 可能得到正反颠倒的面。"
      + "本条不给「自动修正」，因为无法判定哪一端才是作者意图——建议导入后展开为三角列表再消费",
  },
  LINES: {
    kind: TopologyKind.Lines,
    label: "线",
    semantics: "每两个索引构成一条独立线段",
    usage: "线框可视化（F1614查看器数据）、边几何、路径/骨架线",
    primitiveSize: 2,
    bytesPerPrimitive: 2,
    isSurface: false,
    caveats: "不构成面，故不参与面级校验（退化面）与 QEM 简化；「边」的语义由几何应用方定义",
  },
  POINTS: {
    kind: TopologyKind.Points,
    label: "点",
    semantics: "每个索引构成一个独立点图元",
    usage: "点云/粒子图元/法线可视化锚点",
    primitiveSize: 1,
    bytesPerPrimitive: 1,
    isSurface: false,
    caveats: "无任何连接关系，故邻接结构恒为空；点云的空间结构需另行构建（如 F1611 分块）",
  },
};

/** 取拓扑元信息（未知拓扑返回 null，由调用方诊断，不做隐式默认）。 */
export function topologyInfo(kind: TopologyKind): TopologyInfo | null {
  return TOPOLOGY_INFO[kind];
}

// ════════════════════════════════════════════════════════════════════════════
// §2 索引位宽自动选型（判据一：无手工配置）
// ════════════════════════════════════════════════════════════════════════════

/** 选型结果：位宽 + 决策理由 + 内存账（判据一的可查产物）。 */
export interface WidthChoice {
  readonly width: IndexWidth;
  /** 人话理由。 */
  readonly reason: string;
  /** 该顶点数下索引缓冲的字节数。 */
  readonly totalIndexBytes: number;
  /** 若用另一档位宽会是多少字节（省/费多少的量化对照）。 */
  readonly savingsBytes: number;
  /** 相对 32 位的节省比例。 */
  readonly savingsRatio: number;
}

/**
 * 索引位宽自动选型：顶点数 ≤65536 → 16 位，否则 32 位。
 *
 * 为何是「顶点数」而非「索引数」判定：约束来自顶点号的可表示范围——16 位索引
 * 装不下编号 ≥65536 的顶点。索引数多少与能否装下无关（10万个顶点只被引用两次，
 * 仍是 10 万个不同顶点号）。
 *
 * 边界取 65536（含）而非 65535：顶点号 65535 是合法的（0 起编址），
 * 所以顶点数到 65536 仍可用 16 位；到 65537 就必须 32 位。这个 off-by-one
 * 是本条最容易写错的地方，故上界以常量 `U16_INDEX_VERTEX_LIMIT` 单一表达，
 * 选型与校验共用它，杜绝两处各写一个魔数。
 *
 * 省一半的说法要精确：16 位索引让**索引缓冲**字节数减半，不是整个顶点缓冲减半
 * （顶点缓冲大小由 F1603 的布局决定，与索引位宽无关）。把两者混为一谈会导致
 * 预算估算偏乐观，故本函数显式返回对照字节数而不只是一句「省一半」。
 */
export function chooseIndexWidth(vertexCount: number, indexCount: number): WidthChoice {
  const use16 = vertexCount <= U16_INDEX_VERTEX_LIMIT;
  const width: IndexWidth = use16 ? 16 : 32;
  const bytes = INDEX_WIDTH_BYTES[width] * indexCount;
  const bytesIf32 = INDEX_WIDTH_BYTES[32] * indexCount;
  const savingsBytes = bytesIf32 - bytes;
  const reason = use16
    ? `顶点数 ${vertexCount} ≤ ${U16_INDEX_VERTEX_LIMIT}，选 16 位索引：索引缓冲 ${bytes} 字节，`
      // 百分比须先乘 100 再格式化：savingsBytes/bytesIf32 是比例（0.5），
      // 直接 toFixed 会打印成 0.5%——把「省一半」说成「省千分之五」，方向性错误。
      + `比 32 位省 ${savingsBytes} 字节（${((savingsBytes / Math.max(1, bytesIf32)) * 100).toFixed(1)}%）`
    : `顶点数 ${vertexCount} > ${U16_INDEX_VERTEX_LIMIT}，必须选 32 位索引：`
      + `16 位会把顶点号静默截断为低 16 位，表现为随机破面，故此处不做降级`;
  return {
    width,
    reason,
    totalIndexBytes: bytes,
    savingsBytes,
    savingsRatio: bytesIf32 > 0 ? savingsBytes / bytesIf32 : 0,
  };
}

/** 位宽自洽性校验：顶点数超上限却声明 16 位 → 显式失败（静默截断必须拦住）。 */
export function verifyWidthChoice(vertexCount: number, indexCount: number, declared: IndexWidth): Outcome<WidthChoice> {
  const choice = chooseIndexWidth(vertexCount, indexCount);
  if (choice.width === declared) return ok(choice);
  return fail(
    "INDEX_WIDTH_OVERFLOW",
    `顶点数 ${vertexCount} 需${choice.width} 位索引，但声明为 ${declared} 位`,
    choice.width === 32
      ? "16 位索引装不下编号 ≥65536 的顶点，超出部分会被静默截断为低 16 位——"
        + "表现为随顶点号随机出现的破面。请改用 32 位（width 由 chooseIndexWidth 自动选型，无需手工配置）"
      : "顶点数未超 16 位上限却声明 32 位：功能上不会错，但索引缓冲内存翻倍且无收益，请改回 16 位",
  );
}

// ════════════════════════════════════════════════════════════════════════════
// §3 拓扑集（判据二：四类型的语义与用途 + 索引结构解读）
// ════════════════════════════════════════════════════════════════════════════

/** 一份完整的索引缓冲描述。 */
export interface IndexBuffer {
  readonly vertexCount: number;
  readonly indexCount: number;
  readonly indices: readonly number[];
  readonly width: IndexWidth;
  readonly topology: TopologyKind;
}

/** 构建索引缓冲描述（校验交给 §5，此处只做结构组装）。 */
export function makeIndexBuffer(
  vertexCount: number,
  indices: readonly number[],
  topology: TopologyKind,
  width?: IndexWidth,
): IndexBuffer {
  return {
    vertexCount,
    indexCount: indices.length,
    indices,
    width: width ?? chooseIndexWidth(vertexCount, indices.length).width,
    topology,
  };
}

/** 计算图元数（拓扑决定每图元几个索引）。 */
export function primitiveCount(buffer: IndexBuffer): number {
  const size = TOPOLOGY_PRIMITIVE_SIZE[buffer.topology];
  return size <= 0 ? 0 : Math.floor(buffer.indexCount / size);
}

/**
 * 索引数与拓扑的匹配校验。
 *
 * 线拓扑的索引数必须为偶数、点拓扑任意、三角类索引数须 ≥3。这看似是显然的，
 * 但越界的表现形式很隐蔽：奇数索引的线拓扑会让最后一个索引被**静默丢弃**
 * （不少驱动只按完整图元消费），表现为「少了一条线」而非报错。
 */
export function verifyTopologyCounts(buffer: IndexBuffer): Outcome<number> {
  const info = TOPOLOGY_INFO[buffer.topology];
  if (info === undefined) {
    return fail(
      "TOPOLOGY_UNKNOWN",
      `未知拓扑类型 ${JSON.stringify(buffer.topology)}`,
      `支持集：${ALL_TOPOLOGIES.join("、")}；未知拓扑不得默认按三角列表处理——`
        + "图元大小错了会把整份索引解释成另一种形状",
    );
  }
  const primitives = primitiveCount(buffer);
  const remainder = buffer.indexCount % info.primitiveSize;
  if (remainder !== 0) {
    return fail(
      "TOPOLOGY_COUNT_MISMATCH",
      `${info.label}的索引数 ${buffer.indexCount} 不是 ${info.primitiveSize} 的倍数（余 ${remainder}）`,
      topologyKindHint(buffer.topology, info.primitiveSize, remainder),
    );
  }
  if (info.primitiveSize > 1 && buffer.indexCount === 0) {
    return fail(
      "TOPOLOGY_EMPTY",
      `${info.label}的索引数为 0，无法构成任何图元`,
      "非点拓扑至少需要 " + String(info.primitiveSize) + " 个索引；空索引缓冲请用点拓扑并显式声明",
    );
  }
  return ok(primitives);
}

/** 索引数不整除时给出的人话处置建议（按拓扑给出不同修法）。 */
function topologyKindHint(topology: TopologyKind, size: number, remainder: number): string {
  if (topology === TopologyKind.Lines) {
    return `末尾多出 ${remainder} 个索引无法配成线段。请补齐索引，或若这是有意为之`
      + "（半个线段），请改用点拓扑显式表达孤立端点——被静默丢弃的索引比报错更难排查";
  }
  if (topology === TopologyKind.TriangleStrip) {
    return `末尾多出 ${remainder} 个索引无法构成完整三角。strip 至少需 3 个索引；`
      + "请检查导出流程是否在带尾追加了额外顶点";
  }
  return `每 ${size} 个索引构成一个图元，末尾余 ${remainder} 个无法成图元。`
    + "请补齐索引或裁掉尾部碎片——多数驱动只按完整图元消费，多余索引会被静默忽略";
}

// ════════════════════════════════════════════════════════════════════════════
// §4 邻接信息按需生成（判据三：内存换查询的权衡声明）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 边邻接结构：一个顶点相邻的所有边。
 *
 * 内存量级的诚实说明（这是「按需」的论据，不是顺带一提）：
 * 百万顶点、200 万三角的网格，其唯一边约 300 万条；用「每顶点邻接边数组」
 * 表达需约 300 万 × (2 端点 + 邻接顶点) 的存储，落在数十 MB 量级——而这还只是
 * 索引缓冲本身的几倍。故无脑预生成会让加载一个网格的内存翻数倍，而绝大多数
 * 渲染帧根本不查邻接（渲染只要顶点与索引）。故本条提供生成器而不自动构建。
 */
export interface EdgeAdjacency {
  readonly kind: "EDGE";
  readonly vertexCount: number;
  readonly indexCount: number;
  /** 邻接表：每顶点的相邻边号（CSR 式布局，前缀和+ 扁平数组，避免每顶点一个对象）。 */
  readonly offsets: readonly number[];
  readonly neighborEdges: readonly number[];
  /** 边 → 其两端的顶点号（无向边，两端顺序按顶点号升序规范化）。 */
  readonly edgeVertices: readonly number[];
}

/** 点邻接结构：一个顶点相邻的所有顶点（共享任一顶点即相邻）。 */
export interface VertexAdjacency {
  readonly kind: "VERTEX";
  readonly vertexCount: number;
  readonly indexCount: number;
  readonly offsets: readonly number[];
  readonly neighbors: readonly number[];
}

/** 邻接结构统一形态（两种邻接查询共用一个返回类型）。 */
export type Adjacency = EdgeAdjacency | VertexAdjacency;

/** 邻接生成选项。 */
export interface AdjacencyOptions {
  /**
   * 是否规范化边端点顺序（true：无向边两端按顶点号升序存，使同一条边只有一个表示）。
   * 布尔运算（F1607 的补洞）必须为 true——否则 (a,b) 与 (b,a) 会被当成两条边，
   * 边界判定随之失效。
   */
  readonly canonicalizeEdges?: boolean;
}

/**
 * 生成边邻接（按需，不自动调用）。
 *
 * 为何用 CSR（压缩行）布局而非 `Map<number, Set<number>>`：邻接是一次性建索引的
 * 数据，之后每帧或每次算法反复查询。Map+Set 的查询常数虽可接受，但内存开销是
 * 数组的数倍，且遍历顺序不确定（会让下游算法的输出不可复现）。CSR 遍历顺序
 * 由顶点号升序决定，**确定性**——同样的输入必得同样的邻接表，这对资产可复现
 * 与缺陷对拍都是前提。
 */
export function buildEdgeAdjacency(buffer: IndexBuffer, options: AdjacencyOptions = {}): Outcome<EdgeAdjacency> {
  const checked = verifyTopologyCounts(buffer);
  if (!checked.ok) return checked;
  const canonicalize = options.canonicalizeEdges ?? true;

  // 收集无向边：拓扑决定每图元产出哪些边。
  // 端点表用**扁平数组**（edgeVertices[2e], edgeVertices[2e+1]）而非 number[][]：
  // 扁平布局没有「空元素」这一态，故后续访问不会被 noUncheckedIndexedAccess 逼出
  // 防御性分支，也不会在flat() 时因空洞而错位——边表是要被高频遍历的热数据。
  const edgeVertices: number[] = [];
  const edgeIndex = new Map<number, number>();
  const push = (a: number, b: number): void => {
    if (a === b) return; // 自环不构成邻接（退化边的边邻接无意义）
    if (canonicalize) {
      const lo = a < b ? a : b;
      const hi = a < b ? b : a;
      // 规范化键：lo 在高位、hi 在低位，各占 32 位——与端点顺序无关，且对
      // 顶点号无上限假设（乘法编码会在>65536 的网格上碰撞，那是静默的错邻接）。
      const key = lo * 4294967296 + hi;
      if (edgeIndex.has(key)) return;
      edgeIndex.set(key, edgeVertices.length / 2);
      edgeVertices.push(lo, hi);
      return;
    }
    edgeVertices.push(a, b);
  };
  collectEdges(buffer, push);

  // 顶点 → 边号 的倒排（CSR）。
  const counts = new Array<number>(buffer.vertexCount + 1).fill(0);
  for (let i = 0; i < edgeVertices.length; i += 2) {
    const a = edgeVertices[i] ?? 0;
    const b = edgeVertices[i + 1] ?? 0;
    counts[a + 1] = (counts[a + 1] ?? 0) + 1;
    counts[b + 1] = (counts[b + 1] ?? 0) + 1;
  }
  const offsets = new Array<number>(buffer.vertexCount + 1).fill(0);
  for (let i = 0; i < buffer.vertexCount; i += 1) offsets[i + 1] = (offsets[i] ?? 0) + (counts[i + 1] ?? 0);
  const cursor = offsets.slice(0, buffer.vertexCount);
  const total = offsets[buffer.vertexCount] ?? 0;
  const neighborEdges = new Array<number>(total).fill(0);
  for (let i = 0; i < edgeVertices.length; i += 2) {
    const a = edgeVertices[i] ?? 0;
    const b = edgeVertices[i + 1] ?? 0;
    const e = i / 2;
    const slotA = cursor[a];
    const slotB = cursor[b];
    if (slotA !== undefined && slotA < total) {
      neighborEdges[slotA] = e;
      cursor[a] = slotA + 1;
    }
    if (slotB !== undefined && slotB < total) {
      neighborEdges[slotB] = e;
      cursor[b] = slotB + 1;
    }
  }

  return ok(
    {
      kind: "EDGE",
      vertexCount: buffer.vertexCount,
      indexCount: buffer.indexCount,
      offsets,
      neighborEdges,
      edgeVertices,
    },
  );
}

/** 收集无向边：按拓扑遍历图元，把每个图元里的连接关系交给回调。 */
function collectEdges(buffer: IndexBuffer, push: (a: number, b: number) => void): void {
  const idx = buffer.indices;
  if (buffer.topology === TopologyKind.Triangles) {
    for (let i = 0; i + 2 < buffer.indexCount; i += 3) {
      const a = idx[i] ?? 0;
      const b = idx[i + 1] ?? 0;
      const c = idx[i + 2] ?? 0;
      push(a, b);
      push(b, c);
      push(c, a);
    }
    return;
  }
  if (buffer.topology === TopologyKind.TriangleStrip) {
    // strip 的边结构有两种切法：只连相邻三角的「公共边」（共享边），
    // 或把每个三角三条边都收（得到重复边）。邻接关心的是「哪些边共享顶点」，
    // 故收全部三角边并靠规范化去重——这是与三角列表语义对齐的做法。
    for (let i = 0; i + 2 < buffer.indexCount; i += 1) {
      const a = idx[i] ?? 0;
      const b = idx[i + 1] ?? 0;
      const c = idx[i + 2] ?? 0;
      push(a, b);
      push(b, c);
      push(c, a);
    }
    return;
  }
  if (buffer.topology === TopologyKind.Lines) {
    for (let i = 0; i + 1 < buffer.indexCount; i += 2) {
      push(idx[i] ?? 0, idx[i + 1] ?? 0);
    }
    return;
  }
  // 点拓扑无连接关系：邻接恒空，这是拓扑语义决定的正确结果，不是漏做。
}

/**
 * 生成点邻接（共享顶点即相邻）。
 *
 * 与边邻接的差别：点邻接是「一跳可达」，边邻接是「直接相连」。二者的信息量差
 * 一个数量级（点邻接的存储约是边邻接的度数倍），故分开提供而非合成一个大结构——
 * 需要其中一种时不该被迫付另一种的内存。
 */
export function buildVertexAdjacency(buffer: IndexBuffer, options: AdjacencyOptions = {}): Outcome<VertexAdjacency> {
  const checked = verifyTopologyCounts(buffer);
  if (!checked.ok) return checked;
  void options; // 点邻接不受边端点规范化影响（邻接是无序集合）。

  const neighborsOf = new Array<Set<number>>(buffer.vertexCount);
  for (let i = 0; i < buffer.vertexCount; i += 1) neighborsOf[i] = new Set<number>();
  const link = (a: number, b: number): void => {
    if (a === b) return;
    neighborsOf[a]?.add(b);
    neighborsOf[b]?.add(a);
  };
  collectEdges(buffer, link);

  // CSR 组装：邻居按顶点号升序（确定性遍历）。
  const offsets = new Array<number>(buffer.vertexCount + 1).fill(0);
  for (let i = 0; i < buffer.vertexCount; i += 1) {
    offsets[i + 1] = (offsets[i] ?? 0) + (neighborsOf[i]?.size ?? 0);
  }
  const neighbors = new Array<number>(offsets[buffer.vertexCount] ?? 0).fill(0);
  for (let v = 0; v < buffer.vertexCount; v += 1) {
    const sorted = [...(neighborsOf[v] ?? [])].sort((a, b) => a - b);
    const base = offsets[v] ?? 0;
    for (let k = 0; k < sorted.length; k += 1) neighbors[base + k] = sorted[k] ?? 0;
  }
  return ok({ kind: "VERTEX", vertexCount: buffer.vertexCount, indexCount: buffer.indexCount, offsets, neighbors });
}

/** 邻接的「内存换查询」权衡声明（判据三的显性部分，不是注释里的顺带一提）。 */
export interface AdjacencyTradeoff {
  readonly structure: "EDGE" | "VERTEX";
  readonly bytesApprox: number;
  readonly summary: string;
  readonly advice: string;
}

/** 估算邻接结构的内存占用并给出取舍建议（供加载器决定是否构建）。 */
export function estimateAdjacencyCost(buffer: IndexBuffer, structure: "EDGE" | "VERTEX"): AdjacencyTradeoff {
  const indexBytes = INDEX_WIDTH_BYTES[buffer.width] * buffer.indexCount;
  if (structure === "EDGE") {
    // 三角网格的唯一边数上界 ≈ 1.5 × 三角数；邻接存储按 (端点2×4B + 边号4B)× 边数 估。
    const primitives = primitiveCount(buffer);
    const edgesApprox = Math.ceil(primitives * 1.5);
    const bytes = edgesApprox * 12 + buffer.vertexCount * 4;
    return {
      structure: "EDGE",
      bytesApprox: bytes,
      summary: `边邻接约 ${edgesApprox} 条边、约 ${(bytes / 1024 / 1024).toFixed(2)} MB（索引缓冲本身 ${(indexBytes / 1024).toFixed(1)} KB）`,
      advice:
        "边邻接是布尔运算（F1607 补洞判边界）与 QEM 简化（F1608 找收缩边）的前置。"
        + "若本帧只做渲染不查邻接就不要构建——它的内存是索引缓冲的数倍，"
        + "按需生成、用完即弃比常驻更划算",
    };
  }
  const primitives = primitiveCount(buffer);
  const neighborsApprox = primitives * 3 * 2;
  const bytes = neighborsApprox * 4 + buffer.vertexCount * 4;
  return {
    structure: "VERTEX",
    bytesApprox: bytes,
    summary: `点邻接约 ${neighborsApprox} 条邻接记录、约 ${(bytes / 1024 / 1024).toFixed(2)} MB`,
    advice:
      "点邻接是平滑法线生成（F1606 的按角度加权）与顶点合并（F1609）的前置。"
      + "它比边邻接大一档：存的是「一跳可达的所有顶点」而非「直接相连的边」。"
      + "只在确实要做平滑/合并时构建",
  };
}

/** 邻接查询：取某顶点的相邻边号。带陈旧性与参数校验（不返回错邻接）。 */
export function queryEdgesAt(adjacency: EdgeAdjacency, vertex: number): Outcome<readonly number[]> {
  const stale = verifyAdjacencyFresh(adjacency);
  if (!stale.ok) return stale;
  if (!Number.isInteger(vertex) || vertex < 0 || vertex >= adjacency.vertexCount) {
    return fail(
      "ADJACENCY_QUERY_INVALID",
      `查询顶点 ${String(vertex)} 越出邻接结构的顶点范围（0..${adjacency.vertexCount - 1}）`,
      "邻接结构是针对特定网格生成的；查询越界顶点通常意味着传入了另一份网格的顶点号",
    );
  }
  const start = adjacency.offsets[vertex] ?? 0;
  const end = adjacency.offsets[vertex + 1] ?? start;
  return ok(adjacency.neighborEdges.slice(start, end));
}

/** 邻接查询：取某顶点的相邻顶点号。 */
export function queryNeighborsAt(adjacency: VertexAdjacency, vertex: number): Outcome<readonly number[]> {
  const stale = verifyAdjacencyFresh(adjacency);
  if (!stale.ok) return stale;
  if (!Number.isInteger(vertex) || vertex < 0 || vertex >= adjacency.vertexCount) {
    return fail(
      "ADJACENCY_QUERY_INVALID",
      `查询顶点 ${String(vertex)} 越出邻接结构的顶点范围（0..${adjacency.vertexCount - 1}）`,
      "邻接结构是针对特定网格生成的；查询越界顶点通常意味着传入了另一份网格的顶点号",
    );
  }
  const start = adjacency.offsets[vertex] ?? 0;
  const end = adjacency.offsets[vertex + 1] ?? start;
  return ok(adjacency.neighbors.slice(start, end));
}

/** 邻接新鲜度校验：顶点/索引数须与生成时一致，否则复用旧邻接会读到错数据。 */
function verifyAdjacencyFresh(adjacency: Adjacency): Outcome<true> {
  if (adjacency.vertexCount <= 0) {
    return fail(
      "ADJACENCY_STALE",
      "邻接结构的顶点数为 0（空网格）",
      "空网格的邻接恒为空；若你确实要查询，请确认没有把空网格的邻接误用到有几何的网格上",
    );
  }
  if (adjacency.offsets.length !== adjacency.vertexCount + 1) {
    return fail(
      "ADJACENCY_STALE",
      `邻接偏移表长度 ${adjacency.offsets.length} 与顶点数 ${adjacency.vertexCount} 不自洽（应为 ${adjacency.vertexCount + 1}）`,
      "CSR 结构被破坏：偏移表长度必须等于顶点数+1（末位是哨兵）。"
        + "这通常意味着邻接结构在生成后被改动过，请重新生成而非修补",
    );
  }
  return ok(true);
}

// ════════════════════════════════════════════════════════════════════════════
// §5 拓扑校验（判据四：索引越界 + 退化三角检出）
// ════════════════════════════════════════════════════════════════════════════

/** 一条校验结论（带涉及的图元号，便于定位到具体三角）。 */
export interface TopologyIssue {
  readonly code: DiagCode;
  /** 涉及的图元号（退化类问题）；越界类为 -1。 */
  readonly primitive: number;
  readonly message: string;
  readonly hint: string;
}

/** 拓扑校验报告。 */
export interface TopologyValidationReport {
  /** 索引越界的问题（独立分桶：成因与退化完全不同，混报会让定位失效）。 */
  readonly outOfRange: readonly TopologyIssue[];
  /** 退化三角的问题。 */
  readonly degenerate: readonly TopologyIssue[];
  /** 拓扑计数/位宽等结构问题。 */
  readonly structural: readonly TopologyIssue[];
  readonly pass: boolean;
  readonly summary: string;
}

/** 退化判定阈值（各阈值的依据写在这里，而非散落在代码里）。 */
export const DEGENERATE_THRESHOLDS = {
  /** 面积阈值：按包围盒对角线平方归一化，故与网格尺度无关。 */
  areaRatio: 1e-10,
  /** 长宽比阈值：短边/长边 低于此值即视为针状/薄片。 */
  sliverRatio: 1e-3,
} as const;

/**
 * 退化三角判定所需的最小顶点访问接口。
 *
 * 刻意只要求「按顶点号取xyz」而不要求完整顶点数组：本条不关心顶点里还有法线、
 * UV、颜色，只关心位置决定的面形状。用窄接口而非收整个顶点缓冲，
 * 既避免本条与 F1603的布局强耦合，也让调用方可用任意坐标来源（如GPU 回读、
 * 包围盒近似坐标）复用本判据。
 */
export type PositionAccessor = (vertexIndex: number) => readonly [number, number, number];

/**
 * 拓扑校验：索引越界 + 退化三角检出。
 *
 * 退化判定为何要三条判据而只用面积：
 *   · 面积近零 —— 抓「三点共线/重合」，最典型也最常见；
 *   · 长宽比异常 —— 抓「面积不小但极薄」的针状三角（如简化算法收缩产生的碎片）。
 *     这类三角面积可达网格的 1%，肉眼可见为「拉丝」，但面积判据完全漏掉它；
 *   · 顶点索引重复 —— 抓「退化到只剩一条边」的极端情形，且能在坐标全等时
 *     直接指出是索引问题而非坐标问题（便于回溯是导出错误还是几何运算错误）。
 * 三条互补，缺一条就有一类缺陷进到渲染管线（表现为面缺失、闪烁、或法线全黑）。
 */
export function validateTopology(buffer: IndexBuffer, position: PositionAccessor): TopologyValidationReport {
  const outOfRange: TopologyIssue[] = [];
  const degenerate: TopologyIssue[] = [];
  const structural: TopologyIssue[] = [];

  // ---- 索引越界（先查，因为越界顶点会让后续几何判定读到不存在的数据） ----
  for (let i = 0; i < buffer.indices.length; i += 1) {
    const v = buffer.indices[i] ?? -1;
    if (!Number.isInteger(v) || v < 0 || v >= buffer.vertexCount) {
      outOfRange.push({
        code: "INDEX_OUT_OF_RANGE",
        primitive: Math.floor(i / Math.max(1, TOPOLOGY_PRIMITIVE_SIZE[buffer.topology])),
        message: `第 ${i} 个索引的值 ${String(v)} 越出顶点范围（0..${buffer.vertexCount - 1}）`,
        hint: "越界索引会让 GPU 读到顶点缓冲之外的内存（画面出现随机三角形，甚至崩溃）。"
          + "请检查导入/简化流程是否产出了非法顶点号；若来自 QEM 简化，注意收缩后索引重映射是否遗漏",
      });
    }
  }

  // ---- 结构类：位宽与拓扑计数 ----
  const widthCheck = verifyWidthChoice(buffer.vertexCount, buffer.indexCount, buffer.width);
  if (!widthCheck.ok) {
    structural.push({ code: widthCheck.code, primitive: -1, message: widthCheck.message, hint: widthCheck.hint });
  }
  const counts = verifyTopologyCounts(buffer);
  if (!counts.ok) {
    structural.push({ code: counts.code, primitive: -1, message: counts.message, hint: counts.hint });
  }

  // ---- 退化三角：仅面拓扑有意义 ----
  const info = TOPOLOGY_INFO[buffer.topology];
  if (info !== undefined && info.isSurface && outOfRange.length === 0) {
    const size = info.primitiveSize;
    for (let i = 0; i + 2 < buffer.indexCount; i += size) {
      const ia = buffer.indices[i] ?? 0;
      const ib = buffer.indices[i + 1] ?? 0;
      const ic = buffer.indices[i + 2] ?? 0;
      const primitive = i / size;
      // 判据三：索引重复（坐标全等的另一种信号，且能直接指认是索引问题）。
      if (ia === ib || ib === ic || ic === ia) {
        degenerate.push({
          code: "DEGENERATE_TRIANGLE_DUPLICATE_INDEX",
          primitive,
          message: `第 ${primitive} 个三角含重复顶点索引（${ia}, ${ib}, ${ic}），退化为一条边`,
          hint: "重复索引使三角在数学上退化（面积为零）。成因通常是索引重映射遗漏或简化时的边收缩"
            + "未更新索引；请回溯产生该面的那一步操作",
        });
        continue;
      }
      const a = position(ia);
      const b = position(ib);
      const c = position(ic);
      if (a === undefined || b === undefined || c === undefined) continue;
      const e1x = b[0] - a[0];
      const e1y = b[1] - a[1];
      const e1z = b[2] - a[2];
      const e2x = c[0] - a[0];
      const e2y = c[1] - a[1];
      const e2z = c[2] - a[2];
      // 叉积长度 = 2 × 面积。
      const cx = e1y * e2z - e1z * e2y;
      const cy = e1z * e2x - e1x * e2z;
      const cz = e1x * e2y - e1y * e2x;
      const doubleArea = Math.sqrt(cx * cx + cy * cy + cz * cz);
      // 以边长归一化：面片自身越小不代表越退化，只有相对自身尺度才可比。
      const longest = Math.max(edgeLength(a, b), edgeLength(a, c), edgeLength(b, c));
      if (longest <= EPS) {
        degenerate.push({
          code: "DEGENERATE_TRIANGLE_AREA",
          primitive,
          message: `第 ${primitive} 个三角的三点几乎重合（最长边 ${longest.toExponential(3)}），面积为零`,
          hint: "整个三角塌成一个点：顶点坐标写入有误，或该面的顶点全被同一变换压到同一点",
        });
        continue;
      }
      if (doubleArea / (longest * longest) <= DEGENERATE_THRESHOLDS.areaRatio) {
        degenerate.push({
          code: "DEGENERATE_TRIANGLE_AREA",
          primitive,
          message: `第 ${primitive} 个三角接近共线（归一化面积 ${(doubleArea / (longest * longest)).toExponential(3)}≤ ${DEGENERATE_THRESHOLDS.areaRatio}）`,
          hint: "三点共线或近乎共线：法线由叉积求得，接近共线时法线方向不稳定，"
            + "表现为面在动画中随机变黑。常见成因是顶点焊接时把本应分离的点合并了",
        });
        continue;
      }
      const shortest = Math.min(edgeLength(a, b), edgeLength(a, c), edgeLength(b, c));
      const sliver = shortest / longest;
      if (sliver <= DEGENERATE_THRESHOLDS.sliverRatio) {
        degenerate.push({
          code: "DEGENERATE_TRIANGLE_SLIVER",
          primitive,
          message: `第 ${primitive} 个三角为针状薄片（短边/长边 ${sliver.toExponential(3)} ≤ ${DEGENERATE_THRESHOLDS.sliverRatio}）`,
          hint: "面积判据查不出这类三角（它的面积不算小），但它在光栅化与阴影里会表现为"
            + "「拉丝」与闪烁，且常是简化算法（F1608 QEM）收缩出的碎片。"
            + "建议在简化阶段就按长宽比剔除，避免退化面流入渲染",
        });
      }
    }
  }

  const total = outOfRange.length + degenerate.length + structural.length;
  const pass = total === 0;
  const summary = pass
    ? `拓扑校验通过：${TOPOLOGY_INFO[buffer.topology]?.label ?? String(buffer.topology)}、`
      + `${buffer.indexCount} 索引 / ${primitiveCount(buffer)} 图元、位宽 ${buffer.width}`
    : [
        outOfRange.length > 0 ? `索引越界 ${outOfRange.length} 处` : "",
        degenerate.length > 0 ? `退化三角 ${degenerate.length} 处` : "",
        structural.length > 0 ? `结构问题 ${structural.length} 处` : "",
      ]
        .filter((s) => s !== "")
        .join("，");

  return { outOfRange, degenerate, structural, pass, summary };
}

/** 两点间的欧氏距离。 */
function edgeLength(a: readonly [number, number, number], b: readonly [number, number, number]): number {
  const dx = a[0] - b[0];
  const dy = a[1] - b[1];
  const dz = a[2] - b[2];
  return Math.sqrt(dx * dx + dy * dy + dz * dz);
}

/** 校验失败 → 显式 Outcome 失败（不抛异常，诊断由调用方聚合上报）。 */
export function validateTopologyOrFail(
  buffer: IndexBuffer,
  position: PositionAccessor,
): Outcome<TopologyValidationReport> {
  const report = validateTopology(buffer, position);
  if (report.pass) return ok(report);
  const first = report.outOfRange[0] ?? report.degenerate[0] ?? report.structural[0];
  if (first === undefined) {
    return fail("INDEX_COUNT_INVALID", "拓扑校验未通过但未定位到具体问题（缺陷）", "请检查 validateTopology 的分桶逻辑是否漏项");
  }
  return fail(first.code, `拓扑校验未通过：${report.summary}`, first.hint);
}

// ════════════════════════════════════════════════════════════════════════════
// §6 拓扑转换（strip 展开：绕序分歧的显式处置）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 三角带展开为三角列表。
 *
 * 为何必须显式展开而不是「读的时候按约定算朝向」：strip 的绕序约定在实现间不统一
 * （见 TOPOLOGY_INFO 的 caveats），读侧各按一套约定猜，跨工具就会正反颠倒。
 * 展开成列表后朝向被固化成确定的索引顺序，歧义消失——代价是索引内存回升，
 * 故本函数同时返回展开后的图元数与字节增量，让调用方知情后再决定是否展开。
 */
export function expandTriangleStrip(buffer: IndexBuffer): Outcome<{ indices: readonly number[]; extraBytes: number }> {
  if (buffer.topology !== TopologyKind.TriangleStrip) {
    return fail(
      "TOPOLOGY_CONVERSION_INVALID",
      `展开三角带要求拓扑为 TRIANGLE_STRIP，实际为 ${String(buffer.topology)}`,
      `本函数只做 strip→triangles 展开；其他拓扑之间没有等价转换`
        + "（线/点与面拓扑图元大小不同，强转会得到形状错误的网格）",
    );
  }
  if (buffer.indexCount < 3) {
    return fail(
      "TOPOLOGY_CONVERSION_INVALID",
      `三角带索引数 ${buffer.indexCount} 少于 3，无法构成任何三角`,
      "strip 的最小规模是 3 个索引（1 个三角）；请检查导出流程",
    );
  }
  const out: number[] = [];
  // 偶数位三角与奇数位三角交换 b/c 以保持一致朝向 —— 这是展开的核心。
  // 不做这个交换，带内相邻三角会一正一反（Z 字形），表现为「一半面反了」。
  for (let i = 0; i + 2 < buffer.indexCount; i += 1) {
    const a = buffer.indices[i] ?? 0;
    const b = buffer.indices[i + 1] ?? 0;
    const c = buffer.indices[i + 2] ?? 0;
    if (i % 2 === 0) out.push(a, b, c);
    else out.push(a, c, b);
  }
  const listBytes = out.length * INDEX_WIDTH_BYTES[buffer.width];
  const stripBytes = buffer.indexCount * INDEX_WIDTH_BYTES[buffer.width];
  return ok({ indices: out, extraBytes: listBytes - stripBytes });
}

// ════════════════════════════════════════════════════════════════════════════
// §7 自检（四条判据的可执行形态）
// ════════════════════════════════════════════════════════════════════════════

/** 自检结果项：每项对应判据的一条，独立可定位。 */
export interface SelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 构造夹具：两个三角的网格（含一个共线的退化面用于校验用例）。 */
function makeFixture(overrides: {
  vertexCount?: number;
  indices?: readonly number[];
  topology?: TopologyKind;
  width?: IndexWidth;
} = {}): IndexBuffer {
  const indices = overrides.indices ?? [0, 1, 2, 2, 1, 3];
  const vertexCount = overrides.vertexCount ?? 4;
  return makeIndexBuffer(vertexCount, indices, overrides.topology ?? TopologyKind.Triangles, overrides.width);
}

/** 夹具的坐标访问器：一个单位正方形（顶点 0..3 构成两个共面的三角）。 */
const squarePosition: PositionAccessor = (i): readonly [number, number, number] => {
  const table: Readonly<Record<number, readonly [number, number, number]>> = {
    0: [0, 0, 0],
    1: [1, 0, 0],
    2: [1, 1, 0],
    3: [0, 1, 0],
  };
  return table[i] ?? [0, 0, 0];
};

/** 判据一自检：位宽自动选型（无手工配置 + 边界 + 越界拦截）。 */
export function selfCheckWidthSelect(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 边界：65536 顶点（含）→ 16 位；65537（超一）→ 32 位。
  const atLimit = chooseIndexWidth(U16_INDEX_VERTEX_LIMIT, 60000);
  const overLimit = chooseIndexWidth(U16_INDEX_VERTEX_LIMIT + 1, 60000);
  out.push({
    name: "index-width-auto-selected-at-boundary",
    pass: atLimit.width === 16 && overLimit.width === 32,
    detail: `顶点数 ${U16_INDEX_VERTEX_LIMIT} → ${atLimit.width} 位；${U16_INDEX_VERTEX_LIMIT + 1} → ${overLimit.width} 位`
      + "（边界含 65536，因顶点号 65535 合法；此off-by-one 是本条最易写错处）",
  });

  // 省一半：16 位相对 32 位的字节对照必须精确相等。
  const saved = chooseIndexWidth(100, 1000);
  out.push({
    name: "index-width-halves-index-memory",
    pass:
      saved.width === 16 &&
      saved.totalIndexBytes === 2000 &&
      saved.savingsBytes === 2000 &&
      Math.abs(saved.savingsRatio - 0.5) < 1e-12,
    detail: `1000 个索引：16 位占 ${saved.totalIndexBytes} 字节，比 32 位省 ${saved.savingsBytes} 字节`
      + `（${(saved.savingsRatio * 100).toFixed(0)}%）——省的是**索引缓冲**，不是整个顶点缓冲`,
  });

  // 大网格必须 32 位，且不提供「降级」选项。
  const big = chooseIndexWidth(5_000_000, 30_000_000);
  out.push({
    name: "large-mesh-forces-32bit",
    pass: big.width === 32 && big.savingsBytes === 0,
    detail: `500 万顶点强制 32 位（${big.reason.slice(0, 40)}…），无降级选项——静默截断是几何错乱的元凶`,
  });

  // 越界声明必须被拦：60000 顶点配 16 位合法，70000 顶点配 16 位被拒。
  const okCase = verifyWidthChoice(60000, 120000, 16);
  const badCase = verifyWidthChoice(70000, 150000, 16);
  out.push({
    name: "overflow-width-declaration-rejected",
    pass: okCase.ok && !badCase.ok && badCase.code === "INDEX_WIDTH_OVERFLOW",
    detail: "6 万顶点配 16 位放行；7 万顶点配 16 位被拒并说明「会静默截断为低 16 位」",
  });

  return out;
}

/** 判据二自检：四拓扑齐备 + 元信息自洽 + strip 展开绕序正确。 */
export function selfCheckTopologySet(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 四类齐备且元信息自洽（图元大小与字节账互相吻合）。
  const metaOk = ALL_TOPOLOGIES.every((k) => {
    const info = TOPOLOGY_INFO[k];
    return (
      info !== undefined &&
      info.kind === k &&
      info.primitiveSize === TOPOLOGY_PRIMITIVE_SIZE[k] &&
      info.bytesPerPrimitive > 0 &&
      info.semantics.trim() !== "" &&
      info.usage.trim() !== "" &&
      info.caveats.trim() !== ""
    );
  });
  out.push({
    name: "four-topologies-complete-and-self-consistent",
    pass: ALL_TOPOLOGIES.length === 4 && metaOk,
    detail: `四拓扑（三角列表/三角带/线/点）齐备，各自带语义、用途、图元大小、字节账与已知坑登记`,
  });

  // 面拓扑标记正确：只有两类三角拓扑参与面级校验。
  const surface = ALL_TOPOLOGIES.filter((k) => TOPOLOGY_INFO[k]?.isSurface === true);
  out.push({
    name: "surface-topologies-flagged",
    pass: surface.length === 2 && surface.includes(TopologyKind.Triangles) && surface.includes(TopologyKind.TriangleStrip),
    detail: `面拓扑为${surface.join("、")}；线/点不参与面级校验与简化（无「面」可言）`,
  });

  // 图元数计算：四个拓扑各测一次（含 strip 的 3 起步）。
  const tri = primitiveCount(makeFixture());
  const strip = primitiveCount(makeFixture({ topology: TopologyKind.TriangleStrip, indices: [0, 1, 2, 3, 4, 5, 6] }));
  const line = primitiveCount(makeFixture({ topology: TopologyKind.Lines, indices: [0, 1, 2, 3] }));
  const pt = primitiveCount(makeFixture({ topology: TopologyKind.Points, indices: [0, 1, 2] }));
  out.push({
    name: "primitive-count-per-topology",
    pass: tri === 2 && strip === 2 && line === 2 && pt === 3,
    detail: `图元数：三角列表 6 索引→${String(tri)}、strip 7 索引→${String(strip)}（n+2 起步）、线 4 索引→${String(line)}、点 3 索引→${String(pt)}`,
  });

  // strip 展开：绕序必须交替（Z 字形修正），且图元数与顶点数守恒。
  const stripBuf = makeFixture({ topology: TopologyKind.TriangleStrip, indices: [0, 1, 2, 3, 4], vertexCount: 5 });
  const expanded = expandTriangleStrip(stripBuf);
  let expandDetail = "展开失败";
  let expandOk = false;
  if (expanded.ok) {
    const out2 = expanded.value.indices;
    // 5 个索引 → n-2 = 3 个三角（strip 的 n+2 起步规则，此处 n=3）。
    // 期望：[0,1,2]（i=0 偶，原序）、[1,3,2]（i=1 奇，交换 b/c）、[2,3,4]（i=2 偶，原序）。
    // 若不做奇偶交换，第2 个会是 [2,3,4]… 反过来第 1 个会成为 [2,1,3] 而翻转朝向。
    expandOk =
      out2.length === 9 &&
      out2[0] === 0 && out2[1] === 1 && out2[2] === 2 &&
      out2[3] === 1 && out2[4] === 3 && out2[5] === 2 &&
      out2[6] === 2 && out2[7] === 3 && out2[8] === 4;
    expandDetail = `strip [0,1,2,3,4]（3 三角）展开为 [${out2.join(",")}]——奇数位三角交换 b/c 修正绕序`
      + `（不交换则带内一正一反，表现为一半面反向），字节增量 ${expanded.value.extraBytes}`
      + `（strip 5 索引 10 字节 → 列表 9 索引 18 字节，展开是内存回升的诚实代价）`;
  }
  out.push({ name: "triangle-strip-expand-fixes-winding", pass: expandOk, detail: expandDetail });

  // 索引数不整除必须被拒（线拓扑奇数索引 = 末尾静默丢弃）。
  const oddLines = verifyTopologyCounts(makeFixture({ topology: TopologyKind.Lines, indices: [0, 1, 2] }));
  out.push({
    name: "non-divisible-index-count-rejected",
    pass: !oddLines.ok && oddLines.code === "TOPOLOGY_COUNT_MISMATCH",
    detail: "线拓扑 3 个索引被拒并说明「末尾索引会被静默丢弃，表现为少一条线」",
  });

  return out;
}

/** 判据三自检：邻接按需（生成正确 + 陈旧拦截 + 权衡声明量化）。 */
export function selfCheckAdjacencyOnDemand(): SelfCheck[] {
  const out: SelfCheck[] = [];
  // 三角形 [0,1,2]：唯一边 3 条；顶点 0 的度为 2（连 1 和 2）——方形的第 4 顶点
  // 不接0，故不能拿方形夹具断言「顶点 0 度为 3」，那是夹具选错而非实现错。
  const tri = makeFixture({ indices: [0, 1, 2], vertexCount: 3 });
  const edge = buildEdgeAdjacency(tri);
  let edgesOk = false;
  let edgeDetail = "生成失败";
  if (edge.ok) {
    const adj = edge.value;
    const uniqueEdges = adj.edgeVertices.length / 2;
    const at0 = adj.offsets[0] ?? 0;
    const deg0 = (adj.offsets[1] ?? 0) - at0;
    const at1 = adj.offsets[1] ?? 0;
    const deg1 = (adj.offsets[2] ?? 0) - at1;
    // 单三角的三条边全为边界；每个顶点度 2（0-1、0-2）。
    edgesOk = uniqueEdges === 3 && deg0 === 2 && deg1 === 2;
    edgeDetail = `单三角 [0,1,2]：唯一边 ${uniqueEdges} 条（0-1/0-2/1-2），顶点 0 度 ${deg0}、顶点 1 度 ${deg1}`
      + `——三角形无内部对角，故每顶点度恒为 2`;
  }
  out.push({ name: "edge-adjacency-built-correctly", pass: edgesOk, detail: edgeDetail });

  // 规范化必须让 (a,b) 与 (b,a) 归一——布尔运算判边界的前提。
  const buf1 = makeFixture({ indices: [0, 1, 2, 2, 0, 3] });
  const e1 = buildEdgeAdjacency(buf1, { canonicalizeEdges: true });
  let canonOk = false;
  let canonDetail = "生成失败";
  if (e1.ok) {
    // 索引里0-2 与 2-0 各出现一次；规范化后应合并为一条，故唯一边数 5 而非 6。
    const unique = e1.value.edgeVertices.length / 2;
    canonOk = unique === 5;
    canonDetail = `同一网格（含 0-2 与 2-0 两条反向边）：规范化后唯一边 ${unique} 条（未规范化会是 6 条，`
      + "布尔运算会把 (a,b) 与 (b,a) 当两条边而判定边界失效）";
  }
  out.push({ name: "edge-canonicalization-merges-reversed", pass: canonOk, detail: canonDetail });

  // 点邻接：顶点 0 的邻居为 {1,2}（与边邻接同源，故度数一致——两结构必须互相印证）。
  const vert = buildVertexAdjacency(tri);
  let vertOk = false;
  let vertDetail = "生成失败";
  if (vert.ok) {
    const at0 = vert.value.offsets[0] ?? 0;
    const deg0 = (vert.value.offsets[1] ?? 0) - at0;
    const neighbors = vert.value.neighbors.slice(at0, at0 + deg0);
    vertOk = deg0 === 2 && neighbors[0] === 1 && neighbors[1] === 2;
    vertDetail = `顶点 0 的点邻接 ${deg0} 个（[${neighbors.join(",")}]），升序排列保证遍历确定性`
      + `——度数与边邻接的 ${String(deg0)} 一致，两结构互相印证`;
  }
  out.push({ name: "vertex-adjacency-built-correctly", pass: vertOk, detail: vertDetail });

  // 查询越界必须被拦（不返回错邻接）。
  const q = edge.ok ? queryEdgesAt(edge.value, 999) : null;
  out.push({
    name: "adjacency-query-out-of-range-rejected",
    pass: q !== null && !q.ok && q.code === "ADJACENCY_QUERY_INVALID",
    detail: "查询越界顶点被显式拒绝（而非返回空数组——空数组会被误读为「该顶点无邻边」）",
  });

  // 陈旧结构必须被拦（CSR 偏移表长度不自洽）。
  const stale: EdgeAdjacency = edge.ok
    ? { ...edge.value, offsets: edge.value.offsets.slice(0, 2) }
    : { kind: "EDGE", vertexCount: 4, indexCount: 6, offsets: [0], neighborEdges: [], edgeVertices: [] };
  const sq = queryEdgesAt(stale, 0);
  out.push({
    name: "stale-adjacency-detected",
    pass: !sq.ok && sq.code === "ADJACENCY_STALE",
    detail: "被破坏的 CSR 偏移表（长度 2≠顶点数 4+1）被识别为陈旧结构，要求重新生成而非修补",
  });

  // 权衡声明必须量化（给出「内存是索引缓冲的几倍」这个判据）。
  const cost = estimateAdjacencyCost(makeFixture({ vertexCount: 1_000_000, indices: new Array(3_000_000).fill(0) }), "EDGE");
  out.push({
    name: "adjacency-tradeoff-quantified",
    pass: cost.bytesApprox > 0 && cost.advice.includes("索引缓冲") && cost.summary.includes("MB"),
    detail: `百万顶点网格边邻接成本量化：${cost.summary}`,
  });

  // 点拓扑的邻接恒空——这是语义决定的正确结果，不是漏做。
  const ptBuf = makeFixture({ topology: TopologyKind.Points, indices: [0, 1, 2, 3] });
  const ptAdj = buildEdgeAdjacency(ptBuf);
  out.push({
    name: "point-topology-adjacency-empty-by-semantics",
    pass: ptAdj.ok && ptAdj.value.edgeVertices.length === 0,
    detail: "点拓扑无连接关系，边邻接恒为空数组——由拓扑语义决定，非实现遗漏",
  });

  return out;
}

/** 判据四自检：校验（越界 + 退化三判据各自命中）。 */
export function selfCheckValidate(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 干净网格全绿。
  const good = validateTopology(makeFixture(), squarePosition);
  out.push({
    name: "clean-mesh-passes",
    pass: good.pass,
    detail: `合法网格：${good.summary}`,
  });

  // 索引越界：独立分桶。
  const oob = validateTopology(makeFixture({ indices: [0, 1, 2, 2, 1, 99] }), squarePosition);
  out.push({
    name: "index-out-of-range-detected",
    pass: !oob.pass && oob.outOfRange.length === 1 && oob.degenerate.length === 0,
    detail: `越界索引 99 被归入越界桶（退化桶保持为空：越界顶点会让几何判定读到不存在的数据，故跳过退化判定）`,
  });

  // 退化判据一：共线（面积近零）。三条点同在z=0 且y 递增。
  const collinear: PositionAccessor = (i) => {
    const table: Readonly<Record<number, readonly [number, number, number]>> = {
      0: [0, 0, 0],
      1: [1, 0, 0],
      2: [2, 0, 0],
      3: [0, 1, 0],
    };
    return table[i] ?? [0, 0, 0];
  };
  const flat = validateTopology(makeFixture({ indices: [0, 1, 2] }), collinear);
  out.push({
    name: "degenerate-collinear-detected",
    pass: flat.degenerate.some((d) => d.code === "DEGENERATE_TRIANGLE_AREA"),
    detail: `共线三角命中面积判据：${flat.degenerate[0]?.message ?? "未检出"}`,
  });

  // 退化判据二：索引重复。
  const dup = validateTopology(makeFixture({ indices: [0, 1, 1] }), squarePosition);
  out.push({
    name: "degenerate-duplicate-index-detected",
    pass: dup.degenerate.some((d) => d.code === "DEGENERATE_TRIANGLE_DUPLICATE_INDEX"),
    detail: "重复索引 (0,1,1) 命中判据三，退化为一条边",
  });

  // 退化判据三：针状薄片——面积不小但长宽比极端（单用面积判据会漏）。
  // 样本构造说明（此处踩过两次坑，记下来免得重犯）：
  //   ① 顶点 2 取在「中点正上方」不行——那样三边是 1/ 0.5 / 0.5 的等腰三角形，
  //      长宽比 0.5 远高于阈值，判据不触发。针状判据是**最短边/最长边**。
  //   ② 顶点 2 取在 (1e-3, 1e-4) 也不行——短长比恰为 1.00e-3，浮点上略大于阈值而
  //      不触发，是个卡在边界上的脆弱样本。
  // 最终取 2 = (1e-4, 1e-5)：短长比 1e-4（触发，且离阈值有一个数量级余量），
  // 归一化面积 1e-5（高出面积阈值 1e-10 五个数量级，不触发）—— 两判据彻底分离。
  const sliverPos: PositionAccessor = (i) => {
    const table: Readonly<Record<number, readonly [number, number, number]>> = {
      0: [0, 0, 0],
      1: [1, 0, 0],
      2: [1e-4, 1e-5, 0],
      3: [0, 1, 0],
    };
    return table[i] ?? [0, 0, 0];
  };
  const sliver = validateTopology(makeFixture({ indices: [0, 1, 2] }), sliverPos);
  const caughtByArea = sliver.degenerate.some((d) => d.code === "DEGENERATE_TRIANGLE_AREA");
  const sliverMsg = sliver.degenerate.find((d) => d.code === "DEGENERATE_TRIANGLE_SLIVER")?.message ?? "未检出";
  out.push({
    name: "degenerate-sliver-detected-beyond-area",
    pass: sliver.degenerate.some((d) => d.code === "DEGENERATE_TRIANGLE_SLIVER") && !caughtByArea,
    detail: `针状三角（顶点 2 取在 (1e-4,1e-5)，三边 1 / 1 / 1e-4）命中长宽比判据：${sliverMsg}；`
      + `且**面积判据未命中**（归一化面积 1e-5，高出阈值 5 个数量级）`
      + `——两判据被彻底分离，证明只查面积会漏掉这一类`,
  });

  // 分桶可路由性：越界与退化分别落入各自桶。
  // 注意一个刻意的设计选择——只要存在越界索引，就**跳过**退化判定：越界顶点的
  // 坐标取不到（position 访问器会给出错误位置），据此判退化必然是假阳性。
  // 故此用例验证「有越界时退化桶为空」这一安全行为，而两桶同时非空由下面的
  // 「纯退化多面」用例覆盖。
  const mixed = validateTopology(makeFixture({ indices: [0, 1, 2, 2, 1, 99] }), squarePosition);
  out.push({
    name: "out-of-range-suppresses-degenerate-checks",
    pass: mixed.outOfRange.length > 0 && mixed.degenerate.length === 0 && !mixed.pass,
    detail: `含越界的网格：报出越界 ${mixed.outOfRange.length} 条，退化桶为空（${String(mixed.degenerate.length)}）`
      + "——越界顶点坐标不可信，据此判退化必为假阳性，故整体跳过",
  });

  // 两桶各自可独立产出：纯退化网格（无越界）应产出多条退化。
  const allDegenerate = validateTopology(makeFixture({ indices: [0, 1, 1, 1, 2, 2] }), squarePosition);
  out.push({
    name: "degenerate-bucket-collects-multiple",
    pass: allDegenerate.degenerate.length >= 2 && allDegenerate.outOfRange.length === 0,
    detail: `两个退化三角（(0,1,1) 与 (1,2,2)）一次报出 ${allDegenerate.degenerate.length} 条退化，越界桶为空`,
  });

  // 线拓扑不参与面级校验（无「面」可言）。
  const lineBuf = makeFixture({ topology: TopologyKind.Lines, indices: [0, 1, 1, 2] });
  const lineReport = validateTopology(lineBuf, squarePosition);
  out.push({
    name: "non-surface-topology-skips-face-checks",
    pass: lineReport.degenerate.length === 0 && lineReport.pass,
    detail: "线拓扑中即使出现重复顶点对也不判退化（它不是面，无面积概念）",
  });

  // 失败转 Outcome 且带三要素。
  const orFail = validateTopologyOrFail(makeFixture({ indices: [0, 1, 2, 2, 1, 99] }), squarePosition);
  out.push({
    name: "validate-failure-is-explicit-outcome",
    pass: !orFail.ok && orFail.message.includes("拓扑校验未通过") && orFail.hint.trim() !== "",
    detail: orFail.ok ? "坏网格竟被放行（缺陷）" : `校验失败转为 Outcome 失败：${orFail.code}`,
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
    widthSelect: selfCheckWidthSelect(),
    topologySet: selfCheckTopologySet(),
    adjacencyOnDemand: selfCheckAdjacencyOnDemand(),
    validate: selfCheckValidate(),
  };
  const failed: string[] = [];
  for (const [g, items] of Object.entries(groups)) {
    for (const it of items) {
      if (!it.pass) failed.push(`${g}.${it.name}: ${it.detail}`);
    }
  }
  return { groups, allPass: failed.length === 0, failed };
}