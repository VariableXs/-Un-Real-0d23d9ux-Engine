//! UNX-F12032 STATUS_INSUFFICIENT_RESOURCES 资源耗尽语义（0x19620 段）
//!
//! 判据原文：资源池面——资源按槽位发放（slot unit），池空（余量不足）
//! 即 STATUS_INSUFFICIENT_RESOURCES（0xC000009A）；归还回收槽位；双归
//! 还即账目冲突；发放量核对（在用+空闲==容量）。
//!
//! 结构：资源池（ResPool：容量 × 在用位图 + 发放账）+ 发放/归还双核
//! （四码守卫）+ 对平面 + 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12032_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19620 独占 0x20）。
pub const SEG_BASE: u32 = 0x19620;
pub const RS_EXHAUSTED: u32 = SEG_BASE + 0x01; // 池空
pub const RS_DOUBLE_FREE: u32 = SEG_BASE + 0x02; // 双归还
pub const RS_ZERO_UNIT: u32 = SEG_BASE + 0x03; // 零槽操作
pub const RS_BAD_REC: u32 = SEG_BASE + 0x04; // 伪造发放账
pub const RS_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// STATUS_INSUFFICIENT_RESOURCES 语义钉值。
pub const STATUS_INSUFFICIENT_RESOURCES: u32 = 0xC000_009A;

/// 一笔资源操作的账目。
#[derive(Clone)]
pub struct RsRec {
    pub seq: u32,
    /// 操作码：1=发放 2=归还。
    pub op: u8,
    pub slot: u32,
    pub note: String,
}

