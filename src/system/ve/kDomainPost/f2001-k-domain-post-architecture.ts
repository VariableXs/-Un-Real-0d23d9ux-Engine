/**
 * VE-F2001 · K 域开工与后处理架构（K 域 · 后处理链域 · 批次 K01 · 组内第 1 条）
 * ---------------------------------------------------------------------------
 * 职责定位：K 域开工条目——宣告后处理链域（F2001-F2200，官方主题十项：Bloom /
 *   ToneMapping / DoF / MotionBlur / 色差 / 暗角 / 颗粒 / 锐化 / 色彩分级 / AA）
 *   正式开工。本条定义**域架构**本身，全后处理效果在同一执行框架下组合：
 *     1. 效果 DAG 图模型（节点=效果，边=数据依赖）；
 *     2. 统一效果接口（每效果：名称 / 输入 RT 依赖 / 输出 RT / 参数块 / 开关）；
 *     3. 中间 RT 池（三件套之一，保证效果链的 RT 分配可控可配��）；
 *     4. 与 I02 / J 域的挂接位（后处理在光照输出之后、输出编码之前的管线序）；
 *     5. J01→K 侧的**三契约接收核对**（bloom 源 F1849 / 曝光 F1812 / 输出编码边界）；
 *     6. K01 组内 20 条的分工总览（分工表 20 行）。
 *
 * ┌── 上游契约（三契约，缺签即开工阻断）───────────────────────────────────┐
 * │ ① bloom 源契约 F1849    —— 氛围组（J03）产出 bloom 源，K 域 Bloom 消费；  │
 * │ ② 曝光契约 F1812        —— J 域曝光语义，K 域 TM 前消费（不得二次曝光）； │
 * │ ③ 输出编码边界          —— 与 V 域划清：K 域管场景线性→显示编码前的最后  │
 * │    一段，UI 层显示亮度归 V 域，防双重调光（同 J 域 F1812 的边界纪律）。   │
 * │ 三者任一未签 → `verifyInboundContracts` 返回阻断，开工不成立。          │
 * └────────────────────────────────────────────────────────────────────────┘
 *
 * 基准管线序（锚点原文：光照输出 → AA → TM → LUT → 分级 → 镜头效果 → 输出编码）：
 *
 *   J 域光照输出（VE-F1801 三段式产出线性 HDR）
 *        ↓
 *   [AA]              K03 · F2009-F2012（MSAA / FXAA / TAA / SMAA 预留）
 *        ↓
 *   [TM]              K01 · F2006 色调映射（F2007 曝光联动 / F2021 数学验证）
 *        ↓
 *   [LUT]             K02 · F2023 LUT 应用管线
 *        ↓
 *   [分级 GRADE]       K01 · 色彩分级（提亮/对比/饱和/色轮）
 *        ↓
 *   [镜头效果 LENS]    Bloom / DoF / MotionBlur / 色差 / 暗角 / 颗粒 / 锐化
 *        ↓
 *   [输出编码 ENCODE]  K01 · F2008 色彩空间输出（边界：此为最后一段，UI 亮度归 V 域）
 *
 * ⚠️ 基准序 ≠ 强制序。锚点原文明确「DAG 允许重排但基准序为默认」——所以本条
 * 把序表达为**带权重的建议序**：DAG 校验器校验的是「依赖关系自洽」，而非
 * 「执行序等于基准序」。违反基准序但依赖自洽（如 Bloom 前置以便与 TM 融合）
 * 产出 `PIPELINE_ORDER_DEVIATION` 告警并放行；违反依赖（读取尚未产出的 RT）
 * 则是硬错 `EFFECT_DEPENDENCY_VIOLATED`，阻断。
 *
 * 三条不可协商的裁决规则（均以锚点原文为准）：
 *   R1 接收件缺签（上游契约未确认）→ 开工阻断，并**点名**是哪一份契约缺签
 *      （错误三要素齐全，不让调用方猜）。
 *   R2 效果接口与 DAG 执行器（F2002）设计冲突 → **以 F2002 执行器为准，回改本条**。
 *      本条因此把「效果接口」写成对执行器能力需求的**声明**，而非实现。
 *   R3 管线序与 I02/J 域冻结序冲突 → **以上游冻结序为准，回改本条**。
 *      与 VE-F1801 同构：J 域不可反向改动 I02，K 域亦不可反向改动 J/I02。
 *
 * 静态分发设计声明（锚点：效果接口调用零虚拟开销）：
 *   本条的效果接口以**数据化声明**（EffectDeclaration）而非 class 抽象呈现，
 *   执行器（F2002）据此在构建期生成分发表，运行时按整数 id 查表调用。
 *   这样「零虚拟开销」不是靠注释承诺，而是架构层就没有多态对象这一层间接。
 *
 * 零静默纪律：所有拒绝、冲突、阻断、缺签都产出 Diagnostic（code + message +
 *   hint），由调用方聚合上报。本模块不向 UI 直接抛异常，也不吞掉任何诊断。
 *
 * 判据：域开工、效果接口、三契约接收、管线序。
 *
 * 依赖锚点：F1997（J 域移交包 · K 相关三契约）、F2002（DAG 执行器 · 本条冲突时以其为准）、
 *   F2004 Bloom（消费 bloom 源契约 F1849）、F2007 曝光联动（消费曝光契约 F1812）、
 *   F2008 色彩空间输出（兑现输出编码边界）、F2014（全链性能预算）、F2015（质量档位）。
 * 跨卷衔接：VE-F1801（J 域开工 · 三段式管线序 · 本条的光照输入来源）、
 *   F1841（J03 环境氛围冻结序 · 后处理接在其后）、V 域（UI 显示亮度 · 边界另一侧）。
 * 下游消费：F2002 执行器（本条效果声明即其输入）、F2003（中间 RT 池实现本条第三件套）、
 *   F2004-F2012（各效果条目实现本条接口）、F2013 调试数据、F2014 预算、F2020 组收口。
 * 交接说明：本条只管「效果接口 + DAG 模型 + 管线序 + 三契约接收 + 分工表」；
 *   DAG 执行、RT 池分配、具体效果算法均归后续条目。
 */

