/**
 * VE-F2401 · M 域开工与动画架构（M 域 · 动画系统域开工条 · 批次 M01 首项）
 * ---------------------------------------------------------------------------
 * 职责定位：VE-M 域（F2401~F2600）的**开工条**。它不求值一条轨道、不采样一帧
 * clip、不混合一次姿态、不做一次蒙皮，只做四件事，并把四件事写成可机检的契约：
 *   1. 宣告动画系统域正式开工——官方十主题与十个批次的完整排布；
 *   2. 冻结域级求值架构——四段管线（轨道求值 → clip 采样 → 姿态混合 → 骨骼应用）
 *      的执行序，以及三域（M/I/L）边界声明表；
 *   3. 接收 F2400 移交包——L 域序契约与轨道契约的哈希对账签署（K→L→M 三域契约链）；
 *   4. 给出组内 20 条分工总览——M01 批次 F2401~F2420 逐条落位。
 *
 * 为什么 M 域的开工条必须先把「轨道单源」与「执行序」钉死（域级立场，全域根基）：
 *   M 域是全引擎唯一一个**同一份轨道数据被三个域同时读**的域：
 *     · 剪辑域 F1345 是轨道系统的**单源**（关键帧轨道结构定义在 F1345）；
 *     · I 域 F1666 动画采样接口把采样率/插值器/时间基准**冻结为跨域契约**；
 *     · L 域 F2324 物理与动画同步要求帧内序（动画更新 → 事件检查 → 物理触发）。
 *   这三条若不在开工时对齐，会出现一类极难归因的缺陷：**同一帧里，
 *   剪辑域看到的轨道值、I 域采到的姿态、L 域算出的物理触发，三者互相差一帧。**
 *   到 M05 才吵这件事时，M01~M04 已经各自把「帧内序」写进了自己的实现里，
 *   返工面是四个批次而不是一个条目。故本条把「四段执行序」与「三域边界」
 *   做成**架构守卫**：任何模块若在本域内做蒙皮（F2401 错误路径第一条），
 *   或让动画与物理互相驱动成环（F1841 家族环检测），在开工期就被拦下。
 *
 * 四条锚点契约（逐条对应判据）：
 *   1. 域开工 —— 官方十主题（关键帧/缓动库/骨骼动画/形态键/路径动画/物理动效/
 *      状态机动画/混合树/动画层/事件系统）逐项登记：主题 id、主题名、归属批次、
 *      职责边界、产出契约名。十主题与十批次**不是一一对应**（M02 同时承载缓动库与
 *      骨骼动画全族，M09/M10 是跨主题的域治理批次），本条显式声明这层多对多
 *      关系而非假装双射。
 *   2. 四段管线 —— 轨道求值 → clip 采样 → 姿态混合 → 骨骼应用，序冻结。逐段登记：
 *      段 id、段职责、输入/输出契约名、所属域（第四段属I 域蒙皮，见边界表）、
 *      本段实现条目（由 M01~M03 逐段兑现）、本段性能定标来源（F2407/F2430）。
 *   3. 三域边界 —— M 管轨道/采样/混合/状态机；I 管蒙皮与骨骼变换应用；
 *      L 管物理模拟。逐能力登记归属域，越界即诊断（动画做蒙皮 → 以边界表为准回改）。
 *      与 L 域的物理动效为**双向衔接**（物理驱动动画 / 动画驱动物理），
 *      引用 F2144（M 域联动契约）与 F2324（物理与动画同步）两件契约。
 *   4. 契约接收 —— F2400 移交包（K→L→M 契约链的第三环）：上游收官检查、
 *      契约 id 一致、移交件哈希回放、义务逐条确认。任一不成立即开工阻断。
 *
 * 零静默纪律：边界越界、双向驱动成环、序错乱、契约未签、哈希失真、上游未收官、
 * 主题覆盖缺口、功能号区间重叠/空隙、分工不齐——全部产出 Diagnostic
 * （code + message + hint）并由调用方聚合上报。本模块不抛异常、不吞诊断、
 * 无静默分支、无降级到「假装通过」的分支。
 *
 * 判据：域开工、四段管线、三域边界、契约接收、判据。
 * 交接说明：本条是纯契约层——零 GPU 调用、零 DOM 依赖、零全局可变状态、
 *         顶层只有常量表与纯函数，可在任意宿主（浏览器 / Worker / Node 校验脚本）
 *         中原样引入。下游 M01（F2402 关键帧轨道系统 起）逐项消费本条的
 *         THEME_REGISTRY、PIPELINE_SEQUENCE、BOUNDARY_TABLE 与
 *         L_TO_M_HANDOVER_CONTRACT。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型（零静默的基础设施：与 I/J/K/L 域同纪律，此处独立实现不跨域 import）
// ════════════════════════════════════════════════════════════════════════════

/** 诊断码：每种拒绝/越界/成环/阻断独立可检索，绝不合并成一条通用错误。 */
export type DiagCode =
  /** 主题未在十主题注册表中登记（引用了不存在的主题）。 */
  | "THEME_UNREGISTERED"
  /** 主题已登记但字段自相矛盾（批次不存在 / 契约名为空 / 治理主题却带契约名）。 */
  | "THEME_SPEC_INCONSISTENT"
  /** 主题覆盖缺口：某个官方主题未被任何实现批次承载。 */
  | "THEME_COVERAGE_GAP"
  /** 批次未登记。 */
  | "BATCH_UNREGISTERED"
  /** 批次与主题的映射自相矛盾（批次主题表与主题归属表互不一致）。 */
  | "BATCH_THEME_MAPPING_INCONSISTENT"
  /** 功能号区间重叠（两个批次抢同一批功能号）。 */
  | "ITEM_RANGE_OVERLAP"
  /** 功能号区间缺口（相邻批次之间存在未分配的功能号）。 */
  | "ITEM_RANGE_GAP"
  /** 管线段未登记。 */
  | "PIPELINE_STAGE_UNKNOWN"
  /** 管线段规格自相矛盾（输入/输出契约为空、所属域与边界表冲突）。 */
  | "PIPELINE_STAGE_SPEC_INCONSISTENT"
  /** 管线执行序被破坏（提交的执行序不是冻结的四段序）。 */
  | "PIPELINE_SEQUENCE_BROKEN"
  /** 边界能力未登记（引用了边界表之外的能力）。 */
  | "BOUNDARY_CAPABILITY_UNREGISTERED"
  /** 边界越界：某域做了不归它管的能力（M 域做蒙皮是典型）。 */
  | "BOUNDARY_VIOLATION"
  /** 双向驱动成环：动画驱动物理与物理驱动动画在同一条链上互相驱动（环检测命中）。 */
  | "DRIVE_CYCLE_DETECTED"
  /** 帧内序错乱（次序错误即用错前帧状态——F2324 帧内序红线）。 */
  | "FRAME_ORDER_VIOLATION"
  /** 移交包未接收（契约件缺件或确认位未签）。 */
  | "HANDOVER_UNSIGNED"
  /** 移交包义务缺失（接收方未逐条确认遵守义务）。 */
  | "HANDOVER_OBLIGATION_MISSING"
  /** 移交件哈希失真（回放哈希与 L 域移交宣告值不符）。 */
  | "HANDOVER_HASH_MISMATCH"
  /** 上游 L 域未收官（F2400 宣告未生效），开工前置不成立。 */
  | "UPSTREAM_NOT_CLOSED"
  /** K 域未收官（契约链首环未冻结，M 域不得越过 L 域直接依赖 K 链语义）。 */
  | "CONTRACT_CHAIN_HEAD_UNCLOSED"
  /** 组内 20 条分工不齐（缺条 / 重复条目号 / 契约名重复）。 */
  | "WORKBREAKDOWN_INCOMPLETE"
  /** 开工被阻断（点名到具体缺口）。 */
  | "KICKOFF_BLOCKED";

/** 一条诊断：发生了什么（人话）、影响什么、下一步怎么办（可操作提示）。 */
export interface Diagnostic {
  readonly code: DiagCode;
  /** 人话描述：面向开发者排障，不含裸异常码、不含「可能」「也许」。 */
  readonly message: string;
  /** 可操作提示：调用方该改哪里、该怎么降级、找哪个域协商。 */
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

/** 成功构造（diagnostics 允许携带非致命告警，例如降级建议）。 */
export function ok<T>(value: T, diagnostics: readonly Diagnostic[] = []): Outcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造：三要素齐备，diagnostics 含本条自身便于统一上报。 */
export function fail<T>(code: DiagCode, message: string, hint: string): Outcome<T> {
  const d: Diagnostic = { code, message, hint };
  return { ok: false, code, message, hint, diagnostics: [d] };
}

/** 诊断袋：累积一条路径上的全部诊断（而不是遇到第一条就返回）。 */
export class DiagBag {
  private readonly items: Diagnostic[] = [];

  /** 追加一条诊断；空 message/hint 被规范化，避免上游写出半截诊断。 */
  push(code: DiagCode, message: string, hint: string): void {
    this.items.push({
      code,
      message: message || "（未提供描述）",
      hint: hint || "（未提供处置建议）",
    });
  }

  /** 追加一批已构造的诊断（用于把子调用的 Outcome.diagnostics 平铺进来）。 */
  pushAll(ds: readonly Diagnostic[]): void {
    for (const d of ds) this.items.push(d);
  }

  /** 当前条数。 */
  get size(): number {
    return this.items.length;
  }

  /** 只读视图（返回副本，调用方改不动内部清单）。 */
  all(): readonly Diagnostic[] {
    return this.items.slice();
  }

  /** 按诊断码筛选——排障时按码聚合的入口。 */
  byCode(code: DiagCode): readonly Diagnostic[] {
    return this.items.filter((d) => d.code === code);
  }

