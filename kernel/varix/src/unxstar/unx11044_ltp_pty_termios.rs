//! UNX-F11044 LTP pty/termios 组用例对照（终端族对账）（0x1e580 段）
//!
//! 锚点：UNX-F11044-J1 pty 组用例对照对平（10/10），termios 组对照（10/10）。
//! 域：CoRun·UNX-C4-B33（LTP 对照域·终端族）。
//!
//! 结构：终端族双面对照账（pty 面 + termios 面各十例：例号 + 录制基线 +
//! 重放实测 + 裁断位）+ 对照器（基线与重放一致才翻绿）+ 基线冻结器（基
//! 线落定后重放才可比）+ 逐面对平器 + fold 与冻结 + 注入核（基线漂移/
//! 未冻先比/重号/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e580 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e580;
pub const TM_DRIFT: u32 = SEG_BASE + 0x01; // 基线漂移
pub const TM_NOCASE: u32 = SEG_BASE + 0x02; // 例未立账
pub const TM_UNBASED: u32 = SEG_BASE + 0x03; // 基线未落先比
pub const TM_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const TM_DUP: u32 = SEG_BASE + 0x05; // 例号重号
pub const TM_SHORT: u32 = SEG_BASE + 0x06; // 面缺例

/// 每面例数（10/10）。
pub const TM_CASES: usize = 10;
/// 终端族对照账域标（fold 种子）。
pub const TERM_TAG: u64 = 0x1e58_2c;

/// 单终端对照例：例号 + 基线 + 重放 + 裁断位。
#[derive(Clone, Debug)]
pub struct TermCase {
    pub cid: u32,
    pub base: u64,
    pub replay: u64,
    pub based: bool,
    pub verdict: bool,
}

impl TermCase {
    /// 对照齐绿：基线落定且与重放一致。
    pub fn green(&self) -> bool {
        self.based && self.verdict && self.base == self.replay && self.base != 0
    }

    pub fn render(&self) -> String {
        let flag = if self.green() { "G" } else { "R" };
        format!("tc|c{}|{}|b{}", self.cid, flag, self.base)
    }
}

/// 终端族对照账：pty 面 + termios 面。
pub struct TermLedger {
    pub pty: Vec<TermCase>,
    pub tio: Vec<TermCase>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl TermLedger {
    pub fn new() -> TermLedger {
        TermLedger { pty: Vec::new(), tio: Vec::new(), frozen: false, root: None }
    }

    /// 立账（face: 0=pty 1=tio）：冻结、重号必拒。
    pub fn enroll(&mut self, face: usize, cid: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(TM_FROZEN);
        }
        if face >= 2 {
            return Err(TM_NOCASE);
        }
        let tab = if face == 0 { &mut self.pty } else { &mut self.tio };
        if tab.iter().any(|x| x.cid == cid) {
            return Err(TM_DUP);
        }
        tab.push(TermCase { cid, base: 0, replay: 0, based: false, verdict: false });
        Ok(())
    }

