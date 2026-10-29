//! UNX-F12280 文件族集成账（0x64a00 段）
//!
//! 卡面：B14 文件族集成收口——子系统对签账：每个签入记录须由两个不同
//! 子系统互签（不得自签），签入对不得重复；全部计划签入对齐完毕方可
//! seal；未知子系统必拒。
//! 域：CoRun·UNX-D1-B14。
//!
//! 判据原文：
//! - J1 子系统闭集：未知子系统号必拒。
//! - J2 不得自签：两侧同号必拒。
//! - J3 签入对唯一：同对重复签入必拒。
//! - J4 对向等价：(A,B) 与 (B,A) 视为同对拒。
//! - J5 未齐不可 seal：计划签入未对齐印账必拒。
//! - J6 印账冻结：seal 后签入必拒。
//! - J7 摘要可回读。
//! - J8 指纹幂等。
//! - J9 注入五况全抓。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64a00 独占 0x20）。
pub const SEG_BASE: u32 = 0x64a00;
pub const IG_UNKNOWN: u32 = SEG_BASE + 0x01; // 子系统越闭集
pub const IG_SELF: u32 = SEG_BASE + 0x02; // 自签
pub const IG_DUP: u32 = SEG_BASE + 0x03; // 签入对重号
pub const IG_UNSIGNED: u32 = SEG_BASE + 0x04; // 未齐印账
pub const IG_FROZEN: u32 = SEG_BASE + 0x05; // 印账后签入

/// 子系统闭集（文件族集成面）。
pub const SS_VFS: u8 = 1;
pub const SS_HANDLE: u8 = 2;
pub const SS_IOSB: u8 = 3;
pub const SS_CANCEL: u8 = 4;
pub const SS_CACHE: u8 = 5;
pub const SS_MSYNC: u8 = 6;

fn ss_known(s: u8) -> bool {
    (SS_VFS..=SS_MSYNC).contains(&s)
}

/// 计划签入对（批内收口清单）：6 对全覆盖才算齐。
const PLAN_PAIRS: [(u8, u8); 6] = [
    (SS_VFS, SS_HANDLE),
    (SS_HANDLE, SS_IOSB),
    (SS_IOSB, SS_CANCEL),
    (SS_CANCEL, SS_CACHE),
    (SS_CACHE, SS_MSYNC),
    (SS_MSYNC, SS_VFS),
];

/// 一条签入记录。
#[derive(Clone)]
pub struct SignRec {
    pub a: u8,
    pub b: u8,
}

impl SignRec {
    pub fn render(&self) -> String {
        format!("{}+{}", self.a, self.b)
    }
}

/// 集成账册。
pub struct IntegBook {
    pub recs: Vec<SignRec>,
    pub sealed: bool,
}

impl IntegBook {
    pub fn new() -> IntegBook {
        IntegBook { recs: Vec::new(), sealed: false }
    }

    /// 签入对是否已在册（对向等价）。
    fn has_pair(&self, a: u8, b: u8) -> bool {
        self.recs
            .iter()
            .any(|r| (r.a == a && r.b == b) || (r.a == b && r.b == a))
    }

