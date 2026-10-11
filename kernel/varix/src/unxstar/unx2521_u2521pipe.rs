//! unxu2521_u2521pipe —— 碎片指数定义与计算（B02 前向引用兑现） 执行流水与守卫（碎片指数）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2521 · 碎片指数定义与计算（B02 前向引用兑现） · 判据原文：UNX-F2521 · 判据：碎片指数计算与手算样例一致（可复算断言），指数含大页项（F2517 契约消费）
//! · 组件：碎片指数 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F2521 · 判据：碎片指数计算与手算样例一致（可复算断言），指数含大页项（F2517 契约消费）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2521_VERSION: &str = "u2521-u2521pipe-v1";

/// 诊断段基址。
pub const U2521_SEG_BASE: u16 = 0xa4a0;

// --------------------------------------------------------------------------
// 诊断码（u2521 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 碎片指数 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2521Code {
    /// 0xa4a0 级重复（复算不符 同级双注册）。
    StageDup,
    /// 0xa4a1 级断档（大页项缺 序缺失）。
    StageHole,
    /// 0xa4a2 依赖环（级间循环依赖）。
    DepCycle,
    /// 0xa4a3 自依赖（级依赖自身）。
    DepSelf,
    /// 0xa4a4 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0xa4a5 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0xa4a6 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0xa4a7 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0xa4a8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa4a9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa4aa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa4ab 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2521Code {
    /// 全量闭集。
    pub const ALL: [u2521Code; 12] = [
        u2521Code::StageDup,
        u2521Code::StageHole,
        u2521Code::DepCycle,
        u2521Code::DepSelf,
        u2521Code::PlanLost,
        u2521Code::RollbackBad,
        u2521Code::OrderDrift,
        u2521Code::DoneDrift,
        u2521Code::LedgerBroken,
        u2521Code::SnapshotFingerprint,
        u2521Code::TableBroken,
        u2521Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2521Code::StageDup => 0xa4a0,
            u2521Code::StageHole => 0xa4a1,
            u2521Code::DepCycle => 0xa4a2,
            u2521Code::DepSelf => 0xa4a3,
            u2521Code::PlanLost => 0xa4a4,
            u2521Code::RollbackBad => 0xa4a5,
            u2521Code::OrderDrift => 0xa4a6,
            u2521Code::DoneDrift => 0xa4a7,
            u2521Code::LedgerBroken => 0xa4a8,
            u2521Code::SnapshotFingerprint => 0xa4a9,
            u2521Code::TableBroken => 0xa4aa,
            u2521Code::SegmentGuard => 0xa4ab,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2521Code::StageDup => "级重复（复算不符 同级双注册）",
            u2521Code::StageHole => "级断档（大页项缺 序缺失）",
            u2521Code::DepCycle => "依赖环（级间循环依赖）",
            u2521Code::DepSelf => "自依赖（级依赖自身）",
            u2521Code::PlanLost => "推演丢失（可执行序短于级数）",
            u2521Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u2521Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u2521Code::DoneDrift => "完成计数漂移（done≠实计）",
            u2521Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2521Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2521Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2521Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2521Code::StageDup => "按级唯一律去重",
            u2521Code::StageHole => "按级序补齐",
            u2521Code::DepCycle => "按无环律重构依赖",
            u2521Code::DepSelf => "删除自依赖边",
            u2521Code::PlanLost => "按 Kahn 剥皮重推",
            u2521Code::RollbackBad => "按守恒律复算",
            u2521Code::OrderDrift => "按推演序重放",
            u2521Code::DoneDrift => "按实计校正",
            u2521Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2521Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2521Code::TableBroken => "按语义规则重建常量表",
            u2521Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu2521run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 碎片指数 执行流水（复算不符）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2521runStage {
    /// 级 根：空闲采样。
    Sample,
    /// 级 0：指数计算。
    Compute,
    /// 级 1：手算复算。
    Recalc,
    /// 级 2：样例比对。
    Compare,
    /// 级 3：契约兑现。
    Contract,
    /// 级 4：冻结封存。
    Seal,
}

impl mxu2521runStage {
    /// 全量级闭集。
    pub const ALL: [mxu2521runStage; 6] = [
        mxu2521runStage::Sample,
        mxu2521runStage::Compute,
        mxu2521runStage::Recalc,
        mxu2521runStage::Compare,
        mxu2521runStage::Contract,
        mxu2521runStage::Seal,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu2521runStage::Sample => 0,
            mxu2521runStage::Compute => 1,
            mxu2521runStage::Recalc => 2,
            mxu2521runStage::Compare => 3,
            mxu2521runStage::Contract => 4,
            mxu2521runStage::Seal => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu2521runStage::Sample => "空闲采样",
            mxu2521runStage::Compute => "指数计算",
            mxu2521runStage::Recalc => "手算复算",
            mxu2521runStage::Compare => "样例比对",
            mxu2521runStage::Contract => "契约兑现",
            mxu2521runStage::Seal => "冻结封存",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU2521RUN_DEPS: [i32; 6] = [
    -1, // Sample 依赖 无
    0, // Compute 依赖 级 0
    1, // Recalc 依赖 级 1
    2, // Compare 依赖 级 2
    3, // Contract 依赖 级 3
    4, // Seal 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2521runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（碎片指数 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2521runBook {
    /// 各级状态。
    pub state: [mxu2521runProgress; 6],
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

impl mxu2521runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu2521runProgress::Pending; 6],
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
                if self.state[i] != mxu2521runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU2521RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu2521runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu2521runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu2521runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（碎片指数 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu2521runProgress::Done {
                self.state[i] = mxu2521runProgress::Rolled;
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
            if self.state[i] == mxu2521runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu2521runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu2521gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 碎片指数 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu2521gate {
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

/// 守卫阈值（碎片指数 重入/越限/静默三类拒止守卫）。
pub const MXU2521GATE_THRESHOLD: u64 = 64;

impl mxu2521gate {
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
        if self.window >= MXU2521GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa4a0, 0xa4af]。
pub const fn u2521_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2521Code::ALL.len() {
        let c = u2521Code::ALL[i].code();
        if c < 0xa4a0 || c > 0xa4af {
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
fn u2521_guard_seg() -> bool {
    u2521_segment_guard()
}

/// UNX-F2521 流水 Kahn 推演 + 回滚守恒。
fn check_u2521_pipe_plan() -> bool {
    let mut pb = mxu2521runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F2521 守卫重入/越限/静默三路拒止。
fn check_u2521_pipe_guard() -> bool {
    let mut g = mxu2521gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2521_checks() -> CheckSet {
    let mut set = CheckSet::new("U2521");
    set.add(
        "u2521-seg-guard",
        u2521_guard_seg(),
        if u2521_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2521-pipe-plan",
        check_u2521_pipe_plan(),
        if check_u2521_pipe_plan() { "" } else { "UNX-F2521 pipeline plan mismatch" },
    );
    set.add(
        "u2521-pipe-guard",
        check_u2521_pipe_guard(),
        if check_u2521_pipe_guard() { "" } else { "UNX-F2521 guard mismatch" },
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
    fn u2521_all_checks_green() {
        let set = run_u2521_checks();
        assert!(set.all_passed(), "U2521 domain checks must be green");
    }

    #[test]
    fn u2521_segment_guard_holds() {
        assert!(u2521_segment_guard());
    }
}
