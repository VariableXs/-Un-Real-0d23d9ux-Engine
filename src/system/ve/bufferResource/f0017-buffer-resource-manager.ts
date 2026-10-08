/**
 * VE-F0017 · 缓冲资源管理器（VE-A 域 · 资源与管线对象组 · 批次 A02）
 * ---------------------------------------------------------------------------
 * 职责定位：顶点/索引/统一缓冲的统一管理（子分配器 + 碎片整理）；缓冲别名
 * 安全（别名使用显式声明）；上传堆与默认堆的策略选择；含子分配的碎片率
 * 监控与整理阈值。
 *
 * 上游：VE-F0008 GPU 资源句柄表（句柄代数戳、复用清零）、VE-F0016 纹理
 *      资源管理器（本条的**格式与预算纪律同构**：预算申请/归还、诊断三要素、
 *      代戳防旧代句柄）、A05 显存预算池。
 * 下游：VE-F0019 混合状态机、VE-F0022 顶点输入布局描述器、F0017 的
 *      aliasing 声明表供验证器与渲染通道消费。
 *
 * 锚点契约（七条，逐条对应判据）：
 *   1. 统一管理：三类缓冲（vertex / index / uniform）走**同一套**生命周期
 *      与台账，不为每类各做一份。差异只体现在**对齐与用途约束**两处数据上。
 *   2. 子分配器：大缓冲按「对齐后的块」从父分配里切出，切分必须留下**可
 *      合并的空洞账**——碎片整理的正确性完全依赖这份账。切分前的空闲区
 *      搜索须用**首次适配**并按地址升序，保证确定性。
 *   3. 碎片率监控与整理阈值：碎片率 = 空闲字节 / 父分配字节；超过阈值触发
 *      整理，但**交互高峰必须暂停整理**（暂停机制是判据的独立一条，见 6）。
 *   4. 上传节流：每帧上传字节数上限（与 F0016 的帧预算是同一条纪律的
 *      缓冲侧镜像），超出的排队顺延，绝不截断。
 *   5. 别名安全：别名（同一显存被两处声明使用）必须**显式声明**，且声明
 *      与实际使用须由**验证器**核对一致——声明了不用是虚报（审计），
 *      用了没声明是违例（拒绝）。
 *   6. 暂停整理机制：交互高峰（帧预算紧张 / 用户正在拖拽）时整理必须暂停。
 *      暂停是**延后**不是**放弃**——高峰过后整理队列原样恢复。
 *   7. 越界写入阻断：子分配的写入若越出本块边界，即为**阻断级**缺陷。
 *      边界判定用本块的 [offset, offset+size) 半开区间，越界 1 字节也拦。
 *
 * 五条容易做错、故显式记录的设计立场：
 *
 *   一、子分配的对齐必须在「切」之前算，不能在「用」的时候对齐。
 *     朴素做法是按请求大小直接切，用的时候再按对齐向上取——这会让实际
 *     占用大于台账记录，碎片率随之虚高，且整理无法合并（空洞边界不再
 *     对齐，合并条件永不成立）。故本条在 `allocateRegion` 入口就把
 *     size 与 offset 双向对齐，并让台账只记对齐后的值。
 *
 *   二、碎片整理必须是「搬迁规划」，只重建空闲链表是空操作。
 *     这是本条被运行时探针纠正过的设计错误，值得写明：空闲区在释放时
 *     已就地合并，若活跃区不搬迁，空洞永远被活跃区隔开——「重建空闲链表」
 *     得到的形态与整理前**逐位一致**，等于什么都没做。真正的整理要把活跃区
 *     向低地址紧凑排列使空洞集中到尾部；但搬数据不可行（GPU 缓冲无法原地
 *     搬）。故本条的整理产出的是**搬迁规划**：每个活跃区的目标偏移、搬迁
 *     清单、搬迁后的空洞形态，由上层换新父分配 + 重传来落实。
 *
 *   三、整理的暂停必须按「请求来源」而非按「时间」判定。
 *     按时间判（每隔 N 帧整理一次）在交互卡顿时照样会整理——而卡顿正是
 *     最不该整理的时刻。故本条的暂停信号来自**帧预算余量与交互标志**，
 *     由调用方显式传入；时间不是判据。
 *
 *   四、别名验证器必须双向核对，且「不一致」分两种处置方向。
 *     声明了未使用 → 虚报（审计，不阻断，因为可能只是尚未提交绘制）；
 *     使用了未声明 → 违例（拒绝，阻断级）。两种方向相反，绝不共用码。
 *
 *   五、越界阻断不得「钳制后继续」。
 *     朴素做法是把越界写裁剪到边界内——那会把一次越界缺陷变成静默的
 *     数据错误，比越界本身更难查。故本条对越界一律拒绝并产出阻断级诊断，
 *     调用方须显式重新分配，而不是让管理器替它猜意图。
 *
 * 零静默纪律：越界写入（阻断）、别名违例（阻断）、碎片超标（整理）、
 *      整理被暂停（延后记录）、上传节流溢出（顺延）、堆策略不匹配
 *      （纠正建议）、别名虚报（审计），全部产出 BufferDiagnostic
 *      （code + message + hint），降级一律显性不做暗转。
 *
 * 判据：统一管理、子分配、别名声明、越界阻断、上传节流、暂停整理、
 *      验证器、堆策略理由。
 * 边界：本条是**零运行时契约层**（常量表 + 纯函数 + 台账类 + 诊断袋），
 *      不持有设备句柄、不发起 GPU 调用；与 A08 句柄治理通过显式的
 *      代戳与清零契约对接。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型
// ════════════════════════════════════════════════════════════════════════════

/**
 * 本条专属诊断码。
 * 处置方向相反的状态刻意不共码：
 *   - 别名类：ALIAS_UNDECLARED_USE（阻断，必须拒绝）vs ALIAS_OVERDECLARED（审计，非阻断）。
 *   - 整理类：FRAGMENTATION_EXCEEDED（触发整理）vs COMPACTION_DEFERRED（延后，非阻断）。
 */
export type BufferDiagCode =
  // 描述与用途
  | "USAGE_EMPTY"
  | "USAGE_INCOMPATIBLE"
  | "SIZE_INVALID"
  | "ALIGNMENT_INVALID"
  | "INDEX_FORMAT_UNKNOWN"
  // 子分配与边界
  | "OUT_OF_BOUNDS"
  | "REGION_OVERLAP"
  | "PARENT_ALLOC_TOO_SMALL"
  | "FRAGMENTATION_EXCEEDED"
  | "COMPACTION_DEFERRED"
  | "COMPACTION_REQUIRES_MIGRATION"
  // 别名
  | "ALIAS_CONFLICT"
  | "ALIAS_UNDECLARED_USE"
  | "ALIAS_OVERDECLARED"
  // 上传节流
  | "UPLOAD_THROTTLED"
  | "UPLOAD_BYTE_LIMIT_INVALID"
  // 堆策略
  | "HEAP_POLICY_MISMATCH"
  // 生命周期
  | "ILLEGAL_TRANSITION"
  | "GENERATION_STALE"
  | "BUFFER_RETIRED_REUSE"
  // 跨域协议
  | "PROTOCOL_HASH_PENDING"
  | "PROTOCOL_HASH_DRIFT";

/** 一条诊断：发生了什么、影响什么、下一步怎么办。 */
export interface BufferDiagnostic {
  readonly code: BufferDiagCode;
  readonly message: string;
  readonly hint: string;
}

/** 结果判别联合。 */
export type BufferOutcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly BufferDiagnostic[] }
  | {
      readonly ok: false;
      readonly code: BufferDiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly BufferDiagnostic[];
    };

export function bufOk<T>(value: T, diagnostics: readonly BufferDiagnostic[] = []): BufferOutcome<T> {
  return { ok: true, value, diagnostics };
}

export function bufFail<T>(
  code: BufferDiagCode,
  message: string,
  hint: string,
  prior: readonly BufferDiagnostic[] = [],
): BufferOutcome<T> {
  const d: BufferDiagnostic = { code, message, hint };
  return { ok: false, code, message, hint, diagnostics: [...prior, d] };
}

/** 诊断聚合器。 */
export class BufferDiagBag {
  private readonly items: BufferDiagnostic[] = [];

  push(code: BufferDiagCode, message: string, hint: string): void {
    this.items.push({ code, message: message || "（未提供描述）", hint: hint || "（未提供处置建议）" });
  }

  absorb(other: BufferDiagBag | readonly BufferDiagnostic[]): void {
    const src = other instanceof BufferDiagBag ? other.all() : other;
    for (const d of src) this.items.push(d);
  }

  get size(): number {
    return this.items.length;
  }

  all(): readonly BufferDiagnostic[] {
    return this.items.slice();
  }

  byCode(code: BufferDiagCode): readonly BufferDiagnostic[] {
    return this.items.filter((d) => d.code === code);
  }

