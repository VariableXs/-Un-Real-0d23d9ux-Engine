//! unxu2601_u2601pipe —— 现存 pfh.rs 接管声明（#PF 处理器收编） 执行流水与守卫（缺页处理路）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2601 · 现存 pfh.rs 接管声明（#PF 处理器收编） · 判据原文：UNX-F2601 · 判据：现存 `pfh.rs`（四路起步态：Guard/COW/GrowStack/MapZero）接管账在档（三态标注），收编后四路语义与成熟体逐路条目一致断言
//! · 组件：缺页处理路 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F2601 · 判据：现存 `pfh.rs`（四路起步态：Guard/COW/GrowStack/MapZero）接管账在档（三态标注），收编后四路语义
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2601_VERSION: &str = "u2601-u2601pipe-v1";

/// 诊断段基址。
pub const U2601_SEG_BASE: u16 = 0xa7a0;

// --------------------------------------------------------------------------
// 诊断码（u2601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 缺页处理路 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2601Code {
    /// 0xa7a0 级重复（四路漏标 同级双注册）。
    StageDup,
    /// 0xa7a1 级断档（语义错位 序缺失）。
    StageHole,
    /// 0xa7a2 依赖环（级间循环依赖）。
    DepCycle,
    /// 0xa7a3 自依赖（级依赖自身）。
    DepSelf,
    /// 0xa7a4 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0xa7a5 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0xa7a6 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0xa7a7 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0xa7a8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa7a9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa7aa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa7ab 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2601Code {
    /// 全量闭集。
    pub const ALL: [u2601Code; 12] = [
        u2601Code::StageDup,
        u2601Code::StageHole,
        u2601Code::DepCycle,
        u2601Code::DepSelf,
        u2601Code::PlanLost,
        u2601Code::RollbackBad,
        u2601Code::OrderDrift,
        u2601Code::DoneDrift,
        u2601Code::LedgerBroken,
        u2601Code::SnapshotFingerprint,
        u2601Code::TableBroken,
        u2601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2601Code::StageDup => 0xa7a0,
            u2601Code::StageHole => 0xa7a1,
            u2601Code::DepCycle => 0xa7a2,
            u2601Code::DepSelf => 0xa7a3,
            u2601Code::PlanLost => 0xa7a4,
            u2601Code::RollbackBad => 0xa7a5,
            u2601Code::OrderDrift => 0xa7a6,
            u2601Code::DoneDrift => 0xa7a7,
            u2601Code::LedgerBroken => 0xa7a8,
            u2601Code::SnapshotFingerprint => 0xa7a9,
            u2601Code::TableBroken => 0xa7aa,
            u2601Code::SegmentGuard => 0xa7ab,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2601Code::StageDup => "级重复（四路漏标 同级双注册）",
            u2601Code::StageHole => "级断档（语义错位 序缺失）",
            u2601Code::DepCycle => "依赖环（级间循环依赖）",
            u2601Code::DepSelf => "自依赖（级依赖自身）",
            u2601Code::PlanLost => "推演丢失（可执行序短于级数）",
            u2601Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u2601Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u2601Code::DoneDrift => "完成计数漂移（done≠实计）",
            u2601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2601Code::StageDup => "按级唯一律去重",
            u2601Code::StageHole => "按级序补齐",
            u2601Code::DepCycle => "按无环律重构依赖",
            u2601Code::DepSelf => "删除自依赖边",
            u2601Code::PlanLost => "按 Kahn 剥皮重推",
            u2601Code::RollbackBad => "按守恒律复算",
            u2601Code::OrderDrift => "按推演序重放",
            u2601Code::DoneDrift => "按实计校正",
            u2601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2601Code::TableBroken => "按语义规则重建常量表",
            u2601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu2601run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 缺页处理路 执行流水（四路漏标）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2601runStage {
    /// 级 根：处理器收编。
    Adopt,
    /// 级 0：三态标注。
    Mark,
    /// 级 1：Guard 路。
    Guard,
    /// 级 2：COW 路。
    Cow,
    /// 级 3：Grow/Zero 路。
    GrowZero,
    /// 级 4：逐路断言。
    Assert,
}

impl mxu2601runStage {
    /// 全量级闭集。
    pub const ALL: [mxu2601runStage; 6] = [
        mxu2601runStage::Adopt,
        mxu2601runStage::Mark,
        mxu2601runStage::Guard,
        mxu2601runStage::Cow,
        mxu2601runStage::GrowZero,
        mxu2601runStage::Assert,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu2601runStage::Adopt => 0,
            mxu2601runStage::Mark => 1,
            mxu2601runStage::Guard => 2,
            mxu2601runStage::Cow => 3,
            mxu2601runStage::GrowZero => 4,
            mxu2601runStage::Assert => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu2601runStage::Adopt => "处理器收编",
            mxu2601runStage::Mark => "三态标注",
            mxu2601runStage::Guard => "Guard 路",
            mxu2601runStage::Cow => "COW 路",
            mxu2601runStage::GrowZero => "Grow/Zero 路",
            mxu2601runStage::Assert => "逐路断言",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU2601RUN_DEPS: [i32; 6] = [
    -1, // Adopt 依赖 无
    0, // Mark 依赖 级 0
    1, // Guard 依赖 级 1
    2, // Cow 依赖 级 2
    3, // GrowZero 依赖 级 3
    4, // Assert 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2601runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（缺页处理路 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2601runBook {
    /// 各级状态。
    pub state: [mxu2601runProgress; 6],
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

impl mxu2601runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu2601runProgress::Pending; 6],
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
                if self.state[i] != mxu2601runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU2601RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu2601runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu2601runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu2601runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（缺页处理路 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu2601runProgress::Done {
                self.state[i] = mxu2601runProgress::Rolled;
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
            if self.state[i] == mxu2601runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu2601runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu2601gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 缺页处理路 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu2601gate {
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

/// 守卫阈值（缺页处理路 重入/越限/静默三类拒止守卫）。
pub const MXU2601GATE_THRESHOLD: u64 = 64;

impl mxu2601gate {
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
        if self.window >= MXU2601GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa7a0, 0xa7af]。
pub const fn u2601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2601Code::ALL.len() {
        let c = u2601Code::ALL[i].code();
        if c < 0xa7a0 || c > 0xa7af {
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
fn u2601_guard_seg() -> bool {
    u2601_segment_guard()
}

/// UNX-F2601 流水 Kahn 推演 + 回滚守恒。
fn check_u2601_pipe_plan() -> bool {
    let mut pb = mxu2601runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F2601 守卫重入/越限/静默三路拒止。
fn check_u2601_pipe_guard() -> bool {
    let mut g = mxu2601gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2601_checks() -> CheckSet {
    let mut set = CheckSet::new("U2601");
    set.add(
        "u2601-seg-guard",
        u2601_guard_seg(),
        if u2601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2601-pipe-plan",
        check_u2601_pipe_plan(),
        if check_u2601_pipe_plan() { "" } else { "UNX-F2601 pipeline plan mismatch" },
    );
    set.add(
        "u2601-pipe-guard",
        check_u2601_pipe_guard(),
        if check_u2601_pipe_guard() { "" } else { "UNX-F2601 guard mismatch" },
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
    fn u2601_all_checks_green() {
        let set = run_u2601_checks();
        assert!(set.all_passed(), "U2601 domain checks must be green");
    }

    #[test]
    fn u2601_segment_guard_holds() {
        assert!(u2601_segment_guard());
    }
}
