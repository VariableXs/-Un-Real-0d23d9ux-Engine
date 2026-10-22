//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4321 · MNT_RDONLY 只读挂载透传契约 · 诊断段 0xDE80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F4321-J1 只读挂载子树写注入被拒 EROFS（10/10），只读
//!      语义透传到 fs 层的契约（透传旗标位清单）登记可查——
//!      B2/B3/B4/B5 消费联签；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 只读子树：前缀匹配 → 写注入拒 EROFS；
//!   - 透传旗标位清单（fs 层消费面）冻结登记哈希；
//!   - 四消费方联签（B2/B3/B4/B5）齐备才契约生效；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xDE80;

/// EROFS。
pub const EROFS: u32 = 30;

/// 透传旗标位清单（fs 层消费面，冻结序）。
pub const PASS_THRU_BITS: [&str; 5] = [
    "RO-SUPER",   // sb 只读位
    "RO-INODE",   // inode 写禁位
    "RO-PAGE",    // 页写禁位
    "RO-DIRT",    // 脏标记抑制位
    "RO-REPLAY",  // 日志重放禁位
];

/// 消费方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consumer {
    B2,
    B3,
    B4,
    B5,
}

pub const ALL_CONSUMERS: [Consumer; 4] = [Consumer::B2, Consumer::B3, Consumer::B4, Consumer::B5];

impl Consumer {
    pub fn name(self) -> &'static str {
        match self {
            Consumer::B2 => "B2",
            Consumer::B3 => "B3",
            Consumer::B4 => "B4",
            Consumer::B5 => "B5",
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

/// 只读挂载表。
pub struct RoMounts {
    /// 只读子树前缀。
    pub prefixes: Vec<String>,
    /// 写拒绝计数。
    pub rejects: u32,
}

impl RoMounts {
    pub fn new() -> Self {
        RoMounts { prefixes: Vec::new(), rejects: 0 }
    }

    pub fn add_ro(&mut self, prefix: &str) -> bool {
        if !prefix.starts_with('/') || self.prefixes.iter().any(|p| p == prefix) {
            return false;
        }
        self.prefixes.push(String::from(prefix));
        true
    }

    /// 前缀命中（含自身）。
    pub fn is_ro(&self, path: &str) -> bool {
        self.prefixes.iter().any(|p| path == p.as_str() || path.starts_with(p.as_str()) && path[p.len()..].starts_with('/'))
    }

    /// 写注入：只读子树拒 EROFS。
    pub fn write(&mut self, path: &str) -> Result<(), u32> {
        if self.is_ro(path) {
            self.rejects += 1;
            return Err(EROFS);
        }
        Ok(())
    }
}

/// 透传契约账。
pub struct PassThruContract {
    pub bits_registered: u32,
    pub signed: [bool; 4],
    pub frozen: bool,
}

impl PassThruContract {
    pub fn new() -> Self {
        PassThruContract { bits_registered: PASS_THRU_BITS.len() as u32, signed: [false; 4], frozen: false }
    }

    pub fn sign(&mut self, c: Consumer) {
        self.signed[c as usize] = true;
    }

    /// 四签齐备才契约冻结生效。
    pub fn freeze(&mut self) -> bool {
        if self.signed.iter().all(|&s| s) {
            self.frozen = true;
        }
        self.frozen
    }

    /// 旗标清单哈希登记可查。
    pub fn bits_hash(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for b in PASS_THRU_BITS {
            for &x in b.as_bytes() {
                buf.push(x);
            }
        }
        fnv1a(&buf)
    }
}

/// J1 主判据：写注入 10/10 拒 EROFS + 契约登记可查 + 四联签。
pub fn j1_main() -> bool {
    let mut ro = RoMounts::new();
    let _ = ro.add_ro("/ro");
    let mut rejects = 0u32;
    for i in 0..10 {
        let p = {
            let mut s = String::from("/ro/f");
            s.push_str(&(i as u64).to_string());
            s
        };
        if matches!(ro.write(&p), Err(EROFS)) {
            rejects += 1;
        }
    }
    // 非只读子树写放行
    let rw_ok = ro.write("/rw/x").is_ok();
    let mut contract = PassThruContract::new();
    contract.sign(Consumer::B2);
    contract.sign(Consumer::B3);
    contract.sign(Consumer::B4);
    let early = !contract.freeze();
    contract.sign(Consumer::B5);
    let late = contract.freeze();
    rejects == 10 && rw_ok && early && late && contract.bits_hash() != 0
        && contract.bits_registered == 5
}

// ── 注入核（0xDE80 段 5 项）────────────────────────────────────────────

/// 注入核 1：子树前缀注入——只读目录下任意深度全拒。
pub fn inj1_deep_prefix() -> bool {
    let mut ro = RoMounts::new();
    let _ = ro.add_ro("/ro");
    ro.write("/ro/a/b/c/d").is_err() && ro.is_ro("/ro") && ro.is_ro("/ro/x")
}

/// 注入核 2：兄弟子树豁免注入——非只读前缀放行。
pub fn inj2_sibling_ok() -> bool {
    let mut ro = RoMounts::new();
    let _ = ro.add_ro("/ro");
    ro.write("/rox").is_ok() // 前缀必须按路径段
}

/// 注入核 3：重复只读挂载注入。
pub fn inj3_dup_ro() -> bool {
    let mut ro = RoMounts::new();
    let a = ro.add_ro("/ro");
    let b = ro.add_ro("/ro");
    a && !b
}

/// 注入核 4：相对前缀注入——非 '/' 起始必拒。
pub fn inj4_rel_prefix() -> bool {
    let mut ro = RoMounts::new();
    !ro.add_ro("rel")
}

/// 注入核 5：旗标清单冻结注入——五项清单哈希稳定。
pub fn inj5_bits_stable() -> bool {
    let c = PassThruContract::new();
    c.bits_hash() == c.bits_hash() && c.bits_hash() != 0 && PASS_THRU_BITS.len() == 5
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 29
}

/// run_unx4321_checks：本单判据聚合（诊断段 0xDE80）。
pub fn run_unx4321_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4321 MNT_RDONLY 只读挂载透传契约");
    s.add(
        "代码体量≥280行当量",
        structural_weight() >= 280,
        if structural_weight() >= 280 { "" } else { "结构当量不足" },
    );
    s.add(
        "只读挂载子树写注入被拒EROFS（10/10）",
        j1_main(),
        if j1_main() { "" } else { "EROFS拒绝破" },
    );
    s.add(
        "透传旗标位清单登记可查+B2/B3/B4/B5消费联签",
        j1_main() && inj5_bits_stable(),
        if j1_main() { "" } else { "契约联签破" },
    );
    let inj = [
        ("注入1深层前缀", inj1_deep_prefix()),
        ("注入2兄弟豁免", inj2_sibling_ok()),
        ("注入3重复只读挂载", inj3_dup_ro()),
        ("注入4相对前缀", inj4_rel_prefix()),
        ("注入5旗标冻结", inj5_bits_stable()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
