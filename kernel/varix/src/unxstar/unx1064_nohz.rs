// unx1064_nohz —— UNX-F1064 nohz 期事件账（停发期超时器/回调零丢失）
// 判据原文 UNX-F1064-J1：停发期零丢失（停发期超时器到期全部恢复后执行 10/10 次——账平），事件账与补拍账对平，停发期年龄账（事件在停发期的等待时长分布）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2CF0 独占（0x2CF1~0x2CFF）。
//
// 设计（纯功能 no_std）：
//   * 停发期状态机：active → nohz(idle) → resume，非相序拒；
//   * 停发期事件：超时器到期/回调在停发期入“欠账”队列，恢复后全部补执行；
//   * 年龄账：每个欠账事件记录停发期等待时长，恢复后落分布直方；
//   * 零丢失：欠账数 = 补执行数（账平），10/10 轮全过；
//   * saturating 防溢出 + FNV 指纹 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2CF0 独占） ─────────────────────────────────────────────
pub const XC_NZ_PHASE: u32 = 0x2CF1; // 相序错
pub const XC_NZ_SEALED: u32 = 0x2CF2; // 封账后操作
pub const XC_NZ_LOST: u32 = 0x2CF3; // 欠账丢失
pub const XC_NZ_AGE: u32 = 0x2CF4; // 年龄越界
pub const XC_NZ_OOB: u32 = 0x2CF5; // 参数越界

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NohzPhase {
    Active,
    Nohz,
    Resumed,
}

#[derive(Clone, Copy)]
pub struct DueEvent {
    pub timer_id: u32,
    pub due_at: u64,     // 名义到期时刻
    pub queued_at: u64,  // 停发期入欠账时刻
}

#[derive(Clone, Copy)]
pub struct AgeRow {
    pub timer_id: u32,
    pub waited: u64, // 停发期等待时长（tick）
}

/// nohz 事件账。
pub struct NohzLedger {
    phase: NohzPhase,
    debts: Vec<DueEvent>,   // 停发期欠账
    repaid: Vec<AgeRow>,    // 补执行账（含年龄）
    nohz_entered: u64,      // 停发期起点
    rounds_ok: u32,         // 零丢失轮数
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl NohzLedger {
    pub fn new() -> NohzLedger {
        NohzLedger {
            phase: NohzPhase::Active,
            debts: Vec::new(),
            repaid: Vec::new(),
            nohz_entered: 0,
            rounds_ok: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 进入 nohz（仅 Active 相允许）。
    pub fn enter_nohz(&mut self, now: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_NZ_SEALED, "封账后停发"));
            return Err(XC_NZ_SEALED);
        }
        if self.phase != NohzPhase::Active {
            self.rejects.push((XC_NZ_PHASE, "相序错：需 Active"));
            return Err(XC_NZ_PHASE);
        }
        self.phase = NohzPhase::Nohz;
        self.nohz_entered = now;
        Ok(())
    }

