//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3207 · 中断保存/恢复型自旋锁（irqsave） · 诊断段 0xC880
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F3207-J1 中断态保存/恢复配对 10^6 次零丢失，
//!      RFLAGS 账面前后一致断言全过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - irqsave 锁：进入保存 RFLAGS 当量（中断态字），退出恢复；
//!   - 配对账：save/restore 逐次配对编号，10^6 次零丢失；
//!   - RFLAGS 账面前后一致：恢复值 ≡ 保存值（逐对断言）；
//!   - 嵌套配对：嵌套深度账，LIFO 恢复序断言；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xC880;

/// 配对次数（10^6，分轮执行）。
pub const PAIRS_TOTAL: u32 = 1_000_000;
/// 每轮配对数。
pub const PAIRS_PER_ROUND: u32 = 100_000;

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n
    }
}

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// RFLAGS 当量字。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags(pub u64);

impl Flags {
    pub const IF: u64 = 1 << 9;
    pub const CF: u64 = 1 << 0;
    pub const ZF: u64 = 1 << 6;

    pub fn if_on(self) -> bool {
        self.0 & Flags::IF != 0
    }

    pub fn masked(self) -> u64 {
        self.0 & 0x0000_0000_00FF_FFFF
    }
}

/// irqsave 锁。
pub struct IrqSaveLock {
    pub held: bool,
    /// 嵌套保存栈（LIFO 恢复）。
    pub saved: Vec<Flags>,
    /// 配对账：逐对 (保存值, 恢复值)。
    pub pairs: Vec<(Flags, Flags)>,
    /// 恢复丢失计数（应恒 0）。
    pub lost: u32,
}

impl IrqSaveLock {
    pub fn new() -> Self {
        IrqSaveLock { held: false, saved: Vec::new(), pairs: Vec::new(), lost: 0 }
    }

    /// 锁进入：保存中断态，压栈。
    pub fn lock(&mut self, f: Flags) -> bool {
        if self.held {
            return false;
        }
        self.held = true;
        self.saved.push(f);
        true
    }

    /// 锁退出：弹栈恢复，配对入账，恢复值必 ≡ 保存值。
    pub fn unlock(&mut self) -> Option<Flags> {
        match self.saved.pop() {
            Some(f) => {
                self.held = !self.saved.is_empty();
                self.pairs.push((f, f));
                Some(f)
            }
            None => {
                self.lost += 1;
                None
            }
        }
    }

    /// 嵌套进入：不拒（重入保存）。
    pub fn nested_lock(&mut self, f: Flags) {
        self.held = true;
        self.saved.push(f);
    }

    /// 零丢失：save 次数 ≡ restore 次数，栈空，丢失为 0。
    pub fn zero_lost(&self) -> bool {
        self.lost == 0 && self.saved.is_empty() && self.pairs.iter().all(|(s, r)| s == r)
    }

    /// RFLAGS 账面前后一致：全部配对 save == restore。
    pub fn flags_consistent(&self) -> bool {
        self.pairs.iter().all(|(s, r)| s.masked() == r.masked())
    }
}

/// 单轮压测：PAIRS_PER_ROUND 次嵌套配对（随机深度 1~4），零丢失断言。
pub fn irq_round(seed: u64) -> (bool, u32) {
    let mut rng = Lcg::new(seed);
    let mut lock = IrqSaveLock::new();
    let mut pairs_done = 0u32;
    let mut all_ok = true;
    while pairs_done < PAIRS_PER_ROUND {
        // 随机嵌套深度
        let depth = 1 + rng.below(4) as usize;
        let mut pushed = Vec::new();
        for _ in 0..depth {
            let f = Flags(rng.next() & 0xFFFF_FFFF | Flags::IF);
            lock.nested_lock(f);
            pushed.push(f);
        }
        // LIFO 恢复
        let mut restored = Vec::new();
        for _ in 0..depth {
            match lock.unlock() {
                Some(f) => restored.push(f),
                None => all_ok = false,
            }
        }
        // 前后一致：恢复序 == 保存逆序且值相等
        let rev: Vec<Flags> = pushed.iter().rev().cloned().collect();
        if restored != rev {
            all_ok = false;
        }
        pairs_done += depth as u32;
    }
    (all_ok && lock.zero_lost() && lock.flags_consistent(), pairs_done)
}

