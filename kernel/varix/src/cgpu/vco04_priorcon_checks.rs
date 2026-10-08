//! CGPU-F2244 判据层：降级优先级与冲突（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2244`
//!
//! **锚点判据（优先级复用/消解规则/两组/判据）→ 判据族**：
//! PRI 4 / DIM 2 / RES 4 / META 4 = 14 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 四级序与 F1496 对照表**判据侧独立写死逐字对拍**；rank 单调性判据
//! 侧重算；六对消解规则判据侧手算对拍（锚点典型例 帧率 vs 画质→帧率
//! 胜 显性落判据）；同维拒专属码；码段判据 `!=` 防自判死
//! （0x50..0x59 全排除）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vco04_priorcon::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的四级序（官方序逐字）。
const EXP_LAYERS: [&str; 4] = ["安全", "合同", "体验", "资源"];

/// 判据侧独立写死的 F1496 原表对照（逐字）。
const EXP_F1496: [&str; 4] = ["安全", "合同", "用户显式", "自动策略"];

/// 判据侧独立写死的四维标签（官方序逐字）。
const EXP_DIMS: [&str; 4] = ["帧率", "画质", "延迟", "功耗"];

/// 判据侧独立写死的复用声明句。
const EXP_REUSE: &str =
    "降级优先级复用 F1496 四层优先级模式——同构四级序取高不取低，降级域层级为 安全>合同>体验>资源";

/// 判据侧独立写死的六对期望规则（id, a, b, winner）——独立契约防同源恒绿。
const EXP_RULES: [(&str, ConflictDim, ConflictDim, ConflictDim); 6] = [
    ("R1", ConflictDim::Fps, ConflictDim::Quality, ConflictDim::Fps),
    ("R2", ConflictDim::Fps, ConflictDim::Latency, ConflictDim::Fps),
    ("R3", ConflictDim::Fps, ConflictDim::Power, ConflictDim::Fps),
    ("R4", ConflictDim::Quality, ConflictDim::Latency, ConflictDim::Latency),
    ("R5", ConflictDim::Quality, ConflictDim::Power, ConflictDim::Quality),
    ("R6", ConflictDim::Latency, ConflictDim::Power, ConflictDim::Latency),
];

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [PcCode; 5] = [
    PcCode::SAME_DIMENSION,
    PcCode::UNKNOWN_PAIR,
    PcCode::TIER_TABLE_BROKEN,
    PcCode::RULE_TABLE_BROKEN,
    PcCode::RULING_UNVERIFIABLE,
];

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 14;

// ---------------------------------------------------------------------------
// 主判据
// ---------------------------------------------------------------------------

