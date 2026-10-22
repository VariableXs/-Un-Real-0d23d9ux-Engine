//! CGPU-F0212 · 实时保障通道（判据段 0x06E0）
//! 对延迟极端敏感的任务的保障通道：保障通道语义（通道内任务跳过
//! 所有排队直接提交——绕过预算排队），通道准入（只有合成器/输入
//! 响应两类任务可进——准入白名单严格），通道代价（保障通道使用可
//! 能降低非通道任务吞吐——代价量化入遥测），通道滥用防护（使用频
//! 率监控——滥用自动降级到标准通道）。判据：通道语义、准入白名单、
//! 代价量化、滥用防护、延迟改善实测。
//!
//! 诊断码段 0x06E0（本模块独占：0x06E0~0x06EF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x06E0;

/// 通道准入白名单（判据「准入白名单」）：合成器与输入响应两类。
pub const CLASS_COMPOSITOR: u8 = 1;
/// 输入响应类。
pub const CLASS_INPUT: u8 = 2;

/// 滥用率红线（千分比）：通道任务占全部任务 > 300‰ 判滥用。
pub const ABUSE_THRESHOLD_PERMILLE: u32 = 300;

/// 通道代价观测窗（次）：滑动窗口大小。
pub const COST_WINDOW: usize = 32;

/// 通道任务类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FastLaneClass {
    /// 合成器任务。
    Compositor,
    /// 输入响应任务。
    Input,
}

impl FastLaneClass {
    /// 类别码。
    pub fn code(&self) -> u8 {
        match self {
            FastLaneClass::Compositor => CLASS_COMPOSITOR,
            FastLaneClass::Input => CLASS_INPUT,
        }
    }
}

/// 通道任务（判据「通道语义」载体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FastLaneTask {
    /// 任务号。
    pub id: u32,
    /// 类别。
    pub class: FastLaneClass,
    /// 通道内直提延迟（微秒）。
    pub lane_us: u32,
    /// 同任务标准通道延迟（微秒）。
    pub normal_us: u32,
}

