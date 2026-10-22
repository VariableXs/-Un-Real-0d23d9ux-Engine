//! CGPU-F0208 · B03 组收口（判据段 0x06A0）
//! B03 组 16 项收口（组级范式）：自检入树、性能基准（F0037/F0116
//! 联动）、确定性验证汇总（F0172 域级）、组双签、台账、向 B04 优先
//! 级组移交（切分就绪——切分后的并行执行需要优先级与抢占的精细化，
//! B03 提供了切分基础设施）。判据：16 项入树、报告、双签、移交、
//! 断点。
//!
//! 诊断码段 0x06A0（本模块独占：0x06A0~0x06AF）。
//! no_std + alloc 零 unwrap，全部 Result 化。
//!
//! B03 组 16 项清单（F0193~F0208）：
//!   1. 帧图并行切分策略   2. 负载均衡与工作窃取
//!   3. CPU/GPU 混合并行   4. 并行合成撕裂防护
//!   5. NUMA 感知调度      6. 异步编译重叠
//!   7. 多 GPU 帧图拆分    8. 实时监控面板
//!   9. A/B 对照          10. 模板扩展
//!  11. 优化建议器        12. 历史与趋势
//!  13. API 冻结          14. 图 fuzz
//!  15. 帧图文档          16. 组收口（本模块）

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x06A0;

/// B03 组项数（判据「16 项入树」的清单长度）。
pub const B03_ITEMS: usize = 16;

/// 组项收口状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemState {
    /// 未入树。
    Absent,
    /// 已入树（模块注册 + 判据可执行）。
    Wired,
    /// 已验证（判据全绿 + 报告归档）。
    Verified,
}

impl ItemState {
    /// 状态序号。
    pub fn ordinal(&self) -> u8 {
        match self {
            ItemState::Absent => 0,
            ItemState::Wired => 1,
            ItemState::Verified => 2,
        }
    }
    /// 可移交性：仅 Verified 可移交。
    pub fn handover_ready(&self) -> bool {
        *self == ItemState::Verified
    }
}

/// 组项收口记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GroupItem {
    /// 项号（1~16）。
    pub item: u8,
    /// 状态。
    pub state: ItemState,
    /// 判据段位。
    pub seg: u16,
    /// 判据通过数。
    pub checks_passed: u32,
    /// 判据总数。
    pub checks_total: u32,
}

impl GroupItem {
    /// 合法性：项号在册、段位非零、判据账自洽。
    pub fn validate(&self) -> Result<(), u16> {
        if self.item == 0 || self.item as usize > B03_ITEMS || self.seg == 0 {
            return Err(SEG_BASE);
        }
        if self.checks_passed > self.checks_total {
            return Err(SEG_BASE + 1);
        }
        if self.state == ItemState::Verified && self.checks_passed != self.checks_total {
            return Err(SEG_BASE + 2);
        }
        if self.state == ItemState::Absent && self.checks_total != 0 {
            return Err(SEG_BASE + 3);
        }
        Ok(())
    }
}

/// 组收口台账（判据「16 项入树」容器）。
#[derive(Default)]
pub struct B03Ledger {
    items: Vec<GroupItem>,
}

