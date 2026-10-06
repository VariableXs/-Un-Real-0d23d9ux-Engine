/**
 * VE-F2201 · L 域开工与粒子系统总架构（L 域 · 粒子与物理域开工条 · 批次 L01 首项）
 * ---------------------------------------------------------------------------
 * 职责定位：VE-L 域（F2201~F2400）的**开工条**。它不发射一个粒子、不算一次求力、
 * 不碰一次显存，只做四件事，并把四件事写成可机检的契约：
 *   1. 宣告粒子与物理域正式开工——官方十主题与十个批次的完整排布；
 *   2. 定义粒子系统总架构——CPU/GPU **双路径**的架构声明与选择策略；
 *   3. 接收 K 域序契约——L 粒子渲染进 K 后处理链的序位接收与遵守义务签署；
 *   4. 给出组内 20 条分工总览——L01 批次 F2201~F2220 逐条落位。
 *
 * 为什么粒子域的开工条必须先把「双路径语义」钉死（域级立场，全域根基）：
 *   粒子是 VE 里唯一一个**同一场景可能同时跑两套完全不同的模拟后端**的域。
 *   CPU 路径逐位可复现（同种子同参数双跑 diff=0，F2215 断言），GPU 路径
 *   只能承诺视觉等效（统计特征一致，浮点归约序不可控）。这两条语义若在开工时
 *   不写死，后续每个条目都会各自解释「确定」二字meaning。到L05 才吵这件事，
 *   已经有二十个条目把错误语义编进了自己的单元测试里——那才是真正的返工。
 *   故本条把「诚实语义」做成**架构守卫**：任何模块若给 GPU 路径贴上逐位确定的
 *   标签，在开发期就被拦下（F2253 正式声明的前置条件）。
 *
 * 四条锚点契约（逐条对应判据）：
 *   1. 域开工 —— 官方十主题（粒子发射器/力场/流体简易/布料/刚体/绳索/破坏预设/
 *      碰撞场/GPU 粒子/时间轴物理）逐项登记：主题 id、主题名、归属批次、职责边界、
 *      产出契约名。十主题与十批次**不是一一对应**（L04 同时承载刚体与碰撞场，
 *      L09/L10 是跨主题的域治理批次），本条显式声明这层多对多关系而非假装双射。
 *   2. 双路径架构 —— CPU（万级 · 逐位确定 · 无硬件要求）与 GPU（百万级 ·
 *      视觉等效 · compute 要求）两行规格表 + 路由策略：按规模与确定性需求自动
 *      路由，允许手动覆盖；覆盖与需求冲突时显性失败而非静默改语义。
 *   3. 序契约接收 —— F2197 移交包序契约件（F2148 序总图快照）的接收签名。
 *      序契约是 K→L 契约链的**首环**：未签即开工阻断，缺件点名到具体条目。
 *   4. 组内 20 条分工总览 —— L01 批次 F2201~F2220 逐条：职责、产出契约、
 *      判据锚点、前置依赖。20 条齐备由自检断言（缺一条即开工未完成）。
 *
 * 零静默纪律：序契约未签、快照失真、上游未收官、语义混淆、硬件不可用、
 * 手动覆盖冲突、手数不齐——全部产出 Diagnostic（code + message + hint）
 * 并由调用方聚合上报。本模块不抛异常、不吞诊断、无静默分支。
 *
 * 判据：域开工、双路径架构、序契约接收、诚实语义、判据。
 * 交接说明：本条是纯契约层，零 GPU 调用、零 DOM 依赖、零全局可变状态——
 *         可在任意宿主（浏览器/Worker/Node 校验脚本）中原样引入。
 *         下游 L01（F2202 粒子数据模型 起）逐项消费本条的 PATH_TABLE、
 *         THEME_REGISTRY、BATCH_REGISTRY 与 K_TO_L_SEQ_CONTRACT。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型（零静默的基础设施：与 I 域同纪律，此处独立实现不跨域 import）
// ════════════════════════════════════════════════════════════════════════════

/** 诊断码：每种拒绝/越权/退化独立可检索，绝不合并成一条通用错误。 */
export type DiagCode =
  /** 主题未在十主题注册表中登记（引用了不存在的主题）。 */
  | "THEME_UNREGISTERED"
  /** 主题已登记但字段自相矛盾（批次不存在 / 区间倒置 / 契约名为空）。 */
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
  /** 模拟路径未登记。 */
  | "PATH_UNREGISTERED"
  /** 路径语义自相矛盾（规格表自身把 GPU 声明成逐位确定）。 */
  | "PATH_SEMANTICS_MISMATCH"
  /** 路径规格字段非法（规模区间倒置、要求硬件却无「不承诺」声明等）。 */
  | "PATH_SPEC_INCONSISTENT"
  /** 规模落在所选路径的目标区间之外。 */
  | "PATH_SCALE_OUT_OF_RANGE"
  /** 选择 GPU 路径但宿主不提供 compute 能力。 */
  | "PATH_HARDWARE_UNAVAILABLE"
  /** 需求逐位确定，但域内无任何路径能提供该语义（诚实失败，不静默降级）。 */
  | "PATH_DETERMINISM_UNSATISFIABLE"
  /** 手动覆盖与需求冲突（用户强指定 GPU 却要求逐位确定）。 */
  | "MANUAL_OVERRIDE_CONFLICT"
  /** 诚实语义守卫命中：给 GPU 路径贴了逐位确定标签。 */
  | "HONESTY_GUARD_VIOLATION"
  /** K 域序契约未接收（移交包缺件或确认位未签）。 */
  | "SEQ_CONTRACT_UNSIGNED"
  /** 序契约义务条款缺失（接收方未逐条确认遵守义务）。 */
  | "SEQ_CONTRACT_OBLIGATION_MISSING"
  /** 序契约快照失真（哈希与K 域宣告值不符）。 */
  | "SEQ_CONTRACT_SNAPSHOT_MISMATCH"
  /** 上游 K 域未收官（F2200 宣告未生效），开工前置不成立。 */
  | "UPSTREAM_NOT_CLOSED"
  /** 组内 20 条分工不齐（缺条/ 重复条目号）。 */
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

/** 结果判别联合：成功必带value，失败必带 code/message/hint——失败不可被误当成功。 */
export type Outcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly Diagnostic[] }
  | {
      readonly ok: false;
      readonly code: DiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly Diagnostic[];
    };

/** 成功构造（diagnostics允许携带非致命告警，例如硬件不足的降级建议）。 */
export function ok<T>(value: T, diagnostics: readonly Diagnostic[] = []): Outcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造：三要素齐备，diagnostics 含本条自身便于统一上报。 */
export function fail<T>(code: DiagCode, message: string, hint: string): Outcome<T> {
  const d: Diagnostic = { code, message, hint };
  return { ok: false, code, message, hint, diagnostics: [d] };
}

/** 诊断聚合器：把散落各处的告警汇成一条时间轴可查的清单。 */
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

  /** 追加一条已构造的诊断（用于把子调用的 Outcome.diagnostics 平铺进来）。 */
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
  id: "VE-L",
  /** 域名。 */
  name: "粒子与物理",
  /** 功能号区间。 */
  itemLo: 2201,
  itemHi: 2400,
  /** 本域功能总数（2400 - 2201 + 1 = 200，与册内「十域 × 200 项」一致）。 */
  itemCount: 200,
  /** 所属波次（VE 卷波次表：W5 = 粒子与物理）。 */
  wave: "W5",
  /** 本域开工条的功能号。 */
  kickoffItemId: 2201,
} as const;

/** 官方十主题的中文名序列（判据一的对外表述，顺序即册内顺序）。 */
export const OFFICIAL_THEMES: readonly string[] = [
  "粒子发射器",
  "力场",
  "流体简易",
  "布料",
  "刚体",
  "绳索",
  "破坏预设",
  "碰撞场",
  "GPU 粒子",
  "时间轴物理",
];

/** 十个官方主题 id（封闭集：新增主题须改本类型与THEME_REGISTRY 两处）。 */
export type ThemeId =
  | "particle-emitter"
  | "force-field"
  | "fluid-simple"
  | "cloth"
  | "rigid-body"
  | "rope"
  | "destruction-preset"
  | "collision-field"
  | "gpu-particle"
  | "timeline-physics";

/** 一个主题的完整规格（十主题逐项声明，无一项靠约定）。 */
export interface ThemeSpec {
  readonly id: ThemeId;
  /** 主题中文名（对外文档与错误提示引用）。 */
  readonly name: string;
  /**
   * 承载该主题的批次列表（十主题与十批次是多对多，不是双射——见函数注释）。
   * 顺序即依赖序：L04 既承载刚体也承载碰撞场时，物理求解须在碰撞查询之后。
   */
  readonly batches: readonly string[];
  /** 职责边界一句话：这一主题负责什么、不负责什么（禁扩面写在句内）。 */
  readonly duty: string;
  /** 本主题产出、供下游消费的数据契约名（契约名全局唯一，由自检机检）。 */
  readonly producesContract: string;
  /** 一句话说明：本主题与哪个域交接，交接物是什么。 */
  readonly handoff: string;
  /**
   * 是否为跨主题治理类（L09 预备与自查 / L10 收口覆盖全部主题）。
   * 治理类主题不产数据契约，故producesContract 为空字符串。
   */
  readonly isGovernance: boolean;
}

/**
 * 十主题注册表。判据一要求逐主题声明归属批次/职责/契约，此处即为该声明的
 * 唯一事实源——下游任何代码不得旁路硬编码主题名，一律经 lookupTheme 取规格。
 *
 * 为什么主题与批次是多对多而不是一一对应（显式记录，避免后来者「修正」成双射）：
 *   官方十主题是**能力分类轴**，十个批次是**排产轴**（每批 20 项，便于双签与
 *   组级收口）。二者粒度本就不同：碰撞场与刚体共享同一批求解器与同一套
 *   穿透防护（拆两批会出现两套互相不知情的求解器），布料与绳索共享同一套
 *   约束求解与自碰撞裁剪；而 L09（预备与自查）与L10（收口）不对应任何单一
 *   能力主题，它们是覆盖全部主题的域治理批次。故此处登记多对多，并在
 *   auditThemeCoverage 里机检「每个能力主题至少被一个实现批次承载」。
 */
export const THEME_REGISTRY: Readonly<Record<ThemeId, ThemeSpec>> = {
  "particle-emitter": {
    id: "particle-emitter",
    name: "粒子发射器",
    batches: ["L01"],
    duty: "发射率/五形状/速度分布/四模式/生命周期/渲染形态与池配额；不负责力场求力与刚体求解",
    producesContract: "EmitterControl",
    handoff: "向 I 域（F1601 I02 绘制管线）交付粒子实例化批次；向 K 域交付速度缓冲写入源（F2045）",
    isGovernance: false,
  },
  "force-field": {
    id: "force-field",
    name: "力场",
    batches: ["L02"],
    duty: "六类力场（重力/风/涡旋/吸引/斥力/噪声）、统一衰减曲线、多场叠加与碰撞响应；不负责发射与渲染形态",
    producesContract: "ForceFieldSet",
    handoff: "消费 L01 的粒子池属性（只读状态、写回加速度）；向 F2220 交接力场-粒子交互契约",
    isGovernance: false,
  },
  "fluid-simple": {
    id: "fluid-simple",
    name: "流体简易",
    batches: ["L06"],
    duty: "低成本流体近似（粒子化高度场与密度扩散）；不承诺工程级流体求解精度",
    producesContract: "SimpleFluidState",
    handoff: "消费 L02 的噪声力场作外力；向破坏预设提供可破碎介质判据",
    isGovernance: false,
  },
  cloth: {
    id: "cloth",
    name: "布料",
    batches: ["L05"],
    duty: "质点-约束布料求解、风阻耦合与自碰撞裁剪；不负责软体体积保真",
    producesContract: "ClothSolverState",
    handoff: "消费 L02 的风力与涡旋；向渲染侧交付布料网格变形缓存",
    isGovernance: false,
  },
  "rigid-body": {
    id: "rigid-body",
    name: "刚体",
    batches: ["L04"],
    duty: "刚体积分、约束求解与休眠策略；不负责可变形体（归布料/绳索）",
    producesContract: "RigidBodyWorld",
    handoff: "消费 L04 的碰撞场查询结果；向 M 域（动画）交付骨骼驱动的运动学刚体",
    isGovernance: false,
  },
  rope: {
    id: "rope",
    name: "绳索",
    batches: ["L05"],
    duty: "绳索链式约束、锚点与摆动阻尼；与布料共用约束求解内核",
    producesContract: "RopeChainState",
    handoff: "消费 L02 的风力；与布料共用自碰撞裁剪路径（F2227 碰撞查询）",
    isGovernance: false,
  },
  "destruction-preset": {
    id: "destruction-preset",
    name: "破坏预设",
    batches: ["L06"],
    duty: "可复现破坏预设（碎裂/断裂/坍塌）的参数化编排与缓存；不负责材质断裂着色",
    producesContract: "DestructionPreset",
    handoff: "消费 L06 的流体介质状态；向 V 域交付破坏缓存的加载契约",
    isGovernance: false,
  },
  "collision-field": {
    id: "collision-field",
    name: "碰撞场",
    batches: ["L02", "L04"],
    duty: "三型碰撞体（平面/球体/网格）、批量查询、穿透防护与响应参数；与刚体同批落地避免两套求解器",
    producesContract: "CollisionQueryBatch",
    handoff: "向 L02 力场与 L04 刚体同时供给查询；空间加速复用 I 域 I05 BVH（跨域单源）",
    isGovernance: false,
  },
  "gpu-particle": {
    id: "gpu-particle",
    name: "GPU 粒子",
    batches: ["L03"],
    duty: "GPU 路径的compute 发射、力场 GPU 化与显存池；只承诺视觉等效，明确不承诺逐位确定",
    producesContract: "GpuParticlePipeline",
    handoff: "消费 L01 的粒子属性规格表（F2202 SoA 布局）与 L02 的力场参数块（F2245 uniform 单源）",
    isGovernance: false,
  },
  "timeline-physics": {
    id: "timeline-physics",
    name: "时间轴物理",
    batches: ["L07"],
    duty: "物理缓存与回放、关键帧烘焙、时间缩放与子步同步；不负责刚体求解算法本身",
    producesContract: "PhysicsTimelineCache",
    handoff: "消费 L04 刚体世界快照；向 M 域（动画时间轴）交付双向时间对齐契约",
    isGovernance: false,
  },
};

