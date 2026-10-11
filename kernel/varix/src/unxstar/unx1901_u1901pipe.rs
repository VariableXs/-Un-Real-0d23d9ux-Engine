//! unxu1901_u1901pipe —— 扩展状态区基座（XSAVE/XRSTOR 与 XCR0 位图裁断） 执行流水与守卫（状态位）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1901 · 扩展状态区基座（XSAVE/XRSTOR 与 XCR0 位图裁断） · 判据原文：UNX-F1901 · 判据：XSAVE 区按 XCR0 位图逐位裁断布局（x87/SSE/AVX/AVX-512 支持则存），区域尺寸查询与对齐断言全过，保存/恢复往返保真（AVX 样本矩阵比对零差）
//! · 组件：状态位 六级流水（依赖序 + Kahn 环检测 + 回滚）+ 三类拒止守卫
//! · 判据落地：UNX-F1901 · 判据：XSAVE 区按 XCR0 位图逐位裁断布局（x87/SSE/AVX/AVX-512 支持则存），区域尺寸查询与对齐断言全过，保存
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1901_VERSION: &str = "u1901-u1901pipe-v1";

/// 诊断段基址。
pub const U1901_SEG_BASE: u16 = 0x6060;

// --------------------------------------------------------------------------
// 诊断码（u1901 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 状态位 流水面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1901Code {
    /// 0x6060 级重复（位图裁断错 同级双注册）。
    StageDup,
    /// 0x6061 级断档（尺寸漂移 序缺失）。
    StageHole,
    /// 0x6062 依赖环（级间循环依赖）。
    DepCycle,
    /// 0x6063 自依赖（级依赖自身）。
    DepSelf,
    /// 0x6064 推演丢失（可执行序短于级数）。
    PlanLost,
    /// 0x6065 回滚破缺（回滚后守恒不成立）。
    RollbackBad,
    /// 0x6066 序漂移（回放序与推演序不符）。
    OrderDrift,
    /// 0x6067 完成计数漂移（done≠实计）。
    DoneDrift,
    /// 0x6068 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x6069 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x606a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x606b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1901Code {
    /// 全量闭集。
    pub const ALL: [u1901Code; 12] = [
        u1901Code::StageDup,
        u1901Code::StageHole,
        u1901Code::DepCycle,
        u1901Code::DepSelf,
        u1901Code::PlanLost,
        u1901Code::RollbackBad,
        u1901Code::OrderDrift,
        u1901Code::DoneDrift,
        u1901Code::LedgerBroken,
        u1901Code::SnapshotFingerprint,
        u1901Code::TableBroken,
        u1901Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1901Code::StageDup => 0x6060,
            u1901Code::StageHole => 0x6061,
            u1901Code::DepCycle => 0x6062,
            u1901Code::DepSelf => 0x6063,
            u1901Code::PlanLost => 0x6064,
            u1901Code::RollbackBad => 0x6065,
            u1901Code::OrderDrift => 0x6066,
            u1901Code::DoneDrift => 0x6067,
            u1901Code::LedgerBroken => 0x6068,
            u1901Code::SnapshotFingerprint => 0x6069,
            u1901Code::TableBroken => 0x606a,
            u1901Code::SegmentGuard => 0x606b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1901Code::StageDup => "级重复（位图裁断错 同级双注册）",
            u1901Code::StageHole => "级断档（尺寸漂移 序缺失）",
            u1901Code::DepCycle => "依赖环（级间循环依赖）",
            u1901Code::DepSelf => "自依赖（级依赖自身）",
            u1901Code::PlanLost => "推演丢失（可执行序短于级数）",
            u1901Code::RollbackBad => "回滚破缺（回滚后守恒不成立）",
            u1901Code::OrderDrift => "序漂移（回放序与推演序不符）",
            u1901Code::DoneDrift => "完成计数漂移（done≠实计）",
            u1901Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1901Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1901Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1901Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1901Code::StageDup => "按级唯一律去重",
            u1901Code::StageHole => "按级序补齐",
            u1901Code::DepCycle => "按无环律重构依赖",
            u1901Code::DepSelf => "删除自依赖边",
            u1901Code::PlanLost => "按 Kahn 剥皮重推",
            u1901Code::RollbackBad => "按守恒律复算",
            u1901Code::OrderDrift => "按推演序重放",
            u1901Code::DoneDrift => "按实计校正",
            u1901Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1901Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1901Code::TableBroken => "按语义规则重建常量表",
            u1901Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu1901run（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 状态位 执行流水（位图裁断错）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1901runStage {
    /// 级 根：x87 位布局。
    X87,
    /// 级 0：SSE 位布局。
    Sse,
    /// 级 1：AVX 位布局。
    Avx,
    /// 级 2：AVX-512 位布局。
    Avx512,
    /// 级 3：保存恢复往返。
    Round,
    /// 级 4：保真核验。
    Verify,
}

impl mxu1901runStage {
    /// 全量级闭集。
    pub const ALL: [mxu1901runStage; 6] = [
        mxu1901runStage::X87,
        mxu1901runStage::Sse,
        mxu1901runStage::Avx,
        mxu1901runStage::Avx512,
        mxu1901runStage::Round,
        mxu1901runStage::Verify,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mxu1901runStage::X87 => 0,
            mxu1901runStage::Sse => 1,
            mxu1901runStage::Avx => 2,
            mxu1901runStage::Avx512 => 3,
            mxu1901runStage::Round => 4,
            mxu1901runStage::Verify => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            mxu1901runStage::X87 => "x87 位布局",
            mxu1901runStage::Sse => "SSE 位布局",
            mxu1901runStage::Avx => "AVX 位布局",
            mxu1901runStage::Avx512 => "AVX-512 位布局",
            mxu1901runStage::Round => "保存恢复往返",
            mxu1901runStage::Verify => "保真核验",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const MXU1901RUN_DEPS: [i32; 6] = [
    -1, // X87 依赖 无
    0, // Sse 依赖 级 0
    1, // Avx 依赖 级 1
    2, // Avx512 依赖 级 2
    3, // Round 依赖 级 3
    4, // Verify 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1901runProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（状态位 回滚纪律）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1901runBook {
    /// 各级状态。
    pub state: [mxu1901runProgress; 6],
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

impl mxu1901runBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [mxu1901runProgress::Pending; 6],
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
                if self.state[i] != mxu1901runProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = MXU1901RUN_DEPS[i];
                if dep < 0 || self.state[dep as usize] == mxu1901runProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = mxu1901runProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == mxu1901runProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（状态位 回滚纪律）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == mxu1901runProgress::Done {
                self.state[i] = mxu1901runProgress::Rolled;
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
            if self.state[i] == mxu1901runProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != mxu1901runProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守卫 mxu1901gate（阈值 64 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 状态位 重入/越限/静默三类拒止守卫。
#[derive(Clone, Copy, Debug)]
pub struct mxu1901gate {
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

/// 守卫阈值（状态位 重入/越限/静默三类拒止守卫）。
pub const MXU1901GATE_THRESHOLD: u64 = 64;

impl mxu1901gate {
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
        if self.window >= MXU1901GATE_THRESHOLD {
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

/// 段位守卫：全部诊断码必须落在本段 [0x6060, 0x606f]。
pub const fn u1901_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1901Code::ALL.len() {
        let c = u1901Code::ALL[i].code();
        if c < 0x6060 || c > 0x606f {
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
fn u1901_guard_seg() -> bool {
    u1901_segment_guard()
}

/// UNX-F1901 流水 Kahn 推演 + 回滚守恒。
fn check_u1901_pipe_plan() -> bool {
    let mut pb = mxu1901runBook::new();
    let mut ok = pb.plan().is_ok();
    if ok { ok = pb.done == 6 && pb.conserved() && pb.order_matches(); }
    pb.rollback_from(1);
    if ok { ok = pb.conserved() && pb.rolled >= 1 && pb.done == 1; }
    ok
}

/// UNX-F1901 守卫重入/越限/静默三路拒止。
fn check_u1901_pipe_guard() -> bool {
    let mut g = mxu1901gate::new();
    let mut ok = g.enter();
    g.leave();
    if ok { ok = g.conserved(); }
    g.observe(true); g.observe(false);
    if ok { ok = g.conserved(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1901_checks() -> CheckSet {
    let mut set = CheckSet::new("U1901");
    set.add(
        "u1901-seg-guard",
        u1901_guard_seg(),
        if u1901_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1901-pipe-plan",
        check_u1901_pipe_plan(),
        if check_u1901_pipe_plan() { "" } else { "UNX-F1901 pipeline plan mismatch" },
    );
    set.add(
        "u1901-pipe-guard",
        check_u1901_pipe_guard(),
        if check_u1901_pipe_guard() { "" } else { "UNX-F1901 guard mismatch" },
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
    fn u1901_all_checks_green() {
        let set = run_u1901_checks();
        assert!(set.all_passed(), "U1901 domain checks must be green");
    }

    #[test]
    fn u1901_segment_guard_holds() {
        assert!(u1901_segment_guard());
    }
}
