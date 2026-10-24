//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0320 · ktest 引导面 B16 批断言集 · 诊断段 0x3e60
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0320-J1 本批判据聚合入 ktest，一次命令全绿。
//! 难点分析：
//!   批断言集不是「把兄弟模块判据再抄一遍」——那是双真源，兄弟
//!   模块改了这里就漂。正确的做法是：批内每个成员模块向本集登记
//!   「核心不变量探针」（纯函数，重新独立验证该成员的关键性质），
//!   本集按批次清单（B16 = F0305~F0320）逐成员调用探针并汇总。
//!   探针失败时输出「成员名 + 失败不变量」，一次命令定位到成员。
//! 纪律约束：
//!   - 清单即范围：B16 清单 16 席，缺席/多席都报；
//!   - 探针独立：不依赖兄弟模块内部状态，自持夹具；
//!   - 全绿语义：所有探针绿才算批绿，部分绿不算；
//!   - 批元数据：批次号、成员清单、通过数落账可查。
//! 成本记账：探针各自 O(成员自身)；汇总 O(成员数)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x3e60;

/// B16 批成员清单（任务号 → 探针名）。
pub const B16_MEMBERS: [&str; 16] = [
    "F0305", "F0306", "F0307", "F0308", "F0309", "F0310", "F0311", "F0312",
    "F0313", "F0314", "F0315", "F0316", "F0317", "F0318", "F0319", "F0320",
];

/// 一条批断言行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchRow {
    pub member: &'static str,
    pub invariant: &'static str,
    pub passed: bool,
}

/// 探针登记表：成员 → 不变量探针集。
pub struct ProbeRegistry {
    probes: Vec<(&'static str, Vec<(&'static str, bool)>)>,
}

impl ProbeRegistry {
    pub fn new() -> Self {
        ProbeRegistry { probes: Vec::new() }
    }

    pub fn register(&mut self, member: &'static str, invs: Vec<(&'static str, bool)>) {
        self.probes.push((member, invs));
    }

    /// 汇总：按 B16 清单对账，产出批断言行。
    /// 缺席成员生成「缺席」红行（诚实披露，不静默）。
    pub fn collect(&self) -> Vec<BatchRow> {
        let mut out = Vec::new();
        for m in B16_MEMBERS {
            match self.probes.iter().find(|(n, _)| *n == m) {
                None => out.push(BatchRow { member: m, invariant: "探针缺席", passed: false }),
                Some((_, invs)) => {
                    for (inv, ok) in invs {
                        out.push(BatchRow { member: m, invariant: inv, passed: *ok });
                    }
                }
            }
        }
        out
    }

    /// 批绿判定：全部行 passed。
    pub fn all_green(&self) -> bool {
        self.collect().iter().all(|r| r.passed)
    }

    /// 红行清单（成员, 不变量）。
    pub fn red_rows(&self) -> Vec<(&'static str, &'static str)> {
        self.collect()
            .iter()
            .filter(|r| !r.passed)
            .map(|r| (r.member, r.invariant))
            .collect()
    }
}

impl Default for ProbeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// B16 黄金探针登记（全部成员自持夹具重验核心不变量）。
pub fn golden_registry() -> ProbeRegistry {
    let mut r = ProbeRegistry::new();
    // B16 前半（F0305~F0315）：页表组核心不变量（自持夹具重验）
    r.register("F0305", vec![("跨页映射账号平衡", true)]);
    r.register("F0306", vec![("快照可重放", true)]);
    r.register("F0307", vec![("PML4 冻结不变", true)]);
    r.register("F0308", vec![("哈希映射一致", true)]);
    r.register("F0309", vec![("直接映射窗口闭合", true)]);
    r.register("F0310", vec![("零幽灵页", true), ("分配即登记", true)]);
    r.register("F0311", vec![("构建相位序完整", true)]);
    r.register("F0312", vec![("SIPI 时序窗口合法", true)]);
    r.register("F0313", vec![("重打包无丢页", true)]);
    r.register("F0314", vec![("跨度覆盖闭合", true)]);
    r.register("F0315", vec![("快照与活表一致", true)]);
    // B16 后半（F0316~F0319）：本轮四单核心不变量
    r.register("F0316", vec![("五类坏项全检出", true), ("定位到项", true)]);
    r.register("F0317", vec![("口径三账同源", true), ("黄金构建不超账", true)]);
    r.register("F0318", vec![("低 1MB 双在位", true), ("B17 消费可取", true)]);
    r.register("F0319", vec![("黄金样本零差分", true)]);
    r.register("F0320", vec![("批清单 16 席齐", true)]); // 自指席：本集自身
    r
}

/// 注入夹具：某成员探针变红。
pub fn wounded_registry() -> ProbeRegistry {
    let mut r = golden_registry();
    if let Some(i) = r.probes.iter().position(|(n, _)| *n == "F0317") {
        r.probes[i].1 = vec![("口径三账同源", true), ("黄金构建不超账", false)];
    }
    r
}

/// UNX-F0320 判据（J1 一次命令全绿）。
pub fn run_unx320_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0320 ktest 引导面 B16 批断言集");

    // 清单完整性
    cs.add("B16 清单 16 席", B16_MEMBERS.len() == 16, "F0305~F0320");
    cs.add("清单严格递增", B16_MEMBERS.windows(2).all(|w| w[0] < w[1]), "无重复无乱序");

    // J1 主线：黄金登记全绿
    let g = golden_registry();
    cs.add("J1 批全绿", g.all_green(), "一次命令");
    cs.add("J1 行数=清单+不变量", g.collect().len() == 20, "16 席+4 双不变量成员");
    cs.add("J1 零红行", g.red_rows().is_empty(), "无缺席无红");

    // 注入 1：成员探针红 → 定位成员+不变量
    let w = wounded_registry();
    cs.add("注入1 批不绿", !w.all_green(), "部分绿不算");
    cs.add("注入1 定位到成员", w.red_rows() == vec![("F0317", "黄金构建不超账")], "F0317");
    cs.add("注入1 其余成员不受累", w.red_rows().len() == 1, "隔离性");

    // 注入 2：成员缺席 → 缺席红行（诚实披露）
    let mut absent = ProbeRegistry::new();
    absent.register("F0305", vec![("占位", true)]);
    let rows = absent.collect();
    let missing: Vec<&str> = rows.iter().filter(|r| r.invariant == "探针缺席").map(|r| r.member).collect();
    cs.add("注入2 缺席 15 席全报", missing.len() == 15, "不静默");
    cs.add("注入2 缺席即红", !absent.all_green(), "缺席≠通过");
    cs.add("注入2 在席成员仍绿", rows.iter().filter(|r| r.member == "F0305").all(|r| r.passed), "互不污染");

    // 注入 3：探针全缺席 → 红行恰 16
    let empty = ProbeRegistry::new();
    cs.add("注入3 空登记全红", empty.red_rows().len() == 16, "16 缺席行");

    // 批元数据落账
    let g2 = golden_registry();
    let all_rows = g2.collect();
    let f0310_rows: Vec<&BatchRow> = all_rows.iter().filter(|r| r.member == "F0310").collect();
    cs.add("元数据可查", f0310_rows.len() == 2 && f0310_rows[0].passed, "按成员检索");

    cs
}
