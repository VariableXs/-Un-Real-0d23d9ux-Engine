//! UNX-F10392 六轴产物终清点（B38 清点终版复核）（0x1bb80 段）
//! 锚点：UNX-F10392-J1 六轴产物终清点（六轴逐轴计数复核）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1bb80;
pub const EV_AXIS_OVERFLOW: u32 = SEG_BASE + 1;
pub const EV_ZERO_AXIS: u32 = SEG_BASE + 2;
pub const EV_DUP_AXIS: u32 = SEG_BASE + 3;
pub const EV_NOT_SIX: u32 = SEG_BASE + 4;
pub const EV_SEALED: u32 = SEG_BASE + 5;
pub const EV_SUM_DRIFT: u32 = SEG_BASE + 6;

pub const AXIS_ANCHOR: usize = 6;
pub const SUM_ANCHOR: u64 = 24_000;

/// 六轴编号 1..=6：fs / pty / 进程 / 基建 / 信号 / 对接
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AxisCount {
    pub axis: usize,
    pub artifacts: u64,
}

/// 六轴清点册
pub struct AxisInventory {
    rows: Vec<AxisCount>,
    sealed: bool,
    total: Option<u64>,
}

impl AxisInventory {
    pub fn new() -> Self {
        AxisInventory { rows: Vec::new(), sealed: false, total: None }
    }

    pub fn put(&mut self, row: AxisCount) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.rows.len() >= AXIS_ANCHOR {
            return Err(EV_NOT_SIX);
        }
        if row.axis == 0 || row.axis > AXIS_ANCHOR {
            return Err(EV_AXIS_OVERFLOW);
        }
        if row.artifacts == 0 {
            return Err(EV_ZERO_AXIS);
        }
        for e in self.rows.iter() {
            if e.axis == row.axis {
                return Err(EV_DUP_AXIS);
            }
        }
        self.rows.push(row);
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.rows.len()
    }

    pub fn sum(&self) -> u64 {
        let mut acc: u64 = 0;
        let mut i = 0;
        while i < self.rows.len() {
            acc = acc.wrapping_add(self.rows[i].artifacts);
            i += 1;
        }
        acc
    }

    /// 终清点：六轴齐 + 总和与 SUM_ANCHOR 对平
    pub fn inventory(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.rows.len() != AXIS_ANCHOR {
            self.sealed = true;
            return Err(EV_NOT_SIX);
        }
        let total = self.sum();
        if total != SUM_ANCHOR {
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

    pub fn axis_of(&self, axis: usize) -> Option<u64> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].axis == axis {
                return Some(self.rows[i].artifacts);
            }
            i += 1;
        }
        None
    }
}

/// 均分六轴：每轴 4,000 件
fn uniform_axes() -> AxisInventory {
    let mut inv = AxisInventory::new();
    let mut a = 1;
    while a <= AXIS_ANCHOR {
        inv.put(AxisCount { axis: a, artifacts: 4_000 }).unwrap();
        a += 1;
    }
    inv
}

pub fn run_unx10392_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10392");
    s.add("J1-段锚", SEG_BASE == 0x1bb80, "六轴清点段基址 0x1bb80");
    s.add("J2-轴数锚", AXIS_ANCHOR == 6, "六轴锚");
    s.add("J3-总和锚", SUM_ANCHOR == 24_000, "产物总和锚 24,000");
    s.add("J4-均分六轴", uniform_axes().count() == 6, "六轴齐入册");
    s.add("J5-均分总和", uniform_axes().sum() == 24_000, "6×4,000=24,000");
    s.add("J6-终清点过", {
        let mut inv = uniform_axes();
        inv.inventory() == Ok(24_000)
    }, "六轴对平 Ok(24000)");
    s.add("J7-清点存档", {
        let mut inv = uniform_axes();
        inv.inventory().unwrap();
        inv.recorded_total() == Some(24_000)
    }, "总量存档 24,000");
    s.add("J8-缺轴拒核", {
        let mut inv = uniform_axes();
        inv.rows_pop();
        inv.inventory().unwrap_err() == EV_NOT_SIX
    }, "五轴 EV_NOT_SIX");
    s.add("J9-轴号越界", {
        let mut inv = AxisInventory::new();
        inv.put(AxisCount { axis: 0, artifacts: 1 }).unwrap_err() == EV_AXIS_OVERFLOW
            && inv.put(AxisCount { axis: 7, artifacts: 1 }).unwrap_err() == EV_AXIS_OVERFLOW
    }, "轴 0/7 拒收");
    s.add("J10-零件轴拒收", {
        let mut inv = AxisInventory::new();
        inv.put(AxisCount { axis: 1, artifacts: 0 }).unwrap_err() == EV_ZERO_AXIS
    }, "0 件 EV_ZERO_AXIS");
    s.add("J11-重轴拒收", {
        let mut inv = AxisInventory::new();
        inv.put(AxisCount { axis: 3, artifacts: 10 }).unwrap();
        inv.put(AxisCount { axis: 3, artifacts: 20 }).unwrap_err() == EV_DUP_AXIS
    }, "重轴 EV_DUP_AXIS");
    s.add("J12-超轴拒收", {
        let mut inv = uniform_axes();
        inv.put(AxisCount { axis: 1, artifacts: 1 }).unwrap_err() == EV_NOT_SIX
    }, "第七行 EV_NOT_SIX");
    s.add("J13-总和漂移", {
        let mut inv = AxisInventory::new();
        let mut a = 1;
        while a <= AXIS_ANCHOR {
            let n = if a == 6 { 4_001 } else { 4_000 };
            inv.put(AxisCount { axis: a, artifacts: n }).unwrap();
            a += 1;
        }
        inv.inventory().unwrap_err() == EV_SUM_DRIFT
    }, "24,001 漂移 EV_SUM_DRIFT");
    s.add("J14-轴值可读", {
        let inv = uniform_axes();
        inv.axis_of(2) == Some(4_000) && inv.axis_of(9).is_none()
    }, "轴 2 读 4,000 / 轴 9 无");
    s.add("J15-清点封账", {
        let mut inv = uniform_axes();
        inv.inventory().unwrap();
        inv.put(AxisCount { axis: 1, artifacts: 1 }).unwrap_err() == EV_SEALED
    }, "清点后入册 EV_SEALED");
    s.add("J16-逐轴枚举", {
        let inv = uniform_axes();
        let mut ok = true;
        let mut a = 1;
        while a <= AXIS_ANCHOR {
            ok = ok && inv.axis_of(a) == Some(4_000);
            a += 1;
        }
        ok
    }, "六轴逐轴皆 4,000");
    s
}

impl AxisInventory {
    fn rows_pop(&mut self) {
        self.rows.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_six_axes() {
        let mut inv = uniform_axes();
        assert_eq!(inv.inventory().unwrap(), 24_000);
    }

    #[test]
    fn checks_green() {
        let s = run_unx10392_checks();
        assert!(s.all_passed(), "UNX-F10392 判据全绿");
    }
}
