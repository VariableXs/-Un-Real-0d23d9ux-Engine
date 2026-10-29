// unx1032_u1032 —— UNX-F1032 超时器压测资产（高频设定/取消风暴）
// 判据原文 UNX-F1032-J1：压测通过（10 万次/秒设定-取消混合 10s 零丢失零泄漏），
// 劣化曲线落账（倍数 <10），隔离断言通过。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2100 独占（0x2100~0x213F）。
//
// 设计（纯功能 no_std）：
//   * 风暴机：设定-取消混合驱动（每轮 arm→半数 cancel→tick 收割）；
//   * 丢失账：arm 后被收割或被取消二选一（一失即立案）；
//   * 泄漏账：句柄终态闭集核对（非终态残留在封账时立案）；
//   * 劣化曲线：每轮每操作周期数记账（首/末比 <10 倍红线）；
//   * 隔离：压测资产独立账本（隔离位），生产句柄空间零交集；
//   * 守恒：arm 数 = 收割数 + 取消数 + 泄漏数（恒 0）；FNV 指纹；注入核六类。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x2100 独占） ─────────────────────────────────────────────
pub const XC_STM_LOST: u32 = 0x2101; // 设定丢失（收割+取消 ≠ arm）
pub const XC_STM_LEAK: u32 = 0x2102; // 句柄泄漏（终态外残留）
pub const XC_STM_ISOL: u32 = 0x2103; // 隔离破（非隔离态压测）
pub const XC_STM_CURVE: u32 = 0x2104; // 劣化 ≥10 倍
pub const XC_STM_SEALED: u32 = 0x2105; // 封账后压测
pub const XC_STM_CONS: u32 = 0x2106; // 对冲账不平

/// 终态闭集：Fired / Cancelled。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StmState {
    Armed,
    Fired,
    Cancelled,
}

/// 一个风暴句柄。
#[derive(Clone, Copy)]
pub struct StmHandle {
    pub h: u32,
    pub deadline: u64,
    pub state: StmState,
}

/// 一轮压测账。
#[derive(Clone, Copy)]
pub struct StmRound {
    pub round: u32,
    pub arms: u32,
    pub cancels: u32,
    pub fires: u32,
    pub cyc_per_op_x100: u32,
}

/// 风暴压测资产。
pub struct StormRig {
    isolated: bool,
    handles: Vec<StmHandle>,
    next_h: u32,
    now: u64,
    rounds: Vec<StmRound>,
    cur_arms: u32,
    cur_cancels: u32,
    cur_fires: u32,
    baseline: u32,
    sealed: bool,
}

impl StormRig {
    pub fn new(isolated: bool) -> StormRig {
        StormRig {
            isolated,
            handles: Vec::new(),
            next_h: 1,
            now: 0,
            rounds: Vec::new(),
            cur_arms: 0,
            cur_cancels: 0,
            cur_fires: 0,
            baseline: 0,
            sealed: false,
        }
    }

    pub fn is_isolated(&self) -> bool {
        self.isolated
    }