impl B03Ledger {
    /// 登记一项（重复项号拒绝）。
    pub fn record(&mut self, it: GroupItem) -> Result<(), u16> {
        it.validate()?;
        if self.items.iter().any(|x| x.item == it.item) {
            return Err(SEG_BASE + 4);
        }
        self.items.push(it);
        Ok(())
    }
    /// 台账规模。
    pub fn count(&self) -> usize {
        self.items.len()
    }
    /// 全 16 项入树校验（判据「16 项入树」主判据）：项号 1..=16
    /// 全在册且全部 Verified。
    pub fn all_wired_and_verified(&self) -> Result<(), u16> {
        if self.items.len() != B03_ITEMS {
            return Err(SEG_BASE + 5);
        }
        for n in 1..=B03_ITEMS as u8 {
            let it = self.items.iter().find(|x| x.item == n).ok_or(SEG_BASE + 6)?;
            if !it.state.handover_ready() {
                return Err(SEG_BASE + 7);
            }
        }
        Ok(())
    }
    /// 组判据总账（判据「报告」数据面）：全组通过数/总数。
    pub fn check_totals(&self) -> (u32, u32) {
        let mut p = 0u32;
        let mut t = 0u32;
        for it in &self.items {
            p += it.checks_passed;
            t += it.checks_total;
        }
        (p, t)
    }
    /// 台账指纹。
    pub fn ledger_fp(&self) -> u64 {
        let mut acc: u64 = 0xcbf29ce484222325;
        for it in &self.items {
            acc = acc.wrapping_mul(0x100000001b3) ^ it.item as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ it.state.ordinal() as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ it.checks_passed as u64;
        }
        acc
    }
}

/// 性能基准汇总（判据「性能基准」联动面）：B03 组关键性能量的
/// 组级快照——与 F0037 微基准/F0116 基准册联动对账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfSummary {
    /// 切分三策略最快峰值并行度。
    pub peak_parallelism: u32,
    /// 窃取后不均衡度（千分比，越低越好）。
    pub steal_imbalance_permille: u32,
    /// 混合执行收益（千分比）。
    pub hybrid_gain_permille: u32,
    /// 监控采集开销占比（千分比，≤500 达标）。
    pub monitor_overhead_permille: u32,
}

impl PerfSummary {
    /// 合法性 + 达标线。
    pub fn validate(&self) -> Result<(), u16> {
        if self.peak_parallelism == 0 {
            return Err(SEG_BASE + 8);
        }
        if self.steal_imbalance_permille > 100 {
            return Err(SEG_BASE + 9);
        }
        if self.monitor_overhead_permille > 500 {
            return Err(SEG_BASE + 10);
        }
        Ok(())
    }
}

/// 确定性汇总（判据「确定性验证汇总」面）：B03 组全部含确定性
/// 语义的模块（切分/fuzz/回放）的确定性观测合账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeterminismSummary {
    /// 观测模块数。
    pub modules: u32,
    /// 确定性通过模块数。
    pub deterministic_ok: u32,
    /// fuzz 案例总数。
    pub fuzz_cases: u32,
    /// fuzz 通过案例数。
    pub fuzz_passed: u32,
}

impl DeterminismSummary {
    /// 合法性：全通过才允许组收口。
    pub fn validate(&self) -> Result<(), u16> {
        if self.modules == 0 || self.deterministic_ok != self.modules {
            return Err(SEG_BASE + 11);
        }
        if self.fuzz_cases == 0 || self.fuzz_passed != self.fuzz_cases {
            return Err(SEG_BASE + 12);
        }
        Ok(())
    }
}

/// 双签（判据「组双签」载体）：两名签署人的指纹 + 签署时点组台账
/// 指纹——两签必须对同一台账指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DualSign {
    /// 签署人 A 指纹。
    pub signer_a: u64,
    /// 签署人 B 指纹。
    pub signer_b: u64,
    /// 台账指纹。
    pub ledger_fp: u64,
}

impl DualSign {
    /// 合法性：双人互异、指纹非零。
    pub fn validate(&self) -> Result<(), u16> {
        if self.signer_a == 0 || self.signer_b == 0 {
            return Err(SEG_BASE + 13);
        }
        if self.signer_a == self.signer_b {
            return Err(SEG_BASE + 14);
        }
        if self.ledger_fp == 0 {
            return Err(SEG_BASE + 15);
        }
        Ok(())
    }
}

/// 组收口报告（判据「报告」一等公民）：全部收口面的汇总投影片。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B03Report {
    /// 台账指纹。
    pub ledger_fp: u64,
    /// 性能汇总指纹。
    pub perf_fp: u64,
    /// 确定性汇总指纹。
    pub det_fp: u64,
    /// 双签指纹。
    pub sign_fp: u64,
}

