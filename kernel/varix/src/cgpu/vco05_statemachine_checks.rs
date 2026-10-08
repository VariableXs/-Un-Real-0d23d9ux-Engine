//! CGPU-F2245 判据层：降级状态机（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2245`
//!
//! **锚点判据（全域状态机/形式化复用/迁移复用/三组/判据）→ 判据族**：
//! SM 4 / FM 4 / MG 3 / META 3 = 14 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 四态标签**判据侧独立写死逐字对拍**；合法迁移序列判据侧手算；不可
//! 达态检查判据侧独立 BFS 重算（与被测 formal_check 同算法但独立实现
//! 独立落笔，判据自证而非复读）；六条迁移条件句判据侧写死逐字对拍；
//! 码段判据 `!=` 防自判死（0x50..0x5A 全排除）。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vco05_statemachine::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的四态标签（官方序逐字）。
const EXP_STATES: [&str; 4] = ["正常", "降级中", "降级档", "恢复"];

/// 判据侧独立写死的六条迁移条件句（与 MIGRATIONS 同序逐字）。
const EXP_CONDITIONS: [&str; 6] = [
    "触发源融合事件激活（F2243 七源任一确认）",
    "降级决策下发完成（O02 决策引擎联动预留）",
    "短促扰动解除，未达降档门槛即回退",
    "触发条件解除持续确认（滞回判定通过）",
    "恢复完成全链回绿（各级动作复位到位）",
    "恢复途中再触发（重入降级，防振荡判定在前）",
];

/// 判据侧独立写死的复用声明句。
const EXP_FORMAL: &str = "状态机形式化复用 J03——迁移表全枚举+不可达态检查，形式化语义不另立";
const EXP_MIGRATION: &str =
    "迁移条件显性——每条迁移的触发条件字面量冻结，不隐式推断，迁移语义全域唯一";

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [ScCode; 5] = [
    ScCode::ILLEGAL_TRANSITION,
    ScCode::SAME_STATE,
    ScCode::STATE_TABLE_BROKEN,
    ScCode::UNREACHABLE_STATE,
    ScCode::HISTORY_BROKEN,
];

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 14;

// ---------------------------------------------------------------------------
// 主判据
// ---------------------------------------------------------------------------

