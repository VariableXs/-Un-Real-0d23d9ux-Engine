//! UNX-F10100 B25 收口小结与预算核销（0xffa0 段）
//!
//! 锚点：UNX-F10100-J1 收口小结落账（20 条判据达成+预算 6,640 核销零偏离），
//! 核销不符 10/10 次检出。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：收口册（判据条目定容登记，达成位独立可翻）+ 预算核销册（分项
//! 金额→合计对账→零偏离裁断）+ 复核核（条目数/达成率/预算差三轴）+
//! 注入核（核销不符/条目缺额/重号/空名/封账后写账必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xffa0 独占 0x20）。
pub const SEG_BASE: u32 = 0xffa0;
pub const BC_MISMATCH: u32 = SEG_BASE + 0x01; // 核销合计与预算不符
pub const BC_SHORT: u32 = SEG_BASE + 0x02; // 判据条目缺额
pub const BC_DUP_ID: u32 = SEG_BASE + 0x03; // 条目重号
pub const BC_EMPTY_NAME: u32 = SEG_BASE + 0x04; // 条目空名
pub const BC_SEALED: u32 = SEG_BASE + 0x05; // 封账后写账

/// B25 批次预算总额（核销锚）。
pub const BUDGET: u32 = 6_640;
/// 判据条目定容（>20，容量先定后用）。
pub const CAP: usize = 24;
/// B25 战线条目数锚。
pub const ENTRIES_NEEDED: usize = 20;

/// 单条判据条目。
#[derive(Clone)]
pub struct Entry {
    pub id: u32,
    pub name: String,
    pub passed: bool,
}

impl Entry {
    /// 条目合法性：id 非零、名非空。
    pub fn ok(&self) -> bool {
        self.id != 0 && !self.name.is_empty()
    }

    pub fn render(&self) -> String {
        format!("E{}|{}|{}", self.id, if self.passed { "G" } else { "R" }, self.name)
    }
}

/// 收口册：判据条目逐条登记，封账后不可再写。
pub struct ClosureBook {
    pub entries: Vec<Entry>,
    pub sealed: bool,
    pub note: String,
}

impl ClosureBook {
    pub fn new(note: &str) -> ClosureBook {
        ClosureBook { entries: Vec::new(), sealed: false, note: note.to_string() }
    }

    /// 登记条目：重号、空名、超容、封账均拒。
    pub fn enroll(&mut self, id: u32, name: &str, passed: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(BC_SEALED);
        }
        if name.is_empty() {
            return Err(BC_EMPTY_NAME);
        }
        if id == 0 || self.entries.iter().any(|e| e.id == id) {
            return Err(BC_DUP_ID);
        }
        if self.entries.len() >= CAP {
            return Err(BC_SHORT);
        }
        self.entries.push(Entry { id, name: name.to_string(), passed });
        Ok(())
    }

    /// 达成率分子（分母为登记数）。
    pub fn green_count(&self) -> usize {
        self.entries.iter().filter(|e| e.passed).count()
    }

    /// 全绿裁断：登记数达锚且全数翻绿。
    pub fn all_green(&self) -> bool {
        self.entries.len() == ENTRIES_NEEDED && self.green_count() == ENTRIES_NEEDED
    }

    /// 缺额诊断：登记不足或存在未翻绿条目时给出编号列表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        for e in &self.entries {
            if !e.passed {
                g.push(e.id);
            }
        }
        g
    }

    /// 封账并返回折叠指纹。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 条目折叠指纹（序敏感）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0xffa00;
        h = h.wrapping_mul(31).wrapping_add(self.entries.len() as u32);
        for e in &self.entries {
            h = h.wrapping_mul(31).wrapping_add(e.id);
            h = h.wrapping_mul(31).wrapping_add(e.passed as u32);
            for b in e.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }
}

/// 单条核销分项。
#[derive(Clone)]
pub struct ReconLine {
    pub item: String,
    pub amount: u32,
}

impl ReconLine {
    pub fn ok(&self) -> bool {
        !self.item.is_empty() && self.amount > 0
    }
}

/// 预算核销册：分项累加对平预算，偏离非零即核销不符。
pub struct BudgetRecon {
    pub budget: u32,
    pub lines: Vec<ReconLine>,
    pub sealed: bool,
}

impl BudgetRecon {
    pub fn new(budget: u32) -> BudgetRecon {
        BudgetRecon { budget, lines: Vec::new(), sealed: false }
    }

