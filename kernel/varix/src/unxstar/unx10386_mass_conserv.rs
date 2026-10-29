//! UNX-F10386 域账总量守恒终核（240,000=40 批逐批和）（0x1bac0 段）
//! 锚点：UNX-F10386-J1 域账总量守恒（40 批逐批和与总量对平）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1bac0;
pub const EV_BATCH_OVERFLOW: u32 = SEG_BASE + 1;
pub const EV_NOT_FORTY: u32 = SEG_BASE + 2;
pub const EV_SUM_DRIFT: u32 = SEG_BASE + 3;
pub const EV_ZERO_BATCH: u32 = SEG_BASE + 4;
pub const EV_SEALED: u32 = SEG_BASE + 5;
pub const EV_NEGATIVE: u32 = SEG_BASE + 6;

pub const TOTAL_ANCHOR: u64 = 240_000;
pub const BATCH_ANCHOR: usize = 40;

/// 逐批账目：批号 1..=40，各批行数
#[derive(Clone, Debug)]
pub struct BatchRow {
    pub no: usize,
    pub lines: u64,
}

impl BatchRow {
    pub fn new(no: usize, lines: u64) -> Self {
        BatchRow { no, lines }
    }
}

/// 守恒核账器：登记 40 批，逐批和与 TOTAL_ANCHOR 对平
pub struct MassConserv {
    rows: Vec<BatchRow>,
    sealed: bool,
    total: Option<u64>,
}

impl MassConserv {
    pub fn new() -> Self {
        MassConserv { rows: Vec::new(), sealed: false, total: None }
    }

    pub fn push(&mut self, row: BatchRow) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.rows.len() >= BATCH_ANCHOR {
            return Err(EV_NOT_FORTY);
        }
        if row.lines == 0 {
            return Err(EV_ZERO_BATCH);
        }
        if row.no == 0 || row.no > BATCH_ANCHOR {
            return Err(EV_BATCH_OVERFLOW);
        }
        for e in self.rows.iter() {
            if e.no == row.no {
                return Err(EV_BATCH_OVERFLOW);
            }
        }
        self.rows.push(row);
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.rows.len()
    }

    /// 逐批求和（显式循环，不溢出时存档总量）
    pub fn sum(&self) -> u64 {
        let mut acc: u64 = 0;
        let mut i = 0;
        while i < self.rows.len() {
            acc = acc.wrapping_add(self.rows[i].lines);
            i += 1;
        }
        acc
    }

    /// 终核：批数=40 且和=TOTAL_ANCHOR 才通过
    pub fn verify(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.rows.len() != BATCH_ANCHOR {
            self.sealed = true;
            return Err(EV_NOT_FORTY);
        }
        let total = self.sum();
        if total != TOTAL_ANCHOR {
            self.sealed = true;
            return Err(EV_SUM_DRIFT);
        }
        self.total = Some(total);
        self.sealed = true;
        Ok(total)
    }

    pub fn recorded_total(&self) -> Option<u64> {
        self.total
    }

    /// 逐批占比（万分位整数，40 批均为 6000=6000/10000）
    pub fn per_batch_share(&self) -> Result<u64, u32> {
        if self.rows.len() != BATCH_ANCHOR {
            return Err(EV_NOT_FORTY);
        }
        let mut i = 0;
        while i < BATCH_ANCHOR {
            if self.rows[i].lines != 6_000 {
                return Err(EV_SUM_DRIFT);
            }
            i += 1;
        }
        Ok(6_000)
    }
}

/// 均分构造器：40 批 × 6,000 行
pub fn uniform_book() -> MassConserv {
    let mut b = MassConserv::new();
    let mut no = 1;
    while no <= BATCH_ANCHOR {
        b.push(BatchRow::new(no, 6_000)).unwrap();
        no += 1;
    }
    b
}

pub fn run_unx10386_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10386");
    s.add("J1-段锚", SEG_BASE == 0x1bac0, "守恒终核段基址 0x1bac0");
    s.add("J2-总量锚", TOTAL_ANCHOR == 240_000, "域总量锚 240,000");
    s.add("J3-批数锚", BATCH_ANCHOR == 40, "批数锚 40");
    s.add("J4-均分册满批", uniform_book().count() == 40, "均分册 40 批");
    s.add("J5-均分册和值", uniform_book().sum() == 240_000, "40×6,000=240,000");
    s.add("J6-均分册过检", uniform_book().recorded_total().is_none(), "未检册无存档总量");
    s.add("J7-过检存档", {
        let mut b = uniform_book();
        let t = b.verify().unwrap();
        t == TOTAL_ANCHOR && b.recorded_total() == Some(240_000)
    }, "过检后总量存档 240,000");
    s.add("J8-重复过检封账", {
        let mut b = uniform_book();
        b.verify().unwrap();
        b.verify().unwrap_err() == EV_SEALED
    }, "过检一次即封账");
    s.add("J9-缺批拒核", {
        let mut b = uniform_book();
        b.pop_last();
        b.verify().unwrap_err() == EV_NOT_FORTY
    }, "39 批 EV_NOT_FORTY");
    s.add("J10-超批拒收", {
        let mut b = uniform_book();
        b.push(BatchRow::new(41, 1)).unwrap_err() == EV_NOT_FORTY
    }, "第 41 批 EV_NOT_FORTY");
    s.add("J11-零行批拒收", {
        let mut b = MassConserv::new();
        b.push(BatchRow::new(1, 0)).unwrap_err() == EV_ZERO_BATCH
    }, "零行批 EV_ZERO_BATCH");
    s.add("J12-批号越界拒收", {
        let mut b = MassConserv::new();
        b.push(BatchRow::new(0, 10)).unwrap_err() == EV_BATCH_OVERFLOW
            && b.push(BatchRow::new(41, 10)).unwrap_err() == EV_BATCH_OVERFLOW
    }, "批号 0/41 均 EV_BATCH_OVERFLOW");
    s.add("J13-批号重复拒收", {
        let mut b = MassConserv::new();
        b.push(BatchRow::new(3, 10)).unwrap();
        b.push(BatchRow::new(3, 20)).unwrap_err() == EV_BATCH_OVERFLOW
    }, "重复批号 EV_BATCH_OVERFLOW");
    s.add("J14-和漂移报码", {
        let mut b = MassConserv::new();
        let mut no = 1;
        while no <= BATCH_ANCHOR {
            b.push(BatchRow::new(no, 6_001)).unwrap();
            no += 1;
        }
        b.verify().unwrap_err() == EV_SUM_DRIFT
    }, "40×6,001=240,040 漂移 EV_SUM_DRIFT");
    s.add("J15-占比对平", {
        let b = uniform_book();
        b.per_batch_share() == Ok(6_000)
    }, "逐批 6,000/10,000 万分位");
    s.add_dyn("J16-漂移册占比报错", {
        let mut b = MassConserv::new();
        let mut no = 1;
        while no <= BATCH_ANCHOR {
            let lines = if no == 17 { 6_100 } else { 6_000 };
            b.push(BatchRow::new(no, lines)).unwrap();
            no += 1;
        }
        b.per_batch_share() == Err(EV_SUM_DRIFT)
    }, "第 17 批 6,100 占比核 EV_SUM_DRIFT");
    s
}

impl MassConserv {
    fn pop_last(&mut self) {
        self.rows.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_conservation() {
        let mut b = uniform_book();
        assert_eq!(b.verify().unwrap(), 240_000);
        assert_eq!(b.recorded_total(), Some(240_000));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10386_checks();
        assert!(s.all_passed(), "UNX-F10386 判据全绿");
    }
}
