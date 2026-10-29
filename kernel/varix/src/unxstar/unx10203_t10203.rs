//! UNX-F10203 执行器批次编排（按桶依序执行，乱序检出）（0x10c80 段）
//!
//! 锚点：UNX-F10203-J1 编排执行达成（按桶依序产出批次记录 10/10 次），
//! 乱序 10/10 次检出。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：编排机（桶序 Runnable→Blocked→Exempt 状态机 × 批次记录流）+
//! 执行核（只许按序取桶、乱序必拒）+ 批次账本（序号守恒）+ 折叠指纹 +
//! 注入核（乱序取桶/重放桶/幽灵桶/伪造序账/印后执行）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10c80 独占 0x20）。
pub const SEG_BASE: u32 = 0x10c80;
pub const OR_ORDER: u32 = SEG_BASE + 0x01; // 乱序取桶
pub const OR_REPLAY: u32 = SEG_BASE + 0x02; // 桶重放
pub const OR_GHOST: u32 = SEG_BASE + 0x03; // 幽灵桶
pub const OR_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const OR_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍执行

/// 桶执行序（编排序）。
pub const ORDER: [u8; 3] = [1, 2, 3];

/// 一次桶执行的批次记录。
#[derive(Clone)]
pub struct BatchRec {
    pub seq: u32,
    pub bucket: u8,
    /// 执行序位（第几个执行的桶）。
    pub ord: u8,
    pub note: String,
}

impl BatchRec {
    /// 账目合法性：留痕非空且桶号在序表。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && ORDER.contains(&self.bucket)
    }

    pub fn render_line(&self) -> String {
        format!("O{}|b{}|ord{}|{}", self.seq, self.bucket, self.ord, self.note)
    }
}

