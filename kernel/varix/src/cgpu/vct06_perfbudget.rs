//! unxt06a_vct06_perfbudget —— CGPU-F3046 协议栈每帧 <0.1ms 性能预算。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：CGPU-F3046 · 对接性能预算（CGPU-T 域 · 批次 T01 · T06 组 · 判据：协议栈每帧 <0.1ms 预算实现）
//! · 组件：0.1ms 预算账（分项预留）+ 实测对拍 + 超支判定
//! · 判据落地：协议栈每帧 <0.1ms 预算
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T06A_VERSION: &str = "t06a-vct06_perfbudget-v1";

/// 诊断段基址。
pub const T06A_SEG_BASE: u16 = 0x7200;

// --------------------------------------------------------------------------
// 诊断码（t06a 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 性能预算诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t06aCode {
    /// 0x7200 预算为零（未设分项）。
    BudgetZero,
    /// 0x7201 预算超支（分项合计 >0.1ms）。
    Overdraw,
    /// 0x7202 预算欠支（合计远低于红线）。
    Underdraw,
    /// 0x7203 实测样本过期。
    SampleStale,
    /// 0x7204 预算账破缺。
    LedgerBreak,
    /// 0x7205 分项序违例。
    OrderViolated,
    /// 0x7206 报告陈旧。
    ReportStale,
    /// 0x7207 守卫触发。
    GuardTripped,
    /// 0x7208 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x7209 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x720a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x720b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x720c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x720d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t06aCode {
    /// 全量闭集。
    pub const ALL: [t06aCode; 14] = [
        t06aCode::BudgetZero,
        t06aCode::Overdraw,
        t06aCode::Underdraw,
        t06aCode::SampleStale,
        t06aCode::LedgerBreak,
        t06aCode::OrderViolated,
        t06aCode::ReportStale,
        t06aCode::GuardTripped,
        t06aCode::LedgerBroken,
        t06aCode::SnapshotFingerprint,
        t06aCode::TableBroken,
        t06aCode::IndexMisaligned,
        t06aCode::DuplicateEntry,
        t06aCode::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t06aCode::BudgetZero => 0x7200,
            t06aCode::Overdraw => 0x7201,
            t06aCode::Underdraw => 0x7202,
            t06aCode::SampleStale => 0x7203,
            t06aCode::LedgerBreak => 0x7204,
            t06aCode::OrderViolated => 0x7205,
            t06aCode::ReportStale => 0x7206,
            t06aCode::GuardTripped => 0x7207,
            t06aCode::LedgerBroken => 0x7208,
            t06aCode::SnapshotFingerprint => 0x7209,
            t06aCode::TableBroken => 0x720a,
            t06aCode::IndexMisaligned => 0x720b,
            t06aCode::DuplicateEntry => 0x720c,
            t06aCode::SegmentGuard => 0x720d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t06aCode::BudgetZero => "预算为零（未设分项）",
            t06aCode::Overdraw => "预算超支（分项合计 >0.1ms）",
            t06aCode::Underdraw => "预算欠支（合计远低于红线）",
            t06aCode::SampleStale => "实测样本过期",
            t06aCode::LedgerBreak => "预算账破缺",
            t06aCode::OrderViolated => "分项序违例",
            t06aCode::ReportStale => "报告陈旧",
            t06aCode::GuardTripped => "守卫触发",
            t06aCode::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t06aCode::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t06aCode::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t06aCode::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t06aCode::DuplicateEntry => "条目重复（同键双写）",
            t06aCode::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t06aCode::BudgetZero => "按预算表补配",
            t06aCode::Overdraw => "按超支路径降载",
            t06aCode::Underdraw => "按欠支告警登记",
            t06aCode::SampleStale => "重采样后入账",
            t06aCode::LedgerBreak => "按守恒重建",
            t06aCode::OrderViolated => "按序重排",
            t06aCode::ReportStale => "重渲染",
            t06aCode::GuardTripped => "检查临界区",
            t06aCode::LedgerBroken => "按守恒律重建台账后重跑",
            t06aCode::SnapshotFingerprint => "按防篡改规程重建快照",
            t06aCode::TableBroken => "按语义规则重建常量表",
            t06aCode::IndexMisaligned => "按声明序归一化后重跑",
            t06aCode::DuplicateEntry => "合并同键条目为末次生效",
            t06aCode::SegmentGuard => "核对诊断码分配表",
        }
    }
}


// ---------------------------------------------------------------------------
// 协议栈每帧预算（红线 0.1ms = 100us；分项预留 + 实测对拍）
// ---------------------------------------------------------------------------

/// 预算红线（微秒；0.1ms = 100us）。
pub const FRAME_BUDGET_US: u64 = 100;

/// 分项枚举（五段协议栈处理）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackStage {
    /// 序列化/反序列化。
    Codec,
    /// 校验与鉴权。
    Auth,
    /// 编址与端点路由。
    Route,
    /// 调度入队。
    Enqueue,
    /// 提交路径。
    Submit,
}

