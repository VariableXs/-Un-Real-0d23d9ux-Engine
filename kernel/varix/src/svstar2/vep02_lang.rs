//! VE-F3002 · 动效设计语言总纲（VE-P 域 · 动效库组 · 语言段 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3002`
//!
//! **判据（锚点原文）**：四原则、四级时长、语义化缓动、内建 reduce、单源取值、判据。
//!
//! **职责定位（锚点原文）**：动效设计语言总纲——设计语言声明（VE 动效语言四原则：
//! 节奏（进退对称/时长分级）/ 呼吸（缓动有性格：不全是 ease——主缓动家族定义）/
//! 因果（动效解释变化来源：元素从哪来到哪去——不炫技不装饰性滥用）/克制（一个界面
//! 同时活动元素≤3——注意力预算红线））；时长分级表（微反馈 100-150ms/组件转场
//! 200-300ms/页面转场 300-450ms/复杂编排 500ms 上限——四级各配缓动族与适用清单）；
//! 缓动家族（标准（ease-out 进/ease-in 退）/弹性（spring 参数三档）/强调（cubic
//! 特调三支）——每支命名化登记（名字→曲线+语义——用名字不用裸曲线：语义化缓动
//! 红线））。
//!
//! **数据结构（锚点原文·家族格式）**：语言册（四原则+分级表+缓动家族）；命名
//! 登记（名→参数）；适用清单（界面场景→推荐族）。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：超预算时长→警告+白名单流程（长
//! 动效须理由——默认短红线）；裸曲线入册（未命名缓动）→命名或拒绝（语义化红线）；
//! 原则冲突案例→设计评审裁决记录。
//!
//! **性能逐项分解（锚点原文·家族格式）**：查询 O(1) 名查参；分级断言 O(1)；
//! 登记 O(1)。
//!
//! **跨批对接点（锚点原文·家族格式）**：M/O04 缓动求值（参数→F2867 Easing）；
//! P02/P03 消费（转场与微交互从本册取值——单源红线）；E 域令牌（缓动参数可入
//! 令牌——联动声明）。
//!
//! **无障碍与隐私（锚点原文）**：分级表含 reduced 档（reduce 态时长→直达——语言层
//! 内建无障碍而非外挂补丁）；无隐私面。
//!
//! # 本项的边界（不越界施工，遵守"只做领到的任务"）
//!
//! VE-F3002 是**动效设计语言总纲**——它交付：四原则的可执行判定（不是四句形容词）、
//! 四级时长分级表与逐级断言、缓动家族的命名登记册（名→曲线+语义）、适用清单
//! （界面场景→推荐族）、超预算白名单流程、原则冲突裁决记录。它**不代做**后续
//! 18 项。分工在册（见 [`DOWNSTREAM_OWNERSHIP`]）：
//!
//! - F3003 动效令牌体系拥有**三族令牌全集与 CSS 变量注入器**；本项只立"缓动
//!   参数可入令牌"这条**联动声明**与命名到令牌的映射位，不写令牌表；
//! - F3005 转场编排器拥有**编排图与原语编译器**；本项的 [`DurationTier`] 只被
//!   编排器**消费**，不提供编排实现；
//! - F3006 动效组件化拥有**四件套组件定义**；本项只立"组件默认参数从本册取值"
//!   这条单源承诺；
//! - F3014 微反馈标准拥有**六形态库**；本项的 [`Scene`] 只给场景→推荐族的
//!   映射表，形态本体归 F3014；
//! - F3015 物理引擎拥有**弹簧求解器与预设表**；本项只登记`elastic` 家族的
//!   **语义档位名**（snappy/soft/bouncy）与档位语义，**不给刚度阻尼数值**——
//!   数值归 F3015，本项只保证"名字→语义"的映射是单源的。
//!
//! # 设计要点（每条都是可执行的，不是标签）
//!
//! - **四原则各有可失败的判定**：[`Principle`] 四项各有 [`Principle::verdict`]
//!   的判定口径与 [`PrincipleViolation`] 的检出形态。"呼吸"原则的判定是
//!   "不允许全册都是同一条曲线"——若某级时长只挂一种缓动，判`Violation`，
//!   因为"缓动有性格"的反面就是"一律 ease"；
//! - **注意力预算是硬上限不是建议**：[`AttentionBudget`] 上限
//!   [`MAX_CONCURRENT_MOTIONS`] = 3（锚点原文），超出 [`E_ATTENTION_BUDGET`]。
//!   之所以做成硬门：注意力预算是唯一一条**用户能直接感知被侵犯**的原则，
//!   别的原则越界还能用"效果还行"说服人，这条越界只会让人觉得界面吵；
//! - **四级时长是区间不是点**：[`DurationTier`] 每级给 `min/max`，判定用区间
//!   而非单值。给单值会导致"取上限就算合规"的擦边用法，而时长是**用户能计时
//!   感知**的量，擦边会被立刻发现；
//! - **reduced 档内建在分级表里，不是外挂**：每级都有 [`DurationTier::reduced_ms`]
//!   档位语义（reduce 态时长→0 直达）。放在分级表内建，而不是让每个消费方
//!   自己写 `if reduce { duration = 0 }`——后者必然出现漏写的那一个；
//! - **语义化缓动是硬门**：[`EasingRegistry::admit`] 拒绝**裸曲线**（只给
//!   `cubic-bezier` 数值而无名字），锚点原文「用名字不用裸曲线」。理由很实际：
//!   `cubic-bezier(0.4, 0, 0.2, 1)` 出现在代码里没人知道它是什么情绪，而
//!   `ease-out-enter` 自解释；
//! - **超预算时长走白名单且须理由**：[`DurationExemption`] 与第一红线同构
//!   ——理由与期限缺一不可。白名单存在的理由是"确有场景需要长动效"（如引导
//!   首次播放），但**默认短**意味着不写理由就被拦；
//! - **原则冲突必须留裁决记录**：[`RulingRecord`] 承接"节奏要长但克制要少活动
//!   元素"这类冲突，产出**四要素齐备**的裁决（现象/裁决/理由/适用边界）。
//!   不留裁决的冲突解决等于把决策埋进某个人的记忆里；
//! - **单源取值可机检**：[`MotionLanguage::audit_single_source`] 核对
//!   "每个 [`Scene`] 的推荐族都有登记名"与"每个登记名都有适用清单条目"——
//!   两个方向都查，只查一个方向会让单向的错配漏过去；
//! - **参数域钳制必产告警**：[`clamp_ms`] 越界不静默夹取，每次夹取都往告警账
//!   落一条 [`ClampNotice`]，消费方能回答"这个值为什么不是我写的那个"。
//!
//! **零外部依赖**，只依赖 `crate::checks`（自检侧）与 `alloc`。
//! 确定性：逻辑 tick 注入、零墙钟、零 IO，回归可复现（对拍红线）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、总纲常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 总纲版本。契约变更走版本号，破坏性变更必须升版并留迁移说明。
pub const LANG_VERSION: &str = "P01-lang-v1";

/// 设计原则数（判据「四原则」）。
pub const PRINCIPLE_COUNT: usize = 4;

/// 时长分级数（判据「四级时长」）。
pub const TIER_COUNT: usize = 4;

/// 缓动家族数（标准/弹性/强调）。
pub const FAMILY_COUNT: usize = 3;

/// 适用场景数（界面场景 → 推荐族）。
pub const SCENE_COUNT: usize = 8;

/// **注意力预算上限**（锚点原文「一个界面同时活动元素≤3」）。
///
/// 这是锚点直接给出的数字，不做"工程上取整数"的解释。取 3 的理由也很实在：
/// 三个同时动的元素，用户还能分辨"谁在响应我的操作"；四个就开始变成背景噪音。
pub const MAX_CONCURRENT_MOTIONS: u32 = 3;

/// 登记册中缓动名的最大长度（防超长名污染日志与台账）。
pub const MAX_EASING_NAME_LEN: usize = 32;

/// 登记册容量上限。
pub const MAX_EASINGS: usize = 64;

/// 时长白名单条数上限。
pub const MAX_DURATION_EXEMPTIONS: usize = 32;

/// 原则冲突裁决记录条数上限。
pub const MAX_RULINGS: usize = 64;

/// 告警账条数上限。满后拒绝并计数——告警静默丢弃等于钳制失效。
pub const CLAMP_LOG_CAP: usize = 256;

/// 复杂编排档的时长上限（毫秒；锚点原文「复杂编排 500ms 上限」）。
pub const COMPLEX_ORCHESTRA_MAX_MS: u32 = 500;

/// 复杂编排档在 reduce 态的时长（毫秒；0 = 直达）。
pub const REDUCED_DURATION_MS: u32 = 0;

/// 复杂度声明（人读文本）。实现与本表逐条对应，改动必须两处同步走 ADR。
pub const COMPLEXITY_DOC: &str = "\
P 域动效设计语言总纲复杂度声明（VE-F3002 · P01-lang-v1）：
C1  名查参 easing(name)：O(1)（按 MotionTheme 同源的定长小表线性查；\
    缓动登记上界 MAX_EASINGS，故为有界常量）。
C2  分级断言 assert_tier：O(1)（单值比较 + 可选告警追加）。
C3  登记 admit：O(1)（尾部追加 + 唯一性一趟比较）。
C4  原则判定 verdict：O(1)（四原则各自的定长判定，冲突判定为 O(场景数)）。
C5  注意力预算登记 acquire：O(1)（尾部追加或淘汰最旧，满预算即拒绝）。
C6  时长白名单申请/判定：O(1)（线性查表，表上界 MAX_DURATION_EXEMPTIONS）。
C7  裁决登记 open_ruling：O(1)（尾部追加 + 四要素入参校验）。
C8  单源对账 audit_single_source：O(登记名数 × 场景数)，两者均为定长上界。
C9  参数钳制 clamp_ms：O(1)（单值比较 + 可选告警追加）。
C10 语言册总自检 self_audit：O(登记名数 + 场景数 + 原则数 + 分级数)。";

// ---------------------------------------------------------------------------
// 二、设计语言四原则（判据一：四原则）
// ---------------------------------------------------------------------------

/// 设计原则（判据一：四原则）。
///
/// 四条原则的共同特征是**可失败**：每条都有明确的违反形态，不许把它们写成
/// "要有节奏感"这类无法判定的口号。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Principle {
    /// 原则一：节奏（进退对称 / 时长分级）。
    Rhythm,
    /// 原则二：呼吸（缓动有性格：不全是 ease——主缓动家族定义）。
    Breath,
    /// 原则三：因果（动效解释变化来源：元素从哪来到哪去——不炫技不装饰性滥用）。
    Causality,
    /// 原则四：克制（一个界面同时活动元素≤3——注意力预算红线）。
    Restraint,
}

impl Principle {
    /// 四原则全集（顺序即 [`PRINCIPLE_ORDER`]）。
    pub const ALL: [Principle; PRINCIPLE_COUNT] = [
        Principle::Rhythm,
        Principle::Breath,
        Principle::Causality,
        Principle::Restraint,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Principle::Rhythm => "节奏",
            Principle::Breath => "呼吸",
            Principle::Causality => "因果",
            Principle::Restraint => "克制",
        }
    }

    /// 原则码（台账引用）。
    pub fn code(self) -> &'static str {
        match self {
            Principle::Rhythm => "PR-RHYTHM",
            Principle::Breath => "PR-BREATH",
            Principle::Causality => "PR-CAUSALITY",
            Principle::Restraint => "PR-RESTRAINT",
        }
    }

    /// 原则序号（0 起）。
    pub fn rank(self) -> usize {
        match self {
            Principle::Rhythm => 0,
            Principle::Breath => 1,
            Principle::Causality => 2,
            Principle::Restraint => 3,
        }
    }

    /// 枚举往返守卫：未知码返回 `None`（调用方必须显性拒绝，不许猜）。
    pub fn from_code(code: &str) -> Option<Principle> {
        Principle::ALL.iter().copied().find(|p| p.code() == code)
    }

    /// 该原则的**可执行判定说明**（怎么算违反，写下来就是判据）。
    pub fn verdict_rule(self) -> &'static str {
        match self {
            Principle::Rhythm => {
                "进退须对称（入场与退场互为逆过程，时长落在同一分级内）；\
                 时长须落在四级分级表的对应区间内，越级须走白名单并写明理由。"
            }
            Principle::Breath => {
                "缓动须有性格：同一分级内不得只有一种**无性格的通用 ease**——\
                 全册一律 ease 即判违反，因为「不全是 ease」正是本原则要否定的现状。\
                 注意：单一**带性格**的曲线（如页面转场共用一条弹簧）不违反，\
                 那正是有性格的体现；判据看的是「有没有性格」而不是「有几支」。"
            }
            Principle::Causality => {
                "动效须解释变化来源：元素从哪来到哪去要看得出来；\
                 纯装饰性位移（元素不动却动起来）判违反。"
            }
            Principle::Restraint => {
                "同一界面同时活动的动效元素不得超过 3 个；\
                 超预算即违反——这是四条里唯一用户能直接感知被侵犯的一条。"
            }
        }
    }

    /// 该原则服务的判据项。
    pub fn serves(self) -> &'static [Criterion] {
        match self {
            Principle::Rhythm => &[Criterion::TierLadder, Criterion::ReduceBuiltIn],
            Principle::Breath => &[Criterion::SemanticEasing],
            Principle::Causality => &[Criterion::SemanticEasing, Criterion::SingleSource],
            Principle::Restraint => &[Criterion::FourPrinciples, Criterion::Criterion],
        }
    }
}

