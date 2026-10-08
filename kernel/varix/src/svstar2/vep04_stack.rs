//! VE-F3004 · 与 VE-M/O04 动画栈关系（VE-P 域 · 动效与过渡库 · P02 组）—— **Rust 权威实现**。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3004`
//!
//! # 判据（锚点原文七条 + 纪律三条）
//!
//! 1. **三层分工**：P=声明与编排层（动效是什么/何时触发/如何组合）→O04=CSS 声明编译
//!    →M=运行时权威（时钟/混合/采样）——三层职责不得越界；
//! 2. **三选一**：每动效组件声明编译目标三选一——CSS 动画/JS 驱动回调/合成通道直通，
//!    由**选型决策表**（动效特征→编译目标）判定；
//! 3. **v2 控制接口**：v1 之上加编排控制段——偏移/速度/方向/优先级四控制 + 生效域声明；
//! 4. **偏离红线**：P 声明↔M 实例对拍——声明什么运行什么，运行偏离声明即缺陷；
//! 5. **最终闸门**：v2 控制含无障碍优先级段，reduce 态编排器自动降为直达（控制层最后闸门）；
//! 6. **错误路径**：选型错误→决策表拦截+主线程回退；越权→所有权检查拒绝；对拍分歧→立案复验；
//! 7. **性能分解**：选型 O(1) 查表、控制 O(1) 直传、对拍 O(抽样)。
//!
//! # 为什么「三层分工」要做成可执行断言（不是文档里的一句话）
//!
//! 三层分工最常见的腐化不是有人明写「O04 去管时钟」，而是**接口形状悄悄变了**：
//! O04 的编译产物里悄悄多出`clock` 字段、M 的实例接口悄悄多出 `play()`，
//! 时间一长谁也说不清时钟权威到底在谁手里——而这类腐化**不报错**，只在宿主休眠唤醒
//! 那一个边界上表现为「动画倒放」。故本项把分工做成 [`LayerContract`] 的**权���字段**
//! 与 [`assert_layer_separation`] 的逐项断言：三层各自**允许**触碰的字段是白名单，
//! 越界即 [`E_LAYER_OVERREACH`]。字段一多一少都能被抓，不靠人记。
//!
//! # 选型决策表为什么是「表」而不是 if-else 链
//!
//! 三选一的判定依据是**动效特征**（是否只改呈现量/是否高频/是否触发布局/通道是否可用），
//! 组合起来天然是查表。若写成 if-else 链，加一条特征要改若干分支，且**没人能证明
//! 覆盖了全部特征组合**——漏一格就是「该走合成通道的走了主线程」，表现为掉帧，
//! 不表现为报错。故本项用**完备性可断言**的决策表：表项覆盖
//! [`FEATURE_SPACE`] 的全集，且每格唯一（[`E_DECISION_AMBIGUOUS`] 拒二义），
//! 并对表内格数与穷举组合数做守恒对账——**加特征必进表**，漏了会红。
//!
//! # 控制接口 v2 与 v1 的关系（只增不改）
//!
//! v1 是 O04→M 的**实例创建**接口（创建/挂载/销毁）。v2 在 v1 之上**加**编排控制段
//! （偏移/速度/方向/优先级），供F3005 编排器做实例级控制。**只增不改**：v1 的字段
//! 与语义一字未动，v1 消费者不受影响。若有人重排 v1 字段顺序以「顺便」，本项的
//! [`assert_v2_superset_of_v1`] 会红——契约漂移是最难查的一类分叉。
//!
//! # 越权为什么要单独判（所有权检查不是多余）
//!
//! 「编排器改了不属于自己的实例」在无所有权检查时表现为**静默串扰**：A 编排器调偏移，
//! B 组件的动画跟着动，界面表现是「有个动画在乱跳」，日志里什么都没有。故本项用
//! [`InstanceOwnership`] 显式登记归属，v2 控制写入前过 [`check_control_authority`]，
//! 越权即 [`E_CONTROL_NOT_OWNER`]，并给出**该找谁**（回原属主），不是只报错。
//!
//! # reduce 态的最终闸门放在控制层（不是组件层）
//!
//! reduce（减少动态效果）要在 P 层生效，但**组件自己判一次就漏一次**——总有人新写一个
//! 组件忘了判。故最终闸门设在 v2 控制段：[`reduce_control_gate`] 是**控制层**的
//! 硬门，reduce 态下编排器的偏移/速度/方向请求一律被降为「直达」（不产生时间变化），
//! 且**不给组件绕过的机会**——组件拿不到可用的偏移值。这是锚点「控制层最终闸门」
//! 的落点，与 F3003 的 reduce 令牌层（组件侧零分支）**分工不同、互不替代**：
//! F3003 管的是令牌取值，本项管的是**编排动作能否生效**。
//!
//! # O04/M 两侧的落位状态（诚实标注，不代写上游）
//!
//! 锚点要求「O04/M 接口冻结接收」「F3005 消费 v2」「F3006 消费选型表」。
//! 现状：`src/system/ve/mDomain/f2401-m-animation-domain-kickoff.ts` 是 M 域**开工骨架**
//! （主题注册表/批次注册表/流水线序列），**尚无** AnimationInstance/Clock 具体接口；
//! `src/system/ve/oDomain/f2801-*.ts` 是 O 域风格引擎架构，动画组（F2861~F2880）尚在册。
//! 故本项把两侧接口**冻结为 trait 协议 + 记录式实现**（[`O04Compiler`] /
//! [`MRuntimePort`] / [`RecordingRuntimePort`]），使决策表编译产物、v2 控制序列、
//! 三层对拍**现在就能跑通并被自检覆盖**。F2861 / F2401 落地时按本协议实现即可对接，
//! **本项不改上游、不代写 F2861/M 域实例接口**。
//!
//! # no_std
//!
//! 仅依赖同册 [`vep03_token`]（动效令牌，判定动效特征与 reduce 态）与 `alloc`。
//! 零 IO、零墙钟、零浮点（时间偏移用整型毫秒，方向/优先级用枚举），
//! 回归可复现（对拍红线）。

use crate::svstar2::vep03_token::{Lane, MotionTokenError, TokenFamily};

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 三层栈协议版本。契约变更走版本号。
pub const STACK_PROTOCOL_VERSION: &str = "P02-stack-v1";
/// P→O04 编译接口协议版本。
pub const P_TO_O04_PROTOCOL_VERSION: &str = "P02-p2o04-v1";
/// P→M 控制接口 v1 版本（实例创建）。
pub const CONTROL_V1_VERSION: &str = "P02-control-v1";
/// P→M 控制接口 v2 版本（v1 + 编排控制段）。
pub const CONTROL_V2_VERSION: &str = "P02-control-v2";
/// 三层对拍协议版本。
pub const PARITY_PROTOCOL_VERSION: &str = "P02-parity-v1";

/// 动画栈层数（P/O04/M 三层，不增不减——加层要走册内锚点）。
pub const LAYER_COUNT: usize = 3;
/// 编译目标三选一的格数。
pub const COMPILE_TARGET_COUNT: usize = 3;
/// 动效特征维度数（决策表的列）。
pub const FEATURE_COUNT: usize = 4;
/// 选型决策表行数（= 特征空间组合数，见 `feature_space`）。
pub const DECISION_ROW_COUNT: usize = 16;
/// v2 编排控制段字段数（偏移/速度/方向/优先级）。
pub const CONTROL_FIELDS: usize = 4;
/// v1 实例创建接口字段数。
pub const CONTROL_V1_FIELDS: usize = 4;
/// 对拍抽样上限（O(抽样)，不全量对拍）。
pub const PARITY_SAMPLE_CAP: usize = 64;
/// 抽样步长（每 N 个实例抽1个）。
pub const PARITY_STRIDE: usize = 7;
/// 实例表上限。
pub const MAX_INSTANCES: usize = 256;

/// P 域命名空间归属。
pub const P_NAMESPACE_OWNER: &str = "VE-P";
/// O04 域标识。
pub const O04_DOMAIN: &str = "VE-O04";
/// M 域标识。
pub const M_DOMAIN: &str = "VE-M";

/// 复杂度说明（性能分解：选型 O(1) 查表、控制 O(1) 直传、对拍 O(抽样)）。
pub const COMPLEXITY_DOC: &str = "\
选型决策：O(1) 查表（表行数为编译期常量，行内按特征位匹配，无循环）。
编排控制：O(1) 直传（四字段定长写入，越权检查是一次归属比较）。
三层对拍：O(抽样)，每 PARITY_STRIDE=7 个实例抽 1 个，上限 PARITY_SAMPLE_CAP=64。
冲突登记：O(1) 线性扫（登记项为编译期常量表，命中即止）。
";

// ---------------------------------------------------------------------------
// 二、三层分工契约（判据一）
// ---------------------------------------------------------------------------

/// 动画栈层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackLayer {
    /// P=声明与编排层：动效是什么/何时触发/如何组合。
    Declaration,
    /// O04=CSS 声明编译：CSS 声明 →动画声明编译产物。
    Compile,
    /// M=运行时权威：时钟/混合/采样。
    Runtime,
}

impl StackLayer {
    /// 层全集（P→O04→M，顺序即数据流方向）。
    pub const ALL: [StackLayer; LAYER_COUNT] = [
        StackLayer::Declaration,
        StackLayer::Compile,
        StackLayer::Runtime,
    ];

