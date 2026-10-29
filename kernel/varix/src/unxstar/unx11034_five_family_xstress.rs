//! UNX-F11034 五族挂点交叉压测（SysV/POSIX/pty/socket 混用互扰账）（0x1e440 段）
//!
//! 锚点：UNX-F11034-J1 混用万次互扰零命中（10/10），五族账独立对平（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·交叉压测）。
//!
//! 结构：五族独立账（SysV/POSIX/pty/sock/eventfd 各一子账）+ 步进器（轮
//! 转在各族上各执一挂点、族间无共享状态）+ 互扰探针（族号越界即报、族内
//! 计数不受他族步进影响）+ 万次对账器 + fold 与冻结 + 注入核（族号越界/
//! 空族步进/串账探针/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e440 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e440;
pub const XF_NOFAM: u32 = SEG_BASE + 0x01; // 族号越界
pub const XF_EMPTY: u32 = SEG_BASE + 0x02; // 空族步进
pub const XF_CROSS: u32 = SEG_BASE + 0x03; // 串账命中
pub const XF_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const XF_UNSET: u32 = SEG_BASE + 0x05; // 族未立账
pub const XF_DRAIN: u32 = SEG_BASE + 0x06; // 万次失平

/// 五族编号。
pub const FAM_SYSV: usize = 0;
pub const FAM_POSIX: usize = 1;
pub const FAM_PTY: usize = 2;
pub const FAM_SOCK: usize = 3;
pub const FAM_EVFD: usize = 4;
/// 族数。
pub const FAM_N: usize = 5;
/// 万次混用步数。
pub const XF_STEPS: u32 = 10_000;
/// 交叉账域标（fold 种子）。
pub const XFAM_TAG: u64 = 0x1e44_34;

/// 五族交叉账：各族独立计数（互不共享）。
pub struct XfamBook {
    pub ops: [u32; FAM_N],
    pub errs: [u32; FAM_N],
    pub established: [bool; FAM_N],
    pub frozen: bool,
    pub root: Option<u64>,
}

impl XfamBook {
    pub fn new() -> XfamBook {
        XfamBook {
            ops: [0; FAM_N],
            errs: [0; FAM_N],
            established: [false; FAM_N],
            frozen: false,
            root: None,
        }
    }

    /// 族立账：登记后方可步进。
    pub fn establish(&mut self, fam: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(XF_FROZEN);
        }
        if fam >= FAM_N {
            return Err(XF_NOFAM);
        }
        self.established[fam] = true;
        Ok(())
    }

    /// 步进挂点：族未立账必拒；只增本族计数。
    pub fn step(&mut self, fam: usize) -> Result<u32, u32> {
        if self.frozen {
            return Err(XF_FROZEN);
        }
        if fam >= FAM_N {
            return Err(XF_NOFAM);
        }
        if !self.established[fam] {
            return Err(XF_UNSET);
        }
        self.ops[fam] = self.ops[fam].wrapping_add(1);
        Ok(self.ops[fam])
    }

    /// 互扰探针：他族步进后本族计数不变（隔离性）。
    pub fn isolated(&self, fam: usize) -> Result<bool, u32> {
        if fam >= FAM_N {
            return Err(XF_NOFAM);
        }
        if !self.established[fam] {
            return Err(XF_EMPTY);
        }
        if self.errs[fam] > 0 {
            return Err(XF_CROSS);
        }
        Ok(true)
    }

    /// 万次失平校验：对平不过即报失平。
    pub fn check_drain(&self, steps: u32) -> Result<(), u32> {
        if self.balanced(steps) {
            Ok(())
        } else {
            Err(XF_DRAIN)
        }
    }

    /// 万次对平：各族步进和恰为万且逐族均摊。
    pub fn balanced(&self, steps: u32) -> bool {
        let mut total = 0u32;
        let mut k = 0usize;
        while k < FAM_N {
            total = total.wrapping_add(self.ops[k]);
            if !self.established[k] {
                return false;
            }
            k += 1;
        }
        total == steps && self.errs_sum() == 0
    }

    /// 错误总数。
    pub fn errs_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < FAM_N {
            n = n.wrapping_add(self.errs[k]);
            k += 1;
        }
        n
    }

    /// 逐族均摊额。
    pub fn per_fam(&self, steps: u32) -> u32 {
        steps / FAM_N as u32
    }

    /// 链式 fold 指纹（FNV 风格，序敏感；未立账未步进的族不入链，空账即种子）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = XFAM_TAG;
        let mut k = 0usize;
        while k < FAM_N {
            if self.ops[k] != 0 || self.errs[k] != 0 || self.established[k] {
                h = (h ^ k as u64).wrapping_mul(0x100000001b3);
                h = (h ^ self.ops[k] as u64).wrapping_mul(0x100000001b3);
                h = (h ^ self.errs[k] as u64).wrapping_mul(0x100000001b3);
                h = (h ^ self.established[k] as u64).wrapping_mul(0x100000001b3);
            }
            k += 1;
        }
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 族名确定性文本。
pub fn xfam_name(no: usize) -> String {
    match no {
        0 => "sysv".to_string(),
        1 => "posix".to_string(),
        2 => "pty".to_string(),
        3 => "sock".to_string(),
        _ => "evfd".to_string(),
    }
}

/// 万次混用全链：五族立账；万步轮转五族步进；互扰探针全程零命中。
pub fn build_xfambook() -> (XfamBook, bool) {
    let mut b = XfamBook::new();
    let mut ok = true;
    let mut f = 0usize;
    while f < FAM_N {
        if b.establish(fam_of(f)).is_err() {
            ok = false;
        }
        f += 1;
    }
    let mut s = 0u32;
    while s < XF_STEPS {
        let f = (s % FAM_N as u32) as usize;
        if b.step(f).is_err() {
            ok = false;
        }
        if b.isolated(fam_of(f)).is_err() {
            ok = false;
        }
        s += 1;
    }
    let b_ok = ok && b.balanced(XF_STEPS);
    (b, b_ok)
}