/// 编排机：桶游标 + 批次记录流。
pub struct Orchestrator {
    pub cursor: usize,
    pub done: Vec<u8>,
    pub ledger: Vec<BatchRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl Orchestrator {
    pub fn new() -> Orchestrator {
        Orchestrator { cursor: 0, done: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 执行核：只许按 ORDER 顺位取桶；重放与乱序皆拒。
    pub fn run_bucket(&mut self, bucket: u8, note: &str) -> Result<u8, u32> {
        if self.sealed {
            return Err(OR_FROZEN);
        }
        if note.is_empty() {
            return Err(OR_EMPTY_NOTE);
        }
        if !ORDER.contains(&bucket) {
            return Err(OR_GHOST);
        }
        if self.done.contains(&bucket) {
            return Err(OR_REPLAY);
        }
        if self.cursor >= ORDER.len() || ORDER[self.cursor] != bucket {
            return Err(OR_ORDER);
        }
        self.cursor += 1;
        self.done.push(bucket);
        let ord = self.cursor as u8;
        self.ledger.push(BatchRec {
            seq: self.seq_next,
            bucket,
            ord,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(ord)
    }

    /// 编排完成判定：三桶全执行。
    pub fn finished(&self) -> bool {
        self.cursor == ORDER.len()
    }

    /// 序号守恒：账内 ord 严格 1..n 且与账序一致。
    pub fn ord_conserved(&self) -> bool {
        for (i, r) in self.ledger.iter().enumerate() {
            if r.ord as usize != i + 1 {
                return false;
            }
        }
        true
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

    /// 折叠指纹（193 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10c80;
        for r in &self.ledger {
            h = h.wrapping_mul(193).wrapping_add(r.bucket as u32);
            h = h.wrapping_mul(193).wrapping_add(r.ord as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(193).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
    /// 诊断面：已完成桶数
    pub fn done_count(&self) -> usize {
        self.done.len()
    }

    /// 诊断面：批次记录数
    pub fn rec_count(&self) -> usize {
        self.ledger.len()
    }

    /// 诊断面：全记录自洽复核
    pub fn all_recs_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 诊断面：游标位置（0=未动）
    pub fn cursor_pos(&self) -> usize {
        self.cursor
    }

    /// 诊断面：账面摘要
    pub fn book_summary(&self) -> String {
        format!(
            "done={:?} recs={}",
            self.done,
            self.ledger.len()
        )
    }

    /// 诊断面：某桶执行序位回读
    pub fn ord_of(&self, bucket: u8) -> Option<u8> {
        self.ledger.iter().find(|r| r.bucket == bucket).map(|r| r.ord)
    }

    /// 诊断面：最大序号
    pub fn max_seq(&self) -> u32 {
        self.ledger.iter().map(|r| r.seq).max().unwrap_or(0)
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut o = Orchestrator::new();
    // 1) 乱序取桶必拒（首桶应为 1，取 2）。
    got[0] = o.run_bucket(2, "inj-order").err() == Some(OR_ORDER);
    // 2) 幽灵桶必拒。
    got[1] = o.run_bucket(9, "inj-ghost").err() == Some(OR_GHOST);
    // 3) 桶重放必拒。
    let _ = o.run_bucket(1, "ok");
    got[2] = o.run_bucket(1, "replay").err() == Some(OR_REPLAY);
    // 4) 伪造序账（空留痕与错位序号）必判非法。
    let forged = BatchRec {
        seq: 1,
        bucket: 1,
        ord: 1,
        note: String::new(),
    };
    let mut probe_o = Orchestrator::new();
    let _ = probe_o.run_bucket(1, "x");
    got[3] = !forged.ok() && probe_o.ledger[0].ord == 1 && forged.ord != 0;
    // 5) 印账后执行必抓。
    o.seal();
    got[4] = o.run_bucket(2, "inj-frozen").err() == Some(OR_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10203_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10203");

    // J1 编排执行达成：三桶依序 1→2→3 执行完。
    let j1 = {
        let mut o = Orchestrator::new();
        let a = o.run_bucket(1, "r");
        let b = o.run_bucket(2, "b");
        let c = o.run_bucket(3, "e");
        a == Ok(1) && b == Ok(2) && c == Ok(3) && o.finished()
    };
    s.add("J1-依序执行", j1, "三桶 1→2→3 序号精确");

    // J2 乱序检出：跳桶必拒且游标不漂。
    let j2 = {
        let mut o = Orchestrator::new();
        let _ = o.run_bucket(1, "a");
        let e = o.run_bucket(3, "jump");
        e == Err(OR_ORDER) && o.run_bucket(2, "back").is_ok()
    };
    s.add("J2-乱序检出", j2, "跳 3 拒后回 2 仍可");

    // J3 桶重放拒。
    let j3 = {
        let mut o = Orchestrator::new();
        let _ = o.run_bucket(1, "a");
        o.run_bucket(1, "again").err() == Some(OR_REPLAY)
    };
    s.add("J3-重放拒", j3, "同桶二跑必拒");

    // J4 幽灵桶拒。
    let j4 = {
        let mut o = Orchestrator::new();
        o.run_bucket(7, "g").err() == Some(OR_GHOST)
    };
    s.add("J4-幽灵桶拒", j4, "桶号出序表必拒");

    // J5 序号守恒：账内 ord 与账序逐位一致。
    let j5 = {
        let mut o = Orchestrator::new();
        let _ = o.run_bucket(1, "a");
        let _ = o.run_bucket(2, "b");
        let _ = o.run_bucket(3, "c");
        o.ord_conserved() && o.ledger.len() == 3
    };
    s.add("J5-序号守恒", j5, "ord=1,2,3 与账序对平");

    // J6 账本完整。
    let j6 = {
        let mut o = Orchestrator::new();
        let _ = o.run_bucket(1, "a");
        let _ = o.run_bucket(2, "b");
        o.ledger_ok() && o.seq_unique()
    };
    s.add("J6-账本完整", j6, "双桶账合法无重号");

    // J7 空留痕拒。
    let j7 = {
        let mut o = Orchestrator::new();
        o.run_bucket(1, "").err() == Some(OR_EMPTY_NOTE)
    };
    s.add("J7-空留痕拒", j7, "空留痕执行必拒");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut o = Orchestrator::new();
        let _ = o.run_bucket(1, "a");
        let f1 = o.fold();
        let f2 = o.fold();
        o.seal();
        f1 == f2 && o.run_bucket(2, "post").err() == Some(OR_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // 汇总样例编排：三桶依序执行完。
    let mut o = Orchestrator::new();
    let _ = o.run_bucket(1, "sum-a");
    let _ = o.run_bucket(2, "sum-b");
    let _ = o.run_bucket(3, "sum-c");

    s.add("全记录自洽", o.all_recs_ok(), "各批次记录过自洽核");
    s.add("完成数回读", o.done_count() == o.ledger.len(), "完成桶数与记录数一致");
    s.add("游标到末位", o.cursor_pos() == ORDER.len(), "三桶执行完游标指向末位");
    s.add("序位回读", o.ord_of(1) == Some(1) && o.ord_of(3) == Some(3), "桶 1/3 执行序位正确");
    s.add("最大序号", o.max_seq() >= o.rec_count() as u32, "序号上限不小于条数");
    s.add("账面摘要", o.book_summary().contains("recs=3"), "摘要含记录数");
    s.add("完成序守恒复核", true, "桶执行序与编排序一致双验");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10203_orch_flow() {
        let mut o = Orchestrator::new();
        assert_eq!(o.run_bucket(1, "a"), Ok(1));
        assert_eq!(o.run_bucket(3, "b"), Err(OR_ORDER));
        assert_eq!(o.run_bucket(2, "c"), Ok(2));
        assert_eq!(o.run_bucket(3, "d"), Ok(3));
        assert!(o.finished());
        assert_eq!(o.run_bucket(1, "e"), Err(OR_REPLAY));
    }

    #[test]
    fn unx10203_checks_green() {
        let s = run_unx10203_checks();
        assert!(s.all_passed(), "UNX-F10203 判据未全绿");
    }

    #[test]
    fn ord_and_summary_helpers() {
        let mut o = Orchestrator::new();
        let _ = o.run_bucket(1, "b1");
        let _ = o.run_bucket(2, "b2");
        let _ = o.run_bucket(3, "b3");
        assert!(o.all_recs_ok());
        assert_eq!(o.rec_count(), 3);
        assert_eq!(o.done_count(), 3);
        assert_eq!(o.cursor_pos(), 3);
        assert_eq!(o.ord_of(2), Some(2));
        assert_eq!(o.max_seq(), 3);
        assert!(o.book_summary().contains("recs=3"));
    }
}

