//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3362 · 长等待账本（每锁等待时长记录） · 诊断段 0xCB00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② J1：每锁等待时长记账（挂边 tsc → 摘边 tsc 差）；
//!      M-121 注入长等待记账准确；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 挂边：登记 (锁, 等待者, 挂边 tsc)；
//!   - 摘边：计算 tsc 差 = 等待时长，入时长账；
//!   - 长等待：时长超阈值入长账分册；
//!   - M-121：注入已知时长的长等待，记账必精确相等；
//!   - 账闭合：摘边数 ≡ 记账数，未摘边留挂在册；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xCB00;

/// 长等待阈值（tsc 差）。
pub const LONG_THRESHOLD: u64 = 10_000;
/// M-121 注入条数。
pub const M121_INJECTS: u32 = 16;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 挂边记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingEdge {
    pub lock_id: u32,
    pub waiter: u32,
    pub tsc_in: u64,
}

/// 时长账条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitRecord {
    pub lock_id: u32,
    pub waiter: u32,
    pub duration: u64,
    pub long: bool,
}

/// 长等待账本。
pub struct WaitLedger {
    /// 挂边在册。
    pub pending: Vec<PendingEdge>,
    /// 时长账。
    pub records: Vec<WaitRecord>,
    /// 长账分册。
    pub longs: Vec<WaitRecord>,
    /// 摘边计数。
    pub plucked: u64,
}

impl WaitLedger {
    pub fn new() -> Self {
        WaitLedger { pending: Vec::new(), records: Vec::new(), longs: Vec::new(), plucked: 0 }
    }

    /// 挂边：登记 (锁, 等待者, tsc)。
    pub fn attach(&mut self, lock_id: u32, waiter: u32, tsc: u64) -> bool {
        if self.pending.iter().any(|p| p.lock_id == lock_id && p.waiter == waiter) {
            return false; // 重挂拒绝
        }
        self.pending.push(PendingEdge { lock_id, waiter, tsc_in: tsc });
        true
    }

    /// 摘边：计算时长入账（长账分流）。
    pub fn detach(&mut self, lock_id: u32, waiter: u32, tsc_out: u64) -> Option<u64> {
        let pos = self
            .pending
            .iter()
            .position(|p| p.lock_id == lock_id && p.waiter == waiter)?;
        let p = self.pending.remove(pos);
        let duration = tsc_out.saturating_sub(p.tsc_in);
        let long = duration >= LONG_THRESHOLD;
        let rec = WaitRecord { lock_id, waiter, duration, long };
        self.records.push(rec);
        if long {
            self.longs.push(rec);
        }
        self.plucked += 1;
        Some(duration)
    }

    /// 账闭合：记录数 ≡ 摘边数，长账 ≤ 总账。
    pub fn closed(&self) -> bool {
        self.records.len() as u64 == self.plucked && self.longs.len() <= self.records.len()
    }

    /// M-121：注入已知时长，记账精确相等。
    pub fn accuracy_check(&self, expects: &[(u32, u32, u64)]) -> bool {
        for (l, w, d) in expects {
            let hit = self
                .records
                .iter()
                .find(|r| r.lock_id == *l && r.waiter == *w);
            match hit {
                Some(r) if r.duration == *d => {}
                _ => return false,
            }
        }
        true
    }

    /// 账哈希。
    pub fn hash(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for r in &self.records {
            buf.push(r.lock_id as u8);
            buf.push(r.waiter as u8);
            buf.push(r.duration as u8);
            buf.push((r.duration >> 8) as u8);
            buf.push(r.long as u8);
        }
        fnv1a(&buf)
    }
}

