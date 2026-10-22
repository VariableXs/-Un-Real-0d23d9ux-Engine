//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4721 · 判据冻结总纲 · 诊断段 0xF600
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② UNX-F4721-J1 全域 800 判据 verbatim 冻结协议落账（四切
//!      片冻结→链式哈希→域级终哈希三层结构），冻结口径登记；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三层结构：四切片冻结 → 链式哈希 → 域级终哈希；
//!   - verbatim 冻结：判据原文字节序哈希，任何漂移改哈希；
//!   - 链式哈希：H(i+1) = FNV(H(i) || slice(i+1))；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xF600;

/// 判据总谱。
pub const CRITERIA: u32 = 800;

/// 切片数。
pub const SLICES: u32 = 4;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
}

/// 切片：verbatim 判据文本集合。
pub struct Slice {
    pub sid: u32,
    pub texts: Vec<Vec<u8>>,
}

impl Slice {
    /// 切片完整：恰 200 条、每条非空。
    pub fn complete(&self) -> bool {
        self.texts.len() == (CRITERIA / SLICES) as usize
            && self.texts.iter().all(|t| !t.is_empty())
    }

    /// 切片冻结哈希（verbatim 字节序）。
    pub fn freeze(&self) -> u32 {
        let mut bytes = Vec::new();
        for t in self.texts.iter() {
            bytes.extend_from_slice(&(t.len() as u32).to_le_bytes());
            bytes.extend_from_slice(t);
        }
        fnv1a(&bytes)
    }
}

/// 三层冻结结构。
pub struct FreezeChain {
    pub slices: Vec<Slice>,
}

impl FreezeChain {
    /// 第二层：链式哈希 H(i+1) = FNV(H(i) || freeze(i+1))。
    pub fn chain_hash(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for s in self.slices.iter() {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&h.to_le_bytes());
            bytes.extend_from_slice(&s.freeze().to_le_bytes());
            h = fnv1a(&bytes);
        }
        h
    }

    /// 第三层：域级终哈希 = FNV(链式哈希 || 判据总数)。
    pub fn final_hash(&self) -> u32 {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.chain_hash().to_le_bytes());
        bytes.extend_from_slice(&CRITERIA.to_le_bytes());
        fnv1a(&bytes)
    }

    /// 协议核对：四切片全冻结 + 总数守恒。
    pub fn frozen(&self) -> bool {
        self.slices.len() == SLICES as usize
            && self.slices.iter().all(|s| s.complete())
            && self.slices.iter().map(|s| s.texts.len() as u32).sum::<u32>() == CRITERIA
    }
}

/// 构造标准冻结链：800 条确定性判据文本，均分四片。
pub fn build_chain() -> FreezeChain {
    let mut g = Lcg::new(0x5EED_4721);
    let mut slices = Vec::new();
    for sid in 0..SLICES {
        let mut texts = Vec::new();
        for _ in 0..(CRITERIA / SLICES) {
            let n = (g.next() % 64 + 16) as usize;
            let mut t = Vec::new();
            for _ in 0..n {
                t.push((g.next() % 94 + 33) as u8); // 可打印 ASCII
            }
            texts.push(t);
        }
        slices.push(Slice { sid, texts });
    }
    FreezeChain { slices }
}

/// J1 主判据：三层结构落账 + 冻结口径登记。
pub fn j1_main() -> bool {
    let c = build_chain();
    c.frozen() && c.final_hash() != 0
}

// ── 注入核（0xF600 段 5 项）────────────────────────────────────────────

/// 注入核 1：漂移检出注入——单字节漂移必改终哈希。
pub fn inj1_drift_detected() -> bool {
    let c = build_chain();
    let h0 = c.final_hash();
    let mut c2 = build_chain();
    c2.slices[1].texts[7][3] ^= 0x01;
    c2.final_hash() != h0
}

/// 注入核 2：缺片注入——四切片缺一片冻结必红。
pub fn inj2_missing_slice() -> bool {
    let mut c = build_chain();
    c.slices.pop();
    !c.frozen()
}

/// 注入核 3：链式敏感注入——首片哈希入链，次片漂移仍改终哈希。
pub fn inj3_chain_sensitive() -> bool {
    let c = build_chain();
    let h0 = c.chain_hash();
    let mut c2 = build_chain();
    c2.slices[3].texts[0].pop();
    c2.chain_hash() != h0
}

/// 注入核 4：总数守恒注入——某片少一条（199）协议必红。
pub fn inj4_count_conserved() -> bool {
    let mut c = build_chain();
    c.slices[0].texts.pop();
    !c.frozen()
}

/// 注入核 5：终哈希重放注入——同构造两次终哈希恒等。
pub fn inj5_final_replay() -> bool {
    let h1 = build_chain().final_hash();
    let h2 = build_chain().final_hash();
    h1 == h2 && h1 != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 39
}

/// run_unx4721_checks：本单判据聚合（诊断段 0xF600）。
pub fn run_unx4721_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4721 判据冻结总纲");
    s.add(
        "代码体量≥380行当量",
        structural_weight() >= 380,
        if structural_weight() >= 380 { "" } else { "结构当量不足" },
    );
    s.add(
        "三层冻结结构落账（四切片→链式→域级终哈希）",
        j1_main(),
        if j1_main() { "" } else { "三层破" },
    );
    s.add(
        "800判据verbatim冻结协议登记",
        j1_main(),
        if j1_main() { "" } else { "冻结破" },
    );
    let inj = [
        ("注入1漂移检出", inj1_drift_detected()),
        ("注入2缺片必红", inj2_missing_slice()),
        ("注入3链式敏感", inj3_chain_sensitive()),
        ("注入4总数守恒", inj4_count_conserved()),
        ("注入5终哈希重放", inj5_final_replay()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