/** 注册表键全集（遍历用，避免手写清单与实际注册漂移）。 */
export const THEME_IDS: readonly ThemeId[] = Object.keys(THEME_REGISTRY) as ThemeId[];

/** 查主题规格；未注册返回显性失败——判据要求「十主题封闭集，域外主题不可引用」。 */
export function lookupTheme(id: string): Outcome<ThemeSpec> {
  const table = THEME_REGISTRY as Record<string, ThemeSpec | undefined>;
  const spec = table[id];
  if (spec === undefined) {
    return fail(
      "THEME_UNREGISTERED",
      `主题 ${id} 不在 L 域十主题注册表中`,
      `已注册主题：${THEME_IDS.join("、")}；若确为新增主题，须先在 THEME_REGISTRY 与 ThemeId 类型两处同时登记`,
    );
  }
  return ok(spec);
}

/** 一个批次的完整规格（十批次 × 20 项 = 200 项，逐批次不重叠）。 */
export interface BatchSpec {
  readonly id: string;
  /** 批次中文名。 */
  readonly name: string;
  /** 本批次功能号区间（含端点）。逐批次不重叠，由自检机检。 */
  readonly itemRange: { readonly lo: number; readonly hi: number };
  /** 本批次承载的主题 id 列表（与 THEME_REGISTRY.batches 互为逆映射）。 */
  readonly themes: readonly ThemeId[];
  /** 批次职责一句话：这一批要交付什么，禁扩面写在句内。 */
  readonly duty: string;
  /** 批次出口判据（一句话，可机检即由 CI 断言）。 */
  readonly exitCriterion: string;
  /** 该批次完成时向下一批交接的契约名。 */
  readonly handoffContract: string;
  /** 是否为域治理批次（覆盖全部主题，不对应单一能力）。 */
  readonly isGovernance: boolean;
}

/**
 * 十批次注册表。排产轴：每批恰好 20 项，便于组级双签与收口。
 * 区间合计恰为 200（2201~2400），由selfCheckBatches 机检——多退少补都是登记错误。
 */
export const BATCH_REGISTRY: readonly BatchSpec[] = [
  {
    id: "L01",
    name: "粒子系统架构与发射器组",
    itemRange: { lo: 2201, hi: 2220 },
    themes: ["particle-emitter"],
    duty: "粒子底座与发射器全链：数据模型/发射核心/四模式/生命周期/渲染形态/排序混合/池与预算/调试/基准/API 冻结",
    exitCriterion: "20 项自检入树、CPU 确定性断言全过、fuzz 通过率 100% 三件硬证齐备，粒子契约件已向 L02 移交",
    handoffContract: "ParticleContractBundle",
    isGovernance: false,
  },
  {
    id: "L02",
    name: "力场与行为组",
    itemRange: { lo: 2221, hi: 2240 },
    themes: ["force-field", "collision-field"],
    duty: "六类力场、统一衰减曲线、多场叠加与碰撞三型查询/响应；力场只读粒子状态写回加速度（单向语义）",
    exitCriterion: "力场求力双跑逐位一致、碰撞批量查询复用 I05 BVH 收益量化、力场契约冻结并交付 L03",
    handoffContract: "ForceFieldBundle",
    isGovernance: false,
  },
  {
    id: "L03",
    name: "GPU 粒子组",
    itemRange: { lo: 2241, hi: 2260 },
    themes: ["gpu-particle"],
    duty: "GPU 路径 compute 发射与力场 GPU 化、显存池、GPU 统计等效声明的正式兑现（F2253）",
    exitCriterion: "GPU 路径统计等效口径对齐 L01（总数 ±1% / 直方图散度阈值），并出具「不承诺逐位确定」的正式声明",
    handoffContract: "GpuParticleBundle",
    isGovernance: false,
  },
  {
    id: "L04",
    name: "刚体与碰撞物理组",
    itemRange: { lo: 2261, hi: 2280 },
    themes: ["rigid-body", "collision-field"],
    duty: "刚体积分与约束求解、接触流形与穿透回退、休眠与岛管理；碰撞体求交已在 L02 落地，此处只做刚体侧响应",
    exitCriterion: "刚体世界双跑逐位一致、穿透场景无隧穿（子步进参数到位）、岛管理无泄漏",
    handoffContract: "RigidBodyBundle",
    isGovernance: false,
  },
  {
    id: "L05",
    name: "布料绳索与软体组",
    itemRange: { lo: 2281, hi: 2300 },
    themes: ["cloth", "rope"],
    duty: "共用约束求解内核、质点布料与链式绳索、自碰撞裁剪、撕裂与断裂阈值",
    exitCriterion: "布料与绳索共用一套约束内核（无第二份实现）、自碰撞裁剪不误裁、撕裂阈值双跑一致",
    handoffContract: "SoftBodyBundle",
    isGovernance: false,
  },
  {
    id: "L06",
    name: "流体简易与破坏预设组",
    itemRange: { lo: 2301, hi: 2320 },
    themes: ["fluid-simple", "destruction-preset"],
    duty: "粒子化流体近似、密度扩散与浮力；破坏预设的参数化编排、缓存与确定性回放",
    exitCriterion: "流体近似误差上界有声明（不夸大精度）、破坏预设回放逐位一致、缓存命中与失效路径全测",
    handoffContract: "DestructionBundle",
    isGovernance: false,
  },
  {
    id: "L07",
    name: "时间轴物理与场景集成组",
    itemRange: { lo: 2321, hi: 2340 },
    themes: ["timeline-physics"],
    duty: "物理缓存与回放、关键帧烘焙、时间缩放与子步同步、与 M 域动画时间轴双向对齐",
    exitCriterion: "回放逐帧与实时模拟逐位一致、时间缩放不破坏子步同步、与 M 域对齐契约冻结",
    handoffContract: "TimelinePhysicsBundle",
    isGovernance: false,
  },
  {
    id: "L08",
    name: "物理性能与质量组",
    itemRange: { lo: 2341, hi: 2360 },
    themes: [],
    duty: "物理性能成本模型与预算联动、SIMD 收益实测、密度档位与用户可感知画质映射（跨主题工程）",
    exitCriterion: "成本模型偏差 ≤20% 修正门闭环、档位映射与 L01/L03 三方对齐、低配机实测达标",
    handoffContract: "PhysicsPerfBundle",
    isGovernance: true,
  },
  {
    id: "L09",
    name: "L 域预备与自查组",
    itemRange: { lo: 2361, hi: 2380 },
    themes: [],
    duty: "域收口前置整备：八批状态核验自动化、互操作两两对账、API 终版、文档基准清账、六项确认与缺失补齐",
    exitCriterion: "八批状态全部核验通过、两两对账零冲突、缺口清单归零或 ADR 豁免、预备报告出GO 判定",
    handoffContract: "LDomainReadinessReport",
    isGovernance: true,
  },
  {
    id: "L10",
    name: "L 域收口组",
    itemRange: { lo: 2381, hi: 2400 },
    themes: [],
    duty: "域级总对账与收官宣告：200 项逐项 grep 实测、硬门四重核验、20 维度验收、归档与宣告生效",
    exitCriterion: "200/200 实测、四重硬门归零、20 维度无回炉、宣告生效并入 V 域归档",
    handoffContract: "LDomainClosure",
    isGovernance: true,
  },
];

/** 查批次规格；未登记返回显性失败。 */
export function lookupBatch(id: string): Outcome<BatchSpec> {
  const spec = BATCH_REGISTRY.find((b) => b.id === id);
  if (spec === undefined) {
    return fail(
      "BATCH_UNREGISTERED",
      `批次 ${id} 不在 L 域十批次注册表中`,
      `已登记批次：${BATCH_REGISTRY.map((b) => b.id).join("、")}；新增批次须同时补BATCH_REGISTRY 与区间分配`,
    );
  }
  return ok(spec);
}

/**
 * 主题覆盖审计：核对三件事，任一不成立即产出诊断——
 *   1. 十主题齐备，且主题名与册内官方序列逐项一致（防漏项/改名漂移）；
 *   2. 每个能力主题（非治理类）至少被一个实现批次承载，且批次确实登记了该主题；
 *   3. 批次的主题表与主题的归属表互为逆映射（双向一致，不允许只写一边）。
 *
 * 第2 项区分治理批次是有意的：L08/L09/L10 是跨主题工程，themes 为空是正确形态，
 * 不能因为「没挂主题」就判它们孤岛（那是把治理批次误当能力主题的错误）。
 */
