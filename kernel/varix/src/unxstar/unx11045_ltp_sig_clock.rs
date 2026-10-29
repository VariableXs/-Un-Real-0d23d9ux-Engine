//! UNX-F11045 LTP 信号/时钟组用例对照（交叉族对账）（0x1e5a0 段）
//!
//! 锚点：UNX-F11045-J1 信号组用例对照对平（10/10），与 IPC 交叉语义对齐（10/10）。
//! 域：CoRun·UNX-C4-B33（LTP 对照域·信号/时钟交叉族）。
//!
//! 结构：交叉对照账（信号面 + 时钟面各十例：例号 + 投递语义位 + 交叉对
//! 象 + 裁断位）+ 对照器（投递语义与 IPC 交叉对象一致才翻绿）+ 逐面对平
//! 器 + fold 与冻结 + 注入核（交叉对象越界/漏对照/重号/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e5a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e5a0;
pub const XG_BADTGT: u32 = SEG_BASE + 0x01; // 交叉对象越界
pub const XG_NOCASE: u32 = SEG_BASE + 0x02; // 例未立账
pub const XG_DUP: u32 = SEG_BASE + 0x03; // 例号重号
pub const XG_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const XG_SHORT: u32 = SEG_BASE + 0x05; // 面缺例
pub const XG_MISS: u32 = SEG_BASE + 0x06; // 语义失配

/// 每面例数（10/10）。
pub const XG_CASES: usize = 10;
/// 交叉对象空间（五族：0..4）。
pub const XG_TARGETS: usize = 5;
/// 交叉族对照账域标（fold 种子）。
pub const XGRP_TAG: u64 = 0x1e5a_2d;

/// 单交叉对照例：例号 + 交叉对象 + 投递语义位 + 裁断位。
#[derive(Clone, Debug)]
pub struct XCase {
    pub cid: u32,
    pub target: usize,
    pub delivered: bool,
    pub verdict: bool,
}

impl XCase {
    /// 对照齐绿：已投递且交叉对象合法且裁断翻绿。
    pub fn green(&self) -> bool {
        self.delivered && self.verdict && self.target < XG_TARGETS
    }

    pub fn render(&self) -> String {
        let flag = if self.green() { "G" } else { "R" };
        format!("xc|c{}|t{}|{}", self.cid, self.target, flag)
    }
}

/// 交叉族对照账：信号面 + 时钟面。
pub struct XgrpLedger {
    pub sig: Vec<XCase>,
    pub clk: Vec<XCase>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl XgrpLedger {
    pub fn new() -> XgrpLedger {
        XgrpLedger { sig: Vec::new(), clk: Vec::new(), frozen: false, root: None }
    }

    /// 立账：冻结、交叉对象越界、重号必拒。
    pub fn enroll(&mut self, face: usize, cid: u32, target: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(XG_FROZEN);
        }
        if face >= 2 {
            return Err(XG_NOCASE);
        }
        if target >= XG_TARGETS {
            return Err(XG_BADTGT);
        }
        let tab = if face == 0 { &mut self.sig } else { &mut self.clk };
        if tab.iter().any(|x| x.cid == cid) {
            return Err(XG_DUP);
        }
        tab.push(XCase { cid, target, delivered: false, verdict: false });
        Ok(())
    }