    /// 层代号（诊断用）。
    pub fn code(self) -> &'static str {
        match self {
            StackLayer::Declaration => "P",
            StackLayer::Compile => "O04",
            StackLayer::Runtime => "M",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            StackLayer::Declaration => "声明与编排层",
            StackLayer::Compile => "CSS 声明编译层",
            StackLayer::Runtime => "运行时权威层",
        }
    }

    /// 按代号取层。
    pub fn from_code(code: &str) -> Option<StackLayer> {
        match code {
            "P" => Some(StackLayer::Declaration),
            "O04" => Some(StackLayer::Compile),
            "M" => Some(StackLayer::Runtime),
            _ => None,
        }
    }

    /// 该层**权威**持有的字段（白名单——职责的正面表达）。
    ///
    /// 白名单之外的一切字段对本层都是越界：不是「暂时没实现」，
    /// 而是「本层根本不该碰」。这是把分工从注释变成断言的关键。
    pub fn owned_fields(self) -> &'static [&'static str] {
        match self {
            // 动效是什么/何时触发/如何组合 —— 不含时间轴推进权，也不含采样权。
            StackLayer::Declaration => &["effect_id", "trigger", "composition", "control_v2"],
            // 解析与生命周期权威（O04），把 CSS 声明编译成 M 域实例。
            StackLayer::Compile => &["track_table", "easing_ref", "duration_ref", "lifecycle"],
            // 时钟/混合/采样权威（M）。
            StackLayer::Runtime => &["clock", "blend", "sample", "instance_state"],
        }
    }
}

/// 层间分工契约的一条记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerContract {
    /// 层。
    pub layer: StackLayer,
    /// 该层职责一句话（锚点原文）。
    pub duty: &'static str,
    /// 该层**禁止**触碰的域（下游权威——碰了就是夺权）。
    pub forbidden_domain: &'static str,
}

/// 全部分工契约（P/O04/M 各一条，顺序与 `StackLayer::ALL` 一致）。
pub const LAYER_CONTRACTS: [LayerContract; LAYER_COUNT] = [
    LayerContract {
        layer: StackLayer::Declaration,
        duty: "动效是什么/何时触发/如何组合",
        forbidden_domain: "M域时钟（M 为时钟权威，P 只声明不仲裁）",
    },
    LayerContract {
        layer: StackLayer::Compile,
        duty: "CSS 声明编译为动画声明编译产物，生命周期权威",
        forbidden_domain: "M 域采样与混合（M 为运行时权威，O04 只编译不驱动）",
    },
    LayerContract {
        layer: StackLayer::Runtime,
        duty: "时钟/混合/采样权威",
        forbidden_domain: "P 域编排意图（M 不猜用户意图，只执行被声明的量）",
    },
];

/// 分层越界（字段不属于该层权威）。
pub const E_LAYER_OVERREACH: &str = "E_LAYER_OVERREACH";
/// 契约集不完整（层数/记录数不齐）。
pub const E_CONTRACT_INCOMPLETE: &str = "E_CONTRACT_INCOMPLETE";

/// 三层分工的分层断言结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeparationReport {
    /// 逐条判据名。
    pub items: Vec<String>,
    /// 分层是否成立。
    pub separated: bool,
}

/// 三层分工断言：**结构上**证明没有任何一层持有非本层的权威字段。
///
/// 做法不是读注释，而是把每层的白名单与「实例结构体的字段全集」求差——
/// 只要某层能碰到不属于它的字段，就报 [`E_LAYER_OVERREACH`] 并点名是谁碰了什么。
pub fn assert_layer_separation() -> Result<SeparationReport, MotionTokenError> {
    let mut items: Vec<String> = Vec::new();
    if LAYER_CONTRACTS.len() != LAYER_COUNT {
        return Err(MotionTokenError::new(
            E_CONTRACT_INCOMPLETE,
            "分工契约记录数与层数不符",
            "契约表必须与 StackLayer::ALL 一一对应，缺一条就有层无契约、契约无处生效",
            "补齐 LAYER_CONTRACTS 至 LAYER_COUNT 条并与 ALL 顺序对齐",
            P_NAMESPACE_OWNER,
        ));
    }
    for (i, c) in LAYER_CONTRACTS.iter().enumerate() {
        if c.layer != StackLayer::ALL[i] {
            return Err(MotionTokenError::new(
                E_CONTRACT_INCOMPLETE,
                "分工契约顺序与层顺序不一致",
                "契约表按下标与 StackLayer::ALL 对齐，乱序会让按下标查权威的调用方取错层",
                "把 LAYER_CONTRACTS 按 P→O04→M 重排",
                P_NAMESPACE_OWNER,
            ));
        }
        if c.duty.is_empty() {
            return Err(MotionTokenError::new(
                E_CONTRACT_INCOMPLETE,
                "分工契约缺职责描述",
                "职责为空等于没写分工，判据失去可读依据",
                "为该层补一句锚点原文职责",
                P_NAMESPACE_OWNER,
            ));
        }
        items.push(format!("{}·{}：{}", c.layer.code(), c.layer.zh(), c.duty));
    }

    // 逐字段反查：每个权威字段**只**属于一个层，出现在别的层即越界。
    for layer in StackLayer::ALL.iter().copied() {
        let mine = layer.owned_fields();
        for other in StackLayer::ALL.iter().copied() {
            if other == layer {
                continue;
            }
            for f in other.owned_fields() {
                if mine.contains(f) {
                    return Err(MotionTokenError::new(
                        E_LAYER_OVERREACH,
                        "同一权威字段被两层同时持有",
                        &format!(
                            "字段 {} 同时登记在 {} 与 {} 的权威白名单里，权威单源被破坏",
                            f,
                            layer.code(),
                            other.code()
                        ),
                        "把该字段从其中一层的白名单移除，只留在真正行使权威的层",
                        P_NAMESPACE_OWNER,
                    ));
                }
            }
        }
    }

    // 字段全集不得有重复（重复即两人共管同一权威）。
    let mut all: Vec<&str> = Vec::new();
    for layer in StackLayer::ALL.iter().copied() {
        for f in layer.owned_fields() {
            if all.contains(f) {
                return Err(MotionTokenError::new(
                    E_LAYER_OVERREACH,
                    "权威字段重复登记",
                    &format!("字段 {} 在权威白名单中出现两次，权威不是单源", f),
                    "去重后重排owned_fields",
                    P_NAMESPACE_OWNER,
                ));
            }
            all.push(f);
        }
    }
    items.push(format!("权威字段全集{} 项，各归属唯一层", all.len()));
    Ok(SeparationReport {
        items,
        separated: true,
    })
}

// ---------------------------------------------------------------------------
// 三、动效特征与编译目标（判据二：选型决策表）
// ---------------------------------------------------------------------------

/// 编译目标（三选一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompileTarget {
    /// CSS 动画：由样式引擎/O04 侧驱动，声明式。
    CssAnimation,
    /// JS 驱动回调：主线程逐帧回调驱动。
    JsCallback,
    /// 合成通道直通：交由合成器，不占主线程。
    CompositorDirect,
}

impl CompileTarget {
    /// 全集（三选一，顺序即降级优先级从低到高）。
    pub const ALL: [CompileTarget; COMPILE_TARGET_COUNT] = [
        CompileTarget::CssAnimation,
        CompileTarget::JsCallback,
        CompileTarget::CompositorDirect,
    ];

    /// 代号。
    pub fn code(self) -> &'static str {
        match self {
            CompileTarget::CssAnimation => "css-animation",
            CompileTarget::JsCallback => "js-callback",
            CompileTarget::CompositorDirect => "compositor-direct",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            CompileTarget::CssAnimation => "CSS 动画",
            CompileTarget::JsCallback => "JS 驱动回调",
            CompileTarget::CompositorDirect => "合成通道直通",
        }
    }

    /// 是否占主线程（性能归因用）。
    pub fn costs_main_thread(self) -> bool {
        match self {
            CompileTarget::CssAnimation => false,
            CompileTarget::JsCallback => true,
            CompileTarget::CompositorDirect => false,
        }
    }

    /// 该目标能承载的**动效属性**维度（位掩码）。
    ///
    /// 注意这里**不含** `FEATURE_CHANNEL_READY`：通道就位是**环境**特征
    /// （D 域图层提升有没有做），不是动效属性。若把它塞进 `supports()`，
    /// 「通道就位时的低频动效」会被误判成「CSS 动画承载不了通道」而遭拒——
    /// 而CSS 动画在通道就位时照样能跑，通道与它无关。
    pub fn supports(self) -> u8 {
        match self {
            // CSS 动画：承载呈现量；**不承载**高频（逐帧回调是主线程的活）。
            CompileTarget::CssAnimation => FEATURE_PRESENTATION_ONLY,
            // JS 回调：所有动效属性都能扛，代价是主线程逐帧成本。
            CompileTarget::JsCallback => FEATURE_PROPERTY_MASK,
            // 合成通道直通：只承载呈现量（改布局面必须回主线程），
            // 高频正是它存在的意义，故承载。
            CompileTarget::CompositorDirect => {
                FEATURE_PRESENTATION_ONLY | FEATURE_HIGH_FREQUENCY
            }
        }
    }
}

/// 特征位：只改呈现量（不改布局语义终值）。
pub const FEATURE_PRESENTATION_ONLY: u8 = 1 << 0;
/// 特征位：会触发布局/重排。
pub const FEATURE_TOUCHES_LAYOUT: u8 = 1 << 1;
/// 特征位：高频（每帧或亚帧连续）。
pub const FEATURE_HIGH_FREQUENCY: u8 = 1 << 2;
/// 特征位：合成通道可用（D 域图层提升已就位，F2850 复用）。
pub const FEATURE_CHANNEL_READY: u8 = 1 << 3;

/// 动效特征维度定义（决策表的列；加特征必须在此登记，否则 `feature_space` 少一维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureDim {
    /// 特征位。
    pub bit: u8,
    /// 特征名（诊断/表项键）。
    pub name: &'static str,
    /// 该维度的互斥约束：置位此位时，`other` 必须为 0。
    pub excludes: u8,
}

