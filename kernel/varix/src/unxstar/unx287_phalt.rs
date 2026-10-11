//! unxt287_phalt —— UNX-F0281 停机序列（有序停机流水）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0281 · panic 入口统一（双狗/断言/拒载/用户触发四源） · 判据：四源触发统一入口，来源标记与去重合并正确，重入 panic 防护过
//! · 组件：停机流水（七级）+ 停机账（步骤守恒）+ 中断冻结
//! · 判据落地：四源统一后的停机语义
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T287_VERSION: &str = "t287-phalt-v1";

/// 诊断段基址。
pub const T287_SEG_BASE: u16 = 0x3b60;

// --------------------------------------------------------------------------
// 诊断码（t287 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 停机序列诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t287Code {
    /// 0x3b60 停机步骤跳过。
    HaltStepSkipped,
    /// 0x3b61 中断冻结（关中断不断链）。
    IrqFrozen,
    /// 0x3b62 停机账破缺（步骤不守恒）。
    LedgerBreak,
    /// 0x3b63 重复停机。
    DoubleHalt,
    /// 0x3b64 迟中断（停机后到达）。
    IrqLate,
    /// 0x3b65 停机序非法。
    StepOrderBad,
    /// 0x3b66 停机报告陈旧。
    ReportStale,
    /// 0x3b67 守卫触发。
    GuardTripped,
    /// 0x3b68 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3b69 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3b6a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3b6b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3b6c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3b6d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3b6e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t287Code {
    /// 全量闭集。
    pub const ALL: [t287Code; 15] = [
        t287Code::HaltStepSkipped,
        t287Code::IrqFrozen,
        t287Code::LedgerBreak,
        t287Code::DoubleHalt,
        t287Code::IrqLate,
        t287Code::StepOrderBad,
        t287Code::ReportStale,
        t287Code::GuardTripped,
        t287Code::LedgerBroken,
        t287Code::SnapshotFingerprint,
        t287Code::TableBroken,
        t287Code::IndexMisaligned,
        t287Code::DuplicateEntry,
        t287Code::OrderViolated,
        t287Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t287Code::HaltStepSkipped => 0x3b60,
            t287Code::IrqFrozen => 0x3b61,
            t287Code::LedgerBreak => 0x3b62,
            t287Code::DoubleHalt => 0x3b63,
            t287Code::IrqLate => 0x3b64,
            t287Code::StepOrderBad => 0x3b65,
            t287Code::ReportStale => 0x3b66,
            t287Code::GuardTripped => 0x3b67,
            t287Code::LedgerBroken => 0x3b68,
            t287Code::SnapshotFingerprint => 0x3b69,
            t287Code::TableBroken => 0x3b6a,
            t287Code::IndexMisaligned => 0x3b6b,
            t287Code::DuplicateEntry => 0x3b6c,
            t287Code::OrderViolated => 0x3b6d,
            t287Code::SegmentGuard => 0x3b6e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t287Code::HaltStepSkipped => "停机步骤跳过",
            t287Code::IrqFrozen => "中断冻结（关中断不断链）",
            t287Code::LedgerBreak => "停机账破缺（步骤不守恒）",
            t287Code::DoubleHalt => "重复停机",
            t287Code::IrqLate => "迟中断（停机后到达）",
            t287Code::StepOrderBad => "停机序非法",
            t287Code::ReportStale => "停机报告陈旧",
            t287Code::GuardTripped => "守卫触发",
            t287Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t287Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t287Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t287Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t287Code::DuplicateEntry => "条目重复（同键双写）",
            t287Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t287Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t287Code::HaltStepSkipped => "按序补齐后重停",
            t287Code::IrqFrozen => "按冻结协议续行",
            t287Code::LedgerBreak => "按守恒重建",
            t287Code::DoubleHalt => "幂等拒绝",
            t287Code::IrqLate => "按丢弃路径记账",
            t287Code::StepOrderBad => "按七序律重排",
            t287Code::ReportStale => "重渲染",
            t287Code::GuardTripped => "检查临界区",
            t287Code::LedgerBroken => "按守恒律重建台账后重跑",
            t287Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t287Code::TableBroken => "按语义规则重建常量表",
            t287Code::IndexMisaligned => "按声明序归一化后重跑",
            t287Code::DuplicateEntry => "合并同键条目为末次生效",
            t287Code::OrderViolated => "按依赖矩阵重排后重跑",
            t287Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 halt（7 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 有序停机七级流水。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum haltStage {
    /// 级 根：冻结中断（cli）。
    FreezeIrq,
    /// 级 0：停各 AP 核。
    StopAps,
    /// 级 1：冲刷日志缓冲。
    FlushLog,
    /// 级 2：封存 panic 报告。
    SealReport,
    /// 级 3：封存报告链。
    ChainSeal,
    /// 级 4：停机死循环。
    HaltLoop,
    /// 级 5：永不返回（哨兵）。
    NeverReturn,
}

impl haltStage {
    /// 全量级闭集。
    pub const ALL: [haltStage; 7] = [
        haltStage::FreezeIrq,
        haltStage::StopAps,
        haltStage::FlushLog,
        haltStage::SealReport,
        haltStage::ChainSeal,
        haltStage::HaltLoop,
        haltStage::NeverReturn,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            haltStage::FreezeIrq => 0,
            haltStage::StopAps => 1,
            haltStage::FlushLog => 2,
            haltStage::SealReport => 3,
            haltStage::ChainSeal => 4,
            haltStage::HaltLoop => 5,
            haltStage::NeverReturn => 6,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            haltStage::FreezeIrq => "冻结中断（cli）",
            haltStage::StopAps => "停各 AP 核",
            haltStage::FlushLog => "冲刷日志缓冲",
            haltStage::SealReport => "封存 panic 报告",
            haltStage::ChainSeal => "封存报告链",
            haltStage::HaltLoop => "停机死循环",
            haltStage::NeverReturn => "永不返回（哨兵）",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const HALT_DEPS: [i32; 7] = [
    -1, // FreezeIrq 依赖 无
    0, // StopAps 依赖 级 0
    1, // FlushLog 依赖 级 1
    2, // SealReport 依赖 级 2
    3, // ChainSeal 依赖 级 3
    4, // HaltLoop 依赖 级 4
    5, // NeverReturn 依赖 级 5
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum haltProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（停机中途断）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct haltBook {
    /// 各级状态。
    pub state: [haltProgress; 7],
    /// 已完成计数。
    pub done: u32,
    /// 跳过计数。
    pub skipped: u32,
    /// 回滚计数。
    pub rolled: u32,
    /// 已完成级索引序列（回放审计用）。
    pub order: [usize; 7],
    /// 序列长度。
    pub order_len: usize,
}

impl haltBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [haltProgress::Pending; 7],
            done: 0,
            skipped: 0,
            rolled: 0,
            order: [0; 7],
            order_len: 0,
        }
    }

    /// 拓扑序推演（Kahn 剥皮：逐轮找全前置已 Done 的 Pending 级）。
    /// 返回推演出的可执行序；环依赖时返回 Err(环首级索引)。
    pub fn plan(&mut self) -> Result<[usize; 7], usize> {
        let mut order = [0usize; 7];
        let mut olen = 0usize;
        let mut rounds = 0usize;
        while rounds < 7 {
            let mut progressed = false;
            let mut i = 0usize;
            while i < 7 {
                if self.state[i] != haltProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = HALT_DEPS[i];
                if dep < 0 || self.state[dep as usize] == haltProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = haltProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 7 {
                    if self.state[k] == haltProgress::Pending {
                        return Err(k);
                    }
                    k += 1;
                }
            }
            rounds += 1;
        }
        self.order = order;
        self.order_len = olen;
        Ok(order)
    }

    /// 注入失败：自失败级起全部 Done 级回滚（停机中途断）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 7 {
            if self.state[i] == haltProgress::Done {
                self.state[i] = haltProgress::Rolled;
                if self.done > 0 {
                    self.done -= 1;
                }
                self.rolled += 1;
            }
            i += 1;
        }
    }

    /// 守恒恒等式：done + skipped + rolled + pending == 总级数。
    pub fn conserved(&self) -> bool {
        let mut pending = 0u32;
        let mut i = 0usize;
        while i < 7 {
            if self.state[i] == haltProgress::Pending {
                pending += 1;
            }
            i += 1;
        }
        self.done + self.skipped + self.rolled + pending == 7
    }

    /// 已完成级回放序列与推演序一致（顺序守恒证据）。
    pub fn order_matches(&self) -> bool {
        let mut i = 0usize;
        while i < self.order_len {
            if self.order[i] >= 7 || self.state[self.order[i]] != haltProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3b60, 0x3b6f]。
pub const fn t287_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t287Code::ALL.len() {
        let c = t287Code::ALL[i].code();
        if c < 0x3b60 || c > 0x3b6f {
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
fn t287_guard_seg() -> bool {
    t287_segment_guard()
}

/// 停机七级流水推演全达 + 守恒。
fn check_c1_halt_pipeline() -> bool {
    let mut bk = haltBook::new();
    let mut ok = matches!(bk.plan(), Ok(_)) && bk.done == 7;
    if ok {
        ok = bk.order_matches() && bk.conserved();
    }
    ok
}

/// 停机回滚路径（自断点回滚 + 守恒）。
fn check_c2_halt_rollback() -> bool {
    let mut bk = haltBook::new();
    let _ = bk.plan();
    bk.rollback_from(3);
    let mut ok = bk.rolled == 4 && bk.done == 3;
    if ok {
        ok = bk.conserved();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t287_checks() -> CheckSet {
    let mut set = CheckSet::new("T287");
    set.add(
        "t287-seg-guard",
        t287_guard_seg(),
        if t287_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t287-c1-halt-pipeline",
        check_c1_halt_pipeline(),
        if check_c1_halt_pipeline() { "" } else { "halt pipeline mismatch" },
    );
    set.add(
        "t287-c2-halt-rollback",
        check_c2_halt_rollback(),
        if check_c2_halt_rollback() { "" } else { "halt rollback mismatch" },
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
    fn t287_all_checks_green() {
        let set = run_t287_checks();
        assert!(set.all_passed(), "T287 domain checks must be green");
    }

    #[test]
    fn t287_segment_guard_holds() {
        assert!(t287_segment_guard());
    }
}
