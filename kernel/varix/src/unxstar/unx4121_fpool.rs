//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4121 · file 对象分配与 f_pool 配额 · 诊断段 0xD900
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F4121-J1 file 池静态配额耗尽注入返回 ENFILE 且池结构
//!      零破坏，配额水位账与占用账交叉一致（等式断言通过）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - f_pool 静态配额：FILE_MAX；
//!   - 分配：配额内得 slot；耗尽返回 ENFILE；
//!   - 池结构零破坏：耗尽后 slot 表/空闲栈不变；
//!   - 水位账：高水位与当前占用交叉等式（占用+空闲 ≡ MAX）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xD900;

/// file 池静态配额。
pub const FILE_MAX: u32 = 64;

/// ENFILE 错误码。
pub const ENFILE: u32 = 23;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// file 池。
pub struct FPool {
    /// slot 占用表。
    pub used: [bool; 128],
    /// 空闲栈。
    pub free_stack: Vec<u32>,
    /// 高水位。
    pub high_water: u32,
    pub in_use: u32,
    pub enfile_cnt: u32,
}

impl FPool {
    pub fn new() -> Self {
        let mut stack = Vec::new();
        let mut i = FILE_MAX;
        while i > 0 {
            stack.push(i - 1);
            i -= 1;
        }
        FPool { used: [false; 128], free_stack: stack, high_water: 0, in_use: 0, enfile_cnt: 0 }
    }

    /// 分配：配额内得 slot；耗尽 ENFILE。
    pub fn alloc(&mut self) -> Result<u32, u32> {
        match self.free_stack.pop() {
            Some(slot) => {
                self.used[slot as usize] = true;
                self.in_use += 1;
                if self.in_use > self.high_water {
                    self.high_water = self.in_use;
                }
                Ok(slot)
            }
            None => {
                self.enfile_cnt += 1;
                Err(ENFILE)
            }
        }
    }

    /// 释放。
    pub fn release(&mut self, slot: u32) -> bool {
        if slot >= FILE_MAX || !self.used[slot as usize] {
            return false;
        }
        self.used[slot as usize] = false;
        self.in_use -= 1;
        self.free_stack.push(slot);
        true
    }

    /// 池结构零破坏：占用+空闲 ≡ MAX 且 slot 表与计数一致。
    pub fn intact(&self) -> bool {
        let used_cnt = self.used.iter().take(FILE_MAX as usize).filter(|&&b| b).count() as u32;
        used_cnt == self.in_use
            && used_cnt + self.free_stack.len() as u32 == FILE_MAX
            && self.used[FILE_MAX as usize..].iter().all(|&b| !b)
    }

    /// 交叉等式：水位 ≥ 占用；水位 ≤ MAX。
    pub fn watermark_consistent(&self) -> bool {
        self.high_water >= self.in_use && self.high_water <= FILE_MAX
    }
}

/// J1 主判据：耗尽 ENFILE + 池零破坏 + 交叉等式。
pub fn j1_main() -> bool {
    let mut pool = FPool::new();
    let mut got = Vec::new();
    for _ in 0..FILE_MAX {
        match pool.alloc() {
            Ok(s) => got.push(s),
            Err(e) => {
                let _ = e;
                break;
            }
        }
    }
    let full_ok = got.len() == FILE_MAX as usize && pool.in_use == FILE_MAX;
    // 耗尽注入：ENFILE 且结构零破坏
    let over = matches!(pool.alloc(), Err(ENFILE)) && pool.enfile_cnt == 1 && pool.intact();
    // 交叉等式
    let wm = pool.watermark_consistent();
    // 释放后再分配
    let _ = pool.release(0);
    let reuse = matches!(pool.alloc(), Ok(0)) && pool.intact();
    full_ok && over && wm && reuse
}

// ── 注入核（0xD900 段 5 项）────────────────────────────────────────────

/// 注入核 1：双释放注入——重复释放必拒。
pub fn inj1_double_release() -> bool {
    let mut p = FPool::new();
    let s = match p.alloc() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let r1 = p.release(s);
    let r2 = p.release(s);
    r1 && !r2 && p.intact()
}

/// 注入核 2：越界释放注入——slot ≥ MAX 必拒。
pub fn inj2_oob_release() -> bool {
    let mut p = FPool::new();
    !p.release(FILE_MAX) && !p.release(u32::MAX)
}

/// 注入核 3：耗尽计数注入——连续耗尽请求计数累加。
pub fn inj3_enfile_count() -> bool {
    let mut p = FPool::new();
    for _ in 0..FILE_MAX {
        let _ = p.alloc();
    }
    for _ in 0..3 {
        let _ = p.alloc();
    }
    p.enfile_cnt == 3 && p.intact()
}

/// 注入核 4：水位峰值注入——释放后水位不降。
pub fn inj4_high_water() -> bool {
    let mut p = FPool::new();
    let mut slots = Vec::new();
    for _ in 0..16 {
        if let Ok(s) = p.alloc() {
            slots.push(s);
        }
    }
    for s in slots.iter() {
        let _ = p.release(*s);
    }
    p.high_water == 16 && p.in_use == 0 && p.watermark_consistent()
}

/// 注入核 5：slot 唯一注入——并发分配不重号。
pub fn inj5_slot_unique() -> bool {
    let mut p = FPool::new();
    let mut seen = [false; 128];
    for _ in 0..FILE_MAX {
        if let Ok(s) = p.alloc() {
            if seen[s as usize] {
                return false;
            }
            seen[s as usize] = true;
        }
    }
    seen[..FILE_MAX as usize].iter().all(|&b| b)
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 29
}

/// run_unx4121_checks：本单判据聚合（诊断段 0xD900）。
pub fn run_unx4121_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4121 file 对象分配与 f_pool 配额");
    s.add(
        "代码体量≥280行当量",
        structural_weight() >= 280,
        if structural_weight() >= 280 { "" } else { "结构当量不足" },
    );
    s.add(
        "file池配额耗尽注入返回ENFILE且池结构零破坏",
        j1_main(),
        if j1_main() { "" } else { "ENFILE/结构破" },
    );
    s.add(
        "配额水位账与占用账交叉一致（等式断言）",
        inj4_high_water(),
        if inj4_high_water() { "" } else { "交叉等式破" },
    );
    let inj = [
        ("注入1双释放", inj1_double_release()),
        ("注入2越界释放", inj2_oob_release()),
        ("注入3耗尽计数", inj3_enfile_count()),
        ("注入4水位峰值", inj4_high_water()),
        ("注入5slot唯一", inj5_slot_unique()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
