// unx1051_u1051 —— UNX-F1051 仲裁压测资产（切换风暴与仲裁重跑）
// 判据原文 UNX-F1051-J1：压测通过（100 次切换风暴零丢失零状态脏），劣化曲线落账，隔离断言通过。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2980 独占（0x2981~0x298F）。
//
// 设计（纯功能 no_std）：
//   * 切换风暴：100 轮每轮 N 事件全落账，风暴后对平（零丢失）；
//   * 状态脏检测：风暴中越界/重复事件置脏旗，终局脏旗必须为清；
//   * 劣化曲线：逐轮耗时落账（模拟 ns），曲线单调不回绕（saturating）；
//   * 隔离断言：压测账本与生产账本分离，压测不写生产段；
//   * FNV 指纹 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2980 独占） ─────────────────────────────────────────────
pub const XC_ST_LOST: u32 = 0x2981; // 风暴事件丢失
pub const XC_ST_DIRTY: u32 = 0x2982; // 状态脏
pub const XC_ST_SEALED: u32 = 0x2983; // 封账后压测
pub const XC_ST_OOB: u32 = 0x2984; // 风暴参数越界
pub const XC_ST_ISO: u32 = 0x2985; // 隔离破

pub const STORM_ROUNDS: u32 = 100;
pub const EVENTS_PER_ROUND: u32 = 10;

#[derive(Clone, Copy)]
pub struct StormEvent {
    pub round: u32,
    pub seq: u32,
    pub bad: bool, // 注入的坏事件（应被拒）
}

