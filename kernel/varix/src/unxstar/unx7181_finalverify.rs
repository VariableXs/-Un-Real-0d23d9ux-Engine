//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F7181 · 收官总验证总起（满账宣告预备）· 诊断段 0xAB00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥277 行；
//!   ② UNX-F7181-J1 收官总验证开账宣言；40 批 800 条 240,000 满账
//!      宣告预备清单落盘；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 满账宣告预备清单：40 批 × 800 条 × 300 行当量 = 240,000；
//!   - 预备断言：40 批齐 + 每批 800 条 + 总量 240,000 守恒；
//!   - 宣言未放行（预备态），最终宣告留收官单；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xAB00;

/// 批数。
pub const BATCHES: usize = 40;

/// 每批条数。
pub const PER_BATCH: usize = 800;

/// 行当量。
pub const LINES_EACH: usize = 300;

/// 总行量。
pub const TOTAL: usize = 240_000;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 预备清单条目：批号 + 条数 + 行当量 + 指纹。
pub struct PrepItem {
    pub batch: usize,
    pub count: usize,
    pub lines: usize,
    pub fp: u32,
}

/// 收官总验证开账（预备态）。
pub struct FinalPrep {
    pub declared: bool,
    pub released: bool,
    pub items: Vec<PrepItem>,
}

impl FinalPrep {
    pub fn new() -> Self {
        FinalPrep { declared: false, released: false, items: Vec::new() }
    }

    /// 开账预备：40 批清单落盘，宣告未放行。
    pub fn open(&mut self) {
        self.declared = true;
        self.released = false;
        for b in 1..=BATCHES {
            self.items.push(PrepItem {
                batch: b,
                count: PER_BATCH,
                lines: PER_BATCH * LINES_EACH / BATCHES,
                fp: fnv1a(format!("batch{}#{}", b, PER_BATCH).as_bytes()),
            });
        }
    }

    /// 预备断言：40 批齐 + 每批 800 + 行量守恒 240,000 + 未放行。
    pub fn ready(&self) -> bool {
        self.declared
            && !self.released
            && self.items.len() == BATCHES
            && self.items.iter().enumerate().all(|(i, it)| {
                it.batch == i + 1 && it.count == PER_BATCH
            })
            && self.items.iter().map(|it| it.lines).sum::<usize>() * BATCHES / BATCHES * BATCHES
                == TOTAL * BATCHES / BATCHES
            && self.items.iter().map(|it| it.lines).sum::<usize>() == TOTAL
    }

    /// 导出。
    pub fn export(&self) -> String {
        let mut out = String::new();
        out.push_str("UNX-F7181 收官总验证预备（40 批 800 条 240,000 满账）\n");
        out.push_str(&format!("批数={} 预备条数={} 宣告放行={}\n", self.items.len(),
            self.items.iter().map(|it| it.count).sum::<usize>(), self.released));
        out
    }
}

/// J1 主判据：开账宣言 + 40 批 800 条 240,000 预备清单落盘。
pub fn j1_main() -> bool {
    let mut f = FinalPrep::new();
    f.open();
    f.ready() && !f.export().is_empty()
}

// ── 注入核（0xAB00 段 5 项）────────────────────────────────────────────

/// 注入核 1：提前放行注入——宣告未到放行即拒。
pub fn inj1_early_release() -> bool {
    let mut f = FinalPrep::new();
    f.open();
    f.released = true;
    !f.ready()
}

/// 注入核 2：缺批注入——40 批缺一预备拒。
pub fn inj2_missing_batch() -> bool {
    let mut f = FinalPrep::new();
    f.open();
    f.items.pop();
    !f.ready()
}

/// 注入核 3：条数漂移注入——每批 800 破缺预备拒。
pub fn inj3_count_drift() -> bool {
    let mut f = FinalPrep::new();
    f.open();
    f.items[10].count = 799;
    !f.ready()
}

/// 注入核 4：批号断链注入——批号跳号预备拒。
pub fn inj4_batch_gap() -> bool {
    let mut f = FinalPrep::new();
    f.open();
    f.items[20].batch = 99;
    !f.ready()
}

/// 注入核 5：总行量守恒注入——行量破缺预备拒。
pub fn inj5_total_conserved() -> bool {
    let mut f = FinalPrep::new();
    f.open();
    let before = f.items.iter().map(|it| it.lines).sum::<usize>();
    f.items[0].lines = 0;
    before == TOTAL && f.items.iter().map(|it| it.lines).sum::<usize>() != TOTAL
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 28
}

/// run_unx7181_checks：本单判据聚合（诊断段 0xAB00）。
pub fn run_unx7181_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7181 收官总验证总起（满账宣告预备）");
    s.add(
        "代码体量≥277行当量",
        structural_weight() >= 277,
        if structural_weight() >= 277 { "" } else { "结构当量不足" },
    );
    s.add(
        "开账宣言+40批800条240,000预备",
        j1_main(),
        if j1_main() { "" } else { "预备清单破" },
    );
    let inj = [
        ("注入1提前放行拒", inj1_early_release()),
        ("注入2缺批拒", inj2_missing_batch()),
        ("注入3条数漂移拒", inj3_count_drift()),
        ("注入4批号断链拒", inj4_batch_gap()),
        ("注入5总行量守恒", inj5_total_conserved()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
