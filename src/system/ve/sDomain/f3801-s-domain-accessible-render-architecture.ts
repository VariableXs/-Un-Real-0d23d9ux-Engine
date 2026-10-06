/**
 * VE-F3801 · S 域开工与无障碍渲染总架构（S 域 · 无障碍渲染域 · 批次 S01 · 组内第 1 条）
 * ---------------------------------------------------------------------------
 * 职责定位：S 域开工条目——宣告无障碍渲染域（F3801-F4000，官方主题十项：管线 / 高对比 /
 *   焦点强化 / 大字号 / 动效替代 / 读屏协同 / 纹理 / 光敏 / 认知 / 性能）正式开工。
 *   本条定义**域架构**本身：
 *     1. 第一等公民声明（无障碍不是覆盖层，而是渲染的第一等公民）；
 *     2. 最后一公里声明（语义→像素的执行域定位，附四域关系表）；
 *     3. 架构六段（状态采集→渲染策略→管线注入→执行→验证→反馈），各段签名冻结 v1；
 *     4. 官方主题十项落组映射（十项映射表）；
 *     5. 错误路径与降级矩阵（注入失败降级 / 采集断供 P0 / 语义-像素断链立案）。
 *
 * 本条是**架构声明 + 执法**条目：它交付一份可被机器校验的架构契约（六段签名、十项
 * 映射、四域关系表、降级矩阵），并附架构回归与断链演练的自检能力（由 F3812 承认为
 * 正式测试条目）。后续 19 条与 S02~S10 九个批次消费本条契约。
 *
 * ┌── 第一等公民声明（锚点原文：让渲染层本身成为无障碍能力的执行者）─────────────┐
 * │ 无障碍**不是覆盖层**。覆盖层是「在渲染完之后糊一层滤镜/浮层」的做法，它把     │
 * │ 无Accessibility当成后处理附加项，于是：艺术管线一挤压，无障碍就没了；          │
 * │ 用户请求了高对比，管线末端把它冲掉；语义树声明了可读，屏幕上没画。             │
 * │ S 域的立场是：无障碍能力**下沉进渲染层本身**——它是渲染的一等公民，与         │
 * │ 色彩管线、几何管线、合成管线同级。任何「先画完再覆盖」的实现都判为            │
 * │ `FIRST_CLASS_VIOLATION`，因为它把公民降格成了过客。                            │
 * └─────────────────────────────────────────────────────────────────────────────┘
 *
 * ┌── 最后一公里声明（锚点原文：语义→像素的最后一公里）───────────────────────────┐
 * │ N08 语义树 / N05 偏好 / P08 动效替代是**数据与逻辑层**：它们决定「应该        │
 * │ 是什么样」；S 域是**像素层执行**：把「应该是什么样」真正画进帧缓冲。           │
 * │ 两者之间那段路就是最后一公里。                                                 │
 * │ 断供红线（渲染版）：辅具态（屏幕阅读器 / 放大镜 / 开关机辅助输入）在采集段     │
 * │ 断供时，**不允许静默回退到无障碍关闭态**——那等于对依赖辅具的用户釜底抽薪。     │
 * │ 断供必须走 P0：复述断供红线渲染版，即把「我采不到你的辅助态」这件事本身        │
 * │ 变成像素（可见告警 + 拒降级），而不是变成沉默。                               │
 * └─────────────────────────────────────────────────────────────────────────────┘
 *
 * 语义-像素断链红线（本域最恶劣缺陷）：**语义树说有，屏幕上没画**。
 *   这比「画了但不好看」严重一个量级——它意味着系统向用户（尤其是读屏用户）
 *   宣告了一个不存在的视觉事实，用户据此行动（以为按钮在那里、以为弹窗打开了）。
 *   本域对此唯一的态度是**立案**：不降级、不修辞化、不静默，直接产出断链案卷，
 *   交 F3807（读屏协同）与 F3812（测试）双线追责。
 *
 * 架构六段（锚点原文：状态采集→渲染策略→管线注入→执行→验证→反馈）：
 *
 *   帧内六段序
 *   ──────────────────────────────────────────────────────────────────
 *   [① STATE_CAPTURE 状态采集]   四态采集（辅具接入/reduce/高对比/字号档）
 *        │  产出 AccessibilityState（唯一采集点，采集单源）
 *        ↓  O(态数)
 *   [② RENDER_STRATEGY 渲染策略] 四态→策略映射（策略表公开，透明红线）
 *        │  产出 RenderStrategySet
 *        ↓  O(策略数)
 *   [③ PIPELINE_INJECTION 注入]  注入 D 域渲染管线（样式/过滤/后处理三注入点）
 *        │  产出 InjectionReport（注入显性：每个策略注没注、注到哪、为何不注）
 *        ↓  O(策略数)
 *   [④ EXECUTE 执行]             渲染层按注入后的管线出像素
 *        │  产出 FramePixels
 *        ↓  O(1) 摊销
 *   [⑤ VERIFY 验证]              语义-像素一致断言（语义可见率）
 *        │  产出 VerifyReport（断链 → 立案）
 *        ↓  O(节点) 抽样
 *   [⑥ FEEDBACK 反馈]            降级显性复述 / 断供复述 / 断链案卷上行
 *           产出 FeedbackRecord（降级必须对人话复述，不留技术码）
 *
 * ┌── 上游契约 ──────────────────────────────────────────────────────────────┐
 * │ D 域渲染管线是**注入对端**：S 域只产出「要什么效果 + 什么参数」，像素怎么算   │
 * │ 是 D 域的事。本条因此只读消费 D 域的三个注入点清单，**不反向定义 D 域管线**。 │
 * │ 若本条声明的注入点不在 D 域清单内，裁决规则是「以 D 域为准，调整 S 域       │
 * │ 映射」，产出 `INJECTION_SLOT_UNKNOWN` 诊断 + 重排建议，绝不产出任何指向      │
 * │ D 域的改写。与 F1801 对 I02 的同款不可协商项。                              │
 * └─────────────────────────────────────────────────────────────────────────────┘
 *
 * 零静默纪律：本模块所有拒绝、冲突、阻断、断供、断链、降级都产出 Diagnostic
 *   （code + severity + message + hint + stage），由调用方聚合上报。本模块不向 UI
 *   直接抛异常，也不吞掉任何一条诊断。降级不是静默回落——**降级显性复述**是硬要求：
 *   任何一次降级都必须生成一句人话，告诉用户「你请求的 X 因为 Y 变成了 Z」。
 *
 * 性能逐项分解（锚点原文：六段 O(1) 每段摊销；注入 O(策略数)；映射 O(1)）：
 *   ①②④⑤⑥ 六段各自 O(1) 每帧摊销（策略为常数规模，段内不随节点数增长）；
 *   ③ 注入 O(策略数)——策略数是有限常数（十项主题派生），非 O(节点)；
 *   十项映射查表 O(1)。**没有一段是 O(场景节点数)**：这是架构约束，不是巧合。
 *
 * 工程量（锚点原文分解）：核心逻辑约 205 行（六段+映射+关系）、边界防护约 55 行
 *   （诊断+断链）、错误路径约 45 行（P0+立案）、测试支撑约 95 行（架构回归+
 *   断链演练），合计约 400 行。
 *
 * 判据：第一等公民、最后一公里、六段签名、注入显性、断链 P0、判据。
 *
 * 依赖锚点：F3795（R 域移交包 · S01 开工条件核验 + 签收）、F3807（读屏协同 · 断链
 *   双线追责）、F3812（无障碍渲染测试 · 承认为正式测试条目）、F3816（真源终版 ·
 *   采集单源分叉红线）、N08/F2741（语义树单源）、N05/F3146/F3149（辅具态采集单源）、
 *   P08/F3017（无障碍总纲执法）、D 域（渲染管线注入对端）、K 域 F2125（辅助滤镜
 *   分工：算法归 S 域、执行归 K 域）。
 * 下游消费：F3802 无障碍渲染管线 / F3803 高对比 / F3804 焦点强化 / F3805 大字号 /
 *   F3806 动效替代 / F3807 读屏协同 / F3808 纹理 / F3809 光敏 / F3810 认知 /
 *   F3811 性能 / F3812 测试 / F3813 调试器 / F3814 API 冻结 / F3817 fuzz /
 *   F3818 S01 联调 / F3819 预备自查 / F3820 组收口双签；S02~S10 九批次开工。
 * 交接说明：本条只管「架构声明 + 六段签名 + 十项映射 + 四域关系 + 降级矩阵」，
 *   四态采集实现归 F3802，高对比实现归 F3803，读屏协同实现归 F3807，测试条目归
 *   F3812，本条不越界实现它们。
 */

/* ═══════════════════════════════════════════════════════════════════════════
 * §1 诊断与结果类型（零静默的基础设施）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 诊断严重度分级。
 * 锚点原文「P0（复述断供红线渲染版）」——P0 是本域的阻断级：出现即不许降级、
 * 不许开工、不许收口，只允许立案与上报。
 */
export type Severity =
  /** 阻断级：辅具态断供、语义-像素断链、第一等公民被降格。出现即阻断。 */
  | "P0"
  /** 显性级：降级发生、注入点缺项、策略表不透明——必须对人话复述，但可继续。 */
  | "P1"
  /** 提示级：可观测性、待对账、预留位——记账即可。 */
  | "P2";

/** 诊断码：每种拒绝/冲突/断供/断链/降级都有独立可检索的码，绝不合并成通用错误。 */
export type DiagCode =
  /** 第一等公民被降格：无障碍实现被放到覆盖层/后处理而非渲染层本体。 */
  | "FIRST_CLASS_VIOLATION"
  /** 状态采集断供：辅具态丢失（屏幕阅读器/放大镜/辅助输入断开）。P0。 */
  | "STATE_CAPTURE_BLACKOUT"
  /** 状态采集多点分叉：同一辅具态被两处采集且不一致（态分叉红线）。 */
  | "STATE_CAPTURE_FORK"
  /** 语义-像素断链：语义树声明可见，帧缓冲未画——最恶劣缺陷，立案。 */
  | "SEMANTIC_PIXEL_BROKEN_LINK"
  /** 管线注入失败：注入 D 域失败，走降级路径（降级须显性复述）。 */
  | "INJECTION_FAILED"
  /** 注入点未知：S 域声明的注入点不在 D 域注入点清单内（以 D 域为准）。 */
  | "INJECTION_SLOT_UNKNOWN"
  /** 策略黑箱：渲染策略不透明（用户无法预测自己的设置会带来什么）。 */
  | "STRATEGY_OPAQUE"
  /** 六段签名与冻结版不一致（未走 ADR 的漂移）。 */
  | "STAGE_SIGNATURE_DRIFT"
  /** 六段序乱序或缺段。 */
  | "STAGE_SEQUENCE_INVALID"
  /** 十项映射缺项（官方主题未落组）。 */
  | "THEME_MAPPING_INCOMPLETE"
  /** 十项映射归属冲突（同一主题被两组同时认领）。 */
  | "THEME_OWNERSHIP_CONFLICT"
  /** 四域关系表缺行或行字段非法。 */
  | "RELATION_TABLE_INVALID"
  /** R 域移交包材料缺失（F3795 开工条件）。 */
  | "HANDOVER_MATERIAL_MISSING"
  /** R 域移交包接收确认位未签——开工阻断。 */
  | "HANDOVER_NOT_CONFIRMED"
  /** 无障碍注入预算超限（超 1ms/帧）。 */
  | "A11Y_BUDGET_EXCEEDED"
  /** 分工表条目数与 S01 组 20 条不符。 */
  | "WORKTABLE_CARDINALITY_INVALID"
  /** 分工表存在重复条目号。 */
  | "WORKTABLE_DUPLICATE_ENTRY"
  /** 分工表条目号不连续（缺位）。 */
  | "WORKTABLE_GAP"
  /** 架构修正（ADR）信息不全，无法受理。 */
  | "ADR_INCOMPLETE"
  /** 架构冻结快照与当前声明不一致（漂移告警）。 */
  | "FREEZE_DRIFT";

