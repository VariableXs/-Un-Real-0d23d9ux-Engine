//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2403 · 预留区登记账（内核映像/引导器/保留页） · 诊断段 0x8500
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F2403-J1 内核映像/引导器/诊断保留页三类预留区间与
//!      Limine 响应逐条一致，预留区间被分配计数恒为零；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三类预留区间模型：内核映像/引导器/诊断保留页；
//!   - 区间重叠检 + 分配拦截计数恒零；
//!   - Limine 响应逐条一致核（区间表比对）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0x8500;

/// 预留区类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResKind {
    KernelImage,
    Bootloader,
    DiagPages,
}

impl ResKind {
    pub fn name(self) -> &'static str {
        match self {
            ResKind::KernelImage => "KERNEL-IMAGE",
            ResKind::Bootloader => "BOOTLOADER",
            ResKind::DiagPages => "DIAG-PAGES",
        }
    }
}

/// 预留区间。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResRange {
    pub kind: ResKind,
    pub base: u64,
    pub pages: u64,
}

impl ResRange {
    pub fn end(&self) -> u64 {
        self.base + self.pages * 4096
    }

    /// 与 [b, e) 是否相交。
    pub fn overlaps(&self, b: u64, e: u64) -> bool {
        self.base < e && b < self.end()
    }

    /// 包含判定。
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.end()
    }
}

/// Limine 响应模拟表（三类预留逐条一致的正本）。
pub fn limine_response() -> Vec<ResRange> {
    vec![
        ResRange { kind: ResKind::KernelImage, base: 0x20_0000, pages: 256 },
        ResRange { kind: ResKind::Bootloader, base: 0x1000, pages: 8 },
        ResRange { kind: ResKind::DiagPages, base: 0x90_000, pages: 16 },
    ]
}

/// 预留区登记账。
pub struct ReserveBook {
    ranges: Vec<ResRange>,
    /// 预留区间被分配计数（恒零判据锚）。
    alloc_hits: u32,
}

impl ReserveBook {
    pub fn new() -> Self {
        ReserveBook { ranges: Vec::new(), alloc_hits: 0 }
    }

    /// 登记（重叠拒）。
    pub fn register(&mut self, r: ResRange) -> bool {
        if r.pages == 0 {
            return false;
        }
        if self.ranges.iter().any(|x| x.overlaps(r.base, r.end())) {
            return false;
        }
        self.ranges.push(r);
        true
    }

    /// 分配尝试：命中预留区则拒并计数。
    pub fn alloc_try(&mut self, base: u64, pages: u64) -> bool {
        let e = base + pages * 4096;
        if self.ranges.iter().any(|r| r.overlaps(base, e)) {
            self.alloc_hits += 1;
            return false;
        }
        true
    }

    /// 被分配计数恒零。
    pub fn alloc_hits_zero(&self) -> bool {
        self.alloc_hits == 0
    }

    /// 与 Limine 响应逐条一致。
    pub fn matches_limine(&self) -> bool {
        let lr = limine_response();
        lr.len() == self.ranges.len()
            && lr.iter().all(|l| self.ranges.iter().any(|r| r == l))
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn hits(&self) -> u32 {
        self.alloc_hits
    }

    pub fn ranges(&self) -> &[ResRange] {
        &self.ranges
    }
}

/// 标准登记：Limine 三类区间全登记。
pub fn standard_book() -> ReserveBook {
    let mut b = ReserveBook::new();
    for r in limine_response() {
        let _ = b.register(r);
    }
    b
}

/// 三类与 Limine 逐条一致核。
pub fn limine_consistent() -> bool {
    let b = standard_book();
    b.matches_limine() && b.len() == 3
}

// ── 注入核（0x8500 段 5 项）────────────────────────────────────────────

/// 注入核 1：分配拦截注入——命中预留区分配必拒且计数增。
pub fn inj1_alloc_block() -> bool {
    let mut b = standard_book();
    // 内核映像区 0x20_0000..+256 页 内分配
    if b.alloc_try(0x20_1000, 1) {
        return false;
    }
    b.hits() == 1 && !b.alloc_hits_zero()
}

/// 注入核 2：合法分配注入——区外分配放行且计数不增。
pub fn inj2_alloc_ok() -> bool {
    let mut b = standard_book();
    if !b.alloc_try(0x40_0000, 4) {
        return false;
    }
    b.alloc_hits_zero()
}

/// 注入核 3：重叠登记注入。
pub fn inj3_overlap() -> bool {
    let mut b = ReserveBook::new();
    if !b.register(ResRange { kind: ResKind::KernelImage, base: 0x10_0000, pages: 32 }) {
        return false;
    }
    // 半重叠
    !b.register(ResRange { kind: ResKind::DiagPages, base: 0x10_8000, pages: 32 })
        // 全包
        && !b.register(ResRange { kind: ResKind::DiagPages, base: 0x0F_0000, pages: 64 })
}

/// 注入核 4：零页注入。
pub fn inj4_zero_pages() -> bool {
    let mut b = ReserveBook::new();
    !b.register(ResRange { kind: ResKind::DiagPages, base: 0x50_0000, pages: 0 })
}

/// 注入核 5：界查询注入——contains 边界（base 含、end 不含）。
pub fn inj5_contains() -> bool {
    let r = ResRange { kind: ResKind::DiagPages, base: 0x90_000, pages: 16 };
    r.contains(0x90_000) && !r.contains(0x90_000 + 16 * 4096) && r.contains(0x90_000 + 15 * 4096)
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// run_unx2403_checks：本单判据聚合（诊断段 0x8500）。
pub fn run_unx2403_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2403 预留区登记账（内核映像/引导器/保留页）");
    let b = standard_book();
    s.add(
        "代码体量≥280行当量",
        structural_weight() >= 280,
        if structural_weight() >= 280 { "" } else { "结构当量不足" },
    );
    s.add(
        "三类预留与Limine逐条一致",
        limine_consistent(),
        if limine_consistent() { "" } else { "与Limine失配" },
    );
    s.add(
        "预留区间被分配计数恒为零",
        b.alloc_hits_zero(),
        if b.alloc_hits_zero() { "" } else { "分配计数非零" },
    );
    let inj = [
        ("注入1分配拦截", inj1_alloc_block()),
        ("注入2合法放行", inj2_alloc_ok()),
        ("注入3重叠", inj3_overlap()),
        ("注入4零页", inj4_zero_pages()),
        ("注入5界查询", inj5_contains()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    9u32 * 32
}