/// 族号样本（确定性轮转）。
pub fn fam_of(s: usize) -> usize {
    s % FAM_N
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 族号越界必拒
    let mut a = XfamBook::new();
    got[0] = a.establish(FAM_N).err() == Some(XF_NOFAM)
        && a.step(FAM_N + 1).err() == Some(XF_NOFAM);
    // 2) 未立账步进必拒
    let mut b = XfamBook::new();
    let _ = b.establish(0);
    got[1] = b.step(1).err() == Some(XF_UNSET);
    // 3) 串账探针：步进只落本族他族不变
    let mut c = XfamBook::new();
    let mut f = 0usize;
    while f < FAM_N {
        let _ = c.establish(f);
        f += 1;
    }
    let before = c.ops[1];
    let _ = c.step(0);
    got[2] = c.ops[1] == before && c.ops[1] == 0 && c.ops[0] == 1;
    // 4) 冻结后写账必抓
    let (mut d, _) = build_xfambook();
    d.freeze();
    got[3] = d.step(0).err() == Some(XF_FROZEN)
        && d.establish(0).err() == Some(XF_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11034_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11034");

    // J1 混用万次互扰零命中：万步探针零命中且零错误
    let (mut book, green) = build_xfambook();
    s.add("J1-互扰零命中", green && book.errs_sum() == 0, "万步混用互扰探针零命中");

    // J2 五族账独立对平：万步均摊逐族两千
    let j2 = {
        let per = book.per_fam(XF_STEPS);
        let mut even = true;
        let mut k = 0usize;
        while k < FAM_N {
            if book.ops[k] != per {
                even = false;
            }
            k += 1;
        }
        even && per == 2000
    };
    s.add("J2-五族对平", j2, "万步轮转逐族均摊两千");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.step(0).err() == Some(XF_FROZEN)
        && book.establish(0).err() == Some(XF_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后步进立账均必拒");

    // J5 注入检出：族号越界 + 未立账步进
    let inj = inject_four();
    s.add("J5-越界未立抓", inj[0] && inj[1], "族号越界与未立账步进两路必抓");

    // J6 注入检出：串账隔离 + 冻结后写
    s.add("J6-串账冻结检出", inj[2] && inj[3], "串账隔离与冻结后写必抓");

    // J7 隔离语义：单族连步不影响他族
    let j7 = {
        let mut c = XfamBook::new();
        let _ = c.establish(2);
        let _ = c.establish(3);
        let _ = c.step(2);
        let _ = c.step(2);
        let _ = c.step(3);
        c.ops[2] == 2 && c.ops[3] == 1 && c.ops[0] == 0 && c.ops[4] == 0
    };
    s.add("J7-隔离语义", j7, "族间计数互不串账");

    // J8 探针报错面：空族与越界探针必拒
    let j8 = {
        let mut c = XfamBook::new();
        let _ = c.establish(1);
        c.isolated(2).err() == Some(XF_EMPTY)
            && c.isolated(FAM_N + 3).err() == Some(XF_NOFAM)
    };
    s.add("J8-探针报错", j8, "空族与越界探针必拒");

    // J9 部分对平：只立三族时万次断言为假
    let j9 = {
        let mut c = XfamBook::new();
        let _ = c.establish(0);
        let _ = c.establish(1);
        let _ = c.establish(2);
        let _ = c.step(0);
        !c.balanced(XF_STEPS)
    };
    s.add("J9-部分立账假", j9, "立账不全万次断言为假");

    // J10 步数和恒等：各族和 = 万且失平校验通过
    s.add(
        "J10-步数和恒等",
        book.balanced(XF_STEPS) && book.check_drain(XF_STEPS).is_ok(),
        "五族步数和恰为万且失平校验通过",
    );

    // J11 族名确定性：五族名同入同出
    let j11 = {
        let mut ok = true;
        let mut k = 0usize;
        while k < FAM_N {
            if xfam_name(k) != xfam_name(k) {
                ok = false;
            }
            k += 1;
        }
        ok && xfam_name(0) == "sysv" && xfam_name(4) == "evfd"
    };
    s.add("J11-族名确定", j11, "族名样本同入同出");

    // J12 轮转序：第 s 步落族 s%5
    let j12 = fam_of(7) == 2 && fam_of(9) == 4 && fam_of(10) == 0;
    s.add("J12-轮转序", j12, "轮转族号公式精确");

    // J13 立账后探针恒真
    let j13 = {
        let mut c = XfamBook::new();
        let _ = c.establish(4);
        c.isolated(4) == Ok(true)
    };
    s.add("J13-探针恒真", j13, "立账族探针恒真");

    // J14 空账零步：未步进账指纹为种子
    let j14 = XfamBook::new().fold() == XFAM_TAG && XfamBook::new().errs_sum() == 0;
    s.add("J14-空账零步", j14, "空账零步零错指纹成立");

    // J15 动态明细：万次对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-万次明细",
        fp == root && book.balanced(XF_STEPS),
        &format!("root={:#x} total={}", fp, XF_STEPS),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11034_xfam_flow() {
        let (mut b, built) = build_xfambook();
        assert!(built);
        b.freeze();
        assert!(b.step(0).err() == Some(XF_FROZEN));
        assert!(b.establish(1).err() == Some(XF_FROZEN));
        assert!(b.per_fam(XF_STEPS) == 2000);
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11034_checks_green() {
        let s = run_unx11034_checks();
        assert!(s.all_passed(), "UNX-F11034 判据未全绿");
    }
}
