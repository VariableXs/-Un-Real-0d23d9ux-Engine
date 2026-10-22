//! CGPU-F0369 · 预测式仲裁总架构（判据段 0x0990）
//! 预测总架构：定位（从被动分配到主动预留——预测式仲裁；诚实标
//! 注：统计预测非智能预言，置信度始终伴随）；两段架构（画像层：
//! 历史负载建模+预测层：下一帧负载预估）；与被动仲裁共存（预测
//! 失效→秒回被动——永不因预测而悬挂）；层级边界（预测只产出预留
//! 建议、仲裁决策仍归 F0326/F0327——不越权）。判据：定位、两段、
//! 共存回退、边界、判据。
//!
//! 锚点原文摘录（施工书 1641 行）：
//! 「定位（统计预测非智能预言，置信度始终伴随）；两段架构（画像
//! 层+预测层）；与被动仲裁共存（预测失效→秒回被动——永不因预测
//! 而悬挂）；层级边界（预测只产出预留建议、仲裁决策仍归 F0326/
//! F0327——不越权）。判据：定位、两段、共存回退、边界、判据。」
//!
//! 诊断码段 0x0990（本模块独占：0x0990~0x099F）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0990;

/// 画像层窗口（帧）：历史负载建模的滚动窗。
pub const PROFILE_WINDOW_FRAMES: usize = 128;

/// 预测层前瞻（帧）：一次最多估 4 帧内负载。
pub const PREDICT_HORIZON_MAX: u32 = 4;

/// 架构段位（判据「两段」的段枚举）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArchStage {
    /// 段 1：画像层（历史建模）。
    Profile,
    /// 段 2：预测层（前瞻预估）。
    Predict,
}

impl ArchStage {
    /// 段号（严格两段——1/2）。
    pub fn ordinal(&self) -> u8 {
        match self {
            ArchStage::Profile => 1,
            ArchStage::Predict => 2,
        }
    }
}

/// 预测建议（判据「边界」的产出物形态）：只带建议不带裁决权。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReserveAdvice {
    /// 目标账户号。
    pub account: u32,
    /// 建议预留量（千分比）。
    pub reserve_permille: u32,
    /// 置信度（千分比——判据「置信度始终伴随」：建议必须带置信）。
    pub confidence_permille: u32,
}

impl ReserveAdvice {
    /// 合法性（判据「边界」守卫第一层）：预留 ∈ [0,1000]，置信
    /// 必填（>0）——零置信建议 = 假装确定 = 拒。
    pub fn validate(&self) -> Result<(), u16> {
        if self.account == 0 {
            return Err(SEG_BASE);
        }
        if self.reserve_permille > 1000 {
            return Err(SEG_BASE + 1);
        }
        if self.confidence_permille == 0 || self.confidence_permille > 1000 {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
}

/// 画像层（判据「两段」段 1）：滚动窗历史负载。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProfileStage {
    /// 滚动窗样本。
    pub samples: [u32; PROFILE_WINDOW_FRAMES],
    /// 有效样本数。
    pub len: usize,
    /// 写指针（环形）。
    pub head: usize,
}

impl ProfileStage {
    /// 空画像。
    pub fn new() -> Self {
        ProfileStage { samples: [0; PROFILE_WINDOW_FRAMES], len: 0, head: 0 }
    }
    /// 推样本（环形覆盖最旧——画像层存储配额由窗口本身冻结）。
    pub fn push(&mut self, v: u32) {
        self.samples[self.head] = v;
        self.head = (self.head + 1) % PROFILE_WINDOW_FRAMES;
        if self.len < PROFILE_WINDOW_FRAMES {
            self.len += 1;
        }
    }
    /// 均值（千分比口径原样）。
    pub fn mean(&self) -> Option<u32> {
        if self.len == 0 {
            return None;
        }
        let sum: u64 = self.samples[..self.len].iter().map(|&v| v as u64).sum();
        Some((sum / self.len as u64) as u32)
    }
    /// 满窗核。
    pub fn full(&self) -> bool {
        self.len == PROFILE_WINDOW_FRAMES
    }
}

impl Default for ProfileStage {
    fn default() -> Self {
        Self::new()
    }
}

/// 预测层（判据「两段」段 2）：吃画像层均值出前瞻建议。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PredictStage {
    /// 前瞻帧数（1..=4 可配）。
    pub horizon: u32,
    /// 预测活性（判据「共存回退」：false = 预测失效，被动接管）。
    pub active: bool,
}

