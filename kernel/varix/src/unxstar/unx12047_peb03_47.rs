//! UNX-F12047 PEB LoaderLock 与 LdrLockLoaderLock 占位语义（0x19800 段）
//!
//! 判据原文：LoaderLock 状态面——锁三态（未锁/持锁×持有者×递归计数）；
//! 递归加锁允许且计数递增；非持有者解锁即拒；递归计满（64 层钉值）
//! 拒再入；解锁必须沿递归计数逐层落零。
//!
//! 结构：LoaderLock（LdrLock：态 × 持有者 × 递归计数）+ 加/解锁双核
//! （四码守卫）+ 锁账（LlRec 留痕）+ 折叠指纹 + 印账冻结 + 注入核 +
//! 判据聚合（run_unx12047_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19800 独占 0x20）。
pub const SEG_BASE: u32 = 0x19800;
pub const LL_UNLOCK_FOREIGN: u32 = SEG_BASE + 0x01; // 非持有者解锁
pub const LL_RECURSION_MAX: u32 = SEG_BASE + 0x02; // 递归计满
pub const LL_BAD_OWNER: u32 = SEG_BASE + 0x03; // 零持有者
pub const LL_BAD_REC: u32 = SEG_BASE + 0x04; // 伪造锁账
pub const LL_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 递归计上限钉值。
pub const LL_RECURSION_CAP: u32 = 64;

/// 一笔锁动作的账目。
#[derive(Clone)]
pub struct LlRec {
    pub seq: u32,
    /// 操作码：1=加锁 2=解锁。
    pub op: u8,
    pub owner: u32,
    pub depth: u32,
    pub note: String,
}

impl LlRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0 && self.owner != 0
    }
}

