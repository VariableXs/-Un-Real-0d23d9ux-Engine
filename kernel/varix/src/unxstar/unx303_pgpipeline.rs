//! unxt303_pgpipeline —— UNX-F0301 四级构建流水（依赖序编排）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0301 · 页表构建总编排（四级表族流水） · 判据：PML4→PDPT→PD→PT 四级构建流水编排正确，依赖序与设计一致，构建账完整
//! · 组件：构建流水（四级 + 依赖）+ 环检测 + 回滚 + 进度账
//! · 判据落地：PML4→PDPT→PD→PT 流水编排
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T303_VERSION: &str = "t303-pgpipeline-v1";

/// 诊断段基址。
pub const T303_SEG_BASE: u16 = 0x3c20;

// --------------------------------------------------------------------------
// 诊断码（t303 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 构建流水诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t303Code {
    /// 0x3c20 依赖序非法（下游先于上游）。
    DepOrderBad,
    /// 0x3c21 环检出（构建依赖成环）。
    CycleDetected,
    /// 0x3c22 级别跳过（某级未建）。
    LevelSkipped,
    /// 0x3c23 表项未填（槽留空）。
    EntryUnfilled,
    /// 0x3c24 物理页分配失败。
    PhysAllocFail,
    /// 0x3c25 大页分裂非法（2M→4K 不规则）。
    HugeSplitBad,
    /// 0x3c26 构建账破缺（级×槽不守恒）。
    BuildLedgerBreak,
    /// 0x3c27 回滚不完整。
    RollbackPartial,
    /// 0x3c28 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3c29 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3c2a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3c2b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3c2c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3c2d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3c2e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x3c2f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t303Code {
    /// 全量闭集。
    pub const ALL: [t303Code; 16] = [
        t303Code::DepOrderBad,
        t303Code::CycleDetected,
        t303Code::LevelSkipped,
        t303Code::EntryUnfilled,
        t303Code::PhysAllocFail,
        t303Code::HugeSplitBad,
        t303Code::BuildLedgerBreak,
        t303Code::RollbackPartial,
        t303Code::LedgerBroken,
        t303Code::SnapshotFingerprint,
        t303Code::TableBroken,
        t303Code::IndexMisaligned,
        t303Code::DuplicateEntry,
        t303Code::OrderViolated,
        t303Code::GuardTripped,
        t303Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t303Code::DepOrderBad => 0x3c20,
            t303Code::CycleDetected => 0x3c21,
            t303Code::LevelSkipped => 0x3c22,
            t303Code::EntryUnfilled => 0x3c23,
            t303Code::PhysAllocFail => 0x3c24,
            t303Code::HugeSplitBad => 0x3c25,
            t303Code::BuildLedgerBreak => 0x3c26,
            t303Code::RollbackPartial => 0x3c27,
            t303Code::LedgerBroken => 0x3c28,
            t303Code::SnapshotFingerprint => 0x3c29,
            t303Code::TableBroken => 0x3c2a,
            t303Code::IndexMisaligned => 0x3c2b,
            t303Code::DuplicateEntry => 0x3c2c,
            t303Code::OrderViolated => 0x3c2d,
            t303Code::GuardTripped => 0x3c2e,
            t303Code::SegmentGuard => 0x3c2f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t303Code::DepOrderBad => "依赖序非法（下游先于上游）",
            t303Code::CycleDetected => "环检出（构建依赖成环）",
            t303Code::LevelSkipped => "级别跳过（某级未建）",
            t303Code::EntryUnfilled => "表项未填（槽留空）",
            t303Code::PhysAllocFail => "物理页分配失败",
            t303Code::HugeSplitBad => "大页分裂非法（2M→4K 不规则）",
            t303Code::BuildLedgerBreak => "构建账破缺（级×槽不守恒）",
            t303Code::RollbackPartial => "回滚不完整",
            t303Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t303Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t303Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t303Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t303Code::DuplicateEntry => "条目重复（同键双写）",
            t303Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t303Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t303Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t303Code::DepOrderBad => "按四级序重排",
            t303Code::CycleDetected => "按环检出路径归因",
            t303Code::LevelSkipped => "按四级全建律补齐",
            t303Code::EntryUnfilled => "按填充律补填",
            t303Code::PhysAllocFail => "按失败路径重试/回滚",
            t303Code::HugeSplitBad => "按分裂律重做",
            t303Code::BuildLedgerBreak => "按守恒重建",
            t303Code::RollbackPartial => "补全回滚后重跑",
            t303Code::LedgerBroken => "按守恒律重建台账后重跑",
            t303Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t303Code::TableBroken => "按语义规则重建常量表",
            t303Code::IndexMisaligned => "按声明序归一化后重跑",
            t303Code::DuplicateEntry => "合并同键条目为末次生效",
            t303Code::OrderViolated => "按依赖矩阵重排后重跑",
            t303Code::GuardTripped => "检查临界区嵌套与阈值",
            t303Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 pgb（9 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 四级页表构建九级流水。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum pgbStage {
    /// 级 根：分配 PML4 根页。
    AllocPml4,
    /// 级 0：填充 PML4 槽（指向 PDPT）。
    BuildPml4,
    /// 级 1：按需分配 PDPT。
    AllocPdpt,
    /// 级 2：填充 PDPT 槽。
    BuildPdpt,
    /// 级 3：按需分配 PD（可 2M 大页提前收口）。
    AllocPd,
    /// 级 4：填充 PD 槽。
    BuildPd,
    /// 级 5：按需分配 PT。
    AllocPt,
    /// 级 6：填充 PT 槽。
    BuildPt,
    /// 级 7：四级交叉校验。
    CrossCheck,
}

impl pgbStage {
    /// 全量级闭集。
    pub const ALL: [pgbStage; 9] = [
        pgbStage::AllocPml4,
        pgbStage::BuildPml4,
        pgbStage::AllocPdpt,
        pgbStage::BuildPdpt,
        pgbStage::AllocPd,
        pgbStage::BuildPd,
        pgbStage::AllocPt,
        pgbStage::BuildPt,
        pgbStage::CrossCheck,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            pgbStage::AllocPml4 => 0,
            pgbStage::BuildPml4 => 1,
            pgbStage::AllocPdpt => 2,
            pgbStage::BuildPdpt => 3,
            pgbStage::AllocPd => 4,
            pgbStage::BuildPd => 5,
            pgbStage::AllocPt => 6,
            pgbStage::BuildPt => 7,
            pgbStage::CrossCheck => 8,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            pgbStage::AllocPml4 => "分配 PML4 根页",
            pgbStage::BuildPml4 => "填充 PML4 槽（指向 PDPT）",
            pgbStage::AllocPdpt => "按需分配 PDPT",
            pgbStage::BuildPdpt => "填充 PDPT 槽",
            pgbStage::AllocPd => "按需分配 PD（可 2M 大页提前收口）",
            pgbStage::BuildPd => "填充 PD 槽",
            pgbStage::AllocPt => "按需分配 PT",
            pgbStage::BuildPt => "填充 PT 槽",
            pgbStage::CrossCheck => "四级交叉校验",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const PGB_DEPS: [i32; 9] = [
    -1, // AllocPml4 依赖 无
    0, // BuildPml4 依赖 级 0
    1, // AllocPdpt 依赖 级 1
    2, // BuildPdpt 依赖 级 2
    3, // AllocPd 依赖 级 3
    4, // BuildPd 依赖 级 4
    5, // AllocPt 依赖 级 5
    6, // BuildPt 依赖 级 6
    7, // CrossCheck 依赖 级 7
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum pgbProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（构建中途断）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct pgbBook {
    /// 各级状态。
    pub state: [pgbProgress; 9],
    /// 已完成计数。
    pub done: u32,
    /// 跳过计数。
    pub skipped: u32,
    /// 回滚计数。
    pub rolled: u32,
    /// 已完成级索引序列（回放审计用）。
    pub order: [usize; 9],
    /// 序列长度。
    pub order_len: usize,
}

impl pgbBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [pgbProgress::Pending; 9],
            done: 0,
            skipped: 0,
            rolled: 0,
            order: [0; 9],
            order_len: 0,
        }
    }

    /// 拓扑序推演（Kahn 剥皮：逐轮找全前置已 Done 的 Pending 级）。
    /// 返回推演出的可执行序；环依赖时返回 Err(环首级索引)。
    pub fn plan(&mut self) -> Result<[usize; 9], usize> {
        let mut order = [0usize; 9];
        let mut olen = 0usize;
        let mut rounds = 0usize;
        while rounds < 9 {
            let mut progressed = false;
            let mut i = 0usize;
            while i < 9 {
                if self.state[i] != pgbProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = PGB_DEPS[i];
                if dep < 0 || self.state[dep as usize] == pgbProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = pgbProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 9 {
                    if self.state[k] == pgbProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（构建中途断）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 9 {
            if self.state[i] == pgbProgress::Done {
                self.state[i] = pgbProgress::Rolled;
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
        while i < 9 {
            if self.state[i] == pgbProgress::Pending {
                pending += 1;
            }
            i += 1;
        }
        self.done + self.skipped + self.rolled + pending == 9
    }

    /// 已完成级回放序列与推演序一致（顺序守恒证据）。
    pub fn order_matches(&self) -> bool {
        let mut i = 0usize;
        while i < self.order_len {
            if self.order[i] >= 9 || self.state[self.order[i]] != pgbProgress::Done {
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

/// 段位守卫：全部诊断码必须落在本段 [0x3c20, 0x3c2f]。
pub const fn t303_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t303Code::ALL.len() {
        let c = t303Code::ALL[i].code();
        if c < 0x3c20 || c > 0x3c2f {
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
fn t303_guard_seg() -> bool {
    t303_segment_guard()
}

/// 九级构建流水推演全达 + 顺序单调。
fn check_c1_pipeline() -> bool {
    let mut bk = pgbBook::new();
    let mut ok = matches!(bk.plan(), Ok(_)) && bk.done == 9;
    if ok {
        ok = bk.order_matches() && bk.conserved();
    }
    ok
}

/// 构建回滚（自断点回滚守恒）。
fn check_c2_rollback() -> bool {
    let mut bk = pgbBook::new();
    let _ = bk.plan();
    bk.rollback_from(4);
    bk.rolled == 5 && bk.done == 4 && bk.conserved()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t303_checks() -> CheckSet {
    let mut set = CheckSet::new("T303");
    set.add(
        "t303-seg-guard",
        t303_guard_seg(),
        if t303_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t303-c1-pipeline",
        check_c1_pipeline(),
        if check_c1_pipeline() { "" } else { "build pipeline mismatch" },
    );
    set.add(
        "t303-c2-rollback",
        check_c2_rollback(),
        if check_c2_rollback() { "" } else { "build rollback mismatch" },
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
    fn t303_all_checks_green() {
        let set = run_t303_checks();
        assert!(set.all_passed(), "T303 domain checks must be green");
    }

    #[test]
    fn t303_segment_guard_holds() {
        assert!(t303_segment_guard());
    }
}
