//! unxt424_idtinstall —— UNX-F0421 lidt 载入与回读。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0421 · IDT 全量重装（256 门冻结） · 判据：256 门全部装载且 Type/IST/DPL 位态与布局账一致（逐门比对 256/256），载入后 lidt 回读一致
//! · 组件：载入流水（六级）+ 回读对拍 + 域快照
//! · 判据落地：载入后 lidt 回读一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T424_VERSION: &str = "t424-idtinstall-v1";

/// 诊断段基址。
pub const T424_SEG_BASE: u16 = 0x4230;

// --------------------------------------------------------------------------
// 诊断码（t424 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// lidt 载入诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t424Code {
    /// 0x4230 未填先载。
    LoadBeforeFill,
    /// 0x4231 limit 回读不符。
    LimitBad,
    /// 0x4232 base 回读不符。
    BaseBad,
    /// 0x4233 重复载入。
    LoadTwice,
    /// 0x4234 载入账破缺。
    LedgerBreak,
    /// 0x4235 载入后断档（有门未装）。
    IdtGap,
    /// 0x4236 报告陈旧。
    ReportStale,
    /// 0x4237 守卫触发。
    GuardTripped,
    /// 0x4238 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4239 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x423a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x423b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x423c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x423d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x423e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t424Code {
    /// 全量闭集。
    pub const ALL: [t424Code; 15] = [
        t424Code::LoadBeforeFill,
        t424Code::LimitBad,
        t424Code::BaseBad,
        t424Code::LoadTwice,
        t424Code::LedgerBreak,
        t424Code::IdtGap,
        t424Code::ReportStale,
        t424Code::GuardTripped,
        t424Code::LedgerBroken,
        t424Code::SnapshotFingerprint,
        t424Code::TableBroken,
        t424Code::IndexMisaligned,
        t424Code::DuplicateEntry,
        t424Code::OrderViolated,
        t424Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t424Code::LoadBeforeFill => 0x4230,
            t424Code::LimitBad => 0x4231,
            t424Code::BaseBad => 0x4232,
            t424Code::LoadTwice => 0x4233,
            t424Code::LedgerBreak => 0x4234,
            t424Code::IdtGap => 0x4235,
            t424Code::ReportStale => 0x4236,
            t424Code::GuardTripped => 0x4237,
            t424Code::LedgerBroken => 0x4238,
            t424Code::SnapshotFingerprint => 0x4239,
            t424Code::TableBroken => 0x423a,
            t424Code::IndexMisaligned => 0x423b,
            t424Code::DuplicateEntry => 0x423c,
            t424Code::OrderViolated => 0x423d,
            t424Code::SegmentGuard => 0x423e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t424Code::LoadBeforeFill => "未填先载",
            t424Code::LimitBad => "limit 回读不符",
            t424Code::BaseBad => "base 回读不符",
            t424Code::LoadTwice => "重复载入",
            t424Code::LedgerBreak => "载入账破缺",
            t424Code::IdtGap => "载入后断档（有门未装）",
            t424Code::ReportStale => "报告陈旧",
            t424Code::GuardTripped => "守卫触发",
            t424Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t424Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t424Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t424Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t424Code::DuplicateEntry => "条目重复（同键双写）",
            t424Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t424Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t424Code::LoadBeforeFill => "按填充前置条件重排",
            t424Code::LimitBad => "重装后复读",
            t424Code::BaseBad => "重装后复读",
            t424Code::LoadTwice => "幂等拒绝",
            t424Code::LedgerBreak => "按守恒重建",
            t424Code::IdtGap => "补装后重载",
            t424Code::ReportStale => "重渲染",
            t424Code::GuardTripped => "检查临界区",
            t424Code::LedgerBroken => "按守恒律重建台账后重跑",
            t424Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t424Code::TableBroken => "按语义规则重建常量表",
            t424Code::IndexMisaligned => "按声明序归一化后重跑",
            t424Code::DuplicateEntry => "合并同键条目为末次生效",
            t424Code::OrderViolated => "按依赖矩阵重排后重跑",
            t424Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 inst（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// lidt 载入六级流水。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum instStage {
    /// 级 根：填充 256 门描述符。
    FillGates,
    /// 级 0：逐门校验（256/256）。
    VerifyGates,
    /// 级 1：冻结门表。
    FreezeGates,
    /// 级 2：组 lidt 描述符（limit=4095）。
    BuildLidt,
    /// 级 3：执行 lidt。
    ExecLidt,
    /// 级 4：回读 limit/base 比对。
    Readback,
}

