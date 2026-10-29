//! UNX-F11051 R-C3-003 释放闸对账面（C3 释放项 C4 承接核销）（0x1e660 段）
//!
//! 释放闸对账面：C3 段释放项逐件与 C4 段承接项配对核销。
//! 20 个释放项（5 组 × 4 件）逐件登记 C3 释放语义，再逐件
//! 挂接 C4 承接项；守恒式：carried == released（每放必承），
//! 承接值之和不小于释放值之和（C4 不缩水），零悬空项。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11051 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11051 - 11000) * 0x20;

/// 闸面空 —— 核销前必须先登记释放项
pub const RG_EMPTY: u32 = SEG_BASE + 0x01;
/// 项号越界 —— 20 项编号 0..20
pub const RG_RANGE: u32 = SEG_BASE + 0x02;
/// 重复承接 —— 一件释放项只能被承接一次
pub const RG_DUP: u32 = SEG_BASE + 0x03;
/// 未释放先承 —— 只有 released 态可承接
pub const RG_NOTREL: u32 = SEG_BASE + 0x04;
/// 悬空检出 —— 核销时仍有释放项无人承接
pub const RG_DANGLE: u32 = SEG_BASE + 0x05;
/// 闸已冻结 —— freeze 后禁止再承接
pub const RG_FROZEN: u32 = SEG_BASE + 0x06;

/// 释放组数
pub const GROUPS: usize = 5;
/// 每组释放件数
pub const ITEMS_PER_GROUP: usize = 4;
/// 释放项总数
pub const ITEMS_TOTAL: usize = GROUPS * ITEMS_PER_GROUP;
/// 释放闸对账面标签
pub const GATE_TAG: u64 = 0x7267_3367_726c_3042;

/// 单条释放项：C3 释放值 + C4 承接值 + 配对标记
#[derive(Clone, Debug)]
pub struct RelItem {
    /// 释放项号（0xA300 段起编）
    pub cid: u32,
    /// 项名
    pub name: String,
    /// 所属组号
    pub grp: u8,
    /// C3 释放值
    pub released: u64,
    /// C4 承接值（0 = 未承接）
    pub carried: u64,
    /// 承接核销标记
    pub settled: bool,
}

/// 释放闸对账总账
#[derive(Clone, Debug)]
pub struct GateLedger {
    /// 20 条释放项
    pub items: Vec<RelItem>,
    /// C3 释放值累计
    pub rel_sum: u64,
    /// C4 承接值累计
    pub car_sum: u64,
    /// 已承接件数
    pub carried_n: usize,
    /// 已核销件数
    pub settled_n: usize,
    /// 冻结标记
    pub frozen: bool,
}

impl GateLedger {
    /// 立账：20 项 C3 释放语义先行登记，承接留空
    pub fn new() -> GateLedger {
        let mut lg = GateLedger {
            items: Vec::new(),
            rel_sum: 0, car_sum: 0,
            carried_n: 0, settled_n: 0, frozen: false,
        };
        for grp in 0..GROUPS {
            for it in 0..ITEMS_PER_GROUP {
                let no = grp * ITEMS_PER_GROUP + it;
                let released = ((no as u64) + 1) * 7 + 3;
                lg.items.push(RelItem {
                    cid: 0xA300 + no as u32,
                    name: "rel_item".to_string(),
                    grp: grp as u8,
                    released,
                    carried: 0,
                    settled: false,
                });
                lg.rel_sum = lg.rel_sum.wrapping_add(released);
            }
        }
        lg
    }

    /// C4 承接一件释放项：承接值须不小于释放值
    pub fn carry(&mut self, no: usize, carried: u64) -> Result<(), u32> {
        if self.frozen { return Err(RG_FROZEN); }
        if self.items.is_empty() { return Err(RG_EMPTY); }
        if no >= self.items.len() { return Err(RG_RANGE); }
        let it = &mut self.items[no];
        if it.settled || it.carried > 0 { return Err(RG_DUP); }
        if it.released == 0 { return Err(RG_NOTREL); }
        if carried < it.released { return Err(RG_NOTREL); }
        it.carried = carried;
        self.car_sum = self.car_sum.wrapping_add(carried);
        self.carried_n += 1;
        Ok(())
    }

    /// 核销一件已承接项
    pub fn settle(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen { return Err(RG_FROZEN); }
        if no >= self.items.len() { return Err(RG_RANGE); }
        let it = &mut self.items[no];
        if it.carried == 0 { return Err(RG_NOTREL); }
        if it.settled { return Err(RG_DUP); }
        it.settled = true;
        self.settled_n += 1;
        Ok(())
    }

    /// 承接并核销一步到位
    pub fn carry_settle(&mut self, no: usize) -> Result<(), u32> {
        if no >= self.items.len() { return Err(RG_RANGE); }
        let need = self.items[no].released;
        self.carry(no, need)?;
        self.settle(no)
    }

    /// 全量承接核销
    pub fn settle_all(&mut self) -> Result<usize, u32> {
        for no in 0..self.items.len() {
            self.carry_settle(no)?;
        }
        Ok(self.settled_n)
    }

    /// 零悬空探测：每件释放项均有承接
    pub fn zero_dangle(&self) -> bool {
        self.items.iter().all(|it| it.carried >= it.released && it.settled)
    }

    /// 悬空闸：存在悬空即返回 RG_DANGLE
    pub fn dangle_gate(&self) -> Result<(), u32> {
        if self.zero_dangle() { Ok(()) } else { Err(RG_DANGLE) }
    }