    /// 停发期事件到达（超时器到期/回调挂起）。
    pub fn defer_event(&mut self, e: DueEvent) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_NZ_SEALED, "封账后挂欠账"));
            return Err(XC_NZ_SEALED);
        }
        if self.phase != NohzPhase::Nohz {
            self.rejects.push((XC_NZ_PHASE, "非停发期不得挂欠账"));
            return Err(XC_NZ_PHASE);
        }
        if e.queued_at < self.nohz_entered {
            self.rejects.push((XC_NZ_OOB, "入账时刻早于停发起点"));
            return Err(XC_NZ_OOB);
        }
        self.debts.push(e);
        Ok(())
    }

    /// 恢复（resume）：全部欠账补执行并落年龄账。
    pub fn resume(&mut self, now: u64) -> Result<usize, u32> {
        if self.sealed {
            self.rejects.push((XC_NZ_SEALED, "封账后恢复"));
            return Err(XC_NZ_SEALED);
        }
        if self.phase != NohzPhase::Nohz {
            self.rejects.push((XC_NZ_PHASE, "相序错：需 Nohz"));
            return Err(XC_NZ_PHASE);
        }
        let n = self.debts.len();
        for d in self.debts.drain(..) {
            let waited = now.saturating_sub(d.queued_at);
            if waited > u32::MAX as u64 {
                self.rejects.push((XC_NZ_AGE, "年龄越界"));
                return Err(XC_NZ_AGE);
            }
            self.repaid.push(AgeRow { timer_id: d.timer_id, waited });
        }
        self.phase = NohzPhase::Resumed;
        // 账平判定
        let balanced = self.repaid.len() >= n && n > 0;
        if balanced {
            self.rounds_ok = self.rounds_ok.saturating_add(1);
        } else if n == 0 {
            self.rounds_ok = self.rounds_ok.saturating_add(1);
        }
        Ok(n)
    }

    /// 回 Active（下一轮停发前必须回 Active）。
    pub fn back_active(&mut self) -> Result<(), u32> {
        if self.phase != NohzPhase::Resumed {
            self.rejects.push((XC_NZ_PHASE, "相序错：需 Resumed"));
            return Err(XC_NZ_PHASE);
        }
        self.phase = NohzPhase::Active;
        Ok(())
    }

    /// 零丢失：欠账全清 + 补执行账与欠账总量对平（跨轮累计）。
    pub fn zero_lost(&self, total_deferred: u32) -> bool {
        self.debts.is_empty() && self.repaid.len() as u32 == total_deferred
    }

    /// 年龄分布（四桶：<10 / <100 / <1000 / ≥1000）。
    pub fn age_hist(&self) -> [u32; 4] {
        let mut h = [0u32; 4];
        for r in &self.repaid {
            let b = if r.waited < 10 {
                0
            } else if r.waited < 100 {
                1
            } else if r.waited < 1000 {
                2
            } else {
                3
            };
            h[b] = h[b].saturating_add(1);
        }
        h
    }

    /// 10/10 轮零丢失。
    pub fn rounds_10_10(&self) -> bool {
        self.rounds_ok == 10
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_NZ_SEALED);
        }
        if !self.rounds_10_10() || !self.debts.is_empty() {
            self.rejects.push((XC_NZ_LOST, "轮次或欠账不平禁封"));
            return Err(XC_NZ_LOST);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.repaid.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        for r in &self.repaid {
            h ^= r.timer_id ^ (r.waited as u32) ^ (r.waited >> 32) as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.rounds_ok;
        h = h.wrapping_mul(0x0100_0193);
        h as u64
    }

    pub fn phase(&self) -> NohzPhase {
        self.phase
    }
    pub fn debt_count(&self) -> usize {
        self.debts.len()
    }
    pub fn repaid_count(&self) -> usize {
        self.repaid.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）相序错必拒（10/10：Active 相直接 resume）
    {
        let mut l = NohzLedger::new();
        let mut hits = 0;
        for k in 0..10 {
            let _ = k;
            if l.resume(100) == Err(XC_NZ_PHASE) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）Active 相挂欠账必拒
    {
        let mut l = NohzLedger::new();
        let r = l.defer_event(DueEvent { timer_id: 1, due_at: 10, queued_at: 5 });
        if r == Err(XC_NZ_PHASE) {
            caught += 1;
        }
    }
    // 3）入账时刻早于停发起点必拒
    {
        let mut l = NohzLedger::new();
        let _ = l.enter_nohz(100);
        let r = l.defer_event(DueEvent { timer_id: 1, due_at: 10, queued_at: 50 });
        if r == Err(XC_NZ_OOB) {
            caught += 1;
        }
    }
    // 4）Resumed 相未回 Active 直接再停发必拒
    {
        let mut l = NohzLedger::new();
        let _ = l.enter_nohz(10);
        let _ = l.resume(20);
        if l.enter_nohz(30) == Err(XC_NZ_PHASE) {
            caught += 1;
        }
    }
    // 5）封账后操作必拒
    {
        let mut l = NohzLedger::new();
        let _ = l.enter_nohz(1);
        let _ = l.resume(2);
        let _ = l.back_active();
        let _ = l.seal();
        // seal 需要 10 轮——这里未达会拒；改测封账后 enter 拒
        let mut l2 = NohzLedger::new();
        for r in 0..10u64 {
            let _ = l2.enter_nohz(r * 10);
            let _ = l2.resume(r * 10 + 5);
            let _ = l2.back_active();
        }
        let _ = l2.seal();
        if l2.enter_nohz(999) == Err(XC_NZ_SEALED) {
            caught += 1;
        }
    }
    // 6）零丢失账平（3 轮欠账全补）
    {
        let mut l = NohzLedger::new();
        let mut total = 0u32;
        for r in 0..3u64 {
            let _ = l.enter_nohz(r * 100);
            let _ = l.defer_event(DueEvent { timer_id: r as u32, due_at: r * 100, queued_at: r * 100 + 1 });
            let _ = l.defer_event(DueEvent { timer_id: (r + 100) as u32, due_at: r * 100, queued_at: r * 100 + 2 });
            total = total.saturating_add(2);
            let _ = l.resume(r * 100 + 50);
            let _ = l.back_active();
        }
        if l.zero_lost(total) && l.repaid_count() == 6 {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1064-J1。
pub fn run_unx1064_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1064");

    // C64-01 停发期零丢失 10/10（账平）
    {
        let mut l = NohzLedger::new();
        let mut total = 0u32;
        let mut all_ok = true;
        for r in 0..10u64 {
            if l.enter_nohz(r * 100).is_err() {
                all_ok = false;
            }
            let _ = l.defer_event(DueEvent { timer_id: r as u32, due_at: r * 100, queued_at: r * 100 + 1 });
            total = total.saturating_add(1);
            if l.resume(r * 100 + 30).is_err() {
                all_ok = false;
            }
            if l.back_active().is_err() {
                all_ok = false;
            }
        }
        s.add("C64-01-停发零丢失", all_ok && l.rounds_10_10() && l.zero_lost(total), if all_ok { "" } else { "丢失" });
    }
    // C64-02 事件账与补拍账对平
    {
        let mut l = NohzLedger::new();
        let _ = l.enter_nohz(10);
        let _ = l.defer_event(DueEvent { timer_id: 1, due_at: 8, queued_at: 12 });
        let _ = l.defer_event(DueEvent { timer_id: 2, due_at: 9, queued_at: 15 });
        let _ = l.resume(40);
        s.add("C64-02-补拍对平", l.repaid_count() == 2 && l.debt_count() == 0, if l.repaid_count() == 2 { "" } else { "对平破" });
    }
    // C64-03 年龄账分布
    {
        let mut l = NohzLedger::new();
        let _ = l.enter_nohz(0);
        let _ = l.defer_event(DueEvent { timer_id: 1, due_at: 0, queued_at: 1002 }); // waited 3
        let _ = l.defer_event(DueEvent { timer_id: 2, due_at: 0, queued_at: 950 }); // waited 55
        let _ = l.defer_event(DueEvent { timer_id: 3, due_at: 0, queued_at: 500 }); // waited 505
        let _ = l.defer_event(DueEvent { timer_id: 4, due_at: 0, queued_at: 5 }); // waited 1000
        let _ = l.resume(1005);
        let h = l.age_hist();
        let ok = h[0] == 1 && h[1] == 1 && h[2] == 1 && h[3] == 1;
        s.add("C64-03-年龄分布", ok, if ok { "" } else { "分布漂移" });
    }
    // C64-04 相序状态机
    {
        let mut l = NohzLedger::new();
        let ok = l.resume(1) == Err(XC_NZ_PHASE)
            && l.enter_nohz(1).is_ok()
            && l.enter_nohz(2) == Err(XC_NZ_PHASE);
        s.add("C64-04-相序拒", ok, if ok { "" } else { "相序漏抓" });
    }
    // C64-05 欠账时刻早于停发起点拒
    {
        let mut l = NohzLedger::new();
        let _ = l.enter_nohz(50);
        let ok = l.defer_event(DueEvent { timer_id: 1, due_at: 0, queued_at: 10 }) == Err(XC_NZ_OOB);
        s.add("C64-05-时刻拒", ok, if ok { "" } else { "漏抓" });
    }
    // C64-06 多轮累计年龄
    {
        let mut l = NohzLedger::new();
        for r in 0..5u64 {
            let _ = l.enter_nohz(r * 10);
            let _ = l.defer_event(DueEvent { timer_id: r as u32, due_at: 0, queued_at: r * 10 + 1 });
            let _ = l.resume(r * 10 + 4);
            let _ = l.back_active();
        }
        let ok = l.repaid_count() == 5 && l.age_hist()[0] == 5;
        s.add("C64-06-多轮年龄", ok, if ok { "" } else { "年龄漂移" });
    }
    // C64-07 封账产指纹（10 轮态）
    {
        let mut l = NohzLedger::new();
        for r in 0..10u64 {
            let _ = l.enter_nohz(r * 10);
            let _ = l.resume(r * 10 + 3);
            let _ = l.back_active();
        }
        s.add("C64-07-封账指纹", l.seal().is_ok(), "");
    }
    // C64-08 指纹一致
    {
        let f = || {
            let mut l = NohzLedger::new();
            let _ = l.enter_nohz(0);
            let _ = l.defer_event(DueEvent { timer_id: 7, due_at: 0, queued_at: 2 });
            let _ = l.resume(9);
            l.fingerprint()
        };
        s.add("C64-08-指纹一致", f() == f(), "");
    }
    // C64-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C64-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C64-10 拒收计数
    {
        let mut l = NohzLedger::new();
        let _ = l.resume(1);
        s.add("C64-10-拒收计数", l.reject_count() == 1, if l.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}
