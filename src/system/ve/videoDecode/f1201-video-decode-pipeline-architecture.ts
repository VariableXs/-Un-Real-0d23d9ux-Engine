/**
 * VE-F1201 · 视频解码管线架构（G 域 · 视频解码引擎 · 批次 G01 首项）
 * ---------------------------------------------------------------------------
 * 职责定位：G 域开工条目——宣告视频解码引擎（F1201-F1220）正式开工，并交付
 *   **五层解码管线架构本体**。本条不是"画一张架构图"，而是把五层、层间契约、
 *   帧上下文、路由在层间的位置、错误跨层传播语义、管线延迟分解，落地为
 *   一份可被机器校验、可被下游 19 条直接消费的架构契约。
 *
 * ┌── 五层（锚点原文）────────────────────────────────────────────────────────┐
 * │ L1 容器层  MP4 / MKV / WebM 解封装——轨道 / 编解码器 / 时间戳提取         │
 * │ L2 码流层  H.264 / H.265 / VP9 / AV1 码流 NAL 单元解析                  │
 * │ L3 解码层  硬解路由 / 软解执行——CGPU-F0104 路由决策联动                  │
 * │ L4 后处理  去块 / 去振铃 / 色彩空间转换——CGPU-F0098 联动                │
 * │ L5 输出层  帧输出到表面 / 纹理                                           │
 * └───────────────────────────────────────────────────────────────────────────┘
 *
 * 路由在层间的位置（锚点原文：码流层感知格式、解码层执行路由）——这是本条最
 * 容易被做错的一条，因此写成**可执行断言**而非注释：
 *   · 码流层（L2）持有 `perceivedCodec`，它只**感知**格式、由容器声明与码流
 *     探测共同决定，不做任何"能不能解"的裁决；
 *   · 解码层（L3）持有 `routeDecision`，它**执行**路由，消费 CGPU-F0104 的能力
 *     位图与决策树；
 *   · 若把路由决策放进 L2，则 L2 与硬件能力耦合，L2 就不再可替换（换解码器就
 *     得改容器解析）——这正是「每层可独立替换」要防的事。`verifyRoutePlacement`
 *     把该反例做成会失败的断言。
 *
 * 可替换性契约（锚点原文：每层可独立替换实现，接口冻结，层替换不破坏相邻层）：
 *   每一层注册时携带 `frozenContract`（层 id + 输入语义 + 输出语义 + 失败语义
 *   + 前后邻居）。替换实现时**只允许**换 `implementationId`，`frozenContract`
 *   逐字段不变。`assertReplacementPreservesContract` 在替换发生时强制比对，
 *   任何字段漂移即产出 FREEZE_DRIFT 并拒绝替换——把"层替换不破坏相邻层"从
 *   承诺变成机制。
 *
 * 帧上下文（锚点：跨层传递的帧元数据）：
 *   `FrameContext` 是五层之间唯一的传递载体。它**只增不改**（append-only
 *   分区写入），因为跨层共享的可变状态是解码管线最经典的数据竞争来源：B 帧
 *   重排时 L3 可能晚于 L5 消费同一帧的元数据，写入者覆盖会让输出层读到半写的
 *   时间戳。分区字段由 `FrameFieldSlot` 声明归属层，非归属层写入即字段越权诊断。
 *
 * 错误跨层传播（锚点：层失败 → 层内错误三要素 + 上层可见）：
 *   每层失败产出 `LayerFailure`（发生了什么 / 为什么 / 下一步怎么办 + 失败层 +
 *   失败时刻的上下游快照）。传播策略不是"抛异常"，而是**标注后继续**：
 *   `propagateFailure` 按 `layerPolicy` 决定 abort / skip-frame / degrade-to-soft，
 *   并把 failure 挂到帧上下文，使上游层与下游层**都**能看到（上层可见判据）。
 *   静默吞错是本条的红线：任何 `catch` 之后若无 failure 记录，即自检失败。
 *
 * 管线延迟分解（锚点：每层耗时，层延迟超标 → 定位到层）：
 *   `LayerTiming` 逐层记 wall 耗时，`decomposeLatency` 归一化为占比并给出
 *   瓶颈层裁决。超标定位到层而非"解码慢"——这是本条判据的可执行形态。
 *
 * 零静默纪律：本模块所有拒绝、越权、替换漂移、路由错位、错误吞没都产出
 *   Diagnostic（code + message + hint），由调用方聚合上报，不向 UI 抛裸异常。
 *
 * 判据：五层架构完整、可替换断言、接口冻结、管线延迟分解（每层耗时）。
 *
 * 依赖锚点：F0104（CGPU 路由决策）、F0098（CGPU 色彩/缩放）、F1202（MP4 解封装，
 *   消费 L1 契约）、F1203（MKV 解封装，替换 L1 实现）、F1204-H.264 / F1205-HEVC /
 *   F1206-VP9 / F1207-AV1（消费 L2+L3 契约）、F1208（硬件解码路由，消费 L3 路由
 *   位）、F1209（帧输出与表面集成，消费 L5）、F1210（时间戳同步，消费帧上下文
 *   PTS/DTS 分区）、F1211（错误恢复，扩展 LayerFailure 传播）、F1213（内存治理，
 *   消费 L3 DPB 预算）、F1214（安全审计，消费五层拒绝码）、F1216（API 冻结，
 *   消费本条五层接口作为内部分层）、F1218（文档，渲染本条宣告文本）。
 * 下游不承担：本条只管「架构声明 + 层间契约 + 帧上下文 + 传播语义 + 延迟分解」，
 *   具体 MP4/MKV 解析归 F1202/F1203，四个解码器归 F1204-F1207，路由决策树归
 *   F1208，表面导入归 F1209。
 */

/* ═══════════════════════════════════════════════════════════════════════════
 * §1 诊断与结果类型（零静默的基础设施）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 诊断码：每种拒绝 / 越权 / 漂移 / 错位 / 吞没都有独立可检索的码，
 * 绝不合并成通用错误——合并即意味着其中一类将来无法被统计与告警。
 */
export type DiagCode =
  /** 五层未齐备（缺任一层即为架构不完整）。 */
  | "PIPELINE_LAYER_MISSING"
  /** 层序乱序（层号非单调递增）。 */
  | "PIPELINE_LAYER_ORDER_INVALID"
  /** 层重复注册（同一层 id 出现两次）。 */
  | "PIPELINE_LAYER_DUPLICATE"
  /** 未注册层被引用（悬空层引用）。 */
  | "PIPELINE_LAYER_UNKNOWN"
  /** 层的实现替换破坏了层间契约（冻结字段漂移）。 */
  | "FREEZE_DRIFT"
  /** 层声明替换但冻结契约未登记（无基线可比对 = 无法校验）。 */
  | "CONTRACT_BASELINE_MISSING"
  /** 层间契约的上下游声明与实际装配不一致（邻居错配）。 */
  | "LAYER_ADJACENCY_MISMATCH"
  /** 层的数据契约不匹配（上游输出语义 ≠ 下游输入语义）。 */
  | "LAYER_DATA_CONTRACT_VIOLATION"
  /** 路由决策出现在非解码层（路由必须且只能在解码层执行）。 */
  | "ROUTE_MISPLACED"
  /** 码流层缺失格式感知（路由前置条件不成立）。 */
  | "ROUTE_PERCEPTION_MISSING"
  /** 帧上下文字段越权（非归属层写入该分区）。 */
  | "FRAME_FIELD_ACCESS_VIOLATION"
  /** 帧上下文字段必填项缺失（时间戳/格式等跨层必需元数据缺位）。 */
  | "FRAME_CONTEXT_FIELD_MISSING"
  /** 帧上下文时序倒置（PTS/DTS 关系与帧类型矛盾）。 */
  | "FRAME_TIMEBASE_INCONSISTENT"
  /** 层失败未产出三要素错误（吞错红线）。 */
  | "LAYER_FAILURE_SILENT"
  /** 错误传播策略未登记（层失败但无策略 = 上层不可知）。 */
  | "FAILURE_POLICY_UNREGISTERED"
  /** 错误传播后上层不可见（失败只留在本层，上层拿不到）。 */
  | "FAILURE_NOT_UPPSTREAM_VISIBLE"
  /** 延迟账缺层（某层无耗时记录，无法做延迟分解）。 */
  | "LATENCY_SAMPLE_MISSING"
  /** 延迟预算超标的层未定位到具体层。 */
  | "LATENCY_BREACH_UNATTRIBUTED"
  /** 延迟账时间倒退（wall clock 非单调，账不可信）。 */
  | "LATENCY_CLOCK_NON_MONOTONIC"
  /** 帧预算未声明（无预算即无法判超标）。 */
  | "FRAME_BUDGET_UNDECLARED"
  /** 帧预算非法（非正数 / 上限过宽失去门禁意义）。 */
  | "FRAME_BUDGET_INVALID"
  /** 层实现自报能力缺项（实现未声明必需能力位）。 */
  | "LAYER_CAPABILITY_UNDECLARED"
  /** 层实现上报了契约未授予的能力（越权自抬）。 */
  | "LAYER_CAPABILITY_OVERREACH"
  /** 契约版本非法（非 v1 / 非受支持的演进位）。 */
  | "CONTRACT_VERSION_INVALID"
  /** 判据自检不通过。 */
  | "CRITERION_SELFCHECK_FAILED";

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

/** 成功构造（diagnostics 允许携带非致命告警，例如替换已发生但伴随性能告警）。 */
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

/**
 * 诊断袋：跨函数聚合容器。
 *
 * 存在的理由：本条有大量「产出冲突清单但整体仍成功」的场景（层替换告警、
 * 路由错位告警）。用返回单个 Outcome 会迫使这些告警被丢弃或让函数失败，
 * 二者都错。诊断袋让告警与结果正交——结果回答「成没成」，诊断回答「有异常」。
 */
export class DiagBag {
  private readonly items: Diagnostic[] = [];

  /** 追加一条诊断。 */
  push(code: DiagCode, message: string, hint: string): void {
    this.items.push({ code, message, hint });
  }

  /** 批量并入。 */
  pushAll(list: readonly Diagnostic[]): void {
    this.items.push(...list);
  }

  /** 全部诊断。 */
  all(): readonly Diagnostic[] {
    return this.items;
  }

  /** 按码取子集（统计与告警路由用）。 */
  byCode(code: DiagCode): readonly Diagnostic[] {
    return this.items.filter((d) => d.code === code);
  }

  /** 条数。 */
  size(): number {
    return this.items.length;
  }

  /** 去重后的条数（message 相同视为同一条，防告警洪水）。 */
  uniqueSize(): number {
    return new Set(this.items.map((d) => `${d.code}|${d.message}`)).size;
  }

