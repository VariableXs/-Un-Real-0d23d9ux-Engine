//! UNX-F11122 B01–B02 清账（管道族判据终重放）（0x1f5a2 段）
//!
//! 管道族占 B01 与 B02 两批，每批 20 条判据。每条判据记录写入量、读出量
//! 与关闭标记，清账时逐条终重放：读出不得多于写入，关闭时必须读空。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11122 独占 0x1f5a2。
pub const SEG_BASE: u32 = 0x1f5a2;

/// 批号越出 B01–B02。
pub const PC_BATCH: u32 = SEG_BASE + 0x01;
/// 判据号越出 1..=20。
pub const PC_NO: u32 = SEG_BASE + 0x02;
/// 判据重复登记。
pub const PC_DUP: u32 = SEG_BASE + 0x03;
/// 批内判据不连续。
pub const PC_ORDER: u32 = SEG_BASE + 0x04;
/// 读出量多于写入量。
pub const PC_UNDER: u32 = SEG_BASE + 0x05;
/// 关闭标记与余量矛盾。
pub const PC_EOF: u32 = SEG_BASE + 0x06;
/// 判据重复重放。
pub const PC_REPLAY: u32 = SEG_BASE + 0x07;
/// 清账账已冻结。
pub const PC_FROZEN: u32 = SEG_BASE + 0x08;

/// 首批号。
pub const BATCH_FIRST: usize = 1;
/// 末批号。
pub const BATCH_LAST: usize = 2;
/// 每批判据数。
pub const CRIT_PER_BATCH: usize = 20;
/// 判据总数。
pub const CRITS: usize = 40;
/// 容量。
pub const CAP: usize = 44;
/// 折叠种子。
const PC_TAG: u64 = 0x0000_1f5_a2a2;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// 管道判据。
#[derive(Clone, Debug)]
pub struct PipeCase {
    /// 批号（1 或 2）。
    pub batch: usize,
    /// 批内判据号（1..=20）。
    pub no: usize,
    /// 写入字节数。
    pub wrote: usize,
    /// 读出字节数。
    pub read: usize,
    /// 是否已关闭写端。
    pub eof: bool,
    /// 是否已终重放。
    pub replayed: bool,
}

impl PipeCase {
    /// 判据不变量：读出不多于写入，关闭即读空。
    pub fn invariant(&self) -> bool {
        self.read <= self.wrote && (!self.eof || self.read == self.wrote)
    }

    /// 判据是否清账闭合。
    pub fn settled(&self) -> bool {
        self.invariant() && self.replayed
    }

    /// 判据渲染，形如 P1-07|21/21|E|R。
    pub fn render(&self) -> String {
        let e = if self.eof { "E" } else { "O" };
        let r = if self.replayed { "R" } else { "N" };
        format!("P{}-{:02}|{}/{}|{}|{}", self.batch, self.no, self.read, self.wrote, e, r)
    }
}

/// 管道族清账账。
#[derive(Clone, Debug)]
pub struct PipeClear {
    cases: Vec<PipeCase>,
    frozen: bool,
    root: u64,
}

impl PipeClear {
    /// 建空账。
    pub fn new() -> Self {
        Self { cases: Vec::new(), frozen: false, root: 0 }
    }

