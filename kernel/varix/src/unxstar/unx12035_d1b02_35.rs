//! UNX-F12035 取消与废弃族：CANCELLED/ABANDONED/ABANDONED_WAIT_0（0x19680 段）
//!
//! 判据原文：IRP 取消与互斥废弃双面账——已完成的 IRP 不可取消（取消
//! 竞态守卫）；已取消的 IRP 不可再完成；互斥废弃以弃主标志落地（废弃
//! 后等待者收 ABANDONED_WAIT_0 族码）；未弃互斥的等待收正常 WAIT 码；
//! 双面账逐笔对平。
//!
//! 结构：取消册（IrpBook：IRP × 态）+ 弃互斥账（AbandonBook：互斥 ×
//! 弃主位 × 等待者账）+ 取消/完成/弃主/等待四核（五码守卫）+ 折叠指纹 +
//! 印账冻结 + 注入核 + 判据聚合（run_unx12035_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19680 独占 0x20）。
pub const SEG_BASE: u32 = 0x19680;
pub const CX_COMPLETE_RACE: u32 = SEG_BASE + 0x01; // 完成后取消
pub const CX_CANCEL_COMPLETE: u32 = SEG_BASE + 0x02; // 取消后再完成
pub const CX_NO_SUCH: u32 = SEG_BASE + 0x03; // 未登记 IRP
pub const CX_BAD_REC: u32 = SEG_BASE + 0x04; // 伪造双面账
pub const CX_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 语义钉值。
pub const STATUS_CANCELLED: u32 = 0xC000_0120;
pub const STATUS_ABANDONED_WAIT_0: u32 = 0x0000_0080;
pub const STATUS_WAIT_0: u32 = 0x0000_0000;

/// IRP 态。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IrpState {
    Active = 1,
    Cancelled = 2,
    Completed = 3,
}

/// IRP 条目。
#[derive(Clone)]
pub struct IrpEntry {
    pub id: u64,
    pub state: IrpState,
}

/// 一笔双面账（取消面或弃主面）。
#[derive(Clone)]
pub struct CxRec {
    pub seq: u32,
    /// 面：1=IRP 取消面 2=弃主面。
    pub face: u8,
    pub subject: u64,
    /// 结果码。
    pub code: u32,
    pub note: String,
}

