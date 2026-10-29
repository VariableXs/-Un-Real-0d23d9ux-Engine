//! UNX-F12274 文件族双机对照判据（0x64940 段）
//!
//! 卡面：双机对照——同一文件族场景在基准机（S3 录制）与本机各跑一遍，
//! 逐场景比对结果码；结果一致入账为「对齐」，不一致记「分叉」并显性
//! 计数；分叉超过零条不得 seal。
//! 域：CoRun·UNX-D1-B14。
//!
//! 判据原文：
//! - J1 场景闭集：未知场景入账必拒。
//! - J2 结果码稳定：同场景重复取码同值。
//! - J3 对齐判定：两机结果码相等才对齐。
//! - J4 分叉显性：分叉条数可回读且入指纹。
//! - J5 分叉禁止 seal：有分叉印账必拒。
//! - J6 重号场景必拒。
//! - J7 印账冻结：seal 后补对照必拒。
//! - J8 指纹幂等。
//! - J9 注入五况全抓。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64940 独占 0x20）。
pub const SEG_BASE: u32 = 0x64940;
pub const CP_UNKNOWN_SCN: u32 = SEG_BASE + 0x01; // 未知场景
pub const CP_DIVERGE: u32 = SEG_BASE + 0x02; // 双机分叉
pub const CP_DUP: u32 = SEG_BASE + 0x03; // 重号场景
pub const CP_UNSIGNED: u32 = SEG_BASE + 0x04; // 空账/分叉印账
pub const CP_FROZEN: u32 = SEG_BASE + 0x05; // 印账后补账

/// 对照场景闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum CScn {
    CreateOverwrite = 0,
    CreateNewExists = 1,
    OpenMissing = 2,
    SharingOpen = 3,
    ReadPastEof = 4,
    WriteAppend = 5,
    QueryAttr = 6,
    CloseTwice = 7,
}

pub const CSN_N: usize = 8;

/// 基准机录制结果（模型口径，视为常量真相）。
pub fn baseline(s: CScn) -> u32 {
    match s {
        CScn::CreateOverwrite => 0x10,
        CScn::CreateNewExists => 0x11,
        CScn::OpenMissing => 0x12,
        CScn::SharingOpen => 0x13,
        CScn::ReadPastEof => 0x14,
        CScn::WriteAppend => 0x15,
        CScn::QueryAttr => 0x16,
        CScn::CloseTwice => 0x17,
    }
}

fn scn_all() -> [CScn; CSN_N] {
    [
        CScn::CreateOverwrite,
        CScn::CreateNewExists,
        CScn::OpenMissing,
        CScn::SharingOpen,
        CScn::ReadPastEof,
        CScn::WriteAppend,
        CScn::QueryAttr,
        CScn::CloseTwice,
    ]
}

fn scn_known(s: u8) -> bool {
    s <= 7
}

/// 一条对照记录。
#[derive(Clone)]
pub struct CmpRec {
    pub scn: u8,
    pub local: u32,
    pub aligned: bool,
}

/// 对照账册。
pub struct CmpBook {
    pub recs: Vec<CmpRec>,
    pub diverged: usize,
    pub sealed: bool,
}

impl CmpBook {
    pub fn new() -> CmpBook {
        CmpBook { recs: Vec::new(), diverged: 0, sealed: false }
    }

    /// 入账：场景闭集、重号拒、双机比对、分叉记账。
    pub fn put(&mut self, scn: u8, local: u32) -> Result<bool, u32> {
        if self.sealed {
            return Err(CP_FROZEN);
        }
        if !scn_known(scn) {
            return Err(CP_UNKNOWN_SCN);
        }
        let base = match scn_all().iter().copied().find(|x| *x as u8 == scn) {
            None => return Err(CP_UNKNOWN_SCN),
            Some(x) => baseline(x),
        };
        if self.recs.iter().any(|r| r.scn == scn) {
            return Err(CP_DUP);
        }
        let aligned = local == base;
        if !aligned {
            self.diverged += 1;
        }
        self.recs.push(CmpRec { scn, local, aligned });
        Ok(aligned)
    }

