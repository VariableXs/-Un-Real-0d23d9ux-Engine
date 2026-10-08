/**
 * VE-F2601 · N 域开工与 UI 内核总架构（VE-N 域 · UI 框架内核域开工条 · 批次 N01 首项）
 * ---------------------------------------------------------------------------
 * 职责定位：VE-N 域（F2601~F2800）的**开工条**。它不建一个控件、不排一次布局、
 * 不跑一次命中，只做一件事：把「UI 框架内核域要造什么、十主题按什么序造、
 * 与渲染域（D）/动画域（M）/上层业务控件库（VE-N-UI）如何划界、
 * 从上游 M 域接收哪两份协议并以目标方身份签收」四件事写成**可机检的契约**。
 *
 * 为什么 UI 内核的开工条必须是代码而不是文档（域级立场）：
 *   UI 框架是全VE 中**四域交汇最密**的域——它向D 域交付可视树结构供绘制，向
 *   M 域交付属性系统供动画轨道挂载，又承载 VE-N-UI 上层的全部业务控件。若这些
 *   挂接位只写在文档里，集成期必然出现四类事故：
 *     ① N 域顺手调了绘制接口（D 域的活）→ 两处各画一遍，重影且无人负责；
 *     ② M 域动画轨道直接写属性内部字段（绕过属性引擎四段管线）→ 变更通知丢失，
 *        失效标记不触发，改了属性画面不动，且这类bug 只在「动画与脚本同时改属性」
 *        时偶发，极难定位；
 *     ③ 焦点与命中各写一套逆序遍历规则 → Tab 顺序与鼠标点击指向不同控件，
 *        键盘用户与鼠标用户能力不对等（无障碍红线）；
 *     ④ 上层业务控件库绕过属性引擎直写控件内部状态 → 内核不变量（三不变量）
 *        被上层击穿，fuzz 一夜之间从「无问题」变成「可复现崩溃」。
 *   四类事故的共同点是「错了但没人被告知」。故本条把边界写成注册表 + 裁决器：
 *   越界、越序、缺签、协议与属性系统冲突在开发期就被判，而不是在集成期靠人吵。
 *
 * 五条锚点契约（逐条对应判据）：
 *   1. 域开工 + 十主题 —— 官方十主题（控件树/测量布局/命中测试/滚动/焦点/
 *      数据绑定/虚拟化列表/剪裁/无障碍布局/渲染对接）逐项声明：主题 id、中文名、
 *      功能号区间、兑现组、职责边界、下游消费方、禁做项。九个独占主题号段
 *      无缝覆盖 F2601~F2760；第十主题「渲染对接」为**横切主题**（无独占号段），
 *      其兑现条目分布在 N01/N02（可视树→D 渲染输入）与 N10（渲染契约签署）；
 *      F2761~F2800 为 N09 预备与 N10 收口保留段，登记在CLOSEOUT_RESERVE。
 *      号段口径与偏离一律走 ADR，不允许两处各写一份号段表。
 *   2. 三大件 —— 控件树双树架构（F2605逻辑树+可视树）/ 属性引擎（F2604 四段
 *      管线）/ 增量更新（F2606 精确失效+帧边界批处理）三件的契约面在此冻结：
 *      三大件互相依赖方向（树→属性→增量）由依赖表机检，出现反向依赖即架构违规。
 *      本条只声明契约面，实现分落F2602~F2608。
 *   3. 四域边界 —— N（结构行为）/ D（渲染绘制）/ M（动画驱动）/ VE-N-UI（业务
 *      控件）四域逐域职责与禁做项表，并给出冲突裁决规则：能力归属唯一域，
 *      越界方回改；属性写入为「接收方优先」的**仲裁位**（M 侧须走属性引擎管线，
 *      冲突以属性系统为准回改 M 协议，走 ADR）。
 *   4. 双协议接收 —— F2461 UI 挂载协议 + F2462 系统动画驱动协议，以**协议目标方**
 *      （N 域属性系统）身份签收：逐字段登记消费范围，范围不覆盖即阻断；
 *      上游内容哈希未提供者记为待办告警（不阻断，但必须显式列出下一步）。
 *   5. 组内分工总览 —— N 域十组（N01~N10）共 20 条分工行，每行带交付契约名与门禁判据。
 *
 * 零静默纪律：越界、越序、缺签、范围不符、哈希待办、契约名撞号、隐私面声明，
 * 全部产出 Diagnostic（code + message + hint）并由调用方聚合上报；
 * 本模块不抛异常、不吞诊断。
 *
 * 判据：域开工、三大件、四域边界、双协议接收、判据。
 * 交接说明：本条是纯契约层——零渲染调用、零 DOM 依赖、零全局可变状态，
 *         顶层只有常量表与纯函数，可在任意宿主（浏览器/Worker/Node 校验脚本）
 *         中原样引入。下游 N01（F2602 控件树模型起）逐项消费本条
 *         N_THEME_REGISTRY、ARCHITECTURE_PILLARS、DOMAIN_BOUNDARY_TABLE、
 *         PROTOCOL_RECEIPT_LEDGER 与 HANDOVER_CHAIN_LEDGER。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型（零静默的基础设施：域内独立实现，不跨域 import）
// ════════════════════════════════════════════════════════════════════════════

/** 诊断码：每种拒绝/越界/退化独立可检索，绝不合并成一条通用错误。 */
export type DiagCode =
  /** M04 双协议之一未签收（缺目标方签名或签名字段不全）——开工阻断级。 */
  | "PROTOCOL_UNSIGNED"
  /** 签收了但确认范围不覆盖本域将消费的字段（签了 A 却要 B）。 */
  | "PROTOCOL_SCOPE_MISMATCH"
  /** 上游协议内容哈希待补：未阻断，但必须显式列出下一步（不得静默放行）。 */
  | "PROTOCOL_HASH_PENDING"
  /**
   * 上游协议内容哈希漂移：签收时哈希 ≠ 上游当前哈希 = **阻断**级。
   * 与 PROTOCOL_HASH_PENDING 分开立码，因为二者处置完全相反——待补可以推进，
   * 漂移必须重签；若共用一个码，按码筛选时会把「必须重签」混进「可以先干活」，
   * 处置方向就反了。
   */
  | "PROTOCOL_HASH_DRIFT"
  /** M04 协议与属性系统冲突：须以属性系统为准回改 M 侧协议（ADR——接收方优先）。 */
  | "PROTOCOL_PROPERTY_CONFLICT"
  /** 引用了未在十主题注册表登记的主题（封闭集之外的主题不可进入排产）。 */
  | "THEME_UNREGISTERED"
  /** 主题规格自相矛盾（组别缺失、下游消费方为空、禁做项为空、号段形态与号段不符）。 */
  | "THEME_SPEC_INCONSISTENT"
  /** 主题功能号区间与其他主题重叠（两个主题抢同一批功能号）。 */
  | "ITEM_RANGE_OVERLAP"
  /** 主题功能号区间有空洞（F2601~F2760 未被九个独占主题完整覆盖）。 */
  | "ITEM_RANGE_GAP"
  /** 收口保留段与主题号段越界（F2761~F2800 被主题占用或保留段宽度不对）。 */
  | "RESERVE_RANGE_BROKEN"
  /** 横切主题被误派独占号段（渲染对接兑现于他组条目，再给号段即重复计工）。 */
  | "CROSSCUTTING_THEME_ASSIGNED"
  /** 三大件依赖成环（树/属性/增量互相要求在前）。 */
  | "PILLAR_CYCLE"
  /** 三大件依赖缺失（引用了未登记的件，或三大件不齐备）。 */
  | "PILLAR_DEPENDENCY_MISSING"
  /** 三大件被反向依赖（属性依赖树是对的；树依赖属性即架构违规）。 */
  | "PILLAR_REVERSE_DEPENDENCY"
  /** 三大件规格不完整（对外能力面或职责边界为空）。 */
  | "PILLAR_SPEC_INCONSISTENT"
  /** 能力归属多域声明（同一能力被两个域认领「我管」——裁决无解）。 */
  | "CAPABILITY_OWNER_CONFLICT"
  /** 能力无域认领（有人要用，但四域边界表里没人负责）。 */
  | "CAPABILITY_UNCLAIMED"
  /** 域越界：申请的能力不属于该域（边界表裁决回改）。 */
  | "DOMAIN_BOUNDARY_VIOLATION"
  /** 上游契约链断链（K→L→M→N 任一环缺签收）。 */
  | "HANDOVER_CHAIN_BROKEN"
  /** 上游 M 域收官宣告（F2600）未生效。 */
  | "UPSTREAM_NOT_EFFECTIVE"
  /** 交付契约名重复（两个分工行声明同名契约 = 后续必然撞名）。 */
  | "CONTRACT_NAME_DUPLICATE"
  /** 分工表行数不等于 20（分组总览被裁剪或重复登记）。 */
  | "WORKFORCE_ROW_COUNT_INVALID"
  /** 声明了运行时隐私面（本域零隐私面，出现即架构违规）。 */
  | "PRIVACY_SURFACE_DECLARED"
  /** 横切主题被误当独占主题排产（渲染对接无独占号段）。 */
  | "CROSSCUTTING_THEME_RANGE_ASSIGNED";

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

