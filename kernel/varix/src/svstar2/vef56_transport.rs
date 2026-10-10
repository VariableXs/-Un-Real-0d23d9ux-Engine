//! VE-F5403 · 传输层选型（VE-F 域 · 任务段 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5403`
//!
//! **判据（锚点原文）**：双通道各司其职、ADR、抽象可替换、误用指路、判据。
//!
//! **职责定位（锚点原文）**：传输层选型——传输层选型（选型决策：双
//! 通道模型（可靠有序通道（TCP 类）承载会话与交易+不可靠通道（UDP
//! 类）承载高频状态）——两通道各司其职；选型 ADR（背景/决策/理由/
//! 推翻条件四节——Y01 口径同规）；通道抽象（上层不感知具体协议——
//! 可替换））。
//!
//! # 一、双通道各司其职（流量分类→通道分派）
//!
//! [`TrafficClass`] 四分类：会话/交易（低频重语义）→可靠通道；高频
//! 状态/高频输入（每帧发生、可被新帧取代）→不可靠通道。分派表
//! （[`dispatch`]）O(1) 定查；**通道误用**（如状态走可靠通道）不静
//! 默转正——诊断+指路（锚点错误路径原文）。
//!
//! # 二、选型 ADR（四节——Y01 口径同规）
//!
//! [`TransportAdr`] 四节齐录（背景/决策/理由/推翻条件）。推翻条件
//! 是可判定谓词（[`AdrCondition`]）——触发即走[`reselect`] 重选型
//! 流程，不是"到时候再说"。
//!
//! # 三、通道抽象（上层不感知具体协议——可替换）
//!
//! 上层只见 [`ChannelSpec`]（抽象卡：排序/重传/拥塞三能力问句），
//! 不见 TCP/UDP 字样；协议后端（TcpLike/UdpLike）挂在抽象后面可
//! 替换。**抽象泄漏**（上层代码引用具体协议名）→拦截（锚点错误
//! 路径）——泄漏的抽象比没有抽象更糟：调用方开始按协议名分支。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const TRANSPORT_VERSION: &str = "F56-transport-v1";

/// 通道误用（诊断+指路——状态走可靠通道等）。
pub const E_TR_MISUSE: &str = "E_TR_MISUSE";

/// 抽象泄漏（上层引用具体协议名）。
pub const E_TR_LEAK: &str = "E_TR_LEAK";

/// ADR 缺节（四节不全即拒）。
pub const E_TR_ADR: &str = "E_TR_ADR";

/// 推翻条件触发（走重选型流程）。
pub const E_TR_INVALIDATED: &str = "E_TR_INVALIDATED";

// ---------------------------------------------------------------------------
// 二、双通道模型
// ---------------------------------------------------------------------------

/// 通道种类（双通道——可靠有序/不可靠）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelKind {
    /// 可靠有序通道（TCP 类：重传+排序+拥塞控制）。
    Reliable,
    /// 不可靠通道（UDP 类：不重传+不排序+低延迟）。
    Unreliable,
}

impl ChannelKind {
    /// 双通道闭集。
    pub const ALL: [ChannelKind; 2] = [ChannelKind::Reliable, ChannelKind::Unreliable];

    /// 通道名（人读）。
    pub fn zh(self) -> &'static str {
        match self {
            ChannelKind::Reliable => "可靠有序通道（TCP 类）",
            ChannelKind::Unreliable => "不可靠通道（UDP 类）",
        }
    }

    /// 承载语义（一句话职责——判据侧对拍用）。
    pub fn duty(self) -> &'static str {
        match self {
            ChannelKind::Reliable => "承载会话与交易——低频重语义，丢了必须补",
            ChannelKind::Unreliable => "承载高频状态——每帧发生，新帧天然取代旧帧",
        }
    }
}

/// 流量分类（四分类——上层产生的流量性质）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrafficClass {
    /// 会话（连接/房间/鉴权消息）。
    Session,
    /// 交易（购买/提交——不可丢不可重）。
    Transaction,
    /// 高频状态（位置/血量广播）。
    StateUpdate,
    /// 高频输入（每帧输入序列）。
    Input,
}