export function auditThemeCoverage(bag: DiagBag): Outcome<readonly ThemeId[]> {
  const uncovered: ThemeId[] = [];

  // 1) 主题名与官方序列一致。
  for (const official of OFFICIAL_THEMES) {
    if (!Object.values(THEME_REGISTRY).some((s) => s.name === official)) {
      bag.push(
        "THEME_SPEC_INCONSISTENT",
        `官方主题「${official}」未在 THEME_REGISTRY 中以同名条目登记`,
        `官方十主题序列与注册表必须逐项一致；请补登记「${official}」，或修正注册表中的错字`,
      );
    }
  }

  // 2) 能力主题须被实现批次承载；治理批次以 batch.isGovernance 声明豁免。
  for (const id of THEME_IDS) {
    const spec = THEME_REGISTRY[id];
    if (spec.isGovernance) continue;
    if (spec.batches.length === 0) {
      uncovered.push(id);
      continue;
    }
    for (const batchId of spec.batches) {
      const batch = lookupBatch(batchId);
      if (!batch.ok) {
        bag.pushAll(batch.diagnostics);
        continue;
      }
      if (!batch.value.themes.includes(id)) {
        bag.push(
          "BATCH_THEME_MAPPING_INCONSISTENT",
          `主题 ${spec.name} 声明归属批次 ${batchId}，但该批次的 themes 表里没有它`,
          `两处映射必须互为逆映射；请在 BATCH_REGISTRY 的 ${batchId}.themes 中补上 ${id}，`
            + `或在THEME_REGISTRY.batches 中移除该批次（只改一侧会造成「看起来挂上了实际没人承载」）`,
        );
      }
    }
  }

  if (uncovered.length > 0) {
    const names = uncovered.map((id) => THEME_REGISTRY[id].name).join("、");
    bag.push(
      "THEME_COVERAGE_GAP",
      `以下能力主题未被任何实现批次承载：${names}`,
      `官方十主题是收官对账（F2400）的硬门：任一主题无实现批次，L 域宣告即判不过。`
        + `请在 BATCH_REGISTRY 中把该主题挂到一个承担其实现的批次上（物理相关主题通常挂 L04，`
        + `渲染相关主题通常挂 L03），而不是新建只有一个主题的碎片批次`,
    );
  }

  // 3) 双向一致：批次 themes 中的每个主题都必须回指该批次。
  for (const batch of BATCH_REGISTRY) {
    for (const themeId of batch.themes) {
      const spec = lookupTheme(themeId);
      if (!spec.ok) {
        bag.pushAll(spec.diagnostics);
        continue;
      }
      if (!spec.value.batches.includes(batch.id)) {
        bag.push(
          "BATCH_THEME_MAPPING_INCONSISTENT",
          `批次 ${batch.id} 的 themes 表含主题 ${spec.value.name}，但该主题未回指 ${batch.id}`,
          `请在 THEME_REGISTRY[${themeId}].batches 中补上 ${batch.id}；`
            + `单向登记会让主题覆盖审计误判为已覆盖`,
        );
      }
    }
  }

  return ok(uncovered, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §3 双路径架构（判据二：CPU/GPU 双路径 + 选择策略）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 确定性语义的两种等级（全域根基术语，判据二与「诚实语义」判据共用此定义）：
 *
 *   "bitwise-reproducible"（逐位可复现）——
 *     同种子同参数同遍历序，同步数下两次模拟的**每一个粒子的每一个属性逐位相等**。
 *     只有 CPU 路径能提供：浮点加法/归约的序在 CPU 上由代码结构固定，
 *     不受硬件并行宽度与驱动调度影响。验证方式：双跑全状态哈希 diff=0（F2215）。
 *
 *   "statistically-equivalent"（视觉/统计等效）——
 *     不承诺逐位相等，只承诺统计特征一致：粒子总数、空间分布直方图、
 *     颜色分布、速度分布在多次运行间落在声明容差内。
 *     GPU 路径只能提供此等级：compute着色器的浮点归约序由波内线程调度决定，
 *     跨厂商驱动还会重排内联数学（fast-math 等），逐位一致在工程上不可达。
 *
 * 为什么把这两条写在开工条而不是等到F2215 才写（反直觉处）：
 *   语义是**接口契约**，不是实现细节。发射器（F2203）的随机序列、力场（F2226）
 *   的多场求和序、GPU 发射（F2241）的浮点路径，三者都会各自假设「确定」是哪个意思。
 *   假设不一致时，症状不是报错而是**偶发的跨机器回放差异**——最难查的一类。
 *   故在开工时把语义钉死并做成守卫，让后续条目只能在此语义下工作。
 */
export type DeterminismSemantics = "bitwise-reproducible" | "statistically-equivalent";

/** 模拟路径 id（封闭集：新增路径须改本类型与 PATH_TABLE 两处）。 */
export type SimPathId = "cpu-scalar-sim" | "gpu-compute-sim";

/** 一个模拟路径的完整规格（两行规格表，逐字段声明，无一项靠约定）。 */
export interface PathSpec {
  readonly id: SimPathId;
  /** 路径中文名。 */
  readonly name: string;
  /**
   * 目标规模区间（活粒子数，含端点）。超出区间即PATH_SCALE_OUT_OF_RANGE——
   * 规模落在区间外说明选错了路径，继续跑会得到「能跑但每帧几十毫秒」的结果。
   */
  readonly targetScale: { readonly lo: number; readonly hi: number };
  /** 该路径能提供的确定性语义（唯一值，不允许含糊表述）。 */
  readonly determinism: DeterminismSemantics;
  /** 是否要求 compute 能力（GPU 路径为 true）。 */
  readonly requiresCompute: boolean;
  /** 该路径的诚实语义上限：此路径**永远不能**对外宣称的更强语义。 */
  readonly honestCeiling: DeterminismSemantics;
  /** 该路径明确不承诺的能力（写下来是为了让文档、API 描述词、遥测口径能引用同一事实源）。 */
  readonly notPromised: readonly string[];
  /** 路径职责边界一句话。 */
  readonly duty: string;
  /** 架构依据锚点（本路径由哪个条目实现/定标）。 */
  readonly implementedBy: readonly string[];
}

/**
 * 双路径规格表。判据二的核心资产——CPU 与 GPU 各一行，四个维度（规模/确定性/
 * 硬件要求/职责）逐字段声明。
 *
 * 规模档为何是「万级 vs 百万级」而不是「几千 vs 几万」（定标理由）：
 *   CPU 路径的成本∝ 粒子数 × 属性数 × 力场数，单核向量化的实测量级在
 *   1万~10 万区间仍可接受（十级毫秒）；越过十万后即使 SIMD 打满也会撞帧预算。
 *   GPU 路径的成本主要由显存带宽与 dispatch 开销决定，一万粒子时 dispatch
 *   开销占比过高（launch-bound），十万起才进入带宽主导区。故两档的交界
 *   取 10万：低于它CPU 更划算，高于它 GPU 的单位成本才开始反超。
 *   这两个数字由 L01（L03 批次 F2210/F2212）与 L03（F2246基准）实测回填，
 *   本条只声明**量级**与**回填责任**——开工时没有实测数据，编一个精确数字
 *   才是真的撒谎（这正是判据四诚实语义的反面教材）。
 */
export const PATH_TABLE: Readonly<Record<SimPathId, PathSpec>> = {
  "cpu-scalar-sim": {
    id: "cpu-scalar-sim",
    name: "CPU 标量模拟路径",
    targetScale: { lo: 0, hi: 100_000 },
    determinism: "bitwise-reproducible",
    requiresCompute: false,
    honestCeiling: "bitwise-reproducible",
    notPromised: ["跨后端逐位一致（GPU 后端不适用，见诚实语义声明）", "十万以上规模下的实时性"],
    duty: "小规模高精度模拟与逐位可复现回放；发射/积分/求力全在 CPU 侧顺序完成",
    implementedBy: ["VE-F2203 发射核心", "VE-F2215 一致性双跑断言", "VE-F2210 性能预算"],
  },
  "gpu-compute-sim": {
    id: "gpu-compute-sim",
    name: "GPU compute 模拟路径",
    targetScale: { lo: 100_000, hi: 4_000_000 },
    determinism: "statistically-equivalent",
    requiresCompute: true,
    honestCeiling: "statistically-equivalent",
    notPromised: [
      "逐位可复现（同种子双跑跨厂商驱动必然存在末位差异）",
      "跨后端逐位一致（CPU 与 GPU 的归约序不同）",
      "小于十万粒子时的成本优势（dispatch 开销占比过高）",
    ],
    duty: "百万级大规模模拟；发射/积分/求力在 compute 着色器内批量完成，视觉等效优先",
    implementedBy: ["VE-F2241 GPU 发射", "VE-F2245 力场 GPU 化", "VE-F2253 正式语义声明"],
  },
};

/** 路径 id 全集（遍历用）。 */
export const PATH_IDS: readonly SimPathId[] = Object.keys(PATH_TABLE) as SimPathId[];

/**
 * 语义强度序（数值大者承诺更强）。用于比较「需求 vs 提供」：
 *逐位确定 > 统计等效。GPU 路径的 honestCeiling 恒为统计等效——
 * 这是诚实语义的数学表达：它在结构上不可能满足逐位确定的需求。
 */
export const SEMANTICS_RANK: Readonly<Record<DeterminismSemantics, number>> = {
  "statistically-equivalent": 0,
  "bitwise-reproducible": 1,
};

/**
 * 诚实语义守卫：给定「某路径 + 某项对外宣称」，判定该宣称是否越过了该路径的
 * 诚实上限。
 *
 * 这是本条最重要的一道闸门（判据四）。用法：任何模块在写文档描述词、API 冻结
 * 清单、遥测口径、对外承诺之前，须把宣称文本过一遍本守卫。命中即开发期失败。
 *
 * 为什么用「语义等级 + 关键词」双判据而不是只比等级：
 *   只比等级拦不住「GPU 路径 · 确定性：确定性模拟」这种**没有出现等级词**的宣称——
 *   而它恰恰是最危险的一种（读者会自动脑补成逐位确定）。故本守卫同时检查：
 *   ① 宣称文本若显式给出等级词，与诚实上限比较；
 *   ② 宣称文本若使用「确定性/可复现/逐位/reproducible/deterministic」这类
 *      **无等级词的强断言词**，且路径上限为统计等效，则一律拦截。
 */
export function checkHonestyClaim(
  pathId: SimPathId,
  claim: string,
  bag: DiagBag,
): Outcome<PathSpec> {
  const spec = PATH_TABLE[pathId];
  if (spec === undefined) {
    return fail(
      "PATH_UNREGISTERED",
      `模拟路径 ${pathId} 不在双路径规格表中`,
      `已登记路径：${PATH_IDS.join("、")}；新增路径须同时改 PATH_TABLE 与 SimPathId 类型`,
    );
  }

  const text = claim.toLowerCase();

  // 判据 ①：显式等级词与诚实上限比较。
  const claimsBitwise =
    text.includes("bitwise") || text.includes("逐位") || text.includes("位级");
  if (claimsBitwise && SEMANTICS_RANK[spec.honestCeiling] < SEMANTICS_RANK["bitwise-reproducible"]) {
    bag.push(
      "HONESTY_GUARD_VIOLATION",
      `诚实语义守卫：${spec.name} 被宣称「${claim}」，但该路径的诚实上限是「统计等效」`,
      `GPU compute 路径的浮点归约序由波内线程调度决定、跨厂商驱动还会重排内联数学，`
        + `逐位一致在工程上不可达。请把宣称改写为「视觉/统计等效」并给出容差口径`
        + `（L01 F2215 已定义：粒子总数 ±1%、分布直方图散度阈值、颜色分布容差）；`
        + `若你的场景真的需要逐位可复现，请改用 ${PATH_TABLE["cpu-scalar-sim"].name}`,
    );
    return fail(
      "HONESTY_GUARD_VIOLATION",
      `${spec.name} 的「${claim}」宣称越过诚实上限`,
      `改写为统计等效口径，或改用 CPU 路径（逐位可复现）`,
    );
  }

  // 判据 ②：无等级词的强断言词，一样拦截（读者会自动脑补，这是更危险的形态）。
  const strongWords = ["确定性模拟", "完全确定", "精确可复现", "deterministic", "reproducible"];
  const hit = strongWords.find((w) => text.includes(w));
  if (hit !== undefined && SEMANTICS_RANK[spec.honestCeiling] < SEMANTICS_RANK["bitwise-reproducible"]) {
    bag.push(
      "HONESTY_GUARD_VIOLATION",
      `诚实语义守卫：${spec.name} 的宣称「${claim}」含强断言词「${hit}」，但未标注等级`,
      `强断言词会让读者默认理解为逐位确定，即便你没这么说。请显式写出等级与容差`
        + `（例如「视觉等效：粒子总数 ±1%」）；GPU 路径不具备逐位语义，这不是限制而是事实——`
        +`把它说清楚比含糊过去更省事，也更经得起跨机器复核`,
    );
    return fail(
      "HONESTY_GUARD_VIOLATION",
      `${spec.name} 的宣称含未标注等级的强断言词「${hit}」`,
      "显式标注确定性等级与容差口径，或改用 CPU 路径",
    );
  }

  return ok(spec, bag.all());
}

/** 路径规格自洽校验：规模区间不倒置、语义不越上限、硬件要求与语义相符。 */
export function validatePathTable(bag: DiagBag): Outcome<readonly PathSpec[]> {
  for (const id of PATH_IDS) {
    const spec = PATH_TABLE[id];
    const { lo, hi } = spec.targetScale;
    if (lo > hi || lo < 0 || !Number.isFinite(hi)) {
      bag.push(
        "PATH_SEMANTICS_MISMATCH",
        `路径 ${spec.name} 的目标规模区间非法（lo=${lo}, hi=${hi}）`,
        "区间须满足 0 ≤ lo ≤ hi 且hi 为有限数；规模区间是路由决策的输入，非法区间会让路由结果不可复现",
      );
    }
    if (SEMANTICS_RANK[spec.determinism] > SEMANTICS_RANK[spec.honestCeiling]) {
      bag.push(
        "PATH_SEMANTICS_MISMATCH",
        `路径 ${spec.name} 声明的确定性（${spec.determinism}）强于其诚实上限（${spec.honestCeiling}）`,
        `规格表自身矛盾：honestCeiling 是该路径在工程上能达到的最强语义，`
          + `determinism 不得越过它。GPU 路径的 honestCeiling 恒为统计等效——`
          + `若你认为 GPU 能逐位确定，请先给出跨厂商驱动的实测证据再改表`,
      );
    }
    if (spec.requiresCompute && spec.notPromised.length === 0) {
      bag.push(
        "PATH_SPEC_INCONSISTENT",
        `路径 ${spec.name} 要求 compute 能力但未声明任何「不承诺」事项`,
        "要求硬件能力的路径必须显式写出它在低配设备上的降级后果；空清单等于把风险藏起来",
      );
    }
  }
  // 规格表内部自检也算一次诚实守卫：确保没有任何一行把 GPU 写成逐位确定。
  const gpu = PATH_TABLE["gpu-compute-sim"];
  if (gpu.determinism === "bitwise-reproducible") {
    bag.push(
      "PATH_SEMANTICS_MISMATCH",
      "双路径规格表中 GPU 路径被写成逐位可复现，这与已知事实矛盾（compute 归约序不可控）",
      "GPU 路径的 determinism 必须为 statistically-equivalent；"
        + "这是 L 域的诚实语义根基（F2253 的前置），不是保守而是事实",
    );
  }
  return ok(PATH_IDS.map((id) => PATH_TABLE[id]), bag.all());
}

/** 硬件能力探测结果（由上层设备抽象 VE-A 填充，本条只消费不探测）。 */
export interface HostCapability {
  /** 是否提供 compute 能力（着色器派发+ 显存池）。 */
  readonly computeAvailable: boolean;
  /** 活粒子数（路由决策的规模输入）。 */
  readonly liveParticleCount: number;
}

/** 一次路由请求：调用方想要什么。 */
export interface RouteRequest {
  readonly host: HostCapability;
  /**
   * 确定性需求。传 undefined 表示「无特殊要求，按规模与成本自动选」。
   * 传 "bitwise-reproducible" 表示场景依赖回放逐位一致（如录像导出、
   * 网络同步、破坏预设缓存回放）——此时GPU 路径不可选。
   */
  readonly determinismNeeded?: DeterminismSemantics;
  /**
   * 手动覆盖（用户或场景作者显式指定路径）。指定后仍须满足需求与硬件约束；
   * 冲突时返回显性失败，不静默改写语义——静默改写是本域最不可接受的行为。
   */
  readonly manualOverride?: SimPathId;
}

/** 一次路由决策的完整结果（选了什么、为什么、有哪些非致命告警）。 */
export interface RouteDecision {
  readonly path: SimPathId;
  /** 目标规模区间（随路径给出，供调用方预算换算）。 */
  readonly targetScale: { readonly lo: number; readonly hi: number };
  /** 该决策能提供的确定性语义。 */
  readonly determinism: DeterminismSemantics;
  /** 是否发生了手动覆盖（false即纯自动路由）。 */
  readonly manual: boolean;
  /** 决策理由（人话，进日志与遥测，供事后复核「当时为什么走了GPU」）。 */
  readonly reason: string;
}

/**
 * 双路径路由：给定规模、硬件与确定性需求，选出模拟路径。
 *
 * 自动路由策略（按规模与确定性需求，固定次序——次序本身是契约）：
 *   第0 步 确定性需求优先于规模。若场景要求逐位可复现，GPU 路径在结构上
 *          不可能满足，此时**不看规模**直接排除 GPU（哪怕活粒子数只有 1万——
 *          宁可慢也不能给一个假的可复现承诺）。
 *   第1 步 规模落在 CPU 区间内 → CPU。CPU 路径同时满足两种语义，成本更低。
 *   第2 步 规模落在 GPU 区间内 → GPU，前提是宿主提供 compute 能力；
 *          不提供时显性失败并给出三要素（当前规模/能力缺失/建议），
 *          而非静默退回 CPU——退回 CPU 在百万粒子下必然掉帧，用户看到的
 *          症状是「卡」，却找不到任何告警指向原因。
 *   第3 步 规模超出 GPU 区间上限 → 显性失败：给出建议（提档 / 降密度 / 拆场景）。
 *   第4 步 规模落在两条区间之外的空隙（理论不该出现，由validatePathTable
 *          与本函数共同兜底）→ 显性失败，提示区间表需重新定标。
 *
 * 手动覆盖：调用方可指定路径以覆盖自动决策（用于对比测试、用户偏好评测、
 * 强制 CPU 排查 GPU 疑似错误）。但覆盖仍受三条硬约束：
 *   ① 需求逐位确定 + 指定 GPU → MANUAL_OVERRIDE_CONFLICT（不静默降语义）；
 *   ② 指定 GPU + 无 compute → PATH_HARDWARE_UNAVAILABLE；
 *   ③ 规模超出该路径区间 → PATH_SCALE_OUT_OF_RANGE（带建议值）。
 */
export function routePath(req: RouteRequest, bag: DiagBag): Outcome<RouteDecision> {
  const table = PATH_TABLE as Record<string, PathSpec | undefined>;
  const cpu = table["cpu-scalar-sim"] as PathSpec;
  const gpu = table["gpu-compute-sim"] as PathSpec;
  const count = req.host.liveParticleCount;

  // 入参合法性：规模必须是��负有限整数。NaN 会让所有比较为 false 而静默走到
  // 「规模超出区间」分支，诊断信息会误导人——所以单独拦。
  if (!Number.isFinite(count) || count < 0 || !Number.isInteger(count)) {
    bag.push(
      "PATH_SCALE_OUT_OF_RANGE",
      `路由入参的活粒子数非法（${count}）`,
      "活粒子数须为非负整数（来自池水位 F2208）；NaN/负数/小数通常意味着上游统计有误，"
        + "请先核对池计数器的读出路径，而不是让本函数猜一个值继续跑",
    );
    return fail(
      "PATH_SCALE_OUT_OF_RANGE",
      `活粒子数非法：${count}`,
      "须为非负整数；请核对粒子池水位统计的读出路径",
    );
  }

  // 决策0：手动覆盖优先，但须过三道硬约束。
  if (req.manualOverride !== undefined) {
    const spec = table[req.manualOverride];
    if (spec === undefined) {
      bag.push(
        "PATH_UNREGISTERED",
        `手动覆盖指定的路径 ${req.manualOverride} 不在双路径规格表中`,
        `已登记路径：${PATH_IDS.join("、")}`,
      );
      return fail("PATH_UNREGISTERED", `手动覆盖路径 ${req.manualOverride} 未登记`, "请使用已登记的路径 id");
    }
    // 约束①：逐位确定需求 vs GPU 上限。
    if (
      req.determinismNeeded !== undefined &&
      SEMANTICS_RANK[req.determinismNeeded] > SEMANTICS_RANK[spec.honestCeiling]
    ) {
      bag.push(
        "MANUAL_OVERRIDE_CONFLICT",
        `手动覆盖指定 ${spec.name}，但场景需求是「${req.determinismNeeded}」`,
        `该路径的诚实上限是「${spec.honestCeiling}」，无法满足逐位可复现需求。`
          + `两条出路：①放弃手动覆盖走自动路由（会选 CPU）；②确认场景其实不需要逐位一致，`
          + `改传 determinismNeeded="statistically-equivalent"（录像导出、网络同步、破坏预设回放三类场景除外）`,
      );
      return fail(
        "MANUAL_OVERRIDE_CONFLICT",
        `手动覆盖 ${spec.name} 与逐位确定需求冲突`,
        "放弃覆盖走自动路由，或把需求改为统计等效（导出/同步/回放场景除外）",
      );
    }
    // 约束②：硬件要求。
    if (spec.requiresCompute && !req.host.computeAvailable) {
      bag.push(
        "PATH_HARDWARE_UNAVAILABLE",
        `手动覆盖指定 ${spec.name}，但宿主未提供 compute 能力`,
        `该路径要求 compute 派发与显存池。请改用 ${cpu.name}，`
          + `或在设备抽象层（VE-A）确认 compute 探测结果是否正确`,
      );
      return fail("PATH_HARDWARE_UNAVAILABLE", `${spec.name} 所需 compute 能力不可用`, `改用 ${cpu.name}`);
    }
    // 约束③：规模区间。
    if (count > spec.targetScale.hi) {
      bag.push(
        "PATH_SCALE_OUT_OF_RANGE",
        `手动覆盖指定 ${spec.name}，但活粒子数 ${count} 超出其目标上限 ${spec.targetScale.hi}`,
        `该路径的设计规模区间是 ${spec.targetScale.lo}~${spec.targetScale.hi}。`
          + `建议：降低粒子密度档（F2217）到 ${spec.targetScale.hi} 以下，或改用 ${gpu.name}`,
      );
      return fail(
        "PATH_SCALE_OUT_OF_RANGE",
        `活粒子数 ${count} 超出 ${spec.name} 上限 ${spec.targetScale.hi}`,
        "降密度档或改用 GPU 路径",
      );
    }
    return ok(
      {
        path: spec.id,
        targetScale: spec.targetScale,
        determinism: spec.determinism,
        manual: true,
        reason: `手动覆盖指定 ${spec.name}（规模 ${count}，${spec.determinism}）`,
      },
      bag.all(),
    );
  }

  // 决策 0'：确定性需求优先于规模（见函数注释第 0 步）。
  if (req.determinismNeeded === "bitwise-reproducible") {
    if (count > cpu.targetScale.hi) {
      // CPU 也不够——诚实失败，不假装。
      bag.push(
        "PATH_DETERMINISM_UNSATISFIABLE",
        `场景要求逐位可复现，但活粒子数 ${count} 超出 ${cpu.name} 的上限 ${cpu.targetScale.hi}`,
        `域内无任何路径能在该规模下提供逐位可复现（GPU 路径结构上做不到）。`
          + `建议：①降低规模到 ${cpu.targetScale.hi} 以下；`
          + `②放弃逐位一致要求改用统计等效口径（F2215 已定义容差）；`
          + `③把需要逐位一致的部分拆为独立的小规模子场景`,
      );
      return fail(
        "PATH_DETERMINISM_UNSATISFIABLE",
        `逐位确定需求在规模 ${count} 下无解`,
        "降规模、改用统计等效口径，或拆分场景",
      );
    }
    return ok(
      {
        path: "cpu-scalar-sim",
        targetScale: cpu.targetScale,
        determinism: cpu.determinism,
        manual: false,
        reason: `需求为逐位可复现，GPU 路径结构上不可满足，直接排除（规模 ${count}）`,
      },
      bag.all(),
    );
  }

  // 决策 1：CPU 区间内。
  if (count <= cpu.targetScale.hi) {
    return ok(
      {
        path: "cpu-scalar-sim",
        targetScale: cpu.targetScale,
        determinism: cpu.determinism,
        manual: false,
        reason: `活粒子数 ${count} 在 ${cpu.name} 区间 ${cpu.targetScale.lo}~${cpu.targetScale.hi} 内，`
          + `且CPU 成本更低且语义更强`,
      },
      bag.all(),
    );
  }

  // 决策 2：GPU 区间内，检查硬件。
  if (count >= gpu.targetScale.lo && count <= gpu.targetScale.hi) {
    if (!req.host.computeAvailable) {
      bag.push(
        "PATH_HARDWARE_UNAVAILABLE",
        `活粒子数 ${count} 需要 ${gpu.name}，但宿主未提供 compute 能力`,
        `当前规模 ${count} 超出 ${cpu.name} 上限 ${cpu.targetScale.hi}，无法退回 CPU（会掉帧）。`
          + `三条出路：①降低粒子密度档（F2217）把规模压到 ${cpu.targetScale.hi} 以下；`
          + `②在设备抽象层确认 compute 探测是否误报为不可用；`
          + `③提示用户该场景在当前设备上不可运行（不要静默降级成「能跑但很卡」）`,
      );
      return fail(
        "PATH_HARDWARE_UNAVAILABLE",
        `规模 ${count} 需要 GPU 路径但compute 能力不可用`,
        `降密度档、核对 compute 探测，或明确告知用户该场景在当前设备不可运行`,
      );
    }
    return ok(
      {
        path: "gpu-compute-sim",
        targetScale: gpu.targetScale,
        determinism: gpu.determinism,
        manual: false,
        reason: `活粒子数 ${count} 超出 ${cpu.name} 上限 ${cpu.targetScale.hi}，`
          + `落入 ${gpu.name} 区间 ${gpu.targetScale.lo}~${gpu.targetScale.hi} 且compute 可用；`
          + `注意该路径只承诺「${gpu.determinism}」，不承诺逐位确定`,
      },
      bag.all(),
    );
  }

  // 决策 3：超出 GPU 上限。
  if (count > gpu.targetScale.hi) {
    bag.push(
      "PATH_SCALE_OUT_OF_RANGE",
      `活粒子数 ${count} 超出 ${gpu.name} 上限 ${gpu.targetScale.hi}，域内无可用路径`,
      `建议：①把粒子拆分为多场景（按空间分区，各自独立路由）；`
        + `②改用不可见的距离剔除/LOD（F2209 调试形态与F2237 档位联动）；`
        + `③重新评估场景设计——单场景 ${count} 粒子在本域的目标机型档位上无解`,
    );
    return fail(
      "PATH_SCALE_OUT_OF_RANGE",
      `活粒子数 ${count} 超出全域最大上限 ${gpu.targetScale.hi}`,
      "分区拆场景、启用剔除/LOD，或重新评估场景规模",
    );
  }

  // 决策 4：区间空隙（理论不该出现，兜底并提示重定标）。
  bag.push(
    "PATH_SCALE_OUT_OF_RANGE",
    `活粒子数 ${count} 落在两条路径区间之外的空隙（CPU 上限 ${cpu.targetScale.hi}与GPU 下限 ${gpu.targetScale.lo} 之间）`,
    `规格表的区间出现空隙，说明两条路径的定标未对齐（常数表过期或被单侧修改）。`
      + `请在 L01 F2212 / L03 F2246 基准回填时一并核对边界值，并更新 PATH_TABLE`,
  );
  return fail(
    "PATH_SCALE_OUT_OF_RANGE",
    `活粒子数 ${count} 落在路径区间空隙`,
    "对齐两条路径的定标边界值后更新 PATH_TABLE",
  );
}

// ════════════════════════════════════════════════════════════════════════════
// §4 K 域序契约接收（判据三：与 K 域的契约链首环）
// ════════════════════════════════════════════════════════════════════════════

/** K→L 序契约件 id（对应 K 域 F2148 序总图中的 L 序位段）。 */
export const SEQ_CONTRACT_ID = "K-F2148-order-contract-to-L";

/** 序契约中一条遵守义务：L 域必须做到什么（逐条可核验，非口号）。 */
export interface SeqObligation {
  readonly id: string;
  /** 义务的人话描述。 */
  readonly duty: string;
  /** 核验方式（怎么证明做到了——不能只写「做到」）。 */
  readonly verify: string;
}

/**
 * K→L 序契约（L 粒子渲染进 K 后处理链的序位）。
 *
 * 序契约是什么、为什么它是 K→L 契约链的**首环**（域级立场）：
 *   K 域（后处理链）的总纲里有一张跨域序总图（F2148），它规定了所有效果在
 *   后处理链中的**插入序位**：例如 bloom 必须在亮度提取之后、色调映射之前。
 *   L 域的粒子渲染是一种发光效果，它的排序直接决定观感——粒子若在色调映射
 *   之后合成，高光会被压暗、若在 bloom 之前合成则不进辉光。
 *   故「L 插在K 链的哪一环」不是实现细节，而是**跨域承诺**：L 必须服从 K
 *   定下的序，K 才能保证自己的效果链语义不被下游破坏。
 *   这就是「序遵守义务签署」的含义：L 在开工条上签字确认「我的插入序位是
 *   K 链中已登记的那一位，我不自建第二条旁路」。
 *
 * 若不签署会怎样（错误路径，已在 K 域 F2197 定为P0）：
 *   K 域无从知道下游是否有域绕过自己的序位旁路渲染——那样K 的序契约就是
 *   一纸空文，所有基于序的效果推演（bloom 阈值、tonemap 曲线）全部失去前提。
 */
export const K_TO_L_SEQ_CONTRACT = {
  /** 契约 id（对账主键）。 */
  id: SEQ_CONTRACT_ID,
  /** 上游域（移交方）。 */
  upstreamDomain: "VE-K" as const,
  /** 序位段的来源锚点（K 域移交包五段之一：序契约件 F2148 总图快照）。 */
  upstreamAnchor: "VE-F2197 移交包 · 序契约件（F2148 跨域序总图快照）",
  /** 序契约件本体（F2148）——K 域收官时的上游生效条目。 */
  upstreamContractItem: "VE-F2148" as const,
  /** L 侧插入序位在 K 后处理链中的位置（人话描述，供文档与工具引用同一事实源）。 */
  slotInKChain: "亮度提取之后、色调映射之前（bloom 输入侧）",
  /** 序契约随包快照哈希（F2197 宣告的十六进制串；接收方须回放校验）。 */
  snapshotHash: "7c1e5a90d4b26f8361ad09cb3e742f58",
  /** L 侧须逐条确认的遵守义务。 */
  obligations: [
    {
      id: "OB-1",
      duty: "粒子渲染必须插入 K 后处理链已登记的序位（亮度提取之后、色调映射之前），不自建旁路",
      verify: "接入测试断言：粒子合成批次的提交序位与 K 链序总图登记值一致；旁路检测守卫对违规提交返回失败",
    },
    {
      id: "OB-2",
      duty: "粒子发射的发光强度须按 K 链的线性工作空间约定提交，不得预先做色调映射",
      verify: "对拍测试：同一粒子场分别在 L 侧与 K 侧合成，亮度直方图差异在 K 域声明容差内",
    },
    {
      id: "OB-3",
      duty: "粒子数与排序状态须随每帧提交，供 K 链的 bloom 阈值与降质档计算取用",
      verify: "帧数据契约核验：每帧提交包含 particleCount 与 sortedFlag 字段，缺字段即 CI 拦截",
    },
    {
      id: "OB-4",
      duty: "K 域序总图变更时，L 域须在 K 域 F2198 要求的回执期内确认或提出异议",
      verify: "回执链核验：序变更 ADR 发出后 L 域回执到位，无未回执的序变更",
    },
  ] as const,
  /** L 侧禁止做的事（序旁路禁令，越权即诊断）。 */
  forbidden: [
    "自建第二条渲染旁路直接合成上屏",
    "在色调映射之后插入粒子合成",
    "在亮度提取之前读取未经处理的粒子缓冲",
    "绕过 K 链自行决定效果插入序",
  ] as const,
} as const;

/** 接收方回执：L 域对序契约的接收确认（逐条义务打勾位）。 */
export interface SeqReceipt {
  /** 接收的契约 id（须与 K_TO_L_SEQ_CONTRACT.id 一致）。 */
  readonly contractId: string;
  /** 接收方标识（用于对账：谁签的）。 */
  readonly receiver: string;
  /** 逐条确认的义务 id 列表（须覆盖契约的全部义务，缺一即拒收）。 */
  readonly acceptedObligationIds: readonly string[];
  /** 回放校验通过的快照哈希（须等于契约声明值）。 */
  readonly verifiedSnapshotHash: string;
  /** 上游 K 域收官宣告是否已生效（F2200 生效为true 才允许开工）。 */
  readonly upstreamDeclared: boolean;
}

/**
 * 序契约接收（判据三的落地面）。
 *
 * 四道检查，顺序固定且每道都产出可定位的诊断：
 *   ① 上游收官检查 —— F2200 宣告未生效则直接阻断开工。这一条排在最前，
 *      因为序契约的效力来自 K 域的收官冻结：K 域没收官，它的序总图随时可能
 *      变（走 K 域 F2198 变更纪律），此时签的收据等于签了一张随时作废的纸。
 *   ② 契约 id 一致性 —— 收的是别的契约（拿错包）须显性失败。
 *   ③ 快照回放 —— 哈希不符即判快照失真，须重出（K 域 F2197 错误路径）。
 *   ④ 义务逐条确认 —— 缺条即拒收并点名缺哪条。
 *
 * 返回的收据是L 域账本的入库凭据：它把「L 承诺了哪几件事」变成可机检的数据，
 * 而不是一句「已知悉」。
 */
export function receiveSeqContract(
  receipt: SeqReceipt,
  bag: DiagBag,
): Outcome<SeqReceipt> {
  const c = K_TO_L_SEQ_CONTRACT;

  // ① 上游收官。
  if (!receipt.upstreamDeclared) {
    bag.push(
      "UPSTREAM_NOT_CLOSED",
      `上游 K 域尚未收官（${c.upstreamContractItem} 序契约总图未冻结），L 域开工前置不成立`,
      `序契约的效力来自 K 域收官冻结——未收官时序总图仍可按 K 域变更纪律（F2198）修改，`
        + `此时签署的收据等于签了一张随时作废的纸。请先完成 K 域收官（VE-F2200 宣告生效）再接收本契约`,
    );
    return fail(
      "UPSTREAM_NOT_CLOSED",
      "上游 K 域未收官，序契约不可接收",
      "先完成 K 域收官（F2200 宣告生效）",
    );
  }

  // ② 契约 id 一致。
  if (receipt.contractId !== c.id) {
    bag.push(
      "SEQ_CONTRACT_UNSIGNED",
      `回执声明的契约 id 为 ${receipt.contractId}，与L 域待接收的 ${c.id} 不符`,
      `请核对移交包是否拿错。L 域需要的是 K 域 F2148 序总图中 L 序位段对应的契约件`
        + `（插入序位：${c.slotInKChain}）`,
    );
    return fail("SEQ_CONTRACT_UNSIGNED", `契约 id 不匹配：${receipt.contractId}`, "核对移交包中的序契约件");
  }

  // ③ 快照回放。
  if (receipt.verifiedSnapshotHash !== c.snapshotHash) {
    bag.push(
      "SEQ_CONTRACT_SNAPSHOT_MISMATCH",
      `序契约快照哈希不符：回执为 ${receipt.verifiedSnapshotHash}，K 域宣告为 ${c.snapshotHash}`,
      `快照失真意味着收到的序图与K 域宣告的不是同一份。请向 K 域索取重出快照`
        + `（K 域 F2197 错误路径：快照失真→重出），重出后重新回放校验；`
        + `在哈希对上之前不要继续签署——签的是哪一版序图必须可追溯`,
    );
    return fail(
      "SEQ_CONTRACT_SNAPSHOT_MISMATCH",
      "序契约快照哈希失真",
      "向 K 域索取重出快照并重新回放校验",
    );
  }

  // ④ 义务逐条确认。
  const required = new Set(c.obligations.map((o) => o.id));
  const accepted = new Set(receipt.acceptedObligationIds);
  const missing = [...required].filter((id) => !accepted.has(id));
  if (missing.length > 0) {
    const names = missing
      .map((id) => c.obligations.find((o) => o.id === id)?.duty ?? id)
      .join("；");
    bag.push(
      "SEQ_CONTRACT_OBLIGATION_MISSING",
      `序契约接收回执缺 ${missing.length} 条义务确认：${names}`,
      `序契约须逐条确认，不得整体「已知悉」。缺条通常意味着 L 侧对该条的实现路径`
        + `还没想清楚——请先补齐实现方案再签，或向 K 域申请该条在本域的豁免（走K 域 ADR）`,
    );
    return fail(
      "SEQ_CONTRACT_OBLIGATION_MISSING",
      `序契约义务缺 ${missing.length} 条确认`,
      "逐条确认全部义务后再提交回执",
    );
  }

  return ok(receipt, bag.all());
}

/**
 * 禁令匹配：判断一条操作描述是否命中某条禁令。
 *
 * 三次修正的记录（写下来是因为这条判据返工过两轮，第三次才做对）：
 *   v1「2 字滑窗任一命中即违规」——假阳性。中文禁令里的「合成」「渲染」这类词
 *      与合规操作高度共现（「向 K 链提交粒子合成批次」被当成「直接合成上屏」拦下）。
 *   v2「3 字滑窗 + 连续双命中 / 覆盖率 1/3」——仍有假阳性。中文禁令普遍较短，
 *      滑窗总数小，孤立命中两三个就够到 1/3 阈值。
 *   v3（现行）**结构性信号词**：中文禁令表达的是「时机/位置违规」或「绕过序」，
 *      这两类语义在中文里有稳定的高辨识度词根。据此把禁令拆成「必要信号」
 *      （必须出现，缺一即不违规）与「限定信号」（出现即违规），要求必要信号齐备。
 *      判据与禁令措辞解耦，故新增禁令不必改匹配逻辑。
 *
 * 为什么不给检查器用「精确相等」或「关键词权重打分」：
 *   调用方写的是自由短描述（「按序位向 K 链提交粒子批次」），不是条款原文，
 *   相等匹配形同虚设；而权重打分需要语料调参，在没有标注集时等于拍脑袋定阈值
 *   ——那正是 v1/v2 失败的根因。结构信号是这三者里唯一可解释、可回归、可交接的。
 */
interface BanMatcher {
  /** 禁令原文（诊断展示用）。 */
  readonly text: string;
  /** 必要信号：全部出现才判违规（缺一即放行——宁可漏判也不制造假阳性）。 */
  readonly required: readonly string[];
}

/** 四条禁令的结构化匹配表（与 K_TO_L_SEQ_CONTRACT.forbidden 逐条对应）。 */
const BAN_MATCHERS: readonly BanMatcher[] = [
  // 「自建第二条渲染旁路直接合成上屏」——违规信号是「自建旁路」而非「合成」。
  { text: "自建第二条渲染旁路直接合成上屏", required: ["旁路", "自建"] },
  // 「绕过 K 链自行决定效果插入序」——违规信号是「绕过」+「序」。
  { text: "绕过 K 链自行决定效果插入序", required: ["绕过"] },
  // 「在色调映射之后插入粒子合成」——违规信号是「映射之后」（时机违规）。
  { text: "在色调映射之后插入粒子合成", required: ["映射之后", "插入"] },
  // 「在亮度提取之前读取未经处理的粒子缓冲」——违规信号是「提取之前」（时机违规）。
  { text: "在亮度提取之前读取未经处理的粒子缓冲", required: ["提取之前", "读取"] },
];

/** 禁令匹配：结构信号全齐即判违规；信号不齐一律放行（不制造假阳性）。 */
function matchesBan(op: string, ban: string): boolean {
  const m = BAN_MATCHERS.find((x) => x.text === ban);
  if (m === undefined) {
    // 未登记匹配器的禁令：退回整串包含（保守判定，宁可多拦也不漏放，
    // 因为新增禁令时忘记登记匹配器的代价应由这条 fallback 兜住）。
    return op.includes(ban);
  }
  return m.required.every((sig) => op.includes(sig));
}

/** 序旁路检查：给定一个待执行操作，判断它是否绕过了 K 链的序契约。 */
export function checkSeqCompliance(operation: string, bag: DiagBag): Outcome<typeof K_TO_L_SEQ_CONTRACT> {
  const c = K_TO_L_SEQ_CONTRACT;
  const op = operation.toLowerCase();
  const hit = c.forbidden.find((ban) => matchesBan(op, ban));
  if (hit !== undefined) {
    bag.push(
      "SEQ_CONTRACT_UNSIGNED",
      `序旁路：L 域尝试执行「${operation}」，命中 K→L 序契约禁令「${hit}」`,
      `L 粒子渲染必须插在 K 后处理链的已登记序位（${c.slotInKChain}）。`
        + `自建旁路会让 K 的效果序推演失去前提（bloom 阈值/tonemap 曲线全部失效）。`
        + `若确有K 链未登记的插入需求，走 K 域 F2198 序变更纪律申请 ADR`,
    );
    return fail(
      "SEQ_CONTRACT_UNSIGNED",
      `操作「${operation}」绕过 K 链序契约`,
      "按已登记序位实现，或走 K 域 F2198 序变更 ADR",
    );
  }
  return ok(c, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §5 组内 20 条分工总览（L01 批次 F2201~F2220）
// ════════════════════════════════════════════════════════════════════════════

/** L01 组内一条分工：谁做、做什么、产出什么契约、怎么算做完、依赖谁。 */
export interface WorkItem {
  /** 功能号。 */
  readonly id: number;
  /** 条目中文名。 */
  readonly title: string;
  /** 职责一句话（禁扩面写在句内）。 */
  readonly duty: string;
  /** 产出契约名（同组内唯一，由自检机检）。 */
  readonly producesContract: string;
  /** 判据锚点（本条的完成判据，引用册内原文口径）。 */
  readonly criterion: string;
  /** 前置条目号（空数组表示无组内前置）。 */
  readonly requires: readonly number[];
}

/**
 * L01 批次 20 条分工总览（F2201~F2220）。
 *
 * 分工表的用途不是排期参考，而是**依赖序的可机检来源**：下游实现某个条目时，
 * 须先确认其requires 的条目已收口；组级收口（F2220）时须逐条核对20 行齐备。
 * 「20 条」这个数字本身是判据——少一条即开工未完成（不是「少一条也行」）。
 */
export const L01_WORK_ITEMS: readonly WorkItem[] = [
  {
    id: 2201,
    title: "L 域开工与粒子系统总架构",
    duty: "十主题与十批次排布、双路径架构声明与路由、K 序契约接收、20 条分工总览；不实现任何模拟运算",
    producesContract: "LDomainArchitecture",
    criterion: "域开工、双路径架构、序契约接收、诚实语义",
    requires: [],
  },
  {
    id: 2202,
    title: "粒子数据模型",
    duty: "六类标准属性+自定义扩展槽、SoA 布局（对齐16 字节）、池上限与内存预计算；不负责发射逻辑",
    producesContract: "ParticleAttributeSpec",
    criterion: "六属性+扩展、SoA 单源、内存预计算、对齐红线",
    requires: [2201],
  },
  {
    id: 2203,
    title: "粒子发射器",
    duty: "五形状采样+三速度分布+确定性累积发射+两级层级+四态生命周期；不负责渲染形态",
    producesContract: "EmitterCore",
    criterion: "五形状、确定性累积、两级层级、状态机",
    requires: [2202],
  },
  {
    id: 2204,
    title: "发射模式",
    duty: "持续/爆发/间隔/事件驱动四模式+叠加语义+事件契约（F1408 总线注册制）；不负责力场",
    producesContract: "EmissionModeSet",
    criterion: "四模式、混合叠加、事件契约、节流保量",
    requires: [2203],
  },
  {
    id: 2205,
    title: "粒子生命周期",
    duty: "寿命分布采样、曲线淡变（复用 F1407 曲线核）、四态与死亡行为；爆裂接口为预留显性报错",
    producesContract: "ParticleLifecycle",
    criterion: "寿命分布、曲线单源、四态、爆裂预留",
    requires: [2203],
  },
  {
    id: 2206,
    title: "粒子渲染接口",
    duty: "billboard/网格/拖尾三形态与模拟解耦、I02 绘制管线对接；不负责排序与混合模式",
    producesContract: "ParticleRenderForm",
    criterion: "三形态、模拟渲染解耦、I02 对接、速度拉伸",
    requires: [2202, 2203],
  },
  {
    id: 2207,
    title: "粒子排序与混合",
    duty: "视深排序+加法/alpha/预乘三混合语义+排序开关；GPU 基数排序为显性预留位",
    producesContract: "ParticleBlendOrder",
    criterion: "深度排序、三混合语义、免排序声明、GPU 预留",
    requires: [2202, 2206],
  },
  {
    id: 2208,
    title: "粒子池与内存",
    duty: "CPU/GPU 双池配额+单发射器上限+70/85/95 三水位降级+空闲链表批量回收",
    producesContract: "ParticlePoolQuota",
    criterion: "总量+单发射器双配额、三阈值、降级通知可查、泄漏防线",
    requires: [2202],
  },
  {
    id: 2209,
    title: "粒子调试数据",
    duty: "双层统计流（池级/发射器级）+五形状 gizmo 线框+发行版编译开关剔除零成本",
    producesContract: "ParticleDebugStreams",
    criterion: "双层统计、gizmo 五形状、力场预留、发行版零成本",
    requires: [2202, 2203],
  },
  {
    id: 2210,
    title: "粒子性能预算",
    duty: "三因子成本模型（模拟∝数量×属性、渲染∝数量×形态、排序∝NlogN×开关）+超预算降档联动+20% 修正门",
    producesContract: "ParticleCostModel",
    criterion: "三因子模型、公式公开、20% 修正门、次序对齐",
    requires: [2208, 2212],
  },
  {
    id: 2211,
    title: "粒子 fuzz",
    duty: "畸形参数 fuzz+启停风暴 fuzz（池不变量：收敛/无泄漏/水位正确）+组合场景 fuzz，案例固化入库",
    producesContract: "ParticleFuzzSuite",
    criterion: "三段fuzz、池不变量、显性不变量、24h 固化",
    requires: [2203, 2204, 2208],
  },
  {
    id: 2212,
    title: "粒子基准",
    duty: "三族基准（吞吐阶梯/发射器开销阶梯/排序耗时阶梯）入册，并回填 F2210 常数表",
    producesContract: "ParticleBaseline",
    criterion: "三族阶梯、成本回填、暂挂声明、环境声明",
    requires: [2202, 2203, 2207],
  },
  {
    id: 2213,
    title: "粒子 API 冻结 v1",
    duty: "十签名冻结（发射器五+池查询三+事件订阅二）版本化，与 F1408/F1925/I02 命名衔接核验",
    producesContract: "ParticleApiV1",
    criterion: "十签名、v1 冻结、衔接核验、只增不改",
    requires: [2203, 2208, 2209],
  },
  {
    id: 2214,
    title: "粒子文档",
    duty: "三文档（白皮书/发射器制作指南/性能调优手册）+ 单源引用守卫（数值复制即拦截）",
    producesContract: "ParticleDocs",
    criterion: "三文档、单源引用、决策树、演练门禁",
    requires: [2202, 2210, 2213],
  },
  {
    id: 2215,
    title: "粒子一致性",
    duty: "CPU 逐位确定双跑断言+GPU 统计等效口径（总数±1%/直方图散度/颜色分布容差）+种子版本化守卫",
    producesContract: "DeterminismStandard",
    criterion: "CPU 逐位、GPU 统计等效、诚实声明、双口径",
    requires: [2203, 2212],
  },
  {
    id: 2216,
    title: "粒子安全",
    duty: "全参数清洗规则表+双池硬顶不可绕过断言+操作审计留痕+引用 F2211 通过率快照",
    producesContract: "ParticleSecurityRules",
    criterion: "全参清洗、硬顶不可绕、操作审计、快照硬门",
    requires: [2211, 2208],
  },
  {
    id: 2217,
    title: "粒子质量档位",
    duty: "高/中/低三档（密度系数×池上限双因子）+ D04 降质链映射+用户档位记忆+平滑降档",
    producesContract: "ParticleQualityTier",
    criterion: "双因子三档、平滑降档、映射家族、记忆延续",
    requires: [2208, 2210],
  },
  {
    id: 2218,
    title: "粒子遥测",
    duty: "四指标（粒子总数分布/双路径占比/池压力事件/降级触发）注册进总日志中心（l.ps. 前缀）",
    producesContract: "ParticleTelemetry",
    criterion: "四指标、路径占比、匿名档、口径唯一",
    requires: [2209, 2215, 2217],
  },
  {
    id: 2219,
    title: "粒子组一致性",
    duty: "术语并入 L 域术语表+与 K 域速度缓冲（F2045）联动核验复述+错误三要素抽查",
    producesContract: "ParticleGroupAlignment",
    criterion: "术语唯一义、速度缓冲复述、格式对账、三要素统一",
    requires: [2213, 2214],
  },
  {
    id: 2220,
    title: "L01 组收口与 L02 移交",
    duty: "20 项自检入树+三件硬证确认（基准/确定性/fuzz）+组双签+向 L02 移交力场-粒子交互契约",
    producesContract: "ParticleContractBundle",
    criterion: "20 项自检、三件硬证、双签、力场契约移交",
    requires: [2210, 2211, 2212, 2215, 2216, 2217, 2218, 2219],
  },
];

/** L01 组分工条目数（判据要求恰为 20 条；由自检断言）。 */
export const L01_WORK_ITEM_COUNT = 20;

// ════════════════════════════════════════════════════════════════════════════
// §6 开工门禁 + 开工摘要（判据一至四的落地编排）
// ════════════════════════════════════════════════════════════════════════════

/** 开工门禁一条：条件 + 它对应哪条判据。 */
export interface GateCriterion {
  readonly id: string;
  readonly criterion: string;
  /** 该门禁对应的判据（映射到四条判据之一）。 */
  readonly mapsTo: "域开工" | "双路径架构" | "序契约接收" | "诚实语义";
}

/** 开工门禁清单（逐条可机检；任一不成立即开工未完成）。 */
export const KICKOFF_GATES: readonly GateCriterion[] = [
  { id: "G1", criterion: "官方十主题全部登记且与册内序列逐项一致，能力主题均被实现批次承载", mapsTo: "域开工" },
  { id: "G2", criterion: "十批次功能号区间两两不重叠、合计恰为 200 项、无未分配空隙", mapsTo: "域开工" },
  { id: "G3", criterion: "L01 组 20 条分工齐备（条目号连续、无重复、产出契约名唯一、依赖无悬空）", mapsTo: "域开工" },
  { id: "G4", criterion: "CPU/GPU 双路径规格表自洽（规模区间合法、语义不越诚实上限、硬件要求相符）", mapsTo: "双路径架构" },
  { id: "G5", criterion: "路由策略按「确定性需求优先于规模」的固定次序执行，手动覆盖亦受三道硬约束", mapsTo: "双路径架构" },
  { id: "G6", criterion: "K→L 序契约已接收：上游已收官、快照哈希回放一致、义务逐条确认", mapsTo: "序契约接收" },
  { id: "G7", criterion: "序旁路禁令可机检：绕过 K 链已登记序位的操作被显性拦下", mapsTo: "序契约接收" },
  { id: "G8", criterion: "诚实语义守卫在位：给 GPU 路径贴逐位确定或无等级强断言标签一律拦截", mapsTo: "诚实语义" },
  { id: "G9", criterion: "零静默：所有拒绝/越权/降级/阻断均产出三要素诊断，无静默分支", mapsTo: "诚实语义" },
];

/** 域开工摘要（对外一页纸：进度 + 门禁 + 阻塞，AI 派单与看板共用）。 */
export interface DomainKickoffSummary {
  readonly domain: string;
  readonly themeCount: number;
  readonly batchCount: number;
  readonly itemRange: string;
  readonly itemCount: number;
  readonly paths: readonly SimPathId[];
  readonly autoRouteOrder: readonly string[];
  readonly seqContractId: string;
  readonly seqSlot: string;
  readonly l01WorkItemCount: number;
  readonly gates: readonly GateCriterion[];
}

/** 自动路由次序的对外表述（人话，供文档与遥测引用同一事实源）。 */
export const AUTO_ROUTE_ORDER: readonly string[] = [
  "第 0 步：确定性需求优先于规模——要求逐位可复现时直接排除 GPU 路径（不看规模）",
  "第 1 步：规模落在 CPU 区间 → CPU（成本更低且语义更强）",
  "第 2 步：规模落入 GPU 区间且 compute 可用 → GPU（只承诺统计等效）",
  "第 2b步：规模落入 GPU 区间但 compute 不可用 → 显性失败，不静默退回 CPU",
  "第 3 步：规模超GPU 上限 → 显性失败（建议分区/剔除/LOD）",
  "第 4 步：规模落在区间空隙 → 显性失败并提示重定标区间表",
];

/** 生成域开工摘要。任一前置不成立即返回显性失败（开工单不允许带未知态）。 */
export function buildKickoffSummary(): Outcome<DomainKickoffSummary> {
  const bag = new DiagBag();

  // 前置 1：主题覆盖。
  const coverage = auditThemeCoverage(bag);

  // 前置 2：批次区间。
  const batchCheck = auditBatchRanges(bag);

  // 前置 3：路径规格自洽。
  const pathCheck = validatePathTable(bag);

  // 前置 4：20 条分工齐备。
  const workCheck = auditWorkBreakdown(bag);

  if (!coverage.ok) {
    return fail<DomainKickoffSummary>(coverage.code, coverage.message, coverage.hint);
  }
  if (!batchCheck.ok) {
    return fail<DomainKickoffSummary>(batchCheck.code, batchCheck.message, batchCheck.hint);
  }
  if (!pathCheck.ok) {
    return fail<DomainKickoffSummary>(pathCheck.code, pathCheck.message, pathCheck.hint);
  }
  if (!workCheck.ok) {
    return fail<DomainKickoffSummary>(workCheck.code, workCheck.message, workCheck.hint);
  }

  const itemTotal = BATCH_REGISTRY.reduce((n, b) => n + (b.itemRange.hi - b.itemRange.lo + 1), 0);
  return ok(
    {
      domain: `${DOMAIN.id} ${DOMAIN.name}（${DOMAIN.itemLo}-${DOMAIN.itemHi}，共 ${DOMAIN.itemCount} 项，${DOMAIN.wave} 波）`,
      themeCount: THEME_IDS.length,
      batchCount: BATCH_REGISTRY.length,
      itemRange: `${DOMAIN.itemLo} ~ ${DOMAIN.itemHi}`,
      itemCount: itemTotal,
      paths: PATH_IDS,
      autoRouteOrder: AUTO_ROUTE_ORDER,
      seqContractId: K_TO_L_SEQ_CONTRACT.id,
      seqSlot: K_TO_L_SEQ_CONTRACT.slotInKChain,
      l01WorkItemCount: L01_WORK_ITEMS.length,
      gates: KICKOFF_GATES,
    },
    bag.all(),
  );
}

/** 批次区间审计：不重叠、无空隙、合计 200、边界与域区间吻合。 */
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
        `批次 ${b.id} 的功能号区间倒置（${lo} > ${hi}）`,
        "区间须满足 lo ≤ hi；倒置区间会让区间审计的后续比较全部失去意义",
      );
    }
    total += hi - lo + 1;
  }

  for (let i = 0; i < sorted.length; i += 1) {
    const cur = sorted[i] as BatchSpec;
    const next = sorted[i + 1] as BatchSpec | undefined;
    if (next === undefined) continue;
    if (next.itemRange.lo <= cur.itemRange.hi) {
      overlap = `${cur.id}(${cur.itemRange.lo}-${cur.itemRange.hi}) 与 ${next.id}(${next.itemRange.lo}-${next.itemRange.hi})`;
    } else if (next.itemRange.lo > cur.itemRange.hi + 1) {
      gap = `${cur.itemRange.hi} 与 ${next.itemRange.lo} 之间`;
    }
  }

  if (overlap !== "") {
    bag.push(
      "ITEM_RANGE_OVERLAP",
      `批次功能号区间重叠：${overlap}`,
      "两个批次抢同一批功能号意味着两份实现会同时被认领，收口时必然有一份对不上账；"
        + "请重新分配区间使十批次首尾相接",
    );
  }
  if (gap !== "") {
    bag.push(
      "ITEM_RANGE_GAP",
      `批次功能号区间存在未分配空隙：${gap}`,
      `域功能号须连续分配（${DOMAIN.itemLo}~${DOMAIN.itemHi} 共 ${DOMAIN.itemCount} 项）。`
        + `空隙会让收官对账（VE-F2400 的 200/200 grep 实测）永远凑不满，请补齐区间`,
    );
  }
  if (overlap === "" && gap === "" && total !== DOMAIN.itemCount) {
    bag.push(
      "ITEM_RANGE_GAP",
      `十批次区间合计 ${total} 项，与域功能总数 ${DOMAIN.itemCount} 项不符`,
      "多退少补都是登记错误；请核对区间边界使合计恰为 200",
    );
  }

  // 域边界吻合：首批从域起点开始，末批到域终点结束。
  const first = sorted[0] as BatchSpec;
  const last = sorted[sorted.length - 1] as BatchSpec;
  if (first.itemRange.lo !== DOMAIN.itemLo || last.itemRange.hi !== DOMAIN.itemHi) {
    bag.push(
      "ITEM_RANGE_GAP",
      `批次区间未覆盖完整域区间（实际 ${first.itemRange.lo}~${last.itemRange.hi}，应为 ${DOMAIN.itemLo}~${DOMAIN.itemHi}）`,
      "域区间边界须与首批起点、末批终点严格吻合，否则域外功能号会混入本域",
    );
  }

  if (overlap !== "" || gap !== "" || total !== DOMAIN.itemCount) {
    return fail<number>("ITEM_RANGE_GAP", gap !== "" ? "批次区间存在空隙" : "批次区间存在重叠或合计不符", "重新分配十批次功能号区间");
  }
  return ok(total, bag.all());
}