/// 原则序单源常量。
pub const PRINCIPLE_ORDER: [Principle; PRINCIPLE_COUNT] = Principle::ALL;

/// 原则违反记录（**违反要能定位到原则与场景，不能只说"有问题"**）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrincipleViolation {
    /// 违反的原则。
    pub principle: Principle,
    /// 涉及场景（可空——全局性违反如注意力预算超限无单一场景）。
    pub scene: &'static str,
    /// 现象（人话）。
    pub symptom: String,
    /// 建议（给路，不只是拒绝）。
    pub advice: &'static str,
}

impl PrincipleViolation {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "原则违反[{}]：{}；建议 {}",
            self.principle.zh(),
            self.symptom,
            self.advice
        )
    }
}

/// 呼吸原则的判定输入（由登记册派生，判定函数不需要知道册内结构）。
///
/// 刻意做成结构体而不是裸 `usize`：单传「本级有几种缓动」会诱使人把
/// 「只有一支」直接判成违反，而**只有一支带性格的曲线恰恰是呼吸做对了**
/// ——页面转场全用一条弹簧，那是有性格，不是没性格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BreathInput {
    /// 该分级在册的不同缓动名数量。
    pub distinct: usize,
    /// 该分级唯一那支缓动的家族（`distinct <= 1` 时才有意义）。
    pub sole_family: Option<EasingFamily>,
}

impl BreathInput {
    /// 是否构成「一律 ease」——单一且**无性格**（标准族）。
    pub fn is_uniform_ease(self) -> bool {
        self.distinct <= 1 && matches!(self.sole_family, Some(EasingFamily::Standard))
    }

    /// 读屏单行。
    pub fn screen_line(self) -> String {
        match self.sole_family {
            Some(f) if self.distinct <= 1 => format!(
                "该分级仅 1 支缓动（{}族{}）：{}",
                f.zh(),
                if self.is_uniform_ease() {
                    "无性格，判违反"
                } else {
                    "有性格，不判违反"
                },
                if self.is_uniform_ease() {
                    "至少再登记一支弹性或强调族"
                } else {
                    "保持现状"
                }
            ),
            _ => format!("该分级 {} 支缓动，族别不齐不构成违反", self.distinct),
        }
    }
}

impl Principle {
    /// 原则判定（复杂度 C4：O(1)）。
    ///
    /// 入参刻意做得很"笨"：时长与缓动名、并发数、活动元素是否真的在动。
    /// 这样判定函数不需要知道调用方的内部结构，测试也能直接构造。
    ///
    /// 判定返回**全部**违反（不是第一条）——一次报全比让人来回试二十次快。
    pub fn verdict(
        self,
        scene: &'static str,
        in_ms: u32,
        out_ms: Option<u32>,
        easing_in: &str,
        easing_out: &str,
        breath: BreathInput,
        decorative: bool,
        concurrent: u32,
    ) -> Vec<PrincipleViolation> {
        let distinct = breath.distinct;
        let mut out = Vec::new();
        match self {
            Principle::Rhythm => {
                // 时长须**严格落在**某一级区间内——用 of_strict而不是 of()。
                // of() 会把 60ms 就近归到微反馈，那样越级时长全被"归"没了，
                // 红线永远不亮；就近归属是给「该退到哪一级」用的，不是给
                // 合规判定用的。
                let tier = DurationTier::of_strict(in_ms);
                if tier.is_none() {
                    out.push(PrincipleViolation {
                        principle: self,
                        scene,
                        symptom: format!("时长 {}ms 不落在四级分级表的任何区间内", in_ms),
                        advice: "取最近的一级区间；确需超上限须走时长白名单并写明理由",
                    });
                }
                // 进退对称：退场时长与入场同属一级（容许一级之内的差异）。
                if let Some(o) = out_ms {
                    match (DurationTier::of_strict(in_ms), DurationTier::of_strict(o)) {
                        (Some(a), Some(b)) if a == b => {}
                        _ => out.push(PrincipleViolation {
                            principle: self,
                            scene,
                            symptom: format!(
                                "进退不对称：入场 {}ms（{}）退场 {}ms（{}），不在同一分级",
                                in_ms,
                                DurationTier::of(in_ms)
                                    .map(|t| t.zh())
                                    .unwrap_or("越级"),
                                o,
                                DurationTier::of(o)
                                    .map(|t| t.zh())
                                    .unwrap_or("越级")
                            ),
                            advice: "退场取入场的逆过程且落同一分级；确需不对称须走白名单",
                        }),
                    }
                }
                // 呼吸辅助：进退用了同一条曲线**且该级无性格**也算「一律 ease」
                // 的一种。只判「同曲线」会把页面转场（进出共用一条弹簧）错杀，
                // 只判「单一支」会把有性格的弹簧错杀——两个条件缺一都误判。
                if easing_in == easing_out && breath.is_uniform_ease() {
                    out.push(PrincipleViolation {
                        principle: self,
                        scene,
                        symptom: format!(
                            "进退共用无性格缓动 {}，该分级仅 {} 种缓动且属标准族",
                            easing_in, distinct
                        ),
                        advice: "进场用 ease-out、退场用 ease-in；同一分级至少两种缓动",
                    });
                }
            }
            Principle::Breath => {
                // 「不全是 ease」的反例判定：整级只有一种且是无性格的标准族。
                if breath.is_uniform_ease() {
                    out.push(PrincipleViolation {
                        principle: self,
                        scene,
                        symptom: format!(
                            "该分级只有 {} 种缓动且属标准族，缓动无性格；\
                             「不全是 ease」正是否定这种现状",
                            distinct
                        ),
                        advice: "按适用清单为该场景指定推荐族；至少覆盖标准族的进出两支",
                    });
                }
            }
            Principle::Causality => {
                // 纯装饰：元素不动却动起来。
                if decorative {
                    out.push(PrincipleViolation {
                        principle: self,
                        scene,
                        symptom: "动效不解释任何变化来源（元素未发生位移/尺寸/可见性变化）".to_string(),
                        advice: "让动效承载真实变化（元素从哪来到哪去）；\
                                 无变化可解释时直接去掉动效",
                    });
                }
            }
            Principle::Restraint => {
                if concurrent > MAX_CONCURRENT_MOTIONS {
                    out.push(PrincipleViolation {
                        principle: self,
                        scene,
                        symptom: format!(
                            "同时活动动效元素 {} 个，超过注意力预算 {}",
                            concurrent, MAX_CONCURRENT_MOTIONS
                        ),
                        advice: "合并同源动效或排队执行；注意力预算是硬门，不可调高",
                    });
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 三、时长分级表（判据二：四级时长）
// ---------------------------------------------------------------------------

/// 时长分级（判据二「四级时长」）。
///
/// 每级是**区间** `[min, max]`，不是单值。给单值会招来"取上限就算合规"的擦边
/// 用法，而时长是用户能亲自计时的量，擦边会被立刻发现。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DurationTier {
    /// 级一：微反馈（100-150ms）。
    MicroFeedback,
    /// 级二：组件转场（200-300ms）。
    ComponentTransition,
    /// 级三：页面转场（300-450ms）。
    PageTransition,
    /// 级四：复杂编排（500ms 上限）。
    ComplexOrchestra,
}

impl DurationTier {
    /// 四级全集（顺序即 [`TIER_ORDER`]，唯一真值源）。
    pub const ALL: [DurationTier; TIER_COUNT] = [
        DurationTier::MicroFeedback,
        DurationTier::ComponentTransition,
        DurationTier::PageTransition,
        DurationTier::ComplexOrchestra,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            DurationTier::MicroFeedback => "微反馈",
            DurationTier::ComponentTransition => "组件转场",
            DurationTier::PageTransition => "页面转场",
            DurationTier::ComplexOrchestra => "复杂编排",
        }
    }

    /// 分级码。
    pub fn code(self) -> &'static str {
        match self {
            DurationTier::MicroFeedback => "DT-MICRO",
            DurationTier::ComponentTransition => "DT-COMPONENT",
            DurationTier::PageTransition => "DT-PAGE",
            DurationTier::ComplexOrchestra => "DT-ORCHESTRA",
        }
    }

    /// 分级序号（0 起）。
    pub fn rank(self) -> usize {
        match self {
            DurationTier::MicroFeedback => 0,
            DurationTier::ComponentTransition => 1,
            DurationTier::PageTransition => 2,
            DurationTier::ComplexOrchestra => 3,
        }
    }

    /// 枚举往返守卫。
    pub fn from_code(code: &str) -> Option<DurationTier> {
        DurationTier::ALL.iter().copied().find(|t| t.code() == code)
    }

    /// 该级的时长下界（毫秒）。
    ///
    /// **四级区间互不重叠**：每级下界取上一级上界，故
    /// `ComplexOrchestra.min = PageTransition.max = 450`。这不是巧合而是
    /// 约束——`of()` 的就近反查依赖区间不重叠，否则400ms 到底算页面转场
    /// 还是复杂编排就成了"看谁先遍历到"的事。
    pub fn min_ms(self) -> u32 {
        match self {
            DurationTier::MicroFeedback => 100,
            DurationTier::ComponentTransition => 200,
            DurationTier::PageTransition => 300,
            DurationTier::ComplexOrchestra => 450,
        }
    }

    /// 该级的时长上界（毫秒）。
    ///
    /// 复杂编排档是**上限即语义**（锚点原文「复杂编排 500ms 上限」），
    /// 其余三级是区间。
    pub fn max_ms(self) -> u32 {
        match self {
            DurationTier::MicroFeedback => 150,
            DurationTier::ComponentTransition => 300,
            DurationTier::PageTransition => 450,
            DurationTier::ComplexOrchestra => COMPLEX_ORCHESTRA_MAX_MS,
        }
    }

    /// 该级是**上限档**（超出即须白名单，而非"下一级"）。
    pub fn is_ceiling(self) -> bool {
        matches!(self, DurationTier::ComplexOrchestra)
    }

    /// 该级的**reduce 档时长**（毫秒；0 = 直达，终态原子应用）。
    ///
    /// reduce 档**内建在分级表里**，而不是让每个消费方自己写
    /// `if reduce { duration = 0 }`——后者必然出现漏写的那一个。
    pub fn reduced_ms(self) -> u32 {
        REDUCED_DURATION_MS
    }

    /// 该级是否**内建无障碍**（四档皆真——语言层而非外挂补丁）。
    pub fn has_reduced_band(self) -> bool {
        true
    }

    /// 由毫秒**严格落区间**反查分级（复杂度 C2：O(1)）。
    ///
    /// 与 [`DurationTier::of`] 的区别是本函数**不做就近归属**：不落在任何
    /// 区间内就返回 `None`。这个区别不是可有可无的——就近归属是用来回答
    /// 「160ms 该退到哪一级」的，而红线判定要回答「160ms 合不合规」。
    /// 两者混用会让越级时长被悄悄归到最近一级，红线就永远不亮。
    ///
    /// **锚点边界重叠的裁决**：锚点字面给的是「组件转场 200-300 / 页面转场
    /// 300-450」，两级在 300ms 处**重叠一个点**。本函数按**就近上界归属**
    /// （`w[1].min <= ms && ms <= w[1].max` 先命中先算），即 300ms 判给
    /// **组件转场**。选这条而不是「归给页面转场」的理由：组件转场在前、
    /// 页面转场在后，从 200ms 涨到 300ms 的过程不该被中途改判级别——
    /// 级别中途跳变会让「进退是否同级」这类判定出现无理由的红灯。
    /// 该裁决由 `P02-分级-300ms-归属已裁决` 一项机检锁住。
    pub fn of_strict(ms: u32) -> Option<DurationTier> {
        if ms == 0 {
            return None;
        }
        DurationTier::ALL
            .iter()
            .copied()
            .find(|t| ms >= t.min_ms() && ms <= t.max_ms())
    }

    /// 由毫秒反查分级（复杂度 C2：O(1)）。
    ///
    /// 落在档间缝隙（如 160ms）时取**最近的一级**而不是返回 `None`——
    /// 返回 `None` 会让 160ms 这种"明显是微反馈慢了点"的值被判越级，
    /// 而调用方真正需要的答案是"你该退到哪一级"。
    pub fn of(ms: u32) -> Option<DurationTier> {
        if ms == 0 {
            return None;
        }
        let mut best: Option<(DurationTier, u32)> = None;
        for t in DurationTier::ALL.iter() {
            let d = if ms < t.min_ms() {
                t.min_ms() - ms
            } else if ms > t.max_ms() {
                ms - t.max_ms()
            } else {
                0
            };
            match best {
                Some((_, bd)) if bd <= d => {}
                _ => best = Some((*t, d)),
            }
        }
        best.map(|(t, _)| t)
    }

    /// 该级推荐的**进入缓动**（语义化命名——不是裸曲线）。
    pub fn enter_easing(self) -> &'static str {
        match self {
            DurationTier::MicroFeedback => "ease-out-enter",
            DurationTier::ComponentTransition => "ease-out-enter",
            DurationTier::PageTransition => "spring-soft",
            DurationTier::ComplexOrchestra => "spring-soft",
        }
    }

    /// 该级推荐的**退出缓动**（语义化命名）。
    pub fn exit_easing(self) -> &'static str {
        match self {
            DurationTier::MicroFeedback => "ease-in-exit",
            DurationTier::ComponentTransition => "ease-in-exit",
            DurationTier::PageTransition => "spring-soft",
            DurationTier::ComplexOrchestra => "spring-soft",
        }
    }