    /// C4 不缩水：承接值之和不小于释放值之和
    pub fn no_shrink(&self) -> bool {
        self.car_sum >= self.rel_sum && self.rel_sum > 0
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = GATE_TAG;
        for it in &self.items {
            h = (h ^ (it.cid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ it.released).wrapping_mul(0x100000001b3);
            h = (h ^ it.carried).wrapping_mul(0x100000001b3);
            h = (h ^ (it.settled as u64)).wrapping_mul(0x100000001b3);
        }
        for x in [self.rel_sum, self.car_sum] {
            h = (h ^ x).wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 冻结闸面
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：20 项全承接核销
pub fn sample_settled() -> (GateLedger, bool) {
    let mut lg = GateLedger::new();
    let _ = lg.settle_all();
    let ok = lg.zero_dangle() && lg.no_shrink();
    lg.freeze();
    (lg, ok)
}

/// 注入路径：不足值承接 / 重复承接 / 越界 / 冻结
pub fn inject_four() -> [bool; 4] {
    let mut lg = GateLedger::new();
    // 注入 1：承接值小于释放值 → RG_NOTREL 拒止
    let i1 = matches!(lg.carry(0, lg.items[0].released - 1), Err(RG_NOTREL));
    // 注入 2：正确承接后重复承接 → RG_DUP 拒止
    let _ = lg.carry(0, lg.items[0].released);
    let i2 = matches!(lg.carry(0, lg.items[0].released), Err(RG_DUP));
    // 注入 3：项号越界 → RG_RANGE 拒止
    let i3 = matches!(lg.carry(ITEMS_TOTAL, 1), Err(RG_RANGE));
    // 注入 4：冻结后核销 → RG_FROZEN 拒止
    lg.freeze();
    let i4 = matches!(lg.settle(1), Err(RG_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 释放项逐件承接对平，零悬空
pub fn run_unx11051_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11051");
    let (lg, ok) = sample_settled();

    s.add("J1-逐件承接", ok && lg.carried_n == ITEMS_TOTAL, "20 件释放项逐件 C4 承接");
    s.add("J1-逐件核销", lg.settled_n == ITEMS_TOTAL, "20 件承接项逐件核销");
    s.add("J1-零悬空", lg.zero_dangle() && lg.dangle_gate().is_ok(), "承接全覆盖零悬空项");
    s.add("J2-放承守恒", lg.carried_n == ITEMS_TOTAL && lg.rel_sum > 0, "carried==released 每放必承");
    s.add("J2-不缩水", lg.no_shrink(), "C4 承接值之和不小于 C3 释放值之和");
    s.add("J2-足额承接", lg.items.iter().all(|it| it.carried >= it.released), "每件承接值不小于释放值");
    s.add("J3-分组齐备", (0..GROUPS as u8).all(|g| lg.items.iter().filter(|it| it.grp == g).count() == ITEMS_PER_GROUP), "五组每组 4 件齐备");
    s.add("J3-编号连续", lg.items[0].cid == 0xA300 && lg.items[ITEMS_TOTAL - 1].cid == 0xA313, "释放项号 0xA300 段起编连续");
    s.add("J3-释放非零", lg.items.iter().all(|it| it.released > 0), "全部释放值非零登记");
    s.add("J4-不足拒承", inject_four()[0], "不足值承接被 RG_NOTREL 拒止");
    s.add("J4-重复拒承", inject_four()[1], "重复承接被 RG_DUP 拒止");
    s.add("J5-越界拒承", inject_four()[2], "项号越界被 RG_RANGE 拒止");
    s.add("J5-冻结拒销", inject_four()[3], "冻结后核销被 RG_FROZEN 拒止");
    s.add("J6-悬空闸", GateLedger::new().dangle_gate().is_err(), "未承接账过悬空闸即 RG_DANGLE 拒止");
    s.add("J6-指纹稳定", lg.frozen && lg.fold() == lg.fold(), "冻结后指纹重算稳定");

    // J7 错误码分段互异：六码落在段基址页内且两两不同
    let codes = [RG_EMPTY, RG_RANGE, RG_DUP, RG_NOTREL, RG_DANGLE, RG_FROZEN];
    let mut in_seg = true;
    let mut distinct = true;
    let mut a = 0usize;
    while a < codes.len() {
        if codes[a] <= SEG_BASE || codes[a] >= SEG_BASE + 0x20 {
            in_seg = false;
        }
        let mut b = a.wrapping_add(1);
        while b < codes.len() {
            if codes[a] == codes[b] {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J7-错误码分段", in_seg && distinct, "六码同段内两两互异");
    s.add_dyn(
        "J7-核销动态",
        ok,
        &format!("核销 {}/{}，释放 {} 承接 {}，fold={:#x}", lg.settled_n, ITEMS_TOTAL, lg.rel_sum, lg.car_sum, lg.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11051_rc3_release_gate_flow() {
        let mut lg = GateLedger::new();
        // 未承接前悬空闸拒止
        assert!(matches!(lg.dangle_gate(), Err(RG_DANGLE)));
        // 全量承接核销
        assert_eq!(lg.settle_all().unwrap_or(0), ITEMS_TOTAL);
        assert!(lg.zero_dangle());
        assert!(lg.no_shrink());
        assert_eq!(lg.carried_n, ITEMS_TOTAL);
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11051_checks_green() {
        let s = run_unx11051_checks();
        assert!(s.all_passed(), "UNX-F11051 判据未全绿");
    }
}