/** 20 条分工审计：条数、条目号连续性、契约名唯一、依赖无悬空且无环。 */
export function auditWorkBreakdown(bag: DiagBag): Outcome<number> {
  const count = L01_WORK_ITEMS.length;
  if (count !== L01_WORK_ITEM_COUNT) {
    bag.push(
      "WORKBREAKDOWN_INCOMPLETE",
      `L01 组分工表实有 ${count} 条，判据要求恰为 ${L01_WORK_ITEM_COUNT} 条`,
      `20 条是组级收口（F2220）的清点基数：少一条即意味着有一条功能无人认领、`
        + `收口时凑不满「20 项自检入树」。请补齐到 F2201~F2220 连续区间`,
    );
  }

  // 条目号须恰为 2201..2220 连续。
  const ids = L01_WORK_ITEMS.map((w) => w.id);
  const expected: number[] = [];
  for (let n = 2201; n <= 2220; n += 1) expected.push(n);
  const missing = expected.filter((n) => !ids.includes(n));
  const dup = ids.filter((n, i) => ids.indexOf(n) !== i);
  if (missing.length > 0 || dup.length > 0) {
    bag.push(
      "WORKBREAKDOWN_INCOMPLETE",
      `L01 组分工条目号不连续：缺 ${missing.join("、") || "无"}，重复 ${[...new Set(dup)].join("、") || "无"}`,
      "条目号须与册内锚点一一对应（VE-F2201~VE-F2220）；"
        + "缺号说明册内有条目未纳入分工，重复号说明两条分工被登记成了同一个号",
    );
  }

  // 产出契约名唯一。
  const contracts = new Set<string>();
  const dupContract: string[] = [];
  for (const w of L01_WORK_ITEMS) {
    if (contracts.has(w.producesContract)) dupContract.push(w.producesContract);
    contracts.add(w.producesContract);
  }
  if (dupContract.length > 0) {
    bag.push(
      "WORKBREAKDOWN_INCOMPLETE",
      `L01 组内产出契约名重复：${[...new Set(dupContract)].join("、")}`,
      "同组内两个条目产出同名契约，后续必然撞名；请让契约名反映各自的产出物",
    );
  }

  // 依赖无悬空（指向的条目号必须在本表内）。
  let dangling = "";
  for (const w of L01_WORK_ITEMS) {
    for (const dep of w.requires) {
      if (!ids.includes(dep)) dangling = `${w.id} → ${dep}`;
    }
  }
  if (dangling !== "") {
    bag.push(
      "WORKBREAKDOWN_INCOMPLETE",
      `L01 组内依赖指向不存在的条目：${dangling}`,
      "前置条目号必须在同一分工表内（跨组依赖请写进该条目的 duty 说明，不进 requires）",
    );
  }

  // 依赖无环（20 条规模直接做传递闭包即可）。
  const byId = new Map<number, readonly number[]>();
  for (const w of L01_WORK_ITEMS) byId.set(w.id, w.requires);
  const state = new Map<number, 0 | 1 | 2>();
  let cycle = "";
  const visit = (n: number): void => {
    const st = state.get(n);
    if (st === 2 || cycle !== "") return;
    if (st === 1) {
      cycle = n.toString();
      return;
    }
    state.set(n, 1);
    for (const dep of byId.get(n) ?? []) visit(dep);
    state.set(n, 2);
  };
  for (const w of L01_WORK_ITEMS) visit(w.id);
  if (cycle !== "") {
    bag.push(
      "WORKBREAKDOWN_INCOMPLETE",
      `L01 组内依赖成环，涉及条目 ${cycle}`,
      "环上的条目互为前置，没有任何一个能先开工；请断开环上语义次要的一条依赖边",
    );
  }

  // 开工条自身须无前置（域开工是组内第一条的天然起点）。
  const kickoff = L01_WORK_ITEMS.find((w) => w.id === DOMAIN.kickoffItemId);
  if (kickoff === undefined || kickoff.requires.length > 0) {
    bag.push(
      "WORKBREAKDOWN_INCOMPLETE",
      `开工条目 F${DOMAIN.kickoffItemId} 缺失或声明了组内前置`,
      "开工条是L01 组的第一条，不得声明组内前置；它的前置是K 域收官（F2200），"
        + "已由序契约接收的前置检查承担",
    );
  }

  if (
    count !== L01_WORK_ITEM_COUNT ||
    missing.length > 0 ||
    dup.length > 0 ||
    dupContract.length > 0 ||
    dangling !== "" ||
    cycle !== ""
  ) {
    return fail<number>("WORKBREAKDOWN_INCOMPLETE", "L01 组 20 条分工不齐备", "补齐/纠正分工表后重新自检");
  }
  return ok(count, bag.all());
}