/** 成功构造（diagnostics 允许携带非致命告警，例如哈希待办）。 */
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

  /** 是否存在阻断级诊断。 */
  get hasError(): boolean {
    return this.items.length > 0;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 N 域十主题注册表（判据一：域开工 + 十主题）
// ════════════════════════════════════════════════════════════════════════════

/** 兑现组 id（N01~N10）。组是排产单位，主题是归属单位，二者不是同一个轴。 */
export type GroupId = "N01" | "N02" | "N03" | "N04" | "N05" | "N06" | "N07" | "N08" | "N09" | "N10";

/**
 * 主题 id 封闭集。九个独占主题 + 一个横切主题。
 * 横切主题（渲染对接）没有独占号段——它兑现于其他组的条目之内，
 * 若给它分配独占号段会与那些条目重复计工，故注册表显式区分两类。
 */
export type ThemeId =
  | "control-tree"
  | "measure-layout"
  | "hit-test"
  | "scroll"
  | "focus"
  | "data-binding"
  | "virtualized-list"
  | "clipping-transform"
  | "a11y-semantics"
  | "render-handoff";

/** 主题的号段形态：独占（有独立号段）/ 横切（兑现于他组条目之内）。 */
export type ThemeRangeKind = "exclusive" | "crosscutting";

/** 一个主题的完整规格。锚点要求的五项逐字段落在下列结构上。 */
export interface ThemeSpec {
  readonly id: ThemeId;
  /** 中文名（对外文档与错误提示引用同一事实源）。 */
  readonly name: string;
  readonly rangeKind: ThemeRangeKind;
  /**
   * 独占号段（含端点）。横切主题此字段为 null——不是漏填，是显式的
   * 「无独占号段」；把它填成假数字会让自检误以为号段无缝。
   */
  readonly itemRange: { readonly lo: number; readonly hi: number } | null;
  /** 兑现组（横切主题可列多个兑现点）。 */
  readonly realizedBy: readonly GroupId[];
  /** 职责一句话：负责什么。 */
  readonly duty: string;
  /** 下游消费方：谁拿这个主题的产出继续做事（对端域/上层库）。 */
  readonly consumedBy: readonly string[];
  /** 禁做项：明确不负责什么（越界即DOMAIN_BOUNDARY_VIOLATION）。 */
  readonly mustNot: readonly string[];
  /** 本主题兑现的锚点条目（册内编号，便于追溯来历）。 */
  readonly fulfills: readonly string[];
}

/** 主题 id 全键（遍历用，避免手写清单与实际注册漂移）。 */
export const THEME_IDS: readonly ThemeId[] = [
  "control-tree",
  "measure-layout",
  "hit-test",
  "scroll",
  "focus",
  "data-binding",
  "virtualized-list",
  "clipping-transform",
  "a11y-semantics",
  "render-handoff",
];

/**
 * 号段口径声明（显式记录，避免后来者以为 N09/N10 是漏排）：
 *   锚点 F2601 列出的官方十主题是**主题轴**，而 F2601~F2800 的号段是**组轴**。
 *   两条轴在 N05（焦点系统与数据绑定合组）与 N09/N10（预备与收口，非主题组）
 *   处不对齐——即「十主题」不等于「十组× 二十项」的整齐方阵。本条采取的口径：
 *     ① 九个独占主题按组边界切分，无缝覆盖 F2601~F2760（160 项）；
 *     ② 第十主题「渲染对接」为横切主题，不占独占号段，兑现点登记在
 *        N01/N02（可视树→D 渲染输入）与 N10（渲染契约签署）；
 *     ③ F2761~F2800（40 项）登记为收口保留段：N09 预备自查 20 项 +
 *        N10 收口宣告 20 项。
 *   若将来把横切主题升格为独占主题（或把预备/收口并入某主题），须走 ADR
 *   调整本口径与 CLOSEOUT_RESERVE，并重跑自检——不得靠临时插空号段绕过
 *   区间重叠检查（与 K 域 RANGE_MERGE_NOTE 同一纪律）。
 */
export const THEME_RANGE_NOTE =
  "九独占主题无缝覆盖 F2601~F2760；渲染对接为横切主题无独占号段（兑现于 N01/N02/N10）；"
  + "F2761~F2800 为 N09 预备与 N10 收口保留段。口径调整须走 ADR 并重跑域级自检。";

/** 收口保留段：N09 预备自查 + N10 收口宣告，不属任何主题的独占号段。 */
export const CLOSEOUT_RESERVE: Readonly<{
  lo: number;
  hi: number;
  split: readonly { group: GroupId; lo: number; hi: number }[];
}> = {
  lo: 2761,
  hi: 2800,
  split: [
    { group: "N09", lo: 2761, hi: 2780 },
    { group: "N10", lo: 2781, hi: 2800 },
  ],
};

/** N 域十主题注册表。判据一要求的逐项声明在此为唯一事实源。 */
export const N_THEME_REGISTRY: Readonly<Record<ThemeId, ThemeSpec>> = {
  "control-tree": {
    id: "control-tree",
    name: "控件树",
    rangeKind: "exclusive",
    itemRange: { lo: 2601, hi: 2620 },
    realizedBy: ["N01"],
    duty: "控件树四要素模型（属性/子节点/事件/状态）、三不变量（单亲/无环/序稳定）、三遍历接口与开放序列化",
    consumedBy: ["VE-N-UI 上层业务控件库", "N02 布局（消费遍历与尺寸属性）", "N03命中（消费可视遍历）"],
    mustNot: ["绘制任何像素（归 D 域）", "决定业务控件的产品语义（归 VE-N-UI）", "改写布局测量结果（归 N02）"],
    fulfills: ["VE-F2602 控件树模型", "VE-F2605 双树分离", "VE-F2607 树序列化"],
  },
  "measure-layout": {
    id: "measure-layout",
    name: "测量布局",
    rangeKind: "exclusive",
    itemRange: { lo: 2621, hi: 2640 },
    realizedBy: ["N02"],
    duty: "measure→arrange 两阶段布局模型、五类布局容器语义、增量失效驱动的局部重排、布局与变换树的分工纪律",
    consumedBy: ["N01 增量更新（布局失效消费）", "D 域（布局矩形→绘制位置）", "N03 命中（布局矩形→命中区域）"],
    mustNot: ["在布局里直接调绘制接口（归 D 域）", "动画位移走布局重排（位移走变换，见 F2624纪律）", "测量断行与字形（归 VE-E 文字引擎）"],
    fulfills: ["VE-F2621 布局架构", "VE-F2622 容器语义", "VE-F2623 文本测量"],
  },
  "hit-test": {
    id: "hit-test",
    name: "命中测试",
    rangeKind: "exclusive",
    itemRange: { lo: 2641, hi: 2660 },
    realizedBy: ["N03"],
    duty: "命中管线四步（空间裁剪→逆序遍历→命中规则→事件路由）、三命中模式与豁免、手势状态机、滚轮与命中的一致性",
    consumedBy: ["N05 焦点（命中结果决定点击聚焦目标）", "N04 滚动（滚轮命中→滚动链）", "D 域（命中区域来自布局矩形）"],
    mustNot: ["改变控件结构（归 N01）", "决定滚动位置（归 N04）", "跳过逆序遍历以图快（顺序是语义的一部分）"],
    fulfills: ["VE-F2641 命中架构", "VE-F2642 命中规则", "VE-F2643 输入事件管线"],
  },
  scroll: {
    id: "scroll",
    name: "滚动",
    rangeKind: "exclusive",
    itemRange: { lo: 2661, hi: 2680 },
    realizedBy: ["N04"],
    duty: "滚动容器语义、滚动物理（惯性/边界回弹）、嵌套滚动链、滚动条控件与吸附对齐",
    consumedBy: ["N06 虚拟化（滚动位置→可视窗口）", "N03 命中（滚轮事件→滚动链）", "M 域（滚动动画走变换）"],
    mustNot: ["直接搬运全部子节点（超窗由 N06 虚拟化负责）", "在滚动里做惯性积分之外的物理模拟（归 L 域）", "改写布局约束（归 N02）"],
    fulfills: ["VE-F2661 滚动架构", "VE-F2662 滚动容器语义", "VE-F2667 嵌套滚动链"],
  },
  focus: {
    id: "focus",
    name: "焦点",
    rangeKind: "exclusive",
    itemRange: { lo: 2681, hi: 2690 },
    realizedBy: ["N05"],
    duty: "焦点链维护、Tab 顺序与视觉顺序一致、焦点可见性、焦点与命中的键盘鼠标能力对等",
    consumedBy: ["VE-N-UI 业务控件库（对话框初始焦点）", "N03 命中（命中后请求聚焦）", "M 域（焦点环动画走变换）"],
    mustNot: ["让焦点环不可见（无障碍红线）", "用命中顺序代替视觉顺序定Tab 序", "跳过不可见控件却不告知（须计入无障碍诊断）"],
    fulfills: ["VE-F2681 N05 开工与焦点架构"],
  },
  "data-binding": {
    id: "data-binding",
    name: "数据绑定",
    rangeKind: "exclusive",
    itemRange: { lo: 2691, hi: 2700 },
    realizedBy: ["N05"],
    duty: "属性→属性绑定与绑定路径解析、绑定失效显性告警、值源优先级（本地>继承>默认）的统一出口",
    consumedBy: ["M 域（轨道写属性→属性引擎→绑定传播）", "VE-N-UI 业务控件库（MVVM 接入位）"],
    mustNot: ["绕过属性引擎直写控件内部字段", "绑定路径失效时静默空转（须显性告警）", "在绑定里做业务逻辑（归上层）"],
    fulfills: ["VE-F2681 N05 开工与焦点架构", "VE-F2604 属性系统绑定位"],
  },
  "virtualized-list": {
    id: "virtualized-list",
    name: "虚拟化列表",
    rangeKind: "exclusive",
    itemRange: { lo: 2701, hi: 2720 },
    realizedBy: ["N06"],
    duty: "可视窗口计算、回收池、分组与网格树形扩展、与滚动和焦点的三方联动（滚动定窗/焦点保可见）",
    consumedBy: ["N04 滚动（滚动位置驱动窗口）", "N05 焦点（焦点项必须被窗口包含）", "N08 无障碍（虚拟化项须可被读屏导航）"],
    mustNot: ["为不可见项创建完整控件（那不叫虚拟化）", "让焦点落在窗口外且不自动滚入视口", "在回收时丢失用户已输入内容（数据安全）"],
    fulfills: ["VE-F2701 虚拟化总览", "VE-F2702 虚拟化列表核心", "VE-F2703 虚拟化回收池"],
  },
  "clipping-transform": {
    id: "clipping-transform",
    name: "剪裁",
    rangeKind: "exclusive",
    itemRange: { lo: 2721, hi: 2740 },
    realizedBy: ["N07"],
    duty: "剪裁树与变换树的层级一致性、嵌套剪裁的正确合成、变换优先纪律在剪裁侧的兑现（位移走变换不走布局）",
    consumedBy: ["D 域（剪裁矩形→绘制裁剪区）", "N06 虚拟化（窗口边界即剪裁边界）", "M 域（变换动画的落点）"],
    mustNot: ["在剪裁里重排布局（越权到 N02）", "让子节点绕过父剪裁绘制（层级一致性破裂）", "把 DPI 缩放当变换叠加（缩放归布局侧）"],
    fulfills: ["VE-F2721 N07 开工与剪裁架构"],
  },
  "a11y-semantics": {
    id: "a11y-semantics",
    name: "无障碍布局",
    rangeKind: "exclusive",
    itemRange: { lo: 2741, hi: 2760 },
    realizedBy: ["N08"],
    duty: "语义节点模型与语义树增量维护、读屏导航、与虚拟化/剪裁的语义联动、全域无障碍核验",
    consumedBy: ["VE-N-UI 业务控件库（控件须声明语义角色）", "G09/M07 无障碍体系（四线在UI 侧的落点）"],
    mustNot: ["把语义信息只留在视觉层（读屏不可达即为功能缺失）", "语义树与可视树各自维护（双源必然漂移）", "以「暂时读屏不支持」为由不声明语义"],
    fulfills: ["VE-F2741 N08 开工与无障碍树架构", "VE-F2742 语义节点模型", "VE-F2744 语义树与读屏导航"],
  },
  "render-handoff": {
    id: "render-handoff",
    name: "渲染对接",
    rangeKind: "crosscutting",
    itemRange: null,
    realizedBy: ["N01", "N02", "N10"],
    duty: "把可视树与布局矩形翻译为D 域可消费的结构输入（绘制命令的输入而非绘制本身），并签署 N↔D 渲染契约",
    consumedBy: ["VE-D 域（2D 合成引擎，消费可视树与布局矩形）"],
    mustNot: ["在 N 域内调用绘制/着色器接口（那是 D 域的职责，越界即重影）", "把绘制状态当结构状态缓存（状态归D 域）", "给横切主题分配独占号段（会与兑现条目重复计工）"],
    fulfills: ["VE-F2605 可视树作为渲染结构源", "VE-F2621 布局矩形→绘制位置边界", "VE-F2781 N 域接口总账中的渲染契约签署"],
  },
};

/** 查主题规格；未注册返回显性失败——判据要求「十主题封闭集，排产只认注册表」。 */
export function lookupTheme(id: string): Outcome<ThemeSpec> {
  const table = N_THEME_REGISTRY as Record<string, ThemeSpec | undefined>;
  const spec = table[id];
  if (spec === undefined) {
    return fail(
      "THEME_UNREGISTERED",
      `主题 ${id} 不在 N 域十主题注册表中`,
      `已注册主题：${THEME_IDS.join("、")}；若确为新增主题，须先在 N_THEME_REGISTRY 与 ThemeId 类型两处同时登记`,
    );
  }
  return ok(spec);
}

/** 主题的中文名查询（文档渲染与错误提示共用同一事实源）。 */
export function themeLabel(id: ThemeId): string {
  return N_THEME_REGISTRY[id].name;
}

// ════════════════════════════════════════════════════════════════════════════
// §3 三大件架构声明（判据二：控件树 / 属性引擎 / 增量更新）
// ════════════════════════════════════════════════════════════════════════════

/** 三大件 id。 */
export type PillarId = "control-tree" | "property-engine" | "incremental-update";

/**
 * 一件架构契约。此处**只声明契约面**，不实现——实现分落 F2602~F2608。
 * 之所以开工就把契约面写死：三大件是N 域全部后续组的地基（布局消费树的遍历接口、
 * 命中消费布局矩形、动画挂载属性、虚拟化消费剪裁边界）。若接口在实现期才定，
 * N02~N08 会各自拿到不同版本的接口猜测，返工成本远高于现在写清楚。
 */
export interface PillarContract {
  readonly id: PillarId;
  readonly name: string;
  /** 兑现锚点条目。 */
  readonly anchor: string;
  /** 本件对外提供的能力面（下游只准从这里取，不准另开旁路）。 */
  readonly provides: readonly string[];
  /** 本件依赖的其他件（依赖方向由PILLAR_DEPENDENCIES 统一声明，此字段为自述）。 */
  readonly dependsOn: readonly PillarId[];
  /** 成本定标锚点（性能由基准条目定标，架构声明本身零运行时成本）。 */
  readonly costAnchor: string;
  /** 一句话职责边界：负责什么、不负责什么。 */
  readonly duty: string;
}

/**
 * 三大件依赖方向（生产者 → 消费者）：
 *   control-tree → property-engine：属性挂在树上（树是属性的宿主）；
 *   control-tree → incremental-update：树结构变更产生失效；
 *   property-engine → incremental-update：属性写入的第三段（失效标记）由增量消费。
 * 反向依赖一律不合法——尤其是 tree→property 的反向（树不该知道属性引擎的存在）
 * 与 incremental→tree（增量不该回写结构）。反向依赖出现的典型后果是
 * 「属性变化引发结构重建」，即布局每帧全量重排，性能悬崖且无告警。
 */
export const PILLAR_DEPENDENCIES: Readonly<Record<PillarId, readonly PillarId[]>> = {
  "control-tree": [],
  "property-engine": ["control-tree"],
  "incremental-update": ["control-tree", "property-engine"],
};

/** 三大件契约表（唯一事实源；实现条目须逐字段兑现）。 */
export const ARCHITECTURE_PILLARS: Readonly<Record<PillarId, PillarContract>> = {
  "control-tree": {
    id: "control-tree",
    name: "控件树（逻辑树 + 可视树）",
    anchor: "VE-F2602 控件树模型 / VE-F2605 逻辑树与可视树分离",
    provides: [
      "insert(parent, idx, node) / remove(node) / reorder(node, newIdx) 三树操作（批量=单事务）",
      "逻辑遍历 / 可视遍历 / 命中遍历 三遍历接口",
      "逻辑树↔可视树映射（模板展开一对多，逻辑为源、可视为投影）",
      "树度量（节点数/深度/宽度）",
    ],
    dependsOn: [],
    costAnchor: "VE-F2610 控件树基准（万节点四操作 P95 + 内存模型）",
    duty: "结构与不变量（三不变量）的唯一权威；不持有属性值语义（属性引擎的活）、不做绘制、不排版",
  },
  "property-engine": {
    id: "property-engine",
    name: "属性引擎（四段管线）",
    anchor: "VE-F2604 控件属性系统",
    provides: [
      "四段管线：写（类型校验）→校验（域钳制）→失效标记（渲染/布局/命中三失效）→通知（帧边界合并）",
      "依赖属性：继承（值源优先级 本地>继承>默认）+ 绑定（属性→属性，绑定位预留）",
      "M04 挂载目标：轨道写属性→四段管线触发（M04 契约的属性侧兑现）",
      "属性度量与失效计数（供遥测与调试读取）",
    ],
    dependsOn: ["control-tree"],
    costAnchor: "VE-F2610 四条目之属性引擎补充（万属性写入与通知耗时）",
    duty: "属性值与变更通知的唯一权威；变更通知必须走帧边界合并（风暴防护），禁止任何旁路直写",
  },
  "incremental-update": {
    id: "incremental-update",
    name: "增量更新（精确失效 + 帧边界批处理）",
    anchor: "VE-F2606 控件树增量更新",
    provides: [
      "精确失效：节点变更→仅该子树失效（兄弟与无关祖先不失效）",
      "帧边界批处理：同帧变更合并一次提交（批大小统计进遥测）",
      "三失效类型分发：渲染失效→D 域通知 / 布局失效→N02 通知 / 命中失效→N03 通知",
      "双树增量同步（逻辑变更→可视增量投影）",
    ],
    dependsOn: ["control-tree", "property-engine"],
    costAnchor: "VE-F2610 增量对比（全量 vs 增量收益倍数）+ VE-F2627 布局增量收益",
    duty: "失效范围与提交时机的唯一权威；失效范围退化为全树即P1 性能回退立案",
  },
};

/** 依赖可行性检查：给定候选依赖顺序，验证每个前置是否都排在它之前。 */
export function findPillarOrderViolation(order: readonly PillarId[]): {
  readonly pillar: PillarId;
  readonly missingBefore: PillarId;
} | null {
  const pos = new Map<PillarId, number>();
  order.forEach((p, i) => pos.set(p, i));
  for (const p of order) {
    const deps = PILLAR_DEPENDENCIES[p];
    if (deps === undefined) continue;
    for (const dep of deps) {
      const depPos = pos.get(dep);
      const selfPos = pos.get(p) ?? -1;
      if (depPos === undefined || depPos > selfPos) {
        return { pillar: p, missingBefore: dep };
      }
    }
  }
  return null;
}

/** 三大件依赖核验：覆盖性 → 未知依赖 → 反向依赖 → 序可行性。 */
export function verifyPillars(
  pillars: Readonly<Record<PillarId, PillarContract>> = ARCHITECTURE_PILLARS,
): Outcome<readonly PillarId[]> {
  const bag = new DiagBag();
  const ids = Object.keys(pillars) as PillarId[];

  // ① 三件齐备
  if (ids.length !== 3) {
    bag.push(
      "PILLAR_DEPENDENCY_MISSING",
      `N 域架构三大件应为 3 件（控件树/属性引擎/增量更新），当前 ${ids.length} 件`,
      "三大件缺一即下游无地基：布局要树遍历、动画要属性、渲染要失效范围；请补齐后再开工",
    );
  }

  // ② 依赖引用必须已登记
  for (const id of ids) {
    for (const dep of PILLAR_DEPENDENCIES[id] ?? []) {
      if (!ids.includes(dep)) {
        bag.push(
          "PILLAR_DEPENDENCY_MISSING",
          `三大件 ${id} 声明依赖 ${dep}，但 ${dep} 未在 ARCHITECTURE_PILLARS 登记`,
          `先登记 ${dep} 的契约面，再让 ${id} 依赖它；依赖一个不存在的件等于依赖一个会变的猜测`,
        );
      }
    }
  }

  // ③ 反向依赖：树不得依赖属性、增量不得依赖结构回写
  if ((PILLAR_DEPENDENCIES["control-tree"] ?? []).length > 0) {
    bag.push(
      "PILLAR_REVERSE_DEPENDENCY",
      `控件树声明了依赖 ${(PILLAR_DEPENDENCIES["control-tree"] ?? []).join("、")}，违反「树是宿主、属性挂树上」的方向`,
      "把 PILLAR_DEPENDENCIES['control-tree'] 改回空数组；树依赖属性会让「属性变化引发结构重建」，表现为每帧全量重排且无告警",
    );
  }
  if ((PILLAR_DEPENDENCIES["incremental-update"] ?? []).includes("incremental-update")) {
    bag.push(
      "PILLAR_CYCLE",
      "增量更新依赖自身，成环",
      "增量只消费树与属性的失效，不得回写结构；回写即结构与失效互相触发，热循环",
    );
  }

  // ④ 序可行性
  const order: PillarId[] = ["control-tree", "property-engine", "incremental-update"];
  const violation = findPillarOrderViolation(order);
  if (violation !== null) {
    bag.push(
      "PILLAR_CYCLE",
      `三大件依赖序把「${violation.pillar}」排在「${violation.missingBefore}」之前`,
      `把 ${violation.missingBefore} 前移；依赖方向是硬约束，序不可协商`,
    );
  }

  // ⑤ 每件的对外能力面与职责边界必须齐备（空能力面 = 旁路 invitation）
  for (const id of ids) {
    const p = pillars[id];
    if (p === undefined) continue;
    if (p.provides.length === 0) {
      bag.push(
        "PILLAR_SPEC_INCONSISTENT",
        `三大件 ${p.name} 未声明对外能力面`,
        "补齐 provides；能力面为空等于邀请下游另开旁路，而旁路必然与不变量打架",
      );
    }
    if (p.duty.trim().length === 0) {
      bag.push(
        "PILLAR_SPEC_INCONSISTENT",
        `三大件 ${p.name} 未声明职责边界`,
        "补齐 duty；「负责什么、不负责什么」缺一，边界裁决就无依据",
      );
    }
  }

  if (bag.hasError) {
    return fail(
      bag.all()[0]?.code ?? "PILLAR_DEPENDENCY_MISSING",
      bag.all()[0]?.message ?? "三大件核验失败",
      bag.all()[0]?.hint ?? "按诊断逐条修正后重跑 verifyPillars",
    );
  }
  return ok(order, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §4 四域边界表与冲突裁决（判据三：四域边界总声明）
// ════════════════════════════════════════════════════════════════════════════

/** 参与 UI 相关的四个域。 */
export type DomainId = "VE-N" | "VE-D" | "VE-M" | "VE-N-UI";

/**
 * 一个域拥有的能力条目。
 *
 * key 与 label **必须分开**（早期版本把二者写成一个字符串「draw-execution（绘制…）」，
 * 结果能力反查表按 key 匹配时永远匹配不上，越界裁决全线失灵——这类缺陷在
 * 运行时表现为「所有申请都被判为无主能力」，看起来像边界极严，实际是边界不存在）。
 * 拆开后：key 是机检键（英文 kebab，跨域引用只用它），label 是人话（错误提示与
 * 无障碍替述只用它），两者各司其职，不存在「一串话里既要当标识又要当描述」。
 */
export interface OwnedCapability {
  readonly key: string;
  readonly label: string;
}

/**
 * 一个域的边界声明：负责什么（owns）、明确不负责什么（mustNot）。
 * 两列都必须非空——只有 owns 没有 mustNot 的域形不成边界，
 * 因为没人能说出「越界」是什么，裁决就退化成吵架。
 */
export interface DomainBoundary {
  readonly id: DomainId;
  readonly name: string;
  /** 功能号区间（便于追溯本域职责来源）。 */
  readonly itemRange: { readonly lo: number; readonly hi: number };
  /** 权威锚点（册内条目）。 */
  readonly anchor: string;
  /** 负责的能力（机检键：能力→唯一属主域）。 */
  readonly owns: readonly OwnedCapability[];
  /** 禁做项（越界判据）。 */
  readonly mustNot: readonly string[];
}

/**
 * 四域边界表。锚点原文的机读形式：
 *   N 域管结构与行为（树/布局/命中/焦点）、D 域管渲染绘制、M 域管动画驱动、
 *   VE-N 上层管业务控件。
 * 拆成四个域而不是三个，是因为「上层业务控件库」虽与N 同名域前缀，但它是
 * 消费者而非内核——把它与N 域合并会让「内核不许越界去实现业务控件」这条
 * 边界无处安放，而越界的表现形式恰恰是内核长出业务语义。
 */
export const DOMAIN_BOUNDARY_TABLE: readonly DomainBoundary[] = [
  {
    id: "VE-N",
    name: "N 域 · UI 框架内核",
    itemRange: { lo: 2601, hi: 2800 },
    anchor: "VE-F2601 N 域开工与 UI 内核总架构（本条）",
    owns: [
      { key: "control-tree-structure", label: "控件树结构与三不变量" },
      { key: "measure-layout", label: "测量与排列两阶段布局" },
      { key: "hit-testing", label: "命中管线与事件路由" },
      { key: "scroll-container", label: "滚动容器与滚动链" },
      { key: "focus-chain", label: "焦点链与 Tab 顺序" },
      { key: "property-store", label: "属性值、失效标记与变更通知" },
      { key: "clipping-hierarchy", label: "剪裁层级与变换树一致性" },
      { key: "a11y-semantics", label: "语义树与读屏导航语义" },
      { key: "virtualization-window", label: "可视窗口与回收池" },
      // 仲裁位的属主登记：写入路径本身归属性引擎，故仲裁位在 VE-N 名下；
      // 「仲裁」指的是「非属主域要写必须走管线并走 ADR」，不是「无人负责」。
      { key: "property-write", label: "属性写入路径（仲裁位：非属主域须经四段管线并走 ADR）" },
      { key: "transform-write", label: "变换写入路径（仲裁位：非属主域须经变换树并走 ADR）" },
    ],
    mustNot: [
      "调用绘制/着色器/合成接口（归 D 域）",
      "对属性做动画插值求值（归 M 域）",
      "实现业务控件库与产品语义（归 VE-N-UI 上层）",
      "绕过属性引擎直写控件内部字段",
    ],
  },
  {
    id: "VE-D",
    name: "D 域 · 2D 合成与渲染",
    itemRange: { lo: 601, hi: 800 },
    anchor: "VE-D 域开工（2D 合成引擎，F0601-F0800）",
    owns: [
      { key: "draw-execution", label: "绘制命令执行与混合" },
      { key: "shader-capability", label: "着色器能力与效果实现" },
      { key: "texture-and-atlas", label: "纹理与图集资源" },
      { key: "compositing", label: "合成与输出呈现" },
    ],
    mustNot: [
      "决定控件树结构（归N 域）",
      "计算布局矩形（归 N 域）",
      "决定焦点与命中顺序（归 N 域）",
      "把绘制状态缓存到结构侧当权威（结构与表现必须单向）",
    ],
  },
  {
    id: "VE-M",
    name: "M 域 · 动画系统",
    itemRange: { lo: 2401, hi: 2600 },
    anchor: "VE-F2401 M 域开工与动画架构 / VE-F2461 UI 挂载协议",
    owns: [
      { key: "animation-evaluation", label: "轨道求值、采样、混合、状态机" },
      { key: "easing-library", label: "缓动库单源" },
      { key: "animation-protocol", label: "UI 挂载协议 F2461 / 系统动画驱动协议 F2462" },
    ],
    mustNot: [
      "改变控件树结构（归 N 域）",
      "直写属性内部字段绕过四段管线（须经属性引擎，见仲裁位）",
      "为位移走布局重排（位移走变换）",
      "在系统动画路径上另造求值引擎（单源纪律）",
    ],
  },
  {
    id: "VE-N-UI",
    name: "VE-N 上层 · 业务控件库",
    itemRange: { lo: 2801, hi: 3000 },
    anchor: "VE-F2603 VE-N 边界声明（内核六类最小集 / 上层完整库分层）",
    owns: [
      { key: "business-control-library", label: "完整业务控件库与皮肤" },
      { key: "product-semantics", label: "产品语义、主题、皮肤、预设" },
      { key: "view-composition", label: "业务界面组装" },
    ],
    mustNot: [
      "改写内核三件套的契约面（须走 ADR + N 域对账钩子）",
      "绕过属性引擎直写控件内部状态",
      "自造第二套布局算法（布局归 N02）",
      "把宿主控件嵌入 VARIX 界面（归属与隔离红线，逃逸即缺陷）",
    ],
  },
];

/**
 * 能力 → 唯一属主域 的反查表（由 DOMAIN_BOUNDARY_TABLE 机检生成，不手写）。
 * 键为能力 key（不含中文label），值含属主域与该能力的人话标签——
 * 裁决报错时能同时说出「谁的能力」与「这能力是什么」，不必让读报错的人去猜。
 */
const CAPABILITY_OWNER: ReadonlyMap<string, { domain: DomainId; label: string }> = (() => {
  const m = new Map<string, { domain: DomainId; label: string }>();
  for (const d of DOMAIN_BOUNDARY_TABLE) {
    for (const c of d.owns) m.set(c.key, { domain: d.id, label: c.label });
  }
  return m;
})();

/** 查能力的人话标签；未登记返回null（调用方须自行按「未认领」处理，不得静默）。 */
export function capabilityLabel(key: string): string | null {
  return CAPABILITY_OWNER.get(key)?.label ?? null;
}

/**
 * 仲裁位：属性的**写入路径**。它不属于任何单一域，而是由属性引擎（VE-N 拥有
 * property-store）裁定「谁可以用什么方式写」的跨域协议位。
 * 锚点原文：「M04 双协议与属性系统冲突→以属性系统为准回改 M 协议
 * （ADR——接收方优先声明）」。故本表把 property-write 定为仲裁位：
 *   VE-M 可以声明「我要让这个属性动起来」，但落地必须经属性引擎四段管线；
 *   若 M 侧协议要求的写法与属性系统冲突，以属性系统为准，M 侧走 ADR 回改。
 * 写成仲裁位而不是简单归属某个域，是因为一旦归属，协议冲突时就没有裁决依据了。
 */
export const ARBITRATION_CAPABILITIES: readonly string[] = ["property-write", "transform-write"];

/** 能力申请：谁要做什么。 */
export interface CapabilityClaim {
  readonly domain: DomainId;
  readonly capability: string;
  /** 申请理由（写入诊断，便于裁决留痕）。 */
  readonly reason: string;
}

/** 裁决结果。 */
export interface BoundaryRuling {
  readonly claim: CapabilityClaim;
  readonly admitted: boolean;
  /** 属主域（非仲裁位时即裁决依据）。 */
  readonly owner: DomainId | "arbitration";
  /** 裁决说明（人话）。 */
  readonly ruling: string;
  /** 处置动作：准许/回改/走ADR。 */
  readonly action: "permit" | "rollback-to-owner" | "adr-rewrite-peer-protocol";
  /** 是否必须走 ADR（属性写入冲突必须走，逃避即架构违规）。 */
  readonly requiresAdr: boolean;
  readonly diagnostic: Diagnostic | null;
}

/**
 * 边界裁决器。四条裁决规则（顺序即优先级）：
 *   1. 能力未认领 → CAPABILITY_UNCLAIMED：有人要用但没人负责，这是架构空洞，
 *      优先于任何越界判断（连属主都没有的时候，谈不上谁越界）。
 *   2. 仲裁位（property-write / transform-write）→ 允许「声明意图」但要求走
 *      管线：非属主域（如 VE-M）申请时admitted=true 但 requiresAdr=true，
 *      处置为「以属性系统为准回改协议」；属主域（如 VE-N）申请时直接准许。
 *   3. 属主域自申请 → 准许。
 *   4. 非属主域申请 → 回改（DOMAIN_BOUNDARY_VIOLATION），并点名真正属主。
 */
export function arbitrateBoundary(claim: CapabilityClaim): BoundaryRuling {
  const owners: DomainId[] = [];
  for (const d of DOMAIN_BOUNDARY_TABLE) {
    if (d.owns.some((c) => c.key === claim.capability)) owners.push(d.id);
  }
  const ownerLabel = capabilityLabel(claim.capability) ?? claim.capability;

  if (owners.length === 0) {
    const d: Diagnostic = {
      code: ARBITRATION_CAPABILITIES.includes(claim.capability)
        ? "CAPABILITY_OWNER_CONFLICT"
        : "CAPABILITY_UNCLAIMED",
      message: `能力「${claim.capability}」在四域边界表中无人认领（申请方：${claim.domain}）；`
        + `架构空洞的特征是「谁都能做、谁都不负责」`,
      hint: ARBITRATION_CAPABILITIES.includes(claim.capability)
        ? `「${claim.capability}」是仲裁位，须由属主域在 DOMAIN_BOUNDARY_TABLE 的 owns 中显式登记仲裁规则，否则仲裁无依据`
        : `请在四域边界表中为「${claim.capability}」指定唯一属主域并补齐 mustNot；`
          + `若它本属某个域的能力却漏登记，越界将无法被拦截`,
    };
    return {
      claim,
      admitted: false,
      owner: "arbitration",
      ruling: `能力「${claim.capability}」无属主，申请不予受理。`,
      action: "rollback-to-owner",
      requiresAdr: false,
      diagnostic: d,
    };
  }

  if (owners.length > 1) {
    const d: Diagnostic = {
      code: "CAPABILITY_OWNER_CONFLICT",
      message: `能力「${claim.capability}」被多个域同时声明为职责：${owners.join("、")}；属主不唯一时裁决无解`,
      hint: `把「${claim.capability}」收敛到唯一属主域，其余域改列mustNot；`
        + `属主重复的根因通常是两个域各自实现了同一件事而无人裁决`,
    };
    return {
      claim,
      admitted: false,
      owner: "arbitration",
      ruling: `能力「${claim.capability}」属主不唯一（${owners.join("、")}），不予受理。`,
      action: "adr-rewrite-peer-protocol",
      requiresAdr: true,
      diagnostic: d,
    };
  }

  const owner = owners[0] as DomainId;

  // 仲裁位：非属主域可以声明动画意图，但落地必须走属性引擎管线
  if (ARBITRATION_CAPABILITIES.includes(claim.capability) && claim.domain !== owner) {
    const d: Diagnostic = {
      code: "PROTOCOL_PROPERTY_CONFLICT",
      message: `${claim.domain} 申请直接写「${claim.capability}」（${ownerLabel}），但该能力归属${owner}（属性引擎四段管线）；`
        + `按「接收方优先」原则，冲突以属性系统为准，M 侧协议须走 ADR 回改为经管线写入`,
      hint: `${claim.domain}侧把写入改为：声明属性目标（轨道→属性键）→ 经${owner} 四段管线落地；`
        + `并在 ADR 中记录「为何协议原写法与属性系统冲突、回改后的等价语义」；`
        + `直接写内部字段会让变更通知与失效标记丢失，表现为「属性变了画面不动」且只在并发改属性时偶发`,
    };
    return {
      claim,
      admitted: true,
      owner: "arbitration",
      ruling: `${claim.domain} 可声明写入意图，但落地须经${owner} 管线（${ownerLabel}）；协议写法冲突以属性系统为准，M 侧走 ADR 回改。`,
      action: "adr-rewrite-peer-protocol",
      requiresAdr: true,
      diagnostic: d,
    };
  }

  if (claim.domain === owner) {
    return {
      claim,
      admitted: true,
      owner,
      ruling: `「${claim.capability}」（${ownerLabel}）属主域 ${owner} 自申请，准许。`,
      action: "permit",
      requiresAdr: false,
      diagnostic: null,
    };
  }

  const d: Diagnostic = {
    code: "DOMAIN_BOUNDARY_VIOLATION",
    message: `${claim.domain} 越界申请「${claim.capability}」（${ownerLabel}），该能力属${owner}；`
      + `理由「${claim.reason}」不构成越界许可`,
    hint: `回改 ${claim.domain}：改为消费${owner}的产出（${owner}边界锚点：`
      + `${DOMAIN_BOUNDARY_TABLE.find((x) => x.id === owner)?.anchor ?? "见四域边界表"}）；`
      + `若确有属主划分错误，走 ADR 调整 DOMAIN_BOUNDARY_TABLE，而不是在实现里绕过`,
  };
  return {
    claim,
    admitted: false,
    owner,
    ruling: `「${claim.capability}」（${ownerLabel}）属${owner}，${claim.domain} 须回改。`,
    action: "rollback-to-owner",
    requiresAdr: false,
    diagnostic: d,
  };
}

/** 四域边界表自身的核验：能力属主唯一、owns/mustNot 齐备、域声明齐备。 */
export function verifyBoundaryTable(
  table: readonly DomainBoundary[] = DOMAIN_BOUNDARY_TABLE,
): Outcome<readonly DomainId[]> {
  const bag = new DiagBag();

  if (table.length !== 4) {
    bag.push(
      "CAPABILITY_UNCLAIMED",
      `四域边界表应为 4 行（N/D/M/VE-N-UI），当前 ${table.length} 行`,
      "四域齐备是边界裁决的前提；缺一行意味着某个参与方没有声明边界，越界无从判定",
    );
  }

  const seen = new Map<string, DomainId>();
  for (const d of table) {
    if (d.owns.length === 0) {
      bag.push(
        "CAPABILITY_UNCLAIMED",
        `域 ${d.name} 未声明任何负责能力`,
        "补齐 owns；不声明能力等于宣称什么都不管，四域边界会退化成无边界的自由区",
      );
    }
    if (d.mustNot.length === 0) {
      bag.push(
        "DOMAIN_BOUNDARY_VIOLATION",
        `域 ${d.name} 未声明任何禁做项`,
        "补齐 mustNot；没有禁做项就定义不出「越界」，裁决函数会失去判据",
      );
    }
    for (const c of d.owns) {
      if (c.key.trim().length === 0) {
        bag.push(
          "CAPABILITY_UNCLAIMED",
          `域 ${d.name} 声明了一条无key 的能力（label=「${c.label}」）`,
          "补齐 key；key 是机检键（跨域引用只用它），空 key 的能力无法参与属主唯一性裁决",
        );
        continue;
      }
      const prev = seen.get(c.key);
      if (prev !== undefined) {
        bag.push(
          "CAPABILITY_OWNER_CONFLICT",
          `能力「${c.key}」被 ${prev} 与 ${d.id} 同时声明为职责`,
          "收敛到唯一属主；属主重复时 arbitrateBoundary 会直接拒收该能力的所有申请",
        );
      } else {
        seen.set(c.key, d.id);
      }
    }
  }

  // 仲裁位必须有属主，否则 property-write 无裁决依据
  for (const cap of ARBITRATION_CAPABILITIES) {
    if (!seen.has(cap)) {
      bag.push(
        "CAPABILITY_OWNER_CONFLICT",
        `仲裁位能力「${cap}」在边界表中无属主域`,
        `把「${cap}」登记到VE-N 的 owns（属性引擎/变换树为其属主），`
          + `否则 M 域与上层的写入申请既无法许可也无法拒绝`,
      );
    }
  }

  if (bag.hasError) {
    const first = bag.all()[0];
    return fail(
      first?.code ?? "CAPABILITY_UNCLAIMED",
      first?.message ?? "四域边界表核验失败",
      first?.hint ?? "按诊断逐条修正后重跑 verifyBoundaryTable",
    );
  }
  return ok(table.map((d) => d.id), bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §5 M04 双协议接收（判据四：F2461 UI 挂载协议 + F2462 系统动画驱动协议）
// ════════════════════════════════════════════════════════════════════════════

/** M04 双协议 id。 */
export type M04ProtocolId = "ui-mount-protocol" | "system-animation-protocol";

/** 一份接收确认签名。缺任何一字段即视为未签收——半截签名比没有签名更危险。 */
export interface ProtocolSignature {
  /** 签署方（目标方＝N 域属性系统）。 */
  readonly signer: string;
  /** 签署时刻（ISO 8601；用于陈旧性判定与追溯）。 */
  readonly signedAt: string;
  /** 签收时上游协议的内容哈希。 */
  readonly contentHash: string;
  /** 确认范围：本次签收明确覆盖的字段清单（签了 A 却要 B = 范围不匹配）。 */
  readonly scope: readonly string[];
}

/** 一份 M04 协议在 N 域的接收登记。 */
export interface ProtocolEntry {
  readonly id: M04ProtocolId;
  readonly upstreamAnchor: string;
  /** 上游方（M04 组）职责一句话。 */
  readonly peerDuty: string;
  /** N 域消费的内容（具体到字段，不写「协同」这种无信息量的话）。 */
  readonly weConsume: readonly string[];
  /** N 域必须不做的内容（禁扩面）。 */
  readonly weMustNot: readonly string[];
  /** 兑现该协议的 N 域条目。 */
  readonly realizedBy: readonly string[];
  /** 上游当前内容哈希（待上游提供；未提供为 null 且显式记为待办）。 */
  readonly currentHash: string | null;
  /** 目标方接收确认签名（未签收 = null，且 null 是显式状态而非「忘了填」）。 */
  readonly signature: ProtocolSignature | null;
}

/**
 * M04 双协议接收台账。
 *
 * 初始状态说明（诚实声明，不伪装成已签收）：本条落笔时目标方签名尚未完成——
 * M04 双协议的最终内容哈希需上游提供后回填，故currentHash 与签名 contentHash
 * 均以 null / 待办形态登记，由verifyProtocolReceipt 判为「阻断项+ 待办项」。
 * 这是正确行为而不是缺陷：协议目标方在没看到上游最终内容哈希前签的字是空的。
 * 上游给出哈希后回填本表，自检随即转绿（哈希对账通过）。
 */
export const PROTOCOL_RECEIPT_LEDGER: readonly ProtocolEntry[] = [
  {
    id: "ui-mount-protocol",
    upstreamAnchor: "VE-F2461 M04 开工与 UI 动画对接（UI 挂载协议）",
    peerDuty:
      "M04 组：把 M01 六类轨道挂载到 UI 控件属性，声明控件属性→轨道类型的映射与绑定路径语法",
    weConsume: [
      "属性映射表（控件属性→轨道类型：位置→位置轨/透明度→浮点轨/颜色→颜色轨/尺寸→尺寸轨）",
      "控件属性路径绑定语法（F2402 bind_path 的 UI 维表达式）",
      "UI 动画求值耗时预算（默认 1ms，与场景动画预算隔离）",
      "轨道失效时的目标失效回调约定（控件销毁→绑定失效须显性告警）",
    ],
    weMustNot: [
      "让M04 直写属性内部字段（写入一律经属性引擎四段管线，见仲裁位 property-write）",
      "在 N 域内复制一份轨道求值实现（求值单源归 M 域）",
      "把绑定路径失效当静默空转（须显性告警，指名失效路径）",
    ],
    realizedBy: ["VE-F2604 控件属性系统（M04 挂载目标与属性侧兑现）", "VE-F2605 绑定路径解析"],
    currentHash: null,
    signature: null,
  },
  {
    id: "system-animation-protocol",
    upstreamAnchor: "VE-F2462 系统动画驱动协议（窗口/菜单/对话框/任务栏/通知五类）",
    peerDuty:
      "M04 组：声明五类系统动画（窗口过渡/菜单动效/对话框/任务栏/通知）的驱动协议，统一走M01 求值引擎与 F2421 缓动单源",
    weConsume: [
      "五类系统动画的轨道类型与默认缓动引用（F2421 缓动枚举单源）",
      "系统动画时长规范（过渡 200-300ms 节奏纪律）",
      "宿主边界声明（VARIX 系统内动画归 M 域；宿主 Windows 自身动画不归 M 域）",
      "动画预算超限时的降级路径（UI 响应优先：降级而非冻结界面）",
    ],
    weMustNot: [
      "为系统动画另造求值引擎（单源纪律：同引擎同缓动）",
      "让系统动画控件逃逸到宿主窗口（归属与隔离红线，逃逸即缺陷）",
      "把位移类系统动画落到布局重排上（位移走变换，见 F2624 纪律）",
    ],
    realizedBy: ["VE-F2604 属性系统（系统动画属性挂载）", "VE-F2624 布局动画变换优先纪律"],
    currentHash: null,
    signature: null,
  },
];

/** 接收核验报告。 */
export interface ProtocolReport {
  /** 是否允许开工（双协议全部签收且范围覆盖消费字段）。 */
  readonly mayStart: boolean;
  readonly rows: readonly ProtocolRow[];
  /** 阻断点名（缺签/范围不符）。 */
  readonly blockers: readonly Diagnostic[];
  /** 非阻断待办（上游内容哈希未提供）。 */
  readonly pending: readonly Diagnostic[];
}

/** 单份协议的核验行。 */
export interface ProtocolRow {
  readonly id: M04ProtocolId;
  readonly status: "signed" | "unsigned" | "scope-mismatch";
  readonly signer: string;
  readonly signedAt: string;
  readonly uncoveredFields: readonly string[];
  /** 哈希对账状态：matched=对账通过 / pending=上游未提供（非阻断待办）/ drifted=已漂移（阻断，须重签）。 */
  readonly hashStatus: "matched" | "pending" | "drifted";
}

/**
 * 双协议接收核验（判据四的主函数）：逐份核验签名完整性、范围覆盖与哈希对账。
 *
 * 四种状态各自独立可检索，不合并：
 *   unsigned       —— 没有签名（或签名字段有缺）→ 开工阻断，点名需签方；
 *   scope-mismatch —— 签名范围没覆盖 N 域要消费的字段 → 阻断（签了 A 却要 B）；
 *   hash: pending  —— 上游内容哈希未提供 → **非阻断待办**，但必须显式列出下一步，
 *                     不允许「先放过以后再说」——那正是协议漂移的入口；
 *   hash: drifted  —— 签收哈希 ≠ 上游当前哈希 → **阻断**（上游改过未通知，必须重签）。
 * pending 与 drifted 一个用 PROTOCOL_HASH_PENDING（待办栏）、一个用
 * PROTOCOL_HASH_DRIFT（阻断栏），处置方向相反，不允许共用一个码。
 */
export function verifyProtocolReceipt(
  ledger: readonly ProtocolEntry[] = PROTOCOL_RECEIPT_LEDGER,
): ProtocolReport {
  const blockers: Diagnostic[] = [];
  const pending: Diagnostic[] = [];
  const rows: ProtocolRow[] = [];

  for (const entry of ledger) {
    const sig = entry.signature;

    if (sig === null) {
      blockers.push({
        code: "PROTOCOL_UNSIGNED",
        message: `M04 协议「${entry.id}」未签收：${entry.upstreamAnchor} 尚无目标方（N 域属性系统）接收确认签名，`
          + `N 域不得在缺签状态下开工`,
        hint: `由 N 域属性系统作为协议目标方签收并回填 signature（签署方/时刻/内容哈希/确认范围四项齐备）；`
          + `签收范围须逐字段覆盖 weConsume 清单；缺签状态下开工的典型后果是`
          + `「动画改了属性但界面无反应，且没有任何告警」`,
      });
      if (entry.currentHash === null) {
        pending.push({
          code: "PROTOCOL_HASH_PENDING",
          message: `M04 协议「${entry.id}」的上游内容哈希尚未提供（currentHash 为 null），哈希对账无法完成`,
          hint: `请 M04 组提供 ${entry.upstreamAnchor} 的内容哈希并回填 PROTOCOL_RECEIPT_LEDGER；`
            + `哈希的作用是「上游改过协议时下游能发现」——没有哈希，协议漂移将完全无声`,
        });
      }
      rows.push({
        id: entry.id,
        status: "unsigned",
        signer: "",
        signedAt: "",
        uncoveredFields: [],
        hashStatus: entry.currentHash === null ? "pending" : "matched",
      });
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
        code: "PROTOCOL_UNSIGNED",
        message: `M04 协议「${entry.id}」的签名字段不完整，缺：${missing}；半截签名比无签名更危险，判为未签收`,
        hint: `补齐 ${missing}；签收三要素缺任一即不得开工`,
      });
      rows.push({
        id: entry.id,
        status: "unsigned",
        signer: sig.signer,
        signedAt: sig.signedAt,
        uncoveredFields: [],
        hashStatus: "pending",
      });
      continue;
    }

    if (entry.currentHash === null) {
      pending.push({
        code: "PROTOCOL_HASH_PENDING",
        message: `M04 协议「${entry.id}」已签收但上游内容哈希缺失，无法完成哈希对账`,
        hint: `请 M04 组回填 ${entry.upstreamAnchor} 的 currentHash；`
          + `对账通过前协议内容仍可能被上游改动而本域不知情`,
      });
    } else if (sig.contentHash !== entry.currentHash) {
      blockers.push({
        code: "PROTOCOL_HASH_DRIFT",
        message: `M04 协议「${entry.id}」内容哈希漂移：签收时为 ${sig.contentHash}，上游当前为 ${entry.currentHash}；`
          + `说明上游在签收后改过协议而 N 域未收到通知`,
        hint: `请 M04 组确认变更内容；若变更影响 N 域消费字段，须重新签收并更新 scope；`
          + `不得沿用旧签名继续开工（这是最典型的「上游改了、下游不知情」事故）`,
      });
    }

    const uncovered = entry.weConsume.filter((field) => !sig.scope.includes(field));
    if (uncovered.length > 0) {
      blockers.push({
        code: "PROTOCOL_SCOPE_MISMATCH",
        message: `M04 协议「${entry.id}」的签收范围不覆盖 N 域将消费的字段：${uncovered.join("、")}`,
        hint: `请把上述字段加入签收范围后重签；「签了 A 却要 B」在集成期表现为`
          + `N 域读到上游未承诺的字段，行为不可预期且无人负责`,
      });
      rows.push({
        id: entry.id,
        status: "scope-mismatch",
        signer: sig.signer,
        signedAt: sig.signedAt,
        uncoveredFields: uncovered,
        hashStatus: entry.currentHash === null ? "pending" : "matched",
      });
      continue;
    }

    rows.push({
      id: entry.id,
      status: "signed",
      signer: sig.signer,
      signedAt: sig.signedAt,
      uncoveredFields: [],
      // drifted 与 pending 必须分开：drifted 是阻断（上游改过），pending 只是待办
      // （上游还没给哈希）。合成一个值会让「必须重签」被读成「可以先干活」。
      hashStatus:
        entry.currentHash === null
          ? "pending"
          : sig.contentHash === entry.currentHash
            ? "matched"
            : "drifted",
    });
  }

  return { mayStart: blockers.length === 0, rows, blockers, pending };
}

/**
 * 生成目标方签收签名（供 N02+ 在上游哈希到位后调用）。
 * 显式做成函数而不是让调用方手写对象：签收四要素的顺序与完整性由一处保证，
 * 避免「有人只填了 signer 和 signedAt」这种半截签名。
 */
export function makeCountersign(
  entry: ProtocolEntry,
  signer: string,
  signedAt: string,
  contentHash: string,
  scope?: readonly string[],
): ProtocolEntry {
  return {
    ...entry,
    signature: {
      signer,
      signedAt,
      contentHash,
      scope: scope ?? entry.weConsume.slice(),
    },
  };
}

// ════════════════════════════════════════════════════════════════════════════
// §6 四域契约链移交接收（K→L→M→N，判据一的跨批对接点）
// ════════════════════════════════════════════════════════════════════════════

/** 契约链上的一环。 */
export interface ChainLink {
  /** 上游域。 */
  readonly from: string;
  /** 下游域。 */
  readonly to: string;
  /** 该环的移交条目（收官宣告）。 */
  readonly anchor: string;
  /** 移交内容摘要（具体到契约名，不写「整体移交」）。 */
  readonly carries: readonly string[];
  /** 是否已生效（收官宣告已发布）。 */
  readonly effective: boolean;
}

/**
 * K→L→M→N 四域契约链。
 *
 * 为什么把链条完整写出而不是只查 M→N 一环：UI 内核的每一条边界都站在前序域的
 * 收官宣告上（D 域的绘制能力、M 域的动画协议、VE-E 的文字测量）。
 * 中间任一环未生效而N 域开工，等于在别人的未完工声明上盖楼——
 * 而这类事故在集成期表现为「对方改了地基，我这边静默跑偏」。
 */
export const HANDOVER_CHAIN_LEDGER: readonly ChainLink[] = [
  {
    from: "VE-C",
    to: "VE-D",
    anchor: "VE-F0599 C 域移交包（着色器能力清单供 D 域效果实现）",
    carries: ["shader-capability-list"],
    effective: true,
  },
  {
    from: "VE-D",
    to: "VE-N",
    anchor: "VE-D 域开工 F0601-F0800（2D 合成引擎）",
    carries: ["draw-execution能力边界", "compositing输出契约"],
    effective: true,
  },
  {
    from: "VE-E",
    to: "VE-N",
    anchor: "VE-E文字引擎 F0801 契约（文本测量由E 产出，N 消费）",
    carries: ["text-measure-contract（断行/整形/字形归 E）"],
    effective: true,
  },
  {
    from: "VE-K",
    to: "VE-L",
    anchor: "VE-K F2200 后处理链域收官宣告",
    carries: ["render-output-contract"],
    effective: true,
  },
  {
    from: "VE-L",
    to: "VE-M",
    anchor: "VE-L F2400 物理域收官宣告（物理→动画双向衔接契约）",
    carries: ["physics-animation-bridge-contract"],
    effective: true,
  },
  {
    from: "VE-M",
    to: "VE-N",
    anchor: "VE-F2600 M 域收官宣告（四域契约链末环：动画→UI 内核）",
    carries: [
      "F2461 UI 挂载协议（属性映射与绑定路径语法）",
      "F2462 系统动画驱动协议（五类系统动画驱动）",
      "F2421 缓动单源声明（UI 动效与场景动画同一套缓动语言）",
      "F2401 三域边界表（M 管动画 / N 管结构的边界正名）",
    ],
    effective: true,
  },
];

/**
 * 上游前置核验：链条完整性 + 末环生效。
 *
 * 口径说明（诚实标注锚点文本的继承瑕疵）：锚点 F2601 的错误路径矩阵写的是
 * 「K 域未收官（F2400 已生效——通过）」——这里的 F2400 是 M 域开工条目
 * F2401 的上游门槛，属 F2401 模板的继承文字；在 N 域这里，
 * 真正的前置是 **F2600 M 域收官宣告**（锚点跨批对接点明写「接收 F2600 移交
 * （K→L→M→N 四域契约链）」）。本条按F2600 核验，并把该继承瑕疵登记为
 * 口径说明而非默默按F2400 放行——默默放行会让「上游未就绪」永远查不出来。
 */
export const UPSTREAM_GATE_NOTE =
  "锚点 F2601 错误路径中的「K 域未收官（F2400 已生效）」系 F2401 模板继承文字；"
  + "N 域真实上游门槛为 F2600 M 域收官宣告（锚点跨批对接点明写 K→L→M→N 四域契约链），本条按 F2600 核验。";

/** 上游前置核验报告。 */
export interface UpstreamReport {
  readonly ready: boolean;
  readonly rows: readonly { readonly link: string; readonly effective: boolean }[];
  readonly diagnostics: readonly Diagnostic[];
}

/** 上游前置核验：逐环检查是否生效，任一环断链即阻断并点名该环。 */
export function verifyUpstream(
  chain: readonly ChainLink[] = HANDOVER_CHAIN_LEDGER,
): UpstreamReport {
  const bag = new DiagBag();
  const rows: { link: string; effective: boolean }[] = [];

  const terminal = chain.filter((l) => l.to === "VE-N");
  if (terminal.length === 0) {
    bag.push(
      "HANDOVER_CHAIN_BROKEN",
      "契约链中没有以 VE-N 为下游的移交环，N 域上游前置无从核验",
      "补齐 M→N 移交环（锚点：VE-F2600 M 域收官宣告），否则「上游未就绪」永远查不出来",
    );
  }

  for (const link of chain) {
    rows.push({ link: `${link.from} → ${link.to}（${link.anchor}）`, effective: link.effective });
    if (!link.effective) {
      bag.push(
        link.to === "VE-N" ? "UPSTREAM_NOT_EFFECTIVE" : "HANDOVER_CHAIN_BROKEN",
        `契约链断点：${link.from} → ${link.to}（${link.anchor}）尚未生效；`
          + `N 域开工所依赖的前序域未收官，等于在未完工的声明上盖楼`,
        link.to === "VE-N"
          ? `请 ${link.from} 域完成收官宣告并把 effective 置true 后重跑 verifyUpstream；`
            + `前置未生效时开工的典型后果是「对方改了地基、本域静默跑偏」`
          : `请先推动 ${link.from} → ${link.to} 环生效，再核验 N 域前置`,
      );
    }
  }

  return { ready: !bag.hasError, rows, diagnostics: bag.all() };
}

// ════════════════════════════════════════════════════════════════════════════
// §7 N 域组内分工总览（判据五的落地：20 条分工行）
// ════════════════════════════════════════════════════════════════════════════

/** 分工行：组 id + 席位 + 职责 + 交付契约名 + 门禁判据。 */
export interface WorkforceRow {
  readonly group: GroupId;
  /** 席位：架构席（声明与边界）or 实现席（算法与门禁）。 */
  readonly seat: "architecture" | "implementation";
  readonly duty: string;
  /** 本席位交付的契约名（下游消费的具体数据结构）。 */
  readonly deliverable: string;
  /** 门禁判据：本席位完成的可机检判据。 */
  readonly gate: string;
}

/**
 * N 域十组 × 两席 = 20 条分工行。
 *
 * 为什么每组配架构席与实现席两行（与 K 域同一纪律）：UI 内核最容易出的事故是
 * 「架构声明与执行实现各写各的」——架构说失效范围是子树，执行时整树重建；
 * 架构说属性写入必走四段管线，M 侧轨道直接写内部字段。把两席绑在同一行、
 * 共享同一条门禁，就把「声明与实现必须对账」变成排产结构上的必然。
 */
export const WORKFORCE_TABLE: readonly WorkforceRow[] = [
  {
    group: "N01",
    seat: "architecture",
    duty: "UI 内核总架构：十主题注册、三大件契约、四域边界表、M04 双协议接收（本条）",
    deliverable: "UiKernelDomainArchitecture",
    gate: "十主题号段口径显式登记；三大件依赖无环；四域能力属主唯一；双协议接收核验可跑",
  },
  {
    group: "N01",
    seat: "implementation",
    duty: "控件树三操作与三不变量断言、树度量、双树映射完整性校验",
    deliverable: "ControlTreeOps",
    gate: "单亲/无环/序稳定三断言在 CI 常开；批量操作单事务回滚；万节点树操作 P95 入 F2610",
  },
  {
    group: "N02",
    seat: "architecture",
    duty: "布局架构：measure→arrange 两阶段模型、遍历方向、失效增量驱动、与 N01 契约接收",
    deliverable: "LayoutArchitecture",
    gate: "两阶段序断言（先测量后排列）；失效传播无环；N01 五件树契约哈希对账通过",
  },
  {
    group: "N02",
    seat: "implementation",
    duty: "五类布局容器算法、文本测量缓存、变换优先纪律守卫",
    deliverable: "LayoutContainers",
    gate: "五容器对拍一致；文本测量缓存命中率入册；动画位移零布局重排（纪律断言）",
  },
  {
    group: "N03",
    seat: "architecture",
    duty: "命中架构：命中管线四步、三命中模式与豁免、事件路由捕获冒泡、手势竞争消解",
    deliverable: "HitTestArchitecture",
    gate: "命中与焦点共用同一逆序遍历（Tab 序与点击目标必须一致）；滚轮→滚动链路由唯一",
  },
  {
    group: "N03",
    seat: "implementation",
    duty: "命中四步实现、命中规则表、命中调试三负载与无障碍命中一致性联动",
    deliverable: "HitTestPipeline",
    gate: "命中确定性双跑逐位一致；命中区域与布局矩形一致（无第二套坐标系）",
  },
  {
    group: "N04",
    seat: "architecture",
    duty: "滚动架构：滚动容器语义、滚动物理、嵌套滚动链仲裁、滚动与虚拟化的窗位契约",
    deliverable: "ScrollArchitecture",
    gate: "嵌套滚动链仲裁规则唯一（不得各层各自滚）；滚动物理确定性双跑一致",
  },
  {
    group: "N04",
    seat: "implementation",
    duty: "滚动容器与滚动条控件、吸附对齐、滚动手势执行",
    deliverable: "ScrollContainers",
    gate: "边界回弹不过冲；滚动位置变更只经属性引擎（无旁路直写）",
  },
  {
    group: "N05",
    seat: "architecture",
    duty: "焦点与数据绑定架构：焦点链与 Tab 顺序语义、绑定路径解析与失效告警、值源优先级出口",
    deliverable: "FocusAndBindingArchitecture",
    gate: "焦点环恒可见（无障碍红线）；绑定路径失效必显性告警；值源优先级单源",
  },
  {
    group: "N05",
    seat: "implementation",
    duty: "焦点链维护与可见性保障、属性→属性绑定求值、失效传播",
    deliverable: "FocusChainAndBindings",
    gate: "键盘与鼠标能力对等（同一可达集）；绑定环检测拒绝；焦点不可见项计入诊断",
  },
  {
    group: "N06",
    seat: "architecture",
    duty: "虚拟化架构：可视窗口算法、回收池契约、滚动/焦点/语义三方联动契约",
    deliverable: "VirtualizationArchitecture",
    gate: "窗口 O(可视项)；焦点项必在窗口内否则自动滚入；回收不丢用户已输入内容",
  },
  {
    group: "N06",
    seat: "implementation",
    duty: "虚拟化列表核心、回收池、分组与网格树形扩展",
    deliverable: "VirtualizedListCore",
    gate: "万项列表内存有界；回收池无泄漏（连续 N 帧未回收即告警）；基准入 F2710",
  },
  {
    group: "N07",
    seat: "architecture",
    duty: "剪裁架构：剪裁树与变换树层级一致性、嵌套剪裁合成规则、与 N02 布局的分工",
    deliverable: "ClippingArchitecture",
    gate: "层级一致性断言拦截「子节点绕过父剪裁」；变换优先纪律在剪裁侧兑现",
  },
  {
    group: "N07",
    seat: "implementation",
    duty: "剪裁树构建与增量维护、变换矩阵合成、剪裁调试三负载",
    deliverable: "ClippingTree",
    gate: "嵌套剪裁合成无误差；D 域消费剪裁矩形与布局矩形一一对应",
  },
  {
    group: "N08",
    seat: "architecture",
    duty: "无障碍树架构：语义节点模型、语义树增量维护、读屏导航语义、与虚拟化/剪裁联动",
    deliverable: "A11yTreeArchitecture",
    gate: "语义树与可视树单向派生（禁止双源）；读屏可达集与视觉可达集一致",
  },
  {
    group: "N08",
    seat: "implementation",
    duty: "语义树增量维护、读屏导航、语义节点调试数据与全域无障碍核验",
    deliverable: "SemanticTree",
    gate: "语义变更 O(变更)；无障碍四线（减弱/前庭/可暂停/时间）核验 100%",
  },
  {
    group: "N09",
    seat: "architecture",
    duty: "N 域预备总览：九组收口核验、三线架构（状态/质量/确认）、十主题映射唯一性",
    deliverable: "NDomainPreflightPlan",
    gate: "十主题→组映射唯一性断言通过；三线交付物清单齐备；缺失项有登记不装作齐备",
  },
  {
    group: "N09",
    seat: "implementation",
    duty: "域内互操作两两对账、基准总册、缺陷清账、缺失项补齐执行",
    deliverable: "NDomainPreflightEvidence",
    gate: "两两对账零重复；长稳与 fuzz 总闸全绿；清账无P0/P1 遗留",
  },
  {
    group: "N10",
    seat: "architecture",
    duty: "N 域收口：接口总账与查重对账、交互词典、文档汇总、20 维度自查与收官宣告",
    deliverable: "NDomainCloseoutDeclaration",
    gate: "接口总账无重名；十主题完成终检签名；20 维度无低于标准项；确定性 ≥90、无障碍 ≥90",
  },
  {
    group: "N10",
    seat: "implementation",
    duty: "N↔D 渲染契约签署收口、遗留清单入域归档、架构冻结文档与移交包",
    deliverable: "NRenderContractSignoff",
    gate: "渲染对接契约双签；遗留全部为🟡 或 ADR 态；收官后本域转只读维护",
  },
];

// ════════════════════════════════════════════════════════════════════════════
// §8 隐私面声明与无障碍替述（锚点：无障碍与隐私）
// ════════════════════════════════════════════════════════════════════════════

/** 隐私面声明。本域为纯契约层，运行时零隐私面；出现任何用户数据面即架构违规。 */
export interface PrivacyManifest {
  readonly runtimePrivacySurface: "none";
  readonly collectsUserContent: false;
  readonly rationale: string;
}

/** 隐私面声明（唯一事实源）。 */
export const N_PRIVACY_MANIFEST: PrivacyManifest = {
  runtimePrivacySurface: "none",
  collectsUserContent: false,
  rationale:
    "本条只产出契约与校验结果，不读取用户数据、不写盘、不发网络请求；"
    + "架构层引入隐私面会让「UI 内核到底收集了什么」这个问题在后期无法回答，故显式声明为零。"
    + "需要用户数据的语义树（F2742）与调试负载（F2608）各自单独评审隐私面，不在架构层承担",
};

/**
 * 无障碍替述：为架构声明生成**纯文本**可读描述（图必配文的机读形态）。
 * 替述须覆盖：域定位与四域边界、十主题分组、当前阻断项、下一步怎么办。
 * 返回字符串数组而非富文本，便于屏幕阅读器逐段朗读，也便于文档系统直接引用。
 */
export function accessibleArchitectureNarrative(
  protocolReport: ProtocolReport = verifyProtocolReceipt(),
  upstreamReport: UpstreamReport = verifyUpstream(),
): readonly string[] {
  const out: string[] = [];
  out.push(
    `N 域是 VARIX 全部界面的结构基座，功能号 F2601 到 F2800，共 200 项。`
      + `它负责结构与行为：控件树、测量布局、命中测试、滚动、焦点、属性、剪裁、语义树与虚拟化窗口。`,
  );
  out.push(
    `四域分工是：N 域管结构与行为；D 域管渲染绘制；M 域管动画驱动；上层业务控件库管产品控件。`
      + `举例说明这条边界：控件移动一格，N 域只改属性里的位置值并标记失效，`
      + `由 D 域决定怎么画，由 M 域决定怎么动过去——三者不互相代替。`,
  );
  const exclusive = THEME_IDS.filter((t) => N_THEME_REGISTRY[t].rangeKind === "exclusive");
  const cross = THEME_IDS.filter((t) => N_THEME_REGISTRY[t].rangeKind === "crosscutting");
  out.push(
    `N 域官方十主题：九个独占主题分别是${exclusive.map((t) => themeLabel(t)).join("、")}，`
      + `覆盖 F2601 到 F2760；第${cross.length === 1 ? "十" : String(cross.length)}个主题「${cross.map((t) => themeLabel(t)).join("、")}」`
      + `是横切主题，不占独立号段，兑现于其他组的条目之内；`
      + `F2761 到 F2800 是预备与收口保留段。`,
  );
  for (const id of THEME_IDS) {
    const spec = N_THEME_REGISTRY[id];
    const rangeText =
      spec.itemRange === null
        ? "横切主题，无独立功能号段"
        : `功能号 F${spec.itemRange.lo} 到 F${spec.itemRange.hi}`;
    out.push(
      `主题 ${spec.name}：${rangeText}，由 ${spec.realizedBy.join("、")} 组兑现。${spec.duty}。`
        + `消费方：${spec.consumedBy.join("；")}。`,
    );
  }
  out.push(
    `架构三大件是控件树、属性引擎、增量更新。依赖方向固定为：树是宿主，属性挂在树上，`
      + `增量为树与属性的失效消费方；反向依赖会被自检拦下。`,
  );
  if (upstreamReport.ready) {
    out.push("上游四域契约链已全部生效，M 域收官宣告已发布，N 域上游前置条件满足。");
  } else {
    out.push(`上游契约链存在 ${upstreamReport.diagnostics.length} 项未生效，问题如下：`);
    for (const d of upstreamReport.diagnostics) {
      out.push(`${d.message} 下一步：${d.hint}`);
    }
  }
  if (protocolReport.mayStart) {
    out.push("M04 双协议已签收且范围覆盖消费字段，N 域开工条件已满足。");
  } else {
    out.push(`当前 N 域开工被阻断，共 ${protocolReport.blockers.length} 项待处理：`);
    for (const r of protocolReport.rows) {
      if (r.status === "signed") continue;
      out.push(`协议 ${r.id} 的状态是 ${r.status}；下一步：补齐签名或扩大签收范围后重跑核验。`);
    }
  }
  if (protocolReport.pending.length > 0) {
    out.push(`另有 ${protocolReport.pending.length} 项非阻断待办（协议内容哈希待上游提供）：`);
    for (const d of protocolReport.pending) {
      out.push(`${d.message} 下一步：${d.hint}`);
    }
  }
  out.push("本条为架构契约层，不收集任何用户内容，无运行时隐私面。");
  return out;
}

// ════════════════════════════════════════════════════════════════════════════
// §9 域级自检（把判据变成一条可执行的门禁）
// ════════════════════════════════════════════════════════════════════════════

/** 域开工核验报告。 */
export interface DomainCheckReport {
  /** 是否允许开工（上游前置 + 双协议签收 + 自检无阻断）。 */
  readonly mayStart: boolean;
  /** 自检产生的全部诊断（含阻断与非致命待办）。 */
  readonly diagnostics: readonly Diagnostic[];
  /** 独占主题覆盖的功能号数（应为 160：F2601~F2760）。 */
  readonly coveredItems: number;
  /** 收口保留段功能号数（应为 40：F2761~F2800）。 */
  readonly reservedItems: number;
}

/**
 * 域级自检（开工门禁）：一次跑完判据的全部机检项。
 *
 * 机检项清单（逐条对应判据）：
 *   ① 十主题封闭集完整、九个独占主题号段无缝覆盖 F2601~F2760、收口保留段完整、
 *      横切主题未被误派独占号段（判据一）；
 *   ② 三大件依赖无环/无反向/能力面齐备（判据二）；
 *   ③ 四域边界表能力属主唯一、owns/mustNot 齐备、仲裁位有属主（判据三）；
 *   ④ M04 双协议接收核验（判据四）；
 *   ⑤ 上游四域契约链生效（跨批对接点）；
 *   ⑥ 组内分工表 20 行且交付契约名唯一（判据五）。
 */
export function selfCheckDomain(): DomainCheckReport {
  const bag = new DiagBag();

  // ① 主题完整性与号段
  if (THEME_IDS.length !== 10) {
    bag.push(
      "THEME_UNREGISTERED",
      `N 域应登记 10 个官方主题，当前 ${THEME_IDS.length} 个`,
      `补齐主题至 10 个；官方十主题为控件树/测量布局/命中测试/滚动/焦点/数据绑定/虚拟化列表/剪裁/无障碍布局/渲染对接`,
    );
  }

  const exclusives = THEME_IDS.filter((t) => N_THEME_REGISTRY[t].rangeKind === "exclusive")
    .slice()
    .sort((a, b) => {
      const ra = N_THEME_REGISTRY[a].itemRange;
      const rb = N_THEME_REGISTRY[b].itemRange;
      return (ra?.lo ?? 0) - (rb?.lo ?? 0);
    });

  let expectedLo = 2601;
  let covered = 0;
  for (const id of exclusives) {
    const spec = N_THEME_REGISTRY[id];
    const range = spec.itemRange;
    if (range === null) {
      bag.push(
        "THEME_SPEC_INCONSISTENT",
        `主题 ${spec.name} 声明为独占主题却无号段`,
        "补齐 itemRange；独占主题必须占号段，否则排产无处落账",
      );
      continue;
    }
    if (range.lo !== expectedLo) {
      if (range.lo < expectedLo) {
        bag.push(
          "ITEM_RANGE_OVERLAP",
          `主题 ${spec.name} 的号段起点 F${range.lo} 与前序主题重叠（期望 F${expectedLo}）`,
          `重排号段使各主题不重叠；号段重叠会让功能号失去唯一归属，追溯时无从下手。${THEME_RANGE_NOTE}`,
        );
      } else {
        bag.push(
          "ITEM_RANGE_GAP",
          `主题 ${spec.name} 的号段起点 F${range.lo} 与前序主题之间存在空洞 F${expectedLo}~F${range.lo - 1}`,
          `补齐空洞或调整前后主题号段；空洞意味着有功能号无人认领，排产与验收都会漏。${THEME_RANGE_NOTE}`,
        );
      }
    }
    if (range.hi < range.lo) {
      bag.push(
        "ITEM_RANGE_OVERLAP",
        `主题 ${spec.name} 的号段倒置：F${range.lo} > F${range.hi}`,
        "修正号段为 lo ≤ hi",
      );
    } else {
      covered += range.hi - range.lo + 1;
    }
    expectedLo = range.hi + 1;
  }
  if (expectedLo !== CLOSEOUT_RESERVE.lo) {
    bag.push(
      "RESERVE_RANGE_BROKEN",
      `独占主题号段终点为 F${expectedLo - 1}，收口保留段应从 F${CLOSEOUT_RESERVE.lo} 开始（差 ${CLOSEOUT_RESERVE.lo - expectedLo} 个号位）`,
      `对齐号段边界；${THEME_RANGE_NOTE}`,
    );
  }

  // 保留段完整性
  let reserved = 0;
  for (const seg of CLOSEOUT_RESERVE.split) {
    if (seg.hi < seg.lo) {
      bag.push(
        "RESERVE_RANGE_BROKEN",
        `保留段 ${seg.group} 号段倒置：F${seg.lo} > F${seg.hi}`,
        "修正保留段为 lo ≤ hi",
      );
      continue;
    }
    const width = seg.hi - seg.lo + 1;
    if (width !== 20) {
      bag.push(
        "RESERVE_RANGE_BROKEN",
        `保留段 ${seg.group} 应为 20 项（F${seg.lo}~F${seg.hi}），实际 ${width} 项`,
        "N09 预备与 N10 收口各 20 项；预留段宽度变化须同步调整主题号段并走 ADR",
      );
    }
    reserved += width;
  }
  if (CLOSEOUT_RESERVE.hi !== 2800) {
    bag.push(
      "RESERVE_RANGE_BROKEN",
      `保留段终点为 F${CLOSEOUT_RESERVE.hi}，N 域应覆盖至 F2800`,
      "补齐尾部号段；N 域共 200 项（F2601~F2800），少一即域不完整",
    );
  }

  // 横切主题不得有独占号段
  for (const id of THEME_IDS) {
    const spec = N_THEME_REGISTRY[id];
    if (spec.rangeKind === "crosscutting" && spec.itemRange !== null) {
      bag.push(
        "CROSSCUTTING_THEME_ASSIGNED",
        `横切主题 ${spec.name} 被赋予了独占号段 F${spec.itemRange.lo}~F${spec.itemRange.hi}`,
        `移除该号段；横切主题兑现于${spec.realizedBy.join("、")} 组的条目之内，`
          + `再给号段会与那些条目重复计工，导致验收时同一份产出被数两次`,
      );
    }
    if (spec.rangeKind === "exclusive" && spec.itemRange === null) {
      bag.push(
        "THEME_SPEC_INCONSISTENT",
        `独占主题 ${spec.name} 未分配号段`,
        "补齐 itemRange；或若它其实是横切主题，把 rangeKind 改为 crosscutting",
      );
    }
    if (spec.consumedBy.length === 0) {
      bag.push(
        "THEME_SPEC_INCONSISTENT",
        `主题 ${spec.name} 未声明下游消费方`,
        "补齐 consumedBy；没有消费方的主题意味着产出无人使用，应从排产中撤掉而不是留着占号",
      );
    }
    if (spec.mustNot.length === 0) {
      bag.push(
        "THEME_SPEC_INCONSISTENT",
        `主题 ${spec.name} 未声明禁做项`,
        "补齐 mustNot；没有禁做项就定义不出越界，四域边界裁决会失去该主题的判据",
      );
    }
  }

  // ② 三大件
  const pillars = verifyPillars();
  if (!pillars.ok) bag.push(pillars.code, pillars.message, pillars.hint);
  else bag.pushAll(pillars.diagnostics);

  // ③ 四域边界表
  const boundary = verifyBoundaryTable();
  if (!boundary.ok) bag.push(boundary.code, boundary.message, boundary.hint);
  else bag.pushAll(boundary.diagnostics);

  // ④ M04 双协议接收
  const protocol = verifyProtocolReceipt();
  for (const b of protocol.blockers) bag.push(b.code, b.message, b.hint);
  for (const p of protocol.pending) bag.push(p.code, p.message, p.hint);

  // ⑤ 上游契约链
  const upstream = verifyUpstream();
  for (const d of upstream.diagnostics) bag.push(d.code, d.message, d.hint);

  // ⑥ 分工表行数与契约名唯一
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

  // ⑦ 隐私面
  if (N_PRIVACY_MANIFEST.runtimePrivacySurface !== "none" || N_PRIVACY_MANIFEST.collectsUserContent) {
    bag.push(
      "PRIVACY_SURFACE_DECLARED",
      "N 域架构层声明了运行时隐私面，与「纯契约层零隐私面」的域级立场冲突",
      "把隐私面下沉到具体条目（语义树 F2742 / 调试负载 F2608）并单独评审；架构层出现隐私面会让数据收集面无法审计",
    );
  }

  // 哈希待办为非阻断项（上游未提供内容哈希属可推进的待办，不是开工阻断理由），
  // 故mayStart 只看「除待办外的全部诊断为空」。
  return {
    mayStart:
      protocol.mayStart &&
      upstream.ready &&
      bag.all().filter((d) => d.code !== "PROTOCOL_HASH_PENDING").length === 0,
    diagnostics: bag.all(),
    coveredItems: covered,
    reservedItems: reserved,
  };
}

/**
 * 开工就绪判定的便捷入口：返回一段人话结论（含阻断原因与下一步），供 CLI 与
 * 调度侧直接打印——避免「自检返回了一堆 Diagnostic 但没人读」的静默失败。
 */
export function describeReadiness(report: DomainCheckReport = selfCheckDomain()): string {
  const pending = report.diagnostics.filter((d) => d.code === "PROTOCOL_HASH_PENDING");
  const blockers = report.diagnostics.filter((d) => d.code !== "PROTOCOL_HASH_PENDING");

  if (report.mayStart) {
    return (
      `N 域开工条件已满足：九个独占主题覆盖 ${report.coveredItems} 个功能号位（F2601~F2760），`
      + `收口保留段 ${report.reservedItems} 个号位（F2761~F2800），合计 200 项；`
      + `三大件依赖无环、四域能力属主唯一、M04 双协议已签收、上游四域契约链全通。`
      + `可进入 N01 首批控件树实现。`
      + (pending.length > 0 ? `\n另有 ${pending.length} 项非阻断待办：\n${formatDiags(pending, 4)}` : "")
    );
  }
  return (
    `N 域开工被阻断（${blockers.length} 项）：\n${formatDiags(blockers, 8)}`
    + (pending.length > 0 ? `\n非阻断待办（${pending.length} 项）：\n${formatDiags(pending, 4)}` : "")
    + `\n口径说明：${UPSTREAM_GATE_NOTE}`
  );
}

/** 诊断列表格式化（describeReadiness 与 CLI 共用，避免两处各写一份排版）。 */
function formatDiags(ds: readonly Diagnostic[], limit: number): string {
  const lines = ds.slice(0, limit).map((d, i) => `  ${i + 1}) [${d.code}] ${d.message}\n     下一步：${d.hint}`);
  const more = ds.length > limit ? `\n  ……另有 ${ds.length - limit} 项，见 selfCheckDomain().diagnostics` : "";
  return `${lines.join("\n")}${more}`;
}

/**
 * 边界裁决的便捷封装：返回三要素字符串，供 CLI 直接打印裁决结论。
 * 与 arbitrateBoundary 共用同一裁决逻辑，此处只做排版，不重复判定。
 */
export function describeRuling(ruling: BoundaryRuling): string {
  const head = `裁决：${ruling.ruling}（处置=${ruling.action}${ruling.requiresAdr ? "，必须走 ADR" : ""}）`;
  return ruling.diagnostic === null ? head : `${head}\n  原因：${ruling.diagnostic.message}\n  下一步：${ruling.diagnostic.hint}`;
}