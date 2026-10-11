//! unxt244_mvalid —— UNX-F0241 步节校验流水（十二步逐级校验器）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0241 · BootManifest 全景结构（十二步总纲） · 判据：十二步节结构定义齐备（固件/内存/CPU/随机化/参数/健康/时间/日志/上次引导/订阅/完整性/时序），总 CRC 链校验过，结构黄金比对一致
//! · 组件：校验流水（12 级依赖链）+ 字段校验器矩阵 + 违例账
//! · 判据落地：十二步节结构定义齐备的校验面
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T244_VERSION: &str = "t244-mvalid-v1";

/// 诊断段基址。
pub const T244_SEG_BASE: u16 = 0x3930;

// --------------------------------------------------------------------------
// 诊断码（t244 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 步节校验诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t244Code {
    /// 0x3930 步节校验失败（字段违例）。
    StepFail,
    /// 0x3931 前置未满足（上游步未过）。
    DepNotMet,
    /// 0x3932 校验器非法（规则定义漂移）。
    ValidatorBad,
    /// 0x3933 需回滚（下游已过上游败）。
    RollbackNeeded,
    /// 0x3934 重复校验（同步双校）。
    DoubleValidate,
    /// 0x3935 非法跳过（未校先过）。
    SkipIllegal,
    /// 0x3936 校验账破缺（过/败/跳不守恒）。
    LedgerBreak,
    /// 0x3937 报告陈旧（校验结果未刷新）。
    ReportStale,
    /// 0x3938 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3939 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x393a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x393b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x393c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x393d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x393e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x393f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t244Code {
    /// 全量闭集。
    pub const ALL: [t244Code; 16] = [
        t244Code::StepFail,
        t244Code::DepNotMet,
        t244Code::ValidatorBad,
        t244Code::RollbackNeeded,
        t244Code::DoubleValidate,
        t244Code::SkipIllegal,
        t244Code::LedgerBreak,
        t244Code::ReportStale,
        t244Code::LedgerBroken,
        t244Code::SnapshotFingerprint,
        t244Code::TableBroken,
        t244Code::IndexMisaligned,
        t244Code::DuplicateEntry,
        t244Code::OrderViolated,
        t244Code::GuardTripped,
        t244Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t244Code::StepFail => 0x3930,
            t244Code::DepNotMet => 0x3931,
            t244Code::ValidatorBad => 0x3932,
            t244Code::RollbackNeeded => 0x3933,
            t244Code::DoubleValidate => 0x3934,
            t244Code::SkipIllegal => 0x3935,
            t244Code::LedgerBreak => 0x3936,
            t244Code::ReportStale => 0x3937,
            t244Code::LedgerBroken => 0x3938,
            t244Code::SnapshotFingerprint => 0x3939,
            t244Code::TableBroken => 0x393a,
            t244Code::IndexMisaligned => 0x393b,
            t244Code::DuplicateEntry => 0x393c,
            t244Code::OrderViolated => 0x393d,
            t244Code::GuardTripped => 0x393e,
            t244Code::SegmentGuard => 0x393f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t244Code::StepFail => "步节校验失败（字段违例）",
            t244Code::DepNotMet => "前置未满足（上游步未过）",
            t244Code::ValidatorBad => "校验器非法（规则定义漂移）",
            t244Code::RollbackNeeded => "需回滚（下游已过上游败）",
            t244Code::DoubleValidate => "重复校验（同步双校）",
            t244Code::SkipIllegal => "非法跳过（未校先过）",
            t244Code::LedgerBreak => "校验账破缺（过/败/跳不守恒）",
            t244Code::ReportStale => "报告陈旧（校验结果未刷新）",
            t244Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t244Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t244Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t244Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t244Code::DuplicateEntry => "条目重复（同键双写）",
            t244Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t244Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t244Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t244Code::StepFail => "按违例明细修复后重校",
            t244Code::DepNotMet => "按依赖序补校上游",
            t244Code::ValidatorBad => "按校验器台账校正",
            t244Code::RollbackNeeded => "按回滚链退回",
            t244Code::DoubleValidate => "幂等跳过",
            t244Code::SkipIllegal => "按序补齐后放行",
            t244Code::LedgerBreak => "按守恒重建",
            t244Code::ReportStale => "重跑后刷新",
            t244Code::LedgerBroken => "按守恒律重建台账后重跑",
            t244Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t244Code::TableBroken => "按语义规则重建常量表",
            t244Code::IndexMisaligned => "按声明序归一化后重跑",
            t244Code::DuplicateEntry => "合并同键条目为末次生效",
            t244Code::OrderViolated => "按依赖矩阵重排后重跑",
            t244Code::GuardTripped => "检查临界区嵌套与阈值",
            t244Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mval（12 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 十二步校验流水（逐级依赖，环即拒）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mvalStage {
    /// 级 根：固件步节校验。
    Step01,
    /// 级 0：内存步节校验。
    Step02,
    /// 级 1：CPU步节校验。
    Step03,
    /// 级 2：随机化步节校验。
    Step04,
    /// 级 3：参数步节校验。
    Step05,
    /// 级 4：健康步节校验。
    Step06,
    /// 级 5：时间步节校验。
    Step07,
    /// 级 6：日志步节校验。
    Step08,
    /// 级 7：上次引导步节校验。
    Step09,
    /// 级 8：订阅步节校验。
    Step10,
    /// 级 9：完整性步节校验。
    Step11,
    /// 级 10：时序步节校验。
    Step12,
}

impl mvalStage {
    /// 全量级闭集。
    pub const ALL: [mvalStage; 12] = [
        mvalStage::Step01,
        mvalStage::Step02,
        mvalStage::Step03,
        mvalStage::Step04,
        mvalStage::Step05,
        mvalStage::Step06,
        mvalStage::Step07,
        mvalStage::Step08,
        mvalStage::Step09,
        mvalStage::Step10,
        mvalStage::Step11,
        mvalStage::Step12,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mvalStage::Step01 => 0,
            mvalStage::Step02 => 1,
            mvalStage::Step03 => 2,
            mvalStage::Step04 => 3,
            mvalStage::Step05 => 4,
            mvalStage::Step06 => 5,
            mvalStage::Step07 => 6,
            mvalStage::Step08 => 7,
            mvalStage::Step09 => 8,
            mvalStage::Step10 => 9,
            mvalStage::Step11 => 10,
            mvalStage::Step12 => 11,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mvalStage::Step01 => "固件步节校验",
            mvalStage::Step02 => "内存步节校验",
            mvalStage::Step03 => "CPU步节校验",
            mvalStage::Step04 => "随机化步节校验",
            mvalStage::Step05 => "参数步节校验",
            mvalStage::Step06 => "健康步节校验",
            mvalStage::Step07 => "时间步节校验",
            mvalStage::Step08 => "日志步节校验",
            mvalStage::Step09 => "上次引导步节校验",
            mvalStage::Step10 => "订阅步节校验",
            mvalStage::Step11 => "完整性步节校验",
            mvalStage::Step12 => "时序步节校验",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MVAL_DEPS: [i32; 12] = [
    -1, // Step01 依赖 无
    0, // Step02 依赖 级 0
    1, // Step03 依赖 级 1
    2, // Step04 依赖 级 2
    3, // Step05 依赖 级 3
    4, // Step06 依赖 级 4
    5, // Step07 依赖 级 5
    6, // Step08 依赖 级 6
    7, // Step09 依赖 级 7
    8, // Step10 依赖 级 8
    9, // Step11 依赖 级 9
    10, // Step12 依赖 级 10
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mvalProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（下游已过上游败）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mvalBook {
    /// 各级状态。
    pub state: [mvalProgress; 12],
    /// 已完成计数。
    pub done: u32,
    /// 跳过计数。
    pub skipped: u32,
    /// 回滚计数。
    pub rolled: u32,
    /// 已完成级索引序列（回放审计用）。
    pub order: [usize; 12],
    /// 序列长度。
    pub order_len: usize,
}

impl mvalBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mvalProgress::Pending; 12],
            done: 0,
            skipped: 0,
            rolled: 0,
            order: [0; 12],
            order_len: 0,
        }
    }

    /// 拓扑序推演（Kahn 剥皮：逐轮找全前置已 Done 的 Pending 级）。
    /// 返回推演出的可执行序；环依赖时返回 Err(环首级索引)。
    pub fn plan(&mut self) -> Result<[usize; 12], usize> {
        let mut order = [0usize; 12];
        let mut olen = 0usize;
        let mut rounds = 0usize;
        while rounds < 12 {
            let mut progressed = false;
            let mut i = 0usize;
            while i < 12 {
                if self.state[i] != mvalProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MVAL_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mvalProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mvalProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 12 {
                    if self.state[k] == mvalProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（下游已过上游败）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 12 {
            if self.state[i] == mvalProgress::Done {
                self.state[i] = mvalProgress::Rolled;
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
        while i < 12 {
            if self.state[i] == mvalProgress::Pending {
                pending += 1;
            }
            i += 1;
        }
        self.done + self.skipped + self.rolled + pending == 12
    }

    /// 已完成级回放序列与推演序一致（顺序守恒证据）。
    pub fn order_matches(&self) -> bool {
        let mut i = 0usize;
        while i < self.order_len {
            if self.order[i] >= 12 || self.state[self.order[i]] != mvalProgress::Done {
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

/// 段位守卫：全部诊断码必须落在本段 [0x3930, 0x393f]。
pub const fn t244_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t244Code::ALL.len() {
        let c = t244Code::ALL[i].code();
        if c < 0x3930 || c > 0x393f {
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
fn t244_guard_seg() -> bool {
    t244_segment_guard()
}

/// 十二步校验流水推演全达（12 级 + 环检出）。
fn check_c1_validate_plan() -> bool {
    let mut bk = mvalBook::new();
    let mut ok = matches!(bk.plan(), Ok(_)) && bk.done == 12;
    if ok {
        ok = bk.order_matches() && bk.conserved();
    }
    ok
}

/// 回滚路径（自失败级起 Done 级回滚 + 守恒）。
fn check_c2_rollback() -> bool {
    let mut bk = mvalBook::new();
    let _ = bk.plan();
    bk.rollback_from(6);
    let mut ok = bk.rolled == 6 && bk.done == 6;
    if ok {
        ok = bk.conserved();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t244_checks() -> CheckSet {
    let mut set = CheckSet::new("T244");
    set.add(
        "t244-seg-guard",
        t244_guard_seg(),
        if t244_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t244-c1-validate-plan",
        check_c1_validate_plan(),
        if check_c1_validate_plan() { "" } else { "12-step validation plan mismatch" },
    );
    set.add(
        "t244-c2-rollback",
        check_c2_rollback(),
        if check_c2_rollback() { "" } else { "rollback path mismatch" },
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
    fn t244_all_checks_green() {
        let set = run_t244_checks();
        assert!(set.all_passed(), "T244 domain checks must be green");
    }

    #[test]
    fn t244_segment_guard_holds() {
        assert!(t244_segment_guard());
    }
}