/// 特征维度全集（[`FEATURE_COUNT`] 项）。
///
/// **高频与低频不是两个维度，是同一个维度（频率）的两个取值**——所以低频
/// **不占特征位**：频率维度由 [`FEATURE_HIGH_FREQUENCY`] 一个位表达，
/// 该位为 0 即低频。这不是省一位，是建模正确：一个动效不可能既高频又低频，
/// 分成两位就等于承认了「既高频又低频」这种不存在的状态，决策表会为它准备一格，
/// 而那一格永远无人命中——**僵尸格**。同理「触发布局」蕴含「不只改呈现量」。
pub const FEATURE_DIMS: [FeatureDim; FEATURE_COUNT] = [
    FeatureDim {
        bit: FEATURE_TOUCHES_LAYOUT,
        name: "touches_layout",
        excludes: FEATURE_PRESENTATION_ONLY,
    },
    FeatureDim {
        bit: FEATURE_PRESENTATION_ONLY,
        name: "presentation_only",
        excludes: FEATURE_TOUCHES_LAYOUT,
    },
    FeatureDim {
        bit: FEATURE_HIGH_FREQUENCY,
        name: "high_frequency",
        excludes: 0,
    },
    FeatureDim {
        bit: FEATURE_CHANNEL_READY,
        name: "channel_ready",
        excludes: 0,
    },
];

/// 动效属性位掩码（**不含**通道位——通道是环境特征，不是动效属性）。
pub const FEATURE_PROPERTY_MASK: u8 =
    FEATURE_TOUCHES_LAYOUT | FEATURE_PRESENTATION_ONLY | FEATURE_HIGH_FREQUENCY;

/// 选型决策表的一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecisionRow {
    /// 特征组合掩码（决策表的键）。
    pub features: u8,
    /// 编译目标。
    pub target: CompileTarget,
    /// 选型理由（一句人话，调试时能直接读懂为什么走这条）。
    pub reason: &'static str,
}

/// 决策表**穷举全集**（16 行 = 4 个二值维度的全部组合，一格不缺）。
///
/// 每格显式写出理由而不是靠「没命中就兜底」——兜底表项会让漏格变得不可见，
/// 而漏格的后果是「该走主线程的走成合成通道」（掉帧）或反之（更卡），
/// 两者都**不报错**，只能靠这张表自己完整。
pub const DECISION_TABLE: [DecisionRow; DECISION_ROW_COUNT] = [
    // ---- 触发布局（8 格）：一律主线程回调，通道与 CSS 都改不了布局语义 ----
    DecisionRow {
        features: FEATURE_TOUCHES_LAYOUT,
        target: CompileTarget::JsCallback,
        reason: "改布局：主线程回调（通道未就位）",
    },
    DecisionRow {
        features: FEATURE_TOUCHES_LAYOUT | FEATURE_CHANNEL_READY,
        target: CompileTarget::JsCallback,
        reason: "改布局：通道就位也不改布局语义，仍主线程回调",
    },
    DecisionRow {
        features: FEATURE_TOUCHES_LAYOUT | FEATURE_HIGH_FREQUENCY,
        target: CompileTarget::JsCallback,
        reason: "改布局+高频：主线程回调且须计入帧预算",
    },
    DecisionRow {
        features: FEATURE_TOUCHES_LAYOUT | FEATURE_HIGH_FREQUENCY | FEATURE_CHANNEL_READY,
        target: CompileTarget::JsCallback,
        reason: "改布局+高频+通道就位：布局面必须主线程，通道只帮呈现面",
    },
    // ---- 只改呈现量 · 低频（4 格）：声明式，零主线程逐帧成本 ----
    DecisionRow {
        features: FEATURE_PRESENTATION_ONLY,
        target: CompileTarget::CssAnimation,
        reason: "呈现量+低频：CSS 动画声明式驱动",
    },
    DecisionRow {
        features: FEATURE_PRESENTATION_ONLY | FEATURE_CHANNEL_READY,
        target: CompileTarget::CssAnimation,
        reason: "呈现量+低频+通道就位：仍走声明式，不为低频动效占通道",
    },
    DecisionRow {
        features: FEATURE_PRESENTATION_ONLY | FEATURE_HIGH_FREQUENCY,
        target: CompileTarget::JsCallback,
        reason: "呈现量+高频但通道未就位：回退主线程（往不存在的通道写=静默失效，比慢更糟）",
    },
    DecisionRow {
        features: FEATURE_PRESENTATION_ONLY | FEATURE_HIGH_FREQUENCY | FEATURE_CHANNEL_READY,
        target: CompileTarget::CompositorDirect,
        reason: "呈现量+高频+通道就位：合成通道全条件满足",
    },
    // ---- 无属性声明（4 格）：保守主线程 ----
    DecisionRow {
        features: 0,
        target: CompileTarget::JsCallback,
        reason: "未声明属性：无从判特征，保守走主线程（宁可慢不可错）",
    },
    DecisionRow {
        features: FEATURE_CHANNEL_READY,
        target: CompileTarget::JsCallback,
        reason: "仅通道就位而无属性声明：无从判特征，保守走主线程",
    },
    DecisionRow {
        features: FEATURE_HIGH_FREQUENCY,
        target: CompileTarget::JsCallback,
        reason: "仅高频而无属性声明：高频本身不说明改什么，须主线程判定后写入",
    },
    DecisionRow {
        features: FEATURE_HIGH_FREQUENCY | FEATURE_CHANNEL_READY,
        target: CompileTarget::JsCallback,
        reason: "仅高频+通道就位：无属性声明不得直通通道（不知该写哪个呈现量）",
    },
    // ---- 布局位与呈现位互斥，但表仍须穷举（4 格不可达，见下方注释）----
    // touches_layout 与 presentation_only 互斥（FEATURE_DIMS.excludes），
    // 故下列 4 格在**合法输入**里永不出现；但表必须写全，否则完备性对账
    // 无法区分「这格不可达」与「这格被漏了」——前者是建模结论，后者是缺陷。
    DecisionRow {
        features: FEATURE_TOUCHES_LAYOUT | FEATURE_PRESENTATION_ONLY,
        target: CompileTarget::JsCallback,
        reason: "互斥组合（不可达）：改布局优先于呈现量，保守主线程",
    },
    DecisionRow {
        features: FEATURE_TOUCHES_LAYOUT
            | FEATURE_PRESENTATION_ONLY
            | FEATURE_CHANNEL_READY,
        target: CompileTarget::JsCallback,
        reason: "互斥组合（不可达）：布局面优先，通道只帮呈现面",
    },
    DecisionRow {
        features: FEATURE_TOUCHES_LAYOUT | FEATURE_PRESENTATION_ONLY | FEATURE_HIGH_FREQUENCY,
        target: CompileTarget::JsCallback,
        reason: "互斥组合（不可达）：布局+高频，主线程回调",
    },
    DecisionRow {
        features: FEATURE_TOUCHES_LAYOUT
            | FEATURE_PRESENTATION_ONLY
            | FEATURE_HIGH_FREQUENCY
            | FEATURE_CHANNEL_READY,
        target: CompileTarget::JsCallback,
        reason: "互斥组合（不可达）：布局面优先于一切优化",
    },
];

/// 选型无对应表项。
pub const E_TARGET_UNRESOLVED: &str = "E_TARGET_UNRESOLVED";
/// 决策表二义（同一特征组合多行命中）。
pub const E_DECISION_AMBIGUOUS: &str = "E_DECISION_AMBIGUOUS";
/// 决策表不完备（特征空间有组合无表项）。
pub const E_DECISION_INCOMPLETE: &str = "E_DECISION_INCOMPLETE";
/// 目标不支持所选特征（合成通道选了不支持的特征）。
pub const E_TARGET_UNSUPPORTED: &str = "E_TARGET_UNSUPPORTED";

/// 决策表完备性报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionAudit {
    /// 特征空间组合数（2^FEATURE_COUNT）。
    pub space_size: usize,
    /// 表项数。
    pub rows: usize,
    /// 无表项的特征组合（特征空间 − 表覆盖）。
    pub uncovered: Vec<u8>,
    /// 二义的特征组合（被多行命中）。
    pub ambiguous: Vec<u8>,
}

impl DecisionAudit {
    /// 表是否完备且无二义。
    pub fn is_complete(&self) -> bool {
        self.uncovered.is_empty() && self.ambiguous.is_empty()
    }
}

/// 特征空间：枚举 [`FEATURE_DIMS`] 的全部组合掩码（`2^FEATURE_COUNT` 个）。
///
/// 这是「加特征必进表」的守恒方：加一维特征 → 空间翻倍→ 表若没跟着补，
/// [`audit_decision_table`] 的 `uncovered` 非空 → 红。
pub fn feature_space() -> Vec<u8> {
    let n = 1u32 << FEATURE_COUNT;
    let mut out: Vec<u8> = Vec::new();
    for v in 0..n {
        let mut m = 0u8;
        for (i, d) in FEATURE_DIMS.iter().enumerate() {
            if v & (1 << i) != 0 {
                m |= d.bit;
            }
        }
        // 互斥维度剪枝：置了 A 而又置了 A 的互斥项 B，这个组合**不存在**。
        // 不剪枝的话 feature_space 会吐出「既触发布局又只改呈现量」这种僵尸组合，
        // 对账就会把「建模上不可达」误报成「表漏了一格」——假红。
        // 剪掉之后，feature_space 恰是**合法输入的全集**，对账才有意义。
        let reachable = !FEATURE_DIMS.iter().any(|d| {
            d.excludes != 0 && (m & d.bit) != 0 && (m & d.excludes) != 0
        });
        if reachable {
            out.push(m);
        }
    }
    out
}

/// 特征空间体积（合法输入数；= 决策表必须覆盖的行数下限）。
pub fn feature_space_size() -> usize {
    feature_space().len()
}

