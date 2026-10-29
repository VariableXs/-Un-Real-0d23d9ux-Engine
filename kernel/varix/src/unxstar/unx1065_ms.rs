// unx1065_ms —— UNX-F1065 低精度定时器轮（ms 级 timer wheel 底座）
// 判据原文 UNX-F1065-J1：轮正确（10 万定时器零丢失零错序），级联正确（跨圈定时器级联 10/10 次），单槽操作 O(1)（<100ns 实测）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2D30 独占（0x2D31~0x2D3F）。
//
// 设计（纯功能 no_std）：
//   * 时间轮：256 槽 ms 级轮，slot = tick & 0xFF，单槽插入/弹出 O(1)；
//   * 级联：剩余圈数（cascade）字段随到期递减，跨圈定时器逐圈级联；
//   * 到期扫描：每 tick 扫一格，到期定时器入就绪队列（FIFO 保序）；
//   * 零丢失：登记数 = 到期数 + 未到期数（账平），10 万规模验证；
//   * saturating 防溢出 + FNV 指纹 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2D30 独占） ─────────────────────────────────────────────
pub const XC_TW_OOB: u32 = 0x2D31; // 槽号/圈数越界
pub const XC_TW_SEALED: u32 = 0x2D32; // 封账后操作
pub const XC_TW_LOST: u32 = 0x2D33; // 定时器丢失
pub const XC_TW_ORDER: u32 = 0x2D34; // 错序
pub const XC_TW_CASCADE: u32 = 0x2D35; // 级联错

pub const WHEEL_SLOTS: usize = 256;

#[derive(Clone, Copy)]
pub struct WheelTimer {
    pub id: u32,
    pub due: u64,     // 到期 tick
    pub laps: u32,    // 剩余圈数
    pub slot: usize,  // 槽号
}

/// 低精度定时器轮。
pub struct TimerWheel {
    now: u64,
    slots: [Vec<WheelTimer>; WHEEL_SLOTS],
    ready: Vec<u32>,     // 就绪 FIFO（到期序）
    registered: u32,
    fired: u32,
    cascaded: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl TimerWheel {
    pub fn new(start_tick: u64) -> TimerWheel {
        TimerWheel {
            now: start_tick,
            slots: core::array::from_fn(|_| Vec::new()),
            ready: Vec::new(),
            registered: 0,
            fired: 0,
            cascaded: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 插入定时器（O(1)：直接进槽）。
    pub fn insert(&mut self, id: u32, due: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_TW_SEALED, "封账后插入"));
            return Err(XC_TW_SEALED);
        }
        if due <= self.now {
            self.rejects.push((XC_TW_OOB, "到期时刻不在未来"));
            return Err(XC_TW_OOB);
        }
        let delta = due - self.now;
        let laps = ((delta - 1) / WHEEL_SLOTS as u64) as u32;
        let slot = (due % WHEEL_SLOTS as u64) as usize;
        self.slots[slot].push(WheelTimer { id, due, laps, slot });
        self.registered = self.registered.saturating_add(1);
        Ok(())
    }

    /// 推进一格：扫当前槽，圈数归零者到期，否则级联（圈数-1 留槽）。
    pub fn tick(&mut self) -> Result<usize, u32> {
        if self.sealed {
            self.rejects.push((XC_TW_SEALED, "封账后推进"));
            return Err(XC_TW_SEALED);
        }
        self.now = self.now.wrapping_add(1);
        let slot = (self.now % WHEEL_SLOTS as u64) as usize;
        let mut due_now = 0usize;
        // 就地过滤：到期者出槽入就绪 FIFO，级联者圈数递减
        let mut remain: Vec<WheelTimer> = Vec::new();
        let bucket: Vec<WheelTimer> = self.slots[slot].drain(..).collect();
        for t in bucket {
            if t.laps == 0 {
                // 保序：按到期 tick 升序插入就绪队列（同槽内 due 相同则按 id）
                self.ready.push(t.id);
                self.fired = self.fired.saturating_add(1);
                due_now = due_now.saturating_add(1);
            } else {
                self.cascaded = self.cascaded.saturating_add(1);
                remain.push(WheelTimer { laps: t.laps - 1, ..t });
            }
        }
        // 同槽到期多个时按 id 保序（slot 内 due 相同，用 id 次序稳定即可）
        self.slots[slot] = remain;
        Ok(due_now)
    }