/* ═══════════════════════════════════════════════════════════════════════════
 * §1 诊断与结果类型（零静默的基础设施）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 诊断码：每种拒绝/冲突/阻断/缺签都有独立可检索的码，绝不合并成通用错误。 */
export type DiagCode =
  /** 三契约之一缺签——开工阻断。 */
  | "INBOUND_CONTRACT_UNSIGNED"
  /** 接收件中出现未知契约种类（版本漂移 / 未登记契约）。 */
  | "INBOUND_CONTRACT_UNKNOWN"
  /** 效果声明字段不全（名称 / 输入 / 输出 / 参数块 / 开关缺一）。 */
  | "EFFECT_DECLARATION_INCOMPLETE"
  /** 效果依赖了未声明的效果（悬空依赖）。 */
  | "EFFECT_DEPENDENCY_UNKNOWN"
  /** 效果读取了 DAG 中尚未产出的 RT——依赖违反，硬错。 */
  | "EFFECT_DEPENDENCY_VIOLATED"
  /** DAG 存在环——不可执行。 */
  | "EFFECT_DAG_CYCLIC"
  /** 执行序偏离基准序但依赖自洽——告警放行（DAG 允许重排）。 */
  | "PIPELINE_ORDER_DEVIATION"
  /** 管线序与上游冻结序（I02/J 域）冲突——以上游为准回改本条。 */
  | "PIPELINE_UPSTREAM_CONFLICT"
  /** 试图反向改动上游冻结序——直接拒绝（不可协商项）。 */
  | "PIPELINE_UPSTREAM_MUTATION_REFUSED"
  /** 中间 RT 池容量不足（申请 RT 数超池容量）。 */
  | "RT_POOL_EXHAUSTED"
  /** RT 生命周期违规（使用已归还或尺寸不匹配的 RT）。 */
  | "RT_LIFETIME_VIOLATION"
  /** 效果接口与 F2002 执行器能力声明冲突——以执行器为准回改本条。 */
  | "EFFECT_INTERFACE_EXECUTOR_CONFLICT"
  /** 分工表条目数与 K01 组 20 条不符。 */
  | "WORKTABLE_CARDINALITY_INVALID"
  /** 分工表存在重复条目号。 */
  | "WORKTABLE_DUPLICATE_ENTRY"
  /** 分工表条目号不连续（缺位）。 */
  | "WORKTABLE_GAP"
  /** 架构修正（ADR）信息不全，无法受理。 */
  | "ADR_INCOMPLETE"
  /** 架构冻结快照与当前声明不一致（漂移告警）。 */
  | "FREEZE_DRIFT";

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

/** 成功构造（diagnostics 允许携带非致命告警，例如序偏离告警）。 */
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
 * §2 域标识与官方十主题
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * K 域标识。域号 K，条目区间 F2001-F2200（200 条 = 10 组 × 20 条）。
 */
export const K_DOMAIN = {
  /** 域标签，与册内 `VE-K` 一致。 */
  tag: "VE-K",
  /** 域中文名。 */
  title: "后处理链域",
  /** 条目起始（域开工条）。 */
  firstEntryId: 2001,
  /** 条目结束（域收官条）。 */
  lastEntryId: 2200,
  /** 条目总数。 */
  entryCount: 200,
  /** 组数（10 组，每组 20 条）。 */
  groupCount: 10,
  /** 每组条数。 */
  entriesPerGroup: 20,
} as const;

/**
 * K 域官方十主题（锚点原文：Bloom/ToneMapping/DoF/MotionBlur/色差/暗角/颗粒/
 * 锐化/色彩分级/AA）。用于开工宣告与分工表主题归类核对。
 */
export const K_DOMAIN_TEN_TOPICS = [
  "Bloom",
  "ToneMapping",
  "DoF",
  "MotionBlur",
  "色差",
  "暗角",
  "颗粒",
  "锐化",
  "色彩分级",
  "AA",
] as const;

export type KDomainTopic = (typeof K_DOMAIN_TEN_TOPICS)[number];

/** 每个组对应的条目区间与阶段，供分工表归类与序校验共用。 */
export const K_STAGE_SPANS: Readonly<
  Record<"K01" | "K02" | "K03", { stage: PostStage; firstId: number; lastId: number }>
> = {
  K01: { stage: "TM", firstId: 2001, lastId: 2020 },
  K02: { stage: "LUT", firstId: 2021, lastId: 2040 },
  K03: { stage: "AA", firstId: 2041, lastId: 2060 },
};

/* ═══════════════════════════════════════════════════════════════════════════
 * §3 基准管线序声明
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 基准管线序的阶段标识。
 * 锚点原文序：光照输出 → AA → TM → LUT → 分级 → 镜头效果 → 输出编码。
 * 首项 `LIGHTING_INPUT` 为上游（J 域）产出，非 K 域效果，仅作锚点。
 */
export type PostStage =
  /** 上游光照输入锚点（J 域 F1801 三段式产出线性 HDR）。 */
  | "LIGHTING_INPUT"
  /** 抗锯齿（K03 · F2009-F2012）。 */
  | "AA"
  /** 色调映射（K01 · F2006）。 */
  | "TM"
  /** LUT 应用（K02 · F2023）。 */
  | "LUT"
  /** 色彩分级（K01）。 */
  | "GRADE"
  /** 镜头效果（Bloom / DoF / MotionBlur / 色差 / 暗角 / 颗粒 / 锐化）。 */
  | "LENS"
  /** 输出编码（K01 · F2008 · 最后一段，UI 亮度归 V 域）。 */
  | "ENCODE";

/** 基准序的规范次序（索引即建议执行序，DAG 允许偏离但需告警）。 */
export const POST_STAGE_ORDER: readonly PostStage[] = [
  "LIGHTING_INPUT",
  "AA",
  "TM",
  "LUT",
  "GRADE",
  "LENS",
  "ENCODE",
];

/** 各阶段人话名（宣告与诊断文本用，避免裸枚举名外泄给用户）。 */
export const POST_STAGE_LABELS: Readonly<Record<PostStage, string>> = {
  LIGHTING_INPUT: "光照输入",
  AA: "抗锯齿",
  TM: "色调映射",
  LUT: "LUT 应用",
  GRADE: "色彩分级",
  LENS: "镜头效果",
  ENCODE: "输出编码",
};