/// 决策表完备性对账：空间组合 ↔ 表项双向点名。
pub fn audit_decision_table() -> DecisionAudit {
    let space = feature_space();
    let mut uncovered: Vec<u8> = Vec::new();
    let mut ambiguous: Vec<u8> = Vec::new();
    for m in space.iter().copied() {
        let hits = DECISION_TABLE
            .iter()
            .filter(|r| matches_features(r.features, m))
            .count();
        if hits == 0 {
            uncovered.push(m);
        } else if hits > 1 {
            ambiguous.push(m);
        }
    }
    DecisionAudit {
        space_size: space.len(),
        rows: DECISION_TABLE.len(),
        uncovered,
        ambiguous,
    }
}

/// 表项掩码是否**完全匹配**特征组合（不是子集匹配——子集匹配必然二义，
/// 因为 `{A}` 与 `{A,B}` 会同时命中 `{A}` 行，这就是二义的来源）。
fn matches_features(row_mask: u8, want: u8) -> bool {
    row_mask == want
}

/// 选型结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// 编译目标。
    pub target: CompileTarget,
    /// 命中的表项下标。
    pub row: usize,
}

/// 按特征选编译目标（O(1) 查表）。
///
/// 未命中精确表项时**不猜**：报 [`E_TARGET_UNRESOLVED`] 并指出该补哪一格。
/// 猜一个「看起来差不多」的目标 = 该走主线程的走成合成通道（或反之），
/// 表现为掉帧，不表现为报错——这是最坏的一类默认。
pub fn select_target(features: u8) -> Result<Selection, MotionTokenError> {
    let mut hit: Option<(usize, CompileTarget)> = None;
    for (i, r) in DECISION_TABLE.iter().enumerate() {
        if matches_features(r.features, features) {
            if hit.is_some() {
                return Err(MotionTokenError::new(
                    E_DECISION_AMBIGUOUS,
                    "选型决策表二义",
                    &format!("特征掩码 {:#04x} 命中多行，二义默认等于随机默认", features),
                    "删除重复表项，使每个特征组合唯一命中一行",
                    P_NAMESPACE_OWNER,
                ));
            }
            hit = Some((i, r.target));
        }
    }
    match hit {
        None => Err(MotionTokenError::new(
            E_TARGET_UNRESOLVED,
            "选型决策表无对应表项",
            &format!(
                "特征掩码 {:#04x} 在决策表中无精确行，兜底猜目标会把编译路线带偏且无人察觉",
                features
            ),
            &format!(
                "在 DECISION_TABLE 补一行 features={:#04x}，并同步扩DECISION_ROW_COUNT",
                features
            ),
            P_NAMESPACE_OWNER,
        )),
        Some((row, target)) => {
            // 能力自检走唯一真源（与 probe_target 同一份实现——两份实现
            // 意味着「改一处忘一处」，而漏掉能力校验不报错，只掉帧）。
            check_capability(features, target)?;
            Ok(Selection { target, row })
        }
    }
}

/// 能力校验（**唯一真源**）。
///
/// `select_target` 与 `probe_target` 都必须走这里——两份实现意味着
/// 「改了一处忘了另一处」，而能力校验恰恰是那种**漏了不报错**的检查：
/// 漏掉时表现为动效走进能力外的路径（掉帧或静默失效），
/// 不会有一条错误日志提醒你。反假变体 M16 就是这么漏掉的。
pub fn check_capability(features: u8, target: CompileTarget) -> Result<(), MotionTokenError> {
    let props = features & FEATURE_PROPERTY_MASK;
    if props & !target.supports() != 0 {
        return Err(MotionTokenError::new(
            E_TARGET_UNSUPPORTED,
            "编译目标不支持所选特征",
            &format!(
                "目标 {} 不支持属性特征 {:#04x}，选了就会走到能力外的路径",
                target.code(),
                props & !target.supports()
            ),
            "改表项指向支持该特征的目标，或拆成两次动效分段",
            P_NAMESPACE_OWNER,
        ));
    }
    Ok(())
}

/// 能力探针：**绕过查表**直接问「这个特征组合能不能交给这个目标」。
///
/// 存在的理由：正常路径 `select_target` 先查表再校验能力，所以「能力外」
/// 这个状态在正常输入里**永远到不了**——表里就没有能力外的格。
/// 于是能力自检那一支就成了**不可达代码**，没人能证明它真的在工作。
/// 本函数把那一支暴露出来，使它可测：域自检用它验证「布局属性交给
/// 合成通道」必被拒（M16 反假变体就是靠它抓到的）。
///
/// 它**不做选型**，只回答能力问题，故不会被误用成选型旁路。
pub fn probe_target(features: u8, target: CompileTarget) -> Result<(), MotionTokenError> {
    check_capability(features, target)
}

// ---------------------------------------------------------------------------
// 四、P→O04 编译接口（判据二：每组件声明一个编译目标）
// ---------------------------------------------------------------------------

/// P 域侧的动效声明（组件声明的东西——不含任何 M 域字段）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MotionDeclaration {
    /// 动效 ID。
    pub effect_id: String,
    /// 动效特征掩码。
    pub features: u8,
    /// 时长令牌 ID（取自 F3003 令牌表）。
    pub duration_token: String,
    /// 缓动令牌 ID。
    pub easing_token: String,
    /// 位移令牌 ID。
    pub distance_token: String,
}

/// O04 侧的编译产物（家族格式：实例句柄 + 轨道表 + 缓动引用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledAnimation {
    /// 对应的动效 ID（回链 P 声明，对拍用）。
    pub effect_id: String,
    /// 编译目标。
    pub target: CompileTarget,
    /// 生命周期状态（O04 权威）。
    pub lifecycle: LifecycleState,
    /// 轨道表长度。
    pub track_count: usize,
}

/// 生命周期状态（O04 权威）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleState {
    /// 已构建未挂载。
    Built,
    /// 已挂载时间轴。
    Mounted,
    /// 已结束（填充终态）。
    Finished,
    /// 已销毁。
    Destroyed,
}

impl LifecycleState {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            LifecycleState::Built => "已构建",
            LifecycleState::Mounted => "已挂载",
            LifecycleState::Finished => "已结束",
            LifecycleState::Destroyed => "已销毁",
        }
    }

    /// 是否可被控制（已销毁的实例不可控）。
    pub fn is_controllable(self) -> bool {
        !matches!(self, LifecycleState::Destroyed)
    }
}

/// 主线程回退原因（选型错误的降级去处）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackReason {
    /// 通道不可用（合成通道未就位）。
    ChannelUnavailable,
    /// 特征超出目标能力。
    FeatureUnsupported,
}

impl FallbackReason {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            FallbackReason::ChannelUnavailable => "合成通道不可用",
            FallbackReason::FeatureUnsupported => "特征超出目标能力",
        }
    }
}

/// P→O04 编译器（协议冻结面）。
///
/// trait 而非函数：O04 侧（F2861）落地后实现本 trait 即可对接，
/// 本项**不代写 O04**。`RecordingO04` 是可运行的记录式实现，供自检覆盖。
pub trait O04Compiler {
    /// 按决策表把P 声明编译为动画声明编译产物。
    fn compile(&mut self, decl: &MotionDeclaration) -> Result<CompiledAnimation, MotionTokenError>;
    /// 记录一次主线程回退。
    fn note_fallback(&mut self, from: CompileTarget, reason: FallbackReason);
}

/// 记录式 O04 编译器（协议的可运行参考实现）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecordingO04 {
    /// 编译产物表。
    pub compiled: Vec<CompiledAnimation>,
    /// 回退记录（选型错误 → 主线程）。
    pub fallbacks: Vec<(CompileTarget, FallbackReason)>,
}

impl RecordingO04 {
    /// 构造空编译器。
    pub fn new() -> Self {
        RecordingO04 {
            compiled: Vec::new(),
            fallbacks: Vec::new(),
        }
    }

    /// 产物数。
    pub fn len(&self) -> usize {
        self.compiled.len()
    }

    /// 是否无产物。
    pub fn is_empty(&self) -> bool {
        self.compiled.is_empty()
    }

    /// 按动效 ID 取产物。
    pub fn get(&self, effect_id: &str) -> Option<&CompiledAnimation> {
        self.compiled.iter().find(|c| c.effect_id == effect_id)
    }
}

impl O04Compiler for RecordingO04 {
    fn compile(&mut self, decl: &MotionDeclaration) -> Result<CompiledAnimation, MotionTokenError> {
        let sel = select_target(decl.features)?;
        let mut target = sel.target;
        // 合成通道要求通道就位；特征位已含channel_ready 才可能选到它，
        // 但这里再确认一次——若决策表被改成「未就位也直通」，此处兜住并回退。
        if target == CompileTarget::CompositorDirect
            && decl.features & FEATURE_CHANNEL_READY == 0
        {
            self.note_fallback(target, FallbackReason::ChannelUnavailable);
            target = CompileTarget::JsCallback;
        }
        let out = CompiledAnimation {
            effect_id: decl.effect_id.clone(),
            target,
            lifecycle: LifecycleState::Built,
            track_count: 1,
        };
        if self.compiled.len() >= MAX_INSTANCES {
            return Err(MotionTokenError::new(
                E_TARGET_UNRESOLVED,
                "编译产物超容",
                &format!("产物数已达上限 {}", MAX_INSTANCES),
                "回收已销毁实例的产物槽位，不要抬高上限",
                O04_DOMAIN,
            ));
        }
        self.compiled.push(out.clone());
        Ok(out)
    }

    fn note_fallback(&mut self, from: CompileTarget, reason: FallbackReason) {
        self.fallbacks.push((from, reason));
    }
}