    /// 定位判据下标。
    fn index_of(&self, batch: usize, no: usize) -> usize {
        let mut idx = usize::MAX;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].batch == batch && self.cases[k].no == no {
                idx = k;
            }
            k = k.wrapping_add(1);
        }
        idx
    }

    /// 登记判据：批域、号域、重复、顺序、容量、冻结六道闸。
    pub fn file(&mut self, batch: usize, no: usize, wrote: usize, read: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(PC_FROZEN);
        }
        if batch < BATCH_FIRST || batch > BATCH_LAST {
            return Err(PC_BATCH);
        }
        if no == 0 || no > CRIT_PER_BATCH {
            return Err(PC_NO);
        }
        if self.cases.len() >= CAP {
            return Err(PC_FROZEN);
        }
        if self.index_of(batch, no) != usize::MAX {
            return Err(PC_DUP);
        }
        let batch_base = batch.saturating_sub(1).wrapping_mul(CRIT_PER_BATCH);
        if self.cases.len() != batch_base.wrapping_add(no.saturating_sub(1)) {
            return Err(PC_ORDER);
        }
        if read > wrote {
            return Err(PC_UNDER);
        }
        self.cases.push(PipeCase { batch, no, wrote, read, eof: false, replayed: false });
        Ok(())
    }

    /// 关闭写端：必须已读空。
    pub fn close_write(&mut self, batch: usize, no: usize) -> Result<(), u32> {
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(PC_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if c.read != c.wrote {
                return Err(PC_EOF);
            }
            c.eof = true;
            return Ok(());
        }
        Err(PC_NO)
    }

    /// 终重放一条判据。
    pub fn replay(&mut self, batch: usize, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(PC_FROZEN);
        }
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(PC_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.invariant() {
                return Err(PC_UNDER);
            }
            if c.replayed {
                return Err(PC_REPLAY);
            }
            c.replayed = true;
            return Ok(());
        }
        Err(PC_NO)
    }

    /// 判据条数。
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 已重放条数。
    pub fn replayed(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].replayed {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 某批已重放条数。
    pub fn batch_replayed(&self, batch: usize) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].batch == batch && self.cases[k].replayed {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 某批是否二十条全部重放闭合。
    pub fn batch_closed(&self, batch: usize) -> bool {
        if self.batch_replayed(batch) != CRIT_PER_BATCH {
            return false;
        }
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].batch == batch && !self.cases[k].settled() {
                return false;
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 两批是否全部闭合。
    pub fn closed(&self) -> bool {
        self.cases.len() == CRITS && self.replayed() == CRITS && self.batch_closed(1) && self.batch_closed(2)
    }

    /// 冻结：返回折叠根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.root = self.fold();
        self.root
    }

    /// 已冻结否。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// FNV-1a 折叠。
    pub fn fold(&self) -> u64 {
        let mut h = PC_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = (h ^ c.batch as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.wrote as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.read as u64).wrapping_mul(FP_STEP);
            let b = if c.replayed { 1u64 } else { 2u64 };
            h = (h ^ b).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准管道清账账：两批四十条判据登记、关闭并终重放。
pub fn build_pipe() -> PipeClear {
    let mut book = PipeClear::new();
    let mut batch = BATCH_FIRST;
    while batch <= BATCH_LAST {
        let mut no = 1usize;
        while no <= CRIT_PER_BATCH {
            let wrote = no.wrapping_add(12);
            let read = if no % 3 == 0 { wrote.saturating_sub(1) } else { wrote };
            let _ = book.file(batch, no, wrote, read);
            if read == wrote {
                let _ = book.close_write(batch, no);
            }
            let _ = book.replay(batch, no);
            no = no.wrapping_add(1);
        }
        batch = batch.wrapping_add(1);
    }
    book
}

/// 四路对抗：批号越界、判据号越界、重复登记、读出多于写入。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = PipeClear::new();
    let a = a_book.file(3, 1, 8, 8) == Err(PC_BATCH) && a_book.case_count() == 0;
    let mut b_book = PipeClear::new();
    let b = b_book.file(1, 21, 8, 8) == Err(PC_NO) && b_book.case_count() == 0;
    let mut c_book = PipeClear::new();
    let _ = c_book.file(1, 1, 8, 8);
    let c = c_book.file(1, 1, 8, 8) == Err(PC_DUP) && c_book.case_count() == 1;
    let mut d_book = PipeClear::new();
    let d = d_book.file(1, 1, 5, 9) == Err(PC_UNDER) && d_book.case_count() == 0;
    [a, b, c, d]
}

/// F11122 全部检查：管道族判据终重放与逐条闭合。
pub fn run_unx11122_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11122 B01-B02 清账");
    let book = build_pipe();
    let four = inject_four();

    // J1 两批四十条在册。
    s.add(
        "J1-两批在册",
        book.case_count() == CRITS && CRITS == 2 * CRIT_PER_BATCH,
        "B01 与 B02 各 20 条判据，共 40 条在册",
    );
    // J2 终重放全量。
    s.add(
        "J2-终重放",
        book.replayed() == CRITS,
        "四十条判据全部完成终重放",
    );
    // J3 逐批闭合。
    s.add(
        "J3-逐批闭合",
        book.batch_closed(1) && book.batch_closed(2) && book.closed(),
        "B01 与 B02 各 20 条全部清账闭合",
    );
    // J4 批域闸。
    let mut batch_probe = PipeClear::new();
    s.add(
        "J4-批域闸",
        batch_probe.file(0, 1, 8, 8) == Err(PC_BATCH)
            && batch_probe.file(3, 1, 8, 8) == Err(PC_BATCH),
        "批号 0 与 3 越出 B01–B02，登记报 PC_BATCH",
    );
    // J5 号域闸。
    let mut no_probe = PipeClear::new();
    s.add(
        "J5-号域闸",
        no_probe.file(1, 0, 8, 8) == Err(PC_NO)
            && no_probe.file(1, 21, 8, 8) == Err(PC_NO),
        "判据号 0 与 21 越出 1..=20，登记报 PC_NO",
    );
    // J6 重复登记闸。
    let mut dup_probe = PipeClear::new();
    let _ = dup_probe.file(1, 1, 8, 8);
    s.add(
        "J6-重号闸",
        dup_probe.file(1, 1, 8, 8) == Err(PC_DUP) && dup_probe.case_count() == 1,
        "同一判据二次登记报 PC_DUP",
    );
    // J7 顺序闸。
    let mut order_probe = PipeClear::new();
    s.add(
        "J7-顺序闸",
        order_probe.file(2, 1, 8, 8) == Err(PC_ORDER) && order_probe.case_count() == 0,
        "B01 未登记时先登记 B02 报 PC_ORDER",
    );
    // J8 读出闸。
    let mut under_probe = PipeClear::new();
    s.add(
        "J8-读出闸",
        under_probe.file(1, 1, 5, 9) == Err(PC_UNDER) && under_probe.case_count() == 0,
        "读出量多于写入量报 PC_UNDER，判据不入账",
    );
    // J9 关闭闸。
    let mut eof_probe = PipeClear::new();
    let _ = eof_probe.file(1, 1, 9, 4);
    s.add(
        "J9-关闭闸",
        eof_probe.close_write(1, 1) == Err(PC_EOF),
        "管道尚有 5 字节余量时关闭写端报 PC_EOF",
    );
    // J10 重复重放闸。
    let mut twice = PipeClear::new();
    let _ = twice.file(1, 1, 8, 8);
    let first_ok = twice.replay(1, 1).is_ok();
    s.add(
        "J10-重放闸",
        first_ok && twice.replay(1, 1) == Err(PC_REPLAY) && twice.replayed() == 1,
        "同一条判据二次终重放报 PC_REPLAY",
    );
    // J11 冻结拒写。
    let mut sealed = build_pipe();
    let root = sealed.freeze();
    s.add(
        "J11-冻结拒写",
        sealed.is_frozen()
            && sealed.file(1, 1, 8, 8) == Err(PC_FROZEN)
            && sealed.replay(1, 1) == Err(PC_FROZEN)
            && sealed.fold() == root,
        "冻结后登记与重放均报 PC_FROZEN，根保持不变",
    );
    // J12 四路对抗。
    s.add(
        "J12-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "批域、号域、重号、读出越界四路全部被拒",
    );
    // J13 判据渲染。
    let case = PipeCase {
        batch: 1,
        no: 7,
        wrote: 21,
        read: 21,
        eof: true,
        replayed: true,
    };
    s.add(
        "J13-判据渲染",
        case.render() == "P1-07|21/21|E|R" && case.settled(),
        "首批第 7 条判据渲染为 P1-07|21/21|E|R",
    );
    // J14 动态明细。
    s.add_dyn(
        "J14-清账明细",
        book.closed(),
        &format!(
            "条={} 重放={} B01={} B02={}",
            book.case_count(),
            book.replayed(),
            book.batch_replayed(1),
            book.batch_replayed(2)
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11122_pipe_flow() {
        let mut book = build_pipe();
        assert_eq!(book.case_count(), CRITS);
        assert_eq!(book.replayed(), CRITS);
        assert!(book.closed());
        let root = book.freeze();
        assert_eq!(book.fold(), root);
        assert_eq!(book.replay(1, 1), Err(PC_FROZEN));
    }

    #[test]
    fn unx11122_checks_green() {
        let s = run_unx11122_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
