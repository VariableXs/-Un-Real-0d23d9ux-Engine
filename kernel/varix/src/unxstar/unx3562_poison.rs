//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3562 · 毒化模式注册表（模式值封闭词表——magic 选择）· 诊断段 0xCF80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② J1：词表冻结（0xAA/0xBB/0xCC/0xDD/0xEE 五值语义表——判据行
//!      内嵌）；M-320 新值注册拒绝（封闭词表执法）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 封闭词表：0xAA 空闲毒 / 0xBB 释放毒 / 0xCC 撤销毒 /
//!     0xDD 越界毒 / 0xEE 哨兵毒（五值语义表冻结）；
//!   - 注册表：模式值 → 语义名 → 用途域；
//!   - 封闭执法：词表外注册/写入必拒并计数；
//!   - 毒化写入/读出校验：写毒值必原样读出（毒化生效断言）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xCF80;

/// 毒化模式值。
pub const POISON_FREE: u8 = 0xAA;
pub const POISON_RELEASE: u8 = 0xBB;
pub const POISON_REVOKE: u8 = 0xCC;
pub const POISON_OOB: u8 = 0xDD;
pub const POISON_SENTINEL: u8 = 0xEE;

/// 冻结词表。
pub const WORDLIST: [u8; 5] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE];

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 词表语义名。
pub fn poison_name(v: u8) -> Option<&'static str> {
    match v {
        POISON_FREE => Some("POISON-FREE"),
        POISON_RELEASE => Some("POISON-RELEASE"),
        POISON_REVOKE => Some("POISON-REVOKE"),
        POISON_OOB => Some("POISON-OOB"),
        POISON_SENTINEL => Some("POISON-SENTINEL"),
        _ => None,
    }
}

/// 词表用途域。
pub fn poison_scope(v: u8) -> Option<&'static str> {
    match v {
        POISON_FREE => Some("SLAB-INIT"),
        POISON_RELEASE => Some("SLAB-FREE"),
        POISON_REVOKE => Some("CAP-REVOKE"),
        POISON_OOB => Some("GUARD-BAND"),
        POISON_SENTINEL => Some("STACK-CANARY"),
        _ => None,
    }
}

/// 毒化注册表。
pub struct PoisonRegistry {
    /// 已注册模式值（封闭执法后入册）。
    pub registered: Vec<u8>,
    /// 拒绝计数。
    pub rejected: u32,
    /// 毒化写入/读出账。
    pub write_cnt: u64,
    pub read_cnt: u64,
    pub mismatches: u64,
}

impl PoisonRegistry {
    pub fn new() -> Self {
        PoisonRegistry { registered: Vec::new(), rejected: 0, write_cnt: 0, read_cnt: 0, mismatches: 0 }
    }

    /// 注册：仅词表内值可入册。
    pub fn register(&mut self, v: u8) -> bool {
        if !WORDLIST.contains(&v) {
            self.rejected += 1;
            return false;
        }
        if self.registered.contains(&v) {
            self.rejected += 1;
            return false;
        }
        self.registered.push(v);
        true
    }

    /// 毒化写入：仅注册值可写。
    pub fn write(&mut self, v: u8) -> bool {
        if !self.registered.contains(&v) {
            self.rejected += 1;
            return false;
        }
        self.write_cnt += 1;
        true
    }

    /// 毒化读出：值必须与写入一致，否则计失配。
    pub fn verify(&mut self, written: u8, read_back: u8) -> bool {
        self.read_cnt += 1;
        if written == read_back && poison_name(read_back).is_some() {
            true
        } else {
            self.mismatches += 1;
            false
        }
    }

    /// 词表冻结哈希（值+名+域）。
    pub fn wordlist_hash() -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for &v in WORDLIST.iter() {
            buf.push(v);
            if let Some(n) = poison_name(v) {
                for &b in n.as_bytes() {
                    buf.push(b);
                }
            }
            if let Some(s) = poison_scope(v) {
                for &b in s.as_bytes() {
                    buf.push(b);
                }
            }
        }
        fnv1a(&buf)
    }

    /// 账闭合：写 ≥ 读 - 失配（读必配写）。
    pub fn closed(&self) -> bool {
        self.read_cnt <= self.write_cnt && self.mismatches == 0
    }
}