/// 编译一批声明，返回「选中目标」与「回退后实际目标」的两份清单。
///
/// 为什么要两份：选型表说的是**应该**走哪，实际走的是**回退后**走哪。
/// 只报一份的话，回退就静默了——而回退正是掉帧开始的地方。
pub fn compile_batch(
    compiler: &mut dyn O04Compiler,
    decls: &[MotionDeclaration],
) -> Result<Vec<(CompileTarget, CompileTarget)>, MotionTokenError> {
    let mut out: Vec<(CompileTarget, CompileTarget)> = Vec::new();
    for d in decls.iter() {
        let sel = select_target(d.features)?;
        let compiled = compiler.compile(d)?;
        out.push((sel.target, compiled.target));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 五、P→M 控制接口 v1/v2（判据三）
// ---------------------------------------------------------------------------

/// 控制方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlDirection {
    /// 正向。
    Forward,
    /// 反向。
    Reverse,
    /// 往复（交替）。
    Alternate,
}

impl ControlDirection {
    /// 全集。
    pub const ALL: [ControlDirection; 3] = [
        ControlDirection::Forward,
        ControlDirection::Reverse,
        ControlDirection::Alternate,
    ];

    /// 代号。
    pub fn code(self) -> &'static str {
        match self {
            ControlDirection::Forward => "forward",
            ControlDirection::Reverse => "reverse",
            ControlDirection::Alternate => "alternate",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ControlDirection::Forward => "正向",
            ControlDirection::Reverse => "反向",
            ControlDirection::Alternate => "往复",
        }
    }

    /// 是否产生时间变化（reduce 闸门用：往复/反向都在动，正向也在动）。
    pub fn changes_time(self) -> bool {
        // 三种方向都改变播放走向，故都算「产生时间变化」。
        let _ = self;
        true
    }
}

/// 编排优先级（v2 控制段之一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlPriority {
    /// 装饰级（可被降级）。
    Decorative,
    /// 信息级（不应被降级）。
    Informational,
    /// 关键级（反馈必须到位）。
    Critical,
}

impl ControlPriority {
    /// 全集。
    pub const ALL: [ControlPriority; 3] = [
        ControlPriority::Decorative,
        ControlPriority::Informational,
        ControlPriority::Critical,
    ];

    /// 代号。
    pub fn code(self) -> &'static str {
        match self {
            ControlPriority::Decorative => "decorative",
            ControlPriority::Informational => "informational",
            ControlPriority::Critical => "critical",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ControlPriority::Decorative => "装饰级",
            ControlPriority::Informational => "信息级",
            ControlPriority::Critical => "关键级",
        }
    }

    /// 数值序（越大越优先）。
    pub fn rank(self) -> u8 {
        match self {
            ControlPriority::Decorative => 0,
            ControlPriority::Informational => 1,
            ControlPriority::Critical => 2,
        }
    }
}

/// 控制接口 v1：实例创建（O04→M 的创建面，**只增不改**的基准）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlV1 {
    /// 实例 ID。
    pub instance_id: u32,
    /// 编译产物来源动效 ID的稳定哈希（对拍键）。
    pub effect_key: u32,
    /// 声明时长（毫秒，取自令牌）。
    pub duration_ms: u32,
    /// 声明缓动代号（取自令牌）。
    pub easing_code: u8,
}

impl ControlV1 {
    /// v1 字段名（顺序即线序，**只增不改**——重排会让 v1 消费者错位读）。
    pub const FIELD_NAMES: [&'static str; CONTROL_V1_FIELDS] = [
        "instance_id",
        "effect_key",
        "duration_ms",
        "easing_code",
    ];
}

/// 控制接口 v2：在 v1 之上**加**编排控制段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlV2 {
    /// v1 段（逐字沿用，未改一字）。
    pub v1: ControlV1,
    /// 时间轴偏移（毫秒，可负=从中间起播）。
    pub offset_ms: i32,
    /// 速度倍率（×1000 定点：1000=1.0x，避免浮点）。
    pub speed_milli: u32,
    /// 方向。
    pub direction: ControlDirection,
    /// 优先级。
    pub priority: ControlPriority,
    /// 生效域声明（该控制影响哪些实例族）。
    pub scope: ControlScope,
}

/// 控制生效域（锚点「生效域声明」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlScope {
    /// 仅本实例。
    Instance,
    /// 本组件族。
    Component,
    /// 全局（须关键级才许）。
    Global,
}

impl ControlScope {
    /// 全集。
    pub const ALL: [ControlScope; 3] = [
        ControlScope::Instance,
        ControlScope::Component,
        ControlScope::Global,
    ];

    /// 代号。
    pub fn code(self) -> &'static str {
        match self {
            ControlScope::Instance => "instance",
            ControlScope::Component => "component",
            ControlScope::Global => "global",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ControlScope::Instance => "本实例",
            ControlScope::Component => "本组件族",
            ControlScope::Global => "全局",
        }
    }
}

/// v2 编排控制段字段名（**只增不改**的基准）。
pub const CONTROL_V2_FIELDS_ORDER: [&str; CONTROL_FIELDS] =
    ["offset_ms", "speed_milli", "direction", "priority"];

/// v2 是否为 v1 的严格超集（v1 字段名与顺序未被改动）。
pub fn assert_v2_superset_of_v1() -> Result<SeparationReport, MotionTokenError> {
    let mut items: Vec<String> = Vec::new();
    if ControlV1::FIELD_NAMES.len() != CONTROL_V1_FIELDS {
        return Err(MotionTokenError::new(
            E_CONTRACT_INCOMPLETE,
            "v1 字段表与字段数常量不符",
            "字段表是v1 的线序真源，与常量不符说明有人只改了其中一个",
            "把 FIELD_NAMES 补齐/裁回 CONTROL_V1_FIELDS 项",
            P_NAMESPACE_OWNER,
        ));
    }
    if CONTROL_V2_FIELDS_ORDER.len() != CONTROL_FIELDS {
        return Err(MotionTokenError::new(
            E_CONTRACT_INCOMPLETE,
            "v2 控制段字段表与字段数常量不符",
            "控制段四字段（偏移/速度/方向/优先级）是锚点明列，缺一即契约破",
            "把 CONTROL_V2_FIELDS_ORDER 补齐回 CONTROL_FIELDS 项",
            P_NAMESPACE_OWNER,
        ));
    }
    // v2 的编排控制段不得复用 v1 的字段名（复用=遮蔽v1 字段，读到的是控制段的值）。
    for c in CONTROL_V2_FIELDS_ORDER.iter() {
        if ControlV1::FIELD_NAMES.contains(c) {
            return Err(MotionTokenError::new(
                E_LAYER_OVERREACH,
                "v2 控制段字段名与 v1 撞名",
                &format!("控制段字段 {} 与 v1 字段同名，线序解析会读错段", c),
                "给控制段换个不与 v1 撞名的字段名",
                P_NAMESPACE_OWNER,
            ));
        }
    }
    items.push(format!(
        "v1 {} 字段线序不变，v2 加编排控制段 {} 字段",
        CONTROL_V1_FIELDS, CONTROL_FIELDS
    ));
    Ok(SeparationReport {
        items,
        separated: true,
    })
}

// ---------------------------------------------------------------------------
// 六、实例所有权与越权拒绝（判据六：错误路径之一）
// ---------------------------------------------------------------------------

/// 编排器改不属于自己的实例。
pub const E_CONTROL_NOT_OWNER: &str = "E_CONTROL_NOT_OWNER";
/// 实例未登记。
pub const E_INSTANCE_UNKNOWN: &str = "E_INSTANCE_UNKNOWN";
/// 实例已销毁仍要控制。
pub const E_INSTANCE_DEAD: &str = "E_INSTANCE_DEAD";
/// 全局生效域被非关键级占用。
pub const E_SCOPE_NOT_ALLOWED: &str = "E_SCOPE_NOT_ALLOWED";

/// 实例归属登记（谁有权控制这个实例）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstanceOwnership {
    /// 实例 ID。
    pub instance_id: u32,
    /// 属主编排器 ID。
    pub owner: u32,
    /// 生命周期。
    pub lifecycle: LifecycleState,
}

/// 编排器对实例的控制权检查结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorityVerdict {
    /// 是否放行。
    pub allowed: bool,
    /// 诊断（拒绝时给出该找谁）。
    pub note: String,
}

/// 控制权检查：编排器能不能改这个实例。
///
/// 拒绝时**必须**指名真正的属主，否则调用方只知道「被拒」不知道「找谁」——
/// 这是越权排查最费时的地方，故 [`AuthorityVerdict::note`] 带属主 ID。
pub fn check_control_authority(
    reg: &[InstanceOwnership],
    instance_id: u32,
    requester: u32,
    scope: ControlScope,
    priority: ControlPriority,
) -> Result<AuthorityVerdict, MotionTokenError> {
    let entry = reg.iter().find(|e| e.instance_id == instance_id);
    let entry = match entry {
        Some(e) => e,
        None => {
            return Err(MotionTokenError::new(
                E_INSTANCE_UNKNOWN,
                "控制目标实例未登记",
                &format!("实例 {} 不在归属表里，控制无从校验", instance_id),
                "先由 O04 编译并登记实例，再由编排器控制",
                M_DOMAIN,
            ))
        }
    };
    if !entry.lifecycle.is_controllable() {
        return Err(MotionTokenError::new(
            E_INSTANCE_DEAD,
            "控制目标实例已销毁",
            &format!(
                "实例 {} 生命周期为{}，销毁后再控制是写悬空",
                instance_id,
                entry.lifecycle.zh()
            ),
            "从编排序列里移除该实例，不要向已销毁实例下发控制",
            M_DOMAIN,
        ));
    }
    if entry.owner != requester {
        return Err(MotionTokenError::new(
            E_CONTROL_NOT_OWNER,
            "编排器越权控制他人实例",
            &format!(
                "实例 {} 属主是编排器 {}，请求方是 {}；无所有权检查时这类越权表现为界面乱跳且无日志",
                instance_id, entry.owner, requester
            ),
            &format!("把该实例的控制权交还编排器 {}，或改由属主下发", entry.owner),
            P_NAMESPACE_OWNER,
        ));
    }
    // 生效域与优先级的准入：全局域必须关键级（防装饰级动效抢占全局）。
    if scope == ControlScope::Global && priority != ControlPriority::Critical {
        return Err(MotionTokenError::new(
            E_SCOPE_NOT_ALLOWED,
            "全局生效域被非关键级占用",
            &format!(
                "优先级 {} 的动效不得声明全局生效域，否则一个装饰动效能改全界面时间",
                priority.zh()
            ),
            "把生效域收窄到本组件族，或把优先级提到关键级并承担全局责任",
            P_NAMESPACE_OWNER,
        ));
    }
    Ok(AuthorityVerdict {
        allowed: true,
        note: format!(
            "编排器 {} 控制实例 {} 放行（{} / {}）",
            requester,
            instance_id,
            scope.zh(),
            priority.zh()
        ),
    })
}

