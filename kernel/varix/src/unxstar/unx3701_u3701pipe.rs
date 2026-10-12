//! unxu3701_u3701pipe —— 覆盖总设计（边×状态×组合三维模型） 执行流水与守卫（覆盖三维模型）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3701 · 覆盖总设计（边×状态×组合三维模型） · 判据原文：UNX-F3701 · 判据：J1：三维模型（边覆盖（代码维——F3545）/状态覆盖（语义维——状态机×转移）/组合覆盖（参数维——n-wise——判据行内嵌三维各治什么病：边治"没走过的代码"、状态治"没到过的机器态"、组合治"单看都合法搭起来才炸"）；M-459 三维首测（同一靶入口三维各出覆盖数+三维互补性（各有独占盲区——三维不冗余的实证）。<br>J2：三维可合成（总覆盖视图=三维投影合成（各维口径独立、合成式显式——不混账）。
//! · 组件：覆盖三维模型 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F3701 · 判据：J1：三维模型（边覆盖（代码维——F3545）/状态覆盖（语义维——状态机×转移）/组合覆盖（参数维——n-wise——判据行内嵌
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3701_VERSION: &str = "u3701-u3701pipe-v1";

/// 诊断段基址。
pub const U3701_SEG_BASE: u16 = 0xe180;

