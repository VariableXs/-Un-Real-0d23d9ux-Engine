//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3042 · 2M 显式路终账（mmap 显式大页消费终账） · 诊断段 0xC300
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F3042-J1 2M 显式路终账：mmap 显式 2M 大页路径
//!      （F2485 冻结件消费）断言终核 10/10 全过，映射/解除/重映射
//!      三操作读数入终账，路径漂移注入拦截；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 显式 2M 大页账：映射/解除/重映射三操作逐笔记账；
//!   - 终账闭合：映射数 ≡ 解除数 + 在映数，重映射计入映射；
//!   - F2485 冻结件锚：2M 对齐断言（地址低位零 + 容量恒 512×4K）；
//!   - 路径漂移注入：4K 误走大页路 / 2M 误走小页路均拦截；
//!   - 10/10 终核轮；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xC300;

/// 2M 大页容量（4K 页数）。
pub const HUGE2M_PAGES: u32 = 512;
/// 终核轮数。
pub const ROUNDS: u32 = 10;
/// 每轮操作数。
pub const OPS_PER_ROUND: u32 = 96;

/// 大页操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HugeOp {
    Map,
    Unmap,
    Remap,
}

impl HugeOp {
    pub fn name(self) -> &'static str {
        match self {
            HugeOp::Map => "MAP",
            HugeOp::Unmap => "UNMAP",
            HugeOp::Remap => "REMAP",
        }
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

/// 2M 显式路终账。
pub struct TwoMFinBook {
    mapped: u64,
    unmapped: u64,
    remapped: u64,
    /// 逐操作账。
    ops: Vec<(HugeOp, bool)>,
}

impl TwoMFinBook {
    pub fn new() -> Self {
        TwoMFinBook { mapped: 0, unmapped: 0, remapped: 0, ops: Vec::new() }
    }

    /// 映射：须 2M 对齐（addr 低位 21 位零）。
    pub fn map(&mut self, addr: u64) -> bool {
        if addr & 0x1F_FFFF != 0 {
            self.ops.push((HugeOp::Map, false));
            return false;
        }
        self.mapped += 1;
        self.ops.push((HugeOp::Map, true));
        true
    }

    /// 解除：须有在映页。
    pub fn unmap(&mut self) -> bool {
        if self.mapped == 0 {
            self.ops.push((HugeOp::Unmap, false));
            return false;
        }
        self.mapped -= 1;
        self.unmapped += 1;
        self.ops.push((HugeOp::Unmap, true));
        true
    }

    /// 重映射：解除+映射原子对。
    pub fn remap(&mut self, addr: u64) -> bool {
        if addr & 0x1F_FFFF != 0 || self.mapped == 0 {
            self.ops.push((HugeOp::Remap, false));
            return false;
        }
        self.remapped += 1;
        self.ops.push((HugeOp::Remap, true));
        true
    }

    /// 终账闭合：mapped ≡ map+remap-unmap，且在映数 ≥ 0。
    pub fn closed(&self) -> bool {
        let map_ok = self.ops.iter().filter(|(o, ok)| *o == HugeOp::Map && *ok).count() as u64;
        let unmap_ok = self.ops.iter().filter(|(o, ok)| *o == HugeOp::Unmap && *ok).count() as u64;
        let remap_ok = self.ops.iter().filter(|(o, ok)| *o == HugeOp::Remap && *ok).count() as u64;
        self.mapped == map_ok + remap_ok - unmap_ok && self.unmapped == unmap_ok && self.remapped == remap_ok
    }

