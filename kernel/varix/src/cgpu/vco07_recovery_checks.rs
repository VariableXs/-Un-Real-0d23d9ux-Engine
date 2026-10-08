//! CGPU-F2247 判据层：降级恢复引擎（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2247`
//!
//! **锚点判据（恢复复用/一组/判据）→ 判据族**：
//! RC 3 / LIM 3 / OSC 3 / META 3 = 12 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 恢复序表与复用声明**判据侧独立写死逐字对拍**；限速窗口与滞回带
//! 常量判据侧写死；限速/滞回/触发三闸判据侧手算（恰边界）；过冲回退
//! 与振荡冻结判据侧按 step 序手算；恢复账对账判据侧构造正反语料；
//! 码段判据 `!=` 防自判死（0x50..0x5C 全排除）。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vco07_recovery::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的恢复序标签（官方序逐字）。
const EXP_ORDER: [&str; 4] = ["资源动作解除", "效果动作解除", "呈现动作解除", "调度动作解除"];

/// 判据侧独立写死的复用声明句。
const EXP_REUSE: &str = "恢复复用 J03 家族——两维序/过冲记忆/振荡判据模式复用，恢复语义不另立";

/// 判据侧独立写死的常量（限速窗口/滞回带/振荡限）。
const EXP_INTERVAL: u64 = 500;
const EXP_HYSTERESIS: i32 = 3;
const EXP_OSC_LIMIT: u8 = 3;

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [RcCode; 5] = [
    RcCode::RATE_LIMITED,
    RcCode::TRIGGER_ACTIVE,
    RcCode::OSCILLATION_FROZEN,
    RcCode::ALL_RECOVERED,
    RcCode::ORDER_TABLE_BROKEN,
];

/// 预期判据条数（写前先数实挂 s.add 个数）。
const EXPECTED_CHECKS: usize = 12;

// ---------------------------------------------------------------------------
// 主判据
// ---------------------------------------------------------------------------

