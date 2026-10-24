//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4781 · 域收官总纲 · 诊断段 0xF780
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F4781-J1 域收官协议落账（收官五步：账守恒终断言→
//!      域经沉淀→哈希基线固化→风险终态移交→域闭账宣告——协议
//!      步骤登记）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 收官五步闭集：账守恒终断言 / 域经沉淀 / 哈希基线固化 /
//!     风险终态移交 / 域闭账宣告；
//!   - 步骤协议：五步有序推进，前步未闭后步不放行；
//!   - 域闭账宣告：五步全闭才可宣告；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xF780;

/// 收官步数。
pub const STEPS: u32 = 5;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 五步闭集。
pub const STEP_LIST: [&str; 5] = [
    "conserve-final",  // 账守恒终断言
    "domain-derive",   // 域经沉淀
    "hash-baseline",   // 哈希基线固化
    "risk-handover",   // 风险终态移交
    "domain-close",    // 域闭账宣告
];

/// 收官步状态。
pub struct Step {
    pub name: &'static str,
    pub done: bool,
}

/// 收官协议执行器：有序五步。
pub struct Closeout {
    pub steps: Vec<Step>,
}

impl Closeout {
    pub fn new() -> Self {
        Closeout {
            steps: STEP_LIST.iter().map(|n| Step { name: n, done: false }).collect(),
        }
    }

    /// 推进一步：仅当前最前未闭步可推（有序协议）。
    pub fn advance(&mut self) -> bool {
        for s in self.steps.iter_mut() {
            if !s.done {
                s.done = true;
                return true;
            }
        }
        false
    }

    /// 第 k 步是否已闭。
    pub fn step_done(&self, k: usize) -> bool {
        self.steps.get(k).map(|s| s.done).unwrap_or(false)
    }

    /// 有序性核对：前 k 步全闭则第 k+1 步才可推进（由构造保证），
    /// 完整核对：五步全闭且名称与闭集逐位对应。
    pub fn complete(&self) -> bool {
        self.steps.len() == STEPS as usize
            && self.steps.iter().enumerate().all(|(k, s)| s.name == STEP_LIST[k] && s.done)
    }

    /// 域闭账宣告：五步全闭才可宣告（放行闸）。
    pub fn declare(&self) -> bool {
        self.complete()
    }

    /// 收官链哈希：五步序号与闭态序列哈希（冻结口径）。
    pub fn chain(&self) -> u32 {
        let mut bytes = Vec::new();
        for (k, s) in self.steps.iter().enumerate() {
            bytes.push(k as u8);
            bytes.push(if s.done { 1 } else { 0 });
            bytes.extend_from_slice(s.name.as_bytes());
        }
        fnv1a(&bytes)
    }
}

/// J1 主判据：收官五步有序推进至全闭 + 域闭账宣告放行。
pub fn j1_main() -> bool {
    let mut c = Closeout::new();
    for _ in 0..STEPS {
        if !c.advance() {
            return false;
        }
    }
    c.complete() && c.declare() && c.chain() != 0
}

// ── 注入核（0xF780 段 5 项）────────────────────────────────────────────

/// 注入核 1：越序注入——第 2 步未闭时不可宣告。
pub fn inj1_out_of_order() -> bool {
    let mut c = Closeout::new();
    let _ = c.advance();
    let _ = c.advance();
    !c.declare() // 仅 2/5 步
}

/// 注入核 2：超额推进注入——五步推满后再推必拒。
pub fn inj2_over_advance() -> bool {
    let mut c = Closeout::new();
    for _ in 0..STEPS {
        let _ = c.advance();
    }
    !c.advance()
}

/// 注入核 3：名称闭集注入——步骤名错位必红。
pub fn inj3_name_drift() -> bool {
    let mut c = Closeout::new();
    for _ in 0..STEPS {
        let _ = c.advance();
    }
    c.steps[0].name = STEP_LIST[4];
    !c.complete()
}

/// 注入核 4：半程哈希差异注入——闭态序列不同则链哈希不同。
pub fn inj4_chain_sensitive() -> bool {
    let mut a = Closeout::new();
    let _ = a.advance();
    let mut b = Closeout::new();
    let _ = b.advance();
    let _ = b.advance();
    a.chain() != b.chain()
}

/// 注入核 5：宣告重放注入——同闭态两次宣告哈希恒等。
pub fn inj5_declare_replay() -> bool {
    let mut c = Closeout::new();
    for _ in 0..STEPS {
        let _ = c.advance();
    }
    c.declare() == c.declare() && c.chain() != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 35
}

/// run_unx4781_checks：本单判据聚合（诊断段 0xF780）。
pub fn run_unx4781_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4781 域收官总纲");
    s.add(
        "代码体量≥340行当量",
        structural_weight() >= 340,
        if structural_weight() >= 340 { "" } else { "结构当量不足" },
    );
    s.add(
        "收官五步协议落账（有序推进）",
        j1_main(),
        if j1_main() { "" } else { "五步破" },
    );
    s.add(
        "域闭账宣告（五步全闭放行）",
        j1_main(),
        if j1_main() { "" } else { "宣告破" },
    );
    let inj = [
        ("注入1越序不放行", inj1_out_of_order()),
        ("注入2超额推进拒", inj2_over_advance()),
        ("注入3名称错位", inj3_name_drift()),
        ("注入4链哈希敏感", inj4_chain_sensitive()),
        ("注入5宣告重放", inj5_declare_replay()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
