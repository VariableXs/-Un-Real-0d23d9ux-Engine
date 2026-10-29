//! UNX-F10389 总纲十行翻核（B31–B40 [已深化]+字数回填核）（0x1bb20 段）
//! 锚点：UNX-F10389-J1 总纲十行翻核（B31..B40 深化标记+字数回填非零）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1bb20;
pub const EV_NOT_TEN: u32 = SEG_BASE + 1;
pub const EV_NOT_MARKED: u32 = SEG_BASE + 2;
pub const EV_ZERO_WORDS: u32 = SEG_BASE + 3;
pub const EV_DUP_ROW: u32 = SEG_BASE + 4;
pub const EV_SEALED: u32 = SEG_BASE + 5;
pub const EV_RANGE_OUT: u32 = SEG_BASE + 6;

pub const ROW_ANCHOR: usize = 10;
pub const WORD_ANCHOR: usize = 500;

/// 总纲行：批号 B31..B40，深化标记 + 字数回填
#[derive(Clone, Debug)]
pub struct OutlineRow {
    pub batch_no: usize,
    pub marked_deep: bool,
    pub words: usize,
}

impl OutlineRow {
    pub fn unmarked(batch_no: usize) -> Self {
        OutlineRow { batch_no, marked_deep: false, words: 0 }
    }
    pub fn deep(batch_no: usize, words: usize) -> Self {
        OutlineRow { batch_no, marked_deep: true, words }
    }
}

/// 十行翻核册：B31..B40 十行，逐行翻核
pub struct OutlineFlip {
    rows: Vec<OutlineRow>,
    sealed: bool,
    flipped: Option<usize>,
}

impl OutlineFlip {
    pub fn new() -> Self {
        OutlineFlip { rows: Vec::new(), sealed: false, flipped: None }
    }

    pub fn push(&mut self, r: OutlineRow) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.rows.len() >= ROW_ANCHOR {
            return Err(EV_NOT_TEN);
        }
        if r.batch_no < 31 || r.batch_no > 40 {
            return Err(EV_RANGE_OUT);
        }
        for e in self.rows.iter() {
            if e.batch_no == r.batch_no {
                return Err(EV_DUP_ROW);
            }
        }
        self.rows.push(r);
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.rows.len()
    }

    /// 翻核：十行全标记 [已深化] 且字数≥WORD_ANCHOR 才通过
    pub fn flip(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.rows.len() != ROW_ANCHOR {
            self.sealed = true;
            return Err(EV_NOT_TEN);
        }
        let mut i = 0;
        while i < ROW_ANCHOR {
            if !self.rows[i].marked_deep {
                self.sealed = true;
                return Err(EV_NOT_MARKED);
            }
            if self.rows[i].words < WORD_ANCHOR {
                self.sealed = true;
                return Err(EV_ZERO_WORDS);
            }
            i += 1;
        }
        self.flipped = Some(ROW_ANCHOR);
        self.sealed = true;
        Ok(ROW_ANCHOR)
    }

    pub fn flipped_of(&self) -> Option<usize> {
        self.flipped
    }

    pub fn marked_count(&self) -> usize {
        self.rows.iter().filter(|r| r.marked_deep).count()
    }

    pub fn word_total(&self) -> usize {
        let mut acc = 0;
        let mut i = 0;
        while i < self.rows.len() {
            acc += self.rows[i].words;
            i += 1;
        }
        acc
    }

    /// 字数回填补齐：对字数不足行回填至锚值
    pub fn backfill(&mut self, batch_no: usize, words: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if words == 0 {
            return Err(EV_ZERO_WORDS);
        }
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].batch_no == batch_no {
                self.rows[i].marked_deep = true;
                if words > self.rows[i].words {
                    self.rows[i].words = words;
                }
                return Ok(());
            }
            i += 1;
        }
        Err(EV_RANGE_OUT)
    }
}

/// 均匀十行构造：B31..B40 各 600 字全标记
fn full_outline() -> OutlineFlip {
    let mut o = OutlineFlip::new();
    let mut b = 31;
    while b <= 40 {
        o.push(OutlineRow::deep(b, 600)).unwrap();
        b += 1;
    }
    o
}

