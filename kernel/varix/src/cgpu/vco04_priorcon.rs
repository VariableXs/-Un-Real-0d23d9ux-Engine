//! CGPU-F2244 · 降级优先级与冲突（CGPU-O 域 · 降级链 · 优先级主题）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2244`
//!
//! 优先级冲突：优先级（降级优先级（安全>合同>体验>资源——优先级表
//! （优先级复用 F1496——优先级复用；冲突（维度间冲突（帧率 vs 画质
//! ——冲突消解（消解规则——规则显性；测试（优先级/消解两组）。
//!
//! ## 要点一：降级优先级四级序闭集
//!
//! 安全>合同>体验>资源——全域降级优先级封闭枚举，表外不立层级；
//! rank 越小优先级越高（安全=0 最高，资源=3 最低）；每层一条中文
//! 标签（字面量冻结，判据独立对拍）。
//!
//! ## 要点二：优先级复用 F1496
//!
//! 降级优先级复用 F1496 四层优先级模式——同构四级序、取高不取低、
//! 平局不掷骰子；降级域具体层级映射为 安全>合同>体验>资源（F1496
//! 原表为 安全>合同>用户显式>自动策略——对照表字面量冻结）。
//!
//! ## 要点三：维度间冲突与消解规则显性
//!
//! 冲突维度四闭集：帧率/画质/延迟/功耗；每维归属一个优先层级（帧率
//! →合同、画质→体验、延迟→体验、功耗→资源）；两两 C(4,2)=6 对冲
//! 突全部显性消解规则——高层级维度胜出，同层平局走显性平局规则
//! （交互响应优先：延迟胜出），每条规则带编号与人话理由（规则显性，
//! 判据逐字对拍）。
//!
//! ## 要点四：零 panic 面 + 诊断码独占 0x5Axx 段
//!
//! 与 vco03（0x59xx）/vcq02（0x58xx）/cgr01（0x57xx）/vcq01（0x56xx）/
//! vco01（0x55xx）等互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 一、降级优先级四级序闭集
// ---------------------------------------------------------------------------

/// 降级优先级层级（官方四级闭集：安全>合同>体验>资源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityTier {
    /// 安全——红线层，高于一切（不参与维度间消解，安全冲突直接安全胜）。
    Safety,
    /// 合同——帧合同等对外承诺。
    Contract,
    /// 体验——画质/延迟等用户体感。
    Experience,
    /// 资源——功耗/带宽等成本项。
    Resource,
}

/// 层级总数。
pub const PRIORITY_TIER_COUNT: usize = 4;

impl PriorityTier {
    /// 全部层级（官方序，下标即优先级次序）。
    pub const ALL: [PriorityTier; PRIORITY_TIER_COUNT] = [
        PriorityTier::Safety,
        PriorityTier::Contract,
        PriorityTier::Experience,
        PriorityTier::Resource,
    ];

    /// 中文标签（字面量冻结——判据独立对拍）。
    pub fn label(self) -> String {
        match self {
            PriorityTier::Safety => "安全".to_string(),
            PriorityTier::Contract => "合同".to_string(),
            PriorityTier::Experience => "体验".to_string(),
            PriorityTier::Resource => "资源".to_string(),
        }
    }

    /// 优先级秩（0 最高，3 最低——判据侧独立重算严格递增）。
    pub fn rank(self) -> u8 {
        match self {
            PriorityTier::Safety => 0,
            PriorityTier::Contract => 1,
            PriorityTier::Experience => 2,
            PriorityTier::Resource => 3,
        }
    }
}

/// 优先级表（锚点「安全>合同>体验>资源——优先级表」逐字）。
pub const PRIORITY_LAYERS: [&str; 4] = ["安全", "合同", "体验", "资源"];

/// 安全红线声明（安全层是前置闸不是消解参与方）。
pub const REDLINE_NOTE: &str =
    "安全层为红线前置闸——不参与维度间消解，涉安全冲突直接安全胜，降级不得越安全线";

// ---------------------------------------------------------------------------
// 二、优先级复用 F1496
// ---------------------------------------------------------------------------

/// 优先级复用联动单号（锚点「优先级复用 F1496」——判据独立对拍）。
pub const PRIORITY_UPLINK: u32 = 1496;
/// 复用声明（判据逐字对拍）。
pub const PRIORITY_REUSE_NOTE: &str =
    "降级优先级复用 F1496 四层优先级模式——同构四级序取高不取低，降级域层级为 安全>合同>体验>资源";
/// F1496 原表四层（对照字面量冻结——判据独立对拍）。
pub const F1496_LAYERS: [&str; 4] = ["安全", "合同", "用户显式", "自动策略"];
/// 对照声明。
pub const F1496_ALIGN_NOTE: &str =
    "F1496 原表对照——同构四级序，降级域将 用户显式/自动策略 两层收编为 体验/资源";

// ---------------------------------------------------------------------------
// 三、维度间冲突与消解规则显性
// ---------------------------------------------------------------------------

