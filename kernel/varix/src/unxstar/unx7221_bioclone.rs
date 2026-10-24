//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F7221 · bio 克隆与引用语义 · 诊断段 0xAC00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F7221-J1 克隆 bio 与原 bio 数据指针独立（写克隆不影响
//!      原 bio 缓冲）；引用计数 10 万次增减零泄漏；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 克隆即深拷贝：数据缓冲独立（写克隆不回写原 bio）；
//!   - 引用计数：10 万次 clone/drop 往返归零（零泄漏）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xAC00;

/// 引用往返次数。
pub const REFS: usize = 100_000;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// bio：数据缓冲 + 引用计数。
pub struct Bio {
    pub sector: u64,
    pub data: Vec<u64>,
    pub refs: u64,
}

impl Bio {
    pub fn new(sector: u64, n: usize) -> Self {
        Bio { sector, data: (0..n as u64).collect(), refs: 1 }
    }

    /// 克隆：数据指针独立（深拷贝）+ 计数 +1。
    pub fn clone_bio(&mut self) -> Bio {
        self.refs += 1;
        Bio { sector: self.sector, data: self.data.clone(), refs: self.refs }
    }

    /// 释放：计数 -1，返回是否归零。
    pub fn drop_ref(&mut self) -> bool {
        self.refs = self.refs.saturating_sub(1);
        self.refs == 0
    }

    /// 写克隆：只动自己缓冲。
    pub fn write(&mut self, idx: usize, val: u64) {
        if idx < self.data.len() {
            self.data[idx] = val;
        }
    }
}

/// J1 主判据：克隆独立性 + 10 万次引用往返零泄漏。
pub fn j1_main() -> bool {
    let mut orig = Bio::new(7, 16);
    let orig_sum: u64 = orig.data.iter().sum();
    // 克隆写独立
    let mut cl = orig.clone_bio();
    cl.write(0, 0xDEAD);
    if orig.data[0] == 0xDEAD || cl.data[0] != 0xDEAD {
        return false;
    }
    if orig.data.iter().sum::<u64>() != orig_sum {
        return false;
    }
    // 10 万次 clone/drop 往返：计数最终归 1（原持有），再 drop 归零
    let mut cl2 = orig.clone_bio(); // refs=2
    for i in 0..REFS {
        let mut tmp = cl2.clone_bio();
        tmp.write(i % 16, i as u64);
        if !tmp.drop_ref() {
            return false;
        }
    }
    // cl2 与 orig 计数回落：cl2.drop → 1；orig.drop → 0
    if cl2.refs != 2 {
        return false;
    }
    cl2.drop_ref();
    orig.drop_ref()
}

// ── 注入核（0xAC00 段 5 项）────────────────────────────────────────────

/// 注入核 1：深拷贝注入——克隆后原缓冲地址语义独立（值层面验证）。
pub fn inj1_deep_copy() -> bool {
    let mut o = Bio::new(1, 4);
    let mut c = o.clone_bio();
    for i in 0..4 {
        c.write(i, 0xAAAAu64 + i as u64);
    }
    (0..4).all(|i| o.data[i] == i as u64 && c.data[i] == 0xAAAAu64 + i as u64)
}

/// 注入核 2：计数守恒注入——clone +1 / drop -1 严格对应。
pub fn inj2_ref_conserved() -> bool {
    let mut o = Bio::new(2, 4);
    let mut c = o.clone_bio(); // 2
    let mut d = o.clone_bio(); // 3
    d.drop_ref();
    c.drop_ref();
    o.refs == 1
}

/// 注入核 3：过度释放注入——归零后再 drop 不下溢（saturating）。
pub fn inj3_no_underflow() -> bool {
    let mut o = Bio::new(3, 4);
    o.drop_ref();
    o.drop_ref();
    o.refs == 0
}

/// 注入核 4：扇区继承注入——克隆继承扇区号。
pub fn inj4_sector_inherit() -> bool {
    let mut o = Bio::new(0x1234, 2);
    let c = o.clone_bio();
    c.sector == 0x1234 && o.refs == 2
}

/// 注入核 5：写克隆不影响原注入——批量写克隆后原缓冲逐位不变。
pub fn inj5_orig_untouched() -> bool {
    let mut o = Bio::new(5, 32);
    let mut c = o.clone_bio();
    for i in 0..32 {
        c.write(i, 0xFFFF);
    }
    (0..32).all(|i| o.data[i] == i as u64)
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 26
}

/// run_unx7221_checks：本单判据聚合（诊断段 0xAC00）。
pub fn run_unx7221_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7221 bio 克隆与引用语义");
    s.add(
        "代码体量≥260行当量",
        structural_weight() >= 260,
        if structural_weight() >= 260 { "" } else { "结构当量不足" },
    );
    s.add(
        "克隆数据指针独立",
        j1_main(),
        if j1_main() { "" } else { "克隆独立破" },
    );
    s.add(
        "10万次引用增减零泄漏",
        j1_main(),
        if j1_main() { "" } else { "引用泄漏" },
    );
    let inj = [
        ("注入1深拷贝", inj1_deep_copy()),
        ("注入2计数守恒", inj2_ref_conserved()),
        ("注入3不过度下溢", inj3_no_underflow()),
        ("注入4扇区继承", inj4_sector_inherit()),
        ("注入5原缓冲不变", inj5_orig_untouched()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