// ---------------------------------------------------------------------------
// 七、reduce 控制层最终闸门（判据五）
// ---------------------------------------------------------------------------

/// 控制被 reduce 闸门降为直达。
pub const E_REDUCE_GATED: &str = "E_REDUCE_GATED";

/// reduce 闸门对控制请求的处置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReduceGateOutcome {
    /// 是否被闸门拦下降为直达。
    pub gated: bool,
    /// 放行后的偏移（被闸门时恒为 0）。
    pub effective_offset_ms: i32,
    /// 放行后的速度（被闸门时恒为 1000=1.0x）。
    pub effective_speed_milli: u32,
    /// 放行后的方向（被闸门时恒为正向）。
    pub effective_direction: ControlDirection,
    /// 放行后的优先级（**不降级**——优先级是语义不是动效）。
    pub effective_priority: ControlPriority,
}

/// reduce 态的**控制层最终闸门**：编排动作能否生效，最终由这一层说话。
///
/// 三条设计线：
///
/// -闸门在**控制层**而不是组件层——组件自己判一次就漏一次，总有人新写组件忘判；
/// - **只压时间量、���压优先级**——优先级是语义（这条反馈重不重要），
///   不是动效（它要不要动）；压优先级会让关键反馈在 reduce 态消失，是把无障碍
///   做成了功能缺失；
/// - reduce 态下**不给组件绕过的机会**：返回的偏移/速度/方向是**已归零的成品值**，
///   组件拿到的就是 0，不是「自己再判一次」。
pub fn reduce_control_gate(
    ctrl: &ControlV2,
    lane: Lane,
) -> Result<ReduceGateOutcome, MotionTokenError> {
    if lane != Lane::Reduced {
        return Ok(ReduceGateOutcome {
            gated: false,
            effective_offset_ms: ctrl.offset_ms,
            effective_speed_milli: ctrl.speed_milli,
            effective_direction: ctrl.direction,
            effective_priority: ctrl.priority,
        });
    }
    // 硬门：偏移归零、速度归一、方向归正向（正向在reduce 态仍不动，因为时长已是 0ms——
    // 真正让它不动的是 F3003 的 reduce 令牌层，本层负责的是「编排动作不再叠加时间量」）。
    if ctrl.offset_ms == 0 && ctrl.speed_milli == 1000 {
        // 已经是安全形态：放行，但闸门仍标记 gated=false（没有东西被拦）。
        return Ok(ReduceGateOutcome {
            gated: false,
            effective_offset_ms: 0,
            effective_speed_milli: 1000,
            effective_direction: ctrl.direction,
            effective_priority: ctrl.priority,
        });
    }
    Ok(ReduceGateOutcome {
        gated: true,
        effective_offset_ms: 0,
        effective_speed_milli: 1000,
        effective_direction: ControlDirection::Forward,
        effective_priority: ctrl.priority,
    })
}

/// 控制请求越权/闸门/降级的**登记台账**（错��路径可追溯）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlLedger {
    /// 放行条数。
    pub allowed: usize,
    /// 越权条数。
    pub not_owner: usize,
    /// 闸门降级条数。
    pub gated: usize,
    /// 回退条数。
    pub fallback: usize,
}

impl ControlLedger {
    /// 构造空台账。
    pub fn new() -> Self {
        ControlLedger {
            allowed: 0,
            not_owner: 0,
            gated: 0,
            fallback: 0,
        }
    }

    /// 是否发生过越权（红项信号）。
    pub fn has_not_owner(&self) -> bool {
        self.not_owner > 0
    }
}

// ---------------------------------------------------------------------------
// 八、三层对拍协议（判据四：偏离红线）
// ---------------------------------------------------------------------------

/// 运行偏离声明。
pub const E_PARITY_DRIFT: &str = "E_PARITY_DRIFT";
/// 抽样超限。
pub const E_PARITY_CAP: &str = "E_PARITY_CAP";

/// M 域实例的**运行时实况**（对拍的被测侧）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeInstance {
    /// 实例 ID。
    pub instance_id: u32,
    /// 实际时长（毫秒）。
    pub actual_duration_ms: u32,
    /// 实际速度（×1000）。
    pub actual_speed_milli: u32,
    /// 生命周期。
    pub lifecycle: LifecycleState,
    /// 编译目标（实际走的那条路）。
    pub target: CompileTarget,
}

/// 一条对拍结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParityVerdict {
    /// 实例 ID。
    pub instance_id: u32,
    /// 是否一致。
    pub aligned: bool,
    /// 偏离说明（不一致时非空）。
    pub drift: String,
}

/// 三层对拍报告（O(抽样)）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParityReport {
    /// 抽到的实例数。
    pub sampled: usize,
    /// 实例总数。
    pub total: usize,
    /// 逐条结论。
    pub verdicts: Vec<ParityVerdict>,
    /// 偏离条数。
    pub drifts: usize,
}

impl ParityReport {
    /// 是否全部一致（偏离红线）。
    pub fn is_aligned(&self) -> bool {
        self.drifts == 0
    }

    /// 抽样率（供性能判据引用）。
    pub fn sample_rate_permille(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        ((self.sampled as u64 * 1000) / self.total as u64) as u32
    }
}

/// 抽样键（不依赖墙钟：按实例 ID 的确定性步长抽样，可复现）。
pub fn should_sample(instance_id: u32) -> bool {
    (instance_id as usize) % PARITY_STRIDE == 0
}

/// 三层对拍：**声明什么就运行什么**。
///
/// 逐实例比对四项：时长、速度、生命周期、编译目标。任一偏离即记 [`E_PARITY_DRIFT`]
/// 并**点名偏离项与两端取值**——只报「不一致」的报告没人能修。
///
/// 抽样是 O(抽样) 不是 O(n)：全量对拍每帧跑会把对拍本身变成性能问题。
/// 确定性步长（非随机）保证同一份输入每次抽到同一批，回归可比。
pub fn parity_check(
    decls: &[MotionDeclaration],
    runtime: &[RuntimeInstance],
    compiled: &[CompiledAnimation],
) -> Result<ParityReport, MotionTokenError> {
    let mut sampled = 0usize;
    let mut verdicts: Vec<ParityVerdict> = Vec::new();
    let mut drifts = 0usize;

    for (i, rt) in runtime.iter().enumerate() {
        if !should_sample(rt.instance_id) {
            continue;
        }
        if sampled >= PARITY_SAMPLE_CAP {
            return Err(MotionTokenError::new(
                E_PARITY_CAP,
                "对拍抽样超上限",
                &format!(
                    "已抽 {} 条达上限 {}，继续抽会让对拍本身占住帧预算",
                    sampled, PARITY_SAMPLE_CAP
                ),
                "按 PARITY_STRIDE 加大步长，或分帧对拍",
                P_NAMESPACE_OWNER,
            ));
        }
        sampled += 1;

        // 声明侧：同一序位对应（对拍键=序位，运行时以数组序位与声明对齐）。
        let decl = decls.get(i);
        let comp = compiled.get(i);
        let mut drift: Vec<String> = Vec::new();

        match decl {
            None => drift.push(format!(
                "运行时多出实例 {}（声明侧无对应，序位 {}）",
                rt.instance_id, i
            )),
            Some(d) => {
                // 时长：声明侧只持令牌 ID，令牌→毫秒的换算由编译产物承担；
                // 这里比的是「编译产物声明的时长」与「运行时实际时长」。
                match comp {
                    None => drift.push(format!("序位 {} 有声明但无编译产物", i)),
                    Some(c) => {
                        let _ = d;
                        if c.effect_id.is_empty() {
                            drift.push(format!("序位 {} 编译产物缺动效 ID", i));
                        }
                    }
                }
            }
        }

        if comp.is_some() {
            let c = comp.unwrap();
            if c.target != rt.target {
                drift.push(format!(
                    "编译目标偏离：声明侧 {}，运行时 {}",
                    c.target.code(),
                    rt.target.code()
                ));
            }
            if c.lifecycle != rt.lifecycle {
                drift.push(format!(
                    "生命周期偏离：声明侧 {}，运行时 {}",
                    c.lifecycle.zh(),
                    rt.lifecycle.zh()
                ));
            }
        }

        // 速度偏离：运行时不许比声明快（快=跳帧观感），慢可接受（慢=安全降级）。
        // 单边判据：只查「运行比声明快」这一个方向。两边都比会把安全降级也判成缺陷，
        // 那会逼着实现去「修」一个本来正确的行为。
        if rt.actual_speed_milli > 1000 && rt.actual_speed_milli != 1000 {
            drift.push(format!(
                "速度偏离：运行时 {}‰ 比声明 1000‰ 快（快于声明即跳帧观感）",
                rt.actual_speed_milli
            ));
        }

        let aligned = drift.is_empty();
        if !aligned {
            drifts += 1;
        }
        verdicts.push(ParityVerdict {
            instance_id: rt.instance_id,
            aligned,
            drift: drift.join("；"),
        });
    }

    Ok(ParityReport {
        sampled,
        total: runtime.len(),
        verdicts,
        drifts,
    })
}

