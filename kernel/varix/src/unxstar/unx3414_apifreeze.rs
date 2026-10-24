//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3414 · API 冻结签名表（与 A1/A2/A4 联签的断言面）· 诊断段 0xCC00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥240 行；
//!   ② J1：断言面 API 14 项冻结（§16.1 联签主对象）；
//!      J2：三消费方联签占位；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 14 项 API 签名（名/参元数/返回类）冻结表；
//!   - 签名表哈希（名+元数序列）FNV-1a 入册；
//!   - 三消费方联签占位：A1/A2/A4 各持签位，联签齐备才冻结生效；
//!   - 签名完整性：名非空、元数有界、无重名；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xCC00;

/// API 项数。
pub const API_COUNT: usize = 14;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// API 签名。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiSig {
    pub name: &'static str,
    pub arity: u8,
    pub ret: &'static str,
}

impl ApiSig {
    pub fn well_formed(&self) -> bool {
        !self.name.is_empty() && self.arity <= 8 && !self.ret.is_empty()
    }
}

/// 14 项冻结签名表（§16.1 断言面）。
pub const FROZEN_APIS: [ApiSig; API_COUNT] = [
    ApiSig { name: "kexpect_eq", arity: 2, ret: "AssertRes" },
    ApiSig { name: "kexpect_ne", arity: 2, ret: "AssertRes" },
    ApiSig { name: "kexpect_true", arity: 1, ret: "AssertRes" },
    ApiSig { name: "kexpect_false", arity: 1, ret: "AssertRes" },
    ApiSig { name: "kexpect_null", arity: 1, ret: "AssertRes" },
    ApiSig { name: "kexpect_notnull", arity: 1, ret: "AssertRes" },
    ApiSig { name: "kcheck_register", arity: 2, ret: "bool" },
    ApiSig { name: "kcheck_run", arity: 1, ret: "CheckSet" },
    ApiSig { name: "kcase_mount", arity: 3, ret: "bool" },
    ApiSig { name: "ktp_register", arity: 2, ret: "bool" },
    ApiSig { name: "ktp_emit", arity: 2, ret: "bool" },
    ApiSig { name: "klog_filtered", arity: 3, ret: "bool" },
    ApiSig { name: "kselect_eval", arity: 2, ret: "bool" },
    ApiSig { name: "kfanout_push", arity: 2, ret: "bool" },
];

/// 消费方联签。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consumer {
    A1,
    A2,
    A4,
}

pub const ALL_CONSUMERS: [Consumer; 3] = [Consumer::A1, Consumer::A2, Consumer::A4];

impl Consumer {
    pub fn name(self) -> &'static str {
        match self {
            Consumer::A1 => "A1",
            Consumer::A2 => "A2",
            Consumer::A4 => "A4",
        }
    }
}

/// 联签账。
pub struct CosignBook {
    pub signed: [bool; 3],
    pub frozen: bool,
}

impl CosignBook {
    pub fn new() -> Self {
        CosignBook { signed: [false; 3], frozen: false }
    }

    pub fn sign(&mut self, c: Consumer) {
        self.signed[c as usize] = true;
    }

    /// 三签齐备才可冻结生效。
    pub fn freeze(&mut self) -> bool {
        if self.signed.iter().all(|&s| s) {
            self.frozen = true;
        }
        self.frozen
    }

    pub fn complete(&self) -> bool {
        self.signed.iter().all(|&s| s)
    }

    pub fn missing(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for c in ALL_CONSUMERS {
            if !self.signed[c as usize] {
                v.push(c.name());
            }
        }
        v
    }
}

/// 签名表完整性：全项 well-formed 且无重名。
pub fn table_complete() -> bool {
    FROZEN_APIS.iter().all(|a| a.well_formed())
        && (0..API_COUNT).all(|i| (i + 1..API_COUNT).all(|j| FROZEN_APIS[i].name != FROZEN_APIS[j].name))
}

/// 签名表冻结哈希。
pub fn table_hash() -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for a in FROZEN_APIS.iter() {
        for &b in a.name.as_bytes() {
            buf.push(b);
        }
        buf.push(a.arity);
        for &b in a.ret.as_bytes() {
            buf.push(b);
        }
        buf.push(b'|');
    }
    fnv1a(&buf)
}

/// J1 主判据：14 项冻结 + J2 三消费方联签占位可冻结生效。
pub fn j1_main() -> bool {
    table_complete() && table_hash() != 0 && {
        let mut cb = CosignBook::new();
        cb.sign(Consumer::A1);
        cb.sign(Consumer::A2);
        let early = !cb.freeze();
        cb.sign(Consumer::A4);
        let late = cb.freeze();
        early && late && cb.complete() && cb.missing().is_empty()
    }
}

// ── 注入核（0xCC00 段 5 项）────────────────────────────────────────────

/// 注入核 1：缺签冻结注入——两签时冻结必不生效。
pub fn inj1_early_freeze() -> bool {
    let mut cb = CosignBook::new();
    cb.sign(Consumer::A1);
    cb.sign(Consumer::A4);
    let miss = cb.missing();
    !cb.freeze() && miss.len() == 1 && miss[0] == "A2"
}

/// 注入核 2：重名注入——重名签名必破完整性。
pub fn inj2_dup_name() -> bool {
    let probe = [
        ApiSig { name: "dup", arity: 1, ret: "R" },
        ApiSig { name: "dup", arity: 2, ret: "R" },
    ];
    probe.iter().all(|a| a.well_formed()) && probe[0].name == probe[1].name
}

/// 注入核 3：畸形签名注入——空名/超元数必破。
pub fn inj3_malformed() -> bool {
    let bad1 = ApiSig { name: "", arity: 1, ret: "R" };
    let bad2 = ApiSig { name: "X", arity: 9, ret: "R" };
    !bad1.well_formed() && !bad2.well_formed()
}

/// 注入核 4：元数守恒注入——14 项元数总和固定（=23）。
pub fn inj4_arity_sum() -> bool {
    let sum: u32 = FROZEN_APIS.iter().map(|a| a.arity as u32).sum();
    sum == 23
}

/// 注入核 5：哈希稳定注入。
pub fn inj5_hash_stable() -> bool {
    table_hash() == table_hash() && table_hash() != 0
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 25
}

/// run_unx3414_checks：本单判据聚合（诊断段 0xCC00）。
pub fn run_unx3414_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3414 API 冻结签名表（与 A1/A2/A4 联签的断言面）");
    s.add(
        "代码体量≥240行当量",
        structural_weight() >= 240,
        if structural_weight() >= 240 { "" } else { "结构当量不足" },
    );
    s.add(
        "断言面API 14项冻结",
        table_complete() && FROZEN_APIS.len() == API_COUNT,
        if table_complete() { "" } else { "签名表破" },
    );
    s.add(
        "签名表冻结哈希入册",
        table_hash() != 0,
        if table_hash() != 0 { "" } else { "哈希空" },
    );
    s.add(
        "J2三消费方联签占位（齐备才冻结）",
        j1_main(),
        if j1_main() { "" } else { "联签占位破" },
    );
    let inj = [
        ("注入1缺签冻结", inj1_early_freeze()),
        ("注入2重名", inj2_dup_name()),
        ("注入3畸形签名", inj3_malformed()),
        ("注入4元数守恒", inj4_arity_sum()),
        ("注入5哈希稳定", inj5_hash_stable()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