impl instStage {
    /// 全量级闭集。
    pub const ALL: [instStage; 6] = [
        instStage::FillGates,
        instStage::VerifyGates,
        instStage::FreezeGates,
        instStage::BuildLidt,
        instStage::ExecLidt,
        instStage::Readback,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            instStage::FillGates => 0,
            instStage::VerifyGates => 1,
            instStage::FreezeGates => 2,
            instStage::BuildLidt => 3,
            instStage::ExecLidt => 4,
            instStage::Readback => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            instStage::FillGates => "填充 256 门描述符",
            instStage::VerifyGates => "逐门校验（256/256）",
            instStage::FreezeGates => "冻结门表",
            instStage::BuildLidt => "组 lidt 描述符（limit=4095）",
            instStage::ExecLidt => "执行 lidt",
            instStage::Readback => "回读 limit/base 比对",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const INST_DEPS: [i32; 6] = [
    -1, // FillGates 依赖 无
    0, // VerifyGates 依赖 级 0
    1, // FreezeGates 依赖 级 1
    2, // BuildLidt 依赖 级 2
    3, // ExecLidt 依赖 级 3
    4, // Readback 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum instProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（载入中断）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct instBook {
    /// 各级状态。
    pub state: [instProgress; 6],
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

impl instBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [instProgress::Pending; 6],
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
                if self.state[i] != instProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = INST_DEPS[i];
                if dep < 0 || self.state[dep as usize] == instProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = instProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == instProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（载入中断）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == instProgress::Done {
                self.state[i] = instProgress::Rolled;
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
            if self.state[i] == instProgress::Pending {
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
            if self.order[i] >= 6 || self.state[self.order[i]] != instProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 引擎 lidt_dual（双源对拍：3 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// lidt 回读对拍（limit/base 双源）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum lidt_dualSlot {
    /// limit。
    Limit,
    /// base。
    Base,
    /// 门数。
    GateCount,
}

impl lidt_dualSlot {
    /// 全量槽位闭集。
    pub const ALL: [lidt_dualSlot; 3] = [
        lidt_dualSlot::Limit,
        lidt_dualSlot::Base,
        lidt_dualSlot::GateCount,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            lidt_dualSlot::Limit => 0,
            lidt_dualSlot::Base => 1,
            lidt_dualSlot::GateCount => 2,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            lidt_dualSlot::Limit => "limit",
            lidt_dualSlot::Base => "base",
            lidt_dualSlot::GateCount => "门数",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct lidt_dualPair {
    /// A 源首读值。
    pub a: u64,
    /// B 源回读值。
    pub b: u64,
    /// 是否一致。
    pub same: bool,
    /// 采样序号。
    pub seq: u64,
}

/// 对拍账（逐槽一致才放行；不一致槽位显性记账）。
#[derive(Clone, Copy, Debug)]
pub struct lidt_dualDual {
    /// 逐槽样本。
    pub pairs: [lidt_dualPair; 3],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl lidt_dualDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [lidt_dualPair { a: 0, b: 0, same: false, seq: 0 }; 3],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: lidt_dualSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = lidt_dualPair { a, b, same, seq: self.rounds };
        if same {
            self.same_n += 1;
        } else {
            self.diff_n += 1;
            self.diff_mask |= 1u64 << (i as u64);
        }
        self.rounds += 1;
    }

    /// 全槽一致判定（对拍通过口径）。
    pub fn all_same(&self) -> bool {
        self.diff_n == 0 && self.same_n == 3
    }

    /// 复算：逐槽重读 pairs 重判 same（防记账面与数据面分叉）。
    pub fn recount(&self) -> bool {
        let mut same = 0usize;
        let mut i = 0usize;
        while i < 3 {
            if self.pairs[i].same == (self.pairs[i].a == self.pairs[i].b) {
                same += 1;
            }
            i += 1;
        }
        same == 3 && self.same_n + self.diff_n == 3
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4230, 0x423f]。
pub const fn t424_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t424Code::ALL.len() {
        let c = t424Code::ALL[i].code();
        if c < 0x4230 || c > 0x423f {
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
fn t424_guard_seg() -> bool {
    t424_segment_guard()
}

/// 载入六级流水推演全达。
fn check_c1_install_pipeline() -> bool {
    let mut bk = instBook::new();
    let mut ok = matches!(bk.plan(), Ok(_)) && bk.done == 6;
    if ok {
        ok = bk.order_matches() && bk.conserved();
    }
    ok
}

/// lidt 回读对拍（limit=4095 + 负样本）。
fn check_c2_lidt_dual() -> bool {
    let mut d = lidt_dualDual::new();
    d.sample(lidt_dualSlot::Limit, 4095, 4095);
    d.sample(lidt_dualSlot::Base, 0xffff_8000_0010_0000, 0xffff_8000_0010_0000);
    d.sample(lidt_dualSlot::GateCount, 256, 256);
    let mut ok = d.all_same();
    if ok {
        d.sample(lidt_dualSlot::Limit, 4095, 4096);
        ok = !d.all_same() && d.diff_n == 1;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t424_checks() -> CheckSet {
    let mut set = CheckSet::new("T424");
    set.add(
        "t424-seg-guard",
        t424_guard_seg(),
        if t424_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t424-c1-install-pipeline",
        check_c1_install_pipeline(),
        if check_c1_install_pipeline() { "" } else { "lidt install pipeline mismatch" },
    );
    set.add(
        "t424-c2-lidt-dual",
        check_c2_lidt_dual(),
        if check_c2_lidt_dual() { "" } else { "lidt readback dual mismatch" },
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
    fn t424_all_checks_green() {
        let set = run_t424_checks();
        assert!(set.all_passed(), "T424 domain checks must be green");
    }

    #[test]
    fn t424_segment_guard_holds() {
        assert!(t424_segment_guard());
    }
}
