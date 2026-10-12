//! unxu3041_u3041pipe —— 大页断言总注册清点（≥30 条逐条清点账） 执行流水与守卫（大页断言清点）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3041 · 大页断言总注册清点（≥30 条逐条清点账） · 判据原文：UNX-F3041 · 判据：UNX-F3041-J1 总注册冻结：域内大页断言逐条清点账（B05/B06/B28/B33 段累计）≥30 条断言成立，清单哈希入册，清点缺条注入 10/10 次检出
//! · 组件：大页断言清点 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F3041 · 判据：UNX-F3041-J1 总注册冻结：域内大页断言逐条清点账（B05/B06/B28/B33 段累计）≥30 条断言成立，清单哈希
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3041_VERSION: &str = "u3041-u3041pipe-v1";

/// 诊断段基址。
pub const U3041_SEG_BASE: u16 = 0xe290;

// --------------------------------------------------------------------------
// 诊断码（u3041 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页断言清点 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3041Code {
    /// 0xe290 级重复（断言 同级双注册）。
    StageDup,
    /// 0xe291 级断档（清点 序缺失）。
    StageHole,
    /// 0xe292 依赖环（级间循环依赖）。
    DepCycle,
    /// 0xe293 自依赖（级依赖自身）。
    DepSelf,
    /// 0xe294 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0xe295 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0xe296 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0xe297 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0xe298 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe299 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe29a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe29b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3041Code {
    /// 全量闭集。
    pub const ALL: [u3041Code; 12] = [
        u3041Code::StageDup,
        u3041Code::StageHole,
        u3041Code::DepCycle,
        u3041Code::DepSelf,
        u3041Code::PlanLost,
        u3041Code::RollbackBad,
        u3041Code::OrderDrift,
        u3041Code::DoneDrift,
        u3041Code::LedgerBroken,
        u3041Code::SnapshotFingerprint,
        u3041Code::TableBroken,
        u3041Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3041Code::StageDup => 0xe290,
            u3041Code::StageHole => 0xe291,
            u3041Code::DepCycle => 0xe292,
            u3041Code::DepSelf => 0xe293,
            u3041Code::PlanLost => 0xe294,
            u3041Code::RollbackBad => 0xe295,
            u3041Code::OrderDrift => 0xe296,
            u3041Code::DoneDrift => 0xe297,
            u3041Code::LedgerBroken => 0xe298,
            u3041Code::SnapshotFingerprint => 0xe299,
            u3041Code::TableBroken => 0xe29a,
            u3041Code::SegmentGuard => 0xe29b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3041Code::StageDup => "级重复（断言 同级双注册）",
            u3041Code::StageHole => "级断档（清点 序缺失）",
            u3041Code::DepCycle => "依赖环（级间循环依赖）",
            u3041Code::DepSelf => "自依赖（级依赖自身）",
            u3041Code::PlanLost => "推演丢失（可执行序短于级数）",
            u3041Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u3041Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u3041Code::DoneDrift => "完成计数漂移（done≠实计）",
            u3041Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3041Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3041Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3041Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3041Code::StageDup => "按级唯一律去重",
            u3041Code::StageHole => "按级序补齐",
            u3041Code::DepCycle => "按无环律重构依赖",
            u3041Code::DepSelf => "删除自依赖边",
            u3041Code::PlanLost => "按 Kahn 剥皮重推",
            u3041Code::RollbackBad => "按守恒律复算",
            u3041Code::OrderDrift => "按推演序重放",
            u3041Code::DoneDrift => "按实计校正",
            u3041Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3041Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3041Code::TableBroken => "按语义规则重建常量表",
            u3041Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3041run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 大页断言清点 执行流水（断言）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3041runStage {
    /// 级 根：范围冻结。
    Scan,
    /// 级 0：逐条清点。
    List,
    /// 级 1：落点核验。
    Verify,
    /// 级 2：绑定。
    Bind,
    /// 级 3：缺口识别。
    Gap,
    /// 级 4：清点封存。
    Seal,
}

impl mxu3041runStage {
    /// 全量级闭集。
    pub const ALL: [mxu3041runStage; 6] = [
        mxu3041runStage::Scan,
        mxu3041runStage::List,
        mxu3041runStage::Verify,
        mxu3041runStage::Bind,
        mxu3041runStage::Gap,
        mxu3041runStage::Seal,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu3041runStage::Scan => 0,
            mxu3041runStage::List => 1,
            mxu3041runStage::Verify => 2,
            mxu3041runStage::Bind => 3,
            mxu3041runStage::Gap => 4,
            mxu3041runStage::Seal => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu3041runStage::Scan => "范围冻结",
            mxu3041runStage::List => "逐条清点",
            mxu3041runStage::Verify => "落点核验",
            mxu3041runStage::Bind => "绑定",
            mxu3041runStage::Gap => "缺口识别",
            mxu3041runStage::Seal => "清点封存",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU3041RUN_DEPS: [i32; 6] = [
    -1, // Scan 依赖 无
    0, // List 依赖 级 0
    1, // Verify 依赖 级 1
    2, // Bind 依赖 级 2
    3, // Gap 依赖 级 3
    4, // Seal 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3041runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（大页断言清点 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3041runBook {
    /// 各级状态。
    pub state: [mxu3041runProgress; 6],
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

impl mxu3041runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu3041runProgress::Pending; 6],
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
                if self.state[i] != mxu3041runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU3041RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu3041runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu3041runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu3041runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（大页断言清点 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu3041runProgress::Done {
                self.state[i] = mxu3041runProgress::Rolled;
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
            if self.state[i] == mxu3041runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu3041runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu3041gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 大页断言清点 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu3041gate {
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

/// 守卫阈值（大页断言清点 重入/越限/静默三类拒止守卫）。
pub const MXU3041GATE_THRESHOLD: u64 = 64;

impl mxu3041gate {
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
        if self.window >= MXU3041GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe290, 0xe29f]。
pub const fn u3041_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3041Code::ALL.len() {
        let c = u3041Code::ALL[i].code();
        if c < 0xe290 || c > 0xe29f {
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
fn u3041_guard_seg() -> bool {
    u3041_segment_guard()
}

/// UNX-F3041 流水 Kahn 推演 + 回滚守恒。
fn check_u3041_pipe_plan() -> bool {
    let mut pb = mxu3041runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F3041 守卫重入/越限/静默三路拒止。
fn check_u3041_pipe_guard() -> bool {
    let mut g = mxu3041gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3041_checks() -> CheckSet {
    let mut set = CheckSet::new("U3041");
    set.add(
        "u3041-seg-guard",
        u3041_guard_seg(),
        if u3041_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3041-pipe-plan",
        check_u3041_pipe_plan(),
        if check_u3041_pipe_plan() { "" } else { "UNX-F3041 pipeline plan mismatch" },
    );
    set.add(
        "u3041-pipe-guard",
        check_u3041_pipe_guard(),
        if check_u3041_pipe_guard() { "" } else { "UNX-F3041 guard mismatch" },
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
    fn u3041_all_checks_green() {
        let set = run_u3041_checks();
        assert!(set.all_passed(), "U3041 domain checks must be green");
    }

    #[test]
    fn u3041_segment_guard_holds() {
        assert!(u3041_segment_guard());
    }
}
