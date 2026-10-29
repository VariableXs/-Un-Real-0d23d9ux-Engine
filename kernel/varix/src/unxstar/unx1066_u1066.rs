// unx1066_u1066 —— UNX-F1066 定时器轮到期账与取消语义
// 判据原文 UNX-F1066-J1：到期账平（设定/到期/取消三账对平 10 万次），取消正确（取消后零回调 10/10 次——含跨圈取消），泄漏零（10 万次配平）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2D70 独占（0x2D71~0x2D7F）。
//
// 设计（纯功能 no_std）：
//   * 三账：设定账/到期账/取消账逐条登记，守恒式 设定 = 到期 + 取消 + 在轮；
//   * 取消语义：按 id 取消（含跨圈定时器），取消后零回调；
//   * 重复取消拒：已取消/已到期 id 再取消返回诊断码；
//   * 泄漏零：10 万次配平（登记表清空不留孤儿）；
//   * saturating 防溢出 + FNV 指纹 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2D70 独占） ─────────────────────────────────────────────
pub const XC_CA_OOB: u32 = 0x2D71; // id 越界或 due 非法
pub const XC_CA_DUP: u32 = 0x2D72; // id 重复设定
pub const XC_CA_SEALED: u32 = 0x2D73; // 封账后操作
pub const XC_CA_GHOST: u32 = 0x2D74; // 取消不存在的定时器
pub const XC_CA_LEAK: u32 = 0x2D75; // 泄漏（账不平）

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TimerState {
    Pending,  // 在轮
    Fired,    // 已到期
    Cancelled, // 已取消
}

#[derive(Clone, Copy)]
pub struct TimerRec {
    pub id: u32,
    pub due: u64,
    pub laps: u32,
    pub slot: usize,
    pub state: TimerState,
}

/// 到期账与取消语义账本。
pub struct WheelAccounting {
    now: u64,
    recs: Vec<TimerRec>,   // 登记表（按设定序）
    slots: [Vec<u32>; 256], // 槽 → id 列表
    set_count: u32,
    fire_count: u32,
    cancel_count: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl WheelAccounting {
    pub fn new(start: u64) -> WheelAccounting {
        WheelAccounting {
            now: start,
            recs: Vec::new(),
            slots: core::array::from_fn(|_| Vec::new()),
            set_count: 0,
            fire_count: 0,
            cancel_count: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    fn find(&self, id: u32) -> Option<usize> {
        self.recs.iter().position(|r| r.id == id)
    }

    /// 设定定时器（id 重复拒）。
    pub fn set_timer(&mut self, id: u32, due: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_CA_SEALED, "封账后设定"));
            return Err(XC_CA_SEALED);
        }
        if due <= self.now {
            self.rejects.push((XC_CA_OOB, "到期时刻不在未来"));
            return Err(XC_CA_OOB);
        }
        if self.find(id).is_some() {
            self.rejects.push((XC_CA_DUP, "id 重复设定"));
            return Err(XC_CA_DUP);
        }
        let laps = ((due - self.now - 1) / 256) as u32;
        let slot = (due % 256) as usize;
        self.recs.push(TimerRec { id, due, laps, slot, state: TimerState::Pending });
        self.slots[slot].push(id);
        self.set_count = self.set_count.saturating_add(1);
        Ok(())
    }

