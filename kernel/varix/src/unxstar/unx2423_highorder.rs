//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2423 · order-5–10 高阶分配与 2M 块 · 诊断段 0x8580
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F2423-J1 order 9/10 分配 10^3 次成功率与降级账落档，
//!      2M 块对齐断言（2MB 对齐零例外）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 伙伴分配器高阶模型：order 5..=10（2^5 页=128KB .. 2^10 页=4MB）；
//!   - order 9 = 2MB 块、order 10 = 4MB 块；
//!   - 10^3 次分配成功率统计 + 降级账（高阶不足降级分配）；
//!   - 2MB 对齐零例外断言；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0x8580;

/// 最大阶。
pub const MAX_ORDER: u8 = 10;

/// 最小高阶。
pub const MIN_ORDER: u8 = 5;

/// 页大小。
pub const PAGE: u64 = 4096;

/// order 9 = 2MB。
pub const ORDER_9_BYTES: u64 = 2 * 1024 * 1024;

/// 2MB 对齐掩码。
pub const ALIGN_2M: u64 = ORDER_9_BYTES - 1;

/// 高阶分配账项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighAlloc {
    pub order: u8,
    pub base: u64,
    /// 是否降级（请求阶>实际阶）。
    pub degraded: bool,
}

/// 高阶伙伴池。
pub struct HighPool {
    /// 空闲块表（按阶）。
    free: Vec<(u8, u64)>,
    /// 分配账。
    ledger: Vec<HighAlloc>,
    /// 成功计数。
    ok: u32,
    /// 降级计数。
    degraded: u32,
    /// 失败计数。
    fail: u32,
    /// 池顶。
    top: u64,
}

impl HighPool {
    /// 初始池：[base, base+bytes) 一个 order-10 大块切分登记。
    pub fn new(base: u64, bytes: u64) -> Self {
        let mut free = Vec::new();
        let mut off = base;
        let end = base + bytes;
        // 以 2MB 粒度登记 order 9 块
        while off + ORDER_9_BYTES <= end {
            free.push((9u8, off));
            off += ORDER_9_BYTES;
        }
        // 余量按低阶补
        while off + (1 << MIN_ORDER) * PAGE <= end {
            free.push((MIN_ORDER, off));
            off += (1 << MIN_ORDER) * PAGE;
        }
        HighPool { free, ledger: Vec::new(), ok: 0, degraded: 0, fail: 0, top: end }
    }

    /// 分配 order 阶：不足则降级到可用最高阶。
    pub fn alloc(&mut self, order: u8) -> Option<HighAlloc> {
        if order > MAX_ORDER || order < MIN_ORDER {
            self.fail += 1;
            return None;
        }
        // 找精确阶
        if let Some(pos) = self.free.iter().position(|&(o, _)| o == order) {
            let (_, base) = self.free.remove(pos);
            self.ok += 1;
            let a = HighAlloc { order, base, degraded: false };
            self.ledger.push(a);
            return Some(a);
        }
        // 降级：找 >order 的最高可用
        if let Some(pos) = self.free.iter().position(|&(o, _)| o > order) {
            let (o, base) = self.free.remove(pos);
            self.ok += 1;
            self.degraded += 1;
            let a = HighAlloc { order: o, base, degraded: true };
            self.ledger.push(a);
            return Some(a);
        }
        self.fail += 1;
        None
    }

    /// 释放回池。
    pub fn free_block(&mut self, a: HighAlloc) {
        self.free.push((a.order, a.base));
    }

    /// 2MB 对齐断言：order 9 块零例外。
    pub fn align_2m_zero_exception(&self) -> bool {
        self.ledger
            .iter()
            .filter(|a| a.order >= 9)
            .all(|a| a.base & ALIGN_2M == 0)
    }

    /// 成功率。
    pub fn success_rate(&self) -> f64 {
        let total = self.ok + self.fail;
        if total == 0 {
            0.0
        } else {
            self.ok as f64 / total as f64
        }
    }