impl B03Report {
    /// 报告合法性：四指纹全非零。
    pub fn validate(&self) -> Result<(), u16> {
        if self.ledger_fp == 0 || self.perf_fp == 0 || self.det_fp == 0 || self.sign_fp == 0 {
            return Err(SEG_BASE + 16);
        }
        Ok(())
    }
}

/// 向 B04 移交包（判据「移交」载体）：B03 提供给 B04（优先级与抢
/// 占组）的基础设施清单——切分方案、窃取统计、监控面板、模板库。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoverPack {
    /// 切分基础设施段位。
    pub split_seg: u16,
    /// 窃取统计段位。
    pub steal_seg: u16,
    /// 监控面板段位。
    pub panel_seg: u16,
    /// 模板库段位。
    pub tpl_seg: u16,
    /// 移交前置：B03 报告指纹。
    pub precond_fp: u64,
}

impl HandoverPack {
    /// 合法性：四段位有效且前提报告在册。
    pub fn validate(&self) -> Result<(), u16> {
        for s in [self.split_seg, self.steal_seg, self.panel_seg, self.tpl_seg] {
            if s == 0 {
                return Err(SEG_BASE + 17);
            }
        }
        if self.precond_fp == 0 {
            return Err(SEG_BASE + 18);
        }
        Ok(())
    }
}

/// 组收口断点（判据「断点」载体）：收口完成时落的断点记录——
/// 组号 + 断点序号 + 报告指纹，供跨组审计链。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GroupBreakpoint {
    /// 组号（B03 = 3）。
    pub group: u8,
    /// 断点序号（组内递增）。
    pub seq: u32,
    /// 报告指纹。
    pub report_fp: u64,
}

impl GroupBreakpoint {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.group != 3 || self.seq == 0 || self.report_fp == 0 {
            return Err(SEG_BASE + 19);
        }
        Ok(())
    }
}