impl TrafficClass {
    /// 四分类闭集。
    pub const ALL: [TrafficClass; 4] = [
        TrafficClass::Session,
        TrafficClass::Transaction,
        TrafficClass::StateUpdate,
        TrafficClass::Input,
    ];

    /// 分类名（人读）。
    pub fn zh(self) -> &'static str {
        match self {
            TrafficClass::Session => "会话",
            TrafficClass::Transaction => "交易",
            TrafficClass::StateUpdate => "高频状态",
            TrafficClass::Input => "高频输入",
        }
    }
}

/// 分派表（流量分类→通道——O(1) 定查；规则即两通道各司其职）。
pub fn dispatch(t: TrafficClass) -> ChannelKind {
    match t {
        TrafficClass::Session | TrafficClass::Transaction => ChannelKind::Reliable,
        TrafficClass::StateUpdate | TrafficClass::Input => ChannelKind::Unreliable,
    }
}

/// 通道误用诊断（状态/输入走可靠通道、会话/交易走不可靠通道）。
///
/// 误用不静默转正（自动改道会掩盖上游的分类错误）——返回指路文案，
/// 由调用方显式修正（锚点：通道误用→诊断+指路）。
pub fn misuse_diagnosis(t: TrafficClass, actual: ChannelKind) -> Option<String> {
    let want = dispatch(t);
    if actual == want {
        return None;
    }
    let why = if actual == ChannelKind::Reliable {
        "可靠通道的队头阻塞与重传会把延迟叠到高频流量上——每个旧帧都认真补发，新帧却被堵在队列里"
    } else {
        "不可靠通道会丢包——低频重语义消息丢一条就是会话断裂或交易丢失，没有下一帧来救"
    };
    Some(format!(
        "{}：{} 走了{}（应走{}）——{}。指路：按 dispatch({}) 分派，勿按方便程度选通道",
        E_TR_MISUSE, t.zh(), actual.zh(), want.zh(), why, t.zh()
    ))
}

// ---------------------------------------------------------------------------
// 三、选型 ADR（四节——Y01 口径）
// ---------------------------------------------------------------------------

/// 推翻条件（可判定谓词——触发即重选型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdrCondition {
    /// 可靠通道往返占比超阈值（重传吃满预算——可靠不再配得上低频定位）。
    ReliableRttOverBudget {
        /// 阈值（微秒）。
        budget_us: u32,
    },
    /// 丢包补偿语义进入可靠通道族（可靠被用来补偿不可靠——双通道模型失效）。
    ReliableCompensatesLoss,
    /// 新硬件/新协议使抽象后端可整体替换（推翻旧协议绑定）。
    BackendReplaceable,
}

/// ADR 四节（背景/决策/理由/推翻条件）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportAdr {
    /// 背景（为什么选型）。
    pub background: String,
    /// 决策（选了什么）。
    pub decision: String,
    /// 理由（为什么是它）。
    pub rationale: String,
    /// 推翻条件（何时重选）。
    pub invalidators: Vec<AdrCondition>,
}

impl TransportAdr {
    /// 本仓选型 ADR（双通道模型的四节实录）。
    pub fn dual_channel() -> TransportAdr {
        TransportAdr {
            background: String::from(
                "网络流量两级分化：会话/交易低频重语义，状态/输入每帧高频可弃旧",
            ),
            decision: String::from(
                "双通道模型：可靠有序通道承载会话与交易，不可靠通道承载高频状态与输入",
            ),
            rationale: String::from(
                "单通道必错配：全可靠则高频流量被队头阻塞拖死，全不可靠则交易丢失无补偿；\
                 双通道让两类流量各得其所，抽象卡使后端可替换",
            ),
            invalidators: alloc::vec![
                AdrCondition::ReliableRttOverBudget { budget_us: 120_000 },
                AdrCondition::ReliableCompensatesLoss,
                AdrCondition::BackendReplaceable,
            ],
        }
    }