/// 冲突维度（官方四闭集：帧率/画质/延迟/功耗）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictDim {
    /// 帧率——归属合同层（帧合同是承诺）。
    Fps,
    /// 画质——归属体验层。
    Quality,
    /// 延迟——归属体验层。
    Latency,
    /// 功耗——归属资源层。
    Power,
}

/// 维度总数。
pub const DIM_COUNT: usize = 4;

impl ConflictDim {
    /// 全部维度（官方序）。
    pub const ALL: [ConflictDim; DIM_COUNT] = [
        ConflictDim::Fps,
        ConflictDim::Quality,
        ConflictDim::Latency,
        ConflictDim::Power,
    ];

    /// 中文标签（字面量冻结——判据独立对拍）。
    pub fn label(self) -> String {
        match self {
            ConflictDim::Fps => "帧率".to_string(),
            ConflictDim::Quality => "画质".to_string(),
            ConflictDim::Latency => "延迟".to_string(),
            ConflictDim::Power => "功耗".to_string(),
        }
    }
}

/// 维度→优先级层级映射（显性——判据手算对拍）。
pub fn dim_tier(d: ConflictDim) -> PriorityTier {
    match d {
        ConflictDim::Fps => PriorityTier::Contract,
        ConflictDim::Quality => PriorityTier::Experience,
        ConflictDim::Latency => PriorityTier::Experience,
        ConflictDim::Power => PriorityTier::Resource,
    }
}

/// 一条显性消解规则（编号+胜者+人话理由——规则显性）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConflictRule {
    /// 冲突方 A。
    pub a: ConflictDim,
    /// 冲突方 B。
    pub b: ConflictDim,
    /// 胜出方（高层级维度；同层平局按显性平局规则）。
    pub winner: ConflictDim,
    /// 规则编号（R1..R6 逐字）。
    pub rule_id: &'static str,
    /// 人话理由（判据逐字对拍）。
    pub note: &'static str,
}

/// 六对显性消解规则（C(4,2)=6 全覆盖——表驱动，不硬编码 if 链）。
pub const CONFLICT_RULES: [ConflictRule; 6] = [
    ConflictRule {
        a: ConflictDim::Fps,
        b: ConflictDim::Quality,
        winner: ConflictDim::Fps,
        rule_id: "R1",
        note: "合同承诺的帧率高于体验层画质诉求（合同>体验）",
    },
    ConflictRule {
        a: ConflictDim::Fps,
        b: ConflictDim::Latency,
        winner: ConflictDim::Fps,
        rule_id: "R2",
        note: "合同承诺的帧率高于体验层延迟诉求（合同>体验）",
    },
    ConflictRule {
        a: ConflictDim::Fps,
        b: ConflictDim::Power,
        winner: ConflictDim::Fps,
        rule_id: "R3",
        note: "合同承诺的帧率高于资源层功耗诉求（合同>资源）",
    },
    ConflictRule {
        a: ConflictDim::Quality,
        b: ConflictDim::Latency,
        winner: ConflictDim::Latency,
        rule_id: "R4",
        note: "同层平局显性规则：交互响应优先——延迟胜出（体验层内平局不掷骰子）",
    },
    ConflictRule {
        a: ConflictDim::Quality,
        b: ConflictDim::Power,
        winner: ConflictDim::Quality,
        rule_id: "R5",
        note: "体验层画质高于资源层功耗诉求（体验>资源）",
    },
    ConflictRule {
        a: ConflictDim::Latency,
        b: ConflictDim::Power,
        winner: ConflictDim::Latency,
        rule_id: "R6",
        note: "体验层延迟高于资源层功耗诉求（体验>资源）",
    },
];

/// 消解裁决（胜者+命中规则编号+理由——可解释）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConflictRuling {
    /// 胜出维度。
    pub winner: ConflictDim,
    /// 命中规则编号。
    pub rule_id: &'static str,
    /// 人话理由。
    pub note: &'static str,
}

/// 维度间冲突消解：查显性规则表裁决。
///
/// 规则：① 同维冲突拒绝（消解需要两个不同维度）；② 命中规则表→
/// 裁决（胜者+编号+理由）；③ 裁决自检——胜者层级秩不得劣于败者
/// （平局规则允许相等）；④ 表外冲突对拒绝（防御位——闭集两两全覆
/// 盖时不应触达）。
pub fn resolve(a: ConflictDim, b: ConflictDim) -> Result<ConflictRuling, PcCode> {
    if a == b {
        return Err(PcCode::SAME_DIMENSION);
    }
    for r in CONFLICT_RULES.iter() {
        let hit = (r.a == a && r.b == b) || (r.a == b && r.b == a);
        if hit {
            let loser = if r.winner == r.a { r.b } else { r.a };
            if dim_tier(r.winner).rank() > dim_tier(loser).rank() {
                return Err(PcCode::RULING_UNVERIFIABLE);
            }
            return Ok(ConflictRuling { winner: r.winner, rule_id: r.rule_id, note: r.note });
        }
    }
    Err(PcCode::UNKNOWN_PAIR)
}

