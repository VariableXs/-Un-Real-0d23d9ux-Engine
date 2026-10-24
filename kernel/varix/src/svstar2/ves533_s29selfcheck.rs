//! VE-F4299 · U05 预备自查（U05 工具集成组）
//! 职责：四维自查（同 F4279 口径：体例七要素/断言八条/差异化/指针）+
//! 本组特查：九面集成关系与工具链总图一致性——图实对拍 + 回修重扫
//! + 指针悬空修正 + 特查异议升级复核。
//! 段位：0x2F20（续表 idx533）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2F20;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum U5SkCode {
    DimFail = SEG_BASE,
    DanglingPtr = SEG_BASE + 0x01,
    MapDrift = SEG_BASE + 0x02, // 特查：图实漂移
}

/// U05 自查册（四维+特查）。
pub struct U05SelfCheck {
    pub elements: u8,
    pub asserts: u8,
    pub ptrs: Vec<(u32, bool)>,
    /// 图实对拍：图面数 vs 实际面数。
    pub map_faces: usize,
    pub real_faces: usize,
    pub scans: u32,
    pub spots: Vec<(u16, bool)>,
}

impl U05SelfCheck {
    pub fn new(map_faces: usize, real_faces: usize) -> Self {
        U05SelfCheck { elements: 0, asserts: 0, ptrs: Vec::new(), map_faces, real_faces, scans: 0, spots: Vec::new() }
    }

    pub fn scan(&mut self) -> [u8; 4] {
        self.scans += 1;
        let d0 = (self.elements.min(7) * 100 / 7) as u8;
        let d1 = (self.asserts.min(8) * 100 / 8) as u8;
        let d2 = if d0 == 100 && d1 == 100 { 100 } else { 50 };
        let d3 = if self.map_faces == self.real_faces && self.ptrs.iter().all(|(_, ok)| *ok) && !self.ptrs.is_empty() { 100 } else { 0 };
        [d0, d1, d2, d3]
    }

    pub fn fix_ptr(&mut self, f: u32) -> bool {
        if let Some(p) = self.ptrs.iter_mut().find(|(pf, ok)| *pf == f && !ok) {
            p.1 = true;
            return true;
        }
        false
    }

    pub fn map_drift(&self) -> bool {
        self.map_faces != self.real_faces
    }

    pub fn spot(&mut self, s: u16, dispute: bool) {
        self.spots.push((s, dispute));
    }

    pub fn disputes(&self) -> Vec<u16> {
        self.spots.iter().filter(|(_, d)| *d).map(|(s, _)| *s).collect()
    }

    pub fn ready(&self) -> bool {
        !self.map_drift() && self.ptrs.iter().all(|(_, ok)| *ok) && !self.ptrs.is_empty()
            && self.disputes().is_empty()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C533-01..12。
pub fn run_ves533_checks() -> CheckSet {
    let mut set = CheckSet::new("ves533");

    let mut sk = U05SelfCheck::new(7, 9);
    chk(&mut set, "C533-01", sk.map_drift(), "特查：图实漂移检出（7≠9）");
    sk.map_faces = 9;
    sk.register_dummy();
    chk(&mut set, "C533-02", !sk.map_drift(), "图实对齐");

    sk.elements = 5;
    sk.asserts = 8;
    let dims = sk.scan();
    chk(&mut set, "C533-03", dims[0] == 71 && dims[1] == 100, "七要素缺 2、八断言满");
    sk.elements = 7;
    sk.scan();
    chk(&mut set, "C533-04", sk.scans == 2, "回修重扫计数");

    // 指针
    sk.ptrs.push((4270, true));
    sk.ptrs.push((4281, false));
    chk(&mut set, "C533-05", !sk.ready(), "悬空指针不放行");
    chk(&mut set, "C533-06", sk.fix_ptr(4281), "修正成功");
    chk(&mut set, "C533-07", !sk.fix_ptr(4281), "重复修正拒绝");
    chk(&mut set, "C533-08", sk.ready(), "全绿可进双签");

    // 抽查
    sk.spot(1, true);
    chk(&mut set, "C533-09", sk.disputes() == alloc::vec![1] && !sk.ready(), "特查异议不放行");
    sk.spots.clear();
    sk.spot(1, false);
    chk(&mut set, "C533-10", sk.ready(), "异议复核后放行");

    let mut empty = U05SelfCheck::new(9, 9);
    chk(&mut set, "C533-11", !empty.ready(), "零指针保守不放行");
    chk(&mut set, "C533-12", empty.scan()[3] == 0, "零指针维度 0");

    set
}

impl U05SelfCheck {
    fn register_dummy(&mut self) {
        if self.ptrs.is_empty() {
            self.ptrs.push((4207, true));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u05_selfcheck() {
        let mut sk = U05SelfCheck::new(9, 9);
        sk.ptrs.push((1, true));
        sk.scan();
        assert!(sk.ready());
        sk.map_faces = 3;
        assert!(!sk.ready());
    }
}