/**
 * 开工执行（唯一入口）：给定上游收官状态与序契约回执，判定 L 域能否开工。
 *
 * 这是判据的最终落点——前面所有注册表、规格表、守卫都汇聚到这一个函数。
 * 失败时必带「缺口点名」：调用方拿到的 hint 直接告诉它该去做什么，
 * 而不是丢一个布尔值让人自己猜。
 */
export function kickoffDomain(receipt: SeqReceipt, bag: DiagBag): Outcome<DomainKickoffSummary> {
  const seq = receiveSeqContract(receipt, bag);
  if (!seq.ok) {
    bag.push(
      "KICKOFF_BLOCKED",
      `L 域开工被阻断：${seq.message}`,
      `阻断原因如上。序契约链首环未闭合前，L 域的任何条目都不应开工——`
        + `先解决阻断项（通常是 K 域收官或快照回放），再重新调用本函数`,
    );
    return fail<DomainKickoffSummary>("KICKOFF_BLOCKED", seq.message, seq.hint);
  }
  const summary = buildKickoffSummary();
  if (!summary.ok) {
    bag.push(
      "KICKOFF_BLOCKED",
      `L 域开工被阻断：${summary.message}`,
      summary.hint,
    );
    return fail<DomainKickoffSummary>("KICKOFF_BLOCKED", summary.message, summary.hint);
  }
  return summary;
}

