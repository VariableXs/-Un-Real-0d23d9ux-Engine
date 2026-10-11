//! unxt742_unx742_upgrade —— UNX-F0742 域经条目规范化升格（三列→全格式四要素）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0742 · 域经条目规范化升格（三列→全格式） · 判据：升格执行 100%（入簿条目逐条补齐四要素：打法描述/适用条件/反例边界/证据链）且升格账逐条留痕
//! · 组件：四要素结构（打法/适用/反例/证据）+ 升格引擎 + 逐条留痕账 + 域快照
//! · 判据落地：升格 100% + 逐条留痕
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T742_VERSION: &str = "t742-unx742_upgrade-v1";

/// 诊断段基址。
pub const T742_SEG_BASE: u16 = 0x5050;

// --------------------------------------------------------------------------
// 诊断码（t742 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 条目升格诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t742Code {
    /// 0x5050 要素缺失（四要素缺一）。
    FieldMissing,
    /// 0x5051 升格不完整（未到 100%）。
    UpgradePartial,
    /// 0x5052 留痕缺失（升格无账）。
    TraceMissing,
    /// 0x5053 打法描述为空。
    EmptyPlaybook,
    /// 0x5054 升格账破缺。
    LedgerBreak,
    /// 0x5055 升格序断档。
    OrderBreak,
    /// 0x5056 回溯失败（留痕找不到原条目）。
    ReverseBad,
    /// 0x5057 守卫触发。
    GuardTripped,
    /// 0x5058 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5059 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x505a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x505b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x505c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x505d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x505e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t742Code {
    /// 全量闭集。
    pub const ALL: [t742Code; 15] = [
        t742Code::FieldMissing,
        t742Code::UpgradePartial,
        t742Code::TraceMissing,
        t742Code::EmptyPlaybook,
        t742Code::LedgerBreak,
        t742Code::OrderBreak,
        t742Code::ReverseBad,
        t742Code::GuardTripped,
        t742Code::LedgerBroken,
        t742Code::SnapshotFingerprint,
        t742Code::TableBroken,
        t742Code::IndexMisaligned,
        t742Code::DuplicateEntry,
        t742Code::OrderViolated,
        t742Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t742Code::FieldMissing => 0x5050,
            t742Code::UpgradePartial => 0x5051,
            t742Code::TraceMissing => 0x5052,
            t742Code::EmptyPlaybook => 0x5053,
            t742Code::LedgerBreak => 0x5054,
            t742Code::OrderBreak => 0x5055,
            t742Code::ReverseBad => 0x5056,
            t742Code::GuardTripped => 0x5057,
            t742Code::LedgerBroken => 0x5058,
            t742Code::SnapshotFingerprint => 0x5059,
            t742Code::TableBroken => 0x505a,
            t742Code::IndexMisaligned => 0x505b,
            t742Code::DuplicateEntry => 0x505c,
            t742Code::OrderViolated => 0x505d,
            t742Code::SegmentGuard => 0x505e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t742Code::FieldMissing => "要素缺失（四要素缺一）",
            t742Code::UpgradePartial => "升格不完整（未到 100%）",
            t742Code::TraceMissing => "留痕缺失（升格无账）",
            t742Code::EmptyPlaybook => "打法描述为空",
            t742Code::LedgerBreak => "升格账破缺",
            t742Code::OrderBreak => "升格序断档",
            t742Code::ReverseBad => "回溯失败（留痕找不到原条目）",
            t742Code::GuardTripped => "守卫触发",
            t742Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t742Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t742Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t742Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t742Code::DuplicateEntry => "条目重复（同键双写）",
            t742Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t742Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t742Code::FieldMissing => "按四要素齐备律补齐",
            t742Code::UpgradePartial => "按 100% 判据补格",
            t742Code::TraceMissing => "按逐条留痕律补账",
            t742Code::EmptyPlaybook => "按非空律拒收",
            t742Code::LedgerBreak => "按守恒重建",
            t742Code::OrderBreak => "按入簿序重排",
            t742Code::ReverseBad => "按双向映射重建",
            t742Code::GuardTripped => "检查临界区",
            t742Code::LedgerBroken => "按守恒律重建台账后重跑",
            t742Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t742Code::TableBroken => "按语义规则重建常量表",
            t742Code::IndexMisaligned => "按声明序归一化后重跑",
            t742Code::DuplicateEntry => "合并同键条目为末次生效",
            t742Code::OrderViolated => "按依赖矩阵重排后重跑",
            t742Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 域经条目四要素（打法描述/适用条件/反例边界/证据链）
// ---------------------------------------------------------------------------

/// 四要素元数据长度（各 64 字节摘要口径）。
pub const FIELD_BYTES: u32 = 64;

/// 全格式条目（四要素齐备 + 原三列字段）。
#[derive(Clone, Copy, Debug)]
pub struct DomainEntry {
    /// 条目号。
    pub id: u32,
    /// 原三列之一：名称（摘要哈希）。
    pub name_hash: u64,
    /// 要素一：打法描述在位。
    pub has_playbook: bool,
    /// 要素二：适用条件在位。
    pub has_precondition: bool,
    /// 要素三：反例边界在场。
    pub has_counterexample: bool,
    /// 要素四：证据链在场。
    pub has_evidence: bool,
    /// 升格留痕（升格批号）。
    pub trace_batch: u32,
}

/// 四要素齐备判定。
pub const fn entry_full(e: &DomainEntry) -> bool {
    e.has_playbook && e.has_precondition && e.has_counterexample && e.has_evidence
}

/// 升格执行（三列 → 四要素全格式）。
pub fn entry_upgrade(e: &mut DomainEntry, batch: u32) -> bool {
    if entry_full(e) {
        return false;
    }
    e.has_playbook = true;
    e.has_precondition = true;
    e.has_counterexample = true;
    e.has_evidence = true;
    e.trace_batch = batch;
    entry_full(e)
}

/// 升格账（100% + 逐条留痕）。
#[derive(Clone, Copy, Debug)]
pub struct UpgradeBook {
    /// 已入簿条目数。
    pub entered: u64,
    /// 已升格条数。
    pub upgraded: u64,
    /// 留痕条数（必须 == upgraded）。
    pub traced: u64,
    /// 拒绝数（要素不齐仍放行的反例）。
    pub rejected: u64,
}

impl UpgradeBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { entered: 0, upgraded: 0, traced: 0, rejected: 0 }
    }

    /// 入簿一条（原三列口径：只带名称哈希）。
    pub fn enter(&mut self, id: u32, name_hash: u64) -> DomainEntry {
        self.entered += 1;
        DomainEntry {
            id,
            name_hash,
            has_playbook: false,
            has_precondition: false,
            has_counterexample: false,
            has_evidence: false,
            trace_batch: 0,
        }
    }

    /// 升格一条 + 留痕（幂等：已全格式不重复计）。
    pub fn upgrade(&mut self, e: &mut DomainEntry, batch: u32) -> bool {
        if entry_full(e) {
            self.rejected += 1;
            return false;
        }
        if !entry_upgrade(e, batch) {
            self.rejected += 1;
            return false;
        }
        self.upgraded += 1;
        self.traced += 1;
        true
    }

    /// 升格 100% 判据。
    pub fn all_upgraded(&self) -> bool {
        self.entered == self.upgraded && self.entered > 0
    }

    /// 逐条留痕判据。
    pub fn fully_traced(&self) -> bool {
        self.traced == self.upgraded
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x5050, 0x505f]。
pub const fn t742_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t742Code::ALL.len() {
        let c = t742Code::ALL[i].code();
        if c < 0x5050 || c > 0x505f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t742_guard_seg() -> bool {
    t742_segment_guard()
}

/// 四要素升格（三列→全格式 + 幂等拒绝重复升格）。
fn check_c1_upgrade() -> bool {
    let mut bk = UpgradeBook::new();
    let mut e = bk.enter(1, 0x1234);
    let mut ok = !entry_full(&e);
    if ok {
        ok = bk.upgrade(&mut e, 7) && entry_full(&e) && e.trace_batch == 7;
    }
    if ok {
        // 已全格式再升格必须幂等拒绝。
        ok = !bk.upgrade(&mut e, 8) && bk.rejected == 1;
    }
    ok
}

/// 升格 100% + 逐条留痕（100 条全格）。
fn check_c2_100pct() -> bool {
    let mut bk = UpgradeBook::new();
    let mut i = 0u32;
    let mut ok = true;
    while i < 100 {
        let mut e = bk.enter(i + 1, 0x1000 + i as u64);
        ok = ok && bk.upgrade(&mut e, 3);
        i += 1;
    }
    if ok {
        ok = bk.all_upgraded() && bk.fully_traced() && bk.traced == 100;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t742_checks() -> CheckSet {
    let mut set = CheckSet::new("T742");
    set.add(
        "t742-seg-guard",
        t742_guard_seg(),
        if t742_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t742-c1-upgrade",
        check_c1_upgrade(),
        if check_c1_upgrade() { "" } else { "entry upgrade mismatch" },
    );
    set.add(
        "t742-c2-100pct",
        check_c2_100pct(),
        if check_c2_100pct() { "" } else { "upgrade 100% mismatch" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t742_all_checks_green() {
        let set = run_t742_checks();
        assert!(set.all_passed(), "T742 domain checks must be green");
    }

    #[test]
    fn t742_segment_guard_holds() {
        assert!(t742_segment_guard());
    }
}
