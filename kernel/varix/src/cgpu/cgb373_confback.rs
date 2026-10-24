//! CGPU-F0373 · 预测置信与回退（判据段 0x09D0）
//! 置信回退：置信度评估（预测 vs 实测滚动偏差——实时置信分）；
//! 三级响应（高置信→预留/中置信→部分预留/低置信→纯被动）；回退
//! 协议（预测失效→被动仲裁无缝接管——零窗口期）；回退显性（计数
//! +归因）。判据：置信分、三级响应、零窗口、显性、判据。
//!
//! 锚点原文摘录（施工书 1657 行）：
//! 「置信度评估（预测 vs 实测滚动偏差——实时置信分）；三级响应
//! （高置信→预留/中置信→部分预留/低置信→纯被动）；回退协议（预
//! 测失效→被动仲裁无缝接管——零窗口期）；回退显性（计数+归因）。
//! 判据：置信分、三级响应、零窗口、显性、判据。」
//!
//! 诊断码段 0x09D0（本模块独占：0x09D0~0x09DF）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x09D0;

/// 置信评估窗（帧）。
pub const CONF_WINDOW: usize = 32;

/// 三级响应阈值（千分比置信分）：高 ≥800 → 全预留；650~799 →
/// 半预留；<650 → 纯被动。
pub const CONF_HIGH: u32 = 800;
pub const CONF_MID: u32 = 650;

/// 部分预留折扣（千分比）：中置信档按 500‰ 预留。
pub const PARTIAL_RESERVE_PERMILLE: u32 = 500;

/// 零窗口核验值（判据「零窗口」）：回退生效步数恒 0（无缝）。
pub const FALLBACK_GAP_STEPS: u32 = 0;

/// 滚动偏差评估器（判据「置信分」核心机）：预测 vs 实测逐帧记
/// 偏差，滚动窗内归一化为置信分。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConfidenceEvaluator {
    /// 滚动相对偏差（千分比，非负——|pred-actual|/max(actual,1)）。
    pub window: Vec<u32>,
    /// 有效样本。
    pub len: usize,
}

impl ConfidenceEvaluator {
    /// 空评估器。
    pub fn new() -> Self {
        ConfidenceEvaluator { window: Vec::new(), len: 0 }
    }
    /// 喂一对（预测, 实测）——记相对偏差。
    pub fn observe(&mut self, predicted: u32, actual: u32) -> Result<u32, u16> {
        if actual == 0 && predicted == 0 {
            // 双零：完美命中，偏差 0。
            let dev = 0u32;
            self.push(dev)?;
            return Ok(dev);
        }
        let denom = actual.max(1) as u64;
        let diff = (predicted as i64 - actual as i64).unsigned_abs();
        let dev = ((diff as u64 * 1000) / denom) as u32;
        self.push(dev)?;
        Ok(dev)
    }
    fn push(&mut self, dev: u32) -> Result<(), u16> {
        if self.window.len() >= CONF_WINDOW {
            self.window.remove(0);
        } else {
            self.len += 1;
        }
        self.window.push(dev);
        Ok(())
    }
    /// 实时置信分（判据「置信分」主判据）：score = 1000 − 窗均偏差
    /// （钳 [0,1000]）；空窗不猜——None。
    pub fn score(&self) -> Option<u32> {
        if self.len == 0 {
            return None;
        }
        let sum: u64 = self.window.iter().map(|&d| d as u64).sum();
        let mean = (sum / self.len as u64) as u32;
        Some(1000u32.saturating_sub(mean))
    }
    /// 满窗核。
    pub fn full(&self) -> bool {
        self.len == CONF_WINDOW
    }
}

impl Default for ConfidenceEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

/// 响应级（判据「三级响应」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResponseTier {
    /// 高置信：全预留。
    FullReserve,
    /// 中置信：部分预留（×500‰）。
    PartialReserve,
    /// 低置信：纯被动。
    Passive,
}

impl ResponseTier {
    /// 级号。
    pub fn ordinal(&self) -> u8 {
        match self {
            ResponseTier::FullReserve => 0,
            ResponseTier::PartialReserve => 1,
            ResponseTier::Passive => 2,
        }
    }
    /// 响应裁决（判据「三级响应」主判据）：置信分 → 级 + 预留量。
    pub fn decide(score: u32, demand_permille: u32) -> Result<(ResponseTier, u32), u16> {
        if demand_permille > 1000 {
            return Err(SEG_BASE + 1);
        }
        if score > 1000 {
            return Err(SEG_BASE + 2);
        }
        let (tier, amount) = if score >= CONF_HIGH {
            (ResponseTier::FullReserve, demand_permille)
        } else if score >= CONF_MID {
            (ResponseTier::PartialReserve, demand_permille * PARTIAL_RESERVE_PERMILLE / 1000)
        } else {
            (ResponseTier::Passive, 0)
        };
        Ok((tier, amount))
    }
}

/// 回退账（判据「显性」一等公民）：失效计数 + 归因 + 零窗口证据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FallbackRecord {
    /// 归因域（0=置信不足 1=预测器失活 2=样本断流）。
    pub cause: u8,
    /// 失效帧号。
    pub frame: u64,
    /// 回退窗口（步数）——恒 0（无缝证据）。
    pub gap_steps: u32,
}