// ---------------------------------------------------------------------------
// 四、完整性守卫（表驱动结构的自检——防表被改坏后静默错裁）
// ---------------------------------------------------------------------------

/// 优先级表完整性：层级数恰 4 且官方序 rank 严格递增。
pub fn priority_integrity() -> Result<(), PcCode> {
    if PRIORITY_LAYERS.len() != PRIORITY_TIER_COUNT {
        return Err(PcCode::TIER_TABLE_BROKEN);
    }
    let mut i = 0;
    while i + 1 < PRIORITY_TIER_COUNT {
        if PriorityTier::ALL[i].rank() >= PriorityTier::ALL[i + 1].rank() {
            return Err(PcCode::TIER_TABLE_BROKEN);
        }
        i += 1;
    }
    Ok(())
}

/// 消解规则表完整性：恰 6 条、每条 a!=b 且 winner∈{a,b}、两两对不重复。
pub fn rules_integrity() -> Result<(), PcCode> {
    if CONFLICT_RULES.len() != 6 {
        return Err(PcCode::RULE_TABLE_BROKEN);
    }
    for r in CONFLICT_RULES.iter() {
        if r.a == r.b || (r.winner != r.a && r.winner != r.b) {
            return Err(PcCode::RULE_TABLE_BROKEN);
        }
    }
    let mut i = 0;
    while i < CONFLICT_RULES.len() {
        let mut j = i + 1;
        while j < CONFLICT_RULES.len() {
            let x = &CONFLICT_RULES[i];
            let y = &CONFLICT_RULES[j];
            let same = (x.a == y.a && x.b == y.b) || (x.a == y.b && x.b == y.a);
            if same {
                return Err(PcCode::RULE_TABLE_BROKEN);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、错误契约（独占 0x5Axx 段）
// ---------------------------------------------------------------------------

/// vco04 诊断码。独占 `0x5Axx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcCode(pub u16);

impl PcCode {
    /// 同维冲突。
    pub const SAME_DIMENSION: PcCode = PcCode(0x5A01);
    /// 表外冲突对（防御位）。
    pub const UNKNOWN_PAIR: PcCode = PcCode(0x5A02);
    /// 优先级表完整性破坏。
    pub const TIER_TABLE_BROKEN: PcCode = PcCode(0x5A03);
    /// 消解规则表完整性破坏。
    pub const RULE_TABLE_BROKEN: PcCode = PcCode(0x5A04);
    /// 裁决与优先级序矛盾（防御自检位）。
    pub const RULING_UNVERIFIABLE: PcCode = PcCode(0x5A05);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            PcCode::SAME_DIMENSION => "同维冲突：消解需要两个不同维度".into(),
            PcCode::UNKNOWN_PAIR => "表外冲突对：闭集两两规则全覆盖，此码为防御位".into(),
            PcCode::TIER_TABLE_BROKEN => "优先级表完整性破坏：层级数或单调序不符".into(),
            PcCode::RULE_TABLE_BROKEN => "消解规则表完整性破坏：规则数或对覆盖不符".into(),
            PcCode::RULING_UNVERIFIABLE => "裁决与优先级序矛盾：胜者层级秩不得劣于败者".into(),
            PcCode(_) => "未知 vco04 优先级与冲突域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 六、测试支撑（锚点两组：优先级/消解）
// ---------------------------------------------------------------------------

#[cfg(all(test, not(no_std)))]
mod tests {
    use super::*;

    #[test]
    fn 四级序与f1496对照() {
        assert_eq!(PRIORITY_LAYERS, ["安全", "合同", "体验", "资源"]);
        for i in 0..PRIORITY_TIER_COUNT {
            assert_eq!(PriorityTier::ALL[i].label(), PRIORITY_LAYERS[i]);
            assert_eq!(PriorityTier::ALL[i].rank(), i as u8);
        }
        assert_eq!(F1496_LAYERS, ["安全", "合同", "用户显式", "自动策略"]);
        assert_eq!(priority_integrity(), Ok(()));
    }

    #[test]
    fn 六对规则全覆盖消解() {
        assert_eq!(rules_integrity(), Ok(()));
        // 锚点典型例：帧率 vs 画质 → 帧率胜（合同>体验）
        let r = resolve(ConflictDim::Quality, ConflictDim::Fps).unwrap();
        assert_eq!(r.winner, ConflictDim::Fps);
        assert_eq!(r.rule_id, "R1");
        // 同层平局：画质 vs 延迟 → 延迟胜（显性平局规则）
        let r2 = resolve(ConflictDim::Quality, ConflictDim::Latency).unwrap();
        assert_eq!(r2.winner, ConflictDim::Latency);
        assert_eq!(r2.rule_id, "R4");
        // 同维拒
        assert_eq!(resolve(ConflictDim::Fps, ConflictDim::Fps), Err(PcCode::SAME_DIMENSION));
    }
}
