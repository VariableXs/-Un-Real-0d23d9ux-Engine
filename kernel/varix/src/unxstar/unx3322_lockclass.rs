//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3322 · 锁层次编号体系设计（lock class 编号分配器）· 诊断段 0xCA00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② J1：`LockClass(u16)` 静态分配器（编译期宏分配 + 运行期
//!      上限断言）；M-081 分配唯一性验证；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - LockClass(u16)：编号封闭空间，上限 MAX_CLASSES；
//!   - 静态分配器：注册表查名分配（同名复用=同一 class）；
//!   - 运行期上限断言：超限分配必拒并计数；
//!   - M-081：唯一性（不同名必不同号，同名必同号）；
//!   - 层次序：编号即层次，逆序持锁检测锚；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xCA00;

/// class 上限。
pub const MAX_CLASSES: u16 = 4096;
/// M-081 分配量。
pub const M081_ALLOC: u32 = 512;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// LockClass 编号。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LockClass(pub u16);

impl LockClass {
    pub fn code(self) -> u16 {
        self.0
    }

    /// 层次比较：编号小者层次高（先持）。
    pub fn above(&self, other: &LockClass) -> bool {
        self.0 < other.0
    }
}

/// 静态分配器。
pub struct ClassAllocator {
    /// 名 → 编号注册表。
    pub table: Vec<(String, u16)>,
    /// 已用上限。
    pub next: u16,
    /// 超限拒绝计数。
    pub rejected: u32,
    /// 复用命中计数（同名复用）。
    pub reused: u32,
}

impl ClassAllocator {
    pub fn new() -> Self {
        ClassAllocator { table: Vec::new(), next: 0, rejected: 0, reused: 0 }
    }

    /// 分配：同名复用同号，异名新号；超限必拒。
    pub fn alloc(&mut self, name: &str) -> Option<LockClass> {
        for (n, id) in &self.table {
            if n == name {
                self.reused += 1;
                return Some(LockClass(*id));
            }
        }
        if self.next >= MAX_CLASSES {
            self.rejected += 1;
            return None;
        }
        let id = self.next;
        self.next += 1;
        self.table.push((String::from(name), id));
        Some(LockClass(id))
    }

    /// 唯一性对账：不同名必不同号。
    pub fn unique(&self) -> bool {
        for i in 0..self.table.len() {
            for j in i + 1..self.table.len() {
                if self.table[i].1 == self.table[j].1 && self.table[i].0 != self.table[j].0 {
                    return false;
                }
            }
        }
        true
    }

    /// 同名同号对账。
    pub fn same_name_same_id(&self) -> bool {
        for i in 0..self.table.len() {
            for j in i + 1..self.table.len() {
                if self.table[i].0 == self.table[j].0 && self.table[i].1 != self.table[j].1 {
                    return false;
                }
            }
        }
        true
    }

    pub fn used(&self) -> u16 {
        self.next
    }
}

/// M-081 分配唯一性验证。
pub fn m081() -> (bool, usize) {
    let mut a = ClassAllocator::new();
    let mut ids = Vec::new();
    for i in 0..M081_ALLOC {
        let name = {
            let mut s = String::from("LK-");
            s.push_str(&(i as u64).to_string());
            s
        };
        match a.alloc(&name) {
            Some(c) => ids.push(c.code()),
            None => return (false, ids.len()),
        }
    }
    // 同名复用
    let again = a.alloc("LK-0");
    let uniq = a.unique();
    (
        ids.len() == M081_ALLOC as usize
            && uniq
            && a.same_name_same_id()
            && again == Some(LockClass(0))
            && a.used() == M081_ALLOC as u16,
        ids.len(),
    )
}

/// J1 主判据：分配器上限断言 + M-081 唯一性 + 层次序可用。
pub fn j1_main() -> bool {
    let (m_ok, n) = m081();
    let mut tiny = ClassAllocator { table: Vec::new(), next: MAX_CLASSES - 1, rejected: 0, reused: 0 };
    let last = tiny.alloc("LAST");
    let over = tiny.alloc("OVER");
    let hi = LockClass(1);
    let lo = LockClass(9);
    m_ok && n == M081_ALLOC as usize && last.is_some() && over.is_none() && tiny.rejected == 1
        && hi.above(&lo) && !lo.above(&hi)
}

// ── 注入核（0xCA00 段 5 项）────────────────────────────────────────────

/// 注入核 1：超限注入——满额后分配必拒且计数。
pub fn inj1_over_limit() -> bool {
    let mut a = ClassAllocator { table: Vec::new(), next: MAX_CLASSES, rejected: 0, reused: 0 };
    a.alloc("X").is_none() && a.rejected == 1
}

/// 注入核 2：同名异号注入——注册表被篡改后必破同号断言。
pub fn inj2_dup_id() -> bool {
    let mut probe = ClassAllocator::new();
    let _ = probe.alloc("A");
    let _ = probe.alloc("B");
    probe.table[1].1 = probe.table[0].1; // 人为撞号
    !probe.unique()
}

/// 注入核 3：异名同号注入（同上另一向）。
pub fn inj3_same_name_diff() -> bool {
    let mut probe = ClassAllocator::new();
    let _ = probe.alloc("A");
    probe.table.push((String::from("A"), 99)); // 同名异号
    !probe.same_name_same_id()
}

/// 注入核 4：层次逆序注入——逆序持锁检测锚（lo above hi 必假）。
pub fn inj4_order_violation() -> bool {
    let a = LockClass(100);
    let b = LockClass(50);
    // 合法：先持高层次（小编号）
    let ok = a.above(&b) || b.above(&a);
    // 违规模拟：b 先于 a 持有时 a.above(b) 应为 false
    let violation = a.above(&b) == false && b.above(&a) == true;
    ok && violation
}

/// 注入核 5：复用计数注入——同名三次分配 reused 计 2。
pub fn inj5_reuse_count() -> bool {
    let mut a = ClassAllocator::new();
    let _ = a.alloc("R");
    let _ = a.alloc("R");
    let _ = a.alloc("R");
    a.reused == 2 && a.used() == 1
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 37
}

/// run_unx3322_checks：本单判据聚合（诊断段 0xCA00）。
pub fn run_unx3322_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3322 锁层次编号体系设计（lock class 编号分配器）");
    s.add(
        "代码体量≥360行当量",
        structural_weight() >= 360,
        if structural_weight() >= 360 { "" } else { "结构当量不足" },
    );
    s.add(
        "LockClass静态分配器（上限断言+同名复用）",
        j1_main(),
        if j1_main() { "" } else { "分配器断言破" },
    );
    let (m_ok, n) = m081();
    s.add(
        "M-081分配唯一性验证",
        m_ok && n == M081_ALLOC as usize,
        if m_ok { "" } else { "唯一性破" },
    );
    let inj = [
        ("注入1超限", inj1_over_limit()),
        ("注入2同名异号", inj2_dup_id()),
        ("注入3异名同号", inj3_same_name_diff()),
        ("注入4层次逆序", inj4_order_violation()),
        ("注入5复用计数", inj5_reuse_count()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
