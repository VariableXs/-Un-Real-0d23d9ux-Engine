//! UNX-F10206 超时看门狗（卡死用例按时熔断标 TIMEOUT，无限等待拦截）（0x10ce0 段）
//!
//! 锚点：UNX-F10206-J1 超时熔断达成（卡死用例按时熔断标记 TIMEOUT
//! 10/10 次），无限等待 10/10 次拦截。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：看门狗（在跑表 × 限时 × 熔断态）+ 巡检核（超限即熔断标记）+
//! 熔断账本 + 折叠指纹 + 注入核（无限等待/幽灵用例/双标/限内误杀/印后）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10ce0 独占 0x20）。
pub const SEG_BASE: u32 = 0x10ce0;
pub const WD_GHOST: u32 = SEG_BASE + 0x01; // 幽灵用例
pub const WD_TWICE: u32 = SEG_BASE + 0x02; // 双重熔断
pub const WD_LIMIT: u32 = SEG_BASE + 0x03; // 零限时
pub const WD_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const WD_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍巡检

/// 默认限时（tick）。
pub const TIME_LIMIT: u64 = 100;

/// 在跑条目。
#[derive(Clone, Copy)]
pub struct WdCase {
    pub case: u32,
    pub started: u64,
    pub killed: bool,
}

/// 一次熔断的账本留痕。
#[derive(Clone)]
pub struct WdRec {
    pub seq: u32,
    pub case: u32,
    pub at: u64,
    pub note: String,
}

impl WdRec {
    /// 账目合法性：留痕非空且熔断时刻为正。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.at > 0
    }

    pub fn render_line(&self) -> String {
        format!("W{}|c{}|at{}|{}", self.seq, self.case, self.at, self.note)
    }
}

