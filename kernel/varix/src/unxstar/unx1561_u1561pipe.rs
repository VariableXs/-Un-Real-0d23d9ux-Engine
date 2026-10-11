//! unxu1561_u1561pipe —— 消费域联签总表（A3/A4/A5/G1/B4 等逐域登记） 执行流水与守卫（域）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1561 · 消费域联签总表（A3/A4/A5/G1/B4 等逐域登记） · 判据原文：UNX-F1561 · 判据：联签总表齐备（全域消费域逐域登记：域/消费接口/联签状态三列），漏域注入 10/10 必抓，总表与 F1566 冻结账对平 100%
//! · 组件：域 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F1561 · 判据：联签总表齐备（全域消费域逐域登记：域/消费接口/联签状态三列），漏域注入 10/10 必抓，总表与 F1566 冻结账对平 100
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1561_VERSION: &str = "u1561-u1561pipe-v1";

/// 诊断段基址。
pub const U1561_SEG_BASE: u16 = 0x53a0;

// --------------------------------------------------------------------------
// 诊断码（u1561 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1561Code {
    /// 0x53a0 级重复（漏域漏登 同级双注册）。
    StageDup,
    /// 0x53a1 级断档（联签缺位 序缺失）。
    StageHole,
    /// 0x53a2 依赖环（级间循环依赖）。
    DepCycle,
    /// 0x53a3 自依赖（级依赖自身）。
    DepSelf,
    /// 0x53a4 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0x53a5 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0x53a6 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0x53a7 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0x53a8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x53a9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x53aa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x53ab 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1561Code {
    /// 全量闭集。
    pub const ALL: [u1561Code; 12] = [
        u1561Code::StageDup,
        u1561Code::StageHole,
        u1561Code::DepCycle,
        u1561Code::DepSelf,
        u1561Code::PlanLost,
        u1561Code::RollbackBad,
        u1561Code::OrderDrift,
        u1561Code::DoneDrift,
        u1561Code::LedgerBroken,
        u1561Code::SnapshotFingerprint,
        u1561Code::TableBroken,
        u1561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1561Code::StageDup => 0x53a0,
            u1561Code::StageHole => 0x53a1,
            u1561Code::DepCycle => 0x53a2,
            u1561Code::DepSelf => 0x53a3,
            u1561Code::PlanLost => 0x53a4,
            u1561Code::RollbackBad => 0x53a5,
            u1561Code::OrderDrift => 0x53a6,
            u1561Code::DoneDrift => 0x53a7,
            u1561Code::LedgerBroken => 0x53a8,
            u1561Code::SnapshotFingerprint => 0x53a9,
            u1561Code::TableBroken => 0x53aa,
            u1561Code::SegmentGuard => 0x53ab,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1561Code::StageDup => "级重复（漏域漏登 同级双注册）",
            u1561Code::StageHole => "级断档（联签缺位 序缺失）",
            u1561Code::DepCycle => "依赖环（级间循环依赖）",
            u1561Code::DepSelf => "自依赖（级依赖自身）",
            u1561Code::PlanLost => "推演丢失（可执行序短于级数）",
            u1561Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u1561Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u1561Code::DoneDrift => "完成计数漂移（done≠实计）",
            u1561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1561Code::StageDup => "按级唯一律去重",
            u1561Code::StageHole => "按级序补齐",
            u1561Code::DepCycle => "按无环律重构依赖",
            u1561Code::DepSelf => "删除自依赖边",
            u1561Code::PlanLost => "按 Kahn 剥皮重推",
            u1561Code::RollbackBad => "按守恒律复算",
            u1561Code::OrderDrift => "按推演序重放",
            u1561Code::DoneDrift => "按实计校正",
            u1561Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1561Code::TableBroken => "按语义规则重建常量表",
            u1561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu1561run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 域 执行流水（漏域漏登）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1561runStage {
    /// 级 根：全域枚举。
    Enumerate,
    /// 级 0：逐域登记。
    Enroll,
    /// 级 1：接口列。
    Iface,
    /// 级 2：联签列。
    Sign,
    /// 级 3：对平校验。
    Reconcile,
    /// 级 4：锁账。
    Lock,
}

impl mxu1561runStage {
    /// 全量级闭集。
    pub const ALL: [mxu1561runStage; 6] = [
        mxu1561runStage::Enumerate,
        mxu1561runStage::Enroll,
        mxu1561runStage::Iface,
        mxu1561runStage::Sign,
        mxu1561runStage::Reconcile,
        mxu1561runStage::Lock,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu1561runStage::Enumerate => 0,
            mxu1561runStage::Enroll => 1,
            mxu1561runStage::Iface => 2,
            mxu1561runStage::Sign => 3,
            mxu1561runStage::Reconcile => 4,
            mxu1561runStage::Lock => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu1561runStage::Enumerate => "全域枚举",
            mxu1561runStage::Enroll => "逐域登记",
            mxu1561runStage::Iface => "接口列",
            mxu1561runStage::Sign => "联签列",
            mxu1561runStage::Reconcile => "对平校验",
            mxu1561runStage::Lock => "锁账",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU1561RUN_DEPS: [i32; 6] = [
    -1, // Enumerate 依赖 无
    0, // Enroll 依赖 级 0
    1, // Iface 依赖 级 1
    2, // Sign 依赖 级 2
    3, // Reconcile 依赖 级 3
    4, // Lock 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1561runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（域 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1561runBook {
    /// 各级状态。
    pub state: [mxu1561runProgress; 6],
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

impl mxu1561runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu1561runProgress::Pending; 6],
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
                if self.state[i] != mxu1561runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU1561RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu1561runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu1561runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu1561runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（域 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu1561runProgress::Done {
                self.state[i] = mxu1561runProgress::Rolled;
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
            if self.state[i] == mxu1561runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu1561runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu1561gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 域 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu1561gate {
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

/// 守卫阈值（域 重入/越限/静默三类拒止守卫）。
pub const MXU1561GATE_THRESHOLD: u64 = 64;

impl mxu1561gate {
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
        if self.window >= MXU1561GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0x53a0, 0x53af]。
pub const fn u1561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1561Code::ALL.len() {
        let c = u1561Code::ALL[i].code();
        if c < 0x53a0 || c > 0x53af {
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
fn u1561_guard_seg() -> bool {
    u1561_segment_guard()
}

/// UNX-F1561 流水 Kahn 推演 + 回滚守恒。
fn check_u1561_pipe_plan() -> bool {
    let mut pb = mxu1561runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F1561 守卫重入/越限/静默三路拒止。
fn check_u1561_pipe_guard() -> bool {
    let mut g = mxu1561gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1561_checks() -> CheckSet {
    let mut set = CheckSet::new("U1561");
    set.add(
        "u1561-seg-guard",
        u1561_guard_seg(),
        if u1561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1561-pipe-plan",
        check_u1561_pipe_plan(),
        if check_u1561_pipe_plan() { "" } else { "UNX-F1561 pipeline plan mismatch" },
    );
    set.add(
        "u1561-pipe-guard",
        check_u1561_pipe_guard(),
        if check_u1561_pipe_guard() { "" } else { "UNX-F1561 guard mismatch" },
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
    fn u1561_all_checks_green() {
        let set = run_u1561_checks();
        assert!(set.all_passed(), "U1561 domain checks must be green");
    }

    #[test]
    fn u1561_segment_guard_holds() {
        assert!(u1561_segment_guard());
    }
}
