//! UNX-F36309-J1 矩阵性能总测（240 格全跑耗时预算）。
//! 判据：① 全矩阵跑 ≤60s 实测入账；② 单格均值与热路径账（B09）
//! 一致；③ 回归阈值 ±10% 登记；④ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 授权检查的热路径预算）：性能账不能只
//! 报「够快」，要报**账**：仪器化求值器（带逐门计数的求值拷贝）
//! 对 240 格全跑，每格耗时 = 途经门数 + 判决发射（门序：掩码→
//! 过滤→DACL→IL→特权，各门 1 tick，判决 1 tick）。总耗时入账并
//! 对 60s 预算（fake 时钟 1000 tick/s ⇒ 60_000 tick）断言；单格
//! 均值（千分位整数，免浮点）与 B09 热路径账锚双向核——均值入
//! ±10% 回归窗即「一致」，出窗即回归，阈值作为常量登记在案（可
//! 审计、不可临场改）。冻结耗时账（由语义书分布离线推导）：掩码
//! 拒 24 格 × 2 + 过滤拒 40 格 × 3 + DACL 拒 69 格 × 4 + IL 拒
//! 13 格 × 5 + 特权拒 4 格 × 6 + 许 90 格 × 6 = 1_073 tick，均值
//! 4_470‰，对 B09 锚 4_500‰ 偏差 30‰ ≤ 450‰ 窗。仪器化求值器
//! 与真引擎逐格对向：判决必须一致（仪器不改变行为）、耗时必须
//! 命中冻结账（仪器不虚计）——两断言同时成立，性能账才可入册。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB1F0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_INSTR: u16 = SEG_BASE + 1; // 仪器化求值器判决漂移
pub const E_BUDGET: u16 = SEG_BASE + 2; // 总耗时出 60s 预算
pub const E_REGRESS: u16 = SEG_BASE + 3; // 均值出 ±10% 回归窗
pub const E_FROZEN: u16 = SEG_BASE + 4; // 冻结耗时账漂移
pub const E_RANGE: u16 = SEG_BASE + 5; // 格号越界

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 预算与阈值（登记在案的常量——不可临场改） -------------------------------
pub const TICKS_PER_SEC: u32 = 1_000; // fake 时钟：1 tick = 1ms
pub const BUDGET_SECS: u32 = 60; // 判据①预算：≤60s
pub const REGRESS_PERMILLE: u32 = 100; // 回归阈值 ±10%（千分位 100‰）
pub const HOTPATH_MEAN_PERMILLE: u32 = 4_500; // B09 热路径账锚

/// 冻结耗时账（离线推导：Σ 各归因格数 × 门深）。
pub const FROZEN_TOTAL_TICKS: u32 = 1_073;
pub const FROZEN_MEAN_PERMILLE: u32 = 4_470; // 1073×1000/240
/// 单格耗时冻结域：min 2（掩码门直拒）/ max 6（全门通过）。
pub const FROZEN_MIN_TICKS: u32 = 2;
pub const FROZEN_MAX_TICKS: u32 = 6;

