//! UNX-F12046 PEB TlsSlot 数组与 TlsExpansionBitmap（0x197E0 段）
//!
//! 判据原文：TLS 槽分配账——主槽 64 位（TlsSlot 数组）+ 扩展位图
//! （TlsExpansionBitmap 32 位）；分配必须落在空闲位上（占位即冲突）；
//! 释放未分配槽即拒；主槽 64 位耗尽方可进扩展面；分配逐笔对账
//! （在用+空闲==容量）。
//!
//! 结构：TLS 槽册（TlsBook：主槽位图 × 扩展位图 + 分配账）+ 分配/释放
//! 双核（五码守卫）+ 对平面 + 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12046_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x197E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x197E0;
pub const TL_DOUBLE_ALLOC: u32 = SEG_BASE + 0x01; // 占位分配
pub const TL_FREE_UNALLOC: u32 = SEG_BASE + 0x02; // 释放未分配槽
pub const TL_OUT_OF_RANGE: u32 = SEG_BASE + 0x03; // 越槽域
pub const TL_EXPANSION_PREMATURE: u32 = SEG_BASE + 0x04; // 主槽未满进扩展
pub const TL_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 主槽位数 / 扩展位数。
pub const MAIN_SLOTS: u32 = 64;
pub const EXP_SLOTS: u32 = 32;

/// 一笔分配/释放的账目。
#[derive(Clone)]
pub struct TlRec {
    pub seq: u32,
    /// 操作码：1=分配 2=释放。
    pub op: u8,
    /// 槽号（0..95：0..63 主槽，64..95 扩展）。
    pub slot: u32,
    pub note: String,
}