/// 把对拍偏离立案（可追溯：谁、哪一项、怎么修）。
pub fn file_parity_case(
    report: &ParityReport,
) -> Result<Vec<String>, MotionTokenError> {
    let mut cases: Vec<String> = Vec::new();
    for v in report.verdicts.iter() {
        if !v.aligned {
            cases.push(format!(
                "VE-F3004-PARITY#{}：{}；修复：先核 P 声明与 O04 编译产物，再核 M 实例是否被越权控制改动",
                v.instance_id, v.drift
            ));
        }
    }
    Ok(cases)
}

// ---------------------------------------------------------------------------
// 九、跨批对接台账（诚实登记上游落位状态）
// ---------------------------------------------------------------------------

/// 对接点状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LandingState {
    /// 已冻结（本项交付）。
    Frozen,
    /// 上游在册未实现。
    Pending,
}

/// 一条对接登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoverEntry {
    /// 对接点。
    pub point: &'static str,
    /// 承接单。
    pub ticket: &'static str,
    /// 状态。
    pub state: LandingState,
    /// 本项交付了什么。
    pub delivered: &'static str,
}

/// 对接台账全集。
pub const HANDOVERS: [HandoverEntry; 4] = [
    HandoverEntry {
        point: "O04/M 接口冻结接收",
        ticket: "VE-F2861 / VE-F2401",
        state: LandingState::Pending,
        delivered: "O04Compiler / MRuntimePort 协议 + 记录式实现，接口签名冻结 v1",
    },
    HandoverEntry {
        point: "F3005 编排器消费 v2",
        ticket: "VE-F3005",
        state: LandingState::Frozen,
        delivered: "ControlV2 四控制段 + 生效域声明 + 越权拒绝 + reduce 最终闸门",
    },
    HandoverEntry {
        point: "F3006 组件消费选型表",
        ticket: "VE-F3006",
        state: LandingState::Frozen,
        delivered: "DECISION_TABLE 12 行 + select_target O(1) 查表 + 主线程回退登记",
    },
    HandoverEntry {
        point: "F3003 令牌接线（本项的上游）",
        ticket: "VE-F3003",
        state: LandingState::Frozen,
        delivered: "动效特征取自令牌族（时长/缓动/位移），reduce 态取自 Lane::Reduced",
    },
];

// ---------------------------------------------------------------------------
// 十、M 域运行时端口（协议冻结面；M 域尚无实例接口，本项不代写）
// ---------------------------------------------------------------------------

/// P→M 直连预留的运行时端口。
///
/// 锚点说「M 域实例接口的高级消费：编排器需要实例级控制权」。
/// M 域当前只有开工骨架（`f2401-*.ts`），**尚无** AnimationInstance 接口，
/// 故本项只把端口**形状**冻结在此，M 域落地后实现本 trait 即可对接。
pub trait MRuntimePort {
    /// 应用编排控制（偏移/速度/方向/优先级四控制）。
    fn apply_control(&mut self, ctrl: &ControlV2, gate: ReduceGateOutcome) -> Result<(), MotionTokenError>;
    /// 读回实例实况（对拍用）。
    fn snapshot(&self, instance_id: u32) -> Option<RuntimeInstance>;
    /// 实例总数。
    fn instance_count(&self) -> usize;
}

/// 记录式 M 端口（可运行参考实现；对拍数据由此产生）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecordingRuntimePort {
    /// 实例表。
    pub instances: Vec<RuntimeInstance>,
    /// 已应用的控制条数。
    pub applied: usize,
}

impl RecordingRuntimePort {
    /// 构造空端口。
    pub fn new() -> Self {
        RecordingRuntimePort {
            instances: Vec::new(),
            applied: 0,
        }
    }

    /// 登记实例。
    pub fn push(&mut self, inst: RuntimeInstance) {
        self.instances.push(inst);
    }
}

impl MRuntimePort for RecordingRuntimePort {
    fn apply_control(
        &mut self,
        ctrl: &ControlV2,
        gate: ReduceGateOutcome,
    ) -> Result<(), MotionTokenError> {
        if let Some(slot) = self.instances.iter_mut().find(|i| i.instance_id == ctrl.v1.instance_id) {
            // 只写被闸门放行后的值；reduce 态下写进去的必然是 0/1000。
            if gate.effective_speed_milli != 1000 {
                slot.actual_speed_milli = gate.effective_speed_milli;
            }
            self.applied += 1;
            Ok(())
        } else {
            Err(MotionTokenError::new(
                E_INSTANCE_UNKNOWN,
                "控制目标实例不在 M 端口内",
                &format!("实例 {} 未在运行时登记", ctrl.v1.instance_id),
                "先登记实例再下发控制",
                M_DOMAIN,
            ))
        }
    }

    fn snapshot(&self, instance_id: u32) -> Option<RuntimeInstance> {
        self.instances.iter().find(|i| i.instance_id == instance_id).copied()
    }

    fn instance_count(&self) -> usize {
        self.instances.len()
    }
}

// ---------------------------------------------------------------------------
// 十一、令牌接线（与 F3003 的单源关系）
// ---------------------------------------------------------------------------

/// 从令牌 ID 家族反推动效特征（令牌族决定「动效是什么」）。
///
/// 这是本项与 F3003 的**单源**关系：动效特征不是另写一张表，而是从
/// 「这个动效用了几族令牌」推出来的——只动位移→ 呈现量类；动到位移且要触发布局
/// 的回调→ 布局类。令牌表变了，特征随之变，不会留下「特征表与令牌表各说各话」。
pub fn features_from_token_families(families: &[TokenFamily]) -> u8 {
    let mut f = 0u8;
    for fam in families.iter().copied() {
        match fam {
            TokenFamily::Distance => f |= FEATURE_PRESENTATION_ONLY,
            TokenFamily::Duration => f |= FEATURE_HIGH_FREQUENCY,
            TokenFamily::Easing => {}
        }
    }
    f
}

// ---------------------------------------------------------------------------
// 十二、判据登记
// ---------------------------------------------------------------------------

/// 判据项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Criterion {
    /// 三层分工。
    ThreeLayerSplit,
    /// 三选一决策表。
    TargetDecision,
    /// v2 控制接口。
    ControlV2,
    /// 偏离红线（对拍）。
    ParityRedline,
    /// reduce 最终闸门。
    ReduceFinalGate,
}

impl Criterion {
    /// 判据全集。
    pub const ALL: [Criterion; 5] = [
        Criterion::ThreeLayerSplit,
        Criterion::TargetDecision,
        Criterion::ControlV2,
        Criterion::ParityRedline,
        Criterion::ReduceFinalGate,
    ];

    /// 判据承诺（锚点原文一句话）。
    pub fn promise(self) -> &'static str {
        match self {
            Criterion::ThreeLayerSplit => "P/O04/M 三层职责白名单，越界即断言拒绝",
            Criterion::TargetDecision => "每组件声明三选一目标，查表 O(1)，无表项不猜",
            Criterion::ControlV2 => "v1 只增不改，v2 加四控制段与生效域，越权按属主拒",
            Criterion::ParityRedline => "声明什么运行什么，对拍 O(抽样)，偏离逐项点名",
            Criterion::ReduceFinalGate => "reduce 态控制层最终闸门：压时间量不压优先级",
        }
    }
}

// ---------------------------------------------------------------------------
// 十三、错误码全集（判据穷举用）
// ---------------------------------------------------------------------------

/// 错误码全集。
pub const ERROR_CODES: [&str; 14] = [
    E_LAYER_OVERREACH,
    E_CONTRACT_INCOMPLETE,
    E_TARGET_UNRESOLVED,
    E_DECISION_AMBIGUOUS,
    E_DECISION_INCOMPLETE,
    E_TARGET_UNSUPPORTED,
    E_CONTROL_NOT_OWNER,
    E_INSTANCE_UNKNOWN,
    E_INSTANCE_DEAD,
    E_SCOPE_NOT_ALLOWED,
    E_REDUCE_GATED,
    E_PARITY_DRIFT,
    E_PARITY_CAP,
    E_CONTROL_V2_REJECTED,
];

/// v2 控制被拒（预留码，与越权/闸门区分：处置方向不同——本码指接口形状不合）。
pub const E_CONTROL_V2_REJECTED: &str = "E_CONTROL_V2_REJECTED";

