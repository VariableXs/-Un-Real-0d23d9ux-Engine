//! UNX-F12023 STATUS_PENDING 挂起语义与异步完成协议预告（0x19500 段）
//!
//! 判据原文：异步操作三态账——登记（Registered）→ 挂起（Pending，
//! STATUS_PENDING=0x00000103）→ 完成（Completed，终态码）；完成只许
//! 一次（双完成即冲突）；未登记先完成必拒；等待已挂起操作由等待方拉
//! 回终态码；挂起码不是终态（终账不得以 0x103 落地）。
//!
//! 结构：异步操作册（AsyncBook：操作号 × 三态 × 终态码）+ 登记核/
//! 挂起核/完成核（五码守卫）+ 操作账（ApRec 留痕）+ 折叠指纹 + 印账
//! 冻结 + 注入核 + 判据聚合（run_unx12023_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19500 独占 0x20）。
pub const SEG_BASE: u32 = 0x19500;
pub const AP_NO_SUCH: u32 = SEG_BASE + 0x01; // 未登记先动作
pub const AP_DOUBLE_COMPLETE: u32 = SEG_BASE + 0x02; // 双完成
pub const AP_COMPLETE_BEFORE_PENDING: u32 = SEG_BASE + 0x03; // 未挂起先完成
pub const AP_PENDING_AS_FINAL: u32 = SEG_BASE + 0x04; // 挂起码落终账
pub const AP_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// STATUS_PENDING 语义钉值。
pub const STATUS_PENDING: u32 = 0x0000_0103;

/// 异步操作态。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApState {
    Registered = 1,
    Pending = 2,
    Completed = 3,
}

/// 异步操作条目。
#[derive(Clone)]
pub struct ApEntry {
    pub id: u64,
    pub state: ApState,
    pub final_status: Option<u32>,
}

/// 一笔动作的账目。
#[derive(Clone)]
pub struct ApRec {
    pub seq: u32,
    pub id: u64,
    /// 操作码：1=登记 2=挂起 3=完成。
    pub op: u8,
    pub note: String,
}

impl ApRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0 && self.id != 0
    }
}

/// 异步操作册。
pub struct AsyncBook {
    pub entries: Vec<ApEntry>,
    pub ledger: Vec<ApRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl AsyncBook {
    pub fn new() -> AsyncBook {
        AsyncBook { entries: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    fn pos(&self, id: u64) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    fn rec(&mut self, id: u64, op: u8, note: &str) {
        self.ledger.push(ApRec { seq: self.seq_next, id, op, note: note.to_string() });
        self.seq_next += 1;
    }

    /// 登记核。
    pub fn register(&mut self, id: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(AP_FROZEN);
        }
        if note.is_empty() {
            return Err(AP_NO_SUCH);
        }
        if id == 0 || self.pos(id).is_some() {
            return Err(AP_NO_SUCH);
        }
        self.entries.push(ApEntry { id, state: ApState::Registered, final_status: None });
        self.rec(id, 1, note);
        Ok(())
    }

    /// 挂起核：登记态 → 挂起态。
    pub fn mark_pending(&mut self, id: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(AP_FROZEN);
        }
        if note.is_empty() {
            return Err(AP_NO_SUCH);
        }
        let i = match self.pos(id) {
            Some(i) => i,
            None => return Err(AP_NO_SUCH),
        };
        if self.entries[i].state == ApState::Completed {
            return Err(AP_DOUBLE_COMPLETE);
        }
        if self.entries[i].state == ApState::Pending {
            return Err(AP_COMPLETE_BEFORE_PENDING);
        }
        self.entries[i].state = ApState::Pending;
        self.rec(id, 2, note);
        Ok(())
    }

    /// 完成核：挂起态 → 完成态（终态码不得为挂起码）。
    pub fn complete(&mut self, id: u64, status: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(AP_FROZEN);
        }
        if note.is_empty() {
            return Err(AP_NO_SUCH);
        }
        let i = match self.pos(id) {
            Some(i) => i,
            None => return Err(AP_NO_SUCH),
        };
        if self.entries[i].state == ApState::Completed {
            return Err(AP_DOUBLE_COMPLETE);
        }
        if self.entries[i].state == ApState::Registered {
            return Err(AP_COMPLETE_BEFORE_PENDING);
        }
        if status == STATUS_PENDING {
            return Err(AP_PENDING_AS_FINAL);
        }
        self.entries[i].state = ApState::Completed;
        self.entries[i].final_status = Some(status);
        self.rec(id, 3, note);
        Ok(())
    }

    /// 终态码回读。
    pub fn final_of(&self, id: u64) -> Option<u32> {
        self.entries.iter().find(|e| e.id == id).and_then(|e| e.final_status)
    }