  /** 阻断级诊断计数：调用方据此决定是否拒绝整批命令。 */
  blockingCount(): number {
    const blocking: readonly BufferDiagCode[] = [
      "OUT_OF_BOUNDS",
      "REGION_OVERLAP",
      "ALIAS_UNDECLARED_USE",
      "HEAP_POLICY_MISMATCH",
      "PARENT_ALLOC_TOO_SMALL",
      "PROTOCOL_HASH_DRIFT",
      "GENERATION_STALE",
    ];
    return this.items.filter((d) => blocking.includes(d.code)).length;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 缓冲类别、用途与堆策略
// ════════════════════════════════════════════════════════════════════════════

/** 缓冲类别（三类走同一套管理，仅约束不同）。 */
export type BufferKind = "vertex" | "index" | "uniform";

export const BUFFER_KINDS: readonly BufferKind[] = ["vertex", "index", "uniform"];

/** 用途位。 */
export type BufferUsage = "vertex" | "index" | "uniform" | "indirect" | "storage";

export const BUFFER_USAGES: readonly BufferUsage[] = ["vertex", "index", "uniform", "indirect", "storage"];

/** 类别 → 允许的用途（统一管理下的唯一差异点之一）。 */
export const KIND_USAGE_MATRIX: Readonly<Record<BufferKind, readonly BufferUsage[]>> = {
  vertex: ["vertex"],
  index: ["index"],
  uniform: ["uniform", "storage", "indirect"],
};

/** 内存堆类型。 */
export type HeapKind = "upload" | "default";

/**
 * 堆选择理由（判据：堆策略含选择理由文档）。
 * 这不是「随手写的注释」——每个档都写明代价，因为选错堆的代价是隐性的
 * （上传堆帧率好但带宽被独占；默认堆带宽省但每帧同步会拖管线）。
 */
export interface HeapChoiceRule {
  readonly usage: BufferUsage;
  /** 推荐堆。 */
  readonly prefer: HeapKind;
  /** 推荐理由（面向工程师的决策依据）。 */
  readonly reason: string;
  /** 选错该堆的后果（显性写出，避免「以为无所谓」）。 */
  readonly penalty: string;
  /** 每帧重建的量超过此字节数时，upload 堆不再有优势。 */
  readonly uploadHeapThresholdBytes: number;
}

export const HEAP_CHOICE_RULES: readonly HeapChoiceRule[] = [
  {
    usage: "vertex",
    prefer: "upload",
    reason:
      "顶点数据每帧重传。upload 堆走独立的拷贝引擎，与渲染引擎并行，" +
      "顶点重传不与绘制抢带宽",
    penalty:
      "放default 堆则每帧顶点上传与绘制共享带宽，顶点多的场景直接掉帧；" +
      "且会引入管线停顿（等顶点就绪才能起绘制）",
    uploadHeapThresholdBytes: 64 * 1024,
  },
  {
    usage: "index",
    prefer: "upload",
    reason: "索引数据与顶点同生命周期、同重传频率，策略与顶点一致",
    penalty: "同顶点：共享带宽导致掉帧，且索引就绪晚于顶点会拖住首像素",
    uploadHeapThresholdBytes: 32 * 1024,
  },
  {
    usage: "uniform",
    prefer: "default",
    reason:
      "统一缓冲的数据量小、重传量小。若也走 upload 堆，独占拷贝引擎的" +
      "带宽却没有与之匹配的吞吐需求，反而拖累顶点上传",
    penalty:
      "小体积 uniform 放 upload 挤占顶点上传带宽；若 uniform 是一次性大块" +
      "（> 64KB）则本策略应翻转，见 planHeap",
    uploadHeapThresholdBytes: 64 * 1024,
  },
  {
    usage: "indirect",
    prefer: "default",
    reason:
      "indirect 缓冲被 GPU 读取，其内容由本帧命令写入。放 upload 堆会引入" +
      "上传→回读的隐式同步，抵消并行收益",
    penalty: "upload 堆的隐式同步会让命令流串行化，间接绘制吞吐显著下降",
    uploadHeapThresholdBytes: 0,
  },
  {
    usage: "storage",
    prefer: "default",
    reason: "storage 缓冲通常持久驻留且体积大，upload 堆容量有限且带宽独占不划算",
    penalty: "大块 storage 上 upload 会挤占带宽并可能撑爆 upload 堆容量",
    uploadHeapThresholdBytes: 0,
  },
];

/** 取某用途的堆策略规则。 */
export function heapRuleFor(usage: BufferUsage): HeapChoiceRule | null {
  return HEAP_CHOICE_RULES.find((r) => r.usage === usage) ?? null;
}

/** 一次堆选择的结果。 */
export interface HeapChoice {
  readonly heap: HeapKind;
  readonly reason: string;
  readonly penalty: string;
  /** 是否偏离了默认推荐（偏离需显性说明）。 */
  readonly overridden: boolean;
  /** 若偏离，给出纠正建议（判据：策略不匹配须附建议）。 */
  readonly correction: string | null;
}

/**
 * 选择堆（纯函数）。
 * 规则：按用途取默认推荐；仅当「每帧上传字节数 > 该用途的 upload 堆阈值」
 * 时翻转 uniform 的推荐（一次性大块 uniform 走 upload 才划算）。
 * 传入的 heap 若与推荐不符 → 显性返回 overridden +纠正建议，不静默改写。
 */
export function planHeap(usage: BufferUsage, bytesPerFrame: number, requested: HeapKind | null): BufferOutcome<HeapChoice> {
  const rule = heapRuleFor(usage);
  if (rule === null) {
    return bufFail("USAGE_INCOMPATIBLE", `用途 ${String(usage)} 无堆策略规则`, "在 HEAP_CHOICE_RULES 中登记该用途");
  }
  if (!Number.isFinite(bytesPerFrame) || bytesPerFrame < 0) {
    return bufFail("UPLOAD_BYTE_LIMIT_INVALID", `每帧字节数 ${String(bytesPerFrame)} 非法`, "须为非负有限数");
  }

  // 大块翻转：超过阈值后 upload 堆的带宽独占才划算
  let preferred = rule.prefer;
  let flipReason = rule.reason;
  if (usage === "uniform" && bytesPerFrame > rule.uploadHeapThresholdBytes) {
    preferred = "upload";
    flipReason =
      `uniform 每帧 ${bytesPerFrame} 字节超过 upload 堆阈值 ${rule.uploadHeapThresholdBytes}：` +
      "此体积下独占拷贝引擎带宽才划算，小体积默认值不再适用";
  }

  if (requested === null) {
    return bufOk({ heap: preferred, reason: flipReason, penalty: rule.penalty, overridden: false, correction: null });
  }
  if (requested === preferred) {
    return bufOk({ heap: preferred, reason: flipReason, penalty: rule.penalty, overridden: false, correction: null });
  }
  return bufOk(
    {
      heap: preferred,
      reason: flipReason,
      penalty: rule.penalty,
      overridden: true,
      correction:
        `调用方指定 ${requested} 堆，推荐 ${preferred}：${rule.penalty}` +
        (requested === "upload" && preferred === "default"
          ? "。若确有跨帧复用需求请显式声明，不要靠换堆绕过"
          : "。请确认后修改，或补一条堆策略规则说明例外理由"),
    },
    [
      {
        code: "HEAP_POLICY_MISMATCH",
        message: `用途 ${usage} 指定 ${requested} 堆，与推荐 ${preferred} 不符`,
        hint: `选择理由：${flipReason}；违背代价：${rule.penalty}`,
      },
    ],
  );
}

// ════════════════════════════════════════════════════════════════════════════
// §3 对齐与尺寸校验
// ════════════════════════════════════════════════════════════════════════════

/** 三类缓冲的对齐下限（统一管理下的另一处差异点）。 */
export const KIND_ALIGNMENT: Readonly<Record<BufferKind, number>> = {
  vertex: 4,
  index: 4,
  uniform: 256,
};

/** 统一缓冲按 256 对齐是硬件常量缓冲区的粒度。 */
export const UNIFORM_ALIGNMENT: number = 256;

/** 向上对齐到 alignment 的倍数。 */
export function alignUp(value: number, alignment: number): number {
  if (!Number.isInteger(value) || value < 0) return 0;
  if (!Number.isInteger(alignment) || alignment <= 0) return value;
  return Math.ceil(value / alignment) * alignment;
}

/** 索引缓冲的合法字节宽度（顶点数索引）。 */
export const INDEX_WIDTHS: readonly number[] = [2, 4];

/** 缓冲描述。 */
export interface BufferDesc {
  readonly kind: BufferKind;
  /** 请求字节数（未对齐；由分配器双向对齐）。 */
  readonly sizeBytes: number;
  readonly usages: readonly BufferUsage[];
  /** 索引缓冲的索引宽度；非索引为0。 */
  readonly indexWidthBytes: number;
  /** 是否每帧重建（决定 heap 与节流档）。 */
  readonly perFrame: boolean;
  /** 是否允许别名（显式声明，非声明则禁止别名使用）。 */
  readonly aliasingAllowed: boolean;
}

/** 描述校验。 */
export function validateDesc(desc: BufferDesc): BufferOutcome<BufferDesc> {
  const bag = new BufferDiagBag();

  if (!BUFFER_KINDS.includes(desc.kind)) {
    return bufFail("USAGE_INCOMPATIBLE", `未知缓冲类别 ${String(desc.kind)}`, "类别必须来自 BUFFER_KINDS");
  }
  if (!Number.isInteger(desc.sizeBytes) || desc.sizeBytes <= 0) {
    return bufFail("SIZE_INVALID", `字节数 ${String(desc.sizeBytes)} 非正整数`, "缓冲字节数必须为正整数");
  }
  if (desc.usages.length === 0) {
    return bufFail("USAGE_EMPTY", "用途集合为空", "至少声明一项用途");
  }
  const allowed = KIND_USAGE_MATRIX[desc.kind];
  for (const u of desc.usages) {
    if (!BUFFER_USAGES.includes(u)) {
      return bufFail("USAGE_INCOMPATIBLE", `未知用途 ${String(u)}`, "用途必须来自 BUFFER_USAGES");
    }
    if (!allowed.includes(u)) {
      return bufFail(
        "USAGE_INCOMPATIBLE",
        `${desc.kind} 缓冲不接受用途 ${u}`,
        `${desc.kind} 缓冲只接受：${allowed.join(" / ")}`,
      );
    }
  }
  if (desc.kind === "index") {
    if (!INDEX_WIDTHS.includes(desc.indexWidthBytes)) {
      return bufFail(
        "INDEX_FORMAT_UNKNOWN",
        `索引宽度 ${String(desc.indexWidthBytes)} 非法`,
        `索引宽度只允许 ${INDEX_WIDTHS.join(" / ")} 字节（16bit / 32bit 索引）`,
      );
    }
  } else if (desc.indexWidthBytes !== 0) {
    return bufFail(
      "INDEX_FORMAT_UNKNOWN",
      `${desc.kind} 缓冲不应声明索引宽度 ${desc.indexWidthBytes}`,
      "索引宽度仅索引缓冲可用，非索引缓冲填 0",
    );
  }

  return bufOk(desc, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §4 子分配器（判据：子分配 · 碎片整理）
// ════════════════════════════════════════════════════════════════════════════

/** 一个子分配区（半开区间 [offset, offset+size)）。 */
export interface Region {
  readonly regionId: string;
  readonly offset: number;
  readonly size: number;
  readonly kind: BufferKind;
  readonly alignment: number;
  /** 代戳（与 F0008 同源；区域被回收后复用同名时抬高）。 */
  readonly serial: number;
  /** 是否仍被占用。 */
  readonly live: boolean;
}

/** 空闲区（半开区间）。 */
export interface FreeSpan {
  readonly offset: number;
  readonly size: number;
}

/** 碎片统计。 */
export interface FragmentationStats {
  readonly parentBytes: number;
  readonly usedBytes: number;
  readonly freeBytes: number;
  /** 碎片率 = 空闲字节 / 父分配字节。 */
  readonly fragmentationRatio: number;
  readonly liveRegions: number;
  readonly freeSpans: number;
  /** 最大连续空闲区；碎片整理的核心指标。 */
  readonly largestFreeSpan: number;
}

/** 整理阈值（判据：碎片整理阈值）。 */
export const COMPACTION_THRESHOLD: number = 0.25;

/** 子分配失败的原因（供上层选择降级路径）。 */
export type AllocFailureReason =
  | "no-space"
  | "fragmented"
  | "size-too-large";

/** 一次搬迁（整理规划的一条）。 */
export interface CompactionMove {
  readonly regionId: string;
  readonly fromOffset: number;
  readonly toOffset: number;
  readonly kind: BufferKind;
  readonly reason: string;
}

/** 碎片整理的规划结果（本条只规划，不搬数据）。 */
export interface CompactionPlan {
  /** 需搬迁的活跃区及目标偏移。 */
  readonly moves: readonly CompactionMove[];
  readonly requiresMigration: readonly string[];
  /** 整理前的碎片形态。 */
  readonly before: FragmentationStats;
  /** 搬迁落实后的空闲区形态（空洞集中到尾部）。 */
  readonly afterSpans: readonly FreeSpan[];
  readonly afterFreeSpans: number;
  readonly trailingFreeBytes: number;
  readonly alignedBase: number;
}

/** 子分配器台账。 */
export class SubAllocator {
  private readonly regions = new Map<string, Region>();
  private free: FreeSpan[] = [];

  constructor(readonly parentBytes: number) {
    this.free = parentBytes > 0 ? [{ offset: 0, size: parentBytes }] : [];
  }

  /** 空闲区按地址升序（确定性：切分顺序必须可复算）。 */
  private sortedFree(): FreeSpan[] {
    return this.free.slice().sort((a, b) => a.offset - b.offset);
  }

  /**
   * 分配一个子区（首次适配，地址升序）。
   *
   * 对齐纪律（立场一）：offset 与 size **双向对齐**，台账只记对齐后的值。
   * 若只对齐 size 而不对齐 offset，后续合并空洞的对齐前提不成立。
   */
  allocate(
    regionId: string,
    kind: BufferKind,
    sizeBytes: number,
    alignment: number,
  ): BufferOutcome<Region> {
    const bag = new BufferDiagBag();
    if (this.regions.has(regionId)) {
      return bufFail(
        "REGION_OVERLAP",
        `区域 ${regionId} 已存在`,
        "区域 id 必须唯一；同名复用请走 free 后重新 allocate（代戳会自动抬高）",
      );
    }
    const alignedSize = alignUp(sizeBytes, alignment);
    if (alignedSize === 0) {
      return bufFail("ALIGNMENT_INVALID", `对齐 ${String(alignment)} 或字节数 ${String(sizeBytes)} 非法`, "对齐须为正整数");
    }
    if (alignedSize > this.parentBytes) {
      return bufFail(
        "PARENT_ALLOC_TOO_SMALL",
        `请求 ${alignedSize} 字节（对齐后）超过父分配 ${this.parentBytes} 字节`,
        "减小缓冲，或向上申请更大的父分配",
      );
    }

    const spans = this.sortedFree();
    for (let i = 0; i < spans.length; i += 1) {
      const span = spans[i];
      if (span === undefined) continue;
      const alignedOffset = alignUp(span.offset, alignment);
      // 对齐后仍需容纳：对齐可能吃掉空洞头部
      if (alignedOffset + alignedSize > span.offset + span.size) continue;

      const head = alignedOffset - span.offset;
      const tail = span.offset + span.size - (alignedOffset + alignedSize);
      const rest: FreeSpan[] = [];
      if (head > 0) rest.push({ offset: span.offset, size: head });
      if (tail > 0) rest.push({ offset: alignedOffset + alignedSize, size: tail });
      const kept = spans.filter((_, k) => k !== i);
      this.free = mergeSpans(kept.concat(rest));

      const region: Region = {
        regionId,
        offset: alignedOffset,
        size: alignedSize,
        kind,
        alignment,
        serial: 0,
        live: true,
      };
      this.regions.set(regionId, region);
      return bufOk(region, bag.all());
    }

    // 无可容纳区间：区分「确实没空间」与「空间在但碎了」
    const largest = this.free.reduce((m, s) => Math.max(m, s.size), 0);
    const reason: AllocFailureReason =
      largest < alignedSize ? "no-space" : alignedSize <= this.parentBytes ? "fragmented" : "size-too-large";
    return bufFail(
      reason === "fragmented" ? "FRAGMENTATION_EXCEEDED" : "PARENT_ALLOC_TOO_SMALL",
      reason === "fragmented"
        ? `总空闲 ${this.free.reduce((a, s) => a + s.size, 0)} 字节但无单块 ${alignedSize} 字节可用（最大空洞 ${largest}）`
        : `无可用空间：需 ${alignedSize} 字节，最大空洞 ${largest} 字节`,
      reason === "fragmented"
        ? "触发碎片整理（整理会重建空闲链表；若需搬迁数据见 COMPACTION_REQUIRES_MIGRATION）"
        : "减小缓冲，或向上申请更大的父分配",
      bag.all(),
    );
  }

  /**
   * 释放一个子区。空闲区立即合并——合并不需要搬数据（立场二：重建空闲链表）。
   */
  free_region(regionId: string): BufferOutcome<FreeSpan> {
    const region = this.regions.get(regionId);
    if (region === undefined) {
      return bufFail("ILLEGAL_TRANSITION", `区域 ${regionId} 未分配`, "先 allocate 再 free");
    }
    if (!region.live) {
      return bufFail("ILLEGAL_TRANSITION", `区域 ${regionId} 已释放`, "重复释放是调用方缺陷（会导致显存账虚增）");
    }
    this.regions.set(regionId, { ...region, live: false, serial: region.serial + 1 });
    this.free = mergeSpans(this.free.concat([{ offset: region.offset, size: region.size }]));
    const span = this.free.filter((s) => s.offset === region.offset).sort((a, b) => b.size - a.size)[0];
    return bufOk(span ?? { offset: region.offset, size: region.size });
  }

  /** 取区域。 */
  get(regionId: string): Region | null {
    return this.regions.get(regionId) ?? null;
  }

  /** 全部活跃区域（按 offset 升序）。 */
  liveRegions(): readonly Region[] {
    return Array.from(this.regions.values())
      .filter((r) => r.live)
      .sort((a, b) => a.offset - b.offset);
  }

  /** 当前空闲区（合并后、地址升序）。 */
  freeSpans(): readonly FreeSpan[] {
    return this.sortedFree();
  }

  /** 碎片统计。 */
  stats(): FragmentationStats {
    const live = this.liveRegions();
    const used = live.reduce((a, r) => a + r.size, 0);
    const freeTotal = this.parentBytes - used;
    const spans = this.sortedFree();
    const largest = spans.reduce((m, s) => Math.max(m, s.size), 0);
    return {
      parentBytes: this.parentBytes,
      usedBytes: used,
      freeBytes: freeTotal,
      fragmentationRatio: this.parentBytes === 0 ? 0 : freeTotal / this.parentBytes,
      liveRegions: live.length,
      freeSpans: spans.length,
      largestFreeSpan: largest,
    };
  }

  /** 碎片率是否超阈值。 */
  needsCompaction(threshold: number = COMPACTION_THRESHOLD): boolean {
    return this.stats().fragmentationRatio > threshold;
  }

  /**
   * 碎片整理（判据：碎片整理 · 立场二：**规划**紧凑重排，不搬数据）。
   *
   * 关键认知（曾被探针纠正的错误设计）：本操作**只重排空闲账是不够的**。
   * 空闲区在 free_region 时已就地合并；若活跃区不搬迁，空洞永远被活跃区
   * 隔开，「重建空闲链表」得到的形态与整理前完全一致——那是空操作。
   * 真正的整理必须**把活跃区向低地址紧凑排列**，使空洞集中成尾部一段。
   *
   * 但搬数据不可行：GPU 缓冲无法原地搬。故本条做的是**搬迁规划**——
   * 计算每个活跃区的目标偏移、需搬迁的清单与搬迁后的空洞形态，
   * 由上层换新父分配 + 重传来落实。本条只保证规划自洽且可验。
   */
  compact(): BufferOutcome<CompactionPlan> {
    const live = this.liveRegions();
    const alignBase = live.length > 0 ? (live[0]?.alignment ?? 1) : 1;

    // 目标偏移：从 0 起，逐块按各自对齐向上取，紧凑排列
    const moves: CompactionMove[] = [];
    let cursor = 0;
    for (const r of live) {
      const target = alignUp(cursor, r.alignment);
      if (target !== r.offset) {
        moves.push({
          regionId: r.regionId,
          fromOffset: r.offset,
          toOffset: target,
          kind: r.kind,
          reason:
            target > r.offset
              ? `前序块紧凑后本格需右移 ${target - r.offset} 字节（对齐到${r.alignment}）`
              : `本格可左移 ${r.offset - target} 字节`,
        });
      }
      cursor = target + r.size;
    }

    // 搬迁后的空洞形态：所有空洞集中到尾部，段数为1（无空洞时为 0）
    const trailingFree = this.parentBytes - cursor;
    const afterSpans: readonly FreeSpan[] =
      trailingFree > 0 ? [{ offset: cursor, size: trailingFree }] : [];

    const bag = new BufferDiagBag();
    if (moves.length > 0) {
      bag.push(
        "COMPACTION_REQUIRES_MIGRATION",
        `整理规划：${moves.length} 个活跃区需搬迁，紧凑后空洞集中为尾部 ${trailingFree} 字节一段` +
          `（整理前为 ${this.stats().freeSpans} 段、最大连续空洞 ${this.stats().largestFreeSpan} 字节）`,
        "本条只做规划不搬数据（GPU 缓冲不可原地搬）；上层按 toOffset 换新父分配 + 重传即可落实紧凑形态",
      );
    }

    return bufOk(
      {
        moves,
        requiresMigration: moves.map((m) => m.regionId),
        before: this.stats(),
        afterSpans,
        afterFreeSpans: afterSpans.length,
        trailingFreeBytes: trailingFree,
        alignedBase: alignBase,
      },
      bag.all(),
    );
  }
}

/** 合并相邻或重叠的空闲区（升序输入，输出升序且互不重叠）。 */
export function mergeSpans(spans: readonly FreeSpan[]): FreeSpan[] {
  const sorted = spans
    .filter((s) => s.size > 0)
    .slice()
    .sort((a, b) => a.offset - b.offset);
  const out: FreeSpan[] = [];
  for (const s of sorted) {
    const last = out[out.length - 1];
    if (last !== undefined && s.offset <= last.offset + last.size) {
      const end = Math.max(last.offset + last.size, s.offset + s.size);
      out[out.length - 1] = { offset: last.offset, size: end - last.offset };
      continue;
    }
    out.push({ offset: s.offset, size: s.size });
  }
  return out;
}

// ════════════════════════════════════════════════════════════════════════════
// §5 越界写入阻断（判据：越界阻断 · 立场五）
// ════════════════════════════════════════════════════════════════════════════

/** 一次写入请求。 */
export interface WriteRequest {
  readonly regionId: string;
  /** 相对本块起点的偏移。 */
  readonly relativeOffset: number;
  readonly byteLength: number;
  /** 持有者所持代戳（防旧代区域写入）。 */
  readonly serial: number;
}

/** 写入校验结果。 */
export interface WriteCheck {
  readonly ok: boolean;
  /** 校验后的绝对偏移（仅 ok 时有效）。 */
  readonly absoluteOffset: number;
  readonly diagnostic: BufferDiagnostic | null;
}

/**
 * 写入边界校验（半开区间 [offset, offset+size)）。
 * 越界 1 字节也拦；**绝不钳制后放行**（立场五）。
 */
export function checkWrite(region: Region, req: WriteRequest): WriteCheck {
  const reject = (message: string, hint: string): WriteCheck => ({
    ok: false,
    absoluteOffset: 0,
    diagnostic: { code: "OUT_OF_BOUNDS", message, hint },
  });

  if (!region.live) {
    return reject(
      `写入已释放区域 ${region.regionId}（代戳 ${region.serial}）`,
      "区域已回收；重新 allocate 后用新代戳写入",
    );
  }
  if (req.serial !== region.serial) {
    return {
      ok: false,
      absoluteOffset: 0,
      diagnostic: {
        code: "GENERATION_STALE",
        message: `代戳不符：持有 ${req.serial}，台账 ${region.serial}`,
        hint: "旧代区域句柄自动失效；重新从台账取句柄",
      },
    };
  }
  if (!Number.isInteger(req.relativeOffset) || req.relativeOffset < 0) {
    return reject(
      `相对偏移 ${String(req.relativeOffset)} 非法`,
      "相对偏移必须是非负整数",
    );
  }
  if (!Number.isInteger(req.byteLength) || req.byteLength <= 0) {
    return reject(`写入字节数 ${String(req.byteLength)} 非正整数`, "写入长度必须为正整数");
  }
  // 半开区间右端：relativeOffset + byteLength <= size
  const end = req.relativeOffset + req.byteLength;
  if (end > region.size) {
    return reject(
      `写入越界：偏移 ${req.relativeOffset} + 长度 ${req.byteLength} = ${end}，超出区域大小 ${region.size}`,
      "越界一律拒绝（不钳制）——钳制会把越界缺陷变成静默数据错误；请按真实需求重新 allocate",
    );
  }
  return { ok: true, absoluteOffset: region.offset + req.relativeOffset, diagnostic: null };
}

// ════════════════════════════════════════════════════════════════════════════
// §6 碎片整理的暂停机制（判据：暂停整理）
// ════════════════════════════════════════════════════════════════════════════

/** 整理许可信号（立场三：按请求来源而非时间判定）。 */
export interface CompactionGate {
  /** 本帧预算余量比例（0~1）；余量低于 minHeadroom 则不允许整理。 */
  readonly frameBudgetHeadroom: number;
  /** 用户是否正在交互（拖拽/缩放）。 */
  readonly interacting: boolean;
  /** 整理所需的最小预算余量。 */
  readonly minHeadroom: number;
}

/** 整理裁决。 */
export type CompactionDecision =
  | { readonly proceed: true; readonly reason: string }
  | { readonly proceed: false; readonly reason: string; readonly diagnostic: BufferDiagnostic };

/**
 * 整理许可裁决。
 * 暂停是**延后**不是放弃：高峰过后同一信号会放行，整理队列原样保留。
 */
export function decideCompaction(gate: CompactionGate): CompactionDecision {
  if (!Number.isFinite(gate.frameBudgetHeadroom) || gate.frameBudgetHeadroom < 0) {
    return {
      proceed: false,
      reason: "帧预算余量非法",
      diagnostic: {
        code: "COMPACTION_DEFERRED",
        message: `帧预算余量 ${String(gate.frameBudgetHeadroom)} 非法`,
        hint: "传入 0~1 的余量比例",
      },
    };
  }
  if (gate.interacting) {
    return {
      proceed: false,
      reason: "交互高峰：用户正在拖拽/缩放，整理延后到交互结束",
      diagnostic: {
        code: "COMPACTION_DEFERRED",
        message: "交互高峰暂停碎片整理",
        hint: "整理本身耗时（重建空闲账 + 潜在搬迁）；交互期做整理会放大已存在的卡顿",
      },
    };
  }
  if (gate.frameBudgetHeadroom < gate.minHeadroom) {
    return {
      proceed: false,
      reason:
        `帧预算余量 ${gate.frameBudgetHeadroom.toFixed(3)} 低于整理所需 ${gate.minHeadroom}，整理延后`,
      diagnostic: {
        code: "COMPACTION_DEFERRED",
        message: "帧预算紧张，整理延后",
        hint: "整理要占 CPU 与可能的重传带宽；余量不足时先保帧率",
      },
    };
  }
  return { proceed: true, reason: `余量 ${gate.frameBudgetHeadroom.toFixed(3)} ≥ ${gate.minHeadroom} 且无交互，允许整理` };
}

// ════════════════════════════════════════════════════════════════════════════
// §7 上传节流（判据：上传节流：每帧上传字节数上限防卡帧）
// ════════════════════════════════════════════════════════════════════════════

/** 节流参数。 */
export interface ThrottleConfig {
  /** 每帧上传字节上限。 */
  readonly maxBytesPerFrame: number;
  /** 标记为「屏上关键」的上传（优先占额度）。 */
  readonly criticalBytesPerFrame: number;
}

/** 节流参数有效性校验（配置错会导致节流形同虚设）。 */
export function validateThrottle(cfg: ThrottleConfig): BufferOutcome<ThrottleConfig> {
  if (!Number.isInteger(cfg.maxBytesPerFrame) || cfg.maxBytesPerFrame <= 0) {
    return bufFail(
      "UPLOAD_BYTE_LIMIT_INVALID",
      `每帧上传上限 ${String(cfg.maxBytesPerFrame)} 非正整数`,
      "上限必须为正整数（上限为 0 等于禁上传，非 0 无意义）",
    );
  }
  if (!Number.isInteger(cfg.criticalBytesPerFrame) || cfg.criticalBytesPerFrame <= 0) {
    return bufFail(
      "UPLOAD_BYTE_LIMIT_INVALID",
      `关键上传上限 ${String(cfg.criticalBytesPerFrame)} 非正整数`,
      "关键上传上限必须为正整数",
    );
  }
  if (cfg.criticalBytesPerFrame > cfg.maxBytesPerFrame) {
    return bufFail(
      "UPLOAD_BYTE_LIMIT_INVALID",
      `关键上传上限 ${cfg.criticalBytesPerFrame} 超过总上限 ${cfg.maxBytesPerFrame}`,
      "关键档是总档的子集：屏上关键上传不能突破帧预算总上限",
    );
  }
  return bufOk(cfg);
}

/** 一个待上传项。 */
export interface UploadItem {
  readonly itemId: string;
  readonly regionId: string;
  readonly byteLength: number;
  /** 是否屏上关键。 */
  readonly critical: boolean;
}

/** 一帧的节流计划。 */
export interface ThrottlePlan {
  readonly issued: readonly UploadItem[];
  readonly issuedBytes: number;
  readonly deferred: readonly UploadItem[];
  readonly diagnostics: readonly BufferDiagnostic[];
}

/**
 * 一帧节流计划。
 * 纪律（与 F0016 同源）：超限**顺延**不截断——截断会让上传项处于
 * 「发了半截」而无法判定完成。关键项优先占额度，但绝不突破总上限。
 */
export function planThrottle(
  items: readonly UploadItem[],
  cfg: ThrottleConfig,
): BufferOutcome<ThrottlePlan> {
  const v = validateThrottle(cfg);
  if (!v.ok) return { ...v, diagnostics: v.diagnostics } as BufferOutcome<ThrottlePlan>;

  const bag = new BufferDiagBag();
  // 关键优先；同级按itemId 稳定排序（保证同输入同输出）
  const sorted = items
    .slice()
    .sort((a, b) => (a.critical === b.critical ? 0 : a.critical ? -1 : 1))
    .sort((a, b) => (a.critical === b.critical ? (a.itemId < b.itemId ? -1 : 1) : a.critical ? -1 : 1));

  const issued: UploadItem[] = [];
  const deferred: UploadItem[] = [];
  let spent = 0;
  let criticalSpent = 0;

  for (const it of sorted) {
    const cap = it.critical ? cfg.criticalBytesPerFrame : cfg.maxBytesPerFrame;
    const capSpent = it.critical ? criticalSpent : spent;
    if (it.byteLength > cap) {
      // 单项就超自己的档位上限：本帧不可发（它是单项超限，不是被挤掉）
      deferred.push(it);
      bag.push(
        "UPLOAD_THROTTLED",
        `上传项 ${it.itemId} 字节 ${it.byteLength} 超过${it.critical ? "关键" : "普通"}档上限 ${cap}`,
        "单项超限只能顺延——拆分该缓冲或上调上限；不截断（半截无法判定完成）",
      );
      continue;
    }
    if (capSpent + it.byteLength > cap) {
      deferred.push(it);
      continue;
    }
    issued.push(it);
    if (it.critical) criticalSpent += it.byteLength;
    spent += it.byteLength;
  }

  if (deferred.length > 0) {
    bag.push(
      "UPLOAD_THROTTLED",
      `本帧顺延 ${deferred.length} 项（发出 ${issued.length} 项 / ${spent} 字节，上限 ${cfg.maxBytesPerFrame}）`,
      "顺延项保留在队列，下帧优先；持续顺延说明每帧重建量超出上限，需降低重建频率",
    );
  }

  return bufOk({ issued, issuedBytes: spent, deferred, diagnostics: bag.all() }, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §8 别名安全与验证器（判据：别名声明 · 验证器）
// ════════════════════════════════════════════════════════════════════════════

/** 别名声明。 */
export interface AliasDeclaration {
  /** 别名（同两处使用的名字）。 */
  readonly alias: string;
  readonly regionId: string;
  /** 声明理由（同一份数据被两处共享的理由，必填——空理由的声明是虚报）。 */
  readonly reason: string;
  /** 声明方（材质/UI/工具链），归因用。 */
  readonly declaredBy: string;
}

/** 别名台账：声明与实际使用的唯一真值。 */
export class AliasLedger {
  private readonly declarations = new Map<string, AliasDeclaration>();
  /** alias → 实际使用次数。 */
  private readonly uses = new Map<string, number>();

  /** 声明别名。空理由拒绝（这是「虚报」的第一道闸）。 */
  declare(d: AliasDeclaration): BufferOutcome<AliasDeclaration> {
    if (d.reason.trim().length === 0) {
      return bufFail(
        "ALIAS_OVERDECLARED",
        `别名「${d.alias}」的声明理由为空`,
        "别名共享同一份显存，必须写明为何两处共用；无理由的声明按虚报处理",
      );
    }
    const existing = this.declarations.get(d.alias);
    if (existing !== undefined) {
      if (existing.regionId === d.regionId) return bufOk(existing);
      return bufFail(
        "ALIAS_CONFLICT",
        `别名「${d.alias}」已声明给 ${existing.regionId}（${existing.declaredBy}），不能改指 ${d.regionId}`,
        "别名即契约，禁止静默改指；先撤销旧声明",
      );
    }
    this.declarations.set(d.alias, d);
    return bufOk(d);
  }

  /** 撤销声明。 */
  revoke(alias: string): BufferOutcome<void> {
    const d = this.declarations.get(alias);
    if (d === undefined) {
      return bufFail("ALIAS_OVERDECLARED", `别名「${alias}」未声明`, "先查 declarationsOf 确认当前声明");
    }
    this.declarations.delete(alias);
    this.uses.delete(alias);
    return bufOk(undefined);
  }

  /**
   * 记录一次实际使用（判据：验证器的一致性校验的数据来源）。
   * 未声明就用 → **阻断级**拒绝（立场四）。
   */
  noteUse(alias: string): BufferOutcome<void> {
    const d = this.declarations.get(alias);
    if (d === undefined) {
      return bufFail(
        "ALIAS_UNDECLARED_USE",
        `别名「${alias}」被使用但未声明`,
        "别名使用必须显式声明；未声明的使用是违例（阻断级）——先 declare 再用",
      );
    }
    this.uses.set(alias, (this.uses.get(alias) ?? 0) + 1);
    return bufOk(undefined);
  }

  declarationsOf(regionId: string): readonly AliasDeclaration[] {
    return Array.from(this.declarations.values()).filter((d) => d.regionId === regionId);
  }

  usesOf(alias: string): number {
    return this.uses.get(alias) ?? 0;
  }

  /**
   * 全部已声明别名（按 alias 升序）。
   * 验证器需要「声明侧全集」——declarationsOf 按regionId 过滤，
   * 而验证器手里只有 alias，不知道 regionId，故必须有一个全量视图。
   */
  allDeclarations(): readonly AliasDeclaration[] {
    return Array.from(this.declarations.values()).sort((a, b) => (a.alias < b.alias ? -1 : 1));
  }

  hasDeclaration(alias: string): boolean {
    return this.declarations.has(alias);
  }

  get size(): number {
    return this.declarations.size;
  }
}

/** 验证器的一条发现。 */
export interface AliasVerification {
  readonly alias: string;
  readonly kind: "undeclared-use" | "overdeclared" | "consistent";
  readonly detail: string;
}

/** 一致性校验结果。 */
export interface AliasVerificationReport {
  readonly findings: readonly AliasVerification[];
  readonly violations: number;
  readonly overdeclared: number;
  readonly diagnostics: readonly BufferDiagnostic[];
}

/**
 * 别名一致性验证（判据：验证器：声明与实际使用一致性校验）。
 * 双向核对，两个方向处置相反（立场四）：
 *   - 使用了未声明 → undeclared-use，**阻断级**；
 *   - 声明了未使用 → overdeclared，**审计级**（非阻断：可能只是尚未提交绘制）。
 */
export function verifyAliases(
  ledger: AliasLedger,
  observedUses: ReadonlyMap<string, number>,
): BufferOutcome<AliasVerificationReport> {
  const bag = new BufferDiagBag();
  const findings: AliasVerification[] = [];
  const declared = ledger.allDeclarations();

  // 方向一：用了但未声明 —— 阻断级（立场四）。
  // 判据来自台账侧：noteUse 对未声明别名的调用已被拒绝，故能出现在
  // observedUses 里却不在声明全集里的，就是绕过声明直接用的违例。
  for (const [alias, count] of Array.from(observedUses.entries()).sort((a, b) => (a[0] < b[0] ? -1 : 1))) {
    if (count <= 0) continue;
    if (ledger.hasDeclaration(alias)) {
      findings.push({ alias, kind: "consistent", detail: `已声明，使用 ${count} 次` });
      continue;
    }
    findings.push({
      alias,
      kind: "undeclared-use",
      detail: `被使用 ${count} 次但无声明：违例`,
    });
    bag.push(
      "ALIAS_UNDECLARED_USE",
      `别名「${alias}」使用 ${count} 次但未声明`,
      "阻断级：补 declare 或改用独立分配——别名共享同一份显存，未声明的共享会让回收时机错判",
    );
  }

  // 方向二：声明了但未使用 —— 审计级（非阻断：可能只是尚未提交绘制）。
  for (const d of declared) {
    const count = observedUses.get(d.alias) ?? 0;
    if (count > 0) continue;
    findings.push({
      alias: d.alias,
      kind: "overdeclared",
      detail: `已声明给 ${d.regionId}（${d.declaredBy}）但实际使用 0 次：虚报`,
    });
    bag.push(
      "ALIAS_OVERDECLARED",
      `别名「${d.alias}」声明后未被使用（虚报）`,
      `声明理由写的是「${d.reason}」；若已不再共享请 revoke，若即将使用请忽略本项`,
    );
  }

  const violations = findings.filter((f) => f.kind === "undeclared-use").length;
  const overdeclared = findings.filter((f) => f.kind === "overdeclared").length;
  findings.sort((a, b) => (a.alias === b.alias ? (a.kind < b.kind ? -1 : 1) : a.alias < b.alias ? -1 : 1));
  return bufOk({ findings, violations, overdeclared, diagnostics: bag.all() }, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §9 统一台账（判据：统一管理）
// ════════════════════════════════════════════════════════════════════════════

/** 生命周期状态。 */
export type BufferPhase = "declared" | "allocated" | "active" | "retired";

/** 生命周期事件。 */
export type BufferEvent = "allocate" | "activate" | "retire";

export const BUFFER_EVENTS: readonly BufferEvent[] = ["allocate", "activate", "retire"];

/** 显式迁移表。 */
export const BUFFER_TRANSITIONS: Readonly<
  Record<BufferEvent, { readonly from: readonly BufferPhase[]; readonly to: BufferPhase }>
> = {
  allocate: { from: ["declared"], to: "allocated" },
  activate: { from: ["allocated"], to: "active" },
  retire: { from: ["declared", "allocated", "active"], to: "retired" },
};

export interface BufferStamp {
  readonly serial: number;
  readonly phase: BufferPhase;
}

/** 状态迁移（纯函数）。 */
export function applyBufferTransition(
  stamp: BufferStamp,
  event: BufferEvent,
): BufferOutcome<BufferStamp> {
  const rule = BUFFER_TRANSITIONS[event];
  if (!rule.from.includes(stamp.phase)) {
    return bufFail(
      "ILLEGAL_TRANSITION",
      `状态 ${stamp.phase} 不接受事件 ${event}`,
      `该状态可接受：${rule.from.join(" / ")}`,
    );
  }
  if (stamp.phase === "retired") {
    return bufFail(
      "BUFFER_RETIRED_REUSE",
      `退役缓冲（代戳 ${stamp.serial}）收到事件 ${event}`,
      "退役对象不可复用；同名重建须先 allocate（会抬高代戳）",
    );
  }
  const next = event === "retire" ? stamp.serial + 1 : stamp.serial;
  return bufOk({ serial: next, phase: rule.to });
}

/** 统一台账条目。 */
export interface BufferLedgerEntry {
  readonly bufferId: string;
  readonly desc: BufferDesc;
  readonly phase: BufferPhase;
  readonly serial: number;
  readonly heap: HeapKind;
  /** 在父分配内的区间（未分配时为 null）。 */
  readonly region: Region | null;
  readonly aliases: readonly string[];
  /** 每帧重建标记的派生视图（节流分档用）。 */
  readonly perFrame: boolean;
}

/** 统一台账（一个管理器管三类缓冲）。 */
export class BufferLedger {
  private readonly entries = new Map<string, BufferLedgerEntry>();
  private readonly allocators = new Map<BufferKind, SubAllocator>();

  constructor(parentBytesByKind: Readonly<Record<BufferKind, number>>) {
    for (const k of BUFFER_KINDS) {
      this.allocators.set(k, new SubAllocator(parentBytesByKind[k]));
    }
  }

  /** 声明一个缓冲（创建段）。 */
  declare(bufferId: string, desc: BufferDesc, heap: HeapKind): BufferOutcome<BufferLedgerEntry> {
    const v = validateDesc(desc);
    if (!v.ok) return { ...v, diagnostics: v.diagnostics } as BufferOutcome<BufferLedgerEntry>;
    if (this.entries.has(bufferId)) {
      return bufFail("ILLEGAL_TRANSITION", `缓冲 ${bufferId} 已存在`, "缓冲 id 唯一");
    }
    const entry: BufferLedgerEntry = {
      bufferId,
      desc,
      phase: "declared",
      serial: 0,
      heap,
      region: null,
      aliases: [],
      perFrame: desc.perFrame,
    };
    this.entries.set(bufferId, entry);
    return bufOk(entry);
  }

  /** 分配（上传段的前置）：在对应类别的子分配器里切一块。 */
  allocate(bufferId: string): BufferOutcome<BufferLedgerEntry> {
    const e = this.entries.get(bufferId);
    if (e === undefined) {
      return bufFail("ILLEGAL_TRANSITION", `缓冲 ${bufferId} 未声明`, "先 declare");
    }
    if (e.phase !== "declared") {
      return bufFail("ILLEGAL_TRANSITION", `缓冲 ${bufferId} 处于 ${e.phase}，不可分配`, "仅 declared 可 allocate");
    }
    const alloc = this.allocators.get(e.desc.kind);
    if (alloc === undefined) {
      return bufFail("USAGE_INCOMPATIBLE", `无 ${e.desc.kind} 子分配器`, "构造台账时须给三类都传容量");
    }
    const align = KIND_ALIGNMENT[e.desc.kind];
    const r = alloc.allocate(bufferId, e.desc.kind, e.desc.sizeBytes, align);
    if (!r.ok) return { ...r, diagnostics: r.diagnostics } as BufferOutcome<BufferLedgerEntry>;
    const next: BufferLedgerEntry = { ...e, phase: "allocated", region: r.value };
    this.entries.set(bufferId, next);
    return bufOk(next, r.diagnostics);
  }

  /** 激活（可被渲染通道使用）。 */
  activate(bufferId: string): BufferOutcome<BufferLedgerEntry> {
    const e = this.entries.get(bufferId);
    if (e === undefined) {
      return bufFail("ILLEGAL_TRANSITION", `缓冲 ${bufferId} 未声明`, "先 declare");
    }
    const t = applyBufferTransition({ serial: e.serial, phase: e.phase }, "activate");
    if (!t.ok) return { ...t, diagnostics: t.diagnostics } as BufferOutcome<BufferLedgerEntry>;
    const next: BufferLedgerEntry = { ...e, phase: t.value.phase, serial: t.value.serial };
    this.entries.set(bufferId, next);
    return bufOk(next);
  }

  /** 回收（回收段）：退子分配。 */
  retire(bufferId: string): BufferOutcome<BufferLedgerEntry> {
    const e = this.entries.get(bufferId);
    if (e === undefined) {
      return bufFail("ILLEGAL_TRANSITION", `缓冲 ${bufferId} 未声明`, "先 declare");
    }
    const t = applyBufferTransition({ serial: e.serial, phase: e.phase }, "retire");
    if (!t.ok) return { ...t, diagnostics: t.diagnostics } as BufferOutcome<BufferLedgerEntry>;
    const alloc = this.allocators.get(e.desc.kind);
    if (alloc !== undefined && e.region !== null) {
      alloc.free_region(bufferId);
    }
    const next: BufferLedgerEntry = { ...e, phase: t.value.phase, serial: t.value.serial, region: null };
    this.entries.set(bufferId, next);
    return bufOk(next);
  }

  get(bufferId: string): BufferLedgerEntry | null {
    return this.entries.get(bufferId) ?? null;
  }

  /** 某类别的碎片统计。 */
  stats(kind: BufferKind): FragmentationStats {
    const a = this.allocators.get(kind);
    return a === undefined
      ? { parentBytes: 0, usedBytes: 0, freeBytes: 0, fragmentationRatio: 0, liveRegions: 0, freeSpans: 0, largestFreeSpan: 0 }
      : a.stats();
  }

  /** 整理某类别（只出规划，不搬数据）。 */
  compact(kind: BufferKind): BufferOutcome<CompactionPlan> {
    const a = this.allocators.get(kind);
    if (a === undefined) {
      return bufFail("USAGE_INCOMPATIBLE", `无 ${kind} 子分配器`, "构造台账时须给三类都传容量");
    }
    return a.compact();
  }

  /** 全部条目（按 bufferId 升序）。 */
  all(): readonly BufferLedgerEntry[] {
    return Array.from(this.entries.values()).sort((a, b) => (a.bufferId < b.bufferId ? -1 : 1));
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §10 无障碍：缓冲面板读屏可达
// ════════════════════════════════════════════════════════════════════════════

/** 读屏状态形状（形状 + 文字双冗余，不依赖颜色）。 */
export type ReaderShape = "circle" | "square" | "triangle";

const PHASE_SHAPE: Readonly<Record<BufferPhase, ReaderShape>> = {
  declared: "circle",
  allocated: "circle",
  active: "square",
  retired: "circle",
};

const PHASE_SPOKEN: Readonly<Record<BufferPhase, string>> = {
  declared: "已声明，尚未分配",
  allocated: "已分配内存，数据未就绪",
  active: "已激活，可被渲染通道使用",
  retired: "已回收退役",
};

/** 读屏行。 */
export interface BufferReaderRow {
  readonly bufferId: string;
  readonly shape: ReaderShape;
  readonly spoken: string;
}

/** 类别人话译名（读屏需要——英文标识符对读屏用户是无意义的音节）。 */
const KIND_SPOKEN: Readonly<Record<BufferKind, string>> = {
  vertex: "顶点缓冲",
  index: "索引缓冲",
  uniform: "统一缓冲",
};

const HEAP_SPOKEN: Readonly<Record<HeapKind, string>> = {
  upload: "上传堆（拷贝引擎独占，帧率好但抢带宽）",
  default: "默认堆（与渲染共享带宽，容量大）",
};

/** 产出读屏行（文本自足于人类语言：类别、状态、大小、堆、区间、碎片）。 */
export function buildBufferReaderRows(
  ledger: BufferLedger,
  statsByKind: Readonly<Record<BufferKind, FragmentationStats>>,
): readonly BufferReaderRow[] {
  return ledger.all().map((e) => {
    const st = statsByKind[e.desc.kind];
    const fragPct = Math.round(st.fragmentationRatio * 100);
    return {
      bufferId: e.bufferId,
      shape: PHASE_SHAPE[e.phase],
      spoken:
        `${e.bufferId}：${PHASE_SPOKEN[e.phase]}；` +
        `类别${KIND_SPOKEN[e.desc.kind]}，请求 ${e.desc.sizeBytes} 字节` +
        `（对齐后 ${String(e.region?.size ?? e.desc.sizeBytes)} 字节），` +
        `堆${HEAP_SPOKEN[e.heap]}，` +
        (e.region === null
          ? "未分配内存区间"
          : `区间从第 ${e.region.offset} 字节到第 ${e.region.offset + e.region.size} 字节，` +
            `代戳 ${e.region.serial}，` +
            `该类别剩余 ${st.freeBytes} 字节，碎片率 ${fragPct}%`),
    };
  });
}

/** 面板摘要。 */
export function summarizeBufferPanel(
  ledger: BufferLedger,
  statsByKind: Readonly<Record<BufferKind, FragmentationStats>>,
): string {
  const all = ledger.all();
  const active = all.filter((e) => e.phase === "active").length;
  const frag = BUFFER_KINDS.map(
    (k) => `${KIND_SPOKEN[k]} ${Math.round(statsByKind[k].fragmentationRatio * 100)}%`,
  ).join("，");
  return `缓冲面板：共 ${all.length} 项，已激活 ${active} 项；碎片率 ${frag}。`;
}

// ════════════════════════════════════════════════════════════════════════════
// §11 跨域协议哈希（A08 句柄治理 · A05 预算池）
// ════════════════════════════════════════════════════════════════════════════

/** 本条对外数据契约。 */
export interface BufferDomainContract {
  readonly name: string;
  readonly version: number;
  readonly kinds: readonly BufferKind[];
  readonly usages: readonly BufferUsage[];
  readonly heaps: readonly HeapKind[];
  readonly phases: readonly BufferPhase[];
  readonly events: readonly BufferEvent[];
  readonly diagCodes: readonly BufferDiagCode[];
}

/** 规范化串（字段序固定）。 */
export function canonicalBufferContract(c: BufferDomainContract): string {
  return [
    c.name,
    String(c.version),
    c.kinds.join(","),
    c.usages.join(","),
    c.heaps.join(","),
    c.phases.join(","),
    c.events.join(","),
    c.diagCodes.join(","),
  ].join("|");
}

/** 协议行状态（待补与漂移分离）。 */
export type ProtocolRowState = "matched" | "pending" | "drifted";

export interface BufferProtocolRow {
  readonly field: string;
  readonly expected: string;
  readonly actual: string;
  readonly state: ProtocolRowState;
  readonly diagnostic: BufferDiagnostic | null;
}

/** 跨域核对（待补=非阻断、漂移=必须重签，不共码）。 */
export function verifyBufferContract(
  expected: BufferDomainContract,
  actual: Partial<BufferDomainContract> | null,
): readonly BufferProtocolRow[] {
  if (actual === null) {
    return [
      {
        field: "contract",
        expected: canonicalBufferContract(expected),
        actual: "",
        state: "pending",
        diagnostic: {
          code: "PROTOCOL_HASH_PENDING",
          message: "对方域尚未提供契约",
          hint: "待补为非阻断项：按 canonicalBufferContract 实现后重核",
        },
      },
    ];
  }
  const rows: BufferProtocolRow[] = [];
  const push = (field: string, e: unknown, a: unknown): void => {
    const es = Array.isArray(e) ? e.join(",") : String(e);
    const as = a === undefined || a === null ? "" : Array.isArray(a) ? a.join(",") : String(a);
    if (as === "") {
      rows.push({
        field,
        expected: es,
        actual: as,
        state: "pending",
        diagnostic: {
          code: "PROTOCOL_HASH_PENDING",
          message: `${field} 对方未提供`,
          hint: "待补为非阻断项",
        },
      });
    } else if (as !== es) {
      rows.push({
        field,
        expected: es,
        actual: as,
        state: "drifted",
        diagnostic: {
          code: "PROTOCOL_HASH_DRIFT",
          message: `${field} 契约漂移：期望 ${es.slice(0, 96)}，实际 ${as.slice(0, 96)}`,
          hint: "漂移必须重签：双方对齐后升版本号，不得就地私改",
        },
      });
    } else {
      rows.push({ field, expected: es, actual: as, state: "matched", diagnostic: null });
    }
  };
  push("name", expected.name, actual.name);
  push("version", expected.version, actual.version);
  push("kinds", expected.kinds, actual.kinds);
  push("usages", expected.usages, actual.usages);
  push("heaps", expected.heaps, actual.heaps);
  push("phases", expected.phases, actual.phases);
  push("events", expected.events, actual.events);
  push("diagCodes", expected.diagCodes, actual.diagCodes);
  return rows;
}

export const BUFFER_DOMAIN_CONTRACT: BufferDomainContract = {
  name: "ve.bufferResource",
  version: 1,
  kinds: BUFFER_KINDS,
  usages: BUFFER_USAGES,
  heaps: ["upload", "default"],
  phases: ["declared", "allocated", "active", "retired"],
  events: BUFFER_EVENTS,
  diagCodes: [
    "USAGE_EMPTY",
    "USAGE_INCOMPATIBLE",
    "SIZE_INVALID",
    "ALIGNMENT_INVALID",
    "INDEX_FORMAT_UNKNOWN",
    "OUT_OF_BOUNDS",
    "REGION_OVERLAP",
    "PARENT_ALLOC_TOO_SMALL",
    "FRAGMENTATION_EXCEEDED",
    "COMPACTION_DEFERRED",
    "COMPACTION_REQUIRES_MIGRATION",
    "ALIAS_CONFLICT",
    "ALIAS_UNDECLARED_USE",
    "ALIAS_OVERDECLARED",
    "UPLOAD_THROTTLED",
    "UPLOAD_BYTE_LIMIT_INVALID",
    "HEAP_POLICY_MISMATCH",
    "ILLEGAL_TRANSITION",
    "GENERATION_STALE",
    "BUFFER_RETIRED_REUSE",
    "PROTOCOL_HASH_PENDING",
    "PROTOCOL_HASH_DRIFT",
  ],
};

// ════════════════════════════════════════════════════════════════════════════
// §12 域级自检
// ════════════════════════════════════════════════════════════════════════════

/** 一条自检结论。 */
export interface BufferSelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

function mkDesc(over: Partial<BufferDesc> = {}): BufferDesc {
  return {
    kind: "vertex",
    sizeBytes: 1024,
    usages: ["vertex"],
    indexWidthBytes: 0,
    perFrame: true,
    aliasingAllowed: false,
    ...over,
  };
}

/** §A 描述与对齐校验。 */
export function selfCheckDesc(): readonly BufferSelfCheck[] {
  const out: BufferSelfCheck[] = [];
  const good = validateDesc(mkDesc());
  out.push({
    name: "valid-desc-accepted",
    pass: good.ok,
    detail: good.ok ? "合规描述被接受" : `合规描述被拒：${good.message}`,
  });
  const emptyUsage = validateDesc(mkDesc({ usages: [] }));
  out.push({
    name: "empty-usage-rejected",
    pass: !emptyUsage.ok && emptyUsage.code === "USAGE_EMPTY",
    detail: "空用途被USAGE_EMPTY 拦截",
  });
  const wrongUsage = validateDesc(mkDesc({ kind: "index", usages: ["vertex"] }));
  out.push({
    name: "kind-usage-mismatch-rejected",
    pass: !wrongUsage.ok && wrongUsage.code === "USAGE_INCOMPATIBLE",
    detail: "索引缓冲声明 vertex 用途被拦截（类别-用途矩阵校验）",
  });
  const badIndexWidth = validateDesc(mkDesc({ kind: "index", usages: ["index"], indexWidthBytes: 3 }));
  out.push({
    name: "bad-index-width-rejected",
    pass: !badIndexWidth.ok && badIndexWidth.code === "INDEX_FORMAT_UNKNOWN",
    detail: "3字节索引宽度被拦截（只允许 16/32bit）",
  });
  const badSize = validateDesc(mkDesc({ sizeBytes: 0 }));
  out.push({
    name: "nonpositive-size-rejected",
    pass: !badSize.ok && badSize.code === "SIZE_INVALID",
    detail: "零字节缓冲被拦截",
  });
  out.push({
    name: "align-up-correct",
    pass: alignUp(1, 256) === 256 && alignUp(256, 256) === 256 && alignUp(257, 256) === 512,
    detail: "alignUp 向上对齐正确（1→256, 256→256, 257→512）",
  });
  return out;
}

/** §B 子分配与碎片。 */
export function selfCheckSubAlloc(): readonly BufferSelfCheck[] {
  const out: BufferSelfCheck[] = [];
  const a = new SubAllocator(8192);
  const r1 = a.allocate("r1", "vertex", 1000, 4);
  out.push({
    name: "alloc-aligns-size-and-offset",
    pass: r1.ok && r1.value.size === 1000 && r1.value.offset === 0,
    detail: r1.ok ? `分配 1000 字节 → size ${r1.value.size} offset ${r1.value.offset}` : "分配失败",
  });
  const r2 = a.allocate("r2", "vertex", 100, 256);
  out.push({
    name: "uniform-align-offset",
    pass: r2.ok && r2.value.offset % 256 === 0 && r2.value.offset >= 1000,
    detail: r2.ok ? `uniform 对齐：offset ${r2.value.offset}（≥前块尾且 256 对齐）` : "分配失败",
  });
  const dup = a.allocate("r1", "vertex", 10, 4);
  out.push({
    name: "duplicate-region-rejected",
    pass: !dup.ok && dup.code === "REGION_OVERLAP",
    detail: "同名区域二次分配被拒绝",
  });
  const freed = a.free_region("r1");
  out.push({
    name: "free-merges-adjacent-spans",
    pass:
      freed.ok &&
      // 释放 r1(offset0,size1000) 后，空洞 [0,1024) 与 r2(offset1024) 相邻无缝，
      // 整段应合并为一段；另一段 [1280, ...) 被活跃区 r2 隔开，故仍是 2 段。
      a.stats().freeSpans === 2 &&
      a.freeSpans().some((s) => s.offset === 0 && s.size === 1024),
    detail: `释放 r1 后空洞 [0,1024) 已合并；总空闲区 ${a.stats().freeSpans} 段（r2 隔开，不可合并）`,
  });
  const dbl = a.free_region("r1");
  out.push({
    name: "double-free-rejected",
    pass: !dbl.ok,
    detail: "重复释放被拒（防显存账虚增）",
  });

  // 碎片整理：先占满再释放产生碎片
  const b = new SubAllocator(4096);
  const blocks: string[] = [];
  for (let i = 0; i < 4; i += 1) {
    const rr = b.allocate(`b${i}`, "uniform", 1024, 4);
    if (rr.ok) blocks.push(`b${i}`);
  }
  for (const id of ["b1", "b3"]) b.free_region(id);
  const frag = b.stats();
  out.push({
    name: "fragmentation-measured",
    pass: frag.freeSpans === 2 && frag.fragmentationRatio > 0,
    detail: `释放b1/b3 后空闲区 ${frag.freeSpans} 段，碎片率 ${(frag.fragmentationRatio * 100).toFixed(0)}%`,
  });
  const c = b.compact();
  // 整理 = 搬迁规划：把活跃区紧凑排列后，空洞应集中到**尾部一段**。
  // 关键区别：若只重建空闲链表，形态与整理前完全一致（空操作）。
  out.push({
    name: "compact-plans-tight-consolidation",
    pass:
      c.ok &&
      c.value.afterFreeSpans === 1 &&
      c.value.trailingFreeBytes === c.value.before.freeBytes,
    detail: c.ok
      ? `整理前 ${c.value.before.freeSpans} 段 → 搬迁落实后 ${c.value.afterFreeSpans} 段` +
        `（尾部 ${c.value.trailingFreeBytes} 字节）`
      : "整理规划失败",
  });
  out.push({
    name: "compact-lists-migrations",
    pass: c.ok && c.value.requiresMigration.length > 0 && c.value.moves.every((m) => m.reason.length > 0),
    detail: c.ok
      ? `搬迁清单 ${c.value.requiresMigration.length} 项，每项带理由`
      : "无搬迁清单",
  });
  out.push({
    name: "compact-reports-migration-diagnostic",
    pass: c.ok && c.diagnostics.some((d) => d.code === "COMPACTION_REQUIRES_MIGRATION"),
    detail: c.ok ? `整理产 ${c.diagnostics.length} 条诊断（含搬迁提示）` : "无诊断",
  });
  // 反例防回归：只重建空闲链表会得到与整理前相同的段数（空操作）
  out.push({
    name: "compact-is-not-noop",
    pass: c.ok && c.value.before.freeSpans > c.value.afterFreeSpans,
    detail: c.ok
      ? `整理确实改变形态（${c.value.before.freeSpans} → ${c.value.afterFreeSpans} 段），非空操作`
      : "—",
  });
  out.push({
    name: "needs-compaction-by-threshold",
    pass: b.needsCompaction(0.1) === true && b.needsCompaction(0.99) === false,
    detail: "碎片率 0.4 对阈值 0.1 触发、对 0.99 不触发（阈值机制生效）",
  });
  return out;
}

/** §C 越界阻断。 */
export function selfCheckBounds(): readonly BufferSelfCheck[] {
  const out: BufferSelfCheck[] = [];
  const a = new SubAllocator(4096);
  const r = a.allocate("x", "vertex", 256, 4);
  if (!r.ok) {
    out.push({ name: "setup", pass: false, detail: "分配失败，无法测越界" });
    return out;
  }
  const region = r.value;
  const inBounds = checkWrite(region, { regionId: "x", relativeOffset: 0, byteLength: 256, serial: 0 });
  out.push({
    name: "in-bounds-write-ok",
    pass: inBounds.ok && inBounds.absoluteOffset === region.offset,
    detail: `边界内写入通过（绝对偏移 ${inBounds.absoluteOffset}）`,
  });
  const overOne = checkWrite(region, { regionId: "x", relativeOffset: 256, byteLength: 1, serial: 0 });
  out.push({
    name: "one-byte-over-blocked",
    pass: !overOne.ok && overOne.diagnostic?.code === "OUT_OF_BOUNDS",
    detail: "越界 1 字节被阻断（半开区间右端闭于 size）",
  });
  const overTail = checkWrite(region, { regionId: "x", relativeOffset: 250, byteLength: 10, serial: 0 });
  out.push({
    name: "tail-overwrite-blocked",
    pass: !overTail.ok,
    detail: "尾部越界（250+10>256）被阻断",
  });
  out.push({
    name: "no-clamp-on-overflow",
    pass: !overTail.ok && overTail.absoluteOffset === 0 && (overTail.diagnostic?.hint.includes("不钳制") ?? false),
    detail: "越界不钳制（诊断明示不钳制，修正越界需重分配）",
  });
  const stale = checkWrite(region, { regionId: "x", relativeOffset: 0, byteLength: 4, serial: 99 });
  out.push({
    name: "stale-serial-write-blocked",
    pass: !stale.ok && stale.diagnostic?.code === "GENERATION_STALE",
    detail: "旧代句柄写入被 GENERATION_STALE 拦截",
  });
  return out;
}

/** §D 暂停整理。 */
export function selfCheckCompactionGate(): readonly BufferSelfCheck[] {
  const out: BufferSelfCheck[] = [];
  const base = { frameBudgetHeadroom: 0.5, interacting: false, minHeadroom: 0.2 };
  const ok = decideCompaction(base);
  out.push({
    name: "compaction-allowed-when-idle",
    pass: ok.proceed,
    detail: ok.proceed ? `空闲且余量足→允许整理：${ok.reason}` : `被拒：${ok.reason}`,
  });
  const interacting = decideCompaction({ ...base, interacting: true });
  out.push({
    name: "compaction-deferred-when-interacting",
    pass: !interacting.proceed && interacting.diagnostic?.code === "COMPACTION_DEFERRED",
    detail: interacting.proceed ? "交互期仍允许整理（违规）" : `交互期延后：${interacting.reason}`,
  });
  const tight = decideCompaction({ ...base, frameBudgetHeadroom: 0.1 });
  out.push({
    name: "compaction-deferred-when-tight",
    pass: !tight.proceed && tight.diagnostic?.code === "COMPACTION_DEFERRED",
    detail: tight.proceed ? "预算紧张仍整理（违规）" : `余量不足延后：${tight.reason}`,
  });
  return out;
}

/** §E 上传节流。 */
export function selfCheckThrottle(): readonly BufferSelfCheck[] {
  const out: BufferSelfCheck[] = [];
  const cfg = { maxBytesPerFrame: 1000, criticalBytesPerFrame: 600 };
  const items: UploadItem[] = [
    { itemId: "bg1", regionId: "r", byteLength: 500, critical: false },
    { itemId: "on1", regionId: "r", byteLength: 500, critical: true },
    { itemId: "bg2", regionId: "r", byteLength: 800, critical: false },
  ];
  const plan = planThrottle(items, cfg);
  out.push({
    name: "throttle-never-exceeds-cap",
    pass: plan.ok && plan.value.issuedBytes <= cfg.maxBytesPerFrame,
    detail: plan.ok ? `发出 ${plan.value.issuedBytes} ≤ 上限 ${cfg.maxBytesPerFrame}` : "节流失败",
  });
  out.push({
    name: "throttle-prioritizes-critical",
    pass: plan.ok && plan.value.issued.some((i) => i.itemId === "on1"),
    detail: plan.ok ? `发出项 ${plan.value.issued.map((i) => i.itemId).join(",")}（关键项在列）` : "—",
  });
  out.push({
    name: "throttle-defers-not-truncates",
    pass: plan.ok && plan.value.deferred.length > 0,
    detail: plan.ok ? `顺延 ${plan.value.deferred.length} 项（不截断）` : "—",
  });
  const bad = planThrottle(items, { maxBytesPerFrame: 100, criticalBytesPerFrame: 200 });
  out.push({
    name: "critical-cap-cannot-exceed-total",
    pass: !bad.ok && bad.code === "UPLOAD_BYTE_LIMIT_INVALID",
    detail: "关键档上限超过总上限被拒（关键档是总档子集）",
  });
  return out;
}

/** §F 别名验证器。 */
export function selfCheckAliases(): readonly BufferSelfCheck[] {
  const out: BufferSelfCheck[] = [];
  const led = new AliasLedger();
  led.declare({ alias: "vtx/hero", regionId: "r1", reason: "阴影 pass 与主 pass 共享同一份静态顶点", declaredBy: "material" });
  const used = led.noteUse("vtx/hero");
  out.push({
    name: "declared-use-allowed",
    pass: used.ok,
    detail: "已声明别名可正常记录使用",
  });
  const undecl = led.noteUse("vtx/nobody");
  out.push({
    name: "undeclared-use-blocked",
    pass: !undecl.ok && undecl.code === "ALIAS_UNDECLARED_USE",
    detail: "未声明别名使用被阻断（违例）",
  });
  const emptyReason = led.declare({ alias: "x", regionId: "r2", reason: "  ", declaredBy: "tool" });
  out.push({
    name: "empty-reason-declaration-rejected",
    pass: !emptyReason.ok && emptyReason.code === "ALIAS_OVERDECLARED",
    detail: "空理由别名声明被拒（虚报第一道闸）",
  });

  // 验证器双向核对
  const led2 = new AliasLedger();
  led2.declare({ alias: "used", regionId: "r1", reason: "共享", declaredBy: "m" });
  led2.declare({ alias: "unused", regionId: "r2", reason: "预留", declaredBy: "m" });
  const report = verifyAliases(led2, new Map([["used", 3]]));
  out.push({
    name: "verifier-flags-overdeclared",
    pass: report.ok && report.value.overdeclared === 1 && report.value.violations === 0,
    detail: report.ok ? `虚报 ${report.value.overdeclared} 处、违例 ${report.value.violations} 处` : "验证失败",
  });
  out.push({
    name: "verifier-two-directions-distinct",
    pass:
      report.ok &&
      report.value.findings.some((f) => f.kind === "overdeclared") &&
      report.diagnostics.some((d) => d.code === "ALIAS_OVERDECLARED"),
    detail: "虚报（审计级）与违例（阻断级）方向分离，不共码",
  });
  return out;
}

/** §G 堆策略。 */
export function selfCheckHeap(): readonly BufferSelfCheck[] {
  const out: BufferSelfCheck[] = [];
  const v = planHeap("vertex", 1024, null);
  out.push({
    name: "vertex-prefers-upload",
    pass: v.ok && v.value.heap === "upload",
    detail: v.ok ? `顶点推荐 ${v.value.heap}堆` : "堆规划失败",
  });
  const smallU = planHeap("uniform", 1024, null);
  out.push({
    name: "small-uniform-prefers-default",
    pass: smallU.ok && smallU.value.heap === "default",
    detail: smallU.ok ? `小 uniform 推荐 ${smallU.value.heap} 堆` : "堆规划失败",
  });
  const bigU = planHeap("uniform", 128 * 1024, null);
  out.push({
    name: "large-uniform-flips-to-upload",
    pass: bigU.ok && bigU.value.heap === "upload",
    detail: bigU.ok ? `大 uniform（128KB）翻转为 ${bigU.value.heap} 堆` : "堆规划失败",
  });
  const mismatch = planHeap("vertex", 1024, "default");
  out.push({
    name: "heap-mismatch-flagged",
    pass: mismatch.ok && mismatch.value.overridden && mismatch.value.correction !== null,
    detail: mismatch.ok ? `偏离推荐被标记，附纠正建议` : "堆规划失败",
  });
  out.push({
    name: "heap-mismatch-diagnostic",
    pass: mismatch.ok && mismatch.diagnostics.some((d) => d.code === "HEAP_POLICY_MISMATCH"),
    detail: "堆策略不匹配产阻断级诊断（HEAP_POLICY_MISMATCH）",
  });
  out.push({
    name: "heap-rules-have-reasons",
    pass: HEAP_CHOICE_RULES.every((r) => r.reason.length > 0 && r.penalty.length > 0),
    detail: `${HEAP_CHOICE_RULES.length} 条堆规则均含选择理由与违背代价`,
  });
  return out;
}

/** §H 统一台账与生命周期。 */
export function selfCheckLedger(): readonly BufferSelfCheck[] {
  const out: BufferSelfCheck[] = [];
  const caps: Record<BufferKind, number> = { vertex: 4096, index: 4096, uniform: 4096 };
  const led = new BufferLedger(caps);
  led.declare("v0", mkDesc({ kind: "vertex" }), "upload");
  led.declare("i0", mkDesc({ kind: "index", usages: ["index"], indexWidthBytes: 4 }), "upload");
  const a = led.allocate("v0");
  out.push({
    name: "declare-allocate-unified",
    pass: a.ok && a.value.region !== null,
    detail: a.ok ? `顶点分配区间 ${a.value.region?.offset}..${(a.value.region?.offset ?? 0) + (a.value.region?.size ?? 0)}` : "分配失败",
  });
  const act = led.activate("v0");
  out.push({
    name: "activate-after-allocate",
    pass: act.ok && act.value.phase === "active",
    detail: act.ok ? `激活后 ${act.value.phase}` : "激活失败",
  });
  const ret = led.retire("v0");
  out.push({
    name: "retire-frees-region",
    pass: ret.ok && ret.value.phase === "retired" && led.get("v0")?.region === null,
    detail: ret.ok ? "回收后区间已退分配" : "回收失败",
  });
  out.push({
    name: "three-kinds-one-ledger",
    pass: led.all().length === 2 && led.stats("vertex").parentBytes === 4096,
    detail: "顶点/索引共用一台账，碎片统计按类别可分",
  });
  const transTotal = BUFFER_EVENTS.every((e) => {
    const r = applyBufferTransition({ serial: 0, phase: "declared" }, e);
    return r.ok || r.code === "ILLEGAL_TRANSITION" || r.code === "BUFFER_RETIRED_REUSE";
  });
  out.push({
    name: "transition-table-total",
    pass: transTotal,
    detail: "所有事件对declared 都有显式结论（无静默）",
  });
  return out;
}

/** §I 无障碍读屏。 */
export function selfCheckAccessible(): readonly BufferSelfCheck[] {
  const out: BufferSelfCheck[] = [];
  const caps: Record<BufferKind, number> = { vertex: 4096, index: 4096, uniform: 4096 };
  const led = new BufferLedger(caps);
  led.declare("v0", mkDesc({ kind: "vertex", sizeBytes: 2048 }), "upload");
  led.allocate("v0");
  led.activate("v0");
  const statsByKind: Record<BufferKind, FragmentationStats> = {
    vertex: led.stats("vertex"),
    index: led.stats("index"),
    uniform: led.stats("uniform"),
  };
  const rows = buildBufferReaderRows(led, statsByKind);
  out.push({
    name: "reader-row-self-contained",
    pass:
      rows.length === 1 &&
      // 读屏文本须是**人类语言**，英文标识符对读屏用户是无意义音节
      (rows[0]?.spoken.includes("顶点缓冲") ?? false) &&
      (rows[0]?.spoken.includes("碎片率") ?? false) &&
      (rows[0]?.spoken.includes("上传堆") ?? false) &&
      !/[=]/.test(rows[0]?.spoken ?? ""),
    detail: "读屏文本自足且为人话：类别译名、状态、大小、堆说明、区间、碎片率",
  });
  const sum = summarizeBufferPanel(led, statsByKind);
  out.push({
    name: "panel-summary-spoken",
    pass: sum.includes("缓冲面板") && sum.includes("顶点缓冲") && sum.includes("碎片率"),
    detail: "面板摘要含总数、激活数与各类人话碎片率",
  });
  return out;
}

/** §J 跨域协议。 */
export function selfCheckProtocol(): readonly BufferSelfCheck[] {
  const out: BufferSelfCheck[] = [];
  const pending = verifyBufferContract(BUFFER_DOMAIN_CONTRACT, null);
  out.push({
    name: "missing-contract-pending",
    pass: pending[0]?.state === "pending" && pending[0]?.diagnostic?.code === "PROTOCOL_HASH_PENDING",
    detail: "对方未提供契约 → pending（非阻断）",
  });
  const drift = verifyBufferContract(BUFFER_DOMAIN_CONTRACT, { version: 3 });
  const row = drift.find((r) => r.field === "version");
  out.push({
    name: "version-drift-detected",
    pass: row?.state === "drifted" && row.diagnostic?.code === "PROTOCOL_HASH_DRIFT",
    detail: "版本漂移 → drifted（必须重签）",
  });
  out.push({
    name: "pending-drift-codes-distinct",
    pass: pending[0]?.diagnostic?.code !== row?.diagnostic?.code,
    detail: "待补与漂移不共码",
  });
  const matched = verifyBufferContract(BUFFER_DOMAIN_CONTRACT, { ...BUFFER_DOMAIN_CONTRACT });
  out.push({
    name: "identical-contract-matched",
    pass: matched.every((r) => r.state === "matched"),
    detail: `一致契约 ${matched.length} 项全 matched`,
  });
  return out;
}

/** 全量自检入口。 */
export function runBufferSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly BufferSelfCheck[]>>;
  readonly allPass: boolean;
  readonly total: number;
  readonly failed: readonly string[];
} {
  const groups = {
    desc: selfCheckDesc(),
    subAlloc: selfCheckSubAlloc(),
    bounds: selfCheckBounds(),
    gate: selfCheckCompactionGate(),
    throttle: selfCheckThrottle(),
    aliases: selfCheckAliases(),
    heap: selfCheckHeap(),
    ledger: selfCheckLedger(),
    accessible: selfCheckAccessible(),
    protocol: selfCheckProtocol(),
  };
  const failed: string[] = [];
  let total = 0;
  for (const [g, items] of Object.entries(groups)) {
    for (const it of items) {
      total += 1;
      if (!it.pass) failed.push(`${g}.${it.name}: ${it.detail}`);
    }
  }
  return { groups, allPass: failed.length === 0, total, failed };
}