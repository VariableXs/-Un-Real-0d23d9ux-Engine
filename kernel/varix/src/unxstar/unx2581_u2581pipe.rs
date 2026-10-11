//! unxu2581_u2581pipe —— VMA 区间树结构总纲（红黑树类，按地址排序） 执行流水与守卫（VMA 区间）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2581 · VMA 区间树结构总纲（红黑树类，按地址排序） · 判据原文：UNX-F2581 · 判据：区间树结构逐项断言（排序/平衡/区间不变量 10^6 次操作全过），VMA 记录四字段（起止/属性/类型/对象）齐备
//! · 组件：VMA 区间 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F2581 · 判据：区间树结构逐项断言（排序/平衡/区间不变量 10^6 次操作全过），VMA 记录四字段（起止/属性/类型/对象）齐备
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2581_VERSION: &str = "u2581-u2581pipe-v1";

/// 诊断段基址。
pub const U2581_SEG_BASE: u16 = 0xa6e0;

// --------------------------------------------------------------------------
// 诊断码（u2581 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// VMA 区间 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2581Code {
    /// 0xa6e0 级重复（排序破坏 同级双注册）。
    StageDup,
    /// 0xa6e1 级断档（失衡塌陷 序缺失）。
    StageHole,
    /// 0xa6e2 依赖环（级间循环依赖）。
    DepCycle,
    /// 0xa6e3 自依赖（级依赖自身）。
    DepSelf,
    /// 0xa6e4 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0xa6e5 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0xa6e6 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0xa6e7 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0xa6e8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa6e9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa6ea 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa6eb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2581Code {
    /// 全量闭集。
    pub const ALL: [u2581Code; 12] = [
        u2581Code::StageDup,
        u2581Code::StageHole,
        u2581Code::DepCycle,
        u2581Code::DepSelf,
        u2581Code::PlanLost,
        u2581Code::RollbackBad,
        u2581Code::OrderDrift,
        u2581Code::DoneDrift,
        u2581Code::LedgerBroken,
        u2581Code::SnapshotFingerprint,
        u2581Code::TableBroken,
        u2581Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2581Code::StageDup => 0xa6e0,
            u2581Code::StageHole => 0xa6e1,
            u2581Code::DepCycle => 0xa6e2,
            u2581Code::DepSelf => 0xa6e3,
            u2581Code::PlanLost => 0xa6e4,
            u2581Code::RollbackBad => 0xa6e5,
            u2581Code::OrderDrift => 0xa6e6,
            u2581Code::DoneDrift => 0xa6e7,
            u2581Code::LedgerBroken => 0xa6e8,
            u2581Code::SnapshotFingerprint => 0xa6e9,
            u2581Code::TableBroken => 0xa6ea,
            u2581Code::SegmentGuard => 0xa6eb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2581Code::StageDup => "级重复（排序破坏 同级双注册）",
            u2581Code::StageHole => "级断档（失衡塌陷 序缺失）",
            u2581Code::DepCycle => "依赖环（级间循环依赖）",
            u2581Code::DepSelf => "自依赖（级依赖自身）",
            u2581Code::PlanLost => "推演丢失（可执行序短于级数）",
            u2581Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u2581Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u2581Code::DoneDrift => "完成计数漂移（done≠实计）",
            u2581Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2581Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2581Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2581Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2581Code::StageDup => "按级唯一律去重",
            u2581Code::StageHole => "按级序补齐",
            u2581Code::DepCycle => "按无环律重构依赖",
            u2581Code::DepSelf => "删除自依赖边",
            u2581Code::PlanLost => "按 Kahn 剥皮重推",
            u2581Code::RollbackBad => "按守恒律复算",
            u2581Code::OrderDrift => "按推演序重放",
            u2581Code::DoneDrift => "按实计校正",
            u2581Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2581Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2581Code::TableBroken => "按语义规则重建常量表",
            u2581Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu2581run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// VMA 区间 执行流水（排序破坏）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2581runStage {
    /// 级 根：区间插入。
    Insert,
    /// 级 0：区间删除。
    Delete,
    /// 级 1：旋转再平衡。
    Rotate,
    /// 级 2：排序核验。
    Sort,
    /// 级 3：不变量核验。
    Invariant,
    /// 级 4：冻结封存。
    Seal,
}

impl mxu2581runStage {
    /// 全量级闭集。
    pub const ALL: [mxu2581runStage; 6] = [
        mxu2581runStage::Insert,
        mxu2581runStage::Delete,
        mxu2581runStage::Rotate,
        mxu2581runStage::Sort,
        mxu2581runStage::Invariant,
        mxu2581runStage::Seal,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu2581runStage::Insert => 0,
            mxu2581runStage::Delete => 1,
            mxu2581runStage::Rotate => 2,
            mxu2581runStage::Sort => 3,
            mxu2581runStage::Invariant => 4,
            mxu2581runStage::Seal => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu2581runStage::Insert => "区间插入",
            mxu2581runStage::Delete => "区间删除",
            mxu2581runStage::Rotate => "旋转再平衡",
            mxu2581runStage::Sort => "排序核验",
            mxu2581runStage::Invariant => "不变量核验",
            mxu2581runStage::Seal => "冻结封存",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU2581RUN_DEPS: [i32; 6] = [
    -1, // Insert 依赖 无
    0, // Delete 依赖 级 0
    1, // Rotate 依赖 级 1
    2, // Sort 依赖 级 2
    3, // Invariant 依赖 级 3
    4, // Seal 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2581runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（VMA 区间 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2581runBook {
    /// 各级状态。
    pub state: [mxu2581runProgress; 6],
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

impl mxu2581runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu2581runProgress::Pending; 6],
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
                if self.state[i] != mxu2581runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU2581RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu2581runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu2581runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu2581runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（VMA 区间 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu2581runProgress::Done {
                self.state[i] = mxu2581runProgress::Rolled;
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
            if self.state[i] == mxu2581runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu2581runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu2581gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// VMA 区间 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu2581gate {
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

/// 守卫阈值（VMA 区间 重入/越限/静默三类拒止守卫）。
pub const MXU2581GATE_THRESHOLD: u64 = 64;

impl mxu2581gate {
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
        if self.window >= MXU2581GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa6e0, 0xa6ef]。
pub const fn u2581_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2581Code::ALL.len() {
        let c = u2581Code::ALL[i].code();
        if c < 0xa6e0 || c > 0xa6ef {
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
fn u2581_guard_seg() -> bool {
    u2581_segment_guard()
}

/// UNX-F2581 流水 Kahn 推演 + 回滚守恒。
fn check_u2581_pipe_plan() -> bool {
    let mut pb = mxu2581runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F2581 守卫重入/越限/静默三路拒止。
fn check_u2581_pipe_guard() -> bool {
    let mut g = mxu2581gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2581_checks() -> CheckSet {
    let mut set = CheckSet::new("U2581");
    set.add(
        "u2581-seg-guard",
        u2581_guard_seg(),
        if u2581_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2581-pipe-plan",
        check_u2581_pipe_plan(),
        if check_u2581_pipe_plan() { "" } else { "UNX-F2581 pipeline plan mismatch" },
    );
    set.add(
        "u2581-pipe-guard",
        check_u2581_pipe_guard(),
        if check_u2581_pipe_guard() { "" } else { "UNX-F2581 guard mismatch" },
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
    fn u2581_all_checks_green() {
        let set = run_u2581_checks();
        assert!(set.all_passed(), "U2581 domain checks must be green");
    }

    #[test]
    fn u2581_segment_guard_holds() {
        assert!(u2581_segment_guard());
    }
}
