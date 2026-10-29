//! UNX-F12016 进程拆除次序协议：句柄关→内存拆→对象销毁（0x19420 段）
//!
//! 判据原文：进程拆除沿三相严格推进——句柄全关（HandlesClosed）→
//! 地址空间拆（MemoryTorn）→ 对象销毁（ObjectDestroyed）；相序不可跳
//! 不可回；未关全句柄不得拆内存；拆除单次性（双拆即冲突）；拆除后
//! 残留句柄为泄漏红账。
//!
//! 结构：拆除相状态机（TearPhase 闭集）+ 推进核（跳/回/漏关/双拆四码
//! 守卫）+ 相账（TdRec 逐相留痕）+ 折叠指纹 + 印账冻结 + 注入核 +
//! 判据聚合（run_unx12016_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19420 独占 0x20）。
pub const SEG_BASE: u32 = 0x19420;
pub const TD_PHASE_SKIP: u32 = SEG_BASE + 0x01; // 跳相
pub const TD_PHASE_BACK: u32 = SEG_BASE + 0x02; // 回退
pub const TD_HANDLES_OPEN: u32 = SEG_BASE + 0x03; // 未关全句柄拆内存
pub const TD_TWICE: u32 = SEG_BASE + 0x04; // 双重销毁
pub const TD_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 拆除相闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum TearPhase {
    NotStarted = 0,
    HandlesClosed = 1,
    MemoryTorn = 2,
    ObjectDestroyed = 3,
}

/// 一相推进的账目。
#[derive(Clone)]
pub struct TdRec {
    pub seq: u32,
    pub phase: TearPhase,
    pub note: String,
}

impl TdRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 拆除模型：三相机 + 句柄账。
pub struct TearDown {
    pub cur: TearPhase,
    pub open_handles: u32,
    pub ledger: Vec<TdRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl TearDown {
    pub fn new(open_handles: u32) -> TearDown {
        TearDown {
            cur: TearPhase::NotStarted,
            open_handles,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 关句柄：逐个关，关到零方可进第一相。
    pub fn close_handle(&mut self, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(TD_FROZEN);
        }
        if note.is_empty() {
            return Err(TD_PHASE_SKIP);
        }
        // 下溢先判。
        if self.open_handles == 0 {
            return Err(TD_HANDLES_OPEN);
        }
        self.open_handles -= 1;
        Ok(self.open_handles)
    }

    /// 相推进核。
    pub fn advance(&mut self, to: TearPhase, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TD_FROZEN);
        }
        if note.is_empty() {
            return Err(TD_PHASE_SKIP);
        }
        // 未关全句柄不得进第一相及以后。
        if (to as u16) >= (TearPhase::HandlesClosed as u16) && self.open_handles > 0 {
            return Err(TD_HANDLES_OPEN);
        }
        if (to as u16) > (self.cur as u16) + 1 {
            return Err(TD_PHASE_SKIP);
        }
        if (to as u16) < (self.cur as u16) {
            return Err(TD_PHASE_BACK);
        }
        // 双重销毁。
        if to == TearPhase::ObjectDestroyed && self.cur == TearPhase::ObjectDestroyed {
            return Err(TD_TWICE);
        }
        let rec = TdRec { seq: self.seq_next, phase: to, note: note.to_string() };
        if !rec.ok() {
            return Err(TD_PHASE_SKIP);
        }
        self.cur = to;
        self.seq_next += 1;
        self.ledger.push(rec);
        Ok(())
    }

    /// 拆完判定。
    pub fn torn_down(&self) -> bool {
        self.cur == TearPhase::ObjectDestroyed
    }