/// LoaderLock 模型。
pub struct LdrLock {
    pub owner: Option<u32>,
    pub depth: u32,
    pub ledger: Vec<LlRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl LdrLock {
    pub fn new() -> LdrLock {
        LdrLock { owner: None, depth: 0, ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 加锁核：无主即持锁；有主同者递归 +1；计满拒。
    pub fn lock(&mut self, owner: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(LL_FROZEN);
        }
        if note.is_empty() {
            return Err(LL_BAD_OWNER);
        }
        if owner == 0 {
            return Err(LL_BAD_OWNER);
        }
        if let Some(cur) = self.owner {
            if cur == owner {
                if self.depth >= LL_RECURSION_CAP {
                    return Err(LL_RECURSION_MAX);
                }
                self.depth += 1;
            } else {
                return Err(LL_RECURSION_MAX);
            }
        } else {
            self.owner = Some(owner);
            self.depth = 1;
        }
        let rec = LlRec {
            seq: self.seq_next,
            op: 1,
            owner,
            depth: self.depth,
            note: note.to_string(),
        };
        if !rec.ok() {
            return Err(LL_BAD_REC);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        Ok(self.depth)
    }

    /// 解锁核：非持有者拒；逐层递减；落零清主。
    pub fn unlock(&mut self, owner: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(LL_FROZEN);
        }
        if note.is_empty() {
            return Err(LL_BAD_OWNER);
        }
        if owner == 0 {
            return Err(LL_BAD_OWNER);
        }
        match self.owner {
            Some(cur) if cur == owner => {}
            _ => return Err(LL_UNLOCK_FOREIGN),
        }
        // 下溢先判。
        if self.depth == 0 {
            return Err(LL_UNLOCK_FOREIGN);
        }
        self.depth -= 1;
        if self.depth == 0 {
            self.owner = None;
        }
        let rec = LlRec {
            seq: self.seq_next,
            op: 2,
            owner,
            depth: self.depth,
            note: note.to_string(),
        };
        if !rec.ok() {
            return Err(LL_BAD_REC);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        Ok(self.depth)
    }

    /// 锁态查询。
    pub fn is_locked(&self) -> bool {
        self.owner.is_some()
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
        h = h.wrapping_mul(211).wrapping_add(self.owner.unwrap_or(0));
        h = h.wrapping_mul(211).wrapping_add(self.depth);
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut l = LdrLock::new();
    // 1) 未锁先解锁 → LL_UNLOCK_FOREIGN。
    got[0] = l.unlock(0x11, "u").err() == Some(LL_UNLOCK_FOREIGN);
    // 2) 非持有者解锁 → LL_UNLOCK_FOREIGN。
    let _ = l.lock(0x11, "a");
    got[1] = l.unlock(0x22, "u2").err() == Some(LL_UNLOCK_FOREIGN);
    // 3) 零持有者 → LL_BAD_OWNER。
    got[2] = l.lock(0, "z").err() == Some(LL_BAD_OWNER);
    // 4) 递归计满：钉值 64 层后拒再入。
    let mut q = LdrLock::new();
    let mut ok_all = true;
    for _ in 0..LL_RECURSION_CAP {
        if q.lock(0x33, "r").is_err() {
            ok_all = false;
        }
    }
    got[3] = ok_all && q.lock(0x33, "over").err() == Some(LL_RECURSION_MAX);
    // 5) 印账后动作 → LL_FROZEN。
    l.seal();
    got[4] = l.lock(0x11, "post").err() == Some(LL_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12047_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12047");

    // J1 正路径：锁→递归×2→逐层解锁→落零清主。
    let j1 = {
        let mut l = LdrLock::new();
        let a = l.lock(0x11, "l1") == Ok(1);
        let b = l.lock(0x11, "l2") == Ok(2);
        let u1 = l.unlock(0x11, "u1") == Ok(1);
        let u2 = l.unlock(0x11, "u2") == Ok(0);
        a && b && u1 && u2 && !l.is_locked()
    };
    s.add("J1-递归全落", j1, "1→2→1→0 清主");

    // J2 非持有者解锁拒。
    let j2 = {
        let mut l = LdrLock::new();
        let _ = l.lock(0x11, "a");
        l.unlock(0x22, "u").err() == Some(LL_UNLOCK_FOREIGN)
    };
    s.add("J2-外人解锁拒", j2, "持有者面精确");

    // J3 未锁解锁拒。
    let j3 = {
        let mut l = LdrLock::new();
        l.unlock(0x11, "u").err() == Some(LL_UNLOCK_FOREIGN)
    };
    s.add("J3-空锁解锁拒", j3, "未锁先解必拒");

    // J4 递归计满拒。
    let j4 = {
        let mut l = LdrLock::new();
        for _ in 0..LL_RECURSION_CAP {
            let _ = l.lock(0x33, "r");
        }
        l.lock(0x33, "over").err() == Some(LL_RECURSION_MAX)
    };
    s.add("J4-递归计满拒", j4, "64 层上沿");

    // J5 边界收：恰好 64 层合法。
    let j5 = {
        let mut l = LdrLock::new();
        let mut ok = true;
        for _ in 0..LL_RECURSION_CAP {
            if l.lock(0x33, "r").is_err() {
                ok = false;
            }
        }
        ok && l.depth == LL_RECURSION_CAP
    };
    s.add("J5-64 层边界收", j5, "恰满合法");

    // J6 零持有者拒。
    let j6 = { LdrLock::new().lock(0, "z").err() == Some(LL_BAD_OWNER) };
    s.add("J6-零主拒", j6, "零 id 永不持锁");

    // J7 异主加锁拒。
    let j7 = {
        let mut l = LdrLock::new();
        let _ = l.lock(0x11, "a");
        l.lock(0x22, "b").err() == Some(LL_RECURSION_MAX)
    };
    s.add("J7-异主加锁拒", j7, "锁独占");

    // J8 账目留痕：深度逐层对账。
    let j8 = {
        let mut l = LdrLock::new();
        let _ = l.lock(0x11, "a");
        let _ = l.lock(0x11, "b");
        let _ = l.unlock(0x11, "c");
        l.ledger.len() == 3 && l.ledger[1].depth == 2 && l.ledger[2].depth == 1 && l.ledger_ok()
    };
    s.add("J8-深度对账", j8, "1/2/1");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = LdrLock::new();
        let _ = a.lock(0x11, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut b = LdrLock::new();
        let _ = b.lock(0x22, "x");
        f1 == f2 && a.fold() != b.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异锁异纹");

    // J10 印账冻结。
    let j10 = {
        let mut l = LdrLock::new();
        let _ = l.lock(0x11, "a");
        l.seal();
        l.lock(0x11, "post").err() == Some(LL_FROZEN)
            && l.unlock(0x11, "post").err() == Some(LL_FROZEN)
    };
    s.add("J10-印账冻结", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：递归两层现场。
    let mut l = LdrLock::new();
    let _ = l.lock(0x77, "sum-1");
    let _ = l.lock(0x77, "sum-2");
    s.add("样例两层持锁", l.is_locked() && l.depth == 2, "depth=2");
    s.add("样例账面合法", l.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12047_lock_flow() {
        let mut l = LdrLock::new();
        assert_eq!(l.lock(0x11, "a"), Ok(1));
        assert_eq!(l.lock(0x22, "b"), Err(LL_RECURSION_MAX));
        assert_eq!(l.lock(0x11, "c"), Ok(2));
        assert_eq!(l.unlock(0x22, "d"), Err(LL_UNLOCK_FOREIGN));
        assert_eq!(l.unlock(0x11, "e"), Ok(1));
        assert_eq!(l.unlock(0x11, "f"), Ok(0));
        assert!(!l.is_locked());
    }

    #[test]
    fn unx12047_checks_green() {
        let s = run_unx12047_checks();
        assert!(s.all_passed(), "UNX-F12047 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12047_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