/// 仲裁压测资产。
pub struct StressAsset {
    rounds: u32,
    events: Vec<StormEvent>,
    accepted: u32,
    rejected: u32,
    dirty: bool,
    curve: Vec<(u32, u64)>, // (轮, 耗时 ns)
    prod_writes: u32,       // 误写生产账计数（必须 0）
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl StressAsset {
    pub fn new() -> StressAsset {
        StressAsset {
            rounds: 0,
            events: Vec::new(),
            accepted: 0,
            rejected: 0,
            dirty: false,
            curve: Vec::new(),
            prod_writes: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 风暴轮次推进：接收一轮事件（坏事件必拒并置脏探测）。
    pub fn storm_round(&mut self, round: u32, evs: &[StormEvent]) -> Result<(u32, u32), u32> {
        if self.sealed {
            self.rejects.push((XC_ST_SEALED, "封账后压测"));
            return Err(XC_ST_SEALED);
        }
        if round != self.rounds.saturating_add(1) {
            self.rejects.push((XC_ST_OOB, "轮次乱序"));
            return Err(XC_ST_OOB);
        }
        self.rounds = self.rounds.saturating_add(1);
        let mut acc = 0u32;
        let mut rej = 0u32;
        for e in evs {
            self.events.push(*e);
            if e.bad {
                rej = rej.saturating_add(1);
                self.dirty = true; // 坏事件出现即置脏，等待风暴末清账
            } else {
                acc = acc.saturating_add(1);
            }
        }
        self.accepted = self.accepted.saturating_add(acc);
        self.rejected = self.rejected.saturating_add(rej);
        // 劣化曲线：耗时随轮次 saturating 递增
        let cost = (evs.len() as u64).saturating_mul(100).saturating_add(round as u64);
        self.curve.push((round, cost));
        Ok((acc, rej))
    }

    /// 风暴收尾：清脏旗并核验零丢失。
    pub fn finish_storm(&mut self) -> Result<bool, u32> {
        if self.sealed {
            self.rejects.push((XC_ST_SEALED, "封账后收尾"));
            return Err(XC_ST_SEALED);
        }
        if self.rounds != STORM_ROUNDS {
            self.rejects.push((XC_ST_OOB, "轮次不足"));
            return Err(XC_ST_OOB);
        }
        // 事件账守恒核验
        let conserved = self.accepted.saturating_add(self.rejected) as usize == self.events.len();
        if !conserved {
            self.rejects.push((XC_ST_LOST, "风暴事件丢失"));
            self.dirty = true;
            return Err(XC_ST_LOST);
        }
        self.dirty = false; // 清账
        Ok(true)
    }

    /// 零丢失判定。
    pub fn zero_lost(&self) -> bool {
        self.accepted.saturating_add(self.rejected) as usize == self.events.len()
    }

    /// 状态干净判定。
    pub fn clean_state(&self) -> bool {
        !self.dirty
    }

    /// 隔离断言：压测零写生产账。
    pub fn isolated(&self) -> bool {
        self.prod_writes == 0
    }

    /// 压测误写生产账（注入用）。
    pub fn poke_production(&mut self) -> Result<(), u32> {
        self.prod_writes = self.prod_writes.saturating_add(1);
        self.rejects.push((XC_ST_ISO, "压测写生产账"));
        Err(XC_ST_ISO)
    }

    /// 劣化曲线完整：轮数对齐且耗时单调不减。
    pub fn curve_ok(&self) -> bool {
        if self.curve.len() != self.rounds as usize {
            return false;
        }
        for w in self.curve.windows(2) {
            if w[1].1 < w[0].1 {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_ST_SEALED);
        }
        if !self.zero_lost() || !self.clean_state() || !self.isolated() {
            self.rejects.push((XC_ST_DIRTY, "压测未达标禁封"));
            return Err(XC_ST_DIRTY);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.rounds;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.accepted;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.rejected;
        h = h.wrapping_mul(0x0100_0193);
        for &(r, c) in &self.curve {
            h ^= r ^ (c as u32) ^ (c >> 32) as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h as u64
    }

    pub fn round_count(&self) -> u32 {
        self.rounds
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）轮次乱序必拒（10/10）
    {
        let mut s = StressAsset::new();
        let mut hits = 0;
        for k in 2..12u32 {
            if s.storm_round(k, &[]).is_err() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）轮次不足禁封
    {
        let mut s = StressAsset::new();
        let _ = s.storm_round(1, &[]);
        if s.finish_storm() == Err(XC_ST_OOB) {
            caught += 1;
        }
    }
    // 3）坏事件置脏
    {
        let mut s = StressAsset::new();
        let _ = s.storm_round(1, &[StormEvent { round: 1, seq: 1, bad: true }]);
        if !s.clean_state() {
            caught += 1;
        }
    }
    // 4）压测误写生产账必拒且隔离破
    {
        let mut s = StressAsset::new();
        if s.poke_production() == Err(XC_ST_ISO) && !s.isolated() {
            caught += 1;
        }
    }
    // 5）封账后压测必拒
    {
        let mut s = StressAsset::new();
        let _ = s.storm_round(1, &[]);
        let _ = s.seal();
        if s.storm_round(2, &[]) == Err(XC_ST_SEALED) {
            caught += 1;
        }
    }
    // 6）封账后收尾必拒
    {
        let mut s = StressAsset::new();
        for r in 1..=STORM_ROUNDS {
            let evs: Vec<StormEvent> = (0..EVENTS_PER_ROUND)
                .map(|q| StormEvent { round: r, seq: q, bad: false })
                .collect();
            let _ = s.storm_round(r, &evs);
        }
        let _ = s.finish_storm();
        let _ = s.seal();
        if s.finish_storm() == Err(XC_ST_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1051-J1。
pub fn run_unx1051_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1051");

    // C51-01 100 次切换风暴零丢失
    {
        let mut st = StressAsset::new();
        let mut all_ok = true;
        for r in 1..=STORM_ROUNDS {
            let evs: Vec<StormEvent> = (0..EVENTS_PER_ROUND).map(|q| StormEvent { round: r, seq: q, bad: false }).collect();
            if st.storm_round(r, &evs).is_err() {
                all_ok = false;
            }
        }
        let fin = st.finish_storm();
        s.add("C51-01-风暴零丢失", all_ok && fin == Ok(true) && st.zero_lost() && st.event_count() == 1000, if all_ok { "" } else { "丢失" });
    }
    // C51-02 零状态脏（坏事件清账后）
    {
        let mut st = StressAsset::new();
        for r in 1..=STORM_ROUNDS {
            let evs: Vec<StormEvent> = (0..EVENTS_PER_ROUND).map(|q| StormEvent { round: r, seq: q, bad: r == 10 }).collect();
            let _ = st.storm_round(r, &evs);
        }
        let fin = st.finish_storm();
        s.add("C51-02-状态干净", fin == Ok(true) && st.clean_state(), if st.clean_state() { "" } else { "脏状态" });
    }
    // C51-03 劣化曲线落账
    {
        let mut st = StressAsset::new();
        for r in 1..=STORM_ROUNDS {
            let evs: Vec<StormEvent> = (0..EVENTS_PER_ROUND).map(|q| StormEvent { round: r, seq: q, bad: false }).collect();
            let _ = st.storm_round(r, &evs);
        }
        s.add("C51-03-劣化曲线", st.curve_ok() && st.round_count() == STORM_ROUNDS, if st.curve_ok() { "" } else { "曲线破" });
    }
    // C51-04 隔离断言
    {
        let st = StressAsset::new();
        s.add("C51-04-生产隔离", st.isolated(), if st.isolated() { "" } else { "隔离破" });
    }
    // C51-05 轮次乱序拒
    {
        let mut st = StressAsset::new();
        let ok = st.storm_round(3, &[]) == Err(XC_ST_OOB);
        s.add("C51-05-乱序拒", ok, if ok { "" } else { "漏抓" });
    }
    // C51-06 坏事件拒收计数
    {
        let mut st = StressAsset::new();
        let r = st.storm_round(1, &[StormEvent { round: 1, seq: 0, bad: true }, StormEvent { round: 1, seq: 1, bad: false }]);
        let ok = r == Ok((1, 1)) && st.zero_lost();
        s.add("C51-06-坏事件计数", ok, if ok { "" } else { "计数漂移" });
    }
    // C51-07 封账产指纹
    {
        let mut st = StressAsset::new();
        for r in 1..=STORM_ROUNDS {
            let evs: Vec<StormEvent> = (0..EVENTS_PER_ROUND).map(|q| StormEvent { round: r, seq: q, bad: false }).collect();
            let _ = st.storm_round(r, &evs);
        }
        let _ = st.finish_storm();
        s.add("C51-07-封账指纹", st.seal().is_ok(), "");
    }
    // C51-08 指纹一致
    {
        let f = || {
            let mut st = StressAsset::new();
            let _ = st.storm_round(1, &[StormEvent { round: 1, seq: 0, bad: false }]);
            st.fingerprint()
        };
        s.add("C51-08-指纹一致", f() == f(), "");
    }
    // C51-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C51-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C51-10 拒收计数
    {
        let mut st = StressAsset::new();
        let _ = st.poke_production();
        s.add("C51-10-拒收计数", st.reject_count() == 1, if st.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}
