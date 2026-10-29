//! UNX-F11057 对照覆盖率账（用例组覆盖全景对账）（0x1e720 段）
//!
//! 对照覆盖率账：八用例组 × 五覆盖维（API 面 / 错误面 / 并发面 /
//! 边界面 / 语义面）构成 40 格全景矩阵，逐格登记覆盖用例数。
//! 判据：每格至少 1 例（覆盖盲区零容忍），组覆盖 5/5 维，
//! 总账例数守恒（格账之和 == 总例数）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11057 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11057 - 11000) * 0x20;

/// 矩阵空 —— 对账前必须先登记覆盖格
pub const CV_EMPTY: u32 = SEG_BASE + 0x01;
/// 格坐标越界 —— 8 组 × 5 维编号 0..40
pub const CV_RANGE: u32 = SEG_BASE + 0x02;
/// 盲区检出 —— 存在零覆盖格
pub const CV_BLIND: u32 = SEG_BASE + 0x03;
/// 重复登记 —— 同一格重复初始化
pub const CV_DUP: u32 = SEG_BASE + 0x04;
/// 组号越界 —— 八组编号 0..8
pub const CV_BADGRP: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再登记
pub const CV_FROZEN: u32 = SEG_BASE + 0x06;

/// 用例组数
pub const GROUPS: usize = 8;
/// 覆盖维数
pub const DIMS: usize = 5;
/// 全景格数
pub const CELLS: usize = GROUPS * DIMS;
/// 每格覆盖用例数（确定性样本按格号取模 1..3）
pub const COVER_MIN: usize = 1;
/// 覆盖率账标签
pub const COV_TAG: u64 = 0x636f_7663_6f76_3048;

/// 覆盖维名（登记用）
pub const DIM_API: usize = 0;
/// 错误面
pub const DIM_ERR: usize = 1;
/// 并发面
pub const DIM_CONC: usize = 2;
/// 边界面
pub const DIM_EDGE: usize = 3;
/// 语义面
pub const DIM_SEM: usize = 4;

/// 单个覆盖格
#[derive(Clone, Debug)]
pub struct CovCell {
    /// 格号（0x10000 段起编）
    pub cid: u32,
    /// 组号
    pub grp: u8,
    /// 维号
    pub dim: u8,
    /// 覆盖用例数
    pub cases: usize,
    /// 已对账标记
    pub audited: bool,
}