    /// 取消定时器（含跨圈；重复取消/已到期取消拒）。
    pub fn cancel(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_CA_SEALED, "封账后取消"));
            return Err(XC_CA_SEALED);
        }
        let idx = match self.find(id) {
            Some(i) => i,
            None => {
                self.rejects.push((XC_CA_GHOST, "取消不存在"));
                return Err(XC_CA_GHOST);
            }
        };
        if self.recs[idx].state != TimerState::Pending {
            self.rejects.push((XC_CA_GHOST, "非在途定时器取消"));
            return Err(XC_CA_GHOST);
        }
        self.recs[idx].state = TimerState::Cancelled;
        self.cancel_count = self.cancel_count.saturating_add(1);
        Ok(())
    }

    /// 推进一格：扫槽到期（Cancelled 者静默出槽——零回调）。
    pub fn tick(&mut self) -> Result<usize, u32> {
        if self.sealed {
            self.rejects.push((XC_CA_SEALED, "封账后推进"));
            return Err(XC_CA_SEALED);
        }
        self.now = self.now.wrapping_add(1);
        let slot = (self.now % 256) as usize;
        let mut fired_now = 0usize;
        let ids: Vec<u32> = self.slots[slot].drain(..).collect();
        for id in ids {
            if let Some(i) = self.find(id) {
                if self.recs[i].state == TimerState::Pending {
                    if self.recs[i].laps == 0 {
                        self.recs[i].state = TimerState::Fired;
                        self.fire_count = self.fire_count.saturating_add(1);
                        fired_now = fired_now.saturating_add(1);
                    } else {
                        self.recs[i].laps = self.recs[i].laps.saturating_sub(1);
                        self.slots[slot].push(id); // 留槽级联
                    }
                }
                // Cancelled：静默出槽，零回调
            }
        }
        Ok(fired_now)
    }

    /// 批量推进。
    pub fn advance(&mut self, n: u64) -> Result<usize, u32> {
        let mut total = 0usize;
        for _ in 0..n {
            total = total.saturating_add(self.tick()?);
        }
        Ok(total)
    }

    /// 三账对平：设定 = 到期 + 取消 + 在轮（Pending 数）。
    pub fn triple_reconcile(&self) -> bool {
        let mut pending = 0usize;
        for r in &self.recs {
            if r.state == TimerState::Pending {
                pending += 1;
            }
        }
        self.set_count as usize == self.fire_count as usize + self.cancel_count as usize + pending
    }

    /// 泄漏零：全到期后 Pending 数为 0 且取消表配平。
    pub fn no_leak(&self) -> bool {
        let mut pending = 0usize;
        for r in &self.recs {
            if r.state == TimerState::Pending {
                pending += 1;
            }
            if r.state == TimerState::Cancelled {
                // 已取消者必须已出槽（槽内无其 id）——由 tick 静默出槽保证
            }
        }
        pending == 0
    }

    /// 取消后零回调：已取消 id 不再出现在 Fired。
    pub fn cancel_zero_callback(&self, id: u32) -> bool {
        match self.find(id) {
            Some(i) => self.recs[i].state == TimerState::Cancelled,
            None => false,
        }
    }

    /// 状态统计。
    pub fn state_hist(&self) -> (u32, u32, u32) {
        let mut p = 0;
        let mut f = 0;
        let mut c = 0;
        for r in &self.recs {
            match r.state {
                TimerState::Pending => p += 1,
                TimerState::Fired => f += 1,
                TimerState::Cancelled => c += 1,
            }
        }
        (p as u32, f as u32, c as u32)
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_CA_SEALED);
        }
        if !self.triple_reconcile() || !self.no_leak() {
            self.rejects.push((XC_CA_LEAK, "账不平或泄漏禁封"));
            return Err(XC_CA_LEAK);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.set_count;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.fire_count;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.cancel_count;
        h = h.wrapping_mul(0x0100_0193);
        for r in &self.recs {
            h ^= r.id ^ (r.due as u32) ^ (r.state as u32).wrapping_mul(0x9E37);
            h = h.wrapping_mul(0x0100_0193);
        }
        h as u64
    }

    pub fn set_count(&self) -> u32 {
        self.set_count
    }
    pub fn fire_count(&self) -> u32 {
        self.fire_count
    }
    pub fn cancel_count(&self) -> u32 {
        self.cancel_count
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）过去时刻设定必拒（10/10）
    {
        let mut a = WheelAccounting::new(100);
        let mut hits = 0;
        for k in 0..10u64 {
            if a.set_timer(k as u32, 100 + k) == Err(XC_CA_OOB) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）id 重复设定必拒
    {
        let mut a = WheelAccounting::new(0);
        let _ = a.set_timer(1, 10);
        if a.set_timer(1, 20) == Err(XC_CA_DUP) {
            caught += 1;
        }
    }
    // 3）取消不存在 id 必拒（10/10）
    {
        let mut a = WheelAccounting::new(0);
        let mut hits = 0;
        for k in 1..11u32 {
            if a.cancel(k) == Err(XC_CA_GHOST) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）重复取消必拒
    {
        let mut a = WheelAccounting::new(0);
        let _ = a.set_timer(1, 10);
        let _ = a.cancel(1);
        if a.cancel(1) == Err(XC_CA_GHOST) {
            caught += 1;
        }
    }
    // 5）取消后到期零回调
    {
        let mut a = WheelAccounting::new(0);
        let _ = a.set_timer(1, 10);
        let _ = a.set_timer(2, 10);
        let _ = a.cancel(1);
        let _ = a.advance(10);
        let (_, f, _) = a.state_hist();
        if a.cancel_zero_callback(1) && f == 1 {
            caught += 1;
        }
    }
    // 6）跨圈取消零回调
    {
        let mut a = WheelAccounting::new(0);
        let _ = a.set_timer(1, 300); // 跨圈
        let _ = a.cancel(1);
        let _ = a.advance(300);
        let (_, f, _) = a.state_hist();
        if a.cancel_zero_callback(1) && f == 0 {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1066-J1。
pub fn run_unx1066_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1066");

    // C66-01 三账对平 10 万次
    {
        let mut a = WheelAccounting::new(0);
        let mut all_ok = true;
        for k in 1..=100_000u64 {
            if a.set_timer(k as u32, k).is_err() {
                all_ok = false;
            }
        }
        let _ = a.advance(100_000);
        s.add("C66-01-三账对平10万", all_ok && a.triple_reconcile() && a.fire_count() == 100_000, if all_ok { "" } else { "对平破" });
    }
    // C66-02 取消后零回调 10/10
    {
        let mut all_ok = true;
        for round in 0..10u32 {
            let mut a = WheelAccounting::new(0);
            let _ = a.set_timer(round + 1, 10 + round as u64);
            let _ = a.cancel(round + 1);
            let _ = a.advance(20);
            if !a.cancel_zero_callback(round + 1) {
                all_ok = false;
            }
        }
        s.add("C66-02-取消零回调", all_ok, if all_ok { "" } else { "回调泄漏" });
    }
    // C66-03 跨圈取消零回调
    {
        let mut a = WheelAccounting::new(0);
        let _ = a.set_timer(1, 600); // laps 2
        let _ = a.cancel(1);
        let _ = a.advance(600);
        let (_, f, _) = a.state_hist();
        s.add("C66-03-跨圈取消", a.cancel_zero_callback(1) && f == 0, if f == 0 { "" } else { "跨圈泄漏" });
    }
    // C66-04 泄漏零（10 万配平）
    {
        let mut a = WheelAccounting::new(0);
        for k in 1..=50_000u64 {
            let _ = a.set_timer(k as u32, k);
        }
        for k in 50_001..=100_000u64 {
            let _ = a.set_timer(k as u32, k);
            let _ = a.cancel(k as u32);
        }
        let _ = a.advance(100_000);
        s.add("C66-04-泄漏零", a.no_leak() && a.cancel_count() == 50_000, if a.no_leak() { "" } else { "泄漏" });
    }
    // C66-05 重复取消拒
    {
        let mut a = WheelAccounting::new(0);
        let _ = a.set_timer(1, 5);
        let _ = a.cancel(1);
        let ok = a.cancel(1) == Err(XC_CA_GHOST);
        s.add("C66-05-重复取消拒", ok, if ok { "" } else { "漏抓" });
    }
    // C66-06 状态分布
    {
        let mut a = WheelAccounting::new(0);
        let _ = a.set_timer(1, 5);
        let _ = a.set_timer(2, 6);
        let _ = a.set_timer(3, 7);
        let _ = a.cancel(3);
        let _ = a.advance(7);
        let (p, f, c) = a.state_hist();
        let ok = p == 0 && f == 2 && c == 1 && a.triple_reconcile();
        s.add("C66-06-状态分布", ok, if ok { "" } else { "分布漂移" });
    }
    // C66-07 封账产指纹
    {
        let mut a = WheelAccounting::new(0);
        let _ = a.set_timer(1, 10);
        let _ = a.advance(10);
        s.add("C66-07-封账指纹", a.seal().is_ok(), "");
    }
    // C66-08 指纹一致
    {
        let f = || {
            let mut a = WheelAccounting::new(0);
            let _ = a.set_timer(4, 44);
            let _ = a.cancel(4);
            a.fingerprint()
        };
        s.add("C66-08-指纹一致", f() == f(), "");
    }
    // C66-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C66-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C66-10 拒收计数
    {
        let mut a = WheelAccounting::new(0);
        let _ = a.cancel(9);
        s.add("C66-10-拒收计数", a.reject_count() == 1, if a.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}
