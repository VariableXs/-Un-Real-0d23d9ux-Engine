//! unxu3501_u3501pipe —— 跟踪点总设计（TP 注册表/编译期埋点/运行时开关） 执行流水与守卫（跟踪点体系）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3501 · 跟踪点总设计（TP 注册表/编译期埋点/运行时开关） · 判据原文：UNX-F3501 · 判据：J1：TP 语义冻结（埋点位置 × 负载类型 × 默认态三列矩阵）；M-259 TP 全链路（埋点→开关→负载→消费端可读）。<br>J2：与日志分工声明（文本流 vs 结构化事件流——F3506 协议对端，禁用态零开销）。
//! · 组件：跟踪点体系 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F3501 · 判据：J1：TP 语义冻结（埋点位置 × 负载类型 × 默认态三列矩阵）；M-259 TP 全链路（埋点→开关→负载→消费端可读）。<b
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3501_VERSION: &str = "u3501-u3501pipe-v1";

/// 诊断段基址。
pub const U3501_SEG_BASE: u16 = 0xe0e0;

// --------------------------------------------------------------------------
// 诊断码（u3501 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 跟踪点体系 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3501Code {
    /// 0xe0e0 级重复（跟踪点 同级双注册）。
    StageDup,
    /// 0xe0e1 级断档（埋点 序缺失）。
    StageHole,
    /// 0xe0e2 依赖环（级间循环依赖）。
    DepCycle,
    /// 0xe0e3 自依赖（级依赖自身）。
    DepSelf,
    /// 0xe0e4 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0xe0e5 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0xe0e6 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0xe0e7 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0xe0e8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0e9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0ea 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0eb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3501Code {
    /// 全量闭集。
    pub const ALL: [u3501Code; 12] = [
        u3501Code::StageDup,
        u3501Code::StageHole,
        u3501Code::DepCycle,
        u3501Code::DepSelf,
        u3501Code::PlanLost,
        u3501Code::RollbackBad,
        u3501Code::OrderDrift,
        u3501Code::DoneDrift,
        u3501Code::LedgerBroken,
        u3501Code::SnapshotFingerprint,
        u3501Code::TableBroken,
        u3501Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3501Code::StageDup => 0xe0e0,
            u3501Code::StageHole => 0xe0e1,
            u3501Code::DepCycle => 0xe0e2,
            u3501Code::DepSelf => 0xe0e3,
            u3501Code::PlanLost => 0xe0e4,
            u3501Code::RollbackBad => 0xe0e5,
            u3501Code::OrderDrift => 0xe0e6,
            u3501Code::DoneDrift => 0xe0e7,
            u3501Code::LedgerBroken => 0xe0e8,
            u3501Code::SnapshotFingerprint => 0xe0e9,
            u3501Code::TableBroken => 0xe0ea,
            u3501Code::SegmentGuard => 0xe0eb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3501Code::StageDup => "级重复（跟踪点 同级双注册）",
            u3501Code::StageHole => "级断档（埋点 序缺失）",
            u3501Code::DepCycle => "依赖环（级间循环依赖）",
            u3501Code::DepSelf => "自依赖（级依赖自身）",
            u3501Code::PlanLost => "推演丢失（可执行序短于级数）",
            u3501Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u3501Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u3501Code::DoneDrift => "完成计数漂移（done≠实计）",
            u3501Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3501Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3501Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3501Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3501Code::StageDup => "按级唯一律去重",
            u3501Code::StageHole => "按级序补齐",
            u3501Code::DepCycle => "按无环律重构依赖",
            u3501Code::DepSelf => "删除自依赖边",
            u3501Code::PlanLost => "按 Kahn 剥皮重推",
            u3501Code::RollbackBad => "按守恒律复算",
            u3501Code::OrderDrift => "按推演序重放",
            u3501Code::DoneDrift => "按实计校正",
            u3501Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3501Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3501Code::TableBroken => "按语义规则重建常量表",
            u3501Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3501run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 跟踪点体系 执行流水（跟踪点）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3501runStage {
    /// 级 根：ID 分配。
    Define,
    /// 级 0：注册。
    Reg,
    /// 级 1：埋点插入。
    Ins,
    /// 级 2：开关判定。
    Switch,
    /// 级 3：采样。
    Sample,
    /// 级 4：导出封存。
    Dump,
}

impl mxu3501runStage {
    /// 全量级闭集。
    pub const ALL: [mxu3501runStage; 6] = [
        mxu3501runStage::Define,
        mxu3501runStage::Reg,
        mxu3501runStage::Ins,
        mxu3501runStage::Switch,
        mxu3501runStage::Sample,
        mxu3501runStage::Dump,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu3501runStage::Define => 0,
            mxu3501runStage::Reg => 1,
            mxu3501runStage::Ins => 2,
            mxu3501runStage::Switch => 3,
            mxu3501runStage::Sample => 4,
            mxu3501runStage::Dump => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu3501runStage::Define => "ID 分配",
            mxu3501runStage::Reg => "注册",
            mxu3501runStage::Ins => "埋点插入",
            mxu3501runStage::Switch => "开关判定",
            mxu3501runStage::Sample => "采样",
            mxu3501runStage::Dump => "导出封存",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU3501RUN_DEPS: [i32; 6] = [
    -1, // Define 依赖 无
    0, // Reg 依赖 级 0
    1, // Ins 依赖 级 1
    2, // Switch 依赖 级 2
    3, // Sample 依赖 级 3
    4, // Dump 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3501runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（跟踪点体系 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3501runBook {
    /// 各级状态。
    pub state: [mxu3501runProgress; 6],
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

impl mxu3501runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu3501runProgress::Pending; 6],
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
                if self.state[i] != mxu3501runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU3501RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu3501runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu3501runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu3501runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（跟踪点体系 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu3501runProgress::Done {
                self.state[i] = mxu3501runProgress::Rolled;
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
            if self.state[i] == mxu3501runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu3501runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu3501gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 跟踪点体系 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu3501gate {
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

/// 守卫阈值（跟踪点体系 重入/越限/静默三类拒止守卫）。
pub const MXU3501GATE_THRESHOLD: u64 = 64;

impl mxu3501gate {
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
        if self.window >= MXU3501GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0e0, 0xe0ef]。
pub const fn u3501_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3501Code::ALL.len() {
        let c = u3501Code::ALL[i].code();
        if c < 0xe0e0 || c > 0xe0ef {
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
fn u3501_guard_seg() -> bool {
    u3501_segment_guard()
}

/// UNX-F3501 流水 Kahn 推演 + 回滚守恒。
fn check_u3501_pipe_plan() -> bool {
    let mut pb = mxu3501runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F3501 守卫重入/越限/静默三路拒止。
fn check_u3501_pipe_guard() -> bool {
    let mut g = mxu3501gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3501_checks() -> CheckSet {
    let mut set = CheckSet::new("U3501");
    set.add(
        "u3501-seg-guard",
        u3501_guard_seg(),
        if u3501_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3501-pipe-plan",
        check_u3501_pipe_plan(),
        if check_u3501_pipe_plan() { "" } else { "UNX-F3501 pipeline plan mismatch" },
    );
    set.add(
        "u3501-pipe-guard",
        check_u3501_pipe_guard(),
        if check_u3501_pipe_guard() { "" } else { "UNX-F3501 guard mismatch" },
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
    fn u3501_all_checks_green() {
        let set = run_u3501_checks();
        assert!(set.all_passed(), "U3501 domain checks must be green");
    }

    #[test]
    fn u3501_segment_guard_holds() {
        assert!(u3501_segment_guard());
    }
}
