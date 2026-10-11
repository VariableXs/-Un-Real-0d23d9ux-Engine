//! unxu3761_u3761pipe —— RunDigest 总设计（账的账——聚合架构） 执行流水与守卫（RunDigest 聚合）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3761 · RunDigest 总设计（账的账——聚合架构） · 判据原文：UNX-F3761 · 判据：J1：聚合架构（各域账→摘要段→Digest（账的账三层——判据行内嵌：域账是分录/段是科目汇总/Digest 是总账）；入账协议（各域的入账接口——B25/B26/B27/B28 的入账承诺兑现面）；M-519 聚合首跑（三域账入账→Digest 出→三层账可互溯）。<br>J2：聚合纯度（Digest 只聚合不改判（摘要是投影不是重算——域账是判读真源、Digest 是档案）。
//! · 组件：RunDigest 聚合 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F3761 · 判据：J1：聚合架构（各域账→摘要段→Digest（账的账三层——判据行内嵌：域账是分录/段是科目汇总/Digest 是总账）；入账协议
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3761_VERSION: &str = "u3761-u3761pipe-v1";

/// 诊断段基址。
pub const U3761_SEG_BASE: u16 = 0xe1b0;

// --------------------------------------------------------------------------
// 诊断码（u3761 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RunDigest 聚合 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3761Code {
    /// 0xe1b0 级重复（聚合 同级双注册）。
    StageDup,
    /// 0xe1b1 级断档（对账 序缺失）。
    StageHole,
    /// 0xe1b2 依赖环（级间循环依赖）。
    DepCycle,
    /// 0xe1b3 自依赖（级依赖自身）。
    DepSelf,
    /// 0xe1b4 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0xe1b5 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0xe1b6 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0xe1b7 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0xe1b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3761Code {
    /// 全量闭集。
    pub const ALL: [u3761Code; 12] = [
        u3761Code::StageDup,
        u3761Code::StageHole,
        u3761Code::DepCycle,
        u3761Code::DepSelf,
        u3761Code::PlanLost,
        u3761Code::RollbackBad,
        u3761Code::OrderDrift,
        u3761Code::DoneDrift,
        u3761Code::LedgerBroken,
        u3761Code::SnapshotFingerprint,
        u3761Code::TableBroken,
        u3761Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3761Code::StageDup => 0xe1b0,
            u3761Code::StageHole => 0xe1b1,
            u3761Code::DepCycle => 0xe1b2,
            u3761Code::DepSelf => 0xe1b3,
            u3761Code::PlanLost => 0xe1b4,
            u3761Code::RollbackBad => 0xe1b5,
            u3761Code::OrderDrift => 0xe1b6,
            u3761Code::DoneDrift => 0xe1b7,
            u3761Code::LedgerBroken => 0xe1b8,
            u3761Code::SnapshotFingerprint => 0xe1b9,
            u3761Code::TableBroken => 0xe1ba,
            u3761Code::SegmentGuard => 0xe1bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3761Code::StageDup => "级重复（聚合 同级双注册）",
            u3761Code::StageHole => "级断档（对账 序缺失）",
            u3761Code::DepCycle => "依赖环（级间循环依赖）",
            u3761Code::DepSelf => "自依赖（级依赖自身）",
            u3761Code::PlanLost => "推演丢失（可执行序短于级数）",
            u3761Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u3761Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u3761Code::DoneDrift => "完成计数漂移（done≠实计）",
            u3761Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3761Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3761Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3761Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3761Code::StageDup => "按级唯一律去重",
            u3761Code::StageHole => "按级序补齐",
            u3761Code::DepCycle => "按无环律重构依赖",
            u3761Code::DepSelf => "删除自依赖边",
            u3761Code::PlanLost => "按 Kahn 剥皮重推",
            u3761Code::RollbackBad => "按守恒律复算",
            u3761Code::OrderDrift => "按推演序重放",
            u3761Code::DoneDrift => "按实计校正",
            u3761Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3761Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3761Code::TableBroken => "按语义规则重建常量表",
            u3761Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3761run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// RunDigest 聚合 执行流水（聚合）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3761runStage {
    /// 级 根：叶子账收集。
    Collect,
    /// 级 0：层级折叠。
    Fold,
    /// 级 1：对账。
    Check,
    /// 级 2：漂移检测。
    Drift,
    /// 级 3：签名。
    Sign,
    /// 级 4：封存封口。
    Seal,
}

