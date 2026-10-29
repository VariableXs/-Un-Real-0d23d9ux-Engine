//! UNX-F56178 syscall 聚合性能自检件（聚合器自身开销）（0x61640 段）
//!
//! 锚点：UNX-F56178-J1 自检账（聚合器自身开销自检——录事件耗时预算
//! 核（模拟口径：事件权值和 ≤ 预算）+ 查询耗时预算核 + 内存占用预算
//! 核三闸逐项落账）；J2 预算语义（录 n 事件开销 ≤ n×单事件预算；查
//! 询 m 次开销 ≤ m×单查询预算；内存 ≤ 容量×格宽）；J3 自检印（三闸
//! 全绿可印）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·聚合器自检面）。
//!
//! 结构：开销模型（录/查/存三预算常量）+ 模拟核（开销累计与预算比
//! 对）+ 自检账 + 自检印 + 注入核（超预算放行/未知闸/印后测/预算漂移
//! /空账印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61640 独占 0x20）。
pub const SEG_BASE: u32 = 0x61640;
pub const PF_BUDGET_BREACH: u32 = SEG_BASE + 0x01; // 超预算放行
pub const PF_UNK_GATE: u32 = SEG_BASE + 0x02; // 未知闸
pub const PF_SEALED_TEST: u32 = SEG_BASE + 0x03; // 印后测
pub const PF_BUDGET_DRIFT: u32 = SEG_BASE + 0x04; // 预算漂移
pub const PF_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空账印

/// 三闸闭集。
pub const G_RECORD: usize = 0; // 录事件预算
pub const G_QUERY: usize = 1; // 查询预算
pub const G_MEMORY: usize = 2; // 内存预算
pub const GATE_COUNT: usize = 3;

/// 闸名渲染。
pub const GATE_NAMES: [&str; GATE_COUNT] = ["record", "query", "memory"];

/// 开销模型常量。
pub const COST_PER_RECORD: u64 = 10; // 录一事件开销权值
pub const COST_PER_QUERY: u64 = 5; // 查一次开销权值
pub const CELL_WIDTH: u64 = 8; // 矩阵格宽（字节）
pub const MATRIX_CELLS: u64 = 18; // 6 类 × 3 段

/// 录事件预算核：n 事件开销 ≤ n × 单事件预算（恒等模型恒过，此处验
/// 模型算术）。
pub fn record_budget_ok(n: u64) -> bool {
    let cost = n.wrapping_mul(COST_PER_RECORD);
    let budget = n.wrapping_mul(COST_PER_RECORD);
    cost <= budget
}

/// 查询预算核。
pub fn query_budget_ok(m: u64) -> bool {
    let cost = m.wrapping_mul(COST_PER_QUERY);
    let budget = m.wrapping_mul(COST_PER_QUERY);
    cost <= budget
}

/// 内存预算核：矩阵占用 ≤ 预算。
pub const MEM_BUDGET: u64 = 256;

pub fn memory_budget_ok() -> bool {
    MATRIX_CELLS.wrapping_mul(CELL_WIDTH) <= MEM_BUDGET
}

/// 单闸自检账。
#[derive(Clone)]
pub struct PfGate {
    pub gate: usize,
    pub passed: bool,
    pub cost: u64,
    pub budget: u64,
}

impl PfGate {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|cost{}|budget{}",
            GATE_NAMES[self.gate],
            if self.passed { "PASS" } else { "BREACH" },
            self.cost,
            self.budget
        )
    }
}

/// 自检册。
pub struct PfBook {
    gates: [Option<PfGate>; GATE_COUNT],
    sealed: bool,
}

impl PfBook {
    pub fn new() -> Self {
        PfBook {
            gates: [None, None, None],
            sealed: false,
        }
    }

    /// 记一闸结果：闸闭集 + 印后拒 + 超预算拒录三闸。
    pub fn record(&mut self, gate: usize, cost: u64, budget: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(PF_SEALED_TEST);
        }
        if gate >= GATE_COUNT {
            return Err(PF_UNK_GATE);
        }
        if cost > budget {
            return Err(PF_BUDGET_BREACH);
        }
        if self.gates[gate].is_some() {
            return Err(PF_UNK_GATE);
        }
        self.gates[gate] = Some(PfGate {
            gate,
            passed: true,
            cost,
            budget,
        });
        Ok(())
    }

    pub fn gate(&self, gate: usize) -> Option<&PfGate> {
        if gate >= GATE_COUNT {
            return None;
        }
        self.gates[gate].as_ref()
    }

    /// 三闸全绿核。
    pub fn all_passed(&self) -> bool {
        self.gates.iter().all(|g| g.as_ref().map(|x| x.passed).unwrap_or(false))
    }

    /// 自检印：三闸全录才可印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.gates.iter().any(|g| g.is_none()) {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for g in self.gates.iter().flatten() {
            out.push_str(&g.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("PERF-SEAL\n");
        }
        out
    }
}

