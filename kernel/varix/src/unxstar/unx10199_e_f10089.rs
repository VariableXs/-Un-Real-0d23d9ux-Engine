//! UNX-F10199 E 型前段对账与 F10089 门复核（对账达成与脱账受控）（0x10c00 段）
//!
//! 锚点：UNX-F10199-J1 对账达成（B25–B30 120 条 J1/J2 齐+E 型门条件核
//! 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：对账册（六批 × 每批 20 条 × J1/J2 双位）+ 批账核（逐批计数
//! 对平）+ 总账核（120 恒等）+ 门核（E 型门开/闭状态机）+ 折叠指纹 +
//! 注入核（缺条门不开/脱账批计数/重号条目/伪造开门/印后入账）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x10c00 独占 0x20）。
pub const SEG_BASE: u32 = 0x10c00;
pub const RC_DUP: u32 = SEG_BASE + 0x01; // 条目重号
pub const RC_DRIFT: u32 = SEG_BASE + 0x02; // 批计数脱账
pub const RC_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 留痕为空
pub const RC_GATE: u32 = SEG_BASE + 0x04; // 门条件未达仍求开
pub const RC_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍入账

/// B25–B30 六批。
pub const BATCHES: [u32; 6] = [25, 26, 27, 28, 29, 30];
/// 每批条目数。
pub const PER_BATCH: usize = 20;
/// 总条目恒等。
pub const TOTAL: usize = BATCHES.len() * PER_BATCH;

/// 对账条目：批号 × 序 × 双判据位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RcLine {
    pub batch: u32,
    pub no: u32,
    pub j1: bool,
    pub j2: bool,
}

/// 门态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateState {
    Closed,
    Open,
}

/// 一次对账核验的留痕。
#[derive(Clone)]
pub struct RcRec {
    pub seq: u32,
    pub lines: u32,
    pub complete: bool,
    pub note: String,
}

impl RcRec {
    /// 账目合法性：留痕非空且齐账必满额。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (!self.complete || self.lines as usize == TOTAL)
    }

    pub fn render_line(&self) -> String {
        format!("R{}|n{}|cmp{}|{}", self.seq, self.lines, self.complete as u8, self.note)
    }
}