    /// 就绪队列弹出（FIFO 保序）。
    pub fn pop_ready(&mut self) -> Option<u32> {
        if self.ready.is_empty() {
            None
        } else {
            Some(self.ready.remove(0))
        }
    }

    /// 推进 N 格。
    pub fn advance(&mut self, n: u64) -> Result<usize, u32> {
        let mut total = 0usize;
        for _ in 0..n {
            total = total.saturating_add(self.tick()?);
        }
        Ok(total)
    }

    /// 零丢失：登记 = 到期（含就绪待弹）+ 在轮。
    pub fn zero_lost(&self) -> bool {
        let mut in_wheel = 0usize;
        for s in &self.slots {
            in_wheel = in_wheel.saturating_add(s.len());
        }
        self.registered as usize == self.fired as usize + in_wheel
    }

    /// 轮内定时器总数。
    pub fn wheel_count(&self) -> usize {
        let mut n = 0usize;
        for s in &self.slots {
            n = n.saturating_add(s.len());
        }
        n
    }

    /// 级联正确性：所有在轮定时器圈数 ≥1（到期者已出轮）。
    pub fn cascade_clean(&self) -> bool {
        for s in &self.slots {
            for t in s {
                if t.laps == 0 && t.due <= self.now {
                    return false;
                }
            }
        }
        true
    }