impl CxRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 取消与废弃册。
pub struct CancelBook {
    pub irps: Vec<IrpEntry>,
    /// 弃互斥表：id → 弃主位。
    pub mutexes: Vec<(u64, bool)>,
    pub ledger: Vec<CxRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl CancelBook {
    pub fn new() -> CancelBook {
        CancelBook { irps: Vec::new(), mutexes: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    fn irp_pos(&self, id: u64) -> Option<usize> {
        self.irps.iter().position(|i| i.id == id)
    }

    fn rec(&mut self, face: u8, subject: u64, code: u32, note: &str) -> Result<(), u32> {
        let r = CxRec { seq: self.seq_next, face, subject, code, note: note.to_string() };
        if !r.ok() {
            return Err(CX_BAD_REC);
        }
        self.seq_next += 1;
        self.ledger.push(r);
        Ok(())
    }

    /// IRP 登记。
    pub fn enlist(&mut self, id: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CX_FROZEN);
        }
        if note.is_empty() || id == 0 || self.irp_pos(id).is_some() {
            return Err(CX_NO_SUCH);
        }
        self.irps.push(IrpEntry { id, state: IrpState::Active });
        Ok(())
    }

    /// 取消核：Active → Cancelled。
    pub fn cancel(&mut self, id: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CX_FROZEN);
        }
        if note.is_empty() {
            return Err(CX_NO_SUCH);
        }
        let i = match self.irp_pos(id) {
            Some(i) => i,
            None => return Err(CX_NO_SUCH),
        };
        if self.irps[i].state == IrpState::Completed {
            return Err(CX_COMPLETE_RACE);
        }
        if self.irps[i].state == IrpState::Cancelled {
            return Err(CX_CANCEL_COMPLETE);
        }
        self.irps[i].state = IrpState::Cancelled;
        self.rec(1, id, STATUS_CANCELLED, note)
    }

    /// 完成核：Active → Completed；取消后完成拒。
    pub fn complete(&mut self, id: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CX_FROZEN);
        }
        if note.is_empty() {
            return Err(CX_NO_SUCH);
        }
        let i = match self.irp_pos(id) {
            Some(i) => i,
            None => return Err(CX_NO_SUCH),
        };
        if self.irps[i].state == IrpState::Cancelled {
            return Err(CX_CANCEL_COMPLETE);
        }
        if self.irps[i].state == IrpState::Completed {
            return Err(CX_CANCEL_COMPLETE);
        }
        self.irps[i].state = IrpState::Completed;
        self.rec(1, id, STATUS_WAIT_0, note)
    }

    /// 互斥登记。
    pub fn enlist_mutex(&mut self, id: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CX_FROZEN);
        }
        if note.is_empty() || id == 0 || self.mutexes.iter().any(|m| m.0 == id) {
            return Err(CX_NO_SUCH);
        }
        self.mutexes.push((id, false));
        Ok(())
    }

    /// 弃主核：互斥持主消失 → 弃主位置位，等待者收 ABANDONED_WAIT_0。
    pub fn abandon(&mut self, id: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CX_FROZEN);
        }
        if note.is_empty() {
            return Err(CX_NO_SUCH);
        }
        let i = match self.mutexes.iter().position(|m| m.0 == id) {
            Some(i) => i,
            None => return Err(CX_NO_SUCH),
        };
        if self.mutexes[i].1 {
            return Err(CX_BAD_REC);
        }
        self.mutexes[i].1 = true;
        self.rec(2, id, STATUS_ABANDONED_WAIT_0, note)
    }

    /// 等待核：未弃互斥收 WAIT_0、已弃收 ABANDONED_WAIT_0。
    pub fn wait_on(&mut self, id: u64, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CX_FROZEN);
        }
        if note.is_empty() {
            return Err(CX_NO_SUCH);
        }
        let i = match self.mutexes.iter().position(|m| m.0 == id) {
            Some(i) => i,
            None => return Err(CX_NO_SUCH),
        };
        let code = if self.mutexes[i].1 { STATUS_ABANDONED_WAIT_0 } else { STATUS_WAIT_0 };
        self.rec(2, id, code, note)?;
        Ok(code)
    }

    /// 拒面清点：取消面/弃主面。
    pub fn count_face(&self, face: u8) -> usize {
        self.ledger.iter().filter(|r| r.face == face).count()
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
        for i in &self.irps {
            h = h.wrapping_mul(211).wrapping_add((i.id & 0xFFFF) as u32);
            h = h.wrapping_mul(211).wrapping_add(i.state as u32);
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
    let mut b = CancelBook::new();
    let _ = b.enlist(0x31, "r");
    let _ = b.enlist_mutex(0x51, "m");
    // 1) 完成后取消 → CX_COMPLETE_RACE。
    let _ = b.complete(0x31, "c");
    got[0] = b.cancel(0x31, "late").err() == Some(CX_COMPLETE_RACE);
    // 2) 取消后再完成 → CX_CANCEL_COMPLETE。
    let _ = b.enlist(0x32, "r2");
    let _ = b.cancel(0x32, "x");
    got[1] = b.complete(0x32, "after").err() == Some(CX_CANCEL_COMPLETE);
    // 3) 未登记 IRP 取消 → CX_NO_SUCH。
    got[2] = b.cancel(0xFEED, "g").err() == Some(CX_NO_SUCH);
    // 4) 空留痕 → CX_NO_SUCH。
    got[3] = b.cancel(0x31, "").err() == Some(CX_NO_SUCH);
    // 5) 印账后动作 → CX_FROZEN。
    b.seal();
    got[4] = b.wait_on(0x51, "post").err() == Some(CX_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12035_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12035");

    // J1 弃主语义：弃主后等待收 ABANDONED_WAIT_0。
    let j1 = {
        let mut b = CancelBook::new();
        let _ = b.enlist_mutex(1, "m");
        let _ = b.abandon(1, "a");
        b.wait_on(1, "w") == Ok(STATUS_ABANDONED_WAIT_0)
    };
    s.add("J1-弃主等待码", j1, "0x80 族码");

    // J2 未弃等待收 WAIT_0。
    let j2 = {
        let mut b = CancelBook::new();
        let _ = b.enlist_mutex(2, "m");
        b.wait_on(2, "w") == Ok(STATUS_WAIT_0)
    };
    s.add("J2-正常等待码", j2, "WAIT_0");

    // J3 完成后取消拒。
    let j3 = {
        let mut b = CancelBook::new();
        let _ = b.enlist(3, "r");
        let _ = b.complete(3, "c");
        b.cancel(3, "x").err() == Some(CX_COMPLETE_RACE)
    };
    s.add("J3-完成竞态拒", j3, "取消竞态守卫");

    // J4 取消后再完成拒。
    let j4 = {
        let mut b = CancelBook::new();
        let _ = b.enlist(4, "r");
        let _ = b.cancel(4, "x");
        b.complete(4, "c").err() == Some(CX_CANCEL_COMPLETE)
    };
    s.add("J4-取消后完成拒", j4, "终态冻结");

    // J5 取消码钉值：取消账记 0xC0000120。
    let j5 = {
        let mut b = CancelBook::new();
        let _ = b.enlist(5, "r");
        let _ = b.cancel(5, "x");
        b.ledger[0].code == STATUS_CANCELLED
    };
    s.add("J5-取消码钉值", j5, "0xC0000120");

    // J6 幽灵 IRP 拒。
    let j6 = {
        let mut b = CancelBook::new();
        b.cancel(9, "g").err() == Some(CX_NO_SUCH)
    };
    s.add("J6-幽灵拒", j6, "未登记必拒");

    // J7 零号登记拒。
    let j7 = {
        let mut b = CancelBook::new();
        b.enlist(0, "z") == Err(CX_NO_SUCH)
    };
    s.add("J7-零号拒", j7, "零 id 永不登记");

    // J8 双面账对平：面 1/2 清点合计==账数。
    let j8 = {
        let mut b = CancelBook::new();
        let _ = b.enlist(6, "r");
        let _ = b.enlist_mutex(7, "m");
        let _ = b.cancel(6, "x");
        let _ = b.abandon(7, "a");
        b.count_face(1) + b.count_face(2) == b.ledger.len()
            && b.count_face(1) == 1 && b.count_face(2) == 1
    };
    s.add("J8-双面对平", j8, "各一面");

    // J9 账目自洽。
    let j9 = {
        let mut b = CancelBook::new();
        let _ = b.enlist(8, "r");
        let _ = b.cancel(8, "x");
        b.ledger_ok()
    };
    s.add("J9-账目自洽", j9, "逐笔过核");

    // J10 印账冻结。
    let j10 = {
        let mut b = CancelBook::new();
        let _ = b.enlist(9, "r");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.cancel(9, "post").err() == Some(CX_FROZEN)
    };
    s.add("J10-印账冻结", j10, "印后动作必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：弃主场景。
    let mut b = CancelBook::new();
    let _ = b.enlist_mutex(0x71, "sum-m");
    let _ = b.abandon(0x71, "sum-a");
    let w = b.wait_on(0x71, "sum-w");
    s.add("样例弃主码", w == Ok(STATUS_ABANDONED_WAIT_0), "等待者见 0x80");
    s.add("样例账面合法", b.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12035_cancel_flow() {
        let mut b = CancelBook::new();
        assert!(b.enlist(1, "r").is_ok());
        assert!(b.cancel(1, "x").is_ok());
        assert_eq!(b.complete(1, "c"), Err(CX_CANCEL_COMPLETE));
        assert!(b.enlist(2, "r2").is_ok());
        assert!(b.complete(2, "c2").is_ok());
        assert_eq!(b.cancel(2, "x2"), Err(CX_COMPLETE_RACE));
    }

    #[test]
    fn unx12035_checks_green() {
        let s = run_unx12035_checks();
        assert!(s.all_passed(), "UNX-F12035 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12035_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