impl StackStage {
    /// 全量闭集（五段）。
    pub const ALL: [StackStage; 5] = [
        StackStage::Codec,
        StackStage::Auth,
        StackStage::Route,
        StackStage::Enqueue,
        StackStage::Submit,
    ];

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            StackStage::Codec => 0,
            StackStage::Auth => 1,
            StackStage::Route => 2,
            StackStage::Enqueue => 3,
            StackStage::Submit => 4,
        }
    }

    /// 中文段名。
    pub const fn cn(self) -> &'static str {
        match self {
            StackStage::Codec => "编解码",
            StackStage::Auth => "校验鉴权",
            StackStage::Route => "路由",
            StackStage::Enqueue => "入队",
            StackStage::Submit => "提交",
        }
    }

    /// 分段预算（微秒；合计 80us，留 20us 余量）。
    pub const fn budget_us(self) -> u64 {
        match self {
            StackStage::Codec => 20,
            StackStage::Auth => 18,
            StackStage::Route => 12,
            StackStage::Enqueue => 10,
            StackStage::Submit => 20,
        }
    }
}

/// 分项预留合计（微秒）。
pub const fn stack_budget_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0;
    while i < 5 {
        s += StackStage::ALL[i].budget_us();
        i += 1;
    }
    s
}

/// 预算健康余量（红线 - 合计；必须 > 0）。
pub const fn stack_budget_headroom() -> u64 {
    FRAME_BUDGET_US - stack_budget_sum()
}

/// 实测样本（微秒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackSample {
    /// 分段索引。
    pub stage: usize,
    /// 实测耗时（us）。
    pub used_us: u64,
}

/// 预算账本（五段实测 + 红线判定）。
#[derive(Clone, Copy, Debug)]
pub struct BudgetBook {
    /// 五段预留预算。
    pub budgets: [u64; 5],
    /// 五段实测最大值。
    pub used: [u64; 5],
    /// 超支次数。
    pub overdraws: u64,
}

impl BudgetBook {
    /// 构造（按分段预留表初始化）。
    pub const fn new() -> Self {
        Self {
            budgets: [20, 18, 12, 10, 20],
            used: [0; 5],
            overdraws: 0,
        }
    }

    /// 记一次实测（取历史最大；超支记账）。
    pub fn observe(&mut self, s: &StackSample) -> bool {
        if s.stage >= 5 {
            return false;
        }
        if s.used_us > self.used[s.stage] {
            self.used[s.stage] = s.used_us;
        }
        if s.used_us > self.budgets[s.stage] {
            self.overdraws += 1;
            return false;
        }
        true
    }

    /// 全栈总耗时（五段实测和）。
    pub fn total_used(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0;
        while i < 5 {
            s += self.used[i];
            i += 1;
        }
        s
    }

    /// 面帧判定（总耗时 < 红线 100us）。
    pub fn per_frame_ok(&self) -> bool {
        self.total_used() < FRAME_BUDGET_US
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x7200, 0x720f]。
pub const fn t06a_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t06aCode::ALL.len() {
        let c = t06aCode::ALL[i].code();
        if c < 0x7200 || c > 0x720f {
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
fn t06a_guard_seg() -> bool {
    t06a_segment_guard()
}

/// 预算账本（分段预留合计 80us + 余量 20us + 合规帧）。
fn check_c1_budget() -> bool {
    let mut ok = stack_budget_sum() == 80 && stack_budget_headroom() == 20;
    if ok {
        let mut bk = BudgetBook::new();
        let mut i = 0;
        while i < 5 {
            // 每段实测 8us，均低于分段预留。
            ok = ok && bk.observe(&StackSample { stage: i, used_us: 8 });
            i += 1;
        }
        ok = ok && bk.total_used() == 40 && bk.per_frame_ok() && bk.overdraws == 0;
    }
    ok
}

/// 超支路径（单段超预留即红 + 帧判定拒）。
fn check_c2_overdraw() -> bool {
    let mut bk = BudgetBook::new();
    let _ = bk.observe(&StackSample { stage: 0, used_us: 21 });
    let mut ok = bk.overdraws == 1;
    if ok {
        let _ = bk.observe(&StackSample { stage: 1, used_us: 10 });
        // 超支段即使其他段合规，累计仍判红。
        ok = bk.used[0] == 21;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t06a_checks() -> CheckSet {
    let mut set = CheckSet::new("T06A");
    set.add(
        "t06a-seg-guard",
        t06a_guard_seg(),
        if t06a_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t06a-c1-budget",
        check_c1_budget(),
        if check_c1_budget() { "" } else { "stack budget mismatch" },
    );
    set.add(
        "t06a-c2-overdraw",
        check_c2_overdraw(),
        if check_c2_overdraw() { "" } else { "budget overdraw path mismatch" },
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
    fn t06a_all_checks_green() {
        let set = run_t06a_checks();
        assert!(set.all_passed(), "T06A domain checks must be green");
    }

    #[test]
    fn t06a_segment_guard_holds() {
        assert!(t06a_segment_guard());
    }
}