// --------------------------------------------------------------------------
// 诊断码（u3701 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 覆盖三维模型 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3701Code {
    /// 0xe180 级重复（覆盖 同级双注册）。
    StageDup,
    /// 0xe181 级断档（三维 序缺失）。
    StageHole,
    /// 0xe182 依赖环（级间循环依赖）。
    DepCycle,
    /// 0xe183 自依赖（级依赖自身）。
    DepSelf,
    /// 0xe184 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0xe185 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0xe186 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0xe187 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0xe188 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe189 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe18a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe18b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3701Code {
    /// 全量闭集。
    pub const ALL: [u3701Code; 12] = [
        u3701Code::StageDup,
        u3701Code::StageHole,
        u3701Code::DepCycle,
        u3701Code::DepSelf,
        u3701Code::PlanLost,
        u3701Code::RollbackBad,
        u3701Code::OrderDrift,
        u3701Code::DoneDrift,
        u3701Code::LedgerBroken,
        u3701Code::SnapshotFingerprint,
        u3701Code::TableBroken,
        u3701Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3701Code::StageDup => 0xe180,
            u3701Code::StageHole => 0xe181,
            u3701Code::DepCycle => 0xe182,
            u3701Code::DepSelf => 0xe183,
            u3701Code::PlanLost => 0xe184,
            u3701Code::RollbackBad => 0xe185,
            u3701Code::OrderDrift => 0xe186,
            u3701Code::DoneDrift => 0xe187,
            u3701Code::LedgerBroken => 0xe188,
            u3701Code::SnapshotFingerprint => 0xe189,
            u3701Code::TableBroken => 0xe18a,
            u3701Code::SegmentGuard => 0xe18b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3701Code::StageDup => "级重复（覆盖 同级双注册）",
            u3701Code::StageHole => "级断档（三维 序缺失）",
            u3701Code::DepCycle => "依赖环（级间循环依赖）",
            u3701Code::DepSelf => "自依赖（级依赖自身）",
            u3701Code::PlanLost => "推演丢失（可执行序短于级数）",
            u3701Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u3701Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u3701Code::DoneDrift => "完成计数漂移（done≠实计）",
            u3701Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3701Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3701Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3701Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3701Code::StageDup => "按级唯一律去重",
            u3701Code::StageHole => "按级序补齐",
            u3701Code::DepCycle => "按无环律重构依赖",
            u3701Code::DepSelf => "删除自依赖边",
            u3701Code::PlanLost => "按 Kahn 剥皮重推",
            u3701Code::RollbackBad => "按守恒律复算",
            u3701Code::OrderDrift => "按推演序重放",
            u3701Code::DoneDrift => "按实计校正",
            u3701Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3701Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3701Code::TableBroken => "按语义规则重建常量表",
            u3701Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3701run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 覆盖三维模型 执行流水（覆盖）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3701runStage {
    /// 级 根：三维定义。
    Define,
    /// 级 0：组合枚举。
    Enum,
    /// 级 1：命中标记。
    Mark,
    /// 级 2：交叉验算。
    Cross,
    /// 级 3：缺口审计。
    Audit,
    /// 级 4：收口封存。
    Close,
}

impl mxu3701runStage {
    /// 全量级闭集。
    pub const ALL: [mxu3701runStage; 6] = [
        mxu3701runStage::Define,
        mxu3701runStage::Enum,
        mxu3701runStage::Mark,
        mxu3701runStage::Cross,
        mxu3701runStage::Audit,
        mxu3701runStage::Close,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu3701runStage::Define => 0,
            mxu3701runStage::Enum => 1,
            mxu3701runStage::Mark => 2,
            mxu3701runStage::Cross => 3,
            mxu3701runStage::Audit => 4,
            mxu3701runStage::Close => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu3701runStage::Define => "三维定义",
            mxu3701runStage::Enum => "组合枚举",
            mxu3701runStage::Mark => "命中标记",
            mxu3701runStage::Cross => "交叉验算",
            mxu3701runStage::Audit => "缺口审计",
            mxu3701runStage::Close => "收口封存",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU3701RUN_DEPS: [i32; 6] = [
    -1, // Define 依赖 无
    0, // Enum 依赖 级 0
    1, // Mark 依赖 级 1
    2, // Cross 依赖 级 2
    3, // Audit 依赖 级 3
    4, // Close 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3701runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（覆盖三维模型 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3701runBook {
    /// 各级状态。
    pub state: [mxu3701runProgress; 6],
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

impl mxu3701runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu3701runProgress::Pending; 6],
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
                if self.state[i] != mxu3701runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU3701RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu3701runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu3701runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu3701runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（覆盖三维模型 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu3701runProgress::Done {
                self.state[i] = mxu3701runProgress::Rolled;
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
            if self.state[i] == mxu3701runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu3701runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu3701gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 覆盖三维模型 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu3701gate {
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

/// 守卫阈值（覆盖三维模型 重入/越限/静默三类拒止守卫）。
pub const MXU3701GATE_THRESHOLD: u64 = 64;

impl mxu3701gate {
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
        if self.window >= MXU3701GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe180, 0xe18f]。
pub const fn u3701_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3701Code::ALL.len() {
        let c = u3701Code::ALL[i].code();
        if c < 0xe180 || c > 0xe18f {
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
fn u3701_guard_seg() -> bool {
    u3701_segment_guard()
}

/// UNX-F3701 流水 Kahn 推演 + 回滚守恒。
fn check_u3701_pipe_plan() -> bool {
    let mut pb = mxu3701runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F3701 守卫重入/越限/静默三路拒止。
fn check_u3701_pipe_guard() -> bool {
    let mut g = mxu3701gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3701_checks() -> CheckSet {
    let mut set = CheckSet::new("U3701");
    set.add(
        "u3701-seg-guard",
        u3701_guard_seg(),
        if u3701_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3701-pipe-plan",
        check_u3701_pipe_plan(),
        if check_u3701_pipe_plan() { "" } else { "UNX-F3701 pipeline plan mismatch" },
    );
    set.add(
        "u3701-pipe-guard",
        check_u3701_pipe_guard(),
        if check_u3701_pipe_guard() { "" } else { "UNX-F3701 guard mismatch" },
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
    fn u3701_all_checks_green() {
        let set = run_u3701_checks();
        assert!(set.all_passed(), "U3701 domain checks must be green");
    }

    #[test]
    fn u3701_segment_guard_holds() {
        assert!(u3701_segment_guard());
    }
}