/** 一条诊断：发生了什么（code + message）、多严重（severity）、怎么办（hint）。 */
export interface Diagnostic {
  readonly code: DiagCode;
  readonly severity: Severity;
  /** 人话描述，面向开发者与用户排障，不含裸异常码。 */
  readonly message: string;
  /** 可操作提示：该改哪里、该怎么降级。 */
  readonly hint: string;
  /** 归属段（六段之一）；域级问题用 "DOMAIN"。 */
  readonly stage: RenderStage | "DOMAIN";
  /**
   * 降级/断供的显性复述文本（人话，直接可上屏）。
   * 锚点「降级显性复述」：降级不是静默回落，这句话必须能直接给用户看。
   * 非降级类诊断为 null。
   */
  readonly restatement: string | null;
}

/** 结果判别联合：成功必带 value，失败必带 code/message/hint——失败不可被误当成功。 */
export type Outcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly Diagnostic[] }
  | {
      readonly ok: false;
      readonly code: DiagCode;
      readonly severity: Severity;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly Diagnostic[];
    };

/** 成功构造（diagnostics 允许携带非致命告警，例如降级已复述）。 */
export function ok<T>(value: T, diagnostics: readonly Diagnostic[] = []): Outcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造：失败路径必须给出可操作提示，不允许裸码。 */
export function err<T>(
  code: DiagCode,
  severity: Severity,
  message: string,
  hint: string,
  diagnostics: readonly Diagnostic[] = [],
): Outcome<T> {
  const self: Diagnostic = { code, severity, message, hint, stage: "DOMAIN", restatement: null };
  return { ok: false, code, severity, message, hint, diagnostics: [...diagnostics, self] };
}

/** 取结果中的诊断（成功失败都取得到，失败时含自诊断）。 */
export function diagnosticsOf<T>(outcome: Outcome<T>): readonly Diagnostic[] {
  return outcome.diagnostics;
}

/** 是否含 P0（P0 出现即视为阻断，调用方必须显式处理，不得忽略）。 */
export function hasBlocking<T>(outcome: Outcome<T>): boolean {
  return outcome.diagnostics.some((d) => d.severity === "P0");
}

/** 过滤出某严重度的诊断（供面板分级展示）。 */
export function bySeverity<T>(outcome: Outcome<T>, severity: Severity): readonly Diagnostic[] {
  return outcome.diagnostics.filter((d) => d.severity === severity);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §2 S 域标识与官方十主题
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * S 域标识与官方十主题。
 *
 * 域号 S，条目区间 F3801-F4000（200 条 = 10 组 × 20 条）。
 *
 * 域号沿革（锚点 F3801 + R 域 F3601 跳段 ADR）：R 域条目号自 F3601 起跳
 * （F3401-F3600 段为 E 域扩展预留），故 S 域自 **F3801** 起，与册内域表
 * （VE-S = F3601-F3800）不同源——**以条目锚点 F3801 与 R 域跳段 ADR 为准**，
 * 域表为跳段前的旧账，按同步更新红线（S 域开工即修正）。此差异已记入
 * 域号沿革声明，避免后续条目按旧域表误定位。
 */
export const S_DOMAIN = {
  /** 域标签，与册内 `VE-S` 一致。 */
  tag: "VE-S",
  /** 域中文名。 */
  title: "无障碍渲染域",
  /** 条目起始（域开工条）。 */
  firstEntryId: 3801,
  /** 条目结束（域收官条）。 */
  lastEntryId: 4000,
  /** 条目总数。 */
  entryCount: 200,
  /** 组数（10 组，每组 20 条）。 */
  groupCount: 10,
  /** 每组条数。 */
  entriesPerGroup: 20,
} as const;

/**
 * S 域官方十主题（锚点原文：管线/高对比/焦点强化/大字号/动效替代/读屏协同/
 * 纹理/光敏/认知/性能——十项落组）。
 */
export const S_DOMAIN_TEN_TOPICS = [
  "管线",
  "高对比",
  "焦点强化",
  "大字号",
  "动效替代",
  "读屏协同",
  "纹理",
  "光敏",
  "认知",
  "性能",
] as const;

export type SDomainTopic = (typeof S_DOMAIN_TEN_TOPICS)[number];

/**
 * 十项落组映射（锚点「十项落组」）：每个官方主题落到哪一批次组、由哪条开工条目
 * 承接。S01 组承载全部十项的**架构落位**（本条即映射表本体），后续批次在各自
 * 主题组内做实现深挖。
 *
 * 落组不是「主题只归一组」——同一主题会在架构组落位、再在专组深挖（例：高对比
 * 在 S01 落位架构，由 S02「高对比与色彩适配组」F3821 起做实现）。此处记录的是
 * **首次落位**，避免重复认领。
 */
export interface ThemeLanding {
  readonly topic: SDomainTopic;
  /** 首次落位批次组标签。 */
  readonly landingGroup: string;
  /** 承接该主题落位的条目号。 */
  readonly landingEntryId: number;
  /** 深挖批次组标签（实现归该组）。 */
  readonly deepeningGroup: string;
  /** 深挖批次首条目号。 */
  readonly deepeningFirstId: number;
}

export const S_THEME_LANDINGS: readonly ThemeLanding[] = [
  { topic: "管线", landingGroup: "S01", landingEntryId: 3801, deepeningGroup: "S01", deepeningFirstId: 3802 },
  { topic: "高对比", landingGroup: "S01", landingEntryId: 3801, deepeningGroup: "S02", deepeningFirstId: 3821 },
  { topic: "焦点强化", landingGroup: "S01", landingEntryId: 3801, deepeningGroup: "S01", deepeningFirstId: 3804 },
  { topic: "大字号", landingGroup: "S01", landingEntryId: 3801, deepeningGroup: "S01", deepeningFirstId: 3805 },
  { topic: "动效替代", landingGroup: "S01", landingEntryId: 3801, deepeningGroup: "S01", deepeningFirstId: 3806 },
  { topic: "读屏协同", landingGroup: "S01", landingEntryId: 3801, deepeningGroup: "S03", deepeningFirstId: 3841 },
  { topic: "纹理", landingGroup: "S01", landingEntryId: 3801, deepeningGroup: "S01", deepeningFirstId: 3808 },
  { topic: "光敏", landingGroup: "S01", landingEntryId: 3801, deepeningGroup: "S01", deepeningFirstId: 3809 },
  { topic: "认知", landingGroup: "S01", landingEntryId: 3801, deepeningGroup: "S01", deepeningFirstId: 3810 },
  { topic: "性能", landingGroup: "S01", landingEntryId: 3801, deepeningGroup: "S01", deepeningFirstId: 3811 },
];

/** 校验十项落组映射：十项齐备、无归属冲突、条目号合法（硬闸）。 */
export function verifyThemeLandings(
  landings: readonly ThemeLanding[] = S_THEME_LANDINGS,
): Outcome<readonly ThemeLanding[]> {
  const diagnostics: Diagnostic[] = [];

  // 2.1 齐备性：官方十项每项都必须有落位。
  const landed = new Set(landings.map((l) => l.topic));
  const missing = S_DOMAIN_TEN_TOPICS.filter((t) => !landed.has(t));
  if (missing.length > 0) {
    diagnostics.push({
      code: "THEME_MAPPING_INCOMPLETE",
      severity: "P1",
      message: `十项映射缺项：${missing.join("、")} 未落组。`,
      hint: "官方主题十项每项都必须在映射表内；无实现的项标「预留」也不得删行——删行即无法核对齐备性。",
      stage: "DOMAIN",
      restatement: null,
    });
  }

  // 2.2 归属冲突：同一主题被两组同时认领首次落位。
  const seen = new Set<SDomainTopic>();
  for (const l of landings) {
    if (seen.has(l.topic)) {
      diagnostics.push({
        code: "THEME_OWNERSHIP_CONFLICT",
        severity: "P1",
        message: `十项映射归属冲突：主题「${l.topic}」被重复认领首次落位。`,
        hint: "首次落位唯一；同一主题的深挖归 deepeningGroup，不在 landingGroup 重复认领。",
        stage: "DOMAIN",
        restatement: null,
      });
    }
    seen.add(l.topic);
  }

  // 2.3 条目号合法性：落位/深挖条目号必须落在 S 域区间内。
  for (const l of landings) {
    const inDomain = (id: number): boolean => id >= S_DOMAIN.firstEntryId && id <= S_DOMAIN.lastEntryId;
    if (!inDomain(l.landingEntryId) || !inDomain(l.deepeningFirstId)) {
      diagnostics.push({
        code: "THEME_MAPPING_INCOMPLETE",
        severity: "P1",
        message: `主题「${l.topic}」条目号越界：落位 F${l.landingEntryId} / 深挖 F${l.deepeningFirstId}。`,
        hint: `条目号须落在 F${S_DOMAIN.firstEntryId}-F${S_DOMAIN.lastEntryId}；越界说明按了旧域表（F3601 起）定位，须按 R 域跳段 ADR 修正。`,
        stage: "DOMAIN",
        restatement: null,
      });
    }
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.severity, first.message, first.hint, diagnostics);
    }
  }
  return ok([...landings], diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §3 第一等公民声明与最后一公里声明（两条不可协商的域级执法条款）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 无障碍在渲染管线中的驻留层（第一等公民声明的可执行形态）。
 *
 * 锚点：「让渲染层本身成为无障碍能力的执行者」——驻留层必须是 RENDERER，
 * 而不是 OVERLAY。「覆盖层」是本域判定的反模式：它在渲染完成后糊一层，
 * 于是艺术管线一挤压就失效、末端合成能冲掉、语义与像素天然可能不一致。
 */
export type AccessibilityResidency =
  /** 正确：下沉进渲染层本体，与色彩/几何/合成管线同级。 */
  | "RENDERER"
  /** 反模式：覆盖层/后处理附加项——判 `FIRST_CLASS_VIOLATION`。 */
  | "OVERLAY"
  /** 反模式：仅 UI 层承担（渲染引擎内部无感知）——判 `FIRST_CLASS_VIOLATION`。 */
  | "UI_ONLY";

/** 第一等公民声明：一条无障碍实现的驻留层登记。 */
export interface FirstClassDeclaration {
  /** 能力标识（如 "高对比"、"焦点强化"）。 */
  readonly capability: string;
  /** 驻留层。 */
  readonly residency: AccessibilityResidency;
  /** 是否参与语义-像素一致断言（断链检测面）。 */
  readonly participatesInLinkAssertion: boolean;
}

/**
 * 校验第一等公民声明：任何 `OVERLAY` / `UI_ONLY` 登记即 P0 违规。
 *
 * 判 `FIRST_CLASS_VIOLATION` 是 P0 而非 P1 的理由：覆盖层实现在艺术管线
 * 挤压时会**静默失效**（无障碍请求被冲掉且无提示），这是无障碍能力的实质丢失，
 * 属阻断级。
 */
export function verifyFirstClassCitizen(
  declarations: readonly FirstClassDeclaration[],
): Outcome<readonly FirstClassDeclaration[]> {
  const diagnostics: Diagnostic[] = [];
  for (const d of declarations) {
    if (d.residency !== "RENDERER") {
      diagnostics.push({
        code: "FIRST_CLASS_VIOLATION",
        severity: "P0",
        message: `无障碍能力「${d.capability}」驻留在 ${d.residency}，不是渲染层本体。`,
        hint:
          "无障碍不是覆盖层：能力须下沉进渲染层管线（与色彩/几何/合成同级）。" +
          "覆盖层会被艺术管线挤压后静默失效——对依赖它的用户等于能力被收回。",
        stage: "DOMAIN",
        restatement: `你启用的「${d.capability}」当前以覆盖层方式实现，可能在复杂画面上被冲掉。我们已拦截该实现。`,
      });
    }
    // 声明了驻层合法却不参与一致断言 → 像素与语义脱钩，是断链的前置条件，同样 P0。
    if (d.residency === "RENDERER" && !d.participatesInLinkAssertion) {
      diagnostics.push({
        code: "FIRST_CLASS_VIOLATION",
        severity: "P0",
        message: `无障碍能力「${d.capability}」驻留渲染层却不参与语义-像素一致断言。`,
        hint: "驻留渲染层的能力必须纳入断链检测面；不参与断言等于放弃「画没画」的核查权。",
        stage: "DOMAIN",
        restatement: `「${d.capability}」的渲染结果无法被核查是否真的画出来了。我们已拦截该实现。`,
      });
    }
  }
  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.severity, first.message, first.hint, diagnostics);
    }
  }
  return ok([...declarations], diagnostics);
}