/// vco05 域自检入口。
pub fn run_vco05_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-statemachine");

    // ================= 一、状态机（SM） =================

    // SM-1：四态闭集 + 标签逐字对拍（判据侧写死）。
    let mut sm1 = DegradeState::ALL.len() == STATE_COUNT && STATE_COUNT == 4;
    for i in 0..4 {
        if DegradeState::ALL[i].label() != EXP_STATES[i] {
            sm1 = false;
        }
    }
    s.add("O05-SM-四态闭集标签对拍", sm1, "");

    // SM-2：合法全链手算——正常→降级中→降级档→恢复→正常 每步可达且回环。
    let chain_ok = transition(DegradeState::Normal, DegradeState::Descending)
        == Ok(DegradeState::Descending)
        && transition(DegradeState::Descending, DegradeState::Degraded)
            == Ok(DegradeState::Degraded)
        && transition(DegradeState::Degraded, DegradeState::Recovering)
            == Ok(DegradeState::Recovering)
        && transition(DegradeState::Recovering, DegradeState::Normal) == Ok(DegradeState::Normal);
    s.add("O05-SM-合法全链手算", chain_ok, "");

    // SM-3：非法迁移逐类拒——跳段（正常→降级档）/倒退（降级档→降级中）/
    // 跳过恢复（降级档→正常）/自迁移。
    s.add(
        "O05-SM-非法迁移逐类拒",
        transition(DegradeState::Normal, DegradeState::Degraded)
            == Err(ScCode::ILLEGAL_TRANSITION)
            && transition(DegradeState::Degraded, DegradeState::Descending)
                == Err(ScCode::ILLEGAL_TRANSITION)
            && transition(DegradeState::Degraded, DegradeState::Normal)
                == Err(ScCode::ILLEGAL_TRANSITION)
            && transition(DegradeState::Normal, DegradeState::Normal) == Err(ScCode::SAME_STATE),
        "",
    );

    // SM-4：带迁移史合法链留痕完整 + 史断裂拒 + 空史起步态必须正常。
    let h0: Vec<DegradeState> = Vec::new();
    let bad_start = transition_with_history(&h0, DegradeState::Degraded, DegradeState::Recovering)
        == Err(ScCode::HISTORY_BROKEN);
    let sm4 = match transition_with_history(&h0, DegradeState::Normal, DegradeState::Descending) {
        Ok(h1) => {
            h1.len() == 1
                && h1[0] == DegradeState::Descending
                && match transition_with_history(&h1, DegradeState::Descending, DegradeState::Normal) {
                    Ok(h2) => h2.len() == 2 && h2[1] == DegradeState::Normal,
                    _ => false,
                }
                && bad_start
        }
        _ => false,
    };
    s.add("O05-SM-迁移史留痕与断裂拒", sm4, "");

    // ================= 二、形式化（FM） =================

    // FM-1：形式化复用声明逐字对拍 + 联动单号 1475。
    s.add(
        "O05-FM-形式化复用对拍",
        FORMAL_UPLINK == 1475 && FORMAL_REUSE_NOTE == EXP_FORMAL,
        "",
    );

    // FM-2：迁移表全枚举——恰 6 条、判据侧逐条 (from,to) 对拍（独立副本）。
    let exp_pairs: [(DegradeState, DegradeState); 6] = [
        (DegradeState::Normal, DegradeState::Descending),
        (DegradeState::Descending, DegradeState::Degraded),
        (DegradeState::Descending, DegradeState::Normal),
        (DegradeState::Degraded, DegradeState::Recovering),
        (DegradeState::Recovering, DegradeState::Normal),
        (DegradeState::Recovering, DegradeState::Descending),
    ];
    let mut fm2 = MIGRATIONS.len() == 6;
    for i in 0..6 {
        if MIGRATIONS[i].from != exp_pairs[i].0 || MIGRATIONS[i].to != exp_pairs[i].1 {
            fm2 = false;
        }
    }
    s.add("O05-FM-迁移表全枚举对拍", fm2, "");

    // FM-3：不可达态检查——判据侧独立 BFS 重算（四态全达）。
    let mut reach = [false; 4];
    reach[0] = true; // Normal 起点（判据侧按官方序独立落笔）
    let mut grew = true;
    while grew {
        grew = false;
        for m in MIGRATIONS.iter() {
            let f = m.from as usize;
            let t = m.to as usize;
            if reach[f] && !reach[t] {
                reach[t] = true;
                grew = true;
            }
        }
    }
    let all_reach = reach[0] && reach[1] && reach[2] && reach[3];
    s.add("O05-FM-不可达态独立重算", all_reach && formal_check() == Ok(()), "");

    // FM-4：形式化三查自证——恰 6 条+无自迁移+无重复对（被测守卫语义）。
    s.add("O05-FM-形式化三查通过", formal_check() == Ok(()), "");

    // ================= 三、迁移复用（MG） =================

    // MG-1：六条迁移条件句判据侧写死逐字对拍（独立副本——迁移条件显性）。
    let mut mg1 = MIGRATION_REUSE_NOTE == EXP_MIGRATION;
    for i in 0..6 {
        if MIGRATIONS[i].condition != EXP_CONDITIONS[i] {
            mg1 = false;
        }
    }
    s.add("O05-MG-迁移条件逐字对拍", mg1, "");

    // MG-2：重入迁移显性——恢复途中再触发回降级中（合法），防振荡语义在条件句内显性。
    s.add(
        "O05-MG-重入迁移显性",
        transition(DegradeState::Recovering, DegradeState::Descending)
            == Ok(DegradeState::Descending)
            && MIGRATIONS[5].condition.contains("重入"),
        "",
    );

    // MG-3：与 vco01 五段流水线分工声明非空且语义双面（段/态各一义）。
    s.add(
        "O05-MG-段态分工声明",
        STAGE_LINK_NOTE.contains("段=管线步进") && STAGE_LINK_NOTE.contains("态=生命周期状态"),
        "",
    );

    // ================= 四、判据自检（META） =================

    // META-3：码段独占——全部 0x5Bxx，且 != 0x50..0x5A（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x5B)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x50 && hi != 0x51 && hi != 0x52 && hi != 0x53
                && hi != 0x54 && hi != 0x55 && hi != 0x56 && hi != 0x57
                && hi != 0x58 && hi != 0x59 && hi != 0x5A
        });
    s.add("O05-META-诊断码段独占", section_ok, "");

    // META-2：码两两互异 + 人话原因非空 + 判据容量无截断。
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
    s.add("O05-META-码互异与无截断", code_ok && !s.truncated(), "");

    // META-1：判据条数对账（放末位：此时 len 应为 12，加自身恰 13）。
    s.add("O05-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