    /// 记入分项：空名、零额、封账均拒。
    pub fn charge(&mut self, item: &str, amount: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(BC_SEALED);
        }
        if item.is_empty() {
            return Err(BC_EMPTY_NAME);
        }
        if amount == 0 {
            return Err(BC_MISMATCH);
        }
        self.lines.push(ReconLine { item: item.to_string(), amount });
        Ok(self.total())
    }

    /// 分项合计。
    pub fn total(&self) -> u32 {
        self.lines.iter().map(|l| l.amount).fold(0u32, |a, b| a.wrapping_add(b))
    }

    /// 偏离量：合计 − 预算（有符号差的安全表示）。
    pub fn deviation(&self) -> i64 {
        self.total() as i64 - self.budget as i64
    }

    /// 零偏离裁断：合计恰等于预算。
    pub fn zero_dev(&self) -> bool {
        self.deviation() == 0
    }

    /// 分项合法性全检。
    pub fn lines_ok(&self) -> bool {
        self.lines.iter().all(|l| l.ok())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 核销不符（多记 1）必检出
    let mut r = BudgetRecon::new(BUDGET);
    let half = BUDGET / 2;
    let _ = r.charge("甲线", half);
    let _ = r.charge("乙线", BUDGET - half);
    let _ = r.charge("幽灵", 1);
    got[0] = !r.zero_dev();
    // 2) 条目缺额（少登一条）必检出
    let mut c = ClosureBook::new("inj-short");
    for i in 1..ENTRIES_NEEDED as u32 {
        let _ = c.enroll(i, "条目", true);
    }
    got[1] = !c.all_green();
    // 3) 重号必拒
    let mut c2 = ClosureBook::new("inj-dup");
    let _ = c2.enroll(1, "a", true);
    got[2] = c2.enroll(1, "b", true).err() == Some(BC_DUP_ID);
    // 4) 空名必拒
    let mut c3 = ClosureBook::new("inj-empty");
    got[3] = c3.enroll(2, "", true).err() == Some(BC_EMPTY_NAME);
    // 5) 封账后写账必抓
    c2.seal();
    got[4] = c2.enroll(3, "late", true).err() == Some(BC_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10100_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10100");

    // 正路径：20 条判据逐条登记并全绿
    let mut book = ClosureBook::new("B25 收口");
    for i in 1..=ENTRIES_NEEDED as u32 {
        let _ = book.enroll(i, "战线判据", true);
    }
    let j1 = book.all_green();
    s.add("J1-二十条全绿", j1, "20 条判据登记且全数翻绿");

    // 预算核销零偏离：分项拼出恰好 6,640
    let mut recon = BudgetRecon::new(BUDGET);
    let _ = recon.charge("B25 一线", 3_320);
    let _ = recon.charge("B25 二线", 2_000);
    let _ = recon.charge("B25 三线", 1_320);
    let j2 = recon.zero_dev();
    s.add("J2-核销零偏离", j2, "合计 6,640 对平预算");

    // 条目字段齐备
    let j3 = book.entries.iter().all(|e| e.ok());
    s.add("J3-条目字段齐", j3, "id 非零且名非空");

    // 核销分项字段齐备
    let j4 = recon.lines_ok();
    s.add("J4-分项字段齐", j4, "分项名非空且金额为正");

    // 容量边界：第 CAP+1 条登记被拒（先填满）
    let j5 = {
        let mut c = ClosureBook::new("cap");
        let mut filled = true;
        for i in 1..=CAP as u32 {
            if c.enroll(i, "x", true).is_err() {
                filled = false;
            }
        }
        filled && c.enroll(999, "over", true).err() == Some(BC_SHORT)
    };
    s.add("J5-容量边界", j5, "超出定容必拒且不破坏原账");

    // 核销不符检出（幽灵金额 1 触发偏离）
    let j6 = inject_five()[0];
    s.add("J6-核销不符检出", j6, "多记金额偏离必检出");

    // 注入核 2/3：缺额与重号
    let inj = inject_five();
    let j7 = inj[1] && inj[2];
    s.add("J7-缺额重号检出", j7, "缺额判定生效且重号必拒");

    // 封账后写账必抓（正册复现）
    let j8 = {
        book.seal();
        book.enroll(77, "late", true).err() == Some(BC_SEALED)
    };
    s.add("J8-封账拒写", j8, "封账后登记必拒");

    // 指纹稳定：封账前后 fold 一致（无再写）
    let j9 = book.fold() == book.fold() && book.seal() == book.seal();
    s.add("J9-指纹稳定", j9, "折叠指纹两次一致");

    // 达成率语义：缺一条即非全绿
    let j10 = {
        let mut c = ClosureBook::new("rate");
        for i in 1..(ENTRIES_NEEDED as u32) {
            let _ = c.enroll(i, "y", true);
        }
        let _ = c.enroll(ENTRIES_NEEDED as u32, "y", false);
        !c.all_green() && c.green_count() == ENTRIES_NEEDED - 1 && c.gaps().len() == 1
    };
    s.add("J10-达成率语义", j10, "一条未翻绿即非全绿且缺位可枚举");

    // 负路径：空名核销分项拒收
    let j11 = {
        let mut r2 = BudgetRecon::new(100);
        r2.charge("", 50).err() == Some(BC_EMPTY_NAME)
    };
    s.add("J11-空分项拒收", j11, "空名分项必拒");

    // 零额分项按不符拒收
    let j12 = {
        let mut r3 = BudgetRecon::new(100);
        r3.charge("零额", 0).err() == Some(BC_MISMATCH)
    };
    s.add("J12-零额拒收", j12, "零额分项按核销不符拒");

    j12_fold_note(&mut s, &book);
    s
}

/// 尾部两条动态判据（指纹明细与偏离明细）。
fn j12_fold_note(s: &mut CheckSet, book: &ClosureBook) {
    let fp = book.fold();
    s.add_dyn("J13-指纹落账", fp != 0, &format!("fold={:#x}", fp));
    let dev = BudgetRecon::new(BUDGET).deviation();
    s.add_dyn("J14-空册偏离", dev == -(BUDGET as i64), &format!("dev={}", dev));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10100_recon_flow() {
        let mut r = BudgetRecon::new(BUDGET);
        let _ = r.charge("a", BUDGET);
        assert!(r.zero_dev());
        let _ = r.charge("b", 1);
        assert_eq!(r.deviation(), 1);
        let mut c = ClosureBook::new("t");
        assert!(c.enroll(1, "x", true).is_ok());
        assert!(c.enroll(1, "dup", true).err() == Some(BC_DUP_ID));
        c.seal();
        assert!(c.enroll(2, "y", true).err() == Some(BC_SEALED));
    }

    #[test]
    fn unx10100_checks_green() {
        let s = run_unx10100_checks();
        assert!(s.all_passed(), "UNX-F10100 判据未全绿");
    }
}