/** 四域关系表的一行：S 域与数据/逻辑层对端的分工声明。 */
export interface DomainRelation {
  /** 对端域标签（如 "VE-N"、"VE-P"）。 */
  readonly peerDomain: string;
  /** 对端提供的组/条目（如 "N08 语义树"）。 */
  readonly provides: string;
  /** 对端所属层。 */
  readonly layer: "DATA_LOGIC" | "PIXEL_EXECUTION";
  /** S 域向对端消费什么（反向依赖声明，防止隐藏耦合）。 */
  readonly sConsumes: string;
}

/**
 * 四域关系表（锚点原文：与 N08/N05/P08/R08 关系；数据结构「四域关系表」）。
 *
 * 最后一公里声明的可执行形态：N/N/P 三域皆为 `DATA_LOGIC` 层——它们决定
 * 「应该是什么样」；S 域是唯一的 `PIXEL_EXECUTION` 层——把「应该是什么样」
 * 画进帧缓冲。R 域（创作生态）供给创作产出的语义描述，同样是数据层。
 */
export const S_RELATION_TABLE: readonly DomainRelation[] = [
  {
    peerDomain: "VE-N",
    provides: "N08 语义树 / F2741 语义单源",
    layer: "DATA_LOGIC",
    sConsumes: "语义树节点可见性声明（断链断言的「语义侧」输入）",
  },
  {
    peerDomain: "VE-N",
    provides: "N05 判定与偏好 / F3146 F3149 采集单源",
    layer: "DATA_LOGIC",
    sConsumes: "四态判定结论（辅具接入/reduce/高对比/字号档）",
  },
  {
    peerDomain: "VE-P",
    provides: "P08 无障碍总纲 / F3017 执法",
    layer: "DATA_LOGIC",
    sConsumes: "动效替代裁决与等效能力红线",
  },
  {
    peerDomain: "VE-R",
    provides: "R08 创作语义描述",
    layer: "DATA_LOGIC",
    sConsumes: "创作产出的语义文案（描述质量双签由 P/S 共责）",
  },
];