/**
 * 上游冻结序约束（不可反向改动）。
 *
 * 本条只**读消费**上游冻结序并登记为守卫基准：
 *   - I02 绘制族的输出交接口（J 域光照产出之后的那个交接点）；
 *   - J 域 F1841 环境氛围冻结序的后继位置（J03 之后即 K 域入口）。
 * 任何 K 域想把效果插到这两者之前的声明，都会被守卫拒绝。
 */
export const UPSTREAM_FROZEN_ANCHORS: readonly { readonly name: string; readonly owner: string; readonly note: string }[] = [
  { name: "I02_RENDER_HANDOFF", owner: "I 域 F1680", note: "I02 绘制族输出交接点（F1680 冻结）；K 域效果链只能在其之后起算。" },
  { name: "J_ATMOSPHERE_END", owner: "J 域 F1841", note: "J03 环境氛围冻结序末端（F1841 冻结）；K 域接在其后，不得插入氛围之前。" },
];

/**
 * 拒绝任何反向改动上游冻结序的请求（不可协商项的守卫函数）。
 * 与 VE-F1801 的 `refuseUpstreamPipelineMutation` 同构：把「禁止反向改动」
 * 从注释变成可执行断言。
 */
export function refuseUpstreamMutation(
  anchorName: string,
  attemptedBefore: string,
  reason: string,
): Outcome<never> {
  const anchor = UPSTREAM_FROZEN_ANCHORS.find((a) => a.name === anchorName);
  const anchorNote = anchor !== undefined ? anchor.note : "上游冻结锚点未登记。";
  const anchorOwner = anchor !== undefined ? anchor.owner : "上游（未登记）";
  const message =
    `拒绝反向改动上游冻结序：试图把后处理效果插到 ${anchorName} 之前` +
    `（拟插入位 ${attemptedBefore}；理由：${reason}）。`;
  const hint =
    `上游冻结序（${anchorNote}；归属 ${anchorOwner}）不可由 K 域改动。` +
    `管线序冲突一律以上游冻结为准、回改 K 域设计；` +
    `若确需上游变更，先走该域 ADR 流程解冻，改动完成后再回来对齐管线序。`;
  return err("PIPELINE_UPSTREAM_MUTATION_REFUSED", message, hint);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §4 统一效果接口（数据化声明 → 执行器构建期生成分发表）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 渲染目标引用。RT 池以整数索引分配，本条只声明「依赖哪个 RT 槽」，
 * 具体分配策略由 F2003（中间 RT 池）实现。
 */
export interface RtRef {
  /** RT 槽位标识（如 "sceneColor" / "temp0"）。 */
  readonly slot: string;
}

/** 一个效果的完整声明（锚点：名称 / 输入 RT 依赖 / 输出 RT / 参数块 / 开关）。 */
export interface EffectDeclaration {
  /** 效果稳定 id（执行器分发键，整数索引由构建期分配）。 */
  readonly effectId: string;
  /** 人话名称（诊断与宣告文本用）。 */
  readonly displayName: string;
  /** 归属阶段。 */
  readonly stage: PostStage;
  /** 输入 RT 依赖（可多个；上游阶段产出的 RT 槽）。 */
  readonly inputs: readonly RtRef[];
  /** 输出 RT。 */
  readonly output: RtRef;
  /** 参数块字段名列表（参数块具体布局由各效果条目定义）。 */
  readonly params: readonly string[];
  /** 开关名（效果可在运行期被质量档或用户开关禁用）。 */
  readonly toggle: string;
  /** 该效果是否被上游契约约束（如 Bloom 受 bloom 源契约 F1849 约束）。 */
  readonly inboundContract: InboundContractKind | null;
}

/**
 * 三契约种类（锚点：bloom 源契约 F1849 / 曝光契约 F1812 / 输出编码边界）。
 */
export type InboundContractKind = "BLOOM_SOURCE" | "EXPOSURE" | "OUTPUT_ENCODE";

/** 三契约的人话名与锚点（缺签诊断直接引用本表，保证点名准确）。 */
export const INBOUND_CONTRACT_META: Readonly<
  Record<InboundContractKind, { label: string; anchor: string; consumer: string }>
> = {
  BLOOM_SOURCE: { label: "bloom 源契约", anchor: "F1849", consumer: "K 域 F2004 Bloom 消费" },
  EXPOSURE: { label: "曝光契约", anchor: "F1812", consumer: "K 域 F2007 曝光联动消费" },
  OUTPUT_ENCODE: { label: "输出编码边界", anchor: "VE-V 边界", consumer: "K 域 F2008 输出编码落地，UI 亮度归 V 域" },
};

/** 三契约全集——接收核对遍历此集合，缺一即阻断。 */
export const INBOUND_CONTRACT_KINDS: readonly InboundContractKind[] = [
  "BLOOM_SOURCE",
  "EXPOSURE",
  "OUTPUT_ENCODE",
];

/* ═══════════════════════════════════════════════════════════════════════════
 * §5 三契约接收核对（缺签即开工阻断 · 点名）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 一份接收件的签名状态（由 F1997 移交包提供）。 */
export interface InboundContractSignature {
  readonly kind: InboundContractKind;
  readonly signed: boolean;
  readonly signature: string | null;
}

/** 接收件整体。 */
export interface InboundContractBundle {
  readonly signatures: readonly InboundContractSignature[];
}

/**
 * 三契约接收核对。
 *
 * R1 裁决：任一契约缺签即开工阻断，且**点名**是哪一份——错误三要素齐全
 * （缺的是 bloom 源契约 / 曝光契约 / 输出编码边界），不让调用方猜是哪份没签。
 * 未知契约种类同样显性报错（版本漂移守卫），不静默忽略。
 */
export function verifyInboundContracts(bundle: InboundContractBundle): Outcome<InboundContractKind[]> {
  const diagnostics: Diagnostic[] = [];
  const byKind = new Map<InboundContractKind, InboundContractSignature>();

  for (const sig of bundle.signatures) {
    if (!INBOUND_CONTRACT_KINDS.includes(sig.kind)) {
      diagnostics.push({
        code: "INBOUND_CONTRACT_UNKNOWN",
        message: `接收件出现未登记的契约种类：${String(sig.kind)}。`,
        hint: `K 域登记的契约为 ${INBOUND_CONTRACT_KINDS.join("、")}。未登记契约不得直接生效，先确认版本并登记后再开工。`,
      });
      continue;
    }
    byKind.set(sig.kind, sig);
  }

  // 缺签点名：逐契约核对，未出现在接收件中同样视为缺签。
  const unsigned: InboundContractKind[] = [];
  for (const kind of INBOUND_CONTRACT_KINDS) {
    const meta = INBOUND_CONTRACT_META[kind];
    const sig = byKind.get(kind);
    if (sig === undefined || !sig.signed || sig.signature === null) {
      unsigned.push(kind);
      diagnostics.push({
        code: "INBOUND_CONTRACT_UNSIGNED",
        message: `${meta.label}（${meta.anchor}）未签收。`,
        hint: `向 J 域（F1997）索取${meta.label}签名；${meta.consumer}。该契约未确认前 K 域不得开工。`,
      });
    }
  }

  // 全部缺签时，首条诊断点名第一份未签契约（调用方拿 code 即知缺哪份）。
  if (unsigned.length > 0) {
    const firstKind = unsigned[0];
    if (firstKind !== undefined) {
      const meta = INBOUND_CONTRACT_META[firstKind];
      return err(
        "INBOUND_CONTRACT_UNSIGNED",
        `开工阻断：${unsigned.length}/${INBOUND_CONTRACT_KINDS.length} 份上游契约缺签，首要为${meta.label}（${meta.anchor}）。`,
        `缺签清单：${unsigned.map((k) => INBOUND_CONTRACT_META[k].label).join("、")}。逐份向 J 域（F1997）索取签名后重试开工。`,
        diagnostics,
      );
    }
  }
  return ok([...INBOUND_CONTRACT_KINDS], diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §6 DAG 模型（节点 / 边 / 校验）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** DAG 图模型：节点=效果，边=数据依赖（上游产出被下游消费）。 */
export interface EffectDag {
  readonly nodes: readonly EffectDeclaration[];
  /** 边：from 产出被 to 消费。 */
  readonly edges: readonly { from: string; to: string }[];
}

/**
 * 校验效果声明完整性（名称 / 输入 / 输出 / 参数块 / 开关缺一不可）。
 * 目的是让 F2002 执行器在构建期就能拒绝不完整声明，而不是运行时拿到半成品。
 */
export function verifyEffectDeclarations(nodes: readonly EffectDeclaration[]): Outcome<EffectDeclaration[]> {
  const diagnostics: Diagnostic[] = [];
  const seenIds = new Set<string>();

  for (const node of nodes) {
    if (seenIds.has(node.effectId)) {
      diagnostics.push({
        code: "EFFECT_DECLARATION_INCOMPLETE",
        message: `效果 id 重复：${node.effectId}。`,
        hint: "效果 id 是执行器分发键，必须唯一；重复即后者覆盖前者，改用唯一 id。",
      });
    }
    seenIds.add(node.effectId);

    if (node.displayName.length === 0) {
      diagnostics.push({
        code: "EFFECT_DECLARATION_INCOMPLETE",
        message: `效果 ${node.effectId} 缺人话名称。`,
        hint: "补 displayName；诊断与宣告文本需人话名称，不允许裸 id 外泄。",
      });
    }
    if (node.output.slot.length === 0) {
      diagnostics.push({
        code: "EFFECT_DECLARATION_INCOMPLETE",
        message: `效果 ${node.effectId} 未声明输出 RT。`,
        hint: "每个效果必须声明输出 RT 槽；无输出的效果无法串进链。",
      });
    }
    if (node.toggle.length === 0) {
      diagnostics.push({
        code: "EFFECT_DECLARATION_INCOMPLETE",
        message: `效果 ${node.effectId} 未声明开关名。`,
        hint: "每个效果必须声明开关名；否则质量档（F2015）无法禁用它。",
      });
    }
    // 入参 RT 允许为空（光源数据类效果可直接读场景缓冲），但参数块须声明字段
    // 名（可为空数组表示无参），不允许留 undefined。
    if (node.params === undefined) {
      diagnostics.push({
        code: "EFFECT_DECLARATION_INCOMPLETE",
        message: `效果 ${node.effectId} 未声明参数块。`,
        hint: "参数块可为无参（空数组），但不可缺失；缺失使执行器无法生成参数字典。",
      });
    }
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.message, first.hint, diagnostics);
    }
  }
  return ok([...nodes], diagnostics);
}

/**
 * 校验 DAG：悬空依赖、环、依赖违反（读未产出的 RT）、序偏离告警。
 *
 * 分级（关键：告警与硬错不可混为一谈）：
 *   硬错（阻断）：悬空依赖、DAG 环、依赖违反（读取尚未产出的 RT）。
 *   告警（放行）：执行序偏离基准序——锚点原文「DAG 允许重排但基准序为默认」。
 *
 * @param dag 效果图
 * @param baselineOrder 给定的执行序（拓扑序之一），与基准序比对产出偏离告警
 */
export function verifyDag(
  dag: EffectDag,
  baselineOrder: readonly string[],
): Outcome<{ deviations: readonly string[] }> {
  const diagnostics: Diagnostic[] = [];
  const byId = new Map<string, EffectDeclaration>();
  for (const n of dag.nodes) byId.set(n.effectId, n);

  // 6.1 悬空依赖（边指向不存在或未声明的效果）——硬错。
  for (const edge of dag.edges) {
    if (!byId.has(edge.from) || !byId.has(edge.to)) {
      diagnostics.push({
        code: "EFFECT_DEPENDENCY_UNKNOWN",
        message: `DAG 边指向未知效果：${edge.from} → ${edge.to}。`,
        hint: "边的两端都必须是已声明效果；补齐声明或删除该边。",
      });
    }
  }
  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.message, first.hint, diagnostics);
    }
  }

  // 6.2 环检测（DFS 三色标记）——硬错。
  const WHITE = 0, GRAY = 1, BLACK = 2;
  const color = new Map<string, number>();
  for (const n of dag.nodes) color.set(n.effectId, WHITE);
  const adjacency = new Map<string, string[]>();
  for (const edge of dag.edges) {
    const list = adjacency.get(edge.from);
    if (list !== undefined) list.push(edge.to);
    else adjacency.set(edge.from, [edge.to]);
  }
  let hasCycle = false;
  const visit = (id: string): void => {
    if (hasCycle) return;
    const c = color.get(id) ?? WHITE;
    if (c === GRAY) { hasCycle = true; return; }
    if (c === BLACK) return;
    color.set(id, GRAY);
    for (const next of adjacency.get(id) ?? []) visit(next);
    color.set(id, BLACK);
  };
  for (const n of dag.nodes) visit(n.effectId);
  if (hasCycle) {
    return err(
      "EFFECT_DAG_CYCLIC",
      "效果 DAG 存在环，无法确定执行序。",
      "后处理链必须无环（A→B 表示 B 消费 A 产出）；断开成环的边，或把循环效果改写为迭代效果（F2002 能力范围内的迭代由执行器承担）。",
    );
  }

  // 6.3 依赖违反：某效果读取的 RT 槽，其产出者必须排在它之前——硬错。
  const posOf = new Map<string, number>();
  baselineOrder.forEach((id, i) => posOf.set(id, i));
  for (const n of dag.nodes) {
    for (const input of n.inputs) {
      // 找出产出该 RT 槽的效果（输入依赖的生产者）。
      for (const producer of dag.nodes) {
        if (producer.output.slot !== input.slot) continue;
        if (producer.effectId === n.effectId) continue; // 自读自写（原地）允许
        const pPos = posOf.get(producer.effectId);
        const nPos = posOf.get(n.effectId);
        if (pPos === undefined || nPos === undefined) continue;
        if (pPos > nPos) {
          return err(
            "EFFECT_DEPENDENCY_VIOLATED",
            `依赖违反：${n.effectId} 读取 ${input.slot}，但其产出者 ${producer.effectId} 排在它之后。`,
            `把 ${producer.effectId} 提到 ${n.effectId} 之前；DAG 执行器按依赖序而非声明序取数，序错会读到未初始化 RT。`,
          );
        }
      }
    }
  }

  // 6.4 序偏离告警——放行（DAG 允许重排）。
  //     偏离定义为：某效果的阶段权重序与其在给定执行序中的相对位置不一致。
  const deviations: string[] = [];
  for (const n of dag.nodes) {
    for (const input of n.inputs) {
      for (const producer of dag.nodes) {
        if (producer.output.slot !== input.slot) continue;
        if (producer.effectId === n.effectId) continue;
        const prodStage = POST_STAGE_ORDER.indexOf(producer.stage);
        const consStage = POST_STAGE_ORDER.indexOf(n.stage);
        if (prodStage > consStage) {
          // 产出者阶段晚于消费者阶段——典型的「Bloom 前置以便与 TM 融合」式重排。
          deviations.push(
            `${producer.effectId}(${POST_STAGE_LABELS[producer.stage]}) 被前置到 ${n.effectId}(${POST_STAGE_LABELS[n.stage]}) 之前`,
          );
        }
      }
    }
  }
  for (const d of deviations) {
    diagnostics.push({
      code: "PIPELINE_ORDER_DEVIATION",
      message: `管线序偏离基准序：${d}。`,
      hint: "基准序为默认但 DAG 允许重排——只要依赖自洽即可放行；若此重排非有意为之，请核对是否应回到基准序。",
    });
  }

  return ok({ deviations }, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §7 中间 RT 池（三件套之三）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** RT 规格（尺寸与格式），池按规格分桶分配。 */
export interface RtSpec {
  readonly width: number;
  readonly height: number;
  readonly format: "RGBA16F" | "RGBA8" | "R11G11B10F";
}

/** 池中一个已分配的 RT。 */
export interface PooledRt {
  /** 池内唯一句柄（归还后被复用时保持不变）。 */
  readonly handle: number;
  readonly slot: string;
  readonly spec: RtSpec;
  /** 已归还——归还后可被后续申请复用，仍在用者不可复用。 */
  readonly released: boolean;
}

/** RT 池（本条声明语义，分配实现归 F2003）。 */
export interface RtPool {
  readonly capacity: number;
  readonly items: readonly PooledRt[];
}

/**
 * 从池中申请一个 RT。
 * 池空即 `RT_POOL_EXHAUSTED`——显性报错，让调用方知道该降级或回收，
 * 而不是拿到 undefined RT 让渲染后段崩在别处（错误发生在远处最难查）。
 */
/**
 * 从池中申请一个 RT。
 *
 * 池语义（与 RT 池的常识一致，不搞特殊）：
 *   1. 优先复用**已归还且规格相同**的槽位——这是池存在的意义（避免每帧重建 RT）；
 *   2. 无可复用则新分配一个槽位；
 *   3. 在用数达容量且无可复用 → `RT_POOL_EXHAUSTED` 显性报错，让调用方知道该降级
 *      或回收，而不是拿到 undefined RT 让渲染后段崩在别处（错误发生在远处最难查）。
 *
 * 句柄为池内唯一编号（不复用 liveCount，否则「归还后重分配」会撞上仍在用的句柄）。
 */
export function acquireRt(pool: RtPool, slot: string, spec: RtSpec): Outcome<{ pool: RtPool; handle: number }> {
  const sameSpec = (r: PooledRt): boolean =>
    r.spec.width === spec.width && r.spec.height === spec.height && r.spec.format === spec.format;

  // 1) 复用已归还的同规格槽位。
  const reusable = pool.items.find((r) => r.released && sameSpec(r));
  if (reusable !== undefined) {
    const items = pool.items.map((r) =>
      r.handle === reusable.handle ? { ...r, slot, released: false } : r,
    );
    return ok({ pool: { capacity: pool.capacity, items }, handle: reusable.handle });
  }

  // 2) 新分配（须未达容量）。
  const liveCount = pool.items.filter((r) => !r.released).length;
  if (liveCount >= pool.capacity) {
    return err(
      "RT_POOL_EXHAUSTED",
      `中间 RT 池已满：容量 ${pool.capacity}，在用 ${liveCount}。`,
      "减少同时存活的效果数（合并相邻效果或提高质量档下限），或由 F2003 扩容池。",
    );
  }
  // 句柄取池内最大编号 +1，保证唯一且单调。
  const nextHandle = pool.items.reduce((max, r) => (r.handle + 1 > max ? r.handle + 1 : max), 0);
  const items: PooledRt[] = [...pool.items, { handle: nextHandle, slot, spec, released: false }];
  return ok({ pool: { capacity: pool.capacity, items }, handle: nextHandle });
}

/** 归还 RT。重复归还即生命周期违规（显性报错）。 */
export function releaseRt(pool: RtPool, handle: number): Outcome<RtPool> {
  const target = pool.items.find((r) => r.handle === handle);
  if (target === undefined) {
    return err("RT_LIFETIME_VIOLATION", `归还失败：句柄 ${handle} 不在池中。`, "核对句柄来源；使用未分配的句柄会掩盖真正的分配缺陷。");
  }
  if (target.released) {
    return err("RT_LIFETIME_VIOLATION", `重复归还 RT 句柄 ${handle}。`, "句柄只能归还一次；重复归还会让在用计数虚低，最终表现为池耗尽。");
  }
  const items = pool.items.map((r) => (r.handle === handle ? { ...r, released: true } : r));
  return ok({ capacity: pool.capacity, items });
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §8 效果接口 × F2002 执行器能力声明（R2 裁决）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * F2002（DAG 执行器）能力声明——本条的效果接口以它为准。
 *
 * 这是 R2 的可执行形态：本条不「发明」执行能力，而是声明「我需要什么」，
 * 再由 `checkAgainstExecutor` 逐项比对。凡本条声明超出执行器能力的，
 * 一律 `EFFECT_INTERFACE_EXECUTOR_CONFLICT` 并要求回改本条——
 * 因为锚点原文明确「效果接口与 DAG 执行器设计冲突 → 以 F2002 执行器为准回改」。
 */
export const F2002_EXECUTOR_CAPABILITIES = {
  /** 是否支持中间 RT 池分配（对应 F2003 实现）。 */
  rtPool: true,
  /** 是否支持按 RT 槽解析生产者（用于依赖校验）。 */
  slotDependencyResolution: true,
  /** 是否支持效果开关（质量档 / 用户开关禁用）。 */
  effectToggle: true,
  /** 是否支持原地效果（读写的 RT 槽相同）。 */
  inPlaceEffect: true,
  /** 最大同时存活效果数（RT 池容量同源上限）。 */
  maxLiveEffects: 16,
} as const;

/** 效果接口的「需求」侧声明——本条对执行器提出的能力要求。 */
export interface ExecutorRequirement {
  readonly rtPool: boolean;
  readonly slotDependencyResolution: boolean;
  readonly effectToggle: boolean;
  readonly inPlaceEffect: boolean;
  readonly maxLiveEffects: number;
}

/**
 * 比对效果接口需求与 F2002 执行器能力。
 * 不满足即冲突——以执行器为准回改本条，产出点名冲突项。
 */
export function checkAgainstExecutor(req: ExecutorRequirement): Outcome<ExecutorRequirement> {
  const conflicts: string[] = [];
  const caps = F2002_EXECUTOR_CAPABILITIES;
  if (req.rtPool && !caps.rtPool) conflicts.push("中间 RT 池");
  if (req.slotDependencyResolution && !caps.slotDependencyResolution) conflicts.push("按槽解析生产者");
  if (req.effectToggle && !caps.effectToggle) conflicts.push("效果开关");
  if (req.inPlaceEffect && !caps.inPlaceEffect) conflicts.push("原地效果");
  if (req.maxLiveEffects > caps.maxLiveEffects) {
    conflicts.push(`同时存活效果数 ${req.maxLiveEffects} 超出执行器上限 ${caps.maxLiveEffects}`);
  }
  if (conflicts.length > 0) {
    return err(
      "EFFECT_INTERFACE_EXECUTOR_CONFLICT",
      `效果接口与 F2002 执行器能力冲突：${conflicts.join("、")}。`,
      "按锚点裁决「以 F2002 执行器为准回改本条」——下调本条效果接口需求，或先由 F2002 走 ADR 扩能力后再对齐。",
    );
  }
  return ok(req);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §9 K01 组分工表（20 行）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 分工表中一条：条目号 + 标题 + 归属主题 + 交付定位。 */
export interface WorkTableRow {
  readonly entryId: number;
  readonly title: string;
  readonly topic: KDomainTopic;
  /** 该条在本域架构中的角色。 */
  readonly role: string;
  /** 上游依赖条目（空串表示无）。 */
  readonly upstream: string;
}

/**
 * K01 组 20 条分工表（F2001-F2020，标题逐条核对册内锚点）。
 * 本条（VE-F2001）为组内第 1 条，即域开工与后处理架构本身。
 */
export const K01_WORK_TABLE: readonly WorkTableRow[] = [
  { entryId: 2001, title: "K 域开工与后处理架构", topic: "色彩分级", role: "域开工·效果 DAG·统一接口·管线序·三契约接收·分工表", upstream: "F1997" },
  { entryId: 2002, title: "后处理 DAG 执行器", topic: "色彩分级", role: "DAG 拓扑执行；本条效果声明的落地实现（冲突时以其为准）", upstream: "F2001" },
  { entryId: 2003, title: "中间渲染目标池", topic: "色彩分级", role: "RT 池分配/回收/配额；本条第三件套实现", upstream: "F2001" },
  { entryId: 2004, title: "Bloom 核心", topic: "Bloom", role: "亮部提取+模糊金字塔；消费 bloom 源契约 F1849", upstream: "F2003" },
  { entryId: 2005, title: "Bloom 参数化", topic: "Bloom", role: "阈值/软膝/强度/半径参数块与质量档联动", upstream: "F2004" },
  { entryId: 2006, title: "色调映射 ToneMapping", topic: "ToneMapping", role: "多算子族注册与选择；默认算子诚实标注", upstream: "F2001" },
  { entryId: 2007, title: "曝光联动", topic: "ToneMapping", role: "消费 J 域曝光契约 F1812；禁止二次曝光", upstream: "F1812" },
  { entryId: 2008, title: "色彩空间输出", topic: "ToneMapping", role: "输出编码落地；划清 VE-V 边界防双重调光", upstream: "F2007" },
  { entryId: 2009, title: "抗锯齿四法之 MSAA", topic: "AA", role: "多重采样实现与质量档分档", upstream: "F2003" },
  { entryId: 2010, title: "抗锯齿四法之 FXAA", topic: "AA", role: "后处理 FXAA；成本最低档默认", upstream: "F2009" },
  { entryId: 2011, title: "抗锯齿四法之 TAA", topic: "AA", role: "时域抗锯齿；抖动序列与历史缓冲", upstream: "F2009" },
  { entryId: 2012, title: "抗锯齿四法之 SMAA 预留", topic: "AA", role: "SMAA 接口位预留与诚实标注（一期不实现）", upstream: "F2001" },
  { entryId: 2013, title: "后处理调试数据", topic: "色彩分级", role: "各阶段中间缓冲可视化经 F1764 信封发出", upstream: "F2002" },
  { entryId: 2014, title: "后处理性能预算", topic: "色彩分级", role: "全链预算表与超预算降档建议", upstream: "F2002" },
  { entryId: 2015, title: "后处理质量档位", topic: "色彩分级", role: "质量档↔效果开关矩阵；与 F1762 对接", upstream: "F2005,F2011" },
  { entryId: 2016, title: "后处理 fuzz", topic: "色彩分级", role: "RT 池耗尽/环形 DAG/畸形参数 fuzz", upstream: "F2003" },
  { entryId: 2017, title: "后处理基准", topic: "色彩分级", role: "全链基准与预算常数定标数据源", upstream: "F2014" },
  { entryId: 2018, title: "后处理 API 冻结 v1", topic: "色彩分级", role: "效果接口签名冻结与描述词成册", upstream: "F2002" },
  { entryId: 2019, title: "后处理文档与一致性", topic: "色彩分级", role: "总纲三章（效果/镜头/编码）+ 一致性审查", upstream: "F2018" },
  { entryId: 2020, title: "K01 组收口与 K02 移交", topic: "ToneMapping", role: "组收口双签+向 K02 移交 LUT 相关段", upstream: "F2002~F2019" },
];

/** 校验分工表：条目数 = 20、条目号 F2001-F2020 连续无重复、主题属官方十主题。 */
export function verifyWorkTable(table: readonly WorkTableRow[]): Outcome<WorkTableRow[]> {
  const diagnostics: Diagnostic[] = [];

  // 9.1 重复（最具体先报）。
  const seen = new Set<number>();
  for (const row of table) {
    if (seen.has(row.entryId)) {
      diagnostics.push({
        code: "WORKTABLE_DUPLICATE_ENTRY",
        message: `分工表条目号重复：F${row.entryId}。`,
        hint: "每个条目号在组内唯一；重复即覆盖了他条职责，修正后重跑校验。",
      });
    }
    seen.add(row.entryId);
  }

  // 9.2 连续性（F2001 起连续 20 条）——先于基数检查：缺位直接指出病因。
  const ids = [...seen].sort((a, b) => a - b);
  const expectedFirst = K_STAGE_SPANS.K01.firstId;
  for (let i = 0; i < ids.length; i++) {
    const expected = expectedFirst + i;
    if (ids[i] !== expected) {
      diagnostics.push({
        code: "WORKTABLE_GAP",
        message: `分工表条目号不连续：期望 F${expected}，实到 F${ids[i]}。`,
        hint: `本组须连续覆盖 F${expectedFirst}-F${K_STAGE_SPANS.K01.lastId}，缺位即漏项。`,
      });
      break;
    }
  }

  // 9.3 基数（症状级）。
  if (table.length !== K_DOMAIN.entriesPerGroup) {
    diagnostics.push({
      code: "WORKTABLE_CARDINALITY_INVALID",
      message: `K01 分工表应为 ${K_DOMAIN.entriesPerGroup} 条，实为 ${table.length} 条。`,
      hint: `按 K 域「每组 ${K_DOMAIN.entriesPerGroup} 条」补齐或修正；本组覆盖 F${expectedFirst}-F${K_STAGE_SPANS.K01.lastId}。`,
    });
  }

  // 9.4 主题归属（每条须属官方十主题）。
  for (const row of table) {
    if (!K_DOMAIN_TEN_TOPICS.includes(row.topic)) {
      diagnostics.push({
        code: "WORKTABLE_GAP",
        message: `条目 F${row.entryId} 主题「${row.topic}」不属 K 域官方十主题。`,
        hint: `主题须取自 ${K_DOMAIN_TEN_TOPICS.join("、")}。`,
      });
    }
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.message, first.hint, diagnostics);
    }
  }
  return ok([...table], diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §10 架构修正 ADR（与 F2002 / 上游冻结序冲突时回改本条）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 一条架构修正提案。 */
export interface ArchitectureAmendment {
  readonly proposedByEntryId: number;
  readonly target: "EFFECT_INTERFACE" | "PIPELINE_ORDER" | "RT_POOL" | "WORK_TABLE";
  readonly rationale: string;
  readonly impact: string;
}

/**
 * 受理架构修正提案。信息不全即拒绝——防止「顺手改架构」而无据。
 * R2/R3 触发的回改必须走此流程并留痕，不允许静默漂移。
 */
export function proposeArchitectureAmendment(
  amendment: ArchitectureAmendment,
): Outcome<{ readonly adrId: string; readonly accepted: boolean }> {
  if (amendment.rationale.trim().length === 0) {
    return err(
      "ADR_INCOMPLETE",
      `F${amendment.proposedByEntryId} 的架构修正提案缺少理由。`,
      "ADR 须写明修正动因；无理由的架构变更不予受理，请补充后再提交。",
    );
  }
  if (amendment.impact.trim().length === 0) {
    return err(
      "ADR_INCOMPLETE",
      `F${amendment.proposedByEntryId} 的架构修正提案缺少影响面。`,
      "ADR 须列明受影响的效果声明 / 挂接位 / 下游条目，便于回改时同步对齐。",
    );
  }
  const adrId = `ADR-K-${String(amendment.proposedByEntryId).padStart(4, "0")}`;
  return ok({ adrId, accepted: true });
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §11 架构冻结快照 + 漂移检测
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 架构冻结快照：域开工的机器可读凭据。 */
export interface ArchitectureFreeze {
  readonly domainTag: "VE-K";
  readonly firstEntryId: number;
  readonly lastEntryId: number;
  readonly stages: readonly PostStage[];
  /** 效果 id 集合（无序语义，排序后参与哈希）。 */
  readonly effectIds: readonly string[];
  readonly inboundContracts: readonly InboundContractKind[];
  readonly workTableRows: number;
  readonly frozenAt: number;
}

/**
 * 快照指纹（FNV-1a 32 位，短、稳定、无依赖，非安全用途）。
 * effectIds 与 inboundContracts 是集合语义——排序后参与哈希，否则同一架构
 * 经不同构造路径会得到不同指纹，漂移检测将大量误报（守卫失效而非架构真变了）。
 */
export function freezeFingerprint(freeze: ArchitectureFreeze): string {
  const parts = [
    freeze.domainTag,
    `${freeze.firstEntryId}-${freeze.lastEntryId}`,
    freeze.stages.join(","),
    [...freeze.effectIds].sort().join(","),
    [...freeze.inboundContracts].sort().join(","),
    String(freeze.workTableRows),
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
  const frozenFp = freezeFingerprint(frozen);
  const currentFp = freezeFingerprint(current);
  if (frozenFp !== currentFp) {
    return err(
      "FREEZE_DRIFT",
      `架构漂移：冻结指纹 ${frozenFp} ≠ 当前指纹 ${currentFp}。`,
      "架构声明已被改动但未走 ADR 回改本条；补 ADR 或还原至冻结态，二选一。",
    );
  }
  return ok(frozen);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §12 域开工（编排）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 域开工结果。 */
export interface DomainKickoff {
  readonly freeze: ArchitectureFreeze;
  readonly fingerprint: string;
  readonly diagnostics: readonly Diagnostic[];
}

/**
 * 宣告 K 域开工。
 *
 * 编排四道关（顺序即依赖顺序，任一硬闸失败则不开工）：
 *   1. 三契约接收核对（F1997）——缺签即阻断并点名，硬闸（R1）；
 *   2. 分工表校验——基数/重复/连续性/主题归属，硬闸；
 *   3. 效果接口 × F2002 能力比对——冲突即回改本条，硬闸（R2）；
 *   4. 产出架构冻结快照与指纹。
 *
 * @param now 冻结时间戳（调用方注入，本模块不读时钟，保持可测试与可重放）。
 */
export function openDomain(bundle: InboundContractBundle, now: number): Outcome<DomainKickoff> {
  // 12.1 三契约接收（硬闸 · R1）。
  const contracts = verifyInboundContracts(bundle);
  if (!contracts.ok) return contracts;

  // 12.2 分工表校验（硬闸）。
  const wt = verifyWorkTable(K01_WORK_TABLE);
  if (!wt.ok) return wt;

  // 12.3 效果接口 × 执行器能力（硬闸 · R2）。
  const exec = checkAgainstExecutor({
    rtPool: true,
    slotDependencyResolution: true,
    effectToggle: true,
    inPlaceEffect: true,
    maxLiveEffects: 8,
  });
  if (!exec.ok) return exec;

  // 12.4 冻结。
  const freeze: ArchitectureFreeze = {
    domainTag: K_DOMAIN.tag as "VE-K",
    firstEntryId: K_DOMAIN.firstEntryId,
    lastEntryId: K_DOMAIN.lastEntryId,
    stages: [...POST_STAGE_ORDER],
    effectIds: [],
    inboundContracts: contracts.value,
    workTableRows: wt.value.length,
    frozenAt: now,
  };

  const merged = [...contracts.diagnostics, ...wt.diagnostics];
  return ok({ freeze, fingerprint: freezeFingerprint(freeze), diagnostics: merged }, merged);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §13 域开工宣告文本（架构文档替述可读，无障碍要求）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 生成 K 域开工宣告（人话版）。
 * 无障碍要求：架构文档替述可读——本函数是文档的可执行生成源，
 * 保证「文档所述」与「代码所声明」同源，不会各说各话。
 */
export function renderKickoffDeclaration(freeze: ArchitectureFreeze, fingerprint: string): string {
  const stages = freeze.stages
    .map((s) => `${POST_STAGE_LABELS[s]}（${s}）`)
    .join(" → ");
  const contracts = freeze.inboundContracts.map((k) => INBOUND_CONTRACT_META[k].label).join("、");
  return [
    "【VE-K 后处理链域 · 开工宣告】",
    `条目区间：F${freeze.firstEntryId}-F${freeze.lastEntryId}（共 ${K_DOMAIN.entryCount} 条 / ${K_DOMAIN.groupCount} 组）。`,
    `官方主题（十项）：${K_DOMAIN_TEN_TOPICS.join("、")}。`,
    `基准管线序：${stages}。`,
    "基准序为默认：DAG 允许重排（依赖自洽即放行），但读取未产出 RT 的依赖违反为硬错。",
    "上游冻结序（I02 输出交接 / J 域 F1841 氛围末端）不可由 K 域反向改动，冲突一律回改 K 域设计。",
    `接收上游契约（三份）：${contracts}。`,
    `K01 组分工表：${freeze.workTableRows} 条（F${K_STAGE_SPANS.K01.firstId}-F${K_STAGE_SPANS.K01.lastId}）。`,
    `效果接口零虚拟开销：效果以数据化声明表达，执行器构建期生成分发表，运行时按 id 查表调用。`,
    `架构冻结指纹：${fingerprint}（冻结时间戳 ${freeze.frozenAt}）。`,
  ].join("\n");
}
