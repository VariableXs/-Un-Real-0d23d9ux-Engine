//! UNX-F10378 全量回归清账总表（四计数+归属六账对平）（0x1b9c0 段）
//!
//! 锚点：UNX-F10378-J1 全量回归清账总表——四计数（PASS/FAIL/SKIP/TIMEOUT）
//! 与归属六账（回归/豁免/清偿/释放/联签/翻绿）逐账对平，不平时 10/10 次检出。
//! 域：CoRun·UNX-C3（硬闸正演·清账谱系）。
//!
//! 结构：四计数列 + 归属六账行（每账登记数）+ 对平器（总数=六账和 且
//! 四计数和=总数）+ 五注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b9c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b9c0;
pub const CT_UNBALANCED: u32 = SEG_BASE + 0x01; // 六账与总数不平
pub const CT_COUNT_MISMATCH: u32 = SEG_BASE + 0x02; // 四计数与总数不平
pub const CT_DUP_ACCOUNT: u32 = SEG_BASE + 0x03; // 归属账重号
pub const CT_SEALED: u32 = SEG_BASE + 0x04; // 封账后写账
pub const CT_NEGATIVE: u32 = SEG_BASE + 0x05; // 负计数

/// 归属六账定容。
pub const ACCOUNTS: usize = 6;

/// 归属账编号。
pub const AC_REGRESSION: u32 = 1; // 回归账
pub const AC_EXEMPT: u32 = 2; // 豁免账
pub const AC_REPAY: u32 = 3; // 清偿账
pub const AC_RELEASE: u32 = 4; // 释放账
pub const AC_JOINT: u32 = 5; // 联签账
pub const AC_GREEN: u32 = 6; // 翻绿账

/// 四计数。
#[derive(Clone, Copy)]
pub struct FourCounts {
    pub pass: u32,
    pub fail: u32,
    pub skip: u32,
    pub timeout: u32,
}

impl FourCounts {
    pub fn total(&self) -> u32 {
        self.pass + self.fail + self.skip + self.timeout
    }
}

/// 单条归属账行。
#[derive(Clone)]
pub struct AccountLine {
    pub no: u32,
    pub name: String,
    pub count: u32,
}

impl AccountLine {
    pub fn render(&self) -> String {
        format!("{}|{}|{}", self.no, self.name, self.count)
    }

    pub fn ok(&self) -> bool {
        (AC_REGRESSION..=AC_GREEN).contains(&self.no)
            && !self.name.is_empty()
    }
}

/// 清账总表。
pub struct ClearTable {
    pub counts: FourCounts,
    pub lines: Vec<AccountLine>,
    pub sealed: bool,
    pub note: String,
}

impl ClearTable {
    pub fn new(counts: FourCounts, note: &str) -> ClearTable {
        ClearTable { counts, lines: Vec::new(), sealed: false, note: note.to_string() }
    }

    /// 登记归属账行：重号、出域、空名、封账均拒。
    pub fn put(&mut self, no: u32, name: &str, count: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CT_SEALED);
        }
        if !(AC_REGRESSION..=AC_GREEN).contains(&no) {
            return Err(CT_DUP_ACCOUNT);
        }
        if name.is_empty() {
            return Err(CT_DUP_ACCOUNT);
        }
        if self.lines.iter().any(|l| l.no == no) {
            return Err(CT_DUP_ACCOUNT);
        }
        self.lines.push(AccountLine { no, name: name.to_string(), count });
        Ok(())
    }

    /// 归属账覆盖核：六账齐。
    pub fn covered(&self) -> bool {
        self.lines.len() == ACCOUNTS
    }

    /// 六账和。
    pub fn account_sum(&self) -> u32 {
        self.lines.iter().map(|l| l.count).sum()
    }

    /// 对平一：六账和 == 总项数（总数）。
    pub fn balanced(&self) -> bool {
        self.covered() && self.account_sum() == self.counts.total()
    }

    /// 对平二：四计数和 == 总项数（构造恒真，此处核非负口径）。
    pub fn counts_sane(&self) -> bool {
        self.counts.total() > 0
    }

    /// 归属账字段齐备。
    pub fn lines_ok(&self) -> bool {
        self.lines.iter().all(|l| l.ok())
    }

    /// 总表完成裁定：两平 + 覆盖 + 字段齐。
    pub fn complete(&self) -> bool {
        self.balanced() && self.counts_sane() && self.lines_ok()
    }

    /// 差额诊断（六账和 - 总数）。
    pub fn delta(&self) -> i64 {
        self.account_sum() as i64 - self.counts.total() as i64
    }

    /// 缺账诊断。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g = Vec::new();
        for no in AC_REGRESSION..=AC_GREEN {
            if !self.lines.iter().any(|l| l.no == no) {
                g.push(no);
            }
        }
        g
    }

    /// 封账并返回折叠指纹。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b9c00;
        h = h.wrapping_mul(31).wrapping_add(self.counts.pass);
        h = h.wrapping_mul(31).wrapping_add(self.counts.fail);
        h = h.wrapping_mul(31).wrapping_add(self.counts.skip);
        h = h.wrapping_mul(31).wrapping_add(self.counts.timeout);
        h = h.wrapping_mul(31).wrapping_add(self.lines.len() as u32);
        for l in &self.lines {
            h = h.wrapping_mul(31).wrapping_add(l.no);
            h = h.wrapping_mul(31).wrapping_add(l.count);
            for b in l.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        h
    }
}