/** 校验四域关系表：四行齐备、层位声明正确、S 域自身像素层身份唯一（硬闸）。 */
export function verifyRelationTable(
  table: readonly DomainRelation[] = S_RELATION_TABLE,
): Outcome<readonly DomainRelation[]> {
  const diagnostics: Diagnostic[] = [];

  // 3.1 行数与字段：四域关系表须恰四行（锚点「四域关系表」）。
  if (table.length !== 4) {
    diagnostics.push({
      code: "RELATION_TABLE_INVALID",
      severity: "P1",
      message: `四域关系表应为 4 行，实为 ${table.length} 行。`,
      hint: "四域 = N08 语义树 / N05 偏好 / P08 动效替代 / R08 创作语义；缺行即分工不全。",
      stage: "DOMAIN",
      restatement: null,
    });
  }

  // 3.2 层位：四域皆为数据与逻辑层——若某行被标为像素层，说明职责被混淆
  //     （「最后一公里」只属于 S 域，别的域也声称画像素即分工破口）。
  for (const row of table) {
    if (row.layer !== "DATA_LOGIC") {
      diagnostics.push({
        code: "RELATION_TABLE_INVALID",
        severity: "P1",
        message: `对端 ${row.peerDomain}（${row.provides}）被标为 ${row.layer}。`,
        hint: "N/P/R 三域均为数据与逻辑层；像素层执行唯一归属 S 域，标错会掩盖重复实现。",
        stage: "DOMAIN",
        restatement: null,
      });
    }
    if (row.sConsumes.trim().length === 0) {
      diagnostics.push({
        code: "RELATION_TABLE_INVALID",
        severity: "P1",
        message: `对端 ${row.peerDomain}（${row.provides}）未声明 S 域的反向消费内容。`,
        hint: "关系表须双向：不只写对端给什么，也写 S 域消费什么——否则隐藏耦合无处审计。",
        stage: "DOMAIN",
        restatement: null,
      });
    }
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.severity, first.message, first.hint, diagnostics);
    }
  }
  return ok([...table], diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §4 架构六段签名冻结 v1
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 六段标识（锚点原文：状态采集→渲染策略→管线注入→执行→验证→反馈）。 */
export type RenderStage =
  /** ① 状态采集：四态采集，唯一采集点。 */
  | "STATE_CAPTURE"
  /** ② 渲染策略：四态→策略映射，策略表公开。 */
  | "RENDER_STRATEGY"
  /** ③ 管线注入：注入 D 域渲染管线三注入点。 */
  | "PIPELINE_INJECTION"
  /** ④ 执行：渲染层出像素。 */
  | "EXECUTE"
  /** ⑤ 验证：语义-像素一致断言（断链检测面）。 */
  | "VERIFY"
  /** ⑥ 反馈：降级复述 / 断供复述 / 断链案卷上行。 */
  | "FEEDBACK";

/** 六段的规范帧内序（索引即执行序，不可乱序）。 */
export const RENDER_STAGE_ORDER: readonly RenderStage[] = [
  "STATE_CAPTURE",
  "RENDER_STRATEGY",
  "PIPELINE_INJECTION",
  "EXECUTE",
  "VERIFY",
  "FEEDBACK",
];

/** 六段中每段的复杂度声明（锚点性能逐项分解：六段 O(1) 每段摊销）。 */
export const STAGE_COMPLEXITY: Readonly<Record<RenderStage, string>> = {
  STATE_CAPTURE: "O(态数)=O(1)",
  RENDER_STRATEGY: "O(1) 查表",
  PIPELINE_INJECTION: "O(策略数)",
  EXECUTE: "O(1) 摊销",
  VERIFY: "O(节点) 抽样（抽样式，非全量）",
  FEEDBACK: "O(1) 摊销",
};

/**
 * 六段签名冻结 v1（锚点：各段签名冻结 v1）。
 *
 * 签名 = 段名 + 入参形状 + 出参形状 + 复杂度 + 失败码 + 段内不变量。
 * 冻结的含义：下游 19 条与 S02~S10 九批次按此签名先行开发（Schema 先行），
 * 实现期若要改签名，必须走 `proposeArchitectureAmendment` 留痕。
 */
export interface StageSignature {
  readonly stage: RenderStage;
  /** 签名版本（v1 冻结）。 */
  readonly version: "v1";
  /** 入参形状描述（人话，便于对拍）。 */
  readonly input: string;
  /** 出参形状描述。 */
  readonly output: string;
  /** 复杂度声明。 */
  readonly complexity: string;
  /** 该段失败时的诊断码。 */
  readonly failureCode: DiagCode;
  /** 段内不变量（违反即 P0 或立案，不允许"差不多就行"）。 */
  readonly invariant: string;
  /** 承接该段实现的条目号。 */
  readonly ownerEntryId: number;
}

export const STAGE_SIGNATURES_V1: readonly StageSignature[] = [
  {
    stage: "STATE_CAPTURE",
    version: "v1",
    input: "宿主无障碍状态源（辅具接入/reduce/高对比/字号档四态原始信号）",
    output: "AccessibilityState（四态归一后的不可变快照）",
    complexity: STAGE_COMPLEXITY.STATE_CAPTURE,
    failureCode: "STATE_CAPTURE_BLACKOUT",
    invariant: "采集单源：四态只允许本段采集，别处采集即态分叉（P0）",
    ownerEntryId: 3802,
  },
  {
    stage: "RENDER_STRATEGY",
    version: "v1",
    input: "AccessibilityState",
    output: "RenderStrategySet（策略表公开，四态→策略映射）",
    complexity: STAGE_COMPLEXITY.RENDER_STRATEGY,
    failureCode: "STRATEGY_OPAQUE",
    invariant: "策略透明：任何生效策略都必须有可解释的用户面文案（否则判黑箱）",
    ownerEntryId: 3802,
  },
  {
    stage: "PIPELINE_INJECTION",
    version: "v1",
    input: "RenderStrategySet + D 域注入点清单（只读消费）",
    output: "InjectionReport（逐策略：注没注 / 注到哪 / 为何不注）",
    complexity: STAGE_COMPLEXITY.PIPELINE_INJECTION,
    failureCode: "INJECTION_FAILED",
    invariant: "注入显性：无静默丢弃；每个未注入策略必须在报告中显性出现",
    ownerEntryId: 3802,
  },
  {
    stage: "EXECUTE",
    version: "v1",
    input: "已注入的渲染管线描述 + RenderStrategySet",
    output: "FramePixels（帧缓冲描述，含无障碍能力落地结果）",
    complexity: STAGE_COMPLEXITY.EXECUTE,
    failureCode: "FIRST_CLASS_VIOLATION",
    invariant: "第一等公民：无障碍能力参与渲染层本体，不在覆盖层/仅 UI 层",
    ownerEntryId: 3801,
  },
  {
    stage: "VERIFY",
    version: "v1",
    input: "N08 语义树可见性声明 + FramePixels",
    output: "VerifyReport（语义可见率 + 断链节点清单）",
    complexity: STAGE_COMPLEXITY.VERIFY,
    failureCode: "SEMANTIC_PIXEL_BROKEN_LINK",
    invariant: "断链零容忍：语义说有而像素没画 = 立案，不降级不修辞化",
    ownerEntryId: 3807,
  },
  {
    stage: "FEEDBACK",
    version: "v1",
    input: "降级事件 / 断供事件 / 断链案卷",
    output: "FeedbackRecord（每条含可上屏的人话复述文本）",
    complexity: STAGE_COMPLEXITY.FEEDBACK,
    failureCode: "INJECTION_FAILED",
    invariant: "降级显性复述：任何降级都生成一句人话，禁止静默回落",
    ownerEntryId: 3801,
  },
];

/**
 * 校验六段签名与冻结版一致（漂移检测，锚点：签名冻结 v1）。
 *
 * 顺序敏感性：六段序有语义（采集必须先于注入，注入必须先于执行），
 * 因此按数组序逐位比对，**不排序**——排序会把真正的乱序漂移洗掉。
 */
export function verifyStageSignatures(
  signatures: readonly StageSignature[] = STAGE_SIGNATURES_V1,
): Outcome<readonly StageSignature[]> {
  const diagnostics: Diagnostic[] = [];

  // 4.1 段数与覆盖：六段齐备，缺一段即架构不完整。
  if (signatures.length !== RENDER_STAGE_ORDER.length) {
    diagnostics.push({
      code: "STAGE_SEQUENCE_INVALID",
      severity: "P1",
      message: `六段签名应为 ${RENDER_STAGE_ORDER.length} 段，实为 ${signatures.length} 段。`,
      hint: `六段 = ${RENDER_STAGE_ORDER.join(" → ")}；缺段则帧内链路断裂。`,
      stage: "DOMAIN",
      restatement: null,
    });
  }

  // 4.2 逐段比对（不排序——乱序漂移必须被抓住）。
  for (let i = 0; i < RENDER_STAGE_ORDER.length; i++) {
    const expectedStage = RENDER_STAGE_ORDER[i];
    const actual = signatures[i];
    if (expectedStage === undefined || actual === undefined) {
      continue;
    }
    if (actual.stage !== expectedStage) {
      diagnostics.push({
        code: "STAGE_SIGNATURE_DRIFT",
        severity: "P1",
        message: `第 ${i + 1} 段应为 ${expectedStage}，实为 ${actual.stage}。`,
        hint: `六段序不可乱序：${RENDER_STAGE_ORDER.join(" → ")}。乱序会让注入跑到采集之前。`,
        stage: expectedStage,
        restatement: null,
      });
      continue;
    }
    // 4.3 签名内容漂移：入参/出参/不变量任一被改即漂移。
    const frozen = STAGE_SIGNATURES_V1[i];
    if (frozen === undefined) {
      continue;
    }
    if (actual.input !== frozen.input || actual.output !== frozen.output || actual.invariant !== frozen.invariant) {
      diagnostics.push({
        code: "STAGE_SIGNATURE_DRIFT",
        severity: "P1",
        message: `段 ${expectedStage} 签名与冻结 v1 不一致。`,
        hint: "签名冻结 v1：入参/出参/不变量变更须先走 ADR（proposeArchitectureAmendment）留痕，再回改本条。",
        stage: expectedStage,
        restatement: null,
      });
    }
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.severity, first.message, first.hint, diagnostics);
    }
  }
  return ok([...signatures], diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §5 状态采集段（①）：四态采集 + 断供 P0 渲染版
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 四态之一（锚点 F3802：辅具接入态/reduce 态/高对比态/字号档四态采集）。 */
export type AccessibilityStateKey =
  /** 辅具接入态：屏幕阅读器 / 放大镜 / 开关机 / 语音控制是否接入。 */
  | "ASSISTIVE_ATTACHED"
  /** reduce 态：用户请求减弱动效。 */
  | "REDUCE_MOTION"
  /** 高对比态：用户请求高对比度。 */
  | "HIGH_CONTRAST"
  /** 字号档：用户系统字号档位。 */
  | "FONT_SCALE_TIER";

/** 四态的规范序（采集与映射表索引序，不可乱序——它是策略表主键序）。 */
export const A11Y_STATE_ORDER: readonly AccessibilityStateKey[] = [
  "ASSISTIVE_ATTACHED",
  "REDUCE_MOTION",
  "HIGH_CONTRAST",
  "FONT_SCALE_TIER",
];

/** 四态标签（人话，用于用户面与复述文本）。 */
export const A11Y_STATE_LABELS: Readonly<Record<AccessibilityStateKey, string>> = {
  ASSISTIVE_ATTACHED: "辅助技术接入",
  REDUCE_MOTION: "减弱动效",
  HIGH_CONTRAST: "高对比度",
  FONT_SCALE_TIER: "字号档位",
};

/** 一个四态采集结果。 */
export interface StateProbe {
  readonly key: AccessibilityStateKey;
  /** 采集到的值。 */
  readonly value: boolean | number;
  /**
   * 本次采集是否成功。
   * 关键：`false` 表示**采不到**（辅具态断供），与「采到了且为 false」
   * 语义完全不同——后者是「用户没开」，前者是「我不知道用户开没开」。
   * 混同二者 = 对依赖辅具的用户静默关掉无障碍，是本域最不可接受的缺陷。
   */
  readonly acquired: boolean;
}

/** 四态采集快照。 */
export interface AccessibilityState {
  readonly probes: readonly StateProbe[];
  /** 采集来源标识（采集单源断言用：同一 frame 只能有一个来源）。 */
  readonly sourceId: string;
}

/** 按 key 取四态值（查表 O(1)）。 */
export function readState(state: AccessibilityState, key: AccessibilityStateKey): StateProbe | undefined {
  return state.probes.find((p) => p.key === key);
}

/**
 * 采集段校验（锚点错误路径：状态采集断（辅具态丢失）→P0 复述断供红线渲染版）。
 *
 * 两条独立红线：
 *   (1) 断供红线：任一态 `acquired === false` → P0。断供**不允许降级为「关闭」**，
 *       必须复述「我采不到你的辅助态」这件事本身（渲染版 = 让断供可见）。
 *   (2) 分叉红线：同一 frame 出现两个不同 `sourceId` → 态分叉，渲染必错乱，
 *       同样 P0（锚点 F3816 采集单源分叉红线的前向声明）。
 */
export function verifyStateCapture(
  state: AccessibilityState,
  expectedSourceId?: string,
): Outcome<AccessibilityState> {
  const diagnostics: Diagnostic[] = [];

  // 5.1 四态齐备性：缺项按「采不到」处理并显性报出，不静默填默认值。
  const present = new Set(state.probes.map((p) => p.key));
  for (const key of A11Y_STATE_ORDER) {
    if (!present.has(key)) {
      diagnostics.push({
        code: "STATE_CAPTURE_BLACKOUT",
        severity: "P0",
        message: `四态采集缺项：${A11Y_STATE_LABELS[key]} 未上报。`,
        hint: "缺项不得按默认值填充——填 false 等于替用户关掉无障碍。请从宿主无障碍状态源补采。",
        stage: "STATE_CAPTURE",
        restatement: `我们读不到你的「${A11Y_STATE_LABELS[key]}」设置，已暂停按该设置调整画面（不会替你猜一个值）。`,
      });
    }
  }

  // 5.2 断供检查：采不到 ≠ 关掉了。
  for (const probe of state.probes) {
    if (!probe.acquired) {
      diagnostics.push({
        code: "STATE_CAPTURE_BLACKOUT",
        severity: "P0",
        message: `辅具态断供：${A11Y_STATE_LABELS[probe.key]} 采集失败。`,
        hint:
          "断供走 P0：把断供本身渲染出来（可见告警 + 拒绝降级），" +
          "严禁回退到「无障碍关闭态」——那等于对依赖辅具的用户釜底抽薪。",
        stage: "STATE_CAPTURE",
        restatement: `我们暂时读不到你的「${A11Y_STATE_LABELS[probe.key]}」状态，画面暂不按它调整。请检查辅助技术是否仍连接。`,
      });
    }
  }

  // 5.3 分叉检查：同帧多来源 = 态分叉。
  if (expectedSourceId !== undefined && state.sourceId !== expectedSourceId) {
    diagnostics.push({
      code: "STATE_CAPTURE_FORK",
      severity: "P0",
      message: `态分叉：本帧采集来源为 ${state.sourceId}，期望唯一来源 ${expectedSourceId}。`,
      hint: "四态只允许一个采集点（锚点 F3816 采集单源）。双处采集必然出现不一致，渲染会错乱。",
      stage: "STATE_CAPTURE",
      restatement: "画面出现了两套互相矛盾的无障碍状态，已暂停自动调整以免显示错乱。",
    });
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.severity, first.message, first.hint, diagnostics);
    }
  }
  return ok(state, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §6 渲染策略段（②）：四态→策略映射 + 策略透明红线
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 注入点（锚点 F3802：样式注入/过滤注入/后处理注入三注入点；只读消费 D 域清单）。 */
export type InjectionSlot =
  /** 样式注入：改布局/尺寸/间距（字号档、大字号走此位）。 */
  | "STYLE"
  /** 过滤注入：像素级滤镜（高对比、色弱变换、光敏柔化走此位）。 */
  | "FILTER"
  /** 后处理注入：帧级调整（动效替代、认知简化走此位）。 */
  | "POST";

/** D 域提供的注入点清单（只读消费；本域不得反向定义）。 */
export const D_DOMAIN_INJECTION_SLOTS: readonly InjectionSlot[] = ["STYLE", "FILTER", "POST"];

/** 一条渲染策略（四态之一映射出的可执行策略）。 */
export interface RenderStrategy {
  /** 策略标识。 */
  readonly id: string;
  /** 由哪一态触发。 */
  readonly fromState: AccessibilityStateKey;
  /** 注入到哪个点。 */
  readonly slot: InjectionSlot;
  /** 策略参数（数值语义由各实现条目定标）。 */
  readonly params: Readonly<Record<string, number>>;
  /**
   * 用户面解释文案（策略透明红线的载体）。
   * 空串即判 `STRATEGY_OPAQUE`：用户无法预测自己的设置带来什么 = 行为不可预期。
   */
  readonly userFacingExplanation: string;
}

/**
 * 校验渲染策略集（锚点：策略表公开——策略透明红线）。
 *
 * 透明红线判 P1 而非 P0：策略不透明不会让画面错，但会让用户无法预期——
 * 「我把高对比打开，界面更亮了」这种不可预期本身就是无障碍缺陷。
 */
export function verifyStrategies(strategies: readonly RenderStrategy[]): Outcome<readonly RenderStrategy[]> {
  const diagnostics: Diagnostic[] = [];

  // 6.1 注入点存在性：策略只能挂 D 域提供的三个注入点之一。
  for (const s of strategies) {
    if (!D_DOMAIN_INJECTION_SLOTS.includes(s.slot)) {
      diagnostics.push({
        code: "INJECTION_SLOT_UNKNOWN",
        severity: "P1",
        message: `策略 ${s.id} 声明注入点 ${s.slot}，不在 D 域注入点清单内。`,
        hint: `D 域注入点为 ${D_DOMAIN_INJECTION_SLOTS.join(" / ")}。以 D 域为准调整 S 域映射，不反向改 D 域。`,
        stage: "RENDER_STRATEGY",
        restatement: `「${s.id}」无处可生效（D 域无此接入点），已按最接近的可用方式处理。`,
      });
    }

    // 6.2 透明红线：生效策略必须能用人话解释。
    if (s.userFacingExplanation.trim().length === 0) {
      diagnostics.push({
        code: "STRATEGY_OPAQUE",
        severity: "P1",
        message: `策略 ${s.id} 无用户面解释文案（策略黑箱）。`,
        hint: "每条生效策略都要有一句用户能懂的话说明「它会怎么改画面」；空文案即判黑箱。",
        stage: "RENDER_STRATEGY",
        restatement: "有一项无障碍设置我们无法用一句话说明它会怎么改画面，已暂缓该项并记账。",
      });
    }

    // 6.3 触发态合法性：策略必须由四态之一触发。
    if (!A11Y_STATE_ORDER.includes(s.fromState)) {
      diagnostics.push({
        code: "STRATEGY_OPAQUE",
        severity: "P1",
        message: `策略 ${s.id} 的触发态 ${s.fromState} 不在四态内。`,
        hint: `四态 = ${A11Y_STATE_ORDER.join(" / ")}；越界触发会让状态与策略的映射失去单源。`,
        stage: "RENDER_STRATEGY",
        restatement: null,
      });
    }

    // 6.4 参数有限性：NaN/Infinity 会污染下游像素计算。
    for (const [k, v] of Object.entries(s.params)) {
      if (!Number.isFinite(v)) {
        diagnostics.push({
          code: "INJECTION_FAILED",
          severity: "P1",
          message: `策略 ${s.id} 参数 ${k} 非有限值（${String(v)}）。`,
          hint: "参数须为有限实数；非有限值会让注入后的像素计算产出 NaN 区域。",
          stage: "RENDER_STRATEGY",
          restatement: `「${s.id}」有一项参数无效，该项已按不生效处理。`,
        });
      }
    }
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.severity, first.message, first.hint, diagnostics);
    }
  }
  return ok([...strategies], diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §7 管线注入段（③）：注入显性 + 1ms 预算红线
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 单条策略的注入结果（注入显性的载体：注没注、注到哪、为何不注）。 */
export interface InjectionOutcome {
  readonly strategyId: string;
  readonly injected: boolean;
  readonly slot: InjectionSlot | null;
  /** 未注入原因（injected=false 时必填——显性复述的依据）。 */
  readonly reason: string | null;
  /** 本条注入耗时（ms）。 */
  readonly costMs: number;
}

/** 注入报告（整帧汇总）。 */
export interface InjectionReport {
  readonly outcomes: readonly InjectionOutcome[];
  /** 注入总耗时（ms）——对 1ms 预算红线。 */
  readonly totalCostMs: number;
}

/**
 * 无障碍注入预算（锚点 F3802：无障碍开销挤占渲染=双向失败，1ms 线）。
 *
 * 为什么单列预算：无障碍渲染是要花钱的（额外 pass、额外纹理）。这笔开销
 * 若不设上限，会挤占正常渲染预算——用户开了高对比，反而整体更卡。
 * 那就是双向失败：无障碍没做好，正常渲染也被拖垮。
 */
export const A11Y_INJECTION_BUDGET_MS = 1.0;

/**
 * 校验注入报告（锚点错误路径：管线注入失败→降级路径+诊断，降级显性复述）。
 *
 * 注入显性：任何 `injected === false` 的策略都必须在报告里有 `reason`，
 * 且上层必须把它复述给用户。**没有原因的未注入 = 静默丢弃 = 本域不可接受。**
 */
export function verifyInjectionReport(
  report: InjectionReport,
  expectedStrategyIds: readonly string[],
): Outcome<InjectionReport> {
  const diagnostics: Diagnostic[] = [];

  // 7.1 覆盖性：每条期望策略都必须在报告里有交代（注入了或没注入+原因）。
  const reported = new Map(report.outcomes.map((o) => [o.strategyId, o]));
  for (const id of expectedStrategyIds) {
    if (!reported.has(id)) {
      diagnostics.push({
        code: "INJECTION_FAILED",
        severity: "P1",
        message: `策略 ${id} 未出现在注入报告中（静默丢弃）。`,
        hint: "注入显性：每条策略都要在报告里出现——要么注入，要么写明为何没注入。缺席即静默丢弃。",
        stage: "PIPELINE_INJECTION",
        restatement: `有一项无障碍设置（${id}）本轮没有生效，原因未记录。我们已记账，请复查该设置。`,
      });
    }
  }

  // 7.2 未注入须有原因（降级显性复述的原料）。
  for (const o of report.outcomes) {
    if (!o.injected && (o.reason === null || o.reason.trim().length === 0)) {
      diagnostics.push({
        code: "INJECTION_FAILED",
        severity: "P1",
        message: `策略 ${o.strategyId} 未注入但未给出原因。`,
        hint: "未注入必须带原因；无原因的未注入对用户与排障者都不可解释。",
        stage: "PIPELINE_INJECTION",
        restatement: "有一项无障碍设置没有生效，且我们没能说清原因——请重试或反馈。",
      });
    }
    // 注入点位必须合法（与 D 域清单一致）。
    if (o.injected && o.slot !== null && !D_DOMAIN_INJECTION_SLOTS.includes(o.slot)) {
      diagnostics.push({
        code: "INJECTION_SLOT_UNKNOWN",
        severity: "P1",
        message: `策略 ${o.strategyId} 注入到非法点位 ${o.slot}。`,
        hint: `D 域注入点为 ${D_DOMAIN_INJECTION_SLOTS.join(" / ")}；越位注入会让 D 域管线行为不可预期。`,
        stage: "PIPELINE_INJECTION",
        restatement: null,
      });
    }
    // 耗时合法性。
    if (!Number.isFinite(o.costMs) || o.costMs < 0) {
      diagnostics.push({
        code: "A11Y_BUDGET_EXCEEDED",
        severity: "P1",
        message: `策略 ${o.strategyId} 注入耗时非法（${String(o.costMs)}ms）。`,
        hint: "耗时须为非负有限值；非法值会让预算核算失效。",
        stage: "PIPELINE_INJECTION",
        restatement: null,
      });
    }
  }

  // 7.3 预算红线：注入总开销超 1ms/帧即立案优化。
  if (report.totalCostMs > A11Y_INJECTION_BUDGET_MS) {
    diagnostics.push({
      code: "A11Y_BUDGET_EXCEEDED",
      severity: "P1",
      message: `无障碍注入开销 ${report.totalCostMs.toFixed(3)}ms 超 1ms 预算。`,
      hint: "无障碍开销挤占渲染是双向失败：立案优化（降采样/合并 pass/缓存），F3811 性能条目承接。",
      stage: "PIPELINE_INJECTION",
      restatement: "无障碍效果已开启，但它让这一帧多花了些渲染时间；若画面变卡请告知我们。",
    });
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.severity, first.message, first.hint, diagnostics);
    }
  }
  return ok(report, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §8 验证段（⑤）：语义-像素断链检测 + 立案
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 一个节点的语义-像素对照（验证段输入单元）。 */
export interface SemanticPixelPair {
  /** 节点稳定 id（对应 N08 语义树节点）。 */
  readonly nodeId: string;
  /** 语义树声明：此节点应对用户可见。 */
  readonly semanticallyVisible: boolean;
  /** 像素层事实：此节点是否真的画进了帧缓冲。 */
  readonly painted: boolean;
  /** 是否为虚拟化保留节点（例外须显性——锚点 F3807 虚拟化语义保留例外）。 */
  readonly virtualizationRetained: boolean;
}

/** 一宗断链案卷（语义树说有、像素没画）。 */
export interface BrokenLinkCase {
  readonly nodeId: string;
  /** 立案时间戳（调用方注入，本模块不读时钟）。 */
  readonly filedAt: number;
  /** 案由（固定人话，便于直接上通报）。 */
  readonly charge: string;
  /** 追责线：F3807 读屏协同 + F3812 测试双线。 */
  readonly routedTo: readonly number[];
}

/**
 * 检测语义-像素断链（锚点：语义-像素断链→立案；最后一公里红线）。
 *
 * 判据（`semanticallyVisible && !painted` 即断链）：
 *   语义树说有 && 像素没画 = 断链 = 本域最恶劣缺陷。
 *
 * 唯一的合法例外是 `virtualizationRetained`（虚拟化语义保留）：节点被虚拟化
 * 移出视口时，语义树仍保留它以便读屏访问，屏幕上看不到是**预期**的。
 * 但例外必须显式标记——未标记的「看不到」一律判断链。
 */
export function detectBrokenLinks(
  pairs: readonly SemanticPixelPair[],
  now: number,
): Outcome<{ readonly visibleRate: number; readonly cases: readonly BrokenLinkCase[] }> {
  const diagnostics: Diagnostic[] = [];
  const cases: BrokenLinkCase[] = [];

  // 8.1 逐对判定：语义可见 && 未绘制 && 非显式例外 = 断链。
  for (const pair of pairs) {
    if (pair.semanticallyVisible && !pair.painted && !pair.virtualizationRetained) {
      cases.push({
        nodeId: pair.nodeId,
        filedAt: now,
        charge: `语义树声明节点 ${pair.nodeId} 对用户可见，但帧缓冲未绘制——系统向用户宣告了不存在的视觉事实。`,
        routedTo: [3807, 3812],
      });
    }
  }

  // 8.2 断链立案：任一断链即 P0（不降级、不修辞化、不静默）。
  if (cases.length > 0) {
    const firstCase = cases[0];
    const sample = firstCase !== undefined ? firstCase.nodeId : "(未知)";
    diagnostics.push({
      code: "SEMANTIC_PIXEL_BROKEN_LINK",
      severity: "P0",
      message: `语义-像素断链 ${cases.length} 处，首例节点 ${sample}。`,
      hint:
        "这是本域最恶劣缺陷：用户（尤其读屏用户）会据此以为某处有可交互物而误操作。" +
        "禁止降级或加例外掩盖——立案例卷交 F3807/F3812 双线追责，并回查该节点的注入路径。",
      stage: "VERIFY",
      restatement:
        "界面有些元素我们已声明它存在，但没能画出来——这会让你以为那里有东西可点。" +
        "我们已记录并正在修复；若你因此误操作，请避开该位置。",
    });
    return err("SEMANTIC_PIXEL_BROKEN_LINK", "P0", diagnostics[0]?.message ?? "语义-像素断链。", diagnostics[0]?.hint ?? "立案追责。", diagnostics);
  }

  // 8.3 语义可见率（抽样式；例外节点不计入分母外的不可见豁免）。
  const visible = pairs.filter((p) => p.semanticallyVisible);
  const paintedVisible = visible.filter((p) => p.painted);
  const visibleRate = visible.length === 0 ? 1 : paintedVisible.length / visible.length;

  return ok({ visibleRate, cases: [] }, diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §9 反馈段（⑥）：降级显性复述
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 一条反馈记录（每条必须带可上屏的人话复述）。 */
export interface FeedbackRecord {
  /** 关联的诊断码。 */
  readonly code: DiagCode;
  /** 归属段；域级诊断（十项映射/移交包/分工表等）无归属段，记 "DOMAIN"。 */
  readonly stage: RenderStage | "DOMAIN";
  /** 可直接上屏的人话复述文本。 */
  readonly restatement: string;
  /** 该复述面向用户还是仅面向开发者。 */
  readonly audience: "USER" | "DEVELOPER";
}

/**
 * 从诊断集生成反馈记录（锚点：降级显性复述）。
 *
 * 规则：
 *   - 有 `restatement` 的诊断 → 生成 USER 记录（降级/断供/断链必须让用户知道）；
 *   - 无 `restatement` 的诊断 → 只进 DEVELOPER 记录（纯工程问题不必打扰用户）。
 *
 * 这条规则是「异常零静默」在像素层的落地：任何影响用户所见所感的降级，
 * 都必须有一句能上屏的话，而不是一条躺在日志里的码。
 */
export function buildFeedback(diagnostics: readonly Diagnostic[]): readonly FeedbackRecord[] {
  const records: FeedbackRecord[] = [];
  for (const d of diagnostics) {
    if (d.restatement !== null && d.restatement.trim().length > 0) {
      records.push({ code: d.code, stage: d.stage, restatement: d.restatement, audience: "USER" });
    } else {
      records.push({
        code: d.code,
        stage: d.stage,
        restatement: d.message,
        audience: "DEVELOPER",
      });
    }
  }
  return records;
}

/** 取面向用户的复述（渲染层据此上屏告警条）。 */
export function userFacingRecords(records: readonly FeedbackRecord[]): readonly FeedbackRecord[] {
  return records.filter((r) => r.audience === "USER");
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §10 域开工前置检查（F3795 R 域移交包接收）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** R 域移交包材料段（锚点 F3795：七件包，含双维无障碍册）。 */
export type HandoverMaterialKind =
  | "INTERFACE_FREEZE"
  | "COMPUTE_LABEL_LEDGER"
  | "LESSON_BOOK"
  | "EVIDENCE_TRIO"
  | "CREATION_A11Y_LEDGER"
  | "CREATION_DELIVERABLE_A11Y_LEDGER"
  | "HANDOVER_ACK";

export const HANDOVER_MATERIAL_LABELS: Readonly<Record<HandoverMaterialKind, string>> = {
  INTERFACE_FREEZE: "接口冻结清单",
  COMPUTE_LABEL_LEDGER: "算力标签终册",
  LESSON_BOOK: "经验教训记录",
  EVIDENCE_TRIO: "证据三件套",
  CREATION_A11Y_LEDGER: "创作工具无障碍册",
  CREATION_DELIVERABLE_A11Y_LEDGER: "创作产出无障碍册",
  HANDOVER_ACK: "接收确认签收",
};

/** 移交包材料一段。 */
export interface HandoverMaterial {
  readonly kind: HandoverMaterialKind;
  readonly present: boolean;
}

/** R 域移交包（S 域接收侧）。 */
export interface HandoverPackage {
  readonly materials: readonly HandoverMaterial[];
  /** S 域签收确认位。 */
  readonly receiverConfirmed: boolean;
  readonly receiverSignature: string | null;
}

/**
 * 域开工前置检查（锚点 F3795：S01 无障碍渲染开工条件核验 + 签收）。
 *
 * 阻断条件（任一命中即阻断开工）：
 *   (1) 七件材料任一缺失；
 *   (2) S 域签收确认位未签。
 *
 * 特别地，双维无障碍册（创作工具 + 创作产出）缺失时也阻断：S 域是像素层
 * 执行者，创作侧若无障碍语义，S 域就没有可执行的「应该是什么样」。
 */
export function verifyKickoffPreconditions(pkg: HandoverPackage): Outcome<readonly HandoverMaterialKind[]> {
  const diagnostics: Diagnostic[] = [];

  const presentKinds = new Set(pkg.materials.filter((m) => m.present).map((m) => m.kind));
  const allKinds: readonly HandoverMaterialKind[] = [
    "INTERFACE_FREEZE",
    "COMPUTE_LABEL_LEDGER",
    "LESSON_BOOK",
    "EVIDENCE_TRIO",
    "CREATION_A11Y_LEDGER",
    "CREATION_DELIVERABLE_A11Y_LEDGER",
    "HANDOVER_ACK",
  ];

  for (const kind of allKinds) {
    if (!presentKinds.has(kind)) {
      diagnostics.push({
        code: "HANDOVER_MATERIAL_MISSING",
        severity: "P1",
        message: `R 域移交包材料缺失：${HANDOVER_MATERIAL_LABELS[kind]}。`,
        hint: "向 R 域（F3795）索取缺失材料；七件包缺一不可开工。双维无障碍册缺失尤甚——S 域无创作侧语义则无可执行目标。",
        stage: "DOMAIN",
        restatement: null,
      });
    }
  }

  if (!pkg.receiverConfirmed || pkg.receiverSignature === null) {
    diagnostics.push({
      code: "HANDOVER_NOT_CONFIRMED",
      severity: "P1",
      message: "S 域接收确认位未签。",
      hint: "在移交包接收确认位签 S 域（域标签 + 日期）；确认前不得宣告 S 域开工。",
      stage: "DOMAIN",
      restatement: null,
    });
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.severity, first.message, first.hint, diagnostics);
    }
  }
  return ok([...presentKinds], diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §11 S01 组分工表（20 行，F3801-F3820）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 分工表中一条。 */
export interface WorkTableRow {
  readonly entryId: number;
  readonly title: string;
  readonly topic: SDomainTopic;
  /** 该条在本域架构中的角色。 */
  readonly role: string;
  /** 上游依赖条目（空串表示无）。 */
  readonly upstream: string;
}

/**
 * S01 组 20 条分工表（F3801-F3820，标题逐条核对册内锚点）。
 * 本条（VE-F3801）为组内第 1 条，即域开工与无障碍渲染总架构本身。
 */
export const S01_WORK_TABLE: readonly WorkTableRow[] = [
  { entryId: 3801, title: "S 域开工与无障碍渲染总架构", topic: "管线", role: "域开工·第一等公民·最后一公里·六段签名·十项映射·四域关系·降级矩阵", upstream: "F3795" },
  { entryId: 3802, title: "无障碍渲染管线", topic: "管线", role: "四态采集+策略透明+三注入点+1ms 预算；本条六段签名的实现方", upstream: "F3801" },
  { entryId: 3803, title: "高对比渲染引擎", topic: "高对比", role: "对比度实时检测+高对比管线参数+与 E 域令牌联动", upstream: "F3802" },
  { entryId: 3804, title: "焦点渲染强化", topic: "焦点强化", role: "焦点环可见性+焦点放大+键盘顺序视觉一致性", upstream: "F3802" },
  { entryId: 3805, title: "大字号与缩放渲染", topic: "大字号", role: "字号档映射+布局重排+四档 DPI 走查承接", upstream: "F3802" },
  { entryId: 3806, title: "动效渲染替代", topic: "动效替代", role: "reduce 态的像素级替代（静态化/淡入替代/无位移）", upstream: "F3802,P08" },
  { entryId: 3807, title: "读屏渲染协同总架构", topic: "读屏协同", role: "双树一致+查询隔离+协同事件+断链追责承接", upstream: "F3801,N08" },
  { entryId: 3808, title: "无障碍纹理与图形", topic: "纹理", role: "纹理可辨识化+图形化替代+图标语义化", upstream: "F3802" },
  { entryId: 3809, title: "光敏渲染安全", topic: "光敏", role: "闪烁频率上限+大面积高亮柔化+与 K 域 F2133 双向对账", upstream: "F3802" },
  { entryId: 3810, title: "认知渲染辅助", topic: "认知", role: "信息密度降级+阅读辅助+简化模式", upstream: "F3802" },
  { entryId: 3811, title: "无障碍渲染性能", topic: "性能", role: "1ms 预算实测+超预算优化立案+开销遥测", upstream: "F3802" },
  { entryId: 3812, title: "无障碍渲染测试", topic: "管线", role: "断链演练回归+四态矩阵回归；本条自检能力的正式承接", upstream: "F3801,F3807" },
  { entryId: 3813, title: "无障碍渲染调试器", topic: "管线", role: "六段逐段可视化+断链节点定位+注入报告查看", upstream: "F3802,F3812" },
  { entryId: 3814, title: "无障碍渲染 API 冻结", topic: "管线", role: "六段签名对外 API 冻结+描述词成册", upstream: "F3802" },
  { entryId: 3815, title: "无障碍渲染文档", topic: "管线", role: "总纲三章（架构/十项/红线）+ 快速上手", upstream: "F3814" },
  { entryId: 3816, title: "无障碍渲染与 S 域真源终版", topic: "读屏协同", role: "采集单源断言+分叉检测+N05 判定与渲染同源对拍", upstream: "F3807,N05" },
  { entryId: 3817, title: "无障碍渲染 fuzz", topic: "管线", role: "四态组合 fuzz+断链构造 fuzz+预算越界 fuzz", upstream: "F3802,F3812" },
  { entryId: 3818, title: "S01 联调", topic: "管线", role: "S01 全组联调场景矩阵+跨域对端联调", upstream: "F3802~F3817" },
  { entryId: 3819, title: "S01 预备自查", topic: "管线", role: "组级预备报告 GO/NO-GO 判定+缺口补齐", upstream: "F3818" },
  { entryId: 3820, title: "S01 组收口双签", topic: "管线", role: "组收口双签+向 S02 移交架构契约段", upstream: "F3801~F3819" },
];

/** 校验分工表：条目数 = 20、条目号 F3801-F3820 连续无重复、每条有主题归属。 */
export function verifyWorkTable(table: readonly WorkTableRow[] = S01_WORK_TABLE): Outcome<readonly WorkTableRow[]> {
  const diagnostics: Diagnostic[] = [];

  // 11.1 重复。
  const seen = new Set<number>();
  for (const row of table) {
    if (seen.has(row.entryId)) {
      diagnostics.push({
        code: "WORKTABLE_DUPLICATE_ENTRY",
        severity: "P1",
        message: `分工表条目号重复：F${row.entryId}。`,
        hint: "每个条目号在组内唯一；重复即覆盖了他条职责。",
        stage: "DOMAIN",
        restatement: null,
      });
    }
    seen.add(row.entryId);
  }

  // 11.2 连续性——先于基数检查：缺位是病因，「只有 19 条」只是症状。
  const ids = [...seen].sort((a, b) => a - b);
  for (let i = 0; i < ids.length; i++) {
    const expected = S_DOMAIN.firstEntryId + i;
    const actual = ids[i];
    if (actual !== undefined && actual !== expected) {
      diagnostics.push({
        code: "WORKTABLE_GAP",
        severity: "P1",
        message: `分工表条目号不连续：期望 F${expected}，实到 F${actual}。`,
        hint: `本组须连续覆盖 F${S_DOMAIN.firstEntryId}-F${S_DOMAIN.firstEntryId + S_DOMAIN.entriesPerGroup - 1}，缺位即漏项。`,
        stage: "DOMAIN",
        restatement: null,
      });
      break;
    }
  }

  // 11.3 基数（症状级）。
  if (table.length !== S_DOMAIN.entriesPerGroup) {
    diagnostics.push({
      code: "WORKTABLE_CARDINALITY_INVALID",
      severity: "P1",
      message: `S01 分工表应为 ${S_DOMAIN.entriesPerGroup} 条，实为 ${table.length} 条。`,
      hint: "按 S 域「每组 20 条」补齐；本组覆盖 F3801-F3820。",
      stage: "DOMAIN",
      restatement: null,
    });
  }

  // 11.4 主题归属合法（十项之一）。
  for (const row of table) {
    if (!S_DOMAIN_TEN_TOPICS.includes(row.topic)) {
      diagnostics.push({
        code: "THEME_MAPPING_INCOMPLETE",
        severity: "P1",
        message: `F${row.entryId} 主题归属「${row.topic}」不在官方十项内。`,
        hint: `十项 = ${S_DOMAIN_TEN_TOPICS.join(" / ")}。`,
        stage: "DOMAIN",
        restatement: null,
      });
    }
  }

  if (diagnostics.length > 0) {
    const first = diagnostics[0];
    if (first !== undefined) {
      return err(first.code, first.severity, first.message, first.hint, diagnostics);
    }
  }
  return ok([...table], diagnostics);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §12 架构冻结快照 + 漂移检测
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 架构冻结快照：域开工的机器可读凭据。 */
export interface ArchitectureFreeze {
  readonly domainTag: "VE-S";
  readonly firstEntryId: number;
  readonly lastEntryId: number;
  /** 六段序快照。 */
  readonly stages: readonly RenderStage[];
  /** 六段签名摘要（段名:版本:失败码:不变量）。 */
  readonly stageSignatures: readonly string[];
  /** 十项主题快照。 */
  readonly topics: readonly SDomainTopic[];
  /** 四域关系表对端快照。 */
  readonly peers: readonly string[];
  /** 分工表条目数。 */
  readonly workTableRows: number;
  /** 冻结时间戳（调用方注入，保持本模块纯函数、无隐式时钟）。 */
  readonly frozenAt: number;
}

/**
 * 快照一致性摘要 → FNV-1a 32 位指纹（短、稳定、无依赖，非安全用途）。
 *
 * 排序口径：无序语义的部分（主题集合、对端集合）先排序再参与哈希——
 * 集合的键序只反映构造顺序，不反映架构语义。不排序会让同一份架构声明
 * 经不同构造路径（字面量 vs Object.keys 推导）得到不同指纹，漂移检测将
 * 大量误报——那是守卫失效，不是架构真的变了。
 * 有序语义的部分（六段序）保留原序。
 */
export function freezeFingerprint(freeze: ArchitectureFreeze): string {
  const parts = [
    freeze.domainTag,
    `${freeze.firstEntryId}-${freeze.lastEntryId}`,
    freeze.stages.join(","),
    freeze.stageSignatures.join("|"),
    [...freeze.topics].sort().join(","),
    [...freeze.peers].sort().join(","),
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
export function detectDrift(
  frozen: ArchitectureFreeze,
  current: ArchitectureFreeze,
): Outcome<ArchitectureFreeze> {
  const frozenFp = freezeFingerprint(frozen);
  const currentFp = freezeFingerprint(current);
  if (frozenFp !== currentFp) {
    return err(
      "FREEZE_DRIFT",
      "P1",
      `架构漂移：冻结指纹 ${frozenFp} ≠ 当前指纹 ${currentFp}。`,
      "架构声明已被改动但未走 ADR 回改本条；补 ADR 或还原至冻结态，二选一。",
    );
  }
  return ok(frozen);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §13 架构修正 ADR（实现期回改本条）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 一条架构修正提案。 */
export interface ArchitectureAmendment {
  readonly proposedByEntryId: number;
  /** 修正的架构面。 */
  readonly target: "STAGE_SIGNATURE" | "THEME_LANDING" | "RELATION_TABLE" | "WORK_TABLE" | "INJECTION_SLOT";
  readonly rationale: string;
  readonly impact: string;
}

/** 受理架构修正提案：信息不全即拒绝——防止「顺手改架构」而无据。 */
export function proposeArchitectureAmendment(
  amendment: ArchitectureAmendment,
): Outcome<{ readonly adrId: string; readonly accepted: boolean }> {
  if (amendment.rationale.trim().length === 0) {
    return err(
      "ADR_INCOMPLETE",
      "P1",
      `F${amendment.proposedByEntryId} 的架构修正提案缺少理由。`,
      "ADR 须写明修正动因；无理由的架构变更不予受理。",
    );
  }
  if (amendment.impact.trim().length === 0) {
    return err(
      "ADR_INCOMPLETE",
      "P1",
      `F${amendment.proposedByEntryId} 的架构修正提案缺少影响面。`,
      "ADR 须列明受影响的段/条目与下游消费者，便于回改时同步对齐。",
    );
  }
  const adrId = `ADR-S-${String(amendment.proposedByEntryId).padStart(4, "0")}`;
  return ok({ adrId, accepted: true });
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §14 域开工（编排：前置闸门 → 第一等公民 → 六段签名 → 十项映射 → 关系表 → 分工表 → 冻结）
 * ═══════════════════════════════════════════════════════════════════════════ */

/** 域开工结果。 */
export interface DomainKickoff {
  readonly freeze: ArchitectureFreeze;
  readonly fingerprint: string;
  readonly diagnostics: readonly Diagnostic[];
}

/**
 * 宣告 S 域开工。
 *
 * 编排六道关（顺序即依赖顺序，任一硬闸失败则不开工）：
 *   1. R 域移交包前置检查（F3795 签收）——未签即阻断，硬闸；
 *   2. 第一等公民声明校验——覆盖层/仅 UI 层即 P0，硬闸；
 *   3. 六段签名校验——漂移/乱序即拒，硬闸；
 *   4. 十项映射校验——缺项/冲突，硬闸；
 *   5. 四域关系表校验——缺行/层位错，硬闸；
 *   6. 分工表校验——基数/重复/连续性，硬闸；然后产出冻结快照与指纹。
 *
 * @param declarations 第一等公民声明集（各能力实现的驻留层登记）
 * @param now 冻结时间戳（调用方注入，本模块不读时钟，保持可测试与可重放）
 */
export function openDomain(
  pkg: HandoverPackage,
  declarations: readonly FirstClassDeclaration[],
  now: number,
): Outcome<DomainKickoff> {
  // 14.1 前置闸门。
  const pre = verifyKickoffPreconditions(pkg);
  if (!pre.ok) return pre;

  // 14.2 第一等公民闸门（最硬的架构红线）。
  const citizen = verifyFirstClassCitizen(declarations);
  if (!citizen.ok) return citizen;

  // 14.3 六段签名闸门。
  const sigs = verifyStageSignatures();
  if (!sigs.ok) return sigs;

  // 14.4 十项映射闸门。
  const themes = verifyThemeLandings();
  if (!themes.ok) return themes;

  // 14.5 四域关系表闸门。
  const rel = verifyRelationTable();
  if (!rel.ok) return rel;

  // 14.6 分工表闸门。
  const wt = verifyWorkTable();
  if (!wt.ok) return wt;

  // 14.7 冻结。
  const freeze: ArchitectureFreeze = {
    domainTag: S_DOMAIN.tag,
    firstEntryId: S_DOMAIN.firstEntryId,
    lastEntryId: S_DOMAIN.lastEntryId,
    stages: [...RENDER_STAGE_ORDER],
    stageSignatures: STAGE_SIGNATURES_V1.map(
      (s) => `${s.stage}:${s.version}:${s.failureCode}:${s.invariant}`,
    ),
    topics: [...S_DOMAIN_TEN_TOPICS],
    peers: S_RELATION_TABLE.map((r) => `${r.peerDomain}:${r.provides}`),
    workTableRows: wt.value.length,
    frozenAt: now,
  };

  const merged: Diagnostic[] = [...pre.diagnostics, ...citizen.diagnostics, ...sigs.diagnostics];
  return ok({ freeze, fingerprint: freezeFingerprint(freeze), diagnostics: merged }, merged);
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §15 架构回归 + 断链演练（测试支撑；F3812 承认为正式测试条目）
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

/** 构造一份「四态全采到、用户全开」的健康采集快照（回归基线）。 */
function healthyState(): AccessibilityState {
  return {
    sourceId: "HOST_A11Y_CHANNEL",
    probes: [
      { key: "ASSISTIVE_ATTACHED", value: true, acquired: true },
      { key: "REDUCE_MOTION", value: true, acquired: true },
      { key: "HIGH_CONTRAST", value: true, acquired: true },
      { key: "FONT_SCALE_TIER", value: 3, acquired: true },
    ],
  };
}

/** 一条覆盖三注入点、带解释文案的健康策略集（回归基线）。 */
function healthyStrategies(): readonly RenderStrategy[] {
  return [
    { id: "hc-filter", fromState: "HIGH_CONTRAST", slot: "FILTER", params: { ratio: 7 }, userFacingExplanation: "把前景与背景的对比度提到 7:1，便于看清细字。" },
    { id: "reduce-motion", fromState: "REDUCE_MOTION", slot: "POST", params: { static: 1 }, userFacingExplanation: "把滑动与缩放换成淡入淡出，不做位移。" },
    { id: "font-scale", fromState: "FONT_SCALE_TIER", slot: "STYLE", params: { tier: 3 }, userFacingExplanation: "按你设定的字号档放大文字与行距。" },
  ];
}

/**
 * 架构回归 + 断链演练（本条的测试支撑能力）。
 *
 * 覆盖三组：
 *   A. 架构回归——六段签名、十项映射、四域关系、分工表在冻结态下必须全绿；
 *   B. 断链演练——四组对照（健康/断链/虚拟化例外/语义不可见）逐项核对判定；
 *   C. 红线演练——第一等公民违规、断供、断链必须产出 P0 且带可上屏复述。
 *
 * 说明：本函数是**架构自检能力**（交付物的一部分，F3812 据此建正式测试条目），
 * 不是临时测试脚本。派发脚本属过程产物，归 `_attic/`，不入库。
 */
export function runSelfCheck(): SelfCheckReport {
  const groups: Record<string, SelfCheckItem[]> = {};
  const add = (group: string, name: string, pass: boolean, detail: string): void => {
    const list = groups[group] ?? [];
    list.push({ group, name, pass, detail });
    groups[group] = list;
  };

  // ── A. 架构回归 ────────────────────────────────────────────────────────────
  const sigs = verifyStageSignatures();
  add("A-架构回归", "六段签名冻结 v1 无漂移", sigs.ok, sigs.ok ? "六段签名与冻结版逐位一致" : `漂移：${sigs.message}`);

  const stagesCovered = new Set(STAGE_SIGNATURES_V1.map((s) => s.stage));
  const allStages = RENDER_STAGE_ORDER.every((s) => stagesCovered.has(s));
  add("A-架构回归", "六段齐备无缺段", allStages, `已定义 ${stagesCovered.size}/${RENDER_STAGE_ORDER.length} 段`);

  const themes = verifyThemeLandings();
  add("A-架构回归", "十项映射齐备无冲突", themes.ok, themes.ok ? `十项落位齐备（${S_THEME_LANDINGS.length} 项）` : `缺项/冲突：${themes.message}`);

  const rel = verifyRelationTable();
  add("A-架构回归", "四域关系表完整", rel.ok, rel.ok ? `四域齐备（${S_RELATION_TABLE.length} 行）` : `关系表缺陷：${rel.message}`);

  const wt = verifyWorkTable();
  add("A-架构回归", "S01 分工表 20 条连续", wt.ok, wt.ok ? `分工表 ${wt.value.length} 条，F3801-F3820 连续` : `分工表缺陷：${wt.message}`);

  const budgetPositive = A11Y_INJECTION_BUDGET_MS > 0 && Number.isFinite(A11Y_INJECTION_BUDGET_MS);
  add("A-架构回归", "1ms 注入预算红线已定义", budgetPositive, `预算 = ${A11Y_INJECTION_BUDGET_MS}ms/帧`);

  // ── B. 断链演练（最后一公里红线）───────────────────────────────────────────
  const healthyPairs: readonly SemanticPixelPair[] = [
    { nodeId: "n1", semanticallyVisible: true, painted: true, virtualizationRetained: false },
    { nodeId: "n2", semanticallyVisible: true, painted: true, virtualizationRetained: false },
  ];
  const healthyLink = detectBrokenLinks(healthyPairs, 1000);
  add(
    "B-断链演练",
    "健康场景：语义可见且已绘制 → 无断链",
    healthyLink.ok,
    healthyLink.ok ? `语义可见率 = ${(healthyLink.value.visibleRate * 100).toFixed(0)}%` : `误报断链：${healthyLink.message}`,
  );

  const brokenPairs: readonly SemanticPixelPair[] = [
    { nodeId: "n1", semanticallyVisible: true, painted: true, virtualizationRetained: false },
    { nodeId: "n2", semanticallyVisible: true, painted: false, virtualizationRetained: false },
  ];
  const brokenLink = detectBrokenLinks(brokenPairs, 1000);
  const brokenP0 = !brokenLink.ok && brokenLink.severity === "P0";
  const brokenHasRestatement = brokenLink.diagnostics.some((d) => d.restatement !== null);
  add("B-断链演练", "断链场景：语义说有像素没画 → 立案 P0", brokenP0, brokenP0 ? `立案成功，严重度 P0` : `未按 P0 立案：ok=${brokenLink.ok}`);
  add("B-断链演练", "断链必须带可上屏复述", brokenHasRestatement, brokenHasRestatement ? "断链诊断带用户复述文本" : "断链缺复述——用户不会知道自己被误导了");

  const vRetained: readonly SemanticPixelPair[] = [
    { nodeId: "n3", semanticallyVisible: true, painted: false, virtualizationRetained: true },
  ];
  const vRetainedLink = detectBrokenLinks(vRetained, 1000);
  add(
    "B-断链演练",
    "虚拟化保留例外显式标记 → 豁免",
    vRetainedLink.ok,
    vRetainedLink.ok ? "显式虚拟化例外不判断链" : "显式例外仍被误判断链——例外机制失效",
  );

  const invisible: readonly SemanticPixelPair[] = [
    { nodeId: "n4", semanticallyVisible: false, painted: false, virtualizationRetained: false },
  ];
  const invisibleLink = detectBrokenLinks(invisible, 1000);
  add(
    "B-断链演练",
    "语义声明不可见 → 不判断链",
    invisibleLink.ok,
    invisibleLink.ok ? "不可见节点不在断链面" : "不可见节点被误判为断链",
  );

  // ── C. 红线演练 ────────────────────────────────────────────────────────────
  const overlayViolation = verifyFirstClassCitizen([
    { capability: "高对比", residency: "OVERLAY", participatesInLinkAssertion: true },
  ]);
  const overlayP0 = !overlayViolation.ok && overlayViolation.severity === "P0";
  add("C-红线演练", "覆盖层实现 → 第一等公民 P0", overlayP0, overlayP0 ? "覆盖层被判 P0 违规" : "覆盖层未被拦——第一等公民声明形同虚设");

  const uiOnlyViolation = verifyFirstClassCitizen([
    { capability: "焦点强化", residency: "UI_ONLY", participatesInLinkAssertion: true },
  ]);
  add("C-红线演练", "仅 UI 层实现 → 第一等公民 P0", !uiOnlyViolation.ok, uiOnlyViolation.ok ? "仅 UI 层未被拦" : "仅 UI 层被判违规");

  const blackout = verifyStateCapture({
    sourceId: "HOST_A11Y_CHANNEL",
    probes: [
      { key: "ASSISTIVE_ATTACHED", value: false, acquired: false },
      { key: "REDUCE_MOTION", value: false, acquired: true },
      { key: "HIGH_CONTRAST", value: false, acquired: true },
      { key: "FONT_SCALE_TIER", value: 0, acquired: true },
    ],
  });
  const blackoutP0 = !blackout.ok && blackout.severity === "P0";
  const blackoutRestatement = blackout.diagnostics.some((d) => d.restatement !== null);
  add("C-红线演练", "辅具态断供 → P0", blackoutP0, blackoutP0 ? "断供被判 P0" : "断供未按 P0 处理");
  add("C-红线演练", "断供必须复述（渲染版断供红线）", blackoutRestatement, blackoutRestatement ? "断供带用户复述" : "断供静默——等于替用户关掉了无障碍");

  const forked = verifyStateCapture(healthyState(), "OTHER_CHANNEL");
  add("C-红线演练", "双来源采集 → 态分叉 P0", !forked.ok, forked.ok ? "态分叉未被检出" : "态分叉已检出");

  const opaque = verifyStrategies([
    { id: "mystery", fromState: "HIGH_CONTRAST", slot: "FILTER", params: {}, userFacingExplanation: "" },
  ]);
  add("C-红线演练", "策略黑箱 → 策略透明红线", !opaque.ok, opaque.ok ? "无解释文案的策略未被拦" : "策略黑箱已拦");

  const missing = verifyStrategies([
    { id: "ghost", fromState: "HIGH_CONTRAST", slot: "GHOST_SLOT" as InjectionSlot, params: {}, userFacingExplanation: "说明" },
  ]);
  add("C-红线演练", "越位注入点 → 以 D 域为准", !missing.ok, missing.ok ? "越位注入点未被拦" : "越位注入点已拦");

  const overBudget = verifyInjectionReport(
    { outcomes: [], totalCostMs: 1.6 },
    [],
  );
  add("C-红线演练", "注入超 1ms → 预算红线", !overBudget.ok, overBudget.ok ? "超预算未被拦" : "超预算已拦");

  const silentDrop = verifyInjectionReport(
    { outcomes: [{ strategyId: "a", injected: false, slot: null, reason: null, costMs: 0 }], totalCostMs: 0.1 },
    ["a"],
  );
  add("C-红线演练", "未注入无原因 → 静默丢弃拦截", !silentDrop.ok, silentDrop.ok ? "静默丢弃未被拦" : "静默丢弃已拦");

  // 反馈段：降级显性复述的分级。
  const feedback = buildFeedback([
    ...(healthyState() ? [] : []),
    { code: "INJECTION_FAILED", severity: "P1", message: "工程侧问题", hint: "", stage: "PIPELINE_INJECTION", restatement: "有一项设置本轮没生效。" },
    { code: "STRATEGY_OPAQUE", severity: "P1", message: "工程侧问题2", hint: "", stage: "RENDER_STRATEGY", restatement: null },
  ]);
  const userCount = userFacingRecords(feedback).length;
  add("C-红线演练", "降级显性复述分流正确", userCount === 1, `面向用户 ${userCount} 条 / 面向开发者 ${feedback.length - userCount} 条`);

  // 注入显性：健康路径应全绿。
  const healthyInjection = verifyInjectionReport(
    {
      outcomes: healthyStrategies().map((s) => ({ strategyId: s.id, injected: true, slot: s.slot, reason: null, costMs: 0.1 })),
      totalCostMs: 0.3,
    },
    healthyStrategies().map((s) => s.id),
  );
  add("C-红线演练", "健康注入路径全绿", healthyInjection.ok, healthyInjection.ok ? "三策略均注入且在预算内" : `健康路径误报：${healthyInjection.message}`);

  // 采集健康路径。
  const healthyCapture = verifyStateCapture(healthyState(), "HOST_A11Y_CHANNEL");
  add("C-红线演练", "健康采集路径全绿", healthyCapture.ok, healthyCapture.ok ? "四态齐备且来源唯一" : `健康采集误报：${healthyCapture.message}`);

  // 策略健康路径。
  const healthyStrategyCheck = verifyStrategies(healthyStrategies());
  add("C-红线演练", "健康策略集全绿", healthyStrategyCheck.ok, healthyStrategyCheck.ok ? "三策略注入点合法且均有解释" : `健康策略误报：${healthyStrategyCheck.message}`);

  // 冻结指纹稳定性（同输入逐位一致，可重放）。
  const freezeSample: ArchitectureFreeze = {
    domainTag: S_DOMAIN.tag,
    firstEntryId: S_DOMAIN.firstEntryId,
    lastEntryId: S_DOMAIN.lastEntryId,
    stages: [...RENDER_STAGE_ORDER],
    stageSignatures: STAGE_SIGNATURES_V1.map((s) => `${s.stage}:${s.version}:${s.failureCode}:${s.invariant}`),
    topics: [...S_DOMAIN_TEN_TOPICS],
    peers: S_RELATION_TABLE.map((r) => `${r.peerDomain}:${r.provides}`),
    workTableRows: S01_WORK_TABLE.length,
    frozenAt: 1,
  };
  const fpA = freezeFingerprint(freezeSample);
  const fpB = freezeFingerprint(freezeSample);
  add("A-架构回归", "冻结指纹可重放（同输入逐位一致）", fpA === fpB, `指纹 = ${fpA}`);

  const driftDetected = !detectDrift(freezeSample, { ...freezeSample, workTableRows: 19 }).ok;
  add("A-架构回归", "架构漂移可检出", driftDetected, driftDetected ? "改一行即被指纹捕获" : "漂移未检出——守卫失效");

  const allItems: SelfCheckItem[] = Object.values(groups).flat();
  const failed = allItems.filter((i) => !i.pass).map((i) => `[${i.group}] ${i.name}`);
  return { groups, allPass: failed.length === 0, failed };
}

/* ═══════════════════════════════════════════════════════════════════════════
 * §16 域开工宣告文本（架构文档替述可读，无障碍要求）
 * ═══════════════════════════════════════════════════════════════════════════ */

/**
 * 生成 S 域开工宣告（人话版）。
 *
 * 无障碍要求：架构文档替述可读——本函数是文档的可执行生成源，保证「文档所述」
 * 与「代码所声明」同源，不会各说各话。
 */
export function renderKickoffDeclaration(freeze: ArchitectureFreeze, fingerprint: string): string {
  const sigLines = STAGE_SIGNATURES_V1.map(
    (s, i) => `  ${i + 1}. ${s.stage}（${s.complexity}）→ ${s.output}`,
  ).join("\n");
  const landingLines = S_THEME_LANDINGS.map(
    (l) => `  · ${l.topic}：落位 F${l.landingEntryId}（${l.landingGroup}）→ 深挖 F${l.deepeningFirstId}（${l.deepeningGroup}）`,
  ).join("\n");
  return [
    `【VE-S 无障碍渲染域 · 开工宣告】`,
    `条目区间：F${freeze.firstEntryId}-F${freeze.lastEntryId}（共 ${S_DOMAIN.entryCount} 条 / ${S_DOMAIN.groupCount} 组）。`,
    ``,
    `第一等公民声明：无障碍不是覆盖层，而是渲染层的第一等公民。`,
    `  能力必须驻留渲染层本体（RENDERER）；驻留覆盖层（OVERLAY）或仅 UI 层（UI_ONLY）判 P0。`,
    ``,
    `最后一公里声明：N08 语义树 / N05 偏好 / P08 动效替代 / R08 创作语义是数据与逻辑层，`,
    `  S 域是像素层执行——把「应该是什么样」画进帧缓冲，是语义→像素的最后一公里。`,
    ``,
    `架构六段（签名冻结 v1）：`,
    sigLines,
    ``,
    `官方主题十项落组：`,
    landingLines,
    ``,
    `四域关系表：${freeze.peers.join("；")}。`,
    ``,
    `红线三条：`,
    `  ① 第一等公民：覆盖层实现 = P0；`,
    `  ② 断供红线：辅具态采不到 = P0，复述渲染版断供，严禁静默回退到「无障碍关闭」；`,
    `  ③ 最后一公里：语义树说有而像素没画 = 断链立案（P0），交 F3807/F3812 双线追责。`,
    ``,
    `无障碍注入预算：≤${A11Y_INJECTION_BUDGET_MS}ms/帧（超预算 = 双向失败，立案优化）。`,
    `注入显性：每条策略注没注、注到哪、为何不注，逐条在 InjectionReport 显性可查。`,
    `S01 组分工表：${freeze.workTableRows} 条（F${S_DOMAIN.firstEntryId}-F${S_DOMAIN.firstEntryId + S_DOMAIN.entriesPerGroup - 1}）。`,
    `架构冻结指纹：${fingerprint}（冻结时间戳 ${freeze.frozenAt}）。`,
  ].join("\n");
}