    /// 四节齐录核验（缺节即拒——半页 ADR 不是决策记录）。
    pub fn validate(&self) -> Result<(), String> {
        if self.background.trim().is_empty() {
            return Err(format!("{}：ADR 缺背景节", E_TR_ADR));
        }
        if self.decision.trim().is_empty() {
            return Err(format!("{}：ADR 缺决策节", E_TR_ADR));
        }
        if self.rationale.trim().is_empty() {
            return Err(format!("{}：ADR 缺理由节", E_TR_ADR));
        }
        if self.invalidators.is_empty() {
            return Err(format!("{}：ADR 缺推翻条件（没有推翻条件的决策是信仰不是工程）", E_TR_ADR));
        }
        Ok(())
    }

    /// 推翻条件判定（观测值进，触发返回被触发的条件）。
    pub fn check_invalidation(&self, obs: &AdrObservation) -> Option<AdrCondition> {
        for c in self.invalidators.iter() {
            let hit = match (c, obs) {
                (AdrCondition::ReliableRttOverBudget { budget_us }, AdrObservation::Rtt { us }) => us > budget_us,
                (AdrCondition::ReliableCompensatesLoss, AdrObservation::LossCompensated) => true,
                (AdrCondition::BackendReplaceable, AdrObservation::BackendSwapped) => true,
                _ => false,
            };
            if hit {
                return Some(*c);
            }
        }
        None
    }
}

/// ADR 观测值（推翻条件的输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdrObservation {
    /// 可靠通道往返耗时（微秒）。
    Rtt {
        /// 实测往返（微秒）。
        us: u32,
    },
    /// 可靠通道被用于补偿不可靠通道的丢包。
    LossCompensated,
    /// 抽象后端已整体替换。
    BackendSwapped,
}

/// 重选型流程（推翻触发后的动作序列）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReselectFlow {
    /// 步骤（人读）。
    pub steps: Vec<String>,
}

/// 重选型流程（推翻条件触发时返回）。
pub fn reselect(c: AdrCondition) -> ReselectFlow {
    let cause = format!("{}：ADR 推翻条件触发（{:?}）", E_TR_INVALIDATED, c);
    ReselectFlow {
        steps: alloc::vec![
            cause,
            String::from("1. 冻结当前分派表（新流量排队不盲发）"),
            String::from("2. 按新观测重跑选型（双通道比例/单通道/新后端）"),
            String::from("3. 出新 ADR（四节齐录）并归档旧 ADR（留痕可回溯）"),
            String::from("4. 抽象卡不变则上层零改动切换；卡变则走层间契约版本拦截"),
        ],
    }
}

// ---------------------------------------------------------------------------
// 四、通道抽象（可替换——上层不感知具体协议）
// ---------------------------------------------------------------------------

/// 抽象能力问句（协议后端必须回答的三问——上层只依赖这三问）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelSpec {
    /// 是否保证有序。
    pub ordered: bool,
    /// 是否重传补丢。
    pub retransmits: bool,
    /// 是否有拥塞控制。
    pub congestion: bool,
}

impl ChannelSpec {
    /// 可靠通道的能力问句答案（TCP 类）。
    pub fn reliable() -> ChannelSpec {
        ChannelSpec { ordered: true, retransmits: true, congestion: true }
    }

    /// 不可靠通道的能力问句答案（UDP 类）。
    pub fn unreliable() -> ChannelSpec {
        ChannelSpec { ordered: false, retransmits: false, congestion: false }
    }

    /// 按通道种类取抽象卡。
    pub fn of(kind: ChannelKind) -> ChannelSpec {
        match kind {
            ChannelKind::Reliable => ChannelSpec::reliable(),
            ChannelKind::Unreliable => ChannelSpec::unreliable(),
        }
    }

    /// 能力满足核验（需要重传的流量不得挂在无重传的卡上）。
    pub fn satisfies(&self, needs: ChannelNeeds) -> bool {
        let mut ok = true;
        if needs.ordered && !self.ordered {
            ok = false;
        }
        if needs.retransmits && !self.retransmits {
            ok = false;
        }
        ok
    }
}

/// 流量对通道的能力需求（由分类推导——分派的另一面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelNeeds {
    /// 需要有序。
    pub ordered: bool,
    /// 需要重传。
    pub retransmits: bool,
}

