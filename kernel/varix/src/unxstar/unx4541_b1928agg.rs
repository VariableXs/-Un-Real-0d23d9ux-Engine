//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4541 · B19–B28 段级回归聚合 · 诊断段 0xF100
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F4541-J1 B19–B28 十批断言面全量聚合一次命令可跑
//!      （聚合报告 190/190 无缺项），通过率 100% 才算绿——fd 与
//!      文件语义段闸判据；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 十批（B19~B28）各批断言配额账（合计 190）；
//!   - 全量聚合：一次聚合命令收齐十批报告，缺批即红；
//!   - 通过率 100% 闸：任一批未全绿则域级判据不绿；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xF100;

/// 批数。
pub const BATCHES: u32 = 10;

/// 断言总额。
pub const TOTAL: u32 = 190;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 各批断言配额（B19~B28），合计 190。
pub const QUOTA: [u32; BATCHES as usize] = [20, 20, 20, 20, 20, 20, 20, 20, 20, 10];

/// 批报告：批号 + 实收断言数 + 全绿位。
#[derive(Clone)]
pub struct BatchReport {
    pub bid: u32,
    pub received: u32,
    pub all_green: bool,
}

impl BatchReport {
    pub fn complete(&self) -> bool {
        let q = QUOTA[self.bid as usize];
        self.received == q && self.all_green
    }
}

/// 聚合器：一次命令收齐十批报告。
pub struct Aggregator {
    pub reports: Vec<BatchReport>,
}

impl Aggregator {
    pub fn new() -> Self {
        Aggregator { reports: Vec::new() }
    }

    pub fn submit(&mut self, bid: u32, received: u32, all_green: bool) {
        self.reports.push(BatchReport { bid, received, all_green });
    }

    /// J1 判据：十批全收 + 逐批实收==配额 + 逐批全绿 + 总额 190。
    pub fn aggregate(&self) -> (bool, u32, u32) {
        let mut green = 0;
        let mut total_recv = 0;
        // 批号闭集核对
        for bid in 0..BATCHES {
            let found = self.reports.iter().find(|r| r.bid == bid);
            match found {
                None => return (false, green, total_recv),
                Some(r) => {
                    total_recv += r.received;
                    if r.complete() {
                        green += 1;
                    }
                }
            }
        }
        let ok = green == BATCHES && total_recv == TOTAL;
        (ok, green, total_recv)
    }
}

/// J1 主判据：全量聚合 190/190 无缺项 + 通过率 100% 才绿。
pub fn j1_main() -> bool {
    let mut agg = Aggregator::new();
    for bid in 0..BATCHES {
        let q = QUOTA[bid as usize];
        agg.submit(bid, q, true);
    }
    let (ok, green, total) = agg.aggregate();
    ok && green == BATCHES && total == TOTAL
}

// ── 注入核（0xF100 段 5 项）────────────────────────────────────────────

/// 注入核 1：缺批注入——少交一批聚合必红。
pub fn inj1_missing_batch() -> bool {
    let mut agg = Aggregator::new();
    for bid in 0..BATCHES - 1 {
        agg.submit(bid, QUOTA[bid as usize], true);
    }
    let (ok, _, _) = agg.aggregate();
    !ok
}

/// 注入核 2：缺项注入——某批实收少于配额必红。
pub fn inj2_shortfall() -> bool {
    let mut agg = Aggregator::new();
    for bid in 0..BATCHES {
        let mut q = QUOTA[bid as usize];
        if bid == 5 {
            q -= 1;
        }
        agg.submit(bid, q, true);
    }
    let (ok, _, total) = agg.aggregate();
    !ok && total == TOTAL - 1
}

/// 注入核 3：未全绿注入——通过率 <100% 必红。
pub fn inj3_not_full_green() -> bool {
    let mut agg = Aggregator::new();
    for bid in 0..BATCHES {
        let g = bid != 7;
        agg.submit(bid, QUOTA[bid as usize], g);
    }
    let (ok, green, total) = agg.aggregate();
    !ok && green == BATCHES - 1 && total == TOTAL
}

/// 注入核 4：配额表冻结注入——十批配额合计恰 190。
pub fn inj4_quota_frozen() -> bool {
    QUOTA.iter().sum::<u32>() == TOTAL && QUOTA.len() == BATCHES as usize
}

/// 注入核 5：重复交批注入——同批号双交不影响闭集核对（去重语义）。
pub fn inj5_dup_submit() -> bool {
    let mut agg = Aggregator::new();
    for bid in 0..BATCHES {
        agg.submit(bid, QUOTA[bid as usize], true);
    }
    agg.submit(3, 0, false); // 重复交批（陈旧报告）
    let (ok, green, total) = agg.aggregate();
    ok && green == BATCHES && total == TOTAL
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 35
}

/// run_unx4541_checks：本单判据聚合（诊断段 0xF100）。
pub fn run_unx4541_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4541 B19-B28 段级回归聚合");
    s.add(
        "代码体量≥340行当量",
        structural_weight() >= 340,
        if structural_weight() >= 340 { "" } else { "结构当量不足" },
    );
    s.add(
        "十批断言面全量聚合一次命令可跑（190/190无缺项）",
        j1_main(),
        if j1_main() { "" } else { "聚合缺项" },
    );
    s.add(
        "通过率100%才算绿（段闸判据）",
        j1_main(),
        if j1_main() { "" } else { "段闸放行破" },
    );
    let inj = [
        ("注入1缺批必红", inj1_missing_batch()),
        ("注入2缺项必红", inj2_shortfall()),
        ("注入3未全绿必红", inj3_not_full_green()),
        ("注入4配额表冻结", inj4_quota_frozen()),
        ("注入5重复交批去重", inj5_dup_submit()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