impl FallbackRecord {
    /// 合法性（判据「零窗口」硬闸）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.cause > 2 {
            return Err(SEG_BASE + 3);
        }
        if self.gap_steps != FALLBACK_GAP_STEPS {
            return Err(SEG_BASE + 4);
        }
        Ok(())
    }
    /// 归因名。
    pub fn cause_name(&self) -> &'static str {
        match self.cause {
            0 => "置信不足",
            1 => "预测器失活",
            _ => "样本断流",
        }
    }
}

/// 回退账本（判据「显性」主判据）：失效必记账，账行全合法，账
/// 满显性拒绝（封顶 32 行）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FallbackLedger {
    /// 账行。
    pub rows: Vec<FallbackRecord>,
}

impl FallbackLedger {
    /// 空。
    pub fn new() -> Self {
        FallbackLedger { rows: Vec::new() }
    }
    /// 记一笔（回退显性）。
    pub fn record(&mut self, r: FallbackRecord) -> Result<(), u16> {
        r.validate()?;
        if self.rows.len() >= 32 {
            return Err(SEG_BASE + 5);
        }
        self.rows.push(r);
        Ok(())
    }
    /// 按归因统计（判据「显性」输出面）。
    pub fn count_by_cause(&self, cause: u8) -> Result<u32, u16> {
        if cause > 2 {
            return Err(SEG_BASE + 3);
        }
        Ok(self.rows.iter().filter(|r| r.cause == cause).count() as u32)
    }
    /// 全账合法核。
    pub fn audit(&self) -> Result<(), u16> {
        for r in &self.rows {
            r.validate()?;
        }
        Ok(())
    }
}

impl Default for FallbackLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 无缝回退全链（判据「零窗口」主判据）：置信分低于中线 → 立即
/// 切被动 + 记账 + 窗口证据 = 0——三步同帧完成（零窗口的落地）。
pub fn seamless_fallback(
    score: u32,
    demand: u32,
    frame: u64,
    cause: u8,
    ledger: &mut FallbackLedger,
) -> Result<(ResponseTier, u32), u16> {
    let (tier, amount) = ResponseTier::decide(score, demand)?;
    if tier == ResponseTier::Passive {
        ledger.record(FallbackRecord { cause, frame, gap_steps: FALLBACK_GAP_STEPS })?;
    }
    Ok((tier, amount))
}

/// 置信回退诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "内部错误",
        1 => "需求量 >1000",
        2 => "置信分 >1000",
        3 => "归因域非法（0/1/2）",
        4 => "回退窗口≠0——零窗口被破坏",
        5 => "回退账满（32 行）——先归档",
        _ => "未知诊断码——查本模块 SEG_BASE 0x09D0",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_scoring() {
        let mut ev = ConfidenceEvaluator::new();
        assert!(ev.score().is_none());
        // 完美预测：偏差 0 → 满分。
        for _ in 0..CONF_WINDOW {
            ev.observe(400, 400).unwrap();
        }
        assert!(ev.full());
        assert_eq!(ev.score(), Some(1000));
        // 持续低估 20%：偏差 200 → 分 800。
        let mut ev2 = ConfidenceEvaluator::new();
        for _ in 0..CONF_WINDOW {
            ev2.observe(320, 400).unwrap();
        }
        assert_eq!(ev2.score(), Some(800));
        // 双零完美命中。
        let mut ev3 = ConfidenceEvaluator::new();
        assert_eq!(ev3.observe(0, 0).unwrap(), 0);
        assert_eq!(ev3.score(), Some(1000));
    }

    #[test]
    fn three_tier_response() {
        let (t1, a1) = ResponseTier::decide(900, 600).unwrap();
        assert_eq!(t1, ResponseTier::FullReserve);
        assert_eq!(a1, 600);
        let (t2, a2) = ResponseTier::decide(700, 600).unwrap();
        assert_eq!(t2, ResponseTier::PartialReserve);
        assert_eq!(a2, 300); // 600×500‰。
        let (t3, a3) = ResponseTier::decide(400, 600).unwrap();
        assert_eq!(t3, ResponseTier::Passive);
        assert_eq!(a3, 0);
        assert_eq!(ResponseTier::decide(500, 1001).unwrap_err(), SEG_BASE + 1);
        assert_eq!(ResponseTier::decide(1001, 100).unwrap_err(), SEG_BASE + 2);
    }

    #[test]
    fn seamless_fallback_zero_gap() {
        let mut led = FallbackLedger::new();
        let (tier, amt) = seamless_fallback(300, 500, 77, 0, &mut led).unwrap();
        assert_eq!(tier, ResponseTier::Passive);
        assert_eq!(amt, 0);
        assert_eq!(led.rows.len(), 1);
        assert_eq!(led.rows[0].frame, 77);
        assert_eq!(led.rows[0].gap_steps, FALLBACK_GAP_STEPS);
        assert!(led.audit().is_ok());
        assert_eq!(led.count_by_cause(0).unwrap(), 1);
        // 高置信不记账。
        seamless_fallback(950, 500, 78, 0, &mut led).unwrap();
        assert_eq!(led.rows.len(), 1);
        // 非零窗口拒绝。
        let bad = FallbackRecord { cause: 1, frame: 1, gap_steps: 3 };
        assert_eq!(led.record(bad).unwrap_err(), SEG_BASE + 4);
        assert!(diagnose(SEG_BASE + 4).contains("零窗口"));
        assert_eq!(bad.cause_name(), "预测器失活");
    }
}
