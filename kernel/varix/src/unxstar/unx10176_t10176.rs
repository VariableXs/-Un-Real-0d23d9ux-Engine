//! UNX-F10176 实时限流正反双判据（熔断达成与连红误用受控）（0x10920 段）
//!
//! 锚点：UNX-F10176-J1 熔断达成（超时熔断 K 生效拦截 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：限流闸（周期预算 K × 累计用量 × 熔断态）+ 准入核（带内放行/
//! 超限熔断/熔断锁定）+ 周期复位核 + 熔断账本 + 折叠指纹 +
//! 注入核（幽灵任务/零成本/锁定准入/伪造熔断账/印后准入）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10920 独占 0x20）。
pub const SEG_BASE: u32 = 0x10920;
pub const TH_GHOST: u32 = SEG_BASE + 0x01; // 幽灵任务
pub const TH_COST: u32 = SEG_BASE + 0x02; // 零成本切片
pub const TH_LOCKED: u32 = SEG_BASE + 0x03; // 熔断态连红准入
pub const TH_TRIPPED: u32 = SEG_BASE + 0x04; // 超限熔断
pub const TH_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍准入

/// 一次准入的账本留痕。
#[derive(Clone)]
pub struct ThrRec {
    pub seq: u32,
    pub pid: u32,
    pub cost: u32,
    /// 本切片是否触发熔断。
    pub tripped_now: bool,
    pub note: String,
}

impl ThrRec {
    /// 账目合法性：留痕非空；熔断账必须带正成本（零成本熔断=伪造）。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (!self.tripped_now || self.cost > 0)
    }

    pub fn render_line(&self) -> String {
        format!(
            "T{}|p{}|c{}|trip{}|{}",
            self.seq, self.pid, self.cost, self.tripped_now as u8, self.note
        )
    }
}

/// 限流闸：单周期预算 K，累计用量超限即熔断锁定至周期复位。
pub struct ThrottleGate {
    pub cap: u32,
    pub used: u32,
    pub tasks: Vec<u32>,
    pub tripped: bool,
    pub overruns: u32,
    pub ledger: Vec<ThrRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ThrottleGate {
    pub fn new(cap: u32) -> ThrottleGate {
        ThrottleGate {
            cap,
            used: 0,
            tasks: Vec::new(),
            tripped: false,
            overruns: 0,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 注册实时任务（重号必拒）。
    pub fn enroll(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TH_FROZEN);
        }
        if pid == 0 || self.tasks.contains(&pid) {
            return Err(TH_GHOST);
        }
        self.tasks.push(pid);
        Ok(())
    }

    /// 准入核：带内放行回余量；越限即熔断；熔断态锁定连红。
    pub fn admit(&mut self, pid: u32, cost: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(TH_FROZEN);
        }
        if note.is_empty() {
            return Err(TH_COST);
        }
        if !self.tasks.contains(&pid) {
            return Err(TH_GHOST);
        }
        if cost == 0 {
            return Err(TH_COST);
        }
        if self.tripped {
            return Err(TH_LOCKED);
        }
        let tripped_now = self.used + cost > self.cap;
        self.ledger.push(ThrRec {
            seq: self.seq_next,
            pid,
            cost,
            tripped_now,
            note: note.to_string(),
        });
        self.seq_next += 1;
        if tripped_now {
            self.tripped = true;
            self.overruns += 1;
            return Err(TH_TRIPPED);
        }
        self.used += cost;
        Ok(self.cap - self.used)
    }

    /// 周期复位：用量清零、熔断解锁。
    pub fn new_period(&mut self) {
        self.used = 0;
        self.tripped = false;
    }

    /// 诊断面：预算利用率（permille，千分位整型避浮点）。
    pub fn utilization_permille(&self) -> u32 {
        if self.cap == 0 {
            return 0;
        }
        self.used * 1000 / self.cap
    }

    /// 诊断面：余量。
    pub fn headroom(&self) -> u32 {
        self.cap - self.used.min(self.cap)
    }