impl RsRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 资源池模型（位图面：false=空闲 true=在用）。
pub struct ResPool {
    pub slots: Vec<bool>,
    pub ledger: Vec<RsRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ResPool {
    pub fn new(cap: usize) -> ResPool {
        ResPool {
            slots: core::iter::repeat(false).take(cap).collect(),
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| !*s)
    }

    fn rec(&mut self, op: u8, slot: u32, note: &str) -> Result<(), u32> {
        let r = RsRec { seq: self.seq_next, op, slot, note: note.to_string() };
        if !r.ok() {
            return Err(RS_BAD_REC);
        }
        self.seq_next += 1;
        self.ledger.push(r);
        Ok(())
    }

    /// 发放核：找空闲槽；无空闲即池空。
    pub fn acquire(&mut self, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RS_FROZEN);
        }
        if note.is_empty() {
            return Err(RS_ZERO_UNIT);
        }
        let i = match self.free_slot() {
            Some(i) => i,
            None => return Err(RS_EXHAUSTED),
        };
        self.slots[i] = true;
        self.rec(1, i as u32, note)?;
        Ok(i as u32)
    }

    /// 归还核：在用槽回收；空闲槽双归还即冲突。
    pub fn release(&mut self, slot: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(RS_FROZEN);
        }
        if note.is_empty() {
            return Err(RS_ZERO_UNIT);
        }
        if slot as usize >= self.slots.len() {
            return Err(RS_DOUBLE_FREE);
        }
        if !self.slots[slot as usize] {
            return Err(RS_DOUBLE_FREE);
        }
        self.slots[slot as usize] = false;
        self.rec(2, slot, note)
    }

    /// 在用清点。
    pub fn in_use(&self) -> usize {
        self.slots.iter().filter(|s| **s).count()
    }

    /// 空闲清点。
    pub fn free_count(&self) -> usize {
        self.slots.iter().filter(|s| !*s).count()
    }

    /// 对平面：在用+空闲==容量。
    pub fn tallies(&self) -> bool {
        self.in_use() + self.free_count() == self.slots.len()
    }

    /// 账目对平面：发放笔数==在用+已归还笔数（净额面）。
    pub fn ledger_net(&self) -> i64 {
        let mut net: i64 = 0;
        for r in &self.ledger {
            if r.op == 1 {
                net += 1;
            } else {
                net -= 1;
            }
        }
        net
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for s in &self.slots {
            h = h.wrapping_mul(211).wrapping_add(*s as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut p = ResPool::new(2);
    // 1) 池空：两槽占满后第三发放 → RS_EXHAUSTED。
    let _ = p.acquire("a");
    let _ = p.acquire("b");
    got[0] = p.acquire("c").err() == Some(RS_EXHAUSTED);
    // 2) 双归还 → RS_DOUBLE_FREE。
    let _ = p.release(0, "r");
    got[1] = p.release(0, "r2").err() == Some(RS_DOUBLE_FREE);
    // 3) 越界归还 → RS_DOUBLE_FREE（同守卫面）。
    got[2] = p.release(99, "oob").err() == Some(RS_DOUBLE_FREE);
    // 4) 空留痕 → RS_ZERO_UNIT。
    got[3] = p.acquire("").err() == Some(RS_ZERO_UNIT);
    // 5) 印账后动作 → RS_FROZEN。
    p.seal();
    got[4] = p.acquire("post").err() == Some(RS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12032_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12032");

    // J1 正路径：发二归一发一（槽复用首空闲）。
    let j1 = {
        let mut p = ResPool::new(2);
        let a = p.acquire("a") == Ok(0);
        let b = p.acquire("b") == Ok(1);
        let _ = p.release(0, "r");
        let c = p.acquire("c") == Ok(0);
        a && b && c && p.in_use() == 2
    };
    s.add("J1-槽复用", j1, "首空闲复用");

    // J2 池空拒。
    let j2 = {
        let mut p = ResPool::new(1);
        let _ = p.acquire("a");
        p.acquire("b").err() == Some(RS_EXHAUSTED)
    };
    s.add("J2-池空拒", j2, "INSUFFICIENT_RESOURCES");

    // J3 池空即还即再发。
    let j3 = {
        let mut p = ResPool::new(1);
        let _ = p.acquire("a");
        let _ = p.release(0, "r");
        p.acquire("b") == Ok(0)
    };
    s.add("J3-还后再发", j3, "归还解压");

    // J4 双归还拒。
    let j4 = {
        let mut p = ResPool::new(2);
        let _ = p.acquire("a");
        let _ = p.release(0, "r");
        p.release(0, "r2").err() == Some(RS_DOUBLE_FREE)
    };
    s.add("J4-双归还拒", j4, "冲突账");

    // J5 越界归还拒。
    let j5 = {
        let mut p = ResPool::new(2);
        p.release(2, "oob").err() == Some(RS_DOUBLE_FREE)
    };
    s.add("J5-越界归还拒", j5, "界外同守卫");

    // J6 对平面：位图清点合计==容量。
    let j6 = {
        let p = ResPool::new(8);
        p.tallies() && p.free_count() == 8 && p.in_use() == 0
    };
    s.add("J6-容量对平", j6, "0+8==8");

    // J7 账目净额：发放-归还==在用。
    let j7 = {
        let mut p = ResPool::new(4);
        let _ = p.acquire("a");
        let _ = p.acquire("b");
        let _ = p.release(0, "r");
        p.ledger_net() == 1 && p.in_use() == 1
    };
    s.add("J7-净额对账", j7, "2 发 1 还=1 用");

    // J8 空留痕拒。
    let j8 = {
        let mut p = ResPool::new(2);
        p.acquire("").err() == Some(RS_ZERO_UNIT)
    };
    s.add("J8-空留痕拒", j8, "留痕必填");

    // J9 语义钉值核对。
    let j9 = STATUS_INSUFFICIENT_RESOURCES == 0xC000_009A;
    s.add("J9-钉值核对", j9, "0x9A");

    // J10 印账冻结。
    let j10 = {
        let mut p = ResPool::new(2);
        let _ = p.acquire("a");
        let f1 = p.fold();
        let f2 = p.fold();
        p.seal();
        f1 == f2
            && p.acquire("post").err() == Some(RS_FROZEN)
            && p.release(0, "post").err() == Some(RS_FROZEN)
    };
    s.add("J10-印账冻结", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：池压力面。
    let mut p = ResPool::new(4);
    let _ = p.acquire("s1");
    let _ = p.acquire("s2");
    let _ = p.release(1, "s3");
    s.add("样例占用核对", p.in_use() == 1 && p.free_count() == 3, "1 用 3 空");
    s.add("样例账面合法", p.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12032_pool_flow() {
        let mut p = ResPool::new(2);
        assert_eq!(p.acquire("a"), Ok(0));
        assert_eq!(p.acquire("b"), Ok(1));
        assert_eq!(p.acquire("c"), Err(RS_EXHAUSTED));
        assert!(p.release(0, "r").is_ok());
        assert_eq!(p.release(0, "r2"), Err(RS_DOUBLE_FREE));
        assert_eq!(p.acquire("d"), Ok(0));
    }

    #[test]
    fn unx12032_checks_green() {
        let s = run_unx12032_checks();
        assert!(s.all_passed(), "UNX-F12032 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12032_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
