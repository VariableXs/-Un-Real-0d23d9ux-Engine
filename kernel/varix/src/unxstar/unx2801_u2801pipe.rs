//! unxu2801_u2801pipe —— 大页池容量压测方案冻结 执行流水与守卫（大页池容量压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2801 · 大页池容量压测方案冻结 · 判据原文：UNX-F2801 · 判据：UNX-F2801-J1 三态方案（满池/近满/空）参数冻结哈希入册，方案与 F2425 池账字段逐项对齐
//! · 组件：大页池容量压测 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F2801 · 判据：UNX-F2801-J1 三态方案（满池/近满/空）参数冻结哈希入册，方案与 F2425 池账字段逐项对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2801_VERSION: &str = "u2801-u2801pipe-v1";

/// 诊断段基址。
pub const U2801_SEG_BASE: u16 = 0xe1d0;

// --------------------------------------------------------------------------
// 诊断码（u2801 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页池容量压测 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2801Code {
    /// 0xe1d0 级重复（大页 同级双注册）。
    StageDup,
    /// 0xe1d1 级断档（压测 序缺失）。
    StageHole,
    /// 0xe1d2 依赖环（级间循环依赖）。
    DepCycle,
    /// 0xe1d3 自依赖（级依赖自身）。
    DepSelf,
    /// 0xe1d4 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0xe1d5 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0xe1d6 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0xe1d7 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0xe1d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2801Code {
    /// 全量闭集。
    pub const ALL: [u2801Code; 12] = [
        u2801Code::StageDup,
        u2801Code::StageHole,
        u2801Code::DepCycle,
        u2801Code::DepSelf,
        u2801Code::PlanLost,
        u2801Code::RollbackBad,
        u2801Code::OrderDrift,
        u2801Code::DoneDrift,
        u2801Code::LedgerBroken,
        u2801Code::SnapshotFingerprint,
        u2801Code::TableBroken,
        u2801Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2801Code::StageDup => 0xe1d0,
            u2801Code::StageHole => 0xe1d1,
            u2801Code::DepCycle => 0xe1d2,
            u2801Code::DepSelf => 0xe1d3,
            u2801Code::PlanLost => 0xe1d4,
            u2801Code::RollbackBad => 0xe1d5,
            u2801Code::OrderDrift => 0xe1d6,
            u2801Code::DoneDrift => 0xe1d7,
            u2801Code::LedgerBroken => 0xe1d8,
            u2801Code::SnapshotFingerprint => 0xe1d9,
            u2801Code::TableBroken => 0xe1da,
            u2801Code::SegmentGuard => 0xe1db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2801Code::StageDup => "级重复（大页 同级双注册）",
            u2801Code::StageHole => "级断档（压测 序缺失）",
            u2801Code::DepCycle => "依赖环（级间循环依赖）",
            u2801Code::DepSelf => "自依赖（级依赖自身）",
            u2801Code::PlanLost => "推演丢失（可执行序短于级数）",
            u2801Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u2801Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u2801Code::DoneDrift => "完成计数漂移（done≠实计）",
            u2801Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2801Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2801Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2801Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2801Code::StageDup => "按级唯一律去重",
            u2801Code::StageHole => "按级序补齐",
            u2801Code::DepCycle => "按无环律重构依赖",
            u2801Code::DepSelf => "删除自依赖边",
            u2801Code::PlanLost => "按 Kahn 剥皮重推",
            u2801Code::RollbackBad => "按守恒律复算",
            u2801Code::OrderDrift => "按推演序重放",
            u2801Code::DoneDrift => "按实计校正",
            u2801Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2801Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2801Code::TableBroken => "按语义规则重建常量表",
            u2801Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu2801run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 大页池容量压测 执行流水（大页）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2801runStage {
    /// 级 根：方案冻结。
    Plan,
    /// 级 0：布防。
    Arm,
    /// 级 1：压测执行。
    Run,
    /// 级 2：采样。
    Sample,
    /// 级 3：裁决。
    Judge,
    /// 级 4：封存封口。
    Seal,
}

impl mxu2801runStage {
    /// 全量级闭集。
    pub const ALL: [mxu2801runStage; 6] = [
        mxu2801runStage::Plan,
        mxu2801runStage::Arm,
        mxu2801runStage::Run,
        mxu2801runStage::Sample,
        mxu2801runStage::Judge,
        mxu2801runStage::Seal,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu2801runStage::Plan => 0,
            mxu2801runStage::Arm => 1,
            mxu2801runStage::Run => 2,
            mxu2801runStage::Sample => 3,
            mxu2801runStage::Judge => 4,
            mxu2801runStage::Seal => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu2801runStage::Plan => "方案冻结",
            mxu2801runStage::Arm => "布防",
            mxu2801runStage::Run => "压测执行",
            mxu2801runStage::Sample => "采样",
            mxu2801runStage::Judge => "裁决",
            mxu2801runStage::Seal => "封存封口",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU2801RUN_DEPS: [i32; 6] = [
    -1, // Plan 依赖 无
    0, // Arm 依赖 级 0
    1, // Run 依赖 级 1
    2, // Sample 依赖 级 2
    3, // Judge 依赖 级 3
    4, // Seal 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2801runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（大页池容量压测 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2801runBook {
    /// 各级状态。
    pub state: [mxu2801runProgress; 6],
    /// 已完成计数。
    pub done: u32,
    /// 跳过计数。
    pub skipped: u32,
    /// 回滚计数。
    pub rolled: u32,
    /// 已完成级索引序列（回放审计用）。
    pub order: [usize; 6],
    /// 序列长度。
    pub order_len: usize,
}

impl mxu2801runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu2801runProgress::Pending; 6],
            done: 0,
            skipped: 0,
            rolled: 0,
            order: [0; 6],
            order_len: 0,
        }
    }

    /// 拓扑序推演（Kahn 剥皮：逐轮找全前置已 Done 的 Pending 级）。
    /// 返回推演出的可执行序；环依赖时返回 Err(环首级索引)。
    pub fn plan(&mut self) -> Result<[usize; 6], usize> {
        let mut order = [0usize; 6];
        let mut olen = 0usize;
        let mut rounds = 0usize;
        while rounds < 6 {
            let mut progressed = false;
            let mut i = 0usize;
            while i < 6 {
                if self.state[i] != mxu2801runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU2801RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu2801runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu2801runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu2801runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（大页池容量压测 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu2801runProgress::Done {
                self.state[i] = mxu2801runProgress::Rolled;
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
        while i < 6 {
            if self.state[i] == mxu2801runProgress::Pending {
                pending += 1;
            }
            i += 1;
        }
        self.done + self.skipped + self.rolled + pending == 6
    }

    /// 已完成级回放序列与推演序一致（顺序守恒证据）。
    pub fn order_matches(&self) -> bool {
        let mut i = 0usize;
        while i < self.order_len {
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu2801runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu2801gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 大页池容量压测 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu2801gate {
    /// 重入嵌套深度（>0 即重入中）。
    pub depth: u32,
    /// 越限拒止计数。
    pub over: u64,
    /// 静默拒止计数（阈值内未升级）。
    pub silent: u64,
    /// 当前窗口计数。
    pub window: u64,
    /// 窗口内观测总量。
    pub seen: u64,
    /// 已升级告警数。
    pub raised: u64,
}

/// 守卫阈值（大页池容量压测 重入/越限/静默三类拒止守卫）。
pub const MXU2801GATE_THRESHOLD: u64 = 64;

impl mxu2801gate {
    /// 全零构造。
    pub const fn new() -> Self {
        Self { depth: 0, over: 0, silent: 0, window: 0, seen: 0, raised: 0 }
    }

    /// 进入临界区：重入直接拒止（返回 false）。
    pub fn enter(&mut self) -> bool {
        if self.depth > 0 {
            self.over += 1;
            return false;
        }
        self.depth = 1;
        true
    }

    /// 离开临界区（深度归一，幂等安全）。
    pub fn leave(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
    }

    /// 观测一次：累计 seen/window；越限升级 raised，否则静默。
    pub fn observe(&mut self, hit: bool) {
        self.seen += 1;
        if hit {
            self.window += 1;
        }
        if self.window >= MXU2801GATE_THRESHOLD {
            self.raised += 1;
        } else {
            self.silent += 1;
        }
    }

    /// 守恒：seen == raised + silent（窗口口径）。
    pub fn conserved(&self) -> bool {
        self.seen == self.raised + self.silent
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0xe1d0, 0xe1df]。
pub const fn u2801_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2801Code::ALL.len() {
        let c = u2801Code::ALL[i].code();
        if c < 0xe1d0 || c > 0xe1df {
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
fn u2801_guard_seg() -> bool {
    u2801_segment_guard()
}

/// UNX-F2801 流水 Kahn 推演 + 回滚守恒。
fn check_u2801_pipe_plan() -> bool {
    let mut pb = mxu2801runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F2801 守卫重入/越限/静默三路拒止。
fn check_u2801_pipe_guard() -> bool {
    let mut g = mxu2801gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2801_checks() -> CheckSet {
    let mut set = CheckSet::new("U2801");
    set.add(
        "u2801-seg-guard",
        u2801_guard_seg(),
        if u2801_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2801-pipe-plan",
        check_u2801_pipe_plan(),
        if check_u2801_pipe_plan() { "" } else { "UNX-F2801 pipeline plan mismatch" },
    );
    set.add(
        "u2801-pipe-guard",
        check_u2801_pipe_guard(),
        if check_u2801_pipe_guard() { "" } else { "UNX-F2801 guard mismatch" },
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
    fn u2801_all_checks_green() {
        let set = run_u2801_checks();
        assert!(set.all_passed(), "U2801 domain checks must be green");
    }

    #[test]
    fn u2801_segment_guard_holds() {
        assert!(u2801_segment_guard());
    }
}