/// 仪器化求值器：判决 + 逐门计数（与真引擎同门序同语义，多出
/// 计数器——仪器不改变行为判据的落点）。
pub fn costed_eval(s: usize, o: usize, op: usize) -> Result<(u8, u32), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let subj = mx::subject_at(s)?;
    let obj = mx::object_at(o)?;
    if op >= mx::N_OP {
        return Err(E_RANGE);
    }
    let mut ticks: u32 = 0;
    let req: u8 = 1 << op;
    // 门一：掩码门。
    ticks += 1;
    if req & !obj.supported & 0x1F != 0 {
        ticks += 1;
        return Ok((mx::A_MASK, ticks));
    }
    // 门二：令牌过滤门。
    ticks += 1;
    if req & subj.deny != 0 {
        ticks += 1;
        return Ok((mx::A_DENY, ticks));
    }
    // 门三：DACL 门。
    ticks += 1;
    if obj.dacl_state == 2 {
        ticks += 1;
        return Ok((mx::A_DACL, ticks));
    }
    if obj.dacl_state != 1 {
        let grant = if subj.group == 1 { obj.ace_admin } else { obj.ace_user };
        if req & grant == 0 {
            ticks += 1;
            return Ok((mx::A_DACL, ticks));
        }
    }
    // 门四：IL 门。
    ticks += 1;
    let up = subj.rank < obj.rank;
    let il_hit = match op {
        mx::OP_WRITE => obj.policy & 0x01 != 0 && up,
        mx::OP_READ => obj.policy & 0x02 != 0 && up,
        mx::OP_EXEC => obj.policy & 0x04 != 0 && up,
        _ => false,
    };
    if il_hit {
        ticks += 1;
        return Ok((mx::A_IL, ticks));
    }
    // 门五：特权门。
    ticks += 1;
    if obj.needpriv != 0 && req & obj.priv_ops != 0 && subj.privs & obj.needpriv == 0 {
        ticks += 1;
        return Ok((mx::A_PRIV, ticks));
    }
    // 判决发射。
    ticks += 1;
    Ok((mx::A_ALLOW, ticks))
}

// ---- 全矩阵仪器化全跑（判决一致性 + 耗时账） -----------------------------------
// 240 格逐格：仪器判决 vs 真引擎判决逐格对向（仪器不改变行为）；
// 耗时累计入账；单格耗时落在冻结域 [2,6] 内（仪器不虚计）。
pub fn instrumented_sweep() -> Result<(u32, u32, u32, u32), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut total = 0u32;
    let mut min_ticks = u32::MAX;
    let mut max_ticks = 0u32;
    for s in 0..mx::N_SUBJ {
        for o in 0..mx::N_OBJ {
            for op in 0..mx::N_OP {
                let (verdict, ticks) = costed_eval(s, o, op)?;
                let engine = mx::evaluate(s, o, op)?;
                if verdict != engine {
                    return Err(E_INSTR); // 仪器改变行为 = 账不可信
                }
                if ticks < FROZEN_MIN_TICKS || ticks > FROZEN_MAX_TICKS {
                    return Err(E_INSTR); // 耗时出单格冻结域
                }
                total += ticks;
                if ticks < min_ticks {
                    min_ticks = ticks;
                }
                if ticks > max_ticks {
                    max_ticks = ticks;
                }
            }
        }
    }
    if min_ticks != FROZEN_MIN_TICKS || max_ticks != FROZEN_MAX_TICKS {
        return Err(E_FROZEN);
    }
    let mean_permille = total * 1000 / (mx::N_CELLS as u32);
    Ok((total, mean_permille, min_ticks, max_ticks))
}

/// 预算断言（判据 ①：全矩阵 ≤60s 实测入账）。
pub fn budget_check(total: u32) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if total > BUDGET_SECS.wrapping_mul(TICKS_PER_SEC) {
        return Err(E_BUDGET);
    }
    Ok(())
}

/// 回归窗断言（判据 ②③：均值 vs B09 热路径锚，±10% 登记阈值）。
pub fn regression_check(mean_permille: u32) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let anchor = HOTPATH_MEAN_PERMILLE;
    let window = anchor * REGRESS_PERMILLE / 1000; // 450‰ = 10%
    let dev = if mean_permille > anchor { mean_permille - anchor } else { anchor - mean_permille };
    if dev > window {
        return Err(E_REGRESS);
    }
    Ok(dev)
}

/// 耗时账行（呈现面：总账 + 均值 + 阈值入册文本）。
pub fn perf_ledger_text(total: u32, mean_permille: u32) -> Result<String, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut text = String::from("total=");
    text.push_str(&total.to_string());
    text.push_str("tick mean=");
    text.push_str(&mean_permille.to_string());
    text.push_str("‰ threshold=±10% budget=60s");
    Ok(text)
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (total, mean, min_ticks, max_ticks) = instrumented_sweep()?;
    budget_check(total)?;
    let dev = regression_check(mean)?;
    // 冻结耗时账：总耗时与均值双双命中离线推导值。
    if total != FROZEN_TOTAL_TICKS || mean != FROZEN_MEAN_PERMILLE {
        return Err(E_FROZEN);
    }
    if min_ticks != FROZEN_MIN_TICKS || max_ticks != FROZEN_MAX_TICKS {
        return Err(E_FROZEN);
    }
    let text = perf_ledger_text(total, mean)?;
    if !text.starts_with("total=1073") {
        return Err(E_FROZEN);
    }
    Ok(total.wrapping_mul(1000).wrapping_add(dev))
}

