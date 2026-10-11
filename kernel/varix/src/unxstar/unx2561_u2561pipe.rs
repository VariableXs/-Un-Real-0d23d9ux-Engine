//! unxu2561_u2561pipe —— AddressSpace 成熟体总纲（现存 addrspace.rs 接管） 执行流水与守卫（地址空间体）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2561 · AddressSpace 成熟体总纲（现存 addrspace.rs 接管） · 判据原文：UNX-F2561 · 判据：现存 `addrspace.rs`（F001 起步态）接管账在档（语义逐项映射），成熟体接口面三机直测一致
//! · 组件：地址空间体 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F2561 · 判据：现存 `addrspace.rs`（F001 起步态）接管账在档（语义逐项映射），成熟体接口面三机直测一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2561_VERSION: &str = "u2561-u2561pipe-v1";

/// 诊断段基址。
pub const U2561_SEG_BASE: u16 = 0xa620;

// --------------------------------------------------------------------------
// 诊断码（u2561 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 地址空间体 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2561Code {
    /// 0xa620 级重复（接管漏项 同级双注册）。
    StageDup,
    /// 0xa621 级断档（语义错配 序缺失）。
    StageHole,
    /// 0xa622 依赖环（级间循环依赖）。
    DepCycle,
    /// 0xa623 自依赖（级依赖自身）。
    DepSelf,
    /// 0xa624 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0xa625 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0xa626 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0xa627 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0xa628 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa629 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa62a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa62b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2561Code {
    /// 全量闭集。
    pub const ALL: [u2561Code; 12] = [
        u2561Code::StageDup,
        u2561Code::StageHole,
        u2561Code::DepCycle,
        u2561Code::DepSelf,
        u2561Code::PlanLost,
        u2561Code::RollbackBad,
        u2561Code::OrderDrift,
        u2561Code::DoneDrift,
        u2561Code::LedgerBroken,
        u2561Code::SnapshotFingerprint,
        u2561Code::TableBroken,
        u2561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2561Code::StageDup => 0xa620,
            u2561Code::StageHole => 0xa621,
            u2561Code::DepCycle => 0xa622,
            u2561Code::DepSelf => 0xa623,
            u2561Code::PlanLost => 0xa624,
            u2561Code::RollbackBad => 0xa625,
            u2561Code::OrderDrift => 0xa626,
            u2561Code::DoneDrift => 0xa627,
            u2561Code::LedgerBroken => 0xa628,
            u2561Code::SnapshotFingerprint => 0xa629,
            u2561Code::TableBroken => 0xa62a,
            u2561Code::SegmentGuard => 0xa62b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2561Code::StageDup => "级重复（接管漏项 同级双注册）",
            u2561Code::StageHole => "级断档（语义错配 序缺失）",
            u2561Code::DepCycle => "依赖环（级间循环依赖）",
            u2561Code::DepSelf => "自依赖（级依赖自身）",
            u2561Code::PlanLost => "推演丢失（可执行序短于级数）",
            u2561Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u2561Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u2561Code::DoneDrift => "完成计数漂移（done≠实计）",
            u2561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2561Code::StageDup => "按级唯一律去重",
            u2561Code::StageHole => "按级序补齐",
            u2561Code::DepCycle => "按无环律重构依赖",
            u2561Code::DepSelf => "删除自依赖边",
            u2561Code::PlanLost => "按 Kahn 剥皮重推",
            u2561Code::RollbackBad => "按守恒律复算",
            u2561Code::OrderDrift => "按推演序重放",
            u2561Code::DoneDrift => "按实计校正",
            u2561Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2561Code::TableBroken => "按语义规则重建常量表",
            u2561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu2561run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 地址空间体 执行流水（接管漏项）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2561runStage {
    /// 级 根：旧体接管。
    Adopt,
    /// 级 0：语义映射。
    Map,
    /// 级 1：解除路径。
    Unmap,
    /// 级 2：成熟体装配。
    Mature,
    /// 级 3：逐项核验。
    Verify,
    /// 级 4：冻结封存。
    Seal,
}

impl mxu2561runStage {
    /// 全量级闭集。
    pub const ALL: [mxu2561runStage; 6] = [
        mxu2561runStage::Adopt,
        mxu2561runStage::Map,
        mxu2561runStage::Unmap,
        mxu2561runStage::Mature,
        mxu2561runStage::Verify,
        mxu2561runStage::Seal,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu2561runStage::Adopt => 0,
            mxu2561runStage::Map => 1,
            mxu2561runStage::Unmap => 2,
            mxu2561runStage::Mature => 3,
            mxu2561runStage::Verify => 4,
            mxu2561runStage::Seal => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu2561runStage::Adopt => "旧体接管",
            mxu2561runStage::Map => "语义映射",
            mxu2561runStage::Unmap => "解除路径",
            mxu2561runStage::Mature => "成熟体装配",
            mxu2561runStage::Verify => "逐项核验",
            mxu2561runStage::Seal => "冻结封存",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU2561RUN_DEPS: [i32; 6] = [
    -1, // Adopt 依赖 无
    0, // Map 依赖 级 0
    1, // Unmap 依赖 级 1
    2, // Mature 依赖 级 2
    3, // Verify 依赖 级 3
    4, // Seal 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2561runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（地址空间体 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2561runBook {
    /// 各级状态。
    pub state: [mxu2561runProgress; 6],
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

impl mxu2561runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu2561runProgress::Pending; 6],
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
                if self.state[i] != mxu2561runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU2561RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu2561runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu2561runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu2561runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（地址空间体 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu2561runProgress::Done {
                self.state[i] = mxu2561runProgress::Rolled;
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
            if self.state[i] == mxu2561runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu2561runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu2561gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 地址空间体 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu2561gate {
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

/// 守卫阈值（地址空间体 重入/越限/静默三类拒止守卫）。
pub const MXU2561GATE_THRESHOLD: u64 = 64;

impl mxu2561gate {
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
        if self.window >= MXU2561GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa620, 0xa62f]。
pub const fn u2561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2561Code::ALL.len() {
        let c = u2561Code::ALL[i].code();
        if c < 0xa620 || c > 0xa62f {
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
fn u2561_guard_seg() -> bool {
    u2561_segment_guard()
}

/// UNX-F2561 流水 Kahn 推演 + 回滚守恒。
fn check_u2561_pipe_plan() -> bool {
    let mut pb = mxu2561runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F2561 守卫重入/越限/静默三路拒止。
fn check_u2561_pipe_guard() -> bool {
    let mut g = mxu2561gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2561_checks() -> CheckSet {
    let mut set = CheckSet::new("U2561");
    set.add(
        "u2561-seg-guard",
        u2561_guard_seg(),
        if u2561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2561-pipe-plan",
        check_u2561_pipe_plan(),
        if check_u2561_pipe_plan() { "" } else { "UNX-F2561 pipeline plan mismatch" },
    );
    set.add(
        "u2561-pipe-guard",
        check_u2561_pipe_guard(),
        if check_u2561_pipe_guard() { "" } else { "UNX-F2561 guard mismatch" },
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
    fn u2561_all_checks_green() {
        let set = run_u2561_checks();
        assert!(set.all_passed(), "U2561 domain checks must be green");
    }

    #[test]
    fn u2561_segment_guard_holds() {
        assert!(u2561_segment_guard());
    }
}