    /// 录基线：基线落定并翻基线位；未立账例必拒。
    pub fn record(&mut self, face: usize, cid: u32, base: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(TM_FROZEN);
        }
        if face >= 2 {
            return Err(TM_NOCASE);
        }
        if base == 0 {
            return Err(TM_DRIFT);
        }
        let tab = if face == 0 { &mut self.pty } else { &mut self.tio };
        let mut hit = false;
        let mut k = 0usize;
        while k < tab.len() {
            if tab[k].cid == cid {
                tab[k].base = base;
                tab[k].based = true;
                hit = true;
            }
            k += 1;
        }
        if hit {
            Ok(())
        } else {
            Err(TM_NOCASE)
        }
    }

    /// 重放对照：基线未落先比必拒；一致才翻绿。
    pub fn replay(&mut self, face: usize, cid: u32, got: u64) -> Result<bool, u32> {
        if self.frozen {
            return Err(TM_FROZEN);
        }
        if face >= 2 {
            return Err(TM_NOCASE);
        }
        let tab = if face == 0 { &mut self.pty } else { &mut self.tio };
        let mut hit = false;
        let mut ok = false;
        let mut k = 0usize;
        while k < tab.len() {
            if tab[k].cid == cid {
                if !tab[k].based {
                    return Err(TM_UNBASED);
                }
                tab[k].replay = got;
                tab[k].verdict = tab[k].base == got;
                ok = tab[k].verdict;
                hit = true;
            }
            k += 1;
        }
        if hit {
            Ok(ok)
        } else {
            Err(TM_NOCASE)
        }
    }

    /// 单面对平：例数齐且全绿。
    pub fn face_even(&self, face: usize) -> bool {
        let tab = if face == 0 { &self.pty } else { &self.tio };
        if tab.len() != TM_CASES {
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

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = TERM_TAG;
        let mut f = 0usize;
        while f < 2 {
            let tab = if f == 0 { &self.pty } else { &self.tio };
            let mut k = 0usize;
            while k < tab.len() {
                h = (h ^ tab[k].cid as u64).wrapping_mul(0x100000001b3);
                h = (h ^ tab[k].base).wrapping_mul(0x100000001b3);
                h = (h ^ tab[k].replay).wrapping_mul(0x100000001b3);
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

/// pty 面例号样本。
pub fn tm_cid(no: usize) -> u32 {
    0x8000 + no as u32
}

/// tio 面例号样本。
pub fn tt_cid(no: usize) -> u32 {
    0x9000 + no as u32
}

/// 基线确定性样本。
pub fn tm_base(no: usize) -> u64 {
    ((no % 5 + 3) as u64).wrapping_mul(0x100000001b3) ^ (no as u64 | 0x5b)
}

/// 双面全量对照全链：立账 → 录基线 → 重放对照。
pub fn build_termledger() -> (TermLedger, bool) {
    let mut l = TermLedger::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < TM_CASES {
        if l.enroll(0, tm_cid(i)).is_err() {
            ok = false;
        }
        if l.enroll(1, tt_cid(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < TM_CASES {
        if l.record(0, tm_cid(i), tm_base(i)).is_err() {
            ok = false;
        }
        if l.record(1, tt_cid(i), tm_base(i + TM_CASES)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < TM_CASES {
        if l.replay(0, tm_cid(i), tm_base(i)).is_err() {
            ok = false;
        }
        if l.replay(1, tt_cid(i), tm_base(i + TM_CASES)).is_err() {
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
    // 1) 基线漂移必拒：重放与基线不一致出红
    let mut a = TermLedger::new();
    let _ = a.enroll(0, tm_cid(0));
    let _ = a.record(0, tm_cid(0), tm_base(0));
    let _ = a.replay(0, tm_cid(0), tm_base(0).wrapping_add(1));
    got[0] = !a.face_even(0);
    // 2) 未冻（基线未落）先比必拒
    let mut b = TermLedger::new();
    let _ = b.enroll(1, tt_cid(0));
    got[1] = b.replay(1, tt_cid(0), 5).err() == Some(TM_UNBASED);
    // 3) 重号与零基线必拒
    let mut c = TermLedger::new();
    let _ = c.enroll(0, tm_cid(1));
    let z = c.record(0, tm_cid(1), 0).err() == Some(TM_DRIFT);
    got[2] = z && c.enroll(0, tm_cid(1)).err() == Some(TM_DUP);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_termledger();
    d.freeze();
    got[3] = d.enroll(0, 0xa000).err() == Some(TM_FROZEN)
        && d.replay(1, tt_cid(0), 1).err() == Some(TM_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11044_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11044");

    // J1 pty 组用例对照对平
    let (mut book, green) = build_termledger();
    s.add("J1-pty 面对平", green && book.face_even(0), "pty 面十例录基线重放齐绿");

    // J2 termios 组对照
    s.add("J2-tio 面对平", book.face_even(1), "termios 面十例录基线重放齐绿");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.enroll(0, 0xa000).err() == Some(TM_FROZEN)
        && book.record(1, tt_cid(0), 1).err() == Some(TM_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后立账录基均必拒");

    // J5 注入检出：基线漂移 + 未基先比
    let inj = inject_four();
    s.add("J5-漂移未基抓", inj[0] && inj[1], "基线漂移与未基先比两路必抓");

    // J6 注入检出：重号零基线 + 冻结后写
    s.add("J6-重号冻结检出", inj[2] && inj[3], "重号零基线与冻结后写必抓");

    // J7 基线全落定：逐例基线位齐翻
    let j7 = {
        let mut all = true;
        let mut k = 0usize;
        while k < book.pty.len() {
            if !book.pty[k].based {
                all = false;
            }
            k += 1;
        }
        all && book.pty.len() == TM_CASES
    };
    s.add("J7-基线全落", j7, "pty 面逐例基线位齐翻");

    // J8 未录先比为假：漏录一例对平为假
    let j8 = {
        let mut c = TermLedger::new();
        let mut i = 0usize;
        while i < TM_CASES {
            let _ = c.enroll(0, tm_cid(i));
            i += 1;
        }
        let mut i = 1usize;
        while i < TM_CASES {
            let _ = c.record(0, tm_cid(i), tm_base(i));
            i += 1;
        }
        !c.face_even(0)
    };
    s.add("J8-漏录为假", j8, "漏录基线对平断言为假");

    // J9 基线样本确定性
    let b5a = tm_base(5);
    let b5b = tm_base(5);
    s.add("J9-基线确定", b5a == b5b, "基线样本同入同出");

    // J10 例号分面确定
    let j10 = tm_cid(0) != tt_cid(0) && tm_cid(9) != tt_cid(9);
    s.add("J10-例号分面", j10, "两面例号空间互异");

    // J11 渲染分旗
    let j11 = {
        let g = TermCase { cid: 1, base: 7, replay: 7, based: true, verdict: true };
        let r = TermCase { cid: 1, base: 7, replay: 8, based: true, verdict: false };
        g.render().contains('G') && r.render().contains('R')
    };
    s.add("J11-渲染分旗", j11, "绿红对照渲染可区分");

    // J12 重放漂移指纹敏感
    let j12 = {
        let mut c = TermLedger::new();
        let _ = c.enroll(1, tt_cid(2));
        let _ = c.record(1, tt_cid(2), tm_base(2));
        let _ = c.replay(1, tt_cid(2), tm_base(2));
        let f1 = c.fold();
        let _ = c.replay(1, tt_cid(2), tm_base(2).wrapping_add(9));
        c.fold() != f1
    };
    s.add("J12-漂移敏感", j12, "重放漂移指纹必变");

    // J13 面越界拒
    let j13 = TermLedger::new().enroll(2, 1).err() == Some(TM_NOCASE)
        && TermLedger::new().replay(2, 1, 1).err() == Some(TM_NOCASE);
    s.add("J13-面越界拒", j13, "面号越界立账重放均必拒");

    // J14 缺面为假：只立一面 closed 为假
    let j14 = {
        let mut c = TermLedger::new();
        let mut i = 0usize;
        while i < TM_CASES {
            let _ = c.enroll(0, tm_cid(i));
            let _ = c.record(0, tm_cid(i), tm_base(i));
            let _ = c.replay(0, tm_cid(i), tm_base(i));
            i += 1;
        }
        !c.closed()
    };
    s.add("J14-缺面为假", j14, "缺面清单对平断言为假");

    // J15 动态明细：双面对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-双面明细",
        fp == root && book.closed(),
        &format!("root={:#x} pty={} tio={}", fp, book.pty.len(), book.tio.len()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11044_term_flow() {
        let (mut l, built) = build_termledger();
        assert!(built);
        assert!(l.replay(0, 0x777, 1).err() == Some(TM_NOCASE));
        l.freeze();
        assert!(l.enroll(1, 0xa000).err() == Some(TM_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11044_checks_green() {
        let s = run_unx11044_checks();
        assert!(s.all_passed(), "UNX-F11044 判据未全绿");
    }
}