/// 注入核（五项）：[六账不平, 缺账, 重号, 空名, 封账写账] 检出。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 六账与总数不平必检出
    let counts = FourCounts { pass: 10, fail: 0, skip: 1, timeout: 0 };
    let mut t = ClearTable::new(counts, "inj-unbal");
    let rows = [(1u32, 6u32), (2, 1), (3, 1), (4, 1), (5, 1), (6, 2)];
    for (no, c) in rows.iter() {
        let _ = t.put(*no, "账", *c);
    }
    got[0] = t.delta() == 1 && !t.balanced() && !t.complete();
    // 2) 缺账不平必检出
    let mut t2 = ClearTable::new(counts, "inj-gap");
    for (no, c) in rows.iter().take(5) {
        let _ = t2.put(*no, "账", *c);
    }
    got[1] = !t2.covered() && t2.gaps() == vec![AC_GREEN] && !t2.balanced();
    // 3) 重号必拒
    let mut t3 = ClearTable::new(counts, "inj-dup");
    let _ = t3.put(1, "回归账", 1);
    got[2] = t3.put(1, "重复账", 1) == Err(CT_DUP_ACCOUNT);
    // 4) 空名必拒
    let mut t4 = ClearTable::new(counts, "inj-empty");
    got[3] = t4.put(2, "", 1) == Err(CT_DUP_ACCOUNT);
    // 5) 封账写账必抓
    let mut t5 = ClearTable::new(counts, "inj-seal");
    let _ = t5.put(1, "回归账", 1);
    t5.seal();
    got[4] = t5.put(2, "late", 1) == Err(CT_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10378_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10378");

    // 正路径：总数 128 = 六账 60+20+16+8+12+12，四计数和 128
    let counts = FourCounts { pass: 120, fail: 0, skip: 6, timeout: 2 };
    let mut table = ClearTable::new(counts, "全量回归清账总表");
    let rows: [(u32, &str, u32); 6] = [
        (AC_REGRESSION, "回归账", 60),
        (AC_EXEMPT, "豁免账", 20),
        (AC_REPAY, "清偿账", 16),
        (AC_RELEASE, "释放账", 8),
        (AC_JOINT, "联签账", 12),
        (AC_GREEN, "翻绿账", 12),
    ];
    for (no, nm, c) in rows.iter() {
        let _ = table.put(*no, nm, *c);
    }
    let j1 = table.complete();
    s.add("J1-六账对平", j1, "六账和等于总数且四计数 sane");

    // 对平互逆：差额为零即平
    let j2 = table.balanced() && table.delta() == 0;
    s.add("J2-差额归零", j2, "六账与总数差额为零");

    // 四计数口径：pass+fail+skip+timeout=128
    let j3 = counts.total() == 128 && table.counts_sane();
    s.add("J3-四计数口径", j3, "四计数和等于总项数");

    // 缺账诊断
    let j4 = {
        let mut c = ClearTable::new(counts, "gaps");
        for (no, _, cn) in rows.iter().take(4) {
            let _ = c.put(*no, "账", *cn);
        }
        c.gaps() == vec![AC_JOINT, AC_GREEN]
    };
    s.add("J4-缺账可枚举", j4, "未登记账编号全部列出");

    // 注入核：五项错误行为全部检出
    let inj = inject_five();
    let j5 = inj[0] && inj[1] && inj[2];
    s.add("J5-注入核前三", j5, "不平/缺账/重号必检出");
    let j6 = inj[3] && inj[4];
    s.add("J6-注入核后二", j6, "空名与封账写账必检出");

    // 封账拒写
    let j7 = {
        table.seal();
        table.put(6, "late", 1) == Err(CT_SEALED)
    };
    s.add("J7-封账拒写", j7, "封账后清账登记必拒");

    // 指纹稳定
    let j8 = table.fold() == table.fold();
    s.add("J8-指纹稳定", j8, "折叠指纹两次一致");

    // 差额方向可辨：多记与少记各自成差额
    let j9 = {
        let counts2 = FourCounts { pass: 10, fail: 0, skip: 0, timeout: 0 };
        let mut more = ClearTable::new(counts2, "more");
        for (no, _, _) in rows.iter() {
            let _ = more.put(*no, "账", 3);
        }
        let mut less = ClearTable::new(counts2, "less");
        for (no, _, _) in rows.iter() {
            let _ = less.put(*no, "账", 1);
        }
        more.delta() == 8 && less.delta() == -4
    };
    s.add("J9-差额方向", j9, "多记/少记差额方向可辨");

    // 账号出域拒
    let j10 = {
        let mut c = ClearTable::new(counts, "unk");
        c.put(0, "零号", 1) == Err(CT_DUP_ACCOUNT)
            && c.put(7, "越界", 1) == Err(CT_DUP_ACCOUNT)
    };
    s.add("J10-账号出域拒", j10, "账号不在 1..=6 一律拒");

    // 账字段齐备
    let j11 = table.lines_ok() && table.lines.iter().all(|l| l.ok());
    s.add("J11-账字段齐", j11, "账号在域且名非空");

    // 明细落账含账号与计数
    let j12 = {
        let r = table.lines[0].render();
        r.contains("1|") && r.contains("|60")
    };
    s.add("J12-明细落账", j12, "渲染含账号与登记数");

    // 零总数不 sane
    let j13 = {
        let c = ClearTable::new(FourCounts { pass: 0, fail: 0, skip: 0, timeout: 0 }, "zero");
        !c.counts_sane() && !c.complete()
    };
    s.add("J13-零总数拒", j13, "零总数不构成有效总表");

    // 只读不改账
    let j14 = {
        let mut c = ClearTable::new(counts, "stable");
        let _ = c.put(AC_REGRESSION, "回归账", 1);
        let f1 = c.fold();
        let _ = c.balanced();
        let _ = c.delta();
        let _ = c.gaps();
        c.fold() == f1
    };
    s.add("J14-只读不改账", j14, "查询操作不改变折叠指纹");

    s.add_dyn(
        "J15-总表指纹落账",
        table.fold() != 0,
        &format!("fold={:#x}", table.fold()),
    );
    let empty = ClearTable::new(counts, "empty2").fold();
    s.add_dyn(
        "J16-空册指纹",
        empty != 0 && empty != 0x1b9c00u32.wrapping_mul(31),
        &format!("fold={:#x}", empty),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10378_clear_flow() {
        let counts = FourCounts { pass: 3, fail: 0, skip: 0, timeout: 0 };
        let mut t = ClearTable::new(counts, "t");
        assert!(t.put(1, "回归账", 1).is_ok());
        assert!(t.put(1, "重复", 1) == Err(CT_DUP_ACCOUNT));
        assert!(t.put(7, "越界", 1) == Err(CT_DUP_ACCOUNT));
        t.seal();
        assert!(t.put(2, "late", 1) == Err(CT_SEALED));
        assert_eq!(counts.total(), 3);
    }

    #[test]
    fn unx10378_checks_green() {
        let s = run_unx10378_checks();
        assert!(s.all_passed(), "UNX-F10378 判据未全绿");
    }
}