  /** 是否存在指定码。 */
  has(code: DiagCode): boolean {
    return this.items.some((d) => d.code === code);
  }
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §2 五层标识与域声明
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * G 域标识（视频解码引擎域）。
 * 域号 G，条目区间 F1201-F1400（200 条 = 10 组 × 20 条），本条为组内第 1 条。
 */
export const G_DOMAIN = {
  /** 域标签，与册内 `VE-G` 一致。 */
  tag: "VE-G",
  /** 域中文名。 */
  title: "视频解码引擎域",
  /** 条目起始（域开工条，即本条）。 */
  firstEntryId: 1201,
  /** 条目结束（域收官条）。 */
  lastEntryId: 1400,
  /** 条目总数。 */
  entryCount: 200,
  /** 组数（10 组，每组 20 条）。 */
  groupCount: 10,
  /** 每组条数。 */
  entriesPerGroup: 20,
} as const;

/** 五层标识（锚点原文五层 + 顺序）。序号即数据流方向，不可乱序。 */
export type LayerId =
  /** L1 容器层：MP4/MKV/WebM 解封装——轨道/编解码器/时间戳提取。 */
  | "L1-container"
  /** L2 码流层：H.264/H.265/VP9/AV1 码流 NAL 单元解析。 */
  | "L2-bitstream"
  /** L3 解码层：硬解路由/软解执行（CGPU-F0104 联动）。 */
  | "L3-decode"
  /** L4 后处理层：去块/去振铃/色彩空间转换（CGPU-F0098 联动）。 */
  | "L4-postprocess"
  /** L5 输出层：帧输出到表面/纹理。 */
  | "L5-output";

/** 五层的规范序（数据流方向：容器 → 码流 → 解码 → 后处理 → 输出）。 */
export const LAYER_ORDER: readonly LayerId[] = [
  "L1-container",
  "L2-bitstream",
  "L3-decode",
  "L4-postprocess",
  "L5-output",
];

/** 层号（用于序比较与预算排序，不可为 0 或负）。 */
export const LAYER_RANK: Readonly<Record<LayerId, number>> = {
  "L1-container": 1,
  "L2-bitstream": 2,
  "L3-decode": 3,
  "L4-postprocess": 4,
  "L5-output": 5,
};

/** 层中文名（文档与诊断文案共用，避免同一层两种叫法导致检索失效）。 */
export const LAYER_LABEL: Readonly<Record<LayerId, string>> = {
  "L1-container": "容器层",
  "L2-bitstream": "码流层",
  "L3-decode": "解码层",
  "L4-postprocess": "后处理层",
  "L5-output": "输出层",
};

/** 层职责一句话（架构文档的表列数据，代码与文档同源）。 */
export const LAYER_DUTY: Readonly<Record<LayerId, string>> = {
  "L1-container": "MP4/MKV/WebM 解封装，产出轨道描述、编解码器标识与时间戳",
  "L2-bitstream": "解析码流语法单元（NAL/OBU），产出参数集与切片序列",
  "L3-decode": "按路由决策执行硬解或软解，产出参考帧与重建帧",
  "L4-postprocess": "去块、去振铃、色彩空间转换，产出可渲染像素",
  "L5-output": "帧输出到表面或纹理，交给合成器采样",
};

/**
 * 每层的责任条目号（实现归谁，架构归本条）。
 * 这张表把「架构声明」与「实现归属」焊在一起：出现一个没有责任条目的层，
 * 即为无主层（无人实现也无人验收），必须报错而非默认存在。
 */
export const LAYER_OWNER_ENTRY: Readonly<Record<LayerId, number>> = {
  "L1-container": 1202, // MP4 解封装（F1203 替换为 MKV/WebM 实现）
  "L2-bitstream": 1204, // H.264 解析（F1205/F1206/F1207 各自实现）
  "L3-decode": 1204, // 解码核心（F1204-F1207 各自实现）
  "L4-postprocess": 1201, // 后处理挂接契约由本条冻结，算法归 L4 消费方
  "L5-output": 1209, // 帧输出与表面集成
};

/**
 * 每层的实现条目集合（同一层可以有多个可替换实现——这正是"可替换"的前提）。
 * 单实现层同样登记为单元素数组：可替换性断言不因"当前只有一个"而失效。
 */
export const LAYER_IMPLEMENTATION_ENTRIES: Readonly<Record<LayerId, readonly number[]>> = {
  "L1-container": [1202, 1203], // MP4 / MKV-WebM 两实现，可互相替换
  "L2-bitstream": [1204, 1205, 1206, 1207], // H.264 / HEVC / VP9 / AV1
  "L3-decode": [1204, 1205, 1206, 1207, 1208], // 四解码器 + 硬件路由实现
  "L4-postprocess": [1201],
  "L5-output": [1209],
};

/**
 * 路由在层间的位置（锚点原文：码流层感知格式、解码层执行路由）。
 * 写成两个可查询的常量位，而非散落在注释里——因为位置错了不会有编译错误，
 * 只会让 L2 与硬件耦合，而那要到换解码器时才暴露。
 */
export const ROUTE_PERCEPTION_LAYER: LayerId = "L2-bitstream";
export const ROUTE_EXECUTION_LAYER: LayerId = "L3-decode";

/* ═══════════════════════════════════════════════════════════════════════════
 * §3 层间契约（冻结面：五层接口各自冻结，十年承诺）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 数据语义标签：层间传什么。
 *
 * 用封闭枚举而非字符串，是为了让"上游输出 ≠ 下游输入"这种错配在类型层
 * 就无法被误写。数值即语义码，比对时不做字符串前缀匹配（`hevc` 与
 * `hevc-part2` 用 startsWith 会漏判）。
 */
export type PayloadKind =
  /** 轨道描述（编解码器标识/宽高/时基/轨道数）。 */
  | "track-descriptor"
  /** 封装样本（带 PTS/DTS 的压缩包）。 */
  | "encoded-sample"
  /** 语法单元序列（NAL / OBU）。 */
  | "syntax-unit"
  /** 解码帧（YUV 平面 + 步幅）。 */
  | "decoded-frame"
  /** 后处理帧（已做环路滤波与色彩转换）。 */
  | "processed-frame"
  /** 输出表面句柄（零拷贝表面或纹理引用）。 */
  | "output-surface";

/** 每层「消费什么 / 产出什么」的语义表（数据契约的单一事实源）。 */
export const LAYER_PAYLOAD: Readonly<Record<LayerId, { readonly consumes: PayloadKind; readonly produces: PayloadKind }>> = {
  "L1-container": { consumes: "track-descriptor", produces: "encoded-sample" },
  "L2-bitstream": { consumes: "encoded-sample", produces: "syntax-unit" },
  "L3-decode": { consumes: "syntax-unit", produces: "decoded-frame" },
  "L4-postprocess": { consumes: "decoded-frame", produces: "processed-frame" },
  "L5-output": { consumes: "processed-frame", produces: "output-surface" },
};

/**
 * 层能力位：实现可自报的能力（契约授予，非自抬）。
 *
 * 存在的理由：可替换性要求"换实现不影响相邻层"，但若实现 B 比 A 少一个
 * 能力位（例如 MKV 不支持 lacing → Attachment），替换就会静默削功能。
 * 能力位把这种差异变成**显式声明**，替换时强制比对。
 */
export type LayerCapability =
  /** 容器层：解析 EBML/Matroska 家族。 */
  | "container-ebml"
  /** 容器层：解析 ISO BMFF 家族。 */
  | "container-isobmff"
  /** 容器层：解析分片 MP4（流式）。 */
  | "container-fragmented"
  /** 码流层：解析 NAL 单元（avc/hevc 家族）。 */
  | "bitstream-nal"
  /** 码流层：解析 OBU（AV1）。 */
  | "bitstream-obu"
  /** 码流层：bool coder 熵状态（VP9 家族）。 */
  | "bitstream-boolcoder"
  /** 解码层：执行硬解（需硬件能力位配合）。 */
  | "decode-hardware"
  /** 解码层：执行软解。 */
  | "decode-software"
  /** 后处理层：去块滤波。 */
  | "post-deblock"
  /** 后处理层：去振铃。 */
  | "post-dering"
  /** 后处理层：色彩空间转换。 */
  | "post-colorspace"
  /** 输出层：DMA-BUF 类表面导入（零拷贝）。 */
  | "output-surface-import"
  /** 输出层：纹理导入。 */
  | "output-texture-import";

/**
 * 冻结契约：层与相邻层之间的十年承诺。
 *
 * 冻结的语义是：替换 `implementationId` 时，本结构**逐字段不变**。
 * 这五个字段就是"层替换不破坏相邻层"的全部含义——不多，也不少：
 *   · id / neighbors —— 装配位置与邻居关系；
 *   · consumes / produces —— 数据语义，相邻层靠它决定能否接上；
 *   · capabilities —— 能力面，替换不得削能力；
 *   · failurePolicy —— 失败语义，相邻层靠它决定如何继续；
 *   · contractVersion —— 版本位，十年承诺下的受控演进出口。
 */
export interface FrozenLayerContract {
  /** 层标识。 */
  readonly layerId: LayerId;
  /** 上游邻居（无上游为 null，源在 L1 之前的 open 动作）。 */
  readonly upstream: LayerId | null;
  /** 下游邻居（无下游为 null，L5 之后是合成器）。 */
  readonly downstream: LayerId | null;
  /** 消费的数据语义。 */
  readonly consumes: PayloadKind;
  /** 产出的数据语义。 */
  readonly produces: PayloadKind;
  /** 能力位集合（契约授予，实现须声明齐全）。 */
  readonly capabilities: readonly LayerCapability[];
  /** 该层失败时的默认传播策略（实现可更严不可更松）。 */
  readonly failurePolicy: FailurePolicy;
  /** 契约版本（十年承诺下唯一允许的演进出口）。 */
  readonly contractVersion: string;
}

/**
 * 失败传播策略（锚点：层失败 → 层内错误三要素 + 上层可见）。
 *
 * 三档而非一档：真实解码管线里，"一个 NAL 坏掉"与"整个码流不是本格式"
 * 需要不同的处置——前者跳过该帧继续，后者中止会话。合成一档会导致
 * 「一个坏帧毁掉整段视频」或「格式完全不对还在空转」两个极端。
 */
export type FailurePolicy =
  /** 中止整条管线（不可恢复：格式不符 / 参数集非法）。 */
  | "abort"
  /** 跳过当前帧继续（可恢复：单帧码流错误）。 */
  | "skip-frame"
  /** 降级到软解重试（可恢复：硬解初始化或运行时失败）。 */
  | "degrade-to-software";

/** 失败策略的严格程度序（实现可更严不可更松：abort 最严，skip-frame 最松）。 */
export const FAILURE_POLICY_STRICTNESS: Readonly<Record<FailurePolicy, number>> = {
  abort: 3,
  "degrade-to-software": 2,
  "skip-frame": 1,
};

/** 默认失败策略表（架构声明的基线；实现可收紧不可放松）。 */
export const LAYER_DEFAULT_FAILURE_POLICY: Readonly<Record<LayerId, FailurePolicy>> = {
  "L1-container": "abort", // 容器解析失败则无任何有意义的下游输入
  "L2-bitstream": "skip-frame", // 单个 NAL 坏掉不影响其余帧
  "L3-decode": "degrade-to-software", // 硬解失败回退软解（F1208 联动）
  "L4-postprocess": "skip-frame", // 滤波失败可跳过该帧的滤波，直出未滤波帧
  "L5-output": "skip-frame", // 表面导入失败可丢该帧，下一帧继续
};

/** 契约版本（十年承诺的唯一演进出口：语义破坏性变更须升版本并走 ADR）。 */
export const PIPELINE_CONTRACT_VERSION = "v1";

/* ═══════════════════════════════════════════════════════════════════════════
 * §4 层注册与替换（可替换性的可执行机制）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 层实现注册项：一层当前挂哪个实现。 */
export interface LayerRegistration {
  readonly layerId: LayerId;
  /** 实现标识（如 `mp4-iso-bmff` / `mkv-ebml` / `h264-software` / `hevc-hardware`）。 */
  readonly implementationId: string;
  /** 该实现的责任条目号（必须落在 LAYER_IMPLEMENTATION_ENTRIES 内）。 */
  readonly ownerEntryId: number;
  /** 实现自报能力位（须是契约授予能力的子集）。 */
  readonly declaredCapabilities: readonly LayerCapability[];
  /** 该实现自报在路由链上的角色（须与规范位置一致；不一致即越位）。 */
  readonly routeRole: RouteRole;
  /** 该实现实际挂载的契约（替换时须与基线逐字段一致）。 */
  readonly contract: FrozenLayerContract;
}

/**
 * 层在路由链上的自报角色。
 *
 * 存在的理由：路由位置写死在常量里就**检不出越位**——装配方硬把路由塞进
 * L1 时，架构层看不到任何异常，直到换解码器才发现 L1 依赖了硬件。
 * 让每个层注册项显式自报角色，越位才成为装配期可查的事实。
 */
export type RouteRole =
  /** 不参与路由（默认位）。 */
  | "none"
  /** 格式感知者（规范上只能是 L2 码流层）。 */
  | "perception"
  /** 路由执行者（规范上只能是 L3 解码层）。 */
  | "execution";

/** 替换事件（留痕：谁在什么时候把哪层换成了什么）。 */
export interface ReplacementEvent {
  readonly layerId: LayerId;
  readonly fromImplementationId: string;
  readonly toImplementationId: string;
  /** 逐字段比对结果（全等为 true）。 */
  readonly contractPreserved: boolean;
  /** 伴随诊断（能力削减、版本漂移等）。 */
  readonly diagnostics: readonly Diagnostic[];
}

/**
 * 管线装配体：五层注册表 + 路由位置 + 帧预算。
 * 所有校验函数都作用在它上面，保证"架构声明"只有一个可校验的对象。
 */
export interface PipelineAssembly {
  readonly layers: readonly LayerRegistration[];
  /** 每帧时间预算（毫秒）；超标即定位到层。 */
  readonly frameBudgetMs: number;
  /** 冻结时间戳（调用方注入，本模块不读时钟，保持可测试与可重放）。 */
  readonly assembledAt: number;
}

/** 由规范序与默认策略派生的基线契约表（架构声明的单一事实源）。 */
export function buildBaselineContracts(): ReadonlyMap<LayerId, FrozenLayerContract> {
  const table = new Map<LayerId, FrozenLayerContract>();
  for (let i = 0; i < LAYER_ORDER.length; i++) {
    const layerId = LAYER_ORDER[i];
    if (layerId === undefined) continue;
    const payload = LAYER_PAYLOAD[layerId];
    table.set(layerId, {
      layerId,
      upstream: i === 0 ? null : (LAYER_ORDER[i - 1] ?? null),
      downstream: i === LAYER_ORDER.length - 1 ? null : (LAYER_ORDER[i + 1] ?? null),
      consumes: payload.consumes,
      produces: payload.produces,
      capabilities: baselineCapabilities(layerId),
      failurePolicy: LAYER_DEFAULT_FAILURE_POLICY[layerId],
      contractVersion: PIPELINE_CONTRACT_VERSION,
    });
  }
  return table;
}

/** 每层基线能力位（契约授予面；实现自报必须是它的子集）。 */
function baselineCapabilities(layerId: LayerId): readonly LayerCapability[] {
  switch (layerId) {
    case "L1-container":
      return ["container-ebml", "container-isobmff", "container-fragmented"];
    case "L2-bitstream":
      return ["bitstream-nal", "bitstream-obu", "bitstream-boolcoder"];
    case "L3-decode":
      return ["decode-hardware", "decode-software"];
    case "L4-postprocess":
      return ["post-deblock", "post-dering", "post-colorspace"];
    case "L5-output":
      return ["output-surface-import", "output-texture-import"];
    default: {
      // 默认分支显性拒绝而非静默返回空集：新增层忘了配基线能力，
      // 空集会让所有实现都"零越权"，从而悄悄失去能力面校验。
      const exhaustive: never = layerId;
      throw new Error(`未登记基线能力位的层：${String(exhaustive)}`);
    }
  }
}

/**
 * 校验契约结构本身：邻居对称性 + 数据契约衔接 + 版本合法 + 策略已登记。
 *
 * 数据契约衔接的判据：下游层的 consumes 必须等于上游层的 produces。
 * 这一条能直接抓住「把 L3 实现换成不认识 L2 语法的另一种解码器」这类
 * 只在运行时才炸的错配——在装配期就拦住。
 */
export function verifyContractShape(
  contract: FrozenLayerContract,
  all: ReadonlyMap<LayerId, FrozenLayerContract>,
): Outcome<FrozenLayerContract> {
  const diagnostics: Diagnostic[] = [];

  // 4.0 邻接必须与序表逐位吻合（不许「无上游」蔓延到中间层）。
  //
  // 存在的理由：本函数早先只校验「非null 侧的邻居对称性」，于是把中间层的
  // upstream 改成 null 就完全绕过——L1 声明无上游是合法的，L3 也声明无上游
  // 却意味着容器层的产出凭空消失。校验不写这一条，邻接断裂就成了静默事实。
  const canonical = baselineAdjacency(contract.layerId);
  if (contract.upstream !== canonical.upstream) {
    diagnostics.push({
      code: "LAYER_ADJACENCY_MISMATCH",
      message: `层 ${contract.layerId} 的上游声明为 ${String(contract.upstream)}，序表位置为 ${String(canonical.upstream)}。`,
      hint: `邻接由 LAYER_ORDER 唯一决定：容器层无上游、输出层无下游，中间层不得自认无上游；修正声明而非改序表。`,
    });
  }
  if (contract.downstream !== canonical.downstream) {
    diagnostics.push({
      code: "LAYER_ADJACENCY_MISMATCH",
      message: `层 ${contract.layerId} 的下游声明为 ${String(contract.downstream)}，序表位置为 ${String(canonical.downstream)}。`,
      hint: `邻接由 LAYER_ORDER 唯一决定；修正声明而非改序表。`,
    });
  }

  // 4.1 邻居对称性：我认上游，下游必须认我。
  if (contract.upstream !== null) {
    const up = all.get(contract.upstream);
    if (up === undefined) {
      diagnostics.push({
        code: "PIPELINE_LAYER_UNKNOWN",
        message: `层 ${contract.layerId} 的上游 ${contract.upstream} 未注册。`,
        hint: "先注册上游层再校验本层；悬空邻居意味着装配序写错了。",
      });
    } else if (up.downstream !== contract.layerId) {
      diagnostics.push({
        code: "LAYER_ADJACENCY_MISMATCH",
        message: `邻居错配：${contract.upstream} 的下游是 ${String(up.downstream)}，但 ${contract.layerId} 声明上游为 ${contract.upstream}。`,
        hint: "邻居关系必须互为对称；修正其一即可（L1 上游与 L5 下游恒为 null）。",
      });
    }
  }

  // 4.2 数据契约衔接。
  if (contract.upstream !== null) {
    const up = all.get(contract.upstream);
    if (up !== undefined && up.produces !== contract.consumes) {
      diagnostics.push({
        code: "LAYER_DATA_CONTRACT_VIOLATION",
        message: `数据契约断裂：${contract.upstream} 产出 ${up.produces}，${contract.layerId} 却声明消费 ${contract.consumes}。`,
        hint: `修正 ${contract.layerId} 的 consumes 为 ${up.produces}，或在 ${contract.upstream} 侧加一个转换层；不要在实现里私自做隐式转换。`,
      });
    }
  }

  // 4.3 版本合法：十年承诺下 v1 是唯一基线位，其余显式拒绝。
  if (contract.contractVersion !== PIPELINE_CONTRACT_VERSION) {
    diagnostics.push({
      code: "CONTRACT_VERSION_INVALID",
      message: `层 ${contract.layerId} 的契约版本为 ${contract.contractVersion}，基线为 ${PIPELINE_CONTRACT_VERSION}。`,
      hint: `v1 是十年承诺的基线位；语义破坏性变更须升版本并走 ADR（F1201 修正流程），不得就地改号。`,
    });
  }

  // 4.4 失败策略必须已登记（未登记 = 上层不可知 = 等于吞错）。
  if (!(contract.failurePolicy in FAILURE_POLICY_STRICTNESS)) {
    diagnostics.push({
      code: "FAILURE_POLICY_UNREGISTERED",
      message: `层 ${contract.layerId} 的失败策略 ${contract.failurePolicy} 未在策略表中登记。`,
      hint: "任何层都必须声明失败策略；三档（abort / skip-frame / degrade-to-software）之外的自定义值须先走 ADR。",
    });
  }

  // 4.5 层号序：层自身在序中的位置必须与序表一致（防自造层号）。
  if (LAYER_RANK[contract.layerId] === undefined || LAYER_RANK[contract.layerId] <= 0) {
    diagnostics.push({
      code: "PIPELINE_LAYER_ORDER_INVALID",
      message: `层 ${contract.layerId} 的层号非法。`,
      hint: `层号取自 LAYER_RANK（1-5），不可自造；新增层须先在册内立项并登记层号。`,
    });
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) return err(first.code, first.message, first.hint, diagnostics);
  }
  return ok(contract, diagnostics);
}

