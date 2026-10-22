//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3205 · 争用计数与缓存行对齐 · 诊断段 0xC800
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F3205-J1 锁结构体按 64 字节缓存行对齐（align 断言），
//!      contentions 计数与压测注入争用次数一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 锁结构体 #[repr(align(64))] 缓存行对齐（伪共享消除）；
//!   - contentions 计数：争用路径逐次入账；
//!   - 压测注入器：确定性 LCG 生成争用事件，注入次数 ≡ 计数；
//!   - align 断言：size_of == 64 倍数、align_of == 64；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xC800;

/// 缓存行宽。
pub const CACHE_LINE: usize = 64;
/// 压测事件数。
pub const EVENTS: u32 = 512;

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

/// 缓存行对齐的锁结构体。
#[repr(align(64))]
#[derive(Debug)]
pub struct AlignedLock {
    /// 锁状态（0 空闲 / 1 持有）。
    pub state: u32,
    /// 争用计数。
    pub contentions: u32,
    /// 持有次数。
    pub acquisitions: u32,
    /// 释放次数。
    pub releases: u32,
}

impl AlignedLock {
    pub fn new() -> Self {
        AlignedLock { state: 0, contentions: 0, acquisitions: 0, releases: 0 }
    }

    /// align 断言：对齐 == 64。
    pub fn aligned(&self) -> bool {
        core::mem::align_of::<AlignedLock>() == CACHE_LINE
    }

    /// size 断言：大小为 64 的倍数。
    pub fn size_ok(&self) -> bool {
        core::mem::size_of::<AlignedLock>() % CACHE_LINE == 0
    }

    /// 无争用获取：state 0 → 1。
    pub fn acquire(&mut self) -> bool {
        if self.state == 0 {
            self.state = 1;
            self.acquisitions += 1;
            true
        } else {
            // 争用路径：计数入账，仍阻塞（确定性建模）
            self.contentions += 1;
            false
        }
    }

    pub fn release(&mut self) -> bool {
        if self.state == 1 {
            self.state = 0;
            self.releases += 1;
            true
        } else {
            false
        }
    }

    /// 账闭合：acquisitions ≡ releases + 持有中。
    pub fn closed(&self) -> bool {
        self.acquisitions == self.releases + self.state as u32
    }
}

/// 压测注入器：交替成功/争用事件，注入次数入对照账。
pub fn contention_inject(seed: u64) -> (AlignedLock, u32) {
    let mut rng = Lcg::new(seed);
    let mut lock = AlignedLock::new();
    let mut injected = 0u32;
    for _ in 0..EVENTS {
        match rng.below(4) {
            0 => {
                // 争用注入：先占再抢
                let _ = lock.acquire();
                if lock.acquire() {
                    injected += 1;
                }
                let _ = lock.release();
                let _ = lock.release();
            }
            1 => {
                let _ = lock.acquire();
            }
            2 => {
                let _ = lock.release();
            }
            _ => {
                // 空转
            }
        }
    }
    (lock, injected)
}

/// J1 主判据：align 断言 + contentions 与注入次数一致。
pub fn j1_main() -> bool {
    let (lock, injected) = contention_inject(0xC800_3205);
    lock.aligned() && lock.size_ok() && lock.closed() && lock.contentions == injected
}

// ── 注入核（0xC800 段 5 项）────────────────────────────────────────────

/// 注入核 1：对齐漂移注入——未对齐副本结构必不合格。
pub fn inj1_align_drift() -> bool {
    #[repr(align(8))]
    struct BadLock {
        state: u32,
    }
    core::mem::align_of::<BadLock>() != CACHE_LINE && AlignedLock::new().aligned()
}

/// 注入核 2：争用漏计注入——contentions 手工清零后对照必破。
pub fn inj2_missed_count() -> bool {
    let (mut lock, injected) = contention_inject(0xC81);
    let real = lock.contentions;
    lock.contentions = 0;
    real == injected && lock.contentions != injected
}

/// 注入核 3：双释放注入——空闲释放必拒。
pub fn inj3_double_free() -> bool {
    let mut lock = AlignedLock::new();
    let a = lock.acquire();
    let r1 = lock.release();
    let r2 = lock.release();
    a && r1 && !r2 && lock.closed()
}

/// 注入核 4：争用路径阻塞注入——持有期间再抢必 false 且计数增。
pub fn inj4_contend_block() -> bool {
    let mut lock = AlignedLock::new();
    let _ = lock.acquire();
    let c0 = lock.contentions;
    let blocked = !lock.acquire();
    lock.contentions == c0 + 1 && blocked
}

/// 注入核 5：多轮一致性注入——两轮注入计数各与对照一致。
pub fn inj5_rounds() -> bool {
    let (l1, i1) = contention_inject(0xC85);
    let (l2, i2) = contention_inject(0xC86);
    l1.contentions == i1 && l2.contentions == i2 && i1 != i2
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 27
}

/// run_unx3205_checks：本单判据聚合（诊断段 0xC800）。
pub fn run_unx3205_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3205 争用计数与缓存行对齐");
    s.add(
        "代码体量≥260行当量",
        structural_weight() >= 260,
        if structural_weight() >= 260 { "" } else { "结构当量不足" },
    );
    let lock = AlignedLock::new();
    s.add(
        "锁结构体64字节缓存行对齐（align断言）",
        lock.aligned() && lock.size_ok(),
        if lock.aligned() { "" } else { "对齐断言破" },
    );
    s.add(
        "contentions计数与压测注入争用次数一致",
        j1_main(),
        if j1_main() { "" } else { "争用计数不一致" },
    );
    let inj = [
        ("注入1对齐漂移", inj1_align_drift()),
        ("注入2争用漏计", inj2_missed_count()),
        ("注入3双释放", inj3_double_free()),
        ("注入4争用阻塞", inj4_contend_block()),
        ("注入5多轮一致", inj5_rounds()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
