//! UNX-F12015 进程配额（PagedPool/NonPagedPool）语义账（0x19400 段）
//!
//! 判据原文：进程配额以池（Paged/NonPaged）分账——预扣（charge）不得
//! 越配额上限；释放（release）不得大于当前占用（下溢拒）；零额操作
//! 不入账；两池账目独立对平（总扣 = 总释放 + 当前占用）。
//!
//! 结构：配额账（QuotaBook：双池占用/上限 + 操作账）+ 扣/释双核
//! （四码守卫）+ 对平面 + 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12015_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19400 独占 0x20）。
pub const SEG_BASE: u32 = 0x19400;
pub const QA_EXCEEDED: u32 = SEG_BASE + 0x01; // 预扣越上限
pub const QA_UNDERFLOW: u32 = SEG_BASE + 0x02; // 释放越占用
pub const QA_ZERO_CHARGE: u32 = SEG_BASE + 0x03; // 零额操作
pub const QA_NO_POOL: u32 = SEG_BASE + 0x04; // 未知池
pub const QA_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 池闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pool {
    Paged = 1,
    NonPaged = 2,
}

/// 一笔配额操作的账目。
#[derive(Clone)]
pub struct QaRec {
    pub seq: u32,
    pub pool: Pool,
    /// 操作码：1=charge 2=release。
    pub op: u8,
    pub bytes: u32,
    pub note: String,
}

impl QaRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0 && self.bytes > 0
    }
}

/// 池占用面。
#[derive(Clone, Copy)]
pub struct PoolUsage {
    pub used: u32,
    pub limit: u32,
}

