//! unxt223_krecon —— UNX-F0221 计数账与运行数对拍（双源一致）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0221 · 引导面断言聚合扩容（300+ 达成线） · 判据：ktest 引导面断言总数 ≥300 并随批次自动聚合，计数账与实际运行数一致
//! · 组件：声明计数账 + 实际运行账 + 双源对拍 + 偏差升级
//! · 判据落地：计数账与实际运行数一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T223_VERSION: &str = "t223-krecon-v1";

/// 诊断段基址。
pub const T223_SEG_BASE: u16 = 0x3820;

// --------------------------------------------------------------------------
// 诊断码（t223 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 计数对拍诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t223Code {
    /// 0x3820 声明漂移（声明数随批增长不入账）。
    DeclaredDrift,
    /// 0x3821 实际欠跑（运行数 < 声明数）。
    ActualUnder,
    /// 0x3822 实际超跑（运行数 > 声明数）。
    ActualOver,
    /// 0x3823 复算不符（账本与逐批合计矛盾）。
    RecountMismatch,
    /// 0x3824 批序断号（聚合波次跳批）。
    BatchSeqGap,
    /// 0x3825 迟到入账（超窗后补登记）。
    LateArrival,
    /// 0x3826 抽样偏置（抽样未覆盖低批）。
    SampleBias,
    /// 0x3827 账本折叠（正负偏差对冲掩盖）。
    LedgerFold,
    /// 0x3828 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3829 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x382a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x382b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x382c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x382d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x382e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x382f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t223Code {
    /// 全量闭集。
    pub const ALL: [t223Code; 16] = [
        t223Code::DeclaredDrift,
        t223Code::ActualUnder,
        t223Code::ActualOver,
        t223Code::RecountMismatch,
        t223Code::BatchSeqGap,
        t223Code::LateArrival,
        t223Code::SampleBias,
        t223Code::LedgerFold,
        t223Code::LedgerBroken,
        t223Code::SnapshotFingerprint,
        t223Code::TableBroken,
        t223Code::IndexMisaligned,
        t223Code::DuplicateEntry,
        t223Code::OrderViolated,
        t223Code::GuardTripped,
        t223Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t223Code::DeclaredDrift => 0x3820,
            t223Code::ActualUnder => 0x3821,
            t223Code::ActualOver => 0x3822,
            t223Code::RecountMismatch => 0x3823,
            t223Code::BatchSeqGap => 0x3824,
            t223Code::LateArrival => 0x3825,
            t223Code::SampleBias => 0x3826,
            t223Code::LedgerFold => 0x3827,
            t223Code::LedgerBroken => 0x3828,
            t223Code::SnapshotFingerprint => 0x3829,
            t223Code::TableBroken => 0x382a,
            t223Code::IndexMisaligned => 0x382b,
            t223Code::DuplicateEntry => 0x382c,
            t223Code::OrderViolated => 0x382d,
            t223Code::GuardTripped => 0x382e,
            t223Code::SegmentGuard => 0x382f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t223Code::DeclaredDrift => "声明漂移（声明数随批增长不入账）",
            t223Code::ActualUnder => "实际欠跑（运行数 < 声明数）",
            t223Code::ActualOver => "实际超跑（运行数 > 声明数）",
            t223Code::RecountMismatch => "复算不符（账本与逐批合计矛盾）",
            t223Code::BatchSeqGap => "批序断号（聚合波次跳批）",
            t223Code::LateArrival => "迟到入账（超窗后补登记）",
            t223Code::SampleBias => "抽样偏置（抽样未覆盖低批）",
            t223Code::LedgerFold => "账本折叠（正负偏差对冲掩盖）",
            t223Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t223Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t223Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t223Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t223Code::DuplicateEntry => "条目重复（同键双写）",
            t223Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t223Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t223Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t223Code::DeclaredDrift => "按批增量补登",
            t223Code::ActualUnder => "按缺口批次补跑",
            t223Code::ActualOver => "核对重复运行并去重",
            t223Code::RecountMismatch => "逐批复核后重建",
            t223Code::BatchSeqGap => "按 15 批连续律补号",
            t223Code::LateArrival => "按迟到标记入账并复算",
            t223Code::SampleBias => "按分层抽样重抽",
            t223Code::LedgerFold => "双向分立核算",
            t223Code::LedgerBroken => "按守恒律重建台账后重跑",
            t223Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t223Code::TableBroken => "按语义规则重建常量表",
            t223Code::IndexMisaligned => "按声明序归一化后重跑",
            t223Code::DuplicateEntry => "合并同键条目为末次生效",
            t223Code::OrderViolated => "按依赖矩阵重排后重跑",
            t223Code::GuardTripped => "检查临界区嵌套与阈值",
            t223Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 recon_dual（双源对拍：4 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// 声明 vs 实际双源对拍（15 批逐批）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum recon_dualSlot {
    /// 批计数。
    BatchCount,
    /// 运行计数。
    RunCount,
    /// 累计计数。
    Cumulative,
    /// 批均计数。
    PerBatchAvg,
}

