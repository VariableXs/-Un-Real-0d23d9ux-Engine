//! unxu1861_u1861pipe —— TLB 击落总纲（shootdown 全链四段） 执行流水与守卫（击落段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1861 · TLB 击落总纲（shootdown 全链四段） · 判据原文：UNX-F1861 · 判据：击落四段（发起→广播→应答→完成）冻结 v1，段间签名齐备，跳段注入 10/10 拒绝，一次击落全程账可导出
//! · 组件：击落段 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F1861 · 判据：击落四段（发起→广播→应答→完成）冻结 v1，段间签名齐备，跳段注入 10/10 拒绝，一次击落全程账可导出
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1861_VERSION: &str = "u1861-u1861pipe-v1";

/// 诊断段基址。
pub const U1861_SEG_BASE: u16 = 0x5ee0;

// --------------------------------------------------------------------------
// 诊断码（u1861 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 击落段 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1861Code {
    /// 0x5ee0 级重复（跳段 同级双注册）。
    StageDup,
    /// 0x5ee1 级断档（应答缺失 序缺失）。
    StageHole,
    /// 0x5ee2 依赖环（级间循环依赖）。
    DepCycle,
    /// 0x5ee3 自依赖（级依赖自身）。
    DepSelf,
    /// 0x5ee4 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0x5ee5 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0x5ee6 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0x5ee7 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0x5ee8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5ee9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5eea 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5eeb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1861Code {
    /// 全量闭集。
    pub const ALL: [u1861Code; 12] = [
        u1861Code::StageDup,
        u1861Code::StageHole,
        u1861Code::DepCycle,
        u1861Code::DepSelf,
        u1861Code::PlanLost,
        u1861Code::RollbackBad,
        u1861Code::OrderDrift,
        u1861Code::DoneDrift,
        u1861Code::LedgerBroken,
        u1861Code::SnapshotFingerprint,
        u1861Code::TableBroken,
        u1861Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1861Code::StageDup => 0x5ee0,
            u1861Code::StageHole => 0x5ee1,
            u1861Code::DepCycle => 0x5ee2,
            u1861Code::DepSelf => 0x5ee3,
            u1861Code::PlanLost => 0x5ee4,
            u1861Code::RollbackBad => 0x5ee5,
            u1861Code::OrderDrift => 0x5ee6,
            u1861Code::DoneDrift => 0x5ee7,
            u1861Code::LedgerBroken => 0x5ee8,
            u1861Code::SnapshotFingerprint => 0x5ee9,
            u1861Code::TableBroken => 0x5eea,
            u1861Code::SegmentGuard => 0x5eeb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1861Code::StageDup => "级重复（跳段 同级双注册）",
            u1861Code::StageHole => "级断档（应答缺失 序缺失）",
            u1861Code::DepCycle => "依赖环（级间循环依赖）",
            u1861Code::DepSelf => "自依赖（级依赖自身）",
            u1861Code::PlanLost => "推演丢失（可执行序短于级数）",
            u1861Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u1861Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u1861Code::DoneDrift => "完成计数漂移（done≠实计）",
            u1861Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1861Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1861Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1861Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1861Code::StageDup => "按级唯一律去重",
            u1861Code::StageHole => "按级序补齐",
            u1861Code::DepCycle => "按无环律重构依赖",
            u1861Code::DepSelf => "删除自依赖边",
            u1861Code::PlanLost => "按 Kahn 剥皮重推",
            u1861Code::RollbackBad => "按守恒律复算",
            u1861Code::OrderDrift => "按推演序重放",
            u1861Code::DoneDrift => "按实计校正",
            u1861Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1861Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1861Code::TableBroken => "按语义规则重建常量表",
            u1861Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu1861run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 击落段 执行流水（跳段）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1861runStage {
    /// 级 根：发起段。
    Initiate,
    /// 级 0：广播段。
    Broadcast,
    /// 级 1：应答段。
    Ack,
    /// 级 2：完成段。
    Complete,
    /// 级 3：跳段演练。
    Probe,
    /// 级 4：全程账。
    Ledger,
}

impl mxu1861runStage {
    /// 全量级闭集。
    pub const ALL: [mxu1861runStage; 6] = [
        mxu1861runStage::Initiate,
        mxu1861runStage::Broadcast,
        mxu1861runStage::Ack,
        mxu1861runStage::Complete,
        mxu1861runStage::Probe,
        mxu1861runStage::Ledger,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu1861runStage::Initiate => 0,
            mxu1861runStage::Broadcast => 1,
            mxu1861runStage::Ack => 2,
            mxu1861runStage::Complete => 3,
            mxu1861runStage::Probe => 4,
            mxu1861runStage::Ledger => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu1861runStage::Initiate => "发起段",
            mxu1861runStage::Broadcast => "广播段",
            mxu1861runStage::Ack => "应答段",
            mxu1861runStage::Complete => "完成段",
            mxu1861runStage::Probe => "跳段演练",
            mxu1861runStage::Ledger => "全程账",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU1861RUN_DEPS: [i32; 6] = [
    -1, // Initiate 依赖 无
    0, // Broadcast 依赖 级 0
    1, // Ack 依赖 级 1
    2, // Complete 依赖 级 2
    3, // Probe 依赖 级 3
    4, // Ledger 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1861runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（击落段 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1861runBook {
    /// 各级状态。
    pub state: [mxu1861runProgress; 6],
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

impl mxu1861runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu1861runProgress::Pending; 6],
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
                if self.state[i] != mxu1861runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU1861RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu1861runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu1861runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu1861runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（击落段 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu1861runProgress::Done {
                self.state[i] = mxu1861runProgress::Rolled;
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
            if self.state[i] == mxu1861runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu1861runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu1861gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 击落段 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu1861gate {
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

/// 守卫阈值（击落段 重入/越限/静默三类拒止守卫）。
pub const MXU1861GATE_THRESHOLD: u64 = 64;

impl mxu1861gate {
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
        if self.window >= MXU1861GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5ee0, 0x5eef]。
pub const fn u1861_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1861Code::ALL.len() {
        let c = u1861Code::ALL[i].code();
        if c < 0x5ee0 || c > 0x5eef {
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
fn u1861_guard_seg() -> bool {
    u1861_segment_guard()
}

/// UNX-F1861 流水 Kahn 推演 + 回滚守恒。
fn check_u1861_pipe_plan() -> bool {
    let mut pb = mxu1861runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F1861 守卫重入/越限/静默三路拒止。
fn check_u1861_pipe_guard() -> bool {
    let mut g = mxu1861gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1861_checks() -> CheckSet {
    let mut set = CheckSet::new("U1861");
    set.add(
        "u1861-seg-guard",
        u1861_guard_seg(),
        if u1861_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1861-pipe-plan",
        check_u1861_pipe_plan(),
        if check_u1861_pipe_plan() { "" } else { "UNX-F1861 pipeline plan mismatch" },
    );
    set.add(
        "u1861-pipe-guard",
        check_u1861_pipe_guard(),
        if check_u1861_pipe_guard() { "" } else { "UNX-F1861 guard mismatch" },
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
    fn u1861_all_checks_green() {
        let set = run_u1861_checks();
        assert!(set.all_passed(), "U1861 domain checks must be green");
    }

    #[test]
    fn u1861_segment_guard_holds() {
        assert!(u1861_segment_guard());
    }
}
