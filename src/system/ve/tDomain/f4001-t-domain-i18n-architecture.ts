/**
 * VE-F4001 · T 域开工与国际化总架构（T 域 · 国际化域开工条 · 批次 T01 首项）
 * ---------------------------------------------------------------------------
 * 职责定位：VE-T 域（F4001~F4200）的**开工条**。它不翻译一个字符串、不跑一次
 * 排版、不切一次方向，只做五件事，并把五件事写成可机检的契约：
 *   1. 宣告「国际化不是翻译，是架构」——架构级 i18n 声明（global-first）；
 *   2. 与 N02 / O06 / S03 / R 四域声明分工边界（四域分工表 + 冲突仲裁规则）；
 *   3. 登记十项映射（Locale/方向/排版/字体/格式/复数/密度/输入法/测试/调试）；
 *   4. 冻结架构五段签名 v1（Locale 模型→方向→排版→格式→交付），逐段定契约；
 *   5. 承接F2948lang 单源复述（语言标签与偏好表不在本域另立一份）。
 *
 * 为什么国际化必须是架构而不是翻译（域级立场，全域根基，最重要的一段声明）：
 *   翻译是**末端工序**：等界面写完了，把字符串换成另一种语言。
 *   国际化是**架构约束**：语言、方向、排版、字体、格式、密度在**结构层**就已
 *   决定界面能不能长什么样。这两者差着一个数量级的返工量——
 *     · 硬编码字符串 200 处→ 末序要改的是 200 处调用点加一条提取链；
 *     · 布局用 px 固定宽高、没留膨胀余量 → 德语+35%、阿拉伯语换行规则完全不同，
 *       末序要改的是每一个组件的度量方式；
 *     · RTL 方向在结构上不做隔离→ 拼接用户名的界面会出现「视觉正确但语义反转」
 *       的欺骗显示（F4016 的BiDi 覆写攻击面同源）。
 *   而这些在开工时不决定，到F4020（T01 组收口）时就必须全族返工——200 项里
 *   至少一半的行文与度量都要重写。故本条把 global-first 做成**可机检的守卫**：
 *   任何声称「本域/本模块不做国际化」或「国际化只是最后加翻译」的模块，
 *   在开发期就被拦下，而不是在收口时靠人肉巡检发现。
 *
 * 四域分工（判据二，与 N02/O06/S03/R 的对端声明）：
 *   N 域（文本栈/排版执行）—— 提供「给定一段文字如何分行断词整形」的机制；
 *   T 域（本域国际化底座）—— 提供「给定一个 Locale 这个界面应该是什么样子」的决策；
 *   O 域（字体/Servo 选型与光栅化）—— 提供「用什么字形画、覆盖率与度量对齐」；
 *   S 域 S03（无障碍读屏语义）—— 提供「读屏怎么念、语言切换时语义树怎么跟随」；
 *   R 域（创作/工作流）—— 提供「谁在什么时候改文案、改动怎么流转与校对」。
 *   一句话记法：**N 管怎么排、T 管长什么样、O 管用什么字画、S 管怎么念、R 管谁改**。
 *   冲突仲裁：同一问题被两个域同时认领时，按「决策优先于执行、语义优先于呈现」
 *   的序由 T 域出决策、由对应执行域落地，T 域不越界替执行域做实现。
 *
 * 十项映射（判据三）：每一项都是一个「Locale →界面决策」的映射族，逐项登记
 * 归属条目、产出契约名与不可越界声明。十项齐备由自检断言机检。
 *
 * 架构五段（判据四，签名冻结 v1）：
 *   段1 Locale 模型 → 段2 方向 → 段3 排版策略 → 段4 格式 → 段5 交付。
 *   五段是**有向串行**的：下游段的输入必须来自上游段的产出契约，不得旁路取值。
 *   「段间失配→对拍」即由此而来：若某模块从全局变量直接取 Locale 而不走段1产出，
 *   它与段1的解析结果就可能不一致——这类分歧在开发期由 auditSegmentChain机检。
 *
 * 零静默纪律：Locale 非法→拒绝三要素；方向缺失→默认 LTR+ 诊断（不是静默兜底）；
 *   段间失配→对拍；十项映射缺项→阻断；五段签名不匹配→阻断开工。
 *   本模块不抛异常、不吞诊断、无静默分支、无全局可变状态、零 DOM 依赖。
 *
 * 判据：global-first、四域分工、十项映射、五段签名、单源复述。
 * 交接说明：纯契约层。可在浏览器 / Worker / Node 校验脚本中原样引入。
 *         下游 T01（F4002 语言标签与Locale 模型 起）逐项消费本条的
 *         MAPPING_REGISTRY、SEGMENT_SIGNATURES、DOMAIN_DIVISION 与
 *         F2948_SINGLE_SOURCE_DECLARATION。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型（零静默的基础设施：与 I/L/N 各域同纪律，此处独立实现不跨域 import）
// ════════════════════════════════════════════════════════════════════════════

/** 诊断码：每种拒绝/越权/退化独立可检索，绝不合并成一条通用错误。 */
export type DiagCode =
  /** Locale 标签非法（不是合法 BCP47 子集，或违反本域参数域）。 */
  | "LOCALE_ILLEGAL"
  /** Locale 合法但不在覆盖表内（显性拒绝，不静默当und 处理）。 */
  | "LOCALE_UNCOVERED"
  /** 方向缺失：调用方未声明方向，本域默认 LTR 并产出诊断（不是静默兜底）。 */
  | "DIRECTION_MISSING_DEFAULTED"
  /** 方向取值不在三方向封闭集内。 */
  | "DIRECTION_INVALID"
  /** 层级声明非法（文档级/区块级/字符级三层之外）。 */
  | "DIRECTION_TIER_INVALID"
  /** 十项映射缺项（判据三硬门）。 */
  | "MAPPING_MISSING"
  /** 十项映射重复登记（两项映射共用同一项 id）。 */
  | "MAPPING_DUPLICATE"
  /** 映射与总架构声明不一致（映射试图改写 global-first 立场）。 */
  | "MAPPING_CONTRADICTS_DECLARATION"
  /** 五段签名不齐（某段未登记）。 */
  | "SEGMENT_UNREGISTERED"
  /** 五段签名版本不一致（段间版本号不同=对不上版）。 */
  | "SEGMENT_VERSION_MISMATCH"
  /** 段间失配：下游段的输入不是上游段的产出契约（对拍命中）。 */
  | "SEGMENT_CHAIN_MISMATCH"
  /** 段序倒置：上游段依赖了下游段的产出（成环）。 */
  | "SEGMENT_ORDER_INVERTED"
  /** 四域分工冲突：两个域对同一问题同时认领且未声明仲裁顺序。 */
  | "DIVISION_CONFLICT"
  /** 四域分工越界：T 域替执行域（N/O/S/R）做了实现承诺。 */
  | "DIVISION_OVERREACH"
  /** 四域分工缺项：某个决策面无域认领。 */
  | "DIVISION_GAP"
  /** 单源分叉：某事实源在 T 域被另立一份（F2948 lang 纪律）。 */
  | "SINGLE_SOURCE_FORKED"
  /** 架构声明守卫命中：某模块否认国际化是架构级约束。 */
  | "ARCHITECTURE_DECLARATION_VIOLATION"
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