/**
 * 由序表推导某层的规范邻接（容器层无上游、输出层无下游、中间层前后各一）。
 *
 * 存在的理由：邻接若只靠「双方互指」来校验，就少了一半校验力——
 * 把中间层的 upstream 改成 null 后，它不再声明任何邻居，于是「我认上游、
 * 上游认我」这条对称性校验根本不会触发，断裂静默通过。序表是邻接的
 * 唯一事实源，本函数把该事实显式化，校验才有可对照的基准。
 */
export function baselineAdjacency(layerId: LayerId): { readonly upstream: LayerId | null; readonly downstream: LayerId | null } {
  const idx = LAYER_ORDER.indexOf(layerId);
  if (idx < 0) return { upstream: null, downstream: null };
  const upstream = idx > 0 ? (LAYER_ORDER[idx - 1] ?? null) : null;
  const downstream = idx < LAYER_ORDER.length - 1 ? (LAYER_ORDER[idx + 1] ?? null) : null;
  return { upstream, downstream };
}

/**
 * 断言替换不破坏层间契约（锚点：层替换不破坏相邻层）。
 *
 * 逐字段比对六面：邻居 / 数据语义 / 能力 / 失败策略 / 版本。
 * 任一面漂移即产出 FREEZE_DRIFT 并**拒绝**替换——注意是拒绝而不是告警：
 * 能力被削而邻层不知情，就是解码管线里最难查的一类缺陷。
 */