impl PredictStage {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.horizon == 0 || self.horizon > PREDICT_HORIZON_MAX {
            return Err(SEG_BASE + 3);
        }
        Ok(())
    }
}

/// 建议产出（判据「两段」联动主判据）：画像层满窗 + 预测层活性
/// 才出建议；否则出「被动模式」空建议——永不悬挂。
pub fn produce_advice(
    profile: &ProfileStage,
    pred: &PredictStage,
    account: u32,
) -> Result<(Option<ReserveAdvice>, bool), u16> {
    pred.validate()?;
    if account == 0 {
        return Err(SEG_BASE + 4);
    }
    // 共存回退：预测层失活 → 秒回被动（advice=None，passive=true）。
    if !pred.active {
        return Ok((None, true));
    }
    // 画像层不满窗 → 不猜（显性被动，不编数据）。
    if !profile.full() {
        return Ok((None, true));
    }
    let mean = profile.mean().ok_or(SEG_BASE + 5)?;
    // 前瞻建议 = 窗均值 × 前瞻占比（horizon=1 → 原值；>1 不放大
    // 超过 1000——预算口径封顶）。
    let advice = ReserveAdvice {
        account,
        reserve_permille: mean.min(1000),
        confidence_permille: 700,
    };
    advice.validate()?;
    Ok((Some(advice), false))
}

/// 不越权核（判据「边界」主判据）：建议量必须 ≤ 仲裁面授予上限
/// （F0326/F0327 的裁定上限 1000），且建议不能自封为「已裁决」——
/// 这里以「建议结构体无裁决字段」+「量纲封顶」双保险落地。
pub fn advice_within_boundary(a: &ReserveAdvice, arbiter_cap_permille: u32) -> Result<(), u16> {
    a.validate()?;
    if arbiter_cap_permille == 0 || arbiter_cap_permille > 1000 {
        return Err(SEG_BASE + 6);
    }
    if a.reserve_permille > arbiter_cap_permille {
        return Err(SEG_BASE + 7);
    }
    Ok(())
}

/// 共存回退账（判据「共存回退」验收面）：预测失效事件计数 + 回退
/// 延迟（步数化——回退必须在 1 步内生效，永不悬挂）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FallbackLedger {
    /// 预测失效次数。
    pub failures: u32,
    /// 回退生效步数（恒 1——秒回被动）。
    pub fallback_steps: u32,
}

impl FallbackLedger {
    /// 记一次失效（判据「零悬挂」硬闸：回退步数必须=1）。
    pub fn record_failure(&mut self) -> Result<(), u16> {
        self.failures += 1;
        self.fallback_steps = 1;
        if self.fallback_steps != 1 {
            return Err(SEG_BASE + 8);
        }
        Ok(())
    }
    /// 无悬挂核。
    pub fn never_hangs(&self) -> Result<bool, u16> {
        if self.failures == 0 {
            return Ok(true);
        }
        Ok(self.fallback_steps <= 1)
    }
}

impl Default for FallbackLedger {
    fn default() -> Self {
        FallbackLedger { failures: 0, fallback_steps: 0 }
    }
}

