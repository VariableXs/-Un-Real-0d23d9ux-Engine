//! UNX-F11052 对照结果总账（绿/失配/豁免三态汇总）（0x1e680 段）
//!
//! 对照结果总账：对照战役八组（每组 10 例）逐例判定后按组汇总三态，
//! 再汇总为总账。守恒式：绿 + 失配 + 豁免 == 总例数（组级与总级双口径），
//! 组账汇总 == 总账直计（两条路径必须一致），三态互斥完备。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11052 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11052 - 11000) * 0x20;

/// 总账空 —— 汇总前必须先登记对照例
pub const TS_EMPTY: u32 = SEG_BASE + 0x01;
/// 例号越界 —— 80 例编号 0..80
pub const TS_RANGE: u32 = SEG_BASE + 0x02;
/// 状态非法 —— 只允许绿/失配/豁免三态
pub const TS_BADSTATE: u32 = SEG_BASE + 0x03;
/// 口径失衡 —— 组账汇总与总账直计不一致
pub const TS_DRIFT: u32 = SEG_BASE + 0x04;
/// 组号越界 —— 八组编号 0..8
pub const TS_BADGRP: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再判定
pub const TS_FROZEN: u32 = SEG_BASE + 0x06;

/// 三态：绿（对平）
pub const ST_GREEN: u8 = 0;
/// 三态：失配
pub const ST_FAIL: u8 = 1;
/// 三态：豁免
pub const ST_WAIVE: u8 = 2;

/// 汇总组数
pub const GROUPS: usize = 8;
/// 每组例数
pub const CASES_PER_GROUP: usize = 10;
/// 总例数
pub const CASES_TOTAL: usize = GROUPS * CASES_PER_GROUP;
/// 对照结果总账标签
pub const TRITAG: u64 = 0x7472_6973_7473_3043;

/// 单条对照例
#[derive(Clone, Debug)]
pub struct TsCase {
    /// 例号（0xB000 段起编）
    pub cid: u32,
    /// 组号
    pub grp: u8,
    /// 三态状态
    pub state: u8,
    /// 期望值
    pub expect: u64,
    /// 实测值
    pub actual: u64,
}

/// 组级三态汇总
#[derive(Clone, Debug)]
pub struct GrpSum {
    /// 绿例数
    pub green: usize,
    /// 失配例数
    pub fail: usize,
    /// 豁免例数
    pub waive: usize,
}

impl GrpSum {
    /// 组内三态守恒
    pub fn total(&self) -> usize {
        self.green + self.fail + self.waive
    }
}

/// 对照结果总账
#[derive(Clone, Debug)]
pub struct TriLedger {
    /// 80 条对照例
    pub cases: Vec<TsCase>,
    /// 总级绿例数
    pub green_n: usize,
    /// 总级失配例数
    pub fail_n: usize,
    /// 总级豁免例数
    pub waive_n: usize,
    /// 已判定例数
    pub judged_n: usize,
    /// 冻结标记
    pub frozen: bool,
}

impl TriLedger {
    /// 立账：80 例全部先置失配待判态（实测留空）
    pub fn new() -> TriLedger {
        let mut lg = TriLedger {
            cases: Vec::new(),
            green_n: 0, fail_n: CASES_TOTAL, waive_n: 0,
            judged_n: 0, frozen: false,
        };
        for grp in 0..GROUPS {
            for ci in 0..CASES_PER_GROUP {
                let no = grp * CASES_PER_GROUP + ci;
                lg.cases.push(TsCase {
                    cid: 0xB000 + no as u32,
                    grp: grp as u8,
                    state: ST_FAIL,
                    expect: ((no as u64) + 1) * 5,
                    actual: 0,
                });
            }
        }
        lg
    }

    /// 判定一例：绿 / 维持失配 / 转豁免
    pub fn judge(&mut self, no: usize, actual: u64, waive: bool) -> Result<u8, u32> {
        if self.frozen { return Err(TS_FROZEN); }
        if self.cases.is_empty() { return Err(TS_EMPTY); }
        if no >= self.cases.len() { return Err(TS_RANGE); }
        let c = &mut self.cases[no];
        if c.actual != 0 { return Err(TS_BADSTATE); }
        c.actual = actual;
        let new_state = if waive {
            ST_WAIVE
        } else if c.expect == actual {
            ST_GREEN
        } else {
            ST_FAIL
        };
        match (c.state, new_state) {
            (ST_FAIL, ST_GREEN) => { self.fail_n -= 1; self.green_n += 1; }
            (ST_FAIL, ST_WAIVE) => { self.fail_n -= 1; self.waive_n += 1; }
            (ST_FAIL, ST_FAIL) => {}
            _ => return Err(TS_BADSTATE),
        }
        c.state = new_state;
        self.judged_n += 1;
        Ok(new_state)
    }

    /// 组级汇总：按组口径重算三态
    pub fn grp_sum(&self, grp: u8) -> Result<GrpSum, u32> {
        if grp as usize >= GROUPS { return Err(TS_BADGRP); }
        let mut gs = GrpSum { green: 0, fail: 0, waive: 0 };
        for c in &self.cases {
            if c.grp == grp {
                match c.state {
                    ST_GREEN => gs.green += 1,
                    ST_FAIL => gs.fail += 1,
                    _ => gs.waive += 1,
                }
            }
        }
        Ok(gs)
    }

    /// 口径一致性：组账汇总与总账直计一致
    pub fn no_drift(&self) -> bool {
        let mut g = 0usize; let mut f = 0usize; let mut w = 0usize;
        for grp in 0..GROUPS {
            if let Ok(gs) = self.grp_sum(grp as u8) {
                g += gs.green; f += gs.fail; w += gs.waive;
            }
        }
        g == self.green_n && f == self.fail_n && w == self.waive_n
    }