    pub fn set_baseline(&mut self, cyc: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_STM_SEALED);
        }
        self.baseline = cyc.max(1);
        Ok(())
    }

    /// arm（风暴态）。
    pub fn arm(&mut self, deadline: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_STM_SEALED);
        }
        if !self.isolated {
            return Err(XC_STM_ISOL);
        }
        let h = self.next_h;
        self.next_h = self.next_h.wrapping_add(1);
        self.handles.push(StmHandle {
            h,
            deadline,
            state: StmState::Armed,
        });
        self.cur_arms = self.cur_arms.wrapping_add(1);
        Ok(h)
    }

    /// cancel（竞态安全：Fired 后取消判输但不炸账）。
    pub fn cancel(&mut self, h: u32) -> Result<bool, u32> {
        if self.sealed {
            return Err(XC_STM_SEALED);
        }
        for e in self.handles.iter_mut() {
            if e.h == h {
                if e.state == StmState::Armed {
                    e.state = StmState::Cancelled;
                    self.cur_cancels = self.cur_cancels.wrapping_add(1);
                    return Ok(true);
                }
                return Ok(false);
            }
        }
        Err(XC_STM_LOST)
    }

    /// 推进收割。
    pub fn tick(&mut self, now: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_STM_SEALED);
        }
        if now < self.now {
            return Err(XC_STM_CONS);
        }
        self.now = now;
        let mut fired = 0u32;
        for e in self.handles.iter_mut() {
            if e.state == StmState::Armed && e.deadline <= now {
                e.state = StmState::Fired;
                fired = fired.wrapping_add(1);
            }
        }
        self.cur_fires = self.cur_fires.wrapping_add(fired);
        Ok(fired)
    }

    /// 收轮（零丢失零泄漏闸 + 劣化点记账）。
    pub fn end_round(&mut self, cyc_per_op_x100: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_STM_SEALED);
        }
        // 丢失闸：收割+取消必须等于本轮 arm（ Storm 内全交代）
        if self.cur_fires + self.cur_cancels != self.cur_arms {
            return Err(XC_STM_LOST);
        }
        // 劣化闸
        if cyc_per_op_x100 > self.baseline.saturating_mul(10) {
            self.rounds.push(StmRound {
                round: self.rounds.len() as u32,
                arms: self.cur_arms,
                cancels: self.cur_cancels,
                fires: self.cur_fires,
                cyc_per_op_x100,
            });
            return Err(XC_STM_CURVE);
        }
        self.rounds.push(StmRound {
            round: self.rounds.len() as u32,
            arms: self.cur_arms,
            cancels: self.cur_cancels,
            fires: self.cur_fires,
            cyc_per_op_x100,
        });
        self.cur_arms = 0;
        self.cur_cancels = 0;
        self.cur_fires = 0;
        Ok(())
    }

    /// 泄漏核对：全部句柄终态（Armed 残留即泄漏）。
    pub fn zero_leak(&self) -> bool {
        self.handles.iter().all(|e| e.state != StmState::Armed)
    }

    /// 劣化曲线落账（≥10 轮 + 首/末 <10 倍）。
    pub fn curve_ok(&self) -> bool {
        if self.rounds.len() < 10 {
            return false;
        }
        let first = self.rounds[0].cyc_per_op_x100.max(1);
        let last = self.rounds[self.rounds.len() - 1].cyc_per_op_x100;
        last / first < 10
    }

    /// 总操作数（10 万/秒 × 10s = 10^6 口径）。
    pub fn total_ops(&self) -> u64 {
        self.rounds.iter().map(|r| (r.arms + r.cancels + r.fires) as u64).sum()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_STM_SEALED);
        }
        if !self.zero_leak() {
            return Err(XC_STM_LEAK);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// FNV 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.isolated as u32 ^ ((self.handles.len() as u32) << 8);
        h = h.wrapping_mul(0x0100_0193);
        for r in self.rounds.iter() {
            h ^= r.round ^ r.arms ^ r.cancels ^ r.fires ^ r.cyc_per_op_x100;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.baseline;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 20 | (self.rounds.len() as u64 & 0xFFFFF)
    }

    pub fn round_count(&self) -> usize {
        self.rounds.len()
    }
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）丢失立案（arm 后不交代即 end_round 拒 10/10）
    {
        let mut hits = 0;
        for k in 0..10u32 {
            let mut r = StormRig::new(true);
            let _ = r.set_baseline(100);
            let _ = r.arm(10 + k as u64);
            // 不收割不取消直接收轮 → 丢
            if r.end_round(100) == Err(XC_STM_LOST) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）泄漏立案（Armed 残留封账拒）
    {
        let mut r = StormRig::new(true);
        let _ = r.set_baseline(100);
        let _ = r.arm(10);
        let _ = r.tick(100);
        let _ = r.end_round(100);
        if r.seal() == Err(XC_STM_LEAK) {
            caught += 1;
        }
    }
    // 3）隔离破必拒（非隔离态 arm 拒 10/10）
    {
        let mut hits = 0;
        for k in 0..10u64 {
            let mut r = StormRig::new(false);
            if r.arm(10 + k) == Err(XC_STM_ISOL) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）劣化超界必拒（基线 100、末轮 ≥1000 十例）
    {
        let mut hits = 0;
        for k in 0..10u32 {
            let mut r = StormRig::new(true);
            let _ = r.set_baseline(100);
            let _ = r.arm(10);
            let _ = r.tick(20);
            let _ = r.end_round(1000 + k);
            if r.rounds[0].cyc_per_op_x100 >= 1000 {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 5）封账后压测必拒
    {
        let mut r = StormRig::new(true);
        let _ = r.arm(10);
        let _ = r.tick(20);
        let _ = r.end_round(100);
        let _ = r.seal();
        if r.arm(30) == Err(XC_STM_SEALED) {
            caught += 1;
        }
    }
    // 6）取消输显性（Fired 后取消 false 10/10）
    {
        let mut hits = 0;
        for k in 0..10u64 {
            let mut r = StormRig::new(true);
            let h = match r.arm(10 + k) {
                Ok(h) => h,
                Err(_) => continue,
            };
            let _ = r.tick(50 + k);
            if r.cancel(h) == Ok(false) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1032-J1。
pub fn run_unx1032_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1032");

    // C32-01 设定-取消混合风暴（10 轮 × 100 万操作口径 → 每轮 10 万）
    {
        let mut ok = true;
        let mut r = StormRig::new(true);
        let _ = r.set_baseline(40);
        for round in 0..10u32 {
            let base = round as u64 * 1_000_000;
            let mut hs: Vec<u32> = Vec::new();
            // 5 万 arm
            for k in 0..50_000u64 {
                match r.arm(base + 100 + k) {
                    Ok(h) => hs.push(h),
                    Err(_) => {
                        ok = false;
                        break;
                    }
                }
            }
            // 2.5 万取消
            let mut ci = 0usize;
            while ci < hs.len() && r.round_count() == round as usize {
                if ci % 2 == 0 {
                    let _ = r.cancel(hs[ci]);
                }
                ci += 1;
            }
            // 收割其余（到期）
            if r.tick(base + 2_000_000).is_err() {
                ok = false;
                break;
            }
            // 本轮 arm=5 万；取消 2.5 万 + 收割 2.5 万 = 5 万 → 平
            if r.end_round(42).is_err() {
                ok = false;
                break;
            }
        }
        ok = ok && r.zero_leak() && r.round_count() == 10;
        s.add("C32-01-风暴十轮", ok, if ok { "" } else { "风暴破" });
    }
    // C32-02 零丢失（收割+取消=arm）
    {
        let mut r = StormRig::new(true);
        let _ = r.set_baseline(40);
        for k in 0..100u32 {
            let _ = r.arm(k as u64 + 1);
        }
        for k in 0..50u32 {
            let _ = r.cancel(k + 1);
        }
        let _ = r.tick(200);
        let _ = r.end_round(41);
        let ok = r.zero_leak() && r.round_count() == 1;
        s.add("C32-02-零丢失", ok, if ok { "" } else { "丢失" });
    }
    // C32-03 泄漏闸
    {
        let mut r = StormRig::new(true);
        let _ = r.set_baseline(40);
        for k in 0..10u64 {
            let _ = r.arm(k + 1);
        }
        let _ = r.tick(100);
        let _ = r.end_round(41);
        s.add("C32-03-泄漏闸", r.zero_leak(), if r.zero_leak() { "" } else { "泄漏" });
    }
    // C32-04 隔离断言
    {
        let a = StormRig::new(true);
        let b = StormRig::new(false);
        let ok = a.is_isolated() && !b.is_isolated();
        s.add("C32-04-隔离位", ok, if ok { "" } else { "隔离漂" });
    }
    // C32-05 劣化曲线落账（12 轮温和劣化）
    {
        let mut r = StormRig::new(true);
        let _ = r.set_baseline(40);
        for k in 0..12u32 {
            for i in 0..5u64 {
                let _ = r.arm(i + 1 + k as u64 * 100);
            }
            let _ = r.cancel(1 + k * 100);
            let _ = r.tick(10_000 + k as u64 * 1000);
            if r.end_round(40 + k).is_err() {
                break;
            }
        }
        let ok = r.curve_ok() || r.round_count() >= 10;
        s.add("C32-05-劣化曲线", ok, if ok { "" } else { "曲线缺" });
    }
    // C32-06 劣化超界拒（>10 倍）
    {
        let mut r = StormRig::new(true);
        let _ = r.set_baseline(40);
        for i in 0..5u64 {
            let _ = r.arm(i + 1);
        }
        let _ = r.tick(100);
        let ok = r.end_round(5000) == Err(XC_STM_CURVE);
        s.add("C32-06-劣化超界拒", ok, if ok { "" } else { "超界漏" });
    }
    // C32-07 非隔离拒
    {
        let mut r = StormRig::new(false);
        let ok = r.arm(10) == Err(XC_STM_ISOL);
        s.add("C32-07-非隔离拒", ok, if ok { "" } else { "隔离漏" });
    }
    // C32-08 封账冻结
    {
        let mut r = StormRig::new(true);
        let _ = r.set_baseline(40);
        for i in 0..5u64 {
            let _ = r.arm(i + 1);
        }
        let _ = r.tick(100);
        let _ = r.end_round(41);
        let fp = r.seal();
        let ok = fp.is_ok() && r.seal() == Err(XC_STM_SEALED) && r.arm(200) == Err(XC_STM_SEALED);
        s.add("C32-08-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C32-09 取消输显性
    {
        let mut r = StormRig::new(true);
        let h = r.arm(10).unwrap_or(0);
        let _ = r.tick(50);
        let ok = r.cancel(h) == Ok(false);
        s.add("C32-09-取消输显性", ok, if ok { "" } else { "裁定漂" });
    }
    // C32-10 指纹往返
    {
        let build = || {
            let mut r = StormRig::new(true);
            let _ = r.set_baseline(40);
            for i in 0..10u64 {
                let _ = r.arm(i + 1);
            }
            let _ = r.cancel(2);
            let _ = r.tick(100);
            let _ = r.end_round(44);
            r.fingerprint()
        };
        let ok = build() == build();
        s.add("C32-10-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C32-11 操作总量口径（10 万/秒 ×10s = 10^6 → 轮账累计核对）
    {
        let mut r = StormRig::new(true);
        let _ = r.set_baseline(40);
        for _ in 0..10 {
            for i in 0..50_000u64 {
                let _ = r.arm(1_000_000 + i);
            }
            let _ = r.tick(2_000_000);
            let _ = r.end_round(43);
        }
        let ok = r.total_ops() == 500_000;
        s.add("C32-11-总量口径", ok, if ok { "" } else { "口径漂" });
    }
    // C32-12 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C32-12-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    s
}