/** 成功构造（diagnostics 允许携带非致命告警，例如方向默认化的显性提示）。 */
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

  /** 是否存在任何诊断。 */
  get hasAny(): boolean {
    return this.items.length > 0;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 域身份 + 架构级 i18n 声明（判据一：global-first）
// ════════════════════════════════════════════════════════════════════════════

/** 域身份常量（本域所有对外文本引用同一事实源，不各自硬编码）。 */
export const DOMAIN = {
  /** 域编号。 */
  id: "VE-T",
  /** 域名。 */
  name: "国际化",
  /** 功能号区间。 */
  itemLo: 4001,
  itemHi: 4200,
  /** 本域功能总数（4200 - 4001 + 1 = 200，与册内「十域 × 200 项」一致）。 */
  itemCount: 200,
  /** 所属波次（VE 卷波次表：W10 = 国际化）。 */
  wave: "W10",
  /** 本域开工条的功能号。 */
  kickoffItemId: 4001,
  /** 批次表：十批次 × 20 项。 */
  batchCount: 10,
  /** 架构签名版本（冻结值：五段签名 v1，变更走 ADR）。 */
  architectureSignatureVersion: "v1",
  /** CLDR 版本锚定（格式与复数规则的唯一权威版本，见 VE-F4006）。 */
  cldrVersion: "46",
} as const;

/**
 * 架构级 i18n 声明（六条，全域立场，判据一的核心资产）。
 *
 * 为什么这六条要在开工条写死而不是各条目自行解释（反直觉处）：
 *   国际化最常见的失败形态不是「翻译错了」，而是**每个条目各自决定「国际化
 *   是什么」**。于是 L01 的字体条目认为国际化=选字体，N02 的排版条目认为
 *   国际化=不断词，UI 条目认为国际化=换个 string——三者都不错，合起来却是一个
 *   什么都不保证的系统：语言一换，排版塌、方向反、格式错、密度失控，
 *   而每一项的单测都是绿的。故把六条声明做成**全局立场 + 守卫**，
 *   任何条目想改立场须走 ADR，不能在实现里悄悄换定义。
 *
 * 六条立场逐条含义：
 *   ① 架构级约束——i18n 是结构约束，不是末端工序（见上文模块注释）。
 *   ② 决策与执行分离——T 域出「应该长什么样」的决策；怎么排/用什么字画/
 *      怎么念/谁改，由N/O/S/R 域执行。T 域不越界写排版实现。
 *   ③ 无语言特例——不存在「英文界面是主语言、其他语言是附属」的写法。
 *      中文/英文与阿拉伯/泰文同等是一等公民，度量与布局按最差语言设计。
 *   ④ 参数域钳制——任何 Locale/方向/密度相关的取值都必须过本域钳制函数，
 *      非法值显性拒绝，不允许 undefined/NaN 传播到下游（未定义值是最贵的bug）。
 *   ⑤ 可降级必显性——降级（如未收录语言→Latin）必须带诊断并可被界面展示，
 *      静默降级等于给用户看一个错的界面且不告诉他。
 *   ⑥ 单源纪律——语言标签、字体偏好、格式规则各有唯一权威源（F2948 等），
 *      T 域不另立第二份。发现分叉即立案。
 */
export const ARCHITECTURE_DECLARATION: readonly ArchitecturePillar[] = [
  {
    id: "architectural-not-end-stage",
    title: "国际化是架构级约束",
    normative: true,
    statement:
      "i18n 必须在结构层决定语言、方向、排版、字体、格式与密度；"
      + "把它当作「界面写完再加翻译」的末端工序，在收口阶段必然引发全族返工。",
    rationale:
      "硬编码字符串、px 固定度量、缺方向隔离这四类问题的返工量级随组件数线性增长，"
      + "且其中两类（度量与方向）会静默产出错误画面，测试用绿对错。",
    evidence: ["VE-F4011 伪本地化硬编码检出", "VE-F4008 膨胀 35% 断言", "VE-F4003 三层方向"],
  },
  {
    id: "decision-execution-split",
    title: "决策与执行分离",
    normative: true,
    statement:
      "T 域负责「给定 Locale，界面应该是什么样子」的决策；"
      + "排版（N）、字体光栅（O）、读屏语义（S）、文案流转（R）由各自主责域执行。",
    rationale:
      "同一问题被两个域各自实现一遍时，分歧不会报错，只会在语言切换的那一帧"
      + "表现为「排版对了但字形缺」或「念对了但位置反了」，最难归因。",
    evidence: ["VE-F4004 策略执行分工", "四域分工表 DIVISION_TABLE"],
  },
  {
    id: "no-language-is-second-class",
    title: "无语言特例",
    normative: true,
    statement:
      "不存在「英文是主语言、其他是附属」的写法；度量、密度档、可用性判据"
      + "一律按覆盖表内最差语言（膨胀最大 / 无空格 / 非LTR）设计。",
    rationale:
      "以英文度量出的布局在德语（+35% 膨胀）与阿拉伯语（无空格、右起）下必然破版；"
      + "此时修的不是「翻译」而是「按英文做的所有假设」。",
    evidence: ["VE-F4008 膨胀缓冲红线", "VE-F4010 无空格语言边界语料"],
  },
  {
    id: "clamped-parameter-domain",
    title: "参数域钳制",
    normative: true,
    statement:
      "所有 Locale / 方向 / 密度 / 格式相关取值必须经本域钳制函数入参；"
      + "非法值显性拒绝（返回 Outcome 失败），禁止 undefined / NaN 向下游传播。",
    rationale:
      "未定义值在下游的表现通常不是崩溃而是「渲染成空白」或「按默认值画」；"
      + "后者让bug 活到用户点它为止，且日志里什么都没有。",
    evidence: ["VE-F4002 Locale 钳制", "VE-F4006 格式域钳制", "参数域钳制函数族"],
  },
  {
    id: "degradation-must-be-visible",
    title: "可降级必显性",
    normative: true,
    statement:
      "任何降级（未收录语言→Latin、缺字→下一跳字体、方向缺失→LTR）必须产出诊断"
      + "并可被界面呈现；禁止静默降级。",
    rationale:
      "静默降级的危害不是「不好看」而是「误导」：阿拉伯语用户看到拉丁字母的界面，"
      + "会以为系统坏了而不是自己装错了语言。没有诊断=无法定位、无从告知。",
    evidence: ["VE-F4004 未收录语言降级显性", "VE-F4005 缺字回退计数"],
  },
  {
    id: "single-source-discipline",
    title: "单源纪律",
    normative: true,
    statement:
      "语言标签与匹配树、字体偏好、CLDR 格式规则各有唯一权威源（F2948 等）；"
      + "T 域消费而不另立，任何分叉即立案（模型复用不是复制，是引用同一事实源）。",
    rationale:
      "两份 BCP47 匹配树在「zh-CN 回退到 zh 还是 zh-Hans」上给出不同答案时，"
      + "字体选择与读屏发音会各自按自己那棵树走，最终产生「字对了但念错了」。",
    evidence: ["F2948 lang 单源", "VE-F4005字体偏好表引用", "VE-F4006 CLDR 锚定"],
  },
];

/** 架构声明条目 id（封闭集：新增立场须改本类型与ARCHITECTURE_DECLARATION 两处）。 */
export type ArchitecturePillarId =
  | "architectural-not-end-stage"
  | "decision-execution-split"
  | "no-language-is-second-class"
  | "clamped-parameter-domain"
  | "degradation-must-be-visible"
  | "single-source-discipline";

/** 一条架构立场的完整规格（逐条声明依据与证据锚点，无一项靠约定）。 */
export interface ArchitecturePillar {
  readonly id: ArchitecturePillarId;
  /** 立场标题（对外文档与错误提示引用同一措辞）。 */
  readonly title: string;
  /** 是否为规范性立场（true = 全域必须遵守；预留位为 false）。 */
  readonly normative: boolean;
  /** 规范条文原文（人话，可直接进代码评审 checklist）。 */
  readonly statement: string;
  /** 为什么必须这样（反直觉处写在这里，避免后来者「简化」掉）。 */
  readonly rationale: string;
  /** 证据锚点：这条立场在哪个条目上被兑现/可被机检。 */
  readonly evidence: readonly string[];
}

/** 架构声明注册表id 全集（遍历用，避免手写清单与实际登记漂移）。 */
export const ARCHITECTURE_PILLAR_IDS: readonly ArchitecturePillarId[] =
  ARCHITECTURE_DECLARATION.map((p) => p.id);

// ════════════════════════════════════════════════════════════════════════════
// §3 四域分工（判据二：与 N02/O06/S03/R 的对端声明）
// ════════════════════════════════════════════════════════════════════════════

/** 参与分工的域 id（封闭集：新增对端须改本类型与 DIVISION_TABLE 两处）。 */
export type PartnerDomainId = "VE-T" | "VE-N" | "VE-O" | "VE-S" | "VE-R";

/** 决策面 id（十四个面，逐面须有且仅有一个**决策**归属）。 */
export type DecisionFacet =
  /** Locale 模型：标签解析、回退链、覆盖表。 */
  | "locale-model"
  /** 方向：三层声明与仲裁。 */
  | "direction-model"
  /** 排版策略：语言→排版族路由。 */
  | "typography-policy"
  /** 字体策略：语言→字体偏好表。 */
  | "font-policy"
  /** 格式规则：日期/时间/数字/货币/百分比。 */
  | "format-rules"
  /** 复数与性别规则。 */
  | "plural-gender-rules"
  /** 密度档：按语言差异调档。 */
  | "density-policy"
  /** 输入法协同：IME 与方向、切换联动。 */
  | "ime-coordination"
  /** 测试语料与覆盖断言。 */
  | "corpus-testing"
  /** 调试与检视工具。 */
  | "debug-inspection"
  /** 读屏语言跟随与替代文本。 */
  | "screen-reader-language"
  /** 对比度与字形密度的语言分维门。 */
  | "language-split-contrast"
  /** 布局镜像与逻辑方向落地。 */
  | "layout-mirroring"
  /** 翻译工作流与文案回注。 */
  | "translation-workflow";

/** 一个分工面在某域的归属规格。 */
export interface DivisionEntry {
  /** 该域在此面上是什么角色。 */
  readonly role: "decision-owner" | "execution-owner" | "consulted" | "handoff-peer";
  /** 该域在此面上交付什么（契约名或能力描述，须可被对端机检）。 */
  readonly delivers: string;
  /** 该域在此面上明确不做什么（禁扩面写在句内，这是分工的主要价值）。 */
  readonly doesNot: string;
}

/** 一个决策面的完整分工规格：五域逐一声明 + 仲裁顺序。 */
export interface DivisionFacetSpec {
  readonly facet: DecisionFacet;
  /** 该面的中文名（对外文案引用）。 */
  readonly name: string;
  /** 逐域归属（键集恰为 PARTNER_DOMAIN_IDS，由自检机检）。 */
  readonly entries: Readonly<Record<PartnerDomainId, DivisionEntry>>;
  /**
   * 仲裁顺序（自上而下）：两个域同时主张时，先到者定调。
   * 序的排法遵循「决策优先于执行」与「语义优先于呈现」两条原则。
   */
  readonly arbitrationOrder: readonly PartnerDomainId[];
}

/**
 * 四域分工表（判据二核心资产，十四面 × 五域）。
 *
 * 为什么分工要逐面写「不做什么」而不是只写「做什么」（分工表的核心价值）：
 *   i18n 领域最贵的协调成本不是没人做，而是**两个人都做了**。典型对撞：
 *   T 域排版策略选了「按泰文无空格断词」，N 域排版实现按「按空格断词」写死——
 *   两者单测都过（各自的用例都是拉丁文），到泰语界面才炸。故本表把
 *   doesNot 当必填字段：任何一面若无「明确不做」，视为分工未定义（自检拦截）。
 *
 * 仲裁顺序为何这样排（不是随意列的）：
 *   1. VE-T 先定「长什么样」——它是唯一持有 Locale 全上下文的域；
 *   2. VE-N 再定「怎么排」——排版机制归它，但接受 T 的策略输入；
 *   3. VE-S 定「怎么念」——语义与读屏优先于视觉（视觉可错，语义错是欺骗）；
 *   4. VE-O 定「用什么字画」——字形覆盖受排版与读屏双重约束；
 *   5. VE-R 最后「谁改」——工作流不参与技术仲裁，只承接结论。
 */
export const DIVISION_TABLE: readonly DivisionFacetSpec[] = [
  {
    facet: "locale-model",
    name: "Locale 模型",
    entries: {
      "VE-T": {
        role: "decision-owner",
        delivers: "LocaleModel：解析结果、回退链、覆盖表、方向缺省策略",
        doesNot: "不实现 BCP47 库级解析的底层扫描器（复用 F2948 匹配树单源）",
      },
      "VE-N": {
        role: "consulted",
        delivers: "对回退结果在断词/连字上的可用性反馈",
        doesNot: "不另立语言标签解析实现（避免与 T 域分叉）",
      },
      "VE-O": {
        role: "handoff-peer",
        delivers: "消费 LocaleModel 查询字体偏好表",
        doesNot: "不从 DOM lang 属性自行推导语言（须走 T 域产出）",
      },
      "VE-S": {
        role: "handoff-peer",
        delivers: "消费 LocaleModel 决定读屏发音语言",
        doesNot: "不自行实现标签匹配树",
      },
      "VE-R": {
        role: "consulted",
        delivers: "对回退链提出「译文缺失以哪个语言为准」的流程约定",
        doesNot: "不参与解析语义仲裁",
      },
    },
    arbitrationOrder: ["VE-T", "VE-N", "VE-S", "VE-O", "VE-R"],
  },
  {
    facet: "direction-model",
    name: "文字方向",
    entries: {
      "VE-T": {
        role: "decision-owner",
        delivers: "DirectionModel：文档/区块/字符三层方向与仲裁表",
        doesNot: "不做 BiDi 视觉重排算法本身（由 N02 排版执行）",
      },
      "VE-N": {
        role: "execution-owner",
        delivers: "按 DirectionModel 做视觉重排与isolate 落位",
        doesNot: "不改方向判定（不按字体或段落内容重新推断方向）",
      },
      "VE-O": {
        role: "consulted",
        delivers: "报告字形的固有方向属性（供字符级兜底）",
        doesNot: "不参与三层优先级仲裁",
      },
      "VE-S": {
        role: "handoff-peer",
        delivers: "读屏按DirectionModel 朗读方向一致性",
        doesNot: "不做视觉方向决策",
      },
      "VE-R": {
        role: "consulted",
        delivers: "译文方向标注流程",
        doesNot: "不参与仲裁",
      },
    },
    arbitrationOrder: ["VE-T", "VE-N", "VE-S", "VE-O", "VE-R"],
  },
  {
    facet: "typography-policy",
    name: "排版策略",
    entries: {
      "VE-T": {
        role: "decision-owner",
        delivers: "TypographyPolicy：语言→排版族路由（Latin/CJK/阿拉伯/泰印度系）",
        doesNot: "不实现分词/整形/折行/定位/渲染五段管线（F4004 只定策略）",
      },
      "VE-N": {
        role: "execution-owner",
        delivers: "五段管线执行：分词→整形→折行→定位→渲染",
        doesNot: "不自行增删排版族（新增族须由 T 域先改路由表）",
      },
      "VE-O": {
        role: "execution-owner",
        delivers: "整形后的字形选择与光栅化",
        doesNot: "不决定分段策略",
      },
      "VE-S": {
        role: "consulted",
        delivers: "对断词导致的读屏断句问题提出约束",
        doesNot: "不做排版策略",
      },
      "VE-R": {
        role: "consulted",
        delivers: "不可断词位置的译者提示（术语完整性）",
        doesNot: "不参与技术策略",
      },
    },
    arbitrationOrder: ["VE-T", "VE-N", "VE-O", "VE-S", "VE-R"],
  },
  {
    facet: "font-policy",
    name: "字体策略",
    entries: {
      "VE-T": {
        role: "decision-owner",
        delivers: "FontPolicy：语言→字体回退链配置与顺序（F4005）",
        doesNot: "不实现字形覆盖扫描与度量对齐（单源复用 F2908/F2909）",
      },
      "VE-O": {
        role: "execution-owner",
        delivers: "字形覆盖扫描、混排基线对齐、许可地域核验（F2919）",
        doesNot: "不按DOM 内容自选字体链",
      },
      "VE-N": {
        role: "consulted",
        delivers: "折行可行性对字体宽度变化的反馈",
        doesNot: "不建字体回退链",
      },
      "VE-S": {
        role: "consulted",
        delivers: "字体缺字对读屏可读性的影响评估",
        doesNot: "不做字形选择",
      },
      "VE-R": {
        role: "consulted",
        delivers: "字体许可与译文字形的商用确认流程",
        doesNot: "不参与技术仲裁",
      },
    },
    arbitrationOrder: ["VE-T", "VE-O", "VE-N", "VE-S", "VE-R"],
  },
  {
    facet: "format-rules",
    name: "格式规则",
    entries: {
      "VE-T": {
        role: "decision-owner",
        delivers: "FormatRules：五类格式化 + 历法系统 + 时区显示规范，CLDR 锚定 v46",
        doesNot: "不实现日期库底层换算（复用宿主时区，重述 F3694 红线）",
      },
      "VE-N": { role: "consulted", delivers: "数字排版反馈", doesNot: "不做格式化" },
      "VE-O": { role: "consulted", delivers: "数字字形宽度差异反馈", doesNot: "不做格式规则" },
      "VE-S": {
        role: "handoff-peer",
        delivers: "读屏念出格式化结果的可听性校验",
        doesNot: "不自行格式化再比较",
      },
      "VE-R": {
        role: "consulted",
        delivers: "模板变量与单位表达式的译者约束",
        doesNot: "不参与规则仲裁",
      },
    },
    arbitrationOrder: ["VE-T", "VE-S", "VE-N", "VE-O", "VE-R"],
  },
  {
    facet: "plural-gender-rules",
    name: "复数与性别",
    entries: {
      "VE-T": {
        role: "decision-owner",
        delivers: "PluralGenderRules：六类复数函数表 + 语法性别模板变体",
        doesNot: "不实现模板渲染引擎（消费方提供）",
      },
      "VE-N": { role: "consulted", delivers: "复数切换后的排版影响", doesNot: "不做复数判定" },
      "VE-O": { role: "consulted", delivers: "复数形态字形可用性", doesNot: "不做复数规则" },
      "VE-S": {
        role: "handoff-peer",
        delivers: "性别与复数对代词朗读的影响校验",
        doesNot: "不自行选复数类",
      },
      "VE-R": {
        role: "consulted",
        delivers: "复数形态对译文的上下文要求（译者 notes）",
        doesNot: "不参与技术仲裁",
      },
    },
    arbitrationOrder: ["VE-T", "VE-S", "VE-N", "VE-O", "VE-R"],
  },
  {
    facet: "density-policy",
    name: "排版密度",
    entries: {
      "VE-T": {
        role: "decision-owner",
        delivers: "DensityPolicy：按语言差异调整密度档（变体单源复用 F3442）",
        doesNot: "不实现密度档到像素的换算",
      },
      "VE-O": { role: "consulted", delivers: "字体度量对密度档的适配", doesNot: "不定义密度档" },
      "VE-N": { role: "execution-owner", delivers: "密度档落到排版度量", doesNot: "不改档位表" },
      "VE-S": {
        role: "consulted",
        delivers: "密度压缩对字号可读性的影响（域本色硬门）",
        doesNot: "不定义密度档",
      },
      "VE-R": { role: "consulted", delivers: "膨胀率提示供译者预留长度", doesNot: "不参与仲裁" },
    },
    arbitrationOrder: ["VE-T", "VE-S", "VE-N", "VE-O", "VE-R"],
  },
  {
    facet: "ime-coordination",
    name: "输入法协同",
    entries: {
      "VE-T": {
        role: "decision-owner",
        delivers: "ImeCoordination：方向×输入协同、语言切换→输入法联动断言（F4009）",
        doesNot: "不实现输入法引擎（宿主/ N03 提供）",
      },
      "VE-N": { role: "execution-owner", delivers: "组合期渲染协同通道", doesNot: "不做切换联动决策" },
      "VE-O": { role: "consulted", delivers: "候选窗字形渲染", doesNot: "不做协同" },
      "VE-S": {
        role: "handoff-peer",
        delivers: "组合期不触发快捷键的禁令校验（单源复用 F3024）",
        doesNot: "不做输入法生命周期管理",
      },
      "VE-R": { role: "consulted", delivers: "译文禁用字与输入冲突清单", doesNot: "不参与仲裁" },
    },
    arbitrationOrder: ["VE-T", "VE-S", "VE-N", "VE-O", "VE-R"],
  },
  {
    facet: "corpus-testing",
    name: "测试语料",
    entries: {
      "VE-T": {
        role: "decision-owner",
        delivers: "CorpusKit：全支持语言样文 + 边界样文 + 覆盖断言（F4010）",
        doesNot: "不使用真实用户语料（隐私：全部合成或授权样文）",
      },
      "VE-N": { role: "execution-owner", delivers: "以语料执行断词/折行回归", doesNot: "不建语料库" },
      "VE-O": { role: "execution-owner", delivers: "以语料执行字形覆盖回归", doesNot: "不建语料库" },
      "VE-S": { role: "execution-owner", delivers: "以语料执行读屏回归", doesNot: "不建语料库" },
      "VE-R": { role: "consulted", delivers: "语料更新纳入译词变更流程", doesNot: "不生成合成语料" },
    },
    arbitrationOrder: ["VE-T", "VE-N", "VE-S", "VE-O", "VE-R"],
  },
  {
    facet: "debug-inspection",
    name: "调试检视",
    entries: {
      "VE-T": {
        role: "decision-owner",
        delivers: "I18nInspector：Locale/方向/格式三维检视 + 伪本地化引擎（F4011）",
        doesNot: "常态不开启（开销仅在打开时采样，复述家族零常态纪律）",
      },
      "VE-N": { role: "execution-owner", delivers: "方向树挂可视化", doesNot: "不做伪本地化" },
      "VE-O": { role: "consulted", delivers: "字体回退链展示数据", doesNot: "不建调试器" },
      "VE-S": { role: "consulted", delivers: "读屏语义树展示", doesNot: "不建调试器" },
      "VE-R": { role: "consulted", delivers: "硬编码检出纳入评审清单", doesNot: "不做工具" },
    },
    arbitrationOrder: ["VE-T", "VE-N", "VE-O", "VE-S", "VE-R"],
  },
  {
    facet: "screen-reader-language",
    name: "读屏语言跟随",
    entries: {
      "VE-T": {
        role: "decision-owner",
        delivers: "投影携带 lang；语言切换时语义树lang 同步策略（联动 F3856）",
        doesNot: "不实现读屏引擎与语音合成",
      },
      "VE-S": {
        role: "execution-owner",
        delivers: "读屏按投影 lang 选发音规则",
        doesNot: "不改投影的 lang 值",
      },
      "VE-O": { role: "consulted", delivers: "lang→字体偏好", doesNot: "不发音" },
      "VE-N": { role: "consulted", delivers: "lang 对断句的影响", doesNot: "不发音" },
      "VE-R": { role: "consulted", delivers: "lang 标注的译者约束", doesNot: "不发音" },
    },
    arbitrationOrder: ["VE-T", "VE-S", "VE-O", "VE-N", "VE-R"],
  },
  {
    facet: "language-split-contrast",
    name: "语言分维对比度",
    entries: {
      "VE-T": {
        role: "decision-owner",
        delivers: "按语言分维的对比度门（CJK 字形密→要求更高，F4015）",
        doesNot: "不实现色彩合成与对比度计算内核（单源复用 O域 F2895）",
      },
      "VE-S": {
        role: "execution-owner",
        delivers: "无障碍基线的读屏与放大协同校验",
        doesNot: "不自建对比度公式",
      },
      "VE-O": {
        role: "consulted",
        delivers: "字形覆盖率对对比度对比的影响",
        doesNot: "不定门",
      },
      "VE-N": { role: "consulted", delivers: "行距对密排字形可辨性的反馈", doesNot: "不定门" },
      "VE-R": { role: "consulted", delivers: "译文不承载颜色语义（避免用颜色表意）", doesNot: "不定门" },
    },
    arbitrationOrder: ["VE-T", "VE-S", "VE-O", "VE-N", "VE-R"],
  },
  {
    facet: "layout-mirroring",
    name: "布局镜像",
    entries: {
      "VE-T": {
        role: "decision-owner",
        delivers: "逻辑方向约定：起止边、内边距顺序、滚动方向（决策而非像素）",
        doesNot: "不做布局实现（布局归渲染域），不定具体像素",
      },
      "VE-N": { role: "execution-owner", delivers: "断行与折行的方向相关度量", doesNot: "不定义逻辑边" },
      "VE-O": { role: "consulted", delivers: "字形固有方向反馈", doesNot: "不定义逻辑边" },
      "VE-S": {
        role: "consulted",
        delivers: "焦点顺序与视觉顺序一致性校验",
        doesNot: "不定义逻辑边",
      },
      "VE-R": { role: "consulted", delivers: "译文中的双向文本约束（禁裸 RTL 覆写字符）", doesNot: "不参与仲裁" },
    },
    arbitrationOrder: ["VE-T", "VE-S", "VE-N", "VE-O", "VE-R"],
  },
  {
    facet: "translation-workflow",
    name: "翻译工作流",
    entries: {
      "VE-T": {
        role: "consulted",
        delivers: "字符串外部化扫描口径（复用 F4011 伪检单源）",
        doesNot: "不建设翻译管理平台（归 R 域）",
      },
      "VE-R": {
        role: "decision-owner",
        delivers: "外部化→翻译→回注工作流；无translators notes 阻断提交",
        doesNot: "不做技术扫描与格式校验",
      },
      "VE-N": { role: "consulted", delivers: "占位符断行安全性约束", doesNot: "不建流程" },
      "VE-O": { role: "consulted", delivers: "译文所需字形集合反馈", doesNot: "不建流程" },
      "VE-S": {
        role: "consulted",
        delivers: "译文可朗读性标注",
        doesNot: "不建流程",
      },
    },
    arbitrationOrder: ["VE-R", "VE-T", "VE-N", "VE-S", "VE-O"],
  },
];

/** 决策面 id 全集（遍历用）。 */
export const DECISION_FACETS: readonly DecisionFacet[] = DIVISION_TABLE.map((f) => f.facet);

/** 查分工面规格；未登记返回显性失败。 */
export function lookupFacet(facet: string): Outcome<DivisionFacetSpec> {
  const spec = DIVISION_TABLE.find((f) => f.facet === facet);
  if (spec === undefined) {
    return fail(
      "DIVISION_GAP",
      `决策面 ${facet} 不在四域分工表中`,
      `已登记决策面（${DECISION_FACETS.length} 面）：${DECISION_FACETS.join("、")}；`
        + "新增决策面须先在 DIVISION_TABLE 中逐域声明角色与不做什么",
    );
  }
  return ok(spec);
}

// ════════════════════════════════════════════════════════════════════════════
// §4 十项映射（判据三）
// ════════════════════════════════════════════════════════════════════════════

/** 十项映射 id（封闭集：判据三的十项，新增须改本类型与 MAPPING_REGISTRY 两处）。 */
export type MappingId =
  | "locale"
  | "direction"
  | "typography"
  | "font"
  | "format"
  | "plural"
  | "density"
  | "ime"
  | "testing"
  | "debugging";

/** 十项映射的官方中文名序列（对外表述，顺序即判据顺序）。 */
export const OFFICIAL_MAPPINGS: readonly string[] = [
  "Locale",
  "方向",
  "排版",
  "字体",
  "格式",
  "复数",
  "密度",
  "输入法",
  "测试",
  "调试",
];

/** 一项映射的完整规格（十项逐项登记，无一项靠约定）。 */
export interface MappingSpec {
  readonly id: MappingId;
  /** 映射中文名（与 OFFICIAL_MAPPINGS 逐项一致，由自检机检）。 */
  readonly name: string;
  /** 映射的键（输入侧）：什么决定这条映射的结果。 */
  readonly key: string;
  /** 映射的值（输出侧）：这个键决定出什么界面决策。 */
  readonly value: string;
  /** 承担该映射的条目（册内条目号 + 标题，可机检引用）。 */
  readonly owner: string;
  /** 产出契约名（全局唯一，由自检机检撞名）。 */
  readonly producesContract: string;
  /** 消费该映射的对端（域 id 或条目号），供越界检查。 */
  readonly consumedBy: readonly string[];
  /** 该映射的不可越界声明：做别的事即视为扩面（判据要求逐项写死）。 */
  readonly mustNot: string;
  /** 该映射依赖的架构立场 id（呼应 §2，无立场支撑的映射视为无根）。 */
  readonly groundedBy: readonly ArchitecturePillarId[];
}

/**
 * 十项映射注册表。判据三要求逐项登记键/值/归属/契约/禁扩面，此处即唯一事实源。
 *
 * 键值二元组的写法（本域的关键抽象，解释一句）：
 *   国际化不是「翻译表」，而是十张「键 → 值」表。键恒为 Locale 上下文
 *   （语言、方向、区域、历法、书写系统之一），值恒为界面决策（排版族、字体链、
 *   密度档、格式规则集、复数函数…）。任何界面元素在渲染前，必须先经过
 *   十张表把它的 Locale 上下文查成决策——这就是「global-first」在代码里的形状。
 */
export const MAPPING_REGISTRY: Readonly<Record<MappingId, MappingSpec>> = {
  locale: {
    id: "locale",
    name: "Locale",
    key: "BCP47 语言标签（language[-script][-region][-variant][-extension]）",
    value: "LocaleModel：语言/文字系统/区域/历法偏好/方向缺省 + 回退链",
    owner: "VE-F4002 语言标签与 Locale 模型",
    producesContract: "LocaleModel",
    consumedBy: ["VE-N", "VE-O", "VE-S", "F4011", "F4005", "F4006"],
    mustNot: "不在本域实现 BCP47 底层扫描器；不自建第二份匹配树（F2948 单源）",
    groundedBy: ["clamped-parameter-domain", "single-source-discipline"],
  },
  direction: {
    id: "direction",
    name: "方向",
    key: "文档级 / 区块级 / 字符级三层声明",
    value: "DirectionModel：LTR / RTL / TTB 三方向统一模型与仲裁结果",
    owner: "VE-F4003 文字方向模型",
    producesContract: "DirectionModel",
    consumedBy: ["VE-N", "VE-S", "F4009", "F4011"],
    mustNot: "不做 BiDi 视觉重排算法；不按字体重新推断方向（会与声明冲突）",
    groundedBy: ["clamped-parameter-domain", "decision-execution-split"],
  },
  typography: {
    id: "typography",
    name: "排版",
    key: "语言 → 排版族路由",
    value: "TypographyPolicy：Latin / CJK / 阿拉伯 / 泰印度系 四族策略",
    owner: "VE-F4004 国际化排版管线",
    producesContract: "TypographyPolicy",
    consumedBy: ["VE-N"],
    mustNot: "不执行分词/整形/折行/定位/渲染五段管线（N 域执行）；未收录语言不得静默降级",
    groundedBy: ["decision-execution-split", "degradation-must-be-visible"],
  },
  font: {
    id: "font",
    name: "字体",
    key: "语言 → 字体回退链配置",
    value: "FontPolicy：有序回退链 + 缺字下一跳 + 混排基线约束",
    owner: "VE-F4005 字体国际化选型",
    producesContract: "FontPolicy",
    consumedBy: ["VE-O", "F4013"],
    mustNot: "不实现覆盖扫描/度量对齐/许可核验（F2908/F2909/F2919 单源复用）",
    groundedBy: ["single-source-discipline", "degradation-must-be-visible"],
  },
  format: {
    id: "format",
    name: "格式",
    key: "Locale + 历法 + 时区 + 数值原值",
    value: "FormatRules：日期/时间/数字/货币/百分比五类格式化产物",
    owner: "VE-F4006 日期时间数字格式",
    producesContract: "FormatRules",
    consumedBy: ["VE-S", "N02", "F4007"],
    mustNot: "不实现日期库底层换算；不随宿主时区漂移（F3694 红线复用）",
    groundedBy: ["single-source-discipline", "clamped-parameter-domain"],
  },
  plural: {
    id: "plural",
    name: "复数",
    key: "Locale + 数量 + 语法性别",
    value: "PluralGenderRules：六类复数类别 + 性别模板变体选择结果",
    owner: "VE-F4007 复数与性别规则",
    producesContract: "PluralGenderRules",
    consumedBy: ["VE-N", "VE-S"],
    mustNot: "不实现模板渲染引擎；未知语言不得静默回落other（须显性）",
    groundedBy: ["degradation-must-be-visible", "single-source-discipline"],
  },
  density: {
    id: "density",
    name: "密度",
    key: "语言 → 密度档（按文本膨胀与书写习惯）",
    value: "DensityPolicy：紧凑/标准/舒展三档 + 膨胀缓冲余量",
    owner: "VE-F4008 排版密度与语言",
    producesContract: "DensityPolicy",
    consumedBy: ["VE-N", "VE-S"],
    mustNot: "不实现密度到像素的换算；不改F3442 变体单源；UI 必须容忍 +35% 膨胀",
    groundedBy: ["no-language-is-second-class", "single-source-discipline"],
  },
  ime: {
    id: "ime",
    name: "输入法",
    key: "语言切换事件 + 方向上下文",
    value: "ImeCoordination：输入法联动结果（组合期通道/候选窗跟随/切换联动）",
    owner: "VE-F4009 输入法协同",
    producesContract: "ImeCoordination",
    consumedBy: ["N03", "VE-N"],
    mustNot: "不实现输入法引擎；组合期不得触发快捷键（F3024 红线单源）",
    groundedBy: ["decision-execution-split", "degradation-must-be-visible"],
  },
  testing: {
    id: "testing",
    name: "测试",
    key: "语言/区域/方向维度",
    value: "CorpusKit：样文 + 边界样文 + 覆盖断言结果",
    owner: "VE-F4010 国际化测试语料",
    producesContract: "CorpusKit",
    consumedBy: ["VE-N", "VE-O", "VE-S", "F4019"],
    mustNot: "不使用真实用户语料；无语料的语言不得声明为已支持（无料红线）",
    groundedBy: ["architectural-not-end-stage", "single-source-discipline"],
  },
  debugging: {
    id: "debugging",
    name: "调试",
    key: "Locale / 方向 / 格式三维运行态",
    value: "I18nInspector：五列仪表 + 伪本地化检出结果 + 方向可视化",
    owner: "VE-F4011 国际化调试器",
    producesContract: "I18nInspector",
    consumedBy: ["F4019", "VE-R"],
    mustNot: "常态不开启（零常态开销纪律）；不替代单测与硬门",
    groundedBy: ["architectural-not-end-stage", "degradation-must-be-visible"],
  },
};

/** 映射 id 全集（遍历用）。 */
export const MAPPING_IDS: readonly MappingId[] = Object.keys(MAPPING_REGISTRY) as MappingId[];

/** 查映射规格；未登记返回显性失败。 */
export function lookupMapping(id: string): Outcome<MappingSpec> {
  const table = MAPPING_REGISTRY as Record<string, MappingSpec | undefined>;
  const spec = table[id];
  if (spec === undefined) {
    return fail(
      "MAPPING_MISSING",
      `映射 ${id} 不在十项映射注册表中`,
      `已登记映射（${MAPPING_IDS.length} 项）：${MAPPING_IDS.join("、")}；`
        + "新增映射须同时改 MappingId 类型与 MAPPING_REGISTRY，且须绑定至少一条架构立场",
    );
  }
  return ok(spec);
}

// ════════════════════════════════════════════════════════════════════════════
// §5 架构五段 · 签名冻结 v1（判据四）
// ════════════════════════════════════════════════════════════════════════════

/** 五段序（串行有向：段 N 的输入必须是段 N-1 的产出契约）。顺序即契约。 */
export type SegmentId = "locale-model" | "direction" | "typography" | "format" | "delivery";

/** 五段序的规范序列（自检以此机检段序，不可变乱）。 */
export const SEGMENT_ORDER: readonly SegmentId[] = [
  "locale-model",
  "direction",
  "typography",
  "format",
  "delivery",
];

/** 一段的签名规格（逐段定契约：输入/输出/承接映射/性能量级）。 */
export interface SegmentSignature {
  readonly id: SegmentId;
  /** 段序（第几段，1 起）。 */
  readonly ordinal: number;
  /** 段中文名。 */
  readonly name: string;
  /**
   * 输入契约名列表。首段为空（无上游）；其余段的输入必须**恰好等于**
   * 上游段的输出契约名——不一致即 SEGMENT_CHAIN_MISMATCH（对拍）。
   */
  readonly inputContracts: readonly string[];
  /** 输出契约名（唯一；下游段按此名引用，禁止读全局变量旁路取值）。 */
  readonly outputContract: string;
  /** 本段消费的十项映射 id（呼应 §4，无映射支撑的段视为无根）。 */
  readonly consumesMappings: readonly MappingId[];
  /** 承担本段的条目（册内条目号 + 标题）。 */
  readonly owner: string;
  /** 性能量级声明（锚点「性能逐项分解」：五段 O(1) 每段摊销）。 */
  readonly performance: string;
  /** 本段明确不做的事（禁扩面）。 */
  readonly mustNot: string;
  /** 签名版本（五段须同版本，由 validateSegmentSignatures 机检）。 */
  readonly version: string;
}

/**
 * 五段签名表 v1（判据四核心资产，已冻结）。
 *
 * 为什么必须是串行五段而不是一个「i18n 大函数」（架构立场，最易被简化掉的地方）：
 *   国际化的错误几乎都是**跨段的**，而跨段错误在合并的大函数里无法被分段断言。
 *   例：Locale 解析出 zh-CN（段1）→ 方向判为 LTR（段2）→ 排版按 CJK 断词（段3）
 *   → 数字按中文习惯分组（段4）。若这四步在一个函数里，则当有人把段2 改成
 *   「按段落首字符启发推断方向」时，段3/段4 的单测**全绿**（它们喂的是
 *   正确方向的输入），只有真实阿拉伯语界面才出错。串行分段的价值就在这里：
 *   每段的产出是**唯一具名契约**，段间关系可被 auditSegmentChain 机检——
 *   谁从全局变量旁路取值、谁引用了别人的产出、谁把段序接反，都会被拦下。
 *
 * 段序不可调换的理由（写下来防止后来者「优化」）：
 *   段2 依赖段1（方向缺省值由 Locale 决定）；段3 依赖段2（断词策略随方向）；
 *   段4 依赖段3（数字与日期的排布受行宽与方向约束）；段5 依赖全部（交付要打包）。
 *   反向依赖即 SEGMENT_ORDER_INVERTED。
 */
export const SEGMENT_SIGNATURES: Readonly<Record<SegmentId, SegmentSignature>> = {
  "locale-model": {
    id: "locale-model",
    ordinal: 1,
    name: "Locale 模型段",
    inputContracts: [],
    outputContract: "LocaleModel",
    consumesMappings: ["locale"],
    owner: "VE-F4002 语言标签与 Locale 模型",
    performance: "O(标签长) 解析、缓存后 O(1)",
    mustNot: "不产出方向判定（属段2）；不实现 BCP47 底层扫描器（F2948 单源）",
    version: "v1",
  },
  direction: {
    id: "direction",
    ordinal: 2,
    name: "方向段",
    inputContracts: ["LocaleModel"],
    outputContract: "DirectionModel",
    consumesMappings: ["direction"],
    owner: "VE-F4003 文字方向模型",
    performance: "O(1) 查表 + O(文本采样) 首强字符启发",
    mustNot: "不做 BiDi 视觉重排（N02 执行）；启发结果必须可被显式声明覆盖",
    version: "v1",
  },
  typography: {
    id: "typography",
    ordinal: 3,
    name: "排版策略段",
    inputContracts: ["DirectionModel"],
    outputContract: "TypographyPolicy",
    consumesMappings: ["typography", "font", "density"],
    owner: "VE-F4004 国际化排版管线",
    performance: "O(1) 路由查表；策略展开 O(语言)",
    mustNot: "不执行五段管线（N02 执行）；未收录语言→Latin 降级必须显性标记",
    version: "v1",
  },
  format: {
    id: "format",
    ordinal: 4,
    name: "格式段",
    inputContracts: ["TypographyPolicy"],
    outputContract: "FormatRules",
    consumesMappings: ["format", "plural"],
    owner: "VE-F4006 日期时间数字格式 + VE-F4007 复数性别",
    performance: "O(1) 缓存命中；格式化 O(字段数)",
    mustNot: "不实现日期库底层换算；历法转换失败→公历回退必须显性",
    version: "v1",
  },
  delivery: {
    id: "delivery",
    ordinal: 5,
    name: "交付段",
    inputContracts: ["FormatRules"],
    outputContract: "I18nDelivery",
    consumesMappings: ["ime", "testing", "debugging"],
    owner: "VE-F4017 国际化 API 冻结 + VE-F4011 调试器",
    performance: "O(1) 读取已聚合决策",
    mustNot: "不在交付段做翻译回填（归 R 域工作流）；不带未诊断的降级状态交付",
    version: "v1",
  },
};

/** 段 id 全集（遍历用）。 */
export const SEGMENT_IDS: readonly SegmentId[] = SEGMENT_ORDER.slice();

/** 查段签名；未登记返回显性失败。 */
export function lookupSegment(id: string): Outcome<SegmentSignature> {
  const table = SEGMENT_SIGNATURES as Record<string, SegmentSignature | undefined>;
  const spec = table[id];
  if (spec === undefined) {
    return fail(
      "SEGMENT_UNREGISTERED",
      `架构段 ${id} 不在五段签名表中`,
      `已登记段（${SEGMENT_IDS.length} 段）：${SEGMENT_IDS.join(" → ")}；`
        + "新增段须先改 SEGMENT_ORDER 与 SEGMENT_SIGNATURES 两处，并补齐上下游 inputContracts",
    );
  }
  return ok(spec);
}

/**
 * 段间失配对拍（判据四的「段间失配→对拍」错误路径）：
 * 逐段核对三件事，任一不成立即产出诊断——
 *   1. 段序与 SEGMENT_ORDER 完全一致（ordinal 恰为 1..5，不倒置）；
 *   2. 段 N 的 inputContracts **恰好**等于段 N-1 的 outputContract（不多不少）；
 *   3. 五段版本号一致（版本不齐=对不上版，对拍无意义）。
 *
 * 第 2 项要求「恰好等于」而非「包含」是刻意的：多一个输入即意味着该段偷偷
 * 依赖了别的段的产出或全局状态，而那正是段间失配的成因。允许「包含」等于
 * 允许旁路——本域禁的正是旁路。
 */
export function auditSegmentChain(bag: DiagBag): Outcome<readonly SegmentId[]> {
  // 1) 段序一致性。
  for (let i = 0; i < SEGMENT_ORDER.length; i += 1) {
    const id = SEGMENT_ORDER[i] as SegmentId;
    const spec = SEGMENT_SIGNATURES[id];
    if (spec === undefined) {
      bag.push(
        "SEGMENT_UNREGISTERED",
        `SEGMENT_ORDER 中的 ${id} 在签名表中无登记`,
        "五段必须两侧同时登记；请在 SEGMENT_SIGNATURES 中补齐该段",
      );
      continue;
    }
    if (spec.ordinal !== i + 1) {
      bag.push(
        "SEGMENT_ORDER_INVERTED",
        `段 ${spec.name} 声明 ordinal=${spec.ordinal}，但它在 SEGMENT_ORDER 中位于第 ${i + 1} 位`,
        "段序是契约（方向依赖 Locale、排版依赖方向、格式依赖排版、交付依赖格式）；"
          + "请修正 ordinal 或调整 SEGMENT_ORDER，二者必须同序，不得只改一侧",
      );
    }
  }

  // 2) 上下游契约恰好匹配 + 3) 版本一致。
  let versionSeen: string | null = null;
  for (let i = 0; i < SEGMENT_ORDER.length; i += 1) {
    const id = SEGMENT_ORDER[i] as SegmentId;
    const spec = SEGMENT_SIGNATURES[id];
    if (spec === undefined) continue;

    if (versionSeen === null) {
      versionSeen = spec.version;
    } else if (spec.version !== versionSeen) {
      bag.push(
        "SEGMENT_VERSION_MISMATCH",
        `段 ${spec.name} 的签名版本是 ${spec.version}，而首段是 ${versionSeen}`,
        `本域签名已冻结为 ${DOMAIN.architectureSignatureVersion}；`
          + "版本不齐意味着上下游按不同契约实现，对拍结果无法判定归属。"
          + "请把五段版本统一，或走 ADR 说明为什么必须并存两版",
      );
    }

    if (i === 0) {
      if (spec.inputContracts.length !== 0) {
        bag.push(
          "SEGMENT_CHAIN_MISMATCH",
          `首段 ${spec.name} 声明了上游输入 ${spec.inputContracts.join("、")}，但首段无上游`,
          "首段输入必须为空列表；若有输入，说明它绕过了 Locale 模型直接取全局状态"
            + "（这正是段间失配的典型成因）",
        );
      }
      continue;
    }

    const upstream = SEGMENT_SIGNATURES[SEGMENT_ORDER[i - 1] as SegmentId];
    if (upstream === undefined) continue;
    const want = [upstream.outputContract].sort();
    const actual = spec.inputContracts.slice().sort();
    if (actual.length !== want.length || actual.some((v, k) => v !== want[k])) {
      bag.push(
        "SEGMENT_CHAIN_MISMATCH",
        `段 ${spec.name} 的输入是 [${actual.join("、") || "空"}]，`
          + `而上游段 ${upstream.name} 只产出 [${want.join("、")}]`,
        "每段的输入必须恰好是上游段的唯一产出契约（不多不少）。"
          + "多出的输入通常意味着该段读了全局变量或别的段的内部字段——"
          + "请改为显式引用上游产出契约，否则段间一致性无法被对拍保证",
      );
    }
  }
  return ok(SEGMENT_IDS, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §6 参数域钳制与枚举守卫（判据：钳制 + 枚举守卫）
// ════════════════════════════════════════════════════════════════════════════

/** 文字方向枚举（三方向统一模型，F4003 的封闭集）。 */
export type Direction = "ltr" | "rtl" | "ttb";

/** 方向合法值全集（枚举守卫的事实源）。 */
export const DIRECTIONS: readonly Direction[] = ["ltr", "rtl", "ttb"];

/** 排版族（四族路由，F4004 的封闭集）。 */
export type ScriptFamily = "latin" | "cjk" | "arabic" | "thai-indic";

/** 排版族合法值全集。 */
export const SCRIPT_FAMILIES: readonly ScriptFamily[] = ["latin", "cjk", "arabic", "thai-indic"];

/** 密度档（三档，F4008 的封闭集）。 */
export type DensityTier = "compact" | "standard" | "spacious";

/** 密度档合法值全集。 */
export const DENSITY_TIERS: readonly DensityTier[] = ["compact", "standard", "spacious"];

/**
 * BCP47 语言子标签的最小合法文法（本域参数域，不是完整 BCP47 实现）：
 *   language= 2~3 位字母（可选 3 位扩展简写）
 *   script     = 4 位字母
 *   region     = 2 位字母 | 3 位数字
 *   variant    = 5~8 位字母数字 | 数字 + 3 位字母数字
 * 本域只做**钳制与守卫**（判据要求「参数域钳制、枚举守卫」），
 * 完整解析语义归 F4002/F2948 单源，本条不另立第二份文法实现。
 */
const LANGUAGE_RE = /^[a-z]{2,3}(-[a-z]{3})?$/;
const SCRIPT_RE = /^[a-z]{4}$/;
const REGION_RE = /^([a-z]{2}|[0-9]{3})$/;
const VARIANT_RE = /^([a-z0-9]{5,8}|[0-9][a-z0-9]{3})$/;

/** 钳制后的 Locale 上下文字段（各字段均已过文法/枚举校验）。 */
export interface ClampedLocale {
  /** 规范化后的小写语言子标签。 */
  readonly language: string;
  /** 四位文字系统子脚本（小写），无则为空串。 */
  readonly script: string;
  /** 两位字母或三位数字区域，无则为空串。 */
  readonly region: string;
  /** 变体子标签序列（可空）。 */
  readonly variants: readonly string[];
  /** 规范化后的完整标签（段5 交付用，拼接顺序固定）。 */
  readonly canonical: string;
}

/**
 * Locale 标签钳制（架构立场④的落点）：把任意输入钳制成合法 Locale 上下文。
 *
 * 三要素拒绝口径（锚点错误路径「Locale 非法→拒绝三要素」）：
 *   非法标签一律**拒绝**（返回 Outcome 失败），不做静默修正为 und——
 *   静默修正会让「我拼错了 zh-CN 和 zh_CN」这种问题活到用户看见为止。
 *   容错（畸形标签的容忍）由 F4002 的容错表负责并显性诊断，本条只管钳制。
 *
 * 为什么允许 `_` 分隔符（工程现实）：
 *   POSIX 习惯用 zh_CN，Windows 区域设置用 zh-CN，DOM lang 用 zh-CN。
 *   三种写法都会流进同一个入口，若第一道就拒，用户会看到「应用不认中文」。
 *   故先把 `_` 统一为 `-`（这是一次**无损**的规范化，不是掩盖错误），
 *   再对非法形态拒绝。
 */
export function clampLocaleTag(raw: string, bag: DiagBag): Outcome<ClampedLocale> {
  if (typeof raw !== "string" || raw.trim() === "") {
    bag.push(
      "LOCALE_ILLEGAL",
      "Locale 标签为空或非字符串",
      "Locale 是国际化架构的第一入口（段1），空值意味着调用方漏传或读环境失败；"
        + "请在调用点补默认值（例如宿主偏好 → 'en-US'），而不是让空值继续流",
    );
    return fail("LOCALE_ILLEGAL", "Locale 标签为空或非字符串", "补默认值并确认来源；国际化架构不接受空 Locale");
  }

  // 无损规范化：下划线→连字符、去首尾空白、折叠重复连字符。
  const norm = raw.trim().replace(/_/g, "-").replace(/-{2,}/g, "-").toLowerCase();

  const parts = norm.split("-").filter((p) => p !== "");
  if (parts.length === 0) {
    bag.push(
      "LOCALE_ILLEGAL",
      `Locale 标签「${raw}」规范化后不含任何子标签`,
      "标签须至少含一个语言子标签（如 en / zh / ar）；请检查拼接逻辑是否产出了只有分隔符的串",
    );
    return fail("LOCALE_ILLEGAL", `Locale 标签「${raw}」不含子标签`, "至少提供语言子标签，如 zh-CN");
  }

  // 语言子标签：首段必须是 language。
  const language = parts[0] as string;
  if (!LANGUAGE_RE.test(language)) {
    bag.push(
      "LOCALE_ILLEGAL",
      `Locale 标签「${raw}」的语言子标签「${language}」不符合文法（须 2~3 位字母）`,
      "本域接受 zh / en / ar / th 这类语言子标签。"
        + "若你传的是文字系统或区域（如 Hans / CN），请按 language[-script][-region] 顺序重排",
    );
    return fail(
      "LOCALE_ILLEGAL",
      `语言子标签「${language}」非法`,
      "须为 2~3 位字母；文字系统与区域请放在其后",
    );
  }

  let idx = 1;
  let script = "";
  let region = "";
  const variants: string[] = [];

  // 文字系统：四位字母，位置固定在语言之后。
  const maybeScript = parts[idx];
  if (maybeScript !== undefined && SCRIPT_RE.test(maybeScript)) {
    script = maybeScript;
    idx += 1;
  }

  // 区域：两位字母或三位数字。
  const maybeRegion = parts[idx];
  if (maybeRegion !== undefined && REGION_RE.test(maybeRegion)) {
    region = maybeRegion;
    idx += 1;
  }

  // 其余按 variant / extension 处理：variant 须合文法，单字母视作 extension
  // （extension 语义归 F4002，本域只做形态钳制并保留原文）。
  for (; idx < parts.length; idx += 1) {
    const p = parts[idx] as string;
    if (VARIANT_RE.test(p)) {
      variants.push(p);
    } else if (/^[a-z0-9]{1,8}$/.test(p)) {
      variants.push(p);
    } else {
      bag.push(
        "LOCALE_ILLEGAL",
        `Locale 标签「${raw}」的子标签「${p}」既非合法 variant 也非合法 extension`,
        "子标签只允许字母与数字且长度 1~8；若你使用的是私有扩展（x-…），"
          + "请确认没有把 'x' 单独写成一段",
      );
      return fail(
        "LOCALE_ILLEGAL",
        `子标签「${p}」非法`,
        "只允许字母数字、长度 1~8；私有扩展请写成 x-foo 而非被拆段",
      );
    }
  }

  const canonicalParts = [language];
  if (script !== "") canonicalParts.push(script);
  if (region !== "") canonicalParts.push(region);
  for (const v of variants) canonicalParts.push(v);
  const canonical = canonicalParts.join("-");

  return ok({ language, script, region, variants, canonical }, bag.all());
}

/**
 * 方向枚举守卫：校验方向取值合法；缺失时**默认 LTR 并产出显性诊断**。
 *
 * 为什么缺失方向要「默认 + 诊断」而不是「拒绝」（锚点错误路径原文）：
 *   方向缺失通常是调用方还没接完（文档级方向声明尚未写入），此时拒绝会让整个
 *   界面起不来，用户看到的是白屏而不是「方向未定」。默认 LTR + 一条诊断，
 *   让界面先以 LTR 起来，同时把问题显性暴露给排障者——这条诊断必须能上日志、
 *   能上报，不能只留在本地被吞掉（呼应零静默纪律）。
 *
 * 反向案例（这才是真事故）：某组件静默默认 LTR 而无诊断，团队据此认为「方向
 * 链路已通」，直到阿拉伯语界面上整块布局反转才被发现。故本函数的默认分支
 * **必定**产出诊断，调用方无法在不接诊断的情况下拿到结果。
 */
export function clampDirection(raw: string | undefined, bag: DiagBag): Outcome<Direction> {
  if (raw === undefined || raw === "") {
    bag.push(
      "DIRECTION_MISSING_DEFAULTED",
      "方向声明缺失，已默认 LTR（左起）",
      "请在文档级方向声明中显式给出方向（段2 的输入）。"
        + "若你的界面确实是左起语言，请显式写 'ltr' 而非留空——"
        + "留空会让「忘配」与「配成左起」两种情形无法区分",
    );
    return ok("ltr", bag.all());
  }
  const norm = raw.trim().toLowerCase();
  if (!(DIRECTIONS as readonly string[]).includes(norm)) {
    bag.push(
      "DIRECTION_INVALID",
      `方向取值「${raw}」不在三方向封闭集内`,
      `合法值：${DIRECTIONS.join(" / ")}。`
        + "注意「竖排」在本域是方向 ttb（top-to-bottom），与 writing-mode 属性是两回事，"
        + "请不要把 css 写法传进本域",
    );
    return fail("DIRECTION_INVALID", `方向取值「${raw}」非法`, `合法值：${DIRECTIONS.join(" / ")}`);
  }
  return ok(norm as Direction, bag.all());
}

/** 排版族枚举守卫。 */
export function clampScriptFamily(raw: string, bag: DiagBag): Outcome<ScriptFamily> {
  const norm = raw.trim().toLowerCase();
  if (!(SCRIPT_FAMILIES as readonly string[]).includes(norm)) {
    return fail(
      "LOCALE_ILLEGAL",
      `排版族「${raw}」不在四族封闭集内`,
      `合法值：${SCRIPT_FAMILIES.join(" / ")}。`
        + "若语言未收录，正确的行为是显性降级到 latin 并打降级标记（F4004 降级显性红线），"
        + "而不是编一个新族名——新族名会让下游拿不到对应策略",
    );
  }
  return ok(norm as ScriptFamily, bag.all());
}

/** 密度档枚举守卫。 */
export function clampDensityTier(raw: string, bag: DiagBag): Outcome<DensityTier> {
  const norm = raw.trim().toLowerCase();
  if (!(DENSITY_TIERS as readonly string[]).includes(norm)) {
    return fail(
      "LOCALE_ILLEGAL",
      `密度档「${raw}」不在三档封闭集内`,
      `合法值：${DENSITY_TIERS.join(" / ")}。`
        + "密度档由语言差异驱动（Arabic 舒展 / CJK 紧凑，见 F4008），"
        + "请让语言决定档位，而不是让调用方随手指定",
    );
  }
  return ok(norm as DensityTier, bag.all());
}

/**
 * 架构声明守卫：校验某模块对 i18n 的立场描述没有违反 global-first。
 *
 * 用法：任何模块在写README / 架构说明 / 代码评审自述之前，把立场描述文本
 * 过一遍本守卫。命中即开发期失败（呼应 F2201 域的诚实语义守卫同构做法）。
 *
 * 为什么拦「否认国际化是架构级」这种话（这道闸门的实际价值）：
 *   常见的绕过写法是「本模块只做渲染，国际化不归我管」。听起来合理，实际后果是
 *   该模块里的硬编码字符串、px 度量、方向假设**永远没人改**，因为没有域认领。
 *   而这类绕过在语言切换时才显形，且症状分散在多个模块，难以归因到当初那句声明。
 *   故守卫要求：可以说「我不做翻译」，但不能说「国际化不在我这层」。
 */
export function checkArchitectureClaim(claim: string, bag: DiagBag): Outcome<ArchitecturePillar> {
  const text = claim.toLowerCase();
  const forbidden: readonly string[] = [
    "国际化不归我",
    "i18n 不归我",
    "i18n not my concern",
    "国际化只是翻译",
    "i18n 只是翻译",
    "翻译是末端",
    "最后加翻译",
    "不影响本模块",
  ];
  const hit = forbidden.find((w) => text.includes(w.toLowerCase()));
  if (hit !== undefined) {
    bag.push(
      "ARCHITECTURE_DECLARATION_VIOLATION",
      `架构声明守卫：立场描述「${claim}」含绕过表述「${hit}」`,
      "国际化在 VE 是结构约束（六条立场见 ARCHITECTURE_DECLARATION），"
        + "你可以声明「本模块不做翻译」（那是 R 域的工作），"
        + "但不能声明「国际化不在我这层」——那会让本模块的硬编码与度量假设无人认领，"
        + "直到语言切换时才以「界面莫名其妙错了」的形式暴露。请改为"
        + "「本模块消费 LocaleModel，翻译与文案流转归 R 域」。",
    );
    return fail(
      "ARCHITECTURE_DECLARATION_VIOLATION",
      `立场描述含绕过表述「${hit}」`,
      "改写为「消费哪一段的产出契约 + 不做什么」的分工式声明",
    );
  }
  return ok(ARCHITECTURE_DECLARATION[0] as ArchitecturePillar, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §7 F2948 单源复述（判据五）
// ════════════════════════════════════════════════════════════════════════════

/**
 * F2948 单源复述声明（判据五）。
 *
 * F2948「遗留兑现：lang 语义与 emoji COLR」已经在 N 域侧建成了三样东西：
 *   1. lang 匹配树（祖先 lang 属性继承链 + 通配匹配）；
 *   2. lang→字体偏好映射（语言切换自动字体切换）；
 *   3. lang 投影携带（服务读屏发音，联动 F2932）。
 *
 * T 域对这三样物的立场是**引用，不是重建**。为什么这条必须写死在开工条上
 * （这是最容易「顺手做一份」的地方，也是最容易出事的）：
 *   两份 BCP47 匹配树在具体边界上必然给出不同答案。典型分歧点：
 *     · zh-CN 该回退到 zh、zh-Hans 还是 und？三份实现各选一个；
 *     · 祖父元素的 lang="zh" 与自身 lang="zh-Hant-TW" 谁优先？
 *   这类分歧**不会报错**——字体选择按自己那棵树走、读屏发音按另一棵树走，
 *   最终症状是「字对了但念错了」「繁体界面用简体字形」。归因时两边都觉得自己
 *   是对的，日志里也看不出分叉点。故本条把单源写成**可机检的引用登记**：
 *   本域只登记引用关系与权威条目号，不允许出现本域自建的第二份实现声明。
 */
export const F2948_SINGLE_SOURCE_DECLARATION: readonly SingleSourceRef[] = [
  {
    factName: "lang 匹配树（BCP47 匹配 + 祖先继承链）",
    authoritativeOwner: "VE-F2948（N 域侧已建成）",
    consumedByTDomainAs: "LocaleModel 的回退链输入（段1 只消费，不重建）",
    ifTDomainBuildsItsOwn: "SINGLE_SOURCE_FORKED：两棵树在 zh-Hans/und 边界上必然分歧",
  },
  {
    factName: "lang→字体偏好表",
    authoritativeOwner: "VE-F2915 预留激活 / VE-F2948 兑现（O 域侧消费）",
    consumedByTDomainAs: "FontPolicy 的链序输入（T 域只定序，不做覆盖扫描）",
    ifTDomainBuildsItsOwn: "SINGLE_SOURCE_FORKED：两套偏好表会让字体切换在语言回退时跳变",
  },
  {
    factName: "lang 投影携带（读屏发音依据）",
    authoritativeOwner: "VE-F2948 → F3856 读屏语言跟随",
    consumedByTDomainAs: "方向段与交付段的lang 输出来源",
    ifTDomainBuildsItsOwn: "SINGLE_SOURCE_FORKED：投影与发音依据不同源即产生「念错字」",
  },
  {
    factName: "CLDR 格式与复数规则",
    authoritativeOwner: "CLDR v46（经 VE-F4006 锚定，非本域自研规则）",
    consumedByTDomainAs: "格式段与复数映射的规则来源（引用标准，不内嵌私表）",
    ifTDomainBuildsItsOwn: "规则漂移：同一版本不同实现给出不同小数点位置",
  },
  {
    factName: "密度变体表",
    authoritativeOwner: "VE-F3442 变体单源（F4008 复用）",
    consumedByTDomainAs: "密度映射的基础档位表",
    ifTDomainBuildsItsOwn: "单源分叉：同一语言在两处得到不同密度档",
  },
];

/** 一条单源引用登记（本域消费某事实源，不重建它）。 */
export interface SingleSourceRef {
  /** 事实源名称（被引用的那一份是什么）。 */
  readonly factName: string;
  /** 权威归属条目（谁建的那一份）。 */
  readonly authoritativeOwner: string;
  /** T 域以什么身份消费它（只读引用，具体到哪一段）。 */
  readonly consumedByTDomainAs: string;
  /** 若 T 域自建会发生什么（后果写下来，避免「顺手做一份」）。 */
  readonly ifTDomainBuildsItsOwn: string;
}

/**
 * 单源纪律审计：核对本域登记的事实源清单齐备，且每条都有权威归属与后果说明。
 *
 * 审计本身不做运行时检查（那需要跨包import，破坏本条纯契约层的边界），
 * 它检查的是**声明的完整性**：清单齐不齐、有没有条目漏写「若自建会发生什么」。
 * 漏写后果说明的条目是最危险的——后人看到只登记了「lang 匹配树」而没有
 * 「自建会怎样」，很可能觉得「那我建一份挺方便」。
 */
export function auditSingleSource(bag: DiagBag): Outcome<number> {
  const refs = F2948_SINGLE_SOURCE_DECLARATION;
  if (refs.length === 0) {
    bag.push(
      "SINGLE_SOURCE_FORKED",
      "单源引用清单为空",
      "判据五要求复述 F2948 单源；清单为空意味着本域未声明它引用哪些权威源，"
        + "后人就无从知道该不该在这里自建一份",
    );
    return fail("SINGLE_SOURCE_FORKED", "单源引用清单为空", "补齐 F2948 等权威源登记与后果说明");
  }
  for (const r of refs) {
    if (r.authoritativeOwner.trim() === "") {
      bag.push(
        "SINGLE_SOURCE_FORKED",
        `事实源「${r.factName}」未标注权威归属条目`,
        "没有归属的事实源等于默认可自建；请写明它由哪个条目建成（F2948 / CLDR / F3442）",
      );
    }
    if (r.ifTDomainBuildsItsOwn.trim() === "") {
      bag.push(
        "SINGLE_SOURCE_FORKED",
        `事实源「${r.factName}」未写明「若 T 域自建会发生什么」`,
        "后果说明是这道纪律的实际约束力来源；只登记「有单源」而不写后果，"
          + "后来者会以为那只是个建议",
      );
    }
  }
  return ok(refs.length, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §8 分工审计 + 映射审计（判据二、三的机检）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 四域分工审计：逐面核对四件事，任一不成立即产出诊断——
 *   1. 每一面的entries 键集恰为五个参与域（不多不少）；
 *   2. 每面恰有一个 decision-owner（决策唯一，否则真出问题时无人拍板）；
 *   3. 每域每条都必须写 doesNot（禁扩面是分工的主要价值，缺它分工未定义）；
 *   4. T 域在 execution-owner 面上不得deliver 实现承诺（越界即 DIVISION_OVERREACH）。
 *
 * 第 4 项是本审计的重点（最容易犯的错）：T 域是决策域，一旦在某个面上
 * 同时写成执行域，就会出现「两份排版实现」或「两处方向判定」——那正是
 * 本条要防的协调成本。故规则写死：T 域只能是 decision-owner / consulted /
 * handoff-peer，绝不能是 execution-owner。
 */
export function auditDivision(bag: DiagBag): Outcome<number> {
  const requiredDomains: readonly PartnerDomainId[] = ["VE-T", "VE-N", "VE-O", "VE-S", "VE-R"];
  let issues = 0;

  for (const f of DIVISION_TABLE) {
    // 1) 键集恰为五域。
    const keys = Object.keys(f.entries).sort();
    const want = requiredDomains.slice().sort();
    if (keys.length !== want.length || keys.some((k, i) => k !== want[i])) {
      bag.push(
        "DIVISION_GAP",
        `决策面「${f.name}」的域声明键集为 [${keys.join("、")}]，应为 [${want.join("、")}]`,
        "每个决策面必须五域逐一声明，缺一即该问题在该域无归属；"
          + "多出未知域键通常意味着拼错了域 id",
      );
      issues += 1;
      continue;
    }

    // 2) 决策唯一。
    const owners = requiredDomains.filter((d) => f.entries[d].role === "decision-owner");
    if (owners.length !== 1) {
      bag.push(
        "DIVISION_CONFLICT",
        `决策面「${f.name}」有 ${owners.length} 个决策归属（${owners.join("、") || "无"}）`,
        "决策必须唯一：出问题时需要有人拍板拍板，两个域都「决策」等于没人负责。"
          + "请保留一个 decision-owner，其余改为 execution-owner / consulted",
      );
      issues += 1;
    }

    // 3) 禁扩面必填 + 4) T 域不得越界执行。
    for (const d of requiredDomains) {
      const e = f.entries[d];
      if (e.doesNot.trim() === "") {
        bag.push(
          "DIVISION_OVERREACH",
          `决策面「${f.name}」中 ${d} 未写明「不做什么」`,
          "只写「做什么」的分工会产生隐性扩面（做到一半顺手把邻居的事也做了）；"
            + "请写明该域在此面上的边界，例如「不实现分词，只消费路由结果」",
        );
        issues += 1;
      }
      if (d === "VE-T" && e.role === "execution-owner") {
        bag.push(
          "DIVISION_OVERREACH",
          `决策面「${f.name}」把 T 域登记为执行域`,
          "T 域是决策域，不得同时承担执行实现——决策与执行同体时，"
            + "策略变更会直接改到实现细节，导致下游（N/O/S/R）无法判断契约是否变化。"
            + "请把 T 域改为 decision-owner，执行交给对应主责域",
        );
        issues += 1;
      }
    }
  }
  if (issues > 0) {
    return fail<number>("DIVISION_CONFLICT", `四域分工存在 ${issues} 处问题`, "按诊断逐条修正分工表");
  }
  return ok(DIVISION_TABLE.length, bag.all());
}

/**
 * 十项映射审计（判据三硬门）：逐项核对五件事——
 *   1. 十项齐备，且名称与 OFFICIAL_MAPPINGS 逐项一致（防漏项/改名漂移）；
 *   2. 产出契约名全局唯一（撞名会让下游拿错契约）；
 *   3. 每项至少绑定一条架构立场（无立场支撑的映射视为无根，将来会被「顺手简化」掉）；
 *   4. 每项的 mustNot 非空（禁扩面）；
 *   5. 承担条目须引用册内存在的功能号（防引用不存在的条目）。
 */
export function auditMappings(bag: DiagBag): Outcome<number> {
  // 1) 齐备 + 名称一致。
  if (MAPPING_IDS.length !== OFFICIAL_MAPPINGS.length) {
    bag.push(
      "MAPPING_MISSING",
      `映射注册表实有 ${MAPPING_IDS.length} 项，判据要求 ${OFFICIAL_MAPPINGS.length} 项`,
      `官方十项：${OFFICIAL_MAPPINGS.join("、")}。`
        + "少一项即意味着该维度无人负责——十项正是覆盖全部界面决策的最小集，"
        + "缺一项就会在语言切换时以「那一类元素没跟着变」的形式暴露",
    );
  }
  for (const official of OFFICIAL_MAPPINGS) {
    if (!MAPPING_IDS.some((id) => MAPPING_REGISTRY[id].name === official)) {
      bag.push(
        "MAPPING_MISSING",
        `官方映射「${official}」未在 MAPPING_REGISTRY 中以同名条目登记`,
        "映射名与官方序列必须逐项一致；请补登记或修正错字",
      );
    }
  }

  const contracts = new Set<string>();
  const dupContracts: string[] = [];
  for (const id of MAPPING_IDS) {
    const m = MAPPING_REGISTRY[id];

    // 2) 契约名唯一。
    if (contracts.has(m.producesContract)) dupContracts.push(m.producesContract);
    contracts.add(m.producesContract);

    // 3) 立场支撑。
    if (m.groundedBy.length === 0) {
      bag.push(
        "MAPPING_CONTRADICTS_DECLARATION",
        `映射「${m.name}」未绑定任何架构立场`,
        "没有立场支撑的映射在本域没有根：将来做局部优化时它会第一个被删掉"
          + "（因为看起来只是一次查表）。请绑定它所依据的立场 id",
      );
    }
    for (const p of m.groundedBy) {
      if (!ARCHITECTURE_PILLAR_IDS.includes(p)) {
        bag.push(
          "MAPPING_CONTRADICTS_DECLARATION",
          `映射「${m.name}」绑定了不存在的立场 id「${p}」`,
          `已登记立场：${ARCHITECTURE_PILLAR_IDS.join("、")}`,
        );
      }
    }

    // 4) 禁扩面。
    if (m.mustNot.trim() === "") {
      bag.push(
        "MAPPING_CONTRADICTS_DECLARATION",
        `映射「${m.name}」未写明 mustNot`,
        "十项映射若不写死边界，会各自向邻近维度扩张（排版去管字体、字体去管格式），"
          + "最终十项变一坨相互调用的代码；请写明它明确不做什么",
      );
    }

    // 5) 承担条目须引用本域功能号（F4001~F4020 区间内）。
    const itemMatch = /F(\d{4})/.exec(m.owner);
    if (itemMatch === null) {
      bag.push(
        "MAPPING_MISSING",
        `映射「${m.name}」的承担条目「${m.owner}」未引用册内功能号`,
        "条目号是收口对账（200/200 grep 实测）的锚；请写成「VE-F40xx 标题」的格式",
      );
    } else {
      const num = Number(itemMatch[1]);
      if (num < DOMAIN.itemLo || num > DOMAIN.itemHi) {
        bag.push(
          "MAPPING_MISSING",
          `映射「${m.name}」的承担条目 F${num} 落在 T 域功能号区间（${DOMAIN.itemLo}~${DOMAIN.itemHi}）之外`,
          "本域只能认领自己区间内的条目；域外条目请走对应域的分工面登记",
        );
      }
    }
  }

  if (dupContracts.length > 0) {
    bag.push(
      "MAPPING_DUPLICATE",
      `映射产出契约名重复：${[...new Set(dupContracts)].join("、")}`,
      "契约名是下游引用的唯一凭据；撞名会让两段产出无法区分。请让契约名反映各自的产出物",
    );
    return fail("MAPPING_DUPLICATE", "映射契约名重复", "为每项映射起唯一的契约名");
  }
  return ok(MAPPING_IDS.length, bag.all());
}

/** 架构声明审计：六条立场齐备、均为规范性、证据锚点非空。 */
export function auditArchitectureDeclaration(bag: DiagBag): Outcome<number> {
  if (ARCHITECTURE_DECLARATION.length === 0) {
    bag.push(
      "ARCHITECTURE_DECLARATION_VIOLATION",
      "架构声明为空",
      "判据一要求「国际化不是翻译是架构」被写成可机检的立场；"
        + "空声明意味着这一立场只存在于口头共识，会在进度压力下被稀释",
    );
    return fail("ARCHITECTURE_DECLARATION_VIOLATION", "架构声明为空", "登记六条规范性立场与证据锚点");
  }
  for (const p of ARCHITECTURE_DECLARATION) {
    if (!p.normative) {
      bag.push(
        "ARCHITECTURE_DECLARATION_VIOLATION",
        `立场「${p.title}」被标为非规范性`,
        "本条登记的立场都是全域必须遵守的；若某条实际只是建议，请移出本表"
          + "（放进条目自己的注释里），否则「规范性」这个标记就不再可信",
      );
    }
    if (p.evidence.length === 0) {
      bag.push(
        "ARCHITECTURE_DECLARATION_VIOLATION",
        `立场「${p.title}」没有证据锚点`,
        "没有证据锚点的立场无法被验证，只能靠人记；请指向兑现该立场的条目",
      );
    }
  }
  return ok(ARCHITECTURE_DECLARATION.length, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §9 开工摘要与自检（判据收敛点）
// ════════════════════════════════════════════════════════════════════════════

/** 开工闸门清单（判据逐条对应，可机检项已标）。 */
export const KICKOFF_GATES: readonly KickoffGate[] = [
  { id: "global-first", criterion: "国际化是架构级约束已写成六条规范性立场并可机检", machineChecked: true },
  { id: "four-domain-division", criterion: "与 N02/O06/S03/R 的十四面分工齐备，决策唯一、禁扩面必填", machineChecked: true },
  { id: "ten-mappings", criterion: "十项映射齐备，各有键/值/归属/契约/禁扩面/立场支撑", machineChecked: true },
  { id: "five-segment-signature", criterion: "五段签名 v1 冻结，段序不倒置、上下游契约恰好匹配", machineChecked: true },
  { id: "single-source", criterion: "F2948 lang 单源复述到位，登记引用而非重建", machineChecked: true },
  { id: "clamped-domain", criterion: "Locale/方向/排版族/密度四类取值均有钳制与枚举守卫", machineChecked: true },
  { id: "error-path", criterion: "四条错误路径均有三要素诊断：Locale 非法/方向缺失/段间失配/声明绕过", machineChecked: true },
  { id: "zero-silent-degrade", criterion: "任何降级必产诊断（方向缺失→LTR 带诊断、未收录语言→Latin 带标记）", machineChecked: true },
  { id: "no-dom-purity", criterion: "纯契约层：零 DOM 依赖、零全局可变状态、零异常抛出", machineChecked: true },
];

/** 一条开工闸门。 */
export interface KickoffGate {
  readonly id: string;
  /** 判据原文（人话，可直接进评审 checklist）。 */
  readonly criterion: string;
  /** 是否可由本文件的审计函数机检（false = 需人工核验，本表当前全部为 true）。 */
  readonly machineChecked: boolean;
}

/** 开工摘要（本域开工状态的总账，供下游条目与组级收口引用）。 */
export interface DomainKickoffSummary {
  readonly domain: string;
  readonly pillarCount: number;
  readonly facetCount: number;
  readonly mappingCount: number;
  readonly segmentCount: number;
  readonly segmentChain: string;
  readonly singleSourceCount: number;
  readonly signatureVersion: string;
  readonly cldrVersion: string;
  readonly gates: readonly KickoffGate[];
}

/**
 * 生成域开工摘要。任一审计不通过即返回显性失败（开工单不允许带未知态）。
 *
 * 五道审计全过才允许开工，理由：国际化是**返工成本最高**的域之一——
 * 开工时漏掉一项分工或一条映射的禁扩面，代价是全族 200 项里的近百项返工。
 * 与其开工后靠巡检补救，不如在此处硬拦。这也是为什么本条敢把审计做全：
 * 它一次性替代了「开工评审会」上最容易走过场的那些确认项。
 */
export function buildKickoffSummary(): Outcome<DomainKickoffSummary> {
  const bag = new DiagBag();

  // 审计 1：架构声明（判据一）。
  const pillars = auditArchitectureDeclaration(bag);
  if (!pillars.ok) {
    return fail<DomainKickoffSummary>(pillars.code, pillars.message, pillars.hint);
  }

  // 审计 2：四域分工（判据二）。
  const division = auditDivision(bag);
  if (!division.ok) {
    return fail<DomainKickoffSummary>(division.code, division.message, division.hint);
  }

  // 审计 3：十项映射（判据三）。
  const mappings = auditMappings(bag);
  if (!mappings.ok) {
    return fail<DomainKickoffSummary>(mappings.code, mappings.message, mappings.hint);
  }

  // 审计 4：五段签名（判据四）。
  const chain = auditSegmentChain(bag);
  if (!chain.ok) {
    return fail<DomainKickoffSummary>(chain.code, chain.message, chain.hint);
  }

  // 审计 5：单源复述（判据五）。
  const single = auditSingleSource(bag);
  if (!single.ok) {
    return fail<DomainKickoffSummary>(single.code, single.message, single.hint);
  }

  return ok(
    {
      domain: `${DOMAIN.id} ${DOMAIN.name}（${DOMAIN.itemLo}-${DOMAIN.itemHi}，共 ${DOMAIN.itemCount} 项，${DOMAIN.wave} 波）`,
      pillarCount: pillars.value,
      facetCount: division.value,
      mappingCount: mappings.value,
      segmentCount: SEGMENT_IDS.length,
      segmentChain: SEGMENT_ORDER.map((id) => SEGMENT_SIGNATURES[id].outputContract).join(" → "),
      singleSourceCount: single.value,
      signatureVersion: DOMAIN.architectureSignatureVersion,
      cldrVersion: DOMAIN.cldrVersion,
      gates: KICKOFF_GATES,
    },
    bag.all(),
  );
}