/// 对账册：条目表 + 批账核 + 门核。
pub struct ReconBook {
    pub lines: Vec<RcLine>,
    pub gate: GateState,
    pub ledger: Vec<RcRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ReconBook {
    pub fn new() -> ReconBook {
        ReconBook { lines: Vec::new(), gate: GateState::Closed, ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 入账一条（重号必拒；批号出 B25–B30 必拒）。
    pub fn add_line(&mut self, batch: u32, no: u32, j1: bool, j2: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(RC_FROZEN);
        }
        if !BATCHES.contains(&batch) {
            return Err(RC_DRIFT);
        }
        if self.lines.iter().any(|l| l.batch == batch && l.no == no) {
            return Err(RC_DUP);
        }
        self.lines.push(RcLine { batch, no, j1, j2 });
        Ok(())
    }

    /// 批账核：批号 → 条目数。
    pub fn batch_count(&self, batch: u32) -> usize {
        self.lines.iter().filter(|l| l.batch == batch).count()
    }

    /// 双判据齐审计：全部条目 J1/J2 双位齐备。
    pub fn all_flagged(&self) -> bool {
        self.lines.iter().all(|l| l.j1 && l.j2)
    }

    /// 对账核：六批各 20 且双位齐 → 满额 120。
    pub fn reconcile(&mut self, note: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(RC_FROZEN);
        }
        if note.is_empty() {
            return Err(RC_EMPTY_NOTE);
        }
        let complete = self.lines.len() == TOTAL
            && BATCHES.iter().all(|b| self.batch_count(*b) == PER_BATCH)
            && self.all_flagged();
        self.ledger.push(RcRec {
            seq: self.seq_next,
            lines: self.lines.len() as u32,
            complete,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(complete)
    }

    /// E 型门核：对账达成才许开门（未达求开必拒）。
    pub fn try_open_gate(&mut self, note: &str) -> Result<GateState, u32> {
        if self.sealed {
            return Err(RC_FROZEN);
        }
        if note.is_empty() {
            return Err(RC_EMPTY_NOTE);
        }
        let complete = self.lines.len() == TOTAL
            && BATCHES.iter().all(|b| self.batch_count(*b) == PER_BATCH)
            && self.all_flagged();
        if !complete {
            return Err(RC_GATE);
        }
        self.gate = GateState::Open;
        Ok(self.gate)
    }

    pub fn batch_totals(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for b in BATCHES {
            out.push(self.batch_count(b));
        }
        out
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

    /// 折叠指纹（173 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10c00;
        for l in &self.lines {
            h = h.wrapping_mul(173).wrapping_add(l.batch);
            h = h.wrapping_mul(173).wrapping_add(l.j1 as u32 + l.j2 as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(173).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(173).wrapping_add(by as u32);
            }
        }
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
    let mut b = ReconBook::new();
    // 灌满五批（B30 留缺）。
    for bi in 0..BATCHES.len() - 1 {
        let mut no = 1u32;
        while no <= PER_BATCH as u32 {
            let _ = b.add_line(BATCHES[bi], no, true, true);
            no += 1;
        }
    }
    // 1) 缺条门不开（B30 空批求开门必拒）。
    got[0] = b.try_open_gate("inj-gate").err() == Some(RC_GATE);
    // 2) 批计数脱账检出：B30 缺 20 条。
    got[1] = b.batch_count(30) == 0 && b.batch_totals()[5] != PER_BATCH;
    // 3) 重号条目必拒。
    let _ = b.add_line(30, 1, true, true);
    got[2] = b.add_line(30, 1, true, true).err() == Some(RC_DUP);
    // 4) 伪造开门（缺 J2 求开）必拒。
    let mut b2 = ReconBook::new();
    for bi in BATCHES {
        let mut no = 1u32;
        while no <= PER_BATCH as u32 {
            let _ = b2.add_line(bi, no, true, no != 1); // 每批 1 号缺 J2
            no += 1;
        }
    }
    got[3] = b2.try_open_gate("inj-j2").err() == Some(RC_GATE);
    // 5) 印账后入账必抓。
    b.seal();
    got[4] = b.add_line(30, 2, true, true).err() == Some(RC_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10199_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10199");

    // J1 对账达成：120 条全数入账且六批各 20。
    let j1 = {
        let mut b = ReconBook::new();
        for bi in BATCHES {
            let mut no = 1u32;
            while no <= PER_BATCH as u32 {
                let _ = b.add_line(bi, no, true, true);
                no += 1;
            }
        }
        b.reconcile("r") == Ok(true) && b.batch_totals() == vec![20; 6]
    };
    s.add("J1-对账达成", j1, "120 条六批各 20 对平");

    // J2 E 型门开：对账达成后开门成功。
    let j2 = {
        let mut b = ReconBook::new();
        for bi in BATCHES {
            let mut no = 1u32;
            while no <= PER_BATCH as u32 {
                let _ = b.add_line(bi, no, true, true);
                no += 1;
            }
        }
        let _ = b.reconcile("r");
        b.try_open_gate("open") == Ok(GateState::Open)
    };
    s.add("J2-门开", j2, "满额开门 GateState::Open");

    // J3 缺条门闭：缺一条门条件不达。
    let j3 = {
        let mut b = ReconBook::new();
        for bi in BATCHES {
            let mut no = 1u32;
            while no <= PER_BATCH as u32 {
                // B30 少一条（no=20 跳过）。
                if !(bi == 30 && no == 20) {
                    let _ = b.add_line(bi, no, true, true);
                }
                no += 1;
            }
        }
        b.reconcile("r") == Ok(false) && b.try_open_gate("open").err() == Some(RC_GATE)
    };
    s.add("J3-缺条门闭", j3, "119 条门不开");

    // J4 缺 J2 门闭：满额但双位不齐仍不开。
    let j4 = {
        let mut b = ReconBook::new();
        for bi in BATCHES {
            let mut no = 1u32;
            while no <= PER_BATCH as u32 {
                let _ = b.add_line(bi, no, true, no != 1);
                no += 1;
            }
        }
        b.try_open_gate("open").err() == Some(RC_GATE)
    };
    s.add("J4-缺位门闭", j4, "缺 J2 逐条拦门");

    // J5 重号拒。
    let j5 = {
        let mut b = ReconBook::new();
        let _ = b.add_line(25, 1, true, true);
        b.add_line(25, 1, true, true).err() == Some(RC_DUP)
    };
    s.add("J5-重号拒", j5, "同批同号必拒");

    // J6 野批拒：批号出 B25–B30 必拒。
    let j6 = {
        let mut b = ReconBook::new();
        b.add_line(24, 1, true, true).err() == Some(RC_DRIFT)
            && b.add_line(31, 1, true, true).err() == Some(RC_DRIFT)
    };
    s.add("J6-野批拒", j6, "批号越域必拒");

    // J7 对账账完整：核验账逐条合法。
    let j7 = {
        let mut b = ReconBook::new();
        let _ = b.reconcile("r1");
        let _ = b.reconcile("r2");
        b.ledger.len() == 2 && b.ledger_ok() && b.seq_unique()
    };
    s.add("J7-账本完整", j7, "双核验账合法无重号");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut b = ReconBook::new();
        let _ = b.add_line(25, 1, true, true);
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.add_line(25, 2, true, true).err() == Some(RC_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10199_recon_flow() {
        let mut b = ReconBook::new();
        for bi in BATCHES {
            let mut no = 1u32;
            while no <= PER_BATCH as u32 {
                assert!(b.add_line(bi, no, true, true).is_ok());
                no += 1;
            }
        }
        assert_eq!(b.reconcile("r"), Ok(true));
        assert_eq!(b.try_open_gate("o"), Ok(GateState::Open));
    }

    #[test]
    fn unx10199_checks_green() {
        let s = run_unx10199_checks();
        assert!(s.all_passed(), "UNX-F10199 判据未全绿");
    }
}
