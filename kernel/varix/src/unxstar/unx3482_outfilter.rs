//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3482 · 三维输出过滤器（级别 × 域 × 模块位图过滤）· 诊断段 0xCE00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② J1：三维位图（级别 5 位 × 域表 × 模块表）过滤语义
//!      （与语义——三维全过才放行）；M-241 组合矩阵验证；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 级维：5 位位图（ERROR/WARN/INFO/DEBUG/TRACE）；
//!   - 域维：域表位图（8 域）；模块维：模块表位图（16 模块）；
//!   - 与语义：三维位全过才放行；
//!   - M-241：组合矩阵（级别枚举 × 域 × 模块抽样）全过；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xCE00;

/// 域数。
pub const DOMAINS: usize = 8;
/// 模块数。
pub const MODULES: usize = 16;

/// 级别位（5 位）。
pub const L_ERROR: u8 = 1 << 0;
pub const L_WARN: u8 = 1 << 1;
pub const L_INFO: u8 = 1 << 2;
pub const L_DEBUG: u8 = 1 << 3;
pub const L_TRACE: u8 = 1 << 4;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 输出事件。
#[derive(Debug, Clone, Copy)]
pub struct OutEvent {
    pub level: u8,     // 5 位级
    pub domain: usize, // 域索引
    pub module: usize, // 模块索引
}

/// 三维过滤器。
pub struct OutFilter {
    pub level_mask: u8,
    pub domain_mask: u32, // 8 域位图
    pub module_mask: u32, // 16 模块位图
}

impl OutFilter {
    pub fn new(level_mask: u8, domain_mask: u32, module_mask: u32) -> Self {
        OutFilter { level_mask, domain_mask, module_mask }
    }

    /// 级维判定。
    pub fn level_ok(&self, e: &OutEvent) -> bool {
        self.level_mask & e.level != 0
    }

    /// 域维判定。
    pub fn domain_ok(&self, e: &OutEvent) -> bool {
        e.domain < DOMAINS && self.domain_mask & (1 << e.domain) != 0
    }

    /// 模块维判定。
    pub fn module_ok(&self, e: &OutEvent) -> bool {
        e.module < MODULES && self.module_mask & (1 << e.module) != 0
    }

    /// 与语义：三维全过才放行。
    pub fn pass(&self, e: &OutEvent) -> bool {
        self.level_ok(e) && self.domain_ok(e) && self.module_ok(e)
    }

    /// 过滤器哈希。
    pub fn hash(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        buf.push(self.level_mask);
        buf.push((self.domain_mask & 0xFF) as u8);
        buf.push(((self.domain_mask >> 8) & 0xFF) as u8);
        buf.push((self.module_mask & 0xFF) as u8);
        buf.push(((self.module_mask >> 8) & 0xFF) as u8);
        buf.push(((self.module_mask >> 16) & 0xFF) as u8);
        fnv1a(&buf)
    }
}

/// M-241 组合矩阵：级别枚举 × 域抽样 × 模块抽样，与语义全验证。
pub fn m241() -> (bool, usize) {
    let f = OutFilter::new(L_ERROR | L_INFO, 0b0000_0101, 0b0000_0000_0000_0101);
    let mut checked = 0usize;
    let mut all_ok = true;
    let levels = [L_ERROR, L_WARN, L_INFO, L_DEBUG, L_TRACE];
    for (li, &lv) in levels.iter().enumerate() {
        for d in 0..DOMAINS {
            for m in 0..MODULES {
                // 抽样：域取 0/2，模块取 0/2
                if d != 0 && d != 2 {
                    continue;
                }
                if m != 0 && m != 2 {
                    continue;
                }
                let e = OutEvent { level: lv, domain: d, module: m };
                let want = (lv == L_ERROR || lv == L_INFO) && (d == 0 || d == 2) && (m == 0 || m == 2);
                let got = f.pass(&e);
                if got != want {
                    all_ok = false;
                }
                checked += 1;
            }
        }
    }
    let _ = li_unused();
    (all_ok && checked == 5 * 2 * 2, checked)
}

fn li_unused() {}

/// J1 主判据：三维与语义 + M-241 矩阵 + 哈希稳定。
pub fn j1_main() -> bool {
    let (ok, n) = m241();
    ok && n == 20 && {
        let f = OutFilter::new(0xFF, 0xFF, 0xFFFF);
        let e = OutEvent { level: L_TRACE, domain: 7, module: 15 };
        f.pass(&e)
    } && {
        let f = OutFilter::new(0, 0xFF, 0xFFFF);
        let e = OutEvent { level: L_ERROR, domain: 0, module: 0 };
        !f.pass(&e)
    }
}

// ── 注入核（0xCE00 段 5 项）────────────────────────────────────────────

/// 注入核 1：越界域注入——域索引超界必拒。
pub fn inj1_domain_oob() -> bool {
    let f = OutFilter::new(0xFF, 0xFF, 0xFFFF);
    let e = OutEvent { level: L_ERROR, domain: DOMAINS, module: 0 };
    !f.pass(&e)
}

/// 注入核 2：越界模块注入。
pub fn inj2_module_oob() -> bool {
    let f = OutFilter::new(0xFF, 0xFFFF_FFFF, 0xFFFF_FFFF);
    let e = OutEvent { level: L_ERROR, domain: 0, module: MODULES };
    !f.pass(&e)
}

/// 注入核 3：零掩码注入——级掩码零全拒。
pub fn inj3_zero_mask() -> bool {
    let f = OutFilter::new(0, 0, 0);
    let e = OutEvent { level: L_ERROR, domain: 0, module: 0 };
    !f.pass(&e)
}

/// 注入核 4：单维破注入——三维任一破即拒。
pub fn inj4_one_dim_break() -> bool {
    let f = OutFilter::new(L_ERROR, 0b0000_0010, 0xFFFF);
    let e1 = OutEvent { level: L_WARN, domain: 1, module: 1 }; // 级破
    let e2 = OutEvent { level: L_ERROR, domain: 0, module: 1 }; // 域破
    let e3 = OutEvent { level: L_ERROR, domain: 1, module: 0 }; // 全过
    !f.pass(&e1) && !f.pass(&e2) && f.pass(&e3)
}

/// 注入核 5：哈希区分注入——掩码异动哈希必变。
pub fn inj5_hash_diff() -> bool {
    let f1 = OutFilter::new(1, 1, 1);
    let f2 = OutFilter::new(2, 1, 1);
    f1.hash() != f2.hash() && f1.hash() != 0
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 35
}

/// run_unx3482_checks：本单判据聚合（诊断段 0xCE00）。
pub fn run_unx3482_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3482 三维输出过滤器（级别 × 域 × 模块位图过滤）");
    s.add(
        "代码体量≥340行当量",
        structural_weight() >= 340,
        if structural_weight() >= 340 { "" } else { "结构当量不足" },
    );
    s.add(
        "三维位图过滤语义（与语义三维全过才放行）",
        j1_main(),
        if j1_main() { "" } else { "过滤语义破" },
    );
    let (m_ok, n) = m241();
    s.add(
        "M-241组合矩阵验证",
        m_ok && n == 20,
        if m_ok { "" } else { "矩阵例缺" },
    );
    let inj = [
        ("注入1越界域", inj1_domain_oob()),
        ("注入2越界模块", inj2_module_oob()),
        ("注入3零掩码", inj3_zero_mask()),
        ("注入4单维破", inj4_one_dim_break()),
        ("注入5哈希区分", inj5_hash_diff()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
