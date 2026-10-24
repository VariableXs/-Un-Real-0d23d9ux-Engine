//! VE-F4279 · U04 预备自查（U04 治理组）
//! 职责：家族预备自查模板第十用（同域后续组沿用不递增）+ 四维自查
//! （体例七要素/断言八条/差异化比对/跨批指针有效）+ 双轨扫描
//! （程序化全量+人工抽查）+ 指针悬空修正 + 回修重扫。
//! 段位：0x2920（续表 idx513）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2920;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkCode {
    /// 维度不达标（回修重扫）。
    DimFail = SEG_BASE,
    /// 指针悬空。
    DanglingPtr = SEG_BASE + 0x01,
    /// 抽查异议（升级复核）。
    SpotDispute = SEG_BASE + 0x02,
}

/// 四维。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkDim {
    SevenElements = 0, // 体例七要素
    EightAsserts = 1,  // 断言八条
    DiffCompare = 2,   // 差异化比对
    PtrValid = 3,      // 跨批指针
}
pub const SKDIM_N: usize = 4;

pub const SEVEN: u8 = 7;
pub const EIGHT: u8 = 8;

/// 自查册。
pub struct U04SelfCheck {
    /// 各维完成度（0-100）。
    pub dims: [u8; SKDIM_N],
    /// 要素/断言实际计数。
    pub elements: u8,
    pub asserts: u8,
    /// 跨批指针：(目标 F 号, 有效)。
    pub ptrs: Vec<(u32, bool)>,
    /// 程序化扫描次数。
    pub scans: u32,
    /// 人工抽查：(样本, 异议)。
    pub spots: Vec<(u16, bool)>,
}

impl U04SelfCheck {
    pub fn new() -> Self {
        U04SelfCheck { dims: [0; SKDIM_N], elements: 0, asserts: 0, ptrs: Vec::new(), scans: 0, spots: Vec::new() }
    }

    /// 全量程序化扫描：按四维算完成度。
    pub fn scan(&mut self) {
        self.scans += 1;
        self.dims[0] = (self.elements.min(SEVEN) * 100 / SEVEN) as u8;
        self.dims[1] = (self.asserts.min(EIGHT) * 100 / EIGHT) as u8;
        let diff_ok = self.dims[0] == 100 && self.dims[1] == 100;
        self.dims[2] = if diff_ok { 100 } else { 50 };
        let ptr_ok = !self.ptrs.is_empty() && self.ptrs.iter().all(|(_, ok)| *ok);
        self.dims[3] = if ptr_ok { 100 } else { 0 };
    }

    /// 四维达标。
    pub fn all_dims_ok(&self) -> bool {
        self.dims.iter().all(|d| *d == 100)
    }

    /// 指针登记（悬空检出）。
    pub fn register_ptr(&mut self, f: u32, valid: bool) {
        self.ptrs.push((f, valid));
    }

    /// 悬空指针修正。
    pub fn fix_ptr(&mut self, f: u32) -> bool {
        if let Some(p) = self.ptrs.iter_mut().find(|(pf, ok)| *pf == f && !ok) {
            p.1 = true;
            return true;
        }
        false
    }

    /// 人工抽查。
    pub fn spot(&mut self, sample: u16, dispute: bool) {
        self.spots.push((sample, dispute));
    }

    /// 抽查异议升级复核。
    pub fn disputes(&self) -> Vec<u16> {
        self.spots.iter().filter(|(_, d)| *d).map(|(s, _)| *s).collect()
    }

    /// 可进双签：四维全绿+零悬空+异议已复核。
    pub fn ready_for_close(&self) -> bool {
        self.all_dims_ok() && self.ptrs.iter().all(|(_, ok)| *ok) && self.disputes().is_empty()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C513-01..12。
pub fn run_ves513_checks() -> CheckSet {
    let mut set = CheckSet::new("ves513");

    let mut sk = U04SelfCheck::new();
    sk.scan();
    chk(&mut set, "C513-01", !sk.all_dims_ok(), "空册扫描不达标");
    chk(&mut set, "C513-02", sk.dims[3] == 0, "零指针维度 0");

    sk.elements = 5;
    sk.asserts = 8;
    sk.scan();
    chk(&mut set, "C513-03", sk.dims[0] == 71, "七要素缺 2 → 71%");
    chk(&mut set, "C513-04", sk.dims[1] == 100 && sk.dims[2] == 50, "八断言满+差异化被前维拖低");

    sk.elements = 7;
    sk.scan();
    chk(&mut set, "C513-05", sk.dims == [100, 100, 100, 0], "前三维补齐、指针维仍 0");

    // 指针
    sk.register_ptr(4207, true);
    sk.register_ptr(4281, false);
    sk.scan();
    chk(&mut set, "C513-06", sk.dims[3] == 0, "悬空指针拖低指针维");
    chk(&mut set, "C513-07", sk.fix_ptr(4281), "悬空修正成功");
    chk(&mut set, "C513-08", !sk.fix_ptr(4281), "重复修正拒绝");
    sk.scan();
    chk(&mut set, "C513-09", sk.all_dims_ok(), "回修重扫四维全绿");
    chk(&mut set, "C513-10", sk.scans == 3, "扫描计数");

    // 抽查
    sk.spot(1, false);
    sk.spot(2, true);
    chk(&mut set, "C513-11", sk.disputes() == alloc::vec![2] && !sk.ready_for_close(), "抽查异议升级复核未清不放行");
    sk.spots.clear();
    sk.spot(2, false);
    chk(&mut set, "C513-12", sk.ready_for_close(), "异议复核后可进双签");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u04_selfcheck() {
        let mut sk = U04SelfCheck::new();
        sk.elements = 7;
        sk.asserts = 8;
        sk.register_ptr(1, true);
        sk.scan();
        assert!(sk.all_dims_ok());
        assert!(sk.ready_for_close());
    }
}