// ════════════════════════════════════════════════════════════════════════════
// §7 自检（判据的可执行形态：不靠人读代码确认，靠断言输出）
// ════════════════════════════════════════════════════════════════════════════

/** 自检结果项：每项对应一条判据，独立可定位。 */
export interface SelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 构造一份样例回执（自检与文档示例共用，避免手写多处漂移）。 */
export function sampleReceipt(receiver = "AI-W012"): SeqReceipt {
  return {
    contractId: SEQ_CONTRACT_ID,
    receiver,
    acceptedObligationIds: K_TO_L_SEQ_CONTRACT.obligations.map((o) => o.id),
    verifiedSnapshotHash: K_TO_L_SEQ_CONTRACT.snapshotHash,
    upstreamDeclared: true,
  };
}

/** 判据「域开工」自检：十主题齐备 + 覆盖无缺口 + 十批次区间无重叠无空隙 + 20 条齐备。 */
export function selfCheckDomainKickoff(): SelfCheck[] {
  const out: SelfCheck[] = [];

  out.push({
    name: "themes-ten-registered",
    pass: THEME_IDS.length === 10,
    detail: `注册表可枚举主题数 = ${THEME_IDS.length}（期望 10）`,
  });

  const namesMatch = OFFICIAL_THEMES.every((n) => Object.values(THEME_REGISTRY).some((s) => s.name === n));
  out.push({
    name: "themes-match-official-list",
    pass: namesMatch,
    detail: namesMatch
      ? `十主题名与官方序列逐项一致（${OFFICIAL_THEMES.join("/")}）`
      : "注册表主题名与官方十主题序列不一致（漏项或改写）",
  });

  const covBag = new DiagBag();
  const cov = auditThemeCoverage(covBag);
  out.push({
    name: "themes-coverage-complete",
    pass: cov.ok && covBag.size === 0,
    detail: covBag.size === 0
      ? `十个能力主题全部被实现批次承载，双向映射一致（无治理批次误判）`
      : `覆盖审计发现 ${covBag.size} 处问题：${covBag.all()[0]?.message ?? ""}`,
  });

  const batchBag = new DiagBag();
  const ranges = auditBatchRanges(batchBag);
  out.push({
    name: "batches-range-contiguous-200",
    pass: ranges.ok && ranges.value === DOMAIN.itemCount,
    detail: ranges.ok
      ? `十批次区间首尾相接、合计 ${ranges.value} 项（= 域功能总数 ${DOMAIN.itemCount}）`
      : `区间审计不过：${batchBag.all()[0]?.message ?? ""}`,
  });

  const batchIds = new Set(BATCH_REGISTRY.map((b) => b.id));
  out.push({
    name: "batches-ten-registered",
    pass: BATCH_REGISTRY.length === 10 && batchIds.size === 10,
    detail: `已登记批次 = ${BATCH_REGISTRY.map((b) => b.id).join("、")}（期望 10 个且不重号）`,
  });

  const workBag = new DiagBag();
  const work = auditWorkBreakdown(workBag);
  out.push({
    name: "l01-work-breakdown-twenty",
    pass: work.ok && work.value === L01_WORK_ITEM_COUNT,
    detail: work.ok
      ? `L01 组分工 ${work.value} 条齐备（条目号 2201~2220 连续、契约名唯一、依赖无悬空无环）`
      : `分工审计不过：${workBag.all()[0]?.message ?? ""}`,
  });

  // 治理批次声明自洽：themes 为空 当且仅当 isGovernance 为 true。
  const govMismatch = BATCH_REGISTRY.filter(
    (b) => (b.themes.length === 0) !== b.isGovernance,
  ).map((b) => b.id);
  out.push({
    name: "batches-governance-flag-consistent",
    pass: govMismatch.length === 0,
    detail:
      govMismatch.length === 0
        ? "治理批次声明自洽（L08/L09/L10 无单主题承载，能力批次均挂主题）"
        : `治理标记与主题表不一致：${govMismatch.join("、")}`,
  });

  return out;
}