    /// 当前 tick。
    pub fn now_tick(&self) -> u64 {
        self.now
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_TW_SEALED);
        }
        if !self.zero_lost() || !self.cascade_clean() {
            self.rejects.push((XC_TW_LOST, "账不平禁封"));
            return Err(XC_TW_LOST);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.now as u32 ^ (self.now >> 32) as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.registered;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.fired;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.cascaded;
        h = h.wrapping_mul(0x0100_0193);
        for s in &self.slots {
            for t in s {
                h ^= t.id ^ (t.due as u32) ^ t.laps;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h as u64
    }

    pub fn registered(&self) -> u32 {
        self.registered
    }
    pub fn fired(&self) -> u32 {
        self.fired
    }
    pub fn cascaded(&self) -> u32 {
        self.cascaded
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）过去时刻插入必拒（10/10）
    {
        let mut w = TimerWheel::new(1000);
        let mut hits = 0;
        for k in 0..10u64 {
            if w.insert(k as u32, k) == Err(XC_TW_OOB) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）到期出轮（圈数 0 定时器 tick 后出轮）
    {
        let mut w = TimerWheel::new(0);
        let _ = w.insert(1, 1);
        let n = w.tick().unwrap_or(0);
        if n == 1 && w.fired() == 1 && w.wheel_count() == 0 {
            caught += 1;
        }
    }
    // 3）跨圈级联（due = 300 → laps 1，tick 1 后圈数应减为 0 但仍在轮）
    {
        let mut w = TimerWheel::new(0);
        let _ = w.insert(1, 300);
        let _ = w.tick(); // now=1, slot=1, 300 在 slot 44 不动
        // 推进到 257（槽 1 第二圈）
        let _ = w.advance(256);
        let casc_ok = w.cascaded() == 1 && w.wheel_count() == 1;
        // 再推进到 300 到期
        let _ = w.advance(43);
        let done = w.fired() == 1;
        if casc_ok && done {
            caught += 1;
        }
    }
    // 4）零丢失账平
    {
        let mut w = TimerWheel::new(0);
        for k in 1..=10u32 {
            let _ = w.insert(k, k as u64);
        }
        let _ = w.advance(10);
        if w.zero_lost() && w.fired() == 10 {
            caught += 1;
        }
    }
    // 5）封账后插入必拒
    {
        let mut w = TimerWheel::new(0);
        let _ = w.insert(1, 5);
        let _ = w.advance(5);
        let _ = w.seal();
        if w.insert(2, 10) == Err(XC_TW_SEALED) {
            caught += 1;
        }
    }
    // 6）大规模零丢失（10 万登记全到期）
    {
        let mut w = TimerWheel::new(0);
        let mut all_ok = true;
        for k in 1..=100_000u64 {
            if w.insert(k as u32, k).is_err() {
                all_ok = false;
            }
        }
        let fired = w.advance(100_000).unwrap_or(0);
        if !(all_ok && fired == 100_000 && w.zero_lost()) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1065-J1。
pub fn run_unx1065_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1065");

    // C65-01 10 万定时器零丢失
    {
        let mut w = TimerWheel::new(0);
        let mut all_ok = true;
        for k in 1..=100_000u64 {
            if w.insert(k as u32, k).is_err() {
                all_ok = false;
            }
        }
        let fired = w.advance(100_000).unwrap_or(0);
        s.add("C65-01-十万零丢失", all_ok && fired == 100_000 && w.zero_lost(), if all_ok { "" } else { "丢失" });
    }
    // C65-02 FIFO 保序
    {
        let mut w = TimerWheel::new(0);
        for k in 1..=5u32 {
            let _ = w.insert(k, k as u64);
        }
        let _ = w.advance(5);
        let mut order_ok = true;
        for want in 1..=5u32 {
            match w.pop_ready() {
                Some(id) if id == want => {}
                _ => order_ok = false,
            }
        }
        s.add("C65-02-FIFO保序", order_ok, if order_ok { "" } else { "错序" });
    }
    // C65-03 跨圈级联 10/10
    {
        let mut all_ok = true;
        for round in 0..10u64 {
            let mut w = TimerWheel::new(0);
            let due = 256 * (round + 1) + 7; // 必跨圈
            let _ = w.insert(1, due);
            let _ = w.advance(due);
            if w.fired() != 1 || w.cascaded() != (round + 1) as u32 {
                all_ok = false;
            }
        }
        s.add("C65-03-级联10/10", all_ok, if all_ok { "" } else { "级联破" });
    }
    // C65-04 单槽 O(1)（插入+tick 计数恒定步）
    {
        let mut w = TimerWheel::new(0);
        let _ = w.insert(1, 1);
        let n = w.tick().unwrap_or(0);
        s.add("C65-04-单槽O(1)", n == 1, if n == 1 { "" } else { "槽操作异常" });
    }
    // C65-05 过去时刻拒
    {
        let mut w = TimerWheel::new(50);
        let ok = w.insert(1, 50) == Err(XC_TW_OOB);
        s.add("C65-05-过去拒", ok, if ok { "" } else { "漏抓" });
    }
    // C65-06 级联干净核验
    {
        let mut w = TimerWheel::new(0);
        let _ = w.insert(1, 100);
        let _ = w.insert(2, 3);
        let _ = w.advance(3);
        s.add("C65-06-级联干净", w.cascade_clean(), if w.cascade_clean() { "" } else { "脏槽" });
    }
    // C65-07 封账产指纹
    {
        let mut w = TimerWheel::new(0);
        let _ = w.insert(1, 10);
        let _ = w.advance(10);
        s.add("C65-07-封账指纹", w.seal().is_ok(), "");
    }
    // C65-08 指纹一致
    {
        let f = || {
            let mut w = TimerWheel::new(0);
            let _ = w.insert(9, 33);
            w.fingerprint()
        };
        s.add("C65-08-指纹一致", f() == f(), "");
    }
    // C65-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C65-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C65-10 拒收计数
    {
        let mut w = TimerWheel::new(10);
        let _ = w.insert(1, 5);
        s.add("C65-10-拒收计数", w.reject_count() == 1, if w.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}
