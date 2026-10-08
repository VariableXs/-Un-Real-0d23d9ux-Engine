/**
 * VE-F1801 · J 域开工与光照架构（J 域 · 光照与阴影域 · 批次 J01 · 组内第 1 条）
 * ---------------------------------------------------------------------------
 * 职责定位：J 域开工条目——宣告光照与阴影域（F1801-F2000，官方主题十项：直接光 /
 *   方向光 / 点光 / 聚光 / 阴影贴图 / CSM / SSAO / SSR / 探针 / 光线追踪接口预留）
 *   正式开工。本条定义**域架构**本身：
 *     1. 光照三段式管线序的前置架构声明（直接光组 → 阴影组 → 环境氛围组）；
 *     2. 与 I 域绘制管线（I02 绘制族）的**挂接位**契约；
 *     3. 光源抽象基类接口草案（类型枚举 + 公共参数块 + 逐类型扩展块）；
 *     4. J01 组内 20 条的分工总览（分工表 20 行）。
 *
 * 本条是纯架构声明条目，**无运行时渲染实现**：它交付的是一份可被机器校验的
 * 架构契约（挂接位序、接口草案、分工表），由后续 19 条与 J02/J03 三个批次消费。
 *
 * ┌── 上游契约 ──────────────────────────────────────────────────────────────┐
 * │ I02 绘制族 pass 序已冻结（F1680 组收口冻结态）。本条**只读消费**该序，   │
 * │ 不反向改动：若本条声明的挂接位与 I02 冻结序冲突，裁决规则是「以 I02 为  │
 * │ 准，调整 J 域设计」，产出 PIPELINE_ATTACH_CONFLICT 诊断 + 重排建议，     │
 * │ 绝不产出任何指向 I02 的改写。这是本条最重要的不可协商项。              │
 * └─────────────────────────────────────────────────────────────────────────┘
 *
 * 三段管线序（锚点原文：直接光计算在 I02 光照 pass 内执行 / 阴影 pass 为 I02 的
 * 附加深度 pass / 环境氛围按 F1841 冻结序挂接）：
 *
 *   帧内序（I02 冻结 pass 序内的相对位置）
 *   ────────────────────────────────────────────────────────────────────
 *   [阴影深度 pass · SHADOW_DEPTH]     ← J02 组的附加深度 pass（J 域声明其位）
 *        ↓  产出 shadow map，供直接光采样
 *   [I02 光照 pass · LIGHTING]         ← J01 组在此执行（挂接位 LIGHTING_SLOT）
 *        │   ├ 直接光组 J01（F1802-F1812）
 *        │   └ 阴影采样（J02 产出的 shadow map 在此处被消费）
 *        ↓  产出线性 HDR 颜色
 *   [环境氛围 pass · ATMOSPHERE]       ← J03 组按 F1841 冻结序挂接
 *        │   ├ SSAO / SSR / 探针 / IBL（F1841-F1860）
 *        ↓
 *   [I02 后段 · 后处理交接]            ← 曝光输出交 I02 后段（曝光语义归 F1812，UI 亮度归 V 域）
 *
 * 域开工前置检查（锚点原文：I 域移交包 F1797 接收确认未签 → 开工阻断）：
 *   `verifyKickoffPreconditions` 是开工闸门。移交包四段材料（接口冻结清单 /
 *   算力标签终册 / 经验教训记录 / 证据三件套）中，光照相关段与接收确认位缺一
 *   即返回 ok:false，阻断开工。闸门不通过时 `openDomain` 不产出架构冻结快照。
 *
 * 光源抽象接口草案（锚点原文：类型枚举 + 公共参数块 + 逐类型扩展块）：
 *   本条给的是**草案层**——类型枚举 `LightType` 与 `LightSource` 抽象契约
 *   （公共参数块 color/intensity/enabled/range + 逐类型扩展块的挂载位）。
 *   具体的参数块布局由 F1807 光源管理器注册时确定，本条只保证：
 *     (a) 新增光源类型不改既有类型的字段语义（扩展块隔离）；
 *     (b) 预留类型（面光源 F1806）在被引用时显性报错，不静默降级。
 *   冲突处置：若后续组（F1803-F1807）的实现与本草案冲突，
 *   走 `proposeArchitectureAmendment` 产出 ADR 草案，由组实现期回改本条目——
 *   架构声明可以被修正，但修正必须留痕、可追溯，不允许静默漂移。
 *
 * 零静默纪律：本模块所有拒绝、冲突、阻断、预留未实现都产出 Diagnostic
 *   （code + message + hint），由调用方聚合上报。本模块不向 UI 直接抛异常，
 *   也不吞掉任何一条诊断。架构冲突（挂接位撞 I02）是**显性事件**，不是静默降级。
 *
 * 判据：域开工、三段管线序、光源抽象、移交接收。
 *
 * 依赖锚点：F1797（I 域移交包 · 光照相关段 + 接收确认）、F1680（I02 绘制族 pass 序
 *   冻结态）、F1761（域性能账本，J 段由 F1811 扩）、F1762（质量档）、
 *   F1764（调试总线信封）、F1767（CI 门禁）、F1776（显存配额池扩展位）、
 *   F1841（环境氛围冻结序）。
 * 下游消费：F1803 方向光 / F1804 点光 / F1805 聚光 / F1806 面光源预留 /
 *   F1807 光源管理器（五条实现本条的光源抽象）、F1809 IBL（兑现 I03 F1642 预留）、
 *   F1821 阴影架构（J02 开工消费本条三段序）、F1841 环境氛围（J03 开工消费）、
 *   F1811 光照性能预算（消费本条的成本分段位）、F1819 光照遥测。
 * 交接说明：本条只管「架构声明 + 挂接位 + 接口草案 + 分工表」，具体光源着色
 *   实现归 F1803-F1807，阴影实现归 J02，环境氛围归 J03，成本定标归 F1811。
 */