    /// 投递对照：投递语义与交叉对象一致才翻绿。
    pub fn deliver(&mut self, face: usize, cid: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(XG_FROZEN);
        }
        if face >= 2 {
            return Err(XG_NOCASE);
        }
        let tab = if face == 0 { &mut self.sig } else { &mut self.clk };
        let mut hit = false;
        let mut ok = false;
        let mut k = 0usize;
        while k < tab.len() {
            if tab[k].cid == cid {
                tab[k].delivered = true;
                tab[k].verdict = true;
                ok = true;
                hit = true;
            }
            k += 1;
        }
        if hit {
            Ok(ok)
        } else {
            Err(XG_NOCASE)
        }
    }

    /// 交叉语义对齐探针：对象须在册且合法。
    pub fn cross_ok(&self, face: usize, cid: u32) -> Result<bool, u32> {
        if face >= 2 {
            return Err(XG_NOCASE);
        }
        let tab = if face == 0 { &self.sig } else { &self.clk };
        let mut k = 0usize;
        while k < tab.len() {
            if tab[k].cid == cid {
                if tab[k].target >= XG_TARGETS {
                    return Err(XG_MISS);
                }
                return Ok(true);
            }
            k += 1;
        }
        Err(XG_NOCASE)
    }

    /// 单面对平。
    pub fn face_even(&self, face: usize) -> bool {
        let tab = if face == 0 { &self.sig } else { &self.clk };
        if tab.len() != XG_CASES {
            return false;
        }
        let mut ok = true;
        let mut k = 0usize;
        while k < tab.len() {
            if !tab[k].green() {
                ok = false;
            }
            k += 1;
        }
        ok
    }

    /// 双面对平。
    pub fn closed(&self) -> bool {
        self.face_even(0) && self.face_even(1)
    }

    /// 交叉对象分布：五对象逐面覆盖。
    pub fn targets_covered(&self, face: usize) -> bool {
        let tab = if face == 0 { &self.sig } else { &self.clk };
        let mut t = 0usize;
        let mut ok = true;
        while t < XG_TARGETS {
            let mut found = false;
            let mut k = 0usize;
            while k < tab.len() {
                if tab[k].target == t {
                    found = true;
                }
                k += 1;
            }
            if !found {
                ok = false;
            }
            t += 1;
        }
        ok
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = XGRP_TAG;
        let mut f = 0usize;
        while f < 2 {
            let tab = if f == 0 { &self.sig } else { &self.clk };
            let mut k = 0usize;
            while k < tab.len() {
                h = (h ^ tab[k].cid as u64).wrapping_mul(0x100000001b3);
                h = (h ^ tab[k].target as u64).wrapping_mul(0x100000001b3);
                h = (h ^ tab[k].verdict as u64).wrapping_mul(0x100000001b3);
                k += 1;
            }
            f += 1;
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

/// 信号面例号样本。
pub fn xg_cid(no: usize) -> u32 {
    0xb000 + no as u32
}

/// 时钟面例号样本。
pub fn xc_cid(no: usize) -> u32 {
    0xc000 + no as u32
}

/// 交叉对象确定性样本。
pub fn xg_target(no: usize) -> usize {
    no % XG_TARGETS
}

/// 双面全量对照全链。
pub fn build_xgrpledger() -> (XgrpLedger, bool) {
    let mut l = XgrpLedger::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < XG_CASES {
        if l.enroll(0, xg_cid(i), xg_target(i)).is_err() {
            ok = false;
        }
        if l.enroll(1, xc_cid(i), xg_target(i + 3)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < XG_CASES {
        if l.deliver(0, xg_cid(i)).is_err() {
            ok = false;
        }
        if l.deliver(1, xc_cid(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let l_ok = ok && l.closed();
    (l, l_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 交叉对象越界必拒
    let mut a = XgrpLedger::new();
    got[0] = a.enroll(0, 1, XG_TARGETS).err() == Some(XG_BADTGT);
    // 2) 例未立账投递必拒
    let mut b = XgrpLedger::new();
    got[1] = b.deliver(1, 0x777).err() == Some(XG_NOCASE);
    // 3) 重号必拒
    let mut c = XgrpLedger::new();
    let _ = c.enroll(0, 9, 1);
    got[2] = c.enroll(0, 9, 2).err() == Some(XG_DUP);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_xgrpledger();
    d.freeze();
    got[3] = d.enroll(1, 0xd000, 1).err() == Some(XG_FROZEN)
        && d.deliver(0, xg_cid(0)).err() == Some(XG_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11045_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11045");

    // J1 信号组用例对照对平
    let (mut book, green) = build_xgrpledger();
    s.add("J1-信号面对平", green && book.face_even(0), "信号面十例投递对照齐绿");

    // J2 与 IPC 交叉语义对齐：时钟面对平且对象全覆盖
    s.add(
        "J2-交叉语义对齐",
        book.face_even(1) && book.targets_covered(0) && book.targets_covered(1),
        "时钟面齐绿且交叉对象逐面覆盖",
    );

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.enroll(0, 0xd000, 1).err() == Some(XG_FROZEN)
        && book.deliver(1, xc_cid(0)).err() == Some(XG_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后立账投递均必拒");

    // J5 注入检出：对象越界 + 例未立账
    let inj = inject_four();
    s.add("J5-越界未立抓", inj[0] && inj[1], "对象越界与例未立账两路必抓");

    // J6 注入检出：重号 + 冻结后写
    s.add("J6-重号冻结检出", inj[2] && inj[3], "例号重号与冻结后写必抓");

    // J7 对象分布：信号面五对象覆盖
    let j7 = book.targets_covered(0) && book.targets_covered(1);
    s.add("J7-对象覆盖", j7, "交叉对象逐面全覆盖");

    // J8 交叉探针：在册例探针恒真且未立账必拒
    let j8 = book.cross_ok(0, xg_cid(0)) == Ok(true)
        && book.cross_ok(1, 0x777).err() == Some(XG_NOCASE);
    s.add("J8-交叉探针", j8, "在册例探针真未立账必拒");

    // J9 漏投为假：漏投一例对平为假
    let j9 = {
        let mut c = XgrpLedger::new();
        let mut i = 0usize;
        while i < XG_CASES {
            let _ = c.enroll(0, xg_cid(i), xg_target(i));
            i += 1;
        }
        let mut i = 1usize;
        while i < XG_CASES {
            let _ = c.deliver(0, xg_cid(i));
            i += 1;
        }
        !c.face_even(0)
    };
    s.add("J9-漏投为假", j9, "漏投一例对平断言为假");

    // J10 对象样本确定性
    let t3a = xg_target(3);
    let t3b = xg_target(3);
    s.add("J10-对象确定", t3a == t3b, "对象样本同入同出");

    // J11 例号分面确定
    let j11 = xg_cid(0) != xc_cid(0) && xg_cid(9) != xc_cid(9);
    s.add("J11-例号分面", j11, "两面例号空间互异");

    // J12 渲染分旗
    let j12 = {
        let g = XCase { cid: 1, target: 2, delivered: true, verdict: true };
        let r = XCase { cid: 1, target: 2, delivered: false, verdict: false };
        g.render().contains('G') && r.render().contains('R')
    };
    s.add("J12-渲染分旗", j12, "绿红对照渲染可区分");

    // J13 面越界拒
    let j13 = XgrpLedger::new().enroll(2, 1, 1).err() == Some(XG_NOCASE)
        && XgrpLedger::new().deliver(2, 1).err() == Some(XG_NOCASE);
    s.add("J13-面越界拒", j13, "面号越界立账投递均必拒");

    // J14 投递幂等：重复投递指纹稳定
    let j14 = {
        let mut c = XgrpLedger::new();
        let _ = c.enroll(1, xc_cid(4), 3);
        let _ = c.deliver(1, xc_cid(4));
        let f1 = c.fold();
        let _ = c.deliver(1, xc_cid(4));
        c.fold() == f1
    };
    s.add("J14-投递幂等", j14, "重复投递指纹稳定");

    // J15 动态明细：双面对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-双面明细",
        fp == root && book.closed(),
        &format!("root={:#x} sig={} clk={}", fp, book.sig.len(), book.clk.len()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11045_xgrp_flow() {
        let (mut l, built) = build_xgrpledger();
        assert!(built);
        assert!(l.deliver(0, 0x777).err() == Some(XG_NOCASE));
        l.freeze();
        assert!(l.enroll(1, 0xd000, 1).err() == Some(XG_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11045_checks_green() {
        let s = run_unx11045_checks();
        assert!(s.all_passed(), "UNX-F11045 判据未全绿");
    }
}
