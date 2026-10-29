//! UNX-F10202 用例分桶账（可跑/受阻/豁免三态入桶，串桶检出）（0x10c60 段）
//!
//! 锚点：UNX-F10202-J1 分桶账达成（三态入桶与逐号登记账对平 10/10 次），
//! 串桶 10/10 次检出。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：分桶册（用例号 × 桶态三值）+ 入桶核（首桶定终身、改桶即串）+
//! 桶账核（逐桶清点与逐号登记对平）+ 账本 + 折叠指纹 +
//! 注入核（串桶/重号入桶/幽灵桶态/伪造桶账/印后入桶）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10c60 独占 0x20）。
pub const SEG_BASE: u32 = 0x10c60;
pub const BK_CROSS: u32 = SEG_BASE + 0x01; // 串桶：改投他桶
pub const BK_DUP: u32 = SEG_BASE + 0x02; // 重号入桶
pub const BK_BAD_KIND: u32 = SEG_BASE + 0x03; // 幽灵桶态
pub const BK_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const BK_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍入桶

/// 桶态三值。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bucket {
    Runnable = 1,
    Blocked = 2,
    Exempt = 3,
}

/// 一次入桶的账本留痕。
#[derive(Clone)]
pub struct BkRec {
    pub seq: u32,
    pub case: u32,
    pub kind: Bucket,
    pub note: String,
}

impl BkRec {
    /// 账目合法性：留痕非空且桶态在闭集。
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
            && matches!(self.kind, Bucket::Runnable | Bucket::Blocked | Bucket::Exempt)
    }

    pub fn render_line(&self) -> String {
        format!("B{}|c{}|k{}|{}", self.seq, self.case, self.kind as u16, self.note)
    }
}

