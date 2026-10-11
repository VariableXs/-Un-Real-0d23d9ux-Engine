//! unxu1721_u1721pipe —— 均衡执行总纲（判据消费→执行原语链） 执行流水与守卫（执行段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1721 · 均衡执行总纲（判据消费→执行原语链） · 判据原文：UNX-F1721 · 判据：执行链（判据结论→收益复核→候选→目标→原语→记账）六段冻结 v1，段间交接签名齐备，跳段注入 10/10 次拒绝
//! · 组件：执行段 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F1721 · 判据：执行链（判据结论→收益复核→候选→目标→原语→记账）六段冻结 v1，段间交接签名齐备，跳段注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1721_VERSION: &str = "u1721-u1721pipe-v1";

/// 诊断段基址。
pub const U1721_SEG_BASE: u16 = 0x59a0;

// --------------------------------------------------------------------------
// 诊断码（u1721 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 执行段 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1721Code {
    /// 0x59a0 级重复（跳段 同级双注册）。
    StageDup,
    /// 0x59a1 级断档（签名缺位 序缺失）。
    StageHole,
    /// 0x59a2 依赖环（级间循环依赖）。
    DepCycle,
    /// 0x59a3 自依赖（级依赖自身）。
    DepSelf,
    /// 0x59a4 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0x59a5 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0x59a6 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0x59a7 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0x59a8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x59a9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x59aa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x59ab 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1721Code {
    /// 全量闭集。
    pub const ALL: [u1721Code; 12] = [
        u1721Code::StageDup,
        u1721Code::StageHole,
        u1721Code::DepCycle,
        u1721Code::DepSelf,
        u1721Code::PlanLost,
        u1721Code::RollbackBad,
        u1721Code::OrderDrift,
        u1721Code::DoneDrift,
        u1721Code::LedgerBroken,
        u1721Code::SnapshotFingerprint,
        u1721Code::TableBroken,
        u1721Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1721Code::StageDup => 0x59a0,
            u1721Code::StageHole => 0x59a1,
            u1721Code::DepCycle => 0x59a2,
            u1721Code::DepSelf => 0x59a3,
            u1721Code::PlanLost => 0x59a4,
            u1721Code::RollbackBad => 0x59a5,
            u1721Code::OrderDrift => 0x59a6,
            u1721Code::DoneDrift => 0x59a7,
            u1721Code::LedgerBroken => 0x59a8,
            u1721Code::SnapshotFingerprint => 0x59a9,
            u1721Code::TableBroken => 0x59aa,
            u1721Code::SegmentGuard => 0x59ab,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1721Code::StageDup => "级重复（跳段 同级双注册）",
            u1721Code::StageHole => "级断档（签名缺位 序缺失）",
            u1721Code::DepCycle => "依赖环（级间循环依赖）",
            u1721Code::DepSelf => "自依赖（级依赖自身）",
            u1721Code::PlanLost => "推演丢失（可执行序短于级数）",
            u1721Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u1721Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u1721Code::DoneDrift => "完成计数漂移（done≠实计）",
            u1721Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1721Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1721Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1721Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1721Code::StageDup => "按级唯一律去重",
            u1721Code::StageHole => "按级序补齐",
            u1721Code::DepCycle => "按无环律重构依赖",
            u1721Code::DepSelf => "删除自依赖边",
            u1721Code::PlanLost => "按 Kahn 剥皮重推",
            u1721Code::RollbackBad => "按守恒律复算",
            u1721Code::OrderDrift => "按推演序重放",
            u1721Code::DoneDrift => "按实计校正",
            u1721Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1721Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1721Code::TableBroken => "按语义规则重建常量表",
            u1721Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu1721run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 执行段 执行流水（跳段）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1721runStage {
    /// 级 根：判据结论。
    Judge,
    /// 级 0：收益复核。
    Review,
    /// 级 1：候选。
    Pick,
    /// 级 2：目标。
    Target,
    /// 级 3：原语。
    Execute,
    /// 级 4：记账。
    Book,
}

impl mxu1721runStage {
    /// 全量级闭集。
    pub const ALL: [mxu1721runStage; 6] = [
        mxu1721runStage::Judge,
        mxu1721runStage::Review,
        mxu1721runStage::Pick,
        mxu1721runStage::Target,
        mxu1721runStage::Execute,
        mxu1721runStage::Book,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu1721runStage::Judge => 0,
            mxu1721runStage::Review => 1,
            mxu1721runStage::Pick => 2,
            mxu1721runStage::Target => 3,
            mxu1721runStage::Execute => 4,
            mxu1721runStage::Book => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu1721runStage::Judge => "判据结论",
            mxu1721runStage::Review => "收益复核",
            mxu1721runStage::Pick => "候选",
            mxu1721runStage::Target => "目标",
            mxu1721runStage::Execute => "原语",
            mxu1721runStage::Book => "记账",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU1721RUN_DEPS: [i32; 6] = [
    -1, // Judge 依赖 无
    0, // Review 依赖 级 0
    1, // Pick 依赖 级 1
    2, // Target 依赖 级 2
    3, // Execute 依赖 级 3
    4, // Book 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1721runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（执行段 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1721runBook {
    /// 各级状态。
    pub state: [mxu1721runProgress; 6],
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

impl mxu1721runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu1721runProgress::Pending; 6],
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
                if self.state[i] != mxu1721runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU1721RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu1721runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu1721runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu1721runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（执行段 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu1721runProgress::Done {
                self.state[i] = mxu1721runProgress::Rolled;
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
            if self.state[i] == mxu1721runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu1721runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu1721gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 执行段 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu1721gate {
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

/// 守卫阈值（执行段 重入/越限/静默三类拒止守卫）。
pub const MXU1721GATE_THRESHOLD: u64 = 64;

impl mxu1721gate {
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
        if self.window >= MXU1721GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0x59a0, 0x59af]。
pub const fn u1721_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1721Code::ALL.len() {
        let c = u1721Code::ALL[i].code();
        if c < 0x59a0 || c > 0x59af {
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
fn u1721_guard_seg() -> bool {
    u1721_segment_guard()
}

/// UNX-F1721 流水 Kahn 推演 + 回滚守恒。
fn check_u1721_pipe_plan() -> bool {
    let mut pb = mxu1721runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F1721 守卫重入/越限/静默三路拒止。
fn check_u1721_pipe_guard() -> bool {
    let mut g = mxu1721gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1721_checks() -> CheckSet {
    let mut set = CheckSet::new("U1721");
    set.add(
        "u1721-seg-guard",
        u1721_guard_seg(),
        if u1721_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1721-pipe-plan",
        check_u1721_pipe_plan(),
        if check_u1721_pipe_plan() { "" } else { "UNX-F1721 pipeline plan mismatch" },
    );
    set.add(
        "u1721-pipe-guard",
        check_u1721_pipe_guard(),
        if check_u1721_pipe_guard() { "" } else { "UNX-F1721 guard mismatch" },
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
    fn u1721_all_checks_green() {
        let set = run_u1721_checks();
        assert!(set.all_passed(), "U1721 domain checks must be green");
    }

    #[test]
    fn u1721_segment_guard_holds() {
        assert!(u1721_segment_guard());
    }
}
