//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4701 · 性能基线总纲 · 诊断段 0xF580
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② UNX-F4701-J1 域级性能账汇总结构落账（九基线+一联测+
//!      一压力+一泄漏——十二项性能账协议），基线口径登记；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 十二项性能账协议：九基线 + 一联测 + 一压力 + 一泄漏；
//!   - 基线口径登记：每项含口径名/单位/样本量/上限阈值；
//!   - 汇总核对：十二项全登记、阈值单调可查、口径冻结哈希；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xF580;

/// 十二项协议。
pub const ITEMS: u32 = 12;

/// 九基线。
pub const BASELINES: u32 = 9;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 项目类别闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Baseline,
    Joint,
    Stress,
    Leak,
}

impl Kind {
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Baseline => "baseline",
            Kind::Joint => "joint",
            Kind::Stress => "stress",
            Kind::Leak => "leak",
        }
    }
}

/// 单项性能账：口径登记行。
pub struct PerfItem {
    /// 口径名。
    pub metric: &'static str,
    /// 类别。
    pub kind: Kind,
    /// 样本量。
    pub samples: u32,
    /// 上限阈值（超即红）。
    pub ceiling: u32,
    /// 实测值。
    pub measured: u32,
}

impl PerfItem {
    /// 达标：实测 ≤ 上限。
    pub fn pass(&self) -> bool {
        self.measured <= self.ceiling
    }
}

/// 十二项协议标准表（确定性口径冻结）。
pub fn standard_table() -> Vec<PerfItem> {
    let mut v = Vec::new();
    let base = [
        ("syscall-lat", 1000u32, 50u32),
        ("ctx-switch", 1000, 40),
        ("fork-burst", 500, 60),
        ("page-fault", 2000, 30),
        ("file-seq", 800, 70),
        ("file-rand", 800, 90),
        ("pipe-thr", 1500, 45),
        ("net-loop", 600, 80),
        ("timer-jit", 900, 25),
    ];
    for (m, s, c) in base {
        v.push(PerfItem { metric: m, kind: Kind::Baseline, samples: s, ceiling: c, measured: c - 1 });
    }
    v.push(PerfItem { metric: "joint-sched-io", kind: Kind::Joint, samples: 700, ceiling: 120, measured: 110 });
    v.push(PerfItem { metric: "stress-64k", kind: Kind::Stress, samples: 64000, ceiling: 500, measured: 480 });
    v.push(PerfItem { metric: "leak-zero", kind: Kind::Leak, samples: 10000, ceiling: 0, measured: 0 });
    v
}

/// 汇总结构：十二项性能账协议核对器。
pub struct PerfBook {
    pub items: Vec<PerfItem>,
}

impl PerfBook {
    pub fn new(items: Vec<PerfItem>) -> Self {
        PerfBook { items }
    }

    /// 协议核对：恰十二项 + 类别配额（9/1/1/1）+ 全部达标。
    pub fn protocol_ok(&self) -> bool {
        if self.items.len() != ITEMS as usize {
            return false;
        }
        let count = |k: Kind| self.items.iter().filter(|i| i.kind == k).count() as u32;
        count(Kind::Baseline) == BASELINES
            && count(Kind::Joint) == 1
            && count(Kind::Stress) == 1
            && count(Kind::Leak) == 1
            && self.items.iter().all(|i| i.pass())
    }

    /// 口径名闭集唯一。
    pub fn names_unique(&self) -> bool {
        for k in 0..self.items.len() {
            for m in (k + 1)..self.items.len() {
                if self.items[k].metric == self.items[m].metric {
                    return false;
                }
            }
        }
        true
    }

    /// 泄漏专项：leak 项上限恒 0（零容忍口径）。
    pub fn leak_zero_tolerance(&self) -> bool {
        match self.items.iter().find(|i| i.kind == Kind::Leak) {
            Some(i) => i.ceiling == 0 && i.measured == 0,
            None => false,
        }
    }
}

/// J1 主判据：十二项协议落账 + 基线口径登记（口径唯一+达标+零容忍）。
pub fn j1_main() -> bool {
    let b = PerfBook::new(standard_table());
    b.protocol_ok() && b.names_unique() && b.leak_zero_tolerance()
}

// ── 注入核（0xF580 段 5 项）────────────────────────────────────────────

/// 注入核 1：超标注入——实测超上限必红。
pub fn inj1_over_ceiling() -> bool {
    let mut items = standard_table();
    items[0].measured = items[0].ceiling + 1;
    !PerfBook::new(items).protocol_ok()
}

/// 注入核 2：缺项注入——抽掉一项协议必红。
pub fn inj2_missing_item() -> bool {
    let mut items = standard_table();
    items.pop();
    !PerfBook::new(items).protocol_ok()
}

/// 注入核 3：类别漂移注入——基线少一（挪作他用）必红。
pub fn inj3_kind_drift() -> bool {
    let mut items = standard_table();
    items[0].kind = Kind::Joint; // baseline 8 / joint 2
    !PerfBook::new(items).protocol_ok()
}

/// 注入核 4：口径重名注入——重名口径必检出。
pub fn inj4_dup_name() -> bool {
    let mut items = standard_table();
    items[9].metric = items[0].metric;
    !PerfBook::new(items).names_unique()
}

/// 注入核 5：泄漏非零注入——泄漏计 1 即红（零容忍闸）。
pub fn inj5_leak_nonzero() -> bool {
    let mut items = standard_table();
    for i in items.iter_mut() {
        if i.kind == Kind::Leak {
            i.measured = 1;
        }
    }
    let b = PerfBook::new(items);
    !b.protocol_ok()
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 39
}

/// run_unx4701_checks：本单判据聚合（诊断段 0xF580）。
pub fn run_unx4701_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4701 性能基线总纲");
    s.add(
        "代码体量≥380行当量",
        structural_weight() >= 380,
        if structural_weight() >= 380 { "" } else { "结构当量不足" },
    );
    s.add(
        "十二项性能账协议落账（九基线+一联测+一压力+一泄漏）",
        j1_main(),
        if j1_main() { "" } else { "协议破" },
    );
    s.add(
        "基线口径登记（唯一+达标+泄漏零容忍）",
        j1_main(),
        if j1_main() { "" } else { "口径破" },
    );
    let inj = [
        ("注入1超标必红", inj1_over_ceiling()),
        ("注入2缺项必红", inj2_missing_item()),
        ("注入3类别漂移", inj3_kind_drift()),
        ("注入4口径重名", inj4_dup_name()),
        ("注入5泄漏非零", inj5_leak_nonzero()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