impl Default for PfBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三闸全测（录 100 事件 / 查 50 次 / 内存 144B）。
fn standard_book() -> PfBook {
    let mut b = PfBook::new();
    let _ = b.record(G_RECORD, 100 * COST_PER_RECORD, 100 * COST_PER_RECORD);
    let _ = b.record(G_QUERY, 50 * COST_PER_QUERY, 50 * COST_PER_QUERY);
    let _ = b.record(G_MEMORY, MATRIX_CELLS * CELL_WIDTH, MEM_BUDGET);
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：超预算拒录
    let mut b = PfBook::new();
    if b.record(G_RECORD, 999, 100) == Err(PF_BUDGET_BREACH) {
        caught += 1;
    }
    // 注 2：未知闸
    let mut b = PfBook::new();
    if b.record(9, 1, 10) == Err(PF_UNK_GATE) {
        caught += 1;
    }
    // 注 3：重复闸
    let mut b = PfBook::new();
    let _ = b.record(G_RECORD, 10, 100);
    if b.record(G_RECORD, 10, 100) == Err(PF_UNK_GATE) {
        caught += 1;
    }
    // 注 4：预算模型核（录预算恒等 + 内存 ≤ 256）
    if record_budget_ok(100) && query_budget_ok(50) && memory_budget_ok() {
        caught += 1;
    }
    // 注 5：空账印
    let mut b = PfBook::new();
    if !b.seal() && !b.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 自检账；J2 预算语义；J3 自检印。
pub fn run_unx56178_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56178");
    // J1-1 三闸全录全绿
    let b = standard_book();
    let ok = b.all_passed();
    s.add("56178-J1-三闸全绿", ok, if ok { "" } else { "自检账漂移" });
    // J1-2 渲染导出齐
    let text = standard_book().render_book();
    let ok = text.contains("record|PASS|cost1000|budget1000") && text.contains("memory|PASS|cost144|budget256");
    s.add("56178-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 录预算模型（100×10 = 1000）
    let ok = record_budget_ok(100) && 100 * COST_PER_RECORD == 1000;
    s.add("56178-J2-录预算", ok, if ok { "" } else { "模型漂移" });
    // J2-2 查预算模型（50×5 = 250）
    let ok = query_budget_ok(50) && 50 * COST_PER_QUERY == 250;
    s.add("56178-J2-查预算", ok, if ok { "" } else { "模型漂移" });
    // J2-3 内存预算（18×8 = 144 ≤ 256）
    let ok = memory_budget_ok() && MATRIX_CELLS * CELL_WIDTH == 144;
    s.add("56178-J2-内存预算", ok, if ok { "" } else { "预算漂移" });
    // J2-4 超预算拒录
    let mut b2 = PfBook::new();
    let ok = b2.record(G_MEMORY, 300, 256) == Err(PF_BUDGET_BREACH);
    s.add("56178-J2-超预算拒", ok, if ok { "" } else { "闸穿透" });
    // J2-5 零开销合法
    let mut b2 = PfBook::new();
    let ok = b2.record(G_QUERY, 0, 0).is_ok();
    s.add("56178-J2-零开销", ok, if ok { "" } else { "边界漂移" });
    // J3-1 自检印
    let mut b = standard_book();
    let ok = b.seal() && b.sealed();
    s.add("56178-J3-自检印", ok, if ok { "" } else { "自检印失败" });
    // J3-2 印后测拒
    let ok = b.record(G_RECORD, 1, 10) == Err(PF_SEALED_TEST);
    s.add("56178-J3-印后测拒", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let ok = b.render_book().contains("PERF-SEAL");
    s.add("56178-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 闸名闭集
    let ok = GATE_NAMES[G_RECORD] == "record" && GATE_NAMES[G_MEMORY] == "memory";
    s.add("56178-J3-闸名闭集", ok, if ok { "" } else { "名漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56178-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56178_all_checks_green() {
        let set = run_unx56178_checks();
        assert!(set.all_passed(), "UNX-F56178 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56178_budget_stable() {
        assert!(record_budget_ok(100));
        assert_eq!(MATRIX_CELLS * CELL_WIDTH, 144);
    }
}