    /// 诊断面：熔断史（累计熔断次数）。
    pub fn trip_history(&self) -> u32 {
        self.overruns
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

    /// 折叠指纹（59 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10920;
        for r in &self.ledger {
            h = h.wrapping_mul(59).wrapping_add(r.seq);
            h = h.wrapping_mul(59).wrapping_add(r.cost);
            h = h.wrapping_mul(59).wrapping_add(r.tripped_now as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(59).wrapping_add(by as u32);
            }
        }
        h = h.wrapping_mul(59).wrapping_add(self.used);
        h = h.wrapping_mul(59).wrapping_add(self.overruns);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut g = ThrottleGate::new(100);
    let _ = g.enroll(1);
    // 1) 幽灵任务准入必拒。
    got[0] = g.admit(99, 10, "inj-ghost").err() == Some(TH_GHOST);
    // 2) 零成本切片必拒。
    got[1] = g.admit(1, 0, "inj-zero").err() == Some(TH_COST);
    // 3) 超限熔断必拦（带内 10 先落账，10+95 > 100）。
    let _ = g.admit(1, 10, "warm");
    got[2] = g.admit(1, 95, "over").err() == Some(TH_TRIPPED) && g.tripped;
    // 4) 熔断态连红准入必锁。
    got[3] = g.admit(1, 5, "inj-locked").err() == Some(TH_LOCKED);
    // 5) 伪造熔断账（零成本熔断）必判非法。
    let forged = ThrRec {
        seq: 1,
        pid: 1,
        cost: 0,
        tripped_now: true,
        note: "inj-fake-trip".to_string(),
    };
    got[4] = !forged.ok();
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10176_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10176");

    // J1 带内放行：限额内准入全成功且余量递减。
    let j1 = {
        let mut g = ThrottleGate::new(100);
        let _ = g.enroll(1);
        let a = g.admit(1, 30, "a");
        let b = g.admit(1, 30, "b");
        a == Ok(70) && b == Ok(40) && !g.tripped
    };
    s.add("J1-带内放行", j1, "60/100 用量余量精确递减");

    // J2 熔断达成：越限切片必拦且闸进熔断态。
    let j2 = {
        let mut g = ThrottleGate::new(100);
        let _ = g.enroll(1);
        let _ = g.admit(1, 60, "a");
        let e = g.admit(1, 60, "b");
        e == Err(TH_TRIPPED) && g.tripped && g.overruns == 1
    };
    s.add("J2-熔断达成", j2, "120>100 越限即熔断");

    // J3 连红受控：熔断态再准入必锁。
    let j3 = {
        let mut g = ThrottleGate::new(100);
        let _ = g.enroll(1);
        let _ = g.admit(1, 101, "trip");
        g.admit(1, 1, "more").err() == Some(TH_LOCKED)
    };
    s.add("J3-连红锁定", j3, "熔断态连红准入必锁");

    // J4 周期复位解锁：复位后准入恢复。
    let j4 = {
        let mut g = ThrottleGate::new(100);
        let _ = g.enroll(1);
        let _ = g.admit(1, 101, "trip");
        g.new_period();
        !g.tripped && g.used == 0 && g.admit(1, 50, "new").is_ok()
    };
    s.add("J4-周期复位", j4, "新周期解锁且用量清零");

    // J5 幽灵/零成本双拒。
    let j5 = {
        let mut g = ThrottleGate::new(100);
        let _ = g.enroll(1);
        g.admit(99, 10, "g").err() == Some(TH_GHOST)
            && g.admit(1, 0, "z").err() == Some(TH_COST)
    };
    s.add("J5-幽灵零价", j5, "幽灵与零成本切片必拒");

    // J6 账本完整：正常账与熔断账混排全合法。
    let j6 = {
        let mut g = ThrottleGate::new(100);
        let _ = g.enroll(1);
        let _ = g.enroll(2);
        let _ = g.admit(1, 40, "a");
        let _ = g.admit(2, 40, "b");
        let _ = g.admit(1, 40, "c"); // 触发熔断
        g.ledger.len() == 3 && g.ledger_ok() && g.seq_unique() && g.ledger[2].tripped_now
    };
    s.add("J6-账本完整", j6, "熔断标记逐条如实落账");

    // J7 用量守恒：成功切片成本和 == used。
    let j7 = {
        let mut g = ThrottleGate::new(200);
        let _ = g.enroll(1);
        let _ = g.admit(1, 50, "a");
        let _ = g.admit(1, 70, "b");
        let manual: u32 = g.ledger.iter().filter(|r| !r.tripped_now).map(|r| r.cost).sum();
        manual == g.used && manual == 120
    };
    s.add("J7-用量守恒", j7, "账面成本和与闸上用量对平");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut g = ThrottleGate::new(100);
        let _ = g.enroll(1);
        let _ = g.admit(1, 30, "a");
        let f1 = g.fold();
        let f2 = g.fold();
        g.seal();
        f1 == f2 && g.admit(1, 10, "post").err() == Some(TH_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // J10 诊断面自洽：利用率/余量/熔断史三角互洽。
    let j10 = {
        let mut g = ThrottleGate::new(1000);
        let _ = g.enroll(1);
        let _ = g.admit(1, 250, "a");
        let _ = g.admit(1, 250, "b");
        let u = g.utilization_permille();
        g.headroom() == 500 && u == 500 && g.trip_history() == 0
    };
    s.add("J10-诊断面自洽", j10, "permille=500 与余量 500 互洽");

    // J11 多任务共享闸：双任务共用预算互不越权。
    let j11 = {
        let mut g = ThrottleGate::new(100);
        let _ = g.enroll(1);
        let _ = g.enroll(2);
        let a = g.admit(1, 60, "a");
        let b = g.admit(2, 30, "b");
        a == Ok(40) && b == Ok(10)
    };
    s.add("J11-共享闸", j11, "双任务共享预算余量共享递减");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10176_throttle_flow() {
        let mut g = ThrottleGate::new(100);
        let _ = g.enroll(1);
        assert_eq!(g.admit(1, 40, "a"), Ok(60));
        assert_eq!(g.admit(1, 70, "b"), Err(TH_TRIPPED));
        assert_eq!(g.admit(1, 1, "c"), Err(TH_LOCKED));
        g.new_period();
        assert_eq!(g.admit(1, 100, "d"), Ok(0));
        assert_eq!(g.admit(1, 1, "e"), Err(TH_TRIPPED));
    }

    #[test]
    fn unx10176_checks_green() {
        let s = run_unx10176_checks();
        assert!(s.all_passed(), "UNX-F10176 判据未全绿");
    }

    #[test]
    fn unx10176_period_cycle() {
        // 三周期循环：每周期熔断一次，熔断史累计 3。
        let mut g = ThrottleGate::new(50);
        let _ = g.enroll(1);
        let mut c = 0;
        while c < 3 {
            let _ = g.admit(1, 60, "trip");
            assert!(g.tripped);
            g.new_period();
            c += 1;
        }
        assert_eq!(g.trip_history(), 3);
        assert_eq!(g.utilization_permille(), 0);
    }
}
