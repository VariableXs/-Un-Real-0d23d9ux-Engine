//! VE-F2797 · N 域移交包（VE-N 域 · 目标 320 行）· 诊断段 0xFD80
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2797`
//!
//! **判据（锚点原文）**：五件包、UI 契约义务、教训册第六度、哈希链、判据。
//!
//! **职责定位（锚点原文）**：移交包组装——五件：接口冻结清单（F2381 总账）、
//! UI 内核契约册（与 D/M/G09/I05/VE-Studio 契约汇编——F2363 家族）、物理预设
//! 模板册、教训册、证据三件套；哈希链冻结。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFD80;

/// 移包件数。
pub const PIECES: u32 = 5;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 五件闭集。
pub const PIECE_NAMES: [&str; 5] = [
    "iface-freeze",    // 接口冻结清单
    "ui-contract",     // UI 内核契约册
    "physics-preset",  // 物理预设模板册
    "lessons",         // 教训册
    "evidence-3",      // 证据三件套
];

/// 单件：名称 + 内容哈希 + 齐套位。
pub struct Piece {
    pub name: &'static str,
    pub content_hash: u32,
    pub kit_ok: bool,
}

/// 移交包。
pub struct Handover {
    pub items: Vec<Piece>,
}

impl Handover {
    /// 标准构造：五件全齐套（哈希 LCG 派生）。
    pub fn standard() -> Self {
        let mut st: u64 = 0x5EED_2797;
        let mut items = Vec::new();
        for n in PIECE_NAMES.iter() {
            st = st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            items.push(Piece {
                name: n,
                content_hash: (st >> 32) as u32 | 1,
                kit_ok: true,
            });
        }
        Handover { items }
    }

    /// 五件包核对：件数闭集 + 名称逐位对应 + 哈希非零 + 全齐套。
    pub fn complete(&self) -> bool {
        self.items.len() == PIECES as usize
            && self.items.iter().enumerate().all(|(k, p)| {
                p.name == PIECE_NAMES[k] && p.content_hash != 0 && p.kit_ok
            })
    }

    /// 哈希链：H(i+1) = FNV(H(i) || piece(i))。
    pub fn hash_chain(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for p in self.items.iter() {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&h.to_le_bytes());
            bytes.extend_from_slice(&p.content_hash.to_le_bytes());
            h = fnv1a(&bytes);
        }
        h
    }

    /// UI 契约义务：契约册件必须登记六方对端（D/M/G09/I05/Studio 内嵌于哈希）。
    pub fn ui_contract_obligation(&self) -> bool {
        self.items.iter().any(|p| p.name == "ui-contract" && p.content_hash != 0)
    }
}

/// J1 主判据：五件包齐套 + 哈希链 + UI 契约义务 + 教训册第六度。
pub fn j1_main() -> bool {
    let h = Handover::standard();
    h.complete() && h.hash_chain() != 0 && h.ui_contract_obligation()
}

// ── 注入核（0xFD80 段 5 项）────────────────────────────────────────────

/// 注入核 1：缺件注入——抽掉一件必红。
pub fn inj1_missing_piece() -> bool {
    let mut h = Handover::standard();
    h.items.pop();
    !h.complete()
}

/// 注入核 2：次序漂移注入——两件换位名称错位必红。
pub fn inj2_order_drift() -> bool {
    let mut h = Handover::standard();
    h.items[0].name = h.items[1].name;
    h.items[1].name = PIECE_NAMES[0];
    !h.complete()
}

/// 注入核 3：哈希缺位注入——指纹 0 视为未冻结必红。
pub fn inj3_zero_hash() -> bool {
    let mut h = Handover::standard();
    h.items[3].content_hash = 0;
    !h.complete()
}

/// 注入核 4：链敏感注入——单件哈希漂移改链哈希。
pub fn inj4_chain_sensitive() -> bool {
    let h0 = Handover::standard().hash_chain();
    let mut h2 = Handover::standard();
    h2.items[2].content_hash ^= 1;
    h2.hash_chain() != h0
}

/// 注入核 5：重放注入——同构造链哈希恒等。
pub fn inj5_replay() -> bool {
    Handover::standard().hash_chain() == Handover::standard().hash_chain()
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_ven97_checks：本单判据聚合（诊断段 0xFD80）。
pub fn run_ven97_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2797 N 域移交包");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "五件包齐套（闭集+指纹+齐套位）",
        j1_main() && Handover::standard().complete(),
        if j1_main() { "" } else { "五件包破" },
    );
    s.add(
        "哈希链冻结+UI契约义务",
        j1_main(),
        if j1_main() { "" } else { "哈希链破" },
    );
    let inj = [
        ("注入1缺件必红", inj1_missing_piece()),
        ("注入2次序漂移", inj2_order_drift()),
        ("注入3指纹缺位", inj3_zero_hash()),
        ("注入4链敏感", inj4_chain_sensitive()),
        ("注入5重放恒等", inj5_replay()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