/// vco04 域自检入口。
pub fn run_vco04_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-priorcon");

    // ================= 一、优先级（PRI） =================

    // PRI-1：四级序闭集 + 标签逐字对拍（判据侧写死）。
    let mut pri1 = PriorityTier::ALL.len() == PRIORITY_TIER_COUNT && PRIORITY_TIER_COUNT == 4;
    for i in 0..4 {
        if PriorityTier::ALL[i].label() != EXP_LAYERS[i] || PRIORITY_LAYERS[i] != EXP_LAYERS[i] {
            pri1 = false;
        }
    }
    s.add("O04-PRI-四级序表逐字对拍", pri1, "");

    // PRI-2：rank 严格递增（判据侧独立重算：0,1,2,3——安全最高资源最低）。
    let mut pri2 = true;
    for i in 0..4 {
        if PriorityTier::ALL[i].rank() != i as u8 {
            pri2 = false;
        }
    }
    pri2 = pri2
        && PriorityTier::Safety.rank() < PriorityTier::Contract.rank()
        && PriorityTier::Contract.rank() < PriorityTier::Experience.rank()
        && PriorityTier::Experience.rank() < PriorityTier::Resource.rank();
    s.add("O04-PRI-rank单调重算", pri2, "");

    // PRI-3：F1496 对照表逐字对拍（判据侧写死）+ 联动单号。
    let mut pri3 = PRIORITY_UPLINK == 1496;
    for i in 0..4 {
        if F1496_LAYERS[i] != EXP_F1496[i] {
            pri3 = false;
        }
    }
    s.add("O04-PRI-F1496对照逐字对拍", pri3, "");

    // PRI-4：复用声明逐字对拍 + 安全红线声明非空且含「安全」。
    s.add(
        "O04-PRI-复用与红线声明对拍",
        PRIORITY_REUSE_NOTE == EXP_REUSE
            && REDLINE_NOTE.contains("安全")
            && REDLINE_NOTE.contains("不得越安全线"),
        "",
    );

    // ================= 二、维度闭集（DIM） =================

    // DIM-1：四维闭集 + 标签逐字对拍（判据侧写死）。
    let mut dim1 = ConflictDim::ALL.len() == DIM_COUNT && DIM_COUNT == 4;
    for i in 0..4 {
        if ConflictDim::ALL[i].label() != EXP_DIMS[i] {
            dim1 = false;
        }
    }
    s.add("O04-DIM-四维闭集标签对拍", dim1, "");

    // DIM-2：维度→层级映射判据侧手算对拍（帧率→合同/画质→体验/延迟→体验/功耗→资源）。
    let dim2 = dim_tier(ConflictDim::Fps) == PriorityTier::Contract
        && dim_tier(ConflictDim::Quality) == PriorityTier::Experience
        && dim_tier(ConflictDim::Latency) == PriorityTier::Experience
        && dim_tier(ConflictDim::Power) == PriorityTier::Resource;
    s.add("O04-DIM-维度层级映射手算", dim2, "");

    // ================= 三、消解规则（RES） =================

    // RES-1：规则表完整性守卫通过（恰 6 条=C(4,2)、a!=b、winner∈{a,b}、对不重复）。
    s.add("O04-RES-规则表完整性守卫", rules_integrity() == Ok(()), "");

    // RES-2：锚点典型例手算——帧率 vs 画质 → 帧率胜（R1）；反向查询同一裁决
    // （消解与查询序无关——确定性）。
    let fwd = resolve(ConflictDim::Fps, ConflictDim::Quality);
    let rev = resolve(ConflictDim::Quality, ConflictDim::Fps);
    let res2 = match (fwd, rev) {
        (Ok(x), Ok(y)) => {
            x.winner == ConflictDim::Fps
                && y.winner == ConflictDim::Fps
                && x.rule_id == "R1"
                && y.rule_id == "R1"
                && x.note.contains("合同")
        }
        _ => false,
    };
    s.add("O04-RES-典型例双向裁决手算", res2, "");

    // RES-3：六对全覆盖判据侧独立对拍（EXP_RULES 独立契约防同源恒绿）+
    // resolve 查表自洽 + 平局规则显性（R4 胜者与败者同层——体验==体验）。
    let mut res3 = true;
    for (id, ea, eb, ew) in EXP_RULES.iter() {
        let mut found = false;
        for r in CONFLICT_RULES.iter() {
            if r.rule_id == *id {
                found = true;
                if r.a != *ea || r.b != *eb || r.winner != *ew {
                    res3 = false;
                }
            }
        }
        if !found {
            res3 = false;
        }
    }
    for r in CONFLICT_RULES.iter() {
        match resolve(r.a, r.b) {
            Ok(got) => {
                if got.winner != r.winner || got.rule_id != r.rule_id {
                    res3 = false;
                }
            }
            _ => res3 = false,
        }
        // 平局规则显性：R4 胜者与败者同层（体验==体验）——秩相等允许。
        if r.rule_id == "R4"
            && (dim_tier(r.winner) != PriorityTier::Experience || dim_tier(r.a) != dim_tier(r.b))
        {
            res3 = false;
        }
    }
    s.add("O04-RES-六对全覆盖与平局显性", res3, "");

    // RES-4：逐码拒——同维冲突专属码；表驱动守卫破坏可检出
    // （层级数哨兵：PRIORITY_LAYERS 长度与秩单调性联合成立才过）。
    let same_dim = resolve(ConflictDim::Fps, ConflictDim::Fps) == Err(PcCode::SAME_DIMENSION);
    let guard = priority_integrity() == Ok(());
    s.add("O04-RES-同维拒与优先级守卫", same_dim && guard, "");

    // ================= 四、判据自检（META） =================

    // META-2：判据容量无截断。
    s.add("O04-META-判据容量无截断", !s.truncated(), "");

    // META-3：码段独占——全部 0x5Axx，且 != 0x50..0x59（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x5A)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x50 && hi != 0x51 && hi != 0x52 && hi != 0x53
                && hi != 0x54 && hi != 0x55 && hi != 0x56 && hi != 0x57
                && hi != 0x58 && hi != 0x59
        });
    s.add("O04-META-诊断码段独占", section_ok, "");

    // META-4：码两两互异 + 人话原因非空。
    let mut code_ok = true;
    for i in 0..ALL_CODES.len() {
        for j in 0..ALL_CODES.len() {
            if i != j && ALL_CODES[i].code() == ALL_CODES[j].code() {
                code_ok = false;
            }
        }
    }
    for c in ALL_CODES {
        if c.reason().is_empty() {
            code_ok = false;
        }
    }
    s.add("O04-META-码互异原因非空", code_ok, "");

    // META-1：判据条数对账（放末位：此时 len 应为 13，加自身恰 14）。
    s.add("O04-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