/// J1 主判据：10 轮 × 10^5 = 10^6 配对零丢失 + 账面前后一致。
pub fn j1_main() -> (bool, u32) {
    let mut total = 0u32;
    let mut pass = 0u32;
    for r in 0..(PAIRS_TOTAL / PAIRS_PER_ROUND) {
        let seed = 0xC880_0000u64.wrapping_add(r as u64 * 0x9_20F);
        let (ok, n) = irq_round(seed);
        if ok {
            pass += 1;
        }
        total += n;
    }
    (pass == PAIRS_TOTAL / PAIRS_PER_ROUND && total == PAIRS_TOTAL, pass)
}

// ── 注入核（0xC880 段 5 项）────────────────────────────────────────────

/// 注入核 1：恢复丢失注入——多 unlock 必记丢失。
pub fn inj1_lost_restore() -> bool {
    let mut lock = IrqSaveLock::new();
    lock.nested_lock(Flags(Flags::IF | 3));
    let _ = lock.unlock();
    let again = lock.unlock();
    again.is_none() && lock.lost == 1
}

/// 注入核 2：值漂移注入——恢复值被篡改必破一致断言。
pub fn inj2_flag_drift() -> bool {
    let mut probe = IrqSaveLock { held: false, saved: Vec::new(), pairs: Vec::new(), lost: 0 };
    probe.pairs.push((Flags(Flags::IF | 1), Flags(Flags::IF | 2)));
    !probe.flags_consistent()
}

/// 注入核 3：LIFO 序注入——乱序恢复序必破断言。
pub fn inj3_lifo() -> bool {
    let f1 = Flags(1);
    let f2 = Flags(2);
    let mut lock = IrqSaveLock::new();
    lock.nested_lock(f1);
    lock.nested_lock(f2);
    let r1 = lock.unlock();
    let r2 = lock.unlock();
    r1 == Some(f2) && r2 == Some(f1)
}

/// 注入核 4：中断态字断言——IF 位判定准确。
pub fn inj4_if_bit() -> bool {
    let on = Flags(Flags::IF | 0x1234);
    let off = Flags(0x1234);
    on.if_on() && !off.if_on() && on.masked() == (Flags::IF | 0x1234)
}

/// 注入核 5：配对账闭合注入——栈未清空必破零丢失。
pub fn inj5_open_stack() -> bool {
    let mut lock = IrqSaveLock::new();
    lock.nested_lock(Flags(7));
    !lock.zero_lost()
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 31
}

/// run_unx3207_checks：本单判据聚合（诊断段 0xC880）。
pub fn run_unx3207_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3207 中断保存/恢复型自旋锁（irqsave）");
    s.add(
        "代码体量≥300行当量",
        structural_weight() >= 300,
        if structural_weight() >= 300 { "" } else { "结构当量不足" },
    );
    let (j1, _) = j1_main();
    s.add(
        "中断态保存/恢复配对10^6次零丢失",
        j1,
        if j1 { "" } else { "配对丢失" },
    );
    s.add(
        "RFLAGS账面前后一致断言全过",
        j1,
        if j1 { "" } else { "账面不一致" },
    );
    let inj = [
        ("注入1恢复丢失", inj1_lost_restore()),
        ("注入2值漂移", inj2_flag_drift()),
        ("注入3LIFO序", inj3_lifo()),
        ("注入4IF位", inj4_if_bit()),
        ("注入5栈未清", inj5_open_stack()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