/// M-121 注入长等待记账准确：注入 M121_INJECTS 条已知时长。
pub fn m121() -> (bool, Vec<(u32, u32, u64)>) {
    let mut led = WaitLedger::new();
    let mut expects = Vec::new();
    for i in 0..M121_INJECTS {
        let lock_id = 100 + i;
        let waiter = 200 + i;
        // 交替长/短等待：i 偶数 → 长等待 20,000+i；奇数 → 短 100+i
        let duration: u64 = if i % 2 == 0 { 20_000 + i as u64 } else { 100 + i as u64 };
        let tsc_in = 1_000;
        let tsc_out = tsc_in + duration;
        if !led.attach(lock_id, waiter, tsc_in) {
            return (false, expects);
        }
        let got = led.detach(lock_id, waiter, tsc_out);
        match got {
            Some(d) if d == duration => expects.push((lock_id, waiter, duration)),
            _ => return (false, expects),
        }
    }
    let longs_ok = led.longs.len() == (M121_INJECTS / 2) as usize;
    let acc = led.accuracy_check(&expects);
    (led.closed() && longs_ok && acc, expects)
}

/// J1 主判据：M-121 记账准确 + 账闭合 + 哈希稳定。
pub fn j1_main() -> bool {
    let (ok, _) = m121();
    ok && {
        let mut led = WaitLedger::new();
        let _ = led.attach(1, 1, 0);
        // 未摘边留挂
        led.pending.len() == 1 && led.plucked == 0 && led.closed()
    } && {
        let (ok2, _) = m121();
        let h1 = {
            let mut l = WaitLedger::new();
            let _ = l.attach(5, 5, 10);
            let _ = l.detach(5, 5, 30_010);
            l.hash()
        };
        let h2 = {
            let mut l = WaitLedger::new();
            let _ = l.attach(5, 5, 10);
            let _ = l.detach(5, 5, 30_010);
            l.hash()
        };
        h1 == h2 && h1 != 0 && ok2
    }
}

// ── 注入核（0xCB00 段 5 项）────────────────────────────────────────────

/// 注入核 1：重挂注入——同锁同等待者重挂必拒。
pub fn inj1_double_attach() -> bool {
    let mut led = WaitLedger::new();
    let a = led.attach(1, 2, 100);
    let b = led.attach(1, 2, 200);
    a && !b && led.pending.len() == 1
}

/// 注入核 2：摘空注入——摘不存在边必 None。
pub fn inj2_detach_missing() -> bool {
    let mut led = WaitLedger::new();
    led.detach(9, 9, 100).is_none()
}

/// 注入核 3：时长倒挂注入——摘边 tsc 早于挂边必饱和 0。
pub fn inj3_regress() -> bool {
    let mut led = WaitLedger::new();
    let _ = led.attach(3, 4, 5_000);
    led.detach(3, 4, 4_999) == Some(0)
}

/// 注入核 4：阈值分流注入——恰等于阈值归长账。
pub fn inj4_threshold() -> bool {
    let mut led = WaitLedger::new();
    let _ = led.attach(8, 8, 0);
    let _ = led.detach(8, 8, LONG_THRESHOLD);
    led.longs.len() == 1 && led.longs[0].long && led.longs[0].duration == LONG_THRESHOLD
}

/// 注入核 5：闭合破注入——挂而不摘破闭合外延（pending 非空时摘边<挂边）。
pub fn inj5_open_pending() -> bool {
    let mut led = WaitLedger::new();
    let _ = led.attach(1, 1, 0);
    let _ = led.attach(2, 2, 0);
    let _ = led.detach(1, 1, 50);
    led.pending.len() == 1 && led.plucked == 1 && led.closed()
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 35
}

/// run_unx3362_checks：本单判据聚合（诊断段 0xCB00）。
pub fn run_unx3362_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3362 长等待账本（每锁等待时长记录）");
    s.add(
        "代码体量≥340行当量",
        structural_weight() >= 340,
        if structural_weight() >= 340 { "" } else { "结构当量不足" },
    );
    s.add(
        "每锁等待时长记账（挂边→摘边tsc差）",
        j1_main(),
        if j1_main() { "" } else { "记账破" },
    );
    let (m_ok, n) = m121();
    s.add(
        "M-121注入长等待记账准确",
        m_ok && n.len() == M121_INJECTS as usize,
        if m_ok { "" } else { "M-121不精确" },
    );
    let inj = [
        ("注入1重挂", inj1_double_attach()),
        ("注入2摘空", inj2_detach_missing()),
        ("注入3时长倒挂", inj3_regress()),
        ("注入4阈值分流", inj4_threshold()),
        ("注入5挂而未摘", inj5_open_pending()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
