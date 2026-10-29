//! UNX-F11124 B05–B06 清账（pty/termios 判据终重放）（0x1f5a4 段）
//!
//! 终端族占 B05 与 B06 两批，每批 20 条属性判据。每条记录基线 lflag、
//! 当前 lflag 与原始模式标记：原始模式必须清 ECHO 与 ICANON，恢复后归位。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11124 独占 0x1f5a4。
pub const SEG_BASE: u32 = 0x1f5a4;

/// 批号越出 B05–B06。
pub const TC_BATCH: u32 = SEG_BASE + 0x01;
/// 判据号越出 1..=20。
pub const TC_NO: u32 = SEG_BASE + 0x02;
/// 判据重复登记。
pub const TC_DUP: u32 = SEG_BASE + 0x03;
/// 原始模式未清回显或规范位。
pub const TC_FLAG: u32 = SEG_BASE + 0x04;
/// 未进入原始模式却请求恢复。
pub const TC_RESTORE: u32 = SEG_BASE + 0x05;
/// 判据重复重放。
pub const TC_REPLAY: u32 = SEG_BASE + 0x06;
/// 清账账已冻结。
pub const TC_FROZEN: u32 = SEG_BASE + 0x07;
/// 批内判据不连续。
pub const TC_ORDER: u32 = SEG_BASE + 0x08;

/// 首批号。
pub const BATCH_FIRST: usize = 5;
/// 末批号。
pub const BATCH_LAST: usize = 6;
/// 每批判据数。
pub const CRIT_PER_BATCH: usize = 20;
/// 判据总数。
pub const CRITS: usize = 40;
/// 回显位。
pub const LFLAG_ECHO: u32 = 0x0000_0008;
/// 规范位。
pub const LFLAG_ICANON: u32 = 0x0000_0002;
/// 基线 lflag。
pub const BASE_LFLAG: u32 = LFLAG_ECHO | LFLAG_ICANON | 0x0000_0001;
/// 折叠种子。
const TC_TAG: u64 = 0x0000_1f5_a4a4;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// 终端属性判据。
#[derive(Clone, Debug)]
pub struct TermCase {
    /// 批号（5 或 6）。
    pub batch: usize,
    /// 批内判据号。
    pub no: usize,
    /// 基线 lflag。
    pub base: u32,
    /// 当前 lflag。
    pub cur: u32,
    /// 是否处于原始模式。
    pub raw: bool,
    /// 是否已终重放。
    pub replayed: bool,
}

impl TermCase {
    /// 原始模式判定：回显与规范位必须清零。
    pub fn raw_ok(&self) -> bool {
        !self.raw || (self.cur & (LFLAG_ECHO | LFLAG_ICANON)) == 0
    }

    /// 是否已归位到基线。
    pub fn restored(&self) -> bool {
        !self.raw && self.cur == self.base
    }

    /// 判据是否闭合：原始模式干净且已归位。
    pub fn settled(&self) -> bool {
        self.raw_ok() && self.restored() && self.replayed
    }

    /// 判据渲染，形如 T5-01|RAW|R。
    pub fn render(&self) -> String {
        let mode = if self.raw { "RAW" } else { "COK" };
        let r = if self.replayed { "R" } else { "N" };
        format!("T{}-{:02}|{}|{}", self.batch, self.no, mode, r)
    }
}

/// 终端族清账账。
#[derive(Clone, Debug)]
pub struct TermClear {
    cases: Vec<TermCase>,
    frozen: bool,
    root: u64,
}

impl TermClear {
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