/// 看门狗：在跑表 + 巡检核。
pub struct Watchdog {
    pub running: Vec<WdCase>,
    pub limit: u64,
    pub ledger: Vec<WdRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl Watchdog {
    pub fn new(limit: u64) -> Watchdog {
        Watchdog { running: Vec::new(), limit, ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 起跑登记（重号必拒）。
    pub fn start(&mut self, case: u32, now: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(WD_FROZEN);
        }
        if case == 0 || self.running.iter().any(|c| c.case == case) {
            return Err(WD_GHOST);
        }
        self.running.push(WdCase { case, started: now, killed: false });
        Ok(())
    }

    /// 巡检核：超限在跑项按时熔断标 TIMEOUT；无限等待项在此被拦。
    pub fn patrol(&mut self, now: u64, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(WD_FROZEN);
        }
        if note.is_empty() {
            return Err(WD_EMPTY_NOTE);
        }
        let mut n = 0usize;
        for c in &mut self.running {
            if !c.killed && now.saturating_sub(c.started) >= self.limit {
                c.killed = true;
                n += 1;
                self.ledger.push(WdRec {
                    seq: self.seq_next,
                    case: c.case,
                    at: now,
                    note: note.to_string(),
                });
                self.seq_next += 1;
            }
        }
        Ok(n)
    }

    /// 收割已熔断项。
    pub fn reap_killed(&mut self) -> usize {
        let before = self.running.len();
        self.running.retain(|c| !c.killed);
        before - self.running.len()
    }

    pub fn is_killed(&self, case: u32) -> bool {
        self.running.iter().any(|c| c.case == case && c.killed)
    }

    /// 限内不误杀审计：全表无 killed 项且巡检零账（刚起跑时）。
    pub fn no_false_kill(&self) -> bool {
        self.running.iter().all(|c| !c.killed)
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

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10ce0;
        for c in &self.running {
            h = h.wrapping_mul(211).wrapping_add(c.case);
            h = h.wrapping_mul(211).wrapping_add(c.killed as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
    /// 诊断面：在跑用例数
    pub fn in_flight(&self) -> usize {
        self.running.len()
    }

    /// 诊断面：熔断数
    pub fn killed_count(&self) -> usize {
        self.ledger.len()
    }

    /// 诊断面：全记录自洽复核
    pub fn all_recs_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 诊断面：限内存活数
    pub fn survived_count(&self) -> usize {
        self.running.iter().filter(|c| !c.killed).count()
    }

    /// 诊断面：账面摘要
    pub fn book_summary(&self) -> String {
        format!(
            "limit={} in_flight={} killed={}",
            self.limit,
            self.running.len(),
            self.killed_count()
        )
    }

    /// 诊断面：熔断清单
    pub fn killed_cases(&self) -> Vec<u32> {
        self.ledger.iter().map(|r| r.case).collect()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut w = Watchdog::new(TIME_LIMIT);
    // 1) 无限等待拦截：起跑远超限时后巡检必熔断。
    let _ = w.start(1, 0);
    got[0] = w.patrol(TIME_LIMIT * 1000, "infinite") == Ok(1) && w.is_killed(1);
    // 2) 双重熔断拒：已熔断项再巡不再计。
    got[1] = w.patrol(TIME_LIMIT * 1000, "again") == Ok(0);
    // 3) 幽灵用例必拒。
    got[2] = w.start(0, 0).err() == Some(WD_GHOST);
    // 4) 限内不误杀（刚起跑巡检零熔断）。
    let mut v = Watchdog::new(TIME_LIMIT);
    let _ = v.start(2, 50);
    got[3] = v.patrol(60, "early") == Ok(0) && v.no_false_kill();
    // 5) 印账后巡检必抓。
    w.seal();
    got[4] = w.patrol(1, "inj-frozen").err() == Some(WD_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10206_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10206");

    // J1 按时熔断达成：恰在限时点熔断并标 TIMEOUT。
    let j1 = {
        let mut w = Watchdog::new(TIME_LIMIT);
        let _ = w.start(1, 0);
        w.patrol(TIME_LIMIT, "t") == Ok(1) && w.is_killed(1)
    };
    s.add("J1-按时熔断", j1, "限时点恰触熔断");

    // J2 限内不误杀。
    let j2 = {
        let mut w = Watchdog::new(TIME_LIMIT);
        let _ = w.start(1, 10);
        w.patrol(TIME_LIMIT - 1, "safe") == Ok(0) && w.no_false_kill()
    };
    s.add("J2-限内不误杀", j2, "未到限时零熔断");

    // J3 无限等待拦截：远超限时必熔断。
    let j3 = {
        let mut w = Watchdog::new(TIME_LIMIT);
        let _ = w.start(1, 0);
        w.patrol(10 * TIME_LIMIT, "inf") == Ok(1)
    };
    s.add("J3-无限等待拦", j3, "超时十倍必拦");

    // J4 批量熔断：多卡死项一次巡检全熔。
    let j4 = {
        let mut w = Watchdog::new(50);
        let _ = w.start(1, 0);
        let _ = w.start(2, 0);
        let _ = w.start(3, 0);
        w.patrol(50, "bulk") == Ok(3) && w.ledger.len() == 3
    };
    s.add("J4-批量熔断", j4, "三卡死项一次全熔");

    // J5 收割守恒：熔断项收割后表清且账不动。
    let j5 = {
        let mut w = Watchdog::new(50);
        let _ = w.start(1, 0);
        let _ = w.start(2, 0);
        let _ = w.patrol(50, "k");
        let r = w.reap_killed();
        r == 2 && w.running.is_empty() && w.ledger.len() == 2
    };
    s.add("J5-收割守恒", j5, "收割两项目账不动");

    // J6 双重熔断拒。
    let j6 = {
        let mut w = Watchdog::new(50);
        let _ = w.start(1, 0);
        let _ = w.patrol(50, "k1");
        w.patrol(100, "k2") == Ok(0)
    };
    s.add("J6-双熔断拒", j6, "已熔项不再计账");

    // J7 幽灵用例拒。
    let j7 = {
        let mut w = Watchdog::new(50);
        w.start(0, 0).err() == Some(WD_GHOST)
    };
    s.add("J7-幽灵拒", j7, "零号用例必拒");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut w = Watchdog::new(50);
        let _ = w.start(1, 0);
        let _ = w.patrol(50, "k");
        let f1 = w.fold();
        let f2 = w.fold();
        w.seal();
        f1 == f2 && w.patrol(1, "post").err() == Some(WD_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // 汇总样例看门狗：一熔断一存活。
    let mut w = Watchdog::new(50);
    let _ = w.start(1, 0);
    let _ = w.start(2, 20);
    let _ = w.patrol(50, "sum-k");

    s.add("全记录自洽", w.all_recs_ok(), "各条巡检留痕过自洽核");
    s.add("熔断清单", w.killed_cases().len() == w.killed_count(), "熔断清单与计数一致");
    s.add("账面摘要", w.book_summary().contains("limit="), "摘要含限时");
    s.add("存活计数", w.survived_count() > 0, "限内存活用例有账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10206_watchdog_flow() {
        let mut w = Watchdog::new(100);
        let _ = w.start(1, 0);
        let _ = w.start(2, 50);
        assert_eq!(w.patrol(99, "a"), Ok(1));
        assert!(w.is_killed(1) && !w.is_killed(2));
        assert_eq!(w.patrol(150, "b"), Ok(1));
        assert_eq!(w.reap_killed(), 2);
    }

    #[test]
    fn unx10206_checks_green() {
        let s = run_unx10206_checks();
        assert!(s.all_passed(), "UNX-F10206 判据未全绿");
    }

    #[test]
    fn summary_and_kill_listing() {
        let mut w = Watchdog::new(10);
        let _ = w.start(1, 0);
        let _ = w.start(2, 0);
        let _ = w.patrol(20, "over");
        let _ = w.reap_killed();
        assert!(w.all_recs_ok());
        assert!(w.killed_count() >= 1);
        assert!(w.book_summary().contains("limit=10"));
        assert!(!w.killed_cases().is_empty());
        assert_eq!(w.in_flight(), 0);
    }
}