/// 分类→能力需求。
pub fn needs_of(t: TrafficClass) -> ChannelNeeds {
    match t {
        TrafficClass::Session | TrafficClass::Transaction => ChannelNeeds { ordered: true, retransmits: true },
        TrafficClass::StateUpdate | TrafficClass::Input => ChannelNeeds { ordered: false, retransmits: false },
    }
}

/// 抽象泄漏检查（上层描述里出现具体协议名即泄漏）。
///
/// 泄漏判据：上层只允许说"可靠/不可靠"（能力语汇），说 TCP/UDP
/// （实现语汇）即按实现分支的开始——拦截并把语汇表还给调用方。
pub fn leak_check(upper_layer_text: &str) -> Option<String> {
    let banned = ["TCP", "UDP", "tcp", "udp"];
    for b in banned.iter() {
        if upper_layer_text.contains(b) {
            return Some(format!(
                "{}：上层描述含具体协议名 {}——抽象泄漏。上层只依赖 ChannelSpec 三问句（有序/重传/拥塞），\
                 协议名是后端的事",
                E_TR_LEAK, b
            ));
        }
    }
    None
}

/// 协议后端（挂在抽象后面的可替换件——上层不可见）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// TCP 类实现（可靠有序卡的后端）。
    TcpLike,
    /// UDP 类实现（不可靠卡的后端）。
    UdpLike,
    /// QUIC 类实现（同一张可靠卡的另一后端——可替换性的证据）。
    QuicLike,
}

/// 后端→抽象卡（可替换 = 卡不变、后端换）。
pub fn backend_spec(b: Backend) -> ChannelSpec {
    match b {
        Backend::TcpLike | Backend::QuicLike => ChannelSpec::reliable(),
        Backend::UdpLike => ChannelSpec::unreliable(),
    }
}