/// 收口主流程（判据「16 项入树」到「断点」的编排核心）：台账全绿
/// → 性能/确定性达标 → 双签对账 → 出报告 → 断点。任一环失败即
/// 组收口拒绝。
pub fn b03_closeout(
    ledger: &B03Ledger,
    perf: &PerfSummary,
    det: &DeterminismSummary,
    sign: &DualSign,
) -> Result<B03Report, u16> {
    ledger.all_wired_and_verified()?;
    perf.validate()?;
    det.validate()?;
    if sign.ledger_fp != ledger.ledger_fp() {
        return Err(SEG_BASE + 20);
    }
    let (p, t) = ledger.check_totals();
    if t == 0 || p != t {
        return Err(SEG_BASE + 21);
    }
    let rep = B03Report {
        ledger_fp: ledger.ledger_fp(),
        perf_fp: {
            let mut a: u64 = 0xcbf29ce484222325;
            a = a.wrapping_mul(0x100000001b3) ^ perf.peak_parallelism as u64;
            a = a.wrapping_mul(0x100000001b3) ^ perf.steal_imbalance_permille as u64;
            a
        },
        det_fp: {
            let mut a: u64 = 0xcbf29ce484222325;
            a = a.wrapping_mul(0x100000001b3) ^ det.modules as u64;
            a = a.wrapping_mul(0x100000001b3) ^ det.fuzz_cases as u64;
            a
        },
        sign_fp: {
            let mut a: u64 = 0xcbf29ce484222325;
            a = a.wrapping_mul(0x100000001b3) ^ sign.signer_a;
            a = a.wrapping_mul(0x100000001b3) ^ sign.signer_b;
            a
        },
    };
    rep.validate()?;
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(n: u8, seg: u16) -> GroupItem {
        GroupItem { item: n, state: ItemState::Verified, seg, checks_passed: 8, checks_total: 8 }
    }
    fn full_ledger() -> B03Ledger {
        let mut l = B03Ledger::default();
        for n in 1..=16u8 {
            l.record(item(n, 0x05B0 + n as u16 * 0x10)).unwrap();
        }
        l
    }
    #[test]
    fn ledger_full_walk() {
        let l = full_ledger();
        assert_eq!(l.count(), 16);
        assert!(l.all_wired_and_verified().is_ok());
        let (p, t) = l.check_totals();
        assert_eq!((p, t), (128, 128));
    }
    #[test]
    fn ledger_gaps_rejected() {
        let mut l = full_ledger();
        // 少一项（16 项只录 15）模拟：新建漏第 7 项。
        let mut l2 = B03Ledger::default();
        for n in 1..=16u8 {
            if n != 7 {
                l2.record(item(n, 0x0500 + n as u16)).unwrap();
            }
        }
        assert!(l2.all_wired_and_verified().is_err());
        let _ = l;
        // 未验证项混入 → 拒绝。
        let mut l3 = full_ledger();
        l3.items[0].state = ItemState::Wired;
        assert!(l3.all_wired_and_verified().is_err());
    }
    #[test]
    fn item_state_rules() {
        let it = GroupItem { item: 1, state: ItemState::Verified, seg: 1, checks_passed: 5, checks_total: 4 };
        assert!(it.validate().is_err()); // 通过数 > 总数。
        let it2 = GroupItem { item: 1, state: ItemState::Verified, seg: 1, checks_passed: 4, checks_total: 5 };
        assert!(it2.validate().is_err()); // Verified 但没全过。
    }
    #[test]
    fn perf_and_det_gates() {
        let perf = PerfSummary { peak_parallelism: 8, steal_imbalance_permille: 90, hybrid_gain_permille: 300, monitor_overhead_permille: 400 };
        assert!(perf.validate().is_ok());
        let bad = PerfSummary { peak_parallelism: 1, steal_imbalance_permille: 200, hybrid_gain_permille: 0, monitor_overhead_permille: 400 };
        assert!(bad.validate().is_err());
        let det = DeterminismSummary { modules: 3, deterministic_ok: 3, fuzz_cases: 64, fuzz_passed: 64 };
        assert!(det.validate().is_ok());
        let det_bad = DeterminismSummary { modules: 3, deterministic_ok: 2, fuzz_cases: 64, fuzz_passed: 64 };
        assert!(det_bad.validate().is_err());
    }
    #[test]
    fn dual_sign_rules() {
        let s = DualSign { signer_a: 0xAAAA, signer_b: 0xBBBB, ledger_fp: 0xCCCC };
        assert!(s.validate().is_ok());
        let same = DualSign { signer_a: 0xAAAA, signer_b: 0xAAAA, ledger_fp: 0xCCCC };
        assert!(same.validate().is_err());
    }
    #[test]
    fn closeout_end_to_end() {
        let ledger = full_ledger();
        let perf = PerfSummary { peak_parallelism: 8, steal_imbalance_permille: 90, hybrid_gain_permille: 300, monitor_overhead_permille: 400 };
        let det = DeterminismSummary { modules: 3, deterministic_ok: 3, fuzz_cases: 64, fuzz_passed: 64 };
        let sign = DualSign { signer_a: 0xA, signer_b: 0xB, ledger_fp: ledger.ledger_fp() };
        let rep = b03_closeout(&ledger, &perf, &det, &sign).unwrap();
        assert!(rep.validate().is_ok());
        // 双签对错台账 → 拒绝。
        let wrong_sign = DualSign { signer_a: 0xA, signer_b: 0xB, ledger_fp: 0xDEAD };
        assert!(b03_closeout(&ledger, &perf, &det, &wrong_sign).is_err());
        // 移交包合法。
        let hp = HandoverPack { split_seg: 0x05B0, steal_seg: 0x05C0, panel_seg: 0x0620, tpl_seg: 0x0640, precond_fp: rep.ledger_fp };
        assert!(hp.validate().is_ok());
        let bp = GroupBreakpoint { group: 3, seq: 1, report_fp: rep.ledger_fp };
        assert!(bp.validate().is_ok());
    }
}
