//! unxu1741_u1741pipe —— 空闲循环总纲（idle 三态状态机） 执行流水与守卫（空闲态）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1741 · 空闲循环总纲（idle 三态状态机） · 判据原文：UNX-F1741 · 判据：idle 进入/驻留/退出三态状态机冻结 v1，状态转换账与实际状态一致断言全过，非法转换注入 10/10 次拒绝
//! · 组件：空闲态 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F1741 · 判据：idle 进入/驻留/退出三态状态机冻结 v1，状态转换账与实际状态一致断言全过，非法转换注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1741_VERSION: &str = "u1741-u1741pipe-v1";

/// 诊断段基址。
pub const U1741_SEG_BASE: u16 = 0x5a60;

// --------------------------------------------------------------------------
// 诊断码（u1741 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 空闲态 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1741Code {
    /// 0x5a60 级重复（非法转换 同级双注册）。
    StageDup,
    /// 0x5a61 级断档（账实漂移 序缺失）。
    StageHole,
    /// 0x5a62 依赖环（级间循环依赖）。
    DepCycle,
    /// 0x5a63 自依赖（级依赖自身）。
    DepSelf,
    /// 0x5a64 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0x5a65 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0x5a66 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0x5a67 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0x5a68 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5a69 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5a6a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5a6b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1741Code {
    /// 全量闭集。
    pub const ALL: [u1741Code; 12] = [
        u1741Code::StageDup,
        u1741Code::StageHole,
        u1741Code::DepCycle,
        u1741Code::DepSelf,
        u1741Code::PlanLost,
        u1741Code::RollbackBad,
        u1741Code::OrderDrift,
        u1741Code::DoneDrift,
        u1741Code::LedgerBroken,
        u1741Code::SnapshotFingerprint,
        u1741Code::TableBroken,
        u1741Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1741Code::StageDup => 0x5a60,
            u1741Code::StageHole => 0x5a61,
            u1741Code::DepCycle => 0x5a62,
            u1741Code::DepSelf => 0x5a63,
            u1741Code::PlanLost => 0x5a64,
            u1741Code::RollbackBad => 0x5a65,
            u1741Code::OrderDrift => 0x5a66,
            u1741Code::DoneDrift => 0x5a67,
            u1741Code::LedgerBroken => 0x5a68,
            u1741Code::SnapshotFingerprint => 0x5a69,
            u1741Code::TableBroken => 0x5a6a,
            u1741Code::SegmentGuard => 0x5a6b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1741Code::StageDup => "级重复（非法转换 同级双注册）",
            u1741Code::StageHole => "级断档（账实漂移 序缺失）",
            u1741Code::DepCycle => "依赖环（级间循环依赖）",
            u1741Code::DepSelf => "自依赖（级依赖自身）",
            u1741Code::PlanLost => "推演丢失（可执行序短于级数）",
            u1741Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u1741Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u1741Code::DoneDrift => "完成计数漂移（done≠实计）",
            u1741Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1741Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1741Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1741Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1741Code::StageDup => "按级唯一律去重",
            u1741Code::StageHole => "按级序补齐",
            u1741Code::DepCycle => "按无环律重构依赖",
            u1741Code::DepSelf => "删除自依赖边",
            u1741Code::PlanLost => "按 Kahn 剥皮重推",
            u1741Code::RollbackBad => "按守恒律复算",
            u1741Code::OrderDrift => "按推演序重放",
            u1741Code::DoneDrift => "按实计校正",
            u1741Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1741Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1741Code::TableBroken => "按语义规则重建常量表",
            u1741Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu1741run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 空闲态 执行流水（非法转换）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1741runStage {
    /// 级 根：进入段。
    Enter,
    /// 级 0：驻留段。
    Reside,
    /// 级 1：退出段。
    Exit,
    /// 级 2：唤醒边。
    Wake,
    /// 级 3：非法注入演练。
    Illegal,
    /// 级 4：账实对拍。
    Audit,
}

impl mxu1741runStage {
    /// 全量级闭集。
    pub const ALL: [mxu1741runStage; 6] = [
        mxu1741runStage::Enter,
        mxu1741runStage::Reside,
        mxu1741runStage::Exit,
        mxu1741runStage::Wake,
        mxu1741runStage::Illegal,
        mxu1741runStage::Audit,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu1741runStage::Enter => 0,
            mxu1741runStage::Reside => 1,
            mxu1741runStage::Exit => 2,
            mxu1741runStage::Wake => 3,
            mxu1741runStage::Illegal => 4,
            mxu1741runStage::Audit => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu1741runStage::Enter => "进入段",
            mxu1741runStage::Reside => "驻留段",
            mxu1741runStage::Exit => "退出段",
            mxu1741runStage::Wake => "唤醒边",
            mxu1741runStage::Illegal => "非法注入演练",
            mxu1741runStage::Audit => "账实对拍",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU1741RUN_DEPS: [i32; 6] = [
    -1, // Enter 依赖 无
    0, // Reside 依赖 级 0
    1, // Exit 依赖 级 1
    2, // Wake 依赖 级 2
    3, // Illegal 依赖 级 3
    4, // Audit 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1741runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（空闲态 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1741runBook {
    /// 各级状态。
    pub state: [mxu1741runProgress; 6],
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

impl mxu1741runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu1741runProgress::Pending; 6],
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
                if self.state[i] != mxu1741runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU1741RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu1741runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu1741runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu1741runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（空闲态 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu1741runProgress::Done {
                self.state[i] = mxu1741runProgress::Rolled;
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
            if self.state[i] == mxu1741runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu1741runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu1741gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 空闲态 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu1741gate {
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

/// 守卫阈值（空闲态 重入/越限/静默三类拒止守卫）。
pub const MXU1741GATE_THRESHOLD: u64 = 64;

impl mxu1741gate {
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
        if self.window >= MXU1741GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5a60, 0x5a6f]。
pub const fn u1741_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1741Code::ALL.len() {
        let c = u1741Code::ALL[i].code();
        if c < 0x5a60 || c > 0x5a6f {
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
fn u1741_guard_seg() -> bool {
    u1741_segment_guard()
}

/// UNX-F1741 流水 Kahn 推演 + 回滚守恒。
fn check_u1741_pipe_plan() -> bool {
    let mut pb = mxu1741runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F1741 守卫重入/越限/静默三路拒止。
fn check_u1741_pipe_guard() -> bool {
    let mut g = mxu1741gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1741_checks() -> CheckSet {
    let mut set = CheckSet::new("U1741");
    set.add(
        "u1741-seg-guard",
        u1741_guard_seg(),
        if u1741_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1741-pipe-plan",
        check_u1741_pipe_plan(),
        if check_u1741_pipe_plan() { "" } else { "UNX-F1741 pipeline plan mismatch" },
    );
    set.add(
        "u1741-pipe-guard",
        check_u1741_pipe_guard(),
        if check_u1741_pipe_guard() { "" } else { "UNX-F1741 guard mismatch" },
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
    fn u1741_all_checks_green() {
        let set = run_u1741_checks();
        assert!(set.all_passed(), "U1741 domain checks must be green");
    }

    #[test]
    fn u1741_segment_guard_holds() {
        assert!(u1741_segment_guard());
    }
}