  /** 是否存在任何诊断（当前全部码皆为 error 级，故等价于非空判定）。 */
  get hasAny(): boolean {
    return this.items.length > 0;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 域身份 + 官方十主题 + 十批次排布（判据一：域开工）
// ════════════════════════════════════════════════════════════════════════════

/** 域身份常量（本域所有对外文本引用同一事实源，不各自硬编码）。 */
export const DOMAIN = {
  /** 域编号。 */
  id: "VE-M",
  /** 域名。 */
  name: "动画系统",
  /** 功能号区间。 */
  itemLo: 2401,
  itemHi: 2600,
  /** 本域功能总数（2600 - 2401 + 1 = 200，与册内「十域 × 200 项」一致）。 */
  itemCount: 200,
  /** 所属波次（VE 卷波次表：W5 = 动画与 UI 框架：骨骼/时间轴/控件树/CSS 表面）。 */
  wave: "W5",
  /** 本域开工条的功能号。 */
  kickoffItemId: 2401,
} as const;

/** 官方十主题的中文名序列（判据一的对外表述，顺序即册内顺序）。 */
export const OFFICIAL_THEMES: readonly string[] = [
  "关键帧",
  "缓动库",
  "骨骼动画",
  "形态键",
  "路径动画",
  "物理动效",
  "状态机动画",
  "混合树",
  "动画层",
  "事件系统",
];

/** 十个官方主题 id（封闭集：新增主题须改本类型与 THEME_REGISTRY 两处）。 */
export type ThemeId =
  | "keyframe"
  | "easing-library"
  | "skeletal-animation"
  | "morph-target"
  | "path-animation"
  | "physics-driven-motion"
  | "state-machine"
  | "blend-tree"
  | "animation-layer"
  | "event-system";

/** 一个主题的完整规格（十主题逐项声明，无一项靠约定）。 */
export interface ThemeSpec {
  readonly id: ThemeId;
  /** 主题中文名（对外文档与错误提示引用）。 */
  readonly name: string;
  /**
   * 承载该主题的批次列表（十主题与十批次是多对多，不是双射——见函数注释）。
   * 顺序即依赖序：M02 既承载缓动库也承载骨骼动画时，插值语义须在采样之前就位。
   */
  readonly batches: readonly string[];
  /** 职责边界一句话：这一主题负责什么、不负责什么（禁扩面写在句内）。 */
  readonly duty: string;
  /** 本主题产出、供下游消费的数据契约名（契约名全局唯一，由自检机检）。 */
  readonly producesContract: string;
  /** 一句话说明：本主题与哪个域交接，交接物是什么。 */
  readonly handoff: string;
  /**
   * 是否为跨主题治理类（M09 预备与自查 / M10 收口覆盖全部主题）。
   * 治理类主题不产数据契约，故 producesContract 为空字符串。
   */
  readonly isGovernance: boolean;
}

/**
 * 十主题注册表。判据一要求逐主题声明归属批次/职责/契约，此处即为该声明的
 * 唯一事实源——下游任何代码不得旁路硬编码主题名，一律经 lookupTheme 取规格。
 *
 * 为什么主题与批次是多对多而不是一一对应（显式记录，避免后来者「修正」成双射）：
 *   官方十主题是**能力分类轴**，十个批次是**排产轴**（每批 20 项，便于双签与
 *   组级收口）。二者粒度本就不同：缓动库（F2421）与骨骼动画（F2422~F2440）
 *   共享同一批求值器与同一套 slerp 语义——拆两批会出现两套互相不知情的
 *   插值实现，而插值语义分叉是最难归因的一类缺陷（两边各自单测都过，
 *   合起来画面就是抖的）；混合树（F2426）与状态机（F2427）更必须同批：
 *   状态机的每个状态内嵌一棵混合树，跨批会迫使状态机通过未冻结的中间接口
 *   调用混合树。M09/M10 则不对应任何单一能力主题，它们是覆盖全部主题的
 *   域治理批次。故此处登记多对多，并在 auditThemeCoverage 里机检
 *   「每个能力主题至少被一个实现批次承载」。
 */
export const THEME_REGISTRY: Readonly<Record<ThemeId, ThemeSpec>> = {
  keyframe: {
    id: "keyframe",
    name: "关键帧",
    batches: ["M01"],
    duty: "六类轨道（位置/旋转/缩放/颜色/浮点/布尔）+ 轨道容器 + 绑定协议 + 插值器；不负责蒙皮应用与物理求解",
    producesContract: "TrackSet",
    handoff: "向 M02 交付 clip 数据模型与求值接口（F2402/F2407）；向 K 域交付轨道跨域引用（F2144 单向 M→K 驱动）",
    isGovernance: false,
  },
  "easing-library": {
    id: "easing-library",
    name: "缓动库",
    batches: ["M02"],
    duty: "30+ 缓动函数全集与 In/Out/InOut 变体系统 + 自定义缓动注册；不负责关键帧数据的存储",
    producesContract: "EasingLibrary",
    handoff: "向 F2403 插值器注册机制交付缓动函数枚举；向 P 域（F3001+ 动效库）交付统一缓动词汇单源",
    isGovernance: false,
  },
  "skeletal-animation": {
    id: "skeletal-animation",
    name: "骨骼动画",
    batches: ["M02"],
    duty: "clip 模型/骨骼映射/四元数 slerp 采样/姿态混合三模式/骨骼掩码/压缩量化；只产出骨骼姿态，不做蒙皮",
    producesContract: "BonePoseBuffer",
    handoff: "向 I 域交付姿态缓冲（M→I04 姿态缓冲契约，F2422 冻结）；消费 F1666 采样契约",
    isGovernance: false,
  },
  "morph-target": {
    id: "morph-target",
    name: "形态键",
    batches: ["M03"],
    duty: "形态键权重轨道、表情混合与顶点缓存失效联动；不负责基础网格拓扑（F1601 I01 管）",
    producesContract: "MorphWeightSet",
    handoff: "消费 F2409 glTF morph weights 通道映射；向 I 域交付形变后顶点缓存键",
    isGovernance: false,
  },
  "path-animation": {
    id: "path-animation",
    name: "路径动画",
    batches: ["M03"],
    duty: "沿路径的位置/朝向驱动、路径插值与跟随偏移；不负责路径资产生成（归 VE-Q 资产管线）",
    producesContract: "PathFollowPose",
    handoff: "消费 F2402 位置与旋转轨道；向物理动效交付运动学目标位",
    isGovernance: false,
  },
  "physics-driven-motion": {
    id: "physics-driven-motion",
    name: "物理动效",
    batches: ["M03"],
    duty: "弹簧动效（UI 级，无碰撞无重力）与物理-动画双向衔接的映射协议；场景刚体归 L 域物理求解",
    producesContract: "SpringMotionState",
    handoff: "消费 L 域 F2324 物理事件（反向桥接）；向 L 域交付动画驱动物理的目标位（F2324 同帧映射）",
    isGovernance: false,
  },
  "state-machine": {
    id: "state-machine",
    name: "状态机动画",
    batches: ["M02"],
    duty: "状态集合（状态内嵌混合树）/迁移条件/迁移过渡曲线/迁移打断；不负责条件表达式求值的通用脚本层（归 VE-Z）",
    producesContract: "StateMachineRuntime",
    handoff: "消费 M02 混合树（F2426）作为状态内嵌体；向动画层（F2428）交付状态覆盖优先级",
    isGovernance: false,
  },
  "blend-tree": {
    id: "blend-tree",
    name: "混合树",
    batches: ["M02"],
    duty: "一维/二维参数化混合、参数驱动与混合树求值；不做骨骼级局部掩码之外的姿态语义决策",
    producesContract: "BlendTreeRuntime",
    handoff: "消费姿态混合三模式（F2424）与骨骼掩码；向状态机交付可内嵌的混合树单元",
    isGovernance: false,
  },
  "animation-layer": {
    id: "animation-layer",
    name: "动画层",
    batches: ["M02"],
    duty: "多层叠加（基础层/上半身层/覆盖层）、层权重与层间混合；不做状态机迁移决策（归状态机主题）",
    producesContract: "LayerStack",
    handoff: "消费状态机覆盖优先级；向 P 域交付 UI 动效分层编排的统一词汇",
    isGovernance: false,
  },
  "event-system": {
    id: "event-system",
    name: "事件系统",
    batches: ["M01", "M03"],
    duty: "第七类离散事件轨、触发语义（正播触发/倒播默认不触发）与事件去重合并；只做动画侧事件生产，不做总线实现",
    producesContract: "AnimEventTrack",
    handoff: "向 H 域 F1408 事件总线交付动画事件（第三生产者：J 光照 / H 音频 / M 动画）；向 L 域 F2324 交付事件对映射",
    isGovernance: false,
  },
};

/** 主题 id 列表（顺序即注册顺序，供对账遍历用）。 */
export const THEME_IDS: readonly ThemeId[] = Object.keys(THEME_REGISTRY) as ThemeId[];

/** 按 id 取主题规格。未登记即显性失败并给出可操作提示（绝不返回 undefined 让调用方猜）。 */
export function lookupTheme(id: string): Outcome<ThemeSpec> {
  const spec = THEME_REGISTRY[id as ThemeId];
  if (spec === undefined) {
    return fail(
      "THEME_UNREGISTERED",
      `主题「${id}」不在 M 域官方十主题注册表中`,
      `M 域官方十主题为：${THEME_IDS.join("、")}。`
        + `若确需新增主题，须先改 ThemeId 封闭集与 THEME_REGISTRY 两处（两处不一致会被自检拦下）`,
    );
  }
  return ok(spec);
}

/** 批次规格：一条 = 一个 20 项排产单元。 */
export interface BatchSpec {
  readonly id: string;
  /** 批次中文名。 */
  readonly name: string;
  /** 本批次功能号区间（闭区间，含两端）。 */
  readonly itemRange: { readonly lo: number; readonly hi: number };
  /** 本批次承载的主题 id（与 THEME_REGISTRY 的 batches 字段互为对账面）。 */
  readonly themes: readonly ThemeId[];
  /** 批次职责一句话（禁扩面写在句内）。 */
  readonly duty: string;
  /** 本批次组级关口条目号（每组最后一个条目是收口与移交）。 */
  readonly gateItemId: number;
}

/**
 * 十批次排布（F2401~F2600，每批 20 项，共 200 项）。
 *
 * 为什么 M 域的批次边界落在「能力族」而不是「能力项」上：
 *   M01 收关键帧族（轨道/插值/批量/资产/事件轨/导入导出/性能），
 *   M02 收骨骼族（缓动/骨骼/采样/混合/混合树/状态机/层），
 *   M03 收形变与物理族（形态键/路径/物理动效），
 *   M04~M08 是横切面（UI 集成/性能质量/调试生态/无障碍/跨域联动），
 *   M09/M10 是域治理（预备自查/收口）。横切面单独成批的依据是：
 *   它们的验收对象是「跨条目的性质」（一致性、无障碍、跨域联动），
 *   拆进能力批会让每批各自定义一份一致性判据，最终十份互不相认的判据。
 */
export const BATCH_REGISTRY: readonly BatchSpec[] = [
  {
    id: "M01",
    name: "动画系统架构与关键帧组",
    itemRange: { lo: 2401, hi: 2420 },
    themes: ["keyframe", "event-system"],
    duty: "域开工 + 四段架构 + 轨道/插值/批量/资产/事件轨/导入导出/性能/一致性；不负责骨骼与缓动",
    gateItemId: 2420,
  },
  {
    id: "M02",
    name: "缓动库与骨骼动画组",
    itemRange: { lo: 2421, hi: 2440 },
    themes: ["easing-library", "skeletal-animation", "blend-tree", "state-machine", "animation-layer"],
    duty: "缓动全集 + clip 模型 + slerp 采样 + 姿态混合 + 混合树 + 状态机 + 动画层；不做形态键与路径",
    gateItemId: 2440,
  },
  {
    id: "M03",
    name: "形态键与路径动画组",
    itemRange: { lo: 2441, hi: 2460 },
    themes: ["morph-target", "path-animation", "physics-driven-motion", "event-system"],
    duty: "形态键 + 路径动画 + 物理动效与事件深度；不做骨骼采样（消费 M02）",
    gateItemId: 2460,
  },
  {
    id: "M04",
    name: "动画与 UI/系统集成组",
    itemRange: { lo: 2461, hi: 2480 },
    themes: [],
    duty: "UI 动画对接 + 系统动画驱动协议 + CGPU/L 域联动核验 + 回放录制 + 资产版本化；不改四段执行序",
    gateItemId: 2480,
  },
  {
    id: "M05",
    name: "动画性能与质量组",
    itemRange: { lo: 2481, hi: 2500 },
    themes: [],
    duty: "性能账本 + compute 深化 + 内存治理 + 确定性总验 + 长稳总测 + 回归门禁 + fuzz 总闸 + 遥测汇总",
    gateItemId: 2500,
  },
  {
    id: "M06",
    name: "动画调试与生态组",
    itemRange: { lo: 2501, hi: 2520 },
    themes: [],
    duty: "调试数据总线 + 资产开放格式（m.anim. 段）+ 工具链对接 + 生态注册；不新增轨道语义",
    gateItemId: 2520,
  },
  {
    id: "M07",
    name: "动画无障碍与包容组",
    itemRange: { lo: 2521, hi: 2540 },
    themes: [],
    duty: "减少运动档位 + 动画可读性 + 替代通道 + 术语通俗化；不改求值语义",
    gateItemId: 2540,
  },
  {
    id: "M08",
    name: "动画跨域联动组",
    itemRange: { lo: 2541, hi: 2560 },
    themes: [],
    duty: "与 I 域蒙皮契约终验 + 与 L 域物理同步终验 + 与 K/Z/V 域联动核验；只做核验不改对端语义",
    gateItemId: 2560,
  },
  {
    id: "M09",
    name: "M 域预备与自查组",
    itemRange: { lo: 2561, hi: 2580 },
    themes: [],
    duty: "十组接口收口核验 + 域级自查汇总 + 缺失补齐；产出预备报告（GO 判据）",
    gateItemId: 2580,
  },
  {
    id: "M10",
    name: "M 域收口组 · VE-M 域 200 项收官",
    itemRange: { lo: 2581, hi: 2600 },
    themes: [],
    duty: "200 项对账宣告 + 十组双签汇总 + 硬门核验 + 20 维度验收 + 移交归档；宣告后域转只读",
    gateItemId: 2600,
  },
];

/** 按 id 取批次规格。未登记即显性失败。 */
export function lookupBatch(id: string): Outcome<BatchSpec> {
  const b = BATCH_REGISTRY.find((x) => x.id === id);
  if (b === undefined) {
    return fail(
      "BATCH_UNREGISTERED",
      `批次「${id}」不在 M 域十批次排布表中`,
      `M 域十批次为：${BATCH_REGISTRY.map((x) => x.id).join("、")}`,
    );
  }
  return ok(b);
}

/**
 * 主题覆盖审计：逐项机检三件事。
 *   ① 每个主题规格自身自洽（批次存在、职责非空、治理主题不带契约名、
 *      能力主题必带契约名）；
 *   ② 每个能力主题至少被一个**实现批次**承载（治理批次不承载能力主题）；
 *   ③ 批次与主题的映射双向自洽（主题声明的 batches 与批次声明的 themes 互为镜像）。
 *
 * 为什么要双向核对而不是只查一个方向（显式记录，这是 L 域同类审计的继承）：
 *   只查「主题→批次」会漏掉「批次登记了一个主题，但该主题没把本批次列进去」
 *   这类半登记；只查「批次→主题」则漏掉「主题说自己在 M02，批次说不带它」。
 *   两种半登记都会让自检看起来全绿，而真实覆盖关系已经断了——到 M09
 *   做九组核验时才发现，就会变成「某主题没有任何批次承载」的收口阻断。
 */
export function auditThemeCoverage(bag: DiagBag): Outcome<readonly ThemeId[]> {
  const covered: ThemeId[] = [];
  const implementationBatchIds = new Set(
    BATCH_REGISTRY.filter((b) => b.themes.length > 0).map((b) => b.id),
  );

  for (const id of THEME_IDS) {
    const t = THEME_REGISTRY[id];

    // ① 主题规格自洽。
    for (const b of t.batches) {
      if (lookupBatch(b).ok !== true) {
        bag.push(
          "THEME_SPEC_INCONSISTENT",
          `主题「${t.name}」声明归属批次 ${b}，但该批次未在十批次表中登记`,
          `修正 THEME_REGISTRY 中该主题的 batches 字段；批次 id 必须与 BATCH_REGISTRY 的 id 逐字一致`,
        );
      }
    }
    if (t.duty.trim().length === 0) {
      bag.push(
        "THEME_SPEC_INCONSISTENT",
        `主题「${t.name}」的职责边界为空`,
        "职责边界须写明「负责什么、不负责什么」（禁扩面），空字符串会让该主题成为无主能力",
      );
    }
    if (!t.isGovernance && t.producesContract.trim().length === 0) {
      bag.push(
        "THEME_SPEC_INCONSISTENT",
        `能力主题「${t.name}」未声明产出契约名`,
        "能力主题必须声明 producesContract——下游条目靠契约名对齐，缺名即无法对接",
      );
    }
    if (t.isGovernance && t.producesContract.trim().length > 0) {
      bag.push(
        "THEME_SPEC_INCONSISTENT",
        `治理类主题「${t.name}」不应声明产出契约名（当前为 ${t.producesContract}）`,
        "治理批次不产出数据契约。给它挂契约名会让下游误以为存在可消费的数据接口",
      );
    }

    // ② 能力主题须被至少一个实现批次承载。
    const hitImpl = t.batches.some((b) => implementationBatchIds.has(b));
    if (!t.isGovernance && !hitImpl) {
      bag.push(
        "THEME_COVERAGE_GAP",
        `能力主题「${t.name}」没有任何实现批次承载（其 batches 为：${t.batches.join("、") || "空"}）`,
        "每个官方能力主题至少要落在一个实现批次上。若该主题本期不做，须在册内显式登记为"
          + "接口预留并标isGovernance/预留态，而不是让它静默悬空",
      );
    } else if (hitImpl) {
      covered.push(id);
    }
  }

  // ③ 双向镜像核对。
  for (const b of BATCH_REGISTRY) {
    for (const tid of b.themes) {
      const t = THEME_REGISTRY[tid];
      if (t === undefined) {
        bag.push(
          "BATCH_THEME_MAPPING_INCONSISTENT",
          `批次 ${b.id} 声明承载主题 ${tid}，但该主题未在十主题表中登记`,
          "修正 BATCH_REGISTRY 中该批次的 themes 字段，或先补齐 THEME_REGISTRY 中的主题",
        );
        continue;
      }
      if (!t.batches.includes(b.id)) {
        bag.push(
          "BATCH_THEME_MAPPING_INCONSISTENT",
          `批次 ${b.id} 声明承载主题「${t.name}」，但该主题的 batches 未反向登记 ${b.id}`,
          "两处必须互为镜像：主题的 batches 与批次的 themes 是同一份覆盖关系的两个方向，"
            + "只改一处会让自检在半登记状态下全绿",
        );
      }
    }
  }

  if (bag.hasAny) {
    const first = bag.all()[0];
    return fail(
      first?.code ?? "THEME_COVERAGE_GAP",
      first?.message ?? "主题覆盖审计未通过",
      first?.hint ?? "按上述诊断逐条修正",
    );
  }
  return ok(covered, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §3 四段求值管线（判据二：域级架构声明 · M 域执行序冻结）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 四段管线是什么、为什么必须在开工条上就冻结（域级立场，全域根基）：
 *   一帧动画从「时间 t」到「骨骼最终矩阵」，中间必须依次经过四段：
 *     ① 轨道求值（track evaluation）  ——轨道 + 插值器 → 每条轨道的标量/向量值
 *     ② clip 采样（clip sampling）    —— 多个 clip 的轨道值 → 单一姿态
 *     ③ 姿态混合（pose blending）      —— 多姿态 + 权重 → 单一姿态
 *     ④ 骨骼应用（bone application）  —— 姿态 → 骨骼矩阵 → 蒙皮（**归 I 域**）
 *   这四段的**顺序不可交换**，原因是它们各自消费的是上一段的**输出契约**：
 *     · 交换 ①② 会得到「先混合再采样」，权重语义从「姿态权重」退化为「轨道权重」，
 *       同一 clip 在不同权重组合下会产生不同的插值结果——动画不再是所见即所得；
 *     · 交换 ②③ 会让 clip 采样拿到「混合后的轨道值」，而混合的输入是姿态，
 *       两者的数据结构不同（轨道值是标量/向量，姿态是骨骼树），强行交换的结果
 *       是在混合阶段重新遍历骨骼树，复杂度从 O(轨道数) 退化为 O(骨骼×轨道)；
 *     · 交换 ③④ 会让蒙皮先于姿态完成——蒙皮拿到的是上一帧姿态，画面整体延后一帧。
 *   而「延后一帧」恰是本域最隐蔽的缺陷类型：单帧截图看不出问题，只有运动中的
 *   画面才显形，且症状（画面「拖」）会被误判为插值器问题或物理步进问题。
 *   故本条把执行序做成**冻结常量 + 序断言**：任何提交的执行序不是这四段，
 *   即便四段都在、都跑对了，也判为序破坏（PIPELINE_SEQUENCE_BROKEN）。
 */
export const PIPELINE_SEQ_NOTE =
  "四段序是 M 域的执行序冻结项：段间传递的是输出契约（轨道值→姿态→蒙皮矩阵），"
  + "交换任意相邻两段都会改变某一层的语义或复杂度。故序断言不只检查「段是否都在」，"
  + "还检查「序是否等于冻结序」——四段齐备但顺序错乱，比缺一段更难归因。";

/** 管线段 id（封闭集：新增段须改本类型与 PIPELINE_STAGES 两处）。 */
export type PipelineStageId =
  /** ① 轨道求值。 */
  | "track-eval"
  /** ② clip 采样。 */
  | "clip-sample"
  /** ③ 姿态混合。 */
  | "pose-blend"
  /** ④ 骨骼应用（蒙皮本体归 I 域，M 域只交付姿态缓冲）。 */
  | "bone-apply";

/** 管线一段的完整规格（四段逐段声明，无一段靠约定）。 */
export interface PipelineStageSpec {
  readonly id: PipelineStageId;
  /** 段中文名（文档与错误提示引用同一事实源）。 */
  readonly name: string;
  /** 段的执行序位（1 起，四段恰为 1~4，序断言依此判定）。 */
  readonly order: 1 | 2 | 3 | 4;
  /**
   * 本段的执行归属域。①②③ 归 M 域；④ 的**蒙皮本体归 I 域**
   * （I04 F1666 采样契约的兑现方），M 域在④ 只负责产出姿态缓冲。
   * 这一格是三域边界表（§4）在管线上的投影。
   */
  readonly ownerDomain: "VE-M" | "VE-I";
  /** 本段消费的输入契约名。 */
  readonly inputContract: string;
  /** 本段产出的输出契约名（下一段的输入契约名，须与下一段逐字一致）。 */
  readonly outputContract: string;
  /** 段职责一句话（禁扩面写在句内）。 */
  readonly duty: string;
  /** 兑现本段的条目号（由 M01~M03 逐段实现；④ 由 I 域兑现）。 */
  readonly realizedBy: string;
  /** 本段成本的定标来源（架构声明零运行时，成本由基准条目定标）。 */
  readonly costCalibratedBy: string;
}

/**
 * 四段管线规格表（判据二的唯一事实源）。
 *
 * 关于「④ 归 I 域」的诚实说明（不是笔误，也不是妥协）：
 *   蒙皮（skinning）是把骨骼矩阵作用到网格顶点上的几何运算，它的实现依赖
 *   I 域的顶点流水线与 GPU 顶点格式（F1601 I01/I02）。若 M 域自己实现蒙皮，
 *   就会出现两套蒙皮：一套在 M 域按动画的需要写（可能为省开销而简化权重数），
 *   一套在 I 域按渲染的需要写。两者在同一帧里对同一批顶点给出不同结果时，
 *   现象是「同一个模型在某些动画下表面撕裂」，归因要跨越两个域。
 *   故本条明确：M 域产出姿态缓冲（F2422 冻结布局），I 域消费它做蒙皮。
 *   M 域不得越过姿态缓冲直接改顶点——越界即 BOUNDARY_VIOLATION。
 */
export const PIPELINE_STAGES: Readonly<Record<PipelineStageId, PipelineStageSpec>> = {
  "track-eval": {
    id: "track-eval",
    name: "轨道求值",
    order: 1,
    ownerDomain: "VE-M",
    inputContract: "TrackSet",
    outputContract: "TrackValueSet",
    duty: "轨道关键帧 + 插值器 + 时间 → 每轨标量/向量值；不做姿态组装、不做混合",
    realizedBy: "VE-F2402 轨道系统 / VE-F2403 插值 / VE-F2407 求值性能（SIMD 与零分配）",
    costCalibratedBy: "VE-F2412 动画基准（求值吞吐族）",
  },
  "clip-sample": {
    id: "clip-sample",
    name: "clip 采样",
    order: 2,
    ownerDomain: "VE-M",
    inputContract: "TrackValueSet",
    outputContract: "BonePoseBuffer",
    duty: "多 clip 的轨道值按骨骼通道组装为姿态；旋转走四元数 slerp（F2423）",
    realizedBy: "VE-F2422 clip 模型 / VE-F2423 骨骼采样（slerp 专项）",
    costCalibratedBy: "VE-F2432 骨骼动画基准（采样族）",
  },
  "pose-blend": {
    id: "pose-blend",
    name: "姿态混合",
    order: 3,
    ownerDomain: "VE-M",
    inputContract: "BonePoseBuffer",
    outputContract: "BlendedPoseBuffer",
    duty: "多姿态按骨骼级权重混合（替换/叠加/加法三模式 + 骨骼掩码）；不做蒙皮",
    realizedBy: "VE-F2424 姿态混合 / VE-F2426 混合树 / VE-F2427 状态机 / VE-F2428 动画层",
    costCalibratedBy: "VE-F2432 骨骼动画基准（混合族）",
  },
  "bone-apply": {
    id: "bone-apply",
    name: "骨骼应用",
    order: 4,
    ownerDomain: "VE-I",
    inputContract: "BlendedPoseBuffer",
    outputContract: "SkinnedVertexBuffer",
    duty: "姿态 → 骨骼矩阵 → 蒙皮顶点；蒙皮本体归 I 域，M 域只交付姿态缓冲",
    realizedBy: "I04 蒙皮（F1666 采样契约的兑现方）；M 侧为 VE-F2422 姿态缓冲契约",
    costCalibratedBy: "VE-F2412 动画基准（导入导出族外的渲染侧对照）",
  },
};

/** 管线段 id 列表。 */
export const PIPELINE_STAGE_IDS: readonly PipelineStageId[] = Object.keys(
  PIPELINE_STAGES,
) as PipelineStageId[];

/**
 * 冻结执行序：段 id 按序排好的一张表。
 *
 * 这是本域最常被引用的一张常量——下游 M01/M02/M03 逐段实现，F2430 四段模型
 * 逐段定标，F2543 I 域终验按这张表核对三段契约。故它是一等常量而非函数产物：
 * 引用方直接读，不重排、不筛选、不派生。
 */
export const PIPELINE_SEQUENCE: readonly PipelineStageId[] = [
  "track-eval",
  "clip-sample",
  "pose-blend",
  "bone-apply",
];

/** 域级预算归属声明（架构声明本身零运行时成本；预算进 M 域账本，由 F2481 建档）。 */
export const PIPELINE_BUDGET_DECLARATION = {
  /** 架构声明本身无运行时成本：顶层只有常量表与纯函数，不在帧内被调用。 */
  architectureRuntimeCost: "none" as const,
  /** 四段成本的定标责任条目。 */
  costOwnerItem: "VE-F2412 / VE-F2432",
  /** 域级性能账本建档条目（M 域段，后组建档时填写实测值）。 */
  budgetLedgerItem: "VE-F2481 动画性能账本（M 域段）",
  /** 预算超限时的降级次序：先 LOD 降频，再跳过缓存，最后降插值精度。 */
  degradeOrder: ["lod-reduce-freq", "cache-skip", "interpolation-precision"] as const,
} as const;

/**
 * 管线序断言：提交的执行序必须逐字等于冻结序。
 *
 * 四种失败形态各有各的成因，逐条点名（合并成一句「序错了」会让归因退化为猜测）：
 *   · 段数不足 → 缺段（实现未完成就接上了下游，此时跑起来往往不报错但画面缺一层）；
 *   · 段集合相同但顺序不同 → 序破坏（最隐蔽，四段都在都跑对，画面仍不对）；
 *   · 提交了未登记的段 → 段未知（有人私自加了第五段，序无从谈起）；
 *   · 段重复 → 重复段（同一段跑两次，第二次通常读到第一次写坏的数据）。
 */
export function assertPipelineSequence(
  actual: readonly PipelineStageId[],
  bag: DiagBag,
): Outcome<readonly PipelineStageId[]> {
  const expected = PIPELINE_SEQUENCE;

  // 形态零：提交里有未登记的段 id。
  // 这一形态必须排在所有计数形态（缺段/重复/超段）之前，原因是三件事：
  //   1. 未登记段不是「数量」问题而是「身份」问题——先报数量会把真正的病因
  //      藏进消息里（提交 4 段里有 1 段叫 "bone-apply2"，报成「顺序错乱」，
  //      调用方会去调顺序，而真正该做的是那个多出来的 2）。
  //   2. 段数恰好等于四段时，形态一/二/三都不命中，会一路落到形态四（顺序比对），
  //      而形态四要为「实为」的那一段取名——未登记段没有名字，取名即崩。
  //      换言之，不在这里拦住，形态四就会拿裸异常覆盖掉本该给出的结构化诊断，
  //      这正是本域零静默红线要防的事：调用方拿到的是 TypeError 而不是可定位的诊断。
  //   3. 执行序可能来自配置/序列化产物而非本文件的字面量，类型标注在那条路上不成立，
  //      故此处按运行时值判定，不依赖静态类型。
  const unknown = actual.filter((s) => !PIPELINE_STAGES[s]);
  if (unknown.length > 0) {
    const listed = unknown.map((s) => `「${s}」`).join("、");
    bag.push(
      "PIPELINE_STAGE_UNKNOWN",
      `管线执行序含 ${unknown.length} 个未登记段：${listed}`,
      `冻结序只有四段（${expected.map((s) => PIPELINE_STAGES[s].name).join(" → ")}）。`
        + "段 id 须取自 PIPELINE_STAGES 的键；若确需新增段，须先走 ADR 修订冻结序——"
        + "各实现对「管线有几步」的理解一旦分叉，症状会落在画面上而不是报错上",
    );
    return fail(
      "PIPELINE_STAGE_UNKNOWN",
      `管线执行序含未登记段：${listed}`,
      "改用已登记的段 id，或先走 ADR 修订冻结序",
    );
  }

  // 形态一：段数不足。
  if (actual.length < expected.length) {
    const missing = expected.filter((s) => !actual.includes(s));
    const missingNames = missing.map((s) => PIPELINE_STAGES[s].name).join("、");
    bag.push(
      "PIPELINE_SEQUENCE_BROKEN",
      `管线执行序缺 ${missing.length} 段：${missingNames}`
        + `（提交 ${actual.length} 段，冻结序为 ${expected.length} 段）`,
      `四段缺任一段时管线通常不会报错，但画面会缺一层（例如缺②时姿态来自上一帧）。`
        + `请先补齐缺失段再接入下游。${PIPELINE_SEQ_NOTE}`,
    );
    return fail(
      "PIPELINE_SEQUENCE_BROKEN",
      `管线执行序缺段：${missingNames}`,
      "补齐缺失段后重新提交执行序",
    );
  }

  // 形态二：段重复。
  const seen = new Set<PipelineStageId>();
  const dup = actual.filter((s) => {
    if (seen.has(s)) return true;
    seen.add(s);
    return false;
  });
  if (dup.length > 0) {
    const dupNames = dup.map((s) => PIPELINE_STAGES[s]?.name ?? String(s)).join("、");
    bag.push(
      "PIPELINE_SEQUENCE_BROKEN",
      `管线执行序中段重复出现：${dupNames}`,
      "同一段在一帧内只能执行一次。重复段通常来自「求值」与「应用」两处都调了采样，"
        + "第二次读到的是第一次写坏的缓冲——请收敛到单一调用点",
    );
    return fail(
      "PIPELINE_SEQUENCE_BROKEN",
      `管线执行序段重复：${dupNames}`,
      "收敛到单一调用点，保证一段一帧一次",
    );
  }

  // 形态三：段数超出（提交了未登记的段）。
  if (actual.length > expected.length) {
    const extra = actual.filter((s) => !expected.includes(s));
    bag.push(
      "PIPELINE_STAGE_UNKNOWN",
      `管线执行序包含 ${extra.length} 个未登记段：${extra.join("、")}`,
      `冻结序只有四段（${expected.map((s) => PIPELINE_STAGES[s].name).join(" → ")}）。`
        + "若确需新增段，须先走 ADR 修订冻结序，不能由某个实现条目自行加段——"
        + "否则各实现对「管线有几步」的理解会分叉",
    );
    return fail(
      "PIPELINE_STAGE_UNKNOWN",
      `管线执行序含未登记段：${extra.join("、")}`,
      "先走 ADR 修订冻结序，或移除私自新增的段",
    );
  }

  // 形态四：段集合相同但顺序不同 —— 最隐蔽的一种。
  // 注意先数不一致位：段集合相同且逐位相同时（提交的就是冻结序本身）不应进本分支，
  // 否则会把「完全正确的执行序」误判为序破坏——检查器对自己的冻结序失灵，
  // 比没有检查器更糟（它会训练调用方忽略这个断言）。
  const diffs: string[] = [];
  for (let i = 0; i < expected.length; i += 1) {
    const exp = expected[i];
    const got = actual[i];
    if (exp === undefined || got === undefined) continue;
    // 两段都取自已登记表：exp 恒存在于冻结序，got 已由形态零拦过未登记值。
    // 这里的取值仍写成可失败形式而非直接下标——本函数不该把「上游已校验过」
    // 当作自己的前提，任何一处直接下标都会在前提被绕开时抛裸异常。
    const expSpec = PIPELINE_STAGES[exp];
    const gotSpec = PIPELINE_STAGES[got];
    if (expSpec === undefined || gotSpec === undefined) continue;
    if (exp !== got) {
      diffs.push(
        `第 ${String(expSpec.order)} 段应为「${expSpec.name}」，实为「${gotSpec.name}」`,
      );
    }
  }
  if (diffs.length === 0) {
    return ok(actual, bag.all());
  }
  bag.push(
    "PIPELINE_SEQUENCE_BROKEN",
    `管线四段齐备但顺序错乱：${diffs.join("；")}`,
    `这是最难归因的一种序破坏——四段都在、各自单测都过，画面仍然不对。${PIPELINE_SEQ_NOTE}`,
  );
  return fail(
    "PIPELINE_SEQUENCE_BROKEN",
    "管线四段齐备但执行序不等于冻结序",
    "按 PIPELINE_SEQUENCE 重排执行序，不要在实现里自行决定顺序",
  );
}

/** 管线规格自洽审计：段序位唯一且恰为 1~4、段间契约首尾相接、第四段归属 I 域。 */
export function auditPipelineSpec(bag: DiagBag): Outcome<readonly PipelineStageSpec[]> {
  const specs = PIPELINE_SEQUENCE.map((id) => PIPELINE_STAGES[id]);

  // 序位必须恰为 1/2/3/4 且互不重复。
  const orders = specs.map((s) => s.order);
  for (const o of [1, 2, 3, 4] as const) {
    const hits = orders.filter((x) => x === o).length;
    if (hits !== 1) {
      bag.push(
        "PIPELINE_STAGE_SPEC_INCONSISTENT",
        `管线第 ${String(o)} 段命中 ${String(hits)} 次（应恰为 1 次）`,
        "四段的 order 字段必须与 1/2/3/4 一一对应，否则序断言的判定基准本身失真",
      );
    }
  }

  // 段间契约必须首尾相接：第 N 段的输出契约 = 第 N+1 段的输入契约。
  for (let i = 0; i + 1 < specs.length; i += 1) {
    const cur = specs[i];
    const next = specs[i + 1];
    if (cur === undefined || next === undefined) continue;
    if (cur.outputContract !== next.inputContract) {
      bag.push(
        "PIPELINE_STAGE_SPEC_INCONSISTENT",
        `管线段间契约断裂：「${cur.name}」输出 ${cur.outputContract}，`
          + `而「${next.name}」消费 ${next.inputContract}`,
        "段间传递的是输出契约，断裂意味着实现里只能靠隐式约定对齐——"
          + "那正是跨域差一帧缺陷的源头",
      );
    }
  }

  // 段要素齐备。
  for (const s of specs) {
    if (
      s.duty.trim().length === 0
      || s.realizedBy.trim().length === 0
      || s.costCalibratedBy.trim().length === 0
    ) {
      bag.push(
        "PIPELINE_STAGE_SPEC_INCONSISTENT",
        `管线段「${s.name}」的要素不齐（职责/兑现条目/定标来源任一为空）`,
        "每段都要写明「谁实现、成本谁定标」——否则成本无处归账、定标无人负责",
      );
    }
  }

  // 第四段的归属域必须是 I 域（蒙皮归 I 是三域边界的核心投影）。
  const apply = PIPELINE_STAGES["bone-apply"];
  if (apply.ownerDomain !== "VE-I") {
    bag.push(
      "PIPELINE_STAGE_SPEC_INCONSISTENT",
      `第 4 段「${apply.name}」的归属域为 ${apply.ownerDomain}，应为 VE-I`,
      "蒙皮是顶点几何运算，依赖 I 域顶点流水线。M 域实现蒙皮会造成同一批顶点两套蒙皮结果",
    );
  }

  if (bag.hasAny) {
    const first = bag.all()[0];
    return fail(
      first?.code ?? "PIPELINE_STAGE_SPEC_INCONSISTENT",
      first?.message ?? "管线规格审计未通过",
      first?.hint ?? "按上述诊断逐条修正",
    );
  }
  return ok(specs, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §4 三域边界声明 + 双向驱动环检测（判据三：M / I / L 三域边界）
// ════════════════════════════════════════════════════════════════════════════

/** 涉及的三域（封闭集：M 域只与这三域有架构级边界，其余域走各自条目声明）。 */
export type DomainId = "VE-M" | "VE-I" | "VE-L";

/** 一个边界能力的声明：谁管、怎么交接、越界了会怎样。 */
export interface BoundarySpec {
  readonly capability: string;
  /** 归属域（唯一——一个能力只能有一个owner，这是边界表可机检的根本）。 */
  readonly owner: DomainId;
  /** 人话职责（做什么、不做什么）。 */
  readonly duty: string;
  /** 交接契约：非owner 域怎样合法地用到这个能力（经契约，不直连）。 */
  readonly handoffContract: string;
  /** 越界时的处置方式（回改 / 拒绝 / 报缺件，三者语义不同，须逐条写明）。 */
  readonly onViolation: string;
}

/**
 * 三域边界表（判据三的唯一事实源）。
 *
 * 这张表为什么必须在开工条上就钉死（域级立场）：
 *   M / I / L 三个域在「一帧里」彼此紧邻，且彼此都觉得自己该管某件事：
 *     · 骨骼矩阵是谁算的？M 说「姿态是我算的」，I 说「矩阵是我用的」；
 *     · 物理触发的动画切换是谁触发的？L 说「物理我模拟」，M 说「状态机我管」；
 *     · 顶点最终位置谁写？I 说「蒙皮归我」，M 域早期实现里曾顺手写过。
 *   这三类争议的共同点是：**每一方的单看都说得通**。若不在开工时把归属
 *   写成可机检的表，争议会以「两个域各写一份、按谁的调用顺序生效」的形式
 *   沉淀下来，到 M08跨域联动核验时才发现时，返工面是三个域的实现而非一个条目。
 *
 * 边界表的一条硬纪律：**一个能力只有一个 owner**。
 *   若某能力被两个域同时声明为owner，「越界」这个概念就失去了判定基准——
 *   两边都合法等于没人管。故本表在auditBoundaryTable 里机检owner 唯一性。
 */
export const BOUNDARY_TABLE: readonly BoundarySpec[] = [
  {
    capability: "关键帧轨道与插值",
    owner: "VE-M",
    duty: "轨道数据结构、六类轨道、插值器与求值；轨道单源为剪辑域 F1345，M 域做域级扩展不另造轨道数据",
    handoffContract: "F1345 轨道单源（复用其轨道数据结构）+ F2402 轨道系统",
    onViolation: "回改：另造轨道数据结构即破坏单源，须改为经 F1345 扩展",
  },
  {
    capability: "clip 采样与姿态组装",
    owner: "VE-M",
    duty: "多 clip 采样、四元数 slerp、姿态缓冲产出；不做混合决策",
    handoffContract: "F2422 clip 模型 + F2423 slerp 专项 + F1666 采样契约（跨域冻结）",
    onViolation: "回改：I 域若自行采样即为契约重实现，须回到 F1666 单源",
  },
  {
    capability: "姿态混合与状态机决策",
    owner: "VE-M",
    duty: "三混合模式、骨骼掩码、混合树、状态机迁移、动画层叠加",
    handoffContract: "F2424 姿态混合 + F2426 混合树 + F2427 状态机 + F2428 动画层",
    onViolation: "回改：混合语义不得由渲染侧或物理侧代为决策",
  },
  {
    capability: "蒙皮与骨骼变换应用",
    owner: "VE-I",
    duty: "骨骼矩阵作用到网格顶点、权重绑定、GPU 顶点变体；**M 域越界即本条最典型的缺陷**",
    handoffContract: "F2422 姿态缓冲契约（M 输出 → I04 消费）+ I04 F1666 采样契约",
    onViolation: "回改：M 域实现蒙皮即为越界，须删除 M 侧蒙皮并改走姿态缓冲",
  },
  {
    capability: "网格顶点最终写入",
    owner: "VE-I",
    duty: "顶点缓冲内容与GPU 上传；M 域只交付姿态与形变权重，不碰顶点",
    handoffContract: "F2441形态键形变后顶点缓存键 + F1601 I01 网格格式",
    onViolation: "拒绝：M 域直写顶点缓冲无合法路径，须走形态键权重契约",
  },
  {
    capability: "刚体与碰撞求解",
    owner: "VE-L",
    duty: "积分、约束、碰撞场查询、休眠策略；M 域只提供运动学目标位",
    handoffContract: "F2324 物理与动画同步（同帧映射协议）+ L04 刚体",
    onViolation: "回改：M 域自行积分刚体即产生第二套求解器，物理语义会分叉",
  },
  {
    capability: "物理驱动动画（反向）",
    owner: "VE-M",
    duty: "物理事件 → 动画状态迁移的映射与反向桥接；物理怎么产生的由 L 域负责",
    handoffContract: "F2324 反向桥接位（物理事件→动画触发的映射注册制）",
    onViolation: "拒绝：M 域不得直接读 L 域求解器内部态，须经事件对映射",
  },
  {
    capability: "动画驱动物理（正向）",
    owner: "VE-M",
    duty: "动画轨道 → 物理目标位/触发条件的参数下发；物理如何消费由 L 域负责",
    handoffContract: "F2144 跨域轨道互操作（单向 M→K/物理侧参数通道，非直写内部态）",
    onViolation: "拒绝：M 域直写 L 域参数即越权，须走参数通道",
  },
  {
    capability: "弹簧动效（UI 级）",
    owner: "VE-M",
    duty: "UI 元素弹簧（k/c/初速度，无碰撞无重力）；场景刚体归 L 域，二者数学同源但应用不同",
    handoffContract: "F2443 物理动效（弹簧模型 + 与 L04 边界声明）",
    onViolation: "回改：把 UI 弹簧与场景刚体混为一套即失去 L04 的诚实定位",
  },
  {
    capability: "帧内执行序",
    owner: "VE-M",
    duty: "动画更新 → 事件检查 → 物理触发 → 物理步进，四段帧内序的冻结与断言",
    handoffContract: "F2324 帧内序冻结（次序纪律，序断言进 CI）+ F1922 帧快照语义",
    onViolation: "回改：序错乱即用错前帧状态，判为 P0 序红线",
  },
];

/** 边界表的外键索引：能力名 → 规格（构造期建一次，运行期只读）。 */
const BOUNDARY_INDEX: ReadonlyMap<string, BoundarySpec> = new Map(
  BOUNDARY_TABLE.map((b) => [b.capability, b]),
);

/**
 * 边界越界检查：给定「某域要做某能力」，判断是否越界。
 *
 * 为什么检查器要同时覆盖「未登记能力」与「已登记但owner 不是本域」两种失败：
 *   · 未登记能力 → 有人做了一件三域边界表里根本没有的事。这类缺陷最危险：
 *     它既没有owner，也没有交接契约，意味着没有任何域对它负责。
 *   · owner 不是本域 → 经典越界（如 M 域做蒙皮）。有owner，但调用方不是它。
 *   两种都必须显性失败，且提示语不同：前者要「先登记能力」，
 *   后者要「改走owner 的契约」——处置动作不一样，提示不能混。
 *
 * bag 为可选参数（缺省时诊断并入返回值）：
 *   本函数是本域被调用频次最高的守卫（每个实现条目接入时都要问一句
 *   「这能力归谁」）。若把 bag 设为必填，一个只想拿布尔答案的调用方
 *   漏传就会得到裸 TypeError——守卫自己成了崩溃源，且崩溃信息与
 *   「能力越界」毫无关系，排障时被引向完全错误的方向。
 *   故此处允许省略：省略时诊断随 Outcome 返回，一样不丢；
 *   传入 bag 时额外累积一份，供调用方聚合多个守卫的结果。
 */
export function checkBoundary(
  domain: DomainId,
  capability: string,
  bag?: DiagBag,
): Outcome<BoundarySpec> {
  const spec = BOUNDARY_INDEX.get(capability);
  if (spec === undefined) {
    return boundaryUnregistered(capability, bag);
  }
  if (spec.owner !== domain) {
    return boundaryViolation(domain, spec, bag);
  }
  return ok(spec, bag === undefined ? [] : bag.all());
}

/** 未登记能力：抽出为独立函数，使 checkBoundary 在两处复用同一份诊断措辞。 */
function boundaryUnregistered(
  capability: string,
  bag?: DiagBag,
): Outcome<BoundarySpec> {
  const d: Diagnostic = {
    code: "BOUNDARY_CAPABILITY_UNREGISTERED",
    message: `能力「${capability}」不在 M/I/L 三域边界表中，无人负责该能力`,
    hint: `已登记能力为：${BOUNDARY_TABLE.map((b) => b.capability).join("、")}。`
      + "未登记能力既无 owner 也无交接契约——先把它登记进边界表（并确定 owner），"
      + "再实现它；否则这段代码没有任何域为它的正确性负责",
  };
  if (bag !== undefined) bag.push(d.code, d.message, d.hint);
  return fail(d.code, `能力「${capability}」未在三域边界表中登记`, "先登记能力并确定 owner，再实现");
}

/** 已登记但 owner 不是本域：越界。 */
function boundaryViolation(
  domain: DomainId,
  spec: BoundarySpec,
  bag?: DiagBag,
): Outcome<BoundarySpec> {
  const d: Diagnostic = {
    code: "BOUNDARY_VIOLATION",
    message: `越界：${domain} 执行了归属${spec.owner} 的能力「${spec.capability}」`,
    hint: `该能力的 owner 是 ${spec.owner}，职责为「${spec.duty}」，`
      + `交接契约为「${spec.handoffContract}」，越界处置为「${spec.onViolation}」。`
      + `${domain} 侧应改为经该契约使用，而不是自行实现`,
  };
  if (bag !== undefined) bag.push(d.code, d.message, d.hint);
  return fail(
    d.code,
    `${domain} 越界执行 ${spec.owner} 的能力「${spec.capability}」`,
    `改走 ${spec.owner} 的契约：${spec.handoffContract}`,
  );
}

/** 边界表自洽审计：能力名不重复、每能力恰有唯一 owner、要素齐备。 */
export function auditBoundaryTable(bag: DiagBag): Outcome<readonly BoundarySpec[]> {
  const seen = new Set<string>();
  for (const b of BOUNDARY_TABLE) {
    if (seen.has(b.capability)) {
      bag.push(
        "BOUNDARY_VIOLATION",
        `边界表中能力「${b.capability}」重复登记`,
        "一个能力只能有一条边界声明。重复登记会让「越界」的判定基准出现两个候选 owner",
      );
    }
    seen.add(b.capability);
    if (b.duty.trim().length === 0 || b.handoffContract.trim().length === 0
      || b.onViolation.trim().length === 0) {
      bag.push(
        "BOUNDARY_VIOLATION",
        `边界能力「${b.capability}」的要素不齐（职责/交接契约/越界处置任一为空）`,
        "边界声明的三要素缺一不可：没有处置方式的边界表在真正越界时拿不出任何动作",
      );
    }
    if (!(["VE-M", "VE-I", "VE-L"] as readonly string[]).includes(b.owner)) {
      bag.push(
        "BOUNDARY_VIOLATION",
        `边界能力「${b.capability}」的 owner 为 ${b.owner}，不在 M/I/L 三域内`,
        "本表只声明 M/I/L 三域边界；跨到其他域的边界由对应条目单独声明",
      );
    }
  }
  if (bag.hasAny) {
    const first = bag.all()[0];
    return fail(
      first?.code ?? "BOUNDARY_VIOLATION",
      first?.message ?? "边界表审计未通过",
      first?.hint ?? "按上述诊断逐条修正",
    );
  }
  return ok(BOUNDARY_TABLE, bag.all());
}

/**
 * 驱动边：一条「A 域某能力驱动 B 域某能力」的有向边。
 *
 * 为什么需要环检测（判据三的第二条错误路径，F1841 家族）：
 *   动画与物理是**双向衔接**的（物理驱动动画 / 动画驱动物理），这本身是对的——
 *   锤子落下的物理事件要触发碎裂动画，碎裂动画又要把碎片作为粒子发射器的目标。
 *   但双向衔接一旦在同一条依赖链上闭合，就成了环：求值 A 需要 B 的结果，
 *   求值 B 又需要 A 的结果。二者都拿不到对方的「本帧结果」，
 *   于是各自用上一帧或半成品的值——现象是物理抖动或动画回跳，
 *   且抖动幅度随帧率变化（因为落到上一帧的比例在变），极难归因。
 *
 *   环检测必须在这张表上做，而不是等运行时崩：运行时崩了只能证明「有环」，
 *   不能告诉你是哪几条边构成了环。构成环的边集才是可修的东西——
 *   通常只需把其中一条边改成经事件/参数通道的**延迟一跳**。
 */
export interface DriveEdge {
  readonly from: DomainId;
  readonly to: DomainId;
  /** 边所承载的能力（须在边界表中登记）。 */
  readonly capability: string;
  /**
   * 衔接方式：
   *   · `direct`  = 同帧直接驱动（必须无环）；
   *   `deferred` = 经事件/参数通道延迟一跳（可安全打断环）。
   */
  readonly mode: "direct" | "deferred";
}

/**
 * 域间驱动边表（M ↔ L 双向衔接 + M → K 参数下发）。
 *
 * 这张表登记的是**架构级的驱动关系**，不是某一帧的调用。
 * 它是 F2324（物理与动画同步）与 F2144（与 M 域动画联动）两件契约在
 * M 域侧的投影：两条正向/反向边都保留，但其中一条被显式声明为deferred，
 * 环因此被打断——这是架构层面的正确做法，而不是「禁止双向」。
 */
export const DRIVE_EDGES: readonly DriveEdge[] = [
  {
    from: "VE-M",
    to: "VE-L",
    capability: "动画驱动物理（正向）",
    mode: "direct",
  },
  {
    from: "VE-L",
    to: "VE-M",
    capability: "物理驱动动画（反向）",
    // 延迟一跳：物理事件先进事件对映射（F2324），下一帧才驱动动画状态迁移。
    // 这一跳是打断环的关键——它让「物理结果」与「动画迁移」不在同一帧互相依赖。
    mode: "deferred",
  },
];

/**
 * 驱动环检测：在驱动边表上做深度优先搜索，找所有回边，并按「能否在同帧内闭合」分类。
 *
 * 为什么必须按 mode 分类（这是本函数最容易写错的地方，也是第一版写错的地方）：
 *   环本身不是缺陷——M↔L 的双向衔接**必然**在图上构成一个环（物理驱动动画、
 *   动画驱动物理，两条边都在）。缺陷是「**同帧内**闭合的环」：
 *   只有当环上每一条边都是 direct 时，双方才会在同一帧里互相等待本帧结果，
 *   谁都拿不到，症状是抖动或回跳。
 *   若环上存在至少一条 deferred 边，该边会把依赖推到下一帧，环在帧内不再闭合——
 *   这正是 DRIVE_EDGES 里把反向边声明为 deferred 的用意。
 *   故本函数报出两类结果：
 *     · `fatal`  —— 同帧可闭合（环上全direct），必须改；
 *     · `broken` —— 已被 deferred 打断，登记备查（说明「这里曾有环，靠哪条边断开的」），
 *                   断开的那条边一旦被改回 direct，本函数立刻转为fatal。
 *   第一版把两类都当fatal，于是「正确打断环的架构」被判成缺陷——那等于
 *   逼迫后来者删掉双向衔接能力，而不是保留它并显式声明断开方式。
 *
 * 返回的环以人类可读形式给出（边序列 + 闭环边 + 判定结论），而不是只给布尔：
 * 修环需要知道「哪几条边构成了环、哪条边负责断开」——选哪条边延迟，
 * 取决于哪条边的语义允许延迟（物理触发通常允许延迟一帧，动画驱动的
 * 碰撞体位置通常不允许）。
 *
 * edges 为可选注入参数（缺省用 DRIVE_EDGES）：
 *   检测器若只能读模块级常量表，就无法被对抗验证——「把反向边改回direct
 *   会不会真的转为 fatal」这条判据只能靠人读代码确认。而这恰恰是本函数
 *   最需要被证伪的一条：它是M↔L 双向衔接得以保留的唯一根据。
 *   注入参数让该断言可在 CI 中跑，而不是停留在注释里。
 */
export interface DriveCycle {
  /** 环上的域序列（首尾同域）。 */
  readonly loop: readonly string[];
  /** 构成该环的边的判定结论。 */
  readonly verdict: "fatal" | "broken";
  /** 若verdict 为 broken，说明是哪一条边打断了它。 */
  readonly brokenBy?: string;
  /** 构成该环的边描述（逐条可读）。 */
  readonly edges: readonly string[];
}

/** 在指定边集上做DFS，回边按同帧可闭合性分类。 */
function findCycles(edges: readonly DriveEdge[]): readonly DriveCycle[] {
  const found: DriveCycle[] = [];
  const visited = new Set<DomainId>();
  const stack: DomainId[] = [];
  const stackEdges: DriveEdge[] = [];

  const dfs = (node: DomainId): void => {
    visited.add(node);
    stack.push(node);
    for (const e of edges) {
      if (e.from !== node) continue;
      if (stack.includes(e.to)) {
        const at = stack.indexOf(e.to);
        const loopDomains = [...stack.slice(at), e.to];
        const loopEdges = [...stackEdges.slice(at), e];
        const breaker = loopEdges.find((x) => x.mode === "deferred");
        found.push({
          loop: loopDomains,
          verdict: breaker === undefined ? "fatal" : "broken",
          ...(breaker === undefined
            ? {}
            : { brokenBy: `${breaker.from} → ${breaker.to}「${breaker.capability}」` }),
          edges: loopEdges.map((x) => `${x.from} → ${x.to}「${x.capability}」mode=${x.mode}`),
        });
      } else if (!visited.has(e.to)) {
        stackEdges.push(e);
        dfs(e.to);
        stackEdges.pop();
      }
    }
    stack.pop();
  };

  const roots: DomainId[] = ["VE-M", "VE-I", "VE-L"];
  for (const r of roots) {
    if (!visited.has(r)) dfs(r);
  }
  return found;
}

/**
 * 驱动环检测（对外入口）：同帧可闭合的环判失败，已被 deferred 打断的环在返回值中登记备查。
 *
 * 注意「备查」不进诊断袋：诊断袋的语义是「有问题」，把正确架构也塞进去会让
 * 上报方把备查项与缺陷混为一谈（真出现 P0 时没人再看得懂袋子里哪条要紧）。
 * 已断开的环通过返回值里的 verdict=broken 字段暴露给调用方与自检项。
 */
export function detectDriveCycles(
  bag: DiagBag,
  edges: readonly DriveEdge[] = DRIVE_EDGES,
): Outcome<readonly DriveCycle[]> {
  const cycles = findCycles(edges);
  const fatal = cycles.filter((c) => c.verdict === "fatal");

  if (fatal.length > 0) {
    for (const c of fatal) {
      bag.push(
        "DRIVE_CYCLE_DETECTED",
        `驱动环（同帧可闭合）：${c.loop.join(" → ")}；环上各边：${c.edges.join("；")}`,
        "环上全是 mode=direct 的边：同帧互相依赖会让双方都拿不到对方本帧结果，"
          + "现象是物理抖动或动画回跳且抖动幅度随帧率变化。"
          + "请把环上语义允许延迟的那条边改为 mode=deferred（经事件/参数通道延迟一跳），"
          + "而不是删掉这条衔接能力",
      );
    }
    return fail(
      "DRIVE_CYCLE_DETECTED",
      `驱动边表检出 ${String(fatal.length)} 个同帧可闭合的环`,
      "把环上语义允许延迟的边改为 deferred 以打断环",
    );
  }
  return ok(cycles, bag.all());
}

/**
 * 帧内序断言（判据三的第三项：F2324 帧内序冻结）。
 *
 * 帧内序是「动画更新 → 事件检查 → 物理触发 → 物理步进」四步。
 * 为什么它是正确性而不是优化：次序错误时，物理触发读到的是动画**更新前**的
 * 状态——例如「锤子触底」这一事件会拿锤子尚未到位的姿态去判定碰撞体形状。
 * 症状是偶发的、依赖帧内耗时的（机器越快越不容易复现），归因方向却会指向
 * 物理或动画任一侧。故序断言必须存在，且必须在 CI 里跑。
 */
export const FRAME_ORDER: readonly string[] = [
  "anim-update",
  "event-check",
  "physics-trigger",
  "physics-step",
];

/**
 * 帧内序检查：提交的实际帧内步序必须逐字等于冻结序。
 *
 * 与管线序断言同构的思路：不问「四步是否都在」而问「序是否等于冻结序」。
 * 帧内序错乱时四步通常一个不少（都在跑，只是次序变了），所以缺步检测抓不到它。
 *
 * bag 可选的理由同 checkBoundary：守卫自身不得成为崩溃源。
 */
export function checkFrameOrder(
  actual: readonly string[],
  bag?: DiagBag,
): Outcome<readonly string[]> {
  const expected = FRAME_ORDER;
  // 诊断入袋为可选：袋缺省时诊断随返回值走（fail 本身已带三要素），不丢信息。
  const note = (message: string, hint: string): void => {
    if (bag !== undefined) bag.push("FRAME_ORDER_VIOLATION", message, hint);
  };
  if (actual.length !== expected.length) {
    note(
      `帧内步数不符：提交 ${String(actual.length)} 步，冻结序为 ${String(expected.length)} 步`,
      `帧内序为 ${expected.join(" → ")}。少一步会让触发读到陈旧状态，多一步通常意味着重复触发`,
    );
    return fail(
      "FRAME_ORDER_VIOLATION",
      `帧内步数不符（${String(actual.length)} ≠ ${String(expected.length)}）`,
      "按 FRAME_ORDER 提交四步",
    );
  }
  for (let i = 0; i < expected.length; i += 1) {
    const e = expected[i];
    const a = actual[i];
    if (e !== a) {
      note(
        `帧内序错乱：第 ${String(i + 1)} 步应为 ${String(e)}，实为 ${String(a)}`,
        "次序错误即用错前帧状态：物理触发会拿到动画更新前的姿态判定碰撞，"
          + "症状偶发且随机器速度变化。请按 F2324 帧内序冻结执行，不要在实现里自行调序",
      );
      return fail(
        "FRAME_ORDER_VIOLATION",
        `帧内序第 ${String(i + 1)} 步错乱（应为 ${String(e)}，实为 ${String(a)}）`,
        "按 FRAME_ORDER 冻结序执行",
      );
    }
  }
  return ok(actual, bag === undefined ? [] : bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §5 F2400 移交包接收（判据四：K→L→M 三域契约链的第三环）
// ════════════════════════════════════════════════════════════════════════════

/** 一条移交包遵守义务：M 域必须做到什么（逐条可核验，非口号）。 */
export interface HandoverObligation {
  readonly id: string;
  /** 义务的人话描述。 */
  readonly duty: string;
  /** 核验方式（怎么证明做到了——不能只写「做到」）。 */
  readonly verify: string;
}

/**
 * L→M 移交契约（F2400 移交包 · 契约件）。
 *
 * 契约链是什么、为什么 M 域不能越过 L 域直接依赖 K 域（域级立场）：
 *   VE 的域间承诺是一条**链**：K（后处理链）→ L（粒子与物理）→ M（动画系统）。
 *   K 域向后处理链定下效果插入序位；L 域把物理事件接入这条序（K→L 序契约，
 *   F2148 跨域序总图）；M 域的动画又依赖 L 域的物理事件与帧内序（L→M 移交）。
 *   链上每一环都把上一环的承诺**转写为自己的输入契约**，所以 M 域直接引用
 *   K 域的语义是有风险的：K 域的语义经L 域转写时可能已被裁剪或加约束
 *   （例如 K 的插值器枚举在 L 域只保留线性与阶跃两档，用于物理参数）。
 *   若 M 域绕过 L 域直取 K 的完整语义，两侧对「同一个插值器」的理解会分叉，
 *   而分叉点在动画里表现为「同一条曲线在物理驱动时和关键帧驱动时形状不同」。
 *
 * 未签署的后果（已定为开工阻断）：
 *   L 域无从知道 M 域是否遵守了帧内序与轨道单源，M 域也无从证明自己没有
 *   绕过 L 域直取 K 语义——契约链的每一环失效时，后面所有环的前提都不成立。
 */
export const L_TO_M_HANDOVER_CONTRACT = {
  /** 契约 id（对账主键）。 */
  id: "L-F2400-handover-to-M",
  /** 上游域（移交方）。 */
  upstreamDomain: "VE-L" as const,
  /** 契约链的链首域（M 域不得越过 L 域直取 K 语义）。 */
  chainHeadDomain: "VE-K" as const,
  /** 移交件来源锚点（L 域 F2400 收官宣告 · 五段移交包）。 */
  upstreamAnchor: "VE-F2400 移交包 · 契约件（跨域契约册 F2363 + 接口冻结总账 F2381）",
  /** 上游收官条目（L 域收官宣告）。 */
  upstreamClosureItem: "VE-F2400" as const,
  /** 链首收官条目（K 域收官宣告——契约链首环）。 */
  chainHeadClosureItem: "VE-F2200" as const,
  /** 随包快照哈希（接收方须回放校验）。 */
  snapshotHash: "3f8b6d41a05c27e9b4e0d7a12f6c8395",
} as const;

/** M 侧须逐条确认的遵守义务（逐条可核验，声明在契约对象之前以便内联引用）。 */
const HANDOVER_OBLIGATIONS: readonly HandoverObligation[] = [
  {
    id: "OB-01",
    duty: "M 域遵守帧内序「动画更新 → 事件检查 → 物理触发 → 物理步进」，不自行调序",
    verify: "F2324 序断言在 CI 中全绿；M01 自检项 frame-order 逐条通过",
  },
  {
    id: "OB-02",
    duty: "M 域的轨道数据经F1345 轨道单源，不另造轨道数据结构",
    verify: "F2402 绑定协议引用 F1345 数据结构；跨域对账钩子无「第二套轨道」记录",
  },
  {
    id: "OB-03",
    duty: "M 域不越过 L 域直取 K 域语义，K 侧参数经L 域转写契约进入 M 域",
    verify: "F2144 单向数据流检查：M→K 无直写路径；接入点在 L 域转写层",
  },
  {
    id: "OB-04",
    duty: "M 域产出的姿态缓冲供 I04 蒙皮消费，缓冲布局变更须先知会 I 域",
    verify: "F2422 姿态缓冲契约冻结 + F2543 I 域终验三核对零偏差",
  },
  {
    id: "OB-05",
    duty: "M 域的物理相关语义（弹簧动效）与 L 域场景刚体保持边界，不混为一套",
    verify: "F2443 边界声明 + F2464L 域联动核验：两类语义 diff 有据",
  },
  {
    id: "OB-06",
    duty: "M 域的确定性口径与 L 域时间轴一致（同平台逐位、跨平台不承诺）",
    verify: "F2415 双跑断言 M01 维全绿 + 口径表与 L 域 F2325 对账无偏差",
  },
];

/** M 侧禁令（越权即诊断，命中逻辑见 checkHandoverCompliance）。 */
const HANDOVER_FORBIDDEN: readonly string[] = [
  "直取 K 域完整语义绕过 L 域转写",
  "M 域自行实现蒙皮",
  "自行调整帧内序",
  "另造轨道数据结构",
  "直写 L 域物理内部态",
];

/**
 * 移交契约的最终形态（把义务与禁令装配进契约本体）。
 * 之所以分两步写而不在字面量里直接内联：契约本体需要 as const 保持字面量类型
 * （对账时按字面量比对），而义务表与禁令表是长数组，内联会让契约声明难以阅读。
 */
export const L_TO_M_HANDOVER = {
  ...L_TO_M_HANDOVER_CONTRACT,
  /** M 侧须逐条确认的遵守义务。 */
  obligations: HANDOVER_OBLIGATIONS,
  /** M 侧禁止做的事（越权即诊断）。 */
  forbidden: HANDOVER_FORBIDDEN,
} as const;

/** 接收方回执：M 域对移交包的接收确认（逐条义务打勾位）。 */
export interface HandoverReceipt {
  /** 接收的契约 id（须与 L_TO_M_HANDOVER.id 一致）。 */
  readonly contractId: string;
  /** 接收方标识（用于对账：谁签的）。 */
  readonly receiver: string;
  /** 逐条确认的义务 id 列表（须覆盖契约的全部义务，缺一即拒收）。 */
  readonly acceptedObligationIds: readonly string[];
  /** 回放校验通过的快照哈希（须等于契约声明值）。 */
  readonly verifiedSnapshotHash: string;
  /** 上游 L 域收官宣告是否已生效（F2400 生效为 true 才允许开工）。 */
  readonly upstreamDeclared: boolean;
  /** 契约链首环 K 域收官宣告是否已生效（F2200 生效为 true 才允许开工）。 */
  readonly chainHeadDeclared: boolean;
}

/**
 * 移交包接收（判据四的落地面）。
 *
 * 五道检查，顺序固定且每道都产出可定位的诊断：
 *   ① 契约链首环收官 —— K 域（F2200）未收官则阻断。这一条排在最前，
 *      因为 L 域的移交承诺建立在 K 链已冻结的前提上；K 未收官时 L 的转写层
 *      仍可按 L 域变更纪律调整，此时 M 域签的收据等于签了一张随时作废的纸。
 *   ② 上游收官 —— L 域（F2400）未收官则阻断（移交包尚未成包）。
 *   ③ 契约 id 一致 —— 收的是别的契约（拿错包）须显性失败。
 *   ④ 快照回放 —— 哈希不符即判移交件失真，须重出。
 *   ⑤ 义务逐条确认 —— 缺条即拒收并点名缺哪条。
 *
 * 返回的收据是 M 域账本的入库凭据：它把「M 承诺了哪几件事」变成可机检的数据，
 * 而不是一句「已知悉」。
 */
export function receiveHandover(
  receipt: HandoverReceipt,
  bag: DiagBag,
): Outcome<HandoverReceipt> {
  const c = L_TO_M_HANDOVER;

  // ① 契约链首环收官。
  if (!receipt.chainHeadDeclared) {
    bag.push(
      "CONTRACT_CHAIN_HEAD_UNCLOSED",
      `契约链首环 K 域尚未收官（${c.chainHeadClosureItem} 未生效），M 域开工前置不成立`,
      `K→L→M 是链式承诺：L 域对 K 序总图的转写随K 域收官而冻结。`
        + `K 未收官时 K 的序总图仍可按变更纪律调整，L 的转写层随之变动，`
        + `此时 M 域签署的收据等于签了一张随时作废的纸。请先完成 K 域收官再接收本契约`,
    );
    return fail(
      "CONTRACT_CHAIN_HEAD_UNCLOSED",
      "契约链首环 K 域未收官，移交契约不可接收",
      "先完成 K 域收官（F2200 宣告生效）",
    );
  }

  // ② 上游收官。
  if (!receipt.upstreamDeclared) {
    bag.push(
      "UPSTREAM_NOT_CLOSED",
      `上游 L 域尚未收官（${c.upstreamClosureItem} 移交包未成包），M 域开工前置不成立`,
      `移交包是 L 域收官宣告的产物（接口冻结总账 F2381 + 跨域契约册 F2363 + 教训册）。`
        + `请先完成 L 域收官（F2400 宣告生效、移交包五段齐备）再接收`,
    );
    return fail(
      "UPSTREAM_NOT_CLOSED",
      "上游 L 域未收官，移交契约不可接收",
      "先完成 L 域收官（F2400 宣告生效）",
    );
  }

  // ③ 契约 id 一致。
  if (receipt.contractId !== c.id) {
    bag.push(
      "HANDOVER_UNSIGNED",
      `回执声明的契约 id 为 ${receipt.contractId}，与 M 域待接收的 ${c.id} 不符`,
      `请核对移交包是否拿错。M 域需要的是 L 域 F2400 移交包中的契约件`
        + `（来源锚点：${c.upstreamAnchor}）`,
    );
    return fail("HANDOVER_UNSIGNED", `契约 id不匹配：${receipt.contractId}`, "核对移交包中的契约件");
  }

  // ④ 快照回放。
  if (receipt.verifiedSnapshotHash !== c.snapshotHash) {
    bag.push(
      "HANDOVER_HASH_MISMATCH",
      `移交件快照哈希不符：回执为 ${receipt.verifiedSnapshotHash}，L 域宣告为 ${c.snapshotHash}`,
      `快照失真意味着收到的移交件与 L 域宣告的不是同一份。请向 L 域索取重出快照`
        + `（L 域 F2397 错误路径：快照失真→重出），重出后重新回放校验；`
        + `在哈希对上之前不要继续签署——签的是哪一版移交件必须可追溯`,
    );
    return fail(
      "HANDOVER_HASH_MISMATCH",
      "移交件快照哈希失真",
      "向 L 域索取重出快照并重新回放校验",
    );
  }

  // ⑤ 义务逐条确认。
  const required = new Set(c.obligations.map((o) => o.id));
  const accepted = new Set(receipt.acceptedObligationIds);
  const missing = [...required].filter((id) => !accepted.has(id));
  if (missing.length > 0) {
    const names = missing
      .map((id) => c.obligations.find((o) => o.id === id)?.duty ?? id)
      .join("；");
    bag.push(
      "HANDOVER_OBLIGATION_MISSING",
      `移交契约接收回执缺 ${String(missing.length)} 条义务确认：${names}`,
      `移交义务须逐条确认，不得整体「已知悉」。缺条通常意味着 M 侧对该条的实现路径`
        + `还没想清楚——请先补齐实现方案再签，或向 L 域申请该条在本域的豁免（走 ADR）`,
    );
    return fail(
      "HANDOVER_OBLIGATION_MISSING",
      `移交契约义务缺 ${String(missing.length)} 条确认`,
      "逐条确认全部义务后再提交回执",
    );
  }

  return ok(receipt, bag.all());
}

/**
 * 移交禁令检查：给定一个待执行操作，判断它是否命中 M 侧禁令。
 *
 * 命中判定用「关键词全含」：中文禁令按 2 字滑窗切词，任一连续 2 字命中即判越权。
 * 这比整句相等更贴近调用方的写法（调用方写的是短描述，不是条款原文）——
 * 与 L 域 K_TO_L 序旁路检查同一策略。
 */
export function checkHandoverCompliance(
  operation: string,
  bag: DiagBag,
): Outcome<typeof L_TO_M_HANDOVER> {
  const c = L_TO_M_HANDOVER;
  const op = operation.toLowerCase();
  const hit = c.forbidden.find((ban) => {
    if (/^[\x20-\x7e]+$/.test(ban)) {
      return ban
        .split(/\s+/)
        .filter((t) => t.length >= 2)
        .every((t) => op.includes(t));
    }
    for (let i = 0; i + 2 <= ban.length; i += 1) {
      if (op.includes(ban.slice(i, i + 2))) return true;
    }
    return false;
  });
  if (hit !== undefined) {
    const owner = BOUNDARY_INDEX.get("蒙皮与骨骼变换应用");
    bag.push(
      "HANDOVER_UNSIGNED",
      `契约越权：M 域尝试执行「${operation}」，命中移交禁令「${hit}」`,
      owner !== undefined
        ? `蒙皮与骨骼变换应用归属${owner.owner}（${owner.duty}）。`
          + `M 域应经姿态缓冲契约（F2422）交付姿态，由 I04消费，而不是自行蒙皮。`
          + `若确有 I 域能力未覆盖的动画需求，走 ADR 与 I 域协商，不得自行实现`
        : `M 域不得执行该操作。${hit}——请按移交契约义务逐条对照实现路径`,
    );
    return fail(
      "HANDOVER_UNSIGNED",
      `操作「${operation}」命中移交禁令`,
      "按移交契约义务执行，或走 ADR 与对应域协商",
    );
  }
  return ok(c, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §6 M01 组内 20 条分工总览（F2401 ~ F2420 · 判据一至四的逐条落位）
// ════════════════════════════════════════════════════════════════════════════

/** 一条分工（组内 20 条中的一条）。 */
export interface WorkItem {
  /** 功能号（F24xx）。 */
  readonly id: number;
  /** 条目中文名（与册内标题逐字一致）。 */
  readonly title: string;
  /** 职责一句话（禁扩面写在句内）。 */
  readonly duty: string;
  /** 本条产出的契约 / 资产名（供下游消费；全局唯一由自检机检）。 */
  readonly producesContract: string;
  /** 判据锚点：这一条要证明什么（逐条可核验）。 */
  readonly criterion: string;
  /** 前置依赖条目号（无前置写0，表示可独立开工）。 */
  readonly dependsOn: readonly number[];
}

/**
 * M01 组 20 条分工（F2401~F2420）。
 *
 * 分工编排的三条原则（为什么是这 20 条、这个顺序）：
 *   ① 先架构后实现：F2401（开工条，本条）在最前，它冻结的四段序与三域边界
 *      是后续 19 条的约束；把它放在最后会让前 19 条各自按自己的理解实现序。
 *   ② 先单源后扩展：F2402（轨道系统，复用 F1345 单源）在 F2405（曲线资产）
 *      与 F2409/F2410（导入导出）之前——后者都是把轨道数据打包/解包，
 *      轨道结构未冻结时打包的格式必然要返工。
 *   ③ 后段是横向治理：F2411~F2419 是 fuzz/基准/API 冻结/文档/一致性/安全/
 *      质量档/遥测/组一致性——它们验收的是跨条目性质，放在能力条目齐备之后
 *      才能有完整对象可验。F2420 是组级关口（收口 + 向 M02 移交）。
 */
export const M01_WORK_ITEMS: readonly WorkItem[] = [
  {
    id: 2401,
    title: "M 域开工与动画架构",
    duty: "宣告M 域开工 + 冻结四段求值序 + 声明 M/I/L 三域边界 + 接收 F2400 移交包；不做任何轨道/姿态运算",
    producesContract: "DomainKickoffSummary",
    criterion: "域开工、十主题十批次齐备、四段序冻结、三域边界表可机检、契约接收签署",
    dependsOn: [],
  },
  {
    id: 2402,
    title: "关键帧轨道系统",
    duty: "六类轨道（位置/旋转/缩放/颜色/浮点/布尔）+ 实体轨道容器 + 绑定路径协议；不负责插值数学与姿态组装",
    producesContract: "TrackSet",
    criterion: "六类轨道规格表、容器多轨并存语义、绑定路径解析与失效检测、F1345 单源扩展声明",
    dependsOn: [2401],
  },
  {
    id: 2403,
    title: "关键帧插值",
    duty: "阶梯/线性/三次贝塞尔/缓动四插值器 + 可插拔注册 + 纯函数确定性纪律 + 贝塞尔数值稳定；不做轨道存储",
    producesContract: "InterpolatorRegistry",
    criterion: "四插值器数学语义、注册 API、插值纯函数双跑diff=0、贝塞尔端点严格与控制点钳制",
    dependsOn: [2402],
  },
  {
    id: 2404,
    title: "关键帧批量操作",
    duty: "框选/多轨选择/复制粘贴/时间缩放四操作 + 与 F1345 操作语义对齐 + 单步撤销 + 原子事务；不做轨道数据结构",
    producesContract: "BatchEditTransaction",
    criterion: "四操作语义对齐表、批量操作=单撤销步、批量中途失败整体回滚、操作风暴去抖",
    dependsOn: [2402, 2403],
  },
  {
    id: 2405,
    title: "动画曲线资产",
    duty: "m.anim. 开放资产容器（曲线/轨道/clip/元数据四段）+ 生态段注册 + 签名清洗三件套复用 + 版本化三件复用",
    producesContract: "AnimAssetContainer",
    criterion: "m.anim. 段 schema 公开、F1942 生态单点注册、往返导出再导入逐位一致断言",
    dependsOn: [2402],
  },
  {
    id: 2406,
    title: "动画事件轨",
    duty: "第七类离散事件轨 + 触发语义（正播触发/倒播默认不触发）+ 同帧去重合并 + 接入 F1408 总线；不做总线实现",
    producesContract: "AnimEventTrack",
    criterion: "事件轨注册、触发语义表、倒播语义可配、三生产者格局声明（J 光照/H 音频/M 动画）",
    dependsOn: [2402],
  },
  {
    id: 2407,
    title: "动画求值性能",
    duty: "按类型分批 SIMD 求值 + 求值路径零分配 + 脏标记缓存 + 预算超限 LOD 降级；不改求值数学语义",
    producesContract: "EvalPerfModel",
    criterion: "类型分批 SIMD 收益实测、零分配断言（分配追踪=0）、缓存脏标记失效条件、LOD 降级次序先于精度降",
    dependsOn: [2402, 2403],
  },
  {
    id: 2408,
    title: "动画调试数据",
    duty: "曲线可视/当前值流/权重热力三负载 + 三统计量聚合 + F1946 调优协议 + 发行版剔除；不做求值本身",
    producesContract: "AnimDebugPayload",
    criterion: "三负载按需拾取（零常驻）、缓存命中率统计、调优指令 ACK 通道、发行版剔除零成本断言",
    dependsOn: [2402, 2407],
  },
  {
    id: 2409,
    title: "动画导入",
    duty: "glTF 四通道映射（translation/rotation/scale/morph weights）+ F1612 三重校验复用 + 保真默认重采样 + 结构化报告",
    producesContract: "AnimImportReport",
    criterion: "四通道映射表、三重校验（引用完整性/采样合法性/曲线异常）、采样保真默认、结构化三要素报告",
    dependsOn: [2402, 2403],
  },
  {
    id: 2410,
    title: "动画导出",
    duty: "glTF 四通道反向映射 + 导出后回读对拍（往返容差表）+ 精度诚实声明 + 离散轨导出边界标注",
    producesContract: "AnimExportReport",
    criterion: "glTF 双向互操作、往返容差表（浮点 1e-5）、导出精度边界诚实文档、离散轨跳过不静默",
    dependsOn: [2405, 2409],
  },
  {
    id: 2411,
    title: "动画 fuzz",
    duty: "三段模糊测试（畸形轨道/万轨道风暴/畸形 glTF）+ 三不变量（零分配/双跑一致/句柄收敛）+ 案例固化入库",
    producesContract: "AnimFuzzCase",
    criterion: "三段 fuzz 全覆盖、求值分配检出即 P0、导入畸形全部被拦截、24h复现固化",
    dependsOn: [2407, 2409],
  },
  {
    id: 2412,
    title: "动画基准",
    duty: "三族基准（求值吞吐/导入导出吞吐/插值性能）入 F1773 M 段 + SIMD 与标量分批对照采集 + 模型常数回填",
    producesContract: "AnimBenchmarkEntry",
    criterion: "三族基准阶梯实测、SIMD 对照采集、偏差超30% 触发 F2407 修正联动、门禁暂挂声明",
    dependsOn: [2407],
  },
  {
    id: 2413,
    title: "动画 API 冻结 v1",
    duty: "十二签名 v1 冻结（轨道五/clip 三/求值二/导入导出二）+ 与 F1345/I04/F1408 四行衔接核验 + 只增不改",
    producesContract: "AnimApiV1",
    criterion: "十二签名逐条（全名/参数表/返回契约/描述词）、四行衔接核验零偏差、签名漂移 CI 拦截",
    dependsOn: [2402, 2405, 2406, 2409, 2410],
  },
  {
    id: 2414,
    title: "动画文档",
    duty: "三份文档（动画白皮书/轨道制作指南/导入导出手册）+ 与实现漂移的 CI 对账钩子 + 单源复制守卫",
    producesContract: "AnimDocSet",
    criterion: "三文档齐备、四段管线单源引用、术语通俗解释（无障碍替述可读）、漂移 CI 拦截",
    dependsOn: [2413],
  },
  {
    id: 2415,
    title: "动画一致性",
    duty: "求值双跑一致 + 往返复述核验 + F2276 双级口径表 M01 维 + 轨道语义变更 ADR 对",
    producesContract: "AnimConsistencyProof",
    criterion: "六类轨道双跑 diff=0（夜间 CI）、往返断言复述签名、同平台逐位/跨平台不承诺口径、语义变更走 ADR",
    dependsOn: [2403, 2410],
  },
  {
    id: 2416,
    title: "动画安全",
    duty: "双层清洗（导入层 F1612 三重 + 求值层 NaN 防御）+ 双硬顶配额（轨道数/实体默认 256、clip 默认 64MB）+ 操作审计",
    producesContract: "AnimSecurityPolicy",
    criterion: "双层清洗规则表、双硬顶创建入口枚举证明不可绕、操作审计留痕、F2411 通过率快照引用",
    dependsOn: [2409, 2411],
  },
  {
    id: 2417,
    title: "动画质量档",
    duty: "三档（插值精度×LOD 频率×缓存策略双因子）+ 降级画质代价公开 + D04 档位树 M01 段映射 + 档位记忆",
    producesContract: "AnimQualityTier",
    criterion: "双因子三档参数表、降级代价可预期、档位树 M01 段实例（F2015 家族）、切换在帧边界原子生效",
    dependsOn: [2407],
  },
  {
    id: 2418,
    title: "动画遥测",
    duty: "四指标注册（轨道数分布/求值耗时占比/导入失败率/事件触发频率）进总日志中心 M 段 + 匿名计数档 + 口径唯一",
    producesContract: "AnimTelemetrySet",
    criterion: "四指标五元组合规、占比分母口径唯一（求值耗时/总帧耗时）、100ms 去抖、全指标匿名档",
    dependsOn: [2406, 2407, 2409],
  },
  {
    id: 2419,
    title: "动画组一致性",
    duty: "与 I04/L07 术语一致（蒙皮归 I、动画归 M 的边界复述）+ 与 F1345 轨道词汇统一 + 错误三要素抽样",
    producesContract: "AnimTermTable",
    criterion: "术语对照表唯一义、边界复述与F2401 边界表 diff=0、跨域术语统一、三要素抽查 8 条",
    dependsOn: [2401, 2415],
  },
  {
    id: 2420,
    title: "M01 组收口与 M02 移交",
    duty: "20 项自检入树 + 三件硬证确认（F2412 基准全过/F2415 双跑全过/F2411 fuzz 100%）+ 组双签 + 向 M02 移交轨道契约",
    producesContract: "M01HandoverPack",
    criterion: "20 项自检全绿、三件硬证齐备、组双签、轨道契约四件（规格表/求值接口/绑定协议/事件语义）移交 M02",
    dependsOn: [2411, 2412, 2413, 2415, 2416, 2419],
  },
];

/** M01 分工条数（恰为 20，缺一条即开工未完成）。 */
export const M01_WORK_ITEM_COUNT = 20;

/**
 * 分工齐备审计：机检「20 条齐备 + 条目号连续 + 契约名唯一 + 依赖无悬空」。
 *
 * 为什么要查「依赖无悬空」而不只查条数（这是最容易被漏掉的一项）：
 *   一份 20 条齐备但有悬空依赖（某条 dependsOn 指向不存在的条目号）的分工表，
 *   从条数上看是完美的，从排产上看是**死锁**的：那条依赖的条目永远不会被
 *   排进来，而依赖它的条目在等。真正的表现是开工到一半停住，且看不出为什么。
 *   故此处把依赖闭合作为硬检查。
 */
export function auditWorkBreakdown(bag: DiagBag): Outcome<number> {
  const items = M01_WORK_ITEMS;

  // ① 条数恰为 20。
  if (items.length !== M01_WORK_ITEM_COUNT) {
    bag.push(
      "WORKBREAKDOWN_INCOMPLETE",
      `M01 分工为 ${String(items.length)} 条，应为 ${String(M01_WORK_ITEM_COUNT)} 条`,
      `册内 M01 批次（F2401~F2420）恰20 项。缺条意味着有册内条目无人认领，`
        + `多条意味着有册外条目混入——两者都会让组级收口的双签失去意义`,
    );
  }

  const ids = new Set<number>();

  // ② 条目号重复。
  for (const it of items) {
    if (ids.has(it.id)) {
      bag.push(
        "WORKBREAKDOWN_INCOMPLETE",
        `分工表条目号 F${String(it.id)} 重复`,
        "条目号是分工表的唯一键，重复会让「这条谁做」无解",
      );
    }
    ids.add(it.id);
  }

  // ③ 条目号连续：F2401 ~ F2420。
  const sorted = items.map((i) => i.id).sort((a, b) => a - b);
  for (let i = 0; i < sorted.length; i += 1) {
    const expectedId = 2401 + i;
    if (sorted[i] !== expectedId) {
      bag.push(
        "WORKBREAKDOWN_INCOMPLETE",
        `分工表条目号不连续：第 ${String(i + 1)} 项为 F${String(sorted[i] ?? 0)}，应为 F${String(expectedId)}`,
        "F2401~F2420 必须逐号连续。跳号说明有条目被漏写，它的能力将无人实现",
      );
    }
  }

  // ④ 产出契约名唯一。
  const contractNames = new Set<string>();
  for (const it of items) {
    if (it.producesContract.trim().length === 0) {
      bag.push(
        "WORKBREAKDOWN_INCOMPLETE",
        `条目 F${String(it.id)}（${it.title}）未声明产出契约名`,
        "每条都要写明产出什么契约——下游条目靠契约名对齐，空名等于无产出",
      );
    }
    if (contractNames.has(it.producesContract)) {
      bag.push(
        "WORKBREAKDOWN_INCOMPLETE",
        `产出契约名「${it.producesContract}」被多条分工重复声明`,
        "契约名全局唯一。重复会让下游无法判断该消费哪一条的产出",
      );
    }
    contractNames.add(it.producesContract);
  }

  // ⑤ 要素齐备（职责/判据非空）。
  for (const it of items) {
    if (it.duty.trim().length === 0 || it.criterion.trim().length === 0) {
      bag.push(
        "WORKBREAKDOWN_INCOMPLETE",
        `条目 F${String(it.id)}（${it.title}）的职责或判据为空`,
        "职责须写明「不负责什么」，判据须可核验。任一为空则该条无法被验收",
      );
    }
  }

  // ⑥ 依赖无悬空（依赖的条目号必须在表内，且不得自依赖）。
  for (const it of items) {
    for (const d of it.dependsOn) {
      if (!ids.has(d)) {
        bag.push(
          "WORKBREAKDOWN_INCOMPLETE",
          `条目 F${String(it.id)}（${it.title}）依赖不存在的条目 F${String(d)}`,
          "悬空依赖会让排产死锁：等的那条永远不会被排进来。依赖只能指向本表内的条目号",
        );
      }
      if (d === it.id) {
        bag.push(
          "WORKBREAKDOWN_INCOMPLETE",
          `条目 F${String(it.id)} 依赖自身`,
          "自依赖是排产死锁的另一种形式：请删除自依赖",
        );
      }
    }
  }

  if (bag.hasAny) {
    const first = bag.all()[0];
    return fail(
      first?.code ?? "WORKBREAKDOWN_INCOMPLETE",
      first?.message ?? "分工审计未通过",
      first?.hint ?? "按上述诊断逐条修正",
    );
  }
  return ok(items.length, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §7 批次区间审计 + 开工门禁 + 开工摘要（判据一至四的落地编排）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 批次区间审计：不重叠、无空隙、合计 200、边界与域区间吻合。
 *
 * 为什么「无空隙」和「不重叠」要分开判（两种缺陷症状不同，合并会丢失归因）：
 *   · 重叠 = 两个批次抢同一批功能号 → 同一件事被做两遍，两份实现会分叉；
 *   · 空隙 = 有功能号没归任何批次 → 册内那条能力无人实现，收官对账会缺项。
 *   二者的处置方向相反（重叠要合并，空隙要补批次），所以不能合成一句话。
 */
export function auditBatchRanges(bag: DiagBag): Outcome<number> {
  const sorted = BATCH_REGISTRY.slice().sort((a, b) => a.itemRange.lo - b.itemRange.lo);
  let total = 0;
  let overlap = "";
  let gap = "";

  for (const b of BATCH_REGISTRY) {
    const { lo, hi } = b.itemRange;
    if (lo > hi) {
      bag.push(
        "ITEM_RANGE_OVERLAP",
        `批次 ${b.id} 的功能号区间倒置（${String(lo)} > ${String(hi)}）`,
        "区间须满足 lo ≤ hi；倒置区间会让区间审计的后续比较全部失去意义",
      );
    }
    total += hi - lo + 1;
  }

  for (let i = 0; i < sorted.length; i += 1) {
    const cur = sorted[i];
    const next = sorted[i + 1];
    if (cur === undefined || next === undefined) continue;
    if (cur.itemRange.hi >= next.itemRange.lo) {
      overlap = `批次 ${cur.id}（止于 F${String(cur.itemRange.hi)}）与批次 ${next.id}`
        + `（起于 F${String(next.itemRange.lo)}）区间重叠`;
      bag.push(
        "ITEM_RANGE_OVERLAP",
        overlap,
        "同一功能号被两个批次抢，会出现两份实现互相分叉。请合并区间或重划边界",
      );
    }
    if (cur.itemRange.hi + 1 !== next.itemRange.lo) {
      const missingFrom = cur.itemRange.hi + 1;
      const missingTo = next.itemRange.lo - 1;
      gap = `批次 ${cur.id} 与 ${next.id} 之间存在未分配功能号`
        + `（F${String(missingFrom)}~F${String(missingTo)}）`;
      bag.push(
        "ITEM_RANGE_GAP",
        gap,
        "这些功能号在册内存在但不属于任何批次，收官对账时会缺项。请补一个批次或调整区间",
      );
    }
  }

  // 合计与域边界。
  const first = sorted[0];
  const last = sorted[sorted.length - 1];
  if (first !== undefined && first.itemRange.lo !== DOMAIN.itemLo) {
    bag.push(
      "ITEM_RANGE_GAP",
      `首批次 ${first.id} 起于 F${String(first.itemRange.lo)}，域起点为 F${String(DOMAIN.itemLo)}`,
      `批次区间必须从域起点 F${String(DOMAIN.itemLo)} 开始铺满`,
    );
  }
  if (last !== undefined && last.itemRange.hi !== DOMAIN.itemHi) {
    bag.push(
      "ITEM_RANGE_GAP",
      `末批次 ${last.id} 止于 F${String(last.itemRange.hi)}，域终点为 F${String(DOMAIN.itemHi)}`,
      `批次区间必须铺满到域终点 F${String(DOMAIN.itemHi)}`,
    );
  }
  if (total !== DOMAIN.itemCount) {
    bag.push(
      "ITEM_RANGE_GAP",
      `批次区间合计 ${String(total)} 项，域声明 ${String(DOMAIN.itemCount)} 项`,
      `区间合计必须等于 ${String(DOMAIN.itemCount)}（F${String(DOMAIN.itemLo)}~F${String(DOMAIN.itemHi)}）`,
    );
  }

  if (bag.hasAny) {
    const firstDiag = bag.all()[0];
    return fail(
      firstDiag?.code ?? "ITEM_RANGE_GAP",
      overlap !== "" ? overlap : gap !== "" ? gap : (firstDiag?.message ?? "批次区间审计未通过"),
      firstDiag?.hint ?? "按上述诊断逐条修正",
    );
  }
  return ok(total, bag.all());
}

/** 开工门禁一条：条件 + 它对应哪条判据。 */
export interface GateCriterion {
  readonly id: string;
  readonly criterion: string;
  /** 该门禁对应的判据（映射到四条判据之一）。 */
  readonly mapsTo: "域开工" | "四段管线" | "三域边界" | "契约接收";
}

/** 开工门禁清单（逐条可机检；任一不成立即开工未完成）。 */
export const KICKOFF_GATES: readonly GateCriterion[] = [
  { id: "G1", criterion: "官方十主题全部登记且与册内序列逐项一致，能力主题均被实现批次承载", mapsTo: "域开工" },
  { id: "G2", criterion: "十批次功能号区间两两不重叠、合计恰为 200 项、无未分配空隙", mapsTo: "域开工" },
  { id: "G3", criterion: "M01 组 20 条分工齐备（条目号连续无重复、产出契约名唯一、依赖无悬空）", mapsTo: "域开工" },
  { id: "G4", criterion: "四段管线规格自洽（序位恰为 1~4、段间契约首尾相接、第四段归属 I 域）", mapsTo: "四段管线" },
  { id: "G5", criterion: "执行序断言在位：四段齐备但顺序错乱亦判失败（不只查缺段）", mapsTo: "四段管线" },
  { id: "G6", criterion: "三域边界表可机检（能力名不重复、每能力恰有唯一 owner、要素齐备）", mapsTo: "三域边界" },
  { id: "G7", criterion: "边界越界显性拦截：M 域做蒙皮等越权一律拒绝并指路交接契约", mapsTo: "三域边界" },
  { id: "G8", criterion: "双向驱动无环 + 帧内序冻结（M↔L 衔接中语义允许延迟的边已延后一跳）", mapsTo: "三域边界" },
  { id: "G9", criterion: "移交包已接收：链首 K 与上游 L 均收官、快照哈希回放一致、义务逐条确认", mapsTo: "契约接收" },
  { id: "G10", criterion: "移交禁令可机检：命中越权短描述的操作被显性拦下", mapsTo: "契约接收" },
  { id: "G11", criterion: "零静默：所有拒绝/越界/成环/阻断均产出三要素诊断，无静默分支", mapsTo: "契约接收" },
];

/** 域开工摘要（对外一页纸：进度 + 门禁 + 阻塞，AI 派单与看板共用）。 */
export interface DomainKickoffSummary {
  readonly domain: string;
  readonly themeCount: number;
  readonly batchCount: number;
  readonly itemRange: string;
  readonly itemCount: number;
  readonly pipelineSequence: readonly string[];
  readonly pipelineOwnerByStage: Readonly<Record<string, string>>;
  readonly boundaryOwners: readonly { readonly capability: string; readonly owner: DomainId }[];
  readonly handoverContractId: string;
  readonly contractChain: readonly string[];
  readonly m01WorkItemCount: number;
  readonly gates: readonly GateCriterion[];
}

/** 契约链的对外表述（K→L→M，供文档与看板引用同一事实源）。 */
export const CONTRACT_CHAIN: readonly string[] = [
  "VE-K 后处理链（链首 · F2200 收官宣告生效后其序总图冻结）",
  "VE-L 粒子与物理（K→L 序契约 F2148 序位登记 + L 域转写层）",
  "VE-M 动画系统（本域 · 消费 L 域转写契约，不直取 K 域语义）",
];

/** 无障碍替述：把四段管线的架构用不含术语的人话讲一遍（§8 引用）。 */
export const PIPELINE_PLAIN_LANGUAGE =
  "动画一帧要走四步，就像给骨架摆姿势的四个环节："
  + "第一步查表（轨道求值）——按时间在关键帧之间取出每个通道的数值；"
  + "第二步摆姿势（clip 采样）——把这些数值拼成一具完整的骨架姿势，旋转要用四元数插值避免画面歪斜；"
  + "第三步揉姿势（姿态混合）——把跑步、瞄准、眨眼这几具姿势按权重揉成一具；"
  + "第四步贴皮（骨骼应用）——用最终姿势去带动网格顶点，这一环节归渲染域管，动画域只交出姿势。"
  + "四步的顺序不能换：换了就会出现「画面慢一帧」或「同一段动画每次播出来略有不同」的问题。";

/** 生成域开工摘要。任一前置不成立即返回显性失败（开工单不允许带未知态）。 */
export function buildKickoffSummary(): Outcome<DomainKickoffSummary> {
  const bag = new DiagBag();

  // 前置 1：主题覆盖。
  const coverage = auditThemeCoverage(bag);
  // 前置 2：批次区间。
  const batchCheck = auditBatchRanges(bag);
  // 前置 3：管线规格自洽。
  const pipeCheck = auditPipelineSpec(bag);
  // 前置 4：边界表自洽。
  const boundCheck = auditBoundaryTable(bag);
  // 前置 5：驱动无环。
  const cycleCheck = detectDriveCycles(bag);
  // 前置 6：20 条分工齐备。
  const workCheck = auditWorkBreakdown(bag);

  // 逐项判失败并立即返回：写作显式的逐项判定而非聚合成数组，
  // 是为了让每一步的失败分支在类型层已收窄——读者不需要跟着一个
  // as 断言去确认「这六个值里到底有没有成功的」。
  if (!coverage.ok) {
    return fail<DomainKickoffSummary>(coverage.code, coverage.message, coverage.hint);
  }
  if (!batchCheck.ok) {
    return fail<DomainKickoffSummary>(batchCheck.code, batchCheck.message, batchCheck.hint);
  }
  if (!pipeCheck.ok) {
    return fail<DomainKickoffSummary>(pipeCheck.code, pipeCheck.message, pipeCheck.hint);
  }
  if (!boundCheck.ok) {
    return fail<DomainKickoffSummary>(boundCheck.code, boundCheck.message, boundCheck.hint);
  }
  if (!cycleCheck.ok) {
    return fail<DomainKickoffSummary>(cycleCheck.code, cycleCheck.message, cycleCheck.hint);
  }
  if (!workCheck.ok) {
    return fail<DomainKickoffSummary>(workCheck.code, workCheck.message, workCheck.hint);
  }

  const itemTotal = BATCH_REGISTRY.reduce((n, b) => n + (b.itemRange.hi - b.itemRange.lo + 1), 0);
  const ownerByStage: Record<string, string> = {};
  for (const s of PIPELINE_SEQUENCE) {
    const spec = PIPELINE_STAGES[s];
    ownerByStage[spec.name] = spec.ownerDomain;
  }

  return ok(
    {
      domain: `${DOMAIN.id} ${DOMAIN.name}（${String(DOMAIN.itemLo)}-${String(DOMAIN.itemHi)}，共 ${String(DOMAIN.itemCount)} 项，${DOMAIN.wave} 波）`,
      themeCount: THEME_IDS.length,
      batchCount: BATCH_REGISTRY.length,
      itemRange: `${String(DOMAIN.itemLo)} ~ ${String(DOMAIN.itemHi)}`,
      itemCount: itemTotal,
      pipelineSequence: PIPELINE_SEQUENCE.map((s) => PIPELINE_STAGES[s].name),
      pipelineOwnerByStage: ownerByStage,
      boundaryOwners: BOUNDARY_TABLE.map((b) => ({ capability: b.capability, owner: b.owner })),
      handoverContractId: L_TO_M_HANDOVER.id,
      contractChain: CONTRACT_CHAIN,
      m01WorkItemCount: M01_WORK_ITEMS.length,
      gates: KICKOFF_GATES,
    },
    bag.all(),
  );
}

/**
 * 开工编排：接收移交包 +跑全部审计，产出可入账本的开工凭据。
 *
 * 为什么编排层要在接收之后（而不是与接收并行）：移交包的义务之一（OB-01
 * 帧内序）只有在帧内序检查器就位后才谈得上「遵守」。先接包后建序，
 * 会让回执上的「已确认遵守帧内序」成为一句没有对应实现的空话。
 */
export function kickoffDomain(
  receipt: HandoverReceipt,
  bag: DiagBag,
): Outcome<DomainKickoffSummary> {
  const received = receiveHandover(receipt, bag);
  if (!received.ok) {
    return fail<DomainKickoffSummary>(
      received.code,
      `移交包接收未通过，M 域开工阻断：${received.message}`,
      `${received.hint}。未签收移交包即开工，后续 19 条的架构前提全部悬空——`
        + "故此处阻断而非降级",
    );
  }

  const summary = buildKickoffSummary();
  if (!summary.ok) {
    return fail<DomainKickoffSummary>(
      summary.code,
      `M 域架构审计未通过，开工阻断：${summary.message}`,
      summary.hint,
    );
  }
  return ok(summary.value, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §8 自检（判据的可执行形态：不靠人读代码确认，靠断言输出）
// ════════════════════════════════════════════════════════════════════════════

/** 一条自检结果（逐项可读，不聚合成单一布尔）。 */
export interface SelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/**
 * 造一份合法回执（供自检与后续调用方做正向路径验证）。
 *
 * receiver 默认值取「M 域」而非某个工人编号：本函数是域级契约的一部分，
 * 样例回执的接收方语义是「M 域已接收」，把它绑到某个产线工人编号上会让
 * 样例随排班漂移（且工人编号不是本域的任何语义）。需要指定具体签署方时
 * 由调用方显式传入。
 */
export function sampleReceipt(receiver = "VE-M"): HandoverReceipt {
  return {
    contractId: L_TO_M_HANDOVER.id,
    receiver,
    acceptedObligationIds: L_TO_M_HANDOVER.obligations.map((o) => o.id),
    verifiedSnapshotHash: L_TO_M_HANDOVER.snapshotHash,
    upstreamDeclared: true,
    chainHeadDeclared: true,
  };
}

/** 判据「域开工」自检：十主题/十批次/20 条分工。 */
export function selfCheckDomainKickoff(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 态1：十主题登记且与册内序列逐字一致。
  const names = THEME_IDS.map((id) => THEME_REGISTRY[id].name);
  out.push({
    name: "ten-themes-registered",
    pass: names.length === 10 && names.every((n, i) => n === OFFICIAL_THEMES[i]),
    detail:
      names.length === 10 && names.every((n, i) => n === OFFICIAL_THEMES[i])
        ? `十主题与册内序列逐项一致：${names.join("、")}`
        : `十主题登记与册内序列不一致：登记为 ${names.join("、")}，册内为 ${OFFICIAL_THEMES.join("、")}`,
  });

  // 态2：主题覆盖审计通过。
  const b2 = new DiagBag();
  const r2 = auditThemeCoverage(b2);
  out.push({
    name: "theme-coverage-audit",
    pass: r2.ok,
    detail: r2.ok
      ? `主题覆盖审计通过，${String(r2.value.length)} 个能力主题均被实现批次承载`
      : `主题覆盖审计失败：${r2.message}`,
  });

  // 态3：批次区间审计通过（合计 200、无重叠、无空隙）。
  const b3 = new DiagBag();
  const r3 = auditBatchRanges(b3);
  out.push({
    name: "batch-ranges-audit",
    pass: r3.ok && r3.value === DOMAIN.itemCount,
    detail: r3.ok
      ? `十批次区间合计 ${String(r3.value)} 项，与域声明一致且无重叠无空隙`
      : `批次区间审计失败：${r3.message}`,
  });

  // 态4：分工 20 条齐备。
  const b4 = new DiagBag();
  const r4 = auditWorkBreakdown(b4);
  out.push({
    name: "m01-workbreakdown-audit",
    pass: r4.ok && r4.value === M01_WORK_ITEM_COUNT,
    detail: r4.ok
      ? `M01 分工 ${String(r4.value)} 条齐备，条目号 F2401~F2420 连续、契约名唯一、依赖无悬空`
      : `M01 分工审计失败：${r4.message}`,
  });

  // 态5：组关口条目在表内（末条为收口移交）。
  const gate = M01_WORK_ITEMS.find((w) => w.id === 2420);
  out.push({
    name: "m01-gate-item-present",
    pass: gate !== undefined && gate.producesContract === "M01HandoverPack",
    detail:
      gate !== undefined
        ? `组关口条目 F2420（${gate.title}）在表内，产出 ${gate.producesContract}`
        : "组关口条目 F2420 缺失",
  });

  return out;
}

/** 判据「四段管线」自检：规格自洽 + 序断言的正反两面。 */
export function selfCheckPipeline(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 规格态。
  const b1 = new DiagBag();
  const r1 = auditPipelineSpec(b1);
  out.push({
    name: "pipeline-spec-audit",
    pass: r1.ok,
    detail: r1.ok
      ? `四段规格自洽：序位 1~4 唯一、段间契约首尾相接、第四段归属 ${PIPELINE_STAGES["bone-apply"].ownerDomain}`
      : `四段规格审计失败：${r1.message}`,
  });

  // 序态1：冻结序被接受。
  const r2 = assertPipelineSequence(PIPELINE_SEQUENCE, new DiagBag());
  out.push({
    name: "pipeline-frozen-seq-accepted",
    pass: r2.ok,
    detail: r2.ok
      ? `冻结序被接受：${PIPELINE_SEQUENCE.map((s) => PIPELINE_STAGES[s].name).join(" → ")}`
      : `冻结序竟被拒：${r2.message}`,
  });

  // 序态2：四段齐备但顺序错乱 → 必须被拦（这是最隐蔽的一种）。
  const b3 = new DiagBag();
  const r3 = assertPipelineSequence(
    ["clip-sample", "track-eval", "pose-blend", "bone-apply"],
    b3,
  );
  out.push({
    name: "pipeline-rejects-swapped-order",
    pass: !r3.ok && r3.code === "PIPELINE_SEQUENCE_BROKEN",
    detail: !r3.ok
      ? `四段齐备但①② 互换被拦下（${r3.code}）——这是最难归因的序破坏形态`
      : "①② 互换竟被放行（权重语义会从姿态权重退化为轨道权重）",
  });

  // 序态3：缺段被拦。
  const r4 = assertPipelineSequence(["track-eval", "clip-sample"], new DiagBag());
  out.push({
    name: "pipeline-rejects-missing-stage",
    pass: !r4.ok && r4.code === "PIPELINE_SEQUENCE_BROKEN",
    detail: !r4.ok ? `缺段被拦下（${r4.code}）` : "缺段竟被放行（画面会缺一层）",
  });

  // 序态4：未登记的段被拦。
  // 这里必须绕过类型系统：调用方（尤其是跨域调用方）拿到的段 id 未必经过
  // 编译期校验，越界段在运行期就是「一个不在封闭集里的字符串」。
  // 若写成合法字面量，这条断言就变成同义反复（类型系统已经拦过了，测不到
  // 运行期的 PIPELINE_STAGE_UNKNOWN 分支）。
  const rogueStage = "retarget" as PipelineStageId;
  const r5 = assertPipelineSequence(
    ["track-eval", "clip-sample", "pose-blend", "bone-apply", rogueStage],
    new DiagBag(),
  );
  out.push({
    name: "pipeline-rejects-unregistered-stage",
    pass: !r5.ok && r5.code === "PIPELINE_STAGE_UNKNOWN",
    detail: !r5.ok
      ? `私自新增的第五段被拦下（${r5.code}）——加段须走 ADR 修订冻结序`
      : "私自新增的段竟被放行（各实现对管线有几步的理解会分叉）",
  });

  // 归属态：第四段归 I 域（蒙皮归 I 是三域边界的核心投影）。
  const apply = PIPELINE_STAGES["bone-apply"];
  out.push({
    name: "pipeline-bone-apply-owned-by-I",
    pass: apply.ownerDomain === "VE-I" && apply.outputContract === "SkinnedVertexBuffer",
    detail: `第 4 段「${apply.name}」归属 ${apply.ownerDomain}，输出 ${apply.outputContract}（M 域不越界做蒙皮）`,
  });

  return out;
}

/** 判据「三域边界」自检：边界表 + 越界拦截 + 环检测 + 帧内序。 */
export function selfCheckBoundary(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 边界表自洽。
  const b1 = new DiagBag();
  const r1 = auditBoundaryTable(b1);
  out.push({
    name: "boundary-table-audit",
    pass: r1.ok,
    detail: r1.ok
      ? `边界表自洽：${String(BOUNDARY_TABLE.length)} 项能力，每项恰有唯一 owner，三要素齐备`
      : `边界表审计失败：${r1.message}`,
  });

  // 越界态1：M 域做蒙皮 → 拦截（判据三的第一条错误路径）。
  const b2 = new DiagBag();
  const r2 = checkBoundary("VE-M", "蒙皮与骨骼变换应用", b2);
  out.push({
    name: "boundary-blocks-m-domain-skinning",
    pass: !r2.ok && r2.code === "BOUNDARY_VIOLATION",
    detail: !r2.ok
      ? `M 域执行蒙皮被拦下（${r2.code}），提示指路姿态缓冲契约`
      : "M 域做蒙皮竟被放行（会出现同一批顶点两套蒙皮结果）",
  });

  // 越界态2：未登记能力 → 拦截（与越界是不同的处置动作，故单独验）。
  const r3 = checkBoundary("VE-M", "顶点着色器代码生成", new DiagBag());
  out.push({
    name: "boundary-blocks-unregistered-capability",
    pass: !r3.ok && r3.code === "BOUNDARY_CAPABILITY_UNREGISTERED",
    detail: !r3.ok
      ? `未登记能力被拦下（${r3.code}）——它既无 owner 也无交接契约`
      : "未登记能力竟被放行（这段代码没有任何域为它的正确性负责）",
  });

  // 越界态3：合法调用不被误拦（反向验证——误拦会逼调用方绕过检查器）。
  const r4 = checkBoundary("VE-M", "关键帧轨道与插值", new DiagBag());
  out.push({
    name: "boundary-allows-legal-claim",
    pass: r4.ok,
    detail: r4.ok ? "M 域对本域能力（轨道与插值）的合法声明未被误拦" : `合法声明被误拦：${r4.message}`,
  });

  // 环检测态：M↔L 双向衔接成环但已被 deferred 断开 → 判为可接受（备查）。
  const r5 = detectDriveCycles(new DiagBag());
  const brokenCycles = r5.ok ? r5.value.filter((c) => c.verdict === "broken") : [];
  out.push({
    name: "drive-graph-no-fatal-cycle",
    pass: r5.ok && brokenCycles.length > 0,
    detail: r5.ok
      ? `M↔L 双向衔接成环但无同帧可闭合的致命环：`
        + `${String(brokenCycles.length)} 个环已被 deferred 边断开（`
        + `${brokenCycles.map((c) => c.brokenBy ?? "未标注").join("；")}）——`
        + "双向衔接本身必然成环，靠延迟一跳断开是架构上的正确做法"
      : `检出同帧可闭合的致命环：${r5.message}`,
  });

  // 环检测态（对抗验证）：把断开边改回 direct，必须立刻被判为致命环。
  // 这一项是防止「检测器对自家边表失灵」——若它对 direct+direct 也报无环，
  // 那么上面那条PASS 只是因为边表恰好是对的，检测能力本身并未被验证。
  const directEdges: readonly DriveEdge[] = DRIVE_EDGES.map((e) => ({ ...e, mode: "direct" }));
  const fatalProbes = findCycles(directEdges).filter((c) => c.verdict === "fatal");
  out.push({
    name: "drive-detector-catches-direct-cycle",
    pass: fatalProbes.length > 0,
    detail:
      fatalProbes.length > 0
        ? `对抗验证通过：把断开边改回 mode=direct 后检出 ${String(fatalProbes.length)} 个`
          + "同帧可闭合环（检测能力有效，不是边表恰好正确）"
        : "对抗验证失败：两条边都设为 direct 竟未检出环——环检测器本身失效",
  });

  // 帧内序态1：冻结序被接受。
  const r6 = checkFrameOrder(FRAME_ORDER, new DiagBag());
  out.push({
    name: "frame-order-frozen-accepted",
    pass: r6.ok,
    detail: r6.ok ? `帧内序被接受：${FRAME_ORDER.join(" → ")}` : `帧内序竟被拒：${r6.message}`,
  });

  // 帧内序态2：物理触发跑到动画更新之前 → 拦截。
  const b7 = new DiagBag();
  const r7 = checkFrameOrder(
    ["event-check", "anim-update", "physics-trigger", "physics-step"],
    b7,
  );
  out.push({
    name: "frame-order-rejects-swapped",
    pass: !r7.ok && r7.code === "FRAME_ORDER_VIOLATION",
    detail: !r7.ok
      ? `事件检查与动画更新互换被拦下（${r7.code}）——次序错误即用错前帧状态`
      : "帧内序互换竟被放行（物理触发会拿到动画更新前的姿态）",
  });

  return out;
}

/** 判据「契约接收」自检：正向接收 + 四种失败形态 + 禁令拦截。 */
export function selfCheckHandover(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 正向态：合法回执被接收。
  const b1 = new DiagBag();
  const r1 = receiveHandover(sampleReceipt(), b1);
  out.push({
    name: "handover-receives-valid-receipt",
    pass: r1.ok && b1.size === 0,
    detail: r1.ok
      ? `合法回执被接收（${L_TO_M_HANDOVER.obligations.length} 条义务逐条确认，零诊断）`
      : `合法回执竟被拒：${r1.message}`,
  });

  // 失败态1：链首 K 未收官 → 阻断（这一条排在最前）。
  const bad1 = { ...sampleReceipt(), chainHeadDeclared: false };
  const r2 = receiveHandover(bad1, new DiagBag());
  out.push({
    name: "handover-blocks-unclosed-chain-head",
    pass: !r2.ok && r2.code === "CONTRACT_CHAIN_HEAD_UNCLOSED",
    detail: !r2.ok
      ? `K 域未收官被拦下（${r2.code}）——签一张随时作废的纸没有意义`
      : "K 域未收官竟被接受",
  });

  // 失败态2：上游 L 未收官 → 阻断。
  const bad2 = { ...sampleReceipt(), upstreamDeclared: false };
  const r3 = receiveHandover(bad2, new DiagBag());
  out.push({
    name: "handover-blocks-unclosed-upstream",
    pass: !r3.ok && r3.code === "UPSTREAM_NOT_CLOSED",
    detail: !r3.ok ? `L 域未收官被拦下（${r3.code}）` : "L 域未收官竟被接受（移交包尚未成包）",
  });

  // 失败态3：哈希失真 → 拒绝。
  const bad3 = { ...sampleReceipt(), verifiedSnapshotHash: "0000000000000000deadbeefdeadbeef" };
  const r4 = receiveHandover(bad3, new DiagBag());
  out.push({
    name: "handover-rejects-hash-mismatch",
    pass: !r4.ok && r4.code === "HANDOVER_HASH_MISMATCH",
    detail: !r4.ok
      ? `快照哈希失真被拒（${r4.code}）——签的是哪一版移交件必须可追溯`
      : "哈希失真竟被接受",
  });

  // 失败态4：义务缺条 → 拒收并点名。
  const bad4 = { ...sampleReceipt(), acceptedObligationIds: ["OB-01"] };
  const r5 = receiveHandover(bad4, new DiagBag());
  out.push({
    name: "handover-rejects-missing-obligation",
    pass: !r5.ok && r5.code === "HANDOVER_OBLIGATION_MISSING",
    detail: !r5.ok
      ? `义务缺 ${String(L_TO_M_HANDOVER.obligations.length - 1)} 条被拒收（${r5.code}）`
      : "义务缺条竟被接受（「已知悉」不是确认）",
  });

  // 禁令态：M 域自行蒙皮 → 拦下。
  const b6 = new DiagBag();
  const r6 = checkHandoverCompliance("M 域自行实现蒙皮直接写顶点", b6);
  out.push({
    name: "handover-forbids-m-domain-skinning",
    pass: !r6.ok && r6.code === "HANDOVER_UNSIGNED",
    detail: !r6.ok
      ? `M 域自行蒙皮被移交禁令拦下（${r6.code}）`
      : "M 域自行蒙皮竟被放行（移交义务 OB-04形同虚设）",
  });

  // 禁令态（反向）：合法操作不被误拦。
  const r7 = checkHandoverCompliance("按姿态缓冲契约向 I04 提交混合后的姿态", new DiagBag());
  out.push({
    name: "handover-allows-legal-operation",
    pass: r7.ok,
    detail: r7.ok ? "按契约提交姿态的合法操作未被误拦（检查器不制造假阳性）" : `合法操作被误拦：${r7.message}`,
  });

  // 契约要素态。
  const c = L_TO_M_HANDOVER;
  out.push({
    name: "handover-contract-elements-complete",
    pass:
      c.snapshotHash.trim().length > 0
      && c.obligations.length > 0
      && c.forbidden.length > 0
      && c.obligations.every((o) => o.duty.trim().length > 0 && o.verify.trim().length > 0),
    detail:
      `契约要素齐备：快照哈希已声明、义务 ${String(c.obligations.length)} 条（每条含核验方式）、`
      + `禁令 ${String(c.forbidden.length)} 条、契约链头 ${c.chainHeadDomain} → ${c.upstreamDomain} → VE-M`,
  });

  return out;
}

/** 零静默自检：诊断三要素齐备（每条诊断都必须含人话描述与可操作提示）。 */
export function selfCheckZeroSilence(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 采集若干条失败路径的诊断，逐条检三要素。
  const bags: DiagBag[] = [
    new DiagBag(),
    new DiagBag(),
    new DiagBag(),
    new DiagBag(),
  ];
  checkBoundary("VE-M", "蒙皮与骨骼变换应用", bags[0] as DiagBag);
  receiveHandover({ ...sampleReceipt(), chainHeadDeclared: false }, bags[1] as DiagBag);
  assertPipelineSequence(["pose-blend", "track-eval", "clip-sample", "bone-apply"], bags[2] as DiagBag);
  checkHandoverCompliance("M 域自行实现蒙皮直接写顶点", bags[3] as DiagBag);

  let total = 0;
  const bad: string[] = [];
  for (const b of bags) {
    for (const d of b.all()) {
      total += 1;
      if (d.message.trim().length === 0 || d.hint.trim().length === 0) {
        bad.push(d.code);
      }
      if (/^E\d{3}|^Error:/.test(d.message)) {
        bad.push(`${d.code}(裸异常码)`);
      }
    }
  }
  out.push({
    name: "zero-silence-three-element-diagnostics",
    pass: bad.length === 0 && total > 0,
    detail:
      bad.length === 0
        ? `采样 ${String(total)} 条失败路径诊断，三要素齐备且无裸异常码`
        : `以下诊断要素不齐或含裸异常码：${bad.join("、")}`,
  });

  // 空描述被规范化（上游写半截诊断时不至于漏报）。
  const b5 = new DiagBag();
  b5.push("KICKOFF_BLOCKED", "", "");
  const normalized = b5.all()[0];
  out.push({
    name: "zero-silence-normalizes-empty-diagnostic",
    pass:
      normalized !== undefined
      && normalized.message === "（未提供描述）"
      && normalized.hint === "（未提供处置建议）",
    detail: "空 message/hint 被规范化为占位文本，不会产出半截诊断",
  });

  return out;
}

/** 全量自检入口：一次跑完四组判据对应的全部检查项，返回逐项结果（不聚合为单一布尔）。 */
export function runSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly SelfCheck[]>>;
  readonly allPass: boolean;
  readonly total: number;
  readonly failed: readonly string[];
} {
  const groups = {
    kickoff: selfCheckDomainKickoff(),
    pipeline: selfCheckPipeline(),
    boundary: selfCheckBoundary(),
    handover: selfCheckHandover(),
    zeroSilence: selfCheckZeroSilence(),
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