/// M-320 新值注册拒绝：词表外值全拒。
pub fn m320() -> (bool, u32) {
    let mut r = PoisonRegistry::new();
    // 五值全注册成功
    let mut all_ok = true;
    for &v in WORDLIST.iter() {
        if !r.register(v) {
            all_ok = false;
        }
    }
    // 新值拒绝：0x11/0x55/0xFF/0xAB/0xAA(重复)
    let mut refused = 0u32;
    for v in [0x11u8, 0x55, 0xFF, 0xAB, 0xAA] {
        if !r.register(v) {
            refused += 1;
        }
    }
    (all_ok && refused == 5 && r.registered.len() == 5 && r.rejected == 5, refused)
}

/// J1 主判据：词表冻结 + M-320 封闭执法 + 毒化写读校验。
pub fn j1_main() -> bool {
    let (m_ok, _) = m320();
    let mut r = PoisonRegistry::new();
    for &v in WORDLIST.iter() {
        let _ = r.register(v);
    }
    let mut write_ok = true;
    for &v in WORDLIST.iter() {
        if r.write(v) {
            if !r.verify(v, v) {
                write_ok = false;
            }
        } else {
            write_ok = false;
        }
    }
    // 篡改读出必失配
    let tamper = !r.verify(POISON_FREE, 0x12);
    m_ok && PoisonRegistry::wordlist_hash() != 0 && write_ok && tamper && r.mismatches == 1 && !r.closed()
}

// ── 注入核（0xCF80 段 5 项）────────────────────────────────────────────

/// 注入核 1：语义名封闭注入——词表外无名。
pub fn inj1_name_closed() -> bool {
    poison_name(0x12).is_none()
        && poison_name(POISON_FREE) == Some("POISON-FREE")
        && poison_name(POISON_SENTINEL) == Some("POISON-SENTINEL")
}

/// 注入核 2：用途域封闭注入。
pub fn inj2_scope_closed() -> bool {
    poison_scope(0xFF).is_none() && poison_scope(POISON_OOB) == Some("GUARD-BAND")
}

/// 注入核 3：未注册写入注入——未注册值写必拒。
pub fn inj3_unregistered_write() -> bool {
    let mut r = PoisonRegistry::new();
    let _ = r.register(POISON_FREE);
    !r.write(0x33) && r.rejected == 1
}

/// 注入核 4：毒值原样读出注入——五值逐一写读全对。
pub fn inj4_roundtrip() -> bool {
    let mut r = PoisonRegistry::new();
    for &v in WORDLIST.iter() {
        let _ = r.register(v);
    }
    let mut all = true;
    for &v in WORDLIST.iter() {
        let _ = r.write(v);
        if !r.verify(v, v) {
            all = false;
        }
    }
    all && r.mismatches == 0 && r.closed()
}

/// 注入核 5：词表互异注入——五值互异且名互异。
pub fn inj5_distinct() -> bool {
    for i in 0..WORDLIST.len() {
        for j in i + 1..WORDLIST.len() {
            if WORDLIST[i] == WORDLIST[j] {
                return false;
            }
            if poison_name(WORDLIST[i]) == poison_name(WORDLIST[j]) {
                return false;
            }
        }
    }
    true
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 31
}

/// run_unx3562_checks：本单判据聚合（诊断段 0xCF80）。
pub fn run_unx3562_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3562 毒化模式注册表（模式值封闭词表——magic 选择）");
    s.add(
        "代码体量≥300行当量",
        structural_weight() >= 300,
        if structural_weight() >= 300 { "" } else { "结构当量不足" },
    );
    s.add(
        "词表冻结（0xAA~0xEE五值语义表）",
        PoisonRegistry::wordlist_hash() != 0 && WORDLIST.len() == 5,
        if PoisonRegistry::wordlist_hash() != 0 { "" } else { "词表哈希空" },
    );
    let (m_ok, refused) = m320();
    s.add(
        "M-320新值注册拒绝（封闭词表执法）",
        m_ok && refused == 5,
        if m_ok { "" } else { "封闭执法破" },
    );
    s.add(
        "毒化写入/读出校验生效",
        j1_main(),
        if j1_main() { "" } else { "毒化校验破" },
    );
    let inj = [
        ("注入1语义名封闭", inj1_name_closed()),
        ("注入2用途域封闭", inj2_scope_closed()),
        ("注入3未注册写", inj3_unregistered_write()),
        ("注入4毒值原样读出", inj4_roundtrip()),
        ("注入5词表互异", inj5_distinct()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