    /// 登记一条判据：批、号、重复、顺序、冻结五道闸。
    pub fn file(&mut self, batch: usize, no: usize, base: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(TC_FROZEN);
        }
        if batch < BATCH_FIRST || batch > BATCH_LAST {
            return Err(TC_BATCH);
        }
        if no == 0 || no > CRIT_PER_BATCH {
            return Err(TC_NO);
        }
        if self.index_of(batch, no) != usize::MAX {
            return Err(TC_DUP);
        }
        let want = self.cases.len() / CRIT_PER_BATCH + BATCH_FIRST;
        if batch != want {
            return Err(TC_ORDER);
        }
        if self.cases.len() % CRIT_PER_BATCH != no.saturating_sub(1) {
            return Err(TC_ORDER);
        }
        self.cases.push(TermCase { batch, no, base, cur: base, raw: false, replayed: false });
        Ok(())
    }

    /// 进入原始模式：清 ECHO 与 ICANON。
    pub fn apply(&mut self, batch: usize, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(TC_FROZEN);
        }
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(TC_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            c.raw = true;
            c.cur = c.base & !(LFLAG_ECHO | LFLAG_ICANON);
            return Ok(());
        }
        Err(TC_NO)
    }

    /// 恢复基线：必须已进入原始模式。
    pub fn restore(&mut self, batch: usize, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(TC_FROZEN);
        }
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(TC_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.raw {
                return Err(TC_RESTORE);
            }
            c.raw = false;
            c.cur = c.base;
            return Ok(());
        }
        Err(TC_NO)
    }

    /// 终重放一条判据：必须已归位。
    pub fn replay(&mut self, batch: usize, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(TC_FROZEN);
        }
        let idx = self.index_of(batch, no);
        if idx == usize::MAX {
            return Err(TC_NO);
        }
        if let Some(c) = self.cases.get_mut(idx) {
            if !c.raw_ok() {
                return Err(TC_FLAG);
            }
            if !c.restored() {
                return Err(TC_RESTORE);
            }
            if c.replayed {
                return Err(TC_REPLAY);
            }
            c.replayed = true;
            return Ok(());
        }
        Err(TC_NO)
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

    /// 未归位条数。
    pub fn dirty(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if !self.cases[k].restored() {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 是否全部闭合。
    pub fn closed(&self) -> bool {
        if self.cases.len() != CRITS || self.replayed() != CRITS || self.dirty() != 0 {
            return false;
        }
        let mut k = 0usize;
        while k < self.cases.len() {
            if !self.cases[k].settled() {
                return false;
            }
            k = k.wrapping_add(1);
        }
        true
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
        let mut h = TC_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = (h ^ c.batch as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ c.cur as u64).wrapping_mul(FP_STEP);
            let b = if c.replayed { 1u64 } else { 2u64 };
            h = (h ^ b).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准终端清账账：四十条判据逐条进原始模式再恢复并重放。
pub fn build_term() -> TermClear {
    let mut book = TermClear::new();
    let mut batch = BATCH_FIRST;
    while batch <= BATCH_LAST {
        let mut no = 1usize;
        while no <= CRIT_PER_BATCH {
            let _ = book.file(batch, no, BASE_LFLAG);
            let _ = book.apply(batch, no);
            let _ = book.restore(batch, no);
            let _ = book.replay(batch, no);
            no = no.wrapping_add(1);
        }
        batch = batch.wrapping_add(1);
    }
    book
}

/// 四路对抗：批号越界、判据号越界、未进原始模式即恢复、重复登记。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = TermClear::new();
    let a = a_book.file(4, 1, BASE_LFLAG) == Err(TC_BATCH) && a_book.case_count() == 0;
    let mut b_book = TermClear::new();
    let b = b_book.file(5, 21, BASE_LFLAG) == Err(TC_NO) && b_book.case_count() == 0;
    let mut c_book = TermClear::new();
    let _ = c_book.file(5, 1, BASE_LFLAG);
    let c = c_book.restore(5, 1) == Err(TC_RESTORE) && c_book.dirty() == 0;
    let mut d_book = TermClear::new();
    let _ = d_book.file(5, 1, BASE_LFLAG);
    let d = d_book.file(5, 1, BASE_LFLAG) == Err(TC_DUP) && d_book.case_count() == 1;
    [a, b, c, d]
}

/// F11124 全部检查：终端族判据终重放与逐条闭合。
pub fn run_unx11124_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11124 B05-B06 清账");
    let book = build_term();
    let four = inject_four();

    // J1 两批四十条在册。
    s.add(
        "J1-两批在册",
        book.case_count() == CRITS && CRITS == 2 * CRIT_PER_BATCH,
        "B05 与 B06 各 20 条属性判据，共 40 条在册",
    );
    // J2 终重放全量。
    s.add("J2-终重放", book.replayed() == CRITS, "四十条判据全部完成终重放");
    // J3 零残留。
    s.add(
        "J3-零残留",
        book.dirty() == 0 && book.closed(),
        "所有判据恢复后当前 lflag 等于基线，零残留且清账闭合",
    );
    // J4 批域闸。
    let mut batch_probe = TermClear::new();
    s.add(
        "J4-批域闸",
        batch_probe.file(4, 1, BASE_LFLAG) == Err(TC_BATCH)
            && batch_probe.file(7, 1, BASE_LFLAG) == Err(TC_BATCH),
        "批号 4 与 7 越出 B05–B06，登记报 TC_BATCH",
    );
    // J5 判据号闸。
    let mut no_probe = TermClear::new();
    s.add(
        "J5-判据号闸",
        no_probe.file(5, 0, BASE_LFLAG) == Err(TC_NO)
            && no_probe.file(5, 21, BASE_LFLAG) == Err(TC_NO),
        "判据号 0 与 21 越出 1..=20，登记报 TC_NO",
    );
    // J6 原始模式闸。
    let mut raw_probe = TermClear::new();
    let _ = raw_probe.file(5, 1, BASE_LFLAG);
    let _ = raw_probe.apply(5, 1);
    s.add(
        "J6-原始模式闸",
        raw_probe.dirty() == 1 && raw_probe.replay(5, 1) == Err(TC_RESTORE),
        "进原始模式未恢复时重放报 TC_RESTORE，账内留 1 条未归位",
    );
    // J7 回显清理。
    s.add(
        "J7-回显清理",
        (BASE_LFLAG & LFLAG_ECHO) != 0
            && ((BASE_LFLAG & !(LFLAG_ECHO | LFLAG_ICANON)) & (LFLAG_ECHO | LFLAG_ICANON)) == 0,
        "原始模式清掉 ECHO 与 ICANON 两位，基线中两位原本为 1",
    );
    // J8 恢复闸。
    let mut restore_probe = TermClear::new();
    let _ = restore_probe.file(5, 1, BASE_LFLAG);
    s.add(
        "J8-恢复闸",
        restore_probe.restore(5, 1) == Err(TC_RESTORE) && restore_probe.dirty() == 0,
        "未进原始模式就恢复报 TC_RESTORE",
    );
    // J9 重复重放闸。
    let mut twice = TermClear::new();
    let _ = twice.file(5, 1, BASE_LFLAG);
    let _ = twice.apply(5, 1);
    let _ = twice.restore(5, 1);
    let first_ok = twice.replay(5, 1).is_ok();
    s.add(
        "J9-重放闸",
        first_ok && twice.replay(5, 1) == Err(TC_REPLAY) && twice.replayed() == 1,
        "同一条判据二次终重放报 TC_REPLAY",
    );
    // J10 冻结拒写。
    let mut sealed = build_term();
    let root = sealed.freeze();
    s.add(
        "J10-冻结拒写",
        sealed.is_frozen()
            && sealed.file(5, 1, BASE_LFLAG) == Err(TC_FROZEN)
            && sealed.apply(5, 1) == Err(TC_FROZEN)
            && sealed.restore(5, 1) == Err(TC_FROZEN)
            && sealed.fold() == root,
        "冻结后登记、进原始模式、恢复均报 TC_FROZEN",
    );
    // J11 四路对抗。
    s.add(
        "J11-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "批域、判据号、未进原始模式恢复、重号四路全部被拒",
    );
    // J12 判据渲染。
    let case = TermCase {
        batch: 5,
        no: 1,
        base: BASE_LFLAG,
        cur: BASE_LFLAG,
        raw: false,
        replayed: true,
    };
    s.add(
        "J12-判据渲染",
        case.render() == "T5-01|COK|R" && case.settled(),
        "归位后的判据渲染为 T5-01|COK|R",
    );
    // J13 动态明细。
    s.add_dyn(
        "J13-清账明细",
        book.closed(),
        &format!(
            "条={} 重放={} 残留={} 基线={:#x}",
            book.case_count(),
            book.replayed(),
            book.dirty(),
            BASE_LFLAG
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11124_term_flow() {
        let mut book = build_term();
        assert_eq!(book.case_count(), CRITS);
        assert_eq!(book.replayed(), CRITS);
        assert!(book.closed());
        let root = book.freeze();
        assert_eq!(book.fold(), root);
        assert_eq!(book.apply(5, 1), Err(TC_FROZEN));
    }

    #[test]
    fn unx11124_checks_green() {
        let s = run_unx11124_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