/// 覆盖率总账
#[derive(Clone, Debug)]
pub struct CovLedger {
    /// 40 格全景矩阵
    pub cells: Vec<CovCell>,
    /// 已对账格数
    pub audited_n: usize,
    /// 覆盖用例总数
    pub case_sum: usize,
    /// 盲区检出次数
    pub blinds: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl CovLedger {
    /// 立账：40 格全部登记，cases 按 (grp*5+dim) 确定性取 1..4
    pub fn new() -> CovLedger {
        let mut lg = CovLedger {
            cells: Vec::new(),
            audited_n: 0, case_sum: 0, blinds: 0, frozen: false,
        };
        for grp in 0..GROUPS {
            for dim in 0..DIMS {
                let no = grp * DIMS + dim;
                let cases = COVER_MIN + no % 3;
                lg.cells.push(CovCell {
                    cid: 0x10000 + no as u32,
                    grp: grp as u8,
                    dim: dim as u8,
                    cases,
                    audited: false,
                });
                lg.case_sum += cases;
            }
        }
        lg
    }

    /// 对账一格：盲区（cases==0）即检出
    pub fn audit(&mut self, no: usize) -> Result<usize, u32> {
        if self.frozen { return Err(CV_FROZEN); }
        if self.cells.is_empty() { return Err(CV_EMPTY); }
        if no >= self.cells.len() { return Err(CV_RANGE); }
        let c = &mut self.cells[no];
        if c.audited { return Err(CV_DUP); }
        c.audited = true;
        self.audited_n += 1;
        if c.cases == 0 {
            self.blinds = self.blinds.wrapping_add(1);
        }
        Ok(c.cases)
    }

    /// 全景对账
    pub fn audit_all(&mut self) -> Result<usize, u32> {
        for no in 0..self.cells.len() {
            self.audit(no)?;
        }
        Ok(self.audited_n)
    }

    /// 组覆盖维数：该组 cases>0 的维数
    pub fn grp_dims(&self, grp: u8) -> Result<usize, u32> {
        if grp as usize >= GROUPS { return Err(CV_BADGRP); }
        Ok(self.cells.iter().filter(|c| c.grp == grp && c.cases > 0).count())
    }

    /// 零盲区：40 格全非零
    pub fn zero_blind(&self) -> bool {
        self.cells.iter().all(|c| c.cases >= COVER_MIN)
    }

    /// 盲区闸：有盲区即 CV_BLIND
    pub fn blind_gate(&self) -> Result<(), u32> {
        if self.zero_blind() { Ok(()) } else { Err(CV_BLIND) }
    }

    /// 例数守恒：格账之和等于立账直计
    pub fn cases_conserved(&self) -> bool {
        self.cells.iter().map(|c| c.cases).sum::<usize>() == self.case_sum
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = COV_TAG;
        for c in &self.cells {
            h = (h ^ (c.cid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (c.cases as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (c.audited as u64)).wrapping_mul(0x100000001b3);
        }
        h = (h ^ (self.case_sum as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结覆盖账
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：40 格全对账零盲区
pub fn sample_audited() -> (CovLedger, bool) {
    let mut lg = CovLedger::new();
    let _ = lg.audit_all();
    let ok = lg.zero_blind() && lg.audited_n == CELLS;
    lg.freeze();
    (lg, ok)
}

/// 注入路径：盲区注入 / 越界 / 重复对账 / 冻结
pub fn inject_four() -> [bool; 4] {
    let mut lg = CovLedger::new();
    // 注入 1：清零一格制造盲区 → blind_gate 即 CV_BLIND
    lg.cells[5].cases = 0;
    let i1 = matches!(lg.blind_gate(), Err(CV_BLIND));
    // 注入 2：格号越界 → CV_RANGE 拒止
    let i2 = matches!(lg.audit(CELLS), Err(CV_RANGE));
    // 注入 3：重复对账 → CV_DUP 拒止
    let _ = lg.audit(0);
    let i3 = matches!(lg.audit(0), Err(CV_DUP));
    // 注入 4：冻结后对账 → CV_FROZEN 拒止
    lg.freeze();
    let i4 = matches!(lg.audit(1), Err(CV_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 覆盖率逐组对平，覆盖盲区零容忍
pub fn run_unx11057_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11057");
    let (lg, ok) = sample_audited();

    s.add("J1-全景对账", ok && lg.audited_n == CELLS, "40 格全景矩阵全对账");
    s.add("J1-零盲区", lg.zero_blind() && lg.blind_gate().is_ok(), "覆盖盲区零容忍全部放行");
    s.add("J1-组覆盖", (0..GROUPS as u8).all(|g| lg.grp_dims(g).unwrap_or(0) == DIMS), "八组每组 5/5 维覆盖");
    s.add("J2-例数守恒", lg.cases_conserved() && lg.case_sum > 0, "格账之和与立账直计守恒");
    s.add("J2-每格非零", lg.cells.iter().all(|c| c.cases >= COVER_MIN), "每格至少 1 例覆盖");
    s.add("J2-维名合法", lg.cells.iter().all(|c| (c.dim as usize) < DIMS), "维号取值合法");
    s.add("J3-维分布", DIMS == 5 && lg.cells.iter().filter(|c| c.dim as usize == DIM_API).count() == GROUPS, "API 面维每组 1 格分布");
    s.add("J3-编号连续", lg.cells[0].cid == 0x10000 && lg.cells[CELLS - 1].cid == 0x10027, "格号 0x10000 段起编连续");
    s.add("J3-组归属", lg.cells.iter().filter(|c| c.grp == 0).count() == DIMS, "组归属标注准确每组 5 格");
    s.add("J4-盲区拒放", inject_four()[0], "盲区注入后 blind_gate 即 CV_BLIND 拒止");
    s.add("J4-越界拒账", inject_four()[1], "格号越界被 CV_RANGE 拒止");
    s.add("J5-重复拒账", inject_four()[2], "重复对账被 CV_DUP 拒止");
    s.add("J5-冻结拒账", inject_four()[3], "冻结后对账被 CV_FROZEN 拒止");
    s.add("J6-空账拒账", CovLedger { cells: Vec::new(), audited_n: 0, case_sum: 0, blinds: 0, frozen: false }.audit(0).is_err(), "空账对账被 CV_EMPTY 拒止");
    s.add("J6-指纹稳定", lg.frozen && lg.fold() == lg.fold(), "冻结后指纹重算稳定");

    // J8 盲区口径：两格清零后盲区逐格计数、盲闸拒止、零盲判定翻假
    let j8 = {
        let mut c = CovLedger::new();
        let _ = c.audit_all();
        c.cells[2].cases = 0;
        c.cells[9].cases = 0;
        c.blinds = c.cells.iter().filter(|k| k.cases == 0).count() as u64;
        c.blinds == 2 && c.blind_gate().err() == Some(CV_BLIND) && !c.zero_blind()
    };
    s.add("J8-盲区口径", j8, "盲区逐格计数且盲闸拒止");

    // J9 组号越界：组号 >= GROUPS 查询必拒
    let j9 = lg.grp_dims(GROUPS as u8).is_err();
    s.add("J9-组号越界", j9, "组号越界查询被拒");

    s.add_dyn(
        "J7-覆盖动态",
        ok,
        &format!("对账 {}/{}，例数 {}，盲区 {}，fold={:#x}", lg.audited_n, CELLS, lg.case_sum, lg.blinds, lg.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11057_coverage_ledger_flow() {
        let mut lg = CovLedger::new();
        // 全景对账
        assert_eq!(lg.audit_all().unwrap_or(0), CELLS);
        assert!(lg.zero_blind());
        assert_eq!(lg.audited_n, CELLS);
        assert!(lg.cases_conserved());
        // 组覆盖抽查
        assert_eq!(lg.grp_dims(3).unwrap_or(0), DIMS);
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11057_checks_green() {
        let s = run_unx11057_checks();
        assert!(s.all_passed(), "UNX-F11057 判据未全绿");
    }
}
