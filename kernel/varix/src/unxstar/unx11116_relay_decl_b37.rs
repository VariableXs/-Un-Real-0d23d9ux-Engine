//! UNX-F11116 交接声明账（B37 清账段接力）（0x1f596 段）
//!
//! B36 收官后清账段以 B37 起接力，逐批把前缀行数、本批行数与声明总量
//! 对平。接力账必须批号连续、每批对平、全部接续，任一项缺失即拒绝交接。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11116 独占 0x1f596。
pub const SEG_BASE: u32 = 0x1f596;

/// 批号越出接力域。
pub const RL_RANGE: u32 = SEG_BASE + 0x01;
/// 批号不连续，接力断链。
pub const RL_ORDER: u32 = SEG_BASE + 0x02;
/// 批号重复开账。
pub const RL_DUP: u32 = SEG_BASE + 0x03;
/// 行数三数对不平。
pub const RL_UNBAL: u32 = SEG_BASE + 0x04;
/// 尚有未接续批次。
pub const RL_UNCARRIED: u32 = SEG_BASE + 0x05;
/// 接力账已冻结。
pub const RL_FROZEN: u32 = SEG_BASE + 0x06;
/// 接力批次不足。
pub const RL_THIN: u32 = SEG_BASE + 0x07;
/// 容量溢出。
pub const RL_OVER: u32 = SEG_BASE + 0x08;

/// 接力首批号。
pub const RELAY_FIRST: usize = 37;
/// 接力末批号。
pub const RELAY_LAST: usize = 40;
/// 接力批数。
pub const RELAY_ROWS: usize = 4;
/// 每批行数。
pub const LINES_PER_BATCH: usize = 6000;
/// 容量上限。
pub const CAP: usize = 6;
/// 最终累计行数（40 批 × 6000）。
pub const FINAL_LINES: usize = 240000;
/// 折叠种子。
const RL_TAG: u64 = 0x0000_1f5_9696;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// 单批接力行。
#[derive(Clone, Debug)]
pub struct RelayRow {
    /// 批号（37..=40）。
    pub batch: usize,
    /// 此前累计行数（batch × 6000）。
    pub prefix: usize,
    /// 本批行数。
    pub carry: usize,
    /// 声明总量（prefix + carry）。
    pub declared: usize,
    /// 是否已接续。
    pub carried: bool,
}

impl RelayRow {
    /// 三数是否对平。
    pub fn balanced(&self) -> bool {
        self.prefix.wrapping_add(self.carry) == self.declared
    }

    /// 行渲染，形如 B37|216000+6000=222000|C。
    pub fn render(&self) -> String {
        let flag = if self.carried { "C" } else { "N" };
        format!(
            "B{}|{}+{}={}|{}",
            self.batch, self.prefix, self.carry, self.declared, flag
        )
    }
}

/// 交接声明账。
#[derive(Clone, Debug)]
pub struct RelayBook {
    rows: Vec<RelayRow>,
    frozen: bool,
    root: u64,
}

impl RelayBook {
    /// 建空账。
    pub fn new() -> Self {
        Self { rows: Vec::new(), frozen: false, root: 0 }
    }

    /// 批次前缀行数：(batch - 1) × 6000。
    pub fn prefix_of(batch: usize) -> usize {
        batch.saturating_sub(1).wrapping_mul(LINES_PER_BATCH)
    }

    /// 开一批接力行：域、顺序、重复、容量、冻结五道闸。
    pub fn open(&mut self, batch: usize, carry: usize) -> Result<(), u32> {
        let declared = Self::prefix_of(batch).wrapping_add(carry);
        self.open_with(batch, carry, declared)
    }