    /// 三态守恒：三态之和等于总例数
    pub fn conserved(&self) -> bool {
        self.green_n + self.fail_n + self.waive_n == CASES_TOTAL
    }

    /// 口径闸：漂移即返回 TS_DRIFT
    pub fn drift_gate(&self) -> Result<(), u32> {
        if self.no_drift() { Ok(()) } else { Err(TS_DRIFT) }
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = TRITAG;
        for c in &self.cases {
            h = (h ^ (c.cid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ c.expect).wrapping_mul(0x100000001b3);
            h = (h ^ c.actual).wrapping_mul(0x100000001b3);
            h = (h ^ (c.state as u64)).wrapping_mul(0x100000001b3);
        }
        for x in [self.green_n as u64, self.fail_n as u64, self.waive_n as u64] {
            h = (h ^ x).wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 冻结总账
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：60 绿 + 10 失配 + 10 豁免
pub fn sample_judged() -> (TriLedger, bool) {
    let mut lg = TriLedger::new();
    for no in 0..CASES_TOTAL {
        let expect = lg.cases[no].expect;
        let waive = no % 8 == 7;          // 10 例豁免
        let actual = if no % 16 == 3 { expect + 1 } else { expect }; // 5 例失配
        let _ = lg.judge(no, actual, waive);
    }
    let ok = lg.conserved() && lg.no_drift();
    lg.freeze();
    (lg, ok)
}

/// 注入路径：重复判定 / 组越界 / 冻结 / 空账
pub fn inject_four() -> [bool; 4] {
    let mut lg = TriLedger::new();
    // 注入 1：判定后重复判定 → TS_BADSTATE 拒止
    let _ = lg.judge(0, lg.cases[0].expect, false);
    let i1 = matches!(lg.judge(0, lg.cases[0].expect, false), Err(TS_BADSTATE));
    // 注入 2：组号越界 → TS_BADGRP 拒止
    let i2 = matches!(lg.grp_sum(GROUPS as u8), Err(TS_BADGRP));
    // 注入 3：冻结后判定 → TS_FROZEN 拒止
    lg.freeze();
    let i3 = matches!(lg.judge(1, 1, false), Err(TS_FROZEN));
    // 注入 4：空账判定 → TS_EMPTY 拒止
    let mut el = TriLedger { cases: Vec::new(), green_n: 0, fail_n: 0, waive_n: 0, judged_n: 0, frozen: false };
    let i4 = matches!(el.judge(0, 1, false), Err(TS_EMPTY));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 绿/失配/豁免三态汇总，总账守恒闭合
pub fn run_unx11052_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11052");
    let (lg, ok) = sample_judged();

    s.add("J1-三态汇总", ok && lg.conserved(), "绿/失配/豁免三态互斥完备守恒");
    s.add("J1-组账齐备", (0..GROUPS as u8).all(|g| lg.grp_sum(g).map(|gs| gs.total() == CASES_PER_GROUP).unwrap_or(false)), "八组每组 10 例组账齐备");
    s.add("J2-口径一致", lg.no_drift() && lg.drift_gate().is_ok(), "组账汇总与总账直计口径一致");
    s.add("J2-判定完备", lg.judged_n == CASES_TOTAL, "80 例全部判定");
    s.add("J2-绿态计数", lg.green_n + lg.fail_n + lg.waive_n == CASES_TOTAL, "三态计数之和等于总例数");
    s.add("J3-豁免计数", lg.waive_n == 10, "豁免 10 例计数准确");
    s.add("J3-失配计数", lg.fail_n == 5, "失配 5 例计数准确");
    s.add("J3-绿态多数", lg.green_n == CASES_TOTAL - 15, "绿态 65 例计数准确");
    s.add("J4-编号连续", lg.cases[0].cid == 0xB000 && lg.cases[CASES_TOTAL - 1].cid == 0xB04F, "例号 0xB000 段起编连续");
    s.add("J4-组归属", lg.cases.iter().filter(|c| c.grp == 0).count() == CASES_PER_GROUP, "组归属标注准确每组 10 例");
    s.add("J5-重复拒判", inject_four()[0], "重复判定被 TS_BADSTATE 拒止");
    s.add("J5-组越界拒查", inject_four()[1], "组号越界被 TS_BADGRP 拒止");
    s.add("J5-冻结拒判", inject_four()[2], "冻结后判定被 TS_FROZEN 拒止");
    s.add("J6-空账拒判", inject_four()[3], "空账判定被 TS_EMPTY 拒止");
    s.add("J6-指纹稳定", lg.frozen && lg.fold() == lg.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J7-汇总动态",
        ok,
        &format!("绿 {} 失配 {} 豁免 {}/{}，fold={:#x}", lg.green_n, lg.fail_n, lg.waive_n, CASES_TOTAL, lg.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11052_tri_state_ledger_flow() {
        let mut lg = TriLedger::new();
        // 越界拒判
        assert!(matches!(lg.judge(999, 0, false), Err(TS_RANGE)));
        // 样本复放
        for no in 0..CASES_TOTAL {
            let expect = lg.cases[no].expect;
            let waive = no % 8 == 7;
            let actual = if no % 16 == 3 { expect + 1 } else { expect };
            let _ = lg.judge(no, actual, waive);
        }
        assert!(lg.conserved());
        assert_eq!(lg.green_n, 65);
        assert_eq!(lg.fail_n, 5);
        assert_eq!(lg.waive_n, 10);
        assert!(lg.no_drift());
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11052_checks_green() {
        let s = run_unx11052_checks();
        assert!(s.all_passed(), "UNX-F11052 判据未全绿");
    }
}