export function assertReplacementPreservesContract(
  baseline: FrozenLayerContract,
  candidate: FrozenLayerContract,
  fromImpl: string,
  toImpl: string,
): Outcome<ReplacementEvent> {
  const diagnostics: Diagnostic[] = [];

  if (baseline.layerId !== candidate.layerId) {
    return err(
      "FREEZE_DRIFT",
      `替换非法：基线层为 ${baseline.layerId}，候选层为 ${candidate.layerId}。`,
      "替换只换实现不换层位；跨层搬移属于架构修正，须走 ADR（F1201 修正流程）后由本条回改基线。",
    );
  }

  if (baseline.upstream !== candidate.upstream || baseline.downstream !== candidate.downstream) {
    diagnostics.push({
      code: "LAYER_ADJACENCY_MISMATCH",
      message: `替换 ${baseline.layerId}（${fromImpl} → ${toImpl}）改变了邻居关系。`,
      hint: "邻居关系是层间契约的一部分；改邻居即改管线拓扑，须走架构修正而非实现替换。",
    });
  }

  if (baseline.consumes !== candidate.consumes || baseline.produces !== candidate.produces) {
    diagnostics.push({
      code: "LAYER_DATA_CONTRACT_VIOLATION",
      message: `替换 ${baseline.layerId}（${fromImpl} → ${toImpl}）改变了数据语义。`,
      hint: `数据语义必须保持 ${baseline.consumes} → ${baseline.produces}；语义变化即契约破坏，不是替换。`,
    });
  }

  // 能力面：允许削减之外的任何变化都不行（既不容许削减，也不容许自抬）。
  const baseCaps = new Set(baseline.capabilities);
  for (const cap of candidate.capabilities) {
    if (!baseCaps.has(cap)) {
      diagnostics.push({
        code: "LAYER_CAPABILITY_OVERREACH",
        message: `替换 ${baseline.layerId}（${toImpl}）自报了契约未授予的能力位 ${cap}。`,
        hint: "能力位只能由契约授予；需要新能力请先扩基线契约并走 ADR，否则邻层会误以为该能力可用。",
      });
    }
  }
  for (const cap of baseline.capabilities) {
    if (!candidate.capabilities.includes(cap)) {
      diagnostics.push({
        code: "FREEZE_DRIFT",
        message: `替换 ${baseline.layerId}（${fromImpl} → ${toImpl}）削减了契约能力位 ${cap}。`,
        hint: "层替换不得削减能力。需要分档能力请拆成两个实现并各自声明，而不是在同一实现里少报。",
      });
    }
  }

  // 失败策略：可更严不可更松。
  if (FAILURE_POLICY_STRICTNESS[candidate.failurePolicy] < FAILURE_POLICY_STRICTNESS[baseline.failurePolicy]) {
    diagnostics.push({
      code: "FREEZE_DRIFT",
      message: `替换 ${baseline.layerId}（${toImpl}）把失败策略从 ${baseline.failurePolicy} 放松为 ${candidate.failurePolicy}。`,
      hint: `失败策略只能收紧不能放松（严格度序 abort > degrade-to-software > skip-frame）；放松会让上层对失败一无所知。`,
    });
  }

  if (baseline.contractVersion !== candidate.contractVersion) {
    diagnostics.push({
      code: "FREEZE_DRIFT",
      message: `替换 ${baseline.layerId}（${toImpl}）改动了契约版本（${baseline.contractVersion} → ${candidate.contractVersion}）。`,
      hint: `版本变更不是替换动作；走 ADR 后由本条统一升版并同步所有层的基线。`,
    });
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.message, first.hint, diagnostics);
    }
  }

  return ok(
    {
      layerId: baseline.layerId,
      fromImplementationId: fromImpl,
      toImplementationId: toImpl,
      contractPreserved: true,
      diagnostics,
    },
    diagnostics,
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §5 路由位置校验（码流层感知格式 / 解码层执行路由）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 路由决策（由 CGPU-F0104 决策树产出，解码层消费）。 */
export interface RouteDecision {
  /** 目标路径：硬解 / 软解 / 混合。 */
  readonly path: "hardware" | "software" | "hybrid";
  /** 决策理由链（每节点一条，用户可见的决策透明化，F1208 展开）。 */
  readonly reasons: readonly string[];
  /** 拒绝硬解的首个理由（走了软解时非空——「为什么走软解」必须可追溯）。 */
  readonly softwareReason: string | null;
  /** 供应商标识（硬解路径下非空，用于回退率统计）。 */
  readonly vendor: string | null;
}

/** 层对外暴露的能力面（哪些层"看得见"路由决策）。 */
export interface LayerVisibility {
  readonly layerId: LayerId;
  /** 是否能看到帧上下文中的路由决策分区。 */
  readonly seesRouteDecision: boolean;
  /** 是否持有感知到的编解码器格式。 */
  readonly seesCodecFormat: boolean;
  /** 是否执行路由。 */
  readonly executesRoute: boolean;
}

/**
 * 校验路由在层间的位置（锚点：码流层感知格式、解码层执行路由）。
 *
 * 反例（必须失败）：把路由决策放进 L2。那会让 L2 依赖硬件能力，
 * 于是"换解码器实现"变成"改容器解析"——层可替换性当场失效。
 * 该反例被写成断言而非注释，因为它是本条最贵的设计错误。
 */
export function verifyRoutePlacement(assembly: PipelineAssembly): Outcome<readonly LayerVisibility[]> {
  const bag = new DiagBag();
  const byId = new Map<LayerId, LayerRegistration>();
  for (const reg of assembly.layers) {
    if (byId.has(reg.layerId)) {
      bag.push(
        "PIPELINE_LAYER_DUPLICATE",
        `层 ${reg.layerId} 注册了多次（重复实现 ${reg.implementationId}）。`,
        "每层在一条管线里只能挂一个实现；要并列多实现请装配多条管线。",
      );
    }
    byId.set(reg.layerId, reg);
  }

  const perception = byId.get(ROUTE_PERCEPTION_LAYER);
  if (perception === undefined) {
    // 感知位缺席的完整判定在 5.6（含角色核对）；此处只保底一条不重复的码，
    // 避免同一缺失被两条几乎相同的诊断重复报出（告警洪水）。
    bag.push(
      "ROUTE_PERCEPTION_MISSING",
      `路由前置条件不成立：负责格式感知的 ${LAYER_LABEL[ROUTE_PERCEPTION_LAYER]} 未注册。`,
      "码流层必须先在册；没有格式感知就没有路由依据（解码层将无从判断该交给谁解）。",
    );
  } else if (perception.routeRole === "execution") {
    bag.push(
      "ROUTE_MISPLACED",
      `${LAYER_LABEL[ROUTE_PERCEPTION_LAYER]} 声明了执行路由角色。`,
      "码流层只输出感知到的格式；裁决交给解码层，否则 L2 与硬件能力耦合。",
    );
  }

  for (const reg of assembly.layers) {
    // 5.1 路由只能被执行层执行：自报 execution 的层必须就是 L3。
    if (reg.routeRole === "execution" && reg.layerId !== ROUTE_EXECUTION_LAYER) {
      bag.push(
        "ROUTE_MISPLACED",
        `层 ${LAYER_LABEL[reg.layerId]}（${reg.implementationId}）自报执行路由，但路由执行位在 ${LAYER_LABEL[ROUTE_EXECUTION_LAYER]}。`,
        "把路由决策移回解码层；容器/码流层执行路由会让硬件能力泄漏进与硬件无关的层，"+
          "换解码器时被迫改容器解析，层可替换性当场失效。",
      );
    }

    // 5.2 格式感知只能发生在感知层（锚点用词是「感知」，不是「裁决」）。
    if (reg.routeRole === "perception" && reg.layerId !== ROUTE_PERCEPTION_LAYER) {
      bag.push(
        "ROUTE_MISPLACED",
        `层 ${LAYER_LABEL[reg.layerId]} 自报格式感知，但感知位在 ${LAYER_LABEL[ROUTE_PERCEPTION_LAYER]}。`,
        `把格式探测移回码流层；其它层读容器声明即可，不该自行探测码流内容。`,
      );
    }

    // 5.3 感知层不得同时执行路由（锚点明确分工：L2 感知、L3 执行）。
    if (reg.layerId === ROUTE_PERCEPTION_LAYER && reg.routeRole === "execution") {
      bag.push(
        "ROUTE_MISPLACED",
        `${LAYER_LABEL[ROUTE_PERCEPTION_LAYER]} 既做格式感知又执行路由。`,
        "码流层只输出感知到的格式；裁决交给解码层，否则 L2 与硬件能力耦合。",
      );
    }

    // 5.4 执行层必须看得到格式感知结果，否则路由是盲决策。
    if (reg.layerId === ROUTE_EXECUTION_LAYER && reg.routeRole !== "execution") {
      bag.push(
        "ROUTE_MISPLACED",
        `${LAYER_LABEL[ROUTE_EXECUTION_LAYER]} 的角色为 ${reg.routeRole}，未声明执行路由。`,
        "解码层必须显式声明 execution；隐式承担路由会让「路由在哪一层」无法被审计。",
      );
    }
    if (reg.layerId === ROUTE_EXECUTION_LAYER && !reg.declaredCapabilities.includes("decode-software")) {
      bag.push(
        "ROUTE_PERCEPTION_MISSING",
        `${LAYER_LABEL[ROUTE_EXECUTION_LAYER]} 未声明软解能力，路由无回退目的地。`,
        "解码层必须同时具备软解能力位，否则硬解失败时无处可退（F1208 的回退路径失效）。",
      );
    }

    // 5.5 非执行层不得声称看得到路由决策（避免邻层对路径产生错误预期）。
    if (reg.routeRole !== "execution" && reg.layerId === ROUTE_EXECUTION_LAYER) {
      bag.push(
        "ROUTE_MISPLACED",
        `路由决策不应跨出 ${LAYER_LABEL[ROUTE_EXECUTION_LAYER]}，但该层角色为 ${reg.routeRole}。`,
        "路由决策是解码层内部决策；需要跨层共享的是解码结果与元数据，不是决策过程。",
      );
    }
  }

  // 5.6 感知层角色核对（层缺席的判定已在循环前完成，此处只管角色错位）。
  const perceptionReg = assembly.layers.find((r) => r.layerId === ROUTE_PERCEPTION_LAYER);
  if (perceptionReg !== undefined && perceptionReg.routeRole !== "perception") {
    bag.push(
      "ROUTE_PERCEPTION_MISSING",
      `${LAYER_LABEL[ROUTE_PERCEPTION_LAYER]} 的角色为 ${perceptionReg.routeRole}，未承担格式感知。`,
      `把该层角色改为 perception；感知位错位时，路由只能盲猜路径，且"码流层感知格式"这条锚点约束落空。`,
    );
  }

  // 5.7 反向完备：执行位不得空缺。
  const executionReg = assembly.layers.find((r) => r.routeRole === "execution");
  if (assembly.layers.length > 0 && executionReg === undefined) {
    bag.push(
      "ROUTE_MISPLACED",
      "没有任何层声明执行路由。",
      `${LAYER_LABEL[ROUTE_EXECUTION_LAYER]} 必须声明 execution；否则码流层只能解析不能解码，整条管线是死的。`,
    );
  }

  const visibility: readonly LayerVisibility[] = assembly.layers.map((reg) => ({
    layerId: reg.layerId,
    seesRouteDecision: reg.layerId === ROUTE_EXECUTION_LAYER,
    seesCodecFormat: reg.layerId === ROUTE_PERCEPTION_LAYER || reg.layerId === ROUTE_EXECUTION_LAYER,
    executesRoute: reg.routeRole === "execution",
  }));

  const merged = bag.all();
  if (merged.length > 0) {
    const first = merged[0];
    if (first !== undefined) return err(first.code, first.message, first.hint, merged);
  }
  return ok(visibility, merged);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §6 帧上下文（跨层传递的帧元数据）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 帧类型（跨层通用；B 帧的乱序呈现语义依赖它）。 */
export type FrameKind = "I" | "P" | "B";

/**
 * 帧上下文字段分区（锚点：跨层传递的帧元数据）。
 *
 * 每个字段**恰有一个归属层**：谁产生谁写。归属表的治理价值在于把
 * "谁可以改时间戳"这种问题从口头约定变成机器可查的越权检测。
 */
export type FrameFieldSlot =
  /** 轨道级元数据（编解码器标识/宽高/时基）——L1 写，全层读。 */
  | "track-meta"
  /** 帧序号与呈现/解码时间戳——L1 写，L3/L4 读（L3 可写参考帧标注）。 */
  | "timing"
  /** 感知到的编解码器格式——L2 写，L3 读。 */
  | "codec-format"
  /** 路由决策——L3 写，L3 读（不跨出解码层）。 */
  | "route"
  /** 解码产物引用（平面/步幅/DPB 槽位）——L3 写，L4 读。 */
  | "decoded-frame-ref"
  /** 后处理参数（滤波开关/色彩空间目标）——L4 写，L5 读。 */
  | "post-params"
  /** 输出表面句柄——L5 写，出管线后读。 */
  | "surface-handle"
  /** 层延迟账——全层追加写（这是唯一允许多写的分区，见下）。 */
  | "timing-ledger"
  /** 错误账——失败层写，上下游读（错误跨层传播的载体）。 */
  | "failure-ledger";

/** 字段分区的归属层（唯一归属，多写仅 timing-ledger 例外）。 */
export const FRAME_FIELD_OWNER: Readonly<Record<FrameFieldSlot, LayerId>> = {
  "track-meta": "L1-container",
  timing: "L1-container",
  "codec-format": "L2-bitstream",
  route: "L3-decode",
  "decoded-frame-ref": "L3-decode",
  "post-params": "L4-postprocess",
  "surface-handle": "L5-output",
  "timing-ledger": "L1-container", // 归 L1 名下，但由全层追加写（见 APPEND_ONLY_SLOTS）
  "failure-ledger": "L1-container",
};

/** 追加写白名单分区：这些分区设计上允许多层追加（账本类，不是单一状态）。 */
export const APPEND_ONLY_SLOTS: readonly FrameFieldSlot[] = ["timing-ledger", "failure-ledger"];

/** 必填字段分区（缺失即跨层元数据不完整，上层会拿到不完整的时间基准）。 */
export const REQUIRED_FRAME_FIELDS: readonly FrameFieldSlot[] = [
  "track-meta",
  "timing",
  "codec-format",
  "route",
  "decoded-frame-ref",
  "surface-handle",
];

/** 层延迟采样（每层一条；append-only）。 */
export interface LayerTiming {
  readonly layerId: LayerId;
  /** 层内 wall 耗时（毫秒）。 */
  readonly elapsedMs: number;
  /** 层内调用次数（一次解码一帧时为 1；参数集热更新场景可 >1）。 */
  readonly calls: number;
}

/**
 * 层失败记录（锚点：层内错误三要素 + 上层可见）。
 * 三要素分别是：发生了什么（what）/ 为什么（cause）/ 下一步怎么办（action）。
 */
export interface LayerFailure {
  readonly layerId: LayerId;
  /** 发生了什么（人话）。 */
  readonly what: string;
  /** 为什么（根因链或判定依据）。 */
  readonly cause: string;
  /** 下一步怎么办（可操作：换实现/降级/中止/上报）。 */
  readonly action: string;
  /** 当时的失败层上下文快照（供上层判因，避免上层瞎猜）。 */
  readonly contextSnapshot: string;
  /** 该失败被传播到的层（上游链；空表示已中止未继续传播）。 */
  readonly propagatedTo: readonly LayerId[];
}

/**
 * 帧上下文：五层之间唯一的传递载体。
 *
 * 不可变 + 分区写入：`writeField` 返回**新对象**而非就地改。
 * 理由：B 帧重排时 L3 可能晚于 L5 消费同一帧元数据，就地写会让输出层
 * 读到半写的时间戳——这类 bug 只在特定帧序下偶发，极难定位。
 */
export interface FrameContext {
  /** 帧序号（管线内单调递增，用于账本对账与丢帧检测）。 */
  readonly frameIndex: number;
  /** 帧类型。 */
  readonly kind: FrameKind;
  /** 呈现时间戳（90kHz 或时基单位；语义见 timing.scale）。 */
  readonly pts: number;
  /** 解码时间戳。 */
  readonly dts: number;
  /** 时间基（每秒刻度数）。 */
  readonly timebaseScale: number;
  /** 已写入分区（键为分区名）。 */
  readonly fields: ReadonlyMap<FrameFieldSlot, unknown>;
  /** 层延迟账（append-only）。 */
  readonly timings: readonly LayerTiming[];
  /** 错误账（append-only）。 */
  readonly failures: readonly LayerFailure[];
}

/** 新建帧上下文（唯一构造入口，保证 timebase 合法）。 */
export function createFrameContext(frameIndex: number, kind: FrameKind, timebaseScale: number): Outcome<FrameContext> {
  if (!(timebaseScale > 0) || !Number.isFinite(timebaseScale)) {
    return err(
      "FRAME_TIMEBASE_INCONSISTENT",
      `帧 ${frameIndex} 的时基 ${timebaseScale} 非法（须为正有限数）。`,
      "时基来自容器层 mdhd/tb 声明；若为 0 或 NaN 说明容器解析已错，先修 L1 再解帧。",
    );
  }
  return ok({
    frameIndex,
    kind,
    pts: 0,
    dts: 0,
    timebaseScale,
    fields: new Map<FrameFieldSlot, unknown>(),
    timings: [],
    failures: [],
  });
}

/**
 * 分区写入（越权即拒绝，返回原对象不改动）。
 * 归属层之外的写入产出 FRAME_FIELD_ACCESS_VIOLATION 并附可操作提示。
 */
export function writeField(
  ctx: FrameContext,
  slot: FrameFieldSlot,
  value: unknown,
  byLayer: LayerId,
): Outcome<FrameContext> {
  const owner = FRAME_FIELD_OWNER[slot];
  const appendOnly = APPEND_ONLY_SLOTS.includes(slot);
  if (byLayer !== owner && !(appendOnly && isAnyLayer(byLayer))) {
    return err(
      "FRAME_FIELD_ACCESS_VIOLATION",
      `层 ${byLayer} 试图写入分区 ${slot}，该分区归属 ${owner}。`,
      `只有归属层可写 ${slot}。若确需跨层改写，先在 FRAME_FIELD_OWNER 里改归属并走 ADR——不要在实现里绕过。`,
    );
  }
  const next = new Map(ctx.fields);
  next.set(slot, value);
  return ok({ ...ctx, fields: next });
}

/** 层合法性守卫（只接受已登记层，挡掉自造层名）。 */
function isAnyLayer(layerId: LayerId): boolean {
  return LAYER_RANK[layerId] !== undefined && LAYER_RANK[layerId] > 0;
}

/**
 * 校验帧上下文完整性（必填分区齐备 + 时基关系与帧类型自洽）。
 *
 * 时基自洽判据：B 帧的 PTS 必须晚于其 DTS（乱序解码的必然结果），
 * I/P 帧则应相等或 PTS≥DTS。若 I/P 帧出现 PTS<DTS，说明容器层的
 * 重排序缓冲有 bug——这类错在解码层表现为「画面偶尔倒退」，追到容器层
 * 才是真因。
 */
export function verifyFrameContext(ctx: FrameContext): Outcome<FrameContext> {
  const diagnostics: Diagnostic[] = [];

  // 诊断排序按「病因先于症状」。时序矛盾指向容器层重排序缓冲的根因，
  // 而必填分区缺失往往只是该根因的下游表现（某层因此提前失败没写分区）。
  // 先报病因，调用方才不会去补分区——补了也会被下一个症状打回。
  if (ctx.kind === "B" && ctx.pts < ctx.dts) {
    diagnostics.push({
      code: "FRAME_TIMEBASE_INCONSISTENT",
      message: `帧 ${ctx.frameIndex}（B 帧）PTS=${ctx.pts} 早于 DTS=${ctx.dts}，与乱序解码语义矛盾。`,
      hint: "B 帧必然先解码后呈现（PTS>DTS）。若 PTS<DTS，根因在容器层重排序缓冲（F1202），不在解码层。",
    });
  }
  if (ctx.kind !== "B" && ctx.dts > ctx.pts) {
    diagnostics.push({
      code: "FRAME_TIMEBASE_INCONSISTENT",
      message: `帧 ${ctx.frameIndex}（${ctx.kind} 帧）DTS=${ctx.dts} 晚于 PTS=${ctx.pts}，I/P 帧不应晚解晚呈。`,
      hint: "检查容器层是否把重排序缓冲误用于 I/P 帧；I/P 帧的显示顺序与解码顺序应一致或 PTS≥DTS。",
    });
  }

  const missing = REQUIRED_FRAME_FIELDS.filter((slot) => !ctx.fields.has(slot));
  if (missing.length > 0) {
    diagnostics.push({
      code: "FRAME_CONTEXT_FIELD_MISSING",
      message: `帧 ${ctx.frameIndex} 缺必填分区：${missing.join("、")}。`,
      hint: "必填分区由 LAYER_ORDER 各层依次写入；缺位说明某层被跳过或提前失败，先查层失败账。",
    });
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) return err(first.code, first.message, first.hint, diagnostics);
  }
  return ok(ctx, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §7 层失败与跨层传播（零静默的红线所在）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 传播结果：失败被处置后的管线去向（供调用方决定继续/中止/降级）。 */
export interface FailureOutcome {
  /** 处置后的帧上下文（失败已挂账；skip 时账仍在，上层可见）。 */
  readonly ctx: FrameContext;
  /** 处置动作。 */
  readonly action: "aborted" | "skipped" | "degraded" | "recovered";
  /** 沿上游链可见的层（判据：上层可见）。 */
  readonly upstreamVisible: readonly LayerId[];
  /** 全部诊断（含失败三要素本身）。 */
  readonly diagnostics: readonly Diagnostic[];
}

/** 失败处置策略的解析结果（用于显式声明本层选择哪种处置）。 */
export type FailureDisposition = "abort" | "skip" | "degrade" | "recover";

/**
 * 传播层失败（锚点：层失败 → 层内错误三要素 + 上层可见）。
 *
 * 四条不变量，本函数逐条兑现：
 *   I1 失败必带三要素（what/cause/action），缺任一即 LAYER_FAILURE_SILENT；
 *   I2 处置策略不得比契约声明的更松（否则邻层对失败强度预估错误）；
 *   I3 失败挂到帧上下文后，**上游所有层**都能读到（上层可见判据）；
 *   I4 处置动作必须显式返回，绝不静默返回"看起来成功"。
 *
 * @param upstreamChain 从失败层往上游的层序（如 L3 失败 → ["L2-bitstream","L1-container"]）
 */
export function propagateFailure(
  ctx: FrameContext,
  layerId: LayerId,
  failure: Omit<LayerFailure, "layerId" | "propagatedTo">,
  disposition: FailureDisposition,
  contractPolicy: FailurePolicy,
  upstreamChain: readonly LayerId[],
): Outcome<FailureOutcome> {
  const diagnostics: Diagnostic[] = [];

  // I1 三要素齐备。
  const what = failure.what.trim();
  const cause = failure.cause.trim();
  const action = failure.action.trim();
  if (what.length === 0 || cause.length === 0 || action.length === 0) {
    diagnostics.push({
      code: "LAYER_FAILURE_SILENT",
      message: `层 ${layerId} 的失败记录三要素不完整（what:${what.length > 0 ? "有" : "缺"} cause:${cause.length > 0 ? "有" : "缺"} action:${action.length > 0 ? "有" : "缺"}）。`,
      hint: "任何层失败都必须写全三要素：发生了什么 / 为什么 / 下一步怎么办。裸 error 或空 catch 即视为吞错，按缺陷处理。",
    });
  }

  // I2 处置不得比契约更松：契约 skip-frame 允许 recover/skip，不允许 abort 后假装继续。
  //
  // 标尺必须同量纲，否则「等值即放行」会变成放松通道。
  // 反例（本条修复的真缺陷）：契约 abort 严格度 3，而处置 recover 严格度也是 3，
  // 判定 3 < 3 为假 ⇒ 契约要求「中止整条管线」的实现却用「恢复后继续」被放行，
  // 上层以为会中止，实际拿到的是一帧继续跑的输出——处置声明与实际行为反向。
  //
  // 正解：把处置折算到契约三档上再比。折算规则——
  //   abort   → 就是 abort（最严，一一对应）；
  //   recover → 视作 skip-frame（恢复后跳过坏帧，不中止管线）；
  //   degrade → 视作 degrade-to-software（降级重试，不中止）；
  //   skip    → 视作 skip-frame。
  // 折算后契约 abort(3) 对处置 recover(1) 判定 1 < 3 ⇒ 拒绝，放松被拦。
  const dispositionToPolicy: Readonly<Record<FailureDisposition, FailurePolicy>> = {
    abort: "abort",
    recover: "skip-frame",
    degrade: "degrade-to-software",
    skip: "skip-frame",
  };
  const effectivePolicy = dispositionToPolicy[disposition];
  if (FAILURE_POLICY_STRICTNESS[effectivePolicy] < FAILURE_POLICY_STRICTNESS[contractPolicy]) {
    // 契约比处置更严（例如契约 abort 而处置 recover）——这是危险的放松，直接拒绝。
    diagnostics.push({
      code: "FAILURE_POLICY_UNREGISTERED",
      message: `层 ${layerId} 的处置 ${disposition}（折算为 ${effectivePolicy}）比契约策略 ${contractPolicy} 更松。`,
      hint: `处置强度不得低于契约（严格度序 abort > degrade-to-software > skip-frame）；要么按契约中止，要么先收紧契约。`,
    });
  }

  // I3 上游可见：失败必须出现在失败层之上的**每一层**视野里。
  //     判据用「应见集合 − 实见集合」，而不是「实见集合是否为空」——
  //     后者只能挡住全不传播，挡不住「传给了 L1 却漏了 L2」这种半传播，
  //     而半传播正是最难排查的一类（上层看起来知情，实际在盲决策）。
  const expectedUpstream = LAYER_ORDER.filter((l) => LAYER_RANK[l] < LAYER_RANK[layerId]);
  const seenUpstream = new Set(upstreamChain);
  const missingVisibility = expectedUpstream.filter((l) => !seenUpstream.has(l));
  if (disposition !== "abort" && missingVisibility.length > 0) {
    diagnostics.push({
      code: "FAILURE_NOT_UPPSTREAM_VISIBLE",
      message:
        missingVisibility.length === expectedUpstream.length
          ? `层 ${layerId} 的失败未向任何上游层传播（应见 ${expectedUpstream.length} 层，实见 0 层）。`
          : `层 ${layerId} 的失败未传播到上游层：${missingVisibility.map((l) => LAYER_LABEL[l]).join("、")}。`,
      hint:
        "失败必须跨层传播到失败层之上的每一层（上层可见是判据）；" +
        "只在本层记一笔等于静默吞错，补全上游链后重试。",
    });
  }

  const recorded: LayerFailure = {
    layerId,
    what: what.length > 0 ? what : "<三要素缺失，按吞错处理>",
    cause: cause.length > 0 ? cause : "<三要素缺失，按吞错处理>",
    action: action.length > 0 ? action : "<三要素缺失，按吞错处理>",
    contextSnapshot: failure.contextSnapshot,
    propagatedTo: disposition === "abort" ? [] : [...upstreamChain],
  };

  const nextCtx: FrameContext = { ...ctx, failures: [...ctx.failures, recorded] };

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.message, first.hint, diagnostics);
    }
  }

  const actionOut: FailureOutcome["action"] =
    disposition === "abort"
      ? "aborted"
      : disposition === "skip"
        ? "skipped"
        : disposition === "degrade"
          ? "degraded"
          : "recovered";

  diagnostics.push({
    code: "CRITERION_SELFCHECK_FAILED",
    message: `层 ${layerId} 失败已记录：${recorded.what}`,
    hint: recorded.action,
  });

  return ok({ ctx: nextCtx, action: actionOut, upstreamVisible: recorded.propagatedTo, diagnostics }, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §8 管线延迟分解（层延迟超标 → 定位到层）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 延迟账（一帧的五层耗时）。 */
export interface LatencyLedger {
  /** 帧序号。 */
  readonly frameIndex: number;
  /** 逐层耗时（顺序即执行序）。 */
  readonly samples: readonly LayerTiming[];
}

/** 延迟分解结果：占比 + 瓶颈层裁决。 */
export interface LatencyDecomposition {
  readonly frameIndex: number;
  /** 总耗时。 */
  readonly totalMs: number;
  /** 帧预算。 */
  readonly budgetMs: number;
  /** 是否超标。 */
  readonly breached: boolean;
  /** 逐层占比（0-1，和为 1；无耗时层不参与）。 */
  readonly shareByLayer: ReadonlyMap<LayerId, number>;
  /** 瓶颈层（耗时占比最高者）；样本缺失时为 null 且带诊断。 */
  readonly bottleneckLayer: LayerId | null;
  /** 是否已定位到层（判据：超标必定位）。 */
  readonly attributed: boolean;
  readonly diagnostics: readonly Diagnostic[];
}

/**
 * 帧预算闸门：预算非法即返回诊断，合规则返回 null。
 *
 * 存在的理由：预算在本模块有两处消费点——decomposeLatency 判超标、freeze
 * 记架构凭据。前者自己守正有限，后者曾完全不校验，于是非法预算能进快照。
 * 把校验抽成一个函数供两处共用，杜绝「一处守一处漏」。
 *
 * 上界同样守：预算宽到离谱（比如 1e9ms）时超标永不触发，门禁形同虚设，
 * 与其等到超标判据长期沉默，不如在入口就拒绝。
 */
export const FRAME_BUDGET_MIN_MS = 0.1;
export const FRAME_BUDGET_MAX_MS = 1000;

export function inspectFrameBudget(budgetMs: number): Diagnostic | null {
  if (typeof budgetMs !== "number" || Number.isNaN(budgetMs) || !Number.isFinite(budgetMs) || budgetMs <= 0) {
    return {
      code: "FRAME_BUDGET_INVALID",
      message: `帧预算 ${String(budgetMs)} 非法（须为正有限数）。`,
      hint: "帧预算来自播放时钟（33.3ms@30fps / 16.7ms@60fps）；未声明或非法预算则无法做超标归因。",
    };
  }
  if (budgetMs < FRAME_BUDGET_MIN_MS) {
    return {
      code: "FRAME_BUDGET_INVALID",
      message: `帧预算 ${budgetMs}ms 低于下限 ${FRAME_BUDGET_MIN_MS}ms。`,
      hint: "小于 0.1ms 的预算不可能被任何真实解码路径满足，多为帧率单位换算错误（毫秒/微秒混用）。",
    };
  }
  if (budgetMs > FRAME_BUDGET_MAX_MS) {
    return {
      code: "FRAME_BUDGET_INVALID",
      message: `帧预算 ${budgetMs}ms 超过上限 ${FRAME_BUDGET_MAX_MS}ms。`,
      hint: "预算过宽会让超标永不触发，延迟归因判据形同虚设；按真实帧间隔声明预算（60fps≈16.7ms）。",
    };
  }
  return null;
}

/**
 * 延迟分解（锚点：每层耗时；层延迟超标 → 定位到层）。
 *
 * 三条纪律：
 *   D1 缺层样本即产出诊断——「某层没记耗时」不等于「该层不耗时」，
 *      把它当 0 会让分解表看起来完整而实则失真；
 *   D2 超标必须归因到具体层，产出 attribution 而非一句「解码慢」；
 *   D3 归因基于占比而非绝对值——4K 下每层都慢，但真正该优化的是占比最高者。
 */
export function decomposeLatency(ledger: LatencyLedger, budgetMs: number): Outcome<LatencyDecomposition> {
  const diagnostics: Diagnostic[] = [];

  // D0 预算合法性：不合法即无法判超标（与入口闸门共用同一把尺）。
  const budgetProblem = inspectFrameBudget(budgetMs);
  if (budgetProblem !== null) return err(budgetProblem.code, budgetProblem.message, budgetProblem.hint);

  // 记过的层不允许重复（重复即账被双写，分解会翻倍）。
  const seen = new Set<LayerId>();
  for (const s of ledger.samples) {
    if (seen.has(s.layerId)) {
      diagnostics.push({
        code: "LATENCY_SAMPLE_MISSING",
        message: `层 ${s.layerId} 在帧 ${ledger.frameIndex} 的延迟账中重复出现。`,
        hint: "append-only 账本每层每帧只应有一条；重复说明调用侧重复记账，先修调用侧。",
      });
    }
    if (!(s.elapsedMs >= 0) || !Number.isFinite(s.elapsedMs)) {
      diagnostics.push({
        code: "LATENCY_CLOCK_NON_MONOTONIC",
        message: `层 ${s.layerId} 的耗时 ${s.elapsedMs} 非法（负数或非有限）。`,
        hint: "耗时须为单调时钟的差值；出现负值说明时钟源被重置或跨核取时不同源。",
      });
    }
    seen.add(s.layerId);
  }

  // D1 缺层样本诊断（不阻断：缺层时占比按已有层归一，但必须显性告知）。
  const missing = LAYER_ORDER.filter((l) => !seen.has(l));
  for (const l of missing) {
    diagnostics.push({
      code: "LATENCY_SAMPLE_MISSING",
      message: `层 ${l} 未在帧 ${ledger.frameIndex} 的延迟账中留样。`,
      hint: `补齐 ${l} 的耗时记账；缺层的分解表会把该层开销摊到邻层头上，导致优化方向错误。`,
    });
  }

  const totalMs = ledger.samples.reduce((sum, s) => sum + Math.max(0, s.elapsedMs), 0);
  const shareByLayer = new Map<LayerId, number>();
  let bottleneck: LayerId | null = null;
  let bottleneckMs = -1;
  if (totalMs > 0) {
    for (const s of ledger.samples) {
      const v = Math.max(0, s.elapsedMs) / totalMs;
      shareByLayer.set(s.layerId, v);
      if (s.elapsedMs > bottleneckMs) {
        bottleneckMs = s.elapsedMs;
        bottleneck = s.layerId;
      }
    }
  } else {
    // 零耗时账本身不可信（全 0 通常意味着计时器没跑）。
    diagnostics.push({
      code: "LATENCY_CLOCK_NON_MONOTONIC",
      message: `帧 ${ledger.frameIndex} 的总耗时为 0。`,
      hint: "全 0 通常意味着未启动计时或时钟分辨率不足；分解表此时无意义，先修计时。",
    });
  }

  const breached = totalMs > budgetMs;
  const attributed = breached && bottleneck !== null;

  // D2 超标必须归因。
  if (breached && bottleneck === null) {
    diagnostics.push({
      code: "LATENCY_BREACH_UNATTRIBUTED",
      message: `帧 ${ledger.frameIndex} 超预算（${totalMs.toFixed(2)}ms > ${budgetMs}ms）但无法归因到层。`,
      hint: "无样本即无归因；补齐五层耗时记账后再判瓶颈，不要凭感觉优化。",
    });
  }

  if (breached && bottleneck !== null) {
    const share = (shareByLayer.get(bottleneck) ?? 0) * 100;
    diagnostics.push({
      code: "LATENCY_BREACH_UNATTRIBUTED",
      message: `帧 ${ledger.frameIndex} 超预算（${totalMs.toFixed(2)}ms > ${budgetMs}ms），瓶颈层为 ${bottleneck}（占比 ${share.toFixed(1)}%）。`,
      hint: `按层优化而非全局提速：${LAYER_LABEL[bottleneck]} 占比最高，${layerOptimizationHint(bottleneck)}`,
    });
  }

  return ok(
    {
      frameIndex: ledger.frameIndex,
      totalMs,
      budgetMs,
      breached,
      shareByLayer,
      bottleneckLayer: bottleneck,
      attributed,
      diagnostics,
    },
    diagnostics,
  );
}

/** 按层给出可操作的优化方向（诊断的 hint 必须是动作，不是套话）。 */
function layerOptimizationHint(layerId: LayerId): string {
  switch (layerId) {
    case "L1-container":
      return "容器层占优通常说明每帧都在重复解析 Box 头——把轨道/样本表预解析并缓存（F1202）。";
    case "L2-bitstream":
      return "码流层占优通常说明参数集被逐帧重解析——参数集只在变更时热更新一次，其余帧走快路径（F1204）。";
    case "L3-decode":
      return "解码层占优是正常现象，优化方向是硬解路由（F1208）+ 多线程（F1212），不是软解微调。";
    case "L4-postprocess":
      return "后处理层占优优先把色彩空间转换交给 GPU（F0098 联动），CPU 逐像素转换是性能红线。";
    case "L5-output":
      return "输出层占优说明存在多余像素拷贝——核查零拷贝链（F1209），一次拷贝即非零拷贝即缺陷。";
    default: {
      const exhaustive: never = layerId;
      return String(exhaustive);
    }
  }
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §9 五层架构完整性与装配校验
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 架构完整性报告（判据一的可执行形态）。 */
export interface ArchitectureAudit {
  readonly layerCount: number;
  readonly expectedLayerCount: number;
  /** 每层是否都有责任条目与至少一个实现条目。 */
  readonly everyLayerOwned: boolean;
  /** 每层是否都有基线契约与注册。 */
  readonly everyLayerContracted: boolean;
  /** 是否五层齐备且层序单调。 */
  readonly complete: boolean;
  readonly diagnostics: readonly Diagnostic[];
}

/**
 * 五层架构完整性审计（判据一：五层架构完整）。
 * 齐备性、序单调、每层有主（责任条目）、每层可替换（≥1 实现）四条同时成立才算完整。
 */
export function auditArchitecture(assembly: PipelineAssembly): Outcome<ArchitectureAudit> {
  const bag = new DiagBag();
  const byId = new Map<LayerId, LayerRegistration>();
  for (const reg of assembly.layers) {
    if (byId.has(reg.layerId)) {
      bag.push(
        "PIPELINE_LAYER_DUPLICATE",
        `层 ${reg.layerId} 在装配中重复注册。`,
        "每层一条实现；并列多实现请装配多条管线并按路由择一（F1208）。",
      );
    }
    byId.set(reg.layerId, reg);
  }

  // 9.1 五层齐备。
  for (const l of LAYER_ORDER) {
    if (!byId.has(l)) {
      bag.push(
        "PIPELINE_LAYER_MISSING",
        `五层架构缺失 ${LAYER_LABEL[l]}（${l}）。`,
        `补齐该层的实现（F${LAYER_IMPLEMENTATION_ENTRIES[l].join("/")}）；缺层会让帧在管线上"凭空消失"。`,
      );
    }
  }

  // 9.2 多余层（不在册的层号 = 无主层）。
  for (const reg of assembly.layers) {
    if (!LAYER_ORDER.includes(reg.layerId)) {
      bag.push(
        "PIPELINE_LAYER_UNKNOWN",
        `装配了不在五层册内的层 ${reg.layerId}。`,
        "层集合已冻结为五层；新增层须先在册内立项并走架构修正，否则邻层无契约可依。",
      );
    }
  }

  // 9.3 每层有主 + 每层可替换（实现条目 ≥1）。
  let everyLayerOwned = true;
  let everyLayerContracted = true;
  for (const l of LAYER_ORDER) {
    const owner = LAYER_OWNER_ENTRY[l];
    const impls = LAYER_IMPLEMENTATION_ENTRIES[l];
    if (owner === undefined || impls === undefined || impls.length === 0) {
      everyLayerOwned = false;
      bag.push(
        "LAYER_CAPABILITY_UNDECLARED",
        `层 ${LAYER_LABEL[l]} 没有责任条目或没有可替换实现。`,
        "无主层即无人实现也无人验收；请在册内指定责任条目并至少登记一个实现。",
      );
    }
    if (byId.has(l) && byId.get(l)?.contract === undefined) {
      everyLayerContracted = false;
      bag.push(
        "CONTRACT_BASELINE_MISSING",
        `层 ${LAYER_LABEL[l]} 未挂载冻结契约。`,
        "无契约的层无法参与替换校验；挂载基线契约后再装配。",
      );
    }
  }

  // 9.4 层序单调（按 LAYER_RANK 排序后应与装配序一致）。
  const ranks = assembly.layers.map((r) => LAYER_RANK[r.layerId] ?? Number.MAX_SAFE_INTEGER);
  let orderOk = true;
  for (let i = 1; i < ranks.length; i++) {
    const prev = ranks[i - 1];
    const cur = ranks[i];
    if (prev !== undefined && cur !== undefined && cur <= prev) {
      orderOk = false;
      bag.push(
        "PIPELINE_LAYER_ORDER_INVALID",
        `层序乱序：第 ${i} 位层号 ${cur} 不大于前位 ${prev}。`,
        "按 LAYER_ORDER（容器→码流→解码→后处理→输出）装配；乱序会让帧以错误顺序消费。",
      );
      break;
    }
  }

  // 9.5 能力声明齐备（实现须声明契约授予的全部能力，否则邻层会误判可用性）。
  for (const reg of assembly.layers) {
    const declared = new Set(reg.declaredCapabilities);
    for (const cap of reg.contract.capabilities) {
      if (!declared.has(cap)) {
        bag.push(
          "LAYER_CAPABILITY_UNDECLARED",
          `层 ${LAYER_LABEL[reg.layerId]} 的实现 ${reg.implementationId} 未声明能力位 ${cap}。`,
          "实现须如实自报能力；漏报会让替换校验与路由判断失准，宁可报多不报少。",
        );
      }
    }
    for (const cap of reg.declaredCapabilities) {
      if (!reg.contract.capabilities.includes(cap)) {
        bag.push(
          "LAYER_CAPABILITY_OVERREACH",
          `层 ${LAYER_LABEL[reg.layerId]} 的实现 ${reg.implementationId} 自报了未授予能力 ${cap}。`,
          "去掉越权自报，或先扩基线契约并走 ADR。",
        );
      }
    }
  }

  const merged = bag.all();
  const complete = byId.size === LAYER_ORDER.length && orderOk && merged.length === 0;
  if (!complete && merged.length > 0) {
    const first = merged[0];
    if (first !== undefined) {
      return err(first.code, first.message, first.hint, merged);
    }
  }
  return ok(
    {
      layerCount: byId.size,
      expectedLayerCount: LAYER_ORDER.length,
      everyLayerOwned,
      everyLayerContracted,
      complete,
      diagnostics: merged,
    },
    merged,
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §10 架构冻结快照与漂移检测
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 架构冻结快照：G 域开工的机器可读凭据。 */
export interface ArchitectureFreeze {
  readonly domainTag: "VE-G";
  readonly firstEntryId: number;
  readonly lastEntryId: number;
  /** 五层序快照。 */
  readonly layers: readonly LayerId[];
  /** 层间契约快照（逐层）。 */
  readonly contracts: readonly FrozenLayerContract[];
  /** 路由位置快照。 */
  readonly perceptionLayer: LayerId;
  readonly executionLayer: LayerId;
  /** 帧预算。 */
  readonly frameBudgetMs: number;
  readonly frozenAt: number;
}

/**
 * 冻结指纹（FNV-1a 32 位，短、稳定、无依赖，非安全用途）。
 *
 * 集合语义部分先排序：能力位是集合，键序只反映构造顺序（字面量 vs Object.keys
 * 推导），不反映架构语义。不排序会让同一份声明经不同构造路径得到不同指纹，
 * 漂移检测将大量误报——那是守卫失效，不是架构真的变了。
 */
export function freezeFingerprint(freeze: ArchitectureFreeze): string {
  const parts = [
    freeze.domainTag,
    `${freeze.firstEntryId}-${freeze.lastEntryId}`,
    freeze.layers.join("→"),
    freeze.contracts
      .map((c) =>
        [
          c.layerId,
          String(c.upstream),
          String(c.downstream),
          c.consumes,
          c.produces,
          [...c.capabilities].sort().join("+"),
          c.failurePolicy,
          c.contractVersion,
        ].join(":"),
      )
      .join("|"),
    `${freeze.perceptionLayer}/${freeze.executionLayer}`,
    String(freeze.frameBudgetMs),
  ];
  let hash = 0x811c9dc5;
  for (const part of parts) {
    for (let i = 0; i < part.length; i++) {
      hash ^= part.charCodeAt(i);
      hash = Math.imul(hash, 0x01000193) >>> 0;
    }
    hash ^= 0x2f;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, "0");
}

/** 漂移检测：当前声明指纹与冻结指纹不符即告警（架构被静默改动是严重问题）。 */
export function detectDrift(frozen: ArchitectureFreeze, current: ArchitectureFreeze): Outcome<ArchitectureFreeze> {
  const fpFrozen = freezeFingerprint(frozen);
  const fpCurrent = freezeFingerprint(current);
  if (fpFrozen !== fpCurrent) {
    return err(
      "FREEZE_DRIFT",
      `架构漂移：冻结指纹 ${fpFrozen} ≠ 当前指纹 ${fpCurrent}。`,
      "架构声明已被改动但未走修正流程回改本条；补修正记录或还原至冻结态，二选一。",
    );
  }
  return ok(frozen);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §11 G 域开工编排（闸门序列）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** G 域开工结果。 */
export interface DomainKickoff {
  readonly freeze: ArchitectureFreeze;
  readonly fingerprint: string;
  readonly visibility: readonly LayerVisibility[];
  readonly audit: ArchitectureAudit;
  readonly diagnostics: readonly Diagnostic[];
}

/**
 * 宣告 G 域开工。
 *
 * 闸门序列（顺序即依赖顺序，前闸不过后闸无意义）：
 *   1. 五层架构完整性审计（硬闸）；
 *   2. 层间契约形状校验（硬闸：邻居对称 + 数据衔接 + 版本 + 策略登记）；
 *   3. 路由位置校验（硬闸：码流层感知、解码层执行）；
 *   4. 冻结快照与指纹产出。
 *
 * @param now 冻结时间戳（调用方注入，本模块不读时钟，保持可测试与可重放）。
 */
export function openDomain(assembly: PipelineAssembly, now: number): Outcome<DomainKickoff> {
  // 闸门 0：帧预算合法性。
  //
  // 存在的理由：decomposeLatency 自己守正有限，但冻结快照是它的上游——
  // 若这里不拦，0/负/NaN/无穷预算会一路写进 freeze.fingerprint，
  // 于是「超标必归因到层」这条判据在正式运行时永远不成立（NaN 比较恒假），
  // 而指纹看上去仍是合法架构。预算失真必须止于入口。
  const budgetProblem = inspectFrameBudget(assembly.frameBudgetMs);
  if (budgetProblem !== null) return err(budgetProblem.code, budgetProblem.message, budgetProblem.hint);

  // 闸门 1：五层齐备与装配正确性。
  const audit = auditArchitecture(assembly);
  if (!audit.ok) return audit;

  // 闸门 2：契约形状（逐层互相校验，因此放在装配校验之后）。
  const baselines = buildBaselineContracts();
  const contractDiagnostics: Diagnostic[] = [];
  for (const reg of assembly.layers) {
    const shape = verifyContractShape(reg.contract, baselines);
    contractDiagnostics.push(...shape.diagnostics);
    if (!shape.ok) return shape;
  }

  // 闸门 3：路由位置。
  const route = verifyRoutePlacement(assembly);
  if (!route.ok) return route;

  const freeze: ArchitectureFreeze = {
    domainTag: G_DOMAIN.tag,
    firstEntryId: G_DOMAIN.firstEntryId,
    lastEntryId: G_DOMAIN.lastEntryId,
    layers: [...LAYER_ORDER],
    contracts: LAYER_ORDER.map((l) => {
      const c = baselines.get(l);
      return c !== undefined ? { ...c } : ({ layerId: l } as FrozenLayerContract);
    }),
    perceptionLayer: ROUTE_PERCEPTION_LAYER,
    executionLayer: ROUTE_EXECUTION_LAYER,
    frameBudgetMs: assembly.frameBudgetMs,
    frozenAt: now,
  };

  const diagnostics = [...audit.diagnostics, ...contractDiagnostics, ...route.diagnostics];
  return ok(
    { freeze, fingerprint: freezeFingerprint(freeze), visibility: route.value, audit: audit.value, diagnostics },
    diagnostics,
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §12 判据自检（不靠人读代码确认，靠断言输出）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 自检结果项：每项对应判据的一条，独立可定位。 */
export interface SelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 判据一自检：五层齐备 + 层序单调 + 每层有主 + 每层有实现 + 每层有契约。 */
export function selfCheckFiveLayers(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const registered = LAYER_ORDER;

  out.push({
    name: "layers-five-registered",
    pass: registered.length === 5,
    detail: `五层齐备数 = ${registered.length}（期望 5：容器/码流/解码/后处理/输出）`,
  });

  const ranks = registered.map((l) => LAYER_RANK[l] ?? -1);
  let monotonic = true;
  for (let i = 1; i < ranks.length; i++) {
    const prev = ranks[i - 1];
    const cur = ranks[i];
    if (prev !== undefined && cur !== undefined && cur <= prev) monotonic = false;
  }
  out.push({
    name: "layers-order-monotonic",
    pass: monotonic,
    detail: monotonic ? "层号严格递增（容器→码流→解码→后处理→输出）" : "层号非单调，帧将以错误顺序被消费",
  });

  let owned = 0;
  let impled = 0;
  let contracted = 0;
  const baselines = buildBaselineContracts();
  for (const l of LAYER_ORDER) {
    if (LAYER_OWNER_ENTRY[l] !== undefined) owned += 1;
    const impls = LAYER_IMPLEMENTATION_ENTRIES[l];
    if (impls !== undefined && impls.length > 0) impled += 1;
    if (baselines.has(l)) contracted += 1;
  }
  out.push({
    name: "layers-every-owned",
    pass: owned === LAYER_ORDER.length,
    detail: `有责任条目的层 = ${owned}/${LAYER_ORDER.length}（无主层无人验收）`,
  });
  out.push({
    name: "layers-every-replaceable",
    pass: impled === LAYER_ORDER.length,
    detail: `有可替换实现的层 = ${impled}/${LAYER_ORDER.length}（实现数 ≥1 是可替换的前提）`,
  });
  out.push({
    name: "layers-every-contracted",
    pass: contracted === LAYER_ORDER.length,
    detail: `有基线契约的层 = ${contracted}/${LAYER_ORDER.length}`,
  });

  return out;
}

/** 判据二自检：可替换断言（合法替换通过；削能力/松策略/改语义/动邻居全部拦截）。 */
export function selfCheckReplaceability(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const baselines = buildBaselineContracts();

  // 正例：同契约换实现（MP4 → MKV），必须通过。
  const l1 = baselines.get("L1-container");
  const l3 = baselines.get("L3-decode");
  if (l1 === undefined || l3 === undefined) {
    out.push({ name: "replace-legal-accepted", pass: false, detail: "基线契约缺 L1/L3，无法验证替换" });
    return out;
  }

  const legal = assertReplacementPreservesContract(l1, { ...l1 }, "mp4-iso-bmff", "mkv-ebml");
  out.push({
    name: "replace-legal-accepted",
    pass: legal.ok,
    detail: legal.ok
      ? "MP4 → MKV 同契约替换被接受（层替换不破坏相邻层成立）"
      : `合法替换被拒：${legal.message}`,
  });

  // 反例 1：削减能力位（EBML 换成只支持 ISO BMFF 的实现）。
  const cut = assertReplacementPreservesContract(l1, { ...l1, capabilities: ["container-isobmff"] }, "mp4-iso-bmff", "isobmff-only");
  out.push({
    name: "replace-capability-cut-rejected",
    pass: !cut.ok && cut.code === "FREEZE_DRIFT",
    detail: cut.ok ? "削减能力的替换被接受（邻层会误判 MKV 可用）" : `削减能力被拦截（${cut.code}）`,
  });

  // 反例 2：放松失败策略（abort → skip-frame）。
  const loosen = assertReplacementPreservesContract(l3, { ...l3, failurePolicy: "skip-frame" }, "hw-decode", "sw-decode");
  out.push({
    name: "replace-loose-policy-rejected",
    pass: !loosen.ok && loosen.code === "FREEZE_DRIFT",
    detail: loosen.ok ? "放松失败策略的替换被接受（上层对失败强度预估将错）" : `放松策略被拦截（${loosen.code}）`,
  });

  // 反例 3：改数据语义（produces 偷换）。
  const retype = assertReplacementPreservesContract(
    l3,
    { ...l3, produces: "processed-frame" },
    "hw-decode",
    "post-fused-decode",
  );
  out.push({
    name: "replace-semantics-change-rejected",
    pass: !retype.ok && retype.code === "LAYER_DATA_CONTRACT_VIOLATION",
    detail: retype.ok ? "改数据语义的替换被接受（邻层拿到的东西变了）" : `改语义被拦截（${retype.code}）`,
  });

  // 反例 4：动邻居（下游指向自己形成环）。
  const loop = assertReplacementPreservesContract(l3, { ...l3, downstream: "L3-decode" }, "hw-decode", "self-loop");
  out.push({
    name: "replace-adjacency-change-rejected",
    pass: !loop.ok && loop.code === "LAYER_ADJACENCY_MISMATCH",
    detail: loop.ok ? "改变邻居关系的替换被接受（管线拓扑被静默改动）" : `动邻居被拦截（${loop.code}）`,
  });

  // 反例 5：自抬能力（实现声称能做契约没给的事）。
  const overreach = assertReplacementPreservesContract(
    l3,
    { ...l3, capabilities: ["decode-hardware", "decode-software", "post-deblock"] },
    "hw-decode",
    "decoder-that-also-filters",
  );
  out.push({
    name: "replace-capability-overreach-rejected",
    pass: !overreach.ok && overreach.code === "LAYER_CAPABILITY_OVERREACH",
    detail: overreach.ok ? "自抬能力的替换被接受（邻层会误以为该实现能做后处理）" : `自抬能力被拦截（${overreach.code}）`,
  });

  return out;
}

/** 判据三自检：接口冻结（版本合法 + 漂移可检出）。 */
export function selfCheckFreeze(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const baselines = buildBaselineContracts();

  let versionOk = true;
  for (const l of LAYER_ORDER) {
    const c = baselines.get(l);
    if (c === undefined || c.contractVersion !== PIPELINE_CONTRACT_VERSION) versionOk = false;
  }
  out.push({
    name: "contract-version-baseline",
    pass: versionOk,
    detail: versionOk ? `五层契约版本均为 ${PIPELINE_CONTRACT_VERSION}（十年承诺基线位）` : "存在非基线契约版本",
  });

  const freeze: ArchitectureFreeze = {
    domainTag: G_DOMAIN.tag,
    firstEntryId: G_DOMAIN.firstEntryId,
    lastEntryId: G_DOMAIN.lastEntryId,
    layers: [...LAYER_ORDER],
    contracts: LAYER_ORDER.map((l) => {
      const c = baselines.get(l);
      return c !== undefined ? { ...c } : ({ layerId: l } as FrozenLayerContract);
    }),
    perceptionLayer: ROUTE_PERCEPTION_LAYER,
    executionLayer: ROUTE_EXECUTION_LAYER,
    frameBudgetMs: 33.3,
    frozenAt: 0,
  };

  // 指纹稳定：两次计算必须一致（否则漂移检测形同虚设）。
  const fp1 = freezeFingerprint(freeze);
  const fp2 = freezeFingerprint({ ...freeze, contracts: [...freeze.contracts] });
  out.push({
    name: "freeze-fingerprint-stable",
    pass: fp1 === fp2,
    detail: fp1 === fp2 ? `冻结指纹稳定 = ${fp1}` : `指纹不稳定（${fp1} vs ${fp2}），漂移检测将误报`,
  });

  // 构造语义漂移（改 produces），必须检出。
  const drifted: ArchitectureFreeze = {
    ...freeze,
    contracts: freeze.contracts.map((c, i) => (i === 2 ? { ...c, produces: "processed-frame" } : { ...c })),
  };
  const drift = detectDrift(freeze, drifted);
  out.push({
    name: "freeze-drift-detected",
    pass: !drift.ok && drift.code === "FREEZE_DRIFT",
    detail: drift.ok ? "语义漂移未被检出（守卫失效）" : `漂移被检出（${drift.code}）`,
  });

  // 未漂移必须通过（守卫不能只会报错）。
  const same = detectDrift(freeze, { ...freeze });
  out.push({
    name: "freeze-no-false-positive",
    pass: same.ok,
    detail: same.ok ? "同源快照不被误报为漂移" : `同源快照被误报：${same.message}`,
  });

  return out;
}

/** 判据四自检：管线延迟分解（每层耗时可归因）。 */
export function selfCheckLatency(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 正常帧：解码层占比最高。
  const normal = decomposeLatency(
    {
      frameIndex: 1,
      samples: [
        { layerId: "L1-container", elapsedMs: 0.4, calls: 1 },
        { layerId: "L2-bitstream", elapsedMs: 0.6, calls: 1 },
        { layerId: "L3-decode", elapsedMs: 12.0, calls: 1 },
        { layerId: "L4-postprocess", elapsedMs: 0.8, calls: 1 },
        { layerId: "L5-output", elapsedMs: 0.2, calls: 1 },
      ],
    },
    33.3,
  );
  out.push({
    name: "latency-five-layer-samples",
    pass: normal.ok && normal.value.shareByLayer.size === 5,
    detail: normal.ok
      ? `五层占比合计 = ${[...normal.value.shareByLayer.values()].reduce((a, b) => a + b, 0).toFixed(4)}（期望 1.0000）`
      : `正常帧分解失败：${normal.message}`,
  });
  out.push({
    name: "latency-no-breach-on-healthy-frame",
    pass: normal.ok && !normal.value.breached,
    detail: normal.ok
      ? `总耗时 ${normal.value.totalMs.toFixed(2)}ms ≤ 预算 33.3ms（未超标）`
      : "分解失败，无法判定是否超标",
  });

  // 超标帧：瓶颈必须定位到层。
  const breach = decomposeLatency(
    {
      frameIndex: 2,
      samples: [
        { layerId: "L1-container", elapsedMs: 1.0, calls: 1 },
        { layerId: "L2-bitstream", elapsedMs: 1.0, calls: 1 },
        { layerId: "L3-decode", elapsedMs: 20.0, calls: 1 },
        { layerId: "L4-postprocess", elapsedMs: 25.0, calls: 1 },
        { layerId: "L5-output", elapsedMs: 1.0, calls: 1 },
      ],
    },
    33.3,
  );
  out.push({
    name: "latency-breach-attributed-to-layer",
    pass: breach.ok && breach.value.breached && breach.value.attributed && breach.value.bottleneckLayer === "L4-postprocess",
    detail: breach.ok
      ? `超标帧归因到 ${breach.value.bottleneckLayer}（占比 ${((breach.value.shareByLayer.get("L4-postprocess") ?? 0) * 100).toFixed(1)}%）`
      : "超标帧未能归因（判据要求定位到层）",
  });

  // 缺层必须显性告警（不可当 0 静默摊掉）。
  const missing = decomposeLatency(
    {
      frameIndex: 3,
      samples: [
        { layerId: "L3-decode", elapsedMs: 10.0, calls: 1 },
        { layerId: "L5-output", elapsedMs: 1.0, calls: 1 },
      ],
    },
    33.3,
  );
  out.push({
    name: "latency-missing-layer-reported",
    pass: missing.ok && missing.diagnostics.some((d) => d.code === "LATENCY_SAMPLE_MISSING"),
    detail: missing.ok
      ? `缺 ${missing.diagnostics.filter((d) => d.code === "LATENCY_SAMPLE_MISSING").length} 层的记账，均已显性告警`
      : "缺层分解失败（缺层必须是告警而非失败，否则调用方会忽略它）",
  });

  // 非法预算必须拒绝。
  const badBudget = decomposeLatency({ frameIndex: 4, samples: [{ layerId: "L3-decode", elapsedMs: 1, calls: 1 }] }, 0);
  out.push({
    name: "latency-invalid-budget-rejected",
    pass: !badBudget.ok && badBudget.code === "FRAME_BUDGET_INVALID",
    detail: badBudget.ok ? "非法预算（0ms）被接受，无法判超标" : `非法预算被拒（${badBudget.code}）`,
  });

  return out;
}

/** 判据五自检：错误跨层传播（零静默 + 上层可见）。 */
export function selfCheckFailurePropagation(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const ctx0 = createFrameContext(1, "P", 90000);
  if (!ctx0.ok) {
    return [{ name: "failure-ctx-buildable", pass: false, detail: `帧上下文构造失败：${ctx0.message}` }];
  }
  const ctx = ctx0.value;
  const upstreamChain: readonly LayerId[] = ["L2-bitstream", "L1-container"];

  // 正常传播：L3 硬解失败 → degrade-to-soft，上游必须可见。
  const good = propagateFailure(
    ctx,
    "L3-decode",
    {
      what: "硬解初始化失败（驱动返回 0x80070005 访问被拒）",
      cause: "VPU 设备被其它进程独占，或驱动未加载",
      action: "回退软解并在遥测记一次回退事件（F1215），下一帧起走软解",
      contextSnapshot: "vendor=nvidia surface=handle#3 dpb=full",
    },
    "degrade",
    "degrade-to-software",
    upstreamChain,
  );
  out.push({
    name: "failure-upstream-visible",
    pass: good.ok && good.value.upstreamVisible.length === upstreamChain.length,
    detail: good.ok
      ? `L3 失败已向 ${good.value.upstreamVisible.length} 个上游层传播（上层可见判据成立）`
      : `传播失败：${good.message}`,
  });
  out.push({
    name: "failure-recorded-in-frame",
    pass: good.ok && good.value.ctx.failures.length === 1 && good.value.ctx.failures[0]?.layerId === "L3-decode",
    detail: good.ok ? "失败已挂帧上下文错误账（上层与下游均可读）" : "失败未入账（等于吞错）",
  });

  // 吞错反例：三要素缺失必须被拒。
  const silent = propagateFailure(
    ctx,
    "L2-bitstream",
    { what: "", cause: "", action: "", contextSnapshot: "x" },
    "skip",
    "skip-frame",
    ["L1-container"],
  );
  out.push({
    name: "failure-silent-rejected",
    pass: !silent.ok && silent.code === "LAYER_FAILURE_SILENT",
    detail: silent.ok ? "三要素全空的失败被接受（静默吞错红线被破）" : `吞错被拦截（${silent.code}）`,
  });

  // 上层不可见反例：非中止却未向上游传播。
  // 注意契约/处置必须同档（都用 skip-frame），否则会被更早的「处置比契约松」
  // 检查拦下，测的就不是「零传播」而是「处置过松」——那是另一条自检。
  const invisible = propagateFailure(
    ctx,
    "L2-bitstream",
    {
      what: "参数集语法越界（SPS 声称 64K 宽度）",
      cause: "资源上限校验未前置",
      action: "错误隐藏后跳过该帧",
      contextSnapshot: "sps.width=65536",
    },
    "skip",
    "skip-frame",
    [],
  );
  out.push({
    name: "failure-non-abort-must-propagate",
    pass: !invisible.ok && invisible.code === "FAILURE_NOT_UPPSTREAM_VISIBLE",
    detail: invisible.ok
      ? "非中止失败允许零传播（等于本层独自吞错）"
      : `零传播被拦截（${invisible.code}）`,
  });

  // 半传播反例：只报 L1、漏掉 L2（半传播比零传播更隐蔽——上层看起来知情）。
  const half = propagateFailure(
    ctx,
    "L3-decode",
    {
      what: "硬解运行时错误",
      cause: "VPU 超时",
      action: "回退软解重试该帧",
      contextSnapshot: "vendor=nvidia t=3.2s",
    },
    "degrade",
    "degrade-to-software",
    ["L1-container"],
  );
  out.push({
    name: "failure-half-propagation-rejected",
    pass: !half.ok && half.code === "FAILURE_NOT_UPPSTREAM_VISIBLE",
    detail: half.ok
      ? "只报 L1 漏掉 L2 的半传播被接受（码流层会在不知情的情况下继续喂数据）"
      : `半传播被拦截（${half.code}）`,
  });

  // 放松处置反例：契约 degrade 而处置 skip。
  const loosen = propagateFailure(
    ctx,
    "L3-decode",
    { what: "硬解运行时错误", cause: "VPU 超时", action: "跳过", contextSnapshot: "s" },
    "skip",
    "degrade-to-software",
    upstreamChain,
  );
  out.push({
    name: "failure-disposition-not-looser-than-contract",
    pass: !loosen.ok && loosen.code === "FAILURE_POLICY_UNREGISTERED",
    detail: loosen.ok ? "比契约更松的处置被接受（上层预估错误）" : `过松处置被拦截（${loosen.code}）`,
  });

  return out;
}

/** 判据六自检：帧上下文分区治理（越权写入拒绝）。 */
export function selfCheckFrameContext(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const built = createFrameContext(7, "B", 90000);
  if (!built.ok) {
    return [{ name: "frame-ctx-buildable", pass: false, detail: `帧上下文构造失败：${built.message}` }];
  }
  const ctx = built.value;

  // 合法路由决策样本（字段齐全，避免用半成品值掩盖写入路径的问题）。
  const hwRoute: RouteDecision = {
    path: "hardware",
    reasons: ["格式支持=是", "分辨率 1920x1080 ≤ 上限 4096", "Profile High ≤ 声明上限", "性能需求达标"],
    softwareReason: null,
    vendor: "generic-vpu",
  };
  const swRoute: RouteDecision = {
    path: "software",
    reasons: ["格式支持=是", "分辨率支持=是", "驱动探测失败"],
    softwareReason: "驱动能力探测返回 UNAVAILABLE（虚报驱动探测纠偏，F1208）",
    vendor: null,
  };

  // 正例：归属层写自己的分区。
  const legal = writeField(ctx, "route", hwRoute, "L3-decode");
  out.push({
    name: "frame-field-owner-write-allowed",
    pass: legal.ok,
    detail: legal.ok ? "L3 写 route 分区成功（归属层可写）" : `归属层写入被拒：${legal.message}`,
  });

  // 反例：非归属层写 route。
  const illegal = writeField(ctx, "route", swRoute, "L1-container");
  out.push({
    name: "frame-field-foreign-write-rejected",
    pass: !illegal.ok && illegal.code === "FRAME_FIELD_ACCESS_VIOLATION",
    detail: illegal.ok ? "容器层写路由分区被接受（路由决策会被邻层篡改）" : `越权写入被拦截（${illegal.code}）`,
  });

  // 写入不可变性：原上下文不得被就地改（B 帧重排下的数据竞争防线）。
  const immutable = legal.ok && ctx.fields.has("route") === false;
  out.push({
    name: "frame-field-write-is-immutable",
    pass: immutable,
    detail: immutable
      ? "写入返回新对象，原上下文未被就地改（B 帧乱序消费下不会读到半写元数据）"
      : "写入就地改了原上下文（B 帧重排时输出层可能读到半写的时间戳）",
  });

  // 时基自洽：B 帧 PTS<DTS 必被判错。
  const badB: FrameContext = { ...ctx, kind: "B", pts: 100, dts: 300 };
  const bCheck = verifyFrameContext(badB);
  out.push({
    name: "frame-bframe-order-checked",
    pass: !bCheck.ok && bCheck.code === "FRAME_TIMEBASE_INCONSISTENT",
    detail: bCheck.ok ? "B 帧 PTS<DTS 未被检出（根因在容器层却会表现在解码层）" : `B 帧时序矛盾被检出（${bCheck.code}）`,
  });

  // 必填分区缺失必被判错。
  const emptyCheck = verifyFrameContext(ctx);
  out.push({
    name: "frame-required-fields-checked",
    pass: !emptyCheck.ok && emptyCheck.code === "FRAME_CONTEXT_FIELD_MISSING",
    detail: emptyCheck.ok ? "空帧上下文被判为合法（必填分区校验失效）" : `缺必填分区被检出（${emptyCheck.code}）`,
  });

  return out;
}

/**
 * 判据七自检：路由位置（码流层感知 / 解码层执行）。
 *
 * 关键设计：反例必须**真的**把 execution 角色挂到容器层上，而不是伪造一个
 * 不存在的字段。伪造字段的检查永远通过——那不是判据，是自欺。
 */
export function selfCheckRoutePlacement(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 正例装配：五层齐备、路由角色落在规范位上。
  const good = verifyRoutePlacement(buildReferenceAssembly());
  out.push({
    name: "route-placement-legal",
    pass: good.ok,
    detail: good.ok
      ? `感知层 = ${LAYER_LABEL[ROUTE_PERCEPTION_LAYER]}，执行层 = ${LAYER_LABEL[ROUTE_EXECUTION_LAYER]}（与锚点一致）`
      : `合法装配被拒：${good.message}`,
  });

  // 反例一：容器层越位执行路由（L1 与硬件耦合，可替换性失效）。
  const l1Executes = verifyRoutePlacement(buildReferenceAssembly({ "L1-container": "execution" }));
  out.push({
    name: "route-misplacement-container-execution-rejected",
    pass: !l1Executes.ok && l1Executes.code === "ROUTE_MISPLACED",
    detail: l1Executes.ok
      ? "容器层执行路由未被拦截（换解码器时被迫改容器解析，层可替换性失效）"
      : `容器层越位执行被拦截（${l1Executes.code}）`,
  });

  // 反例二：码流层越位执行路由（锚点明确 L2 只感知）。
  const l2Executes = verifyRoutePlacement(buildReferenceAssembly({ "L2-bitstream": "execution" }));
  out.push({
    name: "route-misplacement-bitstream-execution-rejected",
    pass: !l2Executes.ok && l2Executes.code === "ROUTE_MISPLACED",
    detail: l2Executes.ok
      ? "码流层执行路由未被拦截（L2 与硬件能力耦合）"
      : `码流层越位执行被拦截（${l2Executes.code}）`,
  });

  // 反例三：容器层越位承担格式感知（感知位专属 L2）。
  const l1Perceives = verifyRoutePlacement(buildReferenceAssembly({ "L1-container": "perception" }));
  out.push({
    name: "route-misplacement-container-perception-rejected",
    pass: !l1Perceives.ok,
    detail: l1Perceives.ok
      ? "容器层自报格式感知未被拦截（感知位被非码流层占据）"
      : `容器层越位感知被拦截（${l1Perceives.code}）`,
  });

  // 反例四：执行位空缺（无人执行路由 → 管线是死的）。
  const noExecution = verifyRoutePlacement(buildReferenceAssembly({ "L3-decode": "none" }));
  out.push({
    name: "route-execution-role-required",
    pass: !noExecution.ok && noExecution.code === "ROUTE_MISPLACED",
    detail: noExecution.ok
      ? "执行位空缺未被拦截（码流层能解析但无人解码）"
      : `执行位空缺被拦截（${noExecution.code}）`,
  });

  // 反例五：感知层角色错位（自报 none）。
  const noPerception = verifyRoutePlacement(buildReferenceAssembly({ "L2-bitstream": "none" }));
  out.push({
    name: "route-perception-role-required",
    pass: !noPerception.ok,
    detail: noPerception.ok
      ? "码流层不承担感知未被拦截（路由将无依据）"
      : `感知位空缺被拦截（${noPerception.code}）`,
  });

  return out;
}

/** 参考装配（判据与下游共用）：五层齐备，路由角色落在规范位。 */
export function buildReferenceAssembly(roleOverrides: Partial<Record<LayerId, RouteRole>> = {}): PipelineAssembly {
  const baselines = buildBaselineContracts();
  const layers: LayerRegistration[] = [];
  for (const l of LAYER_ORDER) {
    const contract = baselines.get(l);
    if (contract === undefined) continue;
    const owner = LAYER_OWNER_ENTRY[l];
    layers.push({
      layerId: l,
      implementationId: l === "L1-container" ? "mp4-iso-bmff" : `impl-${l}`,
      ownerEntryId: owner,
      declaredCapabilities: [...contract.capabilities],
      routeRole: roleOverrides[l] ?? referenceRouteRole(l),
      contract: { ...contract },
    });
  }
  return { layers, frameBudgetMs: 33.3, assembledAt: 0 };
}

/** 参考角色：感知位归 L2，执行位归 L3，其余为 none。 */
function referenceRouteRole(layerId: LayerId): RouteRole {
  if (layerId === ROUTE_PERCEPTION_LAYER) return "perception";
  if (layerId === ROUTE_EXECUTION_LAYER) return "execution";
  return "none";
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §13 架构宣告渲染（文档替述可读；文档与代码同源）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 生成五层管线架构宣告（人话版）。
 *
 * 无障碍要求：架构文档替述可读——本函数是文档的可执行生成源，保证
 * 「文档所述」与「代码所声明」同源，不会各说各话。F1218 文档条目直接消费它。
 */
export function renderArchitectureDeclaration(freeze: ArchitectureFreeze, fingerprint: string): string {
  const layerRows = freeze.layers.map((l) => {
    const c = freeze.contracts.find((x) => x.layerId === l);
    const caps = c !== undefined ? [...c.capabilities].sort().join("/") : "—";
    return [
      `  ${LAYER_RANK[l]}. ${LAYER_LABEL[l]}（${l}）`,
      `     职责：${LAYER_DUTY[l]}`,
      `     数据：${c !== undefined ? `${c.consumes} → ${c.produces}` : "—"}`,
      `     能力位：${caps}`,
      `     失败策略：${c !== undefined ? c.failurePolicy : "—"}`,
      `     责任条目：F${LAYER_OWNER_ENTRY[l]}（实现条目 ${LAYER_IMPLEMENTATION_ENTRIES[l].map((e) => `F${e}`).join("、")}）`,
    ].join("\n");
  });

  return [
    `【VE-G 视频解码引擎域 · 五层管线架构宣告】`,
    `条目区间：F${freeze.firstEntryId}-F${freeze.lastEntryId}（共 ${G_DOMAIN.entryCount} 条 / ${G_DOMAIN.groupCount} 组）。`,
    ``,
    `五层管线（数据流方向）：`,
    ...layerRows,
    ``,
    `路由在层间的位置：`,
    `  · 格式感知位 = ${LAYER_LABEL[freeze.perceptionLayer]}（只感知编解码器格式，不做能力裁决）`,
    `  · 路由执行位 = ${LAYER_LABEL[freeze.executionLayer]}（消费 CGPU-F0104 能力位图与决策树）`,
    `  · 越位即拦截：路由若出现在感知层或容器层，L2/L1 将与硬件能力耦合，层可替换性失效。`,
    ``,
    `可替换性契约：`,
    `  · 每层可独立替换实现（硬解/软解/混合），替换时冻结契约五面不变：`,
    `    邻居关系 / 数据语义 / 能力位 / 失败策略 / 契约版本。`,
    `  · 能力削减、策略放松、语义改写、邻居变更、版本漂移——五者任一发生即拒绝替换。`,
    ``,
    `帧上下文：五层之间唯一传递载体，分区归属唯一（L1 写轨道与时基、L2 写格式、`,
    `  L3 写路由与解码产物、L4 写后处理参数、L5 写表面句柄），越权写入即诊断。`,
    ``,
    `错误传播：层失败必带三要素（发生了什么/为什么/下一步怎么办），失败挂帧上下文，`,
    `  上游全层可见；处置不得比契约更松。静默吞错按最高缺陷处理。`,
    ``,
    `帧预算：${freeze.frameBudgetMs}ms；超标必归因到具体层，并给出该层的优化方向。`,
    ``,
    `架构冻结指纹：${fingerprint}（冻结时间戳 ${freeze.frozenAt}）。`,
  ].join("\n");
}

/**
 * 运行全量判据自检。
 * 汇总为一份可机读结果，供门禁消费（F1218 文档、F1219 基准共用）。
 */
/**
 * 判据自检·回归组：本条四次修复留下的护栏。
 *
 * 存在的理由：修复若只落在实现里而无断言，下次重构就会把它悄悄改回去。
 * 每项对应一个曾真实放行的错误输入，断言写死「必须被拒 + 必须报某码」，
 * 任何回归都会立刻红。
 */
export function selfCheckRegressions(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const base = buildReferenceAssembly();
  const baselines = buildBaselineContracts();
  const L3 = baselines.get("L3-decode");

  // R1 处置严格度标尺错位：契约 abort 时用 recover 处置必须被拒。
  //旧实现用两把不同量纲的标尺直接比大小（abort=3 对 recover=3），
  // 判定 3<3 为假而放行——契约要求中止，实现却恢复后继续，行为与声明反向。
  {
    const ctx = createFrameContext(9001, "P", 90000);
    const chain = [...LAYER_ORDER].reverse();
    const r = ctx.ok
      ? propagateFailure(
          ctx.value,
          "L1-container",
          { what: "moov 缺失", cause: "非 faststart", action: "中止播放", contextSnapshot: "box=moov" },
          "recover",
          "abort",
          chain,
        )
      : ctx;
    out.push({
      name: "regression-disposition-strictness-scale",
      pass: !r.ok && r.code === "FAILURE_POLICY_UNREGISTERED",
      detail: r.ok
        ? "契约 abort 被 recover 处置放过（处置严于契约的声明被忽略）"
        : `契约 abort 下的 recover 处置被拒（${r.code}）`,
    });
  }

  // R2 邻接序表基准：中间层自认无上游必须被拒。
  // 旧实现只校验「非null 侧的对称性」，upstream=null 时整段校验被跳过。
  if (L3 !== undefined) {
    const hacked = { ...L3, upstream: null };
    const r = verifyContractShape(hacked, baselines);
    out.push({
      name: "regression-mid-layer-null-upstream-rejected",
      pass: !r.ok && r.code === "LAYER_ADJACENCY_MISMATCH",
      detail: r.ok
        ? "中间层声明 upstream=null 未被拦截（容器层产出凭空消失）"
        : `中间层自认无上游被拒（${r.code}）`,
    });

    // R2b 合法邻接不得误报（序表基准不能把对的判成错的）。
    const good = verifyContractShape(L3, baselines);
    out.push({
      name: "regression-canonical-adjacency-no-false-positive",
      pass: good.ok,
      detail: good.ok ? "序表邻接未被误判" : `序表邻接被误拒（${good.code}）`,
    });
  }

  // R3 帧预算闸门必须止于入口：非法预算不得进入冻结快照。
  // 旧实现 openDomain 完全不校验预算，0/负/NaN/无穷可一路写进 fingerprint，
  // 于是「超标必归因到层」在正式运行时永不成立，而指纹看上去仍合法。
  {
    const bads: readonly number[] = [0, -5, Number.NaN, Number.POSITIVE_INFINITY, FRAME_BUDGET_MAX_MS * 10];
    let allBlocked = true;
    const seen: string[] = [];
    for (const v of bads) {
      const k = openDomain({ ...base, frameBudgetMs: v }, 1);
      if (k.ok) allBlocked = false;
      else seen.push(`${String(v)}→${k.code}`);
    }
    out.push({
      name: "regression-invalid-budget-blocked-at-entry",
      pass: allBlocked,
      detail: allBlocked ? `非法预算全部被拒（${seen.length} 类）` : "非法预算被放进冻结快照",
    });

    // R3b 合法预算不得误拒（含上下界之内与恰在边界上的值）。
    const goods: readonly number[] = [FRAME_BUDGET_MIN_MS, 16.7, 33.3, FRAME_BUDGET_MAX_MS];
    const wrongRejects = goods.filter((v) => inspectFrameBudget(v) !== null);
    out.push({
      name: "regression-valid-budget-accepted",
      pass: wrongRejects.length === 0,
      detail: wrongRejects.length === 0
        ? `合法预算全部放行（${goods.length} 个含边界）`
        : `合法预算被误拒：${wrongRejects.join(",")}`,
    });
  }

  // R4 预算闸门在入口与分解处必须同尺（同一把尺，不允许一处守一处漏）。
  {
    const ledger: LatencyLedger = {
      frameIndex: 1,
      samples: LAYER_ORDER.map((id, i) => ({ layerId: id, elapsedMs: i + 1, calls: 1 })),
    };
    let consistent = true;
    for (const v of [0, -1, Number.NaN, FRAME_BUDGET_MAX_MS * 10]) {
      const atEntry = inspectFrameBudget(v) !== null;
      const atDecompose = !decomposeLatency(ledger, v).ok;
      if (atEntry !== atDecompose) consistent = false;
    }
    out.push({
      name: "regression-budget-gate-single-scale",
      pass: consistent,
      detail: consistent ? "入口与延迟分解对同一批非法预算判定一致" : "入口与延迟分解对非法预算判定不一致",
    });
  }

  return out;
}

export function runSelfCheck(): { readonly checks: readonly SelfCheck[]; readonly allPass: boolean } {
  const checks: SelfCheck[] = [
    ...selfCheckFiveLayers(),
    ...selfCheckReplaceability(),
    ...selfCheckFreeze(),
    ...selfCheckLatency(),
    ...selfCheckFailurePropagation(),
    ...selfCheckFrameContext(),
    ...selfCheckRoutePlacement(),
    ...selfCheckRegressions(),
  ];
  const allPass = checks.every((c) => c.pass);
  return { checks, allPass };
}
