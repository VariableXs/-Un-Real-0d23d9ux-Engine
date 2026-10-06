/**
 * VE-F0207 · Venus Vulkan 通路评估与接入（VE-B 域 · virtio 组 B1 · 组内第 7 条）
 * ---------------------------------------------------------------------------
 * 职责定位：venus 是 virtio-gpu 的 Vulkan 透传（guest Vulkan → host Vulkan）。
 *   本条做**接入评估与骨架**，不是「把 Venus 接进来」——因为诚实结论是：
 *   VARIX 当前**没有完整 Vulkan ICD 栈**（无 loader / 无 ICD / 无 Vulkan 头文件
 *   派生绑定），因此 venus 在本系统里**只能做架构预留**。
 *   本条产出四件：
 *     ① 能力探测（VIRTIO_GPU_CAP_VENUS）——cap 位读取 + 四态判定；
 *     ② venus 协议版本协商——三元组（min/max/ask）区间求交 + ABI 上界钳制；
 *     ③ 上下文类型声明（VIRTIO_GPU_CTX_TYPE_VENUS）——声明而不创建；
 *     ④ guest 侧 Vulkan 驱动栈存在性评估 + 依赖清单（反向驱动 VE-C 着色器域
 *        与 CGPU-K 排期）。
 *
 * ┌── 第一性声明（本域最恶劣缺陷：假装能用）────────────────────────────┐
 * │ 渲染通路类功能最恶劣的缺陷不是「不能透传」，而是**假装能透传**：     │
 * │ 探测到 cap 位就宣称 Venus 可用、把上下文类型声明写进能力表、        │
 * │ 让上层按「有 Vulkan」分配内存建资源，最终在跑第一帧时因缺 ICD 崩掉。│
 * │ 崩掉还算好命。更坏的是它一路伪装到用户面前：界面显示 Vulkan 加速，   │
 * │ 实际全部走软渲，帧率 3fps，没有任何一行错误日志。                  │
 * │ 因此本条的第一性声明是**能力声称与能力事实必须同源可证**：          │
 * │   每一处声称 Venus 可用的地方，都必须能指到一份探测证据；            │
 * │   指不到证据的地方，能力位强制为UNAVAILABLE，且给出**可上屏的       │
 * │   不可用原因**（缺什么、找谁、哪一步缺），不是一句「不支持」。       │
 * └─────────────────────────────────────────────────────────────────────┘
 *
 * 「诚实标注不可用原因」是本条的硬判据，不是文风要求。理由：venus 不可用
 * 是一个**跨三个域的排期事实**——它决定 VE-C 着色器域要不要做 SPIR-V
 * 编译链、CGPU-K 要不要留着色器缓存位。若被一句乐观的「已支持」掩盖，
 * 两个域会在错误的排期上开工，代价是整条链返工。诚实在这里不是谦虚，
 * 是**排期输入**。
 *
 * 能力四态（与 F0206 virgl 四态同构，语义对齐）：
 *   · VENUS_UNSUPPORTED  host 不报 VIRTIO_GPU_CAP_VENUS（host 无 Venus）→终态；
 *   · RESERVED_ONLY      host 报 cap，但 guest 缺 Vulkan ICD 栈 → 架构预留（**本
 *                        系统当前所处状态**，本条的主结论）；
 *   · NEGOTIABLE         cap 在 + ICD 齐备 + 版本区间可求交 → 可进入真实协商；
 *   · AVAILABLE          三者全满足且协商成功 → 真实透传（须 F0209 scanout 承接）。
 *
 * 为什么不把状态叫「部分支持」：部分支持会让上层以为「能跑一点」，于是
 * 资源按 Vulkan 分配、渲染按 Vulkan 提交、失败回退路径没人写。**三态可
 * 跳，不设中间态**——与 U 域口径处置封闭同源：没有「降级豁免」第三向。
 *
 * 架构位（锚点原文：venus 就绪后经同一表面管道输出）：
 *   本条**只预留对接面，不实现输出**。预留面是三段：
 *     ① 提交面 VenusSubmitSink——把 Venus 渲染产物挂进既有表面管道；
 *     ② 呈现面绑定到 F0209 的 SET_SCANOUT 序（不是另开一条呈现路径）；
 *     ③ 失败面回落到软渲（A 域 F0013），回落必须显性通知。
 *   为什么不现在就接：呈现路径一旦为 Venus 分叉，virgl 与 Venus 两套
 *   呈现节奏会各自漂移（F0209 呈现节奏三模式届时要维护两倍测试面）。
 *   **在 venus 真的可用之前，让它与 virgl 共用同一条呈现路径**是唯一
 *   不会产生分叉的做法。
 *
 * 零静默纪律：所有拒绝/缺源/版本不交集/栈缺失/声称无据全部产出
 *   Diagnostic（code + severity + message + hint + stage）。本模块不向 UI
 *   直接抛异常，也不吞掉任何一条诊断。缺 ICD 不是「先开工再说」——要
 *   回溯到依赖清单条目把缺项补齐，回溯不到才允许停在 RESERVED_ONLY。
 *
 * 性能逐项分解（锚点原文：评估一次 + 协商 O(区间数)）：
 *   · 能力探测 O(1)——cap 位是单个 u32 位测试，非遍历；
 *   · 版本协商 O(区间数)——两侧三元组求交为常数规模（宿主×guest 有限集）；
 *   · 栈存在性评估 O(项数)——依赖清单为有限常量表（≤16 项），非 O(文件系统)；
 *   · 报告装配 O(依赖数)——一次性，**不常驻**；不请求不装配。
 *   架构期没有一段是 O(全系统模块数)：若某天需要遍历所有已装模块才能
 *   判定ICD 存在性，说明职责放错了层（那是运行时探测的活，见下）。
 *
 * 工程量（锚点原文分解）：核心逻辑约 230 行（cap 探测 + 版本求交 + 上下文
 *   声明 + 栈评估）、边界防护约 80 行（cap 位越界/版本畸形/栈项缺失校验）、
 *   错误路径约 70 行（不可用原因分级 + 降级与回滚处置）、测试支撑约 60 行
 *   （域级自检回归），合计约 440 行。
 *
 * 判据：探测与协商实现、评估报告诚实标注不可用原因、骨架可编译、依赖清单
 *   进台账、架构位与表面管道对接预留。
 *
 * 依赖锚点：F0201（virtio-gpu 初始化 · cap 探测入点）、F0205（上下文协商
 *   框架 · 本条的协商逻辑挂在同型框架下）、F0206（virgl 命令流编码 ·
 *   四态判定同构）、F0208（virtio blob 资源 · Venus 共享内存的落点）、
 *   F0209（扫描输出与呈现 · 架构位对接面）、F0013（A 域软渲 · 回落终点）、
 *   F0060（表面协议 · 提交面契约源）、F0007（A 域能力位图 · 能力位落图）。
 * 下游消费：F0215（一致性测试套件 · Venus 档 N/A 语义）、F0216（参考驱动
 *   宣告 · 三通路声明的 Venus 一栏）、F0218（QEMU 兼容矩阵 · Venus 行）、
 *   F0220（组收口 · 三档基线中的 Venus 档诚实标注）、VE-C 着色器域排期、
 *   CGPU-K 排期（均由本条依赖清单反向驱动）。
 * 交接说明：本条只管「cap 探测 + 版本协商 + 上下文声明 + 栈评估 + 依赖清单
 *   + 架构位预留」；真实 Venus 渲染实现不在本条范围，本条**不越界假装实现**。
 */

/* ═══════════════════════════════════════════════════════════════════════════
 * §1 诊断与结果类型（零静默的基础设施）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 诊断严重度分级。
 * 锚点原文只给「诚实标注不可用原因」一条，无分级字母。本域按
 * 「阻断 / 显性 / 记账」三档自定义，语义与家族其他域保持一致：
 *   · P0 阻断级——能力声称无证据、或探测出「可透传」但栈缺失。
 *     出现即阻断：宁可能力位为不可用，也不允许无据声称。
 *   · P1 显性级——版本区间不交集、依赖缺项、栈评估不完整。必须对人话
 *     复述（可上屏），修复前不得进入依赖该结论的下游排期。
 *   · P2 记账级——可观测性、预留位、待对账。记账即可，不阻断。
 */
export type Severity = "P0" | "P1" | "P2";

/**
 * 诊断码表。命名纪律：处置方向相反的状态**不得共用码**。
 * 例如「版本交集为空」要停（NO_VERSION_OVERLAP），而「abi 高于上界、
 * 可钳制后继续」只记账（ABI_CLAMPED）——前者必须让排期改，后者可以走。
 */
export type DiagCode =
  /** P0能力位声称 Venus 可用但探测证据缺失。 */
  | "VENUS_CAPABILITY_CLAIM_UNPROVEN"
  /** P0 探测判定可透传，但 guest 侧 Vulkan ICD 栈缺失（自相矛盾）。 */
  | "VENUS_STACK_MISSING_BUT_CLAIMED"
  /** P0 评估结论为可用，却给不出至少一条可核验的支撑证据。 */
  | "VENUS_VERDICT_WITHOUT_EVIDENCE"
  /** P1 host 未报 VIRTIO_GPU_CAP_VENUS——终态不可用（非缺陷）。 */
  | "VENUS_CAP_ABSENT_ON_HOST"
  /** P1 guest 侧缺 Vulkan ICD 栈——架构预留的**首要原因**。 */
  | "VENUS_ICD_STACK_ABSENT"
  /** P1 版本区间无交集——协商不可进行。 */
  | "VENUS_NO_VERSION_OVERLAP"
  /** P2 guest ABI 高于已知上界，已钳制到上界继续（可走，仅记账）。 */
  | "VENUS_ABI_CLAMPED"
  /** P2 版本交集非空但未实际发起协商（预留态下的预期情形）。 */
  | "VENUS_NEGOTIATION_NOT_ATTEMPTED"
  /** P1 依赖清单缺项——排期输入不完整，必须回补。 */
  | "VENUS_DEPENDENCY_ITEM_MISSING"
  /** P1 架构位被误接线到非表面管道（ Venus 必须走同一表面管道）。 */
  | "VENUS_ARCH_SLOT_MISWIRED"
  /** P2 回落路径未显性通知用户（软渲降级须告知）。 */
  | "VENUS_FALLBACK_NOT_ANNOUNCED"
  /** P2 能力位图未登记 Venus 位（记账，登记即可）。 */
  | "VENUS_CAPBIT_NOT_REGISTERED"
  /* ── 扩展段：VE-B 域后续条目（F0208 blob 起）的诊断码由属主条目登记 ──
   * 登记纪律：B1 组共用本域的 DiagCode 联合。后续条目**只增不改**——
   * 增是安全的（联合变大，旧消费者不受影响），改则会让已推送的守卫判据
   * 与远端不一致。故扩段只允许追加，不得删改既有码。
   */
  /** P0 同一 blob 上出现混合 cache/uncache 映射（F0208 · 花屏源）。 */
  | "BLOB_ATTR_MIXED"
  /** P0 销毁顺序乱序（F0208）。 */
  | "BLOB_DESTROY_ORDER_VIOLATION"
  /** P0 生命周期状态机非法跃迁（F0208）。 */
  | "BLOB_LIFECYCLE_TRANSITION_INVALID"
  /** P1 映射属性与 mem_type 推导口径不符（F0208 · 可纠正）。 */
  | "BLOB_MAPPING_ATTR_SUBOPTIMAL"
  /** P1 跨服务导出后退化为拷贝（F0208）。 */
  | "BLOB_EXPORT_DEGRADED_TO_COPY"
  /** P1 blob 创建参数非法（F0208）。 */
  | "BLOB_CREATE_PARAM_INVALID"
  /** P1 非EXPORTED 态导出或重复导出（F0208）。 */
  | "BLOB_EXPORT_STATE_INVALID"
  /** P2 生命周期压测观测项（F0208 · 记账）。 */
  | "BLOB_LIFECYCLE_STRESS_OBSERVED";