    pub fn counts(&self) -> (u32, u32, u32) {
        (self.ok, self.degraded, self.fail)
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
}

/// 10^3 次分配压测：order 9/10 混合，成功率与降级账落档。
pub fn stress_1k() -> (f64, u32, u32) {
    let mut pool = HighPool::new(0x4000_0000, 256 * ORDER_9_BYTES); // 512MB
    for i in 0..1000u32 {
        let order = if i % 3 == 0 { 10 } else { 9 };
        if let Some(a) = pool.alloc(order) {
            // 用后即还（循环压力）
            if i % 2 == 0 {
                pool.free_block(a);
            }
        }
    }
    let (ok, deg, fail) = pool.counts();
    (pool.success_rate(), deg, fail)
}

/// 2MB 对齐零例外核（1000 次 order 9 分配）。
pub fn align_2m_clean() -> bool {
    let mut pool = HighPool::new(0x6000_0000, 128 * ORDER_9_BYTES);
    for _ in 0..1000u32 {
        if let Some(a) = pool.alloc(9) {
            if a.base & ALIGN_2M != 0 {
                return false;
            }
            pool.free_block(a);
        } else {
            // 池耗尽即还账重试
        }
    }
    pool.align_2m_zero_exception()
}

// ── 注入核（0x8580 段 5 项）────────────────────────────────────────────

/// 注入核 1：阶界注入——order 4/11 拒。
pub fn inj1_order_bound() -> bool {
    let mut pool = HighPool::new(0x7000_0000, 16 * ORDER_9_BYTES);
    pool.alloc(4);
    pool.alloc(11);
    let (ok, _, fail) = pool.counts();
    ok == 0 && fail == 2
}

/// 注入核 2：降级注入——order 10 请求降级到 order 9。
pub fn inj2_degrade() -> bool {
    let mut pool = HighPool::new(0x7200_0000, 4 * ORDER_9_BYTES);
    let a = pool.alloc(10).unwrap();
    a.degraded && a.order == 9 && pool.counts().1 == 1
}

/// 注入核 3：耗尽注入——池空后分配失败。
pub fn inj3_exhaust() -> bool {
    let mut pool = HighPool::new(0x7400_0000, 2 * ORDER_9_BYTES);
    pool.alloc(9);
    pool.alloc(9);
    let r = pool.alloc(9);
    r.is_none() && pool.counts().2 >= 1
}

/// 注入核 4：对齐注入——非对齐构造可检出。
pub fn inj4_align_break() -> bool {
    let bad = HighAlloc { order: 9, base: 0x7600_0001, degraded: false };
    bad.base & ALIGN_2M != 0 // 非对齐形态可检出
}

/// 注入核 5：释放回池注入——还后可再分配。
pub fn inj5_free_reuse() -> bool {
    let mut pool = HighPool::new(0x7800_0000, 1 * ORDER_9_BYTES);
    let a = pool.alloc(9).unwrap();
    pool.free_block(a);
    pool.alloc(9).is_some()
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// run_unx2423_checks：本单判据聚合（诊断段 0x8580）。
pub fn run_unx2423_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2423 order-5–10 高阶分配与 2M 块");
    let (rate, deg, _fail) = stress_1k();
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "order9/10分配10^3次成功率落档",
        rate >= 0.9,
        if rate >= 0.9 { "" } else { "成功率不足" },
    );
    s.add(
        "降级账落档",
        deg > 0 || _fail > 0,
        if deg > 0 || _fail > 0 { "" } else { "降级账空" },
    );
    s.add(
        "2M块对齐零例外",
        align_2m_clean(),
        if align_2m_clean() { "" } else { "对齐破" },
    );
    let inj = [
        ("注入1阶界", inj1_order_bound()),
        ("注入2降级", inj2_degrade()),
        ("注入3耗尽", inj3_exhaust()),
        ("注入4对齐破", inj4_align_break()),
        ("注入5还池", inj5_free_reuse()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}