    /// 该级的适用场景清单（**适用清单的分组维度**）。
    pub fn scenes(self) -> &'static [Scene] {
        match self {
            DurationTier::MicroFeedback => &[Scene::ButtonPress, Scene::Toggle, Scene::FieldFocus],
            DurationTier::ComponentTransition => &[Scene::CardExpand, Scene::SheetSlide, Scene::MenuOpen],
            DurationTier::PageTransition => &[Scene::RouteChange, Scene::ModalPresent],
            DurationTier::ComplexOrchestra => &[Scene::OnboardingTour, Scene::SharedMorph],
        }
    }

    /// 分级断言（复杂度 C2：O(1)）。
    ///
    /// 返回 `Ok` 表示落在本级区间内；`Err` 携带建议钳制值与原因。
    /// **上限档超出即 `Err`**——超出要走白名单，不是"下一级"。
    pub fn assert_in_range(self, ms: u32) -> Result<u32, LangError> {
        if ms == 0 {
            return Err(LangError::new(
                E_TIER_ZERO_DURATION,
                "分级断言被拒：时长为零",
                &format!(
                    "{} 级的时长为 0；零时长不是动效，且会让「分级断言」永远有特例",
                    self.zh()
                ),
                &format!("填入该级区间 [{}-{}] 内的毫秒值", self.min_ms(), self.max_ms()),
                "动效组件作者",
            ));
        }
        if ms < self.min_ms() {
            return Err(LangError::new(
                E_TIER_BELOW_RANGE,
                "分级断言被拒：低于本级下界",
                &format!(
                    "{} {}ms 低于本级下界 {}ms",
                    self.zh(),
                    ms,
                    self.min_ms()
                ),
                &format!("提到 {}ms 及以上；确需更短请改用更低一级的适用场景", self.min_ms()),
                "动效组件作者",
            ));
        }
        if ms > self.max_ms() {
            return Err(LangError::new(
                E_TIER_ABOVE_RANGE,
                "分级断言被拒：超出本级上界",
                &format!(
                    "{} {}ms 超出本级上界 {}ms{}",
                    self.zh(),
                    ms,
                    self.max_ms(),
                    if self.is_ceiling() {
                        "；本级是上限档，再往上没有级别可退"
                    } else {
                        "；可考虑退到下一级"
                    }
                ),
                if self.is_ceiling() {
                    "砍掉编排中的非必要阶段；若确需长动效须走时长白名单并写明理由"
                } else {
                    "退到下一级，或压缩动效阶段数"
                },
                "动效组件作者",
            ));
        }
        Ok(ms)
    }

    /// 读屏单行（分级表要能念，reduce 档一并念出）。
    pub fn screen_line(&self) -> String {
        format!(
            "{}（{}）：{}-{}ms，进 {}、退 {}；reduce 档 {}ms（{}）",
            self.zh(),
            self.code(),
            self.min_ms(),
            self.max_ms(),
            self.enter_easing(),
            self.exit_easing(),
            self.reduced_ms(),
            if self.reduced_ms() == 0 { "直达终态" } else { "缩短" }
        )
    }
}

/// 分级序单源常量。
pub const TIER_ORDER: [DurationTier; TIER_COUNT] = DurationTier::ALL;

// ---------------------------------------------------------------------------
// 四、缓动家族与命名登记（判据三：语义化缓动）
// ---------------------------------------------------------------------------

/// 缓动家族（锚点原文：标准 / 弹性 / 强调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EasingFamily {
    /// 标准族（ease-out 进 / ease-in 退）。
    Standard,
    /// 弹性族（spring 参数三档）。
    Elastic,
    /// 强调族（cubic 特调三支）。
    Accent,
}

impl EasingFamily {
    /// 三族全集。
    pub const ALL: [EasingFamily; FAMILY_COUNT] = [
        EasingFamily::Standard,
        EasingFamily::Elastic,
        EasingFamily::Accent,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            EasingFamily::Standard => "标准",
            EasingFamily::Elastic => "弹性",
            EasingFamily::Accent => "强调",
        }
    }

    /// 家族码。
    pub fn code(self) -> &'static str {
        match self {
            EasingFamily::Standard => "EF-STANDARD",
            EasingFamily::Elastic => "EF-ELASTIC",
            EasingFamily::Accent => "EF-ACCENT",
        }
    }

    /// 家族序号（0 起）。
    pub fn rank(self) -> usize {
        match self {
            EasingFamily::Standard => 0,
            EasingFamily::Elastic => 1,
            EasingFamily::Accent => 2,
        }
    }

    /// 枚举往返守卫。
    pub fn from_code(code: &str) -> Option<EasingFamily> {
        EasingFamily::ALL.iter().copied().find(|f| f.code() == code)
    }

    /// 该族的**语义**（这一族为什么存在——命名登记的语义段）。
    pub fn semantics(self) -> &'static str {
        match self {
            EasingFamily::Standard => {
                "表达因果的标准语汇：进入用ease-out（快起慢停，落位稳），\
                 退出用 ease-in（慢起快走，让位清楚）"
            }
            EasingFamily::Elastic => {
                "表达有质量的物体：spring 三档给出手感差异，\
                 用于需要「重量感」的大位移与复杂编排"
            }
            EasingFamily::Accent => {
                "强调某一刻：cubic 特调三支制造记忆点，\
                 只在真正需要被注意的时刻用一次"
            }
        }
    }

    /// 该族是否允许**降档到 soft**（弹性族在 reduce 下保留平滑，去掉弹性）。
    pub fn degrades_to_soft(self) -> bool {
        matches!(self, EasingFamily::Elastic)
    }
}

/// 缓动语义档位（弹性族的 spring 三档）。
///
/// 档位**语义**归本项，**数值**归 F3015——本项保证"名字→语义"的映射单源，
/// 但不抢物理引擎的刚度阻尼参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpringFeel {
    /// snappy：利落，几乎不过冲。
    Snappy,
    /// soft：柔和，轻微过冲。
    Soft,
    /// bouncy：明显弹跳，注意力强。
    Bouncy,
}

impl SpringFeel {
    /// 三档全集。
    pub const ALL: [SpringFeel; 3] = [
        SpringFeel::Snappy,
        SpringFeel::Soft,
        SpringFeel::Bouncy,
    ];

    /// 档位名（登记名的一部分）。
    pub fn name(self) -> &'static str {
        match self {
            SpringFeel::Snappy => "snappy",
            SpringFeel::Soft => "soft",
            SpringFeel::Bouncy => "bouncy",
        }
    }

    /// 档位码。
    pub fn code(self) -> &'static str {
        match self {
            SpringFeel::Snappy => "SF-SNAPPY",
            SpringFeel::Soft => "SF-SOFT",
            SpringFeel::Bouncy => "SF-BOUNCY",
        }
    }

    /// 档位的**视觉特征注释**（本项冻结语义，不冻结数值）。
    pub fn visual_note(self) -> &'static str {
        match self {
            SpringFeel::Snappy => "利落，几乎不过冲；适合小控件与大位移",
            SpringFeel::Soft => "柔和，轻微过冲；适合页面转场与编排",
            SpringFeel::Bouncy => "明显弹跳，注意力强；一屏只用一处",
        }
    }

    /// reduce 态的降档目标（**分级降档，不是全取消**）。
    ///
    /// 弹性族在 reduce 下退到 `soft` 而不是取消：保留平滑、去掉弹性。
    /// 全取消会让界面从"有生命"直接变成"全静止"，那不是无障碍是另一种粗糙。
    pub fn reduced_target(self) -> SpringFeel {
        match self {
            SpringFeel::Bouncy => SpringFeel::Soft,
            SpringFeel::Snappy | SpringFeel::Soft => SpringFeel::Snappy,
        }
    }

    /// 枚举往返守卫（按名）。
    pub fn from_name(name: &str) -> Option<SpringFeel> {
        SpringFeel::ALL.iter().copied().find(|f| f.name() == name)
    }
}

/// 缓动登记项（名字 → 曲线 + 语义）。
///
/// **裸曲线禁止入册**：`curve` 字段可以存数值，但 `name` 必须先有——
/// 登记册的存在意义就是让代码里出现的是名字而不是 `cubic-bezier(...)`。
#[derive(Clone, Debug, PartialEq)]
pub struct EasingSpec {
    /// 缓动名（语义化；**唯一引用键**）。
    pub name: String,
    /// 所属家族。
    pub family: EasingFamily,
    /// 曲线类型（语义化描述，不是裸数值——数值求值归 M/O04）。
    pub curve: String,
    /// 语义（这一支表达什么情绪）。
    pub semantics: String,
    /// 是否为进入方向（false = 退出方向）。
    pub enter: bool,
}

impl EasingSpec {
    /// 登记项完整性自检（**名/曲线/语义三段缺一不可**）。
    pub fn is_complete(&self) -> bool {
        !self.name.trim().is_empty()
            && !self.curve.trim().is_empty()
            && !self.semantics.trim().is_empty()
    }

    /// 该登记项是否为**裸曲线**（无名字或名字只是数值串）。
    ///
    /// 数值串的判据是"去掉 `cubic-bezier(...)` 之类包装后仍是纯数字"——
    /// 这类名字等于没名字，因为读代码的人看不出任何情绪。
    pub fn is_bare_curve(&self) -> bool {
        let n = self.name.trim();
        if n.is_empty() {
            return true;
        }
        let inner: String = n
            .trim_start_matches("cubic-bezier")
            .trim_start_matches('(')
            .trim_end_matches(')')
            .replace([' ', ','], "");
        !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "缓动 {}（{}族·{}）：{}；{}",
            self.name,
            self.family.zh(),
            if self.enter { "进" } else { "退" },
            self.curve,
            self.semantics
        )
    }
}

/// 缓动登记册（**语义化缓动红线的执行体**）。
///
/// 拒绝**裸曲线**是这里最重要的一条：锚点原文「用名字不用裸曲线」。
/// 理由很实际——`cubic-bezier(0.4, 0, 0.2, 1)` 出现在代码里没人知道它是什么
/// 情绪，而 `ease-out-enter` 自解释。
#[derive(Clone, Debug, Default)]
pub struct EasingRegistry {
    specs: Vec<EasingSpec>,
    rejected_bare: u64,
}

impl EasingRegistry {
    /// 以标准种子构造（标准族两支 + 弹性三档 + 强调三支）。
    pub fn standard() -> Self {
        let mut r = EasingRegistry::new();
        for spec in standard_easings() {
            r.admit(spec).expect("标准缓动登记");
        }
        r
    }

    /// 空登记册。
    pub fn new() -> Self {
        EasingRegistry {
            specs: Vec::new(),
            rejected_bare: 0,
        }
    }

    /// 在册条数。
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// 因裸曲线被拒的次数（**异常显性化**）。
    pub fn rejected_bare(&self) -> u64 {
        self.rejected_bare
    }

