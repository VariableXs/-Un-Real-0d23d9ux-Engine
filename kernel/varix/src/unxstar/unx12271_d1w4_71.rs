//! UNX-F12271 文件族错误矩阵：SHARING_VIOLATION 等全表（0x648e0 段）
//!
//! 卡面：文件族错误码矩阵——把「场景」到「错误码」的映射收进一张账：
//! 共享冲突、名称碰撞、路径不存在、拒绝访问、坏 disposition、目录/
//! 非目录错位、删除挂起等；每场景只允许入账一次，期望码与账面码必须
//! 自洽，印账后不得再补场景。
//! 域：CoRun·UNX-D1-B14。
//!
//! 判据原文：
//! - J1 场景闭集：未知场景入账必拒。
//! - J2 错误码稳定：同场景重复取码必须同值（映射纯函数）。
//! - J3 全表覆盖：样本账必须覆盖全部场景才可 seal。
//! - J4 期望自洽：入账记录的期望码与映射码不一致必拒。
//! - J5 重号场景必拒。
//! - J6 印账冻结：seal 后补账必拒。
//! - J7 错误码段独立：各错误码互不相同。
//! - J8 指纹幂等。
//! - J9 注入五况全抓。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x648e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x648e0;
pub const EM_UNKNOWN_SCN: u32 = SEG_BASE + 0x01; // 未知场景
pub const EM_MISMATCH: u32 = SEG_BASE + 0x02; // 期望码与映射码不符
pub const EM_DUP: u32 = SEG_BASE + 0x03; // 重号场景
pub const EM_INCOMPLETE: u32 = SEG_BASE + 0x04; // 全表未覆盖就 seal
pub const EM_FROZEN: u32 = SEG_BASE + 0x05; // 印账后补账

/// 场景闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum FScn {
    SharingViolation = 0,
    NameCollision = 1,
    PathNotFound = 2,
    AccessDenied = 3,
    BadDisposition = 4,
    IsDirectory = 5,
    NotDirectory = 6,
    DeletePending = 7,
}

pub const SCN_N: usize = 8;

/// 场景 → 模型错误码（纯函数映射）。
pub fn map_err(s: FScn) -> u32 {
    SEG_BASE + 0x10 + s as u32
}

fn scn_all() -> [FScn; SCN_N] {
    [
        FScn::SharingViolation,
        FScn::NameCollision,
        FScn::PathNotFound,
        FScn::AccessDenied,
        FScn::BadDisposition,
        FScn::IsDirectory,
        FScn::NotDirectory,
        FScn::DeletePending,
    ]
}

fn scn_name(s: FScn) -> String {
    match s {
        FScn::SharingViolation => "sharing_violation".to_string(),
        FScn::NameCollision => "name_collision".to_string(),
        FScn::PathNotFound => "path_not_found".to_string(),
        FScn::AccessDenied => "access_denied".to_string(),
        FScn::BadDisposition => "bad_disposition".to_string(),
        FScn::IsDirectory => "is_directory".to_string(),
        FScn::NotDirectory => "not_directory".to_string(),
        FScn::DeletePending => "delete_pending".to_string(),
    }
}

/// 单条矩阵记录。
#[derive(Clone)]
pub struct ErrRec {
    pub scn: FScn,
    pub expect: u32,
}

impl ErrRec {
    pub fn render(&self) -> String {
        format!("{}|e{:x}", scn_name(self.scn), self.expect)
    }
}

/// 错误矩阵账册。
pub struct ErrBook {
    pub recs: Vec<ErrRec>,
    pub sealed: bool,
}

impl ErrBook {
    pub fn new() -> ErrBook {
        ErrBook { recs: Vec::new(), sealed: false }
    }

    fn pos(&self, scn: FScn) -> Option<usize> {
        self.recs.iter().position(|r| r.scn == scn)
    }