pub fn run_unx10389_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10389");
    s.add("J1-段锚", SEG_BASE == 0x1bb20, "总纲翻核段基址 0x1bb20");
    s.add("J2-行数锚", ROW_ANCHOR == 10, "十行锚");
    s.add("J3-字数锚", WORD_ANCHOR == 500, "字数下限锚 500");
    s.add("J4-满册十行", full_outline().count() == 10, "B31..B40 十行入册");
    s.add("J5-批号越界", {
        let mut o = OutlineFlip::new();
        o.push(OutlineRow::deep(30, 600)).unwrap_err() == EV_RANGE_OUT
            && o.push(OutlineRow::deep(41, 600)).unwrap_err() == EV_RANGE_OUT
    }, "B30/B41 拒收");
    s.add("J6-重复行拒收", {
        let mut o = OutlineFlip::new();
        o.push(OutlineRow::deep(31, 600)).unwrap();
        o.push(OutlineRow::deep(31, 600)).unwrap_err() == EV_DUP_ROW
    }, "重复 B31 EV_DUP_ROW");
    s.add("J7-超行拒收", {
        let mut o = full_outline();
        o.push(OutlineRow::deep(31, 600)).unwrap_err() == EV_NOT_TEN
    }, "第十一行 EV_NOT_TEN");
    s.add("J8-满册翻核过", {
        let mut o = full_outline();
        o.flip() == Ok(10)
    }, "十行全标记翻核 Ok(10)");
    s.add("J9-翻核存档", {
        let mut o = full_outline();
        o.flip().unwrap();
        o.flipped_of() == Some(10)
    }, "翻核值存档 10");
    s.add("J10-未标记翻核", {
        let mut o = OutlineFlip::new();
        let mut b = 31;
        while b <= 40 {
            o.push(OutlineRow::unmarked(b)).unwrap();
            b += 1;
        }
        o.flip().unwrap_err() == EV_NOT_MARKED
    }, "十行未标记 EV_NOT_MARKED");
    s.add("J11-字数不足翻核", {
        let mut o = OutlineFlip::new();
        let mut b = 31;
        while b <= 40 {
            let words = if b == 37 { 499 } else { 600 };
            o.push(OutlineRow::deep(b, words)).unwrap();
            b += 1;
        }
        o.flip().unwrap_err() == EV_ZERO_WORDS
    }, "B37 499 字 EV_ZERO_WORDS");
    s.add("J12-翻核封账", {
        let mut o = full_outline();
        o.flip().unwrap();
        o.backfill(31, 900).unwrap_err() == EV_SEALED
    }, "翻核后回填 EV_SEALED");
    s.add("J13-回填补齐", {
        let mut o = OutlineFlip::new();
        let mut b = 31;
        while b <= 40 {
            if b == 35 {
                o.push(OutlineRow::deep(b, 100)).unwrap();
            } else {
                o.push(OutlineRow::deep(b, 600)).unwrap();
            }
            b += 1;
        }
        o.backfill(35, 700).unwrap();
        o.flip() == Ok(10)
    }, "B35 回填 700 后翻核过");
    s.add("J14-回填零字拒收", {
        let mut o = full_outline();
        o.backfill(31, 0).unwrap_err() == EV_ZERO_WORDS
    }, "回填 0 字 EV_ZERO_WORDS");
    s.add("J15-回填未知名", {
        let mut o = full_outline();
        o.backfill(29, 700).unwrap_err() == EV_RANGE_OUT
    }, "B29 不在册 EV_RANGE_OUT");
    s.add("J16-字数总账", {
        let o = full_outline();
        o.word_total() == 6_000 && o.marked_count() == 10
    }, "600×10=6,000 且标记 10/10");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_outline_flip() {
        let mut o = full_outline();
        assert_eq!(o.flip().unwrap(), 10);
        assert_eq!(o.flipped_of(), Some(10));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10389_checks();
        assert!(s.all_passed(), "UNX-F10389 判据全绿");
    }
}