    /// 签入：闭集、不自签、对唯一（对向等价拒）。
    pub fn sign(&mut self, a: u8, b: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(IG_FROZEN);
        }
        if !ss_known(a) || !ss_known(b) {
            return Err(IG_UNKNOWN);
        }
        if a == b {
            return Err(IG_SELF);
        }
        if self.has_pair(a, b) {
            return Err(IG_DUP);
        }
        self.recs.push(SignRec { a, b });
        Ok(())
    }

    /// 计划齐套检查。
    pub fn aligned(&self) -> bool {
        PLAN_PAIRS.iter().all(|(a, b)| self.has_pair(*a, *b))
    }

    pub fn seal(&mut self) -> Result<u32, u32> {
        if !self.aligned() {
            return Err(IG_UNSIGNED);
        }
        self.sealed = true;
        Ok(self.fold())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.a as u32);
            h = h.wrapping_mul(211).wrapping_add(r.b as u32);
        }
        h
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("pairs={}/{} aligned={}", self.recs.len(), PLAN_PAIRS.len(), self.aligned() as u8)
    }

    pub fn line_of(&self, a: u8, b: u8) -> String {
        if self.has_pair(a, b) {
            format!("{}+{}", a.min(b), a.max(b))
        } else {
            "missing".to_string()
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = IntegBook::new();
    // 1) 未知子系统必拒。
    got[0] = b.sign(0, SS_VFS).err() == Some(IG_UNKNOWN);
    // 2) 自签必拒。
    got[1] = b.sign(SS_VFS, SS_VFS).err() == Some(IG_SELF);
    // 3) 对向等价必拒。
    let _ = b.sign(SS_VFS, SS_HANDLE);
    got[2] = b.sign(SS_HANDLE, SS_VFS).err() == Some(IG_DUP);
    // 4) 未齐 seal 必拒。
    got[3] = b.seal().err() == Some(IG_UNSIGNED);
    // 5) 印账后签入必抓。
    for (a, c) in PLAN_PAIRS {
        let _ = b.sign(a, c);
    }
    let _ = b.seal();
    got[4] = b.sign(SS_VFS, SS_IOSB).err() == Some(IG_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12280_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12280");

    // J1 子系统闭集。
    let j1 = {
        let mut ok = true;
        for ss in SS_VFS..=SS_MSYNC {
            if !ss_known(ss) {
                ok = false;
            }
        }
        ok && !ss_known(0) && !ss_known(7)
    };
    s.add("J1-子系统闭集", j1, "1..6 在册");

    // J2 不得自签。
    let j2 = {
        let mut b = IntegBook::new();
        b.sign(SS_CACHE, SS_CACHE).err() == Some(IG_SELF)
    };
    s.add("J2-不得自签", j2, "同号必拒");

    // J3 签入对唯一。
    let j3 = {
        let mut b = IntegBook::new();
        let _ = b.sign(SS_VFS, SS_HANDLE);
        b.sign(SS_VFS, SS_HANDLE).err() == Some(IG_DUP)
    };
    s.add("J3-签入对唯一", j3, "同对重签拒");

    // J4 对向等价。
    let j4 = {
        let mut b = IntegBook::new();
        let _ = b.sign(SS_HANDLE, SS_IOSB);
        b.sign(SS_IOSB, SS_HANDLE).err() == Some(IG_DUP)
    };
    s.add("J4-对向等价", j4, "(A,B)=(B,A)");

    // J5 齐套判定。
    let j5 = {
        let mut b = IntegBook::new();
        let before = b.aligned();
        for (a, c) in PLAN_PAIRS {
            let _ = b.sign(a, c);
        }
        !before && b.aligned()
    };
    s.add("J5-齐套判定", j5, "六对齐才算齐");

    // J6 未齐 seal 拒。
    let j6 = {
        let mut b = IntegBook::new();
        let _ = b.sign(SS_VFS, SS_HANDLE);
        b.seal().err() == Some(IG_UNSIGNED)
    };
    s.add("J6-未齐seal拒", j6, "缺对必拒");

    // J7 齐套 seal 成功。
    let j7 = {
        let mut b = IntegBook::new();
        for (a, c) in PLAN_PAIRS {
            let _ = b.sign(a, c);
        }
        b.seal().is_ok() && b.summary().contains("aligned=1")
    };
    s.add("J7-齐套seal", j7, "全对齐印账");

    // J8 印账冻结。
    let j8 = {
        let mut b = IntegBook::new();
        for (a, c) in PLAN_PAIRS {
            let _ = b.sign(a, c);
        }
        let _ = b.seal();
        b.sign(SS_VFS, SS_IOSB).err() == Some(IG_FROZEN)
    };
    s.add("J8-印账冻结", j8, "seal 后必拒");

    // J9 指纹幂等。
    let j9 = {
        let mut b = IntegBook::new();
        let _ = b.sign(SS_VFS, SS_HANDLE);
        b.fold() == b.fold()
    };
    s.add("J9-指纹幂等", j9, "同态同指纹");

    // J10 回读渲染。
    let j10 = {
        let mut b = IntegBook::new();
        let _ = b.sign(SS_IOSB, SS_VFS);
        b.line_of(SS_VFS, SS_IOSB) == "3+1" && b.line_of(SS_VFS, SS_CACHE) == "missing"
    };
    s.add("J10-回读渲染", j10, "签入行可渲染");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = IntegBook::new();
    let _ = b.sign(SS_VFS, SS_HANDLE);
    let _ = b.sign(SS_HANDLE, SS_IOSB);
    s.add("摘要回读", b.summary().contains("pairs=2"), "双对在册");
    s.add("未齐显性", b.summary().contains("aligned=0"), "缺对如实报");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12280_sign_flow() {
        let mut b = IntegBook::new();
        assert!(b.sign(SS_VFS, SS_HANDLE).is_ok());
        assert_eq!(b.sign(SS_HANDLE, SS_VFS), Err(IG_DUP));
        assert_eq!(b.sign(SS_MSYNC, SS_MSYNC), Err(IG_SELF));
        assert_eq!(b.sign(9, SS_VFS), Err(IG_UNKNOWN));
        assert_eq!(b.seal(), Err(IG_UNSIGNED));
    }

    #[test]
    fn unx12280_checks_green() {
        let s = run_unx12280_checks();
        assert!(s.all_passed(), "UNX-F12280 判据未全绿");
    }

    #[test]
    fn unx12280_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