impl FastLaneTask {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.id == 0 || self.lane_us == 0 || self.normal_us == 0 {
            return Err(SEG_BASE);
        }
        if self.lane_us >= self.normal_us {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
    /// 延迟改善（千分比）。
    pub fn improvement_permille(&self) -> Result<u32, u16> {
        self.validate()?;
        Ok(((self.normal_us - self.lane_us) as u64 * 1000 / self.normal_us as u64) as u32)
    }
}

/// 通道准入裁决（判据「准入白名单」主判据）：仅合成器/输入响应
/// 两类可进——白名单外任务一律拒绝。
pub fn admit_to_lane(class: FastLaneClass) -> Result<bool, u16> {
    Ok(matches!(class.code(), CLASS_COMPOSITOR | CLASS_INPUT))
}

/// 通道使用账（判据「滥用防护」容器）：滑动窗口记录通道/标准
/// 提交序列（true = 通道）。
#[derive(Default)]
pub struct LaneLedger {
    window: Vec<bool>,
    total_lane: u64,
    total_normal: u64,
}

impl LaneLedger {
    /// 记录一次提交。
    pub fn record(&mut self, via_lane: bool) {
        if self.window.len() >= COST_WINDOW {
            self.window.remove(0);
        }
        self.window.push(via_lane);
        if via_lane {
            self.total_lane += 1;
        } else {
            self.total_normal += 1;
        }
    }
    /// 窗口占用率（千分比）：通道提交 / 全部提交。
    pub fn window_usage_permille(&self) -> Result<u32, u16> {
        if self.window.is_empty() {
            return Err(SEG_BASE + 2);
        }
        let lane = self.window.iter().filter(|&&b| b).count();
        Ok((lane * 1000 / self.window.len()) as u32)
    }
    /// 累计通道提交数。
    pub fn lane_total(&self) -> u64 {
        self.total_lane
    }
    /// 滥用判定（判据「滥用防护」主判据）：窗口占用率超红线。
    pub fn abusing(&self) -> Result<bool, u16> {
        Ok(self.window_usage_permille()? > ABUSE_THRESHOLD_PERMILLE)
    }
}

/// 滥用降级（判据「滥用防护」执行面）：滥用确认后强制降级——
/// 下一个非白名单任务改走标准通道。返回降级动作是否执行。
pub fn demote_on_abuse(ledger: &LaneLedger, class: FastLaneClass) -> Result<bool, u16> {
    if !admit_to_lane(class)? {
        return Ok(true);
    }
    if ledger.abusing()? {
        // 白名单类在滥用期也要限流（降级到标准通道）。
        return Ok(true);
    }
    Ok(false)
}

/// 通道代价核算（判据「代价量化」核心）：通道任务的直提会挤占
/// 非通道任务吞吐——代价 = 被挤占的标准通道时间片（微秒）。窗口
/// 内通道任务每次直提代价 = 恒定抢占成本 × 通道次数。
pub fn lane_throughput_cost_us(lane_submissions: u32, preempt_us_each: u32) -> Result<u64, u16> {
    if lane_submissions == 0 || preempt_us_each == 0 {
        return Err(SEG_BASE + 3);
    }
    if lane_submissions > 100_000 {
        return Err(SEG_BASE + 4);
    }
    Ok(lane_submissions as u64 * preempt_us_each as u64)
}

/// 代价-收益对账（判据「代价量化」审计面）：通道带来的延迟改善
/// 总量 vs 吞吐代价——收益必须覆盖代价（否则通道策略回退）。
pub fn lane_worthwhile(total_improvement_us: u64, throughput_cost_us: u64) -> Result<bool, u16> {
    if total_improvement_us == 0 || throughput_cost_us == 0 {
        return Err(SEG_BASE + 5);
    }
    Ok(total_improvement_us > throughput_cost_us)
}

/// 延迟改善实测聚合（判据「延迟改善实测」主判据）：窗口内全部
/// 通道任务的平均改善千分比（须 > 0）。
pub fn avg_improvement_permille(tasks: &[FastLaneTask]) -> Result<u32, u16> {
    if tasks.is_empty() {
        return Err(SEG_BASE + 6);
    }
    let mut sum: u64 = 0;
    for t in tasks {
        t.validate()?;
        sum += t.improvement_permille()? as u64;
    }
    let avg = sum / tasks.len() as u64;
    if avg == 0 {
        return Err(SEG_BASE + 7);
    }
    Ok(avg as u32)
}

/// 通道语义完整性（判据「通道语义」主判据）：通道任务不得参与
/// 预算排队——传入任务的排队位（排队深度）必须为 0（直提）。
pub fn lane_bypasses_queue(queue_depth: u32) -> Result<(), u16> {
    if queue_depth != 0 {
        return Err(SEG_BASE + 8);
    }
    Ok(())
}

/// 通道任务账指纹（可审计面）。
pub fn lane_fp(tasks: &[FastLaneTask]) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for t in tasks {
        acc = acc.wrapping_mul(0x100000001b3) ^ t.id as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ t.class.code() as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ t.lane_us as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ t.normal_us as u64;
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    fn lane(id: u32, class: FastLaneClass, l: u32, n: u32) -> FastLaneTask {
        FastLaneTask { id, class, lane_us: l, normal_us: n }
    }
    #[test]
    fn whitelist_admission() {
        assert!(admit_to_lane(FastLaneClass::Compositor).unwrap());
        assert!(admit_to_lane(FastLaneClass::Input).unwrap());
        // 类别码外无白名单项（枚举封闭，白名单语义以 code 校验）。
        assert_eq!(CLASS_COMPOSITOR, 1);
        assert_eq!(CLASS_INPUT, 2);
    }
    #[test]
    fn lane_semantics_direct_submit() {
        assert!(lane_bypasses_queue(0).is_ok());
        assert!(lane_bypasses_queue(3).is_err());
    }
    #[test]
    fn improvement_quantified() {
        let t = lane(1, FastLaneClass::Compositor, 200, 1000);
        assert_eq!(t.improvement_permille().unwrap(), 800);
        // 通道必须更快。
        let bad = FastLaneTask { id: 2, class: FastLaneClass::Input, lane_us: 900, normal_us: 500 };
        assert!(bad.validate().is_err());
        let batch = vec![
            lane(1, FastLaneClass::Compositor, 200, 1000),
            lane(2, FastLaneClass::Input, 400, 2000),
        ];
        assert_eq!(avg_improvement_permille(&batch).unwrap(), 800);
    }
    #[test]
    fn abuse_detection_and_demotion() {
        let mut led = LaneLedger::default();
        for _ in 0..20 {
            led.record(true);
        }
        for _ in 0..12 {
            led.record(false);
        }
        // 20/32 = 625‰ > 300‰ → 滥用。
        assert!(led.abusing().unwrap());
        assert!(demote_on_abuse(&led, FastLaneClass::Compositor).unwrap());
        // 正常使用不降级。
        let mut led2 = LaneLedger::default();
        for _ in 0..4 {
            led2.record(true);
        }
        for _ in 0..28 {
            led2.record(false);
        }
        assert!(!led2.abusing().unwrap());
        assert!(!demote_on_abuse(&led2, FastLaneClass::Compositor).unwrap());
    }
    #[test]
    fn cost_benefit_ledger() {
        let cost = lane_throughput_cost_us(10, 500).unwrap();
        assert_eq!(cost, 5000);
        // 改善 8000 > 代价 5000 → 划算。
        assert!(lane_worthwhile(8000, 5000).unwrap());
        assert!(!lane_worthwhile(3000, 5000).unwrap());
    }
    #[test]
    fn lane_fp_stable() {
        let a = vec![lane(1, FastLaneClass::Input, 100, 400)];
        let b = a.clone();
        assert_eq!(lane_fp(&a), lane_fp(&b));
    }
}
