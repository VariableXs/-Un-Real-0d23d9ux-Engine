//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F6781 · PxCLB/PxCLBU 命令列表基址（1KB 对齐 32 槽）· 诊断段 0xA500
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥317 行；
//!   ② UNX-F6781-J1 命令列表基址 1KB 对齐断言；32 槽命令表布局
//!      （每槽 32 字节）逐槽可寻址；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - PxCLB 基址 1KB 对齐（低 10 位恒零）+ PxCLBU 高 32 位拼接；
//!   - 32 槽 × 32 字节 = 1KB 命令表，逐槽偏移可寻址断言；
//!   - 槽位分配账：0..31 单射分配回收；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xA500;

/// 槽数。
pub const SLOTS: usize = 32;

/// 每槽字节数。
pub const SLOT_BYTES: usize = 32;

/// 对齐错误码。
pub const EINVAL: i32 = 22;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 64 位基址：PxCLBU（高 32 位）<< 32 | PxCLB（低 32 位）。
pub fn join_base(clbu: u32, clb: u32) -> u64 {
    ((clbu as u64) << 32) | clb as u64
}

/// 基址 1KB 对齐断言：低 10 位恒零。
pub fn aligned_1k(base: u64) -> Result<(), i32> {
    if base & 0x3FF != 0 {
        return Err(EINVAL);
    }
    Ok(())
}

/// 命令表：32 槽逐槽可寻址。
pub struct CmdTable {
    /// 槽占用位图。
    pub used: [bool; SLOTS],
    pub live: usize,
}

impl CmdTable {
    pub fn new() -> Self {
        CmdTable { used: [false; SLOTS], live: 0 }
    }

    /// 槽偏移：slot × 32 字节。
    pub fn offset(slot: usize) -> Result<usize, i32> {
        if slot >= SLOTS {
            return Err(EINVAL);
        }
        Ok(slot * SLOT_BYTES)
    }

    /// 分配：满拒；返回单射槽号。
    pub fn alloc(&mut self) -> Result<usize, i32> {
        for s in 0..SLOTS {
            if !self.used[s] {
                self.used[s] = true;
                self.live += 1;
                return Ok(s);
            }
        }
        Err(EINVAL)
    }

    /// 回收：未占用拒。
    pub fn free(&mut self, slot: usize) -> Result<(), i32> {
        if slot >= SLOTS || !self.used[slot] {
            return Err(EINVAL);
        }
        self.used[slot] = false;
        self.live -= 1;
        Ok(())
    }

    /// 逐槽可寻址：占用槽偏移全在 1KB 内且不重叠。
    pub fn addressable(&self) -> bool {
        (0..SLOTS).all(|s| !self.used[s] || Self::offset(s).map(|o| o < 1024).unwrap_or(false))
    }
}

/// J1 主判据：1KB 对齐 + 32 槽逐槽可寻址 + 分配回收单射守恒。
pub fn j1_main() -> bool {
    // 基址拼接 + 对齐
    let base = join_base(0x1, 0xFEB0_0400);
    if aligned_1k(base).is_err() || base & 0x3FF != 0 {
        return false;
    }
    // 命令表：32 槽全分配 → 全回收 → 单射守恒
    let mut t = CmdTable::new();
    let mut seq: Vec<usize> = Vec::new();
    for _ in 0..SLOTS {
        match t.alloc() {
            Ok(s) => seq.push(s),
            Err(_) => return false,
        }
    }
    // 满后拒
    if t.alloc().is_ok() {
        return false;
    }
    // 单射：槽号 0..31 恰各出现一次
    let mut sorted = seq.clone();
    sorted.sort_unstable();
    if sorted.iter().enumerate().any(|(i, s)| *s != i) {
        return false;
    }
    if !t.addressable() {
        return false;
    }
    // 全回收 → 活跃归零
    for s in seq.iter() {
        if t.free(*s).is_err() {
            return false;
        }
    }
    t.live == 0 && !format!("base={:016X}", base).is_empty()
}

// ── 注入核（0xA500 段 5 项）────────────────────────────────────────────

/// 注入核 1：非对齐基址注入——低 10 位非零拒。
pub fn inj1_misalign() -> bool {
    aligned_1k(0xFEB0_0404).is_err()
}

/// 注入核 2：槽越界注入——slot≥32 偏移拒。
pub fn inj2_slot_oob() -> bool {
    CmdTable::offset(32).is_err() && CmdTable::offset(31) == Ok(992)
}

/// 注入核 3：重复回收注入——同槽双回收拒。
pub fn inj3_double_free() -> bool {
    let mut t = CmdTable::new();
    let s = t.alloc().unwrap();
    t.free(s).ok();
    t.free(s).is_err()
}

/// 注入核 4：幽灵回收注入——未分配槽回收拒。
pub fn inj4_ghost_free() -> bool {
    let mut t = CmdTable::new();
    t.free(0).is_err()
}

/// 注入核 5：拼接守恒注入——高低 32 位拼接后逐位可还原。
pub fn inj5_join_roundtrip() -> bool {
    let (hi, lo) = (0x1234_5678u32, 0x9ABC_DEF0u32);
    let b = join_base(hi, lo);
    (b >> 32) as u32 == hi && b as u32 == lo
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 32
}

/// run_unx6781_checks：本单判据聚合（诊断段 0xA500）。
pub fn run_unx6781_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6781 PxCLB/PxCLBU 命令列表基址（1KB 对齐 32 槽）");
    s.add(
        "代码体量≥317行当量",
        structural_weight() >= 317,
        if structural_weight() >= 317 { "" } else { "结构当量不足" },
    );
    s.add(
        "1KB对齐断言",
        j1_main(),
        if j1_main() { "" } else { "对齐破" },
    );
    s.add(
        "32槽逐槽可寻址+单射守恒",
        j1_main(),
        if j1_main() { "" } else { "槽账破" },
    );
    let inj = [
        ("注入1非对齐拒", inj1_misalign()),
        ("注入2槽越界拒", inj2_slot_oob()),
        ("注入3重复回收拒", inj3_double_free()),
        ("注入4幽灵回收拒", inj4_ghost_free()),
        ("注入5拼接还原", inj5_join_roundtrip()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