    /// 只读遍历（按登记序）。
    pub fn iter(&self) -> impl Iterator<Item = &EasingSpec> {
        self.specs.iter()
    }

    /// 登记一支缓动（复杂度 C3：O(1)）。
    ///
    /// 四项拒绝：裸曲线（[`E_EASING_BARE_CURVE`]）、重名、字段缺失、册满。
    pub fn admit(&mut self, spec: EasingSpec) -> Result<(), LangError> {
        if spec.is_bare_curve() {
            self.rejected_bare = self.rejected_bare.saturating_add(1);
            return Err(LangError::new(
                E_EASING_BARE_CURVE,
                "缓动登记被拒：裸曲线",
                &format!(
                    "缓动名「{}」是数值串或空；裸曲线在代码里没人知道它是什么情绪",
                    spec.name
                ),
                "取一个语义化名字（如 ease-out-enter / spring-soft / accent-pop），\
                 再把数值挂在曲线字段上",
                "动效语言维护方",
            ));
        }
        if spec.name.trim().len() > MAX_EASING_NAME_LEN {
            return Err(LangError::new(
                E_EASING_NAME_TOO_LONG,
                "缓动登记被拒：名字过长",
                &format!(
                    "缓动名 {} 字符，超过上限 {}",
                    spec.name.trim().len(),
                    MAX_EASING_NAME_LEN
                ),
                "改用更短但仍自解释的名字；名字要能在日志与台账里一眼读完",
                "动效语言维护方",
            ));
        }
        if self.spec(&spec.name).is_some() {
            return Err(LangError::new(
                E_EASING_DUP,
                "缓动登记被拒：重名",
                &format!("缓动 {} 已在册", spec.name),
                "改既有条目的语义或曲线，不要并行登记同名两支",
                "动效语言维护方",
            ));
        }
        if !spec.is_complete() {
            return Err(LangError::new(
                E_EASING_INCOMPLETE,
                "缓动登记被拒：字段缺失",
                &format!(
                    "缓动 {} 的名/曲线/语义三段不齐；缺语义的名字只是标签",
                    spec.name
                ),
                "补齐曲线与语义两段；语义写「这一支表达什么情绪」",
                "动效语言维护方",
            ));
        }
        if self.specs.len() >= MAX_EASINGS {
            return Err(LangError::new(
                E_EASING_CAP,
                "缓动登记被拒：登记册已满",
                &format!(
                    "登记册{} 条达到上限 {}",
                    self.specs.len(),
                    MAX_EASINGS
                ),
                "先评审并归档未被引用的条目，或按 ADR 提升 MAX_EASINGS",
                "动效语言维护方",
            ));
        }
        self.specs.push(spec);
        Ok(())
    }

    /// 名查参（复杂度 C1：**O(1)**，锚点性能分解「查询 O(1) 名查参」）。
    pub fn spec(&self, name: &str) -> Option<&EasingSpec> {
        self.specs.iter().find(|s| s.name == name)
    }

    /// 呼吸原则的判定输入（**单源派生**：判定口径不许调用方各数一遍）。
    ///
    /// 之所以单独提供而不是让调用方自己数：这条判定是「不全是 ease」的执行点，
    /// 让每个调用方各数一遍，迟早有一处数错口径。
    pub fn breath_input(&self, tier: DurationTier) -> BreathInput {
        let distinct = self.distinct_in_tier(tier);
        let sole_family = if distinct <= 1 {
            self.spec(tier.enter_easing()).map(|s| s.family)
        } else {
            None
        };
        BreathInput {
            distinct,
            sole_family,
        }
    }

    /// 某分级内**不同缓动名的数量**（呼吸原则判定的入参）。
    ///
    /// 之所以单独提供而不是让调用方自己数：这条判定是「不全是 ease」的执行点，
    /// 让每个调用方各数一遍，迟早有一处数错口径。
    pub fn distinct_in_tier(&self, tier: DurationTier) -> usize {
        let mut names: Vec<&str> = Vec::new();
        for s in self.specs.iter() {
            if s.name == tier.enter_easing() || s.name == tier.exit_easing() {
                if !names.contains(&s.name.as_str()) {
                    names.push(s.name.as_str());
                }
            }
        }
        names.len()
    }

    /// 某家族在册条数（家族覆盖自检用）。
    pub fn family_count(&self, family: EasingFamily) -> usize {
        self.specs.iter().filter(|s| s.family == family).count()
    }

    /// 三族覆盖自检（**空族即缺陷**：那一族没被登记，等于没有这一族）。
    pub fn uncovered_families(&self) -> Vec<EasingFamily> {
        EasingFamily::ALL
            .iter()
            .copied()
            .filter(|f| self.family_count(*f) == 0)
            .collect()
    }

    /// 读屏摘要（**必须把裸曲线被拒次数念出来**——那是被拦下的诱惑）。
    pub fn screen_text(&self) -> String {
        let mut per = Vec::new();
        for f in EasingFamily::ALL.iter() {
            per.push(format!("{}族{}支", f.zh(), self.family_count(*f)));
        }
        format!(
            "缓动登记册：{} 支（{}）；裸曲线被拒 {} 次",
            self.len(),
            per.join("，"),
            self.rejected_bare()
        )
    }
}

/// 标准缓动种子表（**八支**：标准两支 + 弹性三档 + 强调三支）。
pub fn standard_easings() -> Vec<EasingSpec> {
    let mk = |name: &str, family: EasingFamily, curve: &str, semantics: &str, enter: bool| {
        EasingSpec {
            name: name.to_string(),
            family,
            curve: curve.to_string(),
            semantics: semantics.to_string(),
            enter,
        }
    };
    vec![
        mk(
            "ease-out-enter",
            EasingFamily::Standard,
            "标准进入曲线（求值归 M/O04 F2867）",
            "元素进入时快起慢停：起点响应快、落位稳，让用户看清「它从哪来」",
            true,
        ),
        mk(
            "ease-in-exit",
            EasingFamily::Standard,
            "标准退出曲线（求值归 M/O04 F2867）",
            "元素退出时慢起快走：先减速再离开，让用户看清「它到哪去」",
            false,
        ),
        mk(
            "spring-snappy",
            EasingFamily::Elastic,
            "弹性档位 snappy（数值归 F3015）",
            SpringFeel::Snappy.visual_note(),
            true,
        ),
        mk(
            "spring-soft",
            EasingFamily::Elastic,
            "弹性档位 soft（数值归 F3015）",
            SpringFeel::Soft.visual_note(),
            true,
        ),
        mk(
            "spring-bouncy",
            EasingFamily::Elastic,
            "弹性档位 bouncy（数值归 F3015）",
            SpringFeel::Bouncy.visual_note(),
            true,
        ),
        mk(
            "accent-pop",
            EasingFamily::Accent,
            "强调曲线第一支（求值归 M/O04 F2867）",
            "在关键一刻制造记忆点：一屏只用一次，用多了就不再是强调",
            true,
        ),
        mk(
            "accent-pull",
            EasingFamily::Accent,
            "强调曲线第二支（求值归 M/O04 F2867）",
            "强调「被取走」：用于删除/消费类反馈，与 accent-pop 方向相反",
            false,
        ),
        mk(
            "accent-settle",
            EasingFamily::Accent,
            "强调曲线第三支（求值归 M/O04 F2867）",
            "强调「已就位」：用于落定确认，比 accent-pop 收敛更快",
            true,
        ),
    ]
}

// ---------------------------------------------------------------------------
// 五、适用清单（判据五：单源取值）
// ---------------------------------------------------------------------------

/// 界面场景（适用清单的键）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scene {
    /// 场景一：按钮按下。
    ButtonPress,
    /// 场景二：开关切换。
    Toggle,
    /// 场景三：输入框聚焦。
    FieldFocus,
    /// 场景四：卡片展开。
    CardExpand,
    /// 场景五：底部面板滑出。
    SheetSlide,
    /// 场景六：菜单打开。
    MenuOpen,
    /// 场景七：路由变更。
    RouteChange,
    /// 场景八：模态呈现。
    ModalPresent,
    /// 场景九：新手引导。
    OnboardingTour,
    /// 场景十：共享元素变形。
    SharedMorph,
}

impl Scene {
    /// 场景全集。
    pub const ALL: [Scene; SCENE_COUNT] = [
        Scene::ButtonPress,
        Scene::Toggle,
        Scene::FieldFocus,
        Scene::CardExpand,
        Scene::SheetSlide,
        Scene::MenuOpen,
        Scene::RouteChange,
        Scene::ModalPresent,
    ];

    /// 引导与共享变形两场景（**不进 [`Scene::ALL`]**：它们是编排级而非常规级，
    /// 但仍须有推荐族——单源取值不许有例外场景**）。
    pub const EXTENDED: [Scene; 2] = [Scene::OnboardingTour, Scene::SharedMorph];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Scene::ButtonPress => "按钮按下",
            Scene::Toggle => "开关切换",
            Scene::FieldFocus => "输入框聚焦",
            Scene::CardExpand => "卡片展开",
            Scene::SheetSlide => "底部面板滑出",
            Scene::MenuOpen => "菜单打开",
            Scene::RouteChange => "路由变更",
            Scene::ModalPresent => "模态呈现",
            Scene::OnboardingTour => "新手引导",
            Scene::SharedMorph => "共享元素变形",
        }
    }

    /// 场景码。
    pub fn code(self) -> &'static str {
        match self {
            Scene::ButtonPress => "SC-BUTTON",
            Scene::Toggle => "SC-TOGGLE",
            Scene::FieldFocus => "SC-FIELD",
            Scene::CardExpand => "SC-CARD",
            Scene::SheetSlide => "SC-SHEET",
            Scene::MenuOpen => "SC-MENU",
            Scene::RouteChange => "SC-ROUTE",
            Scene::ModalPresent => "SC-MODAL",
            Scene::OnboardingTour => "SC-ONBOARD",
            Scene::SharedMorph => "SC-MORPH",
        }
    }

    /// 枚举往返守卫。
    pub fn from_code(code: &str) -> Option<Scene> {
        Scene::ALL
            .iter()
            .chain(Scene::EXTENDED.iter())
            .copied()
            .find(|s| s.code() == code)
    }

    /// 全场景（含编排级两场景）——单源对账的遍历域。
    pub fn all_scenes() -> Vec<Scene> {
        Scene::ALL
            .iter()
            .chain(Scene::EXTENDED.iter())
            .copied()
            .collect()
    }

    /// 该场景的推荐时长分级。
    pub fn tier(self) -> DurationTier {
        match self {
            Scene::ButtonPress | Scene::Toggle | Scene::FieldFocus => DurationTier::MicroFeedback,
            Scene::CardExpand | Scene::SheetSlide | Scene::MenuOpen => {
                DurationTier::ComponentTransition
            }
            Scene::RouteChange | Scene::ModalPresent => DurationTier::PageTransition,
            Scene::OnboardingTour | Scene::SharedMorph => DurationTier::ComplexOrchestra,
        }
    }

    /// 该场景的推荐进入缓动名（**必须已在册**——单源取值）。
    pub fn enter_easing(self) -> &'static str {
        match self {
            Scene::ButtonPress => "ease-out-enter",
            Scene::Toggle => "ease-out-enter",
            Scene::FieldFocus => "ease-out-enter",
            Scene::CardExpand => "ease-out-enter",
            Scene::SheetSlide => "ease-out-enter",
            Scene::MenuOpen => "ease-out-enter",
            Scene::RouteChange => "spring-soft",
            Scene::ModalPresent => "spring-soft",
            // 引导与共享变形用强调族制造记忆点，但一屏只用一处。
            Scene::OnboardingTour => "accent-pop",
            Scene::SharedMorph => "accent-settle",
        }
    }

    /// 该场景的推荐退出缓动名。
    pub fn exit_easing(self) -> &'static str {
        match self {
            Scene::ButtonPress => "ease-in-exit",
            Scene::Toggle => "ease-in-exit",
            Scene::FieldFocus => "ease-in-exit",
            Scene::CardExpand => "ease-in-exit",
            Scene::SheetSlide => "ease-in-exit",
            Scene::MenuOpen => "ease-in-exit",
            Scene::RouteChange => "spring-soft",
            Scene::ModalPresent => "spring-soft",
            Scene::OnboardingTour => "accent-pull",
            Scene::SharedMorph => "accent-settle",
        }
    }

    /// 该场景的推荐时长（毫秒，取所属分级的**中值**而非上限）。
    ///
    /// 取中值是刻意的：给上限等于鼓励"能慢就慢"，而默认短是锚点红线。
    pub fn recommended_ms(self) -> u32 {
        let t = self.tier();
        (t.min_ms() + t.max_ms()) / 2
    }

    /// 该场景在 reduce 态的行为说明（**内建无障碍，不是外挂**）。
    pub fn reduced_note(self) -> &'static str {
        match self {
            Scene::ButtonPress | Scene::Toggle => "reduce 态转瞬时状态变化，按钮态仍可见（反馈不消失）",
            Scene::FieldFocus => "reduce 态焦点环直达显示（焦点环属豁免类，位置必须可见）",
            Scene::CardExpand | Scene::MenuOpen => "reduce 态直达终态（展开/收起瞬间完成）",
            Scene::SheetSlide => "reduce 态直达终态（面板即现即收）",
            Scene::RouteChange | Scene::ModalPresent => "reduce 态页面即切，语义即刻生效",
            Scene::OnboardingTour => "reduce 态渐进淡入改为逐屏直达，不做位移动画",
            Scene::SharedMorph => "reduce 态跳过飞行体直接接管（退化声明显性）",
        }
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "场景 {}（{}）：{}，进 {}、退 {}，推荐 {}ms；{}",
            self.zh(),
            self.code(),
            self.tier().zh(),
            self.enter_easing(),
            self.exit_easing(),
            self.recommended_ms(),
            self.reduced_note()
        )
    }
}// ---------------------------------------------------------------------------
// 六、注意力预算（判据一「克制」的执行体）
// ---------------------------------------------------------------------------