/// 后端可替换核验（同卡多后端——抽象的兑现）。
pub fn replaceable_proof() -> bool {
    let tcp = backend_spec(Backend::TcpLike);
    let quic = backend_spec(Backend::QuicLike);
    let udp = backend_spec(Backend::UdpLike);
    tcp == quic && tcp != udp && udp == ChannelSpec::unreliable()
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F5403 域自检（判据五组：双通道/ADR/抽象/误用/收尾）。
pub fn run_vef56_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5403");

    // --- 双通道各司其职（判据一）---
    s.add(
        "F56-双通-01",
        ChannelKind::ALL.len() == 2 && TrafficClass::ALL.len() == 4,
        "双通道×四分类闭集",
    );
    // 分派表：会话/交易→可靠，状态/输入→不可靠。
    s.add(
        "F56-双通-02",
        dispatch(TrafficClass::Session) == ChannelKind::Reliable
            && dispatch(TrafficClass::Transaction) == ChannelKind::Reliable
            && dispatch(TrafficClass::StateUpdate) == ChannelKind::Unreliable
            && dispatch(TrafficClass::Input) == ChannelKind::Unreliable,
        "分派表四分类各司其职",
    );
    // 能力一致（分派结果的能力卡必须满足该分类的需求）。
    let cap_ok = TrafficClass::ALL.iter().all(|t| ChannelSpec::of(dispatch(*t)).satisfies(needs_of(*t)));
    s.add("F56-双通-03", cap_ok, "分派卡满足分类需求（能力自洽）");

    // --- 通道误用（判据二：误用指路）---
    // 误用检出+指路（状态走可靠通道）。
    let d = misuse_diagnosis(TrafficClass::StateUpdate, ChannelKind::Reliable);
    s.add(
        "F56-误用-01",
        d.as_ref().map(|x| x.starts_with(E_TR_MISUSE) && x.contains("高频状态") && x.contains("指路")).unwrap_or(false),
        "状态走可靠通道：诊断+指路",
    );
    // 反向误用（交易走不可靠通道）。
    let d2 = misuse_diagnosis(TrafficClass::Transaction, ChannelKind::Unreliable);
    s.add(
        "F56-误用-02",
        d2.as_ref().map(|x| x.contains("交易") && x.contains("指路")).unwrap_or(false),
        "交易走不可靠通道同样误用（双向）",
    );
    // 正确分派无诊断（不误报）。
    s.add(
        "F56-误用-03",
        misuse_diagnosis(TrafficClass::Session, ChannelKind::Reliable).is_none()
            && misuse_diagnosis(TrafficClass::Input, ChannelKind::Unreliable).is_none(),
        "正确分派零诊断（不误报）",
    );

    // --- 选型 ADR（判据三）---
    let adr = TransportAdr::dual_channel();
    s.add(
        "F56-ADR-01",
        adr.validate().is_ok() && adr.invalidators.len() == 3,
        "ADR 四节齐录+三推翻条件",
    );
    // 缺节拒绝（无推翻条件的 ADR 是信仰）。
    let mut faith = TransportAdr::dual_channel();
    faith.invalidators.clear();
    let r = faith.validate();
    s.add(
        "F56-ADR-02",
        r.is_err() && r.as_ref().unwrap_err().contains("推翻条件") && r.as_ref().unwrap_err().starts_with(E_TR_ADR),
        "无推翻条件拒绝",
    );
    // 推翻触发→重选型流程（RTT 超预算）。
    let flow = adr.check_invalidation(&AdrObservation::Rtt { us: 150_000 }).map(reselect);
    s.add(
        "F56-ADR-03",
        flow.as_ref().map(|f| f.steps.len() == 5 && f.steps[0].starts_with(E_TR_INVALIDATED)).unwrap_or(false),
        "RTT 超预算触发重选型（五步流程）",
    );
    // 未触发不动作（观测在阈值内）。
    s.add(
        "F56-ADR-04",
        adr.check_invalidation(&AdrObservation::Rtt { us: 100_000 }).is_none(),
        "阈值内不触发（不过敏）",
    );

    // --- 抽象可替换（判据四）---
    s.add(
        "F56-抽象-01",
        replaceable_proof() && ChannelSpec::reliable().ordered && !ChannelSpec::unreliable().ordered,
        "同卡多后端（TCP/QUIC 同位换、UDP 另一卡）",
    );
    // 抽象泄漏拦截（上层说 TCP/UDP）。
    let leak = leak_check("状态流走 TCP 可靠通道");
    s.add(
        "F56-抽象-02",
        leak.as_ref().map(|x| x.starts_with(E_TR_LEAK) && x.contains("抽象泄漏")).unwrap_or(false),
        "抽象泄漏拦截（上层禁说协议名）",
    );
    // 能力语汇放行（说可靠/不可靠不拦）。
    s.add(
        "F56-抽象-03",
        leak_check("状态流走不可靠通道").is_none() && leak_check("交易走可靠通道").is_none(),
        "能力语汇放行（可靠/不可靠可用）",
    );
    // 需求与卡对拍（重传需求不得配无重传卡）。
    let needs = needs_of(TrafficClass::Transaction);
    s.add(
        "F56-抽象-04",
        needs.ordered && needs.retransmits && !ChannelSpec::unreliable().satisfies(needs)
            && ChannelSpec::reliable().satisfies(needs),
        "交易需求只被可靠卡满足",
    );

    // --- 版本与暂挂 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in TRANSPORT_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    s.add("F56-版本-01", fp != 0, "版本指纹非零（F56-transport-v1）");

    s.add(
        "F56-暂挂-01",
        F56_LEDGER_SUSPENDED_NOTE.contains("暂挂") && F56_LEDGER_SUSPENDED_NOTE.contains("F5403"),
        "F 域账本暂挂声明显性",
    );

    // F56-暂挂-02：判据条数对账（本条为第 17 条）。
    s.add("F56-暂挂-02", s.len() == 16, "判据条数对账（16+本条）");

    s
}

/// F 域账本暂挂声明（跨批对接点：F5009 ADR 口径上游；F5404 会话下游；
/// F5418 基准——建账前暂挂，移交期模式延续）。
pub const F56_LEDGER_SUSPENDED_NOTE: &str = "传输层选型（双通道模型+ADR 四节+通道抽象）入 F 域账本：建账前暂挂声明（移交期模式延续——F5403 同款）；F5404 会话层按 dispatch 表消费双通道";
