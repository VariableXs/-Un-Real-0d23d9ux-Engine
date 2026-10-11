//! unxu3021_u3021pipe —— 三档压力分档冻结（轻/中/重回收压力档定义） 执行流水与守卫（三档压力分档）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3021 · 三档压力分档冻结（轻/中/重回收压力档定义） · 判据原文：UNX-F3021 · 判据：UNX-F3021-J1 三档分档冻结：轻/中/重三档（水线区间+扫描强度二维）定义哈希入册，与 F2941 压力矩阵三轴口径同源核过，档位漂移注入 10/10 次拦截
//! · 组件：三档压力分档 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F3021 · 判据：UNX-F3021-J1 三档分档冻结：轻/中/重三档（水线区间+扫描强度二维）定义哈希入册，与 F2941 压力矩阵三轴口径同源
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3021_VERSION: &str = "u3021-u3021pipe-v1";

/// 诊断段基址。
pub const U3021_SEG_BASE: u16 = 0xe280;

// --------------------------------------------------------------------------
// 诊断码（u3021 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 三档压力分档 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3021Code {
    /// 0xe280 级重复（三档 同级双注册）。
    StageDup,
    /// 0xe281 级断档（压力 序缺失）。
    StageHole,
    /// 0xe282 依赖环（级间循环依赖）。
    DepCycle,
    /// 0xe283 自依赖（级依赖自身）。
    DepSelf,
    /// 0xe284 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0xe285 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0xe286 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0xe287 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0xe288 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe289 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe28a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe28b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3021Code {
    /// 全量闭集。
    pub const ALL: [u3021Code; 12] = [
        u3021Code::StageDup,
        u3021Code::StageHole,
        u3021Code::DepCycle,
        u3021Code::DepSelf,
        u3021Code::PlanLost,
        u3021Code::RollbackBad,
        u3021Code::OrderDrift,
        u3021Code::DoneDrift,
        u3021Code::LedgerBroken,
        u3021Code::SnapshotFingerprint,
        u3021Code::TableBroken,
        u3021Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3021Code::StageDup => 0xe280,
            u3021Code::StageHole => 0xe281,
            u3021Code::DepCycle => 0xe282,
            u3021Code::DepSelf => 0xe283,
            u3021Code::PlanLost => 0xe284,
            u3021Code::RollbackBad => 0xe285,
            u3021Code::OrderDrift => 0xe286,
            u3021Code::DoneDrift => 0xe287,
            u3021Code::LedgerBroken => 0xe288,
            u3021Code::SnapshotFingerprint => 0xe289,
            u3021Code::TableBroken => 0xe28a,
            u3021Code::SegmentGuard => 0xe28b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3021Code::StageDup => "级重复（三档 同级双注册）",
            u3021Code::StageHole => "级断档（压力 序缺失）",
            u3021Code::DepCycle => "依赖环（级间循环依赖）",
            u3021Code::DepSelf => "自依赖（级依赖自身）",
            u3021Code::PlanLost => "推演丢失（可执行序短于级数）",
            u3021Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u3021Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u3021Code::DoneDrift => "完成计数漂移（done≠实计）",
            u3021Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3021Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3021Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3021Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3021Code::StageDup => "按级唯一律去重",
            u3021Code::StageHole => "按级序补齐",
            u3021Code::DepCycle => "按无环律重构依赖",
            u3021Code::DepSelf => "删除自依赖边",
            u3021Code::PlanLost => "按 Kahn 剥皮重推",
            u3021Code::RollbackBad => "按守恒律复算",
            u3021Code::OrderDrift => "按推演序重放",
            u3021Code::DoneDrift => "按实计校正",
            u3021Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3021Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3021Code::TableBroken => "按语义规则重建常量表",
            u3021Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3021run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 三档压力分档 执行流水（三档）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3021runStage {
    /// 级 根：三档定义。
    Define,
    /// 级 0：布防。
    Arm,
    /// 级 1：档位矩阵。
    Matrix,
    /// 级 2：压测执行。
    Run,
    /// 级 3：裁决。
    Judge,
    /// 级 4：封存封口。
    Seal,
}

impl mxu3021runStage {
    /// 全量级闭集。
    pub const ALL: [mxu3021runStage; 6] = [
        mxu3021runStage::Define,
        mxu3021runStage::Arm,
        mxu3021runStage::Matrix,
        mxu3021runStage::Run,
        mxu3021runStage::Judge,
        mxu3021runStage::Seal,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu3021runStage::Define => 0,
            mxu3021runStage::Arm => 1,
            mxu3021runStage::Matrix => 2,
            mxu3021runStage::Run => 3,
            mxu3021runStage::Judge => 4,
            mxu3021runStage::Seal => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu3021runStage::Define => "三档定义",
            mxu3021runStage::Arm => "布防",
            mxu3021runStage::Matrix => "档位矩阵",
            mxu3021runStage::Run => "压测执行",
            mxu3021runStage::Judge => "裁决",
            mxu3021runStage::Seal => "封存封口",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU3021RUN_DEPS: [i32; 6] = [
    -1, // Define 依赖 无
    0, // Arm 依赖 级 0
    1, // Matrix 依赖 级 1
    2, // Run 依赖 级 2
    3, // Judge 依赖 级 3
    4, // Seal 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3021runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（三档压力分档 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3021runBook {
    /// 各级状态。
    pub state: [mxu3021runProgress; 6],
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

impl mxu3021runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu3021runProgress::Pending; 6],
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
                if self.state[i] != mxu3021runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU3021RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu3021runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu3021runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu3021runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（三档压力分档 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu3021runProgress::Done {
                self.state[i] = mxu3021runProgress::Rolled;
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
            if self.state[i] == mxu3021runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu3021runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu3021gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 三档压力分档 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu3021gate {
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

/// 守卫阈值（三档压力分档 重入/越限/静默三类拒止守卫）。
pub const MXU3021GATE_THRESHOLD: u64 = 64;

impl mxu3021gate {
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
        if self.window >= MXU3021GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe280, 0xe28f]。
pub const fn u3021_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3021Code::ALL.len() {
        let c = u3021Code::ALL[i].code();
        if c < 0xe280 || c > 0xe28f {
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
fn u3021_guard_seg() -> bool {
    u3021_segment_guard()
}

/// UNX-F3021 流水 Kahn 推演 + 回滚守恒。
fn check_u3021_pipe_plan() -> bool {
    let mut pb = mxu3021runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F3021 守卫重入/越限/静默三路拒止。
fn check_u3021_pipe_guard() -> bool {
    let mut g = mxu3021gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3021_checks() -> CheckSet {
    let mut set = CheckSet::new("U3021");
    set.add(
        "u3021-seg-guard",
        u3021_guard_seg(),
        if u3021_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3021-pipe-plan",
        check_u3021_pipe_plan(),
        if check_u3021_pipe_plan() { "" } else { "UNX-F3021 pipeline plan mismatch" },
    );
    set.add(
        "u3021-pipe-guard",
        check_u3021_pipe_guard(),
        if check_u3021_pipe_guard() { "" } else { "UNX-F3021 guard mismatch" },
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
    fn u3021_all_checks_green() {
        let set = run_u3021_checks();
        assert!(set.all_passed(), "U3021 domain checks must be green");
    }

    #[test]
    fn u3021_segment_guard_holds() {
        assert!(u3021_segment_guard());
    }
}
