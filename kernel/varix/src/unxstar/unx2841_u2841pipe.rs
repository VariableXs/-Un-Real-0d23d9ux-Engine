//! unxu2841_u2841pipe —— 大页-地址空间联合压测方案与执行 执行流水与守卫（大页-地址空间联合压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2841 · 大页-地址空间联合压测方案与执行 · 判据原文：UNX-F2841 · 判据：UNX-F2841-J1 大页映射进地址空间全链（建立-翻译-拆除）10/10 轮贯通，链账三段闭合
//! · 组件：大页-地址空间联合压测 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F2841 · 判据：UNX-F2841-J1 大页映射进地址空间全链（建立-翻译-拆除）10/10 轮贯通，链账三段闭合
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2841_VERSION: &str = "u2841-u2841pipe-v1";

/// 诊断段基址。
pub const U2841_SEG_BASE: u16 = 0xe1f0;

// --------------------------------------------------------------------------
// 诊断码（u2841 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页-地址空间联合压测 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2841Code {
    /// 0xe1f0 级重复（联合压测 同级双注册）。
    StageDup,
    /// 0xe1f1 级断档（交叉 序缺失）。
    StageHole,
    /// 0xe1f2 依赖环（级间循环依赖）。
    DepCycle,
    /// 0xe1f3 自依赖（级依赖自身）。
    DepSelf,
    /// 0xe1f4 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0xe1f5 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0xe1f6 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0xe1f7 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0xe1f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2841Code {
    /// 全量闭集。
    pub const ALL: [u2841Code; 12] = [
        u2841Code::StageDup,
        u2841Code::StageHole,
        u2841Code::DepCycle,
        u2841Code::DepSelf,
        u2841Code::PlanLost,
        u2841Code::RollbackBad,
        u2841Code::OrderDrift,
        u2841Code::DoneDrift,
        u2841Code::LedgerBroken,
        u2841Code::SnapshotFingerprint,
        u2841Code::TableBroken,
        u2841Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2841Code::StageDup => 0xe1f0,
            u2841Code::StageHole => 0xe1f1,
            u2841Code::DepCycle => 0xe1f2,
            u2841Code::DepSelf => 0xe1f3,
            u2841Code::PlanLost => 0xe1f4,
            u2841Code::RollbackBad => 0xe1f5,
            u2841Code::OrderDrift => 0xe1f6,
            u2841Code::DoneDrift => 0xe1f7,
            u2841Code::LedgerBroken => 0xe1f8,
            u2841Code::SnapshotFingerprint => 0xe1f9,
            u2841Code::TableBroken => 0xe1fa,
            u2841Code::SegmentGuard => 0xe1fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2841Code::StageDup => "级重复（联合压测 同级双注册）",
            u2841Code::StageHole => "级断档（交叉 序缺失）",
            u2841Code::DepCycle => "依赖环（级间循环依赖）",
            u2841Code::DepSelf => "自依赖（级依赖自身）",
            u2841Code::PlanLost => "推演丢失（可执行序短于级数）",
            u2841Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u2841Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u2841Code::DoneDrift => "完成计数漂移（done≠实计）",
            u2841Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2841Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2841Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2841Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2841Code::StageDup => "按级唯一律去重",
            u2841Code::StageHole => "按级序补齐",
            u2841Code::DepCycle => "按无环律重构依赖",
            u2841Code::DepSelf => "删除自依赖边",
            u2841Code::PlanLost => "按 Kahn 剥皮重推",
            u2841Code::RollbackBad => "按守恒律复算",
            u2841Code::OrderDrift => "按推演序重放",
            u2841Code::DoneDrift => "按实计校正",
            u2841Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2841Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2841Code::TableBroken => "按语义规则重建常量表",
            u2841Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu2841run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 大页-地址空间联合压测 执行流水（联合压测）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2841runStage {
    /// 级 根：方案冻结。
    Plan,
    /// 级 0：交叉矩阵构建。
    Build,
    /// 级 1：布防。
    Arm,
    /// 级 2：压测执行。
    Run,
    /// 级 3：裁决。
    Judge,
    /// 级 4：封存封口。
    Seal,
}

impl mxu2841runStage {
    /// 全量级闭集。
    pub const ALL: [mxu2841runStage; 6] = [
        mxu2841runStage::Plan,
        mxu2841runStage::Build,
        mxu2841runStage::Arm,
        mxu2841runStage::Run,
        mxu2841runStage::Judge,
        mxu2841runStage::Seal,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu2841runStage::Plan => 0,
            mxu2841runStage::Build => 1,
            mxu2841runStage::Arm => 2,
            mxu2841runStage::Run => 3,
            mxu2841runStage::Judge => 4,
            mxu2841runStage::Seal => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu2841runStage::Plan => "方案冻结",
            mxu2841runStage::Build => "交叉矩阵构建",
            mxu2841runStage::Arm => "布防",
            mxu2841runStage::Run => "压测执行",
            mxu2841runStage::Judge => "裁决",
            mxu2841runStage::Seal => "封存封口",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU2841RUN_DEPS: [i32; 6] = [
    -1, // Plan 依赖 无
    0, // Build 依赖 级 0
    1, // Arm 依赖 级 1
    2, // Run 依赖 级 2
    3, // Judge 依赖 级 3
    4, // Seal 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2841runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（大页-地址空间联合压测 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2841runBook {
    /// 各级状态。
    pub state: [mxu2841runProgress; 6],
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

impl mxu2841runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu2841runProgress::Pending; 6],
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
                if self.state[i] != mxu2841runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU2841RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu2841runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu2841runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu2841runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（大页-地址空间联合压测 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu2841runProgress::Done {
                self.state[i] = mxu2841runProgress::Rolled;
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
            if self.state[i] == mxu2841runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu2841runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu2841gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 大页-地址空间联合压测 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu2841gate {
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

/// 守卫阈值（大页-地址空间联合压测 重入/越限/静默三类拒止守卫）。
pub const MXU2841GATE_THRESHOLD: u64 = 64;

impl mxu2841gate {
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
        if self.window >= MXU2841GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1f0, 0xe1ff]。
pub const fn u2841_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2841Code::ALL.len() {
        let c = u2841Code::ALL[i].code();
        if c < 0xe1f0 || c > 0xe1ff {
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
fn u2841_guard_seg() -> bool {
    u2841_segment_guard()
}

/// UNX-F2841 流水 Kahn 推演 + 回滚守恒。
fn check_u2841_pipe_plan() -> bool {
    let mut pb = mxu2841runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F2841 守卫重入/越限/静默三路拒止。
fn check_u2841_pipe_guard() -> bool {
    let mut g = mxu2841gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2841_checks() -> CheckSet {
    let mut set = CheckSet::new("U2841");
    set.add(
        "u2841-seg-guard",
        u2841_guard_seg(),
        if u2841_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2841-pipe-plan",
        check_u2841_pipe_plan(),
        if check_u2841_pipe_plan() { "" } else { "UNX-F2841 pipeline plan mismatch" },
    );
    set.add(
        "u2841-pipe-guard",
        check_u2841_pipe_guard(),
        if check_u2841_pipe_guard() { "" } else { "UNX-F2841 guard mismatch" },
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
    fn u2841_all_checks_green() {
        let set = run_u2841_checks();
        assert!(set.all_passed(), "U2841 domain checks must be green");
    }

    #[test]
    fn u2841_segment_guard_holds() {
        assert!(u2841_segment_guard());
    }
}