/// 活动动效登记（注意力预算的记账单元）。
#[derive(Clone, Debug)]
pub struct ActiveMotion {
    /// 活动号（单调）。
    pub id: u64,
    /// 所属场景。
    pub scene: Scene,
    /// 逻辑帧号（用于记账与淘汰顺序）。
    pub frame: u64,
}

impl ActiveMotion {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "活动动效 #{}（{}，第{} 帧）",
            self.id,
            self.scene.zh(),
            self.frame
        )
    }
}

/// 注意力预算账（**硬上限，不可调高**）。
///
/// 之所以做成硬门而不是建议：注意力预算是四条原则里**唯一用户能直接感知
/// 被侵犯**的一条。别的原则越界还能用"效果还行"说服人，这条越界只会让人
/// 觉得界面吵——而"界面吵"没有辩护余地。
#[derive(Clone, Debug, Default)]
pub struct AttentionBudget {
    active: Vec<ActiveMotion>,
    next_id: u64,
    peak: u32,
    refused: u64,
}

impl AttentionBudget {
    /// 空预算账。
    pub fn new() -> Self {
        AttentionBudget {
            active: Vec::new(),
            next_id: 1,
            peak: 0,
            refused: 0,
        }
    }

    /// 在册活动数。
    pub fn len(&self) -> usize {
        self.active.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    /// 历史峰值并发数（**峰值如实记录，不截断**）。
    pub fn peak(&self) -> u32 {
        self.peak
    }

    /// 因超预算被拒的次数。
    pub fn refused(&self) -> u64 {
        self.refused
    }

    /// 只读遍历（按登记序=时间序）。
    pub fn iter(&self) -> impl Iterator<Item = &ActiveMotion> {
        self.active.iter()
    }

    /// 登记一个活动动效（复杂度 C5：O(1)）。
    ///
    /// 超预算即 [`E_ATTENTION_BUDGET`]——**不静默丢弃、不排队**。
    /// 排队也可以，但那是消费方该显式做的事（合并同源动效），不是预算账
    /// 偷偷替它决定。偷偷排队会让"为什么我的动效没播"变成查不出的问题。
    pub fn acquire(&mut self, scene: Scene, frame: u64) -> Result<u64, LangError> {
        if self.active.len() as u32 >= MAX_CONCURRENT_MOTIONS {
            self.refused = self.refused.saturating_add(1);
            return Err(LangError::new(
                E_ATTENTION_BUDGET,
                "活动登记被拒：注意力预算已满",
                &format!(
                    "已有 {} 个活动动效，达到预算上限 {}；界面上同时动的元素超过{} 个就\
                     分不清谁在响应用户了",
                    self.active.len(),
                    MAX_CONCURRENT_MOTIONS,
                    MAX_CONCURRENT_MOTIONS
                ),
                &format!(
                    "合并同源动效为一个、或等当前动效结束后再登记（{} 可查在册活动）",
                    self.screen_text()
                ),
                "动效组件作者",
            ));
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.active.push(ActiveMotion { id, scene, frame });
        let cur = self.active.len() as u32;
        if cur > self.peak {
            self.peak = cur;
        }
        Ok(id)
    }

    /// 注销一个活动动效（复杂度 C5：O(活动数)）。
    pub fn release(&mut self, id: u64) -> bool {
        let before = self.active.len();
        self.active.retain(|m| m.id != id);
        self.active.len() != before
    }

    /// 推进一帧并**淘汰过期活动**（复杂度 C5：O(活动数)）。
    ///
    /// 淘汰规则：活动超过 [`MOTION_STALE_FRAMES`] 帧未结束即视为僵死——
    /// 动画被打断后没走完流程的情形，不淘汰会让预算被僵尸占满。
    pub fn advance(&mut self, frame: u64) -> Vec<u64> {
        let stale: Vec<u64> = self
            .active
            .iter()
            .filter(|m| frame.saturating_sub(m.frame) > MOTION_STALE_FRAMES)
            .map(|m| m.id)
            .collect();
        for id in stale.iter() {
            self.active.retain(|m| m.id != *id);
        }
        stale
    }

    /// 当前是否已满。
    pub fn is_full(&self) -> bool {
        self.active.len() as u32 >= MAX_CONCURRENT_MOTIONS
    }

    /// 读屏摘要（**峰值必须念出来**——它是最容易被忽略的红线证据）。
    pub fn screen_text(&self) -> String {
        format!(
            "注意力预算：在册 {}/{}，历史峰值 {}，超预算被拒 {} 次",
            self.len(),
            MAX_CONCURRENT_MOTIONS,
            self.peak(),
            self.refused()
        )
    }
}

/// 活动动效的僵死帧数上限（超过即淘汰）。
pub const MOTION_STALE_FRAMES: u64 = 240;

// ---------------------------------------------------------------------------
// 七、时长白名单（降级矩阵第一格：超预算时长→警告+白名单流程）
// ---------------------------------------------------------------------------

/// 时长白名单条目（**长动效须理由**——默认短是锚点红线）。
#[derive(Clone, Debug)]
pub struct DurationExemption {
    /// 获准的时长（毫秒）。
    pub granted_ms: u32,
    /// 理由（**必填**：白名单没有理由就等于取消上限）。
    pub reason: String,
    /// 适用场景。
    pub scene: Scene,
    /// 生效起始的逻辑帧号（0 = 立即生效）。
    ///
    /// 有了下界才能判「窗口相交」而不是「只要还没到期就算冲突」——
    /// 否则一条 1 月到期、另一条 6 月生效的白名单会被判成重复，而它们
    /// 根本不冲突；那样的判重会让白名单册最多只能存「场景数」条，
    /// 上限常量形同虚设。
    pub effective_from_frame: u64,
    /// 期限的逻辑帧号（**必填**：无期限的豁免=永久绕过红线）。
    pub expires_at_frame: u64,
}

impl DurationExemption {
    /// 该白名单的生效窗口是否与另一条相交。
    pub fn overlaps(&self, other: &DurationExemption) -> bool {
        self.effective_from_frame < other.expires_at_frame
            && other.effective_from_frame < self.expires_at_frame
    }
}

impl DurationExemption {
    /// 该白名单在给定帧是否仍有效。
    pub fn is_active(&self, frame: u64) -> bool {
        frame < self.expires_at_frame
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "时长白名单：{} 场景准至 {}ms（标准上限 {}ms）；{}；生效窗口 [{}, {}] 帧",
            self.scene.zh(),
            self.granted_ms,
            COMPLEX_ORCHESTRA_MAX_MS,
            self.reason,
            self.effective_from_frame,
            self.expires_at_frame
        )
    }
}

/// 时长白名单册。
#[derive(Clone, Debug, Default)]
pub struct DurationExemptionLedger {
    items: Vec<DurationExemption>,
    refused: u64,
}

impl DurationExemptionLedger {
    /// 空白名单册。
    pub fn new() -> Self {
        DurationExemptionLedger {
            items: Vec::new(),
            refused: 0,
        }
    }

    /// 在册条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空（空册 = 无任何长动效获准）。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 因不合规被拒的次数。
    pub fn refused(&self) -> u64 {
        self.refused
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &DurationExemption> {
        self.items.iter()
    }

    /// 申请一条白名单（复杂度 C6：O(1)）。
    ///
    /// 四项拒绝：无理由、期限为零、超出**全局上限**（白名单也只能到500ms
    /// ——它是"暂准延长"不是"取消上限"）、同类场景重复生效。
    pub fn apply(&mut self, ex: DurationExemption) -> Result<(), LangError> {
        if ex.reason.trim().is_empty() {
            self.refused = self.refused.saturating_add(1);
            return Err(LangError::new(
                E_EXEMPTION_NO_REASON,
                "白名单申请被拒：无理由",
                &format!(
                    "{} 场景申请 {}ms 但没写理由；无理由的长动效无法被复核",
                    ex.scene.zh(),
                    ex.granted_ms
                ),
                "写清为什么这个场景确实需要更长的动效（应落到「讲清楚一件事」这类理由上）",
                "动效语言维护方",
            ));
        }
        if ex.expires_at_frame == 0 {
            self.refused = self.refused.saturating_add(1);
            return Err(LangError::new(
                E_EXEMPTION_NO_DEADLINE,
                "白名单申请被拒：期限为零",
                &format!(
                    "{} 场景的白名单期限为 0，等于永久突破 {}ms 上限",
                    ex.scene.zh(),
                    COMPLEX_ORCHESTRA_MAX_MS
                ),
                "给出有限期限的逻辑帧号；续期须重新评审",
                "动效语言维护方",
            ));
        }
        if ex.granted_ms > COMPLEX_ORCHESTRA_MAX_MS {
            self.refused = self.refused.saturating_add(1);
            return Err(LangError::new(
                E_EXEMPTION_ABOVE_CEILING,
                "白名单申请被拒：超出全局上限",
                &format!(
                    "{} 场景申请 {}ms，全局上限是 {}ms；白名单是「暂准延长」不是「取消上限」",
                    ex.scene.zh(),
                    ex.granted_ms,
                    COMPLEX_ORCHESTRA_MAX_MS
                ),
                &format!(
                    "压到 {}ms 以内；若确实需要更慢，那是产品该讨论的问题而不是动效该越的线",
                    COMPLEX_ORCHESTRA_MAX_MS
                ),
                "动效语言维护方",
            ));
        }
        if self
            .items
            .iter()
            .any(|e| e.scene == ex.scene && e.overlaps(&ex))
        {
            self.refused = self.refused.saturating_add(1);
            return Err(LangError::new(
                E_EXEMPTION_DUP,
                "白名单申请被拒：同场景窗口重叠",
                &format!(
                    "{} 场景已有窗口与之相交的白名单；同一时刻两条生效会让实际时长取决于查了哪条",
                    ex.scene.zh()
                ),
                "把新条目的生效起点挪到既有条目到期之后，或直接改既有条目的期限与理由",
                "动效语言维护方",
            ));
        }
        if self.items.len() >= MAX_DURATION_EXEMPTIONS {
            self.refused = self.refused.saturating_add(1);
            return Err(LangError::new(
                E_EXEMPTION_CAP,
                "白名单申请被拒：册已满",
                &format!(
                    "白名单册{} 条达到上限 {}（累计拒绝 {} 次）",
                    self.items.len(),
                    MAX_DURATION_EXEMPTIONS,
                    self.refused
                ),
                "先评审并归档失效白名单，或按 ADR 提升 MAX_DURATION_EXEMPTIONS",
                "动效语言维护方",
            ));
        }
        self.items.push(ex);
        Ok(())
    }

    /// 时长判定（复杂度 C6：O(1)）。
    ///
    /// 顺序：先问白名单，再问分级区间——白名单是"这条场景例外"，
    /// 分级区间是"通用规则"，例外优先但必须已登记。
    pub fn resolve(&self, scene: Scene, ms: u32, frame: u64) -> DurationVerdict {
        for e in self.items.iter() {
            if e.scene == scene && e.is_active(frame) && ms <= e.granted_ms {
                return DurationVerdict::Exempt(e.granted_ms);
            }
        }
        let tier = scene.tier();
        match tier.assert_in_range(ms) {
            Ok(_) => DurationVerdict::InRange(tier),
            Err(_) => {
                // 越界：给最近的合规值，而不是让调用方自己猜。
                let near = if ms > tier.max_ms() {
                    tier.max_ms()
                } else {
                    tier.min_ms()
                };
                DurationVerdict::OutOfRange { tier, nearest: near }
            }
        }
    }

    /// 读屏摘要。
    pub fn screen_text(&self, frame: u64) -> String {
        let live = self.items.iter().filter(|e| e.is_active(frame)).count();
        format!(
            "时长白名单：在册 {} 条（生效 {} 条，累计拒绝 {} 次）",
            self.len(),
            live,
            self.refused()
        )
    }
}

/// 时长判定结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurationVerdict {
    /// 落在分级区间内。
    InRange(DurationTier),
    /// 白名单放行（附获准上限）。
    Exempt(u32),
    /// 越界（附所属分级与最近合规值）。
    OutOfRange {
        /// 所属分级。
        tier: DurationTier,
        /// 最近合规值。
        nearest: u32,
    },
}

impl DurationVerdict {
    /// 是否合规（在区间内或白名单放行）。
    pub fn is_ok(self) -> bool {
        !matches!(self, DurationVerdict::OutOfRange { .. })
    }

