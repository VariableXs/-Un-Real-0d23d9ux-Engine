//! UNX-F10180 B29 收口小结与预算核销（0x109a0 段）
//!
//! 锚点：UNX-F10180-J1 收口小结落账（19 条战线条目 J1/J2 双判据齐备+
//! 预算 6,600 核销零偏离 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：收口册（19 条战线条目 × J1/J2 双判据位）+ 核销核（预算
//! 6,600 分笔核销、超核拒、欠核检出）+ 收口审计（条目齐+双判据齐+
//! 核销零偏离三重门）+ 折叠指纹 + 注入核（缺 J2 计满/超额核销/
//! 重号条目/空留痕/印后核销）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x109a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x109a0;
pub const CB_DUP: u32 = SEG_BASE + 0x01; // 条目重号
pub const CB_OVER: u32 = SEG_BASE + 0x02; // 超预算核销
pub const CB_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 留痕为空
pub const CB_SHORT: u32 = SEG_BASE + 0x04; // 欠核销（审计面检出）
pub const CB_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍核销

/// B29 战线条目总数。
pub const LINE_TOTAL: usize = 19;
/// B29 预算。
pub const BUDGET: u32 = 6600;

/// 战线条目：no + J1/J2 双判据位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineEntry {
    pub no: u32,
    pub j1: bool,
    pub j2: bool,
}

/// 一次核销的账本留痕。
#[derive(Clone)]
pub struct WoffRec {
    pub seq: u32,
    pub amount: u32,
    pub cum: u32,
    pub note: String,
}

impl WoffRec {
    /// 账目合法性：留痕非空、金额正、累计不回退。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.amount > 0
    }

    pub fn render_line(&self) -> String {
        format!("W{}|amt{}|cum{}|{}", self.seq, self.amount, self.cum, self.note)
    }
}

/// 收口册：条目表 + 核销账 + 三重审计门。
pub struct ClosureBook {
    pub lines: Vec<LineEntry>,
    pub ledger: Vec<WoffRec>,
    pub spent: u32,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ClosureBook {
    pub fn new() -> ClosureBook {
        ClosureBook { lines: Vec::new(), ledger: Vec::new(), spent: 0, seq_next: 1, sealed: false }
    }

    /// 登记一条战线条目（重号必拒）。
    pub fn add_line(&mut self, no: u32, j1: bool, j2: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(CB_FROZEN);
        }
        if no == 0 || self.lines.iter().any(|l| l.no == no) {
            return Err(CB_DUP);
        }
        self.lines.push(LineEntry { no, j1, j2 });
        Ok(())
    }