    /// 挂起中清点。
    pub fn pending_count(&self) -> usize {
        self.entries.iter().filter(|e| e.state == ApState::Pending).count()
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
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add((e.id & 0xFFFF) as u32);
            h = h.wrapping_mul(211).wrapping_add(e.state as u32);
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
    let mut b = AsyncBook::new();
    let _ = b.register(0x21, "r");
    // 1) 未登记先挂起 → AP_NO_SUCH。
    got[0] = b.mark_pending(0xFEED, "x").err() == Some(AP_NO_SUCH);
    // 2) 未挂起先完成 → AP_COMPLETE_BEFORE_PENDING。
    got[1] = b.complete(0x21, 0, "c").err() == Some(AP_COMPLETE_BEFORE_PENDING);
    // 3) 挂起码落终账 → AP_PENDING_AS_FINAL。
    let _ = b.mark_pending(0x21, "p");
    got[2] = b.complete(0x21, STATUS_PENDING, "p2").err() == Some(AP_PENDING_AS_FINAL);
    // 4) 双完成。
    let _ = b.complete(0x21, 0xC000_0005, "f");
    got[3] = b.complete(0x21, 0, "f2").err() == Some(AP_DOUBLE_COMPLETE);
    // 5) 印账后动作 → AP_FROZEN。
    b.seal();
    got[4] = b.register(0x22, "post").err() == Some(AP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12023_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12023");

    // J1 正路径：登记→挂起→完成三步。
    let j1 = {
        let mut b = AsyncBook::new();
        let _ = b.register(1, "r");
        let _ = b.mark_pending(1, "p");
        let _ = b.complete(1, 0xC000_0005, "f");
        b.final_of(1) == Some(0xC000_0005) && b.pending_count() == 0
    };
    s.add("J1-异步三步", j1, "登记挂起完成全通");

    // J2 未登记动作拒。
    let j2 = {
        let mut b = AsyncBook::new();
        b.mark_pending(9, "p").err() == Some(AP_NO_SUCH)
            && b.complete(9, 0, "c").err() == Some(AP_NO_SUCH)
    };
    s.add("J2-幽灵操作拒", j2, "双面未登记必拒");

    // J3 零号 id 拒（登记面）。
    let j3 = {
        let mut b = AsyncBook::new();
        b.register(0, "z").err() == Some(AP_NO_SUCH)
    };
    s.add("J3-零号拒", j3, "零 id 永不登记");

    // J4 未挂起先完成拒。
    let j4 = {
        let mut b = AsyncBook::new();
        let _ = b.register(2, "r");
        b.complete(2, 0, "c").err() == Some(AP_COMPLETE_BEFORE_PENDING)
    };
    s.add("J4-未挂先完成拒", j4, "态序不可跨");

    // J5 挂起码不落终账。
    let j5 = {
        let mut b = AsyncBook::new();
        let _ = b.register(3, "r");
        let _ = b.mark_pending(3, "p");
        b.complete(3, STATUS_PENDING, "f").err() == Some(AP_PENDING_AS_FINAL)
    };
    s.add("J5-挂起码终账拒", j5, "0x103 非终态");

    // J6 双完成拒。
    let j6 = {
        let mut b = AsyncBook::new();
        let _ = b.register(4, "r");
        let _ = b.mark_pending(4, "p");
        let _ = b.complete(4, 0, "f");
        b.complete(4, 0, "f2").err() == Some(AP_DOUBLE_COMPLETE)
    };
    s.add("J6-双完成拒", j6, "完成单次性");

    // J7 等待面：挂起中操作可清点。
    let j7 = {
        let mut b = AsyncBook::new();
        let _ = b.register(5, "r");
        let _ = b.register(6, "r");
        let _ = b.mark_pending(5, "p");
        let _ = b.mark_pending(6, "p");
        let _ = b.complete(5, 0, "f");
        b.pending_count() == 1 && b.final_of(5) == Some(0)
    };
    s.add("J7-挂起清点", j7, "一挂一完对账");

    // J8 账目留痕：op=1/2/3 三笔。
    let j8 = {
        let mut b = AsyncBook::new();
        let _ = b.register(7, "r");
        let _ = b.mark_pending(7, "p");
        let _ = b.complete(7, 0, "f");
        b.ledger.len() == 3
            && b.ledger[0].op == 1 && b.ledger[1].op == 2 && b.ledger[2].op == 3
            && b.ledger_ok()
    };
    s.add("J8-三笔留痕", j8, "op 1/2/3 对账");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = AsyncBook::new();
        let _ = a.register(1, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = AsyncBook::new();
        let _ = c.register(2, "x");
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异册异纹");

    // J10 印账冻结。
    let j10 = {
        let mut b = AsyncBook::new();
        let _ = b.register(8, "r");
        let _ = b.mark_pending(8, "p");
        b.seal();
        b.complete(8, 0, "post").err() == Some(AP_FROZEN)
            && b.register(9, "post").err() == Some(AP_FROZEN)
    };
    s.add("J10-印账冻结", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：双操作一挂一完。
    let mut b = AsyncBook::new();
    let _ = b.register(0xA1, "sum-r1");
    let _ = b.register(0xA2, "sum-r2");
    let _ = b.mark_pending(0xA1, "sum-p1");
    let _ = b.mark_pending(0xA2, "sum-p2");
    let _ = b.complete(0xA2, 0, "sum-f2");
    s.add("样例一挂一完", b.pending_count() == 1, "0xA1 挂 0xA2 完");
    s.add("样例账面合法", b.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12023_async_flow() {
        let mut b = AsyncBook::new();
        assert!(b.register(1, "r").is_ok());
        assert!(b.mark_pending(1, "p").is_ok());
        assert_eq!(b.complete(1, STATUS_PENDING, "f"), Err(AP_PENDING_AS_FINAL));
        assert!(b.complete(1, 0, "f").is_ok());
        assert_eq!(b.complete(1, 0, "f2"), Err(AP_DOUBLE_COMPLETE));
        assert_eq!(b.final_of(1), Some(0));
    }

    #[test]
    fn unx12023_checks_green() {
        let s = run_unx12023_checks();
        assert!(s.all_passed(), "UNX-F12023 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12023_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