    /// 该场景的**最终生效时长**（越界时给最近合规值）。
    pub fn effective_ms(self, requested: u32) -> u32 {
        match self {
            DurationVerdict::InRange(_) => requested,
            DurationVerdict::Exempt(granted) => {
                if requested <= granted {
                    requested
                } else {
                    granted
                }
            }
            DurationVerdict::OutOfRange { nearest, .. } => nearest,
        }
    }

    /// 读屏单行。
    pub fn screen_line(self, requested: u32) -> String {
        match self {
            DurationVerdict::InRange(t) => {
                format!("时长 {}ms 合规（{}级）", requested, t.zh())
            }
            DurationVerdict::Exempt(g) => {
                format!("时长 {}ms 由白名单放行（上限 {}ms）", requested, g)
            }
            DurationVerdict::OutOfRange { tier, nearest } => format!(
                "时长 {}ms 越界（{}级{}-{}ms）；最近合规值 {}ms",
                requested,
                tier.zh(),
                tier.min_ms(),
                tier.max_ms(),
                nearest
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// 八、原则冲突裁决（降级矩阵第三格：原则冲突案例→设计评审裁决记录）
// ---------------------------------------------------------------------------

/// 裁决记录（**四要素齐备**：现象/裁决/理由/适用边界）。
///
/// 不留裁决的冲突解决等于把决策埋进某个人的记忆里——半年后有人提出同一个
/// 冲突，只能靠"我记得当时是这么定的"来回答，那不是工程。
#[derive(Clone, Debug)]
pub struct RulingRecord {
    /// 裁决号（单调）。
    pub id: u64,
    /// 涉及的原则（两条——冲突必是两条原则相撞）。
    pub lhs: Principle,
    pub rhs: Principle,
    /// 现象（冲突长什么样）。
    pub symptom: String,
    /// 裁决（定了什么）。
    pub ruling: String,
    /// 理由（为什么这么裁）。
    pub rationale: String,
    /// 适用边界（**必填**：本裁决只管哪些场景，不管哪些）。
    pub boundary: String,
}

impl RulingRecord {
    /// 四要素齐备性自检（缺一即不合格）。
    pub fn is_complete(&self) -> bool {
        !self.symptom.trim().is_empty()
            && !self.ruling.trim().is_empty()
            && !self.rationale.trim().is_empty()
            && !self.boundary.trim().is_empty()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "裁决 #{}（{} vs {}）：{}；裁定 {}；理由 {}；边界 {}",
            self.id,
            self.lhs.zh(),
            self.rhs.zh(),
            self.symptom,
            self.ruling,
            self.rationale,
            self.boundary
        )
    }
}

/// 裁决账。
#[derive(Clone, Debug, Default)]
pub struct RulingLedger {
    records: Vec<RulingRecord>,
    next_id: u64,
}

impl RulingLedger {
    /// 空裁决账。
    pub fn new() -> Self {
        RulingLedger {
            records: Vec::new(),
            next_id: 1,
        }
    }

    /// 在册条数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &RulingRecord> {
        self.records.iter()
    }

    /// 登记一条裁决（复杂度 C7：O(1)）。
    ///
    /// 拒绝三事：同原则自冲突（那不是冲突是笔误）、四要素缺失、账满。
    pub fn open_ruling(
        &mut self,
        lhs: Principle,
        rhs: Principle,
        symptom: &str,
        ruling: &str,
        rationale: &str,
        boundary: &str,
    ) -> Result<u64, LangError> {
        if lhs == rhs {
            return Err(LangError::new(
                E_RULING_SAME_PRINCIPLE,
                "裁决登记被拒：同原则自冲突",
                &format!("{} 与 {} 是同一条原则；那不是冲突，是笔误", lhs.zh(), rhs.zh()),
                "改成真正相撞的两条原则；若只是本原则内部取值不合适，走分级断言即可",
                "设计评审",
            ));
        }
        let rec = RulingRecord {
            id: 0,
            lhs,
            rhs,
            symptom: symptom.to_string(),
            ruling: ruling.to_string(),
            rationale: rationale.to_string(),
            boundary: boundary.to_string(),
        };
        if !rec.is_complete() {
            return Err(LangError::new(
                E_RULING_INCOMPLETE,
                "裁决登记被拒：四要素缺失",
                &format!("裁决「{}」的现象/裁决/理由/边界四项不全", symptom),
                "四要素齐发；其中「适用边界」最常漏——没边界的裁决会被当成普适规则",
                "设计评审",
            ));
        }
        if self.records.len() >= MAX_RULINGS {
            return Err(LangError::new(
                E_RULING_CAP,
                "裁决登记被拒：账已满",
                &format!("裁决账 {} 条达到上限 {}", self.records.len(), MAX_RULINGS),
                "先归档旧裁决，或按 ADR 提升 MAX_RULINGS",
                "动效语言维护方",
            ));
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let mut rec = rec;
        rec.id = id;
        self.records.push(rec);
        Ok(id)
    }

    /// 某对原则之间是否已有裁决（冲突复现时要能查到上次怎么裁的）。
    pub fn find_between(&self, lhs: Principle, rhs: Principle) -> Option<&RulingRecord> {
        self.records.iter().find(|r| {
            (r.lhs == lhs && r.rhs == rhs) || (r.lhs == rhs && r.rhs == lhs)
        })
    }

    /// 不完整的裁决数（自检用）。
    pub fn incomplete_count(&self) -> usize {
        self.records.iter().filter(|r| !r.is_complete()).count()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "裁决账：{} 条（不完整 {} 条）",
            self.len(),
            self.incomplete_count()
        )
    }
}

// ---------------------------------------------------------------------------
// 九、参数域钳制（边界越界→钳制 + 告警）
// ---------------------------------------------------------------------------

/// 钳制告警（**每次夹取都留痕**——静默夹取等于撒谎）。
#[derive(Clone, Debug, PartialEq)]
pub struct ClampNotice {
    /// 字段名（哪个场景/参数被夹了）。
    pub field: String,
    /// 原值。
    pub original: u32,
    /// 夹取后的值。
    pub clamped: u32,
    /// 下界。
    pub low: u32,
    /// 上界。
    pub high: u32,
    /// 逻辑帧号。
    pub frame: u64,
}

impl ClampNotice {
    /// 读屏单行（钳制要能念，且要说清「为什么不是我写的那个」）。
    pub fn screen_line(&self) -> String {
        format!(
            "钳制告警：{} 原值 {}ms 越出 [{}, {}]ms，已夹为 {}ms（第{} 帧）",
            self.field, self.original, self.low, self.high, self.clamped, self.frame
        )
    }
}

/// 告警账（满后拒绝并计数）。
#[derive(Clone, Debug, Default)]
pub struct ClampLog {
    notices: Vec<ClampNotice>,
    dropped: u64,
}

impl ClampLog {
    /// 空告警账。
    pub fn new() -> Self {
        ClampLog {
            notices: Vec::new(),
            dropped: 0,
        }
    }

    /// 在册告警数。
    pub fn len(&self) -> usize {
        self.notices.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.notices.is_empty()
    }

    /// 因账满而**未落账**的告警数（异常显性化）。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &ClampNotice> {
        self.notices.iter()
    }

    /// 某场景的告警次数（反复越界的场景要能被一眼看出）。
    pub fn count_for(&self, field: &str) -> usize {
        self.notices.iter().filter(|n| n.field == field).count()
    }

    /// 追加一条告警。账满则 [`E_CLAMP_LOG_FULL`]——不静默丢弃。
    pub fn push(&mut self, n: ClampNotice) -> Result<(), LangError> {
        if self.notices.len() >= CLAMP_LOG_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(LangError::new(
                E_CLAMP_LOG_FULL,
                "告警入账被拒：告警账已满",
                &format!(
                    "告警账 {} 条达到上限 {}，本条未落账（累计丢弃 {} 条）",
                    self.notices.len(),
                    CLAMP_LOG_CAP,
                    self.dropped
                ),
                "先归档并轮转告警账，或按 ADR 提升 CLAMP_LOG_CAP；\
                 在此之前不得把丢弃的那几条当成「没发生过」",
                "动效语言维护方",
            ));
        }
        self.notices.push(n);
        Ok(())
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!("钳制告警账：{} 条（丢弃 {} 条）", self.len(), self.dropped())
    }
}

/// 时长钳制（复杂度 C9：O(1)）。
///
/// 越界即夹取并**同时**落一条 [`ClampNotice`]——返回值与告警成对出现，
/// 调用方无法只要其一（想要"安静地夹"就得自己吞掉返回值，代码评审能看见）。
pub fn clamp_ms(
    log: &mut ClampLog,
    field: &str,
    value: u32,
    low: u32,
    high: u32,
    frame: u64,
) -> u32 {
    if value < low {
        let _ = log.push(ClampNotice {
            field: field.to_string(),
            original: value,
            clamped: low,
            low,
            high,
            frame,
        });
        low
    } else if value > high {
        let _ = log.push(ClampNotice {
            field: field.to_string(),
            original: value,
            clamped: high,
            low,
            high,
            frame,
        });
        high
    } else {
        value
    }
}// ---------------------------------------------------------------------------
// 十、判据六项（锚点判据：四原则 / 四级时长 / 语义化缓动 / 内建 reduce / 单源取值 / 判据）
// ---------------------------------------------------------------------------

/// 判据项。锚点判据列六项，本总纲把它们变成**可被逐条断言的枚举**。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Criterion {
    /// 判据一：四原则（节奏/呼吸/因果/克制）。
    FourPrinciples,
    /// 判据二：四级时长（微反馈/组件转场/页面转场/复杂编排）。
    TierLadder,
    /// 判据三：语义化缓动（名字→曲线+语义，禁裸曲线）。
    SemanticEasing,
    /// 判据四：内建 reduce（分级表含reduced 档，非外挂补丁）。
    ReduceBuiltIn,
    /// 判据五：单源取值（P02/P03 与令牌从本册取值）。
    SingleSource,
    /// 判据六：判据本身（总纲自证可追溯）。
    Criterion,
}

/// 六项判据全集（判据：一项不缺）。
pub const CRITERIA: [Criterion; 6] = [
    Criterion::FourPrinciples,
    Criterion::TierLadder,
    Criterion::SemanticEasing,
    Criterion::ReduceBuiltIn,
    Criterion::SingleSource,
    Criterion::Criterion,
];