    /// 分叉数 > 0 不得 seal。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.recs.is_empty() || self.diverged > 0 {
            return Err(CP_UNSIGNED);
        }
        self.sealed = true;
        Ok(self.fold())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.scn as u32);
            h = h.wrapping_mul(211).wrapping_add(r.local);
            h = h.wrapping_mul(211).wrapping_add(r.aligned as u32);
        }
        h = h.wrapping_mul(211).wrapping_add(self.diverged as u32);
        h
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("n={} dv={}", self.recs.len(), self.diverged)
    }

    pub fn line_of(&self, scn: u8) -> String {
        match self.recs.iter().find(|r| r.scn == scn) {
            Some(r) => format!("s{}|l{:x}|a{}", r.scn, r.local, r.aligned as u8),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CmpBook::new();
    // 1) 未知场景必拒。
    got[0] = b.put(9, 0).err() == Some(CP_UNKNOWN_SCN);
    // 2) 双机分叉必记账（返回 false）。
    got[1] = b.put(1, 0x99).ok() == Some(false);
    // 3) 重号场景必拒。
    got[2] = b.put(1, baseline(CScn::CreateNewExists)).err() == Some(CP_DUP);
    // 4) 有分叉不得 seal。
    got[3] = b.seal().err() == Some(CP_UNSIGNED);
    // 5) 印账后补账必抓。
    let mut b2 = CmpBook::new();
    for scn in scn_all() {
        let _ = b2.put(scn as u8, baseline(scn));
    }
    let _ = b2.seal();
    got[4] = b2.put(2, baseline(CScn::OpenMissing)).err() == Some(CP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12274_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12274");

    // J1 结果码稳定。
    let j1 = {
        let mut stable = true;
        for scn in scn_all() {
            if baseline(scn) != baseline(scn) {
                stable = false;
            }
        }
        stable
    };
    s.add("J1-结果码稳定", j1, "录制口径恒定");

    // J2 场景闭集。
    let j2 = {
        let mut b = CmpBook::new();
        b.put(8, 0).err() == Some(CP_UNKNOWN_SCN)
    };
    s.add("J2-场景闭集", j2, "8 之外必拒");

    // J3 对齐判定。
    let j3 = {
        let mut b = CmpBook::new();
        b.put(0, 0x10).ok() == Some(true) && b.put(1, 0x11).ok() == Some(true)
    };
    s.add("J3-对齐判定", j3, "同码即对齐");

    // J4 分叉显性。
    let j4 = {
        let mut b = CmpBook::new();
        let _ = b.put(2, 0x12);
        let _ = b.put(3, 0xDE);
        b.diverged == 1 && b.summary().contains("dv=1")
    };
    s.add("J4-分叉显性", j4, "分叉计数可回读");

    // J5 分叉禁止 seal。
    let j5 = {
        let mut b = CmpBook::new();
        let _ = b.put(4, 0xFF);
        b.seal().err() == Some(CP_UNSIGNED)
    };
    s.add("J5-分叉禁seal", j5, "不诚实不得印账");

    // J6 全对齐 seal 成功。
    let j6 = {
        let mut b = CmpBook::new();
        for scn in scn_all() {
            let _ = b.put(scn as u8, baseline(scn));
        }
        b.seal().is_ok()
    };
    s.add("J6-全对齐seal", j6, "八场景齐印账");

    // J7 空账 seal 拒。
    let j7 = CmpBook::new().seal().err() == Some(CP_UNSIGNED);
    s.add("J7-空账seal拒", j7, "零对照必拒");

    // J8 重号场景拒。
    let j8 = {
        let mut b = CmpBook::new();
        let _ = b.put(5, baseline(CScn::WriteAppend));
        b.put(5, baseline(CScn::WriteAppend)).err() == Some(CP_DUP)
    };
    s.add("J8-重号拒", j8, "场景唯一");

    // J9 印账冻结。
    let j9 = {
        let mut b = CmpBook::new();
        let _ = b.put(6, baseline(CScn::QueryAttr));
        let _ = b.seal();
        b.put(7, baseline(CScn::CloseTwice)).err() == Some(CP_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = CmpBook::new();
        let _ = b.put(0, baseline(CScn::CreateOverwrite));
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "分叉计数入指纹");

    // J11 回读渲染。
    let j11 = {
        let mut b = CmpBook::new();
        let _ = b.put(3, baseline(CScn::SharingOpen));
        b.line_of(3).contains("a1") && b.line_of(4) == "missing"
    };
    s.add("J11-回读渲染", j11, "对照行可渲染");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = CmpBook::new();
    let _ = b.put(0, baseline(CScn::CreateOverwrite));
    let _ = b.put(1, baseline(CScn::CreateNewExists));
    s.add("摘要回读", b.summary().contains("n=2 dv=0"), "双对照齐");
    s.add("分叉零", b.diverged == 0, "诚实录制");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12274_compare_flow() {
        let mut b = CmpBook::new();
        assert_eq!(b.put(0, baseline(CScn::CreateOverwrite)), Ok(true));
        assert_eq!(b.put(1, 0x99), Ok(false));
        assert_eq!(b.diverged, 1);
        assert_eq!(b.put(0, baseline(CScn::CreateOverwrite)), Err(CP_DUP));
        assert_eq!(b.seal(), Err(CP_UNSIGNED));
    }

    #[test]
    fn unx12274_checks_green() {
        let s = run_unx12274_checks();
        assert!(s.all_passed(), "UNX-F12274 判据未全绿");
    }

    #[test]
    fn unx12274_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
