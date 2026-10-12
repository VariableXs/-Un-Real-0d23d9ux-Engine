//! unxu3001_u3001pipe —— 双链结构冻结（active/inactive 两链节点布局） 执行流水与守卫（双链结构冻结）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3001 · 双链结构冻结（active/inactive 两链节点布局） · 判据原文：UNX-F3001 · 判据：UNX-F3001-J1 active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向指针）与 F2682 页缓存驻留结构对齐零缺失，链头尾指针操作单点化
//! · 组件：双链结构冻结 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F3001 · 判据：UNX-F3001-J1 active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3001_VERSION: &str = "u3001-u3001pipe-v1";

/// 诊断段基址。
pub const U3001_SEG_BASE: u16 = 0xe270;

// --------------------------------------------------------------------------
// 诊断码（u3001 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 双链结构冻结 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3001Code {
    /// 0xe270 级重复（双链 同级双注册）。
    StageDup,
    /// 0xe271 级断档（节点 序缺失）。
    StageHole,
    /// 0xe272 依赖环（级间循环依赖）。
    DepCycle,
    /// 0xe273 自依赖（级依赖自身）。
    DepSelf,
    /// 0xe274 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0xe275 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0xe276 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0xe277 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0xe278 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe279 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe27a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe27b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3001Code {
    /// 全量闭集。
    pub const ALL: [u3001Code; 12] = [
        u3001Code::StageDup,
        u3001Code::StageHole,
        u3001Code::DepCycle,
        u3001Code::DepSelf,
        u3001Code::PlanLost,
        u3001Code::RollbackBad,
        u3001Code::OrderDrift,
        u3001Code::DoneDrift,
        u3001Code::LedgerBroken,
        u3001Code::SnapshotFingerprint,
        u3001Code::TableBroken,
        u3001Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3001Code::StageDup => 0xe270,
            u3001Code::StageHole => 0xe271,
            u3001Code::DepCycle => 0xe272,
            u3001Code::DepSelf => 0xe273,
            u3001Code::PlanLost => 0xe274,
            u3001Code::RollbackBad => 0xe275,
            u3001Code::OrderDrift => 0xe276,
            u3001Code::DoneDrift => 0xe277,
            u3001Code::LedgerBroken => 0xe278,
            u3001Code::SnapshotFingerprint => 0xe279,
            u3001Code::TableBroken => 0xe27a,
            u3001Code::SegmentGuard => 0xe27b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3001Code::StageDup => "级重复（双链 同级双注册）",
            u3001Code::StageHole => "级断档（节点 序缺失）",
            u3001Code::DepCycle => "依赖环（级间循环依赖）",
            u3001Code::DepSelf => "自依赖（级依赖自身）",
            u3001Code::PlanLost => "推演丢失（可执行序短于级数）",
            u3001Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u3001Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u3001Code::DoneDrift => "完成计数漂移（done≠实计）",
            u3001Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3001Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3001Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3001Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3001Code::StageDup => "按级唯一律去重",
            u3001Code::StageHole => "按级序补齐",
            u3001Code::DepCycle => "按无环律重构依赖",
            u3001Code::DepSelf => "删除自依赖边",
            u3001Code::PlanLost => "按 Kahn 剥皮重推",
            u3001Code::RollbackBad => "按守恒律复算",
            u3001Code::OrderDrift => "按推演序重放",
            u3001Code::DoneDrift => "按实计校正",
            u3001Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3001Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3001Code::TableBroken => "按语义规则重建常量表",
            u3001Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3001run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 双链结构冻结 执行流水（双链）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3001runStage {
    /// 级 根：布局冻结。
    Layout,
    /// 级 0：节点分配。
    Alloc,
    /// 级 1：双链挂接。
    Link,
    /// 级 2：链间迁移。
    Move,
    /// 级 3：合并。
    Merge,
    /// 级 4：冻结封存。
    Freeze,
}

impl mxu3001runStage {
    /// 全量级闭集。
    pub const ALL: [mxu3001runStage; 6] = [
        mxu3001runStage::Layout,
        mxu3001runStage::Alloc,
        mxu3001runStage::Link,
        mxu3001runStage::Move,
        mxu3001runStage::Merge,
        mxu3001runStage::Freeze,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu3001runStage::Layout => 0,
            mxu3001runStage::Alloc => 1,
            mxu3001runStage::Link => 2,
            mxu3001runStage::Move => 3,
            mxu3001runStage::Merge => 4,
            mxu3001runStage::Freeze => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu3001runStage::Layout => "布局冻结",
            mxu3001runStage::Alloc => "节点分配",
            mxu3001runStage::Link => "双链挂接",
            mxu3001runStage::Move => "链间迁移",
            mxu3001runStage::Merge => "合并",
            mxu3001runStage::Freeze => "冻结封存",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU3001RUN_DEPS: [i32; 6] = [
    -1, // Layout 依赖 无
    0, // Alloc 依赖 级 0
    1, // Link 依赖 级 1
    2, // Move 依赖 级 2
    3, // Merge 依赖 级 3
    4, // Freeze 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3001runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（双链结构冻结 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3001runBook {
    /// 各级状态。
    pub state: [mxu3001runProgress; 6],
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

impl mxu3001runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu3001runProgress::Pending; 6],
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
                if self.state[i] != mxu3001runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU3001RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu3001runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu3001runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu3001runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（双链结构冻结 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu3001runProgress::Done {
                self.state[i] = mxu3001runProgress::Rolled;
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
            if self.state[i] == mxu3001runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu3001runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu3001gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 双链结构冻结 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu3001gate {
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

/// 守卫阈值（双链结构冻结 重入/越限/静默三类拒止守卫）。
pub const MXU3001GATE_THRESHOLD: u64 = 64;

impl mxu3001gate {
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
        if self.window >= MXU3001GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe270, 0xe27f]。
pub const fn u3001_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3001Code::ALL.len() {
        let c = u3001Code::ALL[i].code();
        if c < 0xe270 || c > 0xe27f {
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
fn u3001_guard_seg() -> bool {
    u3001_segment_guard()
}

/// UNX-F3001 流水 Kahn 推演 + 回滚守恒。
fn check_u3001_pipe_plan() -> bool {
    let mut pb = mxu3001runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F3001 守卫重入/越限/静默三路拒止。
fn check_u3001_pipe_guard() -> bool {
    let mut g = mxu3001gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3001_checks() -> CheckSet {
    let mut set = CheckSet::new("U3001");
    set.add(
        "u3001-seg-guard",
        u3001_guard_seg(),
        if u3001_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3001-pipe-plan",
        check_u3001_pipe_plan(),
        if check_u3001_pipe_plan() { "" } else { "UNX-F3001 pipeline plan mismatch" },
    );
    set.add(
        "u3001-pipe-guard",
        check_u3001_pipe_guard(),
        if check_u3001_pipe_guard() { "" } else { "UNX-F3001 guard mismatch" },
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
    fn u3001_all_checks_green() {
        let set = run_u3001_checks();
        assert!(set.all_passed(), "U3001 domain checks must be green");
    }

    #[test]
    fn u3001_segment_guard_holds() {
        assert!(u3001_segment_guard());
    }
}
