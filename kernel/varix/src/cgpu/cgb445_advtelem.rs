//! CGPU-F0445 · 高级特性遥测（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 遥测：aging 触发率/动态调节幅度/混沌演练结果/极限逼近度——四指标
//!   进 B08 总线；高级特性全部可观测。判据：四指标、可观测、判据。
//!
//! 设计纪律：
//! - 四指标各自语义：aging 触发率（触发次/登记任务，千分位）、动态调节
//!   幅度（单次最大 |Δ权重|/权重基数，千分位）、混沌演练结果（最近一次
//!   verdict 绿=0 红=诊断码）、极限逼近度（峰值申请/极限基线，千分位）。
//! - 进 B08 同源帧：魔数+序号+四槽+校验字，帧序只增、自校验可往返
//!   （与 F0429 统计遥测同构——总线帧格式全域一致）。
//! - 高级特性全部可观测的自证：四点名称表覆盖 + 计数只增防线。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0F00 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0F00，全仓独占）。
pub const SEG_BASE: u16 = 0x0F00;

pub const E_AT_WINDOW: u16 = SEG_BASE + 1; // 观测窗非法
pub const E_AT_NEG: u16 = SEG_BASE + 2; // 计数回退
pub const E_AT_BUS: u16 = SEG_BASE + 3; // 帧校验错

/// 指标点。
pub const AT_AGING: usize = 0;
pub const AT_FAIRADJ: usize = 1;
pub const AT_CHAOS: usize = 2;
pub const AT_LIMIT: usize = 3;
pub const AT_POINTS: usize = 4;

pub fn at_point_name(p: usize) -> &'static str {
    match p {
        AT_AGING => "aging触发率",
        AT_FAIRADJ => "动态调节幅度",
        AT_CHAOS => "混沌演练",
        _ => "极限逼近度",
    }
}

/// B08 同源帧魔数。
pub const AT_MAGIC: u32 = 0xB08F_0F00;

/// 高级特性遥测帧。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtFrame {
    pub magic: u32,
    pub seq: u64,
    /// 四槽：[aging触发率‰, 最大调节幅度‰, 混沌verdict, 极限逼近‰]。
    pub metrics: [u64; AT_POINTS],
    pub check: u32,
}

impl AtFrame {
    fn compute_check(m: &[u64; AT_POINTS]) -> u32 {
        m.iter().fold(0u64, |a, v| a.wrapping_add(*v)) as u32
    }

    pub fn self_ok(&self) -> bool {
        self.magic == AT_MAGIC && self.check == Self::compute_check(&self.metrics)
    }
}

/// 高级特性遥测器。
#[derive(Debug)]
pub struct AdvTelemetry {
    // aging 观测
    aging_fired: u64,
    aging_enrolled: u64,
    // 动态调节观测
    pub max_adj_pm: u64,
    // 混沌演练观测
    pub last_chaos_verdict: u64, // 0=绿，其他=诊断码
    chaos_runs: u64,
    // 极限逼近
    pub peak_requests: u64,
    limit_base: u64,
    // 总线
    seq: u64,
}

impl Default for AdvTelemetry {
    fn default() -> Self {
        Self {
            aging_fired: 0,
            aging_enrolled: 0,
            max_adj_pm: 0,
            last_chaos_verdict: 0,
            chaos_runs: 0,
            peak_requests: 0,
            limit_base: 1_000,
            seq: 0,
        }
    }
}

impl AdvTelemetry {
    pub fn new(limit_base: u64) -> Result<Self, u16> {
        if limit_base == 0 {
            return Err(E_AT_WINDOW);
        }
        Ok(Self {
            limit_base,
            ..Default::default()
        })
    }

    /// aging 观测：登记任务/触发计数（只增）。
    pub fn note_enroll(&mut self, n: u64) {
        self.aging_enrolled = self.aging_enrolled.saturating_add(n);
    }

    pub fn note_aging_fire(&mut self, n: u64) {
        self.aging_fired = self.aging_fired.saturating_add(n);
    }

    /// 动态调节观测：记录单次幅度峰值。
    pub fn note_fair_adjust(&mut self, adj_pm: u64) {
        if adj_pm > self.max_adj_pm {
            self.max_adj_pm = adj_pm;
        }
    }

    /// 混沌演练观测：verdict（0=绿）。
    pub fn note_chaos(&mut self, verdict: u64) {
        self.last_chaos_verdict = verdict;
        self.chaos_runs = self.chaos_runs.saturating_add(1);
    }

    /// 极限逼近观测：峰值申请。
    pub fn note_requests(&mut self, total: u64) {
        if total > self.peak_requests {
            self.peak_requests = total;
        }
    }

    /// aging 触发率（千分位；无登记 → 0，不虚报）。
    pub fn aging_rate_pm(&self) -> u64 {
        if self.aging_enrolled == 0 {
            0
        } else {
            self.aging_fired.saturating_mul(1000) / self.aging_enrolled
        }
    }

    /// 极限逼近度（千分位）。
    pub fn limit_pm(&self) -> u64 {
        self.peak_requests.saturating_mul(1000) / self.limit_base
    }