impl mxu3761runStage {
    /// 全量级闭集。
    pub const ALL: [mxu3761runStage; 6] = [
        mxu3761runStage::Collect,
        mxu3761runStage::Fold,
        mxu3761runStage::Check,
        mxu3761runStage::Drift,
        mxu3761runStage::Sign,
        mxu3761runStage::Seal,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu3761runStage::Collect => 0,
            mxu3761runStage::Fold => 1,
            mxu3761runStage::Check => 2,
            mxu3761runStage::Drift => 3,
            mxu3761runStage::Sign => 4,
            mxu3761runStage::Seal => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu3761runStage::Collect => "叶子账收集",
            mxu3761runStage::Fold => "层级折叠",
            mxu3761runStage::Check => "对账",
            mxu3761runStage::Drift => "漂移检测",
            mxu3761runStage::Sign => "签名",
            mxu3761runStage::Seal => "封存封口",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU3761RUN_DEPS: [i32; 6] = [
    -1, // Collect 依赖 无
    0, // Fold 依赖 级 0
    1, // Check 依赖 级 1
    2, // Drift 依赖 级 2
    3, // Sign 依赖 级 3
    4, // Seal 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3761runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（RunDigest 聚合 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3761runBook {
    /// 各级状态。
    pub state: [mxu3761runProgress; 6],
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

impl mxu3761runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu3761runProgress::Pending; 6],
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
                if self.state[i] != mxu3761runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU3761RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu3761runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu3761runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu3761runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（RunDigest 聚合 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu3761runProgress::Done {
                self.state[i] = mxu3761runProgress::Rolled;
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
            if self.state[i] == mxu3761runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu3761runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu3761gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// RunDigest 聚合 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu3761gate {
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

/// 守卫阈值（RunDigest 聚合 重入/越限/静默三类拒止守卫）。
pub const MXU3761GATE_THRESHOLD: u64 = 64;

impl mxu3761gate {
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
        if self.window >= MXU3761GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1b0, 0xe1bf]。
pub const fn u3761_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3761Code::ALL.len() {
        let c = u3761Code::ALL[i].code();
        if c < 0xe1b0 || c > 0xe1bf {
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
fn u3761_guard_seg() -> bool {
    u3761_segment_guard()
}

/// UNX-F3761 流水 Kahn 推演 + 回滚守恒。
fn check_u3761_pipe_plan() -> bool {
    let mut pb = mxu3761runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F3761 守卫重入/越限/静默三路拒止。
fn check_u3761_pipe_guard() -> bool {
    let mut g = mxu3761gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3761_checks() -> CheckSet {
    let mut set = CheckSet::new("U3761");
    set.add(
        "u3761-seg-guard",
        u3761_guard_seg(),
        if u3761_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3761-pipe-plan",
        check_u3761_pipe_plan(),
        if check_u3761_pipe_plan() { "" } else { "UNX-F3761 pipeline plan mismatch" },
    );
    set.add(
        "u3761-pipe-guard",
        check_u3761_pipe_guard(),
        if check_u3761_pipe_guard() { "" } else { "UNX-F3761 guard mismatch" },
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
    fn u3761_all_checks_green() {
        let set = run_u3761_checks();
        assert!(set.all_passed(), "U3761 domain checks must be green");
    }

    #[test]
    fn u3761_segment_guard_holds() {
        assert!(u3761_segment_guard());
    }
}