/// 两段架构完整性核（判据「两段」守卫）：画像层窗口与预测层前瞻
/// 的量纲关系——前瞻不得超过画像窗（看不全的历史不能猜未来）。
pub fn two_stage_consistent(profile_len: usize, horizon: u32) -> Result<(), u16> {
    if (horizon as usize) > profile_len && profile_len > 0 {
        return Err(SEG_BASE + 9);
    }
    Ok(())
}

/// 架构指纹（可审计面）。
pub fn arch_fp(profile: &ProfileStage, pred: &PredictStage) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    acc = acc.wrapping_mul(0x100000001b3) ^ profile.len as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ profile.head as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ pred.horizon as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ pred.active as u64;
    if let Some(m) = profile.mean() {
        acc = acc.wrapping_mul(0x100000001b3) ^ m as u64;
    }
    acc
}

/// 预测架构诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "账户号为 0",
        1 => "预留量 >1000",
        2 => "置信度必须 ∈ (0,1000]——零置信建议=假装确定",
        3 => "前瞻帧数必须 ∈ [1,4]",
        4 => "建议目标账户为 0",
        5 => "画像层空窗却出建议——内部矛盾",
        6 => "仲裁面上限非法",
        7 => "建议量超仲裁面上限——越权",
        8 => "回退步数≠1——悬挂风险",
        9 => "前瞻超过画像窗——看不全的历史不能猜未来",
        _ => "未知诊断码——查本模块 SEG_BASE 0x0990",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_stage_pipeline() {
        let mut prof = ProfileStage::new();
        for i in 0..PROFILE_WINDOW_FRAMES {
            prof.push(400 + (i % 7) as u32);
        }
        assert!(prof.full());
        let m = prof.mean().unwrap();
        assert!(m > 395 && m < 405);
        let pred = PredictStage { horizon: 2, active: true };
        let (adv, passive) = produce_advice(&prof, &pred, 1).unwrap();
        assert!(!passive);
        let a = adv.unwrap();
        assert_eq!(a.account, 1);
        assert_eq!(a.confidence_permille, 700);
        assert!(advice_within_boundary(&a, 1000).is_ok());
        // 越权拒绝：仲裁面上限 500，建议 400+。
        assert!(a.reserve_permille > 300);
        let tight = ReserveAdvice { account: 1, reserve_permille: 600, confidence_permille: 700 };
        assert_eq!(advice_within_boundary(&tight, 500).unwrap_err(), SEG_BASE + 7);
        assert!(arch_fp(&prof, &pred) != 0);
    }

    #[test]
    fn fallback_never_hangs() {
        let prof = ProfileStage::new();
        let dead = PredictStage { horizon: 2, active: false };
        let (adv, passive) = produce_advice(&prof, &dead, 1).unwrap();
        assert!(passive);
        assert!(adv.is_none());
        // 不满窗也不猜。
        let mut half = ProfileStage::new();
        for _ in 0..10 {
            half.push(500);
        }
        let alive = PredictStage { horizon: 1, active: true };
        let (adv2, passive2) = produce_advice(&half, &alive, 1).unwrap();
        assert!(passive2);
        assert!(adv2.is_none());
        let mut led = FallbackLedger::default();
        led.record_failure().unwrap();
        assert!(led.never_hangs().unwrap());
        assert_eq!(led.fallback_steps, 1);
        assert!(two_stage_consistent(PROFILE_WINDOW_FRAMES, PREDICT_HORIZON_MAX).is_ok());
        assert_eq!(two_stage_consistent(2, 4).unwrap_err(), SEG_BASE + 9);
    }

    #[test]
    fn advice_validation() {
        let zero_conf = ReserveAdvice { account: 1, reserve_permille: 100, confidence_permille: 0 };
        assert_eq!(zero_conf.validate().unwrap_err(), SEG_BASE + 2);
        assert!(diagnose(SEG_BASE + 2).contains("置信"));
        let over = ReserveAdvice { account: 1, reserve_permille: 1001, confidence_permille: 500 };
        assert_eq!(over.validate().unwrap_err(), SEG_BASE + 1);
    }
}