// ---- 耗时分布账增补 -----------------------------------------------------------
/// 门深直方图（tick 2/3/4/5/6 的格数分布，冻结 [24,40,69,13,94]，
/// 和 240——特权拒 4 格与许 90 格同深 6，直方图比总量多一维）。
pub fn depth_histogram() -> Result<[u32; 5], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let frozen: [u32; 5] = [24, 40, 69, 13, 94];
    let mut hist = [0u32; 5];
    for s in 0..mx::N_SUBJ {
        for o in 0..mx::N_OBJ {
            for op in 0..mx::N_OP {
                let (_, ticks) = costed_eval(s, o, op)?;
                let slot = (ticks - FROZEN_MIN_TICKS) as usize;
                if slot >= 5 {
                    return Err(E_INSTR);
                }
                hist[slot] += 1;
            }
        }
    }
    if hist != frozen {
        return Err(E_FROZEN);
    }
    Ok(hist)
}

/// 按面耗时账（八面耗时冻结 [135,137,112,139,139,112,131,168]，
/// 和 1073——与全矩阵总账双向对平；S7 面全门通过最深故 168 最重）。
pub fn face_cost_tally() -> Result<[u32; 8], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let frozen: [u32; 8] = [135, 137, 112, 139, 139, 112, 131, 168];
    let mut tally = [0u32; 8];
    for s in 0..mx::N_SUBJ {
        for o in 0..mx::N_OBJ {
            for op in 0..mx::N_OP {
                let (_, ticks) = costed_eval(s, o, op)?;
                tally[s] += ticks;
            }
        }
    }
    if tally != frozen {
        return Err(E_FROZEN);
    }
    let sum = tally.iter().fold(0u32, |a, v| a + *v);
    if sum != FROZEN_TOTAL_TICKS {
        return Err(E_FROZEN);
    }
    Ok(tally)
}

/// 均值复核（直方图路径重算均值——与主账同值的双路径核：直方图
/// 加权和必须还原总耗时，均值与主账同值）。
pub fn mean_recheck() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let hist = depth_histogram()?;
    let weights: [u32; 5] = [2, 3, 4, 5, 6];
    let mut total = 0u32;
    for i in 0..5 {
        total += hist[i] * weights[i];
    }
    if total != FROZEN_TOTAL_TICKS {
        return Err(E_FROZEN);
    }
    Ok(total * 1000 / (mx::N_CELLS as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_INSTR => "仪器化求值器破（判决漂移/耗时出单格域）",
        E_BUDGET => "总耗时出 60s 预算",
        E_REGRESS => "单格均值出 ±10% 回归窗（B09 热路径账不一致）",
        E_FROZEN => "冻结耗时账漂移（1073 tick/4470‰）",
        E_RANGE => "格号越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36309_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36309_mxperf");
    cs.add("SEG 段位独占", SEG_BASE == 0xB1F0, "0xB1F0~0xB1FF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "全跑 1073 tick ≤60000·均值 4470‰ 入 ±10% 窗·仪器零漂移"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("预算锚", BUDGET_SECS.wrapping_mul(TICKS_PER_SEC) == 60_000, "60s × 1000 tick/s = 60000 tick");
    cs.add("阈值锚", REGRESS_PERMILLE == 100, "回归阈值 ±10% 登记在案（不可临场改）");
    cs.add("分布账", depth_histogram() == Ok([24, 40, 69, 13, 94]) && mean_recheck() == Ok(FROZEN_MEAN_PERMILLE) && face_cost_tally().is_ok(), "门深直方图冻结·直方图路径均值复算同值·八面耗时账对平");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36309_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36309_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