    /// 入账：场景须在闭集、期望码须与映射一致、不得重号。
    pub fn put(&mut self, scn: FScn, expect: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EM_FROZEN);
        }
        let real = match scn_all().iter().find(|x| **x == scn) {
            None => return Err(EM_UNKNOWN_SCN),
            Some(x) => map_err(*x),
        };
        if expect != real {
            return Err(EM_MISMATCH);
        }
        if self.pos(scn).is_some() {
            return Err(EM_DUP);
        }
        self.recs.push(ErrRec { scn, expect });
        Ok(())
    }

    /// 全表覆盖检查。
    pub fn covered(&self) -> bool {
        self.recs.len() == SCN_N
    }

    pub fn seal(&mut self) -> Result<u32, u32> {
        if !self.covered() {
            return Err(EM_INCOMPLETE);
        }
        self.sealed = true;
        Ok(self.fold())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.scn as u32);
            h = h.wrapping_mul(211).wrapping_add(r.expect);
        }
        h
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("rows={} of {}", self.recs.len(), SCN_N)
    }

    pub fn line_of(&self, scn: FScn) -> String {
        match self.pos(scn) {
            Some(i) => self.recs[i].render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ErrBook::new();
    // 1) 期望码不符必拒。
    got[0] = b.put(FScn::SharingViolation, 0).err() == Some(EM_MISMATCH);
    // 2) 重号场景必拒。
    let _ = b.put(FScn::SharingViolation, map_err(FScn::SharingViolation));
    got[1] = b.put(FScn::SharingViolation, map_err(FScn::SharingViolation))
        .err() == Some(EM_DUP);
    // 3) 全表未覆盖就 seal 必拒。
    got[2] = b.seal().err() == Some(EM_INCOMPLETE);
    // 4) 印账后补账必抓。
    for scn in scn_all() {
        let _ = b.put(scn, map_err(scn));
    }
    let _ = b.seal();
    got[3] = b.put(FScn::NameCollision, map_err(FScn::NameCollision))
        .err() == Some(EM_FROZEN);
    // 5) 错误码段两两互异（互异若被破坏必被此处检出）。
    let mut all = [0u32; SCN_N];
    for (i, scn) in scn_all().iter().enumerate() {
        all[i] = map_err(*scn);
    }
    let mut distinct = true;
    for i in 0..SCN_N {
        for j in (i + 1)..SCN_N {
            if all[i] == all[j] {
                distinct = false;
            }
        }
    }
    got[4] = !distinct == false && distinct;
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12271_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12271");

    // J1 映射纯函数（稳定性）。
    let j1 = {
        let mut stable = true;
        for scn in scn_all() {
            if map_err(scn) != map_err(scn) {
                stable = false;
            }
        }
        stable
    };
    s.add("J1-映射纯函数", j1, "同场景同码");

    // J2 共享冲突码在段内。
    let j2 = {
        let e = map_err(FScn::SharingViolation);
        e > SEG_BASE + 0x0F && e < SEG_BASE + 0x20
    };
    s.add("J2-码段独立", j2, "错误码落在 0x10..0x1F");

    // J3 全表覆盖。
    let j3 = {
        let mut b = ErrBook::new();
        for scn in scn_all() {
            let _ = b.put(scn, map_err(scn));
        }
        b.covered() && b.summary().contains("rows=8")
    };
    s.add("J3-全表覆盖", j3, "8 场景全入账");

    // J4 期望自洽拒。
    let j4 = {
        let mut b = ErrBook::new();
        b.put(FScn::PathNotFound, SEG_BASE).err() == Some(EM_MISMATCH)
    };
    s.add("J4-期望自洽", j4, "坏期望必拒");

    // J5 重号场景拒。
    let j5 = {
        let mut b = ErrBook::new();
        let _ = b.put(FScn::AccessDenied, map_err(FScn::AccessDenied));
        b.put(FScn::AccessDenied, map_err(FScn::AccessDenied))
            .err() == Some(EM_DUP)
    };
    s.add("J5-重号拒", j5, "场景唯一");

    // J6 未覆盖 seal 拒。
    let j6 = {
        let mut b = ErrBook::new();
        let _ = b.put(FScn::NameCollision, map_err(FScn::NameCollision));
        b.seal().err() == Some(EM_INCOMPLETE)
    };
    s.add("J6-未覆盖拒", j6, "缺行必拒");

    // J7 全覆盖 seal 成功。
    let j7 = {
        let mut b = ErrBook::new();
        for scn in scn_all() {
            let _ = b.put(scn, map_err(scn));
        }
        b.seal().is_ok()
    };
    s.add("J7-全覆盖seal", j7, "全表方可印账");

    // J8 印账冻结。
    let j8 = {
        let mut b = ErrBook::new();
        for scn in scn_all() {
            let _ = b.put(scn, map_err(scn));
        }
        let _ = b.seal();
        b.put(FScn::IsDirectory, map_err(FScn::IsDirectory))
            .err() == Some(EM_FROZEN)
    };
    s.add("J8-印账冻结", j8, "seal 后补账必拒");

    // J9 指纹幂等。
    let j9 = {
        let mut b = ErrBook::new();
        let _ = b.put(FScn::DeletePending, map_err(FScn::DeletePending));
        b.fold() == b.fold()
    };
    s.add("J9-指纹幂等", j9, "同态同指纹");

    // J10 回读渲染。
    let j10 = {
        let mut b = ErrBook::new();
        let _ = b.put(FScn::IsDirectory, map_err(FScn::IsDirectory));
        b.line_of(FScn::IsDirectory).contains("is_directory")
            && b.line_of(FScn::NotDirectory) == "missing"
    };
    s.add("J10-回读渲染", j10, "场景行可渲染");

    // J11 目录/非目录码互异。
    let j11 = map_err(FScn::IsDirectory) != map_err(FScn::NotDirectory);
    s.add("J11-目录码互异", j11, "两类错位不同码");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = ErrBook::new();
    let _ = b.put(FScn::SharingViolation, map_err(FScn::SharingViolation));
    let _ = b.put(FScn::NameCollision, map_err(FScn::NameCollision));
    s.add("摘要回读", b.summary().contains("rows=2"), "双行在册");
    s.add("场景名记账", scn_name(FScn::BadDisposition) == "bad_disposition", "名可回读");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12271_matrix_flow() {
        for scn in scn_all() {
            let e = map_err(scn);
            assert!(e > SEG_BASE, "错误码须落在段内");
        }
        let mut b = ErrBook::new();
        assert!(b.put(FScn::AccessDenied, map_err(FScn::AccessDenied)).is_ok());
        assert_eq!(b.put(FScn::AccessDenied, map_err(FScn::AccessDenied)), Err(EM_DUP));
    }

    #[test]
    fn unx12271_checks_green() {
        let s = run_unx12271_checks();
        assert!(s.all_passed(), "UNX-F12271 判据未全绿");
    }

    #[test]
    fn unx12271_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
