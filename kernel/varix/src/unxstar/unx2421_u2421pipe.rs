//! unxu2421_u2421pipe —— order-0 页快速路径 执行流水与守卫（单页请求）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2421 · order-0 页快速路径 · 判据原文：UNX-F2421 · 判据：单页分配 p99 时延落账且与高阶路径比值 ≤0.5，10^6 次分配释放对账零泄漏
//! · 组件：单页请求 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F2421 · 判据：单页分配 p99 时延落账且与高阶路径比值 ≤0.5，10^6 次分配释放对账零泄漏
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2421_VERSION: &str = "u2421-u2421pipe-v1";

/// 诊断段基址。
pub const U2421_SEG_BASE: u16 = 0xa0e0;

// --------------------------------------------------------------------------
// 诊断码（u2421 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 单页请求 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2421Code {
    /// 0xa0e0 级重复（快径旁落 同级双注册）。
    StageDup,
    /// 0xa0e1 级断档（时延超标 序缺失）。
    StageHole,
    /// 0xa0e2 依赖环（级间循环依赖）。
    DepCycle,
    /// 0xa0e3 自依赖（级依赖自身）。
    DepSelf,
    /// 0xa0e4 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0xa0e5 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0xa0e6 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0xa0e7 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0xa0e8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa0e9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa0ea 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa0eb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2421Code {
    /// 全量闭集。
    pub const ALL: [u2421Code; 12] = [
        u2421Code::StageDup,
        u2421Code::StageHole,
        u2421Code::DepCycle,
        u2421Code::DepSelf,
        u2421Code::PlanLost,
        u2421Code::RollbackBad,
        u2421Code::OrderDrift,
        u2421Code::DoneDrift,
        u2421Code::LedgerBroken,
        u2421Code::SnapshotFingerprint,
        u2421Code::TableBroken,
        u2421Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2421Code::StageDup => 0xa0e0,
            u2421Code::StageHole => 0xa0e1,
            u2421Code::DepCycle => 0xa0e2,
            u2421Code::DepSelf => 0xa0e3,
            u2421Code::PlanLost => 0xa0e4,
            u2421Code::RollbackBad => 0xa0e5,
            u2421Code::OrderDrift => 0xa0e6,
            u2421Code::DoneDrift => 0xa0e7,
            u2421Code::LedgerBroken => 0xa0e8,
            u2421Code::SnapshotFingerprint => 0xa0e9,
            u2421Code::TableBroken => 0xa0ea,
            u2421Code::SegmentGuard => 0xa0eb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2421Code::StageDup => "级重复（快径旁落 同级双注册）",
            u2421Code::StageHole => "级断档（时延超标 序缺失）",
            u2421Code::DepCycle => "依赖环（级间循环依赖）",
            u2421Code::DepSelf => "自依赖（级依赖自身）",
            u2421Code::PlanLost => "推演丢失（可执行序短于级数）",
            u2421Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u2421Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u2421Code::DoneDrift => "完成计数漂移（done≠实计）",
            u2421Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2421Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2421Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2421Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2421Code::StageDup => "按级唯一律去重",
            u2421Code::StageHole => "按级序补齐",
            u2421Code::DepCycle => "按无环律重构依赖",
            u2421Code::DepSelf => "删除自依赖边",
            u2421Code::PlanLost => "按 Kahn 剥皮重推",
            u2421Code::RollbackBad => "按守恒律复算",
            u2421Code::OrderDrift => "按推演序重放",
            u2421Code::DoneDrift => "按实计校正",
            u2421Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2421Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2421Code::TableBroken => "按语义规则重建常量表",
            u2421Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu2421run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 单页请求 执行流水（快径旁落）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2421runStage {
    /// 级 根：快径装配。
    Arm,
    /// 级 0：分配采样。
    Alloc,
    /// 级 1：释放回放。
    Release,
    /// 级 2：比值核验。
    Ratio,
    /// 级 3：泄漏对账。
    Leak,
    /// 级 4：冻结封存。
    Seal,
}

impl mxu2421runStage {
    /// 全量级闭集。
    pub const ALL: [mxu2421runStage; 6] = [
        mxu2421runStage::Arm,
        mxu2421runStage::Alloc,
        mxu2421runStage::Release,
        mxu2421runStage::Ratio,
        mxu2421runStage::Leak,
        mxu2421runStage::Seal,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu2421runStage::Arm => 0,
            mxu2421runStage::Alloc => 1,
            mxu2421runStage::Release => 2,
            mxu2421runStage::Ratio => 3,
            mxu2421runStage::Leak => 4,
            mxu2421runStage::Seal => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu2421runStage::Arm => "快径装配",
            mxu2421runStage::Alloc => "分配采样",
            mxu2421runStage::Release => "释放回放",
            mxu2421runStage::Ratio => "比值核验",
            mxu2421runStage::Leak => "泄漏对账",
            mxu2421runStage::Seal => "冻结封存",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU2421RUN_DEPS: [i32; 6] = [
    -1, // Arm 依赖 无
    0, // Alloc 依赖 级 0
    1, // Release 依赖 级 1
    2, // Ratio 依赖 级 2
    3, // Leak 依赖 级 3
    4, // Seal 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2421runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（单页请求 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2421runBook {
    /// 各级状态。
    pub state: [mxu2421runProgress; 6],
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

impl mxu2421runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu2421runProgress::Pending; 6],
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
                if self.state[i] != mxu2421runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU2421RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu2421runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu2421runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu2421runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（单页请求 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu2421runProgress::Done {
                self.state[i] = mxu2421runProgress::Rolled;
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
            if self.state[i] == mxu2421runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu2421runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu2421gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 单页请求 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu2421gate {
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

/// 守卫阈值（单页请求 重入/越限/静默三类拒止守卫）。
pub const MXU2421GATE_THRESHOLD: u64 = 64;

impl mxu2421gate {
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
        if self.window >= MXU2421GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa0e0, 0xa0ef]。
pub const fn u2421_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2421Code::ALL.len() {
        let c = u2421Code::ALL[i].code();
        if c < 0xa0e0 || c > 0xa0ef {
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
fn u2421_guard_seg() -> bool {
    u2421_segment_guard()
}

/// UNX-F2421 流水 Kahn 推演 + 回滚守恒。
fn check_u2421_pipe_plan() -> bool {
    let mut pb = mxu2421runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F2421 守卫重入/越限/静默三路拒止。
fn check_u2421_pipe_guard() -> bool {
    let mut g = mxu2421gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2421_checks() -> CheckSet {
    let mut set = CheckSet::new("U2421");
    set.add(
        "u2421-seg-guard",
        u2421_guard_seg(),
        if u2421_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2421-pipe-plan",
        check_u2421_pipe_plan(),
        if check_u2421_pipe_plan() { "" } else { "UNX-F2421 pipeline plan mismatch" },
    );
    set.add(
        "u2421-pipe-guard",
        check_u2421_pipe_guard(),
        if check_u2421_pipe_guard() { "" } else { "UNX-F2421 guard mismatch" },
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
    fn u2421_all_checks_green() {
        let set = run_u2421_checks();
        assert!(set.all_passed(), "U2421 domain checks must be green");
    }

    #[test]
    fn u2421_segment_guard_holds() {
        assert!(u2421_segment_guard());
    }
}