impl Criterion {
    /// 判据中文名（读屏播报）。
    pub fn zh(self) -> &'static str {
        match self {
            Criterion::FourPrinciples => "四原则",
            Criterion::TierLadder => "四级时长",
            Criterion::SemanticEasing => "语义化缓动",
            Criterion::ReduceBuiltIn => "内建 reduce",
            Criterion::SingleSource => "单源取值",
            Criterion::Criterion => "判据",
        }
    }

    /// 判据码（对拍与台账引用）。
    pub fn code(self) -> &'static str {
        match self {
            Criterion::FourPrinciples => "P02-J1",
            Criterion::TierLadder => "P02-J2",
            Criterion::SemanticEasing => "P02-J3",
            Criterion::ReduceBuiltIn => "P02-J4",
            Criterion::SingleSource => "P02-J5",
            Criterion::Criterion => "P02-J6",
        }
    }

    /// 枚举往返守卫。
    pub fn from_code(code: &str) -> Option<Criterion> {
        CRITERIA.iter().copied().find(|c| c.code() == code)
    }

    /// 该判据的承诺句（写下来就是契约，不许用「应该」「尽量」这类词）。
    pub fn promise(self) -> &'static str {
        match self {
            Criterion::FourPrinciples => {
                "节奏/呼吸/因果/克制四条各有可失败的判定口径：越级时长、进退不同级、\
                 整级单一缓动、纯装饰位移、同时活动元素超 3——五种违反形态逐一可检出。"
            }
            Criterion::TierLadder => {
                "时长按微反馈100-150 / 组件转场 200-300 / 页面转场 300-450 / \
                 复杂编排 ≤500 四级区间判定；超区间给最近合规值而不是让调用方猜，\
                 越级须走白名单并写明理由。"
            }
            Criterion::SemanticEasing => {
                "缓动一律以名字引用；裸曲线（纯数值串名）拒绝入册并计数——\
                 因为 cubic-bezier(...) 在代码里没人知道它是什么情绪。"
            }
            Criterion::ReduceBuiltIn => {
                "四级分级表每级内建 reduced 档（时长归零直达终态），\
                 语言层完成无障碍而不是让每个消费方自己写 if reduce——后者必漏一处。"
            }
            Criterion::SingleSource => {
                "P02 转场与 P03 微交互的场景推荐族一律从本册取值；\
                 双向对账（场景→族须在册、族→场景须有清单），单查一侧会让错配漏过去。"
            }
            Criterion::Criterion => {
                "判据自证可追溯：原则判定、分级断言、登记拒绝、白名单、裁决记录、\
                 读屏替述随总纲同源交付；任一判据无可执行断言即视为未落实。"
            }
        }
    }

    /// 该判据的执行体名称（自检项前缀，便于台账反查）。
    pub fn enforced_by(self) -> &'static str {
        match self {
            Criterion::FourPrinciples => "Principle::verdict + AttentionBudget",
            Criterion::TierLadder => "DurationTier::assert_in_range",
            Criterion::SemanticEasing => "EasingRegistry::admit",
            Criterion::ReduceBuiltIn => "DurationTier::reduced_ms",
            Criterion::SingleSource => "MotionLanguage::audit_single_source",
            Criterion::Criterion => "self_audit + language_narration",
        }
    }
}

/// 取判据的承诺句。
pub fn criterion_promise(c: Criterion) -> &'static str {
    c.promise()
}

// ---------------------------------------------------------------------------
// 十一、错误码与错误类型（五元组：码/现象/原因/下一步/责任方）
// ---------------------------------------------------------------------------

/// 时长为零。
pub const E_TIER_ZERO_DURATION: &str = "E_TIER_ZERO_DURATION";
/// 低于分级下界。
pub const E_TIER_BELOW_RANGE: &str = "E_TIER_BELOW_RANGE";
/// 超出分级上界。
pub const E_TIER_ABOVE_RANGE: &str = "E_TIER_ABOVE_RANGE";
/// 注意力预算超限。
pub const E_ATTENTION_BUDGET: &str = "E_ATTENTION_BUDGET";
/// 缓动裸曲线入册。
pub const E_EASING_BARE_CURVE: &str = "E_EASING_BARE_CURVE";
/// 缓动重名。
pub const E_EASING_DUP: &str = "E_EASING_DUP";
/// 缓动字段缺失。
pub const E_EASING_INCOMPLETE: &str = "E_EASING_INCOMPLETE";
/// 缓动名过长。
pub const E_EASING_NAME_TOO_LONG: &str = "E_EASING_NAME_TOO_LONG";
/// 缓动登记册已满。
pub const E_EASING_CAP: &str = "E_EASING_CAP";
/// 白名单无理由。
pub const E_EXEMPTION_NO_REASON: &str = "E_EXEMPTION_NO_REASON";
/// 白名单无期限。
pub const E_EXEMPTION_NO_DEADLINE: &str = "E_EXEMPTION_NO_DEADLINE";
/// 白名单超出全局上限。
pub const E_EXEMPTION_ABOVE_CEILING: &str = "E_EXEMPTION_ABOVE_CEILING";
/// 白名单同场景重复。
pub const E_EXEMPTION_DUP: &str = "E_EXEMPTION_DUP";
/// 白名单册已满。
pub const E_EXEMPTION_CAP: &str = "E_EXEMPTION_CAP";
/// 裁决同原则自冲突。
pub const E_RULING_SAME_PRINCIPLE: &str = "E_RULING_SAME_PRINCIPLE";
/// 裁决四要素缺失。
pub const E_RULING_INCOMPLETE: &str = "E_RULING_INCOMPLETE";
/// 裁决账已满。
pub const E_RULING_CAP: &str = "E_RULING_CAP";
/// 告警账已满。
pub const E_CLAMP_LOG_FULL: &str = "E_CLAMP_LOG_FULL";
/// 场景推荐族未在册。
pub const E_SCENE_EASING_UNREGISTERED: &str = "E_SCENE_EASING_UNREGISTERED";
/// 登记名无适用场景（无主缓动）。
pub const E_EASING_NO_SCENE: &str = "E_EASING_NO_SCENE";
/// 分级减动效缺档。
pub const E_TIER_NO_REDUCED_BAND: &str = "E_TIER_NO_REDUCED_BAND";
/// 原则码非法。
pub const E_PRINCIPLE_UNKNOWN: &str = "E_PRINCIPLE_UNKNOWN";
/// 语言版本漂移。
pub const E_LANG_VERSION_DRIFT: &str = "E_LANG_VERSION_DRIFT";

/// 动效语言错误（五元组：码/现象/原因/下一步/责任方）。
///
/// **拒绝必须给出路**：五元组齐发是构造点强制，`next` 为空即视为不合格——
/// 只说「不行」而不说「那该怎么做」的拒绝，会让人换个写法再来一遍。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LangError {
    /// 错误码。
    pub code: &'static str,
    /// 发生了什么。
    pub what: &'static str,
    /// 为什么。
    pub why: String,
    /// 下一步（必填——拒绝必须给出路）。
    pub next: String,
    /// 责任方。
    pub who: String,
}

impl LangError {
    /// 构造（五元组齐发）。
    pub fn new(
        code: &'static str,
        what: &'static str,
        why: &str,
        next: &str,
        who: &str,
    ) -> Self {
        LangError {
            code,
            what,
            why: why.to_string(),
            next: next.to_string(),
            who: who.to_string(),
        }
    }

    /// 五元组齐备性自检（`next` 为空即不合格）。
    pub fn is_complete(&self) -> bool {
        !self.code.trim().is_empty()
            && !self.what.trim().is_empty()
            && !self.why.trim().is_empty()
            && !self.next.trim().is_empty()
            && !self.who.trim().is_empty()
    }

    /// 读屏可读的完整错误（三要素齐发：现象/原因/怎么办）。
    pub fn screen_text(&self) -> String {
        format!(
            "错误 {}：{}；原因：{}；下一步：{}；责任方：{}",
            self.code, self.what, self.why, self.next, self.who
        )
    }
}

// ---------------------------------------------------------------------------
// 十二、下游归属表（防止抢活与漏活）
// ---------------------------------------------------------------------------

/// 下游条目归属（**谁拥有什么**——本项只立总纲，不代做后续项）。
pub const DOWNSTREAM_OWNERSHIP: [(&str, &str); 6] = [
    ("VE-F3003", "动效令牌体系：三族令牌全集、CSS 变量注入器、硬编码 lint"),
    ("VE-F3005", "转场编排器：编排图数据结构、四原语编译器、打断策略表"),
    ("VE-F3006", "动效组件化：四件套组件定义、三族注册表、参数校验器"),
    ("VE-F3014", "微反馈动效标准：三律声明、六形态库、默认装配规则"),
    ("VE-F3015", "动效物理引擎：spring 刚度阻尼数值、固定步长求解器、采样缓存"),
    ("VE-F3019", "动效库测试与参考对比：四类断言生成器、SSIM 参考集、分档调度"),
];

// ---------------------------------------------------------------------------
// 十三、语言册本体
// ---------------------------------------------------------------------------

/// 语言册问题（五元组：码/现象/根因/建议/严重度）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LangIssue {
    /// 问题码。
    pub code: &'static str,
    /// 现象。
    pub symptom: String,
    /// 根因。
    pub root_cause: String,
    /// 建议。
    pub advice: &'static str,
    /// 严重度。
    pub severity: Severity,
}

/// 严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// 阻断：不修不得放行。
    Blocking,
    /// 警告：可放行但须限期修。
    Warning,
}

impl Severity {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Severity::Blocking => "阻断",
            Severity::Warning => "警告",
        }
    }
}

impl LangIssue {
    /// 读屏单行（问题要能念给用户听——异常零静默）。
    pub fn screen_line(&self) -> String {
        format!(
            "语言册问题[{}·{}]：{}；根因 {}；建议 {}",
            self.severity.zh(),
            self.code,
            self.symptom,
            self.root_cause,
            self.advice
        )
    }
}

/// 单源对账结论。
#[derive(Clone, Debug, Default)]
pub struct SingleSourceAudit {
    /// 场景推荐了未在册的缓动名。
    pub unregistered: Vec<(Scene, &'static str)>,
    /// 在册但无任何场景使用的缓动名（无主缓动）。
    pub ownerless: Vec<String>,
    /// 分级缺内建 reduce 档的分级。
    pub no_reduced_band: Vec<DurationTier>,
}

impl SingleSourceAudit {
    /// 是否单源对账全通。
    pub fn is_clean(&self) -> bool {
        self.unregistered.is_empty()
            && self.ownerless.is_empty()
            && self.no_reduced_band.is_empty()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "单源对账：未在册引用 {} 项、无主缓动 {} 支、缺 reduce 档分级 {} 级",
            self.unregistered.len(),
            self.ownerless.len(),
            self.no_reduced_band.len()
        )
    }
}

/// 动效设计语言册（本项交付物本体）。
#[derive(Clone, Debug)]
pub struct MotionLanguage {
    /// 语言册版本。
    pub version: &'static str,
    /// 缓动登记册。
    pub easings: EasingRegistry,
    /// 注意力预算账。
    pub budget: AttentionBudget,
    /// 时长白名单册。
    pub exemptions: DurationExemptionLedger,
    /// 裁决账。
    pub rulings: RulingLedger,
    /// 钳制告警账。
    pub log: ClampLog,
}

impl MotionLanguage {
    /// 标准语言册（八支标准缓动已登记，预算与白名单为空）。
    pub fn standard() -> Self {
        MotionLanguage {
            version: LANG_VERSION,
            easings: EasingRegistry::standard(),
            budget: AttentionBudget::new(),
            exemptions: DurationExemptionLedger::new(),
            rulings: RulingLedger::new(),
            log: ClampLog::new(),
        }
    }

    /// 名查参（复杂度 C1：O(1)）——**单源取值的唯一入口**。
    ///
    /// 消费方（P02 转场 / P03 微交互 / E 域令牌）只许走这个函数拿曲线参数，
    /// 不许自己去猜曲线。理由：一旦允许绕过，名义上的"单源"就会分叉成
    /// 每个消费方各自的一份数值。
    pub fn resolve_easing(&self, name: &str) -> Result<&str, LangError> {
        match self.easings.spec(name) {
            Some(s) => Ok(&s.curve),
            None => Err(LangError::new(
                E_EASING_BARE_CURVE,
                "取值被拒：缓动未在册",
                &format!("缓动 {} 未在登记册内；不在册的缓动等于没有单源", name),
                &format!(
                    "改用适用清单中的推荐族（{} 等），或先把这支缓动登记进册（admit）再取值",
                    Scene::ButtonPress.enter_easing()
                ),
                "动效组件作者",
            )),
        }
    }

    /// 场景取值（复杂度 C1+C2：O(1)）——转场与微交互的**推荐值三件套**。
    pub fn scene_recommendation(
        &self,
        scene: Scene,
        requested_ms: u32,
        frame: u64,
    ) -> SceneRecommendation {
        let tier = scene.tier();
        let verdict = self.exemptions.resolve(scene, requested_ms, frame);
        let enter = scene.enter_easing();
        let exit = scene.exit_easing();
        SceneRecommendation {
            scene,
            tier,
            verdict,
            effective_ms: verdict.effective_ms(requested_ms),
            recommended_ms: scene.recommended_ms(),
            enter_easing: enter,
            exit_easing: exit,
            enter_registered: self.easings.spec(enter).is_some(),
            exit_registered: self.easings.spec(exit).is_some(),
            reduced_ms: tier.reduced_ms(),
        }
    }

