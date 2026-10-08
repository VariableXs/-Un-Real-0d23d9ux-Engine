/**
 * VE-F2001 · K 域开工与后处理架构（K 域 · 后处理链域开工条 · 批次 K01 首项）
 * ---------------------------------------------------------------------------
 * 职责定位：VE-K 域（F2001~F2200）的**开工条**。它不跑一次采样、不申请一块中间
 * 渲染目标，只做一件事：把「后处理链域要造什么、十主题按什么序造、与光照域（J）
 * 和3D 管线域（I）如何划界、从上游移交包（F1997）接收哪三份契约」四件事写成
 * **可机检的契约**。
 *
 * 为什么开工条必须是代码而不是文档（域级立场）：
 *   后处理链是全 VE 中**跨域契约最密**的域——它向 J 域要光照输出与曝光值，向
 *   I 域要已 resolve 的颜色缓冲，向 V 域交付已编码帧，自身还要向 A 域要显存配额。
 *   若这些挂接位只写在文档里，集成期必然出现三类事故：
 *     ① K 侧在后处理里又做了一次显示变换（与 V 域重复编码 → 画面发灰/过冲）；
 *     ② Bloom 放在色调映射之后（泛光按显示值算 → 高光语义错，且强泛光更刺眼）；
 *     ③ 上游 F1997 移交包里的三份契约没人签收就开工（契约缺失被静默默认值掩盖）。
 *   三类事故的共同点是「错了但没人被告知」。故本条把边界写成注册表 + 校验器：
 *   越界、越序、缺签在开发期就被拒，而不是在集成期靠人吵。
 *
 * 四条锚点契约（逐条对应判据）：
 *   1. 域开工 + 十主题 —— 官方十主题（Bloom/ToneMapping/DoF/MotionBlur/色差/暗角/
 *      颗粒/锐化/色彩分级/AA）逐项声明：主题 id、中文名、功能号区间、所属管线阶段、
 *      输入 RT 依赖、输出 RT 名、参数块名、开关语义、兑现的上游契约。
 *      未登记的效果不可被引用（注册表封闭集），十个主题号段无缝覆盖 F2001~F2200。
 *   2. 三契约接收 —— F1997 移交包 K 相关段的三份契约（bloom 源契约 F1849 /
 *      曝光契约 F1812 / 输出编码边界 F1897+F1964）逐份登记**接收确认签名**
 *      （签署方、签署时刻、契约内容哈希、确认范围）。缺签 = 开工阻断并点名到
 *      具体契约、具体上游锚点、具体需签方——不是一条笼统的「上游未就绪」。
 *   3. 管线序声明 —— 基准序「光照输出 → AA → TM → LUT → 分级 → 镜头效果 → 输出编码」
 *      是**默认序**，DAG 允许在满足依赖的前提下重排（重排须过拓扑可行性检查）；
 *      与 I 域 / J 域的**冻结序**冲突时一律以上游冻结为准回改（K 域无裁决权）。
 *   4. 组内分工总览 —— K 域十组（K01~K10）共 20 条分工行，每行带交付契约名与门禁判据。
 *
 * 零静默纪律：缺签、越权、越序、契约漂移、隐私面声明，全部产出 Diagnostic
 * （code + message + hint）并由调用方聚合上报；本模块不抛异常、不吞诊断。
 *
 * 判据：域开工、效果接口、三契约接收、管线序、判据。
 * 交接说明：本条是纯契约层——零 GPU 调用、零 DOM 依赖、零全局可变状态，
 *         顶层只有常量表与纯函数，可在任意宿主（浏览器/Worker/Node 校验脚本）
 *         中原样引入。下游 K01（F2004 Bloom 起）逐项消费本条 K_THEME_REGISTRY、
 *         PIPELINE_SEQUENCE 与 HANDOVER_LEDGER。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型（零静默的基础设施：域内独立实现，不跨域 import）
// ════════════════════════════════════════════════════════════════════════════

/** 诊断码：每种拒绝/越权/退化独立可检索，绝不合并成一条通用错误。 */
export type DiagCode =
  /** 上游契约未签收（移交包里三契约之一缺签名）——开工阻断级。 */
  | "HANDOVER_UNSIGNED"
  /** 签收了但确认范围不覆盖本域将消费的字段（签了 A 却要B）。 */
  | "HANDOVER_SCOPE_MISMATCH"
  /** 签收时声明的契约内容哈希与当前上游锚点不一致（上游改了未通知）。 */
  | "HANDOVER_HASH_MISMATCH"
  /** 引用了未在十主题注册表登记的效果（封闭集之外的效果不可进入链）。 */
  | "EFFECT_UNREGISTERED"
  /** 效果规格自相矛盾（输入输出同 RT、参数块缺失、管线阶段越出本主题职责）。 */
  | "EFFECT_SPEC_INCONSISTENT"
  /** 管线序引用了未登记的阶段。 */
  | "STAGE_UNKNOWN"
  /** 阶段依赖成环（渲染次序互相要求在前）。 */
  | "STAGE_CYCLE"
  /** 管线序违反依赖（重排后消费者排到了生产者之前）。 */
  | "PIPELINE_ORDER_VIOLATION"
  /** 管线序与 I/J 域冻结序冲突（K 域试图改动上游挂接位）。 */
  | "UPSTREAM_SEQUENCE_CONFLICT"
  /** 主题功能号区间与其他主题重叠（两个主题抢同一批功能号）。 */
  | "ITEM_RANGE_OVERLAP"
  /** 主题功能号区间有空洞（F2001~F2200 未被完整覆盖）。 */
  | "ITEM_RANGE_GAP"
  /** 交付契约名重复（两个分工行声明同名契约 = 后续必然撞名）。 */
  | "CONTRACT_NAME_DUPLICATE"
  /** 分工表行数不等于 20（分组总览被裁剪或重复登记）。 */
  | "WORKFORCE_ROW_COUNT_INVALID"
  /** 声明了运行时隐私面（本域零隐私面，出现即架构违规）。 */
  | "PRIVACY_SURFACE_DECLARED"
  /** 效果分发改为动态（违反静态分发零虚拟开销的设计声明）。 */
  | "STATIC_DISPATCH_BROKEN";

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

