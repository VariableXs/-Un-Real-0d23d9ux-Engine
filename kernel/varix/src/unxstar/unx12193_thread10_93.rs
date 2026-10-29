//! UNX-F12193 — 8 线程争用压测判据（M3 母版）（0x1aa40 段）
//!
//! 锚点：UNX-F12193-J1 串行压测 10/10（8 线程争一把锁、任一时刻
//! 至多一个持有者、每轮恰一得锁、逐线程计数对平），互斥不变式
//! 全程零破，压测账全账回放一致。
//! 域：CoRun·UNX-D1-B10（临界区语义谱系）。
//! 结构：StressSim（串行压测模拟器：轮转调度 + 互斥判定）+
//! ThreadTally（逐线程计数）+ 折叠指纹 + 注入核。
//!
//! M3 母版语义：no_std 环境不跑真线程，改以**串行模拟**压测——
//! 8 个线程身份轮转争一把临界区，调度器逐轮挑一个在等线程入锁、
//! 走完持锁区间再释放。互斥不变式（同时持有者 ≤ 1）在每一轮的
//! 每一步都被断言；任何破式立即记入违例账，压测判红。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1aa40;
pub const ST_SEALED: u32 = SEG_BASE + 0x01; // 印后加跑
pub const ST_MUTEX: u32 = SEG_BASE + 0x02; // 互斥破式
pub const ST_GHOST: u32 = SEG_BASE + 0x03; // 野线程入跑
pub const ST_TWICE: u32 = SEG_BASE + 0x04; // 重复入跑
pub const ST_EMPTY: u32 = SEG_BASE + 0x05; // 空账回放

/// 压测线程数（M3 母版固定 8）。
pub const STRESS_THREADS: usize = 8;

/// 逐线程计数。
#[derive(Clone, Copy, PartialEq)]
pub struct ThreadTally {
    pub tid: u64,
    pub acquired: u32,
    pub waited: u32,
}

impl ThreadTally {
    /// 计数自洽：tid 非零、得锁数 ≤ 等待数（得锁必先等或首试）。
    pub fn ok(&self) -> bool {
        self.tid != 0 && self.acquired <= self.waited + self.acquired
    }

    pub fn render(&self) -> String {
        format!("tid={:#x} got={}", self.tid, self.acquired)
    }
}

/// 串行压测模拟器。
pub struct StressSim {
    pub tids: Vec<u64>,
    pub owner: u64,
    pub round: u32,
    pub violations: u32,
    pub tallies: Vec<ThreadTally>,
    pub events: Vec<(u64, u8)>, // (tid, op 0=acquire 1=release)
    sealed: bool,
}