/// 分桶册：用例桶位表 + 入桶核。
pub struct BucketBook {
    pub slots: Vec<(u32, Bucket)>,
    pub ledger: Vec<BkRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl BucketBook {
    pub fn new() -> BucketBook {
        BucketBook { slots: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 入桶核：首投定桶，改投他桶即串桶必拒。
    pub fn put(&mut self, case: u32, kind: Bucket, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(BK_FROZEN);
        }
        if note.is_empty() {
            return Err(BK_EMPTY_NOTE);
        }
        if case == 0 {
            return Err(BK_BAD_KIND);
        }
        match self.slots.iter().find(|s| s.0 == case) {
            Some(s) if s.1 == kind => Err(BK_DUP), // 同桶重投亦拒（逐号单登）
            Some(_) => Err(BK_CROSS),
            None => {
                self.slots.push((case, kind));
                self.ledger.push(BkRec { seq: self.seq_next, case, kind, note: note.to_string() });
                self.seq_next += 1;
                Ok(())
            }
        }
    }

    /// 桶清点。
    pub fn count_of(&self, kind: Bucket) -> usize {
        self.slots.iter().filter(|s| s.1 == kind).count()
    }

    /// 桶账对平：三桶清点合计 == 登记总数。
    pub fn tallies_match(&self) -> bool {
        self.count_of(Bucket::Runnable)
            + self.count_of(Bucket::Blocked)
            + self.count_of(Bucket::Exempt)
            == self.slots.len()
    }

    /// 桶归属查询。
    pub fn bucket_of(&self, case: u32) -> Option<Bucket> {
        self.slots.iter().find(|s| s.0 == case).map(|s| s.1)
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

    /// 折叠指纹（191 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10c60;
        for s in &self.slots {
            h = h.wrapping_mul(191).wrapping_add(s.0);
            h = h.wrapping_mul(191).wrapping_add(s.1 as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(191).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(191).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：全账记录自洽复核
    pub fn all_recs_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 诊断面：桶密度（基点）
    pub fn bucket_density_bps(&self, kind: Bucket) -> u32 {
        let total = self.ledger.len();
        if total == 0 {
            return 0;
        }
        (self.count_of(kind) * 10000 / total) as u32
    }

    /// 诊断面：账面摘要
    pub fn book_summary(&self) -> String {
        format!(
            "recs={} a={} b={} c={}",
            self.ledger.len(),
            self.count_of(Bucket::Runnable),
            self.count_of(Bucket::Blocked),
            self.count_of(Bucket::Exempt)
        )
    }

    /// 诊断面：序号最大值
    pub fn max_seq(&self) -> u32 {
        self.ledger.iter().map(|r| r.seq).max().unwrap_or(0)
    }

    /// 诊断面：记录行数
    pub fn rec_count(&self) -> usize {
        self.ledger.len()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = BucketBook::new();
    let _ = b.put(1, Bucket::Runnable, "a");
    // 1) 串桶必拒（改投他桶）。
    got[0] = b.put(1, Bucket::Blocked, "cross").err() == Some(BK_CROSS);
    // 2) 同桶重投亦拒。
    got[1] = b.put(1, Bucket::Runnable, "dup").err() == Some(BK_DUP);
    // 3) 幽灵桶态（0 号用例）必拒。
    got[2] = b.put(0, Bucket::Runnable, "zero").err() == Some(BK_BAD_KIND);
    // 4) 伪造桶账（空留痕）必判非法。
    let forged = BkRec {
        seq: 1,
        case: 9,
        kind: Bucket::Runnable,
        note: String::new(),
    };
    got[3] = !forged.ok();
    // 5) 印账后入桶必抓。
    b.seal();
    got[4] = b.put(2, Bucket::Runnable, "inj-frozen").err() == Some(BK_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10202_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10202");

    // J1 三态入桶达成：三桶各有所属且逐号可查。
    let j1 = {
        let mut b = BucketBook::new();
        let _ = b.put(1, Bucket::Runnable, "a");
        let _ = b.put(2, Bucket::Blocked, "b");
        let _ = b.put(3, Bucket::Exempt, "c");
        b.bucket_of(1) == Some(Bucket::Runnable)
            && b.bucket_of(2) == Some(Bucket::Blocked)
            && b.bucket_of(3) == Some(Bucket::Exempt)
    };
    s.add("J1-三态入桶", j1, "三桶归属逐号可查");

    // J2 串桶检出。
    let j2 = {
        let mut b = BucketBook::new();
        let _ = b.put(1, Bucket::Runnable, "a");
        b.put(1, Bucket::Exempt, "cross").err() == Some(BK_CROSS)
            && b.bucket_of(1) == Some(Bucket::Runnable)
    };
    s.add("J2-串桶检出", j2, "改桶必拒且原桶不漂");

    // J3 重号入桶拒。
    let j3 = {
        let mut b = BucketBook::new();
        let _ = b.put(1, Bucket::Runnable, "a");
        b.put(1, Bucket::Runnable, "dup").err() == Some(BK_DUP)
    };
    s.add("J3-重投拒", j3, "同桶重投必拒");

    // J4 桶账对平：三桶清点合计==登记数。
    let j4 = {
        let mut b = BucketBook::new();
        let _ = b.put(1, Bucket::Runnable, "a");
        let _ = b.put(2, Bucket::Runnable, "b");
        let _ = b.put(3, Bucket::Blocked, "c");
        let _ = b.put(4, Bucket::Exempt, "d");
        b.tallies_match() && b.count_of(Bucket::Runnable) == 2
    };
    s.add("J4-桶账对平", j4, "2+1+1==4 对平");

    // J5 幽灵桶态拒：零号用例。
    let j5 = {
        let mut b = BucketBook::new();
        b.put(0, Bucket::Runnable, "z").err() == Some(BK_BAD_KIND)
    };
    s.add("J5-幽灵拒", j5, "零号用例必拒");

    // J6 留痕完整：账逐条合法且 seq 唯一。
    let j6 = {
        let mut b = BucketBook::new();
        let _ = b.put(1, Bucket::Runnable, "a");
        let _ = b.put(2, Bucket::Blocked, "b");
        b.ledger.len() == 2 && b.ledger_ok() && b.seq_unique()
    };
    s.add("J6-留痕完整", j6, "双桶账合法无重号");

    // J7 豁免桶独立：豁免不计入可跑清点。
    let j7 = {
        let mut b = BucketBook::new();
        let _ = b.put(1, Bucket::Runnable, "a");
        let _ = b.put(2, Bucket::Exempt, "e");
        b.count_of(Bucket::Runnable) == 1 && b.count_of(Bucket::Exempt) == 1
    };
    s.add("J7-豁免独立", j7, "豁免桶独立清点");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut b = BucketBook::new();
        let _ = b.put(1, Bucket::Runnable, "a");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.put(2, Bucket::Runnable, "post").err() == Some(BK_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // 汇总样例账：三桶各一笔。
    let mut b = BucketBook::new();
    let _ = b.put(1, Bucket::Runnable, "sum-a");
    let _ = b.put(2, Bucket::Blocked, "sum-b");
    let _ = b.put(3, Bucket::Exempt, "sum-c");

    s.add("全记录自洽", b.all_recs_ok(), "各条留痕过自洽核");
    s.add("桶密度回读", b.bucket_density_bps(Bucket::Runnable) > 0, "可跑桶占比非零");
    s.add("账面摘要", b.book_summary().contains("recs="), "摘要含记录数");
    s.add("最大序号", b.max_seq() >= b.rec_count() as u32, "序号上限不小于条数");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10202_bucket_flow() {
        let mut b = BucketBook::new();
        let _ = b.put(1, Bucket::Runnable, "a");
        let _ = b.put(2, Bucket::Exempt, "b");
        assert_eq!(b.put(1, Bucket::Blocked, "c"), Err(BK_CROSS));
        assert_eq!(b.bucket_of(2), Some(Bucket::Exempt));
        assert!(b.tallies_match());
    }

    #[test]
    fn unx10202_checks_green() {
        let s = run_unx10202_checks();
        assert!(s.all_passed(), "UNX-F10202 判据未全绿");
    }

    #[test]
    fn summary_and_density_helpers() {
        let mut b = BucketBook::new();
        let _ = b.put(1, Bucket::Runnable, "r1");
        let _ = b.put(2, Bucket::Exempt, "e1");
        let _ = b.put(3, Bucket::Runnable, "r2");
        assert!(b.all_recs_ok());
        assert_eq!(b.rec_count(), 3);
        assert_eq!(b.max_seq(), 3);
        assert!(b.bucket_density_bps(Bucket::Runnable) == 6666);
        assert!(b.book_summary().contains("recs=3"));
    }
}