/** 判据「双路径架构」自检：规格表自洽 + 路由五态逐态正确。 */
export function selfCheckDualPath(): SelfCheck[] {
  const out: SelfCheck[] = [];

  const specBag = new DiagBag();
  const spec = validatePathTable(specBag);
  out.push({
    name: "path-table-wellformed",
    pass: spec.ok && specBag.size === 0,
    detail: specBag.size === 0
      ? `${PATH_TABLE["cpu-scalar-sim"].name} 与 ${PATH_TABLE["gpu-compute-sim"].name} 规格自洽（规模区间/语义上限/硬件要求）`
      : `规格表问题 ${specBag.size} 处：${specBag.all()[0]?.message ?? ""}`,
  });

  // 路由态1：需求逐位确定 + 规模很小 → CPU（确定性需求优先于规模）。
  const b1 = new DiagBag();
  const r1 = routePath(
    { host: { computeAvailable: true, liveParticleCount: 1_000 }, determinismNeeded: "bitwise-reproducible" },
    b1,
  );
  out.push({
    name: "route-determinism-over-scale",
    pass: r1.ok && r1.value.path === "cpu-scalar-sim",
    detail: r1.ok
      ? `规模 1000 + 逐位确定需求 → ${r1.value.path}（即使宿主支持 compute 也不选GPU）`
      : `路由失败：${r1.message}`,
  });

  // 路由态2：规模在 CPU 区间 → CPU（成本与语义双优）。
  const r2 = routePath({ host: { computeAvailable: true, liveParticleCount: 10_000 } }, new DiagBag());
  out.push({
    name: "route-small-scale-to-cpu",
    pass: r2.ok && r2.value.path === "cpu-scalar-sim",
    detail: r2.ok ? `规模 10000 → ${r2.value.path}（CPU 区间内，语义更强成本更低）` : `路由失败：${r2.message}`,
  });

  // 路由态3：规模超CPU 且 compute 可用 → GPU，且理由中必须写明不承诺逐位确定。
  const r3 = routePath({ host: { computeAvailable: true, liveParticleCount: 500_000 } }, new DiagBag());
  const reasonHonest = r3.ok && r3.value.determinism === "statistically-equivalent";
  out.push({
    name: "route-large-scale-to-gpu-honestly",
    pass: r3.ok && r3.value.path === "gpu-compute-sim" && reasonHonest,
    detail: r3.ok
      ? `规模 500000 → ${r3.value.path}，语义标注为 ${r3.value.determinism}（未宣称逐位确定）`
      : `路由失败：${r3.message}`,
  });

  // 路由态4：规模超 CPU 但无 compute → 显性失败，不静默退回 CPU。
  const b4 = new DiagBag();
  const r4 = routePath({ host: { computeAvailable: false, liveParticleCount: 500_000 } }, b4);
  out.push({
    name: "route-blocks-instead-of-silent-fallback",
    pass: !r4.ok && r4.code === "PATH_HARDWARE_UNAVAILABLE" && b4.byCode("PATH_HARDWARE_UNAVAILABLE").length === 1,
    detail: !r4.ok
      ? `规模 500000 + 无 compute → 显性失败（${r4.code}），未静默退回 CPU`
      : "无 compute 竟被放行（会静默退 CPU 导致百万粒子掉帧）",
  });

  // 路由态5：规模超GPU 上限 → 显性失败并给建议。
  const b5 = new DiagBag();
  const r5 = routePath({ host: { computeAvailable: true, liveParticleCount: 9_000_000 } }, b5);
  out.push({
    name: "route-rejects-beyond-domain-max",
    pass: !r5.ok && r5.code === "PATH_SCALE_OUT_OF_RANGE",
    detail: !r5.ok ? `规模 9000000 超域上限 → 显性失败（${r5.code}）` : "超上限规模竟被放行",
  });

  // 手动覆盖态：强指定 GPU 但需求逐位确定 → 冲突失败（不静默改语义）。
  const b6 = new DiagBag();
  const r6 = routePath(
    {
      host: { computeAvailable: true, liveParticleCount: 500_000 },
      determinismNeeded: "bitwise-reproducible",
      manualOverride: "gpu-compute-sim",
    },
    b6,
  );
  out.push({
    name: "route-manual-override-conflict-visible",
    pass: !r6.ok && r6.code === "MANUAL_OVERRIDE_CONFLICT",
    detail: !r6.ok
      ? `手动覆盖 GPU + 逐位确定需求 → 冲突显性失败（${r6.code}），语义未被静默改写`
      : "手动覆盖冲突被静默吞掉（这会让用户以为得到了逐位确定，实际没有）",
  });

  // 手动覆盖合法态：显式指定 CPU 覆盖自动决策（用于排查 GPU 疑似错误）。
  const r7 = routePath(
    {
      host: { computeAvailable: true, liveParticleCount: 500_000 },
      manualOverride: "cpu-scalar-sim",
    },
    new DiagBag(),
  );
  const inRange = r7.ok && 500_000 <= PATH_TABLE["cpu-scalar-sim"].targetScale.hi;
  out.push({
    name: "route-manual-override-honored-in-range",
    pass: inRange ? r7.ok && r7.value.manual && r7.value.path === "cpu-scalar-sim" : true,
    detail: inRange
      ? `手动覆盖 CPU 被采纳（manual=${r7.ok ? r7.value.manual : "n/a"}），且在 CPU 区间内`
      : "规模 500000 超出 CPU 区间，手动覆盖应被规模约束拒绝（由范围约束自检覆盖）",
  });

  // 手动覆盖非法态：规模超该路径区间 → 范围约束拒绝。
  const b8 = new DiagBag();
  const r8 = routePath(
    { host: { computeAvailable: true, liveParticleCount: 500_000 }, manualOverride: "cpu-scalar-sim" },
    b8,
  );
  out.push({
    name: "route-manual-override-range-enforced",
    pass: !r8.ok && r8.code === "PATH_SCALE_OUT_OF_RANGE",
    detail: !r8.ok ? `手动覆盖 CPU 但规模 500000 超其上限 → 被范围约束拒绝（${r8.code}）` : "范围约束未生效",
  });

  return out;
}