    /// 相数对平：账条数 == 已越相数。
    pub fn phases_tally(&self) -> bool {
        self.ledger.len() == self.cur as usize
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
        h = h.wrapping_mul(211).wrapping_add(self.cur as u32);
        h = h.wrapping_mul(211).wrapping_add(self.open_handles);
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 带句柄进第一相 → TD_HANDLES_OPEN。
    let mut a = TearDown::new(2);
    got[0] = a.advance(TearPhase::HandlesClosed, "leak").err() == Some(TD_HANDLES_OPEN);
    // 2) 跳相：关全句柄后直奔 ObjectDestroyed → TD_PHASE_SKIP。
    let mut b = TearDown::new(1);
    let _ = b.close_handle("c");
    got[1] =
        b.advance(TearPhase::ObjectDestroyed, "skip").err() == Some(TD_PHASE_SKIP);
    // 3) 回退：第一相后退回 NotStarted → TD_PHASE_BACK。
    let mut c = TearDown::new(0);
    let _ = c.advance(TearPhase::HandlesClosed, "h");
    got[2] = c.advance(TearPhase::NotStarted, "back").err() == Some(TD_PHASE_BACK);
    // 4) 双重销毁。
    let mut d = TearDown::new(0);
    let _ = d.advance(TearPhase::HandlesClosed, "h");
    let _ = d.advance(TearPhase::MemoryTorn, "m");
    let _ = d.advance(TearPhase::ObjectDestroyed, "o");
    got[3] =
        d.advance(TearPhase::ObjectDestroyed, "o2").err() == Some(TD_TWICE);
    // 5) 句柄关空后再关 → TD_HANDLES_OPEN（关闭面下溢守卫）。
    let mut e = TearDown::new(1);
    let _ = e.close_handle("c1");
    got[4] = e.close_handle("c2").err() == Some(TD_HANDLES_OPEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12016_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12016");

    // J1 正路径：三句柄全关→三相推进→拆完。
    let j1 = {
        let mut t = TearDown::new(3);
        let _ = t.close_handle("h1");
        let _ = t.close_handle("h2");
        let _ = t.close_handle("h3");
        let _ = t.advance(TearPhase::HandlesClosed, "p1");
        let _ = t.advance(TearPhase::MemoryTorn, "p2");
        let _ = t.advance(TearPhase::ObjectDestroyed, "p3");
        t.torn_down() && t.open_handles == 0 && t.phases_tally()
    };
    s.add("J1-三相正路径", j1, "关→拆→毁全通");

    // J2 带句柄进相拒。
    let j2 = {
        let mut t = TearDown::new(1);
        t.advance(TearPhase::HandlesClosed, "l").err() == Some(TD_HANDLES_OPEN)
    };
    s.add("J2-漏关拒", j2, "句柄未清不拆");

    // J3 跳相拒。
    let j3 = {
        let mut t = TearDown::new(0);
        let _ = t.advance(TearPhase::HandlesClosed, "h");
        t.advance(TearPhase::ObjectDestroyed, "s").err() == Some(TD_PHASE_SKIP)
    };
    s.add("J3-跳相拒", j3, "相序不可跨");

    // J4 回退拒。
    let j4 = {
        let mut t = TearDown::new(0);
        let _ = t.advance(TearPhase::HandlesClosed, "h");
        let _ = t.advance(TearPhase::MemoryTorn, "m");
        t.advance(TearPhase::HandlesClosed, "b").err() == Some(TD_PHASE_BACK)
    };
    s.add("J4-回退拒", j4, "相不可逆退");

    // J5 双重销毁拒。
    let j5 = {
        let mut t = TearDown::new(0);
        let _ = t.advance(TearPhase::HandlesClosed, "h");
        let _ = t.advance(TearPhase::MemoryTorn, "m");
        let _ = t.advance(TearPhase::ObjectDestroyed, "o");
        t.advance(TearPhase::ObjectDestroyed, "o2").err() == Some(TD_TWICE)
    };
    s.add("J5-双拆拒", j5, "拆除单次性");

    // J6 逐关对账：剩余计数随关递减。
    let j6 = {
        let mut t = TearDown::new(3);
        let a = t.close_handle("a") == Ok(2);
        let b = t.close_handle("b") == Ok(1);
        let c = t.close_handle("c") == Ok(0);
        a && b && c && t.open_handles == 0
    };
    s.add("J6-逐关递减", j6, "3→2→1→0 对账");

    // J7 关空后再关拒。
    let j7 = {
        let mut t = TearDown::new(1);
        let _ = t.close_handle("a");
        t.close_handle("b").err() == Some(TD_HANDLES_OPEN)
    };
    s.add("J7-关空再关拒", j7, "下溢守卫");

    // J8 未关句柄时全流程阻断：即使三相次序对也进不去。
    let j8 = {
        let mut t = TearDown::new(5);
        let _ = t.close_handle("a");
        let _ = t.close_handle("b");
        let r = t.advance(TearPhase::HandlesClosed, "x");
        r.is_err() && t.cur == TearPhase::NotStarted
    };
    s.add("J8-流程阻断", j8, "残留句柄冻结相序");

    // J9 相数对平与账目合法。
    let j9 = {
        let mut t = TearDown::new(0);
        let _ = t.advance(TearPhase::HandlesClosed, "h");
        let _ = t.advance(TearPhase::MemoryTorn, "m");
        t.phases_tally() && t.ledger_ok() && t.ledger.len() == 2
    };
    s.add("J9-相数对平", j9, "账与相同数");

    // J10 印账冻结。
    let j10 = {
        let mut t = TearDown::new(0);
        let f1 = t.fold();
        let f2 = t.fold();
        t.seal();
        f1 == f2
            && t.advance(TearPhase::HandlesClosed, "post").err() == Some(TD_FROZEN)
            && t.close_handle("post").err() == Some(TD_FROZEN)
    };
    s.add("J10-指纹封账", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：泄漏场景残留核账。
    let mut t = TearDown::new(2);
    let _ = t.close_handle("only");
    s.add("样例残留一句柄", t.open_handles == 1, "泄漏场景显性");
    s.add("样例未起步", t.cur == TearPhase::NotStarted, "相序被冻");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12016_teardown_flow() {
        let mut t = TearDown::new(1);
        assert!(t.close_handle("h").is_ok());
        assert!(t.advance(TearPhase::HandlesClosed, "1").is_ok());
        assert_eq!(t.advance(TearPhase::ObjectDestroyed, "skip"), Err(TD_PHASE_SKIP));
        assert!(t.advance(TearPhase::MemoryTorn, "2").is_ok());
        assert!(t.advance(TearPhase::ObjectDestroyed, "3").is_ok());
        assert!(t.torn_down());
        assert_eq!(t.advance(TearPhase::ObjectDestroyed, "again"), Err(TD_TWICE));
    }

    #[test]
    fn unx12016_checks_green() {
        let s = run_unx12016_checks();
        assert!(s.all_passed(), "UNX-F12016 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12016_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