    /// 发射 B08 帧。
    pub fn emit_bus(&mut self) -> AtFrame {
        self.seq = self.seq.saturating_add(1);
        let metrics = [
            self.aging_rate_pm(),
            self.max_adj_pm,
            self.last_chaos_verdict,
            self.limit_pm(),
        ];
        AtFrame {
            magic: AT_MAGIC,
            seq: self.seq,
            check: AtFrame::compute_check(&metrics),
            metrics,
        }
    }

    /// 只增防线（aging 触发数/混沌轮数/峰值不许回退）。
    pub fn check_monotonic(&self, prev: [u64; AT_POINTS], now: [u64; AT_POINTS]) -> Result<(), u16> {
        for i in 0..AT_POINTS {
            // 混沌槽是 verdict 语义（可变），跳过。
            if i == AT_CHAOS {
                continue;
            }
            if now[i] < prev[i] {
                return Err(E_AT_NEG);
            }
        }
        Ok(())
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn chaos_runs(&self) -> u64 {
        self.chaos_runs
    }
}

// ==========================================================================
// 判据（施工书：四指标、可观测、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_四指标覆盖与名称表() {
        let mut t = AdvTelemetry::new(1_000).unwrap();
        t.note_enroll(40);
        t.note_aging_fire(10);
        t.note_fair_adjust(30);
        t.note_chaos(0);
        t.note_requests(900);
        // 四点名称表覆盖。
        for p in 0..AT_POINTS {
            assert!(!at_point_name(p).is_empty());
        }
        // 帧内四槽各就各位。
        let f = t.emit_bus();
        assert_eq!(f.metrics[AT_AGING], 250); // 10/40。
        assert_eq!(f.metrics[AT_FAIRADJ], 30);
        assert_eq!(f.metrics[AT_CHAOS], 0); // 绿。
        assert_eq!(f.metrics[AT_LIMIT], 900); // 900/1000。
    }

    #[test]
    fn a02_进总线_自校验与序号只增() {
        let mut t = AdvTelemetry::new(1_000).unwrap();
        let f1 = t.emit_bus();
        let f2 = t.emit_bus();
        assert!(f1.self_ok() && f2.self_ok());
        assert_eq!(f1.magic, AT_MAGIC);
        assert_eq!(f2.seq, f1.seq + 1);
        assert_eq!(t.seq(), 2);
        // 篡改槽 → 自校验红。
        let mut bad = f1;
        bad.metrics[AT_LIMIT] = bad.metrics[AT_LIMIT].wrapping_add(1);
        assert!(!bad.self_ok());
        let mut bad2 = f1;
        bad2.magic = 0;
        assert!(!bad2.self_ok());
    }

    #[test]
    fn a03_aging触发率_无登记不虚报() {
        let mut t = AdvTelemetry::new(1_000).unwrap();
        assert_eq!(t.aging_rate_pm(), 0);
        t.note_enroll(0);
        assert_eq!(t.aging_rate_pm(), 0);
        t.note_enroll(3);
        t.note_aging_fire(1);
        assert_eq!(t.aging_rate_pm(), 333);
        t.note_aging_fire(2);
        assert_eq!(t.aging_rate_pm(), 1000);
    }

    #[test]
    fn a04_动态调节峰值与混沌账() {
        let mut t = AdvTelemetry::new(1_000).unwrap();
        t.note_fair_adjust(10);
        t.note_fair_adjust(50);
        t.note_fair_adjust(30); // 非峰值不覆盖。
        assert_eq!(t.max_adj_pm, 50);
        t.note_chaos(0);
        t.note_chaos(E_AT_NEG as u64); // 最近一次为红。
        assert_eq!(t.chaos_runs(), 2);
        assert_eq!(t.last_chaos_verdict, E_AT_NEG as u64);
        // 帧内反映最近状态。
        let f = t.emit_bus();
        assert_eq!(f.metrics[AT_CHAOS], E_AT_NEG as u64);
    }

    #[test]
    fn a05_只增防线_混沌槽豁免() {
        let mut t = AdvTelemetry::new(1_000).unwrap();
        t.note_enroll(10);
        t.note_aging_fire(5);
        t.note_fair_adjust(20);
        t.note_requests(500);
        let prev = t.emit_bus().metrics;
        // 合法推进。
        t.note_aging_fire(5);
        t.note_requests(600);
        let now = t.emit_bus().metrics;
        assert_eq!(t.check_monotonic(prev, now), Ok(()));
        // 回退拒：aging 率掉。
        let mut reg = now;
        reg[AT_AGING] = prev[AT_AGING].saturating_sub(1);
        assert_eq!(t.check_monotonic(prev, reg), Err(E_AT_NEG));
        // 极限逼近回退拒。
        let mut reg2 = now;
        reg2[AT_LIMIT] = 0;
        assert_eq!(t.check_monotonic(prev, reg2), Err(E_AT_NEG));
        // 混沌槽可变（verdict 语义）。
        let mut ch = now;
        ch[AT_CHAOS] = 0;
        assert_eq!(t.check_monotonic(prev, ch), Ok(()));
        // 非法基线拒。
        assert!(matches!(AdvTelemetry::new(0), Err(E_AT_WINDOW)));
    }
}