/// 配额账模型。
pub struct QuotaBook {
    pub paged: PoolUsage,
    pub nonpaged: PoolUsage,
    pub ledger: Vec<QaRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl QuotaBook {
    pub fn new(paged_limit: u32, nonpaged_limit: u32) -> QuotaBook {
        QuotaBook {
            paged: PoolUsage { used: 0, limit: paged_limit },
            nonpaged: PoolUsage { used: 0, limit: nonpaged_limit },
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    fn usage(&self, p: Pool) -> Option<&PoolUsage> {
        match p {
            Pool::Paged => Some(&self.paged),
            Pool::NonPaged => Some(&self.nonpaged),
        }
    }

    fn usage_mut(&mut self, p: Pool) -> Option<&mut PoolUsage> {
        match p {
            Pool::Paged => Some(&mut self.paged),
            Pool::NonPaged => Some(&mut self.nonpaged),
        }
    }

    /// 预扣核：正额 + 未越上限。
    pub fn charge(&mut self, p: Pool, bytes: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(QA_FROZEN);
        }
        if note.is_empty() {
            return Err(QA_NO_POOL);
        }
        if bytes == 0 {
            return Err(QA_ZERO_CHARGE);
        }
        if self.usage(p).is_none() {
            return Err(QA_NO_POOL);
        }
        let u = self.usage(p).unwrap_or(&PoolUsage { used: 0, limit: 0 });
        // 下溢先判语义反向：used + bytes 是否越上限（加法防回绕）。
        if bytes > u.limit - u.used {
            return Err(QA_EXCEEDED);
        }
        let rec = QaRec { seq: self.seq_next, pool: p, op: 1, bytes, note: note.to_string() };
        if !rec.ok() {
            return Err(QA_ZERO_CHARGE);
        }
        if let Some(u) = self.usage_mut(p) {
            u.used += bytes;
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        Ok(())
    }

    /// 释放核：不得大于当前占用。
    pub fn release(&mut self, p: Pool, bytes: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(QA_FROZEN);
        }
        if note.is_empty() {
            return Err(QA_NO_POOL);
        }
        if bytes == 0 {
            return Err(QA_ZERO_CHARGE);
        }
        let u = match self.usage(p) {
            Some(u) => *u,
            None => return Err(QA_NO_POOL),
        };
        if bytes > u.used {
            return Err(QA_UNDERFLOW);
        }
        let rec = QaRec { seq: self.seq_next, pool: p, op: 2, bytes, note: note.to_string() };
        if !rec.ok() {
            return Err(QA_ZERO_CHARGE);
        }
        if let Some(u) = self.usage_mut(p) {
            u.used -= bytes;
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        Ok(())
    }

    /// 占用回读。
    pub fn used_of(&self, p: Pool) -> Option<u32> {
        self.usage(p).map(|u| u.used)
    }

    /// 对平面：双池净额 = 总扣 - 总释放（逐池对账）。
    pub fn tallies(&self) -> bool {
        for p in [Pool::Paged, Pool::NonPaged].iter() {
            let mut net: i64 = 0;
            for r in &self.ledger {
                if r.pool == *p {
                    if r.op == 1 {
                        net += r.bytes as i64;
                    } else {
                        net -= r.bytes as i64;
                    }
                }
            }
            let used = self.usage(*p).map(|u| u.used).unwrap_or(0) as i64;
            if net != used {
                return false;
            }
        }
        true
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
        h = h.wrapping_mul(211).wrapping_add(self.paged.used);
        h = h.wrapping_mul(211).wrapping_add(self.nonpaged.used);
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut q = QuotaBook::new(0x1000, 0x2000);
    // 1) 预扣越上限 → QA_EXCEEDED。
    got[0] = q.charge(Pool::Paged, 0x1001, "over").err() == Some(QA_EXCEEDED);
    // 2) 零额预扣 → QA_ZERO_CHARGE。
    got[1] = q.charge(Pool::Paged, 0, "z").err() == Some(QA_ZERO_CHARGE);
    // 3) 释放越占用 → QA_UNDERFLOW。
    let _ = q.charge(Pool::Paged, 0x100, "c");
    got[2] = q.release(Pool::Paged, 0x200, "u").err() == Some(QA_UNDERFLOW);
    // 4) 恰越一点：占满后再扣 1 → QA_EXCEEDED（边界面）。
    let mut q2 = QuotaBook::new(0x100, 0x100);
    let _ = q2.charge(Pool::NonPaged, 0x100, "full");
    got[3] = q2.charge(Pool::NonPaged, 1, "one").err() == Some(QA_EXCEEDED);
    // 5) 印账后预扣 → QA_FROZEN。
    q.seal();
    got[4] = q.charge(Pool::NonPaged, 1, "post").err() == Some(QA_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12015_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12015");

    // J1 正路径：双池扣释对平。
    let j1 = {
        let mut q = QuotaBook::new(0x1000, 0x1000);
        let _ = q.charge(Pool::Paged, 0x300, "p1");
        let _ = q.charge(Pool::NonPaged, 0x400, "n1");
        let _ = q.release(Pool::Paged, 0x100, "p2");
        q.used_of(Pool::Paged) == Some(0x200)
            && q.used_of(Pool::NonPaged) == Some(0x400)
            && q.tallies()
    };
    s.add("J1-双池对平", j1, "净额=占用");

    // J2 预扣越上限拒。
    let j2 = {
        let mut q = QuotaBook::new(0x100, 0x100);
        q.charge(Pool::Paged, 0x101, "o").err() == Some(QA_EXCEEDED)
    };
    s.add("J2-越上限拒", j2, "超 1 字节必拒");

    // J3 恰满边界收。
    let j3 = {
        let mut q = QuotaBook::new(0x100, 0x100);
        q.charge(Pool::Paged, 0x100, "full").is_ok()
            && q.used_of(Pool::Paged) == Some(0x100)
    };
    s.add("J3-恰满边界", j3, "满额合法");

    // J4 零额操作拒（扣/释双面）。
    let j4 = {
        let mut q = QuotaBook::new(0x100, 0x100);
        let c = q.charge(Pool::Paged, 0, "z").err() == Some(QA_ZERO_CHARGE);
        let r = q.release(Pool::Paged, 0, "z").err() == Some(QA_ZERO_CHARGE);
        c && r
    };
    s.add("J4-零额拒", j4, "零额双面拒");

    // J5 释放越占用拒。
    let j5 = {
        let mut q = QuotaBook::new(0x100, 0x100);
        let _ = q.charge(Pool::NonPaged, 0x10, "c");
        q.release(Pool::NonPaged, 0x11, "r").err() == Some(QA_UNDERFLOW)
    };
    s.add("J5-释放下溢拒", j5, "超占用 1 必拒");

    // J6 池独立性：Paged 扣减不影响 NonPaged。
    let j6 = {
        let mut q = QuotaBook::new(0x100, 0x100);
        let _ = q.charge(Pool::Paged, 0x40, "p");
        q.used_of(Pool::NonPaged) == Some(0)
    };
    s.add("J6-池独立", j6, "双池互不串账");

    // J7 账目留痕：op 1/2 逐笔入账。
    let j7 = {
        let mut q = QuotaBook::new(0x100, 0x100);
        let _ = q.charge(Pool::Paged, 0x40, "c");
        let _ = q.release(Pool::Paged, 0x20, "r");
        q.ledger.len() == 2 && q.ledger[0].op == 1 && q.ledger[1].op == 2 && q.ledger_ok()
    };
    s.add("J7-扣释留痕", j7, "op=1/2 对账");

    // J8 空留痕拒（未知池守卫）。
    let j8 = {
        let mut q = QuotaBook::new(0x100, 0x100);
        q.charge(Pool::Paged, 8, "").err() == Some(QA_NO_POOL)
    };
    s.add("J8-无痕拒", j8, "留痕必填");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = QuotaBook::new(0x100, 0x100);
        let _ = a.charge(Pool::Paged, 0x10, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut b = QuotaBook::new(0x100, 0x100);
        let _ = b.charge(Pool::NonPaged, 0x10, "x");
        f1 == f2 && a.fold() != b.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异账异纹");

    // J10 印账冻结。
    let j10 = {
        let mut q = QuotaBook::new(0x100, 0x100);
        let _ = q.charge(Pool::Paged, 0x10, "a");
        q.seal();
        q.charge(Pool::Paged, 0x10, "post").err() == Some(QA_FROZEN)
            && q.release(Pool::Paged, 0x10, "post").err() == Some(QA_FROZEN)
    };
    s.add("J10-印账冻结", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：配额压力面。
    let mut q = QuotaBook::new(0x400, 0x800);
    let _ = q.charge(Pool::Paged, 0x100, "s1");
    let _ = q.charge(Pool::Paged, 0x100, "s2");
    let _ = q.charge(Pool::NonPaged, 0x200, "s3");
    s.add("样例占用核对", q.used_of(Pool::Paged) == Some(0x200) && q.used_of(Pool::NonPaged) == Some(0x200), "双池 0x200");
    s.add("样例对平", q.tallies(), "净额=占用");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12015_quota_flow() {
        let mut q = QuotaBook::new(0x100, 0x100);
        assert!(q.charge(Pool::Paged, 0x60, "a").is_ok());
        assert_eq!(q.charge(Pool::Paged, 0xA1, "b"), Err(QA_EXCEEDED));
        assert_eq!(q.release(Pool::Paged, 0x60 + 1, "c"), Err(QA_UNDERFLOW));
        assert!(q.release(Pool::Paged, 0x60, "d").is_ok());
        assert_eq!(q.used_of(Pool::Paged), Some(0));
        assert!(q.tallies());
    }

    #[test]
    fn unx12015_checks_green() {
        let s = run_unx12015_checks();
        assert!(s.all_passed(), "UNX-F12015 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12015_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