    /// 带显式声明量开账，供对平校验使用。
    pub fn open_with(&mut self, batch: usize, carry: usize, declared: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(RL_FROZEN);
        }
        if batch < RELAY_FIRST || batch > RELAY_LAST {
            return Err(RL_RANGE);
        }
        if self.rows.len() >= CAP {
            return Err(RL_OVER);
        }
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].batch == batch {
                return Err(RL_DUP);
            }
            k = k.wrapping_add(1);
        }
        if !self.rows.is_empty() {
            let mut tail = 0usize;
            let mut j = 0usize;
            while j < self.rows.len() {
                if self.rows[j].batch > tail {
                    tail = self.rows[j].batch;
                }
                j = j.wrapping_add(1);
            }
            if batch != tail.wrapping_add(1) {
                return Err(RL_ORDER);
            }
        } else if batch != RELAY_FIRST {
            return Err(RL_ORDER);
        }
        let prefix = Self::prefix_of(batch);
        self.rows.push(RelayRow { batch, prefix, carry, declared, carried: false });
        Ok(())
    }

    /// 接续一批。
    pub fn carry(&mut self, batch: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(RL_FROZEN);
        }
        let mut idx = usize::MAX;
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].batch == batch {
                idx = k;
            }
            k = k.wrapping_add(1);
        }
        if idx == usize::MAX {
            return Err(RL_RANGE);
        }
        if let Some(row) = self.rows.get_mut(idx) {
            row.carried = true;
            return Ok(());
        }
        Err(RL_RANGE)
    }

    /// 行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 已接续行数。
    pub fn carried_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].carried {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 逐行对平：所有行 prefix + carry == declared。
    pub fn all_balanced(&self) -> bool {
        let mut k = 0usize;
        while k < self.rows.len() {
            if !self.rows[k].balanced() {
                return false;
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 批号是否严格连续。
    pub fn sequential(&self) -> bool {
        if self.rows.len() != RELAY_ROWS {
            return false;
        }
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].batch != RELAY_FIRST.wrapping_add(k) {
                return false;
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 末行声明量。
    pub fn tail_declared(&self) -> usize {
        let mut tail = 0usize;
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].declared > tail {
                tail = self.rows[k].declared;
            }
            k = k.wrapping_add(1);
        }
        tail
    }

    /// 交接：批次齐、逐行对平、全部接续、末行到顶。
    pub fn relay(&self) -> Result<(), u32> {
        if self.rows.len() < RELAY_ROWS {
            return Err(RL_THIN);
        }
        if !self.sequential() {
            return Err(RL_ORDER);
        }
        if !self.all_balanced() {
            return Err(RL_UNBAL);
        }
        if self.carried_count() != RELAY_ROWS {
            return Err(RL_UNCARRIED);
        }
        if self.tail_declared() != FINAL_LINES {
            return Err(RL_UNBAL);
        }
        Ok(())
    }

    /// 冻结：返回折叠根。
    pub fn seal(&mut self) -> u64 {
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
        let mut h = RL_TAG;
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = &self.rows[k];
            h = (h ^ r.batch as u64).wrapping_mul(FP_STEP);
            h = (h ^ r.prefix as u64).wrapping_mul(FP_STEP);
            h = (h ^ r.carry as u64).wrapping_mul(FP_STEP);
            h = (h ^ r.declared as u64).wrapping_mul(FP_STEP);
            let b = if r.carried { 1u64 } else { 2u64 };
            h = (h ^ b).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准接力账：B37 至 B40 逐批开账并接续。
pub fn build_relay() -> RelayBook {
    let mut book = RelayBook::new();
    let _ = book.open(37, LINES_PER_BATCH);
    let _ = book.open(38, LINES_PER_BATCH);
    let _ = book.open(39, LINES_PER_BATCH);
    let _ = book.open(40, LINES_PER_BATCH);
    let _ = book.carry(37);
    let _ = book.carry(38);
    let _ = book.carry(39);
    let _ = book.carry(40);
    book
}

/// 四路对抗：越域批、断链批、行数不平、重批开账。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = RelayBook::new();
    let a = a_book.open(36, LINES_PER_BATCH) == Err(RL_RANGE) && a_book.row_count() == 0;
    let mut b_book = RelayBook::new();
    let _ = b_book.open(37, LINES_PER_BATCH);
    let b = b_book.open(39, LINES_PER_BATCH) == Err(RL_ORDER) && b_book.row_count() == 1;
    let mut c_book = RelayBook::new();
    let _ = c_book.open_with(37, LINES_PER_BATCH, 1);
    let _ = c_book.open(38, LINES_PER_BATCH);
    let _ = c_book.open(39, LINES_PER_BATCH);
    let _ = c_book.open(40, LINES_PER_BATCH);
    let _ = c_book.carry(37);
    let _ = c_book.carry(38);
    let _ = c_book.carry(39);
    let _ = c_book.carry(40);
    let c = c_book.relay() == Err(RL_UNBAL) && !c_book.all_balanced();
    let mut d_book = RelayBook::new();
    let _ = d_book.open(37, LINES_PER_BATCH);
    let d = d_book.open(37, LINES_PER_BATCH) == Err(RL_DUP) && d_book.row_count() == 1;
    [a, b, c, d]
}

/// F11116 全部检查：接力账逐项对平与接力断言。
pub fn run_unx11116_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11116 交接声明账");
    let book = build_relay();
    let four = inject_four();

    // J1 接力账四批齐备。
    s.add(
        "J1-四批齐备",
        book.row_count() == RELAY_ROWS && book.sequential(),
        "B37 至 B40 四批依次开账，批号严格连续",
    );
    // J2 逐项对平。
    s.add(
        "J2-逐项对平",
        book.all_balanced() && book.tail_declared() == FINAL_LINES,
        "每批前缀加本批等于声明量，末行累计到 240000",
    );
    // J3 接力断言成立。
    s.add(
        "J3-接力成立",
        book.relay().is_ok() && book.carried_count() == RELAY_ROWS,
        "四批全部接续且逐行对平，交接断言通过",
    );
    // J4 越域闸。
    let mut range_book = RelayBook::new();
    s.add(
        "J4-越域闸",
        range_book.open(36, LINES_PER_BATCH) == Err(RL_RANGE)
            && range_book.open(41, LINES_PER_BATCH) == Err(RL_RANGE),
        "批号 36 与 41 越出 B37–B40，开账报 RL_RANGE",
    );
    // J5 断链闸。
    let mut order_book = RelayBook::new();
    let _ = order_book.open(37, LINES_PER_BATCH);
    s.add(
        "J5-断链闸",
        order_book.open(39, LINES_PER_BATCH) == Err(RL_ORDER) && order_book.row_count() == 1,
        "B37 后直接开 B39 报 RL_ORDER，接力链不允许跳批",
    );
    // J6 重批闸。
    let mut dup_book = RelayBook::new();
    let _ = dup_book.open(37, LINES_PER_BATCH);
    let _ = dup_book.open(38, LINES_PER_BATCH);
    s.add(
        "J6-重批闸",
        dup_book.open(38, LINES_PER_BATCH) == Err(RL_DUP) && dup_book.row_count() == 2,
        "同一批号二次开账报 RL_DUP，在册行数保持 2",
    );
    // J7 行数不平闸。
    let mut unbal = RelayBook::new();
    let _ = unbal.open_with(37, LINES_PER_BATCH, 1);
    let _ = unbal.open(38, LINES_PER_BATCH);
    let _ = unbal.open(39, LINES_PER_BATCH);
    let _ = unbal.open(40, LINES_PER_BATCH);
    let _ = unbal.carry(37);
    let _ = unbal.carry(38);
    let _ = unbal.carry(39);
    let _ = unbal.carry(40);
    s.add(
        "J7-不平闸",
        unbal.relay() == Err(RL_UNBAL) && !unbal.all_balanced(),
        "首行声明量被改坏后对平失败，交接报 RL_UNBAL",
    );
    // J8 未接续闸。
    let mut uncarried = RelayBook::new();
    let _ = uncarried.open(37, LINES_PER_BATCH);
    let _ = uncarried.open(38, LINES_PER_BATCH);
    let _ = uncarried.open(39, LINES_PER_BATCH);
    let _ = uncarried.open(40, LINES_PER_BATCH);
    let _ = uncarried.carry(37);
    let _ = uncarried.carry(38);
    let _ = uncarried.carry(39);
    s.add(
        "J8-接续闸",
        uncarried.relay() == Err(RL_UNCARRIED) && uncarried.carried_count() == 3,
        "仅接续 3 批时交接报 RL_UNCARRIED",
    );
    // J9 前缀行数。
    s.add(
        "J9-前缀行数",
        RelayBook::prefix_of(37) == 216000 && RelayBook::prefix_of(40) == 234000,
        "B37 前缀 216000 行，B40 前缀 234000 行，均按每批 6000 递推",
    );
    // J10 末行到顶。
    s.add(
        "J10-末行到顶",
        book.tail_declared() == FINAL_LINES && FINAL_LINES == 40 * LINES_PER_BATCH,
        "末行声明 240000 行，等于 40 批各 6000 行",
    );
    // J11 冻结拒写。
    let mut sealed = build_relay();
    let root = sealed.seal();
    s.add(
        "J11-冻结拒写",
        sealed.is_frozen()
            && sealed.open(37, LINES_PER_BATCH) == Err(RL_FROZEN)
            && sealed.carry(37) == Err(RL_FROZEN),
        "冻结后开账与接续均报 RL_FROZEN",
    );
    // J12 冻结根。
    s.add(
        "J12-冻结根",
        sealed.fold() == root && root != RL_TAG,
        "冻结根等于冻结时刻的折叠值且不等于种子",
    );
    // J13 四路对抗。
    s.add(
        "J13-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "越域批、断链批、行数不平、重批开账四路全部被拒",
    );
    // J14 行渲染。
    let row = RelayRow {
        batch: 37,
        prefix: 216000,
        carry: 6000,
        declared: 222000,
        carried: true,
    };
    s.add(
        "J14-行渲染",
        row.render() == "B37|216000+6000=222000|C" && row.balanced(),
        "B37 行渲染为 B37|216000+6000=222000|C",
    );
    // J15 动态明细。
    s.add_dyn(
        "J15-接力明细",
        book.relay().is_ok(),
        &format!(
            "批={} 接续={} 末行={} 前缀37={}",
            book.row_count(),
            book.carried_count(),
            book.tail_declared(),
            RelayBook::prefix_of(37)
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11116_relay_flow() {
        let mut book = build_relay();
        assert_eq!(book.row_count(), RELAY_ROWS);
        assert!(book.sequential());
        assert!(book.all_balanced());
        assert!(book.relay().is_ok());
        let root = book.seal();
        assert_eq!(book.fold(), root);
        assert_eq!(book.open(38, LINES_PER_BATCH), Err(RL_FROZEN));
    }

    #[test]
    fn unx11116_checks_green() {
        let s = run_unx11116_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