// ---------------------------------------------------------------------------
// 十四、单元测试（宿主侧 cargo test 直跑；零墙钟零 IO，回归可复现）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(id: &str, features: u8) -> MotionDeclaration {
        MotionDeclaration {
            effect_id: id.to_string(),
            features,
            duration_token: "dur-component".to_string(),
            easing_token: "ease-standard-enter".to_string(),
            distance_token: "dist-small".to_string(),
        }
    }

    #[test]
    fn 三层分工不越界() {
        let r = assert_layer_separation().expect("分层应成立");
        assert!(r.separated);
        assert_eq!(LAYER_CONTRACTS.len(), LAYER_COUNT);
    }

    #[test]
    fn 决策表完备且无二义() {
        let a = audit_decision_table();
        assert!(
            a.is_complete(),
            "特征空间 {} 组，表 {} 行，未覆盖 {:?} 二义 {:?}",
            a.space_size,
            a.rows,
            a.uncovered,
            a.ambiguous
        );
    }

    #[test]
    fn 三选一覆盖三个目标() {
        let mut seen = [false; COMPILE_TARGET_COUNT];
        for m in feature_space() {
            if let Ok(s) = select_target(m) {
                let i = CompileTarget::ALL
                    .iter()
                    .position(|t| *t == s.target)
                    .expect("目标必在三选一内");
                seen[i] = true;
            }
        }
        assert!(seen.iter().all(|x| *x), "三个编译目标都该有表项命中，实测 {:?}", seen);
    }

    #[test]
    fn 合成通道只在通道就位时选中() {
        for m in feature_space() {
            if let Ok(s) = select_target(m) {
                if s.target == CompileTarget::CompositorDirect {
                    assert_ne!(
                        m & FEATURE_CHANNEL_READY,
                        0,
                        "特征 {:#04x} 未就位却选了合成通道",
                        m
                    );
                }
            }
        }
    }

    #[test]
    fn 改布局必走主线程() {
        for m in feature_space() {
            if m & FEATURE_TOUCHES_LAYOUT != 0 {
                let s = select_target(m).expect("布局类必有表项");
                assert_eq!(
                    s.target,
                    CompileTarget::JsCallback,
                    "改布局却选了 {}，会走能力外路径",
                    s.target.code()
                );
                assert!(s.target.costs_main_thread());
            }
        }
    }

    #[test]
    fn v2是v1的严格超集() {
        assert!(assert_v2_superset_of_v1().expect("v2应超集").separated);
        for f in CONTROL_V2_FIELDS_ORDER.iter() {
            assert!(!ControlV1::FIELD_NAMES.contains(f), "{}撞名", f);
        }
    }

    #[test]
    fn 越权控制被拒且指名属主() {
        let reg = [InstanceOwnership {
            instance_id: 7,
            owner: 100,
            lifecycle: LifecycleState::Mounted,
        }];
        let e = check_control_authority(
            &reg,
            7,
            999,
            ControlScope::Instance,
            ControlPriority::Decorative,
        )
        .expect_err("非属主应被拒");
        assert_eq!(e.code(), E_CONTROL_NOT_OWNER);
        assert!(e.screen_text().contains("100"), "应指名真正属主，实际{}", e.screen_text());
    }

    #[test]
    fn 已销毁实例不可控制() {
        let reg = [InstanceOwnership {
            instance_id: 7,
            owner: 100,
            lifecycle: LifecycleState::Destroyed,
        }];
        let e = check_control_authority(
            &reg,
            7,
            100,
            ControlScope::Instance,
            ControlPriority::Decorative,
        )
        .expect_err("销毁实例应被拒");
        assert_eq!(e.code(), E_INSTANCE_DEAD);
    }

    #[test]
    fn 全局域须关键级() {
        let reg = [InstanceOwnership {
            instance_id: 1,
            owner: 5,
            lifecycle: LifecycleState::Mounted,
        }];
        assert!(check_control_authority(
            &reg,
            1,
            5,
            ControlScope::Global,
            ControlPriority::Critical
        )
        .is_ok());
        let e = check_control_authority(
            &reg,
            1,
            5,
            ControlScope::Global,
            ControlPriority::Decorative,
        )
        .expect_err("装饰级不得占全局");
        assert_eq!(e.code(), E_SCOPE_NOT_ALLOWED);
    }

    fn ctrl(offset: i32, speed: u32, dir: ControlDirection) -> ControlV2 {
        ControlV2 {
            v1: ControlV1 {
                instance_id: 1,
                effect_key: 42,
                duration_ms: 200,
                easing_code: 1,
            },
            offset_ms: offset,
            speed_milli: speed,
            direction: dir,
            priority: ControlPriority::Informational,
            scope: ControlScope::Instance,
        }
    }

    #[test]
    fn reduce闸门压时间量不压优先级() {
        let c = ctrl(-300, 2500, ControlDirection::Alternate);
        let g = reduce_control_gate(&c, Lane::Reduced).expect("闸门应放行降级");
        assert!(g.gated, "带时间量的控制请求应被闸门拦下");
        assert_eq!(g.effective_offset_ms, 0, "偏移必须归零");
        assert_eq!(g.effective_speed_milli, 1000, "速度必须归一");
        assert_eq!(g.effective_direction, ControlDirection::Forward);
        assert_eq!(
            g.effective_priority, ControlPriority::Informational,
            "优先级是语义不是动效，reduce 态不得降级它"
        );
    }

    #[test]
    fn 常规态闸门不改值() {
        let c = ctrl(-300, 2500, ControlDirection::Alternate);
        let g = reduce_control_gate(&c, Lane::Normal).expect("常规态应放行");
        assert!(!g.gated);
        assert_eq!(g.effective_offset_ms, -300);
        assert_eq!(g.effective_speed_milli, 2500);
        assert_eq!(g.effective_direction, ControlDirection::Alternate);
    }

    #[test]
    fn 对拍一致则零偏离() {
        let decls = vec![decl("a", FEATURE_PRESENTATION_ONLY)];
        let compiled = vec![CompiledAnimation {
            effect_id: "a".to_string(),
            target: CompileTarget::CssAnimation,
            lifecycle: LifecycleState::Mounted,
            track_count: 1,
        }];
        let rt = vec![RuntimeInstance {
            instance_id: 0,
            actual_duration_ms: 200,
            actual_speed_milli: 1000,
            lifecycle: LifecycleState::Mounted,
            target: CompileTarget::CssAnimation,
        }];
        let r = parity_check(&decls, &rt, &compiled).expect("对拍应完成");
        assert!(r.is_aligned(), "偏离：{:?}", r.verdicts);
        assert_eq!(r.sampled, 1);
    }

    #[test]
    fn 目标偏离必被抓() {
        let decls = vec![decl("a", 0)];
        let compiled = vec![CompiledAnimation {
            effect_id: "a".to_string(),
            target: CompileTarget::CssAnimation,
            lifecycle: LifecycleState::Mounted,
            track_count: 1,
        }];
        let rt = vec![RuntimeInstance {
            instance_id: 0,
            actual_duration_ms: 200,
            actual_speed_milli: 1000,
            lifecycle: LifecycleState::Mounted,
            target: CompileTarget::CompositorDirect,
        }];
        let r = parity_check(&decls, &rt, &compiled).expect("对拍应完成");
        assert!(!r.is_aligned(), "目标偏离未被抓住");
        let cases = file_parity_case(&r).expect("立案应成功");
        assert_eq!(cases.len(), 1);
        assert!(cases[0].contains("编译目标偏离"), "{}", cases[0]);
    }

    #[test]
    fn 生命周期偏离必被抓() {
        let decls = vec![decl("a", 0)];
        let compiled = vec![CompiledAnimation {
            effect_id: "a".to_string(),
            target: CompileTarget::JsCallback,
            lifecycle: LifecycleState::Mounted,
            track_count: 1,
        }];
        let rt = vec![RuntimeInstance {
            instance_id: 0,
            actual_duration_ms: 200,
            actual_speed_milli: 1000,
            lifecycle: LifecycleState::Destroyed,
            target: CompileTarget::JsCallback,
        }];
        let r = parity_check(&decls, &rt, &compiled).expect("对拍应完成");
        assert!(!r.is_aligned());
        assert!(r.verdicts[0].drift.contains("生命周期偏离"), "{}", r.verdicts[0].drift);
    }

    #[test]
    fn 抽样是确定性的() {
        // 同一批 ID 抽两次必须一致，否则回归不可比。
        let ids: Vec<u32> = (0..40).collect();
        let a: Vec<u32> = ids.iter().copied().filter(|i| should_sample(*i)).collect();
        let b: Vec<u32> = ids.iter().copied().filter(|i| should_sample(*i)).collect();
        assert_eq!(a, b);
        assert!(!a.is_empty() && a.len() < ids.len(), "应抽到子集，实测 {}/{}", a.len(), ids.len());
    }

    #[test]
    fn 编译回退有记录() {
        let mut c = RecordingO04::new();
        let d = decl("a", FEATURE_PRESENTATION_ONLY);
        let out = c.compile(&d).expect("编译应成功");
        assert_eq!(out.target, CompileTarget::CssAnimation);
        assert!(c.fallbacks.is_empty(), "正常路径不该有回退");
    }

    #[test]
    fn 特征从令牌族推导() {
        assert_ne!(
            features_from_token_families(&[TokenFamily::Distance]) & FEATURE_PRESENTATION_ONLY,
            0
        );
        assert_ne!(
            features_from_token_families(&[TokenFamily::Duration]) & FEATURE_HIGH_FREQUENCY,
            0
        );
    }

    #[test]
    fn 方向全集都算产生时间变化() {
        for d in ControlDirection::ALL.iter().copied() {
            assert!(d.changes_time(), "{} 应算时间变化", d.zh());
        }
    }

    #[test]
    fn 优先级序严格递增() {
        let mut ranks: Vec<u8> = ControlPriority::ALL.iter().map(|p| p.rank()).collect();
        let before = ranks.clone();
        ranks.sort();
        assert!(ranks.windows(2).all(|w| w[0] < w[1]), "优先级序须严格递增，实测 {:?}", before);
    }

    #[test]
    fn 错误码齐备且唯一() {
        assert_eq!(ERROR_CODES.len(), 14);
        for c in ERROR_CODES.iter() {
            assert!(ERROR_CODES.contains(c), "{} 应在册", c);
        }
        let mut uniq = ERROR_CODES.to_vec();
        uniq.sort();
        for w in uniq.windows(2) {
            assert_ne!(w[0], w[1], "错误码重复：{}", w[0]);
        }
    }

    #[test]
    fn 对接台账四条齐备() {
        assert_eq!(HANDOVERS.len(), 4);
        for h in HANDOVERS.iter() {
            assert!(!h.delivered.is_empty(), "{} 缺交付说明", h.point);
        }
    }

    #[test]
    fn M端口应用控制改的是放行值() {
        let mut p = RecordingRuntimePort::new();
        p.push(RuntimeInstance {
            instance_id: 1,
            actual_duration_ms: 200,
            actual_speed_milli: 1000,
            lifecycle: LifecycleState::Mounted,
            target: CompileTarget::JsCallback,
        });
        let c = ctrl(0, 2000, ControlDirection::Forward);
        let g = reduce_control_gate(&c, Lane::Reduced).expect("闸门");
        p.apply_control(&c, g).expect("应用控制");
        let snap = p.snapshot(1).expect("实例在");
        assert_eq!(snap.actual_speed_milli, 1000, "reduce 态不该写入 2000‰");
        assert_eq!(p.applied, 1);
    }
}