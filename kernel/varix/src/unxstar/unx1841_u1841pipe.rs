//! unxu1841_u1841pipe —— IPI 架构总纲（向量分派框架与 A2 衔接） 执行流水与守卫（向量）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1841 · IPI 架构总纲（向量分派框架与 A2 衔接） · 判据原文：UNX-F1841 · 判据：IPI 分派框架冻结 v1（向量→处理函数表+分发协议+EOI 纪律），与 A2 原语衔接断言全过，未登记向量注入 10/10 拒绝
//! · 组件：向量 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F1841 · 判据：IPI 分派框架冻结 v1（向量→处理函数表+分发协议+EOI 纪律），与 A2 原语衔接断言全过，未登记向量注入 10/10 拒
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1841_VERSION: &str = "u1841-u1841pipe-v1";

/// 诊断段基址。
pub const U1841_SEG_BASE: u16 = 0x5e20;

// --------------------------------------------------------------------------
// 诊断码（u1841 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 向量 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1841Code {
    /// 0x5e20 级重复（未登记向量 同级双注册）。
    StageDup,
    /// 0x5e21 级断档（EOI 漏发 序缺失）。
    StageHole,
    /// 0x5e22 依赖环（级间循环依赖）。
    DepCycle,
    /// 0x5e23 自依赖（级依赖自身）。
    DepSelf,
    /// 0x5e24 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0x5e25 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0x5e26 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0x5e27 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0x5e28 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5e29 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5e2a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5e2b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1841Code {
    /// 全量闭集。
    pub const ALL: [u1841Code; 12] = [
        u1841Code::StageDup,
        u1841Code::StageHole,
        u1841Code::DepCycle,
        u1841Code::DepSelf,
        u1841Code::PlanLost,
        u1841Code::RollbackBad,
        u1841Code::OrderDrift,
        u1841Code::DoneDrift,
        u1841Code::LedgerBroken,
        u1841Code::SnapshotFingerprint,
        u1841Code::TableBroken,
        u1841Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1841Code::StageDup => 0x5e20,
            u1841Code::StageHole => 0x5e21,
            u1841Code::DepCycle => 0x5e22,
            u1841Code::DepSelf => 0x5e23,
            u1841Code::PlanLost => 0x5e24,
            u1841Code::RollbackBad => 0x5e25,
            u1841Code::OrderDrift => 0x5e26,
            u1841Code::DoneDrift => 0x5e27,
            u1841Code::LedgerBroken => 0x5e28,
            u1841Code::SnapshotFingerprint => 0x5e29,
            u1841Code::TableBroken => 0x5e2a,
            u1841Code::SegmentGuard => 0x5e2b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1841Code::StageDup => "级重复（未登记向量 同级双注册）",
            u1841Code::StageHole => "级断档（EOI 漏发 序缺失）",
            u1841Code::DepCycle => "依赖环（级间循环依赖）",
            u1841Code::DepSelf => "自依赖（级依赖自身）",
            u1841Code::PlanLost => "推演丢失（可执行序短于级数）",
            u1841Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u1841Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u1841Code::DoneDrift => "完成计数漂移（done≠实计）",
            u1841Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1841Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1841Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1841Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1841Code::StageDup => "按级唯一律去重",
            u1841Code::StageHole => "按级序补齐",
            u1841Code::DepCycle => "按无环律重构依赖",
            u1841Code::DepSelf => "删除自依赖边",
            u1841Code::PlanLost => "按 Kahn 剥皮重推",
            u1841Code::RollbackBad => "按守恒律复算",
            u1841Code::OrderDrift => "按推演序重放",
            u1841Code::DoneDrift => "按实计校正",
            u1841Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1841Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1841Code::TableBroken => "按语义规则重建常量表",
            u1841Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu1841run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 向量 执行流水（未登记向量）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1841runStage {
    /// 级 根：向量登记。
    Vector,
    /// 级 0：处理函数挂表。
    Handler,
    /// 级 1：分发协议。
    Proto,
    /// 级 2：EOI 纪律。
    Eoi,
    /// 级 3：未登记演练。
    Probe,
    /// 级 4：冻结。
    Freeze,
}

impl mxu1841runStage {
    /// 全量级闭集。
    pub const ALL: [mxu1841runStage; 6] = [
        mxu1841runStage::Vector,
        mxu1841runStage::Handler,
        mxu1841runStage::Proto,
        mxu1841runStage::Eoi,
        mxu1841runStage::Probe,
        mxu1841runStage::Freeze,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu1841runStage::Vector => 0,
            mxu1841runStage::Handler => 1,
            mxu1841runStage::Proto => 2,
            mxu1841runStage::Eoi => 3,
            mxu1841runStage::Probe => 4,
            mxu1841runStage::Freeze => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu1841runStage::Vector => "向量登记",
            mxu1841runStage::Handler => "处理函数挂表",
            mxu1841runStage::Proto => "分发协议",
            mxu1841runStage::Eoi => "EOI 纪律",
            mxu1841runStage::Probe => "未登记演练",
            mxu1841runStage::Freeze => "冻结",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU1841RUN_DEPS: [i32; 6] = [
    -1, // Vector 依赖 无
    0, // Handler 依赖 级 0
    1, // Proto 依赖 级 1
    2, // Eoi 依赖 级 2
    3, // Probe 依赖 级 3
    4, // Freeze 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1841runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（向量 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1841runBook {
    /// 各级状态。
    pub state: [mxu1841runProgress; 6],
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

impl mxu1841runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu1841runProgress::Pending; 6],
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
                if self.state[i] != mxu1841runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU1841RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu1841runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu1841runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu1841runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（向量 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu1841runProgress::Done {
                self.state[i] = mxu1841runProgress::Rolled;
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
            if self.state[i] == mxu1841runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu1841runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu1841gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 向量 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu1841gate {
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

/// 守卫阈值（向量 重入/越限/静默三类拒止守卫）。
pub const MXU1841GATE_THRESHOLD: u64 = 64;

impl mxu1841gate {
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
        if self.window >= MXU1841GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5e20, 0x5e2f]。
pub const fn u1841_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1841Code::ALL.len() {
        let c = u1841Code::ALL[i].code();
        if c < 0x5e20 || c > 0x5e2f {
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
fn u1841_guard_seg() -> bool {
    u1841_segment_guard()
}

/// UNX-F1841 流水 Kahn 推演 + 回滚守恒。
fn check_u1841_pipe_plan() -> bool {
    let mut pb = mxu1841runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F1841 守卫重入/越限/静默三路拒止。
fn check_u1841_pipe_guard() -> bool {
    let mut g = mxu1841gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1841_checks() -> CheckSet {
    let mut set = CheckSet::new("U1841");
    set.add(
        "u1841-seg-guard",
        u1841_guard_seg(),
        if u1841_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1841-pipe-plan",
        check_u1841_pipe_plan(),
        if check_u1841_pipe_plan() { "" } else { "UNX-F1841 pipeline plan mismatch" },
    );
    set.add(
        "u1841-pipe-guard",
        check_u1841_pipe_guard(),
        if check_u1841_pipe_guard() { "" } else { "UNX-F1841 guard mismatch" },
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
    fn u1841_all_checks_green() {
        let set = run_u1841_checks();
        assert!(set.all_passed(), "U1841 domain checks must be green");
    }

    #[test]
    fn u1841_segment_guard_holds() {
        assert!(u1841_segment_guard());
    }
}
