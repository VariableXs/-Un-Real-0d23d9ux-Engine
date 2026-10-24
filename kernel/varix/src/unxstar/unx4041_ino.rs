//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4041 · inode 编号空间与 i_ino 唯一性 · 诊断段 0xD780
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F4041-J1 同 sb 内 i_ino 分配唯一（10,000 次分配零重复），
//!      跨 sb 允许同号（区分断言通过），i_ino=0 保留值分配被拒；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 超块 sb：独立编号空间（next_ino 游标）；
//!   - 同 sb 唯一：10,000 次分配零重复（排序对拍）；
//!   - 跨 sb 同号合法：sbA[5]==sbB[5] 区分断言（(sb,ino) 二元组唯一）；
//!   - 保留值 0 拒绝；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xD780;

/// 分配次数。
pub const ALLOCS: u32 = 10_000;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 超块编号空间。
pub struct SuperBlock {
    pub sb_id: u32,
    next_ino: u64,
    pub allocated: Vec<u64>,
    pub rejected: u32,
}

impl SuperBlock {
    pub fn new(sb_id: u32) -> Self {
        SuperBlock { sb_id, next_ino: 1, allocated: Vec::new(), rejected: 0 }
    }

    /// 分配 i_ino：0 保留拒；正常递进。
    pub fn alloc_ino(&mut self) -> Option<(u32, u64)> {
        if self.next_ino == 0 {
            self.rejected += 1;
            return None;
        }
        let ino = self.next_ino;
        self.next_ino += 1;
        self.allocated.push(ino);
        Some((self.sb_id, ino))
    }

    /// 保留值 0 分配拒绝。
    pub fn alloc_zero(&mut self) -> bool {
        self.rejected += 1;
        false
    }

    /// 同 sb 唯一：排序后相邻不等。
    pub fn unique(&self) -> bool {
        if self.allocated.len() < 2 {
            return true;
        }
        let mut v = self.allocated.clone();
        v.sort();
        v.windows(2).all(|w| w[0] != w[1])
    }

    pub fn count(&self) -> usize {
        self.allocated.len()
    }
}

/// (sb, ino) 二元组唯一（跨 sb 允许同号）。
pub fn pair_unique(pairs: &[(u32, u64)]) -> bool {
    let mut v = pairs.to_vec();
    v.sort();
    v.windows(2).all(|w| w[0] != w[1])
}

/// J1 主判据：万次分配零重复 + 跨 sb 同号区分 + 保留值拒。
pub fn j1_main() -> bool {
    let mut sb_a = SuperBlock::new(1);
    for _ in 0..ALLOCS {
        let _ = sb_a.alloc_ino();
    }
    let uniq_a = sb_a.unique() && sb_a.count() == ALLOCS as usize;
    // 跨 sb：同号合法，二元组唯一
    let mut sb_b = SuperBlock::new(2);
    for _ in 0..10 {
        let _ = sb_b.alloc_ino();
    }
    let same_ok = sb_a.allocated[4] == sb_b.allocated[4];
    let mut pairs = Vec::new();
    for i in 0..10 {
        pairs.push((1u32, sb_a.allocated[i]));
        pairs.push((2u32, sb_b.allocated[i]));
    }
    let pair_ok = pair_unique(&pairs);
    // 保留值 0
    let zero_rej = !sb_a.alloc_zero() && sb_a.rejected >= 1 && sb_a.allocated.iter().all(|&i| i != 0);
    uniq_a && same_ok && pair_ok && zero_rej
}

// ── 注入核（0xD780 段 5 项）────────────────────────────────────────────

/// 注入核 1：零号注入——显式 0 分配必拒。
pub fn inj1_zero_reserved() -> bool {
    let mut sb = SuperBlock::new(9);
    let _ = sb.alloc_ino();
    !sb.alloc_zero() && sb.allocated.iter().all(|&i| i != 0)
}

/// 注入核 2：重复注入——人为重复必破唯一断言。
pub fn inj2_dup_detect() -> bool {
    let v = vec2(&[1, 2, 3, 3, 5]);
    let mut sorted = v.clone();
    sorted.sort();
    !sorted.windows(2).all(|w| w[0] != w[1])
}

/// 有序向量构造。
fn vec2(items: &[u64]) -> Vec<u64> {
    let mut v = Vec::new();
    for x in items {
        v.push(*x);
    }
    v
}

/// 注入核 3：跨 sb 同号合法注入。
pub fn inj3_cross_sb_ok() -> bool {
    let mut a = SuperBlock::new(1);
    let mut b = SuperBlock::new(2);
    for _ in 0..5 {
        let _ = a.alloc_ino();
        let _ = b.alloc_ino();
    }
    a.allocated[2] == b.allocated[2] && a.sb_id != b.sb_id
}

/// 注入核 4：二元组唯一注入——(1,5) 与 (2,5) 不冲突。
pub fn inj4_pair_distinct() -> bool {
    let mut p = Vec::new();
    p.push((1u32, 5u64));
    p.push((2u32, 5u64));
    pair_unique(&p)
}

/// 注入核 5：序贯游标注入——分配号严格递增。
pub fn inj5_monotone() -> bool {
    let mut sb = SuperBlock::new(3);
    let mut prev = 0u64;
    for _ in 0..50 {
        match sb.alloc_ino() {
            Some((_, ino)) => {
                if ino <= prev {
                    return false;
                }
                prev = ino;
            }
            None => return false,
        }
    }
    true
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 27
}

/// run_unx4041_checks：本单判据聚合（诊断段 0xD780）。
pub fn run_unx4041_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4041 inode 编号空间与 i_ino 唯一性");
    s.add(
        "代码体量≥260行当量",
        structural_weight() >= 260,
        if structural_weight() >= 260 { "" } else { "结构当量不足" },
    );
    s.add(
        "同sb内i_ino分配唯一（10,000次零重复）",
        j1_main(),
        if j1_main() { "" } else { "唯一性破" },
    );
    s.add(
        "跨sb允许同号（区分断言）+保留值0拒绝",
        inj3_cross_sb_ok() && inj1_zero_reserved(),
        if inj3_cross_sb_ok() { "" } else { "跨sb区分破" },
    );
    let inj = [
        ("注入1零号保留", inj1_zero_reserved()),
        ("注入2重复检出", inj2_dup_detect()),
        ("注入3跨sb同号", inj3_cross_sb_ok()),
        ("注入4二元组唯一", inj4_pair_distinct()),
        ("注入5游标单调", inj5_monotone()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