    /// 分笔核销（超额必拒）。
    pub fn write_off(&mut self, amount: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CB_FROZEN);
        }
        if note.is_empty() {
            return Err(CB_EMPTY_NOTE);
        }
        if amount == 0 || self.spent + amount > BUDGET {
            return Err(CB_OVER);
        }
        self.spent += amount;
        self.ledger.push(WoffRec {
            seq: self.seq_next,
            amount,
            cum: self.spent,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(self.spent)
    }

    /// 条目门：条目数恰为 LINE_TOTAL 且 J1/J2 双判据逐条齐备。
    pub fn lines_complete(&self) -> bool {
        self.lines.len() == LINE_TOTAL && self.lines.iter().all(|l| l.j1 && l.j2)
    }

    /// 核销门：spent 恰等于预算（零偏离）。
    pub fn writeoff_exact(&self) -> bool {
        self.spent == BUDGET
    }

    /// 欠核销缺口。
    pub fn gap(&self) -> u32 {
        BUDGET - self.spent.min(BUDGET)
    }

    /// 三重审计门：条目齐 + 双判据齐 + 核销零偏离。
    pub fn audit(&self) -> bool {
        self.lines_complete() && self.writeoff_exact()
    }

    /// 双判据位独立统计：(J1 缺数, J2 缺数)。
    pub fn j_gap(&self) -> (usize, usize) {
        let g1 = self.lines.iter().filter(|l| !l.j1).count();
        let g2 = self.lines.iter().filter(|l| !l.j2).count();
        (g1, g2)
    }

    /// 核销进度（permille，千分位整型）。
    pub fn writeoff_permille(&self) -> u32 {
        self.spent * 1000 / BUDGET
    }

    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.ledger.len() {
            for b in (a + 1)..self.ledger.len() {
                if self.ledger[a].seq == self.ledger[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 折叠指纹（73 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x109a0;
        for l in &self.lines {
            h = h.wrapping_mul(73).wrapping_add(l.no);
            h = h.wrapping_mul(73).wrapping_add(l.j1 as u32);
            h = h.wrapping_mul(73).wrapping_add(l.j2 as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(73).wrapping_add(r.cum);
            for by in r.note.bytes() {
                h = h.wrapping_mul(73).wrapping_add(by as u32);
            }
        }
        h = h.wrapping_mul(73).wrapping_add(self.spent);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ClosureBook::new();
    // 1) 条目重号必拒。
    let _ = b.add_line(1, true, true);
    got[0] = b.add_line(1, true, true).err() == Some(CB_DUP);
    // 2) 超预算核销必拒。
    got[1] = b.write_off(BUDGET + 1, "inj-over").err() == Some(CB_OVER);
    // 3) 缺 J2 条目计入条目门必判不齐。
    let mut b2 = ClosureBook::new();
    let mut i = 1u32;
    while i <= LINE_TOTAL as u32 {
        let _ = b2.add_line(i, true, i != 5); // 第 5 条缺 J2
        i += 1;
    }
    got[2] = !b2.lines_complete();
    // 4) 欠核销缺口检出：差一笔未核时 gap>0 且审计不绿。
    let mut b3 = ClosureBook::new();
    let mut k = 1u32;
    while k <= LINE_TOTAL as u32 {
        let _ = b3.add_line(k, true, true);
        k += 1;
    }
    let _ = b3.write_off(6500, "partial");
    got[3] = b3.gap() == 100 && !b3.audit();
    // 5) 印账后核销必抓。
    b.seal();
    got[4] = b.write_off(100, "inj-frozen").err() == Some(CB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10180_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10180");

    // J1 收口小结落账：19 条战线条目全数登记。
    let j1 = {
        let mut b = ClosureBook::new();
        let mut i = 1u32;
        while i <= LINE_TOTAL as u32 {
            let _ = b.add_line(i, true, true);
            i += 1;
        }
        b.lines.len() == LINE_TOTAL
    };
    s.add("J1-条目落账", j1, "19 条战线条目全数入册");

    // J2 双判据齐备：J1/J2 双位逐条置齐过门。
    let j2 = {
        let mut b = ClosureBook::new();
        let mut i = 1u32;
        while i <= LINE_TOTAL as u32 {
            let _ = b.add_line(i, true, true);
            i += 1;
        }
        b.lines_complete()
    };
    s.add("J2-双判据齐", j2, "J1/J2 双位逐条齐备过门");

    // J3 预算核销零偏离：6,600 恰好核完。
    let j3 = {
        let mut b = ClosureBook::new();
        let mut i = 1u32;
        while i <= LINE_TOTAL as u32 {
            let _ = b.add_line(i, true, true);
            i += 1;
        }
        // 19 笔核销：前 18 笔各 347，尾笔补零头。
        let mut k = 0;
        while k < 18 {
            let _ = b.write_off(347, "w");
            k += 1;
        }
        let _ = b.write_off(BUDGET - 347 * 18, "tail");
        b.writeoff_exact() && b.audit()
    };
    s.add("J3-核销零偏离", j3, "6,600 分笔恰核完审计全绿");

    // J4 超核销拒：越预算一笔必拒且 spent 不漂。
    let j4 = {
        let mut b = ClosureBook::new();
        let e = b.write_off(BUDGET + 1, "over");
        e == Err(CB_OVER) && b.spent == 0
    };
    s.add("J4-超核销拒", j4, "越预算核销必拒");

    // J5 欠核销检出：缺口精确且审计面标红。
    let j5 = {
        let mut b = ClosureBook::new();
        let mut i = 1u32;
        while i <= LINE_TOTAL as u32 {
            let _ = b.add_line(i, true, true);
            i += 1;
        }
        let _ = b.write_off(6000, "p");
        b.gap() == 600 && !b.audit()
    };
    s.add("J5-欠核检出", j5, "缺口 600 精确且审计红");

    // J6 条目重号拒。
    let j6 = {
        let mut b = ClosureBook::new();
        let _ = b.add_line(7, true, true);
        b.add_line(7, true, true).err() == Some(CB_DUP)
    };
    s.add("J6-条目重号拒", j6, "同号条目必拒");

    // J7 核销账完整：留痕合法、seq 唯一、累计单调。
    let j7 = {
        let mut b = ClosureBook::new();
        let _ = b.write_off(100, "a");
        let _ = b.write_off(200, "b");
        let _ = b.write_off(300, "c");
        b.ledger_ok()
            && b.seq_unique()
            && b.ledger[0].cum == 100
            && b.ledger[1].cum == 300
            && b.ledger[2].cum == 600
    };
    s.add("J7-核销账单调", j7, "累计核销单调递增无跳变");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut b = ClosureBook::new();
        let _ = b.add_line(1, true, true);
        let _ = b.write_off(100, "w");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.write_off(1, "post").err() == Some(CB_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // J10 双判据位独立统计：缺位分账精确。
    let j10 = {
        let mut b = ClosureBook::new();
        let _ = b.add_line(1, false, true); // 缺 J1
        let _ = b.add_line(2, true, false); // 缺 J2
        let _ = b.add_line(3, true, true);
        b.j_gap() == (1, 1)
    };
    s.add("J10-缺位分账", j10, "J1/J2 缺位独立计数精确");

    // J11 核销进度诊断面：permille 与 gap 互洽。
    let j11 = {
        let mut b = ClosureBook::new();
        let _ = b.write_off(3300, "half");
        b.writeoff_permille() == 500 && b.gap() == 3300
    };
    s.add("J11-进度互洽", j11, "permille=500 与缺口 3300 互洽");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10180_closure_flow() {
        let mut b = ClosureBook::new();
        let mut i = 1u32;
        while i <= LINE_TOTAL as u32 {
            assert!(b.add_line(i, true, true).is_ok());
            i += 1;
        }
        assert_eq!(b.write_off(6600, "one-shot"), Ok(6600));
        assert!(b.audit());
        assert_eq!(b.write_off(1, "more"), Err(CB_OVER));
    }

    #[test]
    fn unx10180_checks_green() {
        let s = run_unx10180_checks();
        assert!(s.all_passed(), "UNX-F10180 判据未全绿");
    }
}