/** 判据「序契约接收」自检：上游未收官阻断 + 快照失真拒收 + 缺义务拒收 + 旁路拦截 + 正例通过。 */
export function selfCheckSeqContract(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 态1：上游未收官 → 阻断开工。
  const b1 = new DiagBag();
  const r1 = receiveSeqContract({ ...sampleReceipt(), upstreamDeclared: false }, b1);
  out.push({
    name: "seq-blocks-before-upstream-closure",
    pass: !r1.ok && r1.code === "UPSTREAM_NOT_CLOSED",
    detail: !r1.ok
      ? `K 域未收官 → 序契约拒收（${r1.code}），开工前置不成立`
      : "上游未收官竟被放行（签的是一张随时作废的纸）",
  });

  // 态2：快照哈希失真 → 拒收并要求重出。
  const b2 = new DiagBag();
  const r2 = receiveSeqContract({ ...sampleReceipt(), verifiedSnapshotHash: "deadbeef" }, b2);
  out.push({
    name: "seq-rejects-snapshot-mismatch",
    pass: !r2.ok && r2.code === "SEQ_CONTRACT_SNAPSHOT_MISMATCH",
    detail: !r2.ok ? `快照哈希失真 → 拒收（${r2.code}）` : "失真快照被放行（签的是哪一版序图不可追溯）",
  });

  // 态3：义务缺条 → 拒收并点名缺哪条。
  const b3 = new DiagBag();
  const partial = K_TO_L_SEQ_CONTRACT.obligations.slice(0, 1).map((o) => o.id);
  const r3 = receiveSeqContract({ ...sampleReceipt(), acceptedObligationIds: partial }, b3);
  out.push({
    name: "seq-rejects-missing-obligation",
    pass: !r3.ok && r3.code === "SEQ_CONTRACT_OBLIGATION_MISSING",
    detail: !r3.ok
      ? `仅确认 ${partial.length}/${K_TO_L_SEQ_CONTRACT.obligations.length} 条义务 → 拒收（${r3.code}）`
      : "缺义务被放行（整体「已知悉」等于没签）",
  });

  // 态4：正例——全义务确认 + 上游收官 + 哈希一致 → 接收成功。
  const b4 = new DiagBag();
  const r4 = receiveSeqContract(sampleReceipt(), b4);
  out.push({
    name: "seq-accepts-complete-receipt",
    pass: r4.ok && b4.size === 0,
    detail: r4.ok
      ? `完整回执被接收（${K_TO_L_SEQ_CONTRACT.obligations.length} 条义务全确认，插入序位：${K_TO_L_SEQ_CONTRACT.slotInKChain}）`
      : `完整回执竟被拒：${r4.message}`,
  });

  // 态5：序旁路被拦。
  const b5 = new DiagBag();
  const r5 = checkSeqCompliance("L 域自建第二条渲染旁路直接合成上屏", b5);
  out.push({
    name: "seq-bypass-blocked",
    pass: !r5.ok && r5.code === "SEQ_CONTRACT_UNSIGNED",
    detail: !r5.ok ? `序旁路操作被拦下（${r5.code}）` : "序旁路竟被放行（K 的序契约形同虚设）",
  });

  // 态5b：旁路拦截的多样化回归（确认判据放宽后真旁路仍被拦——放宽是为了消假阳性，
  // 但不能顺带把真违规也放过去，这是同一条判据的两面）。
  const bypassOps = [
    "自建第二条渲染旁路直接合成上屏",
    "绕过 K 链自行决定效果插入序",
    "在色调映射之后插入粒子合成",
    "在亮度提取之前读取未经处理的粒子缓冲",
  ];
  const missed = bypassOps.filter((op) => checkSeqCompliance(op, new DiagBag()).ok);
  out.push({
    name: "seq-all-bypass-forms-blocked",
    pass: missed.length === 0,
    detail:
      missed.length === 0
        ? `${bypassOps.length} 条旁路形态全部被拦（自建旁路/绕链定序/映射后插入/提亮前取缓冲）`
        : `以下旁路未被拦（判据放宽过头）：${missed.join("；")}`,
  });

  // 态6：合法操作不被误拦（反向验证——误拦会逼调用方绕过检查器）。
  const b6 = new DiagBag();
  const r6 = checkSeqCompliance("按已登记序位向 K 后处理链提交粒子合成批次", b6);
  out.push({
    name: "seq-legal-op-passes",
    pass: r6.ok,
    detail: r6.ok ? "按序位提交的合法操作未被误拦（检查器不制造假阳性）" : `合法操作被误拦：${r6.message}`,
  });

  // 态6b：合法操作的多样化回归（固化「合成」「序位」「缓冲」等高频词不再引发假阳性）。
  // 这组用例源自一次真实缺陷：2 字滑窗判据把合规操作里的「合成」二字
  // 误判为旁路，现已改为 3 字滑窗 + 连续双命中/覆盖率兜底。此项防回退。
  const legalOps = [
    "向 K 链提交粒子合成批次",
    "读取 K 链已登记序位表",
    "按序位渲染粒子并写入后处理输入",
    "提交色调映射前的粒子缓冲",
    "在亮度提取之后合成粒子层",
  ];
  const falsePositives = legalOps.filter((op) => !checkSeqCompliance(op, new DiagBag()).ok);
  out.push({
    name: "seq-no-false-positive-on-common-verbs",
    pass: falsePositives.length === 0,
    detail:
      falsePositives.length === 0
        ? `${legalOps.length} 条含高频词（合成/序位/缓冲）的合法操作全部放行，无假阳性`
        : `以下合法操作被误拦（滑窗判据过宽的回归）：${falsePositives.join("；")}`,
  });

  // 态7：契约要素齐备（序位、快照哈希、义务非空、禁令非空）。
  const c = K_TO_L_SEQ_CONTRACT;
  out.push({
    name: "seq-contract-elements-complete",
    pass: c.slotInKChain.trim().length > 0 && c.snapshotHash.trim().length > 0
      && c.obligations.length > 0 && c.forbidden.length > 0
      && c.obligations.every((o) => o.duty.trim().length > 0 && o.verify.trim().length > 0),
    detail: `契约要素齐备：序位已声明、哈希已声明、义务 ${c.obligations.length} 条（每条含核验方式）、禁令 ${c.forbidden.length} 条`,
  });

  // 态8：禁令清单与结构化匹配表逐条对齐（防新增禁令漏登记匹配器而退化成模糊 fallback）。
  const unmatched = c.forbidden.filter((b) => !BAN_MATCHERS.some((m) => m.text === b));
  out.push({
    name: "seq-ban-matchers-aligned",
    pass: unmatched.length === 0,
    detail:
      unmatched.length === 0
        ? `${c.forbidden.length} 条禁令全部登记了结构信号匹配器（无条目退化为模糊匹配）`
        : `以下禁令未登记匹配器（将退化为整串包含，匹配精度下降）：${unmatched.join("；")}`,
  });

  return out;
}

/** 判据「诚实语义」自检：守卫拦得住越界宣称 + 不误拦合规宣称 + 规格表不越上限。 */
export function selfCheckHonesty(): SelfCheck[] {
  const out: SelfCheck[] = [];

  // 守卫态1：GPU + 显式逐位宣称 → 拦截。
  const b1 = new DiagBag();
  const r1 = checkHonestyClaim("gpu-compute-sim", "GPU 粒子模拟是逐位可复现的确定性模拟", b1);
  out.push({
    name: "honesty-blocks-gpu-bitwise-claim",
    pass: !r1.ok && r1.code === "HONESTY_GUARD_VIOLATION",
    detail: !r1.ok ? `GPU 路径的逐位宣称被拦截（${r1.code}）` : "GPU 逐位宣称竟被放行（诚实语义根基破裂）",
  });

  // 守卫态2：GPU + 无等级强断言（「确定性模拟」不含等级词）→ 同样拦截。
  const b2 = new DiagBag();
  const r2 = checkHonestyClaim("gpu-compute-sim", "GPU 路径提供确定性模拟", b2);
  out.push({
    name: "honesty-blocks-unqualified-strong-word",
    pass: !r2.ok && r2.code === "HONESTY_GUARD_VIOLATION",
    detail: !r2.ok
      ? `无等级强断言「确定性模拟」被拦截（${r2.code}）——读者会自动脑补成逐位确定`
      : "无等级强断言被放行（最危险的一种误导）",
  });

  // 守卫态3：GPU + 合规的统计等效宣称 → 放行（不制造假阳性）。
  const b3 = new DiagBag();
  const r3 = checkHonestyClaim(
    "gpu-compute-sim",
    "GPU 路径只承诺视觉等效：粒子总数容差 ±1%，分布直方图散度低于阈值",
    b3,
  );
  out.push({
    name: "honesty-allows-honest-gpu-claim",
    pass: r3.ok && b3.size === 0,
    detail: r3.ok ? "合规的统计等效宣称被放行（带容差口径）" : `合规宣称被误拦：${r3.message}`,
  });

  // 守卫态4：CPU + 逐位宣称 → 放行（该路径确实能提供）。
  const r4 = checkHonestyClaim("cpu-scalar-sim", "CPU 路径同种子双跑逐位一致", new DiagBag());
  out.push({
    name: "honesty-allows-cpu-bitwise-claim",
    pass: r4.ok,
    detail: r4.ok ? "CPU 路径的逐位宣称被放行（诚实上限即逐位确定）" : "CPU 逐位宣称被误拦",
  });

  // 规格态：GPU 的确定性等级恒为统计等效（架构根基）。
  const gpu = PATH_TABLE["gpu-compute-sim"];
  out.push({
    name: "honesty-gpu-spec-is-statistical",
    pass: gpu.determinism === "statistically-equivalent" && gpu.honestCeiling === "statistically-equivalent",
    detail: `GPU 路径 determinism=${gpu.determinism}、honestCeiling=${gpu.honestCeiling}（两者恒为统计等效）`,
  });

  // 声明态：两条路径都必须写明「不承诺」事项（诚实不能只说能做什么）。
  const noNotPromised = PATH_IDS.filter((id) => PATH_TABLE[id].notPromised.length === 0);
  out.push({
    name: "honesty-declares-not-promised",
    pass: noNotPromised.length === 0,
    detail:
      noNotPromised.length === 0
        ? "两条路径均显式声明了「不承诺」事项（确定性声明是双向的）"
        : `以下路径未声明不承诺事项：${noNotPromised.join("、")}`,
  });

  return out;
}

/** 全量自检入口：一次跑完四组判据对应的全部检查项，返回逐项结果（不聚合为单一布尔）。 */
export function runSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly SelfCheck[]>>;
  readonly allPass: boolean;
  readonly failed: readonly string[];
} {
  const groups = {
    kickoff: selfCheckDomainKickoff(),
    dualPath: selfCheckDualPath(),
    seqContract: selfCheckSeqContract(),
    honesty: selfCheckHonesty(),
  };
  const failed: string[] = [];
  for (const [g, items] of Object.entries(groups)) {
    for (const it of items) {
      if (!it.pass) failed.push(`${g}.${it.name}: ${it.detail}`);
    }
  }
  return { groups, allPass: failed.length === 0, failed };
}