/* ═══════════════════════════════════════════════════════════════════════════
 * §1 诊断与结果类型（零静默的基础设施）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 诊断码：每种拒绝/冲突/阻断/预留都有独立可检索的码，绝不合并成通用错误。 */
export type DiagCode =
  /** 挂接位与 I02 冻结 pass 序冲突——以 I02 为准，需重排 J 域设计。 */
  | "PIPELINE_ATTACH_CONFLICT"
  /** J 域试图反向改写 I02 冻结序——直接拒绝（不可协商项）。 */
  | "PIPELINE_UPSTREAM_MUTATION_REFUSED"
  /** 三段管线序缺失或乱序。 */
  | "PIPELINE_STAGE_SEQUENCE_INVALID"
  /** 挂接位名称不在 I02 提供的挂接点清单内。 */
  | "ATTACH_SLOT_UNKNOWN"
  /** 光源扩展块未在抽象契约中登记。 */
  | "LIGHT_EXTENSION_UNREGISTERED"
  /** 光源类型被引用但为预留未实现类型（面光源 F1806）。 */
  | "LIGHT_TYPE_RESERVED"
  /** 公共参数块字段缺失或越界。 */
  | "LIGHT_COMMON_BLOCK_INVALID"
  /** 移交包材料缺失（接口冻结清单/算力标签终册/教训册/证据三件套）。 */
  | "HANDOVER_MATERIAL_MISSING"
  /** 移交包接收确认位未签——开工阻断。 */
  | "HANDOVER_NOT_CONFIRMED"
  /** 分工表条目数与 J01 组 20 条不符。 */
  | "WORKTABLE_CARDINALITY_INVALID"
  /** 分工表存在重复条目号。 */
  | "WORKTABLE_DUPLICATE_ENTRY"
  /** 分工表条目号不连续（缺位）。 */
  | "WORKTABLE_GAP"
  /** 性能预算分段位缺失（J 段由 F1811 承担）。 */
  | "COST_SEGMENT_SLOT_MISSING"
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

/** 成功构造（diagnostics 允许携带非致命告警，例如冲突已按 I02 裁决降级）。 */
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
 * §2 域标识与三段管线序声明
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * J 域标识与官方十主题。
 * 域号 J，条目区间 F1801-F2000（200 条 = 10 组 × 20 条）。
 */
export const J_DOMAIN = {
  /** 域标签，与册内 `VE-J` 一致。 */
  tag: "VE-J",
  /** 域中文名。 */
  title: "光照与阴影域",
  /** 条目起始（域开工条）。 */
  firstEntryId: 1801,
  /** 条目结束（域收官条）。 */
  lastEntryId: 2000,
  /** 条目总数。 */
  entryCount: 200,
  /** 组数（10 组，每组 20 条）。 */
  groupCount: 10,
  /** 每组条数。 */
  entriesPerGroup: 20,
} as const;

/**
 * J 域官方十主题（锚点原文：直接光/方向光/点光/聚光/阴影贴图/CSM/SSAO/SSR/
 * 探针/光线追踪接口预留）。用于域开工宣告与分工表主题归类核对。
 */
export const J_DOMAIN_TEN_TOPICS = [
  "直接光",
  "方向光",
  "点光",
  "聚光",
  "阴影贴图",
  "CSM",
  "SSAO",
  "SSR",
  "探针",
  "光线追踪接口预留",
] as const;

export type JDomainTopic = (typeof J_DOMAIN_TEN_TOPICS)[number];

/**
 * 光照三段式管线序的段标识。
 * 锚点原文：直接光组 → 阴影组 → 环境氛围组。
 */
export type LightStage =
  /** 直接光组（J01，F1801-F1820）：方向光/点光/聚光/探针/IBL/曝光的成本来源。 */
  | "DIRECT_LIGHT"
  /** 阴影组（J02，F1821-F1840）：阴影深度 pass 与阴影贴图采样。 */
  | "SHADOW"
  /** 环境氛围组（J03，F1841-F1860）：SSAO/SSR/IBL 天空/雾等。 */
  | "ATMOSPHERE";

/** 三段序的规范次序（索引即帧内相对执行序，不可乱序）。 */
export const LIGHT_STAGE_ORDER: readonly LightStage[] = ["DIRECT_LIGHT", "SHADOW", "ATMOSPHERE"];

/** 每个三段组对应的条目区间与挂接点，供分工表归类与挂接校验共用。 */
export const LIGHT_STAGE_SPANS: Readonly<
  Record<LightStage, { groupLabel: string; firstId: number; lastId: number }>
> = {
  DIRECT_LIGHT: { groupLabel: "J01", firstId: 1801, lastId: 1820 },
  SHADOW: { groupLabel: "J02", firstId: 1821, lastId: 1840 },
  ATMOSPHERE: { groupLabel: "J03", firstId: 1841, lastId: 1860 },
};

/** 一个挂接点：J 域声明「我需要在这个 pass 上挂一段」。 */
export interface AttachSlot {
  /** 挂接点名（对应 I02 提供给 J 域的挂接点清单项）。 */
  readonly name: string;
  /** 挂接的段。 */
  readonly stage: LightStage;
  /** 该挂接点是否为附加 pass（阴影 pass 是 I02 的附加深度 pass）。 */
  readonly additionalPass: boolean;
  /** 挂接序（三段序内的序号，用于校验阶段先后）。 */
  readonly order: number;
}

/**
 * J 域三段管线序挂接位声明（锚点原文的直接架构声明）。
 *
 * 挂接位名称取自 I02 绘制族提供的挂接点清单；本条只读消费、不反向定义。
 * 阴影为附加深度 pass（additionalPass = true），直接光挂在 I02 光照 pass 内，
 * 环境氛围按 F1841 冻结序挂接。
 */