/** 一条诊断。stage 标注判定发生在哪个阶段，便于 UI 定位而非只报码。 */
export interface Diagnostic {
  readonly code: DiagCode;
  readonly severity: Severity;
  readonly message: string;
  readonly hint: string;
  readonly stage: string;
}

/** 结果类型：要么带值，要么带失败原因（不裸抛，不裸 null）。 */
export type Outcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly Diagnostic[] }
  | { readonly ok: false; readonly message: string; readonly diagnostics: readonly Diagnostic[] };

/** 构造成功结果。 */
export function ok<T>(value: T, diagnostics: readonly Diagnostic[] = []): Outcome<T> {
  return { ok: true, value, diagnostics };
}

/** 构造失败结果。 */
export function err<T>(
  message: string,
  diagnostics: readonly Diagnostic[] = [],
): Outcome<T> {
  return { ok: false, message, diagnostics };
}

/** 取诊断序列（两种结果形态都可取）。 */
export function diagnosticsOf<T>(outcome: Outcome<T>): readonly Diagnostic[] {
  return outcome.diagnostics;
}

/** 是否含阻断级诊断。 */
export function hasBlocking<T>(outcome: Outcome<T>): boolean {
  return outcome.diagnostics.some((d) => d.severity === "P0");
}

/** 按严重度筛选诊断。 */
export function bySeverity<T>(
  outcome: Outcome<T>,
  severity: Severity,
): readonly Diagnostic[] {
  return outcome.diagnostics.filter((d) => d.severity === severity);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §2 VE-B 域标识与条目区间
 * ═══════════════════════════════════════════════════════════════════════════ */

/** VE-B 域标识与 virtio 组 B1 条目区间。 */
export const B_DOMAIN = {
  tag: "VE-B",
  name: "virtio 组",
  groupId: "B1",
  /** B1 组首条（F0201初始化）与末条（F0220 收口移交）。 */
  firstEntryId: 201,
  lastEntryId: 220,
  /** 本条在组内的序位（1 起）。 */
  ordinalInGroup: 7,
  /** 前置条目：F0206（virgl 命令流编码 · 四态判定同构源）。 */
  predecessorEntryId: 206,
} as const;

/**
 * B 域开工条已落位者（本条之前的组内前序，用于台账登记的连续性核验）。
 * 消费点：`verifyB1Continuity` —— 组内条目必须逐条连续，不允许跳号，
 * 跳号会让「谁还没做」这个问题得不到确定答案。
 */
export const B1_LANDED_BEFORE_THIS: readonly number[] = [201, 202, 203, 204, 205, 206];

/** 本条在 B1 组内的条目号（= 组内序位 + 组首条目偏移）。 */
export const B_DOMAIN_THIS_ENTRY_ID = B_DOMAIN.firstEntryId + (B_DOMAIN.ordinalInGroup - 1);

/**
 * B1 组条目连续性核验（本条开工闸）。
 * 核验两件事：① 前序条目号连续无跳号；② 本条号紧接前序末条。
 * 失败产出 P1：组内台账一旦有洞，「这批还剩几条」就再也答不对。
 */
export function verifyB1Continuity(): Outcome<number> {
  const diagnostics: Diagnostic[] = [];
  const list = B1_LANDED_BEFORE_THIS;
  for (let i = 1; i < list.length; i += 1) {
    const prev = list[i - 1] ?? 0;
    const cur = list[i] ?? 0;
    if (cur !== prev + 1) {
      diagnostics.push({
        code: "VENUS_DEPENDENCY_ITEM_MISSING",
        severity: "P1",
        message: `B1 组条目跳号：F${prev} 之后是 F${cur}`,
        hint: "补齐中间条目或修正登记；跳号会使组内剩余工作量无法统计",
        stage: "b1-continuity",
      });
    }
  }
  const last = list[list.length - 1] ?? 0;
  if (B_DOMAIN_THIS_ENTRY_ID !== last + 1) {
    diagnostics.push({
      code: "VENUS_DEPENDENCY_ITEM_MISSING",
      severity: "P1",
      message: `本条号 F${B_DOMAIN_THIS_ENTRY_ID} 与前序末条 F${last} 不连续`,
      hint: "核对 B_DOMAIN.ordinalInGroup 与 firstEntryId；本条须紧接前序",
      stage: "b1-continuity",
    });
  }
  return ok(B_DOMAIN_THIS_ENTRY_ID, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §3 virtio-gpu 能力位常量表（cap 探测的数据源）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * virtio-gpu 设备 cap 位（本条只登记与能力判定相关的位，不全量搬运）。
 * 数值来源为 virtio_gpu.h 的 `enum virtio_gpu_cap` 位序，与内核侧一致。
 * 位序错一位会导致「host 有 Venus 却被判为 2D only」——故逐位显式写出，
 * 不用位移表达式，避免后人「顺手优化」成 `1 << n` 而丢掉可读性。
 */
export const VIRTIO_GPU_CAP = {
  /** 位 1：host 报支持 Venus（Vulkan 透传）。本条的目标位。 */
  VENUS: 1 << 1,
  /** 位 2：host 报支持 virgl（OpenGL 透传），F0206 的目标位。 */
  VIRGL: 1 << 2,
  /** 位 0：host 报支持 2D（virtio 加速 2D 基线能力，恒有）。 */
  VENUS_TERMINAL_2D: 1 << 0,
  /** 位 4：host 报支持 VULKAN2（Venus 的**旧**通路，与 VENUS 是两代）。 */
  VULKAN2: 1 << 4,
  /** 位 8：cursor 2D能力（F0210 目标位，本条仅登记不评估）。 */
  CURSOR_2D: 1 << 8,
  /** 位 9：EDID（F0213 目标位，本条仅登记不评估）。 */
  EDID: 1 << 9,
} as const;

/**
 * cap 位 → 人类可读标签（供评估报告与 UI 复述用）。
 * 本表键为**位掩码值**（与 VIRTIO_GPU_CAP 的键同口径），非位序。
 * 口径纪律：位序（"1"/"4"）与掩码（2/16）是两套数，混用会让 Venus 被
 * 标成 VULKAN2 的名字而不影响判定——标签错、行为对，是最难查的一类缺陷。
 * 故此表与 VIRTIO_GPU_CAP 严格同键，改位时两处同步。
 */
export const VIRTIO_GPU_CAP_LABELS: Readonly<Record<number, string>> = {
  [VIRTIO_GPU_CAP.VENUS_TERMINAL_2D]: "2D 基线",
  [VIRTIO_GPU_CAP.VENUS]: "Venus（Vulkan 透传）",
  [VIRTIO_GPU_CAP.VIRGL]: "virgl（OpenGL 透传）",
  [VIRTIO_GPU_CAP.VULKAN2]: "VULKAN2（Venus 旧通路）",
  [VIRTIO_GPU_CAP.CURSOR_2D]: "cursor 2D",
  [VIRTIO_GPU_CAP.EDID]: "EDID",
};

/** 本条评估涉及的 cap 位（其余位不参与 Venus 判定）。 */
export const VENUS_RELEVANT_CAP_BITS: readonly number[] = [
  VIRTIO_GPU_CAP.VENUS,
  VIRTIO_GPU_CAP.VULKAN2,
];

/**
 * VULKAN2 与 VENUS 的代际关系（诚实标注的关键一环）。
 *
 * 历史上 virtio-gpu 有 VULKAN2 通路，后来被 Venus 取代。二者**不是叠加关系**：
 * host 若同时报两位，说明它是「两代都在」（老版本 host 兼容行为）；此时
 * 取VENUS（新一代）。若只报 VULKAN2 不报 VENUS，**不得**当作 Venus 可用——
 * 那是旧通路，能力集与协议都不同，拿它冒充 Venus 是典型的「假装能用」。
 * 结论：仅 VULKAN2 → 判定 UNSUPPORTED（四态之一，非 RESERVED_ONLY——宿主
 * 未提供 Venus 能力时不存在「预留」语义，预留的前提是 cap 在而栈缺）。
 */
export const VULKAN2_VENUS_GENERATION: {
  /** VULKAN2 是 Venus 的前一代，能力集与协议均不同。 */
  readonly sameGeneration: boolean;
  /**
   * 仅 VULKAN2 时的处置口径。
   * 类型刻意放宽为 `VenusCapabilityState`（而非 `as const` 的字面量收窄）：
   * 本表是**声明**，判定是**执行**，两者须能在运行期比对。若被收窄成
   * 字面量 "UNAVAILABLE"，则「是否等于 AVAILABLE」会被编译器判为永假比较
   * （TS2367），代际对拍守卫便成了死代码——这类缺陷最阴：编译能过、
   * 守卫永不触发、行为看起来完全正常。
   */
  readonly vulkan2OnlyVerdict: VenusCapabilityState;
  /** 两代同报时的选择。 */
  readonly preferWhenBoth: "VENUS";
} = {
  sameGeneration: false,
  vulkan2OnlyVerdict: "UNSUPPORTED",
  preferWhenBoth: "VENUS",
};

/* ═══════════════════════════════════════════════════════════════════════════
 * §4 上下文类型声明（VIRTIO_GPU_CTX_TYPE_VENUS · 声明而不创建）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** virtio-gpu 上下文类型枚举（与内核uapi 同值）。 */
export const VIRTIO_GPU_CTX_TYPE = {
  /** 0：virgl 上下文（F0205/F0206 通路）。 */
  VIRGL: 0,
  /** 1：Venus 单上下文。 */
  VENUS: 1,
  /** 2：Venus Vulkan 多上下文（每 context 一套 host 侧Vulkan 实例语义）。 */
  VENUS_VULKAN: 2,
} as const;

export type VirtioGpuCtxType =
  | typeof VIRTIO_GPU_CTX_TYPE.VIRGL
  | typeof VIRTIO_GPU_CTX_TYPE.VENUS
  | typeof VIRTIO_GPU_CTX_TYPE.VENUS_VULKAN;

/** 上下文类型中文标签。 */
export const CTX_TYPE_LABELS: Readonly<Record<number, string>> = {
  0: "virgl 上下文（OpenGL 透传）",
  1: "Venus 单上下文（Vulkan 透传）",
  2: "Venus Vulkan 多上下文（每上下文独立 Vulkan 语义）",
};

/**
 * 上下文声明：只声明、不创建。
 *
 * 「声明而不创建」是本条的纪律：声明是**编译期/能力表层面**的事实（我们
 * 知道自己有这两个类型位可用），创建是**运行时**事实（要真能在 host 上
 * 建出上下文）。两者混为一谈，就是「cap 探测通过就宣称能用」的起点。
 * 因此本结构体显式携带 `declarationOnly: true`，任何试图在此结构上
 * 直接下发 create 命令的调用方，都能在类型层面看到它不是可执行句柄。
 */
export interface ContextDeclaration {
  /** 上下文类型枚举值。 */
  readonly ctxType: VirtioGpuCtxType;
  /** 该类型在本系统中的声明状态。 */
  readonly declared: boolean;
  /** 恒为 true——本结构不是可执行创建句柄。 */
  readonly declarationOnly: true;
  /** 声明依据（必须可指到证据，指不到则不声明）。 */
  readonly evidence: string;
  /** 中文标签（可上屏）。 */
  readonly label: string;
}

/** 构造一条「仅声明」的上下文声明。 */
export function declareContext(
  ctxType: VirtioGpuCtxType,
  evidence: string,
): ContextDeclaration {
  return {
    ctxType,
    declared: evidence.length > 0,
    declarationOnly: true,
    evidence,
    label: CTX_TYPE_LABELS[ctxType] ?? `未知上下文类型 ${ctxType}`,
  };
}

/**
 * 上下文类型 → 该类型在当前能力态下是否**允许**声明。
 *
 * 纪律：cap 位与栈评估**两者都过**才允许 declared=true。缺任一 → 不声明，
 * 产出 P1。这样即使有人误把本函数当create 前的检查用，也不会拿到一个
 * 无依据的声明。
 */
export function contextDeclarationGate(input: {
  readonly capabilityState: VenusCapabilityState;
  readonly stackReady: boolean;
}): Outcome<readonly ContextDeclaration[]> {
  const diagnostics: Diagnostic[] = [];
  const state = input.capabilityState;
  const declarations: ContextDeclaration[] = [];

  if (state === "AVAILABLE" || state === "NEGOTIABLE") {
    if (!input.stackReady) {
      diagnostics.push({
        code: "VENUS_STACK_MISSING_BUT_CLAIMED",
        severity: "P0",
        message: `能力态判为 ${state} 但 guest 侧 Vulkan ICD 栈缺失，自相矛盾`,
        hint: "把能力态降为 RESERVED_ONLY；无栈不声明上下文类型",
        stage: "context-declaration",
      });
      return err("能力态与栈事实矛盾，拒绝声明上下文类型", diagnostics);
    }
    const evidence = `cap=${VENUS_CAP_EVIDENCE_PREFIX}${state} + ICD 栈齐备`;
    declarations.push(declareContext(VIRTIO_GPU_CTX_TYPE.VENUS, evidence));
    declarations.push(declareContext(VIRTIO_GPU_CTX_TYPE.VENUS_VULKAN, evidence));
  } else {
    diagnostics.push({
      code: "VENUS_NEGOTIATION_NOT_ATTEMPTED",
      severity: "P2",
      message: `能力态为 ${state}，本条不声明 Venus 上下文类型（诚实：不可用即不声明）`,
      hint: "待ICD 栈补齐后重跑本闸，届时才产出声明",
      stage: "context-declaration",
    });
  }

  return ok(declarations, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §5 能力探测（VIRTIO_GPU_CAP_VENUS · 四态判定）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * Venus 能力四态。
 * 命名纪律：无「部分支持」中间态（理由见文件头第一性声明）。
 */
export type VenusCapabilityState =
  /** host 未报 Venus cap（终态）。 */
  | "UNSUPPORTED"
  /** host 报了 cap，但 guest 缺ICD 栈 → 架构预留（**本系统当前态**）。 */
  | "RESERVED_ONLY"
  /** cap 在 + 栈齐备 → 可进入真实协商。 */
  | "NEGOTIABLE"
  /** 三者满足且协商成功 → 真实透传。 */
  | "AVAILABLE";

/** 四态中文标签（可上屏，三要素齐）。 */
export const CAPABILITY_STATE_LABELS: Readonly<Record<VenusCapabilityState, string>> = {
  UNSUPPORTED: "不可用：宿主未提供 Venus 能力",
  RESERVED_ONLY: "架构预留：宿主支持 Venus，但本系统尚无 Vulkan 驱动栈",
  NEGOTIABLE: "可协商：能力与驱动栈齐备，待真实协商",
  AVAILABLE: "可用： Venus 通路已协商成功",
};

/** 能力证据前缀（供 evidence 字段反查来源）。 */
export const VENUS_CAP_EVIDENCE_PREFIX = "VIRTIO_GPU_CAP_VENUS@";

/** 一次cap 探测的原始输入。 */
export interface CapProbeInput {
  /** 设备 feature 寄存器读回的 cap 位图（u32）。 */
  readonly deviceCaps: number;
  /** guest 侧 Vulkan 栈评估结论（来自 §6）。 */
  readonly stackReady: boolean;
  /** 评估时刻（用于报告留痕）。 */
  readonly now: number;
}

/** 能力探测结论。 */
export interface CapProbeResult {
  readonly state: VenusCapabilityState;
  /** VENUS 位是否置位。 */
  readonly venusBitSet: boolean;
  /** VULKAN2 位是否置位（旧代）。 */
  readonly vulkan2BitSet: boolean;
  /** 逐位可读结论（供报告与 UI 复述）。 */
  readonly bitVerdicts: readonly CapBitVerdict[];
  /** 支撑本结论的证据串（非空，否则判P0）。 */
  readonly evidence: string;
}

/** 单个 cap 位的判定。 */
export interface CapBitVerdict {
  readonly bit: number;
  readonly label: string;
  readonly set: boolean;
  /** 该位对 Venus 判定的贡献。 */
  readonly relevance: "TARGET" | "LEGACY" | "IGNORED";
  /** 若该位单独出现，是否足以支撑 Venus（仅 VULKAN2 不足以）。 */
  readonly sufficientAlone: boolean;
}

/**
 * 能力探测（四态判定）。
 *
 * 判定序（顺序不可调换）：
 *   ① 边界：deviceCaps 越界（负数/非 uint32 语义）→ 直接拒绝，不猜；
 *   ② VENUS 位未置位→ UNSUPPORTED（终态；即便栈齐备也不改变）；
 *   ③ 仅 VULKAN2 置位→ UNSUPPORTED（代际不符，理由见 §3）；
 *   ④ VENUS 置位但栈缺→ RESERVED_ONLY（**本系统当前态**）；
 *   ⑤ VENUS 置位且栈齐备→ NEGOTIABLE（真实协商由 §7 完成后可升 AVAILABLE）。
 *
 * 为什么要 ⑤ 不直接给 AVAILABLE：cap 与栈只是**必要条件**，版本区间
 * 仍可能无交集。把AVAILABLE 的判定推迟到协商之后，是为了让「可用」这个
 * 词在系统里只有一个出处——协商成功那一条路径。
 */
export function probeVenusCapability(input: CapProbeInput): Outcome<CapProbeResult> {
  const diagnostics: Diagnostic[] = [];
  const caps = input.deviceCaps;

  if (!Number.isInteger(caps) || caps < 0 || caps > 0xffffffff) {
    diagnostics.push({
      code: "VENUS_CAP_ABSENT_ON_HOST",
      severity: "P1",
      message: `设备 cap 位图越界（读回 ${caps}），不在 uint32 合法域`,
      hint: "重读设备配置空间；越界值不可用于任何能力判定，先停手归因",
      stage: "cap-probe",
    });
    return err("cap 位图越界，拒绝能力判定", diagnostics);
  }

  const venusBitSet = (caps & VIRTIO_GPU_CAP.VENUS) !== 0;
  const vulkan2BitSet = (caps & VIRTIO_GPU_CAP.VULKAN2) !== 0;

  // 判定前先与代际登记表对拍：若日后有人改动 VULKAN2_VENUS_GENERATION
  // 的处置口径（如改成「VULKAN2 也算可用」），而本函数未同步，守卫即报 P0。
  // 这条对拍的意义在于：代际口径是**声明**，判定是**执行**，两者必须同源。
  if (vulkan2BitSet && VULKAN2_VENUS_GENERATION.vulkan2OnlyVerdict === "AVAILABLE") {
    diagnostics.push({
      code: "VENUS_CAPABILITY_CLAIM_UNPROVEN",
      severity: "P0",
      message: "代际登记与 cap 判定口径冲突：登记表称仅 VULKAN2 亦可用，判定函数按不可用处置",
      hint: "二者必须同源；确认代际结论后同步修正登记或判定，不允许两处各说一套",
      stage: "cap-probe",
    });
    return err("代际登记与判定口径冲突，拒绝能力判定", diagnostics);
  }

  //逐位判定由 VENUS_RELEVANT_CAP_BITS 驱动生成，不在此处二次登记位号——
  // 位号若在两处各写一份，日后改位就会只改一处，判定与登记静默分叉。
  const bitVerdicts: CapBitVerdict[] = VENUS_RELEVANT_CAP_BITS.map((bit) => {
    const isTarget = bit === VIRTIO_GPU_CAP.VENUS;
    return {
      bit,
      label: VIRTIO_GPU_CAP_LABELS[bit] ?? `cap 位 ${bit}`,
      set: (caps & bit) !== 0,
      relevance: isTarget ? "TARGET" : "LEGACY",
      // 仅目标位可单独支撑 Venus；旧代位无论是否置位都不足以支撑。
      sufficientAlone: isTarget && VULKAN2_VENUS_GENERATION.vulkan2OnlyVerdict !== "AVAILABLE",
    };
  });

  let state: VenusCapabilityState;
  if (!venusBitSet) {
    state = "UNSUPPORTED";
    const why = vulkan2BitSet
      ? "宿主仅报 VULKAN2（Venus 旧代通路），代际不符，不冒充 Venus"
      : "宿主未置 VIRTIO_GPU_CAP_VENUS";
    diagnostics.push({
      code: "VENUS_CAP_ABSENT_ON_HOST",
      severity: "P1",
      message: why,
      hint: "宿主未提供 Venus 能力，本通路在本宿主上终态不可用；如需 Venus 请换宿主或启用 Venus 后端",
      stage: "cap-probe",
    });
  } else if (!input.stackReady) {
    state = "RESERVED_ONLY";
    diagnostics.push({
      code: "VENUS_ICD_STACK_ABSENT",
      severity: "P1",
      message: "宿主已提供 Venus 能力，但本系统缺完整 Vulkan ICD 栈，本通路仅做架构预留",
      hint: "按依赖清单补齐 loader/ICD/WSI 栈后重跑评估；在此之前不得宣称 Venus 可用",
      stage: "cap-probe",
    });
  } else {
    state = "NEGOTIABLE";
  }

  const evidence =
    state === "UNSUPPORTED"
      ? `${VENUS_CAP_EVIDENCE_PREFIX}未置位 (caps=0x${caps.toString(16)})`
      : `${VENUS_CAP_EVIDENCE_PREFIX}置位 (caps=0x${caps.toString(16)}) + stack=${
          input.stackReady ? "ready" : "absent"
        }`;

  return ok({ state, venusBitSet, vulkan2BitSet, bitVerdicts, evidence }, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §6 guest 侧 Vulkan 驱动栈存在性评估（本条的主结论来源）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * Vulkan 栈分层（自上而下，缺任一层则整栈不可用）。
 * 分层不是学术洁癖：venus 的**每一层都在不同的地方**，缺的层次决定
 * 缺谁、找哪份依赖清单条目。把「缺 loader」与「缺 ICD」混成一句
 * 「没有 Vulkan 支持」，排期时就会去找错的人。
 */
export type VulkanStackLayer =
  /** 应用侧：Vulkan 头文件派生的加载与分发。 */
  | "LOADER"
  /** 驱动侧：virtio-Venus ICD 实现（ioctl → venus ring）。 */
  | "ICD"
  /** 传输侧：venus 共享内存环（ctrl/ack/error ring 建立）。 */
  | "TRANSPORT"
  /** 窗口侧：WSI 表面与显示输出对接。 */
  | "WSI"
  /** 着色器侧：SPIR-V 生成与缓存（VE-C 着色器域供给）。 */
  | "SHADERS";

/** 层中文标签。 */
export const STACK_LAYER_LABELS: Readonly<Record<VulkanStackLayer, string>> = {
  LOADER: "Vulkan 加载器（分发 ICD 与实例创建）",
  ICD: "virtio-Venus 驱动（ICD，ioctl 下发 venus 命令）",
  TRANSPORT: "Venus 传输层（共享内存环 ctrl/ack/error）",
  WSI: "窗口系统集成（WSI 表面与显示输出）",
  SHADERS: "着色器供给（SPIR-V 生成与缓存）",
};

/** 栈层序（自上而下的判定顺序，用于「首个缺项」定位）。 */
export const VULKAN_STACK_LAYER_ORDER: readonly VulkanStackLayer[] = [
  "LOADER",
  "ICD",
  "TRANSPORT",
  "WSI",
  "SHADERS",
];

/** 单层评估结论。 */
export interface StackLayerVerdict {
  readonly layer: VulkanStackLayer;
  readonly present: boolean;
  readonly label: string;
  /** 缺项时：本层向谁要（域名+ 条目号），供依赖清单反向驱动排期。 */
  readonly ownerDomain: string;
  readonly ownerEntryHint: string;
}

/** 栈评估输入。 */
export interface StackProbeInput {
  /** 逐层存在性（本系统当前为全 false——VARIX 无 Vulkan 栈）。 */
  readonly presentLayers: Readonly<Record<VulkanStackLayer, boolean>>;
  readonly now: number;
}

/** 栈评估结论。 */
export interface StackProbeResult {
  /** 整栈是否可用。 */
  readonly stackReady: boolean;
  /** 逐层判定。 */
  readonly verdicts: readonly StackLayerVerdict[];
  /** 首个缺项的层（自上而下）；全齐时为 null。 */
  readonly firstMissingLayer: VulkanStackLayer | null;
  /** 缺项总数（供报告与排期量级感）。 */
  readonly missingCount: number;
  /** 可上屏的不可用原因（诚实标注的**人话位**）。 */
  readonly unavailableReason: string;
}

/** 各层归属方（本域不越界替别域承诺，只登记「谁该补」）。 */
const STACK_LAYER_OWNERS: Readonly<
  Record<VulkanStackLayer, { readonly domain: string; readonly hint: string }>
> = {
  LOADER: { domain: "VE-B", hint: "F0207 依赖清单 LOADER-ICD-01（本条登记，VE-B 自建）" },
  ICD: { domain: "VE-B", hint: "F0207 依赖清单 ICD-01（virtio-Venus 驱动实现）" },
  TRANSPORT: { domain: "VE-B", hint: "F0207 依赖清单 TRANSPORT-01（venus 环，复用 F0202 队列协议）" },
  WSI: { domain: "VE-B", hint: "F0207 依赖清单 WSI-01（表面管道对接，见 §9 架构位）" },
  SHADERS: { domain: "VE-C", hint: "VE-C 着色器域（SPIR-V 生成；CGPU-K 供给着色器缓存位）" },
};

/**
 * guest 侧 Vulkan 驱动栈存在性评估。
 *
 * 结论诚实性由两处保证：
 *   ① `stackReady` 必须**全部层齐备**才为 true（不允许「差不多齐」）；
 *   ② 缺项时 `unavailableReason` 必须是**可上屏的人话**（缺哪层、归谁），
 *     不是「不支持」三个字。
 *
 * 性能：O(层数)= O(5)，层集为编译期常量表——**不是**遍历文件系统。
 * 真实文件系统探测属运行时职责（F0201 初始化期一次性），其结论喂进
 * `presentLayers`；本函数只做**架构期的事实判定与诚实标注**。
 */
export function probeGuestVulkanStack(input: StackProbeInput): Outcome<StackProbeResult> {
  const diagnostics: Diagnostic[] = [];
  const verdicts: StackLayerVerdict[] = [];
  let firstMissing: VulkanStackLayer | null = null;
  let missing = 0;

  for (const layer of VULKAN_STACK_LAYER_ORDER) {
    const present = input.presentLayers[layer] === true;
    const owner = STACK_LAYER_OWNERS[layer];
    if (!present) {
      missing += 1;
      if (firstMissing === null) firstMissing = layer;
    }
    verdicts.push({
      layer,
      present,
      label: STACK_LAYER_LABELS[layer],
      ownerDomain: owner.domain,
      ownerEntryHint: owner.hint,
    });
  }

  const stackReady = missing === 0;
  const unavailableReason = stackReady
    ? "五层齐备"
    : `${missing} 层缺失，首个缺项为「${STACK_LAYER_LABELS[firstMissing ?? "LOADER"]}」` +
      `（归${STACK_LAYER_OWNERS[firstMissing ?? "LOADER"].domain}）`;

  if (!stackReady) {
    diagnostics.push({
      code: "VENUS_ICD_STACK_ABSENT",
      severity: "P1",
      message: `guest 侧 Vulkan 驱动栈不完整：${unavailableReason}`,
      hint: " venus 在本系统只能做架构预留；补齐依赖清单所列各层后方可进入真实协商",
      stage: "stack-probe",
    });
  }

  return ok(
    {
      stackReady,
      verdicts,
      firstMissingLayer: firstMissing,
      missingCount: missing,
      unavailableReason,
    },
    diagnostics,
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §7 venus 协议版本协商（三元组区间求交）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * venus ABI 版本三元组。
 * 语义（与内核 `venus_version` 一致）：
 *   · min    —— 宿主实现的最低 ABI；
 *   · max    —— 宿主实现的最高 ABI；
 *   · ask    —— 宿主**建议**使用的 ABI（宿主填充，guest 侧读）。
 * 三者语义不同：min/max 是区间，ask 是建议。**把 ask 当上限是常见错**——
 * 宿主给的建议可能落在其自身区间内但 guest 不支持，用ask 覆盖 guest 意愿
 * 会导致协商出一个双方都没实现的版本。
 */
export interface VenusVersionTriple {
  readonly min: number;
  readonly max: number;
  readonly ask: number;
}

/** 本条已知的 Venus ABI 版本上界（低于此值的行为未验证）。 */
export const VENUS_KNOWN_ABI_UPPER = 4;

/** 版本区间求交结果。 */
export interface VersionNegotiation {
  /** 交集区间（非空才可用）。 */
  readonly overlapMin: number;
  readonly overlapMax: number;
  /** 最终选定的 ABI（ask 落在交集内则取 ask，否则钳到交集下界）。 */
  readonly chosen: number;
  /** 是否实际发起过协商（预留态下为 false——**诚实**：不假装协商过）。 */
  readonly attempted: boolean;
  /** 是否被钳制（ask 高于交集上界时被钳制，仅记账不阻断）。 */
  readonly clamped: boolean;
  /** 人类可读结论。 */
  readonly summary: string;
}

/**
 * 版本区间求交 + ABI 选择。
 *
 * 错误路径与处置矩阵：
 *   · min > max（畸形三元组）→ P1 拒绝，不做「宽容交换」——畸形输入的
 *     正确处置是停下归因，不是猜一个值继续；
 *   · 交集为空 → P1 不可协商（须换宿主版本或回退软渲）；
 *   · ask 高于已知上界 →钳到交集上界，P2 记账继续（可走，仅记账）；
 *   · ask 不在交集内 → 钳到交集下界，P2 记账（宿主建议不可用时的正解）。
 */
export function negotiateVenusVersion(
  host: VenusVersionTriple,
  guest: VenusVersionTriple,
  capabilityState: VenusCapabilityState,
): Outcome<VersionNegotiation> {
  const diagnostics: Diagnostic[] = [];

  for (const [who, t] of [
    ["宿主", host],
    ["guest", guest],
  ] as const) {
    if (!Number.isInteger(t.min) || !Number.isInteger(t.max) || !Number.isInteger(t.ask)) {
      diagnostics.push({
        code: "VENUS_NO_VERSION_OVERLAP",
        severity: "P1",
        message: `${who}版本三元组非法（min=${t.min} max=${t.max} ask=${t.ask}）`,
        hint: "读回值非整数，拒绝协商；先停手归因设备配置读取路径",
        stage: "version-negotiate",
      });
      return err("版本三元组非法，拒绝协商", diagnostics);
    }
    if (t.min > t.max) {
      diagnostics.push({
        code: "VENUS_NO_VERSION_OVERLAP",
        severity: "P1",
        message: `${who}版本区间畸形（min=${t.min} > max=${t.max}）`,
        hint: "不做宽容交换；修正设备侧版本字段后重试",
        stage: "version-negotiate",
      });
      return err("版本区间畸形，拒绝协商", diagnostics);
    }
  }

  const overlapMin = Math.max(host.min, guest.min);
  const overlapMax = Math.min(host.max, guest.max, VENUS_KNOWN_ABI_UPPER);

  if (overlapMin > overlapMax) {
    diagnostics.push({
      code: "VENUS_NO_VERSION_OVERLAP",
      severity: "P1",
      message: `版本区间无交集：宿主 [${host.min},${host.max}] ∩ guest [${guest.min},${guest.max}] ∩ 已知上界 ${VENUS_KNOWN_ABI_UPPER} = 空`,
      hint: "协商不可进行；须升级宿主 Venus、调整 guest ABI 或按 F0013 回退软渲",
      stage: "version-negotiate",
    });
    return err("版本区间无交集", diagnostics);
  }

  const attempted = capabilityState === "AVAILABLE" || capabilityState === "NEGOTIABLE";
  if (!attempted) {
    diagnostics.push({
      code: "VENUS_NEGOTIATION_NOT_ATTEMPTED",
      severity: "P2",
      message: `能力态为 ${capabilityState}，仅做区间求交不发起真实协商`,
      hint: "本记录为架构预留结论，不代表已协商成功",
      stage: "version-negotiate",
    });
  }

  let chosen = host.ask;
  let clamped = false;
  if (chosen > overlapMax) {
    chosen = overlapMax;
    clamped = true;
  }
  if (chosen < overlapMin) {
    chosen = overlapMin;
    clamped = true;
  }
  if (host.ask > VENUS_KNOWN_ABI_UPPER) {
    diagnostics.push({
      code: "VENUS_ABI_CLAMPED",
      severity: "P2",
      message: `宿主建议 ABI ${host.ask} 高于本条已知上界 ${VENUS_KNOWN_ABI_UPPER}，已钳制到 ${chosen}`,
      hint: "高于已知上界的 ABI 行为未验证；钳制后可走，如需新版请先补验证用例",
      stage: "version-negotiate",
    });
  }

  return ok(
    {
      overlapMin,
      overlapMax,
      chosen,
      attempted,
      clamped,
      summary: attempted
        ? `交集 [${overlapMin},${overlapMax}]，选定 ABI ${chosen}${clamped ? "（已钳制）" : ""}`
        : `交集 [${overlapMin},${overlapMax}]，选定 ABI ${chosen}（预留态，未发起真实协商）`,
    },
    diagnostics,
  );
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §8 依赖清单（反向驱动 VE-C 着色器域与 CGPU-K 排期）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 依赖项类别。 */
export type DependencyKind =
  /** Vulkan 加载器与 ICD 派生物。 */
  | "ICD"
  /** venus 传输（共享内存环 / ioctl 通路）。 */
  | "TRANSPORT"
  /** 同步原语（fence / timeline semaphore 跨guest-host 传递）。 */
  | "SYNC"
  /** 着色器供给（SPIR-V；VE-C 域与 CGPU-K 排期入口）。 */
  | "SHADERS"
  /** 表面与呈现对接（F0209 承接面）。 */
  | "PRESENT"
  /** 验证用例（一致性测试 Venus 档）。 */
  | "VALIDATION";

/** 依赖项状态。 */
export type DependencyState = "PENDING" | "IN_PROGRESS" | "DONE" | "DEFERRED";

/** 依赖项状态标签。 */
export const DEPENDENCY_STATE_LABELS: Readonly<Record<DependencyState, string>> = {
  PENDING: "待开工",
  IN_PROGRESS: "施工中",
  DONE: "已完成",
  DEFERRED: "已登记顺延",
};

/** 一条依赖项。 */
export interface DependencyItem {
  readonly id: string;
  readonly kind: DependencyKind;
  /** 中文描述（可上屏）。 */
  readonly title: string;
  /** 责任域。 */
  readonly ownerDomain: string;
  /** 阻断级别：true=缺此项则 Venus 永不可用；false=可延后。 */
  readonly blocking: boolean;
  readonly state: DependencyState;
  /** 缺失时的可上屏影响说明。 */
  readonly impactIfMissing: string;
}

/**
 * 依赖清单（本条核心交付之一，进台账）。
 *
 * 「反向驱动排期」的含义：这份清单不是给本域自己看的待办，而是**别域的
 * 排期输入**——VE-C 着色器域据此决定要不要投SPIR-V 编译链，CGPU-K 据此
 * 决定要不要留着色器缓存位。所以每条必须有 `ownerDomain` 与
 * `impactIfMissing`：没有归属的依赖项等于没有排期，等于没登记。
 */
export const VENUS_DEPENDENCY_LEDGER: readonly DependencyItem[] = [
  {
    id: "VENUS-ICD-01",
    kind: "ICD",
    title: "virtio-Venus 驱动（ICD）实现：ioctl → venus 命令编码",
    ownerDomain: "VE-B",
    blocking: true,
    state: "PENDING",
    impactIfMissing: "无ICD 则 guest 无法下发任何 Venus 命令，Venus 通路整体不可用",
  },
  {
    id: "VENUS-LOADER-01",
    kind: "ICD",
    title: "Vulkan 加载器：ICD 发现与实例/设备创建分发",
    ownerDomain: "VE-B",
    blocking: true,
    state: "PENDING",
    impactIfMissing: "无加载器则应用侧无入口，ICD 即便存在也无人装载",
  },
  {
    id: "VENUS-TRANSPORT-01",
    kind: "TRANSPORT",
    title: "Venus 传输层：ctrl/ack/error 共享内存环（复用 F0202 队列协议）",
    ownerDomain: "VE-B",
    blocking: true,
    state: "PENDING",
    impactIfMissing: "无传输层则命令无法送到 host，协商与渲染均不可进行",
  },
  {
    id: "VENUS-SYNC-01",
    kind: "SYNC",
    title: "跨 guest/host 同步：fence 与 timeline semaphore 传递",
    ownerDomain: "VE-B",
    blocking: false,
    state: "PENDING",
    impactIfMissing: "缺同步则渲染可跑但结果可能读到未完成数据，表现为间歇性花屏",
  },
  {
    id: "VENUS-SHADERS-01",
    kind: "SHADERS",
    title: "SPIR-V 生成与缓存（着色器供给；VE-C 域 + CGPU-K 缓存位）",
    ownerDomain: "VE-C",
    blocking: true,
    state: "PENDING",
    impactIfMissing: "无着色器则管线无法创建，Venus 通路即使传输层齐备也不可用",
  },
  {
    id: "VENUS-PRESENT-01",
    kind: "PRESENT",
    title: "Venus 产物挂入既有表面管道并经 F0209 SET_SCANOUT 呈现",
    ownerDomain: "VE-B",
    blocking: true,
    state: "PENDING",
    impactIfMissing: "无呈现对接则渲染成功但用户看不到（渲染与呈现断层）",
  },
  {
    id: "VENUS-VALIDATION-01",
    kind: "VALIDATION",
    title: "Venus 档一致性用例（协议/功能/恢复三层；宿主 fast 与 QEMU full 双档）",
    ownerDomain: "VE-B",
    blocking: false,
    state: "PENDING",
    impactIfMissing: "缺验证则无法判定真实可用，F0220 收口时 Venus 档只能标未认证",
  },
];

/** 依赖清单核验：ID 唯一 + 阻断项均未完成时不得声称可用。 */
export function verifyDependencyLedger(
  ledger: readonly DependencyItem[] = VENUS_DEPENDENCY_LEDGER,
): Outcome<readonly DependencyItem[]> {
  const diagnostics: Diagnostic[] = [];
  const seen = new Set<string>();

  for (const item of ledger) {
    if (item.id.length === 0) {
      diagnostics.push({
        code: "VENUS_DEPENDENCY_ITEM_MISSING",
        severity: "P1",
        message: "依赖项缺少 ID，无法进台账",
        hint: "补ID；无 ID 的依赖项排期时无法被引用",
        stage: "dependency-ledger",
      });
    } else if (seen.has(item.id)) {
      diagnostics.push({
        code: "VENUS_DEPENDENCY_ITEM_MISSING",
        severity: "P1",
        message: `依赖项 ID 重复：${item.id}`,
        hint: "去重；重复 ID 会让排期引用指向不明",
        stage: "dependency-ledger",
      });
    }
    seen.add(item.id);
    if (item.ownerDomain.length === 0) {
      diagnostics.push({
        code: "VENUS_DEPENDENCY_ITEM_MISSING",
        severity: "P1",
        message: `依赖项 ${item.id || "(无ID)"} 未指定责任域，等于没有排期`,
        hint: "指定 ownerDomain；无归属的依赖项不会驱动任何域的排期",
        stage: "dependency-ledger",
      });
    }
  }

  const blockingPending = ledger.filter((i) => i.blocking && i.state !== "DONE");
  if (blockingPending.length > 0) {
    diagnostics.push({
      code: "VENUS_DEPENDENCY_ITEM_MISSING",
      severity: "P1",
      message: `阻断级依赖仍有 ${blockingPending.length} 项未完成：${blockingPending
        .map((i) => i.id)
        .join(" / ")}`,
      hint: "阻断项未齐前Venus 通路只能标 RESERVED_ONLY，不得声称可用",
      stage: "dependency-ledger",
    });
  }

  return ok(ledger, diagnostics);
}

/**
 * 依赖台账的人话复述（可上屏，供报告与排期面板引用）。
 * 纪律：状态必须带中文标签与影响说明，不允许只列 ID —— 只列 ID 的台账
 * 读起来像一串字符，看不出「缺它会怎样」，排期时就没人当回事。
 */
export function describeDependencyLedger(
  ledger: readonly DependencyItem[] = VENUS_DEPENDENCY_LEDGER,
): readonly string[] {
  return ledger.map((i) => {
    const stateLabel = DEPENDENCY_STATE_LABELS[i.state];
    const blockMark = i.blocking ? "阻断级" : "非阻断";
    return `${i.id}［${blockMark}·${stateLabel}］${i.title} —— 归${i.ownerDomain}；缺则${i.impactIfMissing}`;
  });
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §9 架构位与表面管道对接预留（venus 就绪后经同一表面管道输出）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 架构位段。 */
export type ArchSlot =
  /** 提交面：Venus 渲染产物挂入表面管道的入口。 */
  | "SUBMIT"
  /** 呈现面：绑定 F0209 SET_SCANOUT 序（不新开呈现路径）。 */
  | "PRESENT"
  /** 回落面：不可用时回落软渲（A 域 F0013）并显性通知。 */
  | "FALLBACK";

/** 架构位段标签。 */
export const ARCH_SLOT_LABELS: Readonly<Record<ArchSlot, string>> = {
  SUBMIT: "提交面：Venus 渲染产物挂入表面管道",
  PRESENT: "呈现面：经 F0209 SET_SCANOUT 输出（与 virgl 共用同一条呈现路径）",
  FALLBACK: "回落面：不可用时回落 A 域软渲并显性通知",
};

/** 一个架构位。 */
export interface ArchSlotSpec {
  readonly slot: ArchSlot;
  readonly label: string;
  /** 承接条目（锚点来源，便于回溯）。 */
  readonly anchorEntryId: number;
  /** 本条是否已实现（预留位为 false——诚实：不假装已接）。 */
  readonly implemented: boolean;
  /** 预留说明（implemented=false 时必填）。 */
  readonly reservation: string;
}

/**
 * 架构位预留表（三段全部 implemented=false）。
 *
 * 「预留而不实现」是本条的核心诚实动作：三段都已想清楚接哪里、由谁接、
 * 什么条件下接，但**现在不接**。原因见文件头架构位说明——在 venus 真可用
 * 之前另开呈现路径会与 virgl 分叉。
 */
export const ARCH_SLOTS: readonly ArchSlotSpec[] = [
  {
    slot: "SUBMIT",
    label: ARCH_SLOT_LABELS.SUBMIT,
    anchorEntryId: 60,
    implemented: false,
    reservation: "表面协议（F0060）预留 Venus 产物类型位；接入条件：ICD + 传输层 + 着色器三阻断项 DONE",
  },
  {
    slot: "PRESENT",
    label: ARCH_SLOT_LABELS.PRESENT,
    anchorEntryId: 209,
    implemented: false,
    reservation: "复用 F0209 SET_SCANOUT 序，不新增呈现路径；接入条件：呈现对接依赖 VENUS-PRESENT-01 DONE",
  },
  {
    slot: "FALLBACK",
    label: ARCH_SLOT_LABELS.FALLBACK,
    anchorEntryId: 13,
    implemented: false,
    reservation: "回落 A 域 F0013 软渲；回落必须产出 P2 通知，不得静默降级",
  },
];

/** 架构位核验：预留位必须带说明；已接段必须指到锚点条目。 */
export function verifyArchSlots(
  slots: readonly ArchSlotSpec[] = ARCH_SLOTS,
): Outcome<readonly ArchSlotSpec[]> {
  const diagnostics: Diagnostic[] = [];
  const covered = new Set<ArchSlot>();

  for (const s of slots) {
    if (covered.has(s.slot)) {
      diagnostics.push({
        code: "VENUS_ARCH_SLOT_MISWIRED",
        severity: "P1",
        message: `架构位段重复登记：${s.slot}`,
        hint: "每段只登记一次；重复会让回落面与呈现面关系不明",
        stage: "arch-slot",
      });
    }
    covered.add(s.slot);
    if (!s.implemented && s.reservation.length === 0) {
      diagnostics.push({
        code: "VENUS_ARCH_SLOT_MISWIRED",
        severity: "P1",
        message: `架构位 ${s.slot} 标为未实现但未写预留说明`,
        hint: "预留位必须写明接入条件与承接条目，否则等于凭空留坑",
        stage: "arch-slot",
      });
    }
    if (s.anchorEntryId <= 0) {
      diagnostics.push({
        code: "VENUS_ARCH_SLOT_MISWIRED",
        severity: "P1",
        message: `架构位 ${s.slot} 未指到承接条目`,
        hint: "补 anchorEntryId；无承接条目的预留位是悬空设计",
        stage: "arch-slot",
      });
    }
  }

  for (const slot of ["SUBMIT", "PRESENT", "FALLBACK"] as const) {
    if (!covered.has(slot)) {
      diagnostics.push({
        code: "VENUS_ARCH_SLOT_MISWIRED",
        severity: "P1",
        message: `架构位段缺失：${slot}`,
        hint: "三段（提交/呈现/回落）必须齐备，缺一则降级路径不明",
        stage: "arch-slot",
      });
    }
  }

  return ok(slots, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §10 评估报告（诚实标注的正式产出）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * Venus 通路评估报告（本条的主交付物）。
 *
 * 报告的**首要字段**是 `unavailableReason`（不可用原因），而不是
 * `verdict`（结论）。理由：结论是机器读的，原因是人读的；把原因放在
 * 前面，是为了防止后续有人只取结论渲染成「已支持」而不给原因。
 */
export interface VenusAssessmentReport {
  readonly reportId: string;
  /** 结论（四态之一）。 */
  readonly verdict: VenusCapabilityState;
  /** 结论标签（可上屏）。 */
  readonly verdictLabel: string;
  /** 不可用原因（可上屏人话；可用态时为「五层齐备且协商成功」）。 */
  readonly unavailableReason: string;
  /** 支撑证据（非空；空则判P0）。 */
  readonly evidence: readonly string[];
  /** 上下文类型声明（本条只声明不创建）。 */
  readonly contextDeclarations: readonly ContextDeclaration[];
  /** 栈评估结论。 */
  readonly stack: StackProbeResult;
  /** 版本协商结论。 */
  readonly negotiation: VersionNegotiation;
  /** 依赖清单（进台账，反向驱动排期）。 */
  readonly dependencyLedger: readonly DependencyItem[];
  /** 架构位预留。 */
  readonly archSlots: readonly ArchSlotSpec[];
  /** 全部诊断。 */
  readonly diagnostics: readonly Diagnostic[];
  /** 评估时刻。 */
  readonly assessedAt: number;
}

/** 评估报告构造输入。 */
export interface AssessmentInput {
  readonly caps: CapProbeInput;
  readonly stackInput: StackProbeInput;
  readonly hostVersion: VenusVersionTriple;
  readonly guestVersion: VenusVersionTriple;
  /**
   * 调用方（能力表侧）**打算对外呈现**的「Venus 已支持」标记。
   * 纪律：本字段由调用方如实提供，不得由本模块按 verdict 自动填成
   * `verdict === "AVAILABLE"`——那会让声称与事实恒等、一致性闸退化为恒真。
   * 典型诚实用法：本系统当前恒传 false（位已登记但未认证）。
   */
  readonly advertiseSupport: boolean;
  readonly now: number;
}

/** 报告 ID 生成（可重放：同输入同 ID，便于引用与对账）。 */
function reportIdOf(now: number, caps: number): string {
  return `VENUS-ASSESS-${now.toString(16)}-${caps.toString(16)}`;
}

/**
 * Venus 通路评估（编排：栈评估 → cap 探测 → 版本协商 → 声明闸 →
 *   依赖台账 → 架构位→ 报告）。
 *
 * 编排序不可调换的理由：cap 探测需要栈结论作输入（RESERVED_ONLY 的
 * 判定依赖栈），而版本协商需要 cap 态决定是否「真实协商」。若先做版本
 * 协商再探测，预留态下就会产出一条「已协商」记录——这正是要避免的假象。
 */
export function assessVenusPathway(input: AssessmentInput): Outcome<VenusAssessmentReport> {
  const diagnostics: Diagnostic[] = [];

  const stack = probeGuestVulkanStack(input.stackInput);
  if (!stack.ok) {
    return err(`栈评估失败：${stack.message}`, [...diagnostics, ...stack.diagnostics]);
  }
  diagnostics.push(...stack.diagnostics);

  const cap = probeVenusCapability({ ...input.caps, stackReady: stack.value.stackReady });
  if (!cap.ok) {
    return err(`cap 探测失败：${cap.message}`, [...diagnostics, ...cap.diagnostics]);
  }
  diagnostics.push(...cap.diagnostics);

  const negotiation = negotiateVenusVersion(
    input.hostVersion,
    input.guestVersion,
    cap.value.state,
  );
  if (!negotiation.ok) {
    // 版本无交集**不**直接判死：cap 与栈若仍可用，走回落面即可。
    diagnostics.push(...negotiation.diagnostics);
    return err(`版本协商失败：${negotiation.message}`, diagnostics);
  }
  diagnostics.push(...negotiation.diagnostics);

  const declarations = contextDeclarationGate({
    capabilityState: cap.value.state,
    stackReady: stack.value.stackReady,
  });
  if (!declarations.ok) {
    return err(`上下文声明闸拒绝：${declarations.message}`, [...diagnostics, ...declarations.diagnostics]);
  }
  diagnostics.push(...declarations.diagnostics);

  const ledger = verifyDependencyLedger();
  diagnostics.push(...ledger.diagnostics);

  const slots = verifyArchSlots();
  diagnostics.push(...slots.diagnostics);

  // B1 组条目连续性开工闸（组内跳号会让剩余工作量无法统计）。
  const continuity = verifyB1Continuity();
  diagnostics.push(...continuity.diagnostics);

  // 能力位对外声称一致性闸：报告的 verdict 会被能力表侧拿去呈现，
  // 因此这里用「报告结论是否会被呈现为已支持」作声称值传入（**独立于**
  // 内部事实，不在核验函数内反推），确保 P0 守卫在真实路径上同样生效。
  const willBeAdvertisedAsSupport = input.advertiseSupport;
  const capBit = verifyCapBitRegistration(cap.value.state, willBeAdvertisedAsSupport);
  diagnostics.push(...capBit.diagnostics);

  // 结论一致性闸：声称可用必须有证据（防止无据声称混入）。
  const evidenceList: string[] = [cap.value.evidence, ...cap.value.bitVerdicts.map((b) => `${b.label}=${b.set ? "置位" : "未置位"}`)];
  const claimsUsable = cap.value.state === "AVAILABLE" || cap.value.state === "NEGOTIABLE";
  if (claimsUsable && evidenceList.length === 0) {
    diagnostics.push({
      code: "VENUS_VERDICT_WITHOUT_EVIDENCE",
      severity: "P0",
      message: `结论为 ${cap.value.state} 但证据列表为空`,
      hint: "无据声称可用是本域最恶劣缺陷；宁可标不可用",
      stage: "report",
    });
  }
  if (hasBlocking({ ok: true, value: null, diagnostics })) {
    return err("存在阻断级诊断，拒绝出具报告", diagnostics);
  }

  const verdict = cap.value.state;
  const unavailableReason = verdict === "AVAILABLE"
    ? "五层齐备且协商成功"
    : verdict === "NEGOTIABLE"
      ? `能力与栈齐备，${negotiation.value.summary}`
      : verdict === "RESERVED_ONLY"
        ? `${stack.value.unavailableReason}；宿主已提供 Venus 能力，故为架构预留而非不可用`
        : stack.value.unavailableReason;

  const report: VenusAssessmentReport = {
    reportId: reportIdOf(input.now, input.caps.deviceCaps),
    verdict,
    verdictLabel: CAPABILITY_STATE_LABELS[verdict],
    unavailableReason,
    evidence: evidenceList,
    contextDeclarations: declarations.value,
    stack: stack.value,
    negotiation: negotiation.value,
    dependencyLedger: ledger.ok ? ledger.value : VENUS_DEPENDENCY_LEDGER,
    archSlots: slots.ok ? slots.value : ARCH_SLOTS,
    diagnostics,
    assessedAt: input.now,
  };

  return ok(report, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §11 能力位图登记（落A 域 F0007 能力位图 · 记账级）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** A 域能力位图中的 Venus 位（登记位，不含语义承诺）。 */
export const A_DOMAIN_VENUS_CAPBIT = {
  bitName: "VENUS_VULKAN_PASSTHROUGH",
  /** 位号（与A 域能力位图协作，本条只登记名与来源，不独占位号分配权）。 */
  bitIndex: 0,
  sourceEntryId: 207,
  /** 登记态与实际能力态必须一致——不一致即记账告警。 */
  registered: true,
} as const;

/**
 * 能力位登记一致性核验。
 * 登记了 Venus 位 ≠ Venus 可用。本函数核验「对外声称」与「探测事实」是否同源：
 * 若能力位对外呈现为「已支持」而探测为 RESERVED_ONLY / UNSUPPORTED，产出 P0
 * （这正是「假装能用」在能力表层的形态）。
 *
 * 参数纪律（关键）：`advertisedSupport` 必须是**能力表侧实际对外呈现的值**，
 * 由调用方传入；**不得**在本函数内部由 verdict 反推。反推会让声称与事实
 * 恒等，守卫退化为恒真——P0 永不触发，而调用方以为「已加守卫」。
 * 这是一个真实被探针抓到的缺陷：初版写成
 * `advertisesSupport === (verdict === "AVAILABLE")`，因为advertisesSupport
 * 就是 `verdict === "AVAILABLE"` 算出来的，比较自己恒真，守卫形同虚设。
 * 因此这里显式区分两个入参，**并要求调用方传真实声称**。
 */
export function verifyCapBitRegistration(
  verdict: VenusCapabilityState,
  /** 能力表侧**实际对外呈现**的「已支持」标记（不得由 verdict 反推）。 */
  advertisedSupport: boolean,
): Outcome<{ readonly advertisesSupport: boolean; readonly consistent: boolean }> {
  const advertisesSupport = advertisedSupport;
  /** 事实侧应得结论：只有 AVAILABLE 才允许对外声称已支持。 */
  const factSupports = verdict === "AVAILABLE";
  const consistent = advertisesSupport === factSupports;
  const diagnostics: Diagnostic[] = [];

  if (!consistent) {
    diagnostics.push({
      code: "VENUS_CAPABILITY_CLAIM_UNPROVEN",
      severity: "P0",
      message: `能力位对外声称与探测事实不一致（对外声称已支持=${advertisesSupport}，事实=${verdict}）`,
      hint: "以探测事实为准回改能力位；无据声称是本域最恶劣缺陷",
      stage: "capbit-register",
    });
  } else if (!advertisesSupport) {
    diagnostics.push({
      code: "VENUS_CAPBIT_NOT_REGISTERED",
      severity: "P2",
      message: `能力位 ${A_DOMAIN_VENUS_CAPBIT.bitName} 已登记但当前态为 ${verdict}，对外不得呈现为已支持`,
      hint: "位保留但标未认证；待阻断依赖齐备后重评",
      stage: "capbit-register",
    });
  }

  return ok({ advertisesSupport, consistent }, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §12 域级自检（回归 + 红线演练；本条交付物的一部分，非临时脚本）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 一条自检结果。 */
export interface SelfCheckItem {
  readonly group: string;
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 自检总结果。 */
export interface SelfCheckReport {
  readonly groups: Readonly<Record<string, readonly SelfCheckItem[]>>;
  readonly allPass: boolean;
  readonly failed: readonly string[];
}

/** 构造一份「全层齐备」的栈输入（回归基线：证明探测器不是恒假）。 */
function healthyStackInput(): StackProbeInput {
  return {
    presentLayers: {
      LOADER: true,
      ICD: true,
      TRANSPORT: true,
      WSI: true,
      SHADERS: true,
    },
    now: 1,
  };
}

/** 构造 VARIX 真实栈输入（回归基线：本系统当前实况——全缺）。 */
function absentStackInput(): StackProbeInput {
  return {
    presentLayers: {
      LOADER: false,
      ICD: false,
      TRANSPORT: false,
      WSI: false,
      SHADERS: false,
    },
    now: 1,
  };
}

/** 构造一组可求交的版本三元组（回归基线）。 */
function healthyVersions(): { readonly host: VenusVersionTriple; readonly guest: VenusVersionTriple } {
  return { host: { min: 1, max: 4, ask: 2 }, guest: { min: 1, max: 3, ask: 2 } };
}

/**
 * 域级自检。
 *
 * 覆盖五组：
 *   A. 能力探测——四态逐态核对（含仅 VULKAN2 的代际陷阱）；
 *   B. 栈评估——全齐 / 全缺两基线 + 首个缺项定位；
 *   C. 版本协商——可求交 / 无交集 / 畸形 /钳制四路径；
 *   D. 红线演练——无据声称可用必产 P0；声明闸拒绝矛盾输入；
 *   E. 台账与架构位——依赖项完备 + 三段齐备 + 呈现路径不分叉。
 */
export function runSelfCheck(): SelfCheckReport {
  const groups: Record<string, SelfCheckItem[]> = {};
  const add = (group: string, name: string, pass: boolean, detail: string): void => {
    const list = groups[group] ?? [];
    list.push({ group, name, pass, detail });
    groups[group] = list;
  };

  // ── A. 能力探测 ────────────────────────────────────────────────────────────
  const capsWithVenus = VIRTIO_GPU_CAP.VENUS | VIRTIO_GPU_CAP.VENUS_TERMINAL_2D;
  const absentStack = probeGuestVulkanStack(absentStackInput());

  const probeReserved = probeVenusCapability({
    deviceCaps: capsWithVenus,
    stackReady: absentStack.ok ? absentStack.value.stackReady : false,
    now: 1,
  });
  add(
    "A-能力探测",
    "cap 在 + 栈缺 → RESERVED_ONLY（本系统当前实况）",
    probeReserved.ok && probeReserved.value.state === "RESERVED_ONLY",
    probeReserved.ok ? `结论 ${probeReserved.value.state}` : `探测失败：${probeReserved.message}`,
  );

  const probeNegotiable = probeVenusCapability({
    deviceCaps: capsWithVenus,
    stackReady: true,
    now: 1,
  });
  add(
    "A-能力探测",
    "cap 在 + 栈齐 → NEGOTIABLE（不越级给 AVAILABLE）",
    probeNegotiable.ok && probeNegotiable.value.state === "NEGOTIABLE",
    probeNegotiable.ok ? `结论 ${probeNegotiable.value.state}` : `探测失败：${probeNegotiable.message}`,
  );

  const probeUnsupported = probeVenusCapability({
    deviceCaps: VIRTIO_GPU_CAP.VENUS_TERMINAL_2D,
    stackReady: true,
    now: 1,
  });
  add(
    "A-能力探测",
    "cap 缺 → UNSUPPORTED（栈齐也不改变终态）",
    probeUnsupported.ok && probeUnsupported.value.state === "UNSUPPORTED",
    probeUnsupported.ok ? `结论 ${probeUnsupported.value.state}` : `探测失败：${probeUnsupported.message}`,
  );

  const probeLegacyOnly = probeVenusCapability({
    deviceCaps: VIRTIO_GPU_CAP.VULKAN2 | VIRTIO_GPU_CAP.VENUS_TERMINAL_2D,
    stackReady: true,
    now: 1,
  });
  add(
    "A-能力探测",
    "仅 VULKAN2 → 不冒充 Venus（代际陷阱）",
    probeLegacyOnly.ok && probeLegacyOnly.value.state === "UNSUPPORTED",
    probeLegacyOnly.ok
      ? `结论 ${probeLegacyOnly.value.state}；理由：${probeLegacyOnly.diagnostics[0]?.message ?? "无"}`
      : "探测失败",
  );

  const probeOutOfRange = probeVenusCapability({ deviceCaps: -1, stackReady: true, now: 1 });
  add(
    "A-能力探测",
    "cap 位图越界 → 拒绝判定（不猜）",
    !probeOutOfRange.ok,
    probeOutOfRange.ok ? "越界值被接受——守卫失效" : `已拒绝：${probeOutOfRange.message}`,
  );

  // 逐位判定须由 VENUS_RELEVANT_CAP_BITS 驱动生成：位集合与逐位判定数须一致。
  const probeForParity = probeVenusCapability({ deviceCaps: capsWithVenus, stackReady: true, now: 1 });
  add(
    "A-能力探测",
    "逐位判定由相关位表驱动（不二次登记位号）",
    probeForParity.ok && probeForParity.value.bitVerdicts.length === VENUS_RELEVANT_CAP_BITS.length,
    probeForParity.ok
      ? probeForParity.value.bitVerdicts.map((b) => `${b.label}=${b.set ? "置位" : "未置位"}`).join("，")
      : "探测失败",
  );

  // 代际对拍守卫：当前登记为「仅 VULKAN2 不可用」，故正常路径不得报 P0。
  add(
    "A-能力探测",
    "代际登记与判定口径一致（对拍守卫不误报）",
    probeForParity.ok && !probeForParity.diagnostics.some((d) => d.severity === "P0"),
    `登记处置=${VULKAN2_VENUS_GENERATION.vulkan2OnlyVerdict}，同代=${VULKAN2_VENUS_GENERATION.sameGeneration}`,
  );

  // ── B. 栈评估 ─────────────────────────────────────────────────────────────
  const healthyStack = probeGuestVulkanStack(healthyStackInput());
  add(
    "B-栈评估",
    "五层齐备 → stackReady（探测器非恒假）",
    healthyStack.ok && healthyStack.value.stackReady,
    healthyStack.ok ? `缺项 ${healthyStack.value.missingCount} 个` : `评估失败：${healthyStack.message}`,
  );

  // 诚实标注的实质要求：原因必须说清「缺几层 + 首个缺项 + 归谁」，
  // 而不是一句「不支持」。故逐项断言其结构化要素，而非只断言非空。
  const reasonOk =
    absentStack.ok &&
    absentStack.value.missingCount === VULKAN_STACK_LAYER_ORDER.length &&
    absentStack.value.unavailableReason.includes("5") &&
    absentStack.value.unavailableReason.includes(STACK_LAYER_LABELS.LOADER) &&
    absentStack.value.unavailableReason.includes(STACK_LAYER_OWNERS.LOADER.domain);
  add(
    "B-栈评估",
    "全缺 → 诚实标注（缺项数+首个缺项+责任域）",
    reasonOk,
    absentStack.ok
      ? `首个缺项 ${absentStack.value.firstMissingLayer ?? "无"}；原因「${absentStack.value.unavailableReason}」`
      : "评估失败",
  );

  // 反向：原因不得退化成「不支持」这类无行动指引的空话。
  add(
    "B-栈评估",
    "原因非空话（必含首个缺项层名）",
    absentStack.ok &&
      absentStack.value.firstMissingLayer !== null &&
      absentStack.value.unavailableReason.includes(
        STACK_LAYER_LABELS[absentStack.value.firstMissingLayer],
      ),
    absentStack.ok ? "含具体层名" : "评估失败",
  );

  add(
    "B-栈评估",
    "缺项层带责任域（排期入口）",
    absentStack.ok && absentStack.value.verdicts.every((v) => v.ownerDomain.length > 0),
    absentStack.ok
      ? absentStack.value.verdicts.map((v) => `${v.layer}→${v.ownerDomain}`).join("，")
      : "评估失败",
  );

  // ── C. 版本协商 ────────────────────────────────────────────────────────────
  const hv = healthyVersions();
  const negHealthy = negotiateVenusVersion(hv.host, hv.guest, "NEGOTIABLE");
  add(
    "C-版本协商",
    "可求交 → 交集正确且发起协商",
    negHealthy.ok && negHealthy.value.overlapMin === 1 && negHealthy.value.overlapMax === 3 && negHealthy.value.attempted,
    negHealthy.ok ? negHealthy.value.summary : `协商失败：${negHealthy.message}`,
  );

  const negReserved = negotiateVenusVersion(hv.host, hv.guest, "RESERVED_ONLY");
  add(
    "C-版本协商",
    "预留态 → attempted=false（不假装协商过）",
    negReserved.ok && !negReserved.value.attempted,
    negReserved.ok ? negReserved.value.summary : "协商失败",
  );

  const negNoOverlap = negotiateVenusVersion(
    { min: 9, max: 10, ask: 9 },
    hv.guest,
    "NEGOTIABLE",
  );
  add(
    "C-版本协商",
    "无交集 → 拒绝协商",
    !negNoOverlap.ok,
    negNoOverlap.ok ? "无交集被放行——守卫失效" : `已拒绝：${negNoOverlap.message}`,
  );

  const negMalformed = negotiateVenusVersion({ min: 4, max: 1, ask: 2 }, hv.guest, "NEGOTIABLE");
  add(
    "C-版本协商",
    "畸形区间（min>max）→ 不宽容交换",
    !negMalformed.ok,
    negMalformed.ok ? "畸形区间被接受" : `已拒绝：${negMalformed.message}`,
  );

  const negClamp = negotiateVenusVersion({ min: 1, max: 99, ask: 99 }, hv.guest, "NEGOTIABLE");
  add(
    "C-版本协商",
    "ask 超已知上界 → 钳制后继续（仅记账）",
    negClamp.ok &&
      negClamp.value.clamped &&
      negClamp.value.chosen <= VENUS_KNOWN_ABI_UPPER &&
      negClamp.diagnostics.some((d) => d.code === "VENUS_ABI_CLAMPED"),
    negClamp.ok ? `选定 ABI ${negClamp.value.chosen}` : "钳制失败",
  );

  // ── D. 红线演练 ────────────────────────────────────────────────────────────
  const contradictory = contextDeclarationGate({ capabilityState: "NEGOTIABLE", stackReady: false });
  add(
    "D-红线演练",
    "声称可协商但栈缺 → 声明闸 P0 拒绝",
    !contradictory.ok && contradictory.diagnostics.some((d) => d.severity === "P0"),
    contradictory.ok ? "矛盾输入被放行" : `已拒绝：${contradictory.message}`,
  );

  const reservedDeclare = contextDeclarationGate({ capabilityState: "RESERVED_ONLY", stackReady: false });
  add(
    "D-红线演练",
    "预留态不产出任何上下文声明",
    reservedDeclare.ok && reservedDeclare.value.length === 0,
    reservedDeclare.ok ? `声明 ${reservedDeclare.value.length} 条（应为 0）` : "声明闸异常",
  );

  // 声称侧传「对外已支持」而事实侧为 RESERVED_ONLY —— 守卫必须捕获。
  const regMismatch = verifyCapBitRegistration("RESERVED_ONLY", true);
  add(
    "D-红线演练",
    "能力位登记与事实不一致 → P0（守卫真能触发）",
    regMismatch.ok &&
      regMismatch.diagnostics.some((d) => d.code === "VENUS_CAPABILITY_CLAIM_UNPROVEN") &&
      regMismatch.diagnostics.some((d) => d.severity === "P0"),
    regMismatch.ok
      ? `对外声称已支持而事实为 RESERVED_ONLY，已产 P0（${regMismatch.diagnostics
          .filter((d) => d.severity === "P0")
          .map((d) => d.code)
          .join("/")}）`
      : "核验异常",
  );

  // 反向：事实 AVAILABLE 且声称已支持 → 一致，无 P0。
  const regConsistent = verifyCapBitRegistration("AVAILABLE", true);
  add(
    "D-红线演练",
    "事实与声称一致 → 不误报 P0（守卫不咬人）",
    regConsistent.ok && !regConsistent.diagnostics.some((d) => d.severity === "P0"),
    regConsistent.ok ? "一致态未误报" : "一致态误报",
  );

  // 预留态下正常口径：位已登记但不得对外呈现已支持 → 仅 P2 记账。
  const regReservedNormal = verifyCapBitRegistration("RESERVED_ONLY", false);
  add(
    "D-红线演练",
    "预留态正常登记 → 仅 P2 记账不阻断",
    regReservedNormal.ok &&
      !regReservedNormal.diagnostics.some((d) => d.severity === "P0") &&
      regReservedNormal.diagnostics.some((d) => d.code === "VENUS_CAPBIT_NOT_REGISTERED"),
    regReservedNormal.ok ? "记账未升级为阻断" : "核验异常",
  );

  // ── E. 台账与架构位 ────────────────────────────────────────────────────────
  const ledger = verifyDependencyLedger();
  add(
    "E-台账与架构位",
    "依赖清单 ID 唯一且阻断项状态明示",
    ledger.ok && ledger.diagnostics.every((d) => d.code !== "VENUS_DEPENDENCY_ITEM_MISSING" || d.severity === "P1"),
    ledger.ok ? `清单 ${ledger.value.length} 项，阻断项未完成故当前判定为预留` : "台账核验失败",
  );

  add(
    "E-台账与架构位",
    "VE-C 着色器依赖已登记（反向驱动排期）",
    ledger.ok && ledger.value.some((i) => i.ownerDomain === "VE-C"),
    ledger.ok
      ? ledger.value.filter((i) => i.ownerDomain === "VE-C").map((i) => i.id).join(" / ")
      : "台账核验失败",
  );

  const slots = verifyArchSlots();
  add(
    "E-台账与架构位",
    "架构位三段齐备且均为预留（不假装已接）",
    slots.ok && slots.value.length === 3 && slots.value.every((s) => !s.implemented),
    slots.ok ? `${slots.value.length} 段，全部未实现（预留）` : `架构位核验失败：${slots.message}`,
  );

  const continuity = verifyB1Continuity();
  add(
    "E-台账与架构位",
    "B1 组条目连续（本条紧接前序）",
    continuity.ok,
    continuity.ok ? `本条 F${continuity.value} 紧接前序末条 F${B1_LANDED_BEFORE_THIS[B1_LANDED_BEFORE_THIS.length - 1]}` : `跳号：${continuity.message}`,
  );

  const descs = describeDependencyLedger();
  add(
    "E-台账与架构位",
    "台账可人话复述（带状态标签+影响说明）",
    descs.length === VENUS_DEPENDENCY_LEDGER.length && descs.every((d) => d.length > 20),
    descs.length > 0 ? descs[0] ?? "空" : "空",
  );

  add(
    "E-台账与架构位",
    "呈现面绑定 F0209 SET_SCANOUT（不新开呈现路径）",
    slots.ok &&
      slots.value.some((s) => s.slot === "PRESENT" && s.anchorEntryId === 209 && s.reservation.includes("F0209")),
    slots.ok ? "呈现面已绑定 F0209" : "呈现面未绑定",
  );

  // ── 端到端 ────────────────────────────────────────────────────────────────
  const report = assessVenusPathway({
    caps: { deviceCaps: capsWithVenus, stackReady: false, now: 7 },
    stackInput: absentStackInput(),
    hostVersion: hv.host,
    guestVersion: hv.guest,
    advertiseSupport: false,
    now: 7,
  });
  add(
    "E-台账与架构位",
    "端到端评估：VARIX 实况 → RESERVED_ONLY + 诚实原因",
    report.ok && report.value.verdict === "RESERVED_ONLY" && report.value.unavailableReason.length > 0,
    report.ok
      ? `报告 ${report.value.reportId}：${report.value.verdict}｜${report.value.unavailableReason}`
      : `评估失败：${report.message}`,
  );

  // 端到端红线：调用方谎报「已支持」而事实为预留态 → 出报告前即被 P0 阻断。
  const reportLying = assessVenusPathway({
    caps: { deviceCaps: capsWithVenus, stackReady: false, now: 7 },
    stackInput: absentStackInput(),
    hostVersion: hv.host,
    guestVersion: hv.guest,
    advertiseSupport: true,
    now: 7,
  });
  add(
    "D-红线演练",
    "端到端：谎报已支持 → 出报告前P0 阻断",
    !reportLying.ok && reportLying.diagnostics.some((d) => d.severity === "P0"),
    reportLying.ok ? "谎报未被拦截——报告已发出" : `已拦截：${reportLying.message}`,
  );

  const allItems = Object.values(groups).flat();
  const failed = allItems.filter((i) => !i.pass).map((i) => `${i.group}/${i.name}`);
  return { groups, allPass: failed.length === 0, failed };
}