/** 成功构造（diagnostics 允许携带非致命告警，例如重排建议）。 */
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

  /** 追加一条已构造的诊断（把子调用的 Outcome.diagnostics 平铺进来）。 */
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

  /** 是否存在阻断级诊断（本模块全部码皆为阻断/告警级，非空即需处理）。 */
  get hasError(): boolean {
    return this.items.length > 0;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 K 域十主题注册表（判据一：域开工 + 十主题 + 效果接口）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 管线阶段 id。阶段是「效果在链上的位置类别」，与主题 id是**不同的轴**：
 * 一个主题固定落在一个阶段，但一个阶段可容纳多个主题（镜头效果阶段容纳
 * DoF/MotionBlur/色差/暗角/颗粒/锐化）。分开声明是因为「阶段决定次序（序由 DAG
 * 校验），主题决定归属（谁来实现）」是两件事，混在一起就会出现「为了保住序号
 * 而调整渲染次序」这类倒因为果的改法。
 *
 * 为什么 AA 被拆成两个阶段（aa-resolve / aa-post）——这是本条最容易被写错的地方：
 *   抗锯齿四法**不在同一个序位**。MSAA 是几何多重采样，resolve 必须在 TM 之前
 *   （它处理的是 HDR 几何边缘，TM 会改变边缘对比度，resolve 放到TM 后等于对
 *   已压过的图像做多重采样平均，白采样）；FXAA/TAA 是屏幕空间效果，序位守卫要求
 *   它们在 TM 之后（需要 0-1 显示域才能做阈值边缘检测）。
 *   若图省事写成单一aa 阶段，则「aa 依赖 bloom 与 tonemap」这条依赖同时把MSAA
 *   拖到 TM 之后——正好把MSAA 做废。故拆为两段，各自的依赖分别只声明它真正
 *   需要的前置。这是本条自检能抓住的典型「声明自洽」错误。
 */
export type PipelineStage =
  /** 光照输出挂接位：J 域 F1841 交付的线性辐射度缓冲，K 域不生产它。 */
  | "lighting-output"
  /** MSAA resolve：几何多重采样解析，TM 之前（I 域 I02 协作位）。 */
  | "aa-resolve"
  /** Bloom 提取与合成（TM 之前——泛光必须按线性高光算，见 F2004 约束）。 */
  | "bloom"
  /** 曝光应用 + 色调映射（曝光乘法并入 TM pass，见 F2007）。 */
  | "tonemap"
  /** 屏幕空间 AA（FXAA/TAA/SMAA 预留）：TM 之后 LDR 域（见 F2010 序位守卫）。 */
  | "aa-post"
  /** LUT 查表（色彩分级的基础层）。 */
  | "lut"
  /** 色彩分级（创意调色层，含 LUT 的胶片/暗角之外的风格变换）。 */
  | "grading"
  /** 镜头效果（景深/运动模糊/色差/暗角/颗粒/锐化——共用镜头语义的一组）。 */
  | "camera"
  /** 输出编码（线性 → sRGB/PQ，写给V 域的已编码帧）。 */
  | "output-encode"
  /** 呈现（V 域职责，K 域到此为止；仅作边界声明用）。 */
  | "present";

/** 阶段的基准次序号（锚点 F2001 管线序文本的机读形式）。 */
export const STAGE_RANK: Readonly<Record<PipelineStage, number>> = {
  "lighting-output": 0,
  "aa-resolve": 1,
  bloom: 2,
  tonemap: 3,
  "aa-post": 4,
  lut: 5,
  grading: 6,
  camera: 7,
  "output-encode": 8,
  present: 9,
};

/**
 * 阶段的人类可读名（无障碍替述与文档渲染引用同一事实源，不允许两处各写一份）。
 */
export const STAGE_LABEL: Readonly<Record<PipelineStage, string>> = {
  "lighting-output": "光照输出挂接位",
  "aa-resolve": "MSAA 解析阶段（色调映射之前）",
  bloom: "泛光阶段",
  tonemap: "曝光与色调映射阶段",
  "aa-post": "屏幕空间抗锯齿阶段（色调映射之后）",
  lut: "LUT 查表阶段",
  grading: "色彩分级阶段",
  camera: "镜头效果阶段",
  "output-encode": "输出编码阶段",
  present: "呈现边界（V 域）",
};

/**
 * 阶段依赖（生产者 → 消费者）。这是 DAG 的**边声明**，F2002 执行器据此做拓扑排序。
 * 这里只声明「谁必须先于谁」，不声明「谁必须紧邻谁」——后者由基准序表达。
 *
 * 关键依赖的物理理由（写下来是为了将来有人想改时能看到代价）：
 *   aa-resolve → bloom： MSAA 解析必须先于一切后处理——它是几何采样问题，
 *                       必须在 HDR 值仍保持几何意义时解决，TM 之后再 resolve
 *                       等于对已压过的显示值做多重采样平均，几何边缘判据全失效。
 *   bloom → tonemap：   泛光阈值是物理亮度（cd/m²，F1802 量纲），必须在曝光与
 *                       TM 之前按场景线性值提取；放到 TM 后等于按显示值提取，
 *                       提亮画面就满屏泛光，且强泛光更刺眼（光敏风险，F2004）。
 *   tonemap → aa-post： 屏幕空间 AA 的边缘检测假定输入已压缩到 0-1 显示域；
 *                       喂 HDR 会让阈值判据失效（F2010 序位守卫）。
 *   aa-post → lut：     LUT 与风格分级应在边缘已清理的底图上做，避免把锯齿
 *                       一起编进分级曲线。
 *   tonemap → lut：     LUT 的输入域是显示 referred（F1897 边界语义）。
 *   lut → grading：     风格分级在基础校正之后（先校正再风格化，顺序反了不可逆）。
 *   grading → camera：  镜头效果对最终调色做光学性破坏（暗角/颗粒），应在风格定稿后。
 *   camera → output-encode：色差/暗角等按线性量施加，编码后施加即为二次错误。
 */
export const STAGE_DEPENDENCIES: Readonly<Record<PipelineStage, readonly PipelineStage[]>> = {
  "lighting-output": [],
  "aa-resolve": ["lighting-output"],
  bloom: ["lighting-output", "aa-resolve"],
  tonemap: ["lighting-output", "bloom"],
  "aa-post": ["tonemap"],
  lut: ["tonemap", "aa-post"],
  grading: ["lut"],
  camera: ["grading"],
  "output-encode": ["camera", "grading"],
  present: ["output-encode"],
};

/** 效果分发型：静态分发（零虚拟开销）是本域的设计声明，改动即违规。 */
export type DispatchKind = "static" | "dynamic";

/**
 * 一个效果的完整规格。锚点要求的五项（名称/输入 RT 依赖/输出 RT/参数块/开关）
 * 逐字段落在下列结构上——五项缺一即为 EFFECT_SPEC_INCONSISTENT。
 */
export interface EffectSpec {
  /** 效果类型枚举 id（封闭集，见 EffectId）。 */
  readonly id: EffectId;
  /** 中文名（对外文档与错误提示引用同一事实源）。 */
  readonly name: string;
  /** 本主题在本域内的功能号区间（含端点），逐主题不重叠且无缝，由自检机检。 */
  readonly itemRange: { readonly lo: number; readonly hi: number };
  /** 所属管线阶段（次序由阶段依赖决定，不由本字段决定）。 */
  readonly stage: PipelineStage;
  /** 输入 RT 依赖：逻辑名清单（"lighting-output" 表示 J 域挂接位）。 */
  readonly inputs: readonly string[];
  /** 输出 RT 逻辑名（经 F2003 中间 RT 池分配物理资源，本域只声明逻辑名）。 */
  readonly output: string;
  /** 参数块名（参数与结构分离声明——纯参数变化免图重编译，见 F2005）。 */
  readonly params: string;
  /** 开关语义：关闭时该节点旁路（显性降级，不静默，见 F2002）。 */
  readonly toggle: "on" | "off" | "bypass-on-init-failure";
  /** 兑现的上游契约 id（K 域消费的上游交接物；无上游契约者为空数组）。 */
  readonly fulfills: readonly string[];
  /** 分发型：全K 域效果一律静态分发。 */
  readonly dispatch: DispatchKind;
  /** 职责边界一句话：负责什么、不负责什么（禁扩面写在句内）。 */
  readonly duty: string;
}

/**
 * 官方十主题的封闭集。新增效果必须同时改EffectId 与 K_THEME_REGISTRY 两处，
 * 只改一处由 selfCheckDomain 拦下——这就是「声明即契约」的机检形态。
 */
export type EffectId =
  | "bloom"
  | "tonemapping"
  | "antialiasing"
  | "color-grading"
  | "depth-of-field"
  | "motion-blur"
  | "chromatic-aberration"
  | "vignette"
  | "film-grain"
  | "sharpening";

/** 效果规格表全键（遍历用，避免手写清单与实际注册漂移）。 */
export const EFFECT_IDS: readonly EffectId[] = [
  "bloom",
  "tonemapping",
  "antialiasing",
  "color-grading",
  "depth-of-field",
  "motion-blur",
  "chromatic-aberration",
  "vignette",
  "film-grain",
  "sharpening",
];

/**
 * 区间归并口径（显式记录，避免后来者以为F2001-F2003 是漏排）：
 *   锚点把F2001~F2020 定为「批次 K01·后处理架构与 Bloom 组」，而本域的架构三件套
 *   （DAG 模型 F2002 / 中间 RT 池 F2003 / 效果接口即本条）**只服务于链上第一个落地
 *   效果 Bloom**——它们不是为了抽象而抽象，是 Bloom 三段管线（提取→mip 模糊链→
 *   合成）能跑起来的前置。故本条把架构三件套计入 Bloom 主题的 20 项预算，
 *   十主题号段由此无缝覆盖 F2001~F2200。
 *   若将来架构被多个主题共用而需要独立号段，须走 ADR 调整本口径并重跑自检——
 *   不得靠临时插空号段绕过区间重叠检查。
 */
export const RANGE_MERGE_NOTE =
  "F2001-F2003 架构三件套（DAG 模型/中间 RT 池/效果接口）计入 bloom 主题预算，理由：架构服务于链上首个落地效果；调整须走 ADR。";

/**
 * K 域十主题注册表。判据一要求的逐项声明在此为唯一事实源——下游任何代码不得旁路
 * 硬编码效果名与RT 名，一律经 lookupEffect 取规格。
 */
export const K_THEME_REGISTRY: Readonly<Record<EffectId, EffectSpec>> = {
  bloom: {
    id: "bloom",
    name: "泛光",
    itemRange: { lo: 2001, hi: 2020 },
    stage: "bloom",
    inputs: ["lighting-output", "bloom-source-mask"],
    output: "bloom-mip-composite",
    params: "BloomParams",
    toggle: "on",
    fulfills: ["bloom-source-contract"],
    dispatch: "static",
    duty: "三段管线（明亮提取/多级 mip 模糊/合成叠加）与物理量纲阈值；不负责曝光与色彩编码",
  },
  tonemapping: {
    id: "tonemapping",
    name: "色调映射",
    itemRange: { lo: 2021, hi: 2040 },
    stage: "tonemap",
    inputs: ["lighting-output", "exposure-value"],
    output: "tonemapped-ldr",
    params: "ToneMapParams",
    toggle: "on",
    fulfills: ["exposure-contract"],
    dispatch: "static",
    duty: "曲线族（ACES 近似/Reinhard/Uncharted2）与曝光乘法应用位；输出止于显示 referred，不做最终显示变换",
  },
  "antialiasing": {
    id: "antialiasing",
    name: "抗锯齿",
    itemRange: { lo: 2041, hi: 2060 },
    stage: "aa-post",
    inputs: ["tonemapped-ldr", "depth-buffer", "velocity-buffer"],
    output: "aa-resolved",
    params: "AaParams",
    toggle: "bypass-on-init-failure",
    fulfills: [],
    dispatch: "static",
    duty: "四法族（MSAA resolve/FXAA/TAA/SMAA 预留位）的挂载与互斥守卫；不负责图像锐化。"
      + "注意本主题跨两个序位：MSAA resolve 属 aa-resolve 阶段（TM 前），"
      + "FXAA/TAA 属本阶段（TM 后）——四法不在同一序位是物理事实，不得为图省事合并",
  },
  "color-grading": {
    id: "color-grading",
    name: "色彩分级",
    itemRange: { lo: 2061, hi: 2080 },
    stage: "grading",
    inputs: ["lut-applied"],
    output: "graded",
    params: "GradingParams",
    toggle: "on",
    fulfills: [],
    dispatch: "static",
    duty: "基础校正后的风格变换（色轮/曲线/风格 LUT）；不负责暗角与颗粒等镜头性破坏",
  },
  "depth-of-field": {
    id: "depth-of-field",
    name: "景深",
    itemRange: { lo: 2081, hi: 2100 },
    stage: "camera",
    inputs: ["graded", "depth-buffer"],
    output: "dof-blended",
    params: "DofParams",
    toggle: "off",
    fulfills: [],
    dispatch: "static",
    duty: "基于深度的散焦模拟与焦平面跟随；不负责运动模糊的时间维度累积",
  },
  "motion-blur": {
    id: "motion-blur",
    name: "运动模糊",
    itemRange: { lo: 2101, hi: 2120 },
    stage: "camera",
    inputs: ["graded", "velocity-buffer"],
    output: "motion-blurred",
    params: "MotionBlurParams",
    toggle: "off",
    fulfills: [],
    dispatch: "static",
    duty: "基于速度缓冲的时域拖影与快门时长建模；不负责 TAA 的历史重投影（两者共用速度缓冲但语义不同）",
  },
  "chromatic-aberration": {
    id: "chromatic-aberration",
    name: "色差",
    itemRange: { lo: 2121, hi: 2140 },
    stage: "camera",
    inputs: ["graded"],
    output: "fringe-corrected",
    params: "ChromaticParams",
    toggle: "off",
    fulfills: [],
    dispatch: "static",
    duty: "横向色散模拟与边缘色纹抑制；不负责色彩分级的风格色偏",
  },
  vignette: {
    id: "vignette",
    name: "暗角",
    itemRange: { lo: 2141, hi: 2160 },
    stage: "camera",
    inputs: ["graded"],
    output: "vignetted",
    params: "VignetteParams",
    toggle: "off",
    fulfills: [],
    dispatch: "static",
    duty: "径向亮度衰减与自然渐晕；不负责整体曝光压暗（那是曝光应用位的事）",
  },
  "film-grain": {
    id: "film-grain",
    name: "颗粒",
    itemRange: { lo: 2161, hi: 2180 },
    stage: "camera",
    inputs: ["graded"],
    output: "grained",
    params: "FilmGrainParams",
    toggle: "off",
    fulfills: [],
    dispatch: "static",
    duty: "时域相关噪声与胶片颗粒形态；噪声必须在暗部受控，不得制造闪烁（光敏红线，F1962 联动）",
  },
  sharpening: {
    id: "sharpening",
    name: "锐化",
    itemRange: { lo: 2181, hi: 2200 },
    stage: "camera",
    inputs: ["aa-resolved"],
    output: "sharpened",
    params: "SharpeningParams",
    toggle: "off",
    fulfills: [],
    dispatch: "static",
    duty: "对比度自适应锐化与边缘保持；与 FXAA 存在对冲关系需序位协调，不负责降噪",
  },
};

/** 查效果规格；未注册返回显性失败——判据要求「十主题封闭集，域外效果不可引用」。 */
export function lookupEffect(id: string): Outcome<EffectSpec> {
  const table = K_THEME_REGISTRY as Record<string, EffectSpec | undefined>;
  const spec = table[id];
  if (spec === undefined) {
    return fail(
      "EFFECT_UNREGISTERED",
      `效果 ${id} 不在 K 域十主题注册表中`,
      `已注册效果：${EFFECT_IDS.join("、")}；若确为新增效果，须先在 K_THEME_REGISTRY 与 EffectId 类型两处同时登记`,
    );
  }
  return ok(spec);
}

/**
 * 中间渲染目标池的契约声明（锚点三件套之二）。
 *
 * 本条**只声明**池的契约面（桶键/生命周期/别名规则/统计口径），不实现池——
 * 实现归F2003。之所以现在就把契约写死：效果规格里的 output 只是逻辑 RT 名，
 * 若物理资源分配规则到F2003 才定，十主题里的输出名可能在实现期被迫改名，
 * 而改名会同时打断 F2013（中间 RT 逐级透视）的可寻址性。故名与规则此刻冻结。
 */
export interface RtPoolContract {
  /** 桶键：尺寸×格式的匹配是强制键（误别名即读写竞争，见 F2003 键匹配强制）。 */
  readonly bucketKey: "size-and-format";
  /** RT 生命周期口径：DAG 编译期标注存活区间，帧边界统一回收。 */
  readonly lifetime: "compile-annotated-interval";
  /** 别名规则：区间不重叠的逻辑 RT 可复用同一物理资源。 */
  readonly aliasingRule: "interval-graph-coloring";
  /** 统计口径（供遥测 F2059 与调试 F2013 双用）。 */
  readonly metrics: readonly ("hit" | "miss" | "peak-occupancy" | "alias-count")[];
  /** 配额归属：后处理中间 RT 显存入中间缓冲池，配额体系统一治理。 */
  readonly quotaOwner: "intermediate-buffer-pool";
  /** 泄漏防线：连续 N 帧未回收即告警并记泄漏账本。 */
  readonly leakGuard: "unreclaimed-frame-ledger";
}

/** 中间 RT 池契约（唯一事实源；F2003 实现须逐字段兑现）。 */
export const RT_POOL_CONTRACT: RtPoolContract = {
  bucketKey: "size-and-format",
  lifetime: "compile-annotated-interval",
  aliasingRule: "interval-graph-coloring",
  metrics: ["hit", "miss", "peak-occupancy", "alias-count"],
  quotaOwner: "intermediate-buffer-pool",
  leakGuard: "unreclaimed-frame-ledger",
};

// ════════════════════════════════════════════════════════════════════════════
// §3 三契约接收核对（判据三：三契约接收 —— F1997 移交包 K 相关段）
// ════════════════════════════════════════════════════════════════════════════

/** 上游契约 id。三份来自 F1997 移交包 K 相关段，一份 K 域自有边界。 */
export type UpstreamContractId =
  /** bloom 源契约：J 域 F1849 产出的源标记掩码（物理光源优先的泛光来源）。 */
  | "bloom-source-contract"
  /** 曝光契约：J 域 F1812 计算曝光，K 域 F2007 应用曝光（计算与应用的分工）。 */
  | "exposure-contract"
  /** 输出编码边界：F1897/F1964 协议——K 侧输出显示 referred 已编码帧，V 侧不二次编码。 */
  | "output-encode-boundary";

/** 一份接收确认签名。缺任何一字段即视为未签收——半截签名比没有签名更危险。 */
export interface HandoverSignature {
  /** 签署方（上游域标识；上游无人签 = 未签收）。 */
  readonly signer: string;
  /** 签署时刻（ISO 8601；用于陈旧性判定与追溯）。 */
  readonly signedAt: string;
  /** 签收时上游契约的内容哈希（与上游当前哈希不符 = 上游改过未通知）。 */
  readonly contentHash: string;
  /** 确认范围：本次签收明确覆盖的字段清单（签了 A 却消费 B =范围不匹配）。 */
  readonly scope: readonly string[];
}

/** 一份上游契约在 K 域的接收登记。 */
export interface HandoverEntry {
  readonly id: UpstreamContractId;
  /** 上游锚点（册内编号 + 条目名，便于追溯来历）。 */
  readonly upstreamAnchor: string;
  /** 上游方职责一句话。 */
  readonly peerDuty: string;
  /** K 域消费的内容（具体到字段，不写「协同」这种无信息量的话）。 */
  readonly weConsume: readonly string[];
  /** K 域必须不做的内容（禁扩面；越权即 PIPELINE_ORDER_VIOLATION 的姊妹判据）。 */
  readonly weMustNot: readonly string[];
  /** 兑现该契约的 K 域条目（谁把它落地）。 */
  readonly realizedBy: readonly string[];
  /** 上游当前内容哈希（用于与签收哈希对拍）。 */
  readonly currentHash: string;
  /** 接收确认签名（未签收 = null，且null 是显式状态而非「忘了填」）。 */
  readonly signature: HandoverSignature | null;
}

/**
 * F1997 移交包 K 相关段的三份契约登记。
 *
 * 初始状态说明（诚实声明，不伪装成已签收）：本条落笔时三份签收均未完成——
 * 上游 J 域 F1997 移交包需在开工前逐项确认。故初始 signature 全为 null，
 * 由 verifyHandoverLedger 判为「开工阻断 + 点名」，这是正确行为而不是缺陷。
 * 真正签收时由上游签署方填入签名，本条随后自检转绿。
 */
export const HANDOVER_LEDGER: readonly HandoverEntry[] = [
  {
    id: "bloom-source-contract",
    upstreamAnchor: "VE-J F1849 光照输出与 bloom 源标记契约（J03 组）",
    peerDuty: "光照与阴影域：产出线性辐射度缓冲并标记物理光源区域，供K 域做物理源优先的泛光提取",
    weConsume: [
      "lighting-output（线性辐射度 RT，J 域 F1841 序位产出）",
      "bloom-source-mask（光源直射区掩码，加权放宽明亮提取阈值）",
    ],
    weMustNot: [
      "在K 域内重新计算光源标记（J 域已算，重算是两处实现必然漂移）",
      "把源标记当作alpha 混进主色缓冲（它是提取侧的加权项，不是合成项）",
    ],
    realizedBy: ["VE-F2004 Bloom 核心"],
    currentHash: "j1849-source-mask@1",
    signature: null,
  },
  {
    id: "exposure-contract",
    upstreamAnchor: "VE-J F1812 自动/手动曝光计算（J 域曝光系统）",
    peerDuty: "光照域负责曝光**计算**（场景亮度统计与目标 EV），K 域只负责曝光**应用**",
    weConsume: [
      "exposure-value（目标 EV，范围 [-6,+6]，一阶惯性过渡由 K 侧承接）",
      "exposure-mode（auto/manual 互斥标志）",
    ],
    weMustNot: [
      "在 K 域内做场景亮度统计（那是 J 域的活，重复统计会出现两套不同的曝光结论）",
      "让曝光过渡跨场景延续（场景切换=曝光快照重置）",
    ],
    realizedBy: ["VE-F2007 曝光联动"],
    currentHash: "j1812-exposure-value@1",
    signature: null,
  },
  {
    id: "output-encode-boundary",
    upstreamAnchor: "VE-K F1897/F1964 显示变换边界协议（K 侧职责终点与 V 侧交接）",
    peerDuty: "V 域负责最终显示变换与呈现；K 域输出止于已编码的显示 referred 帧",
    weConsume: [
      "目标色彩空间能力（显示器能力探测结果，缺省声明为 sRGB）",
      "V 域的呈现时序与目标交换链格式",
    ],
    weMustNot: [
      "在 K 域内对已编码帧做二次编码（编码叠加=画面发灰/过冲，色彩事故首因）",
      "在 K 域内做最终显示变换（伽马/色域映射归V 域）",
    ],
    realizedBy: ["VE-F2008 色彩空间输出"],
    currentHash: "k1897-display-boundary@1",
    signature: null,
  },
];

/** 接收核验报告：逐份契约的可读状态 + 开工是否被阻断 + 阻断点名清单。 */
export interface HandoverReport {
  /** 是否允许开工（仅当三份全部签收且范围覆盖消费字段）。 */
  readonly mayStart: boolean;
  /** 逐份契约状态，便于一眼看出缺谁。 */
  readonly rows: readonly HandoverRow[];
  /** 阻断点名：缺签/范围不符/哈希漂移的逐条说明（含需签方与锚点）。 */
  readonly blockers: readonly Diagnostic[];
}

/** 单份契约的核验行。 */
export interface HandoverRow {
  readonly id: UpstreamContractId;
  readonly status: "signed" | "unsigned" | "scope-mismatch" | "hash-drift";
  readonly signer: string;
  readonly signedAt: string;
  /** 消费字段中未被签名范围覆盖的字段清单（scope-mismatch 时非空）。 */
  readonly uncoveredFields: readonly string[];
}

/**
 * 接收件核对（判据三的主函数）：逐份核验签名完整性与范围覆盖。
 *
 * 三种失败各自独立可检索，不合并：
 *   unsigned       —— 没有签名（或签名三字段有缺）→ 开工阻断，点名需签方；
 *   scope-mismatch —— 签名范围没覆盖K 域要消费的字段 → 阻断（签了 A 却要 B）；
 *   hash-drift     —— 签收哈希 ≠ 上游当前哈希 → 阻断（上游改过未通知，须重签）。
 */
export function verifyHandoverLedger(
  ledger: readonly HandoverEntry[] = HANDOVER_LEDGER,
): HandoverReport {
  const blockers: Diagnostic[] = [];
  const rows: HandoverRow[] = [];

  for (const entry of ledger) {
    const sig = entry.signature;

    if (sig === null) {
      const d: Diagnostic = {
        code: "HANDOVER_UNSIGNED",
        message: `上游契约「${entry.id}」未签收：${entry.upstreamAnchor} 尚无接收确认签名，`
          + `K 域不得在缺签状态下开工`,
        hint: `请${entry.upstreamAnchor} 的负责人签收该契约并回填signature（签署方/时刻/内容哈希/确认范围四项齐备）；`
          + `若上游确认该契约暂不提供，请在特征表中显式声明降级路径（如 bloom 退化为纯亮度提取并告警），`
          + `而不是留null 让下游静默走默认值`,
      };
      blockers.push(d);
      rows.push({ id: entry.id, status: "unsigned", signer: "", signedAt: "", uncoveredFields: [] });
      continue;
    }

    const incomplete = (
      [
        ["signer", sig.signer],
        ["signedAt", sig.signedAt],
        ["contentHash", sig.contentHash],
      ] as const
    ).filter(([, v]) => v.trim().length === 0);
    if (incomplete.length > 0) {
      const missing = incomplete.map(([k]) => k).join("、");
      blockers.push({
        code: "HANDOVER_UNSIGNED",
        message: `上游契约「${entry.id}」的签名字段不完整，缺：${missing}；半截签名比无签名更危险，判为未签收`,
        hint: `补齐${missing}；签收三要素缺任一即不得开工（判定口径见本文件 §3verifyHandoverLedger）`,
      });
      rows.push({ id: entry.id, status: "unsigned", signer: sig.signer, signedAt: sig.signedAt, uncoveredFields: [] });
      continue;
    }

    if (sig.contentHash !== entry.currentHash) {
      blockers.push({
        code: "HANDOVER_HASH_MISMATCH",
        message: `上游契约「${entry.id}」内容哈希漂移：签收时为 ${sig.contentHash}，上游当前为 ${entry.currentHash}；`
          + `说明上游在签收后改过契约而 K 域未收到通知`,
        hint: `请上游确认变更内容；若变更影响 K 域消费字段，须重新签收并更新 scope；`
          + `不得沿用旧签名继续开工（这是最典型的「上游改了、下游不知情」事故）`,
      });
      rows.push({
        id: entry.id,
        status: "hash-drift",
        signer: sig.signer,
        signedAt: sig.signedAt,
        uncoveredFields: [],
      });
      continue;
    }

    const uncovered = entry.weConsume.filter((field) => !sig.scope.includes(field));
    if (uncovered.length > 0) {
      blockers.push({
        code: "HANDOVER_SCOPE_MISMATCH",
        message: `上游契约「${entry.id}」的签收范围不覆盖 K 域将消费的字段：${uncovered.join("、")}`,
        hint: `请上游把上述字段加入签收范围后重签；「签了 A 却要 B」在集成期表现为`
          + `K 域读到上游未承诺的字段，行为不可预期且无人负责`,
      });
      rows.push({
        id: entry.id,
        status: "scope-mismatch",
        signer: sig.signer,
        signedAt: sig.signedAt,
        uncoveredFields: uncovered,
      });
      continue;
    }

    rows.push({ id: entry.id, status: "signed", signer: sig.signer, signedAt: sig.signedAt, uncoveredFields: [] });
  }

  return { mayStart: blockers.length === 0, rows, blockers };
}

// ════════════════════════════════════════════════════════════════════════════
// §4 管线序声明与重排裁决（判据四：管线序）
// ════════════════════════════════════════════════════════════════════════════

/** 上游冻结序声明（I02 / J 域侧不可由 K 域改动）。 */
export interface FrozenSequence {
  readonly id: string;
  readonly owner: "VE-I" | "VE-J" | "VE-V";
  readonly anchor: string;
  /** 冻结内容：K 域必须接受的事实。 */
  readonly frozen: string;
}

/**
 * 三条上游冻结序。冲突时的裁决规则**写死**在裁决函数里：以上游冻结为准回改 K 侧。
 * 理由：K 域是后处理链，它对上游没有依赖倒转的可能（J 域不依赖 K 的后处理来算光照），
 * 故让 K 侧回改永远是代价更小的一方。
 */
export const UPSTREAM_FROZEN_SEQUENCES: readonly FrozenSequence[] = [
  {
    id: "lighting-output-slot",
    owner: "VE-J",
    anchor: "VE-J F1841（J03 组）",
    frozen: "光照输出位于管线序起点：K 域后处理的第一个挂接位就是 J 域产出的线性辐射度缓冲，"
      + "K 域不得在光照输出之前插入任何消费该缓冲的效果",
  },
  {
    id: "msaa-resolve-slot",
    owner: "VE-I",
    anchor: "VE-I I02 主管线与 F16013D 管线总架构",
    frozen: "MSAA resolve 发生在后处理之前（K 域对应 aa-resolve 阶段，位于泛光与色调映射之前）："
      + "几何 pass 以 MSAA 渲染并 resolve 为单采样，后处理链全 1x（后处理不得读 MSAA 多采样数据）",
  },
  {
    id: "encoded-present-slot",
    owner: "VE-V",
    anchor: "VE-K F1897/F1964 显示变换边界协议（V 侧呈现）",
    frozen: "K 域交付已编码帧后即止步：呈现、最终显示变换、UI 合成归 V 域，K 域不得越界",
  },
];

/**
 * 基准管线序（锚点原文的机读形式）。这是**默认序**，不是唯一合法序。
 *
 * 与锚点文本的两处显式差异（都记在下方补注里，此处只给序）：
 *   ① 锚点文本的「AA」在本序中被拆为 aa-resolve（TM 前）与 aa-post（TM 后）；
 *   ② Bloom 在锚点文本中缺位，本序按 F2004 约束插入 lighting-output 与 aa-resolve 之后。
 */
export const PIPELINE_SEQUENCE: readonly PipelineStage[] = [
  "lighting-output",
  "aa-resolve",
  "bloom",
  "tonemap",
  "aa-post",
  "lut",
  "grading",
  "camera",
  "output-encode",
  "present",
];

/**
 * 基准序补注（显式口径声明，不藏进注释——这三处差异将来一定会被人质疑）：
 *
 *   差异① Bloom 在锚点 F2001 的序文本中**缺位**。
 *      F2004 明确要求「bloom 在 TM 前——管线序 F2001 声明执行」，两句合起来的
 *      唯一自洽解是Bloom 位于光照输出之后、TM 之前。本序把它落在aa-resolve 之后：
 *      泛光的输入应是已完成几何多重采样解析的线性缓冲，先resolve 再提泛光，
 *      避免未解析的 MSAA 边缘在泛光里被放大成锯齿光晕。
 *      此插位为 K 域的口径选择，需 F2004 复验确认；调整走 ADR 改 STAGE_RANK
 *      与 PIPELINE_SEQUENCE，不允许两处各写一份序。
 *
 *   差异② 锚点序文本中的「AA」被拆为 aa-resolve 与 aa-post两段。
 *      抗锯齿四法**不在同一序位**：MSAA 是几何多重采样，resolve 必须在 TM 之前
 *      （TM 改变边缘对比度，事后resolve 等于对已压过的显示值做多重采样平均，
 *      几何边缘判据全失效）；FXAA/TAA 是屏幕空间效果，序位守卫要求它们在 TM 之后
 *      （需要 0-1 显示域才能做阈值边缘检测）。
 *      若合并为单一 aa 阶段，则「aa 依赖 bloom 与 tonemap」会把 MSAA 一并拖到
 *      TM 之后——恰好把 MSAA 做废。故拆为两段，各自只声明真正需要的前置。
 *
 *   差异③ 锚点序文本为 8 个阶段，本序为 ${PIPELINE_SEQUENCE.length} 个。
 *      差异①② 合计贡献 +2 段（Bloom +1、AA 拆分 +1），8 + 2 = 10，与实际一致。
 *      段数变化不是笔误，是把「一句话序」展开成「可机检序」后的必然结果。
 */
export const SEQUENCE_INSERTION_NOTE =
  "三处显式差异：① Bloom 在锚点序文本缺位，按 F2004「bloom 在 TM 前」落于 aa-resolve 之后；"
  + "② 锚点的「AA」拆为 aa-resolve（TM 前，MSAA 几何 resolve）与 aa-post（TM 后，FXAA/TAA 屏幕空间）"
  + "——四法不在同一序位，合并会让 MSAA 被拖到 TM 之后而失效；"
  + "③ 段数 8 → 10，由差异①② 各贡献一段。调整均须走 ADR，不允许两处各写一份序。";

/**
 * 依赖可行性检查：给定一个候选序，验证每个阶段的每个前置是否都排在它之前。
 * 返回首个违规（阶段 + 违规前置），无违规返回 null。
 *
 * 该函数是「DAG 允许重排但基准序为默认」的执行形态：重排不是自由的，
 * 只能在不违反 STAGE_DEPENDENCIES 的前提下自由。
 */
export function findOrderViolation(order: readonly PipelineStage[]): {
  readonly stage: PipelineStage;
  readonly missingBefore: PipelineStage;
} | null {
  const pos = new Map<PipelineStage, number>();
  order.forEach((s, i) => pos.set(s, i));
  for (const s of order) {
    for (const dep of STAGE_DEPENDENCIES[s]) {
      const depPos = pos.get(dep);
      if (depPos === undefined || depPos > (pos.get(s) ?? -1)) {
        return { stage: s, missingBefore: dep };
      }
    }
  }
  return null;
}

/**
 * 重排裁决：给定候选序，返回是否接受，并在不接受时给出三要素。
 *
 * 三条裁决规则（顺序即优先级）：
 *   1. 覆盖性检查：候选序必须覆盖基准序的全部阶段——少阶段 = 有效果被静默丢弃，
 *      这是最严重的一类（画面少一道效果但没人知道），优先于次序问题。
 *   2. 上游冻结序检查：起点必须是 lighting-output、终点必须止于 present——
 *      违反即回改 K 侧（K 域无裁决权，见 UPSTREAM_FROZEN_SEQUENCES 的理由）。
 *   3. 依赖可行性检查：见 findOrderViolation。
 */
export function resolvePipelineOrder(order: readonly PipelineStage[]): Outcome<readonly PipelineStage[]> {
  const bag = new DiagBag();

  for (const s of order) {
    if (STAGE_RANK[s] === undefined) {
      bag.push(
        "STAGE_UNKNOWN",
        `候选管线序引用了未登记的阶段 ${s}`,
        `已登记阶段：${PIPELINE_SEQUENCE.join(" → ")}；新增阶段须同时改 PipelineStage 类型、STAGE_RANK 与 STAGE_DEPENDENCIES`,
      );
      return fail("STAGE_UNKNOWN", `未登记的管线阶段 ${s}`, "请先在 PipelineStage/STAGE_RANK/STAGE_DEPENDENCIES 三处登记");
    }
  }

  const missing = PIPELINE_SEQUENCE.filter((s) => !order.includes(s));
  if (missing.length > 0) {
    bag.push(
      "PIPELINE_ORDER_VIOLATION",
      `候选管线序漏掉了阶段：${missing.join("、")}；漏阶段等于有效果被静默丢弃（画面少一道效果但无任何告警）`,
      `管线序必须覆盖基准序全部 ${PIPELINE_SEQUENCE.length} 个阶段；若某效果本帧不参与，`
        + `正确做法是在效果规格里关开关（toggle=off），而不是把它从序里删掉`,
    );
    return fail(
      "PIPELINE_ORDER_VIOLATION",
      `管线序漏阶段：${missing.join("、")}`,
      "请补齐缺失阶段；不参与的效果用 toggle=off 表达，不用删阶段",
    );
  }

  const first = order[0];
  const last = order[order.length - 1];
  if (first !== "lighting-output") {
    bag.push(
      "UPSTREAM_SEQUENCE_CONFLICT",
      `候选管线序的起点是 ${first ?? "(空)"}，但上游 J 域 F1841 已冻结：后处理第一个挂接位就是光照输出`,
      `回改 K 侧——把 lighting-output 放回首位。上游冻结序优先：${UPSTREAM_FROZEN_SEQUENCES[0]?.frozen ?? ""}`,
    );
    return fail("UPSTREAM_SEQUENCE_CONFLICT", `管线序起点被改为 ${first ?? "(空)"}`, "回改 K 侧，恢复 lighting-output 为首位");
  }
  if (last !== "present") {
    bag.push(
      "UPSTREAM_SEQUENCE_CONFLICT",
      `候选管线序的终点是 ${last ?? "(空)"}，但 V 侧已冻结：K 域交付已编码帧后止步，呈现归 V 域`,
      `回改 K 侧——把 present 放回末位。若 K 域确需在呈现前插入新阶段，须先与 V 域谈契约并走 ADR`,
    );
    return fail("UPSTREAM_SEQUENCE_CONFLICT", `管线序终点被改为 ${last ?? "(空)"}`, "回改 K 侧，恢复 present 为末位");
  }

  const violation = findOrderViolation(order);
  if (violation !== null) {
    bag.push(
      "PIPELINE_ORDER_VIOLATION",
      `候选管线序把「${violation.stage}」排在「${violation.missingBefore}」之前，违反阶段依赖`
        + `（${STAGE_LABEL[violation.stage]}消费 ${STAGE_LABEL[violation.missingBefore]} 的产物）`,
      `把 ${violation.missingBefore} 移到 ${violation.stage} 之前；重排的自由度仅限不违反 STAGE_DEPENDENCIES，`
        + `基准序是默认值不是唯一值，但依赖是硬约束`,
    );
    return fail(
      "PIPELINE_ORDER_VIOLATION",
      `管线序违反依赖：${violation.stage} 早于其前置 ${violation.missingBefore}`,
      `请把 ${violation.missingBefore} 前移至 ${violation.stage} 之前`,
    );
  }

  const nonDefault = order.some((s, i) => PIPELINE_SEQUENCE[i] !== s);
  if (nonDefault) {
    bag.push(
      "PIPELINE_ORDER_VIOLATION",
      `候选管线序合法但偏离基准序（基准：${PIPELINE_SEQUENCE.join(" → ")}）`,
      `偏离合法仅在依赖允许范围内；请在提交 ADR 时记录重排理由，供 F2013 调试视图与性能归因对照`,
    );
  }

  return ok(order.slice(), bag.all());
}

/** 管线序的编译期校验：基准序自身必须自洽（否则架构声明本身有错）。 */
export function selfCheckBaselineSequence(): Outcome<readonly PipelineStage[]> {
  return resolvePipelineOrder(PIPELINE_SEQUENCE);
}

// ════════════════════════════════════════════════════════════════════════════
// §5 K 域组内分工总览（判据四的落地：20 条分工行）
// ════════════════════════════════════════════════════════════════════════════

/** 分工行：组 id + 席位 + 职责 + 交付契约名 + 门禁判据。 */
export interface WorkforceRow {
  /** 组 id（K01~K10，与十主题一一对应，序不同但分组一致）。 */
  readonly group: string;
  /** 席位：架构席（声明与边界）or 实现席（算法与门禁）。 */
  readonly seat: "architecture" | "implementation";
  /** 职责一句话。 */
  readonly duty: string;
  /** 本席位交付的契约名（下游消费的具体数据结构）。 */
  readonly deliverable: string;
  /** 门禁判据：本席位完成的可机检判据。 */
  readonly gate: string;
}

/**
 * K 域十组 × 两席 = 20 条分工行。
 *
 * 为什么把「架构席」与「实现席」并列成行而不是只列十行分组：
 *   本域最容易出的事故是「架构声明与执行器实现各写各的」——架构说某 RT 有别名
 *   空间，执行器按另一种假设分配内存；架构说参数与结构分离免重编译，实现里
 *   把参数塞进结构导致每次调参都重编译。把两个席位绑在同一行、共享同一条门禁，
 *   就把「声明与实现必须对账」变成了排产结构上的必然，而不是靠自觉。
 */
export const WORKFORCE_TABLE: readonly WorkforceRow[] = [
  {
    group: "K01",
    seat: "architecture",
    duty: "后处理链架构：DAG 图模型、统一效果接口、中间 RT 池契约、管线序与跨域挂接位（本条）",
    deliverable: "PostFxDomainArchitecture",
    gate: "十主题号段无缝覆盖 F2001~F2200，三契约接收核验可跑，管线序自洽",
  },
  {
    group: "K01",
    seat: "implementation",
    duty: "DAG 执行器：拓扑排序执行、帧边界原子重组、校验拒绝成环与缺失依赖",
    deliverable: "PostFxExecutionList",
    gate: "重组零半新半旧帧；成环与缺边被拒且报错含环路径",
  },
  {
    group: "K02",
    seat: "architecture",
    duty: "Bloom 组架构：源标记语义在本组的落位（掩码加权而非alpha 合成）与光敏联动声明",
    deliverable: "BloomSourceBinding",
    gate: "源标记缺失时退化为纯亮度提取并显性告警，不静默",
  },
  {
    group: "K02",
    seat: "implementation",
    duty: "Bloom 三段实现：明亮提取、多级 mip 模糊链、合成叠加",
    deliverable: "BloomComposite",
    gate: "级数切换帧边界生效；HDR 极亮值在提取端软钳制不炸帧",
  },
  {
    group: "K03",
    seat: "architecture",
    duty: "色调映射组架构：TM 语义边界（输出显示 referred）与 V 域显示变换的分界",
    deliverable: "ToneMapBoundary",
    gate: "边界断言拦截「K 侧做显示变换」；NaN 不得穿透到显示",
  },
  {
    group: "K03",
    seat: "implementation",
    duty: "曝光应用位与三族曲线实现（ACES 近似/Reinhard/Uncharted2）",
    deliverable: "ToneMappedFrame",
    gate: "曲线单调性校验通过；曝光过渡一阶惯性不跳变",
  },
  {
    group: "K04",
    seat: "architecture",
    duty: "输出编码组架构：编码标记系统（每 RT 携带编码类型）与编码/解码方向防错配",
    deliverable: "EncodedFrameContract",
    gate: "方向错配在开发期被断言拦截；边界双端确认（K 写已编码、V 校验）",
  },
  {
    group: "K04",
    seat: "implementation",
    duty: "sRGB/PQ 编码器实现与目标色彩空间能力降级路径",
    deliverable: "EncodedFrame",
    gate: "编码器与参考对拍偏差在声明精度内；目标空间未知时默认 sRGB 并显性声明",
  },
  {
    group: "K05",
    seat: "architecture",
    duty: "抗锯齿组架构：四法族挂载位、MSAA resolve 序位声明、互斥守卫与选型决策树",
    deliverable: "AaFamilyPlan",
    gate: "MSAA 与 TAA 互斥；resolve 位在后处理之前（上游冻结序）",
  },
  {
    group: "K05",
    seat: "implementation",
    duty: "FXAA/TAA 实现与历史校验管线（velocity reject/深度法线相似度/AABB clipping）",
    deliverable: "AaResolvedFrame",
    gate: "鬼影与闪烁在标准场景集下不可检出；速度缓冲缺失时退化显性",
  },
  {
    group: "K06",
    seat: "architecture",
    duty: "色彩分级组架构：基础校正与风格分级的分界、开放格式与预设生态挂接",
    deliverable: "GradingContract",
    gate: "分级资产为开放格式可迁移；档位与预设引用无二义",
  },
  {
    group: "K06",
    seat: "implementation",
    duty: "色轮/曲线/风格 LUT 实现与色域裁剪",
    deliverable: "GradedFrame",
    gate: "越界色域被裁剪并告警；LUT 缺失走显性降级不黑屏",
  },
  {
    group: "K07",
    seat: "architecture",
    duty: "景深组架构：焦平面数据来源、深度采样口径与降采样策略",
    deliverable: "DofContract",
    gate: "焦平面跟随无跳变；深度缺失时降级显性",
  },
  {
    group: "K07",
    seat: "implementation",
    duty: "散景模拟实现与样本抖动（样本数随质量档位变化）",
    deliverable: "DofBlendedFrame",
    gate: "半分辨率链成本减半实测入预算表；样本数与档位一致",
  },
  {
    group: "K08",
    seat: "architecture",
    duty: "运动模糊组架构：速度缓冲复用契约与快门时长的物理语义",
    deliverable: "MotionBlurContract",
    gate: "与 TAA 共用速度缓冲但不共享语义（复用声明单源）",
  },
  {
    group: "K08",
    seat: "implementation",
    duty: "时域拖影实现与运动向量越界钳制",
    deliverable: "MotionBlurredFrame",
    gate: "速度缓冲缺失时退化为无模糊并告警；钳制后无拖影炸裂",
  },
  {
    group: "K09",
    seat: "architecture",
    duty: "镜头效果组架构：色差/暗角/颗粒/锐化的共阶段编排与光敏联动声明",
    deliverable: "CameraFxContract",
    gate: "颗粒噪声不得制造闪烁（光敏红线）；共阶段效果的重排须过依赖检查",
  },
  {
    group: "K09",
    seat: "implementation",
    duty: "四效果实现与参数钳制（越界参数一律钳制不外溢）",
    deliverable: "CameraFxFrame",
    gate: "参数越界钳制生效；锐化与 FXAA 对冲关系在序位上协调",
  },
  {
    group: "K10",
    seat: "architecture",
    duty: "K 域收口组架构：预算/档位/fuzz/文档/验收的域级编排与跨批对接登记",
    deliverable: "KDomainCloseoutPlan",
    gate: "20 维度验收表齐备；预算联动次序单源且与档位表一致",
  },
  {
    group: "K10",
    seat: "implementation",
    duty: "性能打点、档位切换、fuzz 案例库与缺陷账本闭环",
    deliverable: "KDomainEvidenceSet",
    gate: "打点开销<1%；档位切换帧边界原子；fuzz 全非法形态被拦截",
  },
];

// ════════════════════════════════════════════════════════════════════════════
// §6 无障碍替述与隐私面声明（锚点：无障碍与隐私）
// ════════════════════════════════════════════════════════════════════════════

/** 隐私面声明。本域为纯契约层，运行时零隐私面；出现任何用户数据面即架构违规。 */
export interface PrivacyManifest {
  readonly runtimePrivacySurface: "none";
  readonly collectsUserContent: false;
  readonly rationale: string;
}

/** 隐私面声明（唯一事实源）。 */
export const K_PRIVACY_MANIFEST: PrivacyManifest = {
  runtimePrivacySurface: "none",
  collectsUserContent: false,
  rationale:
    "本条只产出契约与校验结果，不读取用户数据、不写盘、不发网络请求；"
    + "架构层引入隐私面会让「K 域到底收集了什么」这个问题在后期无法回答，故显式声明为零",
};

/**
 * 无障碍替述：为架构声明生成**纯文本**可读描述（图必配文的机读形态）。
 * 替述须覆盖：链的起点与终点、十主题分组、当前开工阻断项、下一步怎么办。
 * 返回字符串数组而非富文本，便于屏幕阅读器逐段朗读，也便于文档系统直接引用。
 */
export function accessibleArchitectureNarrative(
  report: HandoverReport = verifyHandoverLedger(),
): readonly string[] {
  const out: string[] = [];
  out.push(
    `K 域后处理链架构：管线从光照输出开始，到呈现结束，共 ${PIPELINE_SEQUENCE.length} 个阶段。`
    + `阶段顺序为 ${PIPELINE_SEQUENCE.map((s) => STAGE_LABEL[s]).join("、然后 ")}。`,
  );
  out.push(
    `K 域包含 ${EFFECT_IDS.length} 个官方效果主题，每个主题分配 20 个功能号，`
      + `合计覆盖 F2001 至 F2200，中间无空洞无重叠。`,
  );
  for (const spec of Object.values(K_THEME_REGISTRY)) {
    out.push(
      `主题 ${spec.name}：功能号 F${spec.itemRange.lo} 到 F${spec.itemRange.hi}，`
        + `位于${STAGE_LABEL[spec.stage]}，读取 ${spec.inputs.join("、")}，产出 ${spec.output}，`
        + `参数块名为 ${spec.params}。${spec.duty}。`,
    );
  }
  if (report.mayStart) {
    out.push("上游三份契约均已签收且范围覆盖消费字段，K 域开工条件已满足。");
  } else {
    out.push(`当前K 域开工被阻断，共 ${report.blockers.length} 项待处理：`);
    for (const row of report.rows) {
      if (row.status === "signed") continue;
      out.push(`契约 ${row.id} 的状态是 ${row.status}；下一步：补齐签名或扩大签收范围后重跑核验。`);
    }
  }
  out.push("本域为架构契约层，不收集任何用户内容，无运行时隐私面。");
  return out;
}

// ════════════════════════════════════════════════════════════════════════════
// §7 域级自检（把判据变成一条可执行的门禁）
// ════════════════════════════════════════════════════════════════════════════

/** 域开工核验报告。 */
export interface DomainCheckReport {
  /** 是否允许开工（三契约已签 + 自检全绿）。 */
  readonly mayStart: boolean;
  /** 自检产生的全部诊断（含阻断与非致命告警）。 */
  readonly diagnostics: readonly Diagnostic[];
  /** 覆盖的功能号总数（应等于 2000? 否——应为 200 个号位：2001..2200）。 */
  readonly coveredItems: number;
}

/**
 * 域级自检（开工门禁）：一次跑完判据的全部机检项。
 *
 * 机检项清单（逐条对应判据）：
 *   ① 十主题封闭集完整、号段无缝覆盖 F2001~F2200、无重叠（判据一）；
 *   ② 每个效果规格五项齐备（输入/输出/参数块/开关/分发）且分发为静态（判据一）；
 *   ③ 三契约接收核验（判据三）；
 *   ④ 基准管线序自洽 + 上游冻结序未被改动（判据四）。
 */
export function selfCheckDomain(): DomainCheckReport {
  const bag = new DiagBag();

  // ① 主题完整性与号段
  if (EFFECT_IDS.length !== 10) {
    bag.push(
      "EFFECT_UNREGISTERED",
      `K 域应登记 10 个官方主题，当前 ${EFFECT_IDS.length} 个`,
      `补齐主题至 10 个；官方十主题为 Bloom/ToneMapping/DoF/MotionBlur/色差/暗角/颗粒/锐化/色彩分级/AA`,
    );
  }

  const sorted = [...EFFECT_IDS].sort(
    (a, b) => K_THEME_REGISTRY[a].itemRange.lo - K_THEME_REGISTRY[b].itemRange.lo,
  );
  let expectedLo = 2001;
  let covered = 0;
  for (const id of sorted) {
    const spec = K_THEME_REGISTRY[id];
    if (spec.itemRange.lo !== expectedLo) {
      if (spec.itemRange.lo < expectedLo) {
        bag.push(
          "ITEM_RANGE_OVERLAP",
          `主题 ${spec.name} 的号段起点 F${spec.itemRange.lo} 与前序主题重叠（期望 F${expectedLo}）`,
          `重排号段使各主题不重叠；号段重叠会让功能号失去唯一归属，追溯时无从下手`,
        );
      } else {
        bag.push(
          "ITEM_RANGE_GAP",
          `主题 ${spec.name} 的号段起点 F${spec.itemRange.lo} 与前序主题之间存在空洞 F${expectedLo}~F${spec.itemRange.lo - 1}`,
          `补齐空洞或调整前后主题号段；空洞意味着有功能号无人认领，排产与验收都会漏`,
        );
      }
    }
    if (spec.itemRange.hi < spec.itemRange.lo) {
      bag.push(
        "ITEM_RANGE_OVERLAP",
        `主题 ${spec.name} 的号段倒置：F${spec.itemRange.lo} > F${spec.itemRange.hi}`,
        "修正号段为 lo ≤ hi",
      );
    } else {
      covered += spec.itemRange.hi - spec.itemRange.lo + 1;
    }
    expectedLo = spec.itemRange.hi + 1;
  }
  if (expectedLo !== 2201) {
    bag.push(
      "ITEM_RANGE_GAP",
      `十主题号段终点为 F${expectedLo - 1}，K 域应覆盖至 F2200（差 ${2201 - expectedLo} 个号位）`,
      "补齐尾部号段；K 域共 200 项（F2001~F2200），少一即域不完整",
    );
  }

  // ② 效果规格五项齐备 + 静态分发
  for (const id of EFFECT_IDS) {
    const spec = K_THEME_REGISTRY[id];
    if (spec.inputs.length === 0) {
      bag.push(
        "EFFECT_SPEC_INCONSISTENT",
        `效果 ${spec.name} 未声明输入 RT 依赖`,
        "补齐 inputs；无输入的效果在链上无位置，属声明缺失",
      );
    }
    if (spec.output.trim().length === 0) {
      bag.push(
        "EFFECT_SPEC_INCONSISTENT",
        `效果 ${spec.name} 未声明输出 RT`,
        "补齐 output；输出 RT 名是 F2003 池分配与 F2013 逐级透视的共同寻址键",
      );
    }
    if (spec.params.trim().length === 0) {
      bag.push(
        "EFFECT_SPEC_INCONSISTENT",
        `效果 ${spec.name} 未声明参数块`,
        "补齐 params；参数与结构必须分离，否则纯参数变化会触发图重编译",
      );
    }
    if (spec.dispatch !== "static") {
      bag.push(
        "STATIC_DISPATCH_BROKEN",
        `效果 ${spec.name} 的分发型为 ${spec.dispatch}，违反「静态分发零虚拟开销」的域级设计声明`,
        `改回 static；动态分发在每效果数十次调用的热路径上引入不可预测的间接开销，`
          + `且与 F2014 预算模型的常量假设冲突`,
      );
    }
    if (spec.inputs.includes(spec.output)) {
      bag.push(
        "EFFECT_SPEC_INCONSISTENT",
        `效果 ${spec.name} 的输入包含其自身输出 ${spec.output}（读写同一 RT）`,
        "把输出改为独立 RT；读写同一 RT 在原位pass 下是未定义行为，且会破坏池的区间别名分析",
      );
    }
    const stageOk = STAGE_RANK[spec.stage] !== undefined;
    if (!stageOk) {
      bag.push(
        "EFFECT_SPEC_INCONSISTENT",
        `效果 ${spec.name} 声明的管线阶段 ${spec.stage} 未在 STAGE_RANK 中登记`,
        "先在 PipelineStage/STAGE_RANK/STAGE_DEPENDENCIES 登记该阶段，再引用",
      );
    }
  }

  // ③ 三契约接收
  const handover = verifyHandoverLedger();
  for (const b of handover.blockers) bag.push(b.code, b.message, b.hint);

  // ④ 管线序自洽 + 冻结序
  const seq = selfCheckBaselineSequence();
  if (!seq.ok) {
    bag.push(seq.code, seq.message, seq.hint);
  } else {
    bag.pushAll(seq.diagnostics);
  }
  if (PIPELINE_SEQUENCE[0] !== "lighting-output") {
    bag.push(
      "UPSTREAM_SEQUENCE_CONFLICT",
      "基准序起点不是光照输出，与 J 域 F1841 冻结序冲突",
      "恢复基准序起点为 lighting-output；上游冻结序优先于本域偏好",
    );
  }
  if (PIPELINE_SEQUENCE[PIPELINE_SEQUENCE.length - 1] !== "present") {
    bag.push(
      "UPSTREAM_SEQUENCE_CONFLICT",
      "基准序终点不是呈现，与 V 域呈现边界冻结序冲突",
      "恢复基准序终点为 present；K 域交付已编码帧后止步",
    );
  }

  // ⑤ 分工表行数与契约名唯一
  if (WORKFORCE_TABLE.length !== 20) {
    bag.push(
      "WORKFORCE_ROW_COUNT_INVALID",
      `分工表应为 20 行，当前 ${WORKFORCE_TABLE.length} 行`,
      "补齐或纠正分工行；分工总览是组内排产的唯一依据，行数漂移意味着有席位无人认领",
    );
  }
  const seenDeliverable = new Map<string, string>();
  for (const row of WORKFORCE_TABLE) {
    const prev = seenDeliverable.get(row.deliverable);
    if (prev !== undefined) {
      bag.push(
        "CONTRACT_NAME_DUPLICATE",
        `交付契约名 ${row.deliverable} 被 ${prev} 与 ${row.group}/${row.seat} 两行重复声明`,
        "改为各自唯一的契约名；同名契约会让下游 import 到错误实现",
      );
    } else {
      seenDeliverable.set(row.deliverable, `${row.group}/${row.seat}`);
    }
  }

  // ⑥ 隐私面
  if (K_PRIVACY_MANIFEST.runtimePrivacySurface !== "none" || K_PRIVACY_MANIFEST.collectsUserContent) {
    bag.push(
      "PRIVACY_SURFACE_DECLARED",
      "K 域架构层声明了运行时隐私面，与「纯契约层零隐私面」的域级立场冲突",
      "把隐私面下沉到具体效果条目并单独评审；架构层出现隐私面会让数据收集面无法审计",
    );
  }

  return { mayStart: handover.mayStart && bag.byCode("PIPELINE_ORDER_VIOLATION").length === 0 && !bag.hasError, diagnostics: bag.all(), coveredItems: covered };
}

/**
 * 开工就绪判定的便捷入口：返回一段人话结论（含阻断原因与下一步），供 CLI 与
 * 调度侧直接打印——避免「自检返回了一堆Diagnostic 但没人读」的静默失败。
 */
export function describeReadiness(
  report: DomainCheckReport = selfCheckDomain(),
): string {
  if (report.mayStart) {
    return (
      `K 域开工条件已满足：十主题覆盖 ${report.coveredItems} 个功能号位，`
      + `三契约全部签收，管线序自洽。可进入 K01 首批效果实现。`
    );
  }
  const blockers = report.diagnostics.filter(
    (d) => d.code !== "PIPELINE_ORDER_VIOLATION",
  );
  const lines = blockers.slice(0, 8).map((d, i) => `  ${i + 1}) [${d.code}] ${d.message}\n     下一步：${d.hint}`);
  const more = blockers.length > 8 ? `\n  ……另有 ${blockers.length - 8} 项，见 selfCheckDomain().diagnostics` : "";
  return `K 域开工被阻断（${blockers.length} 项）：\n${lines.join("\n")}${more}`;
}