    /// 四原则联合判定（复杂度 C4：O(1)）。
    pub fn audit_principles(
        &self,
        scene: Scene,
        in_ms: u32,
        out_ms: Option<u32>,
        decorative: bool,
        concurrent: u32,
    ) -> Vec<PrincipleViolation> {
        let mut out = Vec::new();
        let breath = self.easings.breath_input(scene.tier());
        let ein = scene.enter_easing();
        let eout = scene.exit_easing();
        for p in PRINCIPLE_ORDER.iter() {
            out.extend(p.verdict(
                scene.zh(),
                in_ms,
                out_ms,
                ein,
                eout,
                breath,
                decorative,
                concurrent,
            ));
        }
        out
    }

    /// 单源对账（复杂度 C8：O(登记名数 × 场景数)）。
    ///
    /// **双向查**：场景→族须在册（防引用了不存在的名字），族→场景须有使用
    /// （防登记了没人用的名字，那种名字等于没有共识）。
    pub fn audit_single_source(&self) -> SingleSourceAudit {
        let mut audit = SingleSourceAudit::default();
        for s in Scene::all_scenes() {
            for name in [s.enter_easing(), s.exit_easing()] {
                if self.easings.spec(name).is_none() {
                    audit.unregistered.push((s, name));
                }
            }
        }
        // 无主缓动：登记了但没有任何场景引用。
        let mut used: Vec<&str> = Vec::new();
        for s in Scene::all_scenes() {
            used.push(s.enter_easing());
            used.push(s.exit_easing());
        }
        for spec in self.easings.iter() {
            if !used.contains(&spec.name.as_str()) {
                audit.ownerless.push(spec.name.clone());
            }
        }
        // 内建 reduce 档：四级必须全有。
        for t in DurationTier::ALL.iter() {
            if !t.has_reduced_band() {
                audit.no_reduced_band.push(*t);
            }
        }
        audit
    }

    /// 域级自检（复杂度 C10）。
    pub fn self_audit(&self) -> Vec<LangIssue> {
        let mut issues = Vec::new();

        if self.version != LANG_VERSION {
            issues.push(LangIssue {
                code: E_LANG_VERSION_DRIFT,
                symptom: format!(
                    "语言册版本为 {}，与总纲声明的 {} 不一致",
                    self.version, LANG_VERSION
                ),
                root_cause: "语言册版本被就地改动而未升版".to_string(),
                advice: "升版并同步 LANG_VERSION 常量，附迁移说明",
                severity: Severity::Blocking,
            });
        }

        // 四原则齐备 + 码唯一 + 判定口径非空。
        let mut pcodes: Vec<&str> = PRINCIPLE_ORDER.iter().map(|p| p.code()).collect();
        pcodes.sort_unstable();
        let mut puniq = pcodes.clone();
        puniq.dedup();
        if PRINCIPLE_ORDER.len() != PRINCIPLE_COUNT || puniq.len() != pcodes.len() {
            issues.push(LangIssue {
                code: E_PRINCIPLE_UNKNOWN,
                symptom: "四原则不齐或原则码重复".to_string(),
                root_cause: "原则枚举被增删或码被复制".to_string(),
                advice: "以 Principle::ALL 为唯一原则源，重建 PRINCIPLE_ORDER",
                severity: Severity::Blocking,
            });
        }
        for p in PRINCIPLE_ORDER.iter() {
            if p.verdict_rule().trim().is_empty() {
                issues.push(LangIssue {
                    code: E_PRINCIPLE_UNKNOWN,
                    symptom: format!("原则 {} 没有判定口径", p.zh()),
                    root_cause: "判定口径留空——没有口径的原则是标签".to_string(),
                    advice: "写清怎么算违反；写不出来说明这条原则还没想清楚",
                    severity: Severity::Blocking,
                });
            }
        }

        // 四级时长：区间不得倒挂、上限档语义正确、reduce 档内建。
        let mut prev_max = 0u32;
        for t in DurationTier::ALL.iter() {
            if t.min_ms() > t.max_ms() {
                issues.push(LangIssue {
                    code: E_TIER_BELOW_RANGE,
                    symptom: format!(
                        "{} 区间倒挂：{}-{}ms",
                        t.zh(),
                        t.min_ms(),
                        t.max_ms()
                    ),
                    root_cause: "分级上下界填反".to_string(),
                    advice: "修正区间；倒挂会让任何时长都被判越界",
                    severity: Severity::Blocking,
                });
            }
            if prev_max != 0 && t.min_ms() < prev_max {
                issues.push(LangIssue {
                    code: E_TIER_BELOW_RANGE,
                    symptom: format!(
                        "{} 上界 {}ms 低于上一级上界，分级应单调递增",
                        t.zh(),
                        t.max_ms()
                    ),
                    root_cause: "分级次序被打乱".to_string(),
                    advice: "按时长递增重排分级；Of() 的反查依赖单调性",
                    severity: Severity::Blocking,
                });
            }
            prev_max = t.max_ms();
            if !t.has_reduced_band() {
                issues.push(LangIssue {
                    code: E_TIER_NO_REDUCED_BAND,
                    symptom: format!("{} 没有内建 reduce 档", t.zh()),
                    root_cause: "无障碍被当成外挂补丁".to_string(),
                    advice: "在分级表内建 reduced 档；让每个消费方自己写必漏一处",
                    severity: Severity::Blocking,
                });
            }
        }

        // 语义化缓动：三族覆盖、无裸曲线。
        let uncovered = self.easings.uncovered_families();
        if !uncovered.is_empty() {
            issues.push(LangIssue {
                code: E_EASING_INCOMPLETE,
                symptom: format!("缓动登记缺族：{} 项", uncovered.len()),
                root_cause: "某一族未被登记，等于没有这一族".to_string(),
                advice: "补齐标准/弹性/强调三族的登记条目",
                severity: Severity::Blocking,
            });
        }
        for spec in self.easings.iter() {
            if spec.is_bare_curve() {
                issues.push(LangIssue {
                    code: E_EASING_BARE_CURVE,
                    symptom: format!("缓动 {} 是裸曲线", spec.name),
                    root_cause: "裸曲线入册".to_string(),
                    advice: "换成语义化名字；数值放曲线字段",
                    severity: Severity::Blocking,
                });
            }
            if !spec.is_complete() {
                issues.push(LangIssue {
                    code: E_EASING_INCOMPLETE,
                    symptom: format!("缓动 {} 三段不齐", spec.name),
                    root_cause: "登记项字段缺失".to_string(),
                    advice: "补齐名/曲线/语义三段",
                    severity: Severity::Blocking,
                });
            }
        }

        // 单源对账三态。
        let audit = self.audit_single_source();
        if !audit.unregistered.is_empty() {
            issues.push(LangIssue {
                code: E_SCENE_EASING_UNREGISTERED,
                symptom: format!(
                    "{} 个场景引用了未在册的缓动：{:?}",
                    audit.unregistered.len(),
                    audit.unregistered
                ),
                root_cause: "适用清单写了名字但登记册里没有".to_string(),
                advice: "补登记该缓动或改用已登记的名字；两侧必须同步",
                severity: Severity::Blocking,
            });
        }
        if !audit.ownerless.is_empty() {
            issues.push(LangIssue {
                code: E_EASING_NO_SCENE,
                symptom: format!(
                    "{} 支缓动无任何场景使用：{}",
                    audit.ownerless.len(),
                    audit.ownerless.join("、")
                ),
                root_cause: "登记了没人用的名字，等于没有共识".to_string(),
                advice: "补上适用场景，或归档该条目",
                severity: Severity::Warning,
            });
        }
        if !audit.no_reduced_band.is_empty() {
            issues.push(LangIssue {
                code: E_TIER_NO_REDUCED_BAND,
                symptom: format!("{} 级缺内建 reduce 档", audit.no_reduced_band.len()),
                root_cause: "无障碍外挂化".to_string(),
                advice: "在分级表内建 reduced 档",
                severity: Severity::Blocking,
            });
        }

        // 裁决四要素。
        if self.rulings.incomplete_count() > 0 {
            issues.push(LangIssue {
                code: E_RULING_INCOMPLETE,
                symptom: format!("{} 条裁决四要素不全", self.rulings.incomplete_count()),
                root_cause: "裁决漏写适用边界最常见".to_string(),
                advice: "补齐现象/裁决/理由/边界四要素",
                severity: Severity::Blocking,
            });
        }

        issues
    }

    /// 放行前置检查（**任一阻断级缺口即不放行**）。
    ///
    /// 只收 [`Severity::Blocking`]——把警告也当成阻断会让「标准语言册」永远
    /// 放不了行，而那等于这条门形同虚设。警告不是不管，是走
    /// [`MotionLanguage::warnings`] 单独可见：门只挡真缺陷，账照样留痕。
    pub fn preflight(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        for i in self.self_audit() {
            if i.severity == Severity::Blocking {
                missing.push(i.code);
            }
        }
        // 注意力预算是硬门：峰值超限即须复盘（不是阻断当前放行，但必须被看见）。
        if self.budget.peak() > MAX_CONCURRENT_MOTIONS {
            missing.push(E_ATTENTION_BUDGET);
        }
        missing
    }

    /// 警告级问题（**不阻断放行但必须可见**）。
    pub fn warnings(&self) -> Vec<&'static str> {
        self.self_audit()
            .iter()
            .filter(|i| i.severity == Severity::Warning)
            .map(|i| i.code)
            .collect()
    }

    /// 读屏替述（无障碍替述：把语言册讲成人话，且**覆盖四原则/四级/缓动/reduce/单源**）。
    ///
    /// 替述与语言册同源生成——另写一份的风险是漂移，而漂移的无障碍文档比没有
    /// 更坏：它会让用户以为已经有保障。
    pub fn language_narration(&self, frame: u64) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "VE-P 域动效设计语言册，版本 {}。\n",
            self.version
        ));
        s.push_str("判据共六项：");
        for c in CRITERIA.iter() {
            s.push_str(&format!("{}（{}）；", c.zh(), c.code()));
        }
        s.push_str("\n四原则：");
        for p in PRINCIPLE_ORDER.iter() {
            s.push_str(&format!("{}（{}）——{}；", p.zh(), p.code(), p.verdict_rule()));
        }
        s.push_str("\n四级时长：\n");
        for t in TIER_ORDER.iter() {
            s.push_str(&format!("{}；", t.screen_line()));
        }
        s.push_str(&format!("\n{}\n", self.easings.screen_text()));
        s.push_str("缓动登记明细：\n");
        for spec in self.easings.iter() {
            s.push_str(&format!("{}；", spec.screen_line()));
        }
        s.push_str(&format!("\n{}\n", self.budget.screen_text()));
        s.push_str(&format!("{}\n", self.exemptions.screen_text(frame)));
        s.push_str(&format!("{}\n", self.rulings.screen_text()));
        s.push_str(&format!("{}\n", self.log.screen_text()));
        s.push_str("适用清单（场景 → 推荐族 → reduce 行为）：\n");
        for sc in Scene::all_scenes() {
            s.push_str(&format!("{}；", sc.screen_line()));
        }
        s.push_str(&format!(
            "\n单源对账：{}",
            self.audit_single_source().screen_text()
        ));
        s
    }
}

/// 场景推荐值三件套（**单源取值的交付形态**）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneRecommendation {
    /// 场景。
    pub scene: Scene,
    /// 所属分级。
    pub tier: DurationTier,
    /// 时长判定结论。
    pub verdict: DurationVerdict,
    /// 最终生效时长（毫秒）。
    pub effective_ms: u32,
    /// 该场景推荐时长（毫秒，分级中值）。
    pub recommended_ms: u32,
    /// 推荐进入缓动名。
    pub enter_easing: &'static str,
    /// 推荐退出缓动名。
    pub exit_easing: &'static str,
    /// 进入缓动是否已登记。
    pub enter_registered: bool,
    /// 退出缓动是否已登记。
    pub exit_registered: bool,
    /// reduce 态时长（毫秒；0 = 直达）。
    pub reduced_ms: u32,
}

impl SceneRecommendation {
    /// 该推荐是否**完全合规**（时长合规 + 两个缓动都已登记）。
    pub fn is_clean(&self) -> bool {
        self.verdict.is_ok() && self.enter_registered && self.exit_registered
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "{}：生效 {}ms（推荐 {}ms），{}；进 {}（{}）、退 {}（{}）；reduce {}ms",
            self.scene.zh(),
            self.effective_ms,
            self.recommended_ms,
            self.verdict.screen_line(self.effective_ms),
            self.enter_easing,
            if self.enter_registered { "已登记" } else { "未登记" },
            self.exit_easing,
            if self.exit_registered { "已登记" } else { "未登记" },
            self.reduced_ms
        )
    }
}

/// VE-F3002 域自检（判据逐条映射见 `vep02_checks.rs`）。
pub fn run_vep02_checks() -> CheckSet {
    super::vep02_checks::run_vep02_checks()
}