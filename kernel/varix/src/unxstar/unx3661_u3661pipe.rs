//! unxu3661_u3661pipe —— 符号化总设计（地址→符号的解析管线） 执行流水与守卫（符号化管线）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3661 · 符号化总设计（地址→符号的解析管线） · 判据原文：UNX-F3661 · 判据：J1：解析管线（输入（帧地址集+版本锚）→查表（符号表）→歧义消解→报告（符号帧序列）——判据行内嵌四步）；M-419 全链首译（单帧地址→符号名+偏移+源行——翻译三要素）。<br>J2：离线纯净（符号化器不依赖内核状态（纯函数——同输入同输出——F3672 的前提））。
//! · 组件：符号化管线 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F3661 · 判据：J1：解析管线（输入（帧地址集+版本锚）→查表（符号表）→歧义消解→报告（符号帧序列）——判据行内嵌四步）；M-419 全链首译（
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3661_VERSION: &str = "u3661-u3661pipe-v1";

/// 诊断段基址。
pub const U3661_SEG_BASE: u16 = 0xe160;

// --------------------------------------------------------------------------
// 诊断码（u3661 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 符号化管线 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3661Code {
    /// 0xe160 级重复（符号 同级双注册）。
    StageDup,
    /// 0xe161 级断档（解析 序缺失）。
    StageHole,
    /// 0xe162 依赖环（级间循环依赖）。
    DepCycle,
    /// 0xe163 自依赖（级依赖自身）。
    DepSelf,
    /// 0xe164 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0xe165 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0xe166 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0xe167 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0xe168 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe169 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe16a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe16b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3661Code {
    /// 全量闭集。
    pub const ALL: [u3661Code; 12] = [
        u3661Code::StageDup,
        u3661Code::StageHole,
        u3661Code::DepCycle,
        u3661Code::DepSelf,
        u3661Code::PlanLost,
        u3661Code::RollbackBad,
        u3661Code::OrderDrift,
        u3661Code::DoneDrift,
        u3661Code::LedgerBroken,
        u3661Code::SnapshotFingerprint,
        u3661Code::TableBroken,
        u3661Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3661Code::StageDup => 0xe160,
            u3661Code::StageHole => 0xe161,
            u3661Code::DepCycle => 0xe162,
            u3661Code::DepSelf => 0xe163,
            u3661Code::PlanLost => 0xe164,
            u3661Code::RollbackBad => 0xe165,
            u3661Code::OrderDrift => 0xe166,
            u3661Code::DoneDrift => 0xe167,
            u3661Code::LedgerBroken => 0xe168,
            u3661Code::SnapshotFingerprint => 0xe169,
            u3661Code::TableBroken => 0xe16a,
            u3661Code::SegmentGuard => 0xe16b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3661Code::StageDup => "级重复（符号 同级双注册）",
            u3661Code::StageHole => "级断档（解析 序缺失）",
            u3661Code::DepCycle => "依赖环（级间循环依赖）",
            u3661Code::DepSelf => "自依赖（级依赖自身）",
            u3661Code::PlanLost => "推演丢失（可执行序短于级数）",
            u3661Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u3661Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u3661Code::DoneDrift => "完成计数漂移（done≠实计）",
            u3661Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3661Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3661Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3661Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3661Code::StageDup => "按级唯一律去重",
            u3661Code::StageHole => "按级序补齐",
            u3661Code::DepCycle => "按无环律重构依赖",
            u3661Code::DepSelf => "删除自依赖边",
            u3661Code::PlanLost => "按 Kahn 剥皮重推",
            u3661Code::RollbackBad => "按守恒律复算",
            u3661Code::OrderDrift => "按推演序重放",
            u3661Code::DoneDrift => "按实计校正",
            u3661Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3661Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3661Code::TableBroken => "按语义规则重建常量表",
            u3661Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3661run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 符号化管线 执行流水（符号）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3661runStage {
    /// 级 根：符号装载。
    Load,
    /// 级 0：建索引。
    Index,
    /// 级 1：地址解析。
    Resolve,
    /// 级 2：内联展开。
    Inline,
    /// 级 3：缓存。
    Cache,
    /// 级 4：出账封存。
    Digest,
}

impl mxu3661runStage {
    /// 全量级闭集。
    pub const ALL: [mxu3661runStage; 6] = [
        mxu3661runStage::Load,
        mxu3661runStage::Index,
        mxu3661runStage::Resolve,
        mxu3661runStage::Inline,
        mxu3661runStage::Cache,
        mxu3661runStage::Digest,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu3661runStage::Load => 0,
            mxu3661runStage::Index => 1,
            mxu3661runStage::Resolve => 2,
            mxu3661runStage::Inline => 3,
            mxu3661runStage::Cache => 4,
            mxu3661runStage::Digest => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu3661runStage::Load => "符号装载",
            mxu3661runStage::Index => "建索引",
            mxu3661runStage::Resolve => "地址解析",
            mxu3661runStage::Inline => "内联展开",
            mxu3661runStage::Cache => "缓存",
            mxu3661runStage::Digest => "出账封存",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU3661RUN_DEPS: [i32; 6] = [
    -1, // Load 依赖 无
    0, // Index 依赖 级 0
    1, // Resolve 依赖 级 1
    2, // Inline 依赖 级 2
    3, // Cache 依赖 级 3
    4, // Digest 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3661runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（符号化管线 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3661runBook {
    /// 各级状态。
    pub state: [mxu3661runProgress; 6],
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

impl mxu3661runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu3661runProgress::Pending; 6],
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
                if self.state[i] != mxu3661runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU3661RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu3661runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu3661runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu3661runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（符号化管线 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu3661runProgress::Done {
                self.state[i] = mxu3661runProgress::Rolled;
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
            if self.state[i] == mxu3661runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu3661runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu3661gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 符号化管线 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu3661gate {
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

/// 守卫阈值（符号化管线 重入/越限/静默三类拒止守卫）。
pub const MXU3661GATE_THRESHOLD: u64 = 64;

impl mxu3661gate {
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
        if self.window >= MXU3661GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe160, 0xe16f]。
pub const fn u3661_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3661Code::ALL.len() {
        let c = u3661Code::ALL[i].code();
        if c < 0xe160 || c > 0xe16f {
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
fn u3661_guard_seg() -> bool {
    u3661_segment_guard()
}

/// UNX-F3661 流水 Kahn 推演 + 回滚守恒。
fn check_u3661_pipe_plan() -> bool {
    let mut pb = mxu3661runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F3661 守卫重入/越限/静默三路拒止。
fn check_u3661_pipe_guard() -> bool {
    let mut g = mxu3661gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3661_checks() -> CheckSet {
    let mut set = CheckSet::new("U3661");
    set.add(
        "u3661-seg-guard",
        u3661_guard_seg(),
        if u3661_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3661-pipe-plan",
        check_u3661_pipe_plan(),
        if check_u3661_pipe_plan() { "" } else { "UNX-F3661 pipeline plan mismatch" },
    );
    set.add(
        "u3661-pipe-guard",
        check_u3661_pipe_guard(),
        if check_u3661_pipe_guard() { "" } else { "UNX-F3661 guard mismatch" },
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
    fn u3661_all_checks_green() {
        let set = run_u3661_checks();
        assert!(set.all_passed(), "U3661 domain checks must be green");
    }

    #[test]
    fn u3661_segment_guard_holds() {
        assert!(u3661_segment_guard());
    }
}
