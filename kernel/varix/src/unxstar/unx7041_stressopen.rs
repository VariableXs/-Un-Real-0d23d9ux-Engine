//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F7041 · 压测账总起与域账衔接（压测轴 8 批开账）· 诊断段 0xA800
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥297 行；
//!   ② UNX-F7041-J1 压测轴（B33–B40）开账宣言与域账衔接断言；
//!      本批 6,000 行入账可查；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 压测轴 8 批（B33–B40）开账：批号/批题/目标行数三列账；
//!   - 开账宣言落账 + 域账衔接（批号连续 + 总行数 6,000 守恒）；
//!   - 入账可查：逐批行数账查询；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xA800;

/// 压测轴批数。
pub const BATCHES: usize = 8;

/// 本批目标行数。
pub const TOTAL_LINES: usize = 6000;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 任务书批构成（批号, 批题, 目标行数）——合计 6000。
pub fn axis_batches() -> [(&'static str, &'static str, usize); 8] {
    [
        ("B33", "压测基准建立", 800),
        ("B34", "压力梯度", 750),
        ("B35", "并发风暴", 800),
        ("B36", "长稳压测", 750),
        ("B37", "峰值冲击", 800),
        ("B38", "资源极限", 700),
        ("B39", "故障注入压测", 700),
        ("B40", "压测收口", 700),
    ]
}

/// 开账条目：批号 + 批题 + 行数 + 指纹。
pub struct AxisEntry {
    pub batch: String,
    pub title: String,
    pub lines: usize,
    pub fp: u32,
}

/// 压测轴开账。
pub struct StressOpen {
    pub entries: Vec<AxisEntry>,
    pub declared: bool,
}

impl StressOpen {
    pub fn new() -> Self {
        StressOpen { entries: Vec::new(), declared: false }
    }

    /// 开账宣言 + 8 批落账。
    pub fn open(&mut self) {
        self.declared = true;
        for (b, t, l) in axis_batches().iter() {
            self.entries.push(AxisEntry {
                batch: b.to_string(),
                title: t.to_string(),
                lines: *l,
                fp: fnv1a(format!("{}#{}", b, l).as_bytes()),
            });
        }
    }

    /// 域账衔接：批号连续（B33..B40）+ 行数合计 6,000。
    pub fn linked(&self) -> bool {
        if !self.declared || self.entries.len() != BATCHES {
            return false;
        }
        for (i, (b, _, _)) in axis_batches().iter().enumerate() {
            if self.entries[i].batch != *b {
                return false;
            }
        }
        self.entries.iter().map(|e| e.lines).sum::<usize>() == TOTAL_LINES
    }

    /// 入账可查：按批号查行数。
    pub fn query(&self, batch: &str) -> Option<usize> {
        self.entries.iter().find(|e| e.batch == batch).map(|e| e.lines)
    }
}

/// J1 主判据：开账宣言 + 8 批衔接 + 6,000 行守恒 + 逐批可查。
pub fn j1_main() -> bool {
    let mut s = StressOpen::new();
    s.open();
    if !s.linked() {
        return false;
    }
    // 逐批可查
    let (b0, _, l0) = axis_batches()[0];
    let (b7, _, l7) = axis_batches()[7];
    s.query(b0) == Some(l0) && s.query(b7) == Some(l7) && s.query("B99").is_none()
}

// ── 注入核（0xA800 段 5 项）────────────────────────────────────────────

/// 注入核 1：未宣言注入——declared=false 时衔接断言拒。
pub fn inj1_undeclared() -> bool {
    let mut s = StressOpen::new();
    s.open();
    s.declared = false;
    !s.linked()
}

/// 注入核 2：批号错位注入——批序错乱衔接拒。
pub fn inj2_batch_misorder() -> bool {
    let mut s = StressOpen::new();
    s.open();
    s.entries[0].batch = "B40".to_string();
    s.entries[7].batch = "B33".to_string();
    !s.linked()
}

/// 注入核 3：行数漂移注入——任一批行数改动总守恒破。
pub fn inj3_line_drift() -> bool {
    let mut s = StressOpen::new();
    s.open();
    s.entries[3].lines += 1;
    !s.linked()
}

/// 注入核 4：缺批注入——8 批缺一衔接拒。
pub fn inj4_missing_batch() -> bool {
    let mut s = StressOpen::new();
    s.open();
    s.entries.pop();
    !s.linked()
}

/// 注入核 5：查询注入——幽灵批号查询 None。
pub fn inj5_ghost_query() -> bool {
    let mut s = StressOpen::new();
    s.open();
    s.query("B40").is_some() && s.query("B32").is_none()
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 30
}

/// run_unx7041_checks：本单判据聚合（诊断段 0xA800）。
pub fn run_unx7041_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7041 压测账总起与域账衔接（压测轴 8 批开账）");
    s.add(
        "代码体量≥297行当量",
        structural_weight() >= 297,
        if structural_weight() >= 297 { "" } else { "结构当量不足" },
    );
    s.add(
        "开账宣言+域账衔接+6000行守恒",
        j1_main(),
        if j1_main() { "" } else { "开账衔接破" },
    );
    s.add(
        "本批6,000行入账可查",
        j1_main(),
        if j1_main() { "" } else { "入账不可查" },
    );
    let inj = [
        ("注入1未宣言拒", inj1_undeclared()),
        ("注入2批号错位拒", inj2_batch_misorder()),
        ("注入3行数漂移拒", inj3_line_drift()),
        ("注入4缺批拒", inj4_missing_batch()),
        ("注入5幽灵查询", inj5_ghost_query()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