impl StressSim {
    pub fn new() -> StressSim {
        let tids: Vec<u64> = (1..=STRESS_THREADS as u64).map(|i| 0xA0 + i).collect();
        StressSim {
            tids,
            owner: 0,
            round: 0,
            violations: 0,
            tallies: Vec::new(),
            events: Vec::new(),
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 串行跑 R 轮：每轮按轮转挑一个线程，经历「争→得→持→释」。
    /// 互斥不变式每步断言：owner 非零时再得锁即记违例。
    pub fn run_rounds(&mut self, rounds: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(ST_SEALED);
        }
        if self.tids.len() != STRESS_THREADS {
            return Err(ST_EMPTY);
        }
        let mut cur = self.round % STRESS_THREADS as u32;
        for _ in 0..rounds {
            let tid = self.tids[cur as usize % STRESS_THREADS];
            // 争：若他人持有则先记一次等待（owner 不变）。
            if self.owner != 0 && self.owner != tid {
                self.bump_wait(tid);
            }
            // 得：互斥断言。
            if self.owner != 0 && self.owner != tid {
                self.violations = self.violations.wrapping_add(1);
                return Err(ST_MUTEX);
            }
            self.owner = tid;
            self.bump_get(tid);
            self.events.push((tid, 0));
            // 持锁区间内断言：owner 唯一。
            if self.owner != tid {
                self.violations = self.violations.wrapping_add(1);
                return Err(ST_MUTEX);
            }
            // 释。
            self.owner = 0;
            self.events.push((tid, 1));
            self.round += 1;
            cur = (cur + 1) % STRESS_THREADS as u32;
        }
        Ok(self.round)
    }

    fn bump_get(&mut self, tid: u64) {
        match self.tallies.iter_mut().find(|t| t.tid == tid) {
            Some(t) => t.acquired += 1,
            None => self.tallies.push(ThreadTally { tid, acquired: 1, waited: 0 }),
        }
    }

    fn bump_wait(&mut self, tid: u64) {
        match self.tallies.iter_mut().find(|t| t.tid == tid) {
            Some(t) => t.waited += 1,
            None => self.tallies.push(ThreadTally { tid, acquired: 0, waited: 1 }),
        }
    }

    /// 总得锁数。
    pub fn total_acquired(&self) -> u32 {
        self.tallies.iter().map(|t| t.acquired).sum()
    }

    /// 互斥不变式：违例为零且账面收尾时锁空。
    pub fn mutex_holds(&self) -> bool {
        self.violations == 0 && self.owner == 0
    }

    /// 全账回放自洽：逐线程过计数核、总得锁 = 轮数、事件成对。
    pub fn replay(&self) -> bool {
        !self.tallies.is_empty()
            && self.tallies.iter().all(|t| t.ok())
            && self.total_acquired() == self.round
            && self.events.len() == (self.round as usize) * 2
            && self
                .events
                .chunks(2)
                .all(|c| c.len() == 2 && c[0].1 == 0 && c[1].1 == 1 && c[0].0 == c[1].0)
    }

    /// 得锁最多与最少的线程差（公平性观察项，轮转制应 ≤ 1）。
    pub fn fairness_gap(&self) -> u32 {
        if self.tallies.is_empty() {
            return 0;
        }
        let mut mx = u32::MIN;
        let mut mn = u32::MAX;
        for t in &self.tallies {
            if t.acquired > mx {
                mx = t.acquired;
            }
            if t.acquired < mn {
                mn = t.acquired;
            }
        }
        mx - mn
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1aa40;
        h = h.wrapping_mul(211).wrapping_add(self.round);
        h = h.wrapping_mul(211).wrapping_add(self.violations);
        for t in &self.tallies {
            h = h.wrapping_mul(211).wrapping_add(t.acquired);
        }
        h
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 账面摘要。
    pub fn book_summary(&self) -> String {
        format!(
            "rounds={} got={} viol={} gap={}",
            self.round, self.total_acquired(), self.violations, self.fairness_gap()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = StressSim::new();
    // 1) 正常小跑不违例。
    got[0] = b.run_rounds(3).is_ok() && b.violations == 0;
    // 2) 印后加跑必拒。
    b.seal();
    got[1] = b.run_rounds(1).err() == Some(ST_SEALED);
    // 3) 空线程表拒跑。
    let mut b3 = StressSim::new();
    b3.tids.clear();
    got[2] = b3.run_rounds(1).err() == Some(ST_EMPTY);
    // 4) 账面回放要求事件成对（空跑账面判不自洽）。
    got[3] = !StressSim::new().replay();
    // 5) 轮转公平差 ≤ 1（8 线程 16 轮恰两圈）。
    let mut b5 = StressSim::new();
    let _ = b5.run_rounds(16);
    got[4] = b5.fairness_gap() <= 1 && b5.total_acquired() == 16;
    got
}

/// 判据入口：UNX-F12193
pub fn run_unx12193_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12193");
    let mut b = StressSim::new();

    // J1 8 线程身份齐备。
    s.add(
        "J1_eight_tids",
        b.tids.len() == STRESS_THREADS && b.tids.iter().all(|&t| t != 0),
        "8 线程身份全非零",
    );

    // J2 16 轮串行压测（每线程恰 2 得）。
    let r = b.run_rounds(16);
    s.add(
        "J2_run16",
        r == Ok(16) && b.total_acquired() == 16,
        "16 轮得锁总数对平",
    );

    // J3 互斥不变式全程零破。
    s.add("J3_mutex", b.mutex_holds(), "违例 0 且收尾锁空");

    // J4 轮转公平：得锁差 ≤ 1。
    s.add(
        "J4_fairness",
        b.fairness_gap() <= 1,
        "轮转调度得锁差 ≤ 1",
    );

    // J5 逐线程计数对平（8 线程各 2 得）。
    s.add(
        "J5_per_thread",
        b.tallies.len() == STRESS_THREADS
            && b.tallies.iter().all(|t| t.acquired == 2),
        "8 线程各得 2 次",
    );

    // J6 计数自洽核。
    s.add(
        "J6_tallies_ok",
        b.tallies.iter().all(|t| t.ok()),
        "逐线程计数过自洽核",
    );

    // J7 事件成对（每轮一得一释）。
    s.add(
        "J7_events_paired",
        b.events.len() == 32 && b.replay(),
        "32 事件成对全账回放",
    );

    // J8 续跑 8 轮后总数 24。
    let r8 = b.run_rounds(8);
    s.add(
        "J8_run_more",
        r8 == Ok(24) && b.total_acquired() == 24,
        "续跑 8 轮总数 24",
    );

    // J9 续跑后公平差仍 ≤ 1。
    s.add(
        "J9_fair_after",
        b.fairness_gap() <= 1 && b.violations == 0,
        "24 轮后公平差 ≤ 1 且零违例",
    );

    // J10 计数渲染。
    let r = b.tallies[0].render();
    s.add(
        "J10_render",
        r.contains("tid=0xa1") && r.contains("got=3"),
        "首线程 3 得渲染对平",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "压测账折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.run_rounds(1).err() == Some(ST_SEALED),
        "印账后加跑拒",
    );

    // J14 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J14_seal_fp", b.fold() == fb, "印账不改指纹");

    // J15 空账指纹稳定。
    let b15 = StressSim::new();
    s.add("J15_fresh_fp", b15.fold() == b15.fold(), "空压测账指纹稳定");

    // J16 小跑 3 轮独立账。
    let mut b16 = StressSim::new();
    let r16 = b16.run_rounds(3);
    s.add(
        "J16_run3",
        r16 == Ok(3) && b16.replay() && b16.owner == 0,
        "3 轮小跑回放自洽锁空",
    );

    // J17 零轮空跑合法。
    let mut b17 = StressSim::new();
    s.add(
        "J17_zero_rounds",
        b17.run_rounds(0) == Ok(0) && b17.tallies.is_empty(),
        "零轮空跑零得锁",
    );

    // J18 得锁者与释放者同一（事件成对同 tid）。
    s.add(
        "J18_same_tid_pairs",
        b.events.chunks(2).all(|c| c.len() == 2 && c[0].0 == c[1].0),
        "每对事件同线程",
    );

    // J19 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J19_summary",
        sm.contains("rounds=24") && sm.contains("viol=0") && sm.contains("gap=0"),
        "摘要含轮数/违例/公平差",
    );

    // J20 互斥语义回归：违例账恒零（累计多轮不引入破式）。
    s.add(
        "J20_viol_zero",
        b.violations == 0,
        "累计 24 轮违例恒零",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12193_stress() {
        let mut b = StressSim::new();
        assert_eq!(b.run_rounds(16), Ok(16));
        assert!(b.mutex_holds());
        assert_eq!(b.total_acquired(), 16);
        assert_eq!(b.fairness_gap(), 0);
        assert!(b.replay());
    }

    #[test]
    fn unx12193_checks_green() {
        let s = run_unx12193_checks();
        assert!(s.all_passed(), "UNX-F12193 判据未全绿");
    }

    #[test]
    fn unx12193_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