export const J_PIPELINE_ATTACH_SLOTS: readonly AttachSlot[] = [
  { name: "SHADOW_DEPTH_PASS", stage: "SHADOW", additionalPass: true, order: 0 },
  { name: "LIGHTING_PASS", stage: "DIRECT_LIGHT", additionalPass: false, order: 1 },
  { name: "ATMOSPHERE_PASS", stage: "ATMOSPHERE", additionalPass: false, order: 2 },
];

/**
 * I02 绘制族的挂接点清单（只读消费 F1680 冻结态的简化投影）。
 *
 * 这里以数据形式固化「I02 提供了哪些挂接点」，使挂接校验可离线执行——
 * 架构契约不该依赖运行时渲染环境才能验证。
 */
export const I02_AVAILABLE_SLOTS: readonly string[] = [
  "SHADOW_DEPTH_PASS",
  "LIGHTING_PASS",
  "ATMOSPHERE_PASS",
  "POST_HANDOFF",
];

/** 一处挂接冲突：J 域声明的挂接位与 I02 冻结序的裁决记录。 */
export interface AttachConflict {
  /** 冲突的挂接点名。 */
  readonly slotName: string;
  /** J 域原声明的段。 */
  readonly declaredStage: LightStage;
  /** I02 侧该挂接点实际归属的段（若 I02 已把该点分配给其他域则为 undefined）。 */
  readonly upstreamStage: LightStage | undefined;
  /** 裁决结果：恒为「以 I02 为准，调整 J 域设计」。 */
  readonly resolution: "UPSTREAM_WINS_ADJUST_JDOMAIN";
  /** 给 J 域的重排建议。 */
  readonly suggestion: string;
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §3 光源抽象接口草案
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 光源类型枚举（锚点原文：类型枚举 + 公共参数块 + 逐类型扩展块）。
 *
 * 面光源（F1806）为**预留**类型：接口位冻结防后续破坏性变更，但一期不实现。
 * 引用预留类型必须显性报错，不静默降级。
 */
export type LightType =
  /** 方向光（F1803）：平行光 + 太阳模型。 */
  | "DIRECTIONAL"
  /** 点光（F1804）：平方反比衰减 + 范围钳制。 */
  | "POINT"
  /** 聚光（F1805）：内外锥角 + 半影平滑。 */
  | "SPOT"
  /** 面光源（F1806）：一期预留未实现，接口位冻结。 */
  | "AREA_RECT"
  /** 管灯面光源（F1806）：一期预留未实现，接口位冻结。 */
  | "AREA_TUBE";

/** 光源类型 → 实现条目号（用于架构一致性反查：类型必须由对应条目实现）。 */
export const LIGHT_TYPE_OWNER: Readonly<Record<LightType, number>> = {
  DIRECTIONAL: 1803,
  POINT: 1804,
  SPOT: 1805,
  AREA_RECT: 1806,
  AREA_TUBE: 1806,
};

/** 预留未实现类型集合（F1806 面光源族）。 */
export const RESERVED_LIGHT_TYPES: readonly LightType[] = ["AREA_RECT", "AREA_TUBE"];

/** 线性颜色（线性 sRGB，非 gamma）——跨类型公共。 */
export interface LinearColor {
  readonly r: number;
  readonly g: number;
  readonly b: number;
}

/** 光源公共参数块（锚点：公共参数块）。字段语义跨类型一致，故放基类而非子类。 */
export interface LightCommonBlock {
  /** 线性 sRGB 颜色。 */
  readonly color: LinearColor;
  /** 强度标量（单位制语义由 F1802 定标：物理制 cd/lux）。 */
  readonly intensity: number;
  /** 是否启用。 */
  readonly enabled: boolean;
  /** 有效半径（0 表示方向光这类无空间范围的类型）。 */
  readonly rangeRadius: number;
}

/**
 * 逐类型扩展块（锚点：逐类型扩展块）。
 * 用可辨识联合：type 字段即判别键，扩展块之间互不污染。
 * 这保证「新增光源类型不改既有类型字段语义」——扩展块隔离而非合并进公共块。
 */
export type LightExtension =
  /** 方向光扩展（F1803）：方向向量 + 太阳模型开关。 */
  | { readonly type: "DIRECTIONAL"; readonly direction: { readonly x: number; readonly y: number; readonly z: number }; readonly solarModel: boolean }
  /** 点光扩展（F1804）：位置 + 衰减曲线枚举。 */
  | { readonly type: "POINT"; readonly position: { readonly x: number; readonly y: number; readonly z: number }; readonly falloffCurve: "PHYSICAL" | "ARTISTIC" }
  /** 聚光扩展（F1805）：位置 + 方向 + 内外锥角。 */
  | {
      readonly type: "SPOT";
      readonly position: { readonly x: number; readonly y: number; readonly z: number };
      readonly direction: { readonly x: number; readonly y: number; readonly z: number };
      readonly innerConeRad: number;
      readonly outerConeRad: number;
    }
  /** 面光源扩展（F1806）：一期预留——尺寸 + LTC 矩阵引用位（未实现）。 */
  | { readonly type: "AREA_RECT"; readonly width: number; readonly height: number; readonly ltcMatrixRef: string | null }
  /** 管灯面光源扩展（F1806）：一期预留——长度 + 半径 + LTC 矩阵引用位（未实现）。 */
  | { readonly type: "AREA_TUBE"; readonly length: number; readonly radius: number; readonly ltcMatrixRef: string | null };

/** 光源抽象基类契约（接口草案层）。 */
export interface LightSource {
  readonly type: LightType;
  readonly common: LightCommonBlock;
  /** 扩展块挂载位：类型与 type 不符即为契约违规。 */
  readonly extension: LightExtension;
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §4 挂接位校验与裁决（不可协商项：禁止反向改动 I02）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 校验挂接位声明与 I02 冻结序的一致性。
 *
 * 裁决规则（锚点原文硬约束）：与 I02 管线挂接位冲突 → 以 I02 为准调 J 域设计，
 * 禁止反向改动。本函数因此**只产出调整 J 域的建议**，永不产出修改 I02 的动作。
 *
 * @param declared J 域声明的挂接位
 * @param upstreamSlots I02 提供的挂接点清单（默认取冻结态投影）
 */
export function verifyAttachSlots(
  declared: readonly AttachSlot[],
  upstreamSlots: readonly string[] = I02_AVAILABLE_SLOTS,
): Outcome<{ readonly conflicts: readonly AttachConflict[] }> {
  const diagnostics: Diagnostic[] = [];
  const conflicts: AttachConflict[] = [];

  // 4.1 挂接点存在性：J 域只能挂 I02 明确提供的点。
  for (const slot of declared) {
    if (!upstreamSlots.includes(slot.name)) {
      const message = `挂接点 ${slot.name} 不在 I02 提供的挂接点清单内。`;
      const hint = `改挂 I02 已提供的挂接点（${upstreamSlots.join("、")}），或先与 I 域协商新增挂接点后再挂。禁止自造挂接点名。`;
      diagnostics.push({ code: "ATTACH_SLOT_UNKNOWN", message, hint });
      conflicts.push({
        slotName: slot.name,
        declaredStage: slot.stage,
        upstreamStage: undefined,
        resolution: "UPSTREAM_WINS_ADJUST_JDOMAIN",
        suggestion: hint,
      });
    }
  }

  // 4.2 挂接序单调：order 必须随数组序递增，否则三段序乱序。
  for (let i = 1; i < declared.length; i++) {
    const prev = declared[i - 1];
    const cur = declared[i];
    if (prev !== undefined && cur !== undefined && cur.order <= prev.order) {
      const message = `挂接序乱序：${prev.name}(order=${prev.order}) 之后是 ${cur.name}(order=${cur.order})。`;
      const hint = `按 LIGHT_STAGE_ORDER（${LIGHT_STAGE_ORDER.join(" → ")}）重排挂接位 order，严格递增。`;
      diagnostics.push({ code: "PIPELINE_STAGE_SEQUENCE_INVALID", message, hint });
    }
  }

  // 4.3 段覆盖：三段式必须三段齐备，缺一段即架构声明不完整。
  const coveredStages = new Set(declared.map((s) => s.stage));
  const missing = LIGHT_STAGE_ORDER.filter((stage) => !coveredStages.has(stage));
  if (missing.length > 0) {
    const message = `三段管线序缺失：${missing.join("、")} 未声明挂接位。`;
    const hint = `补齐缺失段的挂接位；直接光挂 I02 光照 pass 内，阴影为附加深度 pass，环境氛围按 F1841 冻结序挂接。`;
    diagnostics.push({ code: "PIPELINE_STAGE_SEQUENCE_INVALID", message, hint });
  }

  // 4.4 冲突产出：语义上「J 域想挂的」与「I02 已分配的」不一致即冲突，
  //     但既有声明全部合法时（同段同点）不产冲突。
  const declaredNames = new Set(declared.map((s) => s.name));
  for (const name of upstreamSlots) {
    // I02 把某点分给了非 J 域的用途，而 J 域也声明了该点 → 真冲突。
    const upstreamOwner: LightStage | undefined = I02_UPSTREAM_SLOT_OWNER[name];
    if (upstreamOwner !== undefined && declaredNames.has(name)) {
      const declaredSlot = declared.find((s) => s.name === name);
      if (declaredSlot !== undefined && declaredSlot.stage !== upstreamOwner) {
        conflicts.push({
          slotName: name,
          declaredStage: declaredSlot.stage,
          upstreamStage: upstreamOwner,
          resolution: "UPSTREAM_WINS_ADJUST_JDOMAIN",
          suggestion: `I02 已把 ${name} 定为 ${upstreamOwner} 段挂接点，J 域不得反向改动。请将 J 域对 ${name} 的挂接调整到 ${upstreamOwner} 段语义相符的位置，或申请 I 域侧新增独立挂接点。`,
        });
      }
    }
  }

  // 4.5 冲突是显性事件：任一冲突都产出诊断（不静默），但函数本身成功返回
  //     冲突清单——调用方据此重排，这是「以 I02 为准」的可执行形态。
  for (const c of conflicts) {
    diagnostics.push({
      code: "PIPELINE_ATTACH_CONFLICT",
      message: `挂接位 ${c.slotName} 冲突：J 域声明为 ${c.declaredStage} 段。`,
      hint: c.suggestion,
    });
  }

  return ok({ conflicts }, diagnostics);
}

/** I02 侧挂接点的归属段（冻结态）。POST_HANDOFF 未分配给 J 域三段 → undefined。 */
const I02_UPSTREAM_SLOT_OWNER: Readonly<Record<string, LightStage>> = {
  SHADOW_DEPTH_PASS: "SHADOW",
  LIGHTING_PASS: "DIRECT_LIGHT",
  ATMOSPHERE_PASS: "ATMOSPHERE",
};

/**
 * 拒绝任何反向改动 I02 冻结 pass 序的请求（不可协商项的守卫函数）。
 *
 * 存在的意义：把「禁止反向改动」从注释变成可执行断言。任何试图让 J 域改写
 * I02 pass 序的调用都会在这里被显性拒绝，而不是悄悄生效。
 */
export function refuseUpstreamPipelineMutation(
  attemptedSlot: string,
  reason: string,
): Outcome<never> {
  const message = `拒绝反向改动 I02 冻结 pass 序：尝试修改挂接点 ${attemptedSlot}（${reason}）。`;
  const hint =
    "I02 pass 序已由 F1680 冻结，J 域挂接冲突一律以 I02 为准、调整 J 域设计。" +
    "若确需 I02 侧变更，先走 I 域 ADR 流程解冻，改动完成后再回来对齐挂接位。";
  return err("PIPELINE_UPSTREAM_MUTATION_REFUSED", message, hint);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §5 光源抽象校验（公共块 + 扩展块 + 预留类型显性报错）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 校验一条光源声明是否符合抽象契约（公共块合法 + 扩展块匹配类型 + 预留类型报错）。 */
export function validateLightSource(source: LightSource): Outcome<LightSource> {
  const diagnostics: Diagnostic[] = [];

  // 5.1 公共块：颜色分量有限、强度有限非负、半径有限非负。
  const { color, intensity, rangeRadius } = source.common;
  if (![color.r, color.g, color.b].every((v) => Number.isFinite(v))) {
    diagnostics.push({
      code: "LIGHT_COMMON_BLOCK_INVALID",
      message: "公共参数块颜色含非有限值（NaN/Infinity）。",
      hint: "颜色三通道必须为有限实数；导入期修正资产，运行时按 F1802 钳制策略处理。",
    });
  }
  if (!Number.isFinite(intensity) || intensity < 0) {
    diagnostics.push({
      code: "LIGHT_COMMON_BLOCK_INVALID",
      message: `公共参数块强度非法：${intensity}。`,
      hint: "强度须为非负有限实数；负值与 NaN 由 F1802 钳制到 [0, 上限] 并遥测。",
    });
  }
  if (!Number.isFinite(rangeRadius) || rangeRadius < 0) {
    diagnostics.push({
      code: "LIGHT_COMMON_BLOCK_INVALID",
      message: `公共参数块有效半径非法：${rangeRadius}。`,
      hint: "半径须为非负有限实数；点光/聚光范围 ≤0 由 F1804 钳制到最小有效半径并告警。",
    });
  }

  // 5.2 扩展块与类型必须一致（扩展块隔离的核心守卫）。
  if (source.extension.type !== source.type) {
    diagnostics.push({
      code: "LIGHT_EXTENSION_UNREGISTERED",
      message: `扩展块类型 ${source.extension.type} 与光源类型 ${source.type} 不匹配。`,
      hint: "扩展块必须与 type 同判别键；跨类型挂载属契约违规，按类型各自的参数块注册。",
    });
  }

  // 5.3 预留类型被引用 → 显性报错（不静默降级为点光近似）。
  if (RESERVED_LIGHT_TYPES.includes(source.type)) {
    diagnostics.push({
      code: "LIGHT_TYPE_RESERVED",
      message: `光源类型 ${source.type} 为一期预留（F1806 面光源族），尚未实现。`,
      hint:
        "当前版本请改用点光/聚光阵列近似；预留接口位已冻结防破坏性变更，但一期不参与调度、不产生光照。",
    });
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.message, first.hint, diagnostics);
    }
  }
  return ok(source, diagnostics);
}

/**
 * 登记一个光源类型到抽象契约（供 F1807 管理器注册时校验归属条目）。
 * 校验：类型必须有归属实现条目；重复登记报错（防类型语义漂移）。
 */
const registeredLightTypes = new Map<LightType, number>();

export function registerLightType(type: LightType, ownerEntryId: number): Outcome<LightType> {
  if (LIGHT_TYPE_OWNER[type] !== ownerEntryId) {
    return err(
      "LIGHT_EXTENSION_UNREGISTERED",
      `光源类型 ${type} 归属条目应为 F${LIGHT_TYPE_OWNER[type]}，收到 F${ownerEntryId}。`,
      "修正注册归属：方向光→F1803、点光→F1804、聚光→F1805、面光源族→F1806。",
    );
  }
  const existing = registeredLightTypes.get(type);
  if (existing !== undefined && existing !== ownerEntryId) {
    return err(
      "LIGHT_EXTENSION_UNREGISTERED",
      `光源类型 ${type} 已由 F${existing} 注册，不可改由 F${ownerEntryId} 注册。`,
      "光源类型语义一经冻结不换归属；若需换实现条目走 ADR 并同步更新 LIGHT_TYPE_OWNER。",
    );
  }
  registeredLightTypes.set(type, ownerEntryId);
  return ok(type);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §6 域开工前置检查（I 域移交包 F1797 接收确认）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 移交包四段材料（锚点 F1797：接口冻结清单/算力标签终册/经验教训记录/证据三件套）。 */
export type HandoverMaterialKind =
  | "INTERFACE_FREEZE"
  | "COMPUTE_LABEL_LEDGER"
  | "LESSON_BOOK"
  | "EVIDENCE_TRIO";

/** 移交包四段材料名（可读）。 */
export const HANDOVER_MATERIAL_LABELS: Readonly<Record<HandoverMaterialKind, string>> = {
  INTERFACE_FREEZE: "接口冻结清单",
  COMPUTE_LABEL_LEDGER: "算力标签终册",
  LESSON_BOOK: "经验教训记录",
  EVIDENCE_TRIO: "证据三件套",
};

/** 移交包材料的一段。 */
export interface HandoverMaterial {
  readonly kind: HandoverMaterialKind;
  readonly present: boolean;
  /** 该段是否含 J 域相关子段（如接口冻结清单含 I02 pass 序、算力标签含光照标签）。 */
  readonly lightingRelevant: boolean;
}

/** 移交包整体。 */
export interface HandoverPackage {
  readonly materials: readonly HandoverMaterial[];
  /** 接收方（J 域）确认位——未签即开工阻断。 */
  readonly receiverConfirmed: boolean;
  readonly receiverSignature: string | null;
}

/**
 * 域开工前置检查（锚点原文：I 域移交包 F1797 接收确认未签 → 开工阻断）。
 *
 * 阻断条件（三类，任一命中即阻断）：
 *   (1) 四段材料任一缺失；
 *   (2) 光照相关子段缺失（I02 pass 序冻结态拿不到 → 无法校验挂接位）；
 *   (3) J 域接收确认位未签。
 */
export function verifyKickoffPreconditions(
  pkg: HandoverPackage,
): Outcome<{ readonly lightingSections: readonly HandoverMaterialKind[] }> {
  const diagnostics: Diagnostic[] = [];

  // 6.1 材料齐备性：四段材料每一种都必须出现（缺整段也算缺失）。
  //     注意：只查 `!m.present` 会漏掉「某段材料根本没提交」的情形——
  //     那时剩余各段皆 present，缺失判断不会触发。故先按 kind 归集再比对全集。
  const presentKinds = new Set(pkg.materials.filter((m) => m.present).map((m) => m.kind));
  const allKinds: readonly HandoverMaterialKind[] = [
    "INTERFACE_FREEZE",
    "COMPUTE_LABEL_LEDGER",
    "LESSON_BOOK",
    "EVIDENCE_TRIO",
  ];
  for (const kind of allKinds) {
    if (!presentKinds.has(kind)) {
      diagnostics.push({
        code: "HANDOVER_MATERIAL_MISSING",
        message: `移交包材料缺失：${HANDOVER_MATERIAL_LABELS[kind]}。`,
        hint: "向 I 域（F1797）索取缺失材料；四段材料缺一不可开工。",
      });
    }
  }
  // 显式标记为 present=false 的段同样计入缺失（重复提交时给出全部缺项）。
  for (const m of pkg.materials) {
    if (!m.present && presentKinds.has(m.kind)) {
      diagnostics.push({
        code: "HANDOVER_MATERIAL_MISSING",
        message: `移交包材料冲突：${HANDOVER_MATERIAL_LABELS[m.kind]} 同时标记为在场与缺失。`,
        hint: "移交包材料状态自相矛盾，先向 I 域核对提交记录再开工。",
      });
    }
  }

  // 6.2 光照相关子段（挂接位校验的前提）。
  const lightingSections = pkg.materials.filter((m) => m.present && m.lightingRelevant).map((m) => m.kind);
  if (lightingSections.length === 0) {
    diagnostics.push({
      code: "HANDOVER_MATERIAL_MISSING",
      message: "移交包中无光照相关子段（I02 pass 序冻结态不可得）。",
      hint: "挂接位校验依赖 I02 冻结序；请从接口冻结清单取 I02 绘制族 pass 序，缺失则无法离线校验架构。",
    });
  }

  // 6.3 接收确认位。
  if (!pkg.receiverConfirmed || pkg.receiverSignature === null) {
    diagnostics.push({
      code: "HANDOVER_NOT_CONFIRMED",
      message: "J 域接收确认位未签。",
      hint: "在移交包接收确认位签 J 域（AI 域号 + 日期）；确认前不得宣告 J 域开工。",
    });
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.message, first.hint, diagnostics);
    }
  }
  return ok({ lightingSections }, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §7 J01 组分工表（20 行）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 分工表中一条：条目号 + 标题 + 归属主题 + 交付定位。 */
export interface WorkTableRow {
  readonly entryId: number;
  readonly title: string;
  readonly topic: JDomainTopic;
  /** 该条在本域架构中的角色。 */
  readonly role: string;
  /** 上游依赖条目（空串表示无）。 */
  readonly upstream: string;
}

/**
 * J01 组 20 条分工表（F1801-F1820，标题逐条核对册内锚点）。
 * 本条（VE-F1801）为组内第 1 条，即域开工与光照架构本身。
 */
export const J01_WORK_TABLE: readonly WorkTableRow[] = [
  { entryId: 1801, title: "J 域开工与光照架构", topic: "直接光", role: "域开工·三段序·挂接位·光源抽象·分工表", upstream: "F1797" },
  { entryId: 1802, title: "物理光照量纲", topic: "直接光", role: "四单位换算与双模式强度语义定标", upstream: "F1801" },
  { entryId: 1803, title: "方向光", topic: "方向光", role: "平行光+太阳模型；J02 CSM 的主光源", upstream: "F1802" },
  { entryId: 1804, title: "点光", topic: "点光", role: "平方反比衰减+范围钳制+双曲线", upstream: "F1802" },
  { entryId: 1805, title: "聚光", topic: "聚光", role: "内外锥角半影平滑；IES 接口预留", upstream: "F1804" },
  { entryId: 1806, title: "面光源预留", topic: "探针", role: "面光源接口位冻结与诚实标注（一期不实现）", upstream: "F1801" },
  { entryId: 1807, title: "光源管理器", topic: "直接光", role: "句柄表+双因子排序+视锥粗剔+参数块上传", upstream: "F1803,F1804,F1805,F1806" },
  { entryId: 1808, title: "光照探针基础", topic: "探针", role: "SH2 编码+双放置模式+烘焙接口+插值", upstream: "F1802" },
  { entryId: 1809, title: "环境贴图光照 IBL", topic: "直接光", role: "预滤波 mip 链+BRDF LUT；兑现 F1642 预留", upstream: "F1808" },
  { entryId: 1810, title: "光照调试数据", topic: "直接光", role: "线框/热力分解/统计三类负载经 F1764 信封发出", upstream: "F1807" },
  { entryId: 1811, title: "光照性能预算", topic: "直接光", role: "五型成本常数表+三档上限+超预算降档建议", upstream: "F1761" },
  { entryId: 1812, title: "曝光与亮度语义", topic: "直接光", role: "自动/手动双模式曝光；划清 VE-V 边界防双重调光", upstream: "F1809" },
  { entryId: 1813, title: "光照 fuzz", topic: "直接光", role: "光源风暴/非法 SH/极端亮度场景 fuzz", upstream: "F1807" },
  { entryId: 1814, title: "光照基准", topic: "直接光", role: "成本常数定标数据源；并发三档阶梯实测", upstream: "F1811" },
  { entryId: 1815, title: "光照 API 冻结 v1", topic: "直接光", role: "五型光源 API 签名冻结与描述词成册", upstream: "F1807" },
  { entryId: 1816, title: "光照文档", topic: "直接光", role: "总纲三章（光照/阴影/环境）+ 快速上手", upstream: "F1815" },
  { entryId: 1817, title: "光照一致性", topic: "直接光", role: "错误三要素/遥测五元组/调试信封逐项比对", upstream: "F1815" },
  { entryId: 1818, title: "光照安全", topic: "直接光", role: "GPU 显存越界终验+配额红线演练", upstream: "F1809" },
  { entryId: 1819, title: "光照遥测", topic: "直接光", role: "schema 化对齐+总日志中心字段映射+三级查询", upstream: "F1811" },
  { entryId: 1820, title: "J01 组收口与 J02 移交", topic: "阴影贴图", role: "组收口双签+向 J02 移交光照相关段", upstream: "F1802~F1819" },
];

/** 校验分工表：条目数 = 20、条目号 F1801-F1820 连续无重复、每条有主题归属。 */
export function verifyWorkTable(table: readonly WorkTableRow[]): Outcome<WorkTableRow[]> {
  const diagnostics: Diagnostic[] = [];

  // 7.1 重复（最具体的问题先报，便于调用方一眼定位）。
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

  // 7.2 连续性（F1801 起连续 20 条）——先于基数检查：缺位直接指出病因，
  //     「只有 19 条」只是症状。
  const ids = [...seen].sort((a, b) => a - b);
  const expectedFirst = LIGHT_STAGE_SPANS.DIRECT_LIGHT.firstId;
  for (let i = 0; i < ids.length; i++) {
    const expected = expectedFirst + i;
    if (ids[i] !== expected) {
      diagnostics.push({
        code: "WORKTABLE_GAP",
        message: `分工表条目号不连续：期望 F${expected}，实到 F${ids[i]}。`,
        hint: `本组须连续覆盖 F${expectedFirst}-F${LIGHT_STAGE_SPANS.DIRECT_LIGHT.lastId}，缺位即漏项。`,
      });
      break;
    }
  }

  // 7.3 基数（症状级，排在连续性与重复之后）。
  if (table.length !== J_DOMAIN.entriesPerGroup) {
    diagnostics.push({
      code: "WORKTABLE_CARDINALITY_INVALID",
      message: `J01 分工表应为 ${J_DOMAIN.entriesPerGroup} 条，实为 ${table.length} 条。`,
      hint: `按 J 域「每组 ${J_DOMAIN.entriesPerGroup} 条」补齐或修正；本组覆盖 F${expectedFirst}-F${LIGHT_STAGE_SPANS.DIRECT_LIGHT.lastId}。`,
    });
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
 * §8 架构修正 ADR（组实现期回改本条）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 一条架构修正提案（锚点：组实现期回改本条目走 ADR）。 */
export interface ArchitectureAmendment {
  /** 提案条目号（发起修正的下游条目）。 */
  readonly proposedByEntryId: number;
  /** 修正的架构面。 */
  readonly target: "ATTACH_SLOT" | "LIGHT_ABSTRACTION" | "STAGE_SPAN" | "WORK_TABLE";
  /** 修正理由（必填，空即拒绝受理）。 */
  readonly rationale: string;
  /** 影响面（必填，空即拒绝受理）。 */
  readonly impact: string;
}

/**
 * 受理架构修正提案。信息不全即拒绝——防止「顺手改架构」而无据。
 * 受理成功产出 ADR 草案编号，供组实现期回改本条目时留痕。
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
      "ADR 须列明受影响的挂接位/条目与下游消费者，便于回改时同步对齐。",
    );
  }
  const adrId = `ADR-J-${String(amendment.proposedByEntryId).padStart(4, "0")}`;
  return ok({ adrId, accepted: true });
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §9 性能预算分段位（J 段由 F1811 承担）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** J 域成本分段位（锚点：性能预算由 F1811 承担；域级账本在 F1761 基础上扩 J 段）。 */
export const J_COST_SEGMENT_SLOTS: readonly LightStage[] = ["DIRECT_LIGHT", "SHADOW", "ATMOSPHERE"];

/** 校验 J 段成本分段位与三段序一致——F1811 建账时对齐用。 */
export function verifyCostSegments(): Outcome<LightStage[]> {
  const missing = LIGHT_STAGE_ORDER.filter((s) => !J_COST_SEGMENT_SLOTS.includes(s));
  if (missing.length > 0) {
    return err(
      "COST_SEGMENT_SLOT_MISSING",
      `J 段成本分段位缺失：${missing.join("、")}。`,
      "F1811 需按三段序各建一段账；缺段则预算无法覆盖该阶段开销。",
    );
  }
  return ok([...J_COST_SEGMENT_SLOTS]);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §10 架构冻结快照 + 漂移检测
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 架构冻结快照：域开工的机器可读凭据。 */
export interface ArchitectureFreeze {
  readonly domainTag: "VE-J";
  readonly firstEntryId: number;
  readonly lastEntryId: number;
  /** 三段序快照。 */
  readonly stages: readonly LightStage[];
  /** 挂接位快照（名称 + 段 + 是否附加 pass）。 */
  readonly attachSlots: readonly AttachSlot[];
  /** 光源类型快照。 */
  readonly lightTypes: readonly LightType[];
  /** 分工表条目数。 */
  readonly workTableRows: number;
  /** 冻结时间戳（由调用方注入，保持本模块纯函数、无隐式时钟）。 */
  readonly frozenAt: number;
}

/** 快照一致性摘要——用于计算指纹做漂移检测。 */
export function freezeFingerprint(freeze: ArchitectureFreeze): string {
  // 无序语义的部分（光源类型集合）先排序再参与哈希：集合的键序只反映构造
  // 顺序，不反映架构语义。若不排序，同一份架构声明经不同构造路径（字面量 vs
  // Object.keys 推导）会得到不同指纹，漂移检测将大量误报——那是守卫失效，
  // 不是架构真的变了。
  const parts = [
    freeze.domainTag,
    `${freeze.firstEntryId}-${freeze.lastEntryId}`,
    // stages 与 attachSlots 有显式顺序语义，保留原序；lightTypes 是集合，排序。
    freeze.stages.join(","),
    freeze.attachSlots.map((s) => `${s.name}:${s.stage}:${s.additionalPass ? 1 : 0}:${s.order}`).join("|"),
    [...freeze.lightTypes].sort().join(","),
    String(freeze.workTableRows),
  ];
  // FNV-1a 32 位——短、稳定、无依赖，适合做漂移指纹（非安全用途）。
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
 * §11 域开工（编排：前置闸门 → 挂接校验 → 分工表校验 → 成本段校验 → 冻结）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 域开工结果。 */
export interface DomainKickoff {
  readonly freeze: ArchitectureFreeze;
  readonly fingerprint: string;
  /** 开工过程中的非致命诊断（冲突、预留告警等）。 */
  readonly diagnostics: readonly Diagnostic[];
}

/**
 * 宣告 J 域开工。
 *
 * 编排五道关（顺序即依赖顺序，任一硬闸失败则不开工）：
 *   1. 移交包前置检查（F1797 接收确认）——未签即阻断，硬闸；
 *   2. 挂接位校验——冲突不阻断（以 I02 为准可重排），但产出显性冲突清单；
 *   3. 分工表校验——基数/重复/连续性，硬闸；
 *   4. J 段成本分段位校验——与三段序一致，硬闸；
 *   5. 产出架构冻结快照与指纹。
 *
 * @param now 冻结时间戳（调用方注入，本模块不读时钟，保持可测试与可重放）。
 */
export function openDomain(pkg: HandoverPackage, now: number): Outcome<DomainKickoff> {
  // 11.1 前置闸门。
  const pre = verifyKickoffPreconditions(pkg);
  if (!pre.ok) return pre;

  // 11.2 挂接位校验（软闸：冲突可重排，不阻断开工）。
  const attach = verifyAttachSlots(J_PIPELINE_ATTACH_SLOTS);
  if (!attach.ok) return attach;

  // 11.3 分工表校验（硬闸）。
  const wt = verifyWorkTable(J01_WORK_TABLE);
  if (!wt.ok) return wt;

  // 11.4 成本分段位校验（硬闸）。
  const cost = verifyCostSegments();
  if (!cost.ok) return cost;

  // 11.5 冻结。
  const freeze: ArchitectureFreeze = {
    domainTag: J_DOMAIN.tag,
    firstEntryId: J_DOMAIN.firstEntryId,
    lastEntryId: J_DOMAIN.lastEntryId,
    stages: [...LIGHT_STAGE_ORDER],
    attachSlots: J_PIPELINE_ATTACH_SLOTS.map((s) => ({ ...s })),
    lightTypes: Object.keys(LIGHT_TYPE_OWNER) as LightType[],
    workTableRows: wt.value.length,
    frozenAt: now,
  };

  const mergedDiagnostics = [...pre.diagnostics, ...attach.diagnostics];
  return ok({ freeze, fingerprint: freezeFingerprint(freeze), diagnostics: mergedDiagnostics }, mergedDiagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §12 域开工宣告文本（架构文档替述可读，无障碍要求）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 生成 J 域开工宣告（人话版）。
 * 无障碍要求：架构文档替述可读——本函数是文档的可执行生成源，
 * 保证「文档所述」与「代码所声明」同源，不会各说各话。
 */
export function renderKickoffDeclaration(freeze: ArchitectureFreeze, fingerprint: string): string {
  const slots = freeze.attachSlots
    .map((s) => `  · ${s.name}（${s.stage}${s.additionalPass ? " · 附加深度 pass" : ""}）`)
    .join("\n");
  return [
    `【VE-J 光照与阴影域 · 开工宣告】`,
    `条目区间：F${freeze.firstEntryId}-F${freeze.lastEntryId}（共 ${J_DOMAIN.entryCount} 条 / ${J_DOMAIN.groupCount} 组）。`,
    `官方主题（十项）：${J_DOMAIN_TEN_TOPICS.join("、")}。`,
    `光照三段式管线序：${freeze.stages.join(" → ")}。`,
    `与 I 域绘制管线的挂接位：`,
    slots,
    `挂接冲突一律以 I02 冻结 pass 序为准，调整 J 域设计；禁止反向改动 I 域。`,
    `光源抽象：类型枚举 ${freeze.lightTypes.length} 型 + 公共参数块 + 逐类型扩展块；面光源族为预留未实现。`,
    `J01 组分工表：${freeze.workTableRows} 条（F${LIGHT_STAGE_SPANS.DIRECT_LIGHT.firstId}-F${LIGHT_STAGE_SPANS.DIRECT_LIGHT.lastId}）。`,
    `架构冻结指纹：${fingerprint}（冻结时间戳 ${freeze.frozenAt}）。`,
  ].join("\n");
}