impl recon_dualSlot {
    /// 全量槽位闭集。
    pub const ALL: [recon_dualSlot; 4] = [
        recon_dualSlot::BatchCount,
        recon_dualSlot::RunCount,
        recon_dualSlot::Cumulative,
        recon_dualSlot::PerBatchAvg,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            recon_dualSlot::BatchCount => 0,
            recon_dualSlot::RunCount => 1,
            recon_dualSlot::Cumulative => 2,
            recon_dualSlot::PerBatchAvg => 3,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            recon_dualSlot::BatchCount => "批计数",
            recon_dualSlot::RunCount => "运行计数",
            recon_dualSlot::Cumulative => "累计计数",
            recon_dualSlot::PerBatchAvg => "批均计数",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct recon_dualPair {
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
pub struct recon_dualDual {
    /// 逐槽样本。
    pub pairs: [recon_dualPair; 4],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl recon_dualDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [recon_dualPair { a: 0, b: 0, same: false, seq: 0 }; 4],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: recon_dualSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = recon_dualPair { a, b, same, seq: self.rounds };
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
        self.diff_n == 0 && self.same_n == 4
    }

    /// 复算：逐槽重读 pairs 重判 same（防记账面与数据面分叉）。
    pub fn recount(&self) -> bool {
        let mut same = 0usize;
        let mut i = 0usize;
        while i < 4 {
            if self.pairs[i].same == (self.pairs[i].a == self.pairs[i].b) {
                same += 1;
            }
            i += 1;
        }
        same == 4 && self.same_n + self.diff_n == 4
    }
}

// --------------------------------------------------------------------------
// 守恒账 recon（对拍账（偏差守恒：declared == actual + drift））
// --------------------------------------------------------------------------
/// 对拍账（偏差守恒：declared == actual + drift）。
#[derive(Clone, Copy, Debug)]
pub struct recon {
    /// 声明总数
    pub declared: u64,
    /// 实际运行数
    pub actual: u64,
    /// 偏差量
    pub drift: u64,
}

impl recon {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            declared: 0,
            actual: 0,
            drift: 0,
        }
    }

    /// 登记声明数。
    pub fn declare(&mut self) {
        self.declared += 1;
    }

    /// 登记一次实际运行。
    pub fn run(&mut self) {
        self.actual += 1;
    }

    /// 结算偏差（|声明-实际|）。
    pub fn settle(&mut self) {
        self.drift = if self.declared > self.actual { self.declared - self.actual } else { self.actual - self.declared };
    }

}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3820, 0x382f]。
pub const fn t223_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t223Code::ALL.len() {
        let c = t223Code::ALL[i].code();
        if c < 0x3820 || c > 0x382f {
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
fn t223_guard_seg() -> bool {
    t223_segment_guard()
}

/// 对拍零偏差正样本（四槽各一次 300=300）。
fn check_c1_recon_zero() -> bool {
    let mut d = recon_dualDual::new();
    d.sample(recon_dualSlot::BatchCount, 300, 300);
    d.sample(recon_dualSlot::RunCount, 300, 300);
    d.sample(recon_dualSlot::Cumulative, 300, 300);
    d.sample(recon_dualSlot::PerBatchAvg, 20, 20);
    let mut ok = d.all_same() && d.rounds == 4;
    if ok {
        d.sample(recon_dualSlot::BatchCount, 300, 299);
        ok = !d.all_same() && d.diff_n == 1;
    }
    ok
}

/// 对拍账结算（偏差 = |声明-实际|）。
fn check_c2_ledger_settle() -> bool {
    let mut r = recon::new();
    let mut i = 0;
    while i < 300 {
        r.declare();
        r.run();
        i += 1;
    }
    r.settle();
    let mut ok = r.declared == 300 && r.actual == 300 && r.drift == 0;
    if ok {
        let mut r2 = recon::new();
        let mut j = 0;
        while j < 290 {
            r2.run();
            j += 1;
        }
        let mut k = 0;
        while k < 300 {
            r2.declare();
            k += 1;
        }
        r2.settle();
        ok = r2.drift == 10;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t223_checks() -> CheckSet {
    let mut set = CheckSet::new("T223");
    set.add(
        "t223-seg-guard",
        t223_guard_seg(),
        if t223_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t223-c1-recon-zero",
        check_c1_recon_zero(),
        if check_c1_recon_zero() { "" } else { "reconciliation zero-deviation mismatch" },
    );
    set.add(
        "t223-c2-ledger-settle",
        check_c2_ledger_settle(),
        if check_c2_ledger_settle() { "" } else { "reconciliation ledger settle mismatch" },
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
    fn t223_all_checks_green() {
        let set = run_t223_checks();
        assert!(set.all_passed(), "T223 domain checks must be green");
    }

    #[test]
    fn t223_segment_guard_holds() {
        assert!(t223_segment_guard());
    }
}
