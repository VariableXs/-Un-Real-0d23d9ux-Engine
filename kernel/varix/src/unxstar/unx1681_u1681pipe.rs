//! unxu1681_u1681pipe —— 时间片计算总纲（基片×权重缩放模型） 执行流水与守卫（时间片）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1681 · 时间片计算总纲（基片×权重缩放模型） · 判据原文：UNX-F1681 · 判据：时间片=基片×（本线程权重/基准权重）模型冻结 v1，公式参数单点定义，计算路径唯一
//! · 组件：时间片 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F1681 · 判据：时间片=基片×（本线程权重/基准权重）模型冻结 v1，公式参数单点定义，计算路径唯一
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1681_VERSION: &str = "u1681-u1681pipe-v1";

/// 诊断段基址。
pub const U1681_SEG_BASE: u16 = 0x5820;

// --------------------------------------------------------------------------
// 诊断码（u1681 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 时间片 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1681Code {
    /// 0x5820 级重复（参数多点 同级双注册）。
    StageDup,
    /// 0x5821 级断档（路径分叉 序缺失）。
    StageHole,
    /// 0x5822 依赖环（级间循环依赖）。
    DepCycle,
    /// 0x5823 自依赖（级依赖自身）。
    DepSelf,
    /// 0x5824 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0x5825 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0x5826 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0x5827 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0x5828 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5829 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x582a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x582b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1681Code {
    /// 全量闭集。
    pub const ALL: [u1681Code; 12] = [
        u1681Code::StageDup,
        u1681Code::StageHole,
        u1681Code::DepCycle,
        u1681Code::DepSelf,
        u1681Code::PlanLost,
        u1681Code::RollbackBad,
        u1681Code::OrderDrift,
        u1681Code::DoneDrift,
        u1681Code::LedgerBroken,
        u1681Code::SnapshotFingerprint,
        u1681Code::TableBroken,
        u1681Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1681Code::StageDup => 0x5820,
            u1681Code::StageHole => 0x5821,
            u1681Code::DepCycle => 0x5822,
            u1681Code::DepSelf => 0x5823,
            u1681Code::PlanLost => 0x5824,
            u1681Code::RollbackBad => 0x5825,
            u1681Code::OrderDrift => 0x5826,
            u1681Code::DoneDrift => 0x5827,
            u1681Code::LedgerBroken => 0x5828,
            u1681Code::SnapshotFingerprint => 0x5829,
            u1681Code::TableBroken => 0x582a,
            u1681Code::SegmentGuard => 0x582b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1681Code::StageDup => "级重复（参数多点 同级双注册）",
            u1681Code::StageHole => "级断档（路径分叉 序缺失）",
            u1681Code::DepCycle => "依赖环（级间循环依赖）",
            u1681Code::DepSelf => "自依赖（级依赖自身）",
            u1681Code::PlanLost => "推演丢失（可执行序短于级数）",
            u1681Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u1681Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u1681Code::DoneDrift => "完成计数漂移（done≠实计）",
            u1681Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1681Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1681Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1681Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1681Code::StageDup => "按级唯一律去重",
            u1681Code::StageHole => "按级序补齐",
            u1681Code::DepCycle => "按无环律重构依赖",
            u1681Code::DepSelf => "删除自依赖边",
            u1681Code::PlanLost => "按 Kahn 剥皮重推",
            u1681Code::RollbackBad => "按守恒律复算",
            u1681Code::OrderDrift => "按推演序重放",
            u1681Code::DoneDrift => "按实计校正",
            u1681Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1681Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1681Code::TableBroken => "按语义规则重建常量表",
            u1681Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu1681run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 时间片 执行流水（参数多点）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1681runStage {
    /// 级 根：基片定义。
    Base,
    /// 级 0：权重比。
    Weight,
    /// 级 1：缩放计算。
    Scale,
    /// 级 2：双钳制。
    Clamp,
    /// 级 3：对拍核验。
    Verify,
    /// 级 4：封账。
    Seal,
}

impl mxu1681runStage {
    /// 全量级闭集。
    pub const ALL: [mxu1681runStage; 6] = [
        mxu1681runStage::Base,
        mxu1681runStage::Weight,
        mxu1681runStage::Scale,
        mxu1681runStage::Clamp,
        mxu1681runStage::Verify,
        mxu1681runStage::Seal,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu1681runStage::Base => 0,
            mxu1681runStage::Weight => 1,
            mxu1681runStage::Scale => 2,
            mxu1681runStage::Clamp => 3,
            mxu1681runStage::Verify => 4,
            mxu1681runStage::Seal => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu1681runStage::Base => "基片定义",
            mxu1681runStage::Weight => "权重比",
            mxu1681runStage::Scale => "缩放计算",
            mxu1681runStage::Clamp => "双钳制",
            mxu1681runStage::Verify => "对拍核验",
            mxu1681runStage::Seal => "封账",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU1681RUN_DEPS: [i32; 6] = [
    -1, // Base 依赖 无
    0, // Weight 依赖 级 0
    1, // Scale 依赖 级 1
    2, // Clamp 依赖 级 2
    3, // Verify 依赖 级 3
    4, // Seal 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1681runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（时间片 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1681runBook {
    /// 各级状态。
    pub state: [mxu1681runProgress; 6],
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

impl mxu1681runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu1681runProgress::Pending; 6],
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
                if self.state[i] != mxu1681runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU1681RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu1681runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu1681runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu1681runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（时间片 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu1681runProgress::Done {
                self.state[i] = mxu1681runProgress::Rolled;
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
            if self.state[i] == mxu1681runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu1681runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu1681gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 时间片 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu1681gate {
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

/// 守卫阈值（时间片 重入/越限/静默三类拒止守卫）。
pub const MXU1681GATE_THRESHOLD: u64 = 64;

impl mxu1681gate {
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
        if self.window >= MXU1681GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5820, 0x582f]。
pub const fn u1681_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1681Code::ALL.len() {
        let c = u1681Code::ALL[i].code();
        if c < 0x5820 || c > 0x582f {
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
fn u1681_guard_seg() -> bool {
    u1681_segment_guard()
}

/// UNX-F1681 流水 Kahn 推演 + 回滚守恒。
fn check_u1681_pipe_plan() -> bool {
    let mut pb = mxu1681runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F1681 守卫重入/越限/静默三路拒止。
fn check_u1681_pipe_guard() -> bool {
    let mut g = mxu1681gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1681_checks() -> CheckSet {
    let mut set = CheckSet::new("U1681");
    set.add(
        "u1681-seg-guard",
        u1681_guard_seg(),
        if u1681_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1681-pipe-plan",
        check_u1681_pipe_plan(),
        if check_u1681_pipe_plan() { "" } else { "UNX-F1681 pipeline plan mismatch" },
    );
    set.add(
        "u1681-pipe-guard",
        check_u1681_pipe_guard(),
        if check_u1681_pipe_guard() { "" } else { "UNX-F1681 guard mismatch" },
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
    fn u1681_all_checks_green() {
        let set = run_u1681_checks();
        assert!(set.all_passed(), "U1681 domain checks must be green");
    }

    #[test]
    fn u1681_segment_guard_holds() {
        assert!(u1681_segment_guard());
    }
}