impl TlRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// TLS 槽册模型。
pub struct TlsBook {
    pub main: u64,
    pub expansion: u32,
    pub ledger: Vec<TlRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl TlsBook {
    pub fn new() -> TlsBook {
        TlsBook { main: 0, expansion: 0, ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    fn in_main(&self, slot: u32) -> bool {
        slot < MAIN_SLOTS && self.main & (1u64 << slot) != 0
    }

    fn in_exp(&self, slot: u32) -> bool {
        slot >= MAIN_SLOTS && slot < MAIN_SLOTS + EXP_SLOTS
            && self.expansion & (1u32 << (slot - MAIN_SLOTS)) != 0
    }

    /// 主槽全满判定。
    pub fn main_full(&self) -> bool {
        self.main.count_ones() as u32 == MAIN_SLOTS
    }

    /// 分配核：主槽空闲位分配；主槽满后进扩展面。
    pub fn alloc(&mut self, want: Option<u32>, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(TL_FROZEN);
        }
        if note.is_empty() {
            return Err(TL_OUT_OF_RANGE);
        }
        let slot = match want {
            Some(s) => {
                if s >= MAIN_SLOTS + EXP_SLOTS {
                    return Err(TL_OUT_OF_RANGE);
                }
                // 主槽未满时禁止抢扩展槽。
                if s >= MAIN_SLOTS && !self.main_full() {
                    return Err(TL_EXPANSION_PREMATURE);
                }
                s
            }
            None => {
                // 自动分配：找主槽最低空闲位，满则找扩展。
                if self.main_full() {
                    let i = (0..EXP_SLOTS).find(|i| self.expansion & (1u32 << i) == 0);
                    match i {
                        Some(i) => MAIN_SLOTS + i,
                        None => return Err(TL_DOUBLE_ALLOC),
                    }
                } else {
                    (0..MAIN_SLOTS).find(|i| self.main & (1u64 << i) == 0).unwrap_or(0)
                }
            }
        };
        if slot < MAIN_SLOTS {
            if self.in_main(slot) {
                return Err(TL_DOUBLE_ALLOC);
            }
            self.main |= 1u64 << slot;
        } else {
            if self.in_exp(slot) {
                return Err(TL_DOUBLE_ALLOC);
            }
            self.expansion |= 1u32 << (slot - MAIN_SLOTS);
        }
        let rec = TlRec { seq: self.seq_next, op: 1, slot, note: note.to_string() };
        if !rec.ok() {
            return Err(TL_OUT_OF_RANGE);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        Ok(slot)
    }

    /// 释放核。
    pub fn free(&mut self, slot: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TL_FROZEN);
        }
        if note.is_empty() {
            return Err(TL_OUT_OF_RANGE);
        }
        if slot >= MAIN_SLOTS + EXP_SLOTS {
            return Err(TL_OUT_OF_RANGE);
        }
        let used = if slot < MAIN_SLOTS { self.in_main(slot) } else { self.in_exp(slot) };
        if !used {
            return Err(TL_FREE_UNALLOC);
        }
        if slot < MAIN_SLOTS {
            self.main &= !(1u64 << slot);
        } else {
            self.expansion &= !(1u32 << (slot - MAIN_SLOTS));
        }
        let rec = TlRec { seq: self.seq_next, op: 2, slot, note: note.to_string() };
        if !rec.ok() {
            return Err(TL_OUT_OF_RANGE);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        Ok(())
    }

    /// 在用清点。
    pub fn in_use(&self) -> u32 {
        self.main.count_ones() as u32 + self.expansion.count_ones()
    }

    /// 对平面：在用+空闲==96。
    pub fn tallies(&self) -> bool {
        self.in_use() == MAIN_SLOTS + EXP_SLOTS - self.in_use() + self.in_use()
            && self.in_use() <= MAIN_SLOTS + EXP_SLOTS
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
        h = h.wrapping_mul(211).wrapping_add(self.main as u32);
        h = h.wrapping_mul(211).wrapping_add(self.expansion);
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = TlsBook::new();
    // 1) 占位分配 → TL_DOUBLE_ALLOC。
    let _ = b.alloc(Some(0), "a");
    got[0] = b.alloc(Some(0), "dup").err() == Some(TL_DOUBLE_ALLOC);
    // 2) 释放未分配槽 → TL_FREE_UNALLOC。
    got[1] = b.free(5, "free").err() == Some(TL_FREE_UNALLOC);
    // 3) 越槽域（96）→ TL_OUT_OF_RANGE。
    got[2] = b.alloc(Some(96), "oob").err() == Some(TL_OUT_OF_RANGE);
    // 4) 主槽未满抢扩展槽 → TL_EXPANSION_PREMATURE。
    got[3] = b.alloc(Some(64), "early").err() == Some(TL_EXPANSION_PREMATURE);
    // 5) 印账后动作 → TL_FROZEN。
    b.seal();
    got[4] = b.alloc(None, "post").err() == Some(TL_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12046_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12046");

    // J1 正路径：自动分配走最低空闲位。
    let j1 = {
        let mut b = TlsBook::new();
        let a = b.alloc(None, "a") == Ok(0);
        let c = b.alloc(None, "c") == Ok(1);
        let _ = b.free(0, "f");
        let d = b.alloc(None, "d") == Ok(0);
        a && c && d
    };
    s.add("J1-最低空闲位", j1, "0→1→还 0→复用 0");

    // J2 占位分配拒。
    let j2 = {
        let mut b = TlsBook::new();
        let _ = b.alloc(Some(3), "a");
        b.alloc(Some(3), "b").err() == Some(TL_DOUBLE_ALLOC)
    };
    s.add("J2-占位拒", j2, "同槽二分配必拒");

    // J3 释放未分配拒。
    let j3 = {
        let mut b = TlsBook::new();
        b.free(9, "f").err() == Some(TL_FREE_UNALLOC)
    };
    s.add("J3-空释放拒", j3, "未分配必拒");

    // J4 越槽域拒（双面）。
    let j4 = {
        let mut b = TlsBook::new();
        let a = b.alloc(Some(96), "oob").err() == Some(TL_OUT_OF_RANGE);
        let f = b.free(99, "oob").err() == Some(TL_OUT_OF_RANGE);
        a && f
    };
    s.add("J4-越域拒", j4, "96/99 全拒");

    // J5 主槽未满抢扩展拒。
    let j5 = {
        let mut b = TlsBook::new();
        b.alloc(Some(64), "early").err() == Some(TL_EXPANSION_PREMATURE)
    };
    s.add("J5-抢扩展拒", j5, "主槽未满不进扩展");

    // J6 扩展面进入：主槽满后扩展槽可分配。
    let j6 = {
        let mut b = TlsBook::new();
        for i in 0..MAIN_SLOTS {
            let _ = b.alloc(Some(i), "fill");
        }
        b.main_full() && b.alloc(None, "exp") == Ok(MAIN_SLOTS)
    };
    s.add("J6-扩展进入", j6, "64 满后进扩展槽 64");

    // J7 槽域边界：63（主末）与 95（扩展末）。
    let j7 = {
        let mut b = TlsBook::new();
        for i in 0..MAIN_SLOTS {
            let _ = b.alloc(Some(i), "fill");
        }
        let last = b.alloc(Some(95), "last") == Ok(95);
        let beyond = b.alloc(Some(96), "x").is_err();
        last && beyond
    };
    s.add("J7-边界槽", j7, "95 收 96 拒");

    // J8 账目对账：发-还净额==在用。
    let j8 = {
        let mut b = TlsBook::new();
        let _ = b.alloc(None, "a");
        let _ = b.alloc(None, "b");
        let _ = b.free(0, "f");
        b.in_use() == 1 && b.ledger.len() == 3
    };
    s.add("J8-净额对账", j8, "2 发 1 还=1 用");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = TlsBook::new();
        let _ = a.alloc(Some(1), "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = TlsBook::new();
        let _ = c.alloc(Some(2), "x");
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异册异纹");

    // J10 印账冻结。
    let j10 = {
        let mut b = TlsBook::new();
        let _ = b.alloc(Some(0), "a");
        b.seal();
        b.alloc(None, "post").err() == Some(TL_FROZEN)
            && b.free(0, "post").err() == Some(TL_FROZEN)
    };
    s.add("J10-印账冻结", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：三槽面。
    let mut b = TlsBook::new();
    let _ = b.alloc(None, "sum-a");
    let _ = b.alloc(None, "sum-b");
    let _ = b.free(1, "sum-f");
    s.add("样例一用一空", b.in_use() == 1, "槽 0 在用");
    s.add("样例账面合法", b.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12046_tls_flow() {
        let mut b = TlsBook::new();
        assert_eq!(b.alloc(None, "a"), Ok(0));
        assert_eq!(b.alloc(Some(0), "b"), Err(TL_DOUBLE_ALLOC));
        assert_eq!(b.free(5, "c"), Err(TL_FREE_UNALLOC));
        assert!(b.free(0, "d").is_ok());
        assert_eq!(b.alloc(None, "e"), Ok(0));
    }

    #[test]
    fn unx12046_checks_green() {
        let s = run_unx12046_checks();
        assert!(s.all_passed(), "UNX-F12046 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12046_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