/// vco07 域自检入口。
pub fn run_vco07_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-recovery");

    // ================= 一、恢复复用（RC） =================

    // RC-1：恢复序四级闭集标签逐字对拍（判据侧写死）。
    let mut rc1 = RecoveryStep::ALL.len() == RECOVERY_LEVELS && RECOVERY_LEVELS == 4;
    for i in 0..4 {
        if RecoveryStep::ALL[i].label() != EXP_ORDER[i] {
            rc1 = false;
        }
    }
    s.add("O07-RC-恢复序表逐字对拍", rc1, "");

    // RC-2：恢复复用声明逐字对拍 + 联动单号 1481。
    s.add(
        "O07-RC-恢复复用对拍",
        RECOVERY_UPLINK == 1481 && RECOVERY_REUSE_NOTE == EXP_REUSE,
        "",
    );

    // RC-3：常量字面量对拍——限速 500/滞回带 3/振荡限 3（判据侧写死）。
    s.add(
        "O07-RC-常量字面量对拍",
        RECOVERY_INTERVAL_TICKS == EXP_INTERVAL
            && HYSTERESIS_BAND == EXP_HYSTERESIS
            && OSCILLATION_LIMIT == EXP_OSC_LIMIT,
        "",
    );

    // ================= 二、限速与滞回（LIM） =================

    // LIM-1：三闸手算——限速（同 tick 二连推拒/窗口不足拒）、触发暂停、
    // 全恢复 None；恰边界 1250=700+500、1750=1250+500 手算对账。
    let mut e = new_engine();
    let l1 = step(&mut e, 100, false, 5) == Ok(Some(RecoveryStep::ResourceRelease))
        && step(&mut e, 100, false, 5) == Err(RcCode::RATE_LIMITED)
        && step(&mut e, 700, false, 5) == Ok(Some(RecoveryStep::EffectsRelease))
        && step(&mut e, 800, false, 5) == Err(RcCode::RATE_LIMITED)
        && step(&mut e, 900, true, 5) == Err(RcCode::TRIGGER_ACTIVE)
        && step(&mut e, 1250, false, 5) == Ok(Some(RecoveryStep::PresentationRelease))
        && step(&mut e, 1750, false, 5) == Ok(Some(RecoveryStep::SchedulingRelease))
        && step(&mut e, 2750, false, 5) == Ok(None);
    s.add("O07-LIM-三闸全链手算", l1, "");

    // LIM-2：滞回恰边界——余量恰等于滞回带（3）可推进，低于（2）拒绝。
    let mut e2 = new_engine();
    let l2 = step(&mut e2, 0, false, HYSTERESIS_BAND) == Ok(Some(RecoveryStep::ResourceRelease))
        && step(&mut e2, 1000, false, HYSTERESIS_BAND - 1) == Err(RcCode::RATE_LIMITED)
        && step(&mut e2, 2000, false, HYSTERESIS_BAND) == Ok(Some(RecoveryStep::EffectsRelease));
    s.add("O07-LIM-滞回恰边界", l2, "");

    // LIM-3：恢复账对账——正语料过；反语料（间隔不足/余量不足）逐码拒。
    let good = [
        RecoveryEntry { step: RecoveryStep::ResourceRelease, tick: 100, margin: 5, attempt: 0 },
        RecoveryEntry { step: RecoveryStep::EffectsRelease, tick: 700, margin: 4, attempt: 0 },
    ];
    let bad_gap = [
        RecoveryEntry { step: RecoveryStep::ResourceRelease, tick: 100, margin: 5, attempt: 0 },
        RecoveryEntry { step: RecoveryStep::EffectsRelease, tick: 300, margin: 5, attempt: 0 },
    ];
    let bad_margin = [
        RecoveryEntry { step: RecoveryStep::ResourceRelease, tick: 100, margin: 1, attempt: 0 },
    ];
    let l3 = audit_entries(&good) == Ok(())
        && audit_entries(&bad_gap) == Err(RcCode::RATE_LIMITED)
        && audit_entries(&bad_margin) == Err(RcCode::RATE_LIMITED);
    s.add("O07-LIM-恢复账正反语料", l3, "");

    // ================= 三、振荡判据（OSC） =================

    // OSC-1：过冲记忆手算——恢复途中再触发回退一级并记账。
    let mut e3 = new_engine();
    let _ = step(&mut e3, 100, false, 5);
    let _ = step(&mut e3, 700, false, 5);
    on_retrigger(&mut e3);
    let o1 = e3.level == 1 && e3.overshoot == 1
        && step(&mut e3, 1300, false, 5) == Ok(Some(RecoveryStep::EffectsRelease));
    s.add("O07-OSC-过冲回退手算", o1, "");

    // OSC-2：振荡冻结——再触发达限 frozen=true，冻结后任何推进拒绝。
    let mut e4 = new_engine();
    on_retrigger(&mut e4);
    on_retrigger(&mut e4);
    on_retrigger(&mut e4);
    let o2 = e4.oscillation == 3 && e4.frozen
        && step(&mut e4, 9999, false, 9) == Err(RcCode::OSCILLATION_FROZEN);
    s.add("O07-OSC-振荡冻结手算", o2, "");

    // OSC-3：引擎与观测账一致性——账=引擎前缀过；级动作错位拒。
    let mut e5 = new_engine();
    let _ = step(&mut e5, 100, false, 5);
    let _ = step(&mut e5, 700, false, 5);
    let ok_ledger = [
        RecoveryEntry { step: RecoveryStep::ResourceRelease, tick: 100, margin: 5, attempt: 0 },
        RecoveryEntry { step: RecoveryStep::EffectsRelease, tick: 700, margin: 4, attempt: 0 },
    ];
    let wrong_ledger = [
        RecoveryEntry { step: RecoveryStep::SchedulingRelease, tick: 100, margin: 5, attempt: 0 },
    ];
    let o3 = engine_ledger_consistent(&e5, &ok_ledger) == Ok(())
        && engine_ledger_consistent(&e5, &wrong_ledger) == Err(RcCode::ORDER_TABLE_BROKEN)
        && order_integrity() == Ok(());
    s.add("O07-OSC-引擎账一致性核验", o3, "");

    // ================= 四、判据自检（META） =================

    // META-3：码段独占——全部 0x5Dxx，且 != 0x50..0x5C（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x5D)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x50 && hi != 0x51 && hi != 0x52 && hi != 0x53
                && hi != 0x54 && hi != 0x55 && hi != 0x56 && hi != 0x57
                && hi != 0x58 && hi != 0x59 && hi != 0x5A && hi != 0x5B && hi != 0x5C
        });
    s.add("O07-META-诊断码段独占", section_ok, "");

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
    s.add("O07-META-码互异与无截断", code_ok && !s.truncated(), "");

    // META-1：判据条数对账（放末位：此时 len 应为 11，加自身恰 12）。
    s.add("O07-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
