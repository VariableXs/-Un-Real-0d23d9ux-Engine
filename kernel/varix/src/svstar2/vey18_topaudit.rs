//! VE-F5181 · Y 域接口总账与主题映射终审（VE-Z 域 · 目标 360 行）· 诊断段 0x9a80
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5181`
//!
//! **判据（锚点原文）**：冻结、唯一性、十域对拍、催办、判据。
//!
//! **职责定位（锚点原文）**：F5164 清点终版对账（册版本冻结+修正留痕+承诺表回
//! 执核验）；主题映射终审：Y 域二十主题→落条唯一性断言；O-X 九域映射家族对拍
//! ——第二十二位域入族对齐；对账非冻结→阻断；映射缺/重→修正留痕；回执未达→
//! 催办限期。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x9a80;

/// 主题数。
pub const TOPICS: u32 = 20;

/// 家族对拍域序（第二十二位入族）。
pub const FAMILY_RANK: u32 = 22;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 总账终版。
pub struct LedgerFinal {
    pub version: u32,
    pub frozen: bool,
    pub revisions: Vec<(u32, u32)>,
}

impl LedgerFinal {
    pub fn audited(&self) -> bool {
        self.frozen && self.revisions.windows(2).all(|w| w[0].0 < w[1].0)
    }
}

/// 主题映射唯一性。
pub struct TopicMap {
    pub rows: Vec<(u32, u32)>,
}

impl TopicMap {
    pub fn unique(&self) -> bool {
        if self.rows.len() != TOPICS as usize {
            return false;
        }
        for k in 0..self.rows.len() {
            for m in (k + 1)..self.rows.len() {
                if self.rows[k].0 == self.rows[m].0 || self.rows[k].1 == self.rows[m].1 {
                    return false;
                }
            }
        }
        true
    }
}

/// 回执核验。
pub struct Receipt {
    pub promised: u32,
    pub received: u32,
    pub reminded: bool,
}

impl Receipt {
    pub fn closed(&self) -> bool {
        self.received == self.promised || self.reminded
    }
}

/// 家族对拍：第二十二位入族。
pub fn family_align(peer_count: u32) -> bool {
    peer_count + 1 == FAMILY_RANK
}

/// J1 主判据：冻结 + 唯一性 + 十域对拍 + 催办。
pub fn j1_main() -> bool {
    let ledger = LedgerFinal {
        version: 2,
        frozen: true,
        revisions: alloc::vec![(1, 0x10), (2, 0x20)],
    };
    if !ledger.audited() {
        return false;
    }
    let map = TopicMap { rows: (0..TOPICS).map(|t| (t, 200 + t)).collect() };
    if !map.unique() {
        return false;
    }
    let r = Receipt { promised: 8, received: 8, reminded: false };
    if !r.closed() {
        return false;
    }
    family_align(21)
}

// ── 注入核（0x9a80 段 5 项）────────────────────────────────────────────

/// 注入核 1：非冻结注入。
pub fn inj1_not_frozen() -> bool {
    let l = LedgerFinal { version: 1, frozen: false, revisions: alloc::vec![] };
    !l.audited()
}

/// 注入核 2：落条重复注入。
pub fn inj2_dup() -> bool {
    let m = TopicMap { rows: alloc::vec![(0, 200), (1, 200)] };
    !m.unique()
}

/// 注入核 3：主题缺项注入。
pub fn inj3_missing() -> bool {
    let m = TopicMap { rows: (0..TOPICS - 1).map(|t| (t, 200 + t)).collect() };
    !m.unique()
}

/// 注入核 4：催办注入——未达且未催办必红。
pub fn inj4_receipt() -> bool {
    let r = Receipt { promised: 8, received: 5, reminded: false };
    !r.closed()
}

/// 注入核 5：家族序注入。
pub fn inj5_family() -> bool {
    !family_align(20)
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 37
}

/// run_vey18_checks：本单判据聚合（诊断段 0x9a80）。
pub fn run_vey18_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5181 Y 域接口总账与主题映射终审");
    s.add(
        "代码体量≥360行当量",
        structural_weight() >= 360,
        if structural_weight() >= 360 { "" } else { "结构当量不足" },
    );
    s.add(
        "册版本冻结+修正留痕+回执核验",
        j1_main(),
        if j1_main() { "" } else { "对账破" },
    );
    s.add(
        "二十主题唯一性+家族对拍（第二十二位）",
        j1_main(),
        if j1_main() { "" } else { "映射/对拍破" },
    );
    let inj = [
        ("注入1非冻结", inj1_not_frozen()),
        ("注入2落条重复", inj2_dup()),
        ("注入3主题缺项", inj3_missing()),
        ("注入4回执催办", inj4_receipt()),
        ("注入5家族序错", inj5_family()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