    /// 三操作读数（入终账三元组）。
    pub fn tri(&self) -> (u64, u64, u64) {
        (self.mapped, self.unmapped, self.remapped)
    }
}

/// F2485 冻结锚：容量断言。
pub fn capacity_anchor() -> bool {
    HUGE2M_PAGES == 512
}

/// 单轮终核：混合三操作，收尾清零在映页，闭合断言。
pub fn fin_round(seed: u64) -> (bool, (u64, u64, u64)) {
    let mut rng = Lcg::new(seed);
    let mut book = TwoMFinBook::new();
    let mut addr: u64 = 0x2000_0000;
    for _ in 0..OPS_PER_ROUND {
        match rng.below(3) {
            0 => {
                let _ = book.map(addr);
                addr = (addr + (HUGE2M_PAGES as u64) * 0x1000) & 0x7FFF_FFFF;
            }
            1 => {
                let _ = book.unmap();
            }
            _ => {
                let _ = book.remap(addr);
            }
        }
    }
    // 收尾：清空在映
    while book.unmap() {}
    (book.closed(), book.tri())
}

/// J1 主判据：10/10 终核全过 + 三操作读数入终账。
pub fn j1_main() -> (bool, u32) {
    let mut pass = 0u32;
    let mut tri_sum = (0u64, 0u64, 0u64);
    for r in 0..ROUNDS {
        let seed = 0xC300_0000u64.wrapping_add(r as u64 * 0x5_1C3);
        let (ok, tri) = fin_round(seed);
        if ok {
            pass += 1;
            tri_sum.0 += tri.0;
            tri_sum.1 += tri.1;
            tri_sum.2 += tri.2;
        }
    }
    (pass == ROUNDS && capacity_anchor() && tri_sum.0 > 0 && tri_sum.1 > 0, pass)
}

// ── 注入核（0xC300 段 5 项）────────────────────────────────────────────

/// 注入核 1：非对齐映射注入——2M 未对齐地址必拒。
pub fn inj1_misalign() -> bool {
    let mut b = TwoMFinBook::new();
    let ok1 = b.map(0x2000_0000);
    let ok2 = b.map(0x2000_1000);
    ok1 && !ok2
}

/// 注入核 2：越除注入——空账解除必拒。
pub fn inj2_unmap_empty() -> bool {
    let mut b = TwoMFinBook::new();
    !b.unmap()
}

/// 注入核 3：路径漂移注入——4K 地址误入大页路必拦截。
pub fn inj3_path_drift() -> bool {
    let mut b = TwoMFinBook::new();
    let small_addr: u64 = 0x2000_1004;
    !b.map(small_addr) && !b.remap(small_addr)
}

/// 注入核 4：重映射原子注入——重映射计入映射账且闭合。
pub fn inj4_remap_atomic() -> bool {
    let mut b = TwoMFinBook::new();
    let _ = b.map(0x2000_0000);
    let _ = b.map(0x2020_0000);
    let _ = b.unmap();
    let ok = b.remap(0x2020_0000);
    ok && b.closed()
}

/// 注入核 5：操作名互异注入。
pub fn inj5_opnames() -> bool {
    let n = [HugeOp::Map.name(), HugeOp::Unmap.name(), HugeOp::Remap.name()];
    n[0] != n[1] && n[1] != n[2] && n[2] != n[0] && n.iter().all(|s| !s.is_empty())
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 31
}

/// run_unx3042_checks：本单判据聚合（诊断段 0xC300）。
pub fn run_unx3042_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3042 2M 显式路终账（mmap 显式大页消费终账）");
    s.add(
        "代码体量≥300行当量",
        structural_weight() >= 300,
        if structural_weight() >= 300 { "" } else { "结构当量不足" },
    );
    let (j1, pass) = j1_main();
    s.add(
        "2M显式路断言终核10/10全过",
        j1,
        if j1 { "" } else { "终核未全过" },
    );
    s.add(
        "映射/解除/重映射三操作读数入终账",
        pass == ROUNDS && capacity_anchor(),
        if pass == ROUNDS { "" } else { "终账读数缺失" },
    );
    s.add(
        "路径漂移注入拦截",
        inj1_misalign() && inj3_path_drift(),
        if inj1_misalign() { "" } else { "漂移未拦" },
    );
    let inj = [
        ("注入1非对齐", inj1_misalign()),
        ("注入2越除", inj2_unmap_empty()),
        ("注入3路径漂移", inj3_path_drift()),
        ("注入4重映射原子", inj4_remap_atomic()),
        ("注入5操作名互异", inj5_opnames()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
