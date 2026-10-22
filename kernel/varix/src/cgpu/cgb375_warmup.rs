//! CGPU-F0375 · 冷启动与热身（判据段 0x0A00）
//! 冷启动：无历史默认（新账户首次出现——类别默认画像）；快速热
//! 身（前 N 帧高频采样加速收敛）；冷启动保护（热身期预算从宽+
//! 显性标注低置信）；画像冷热分层（热数据内存/冷数据压缩持久化
//! ——对接 B07）。判据：默认画像、热身收敛、保护、冷热分层、判据。
//!
//! 锚点原文摘录（施工书 1665 行）：
//! 「无历史默认（新账户首次出现——类别默认画像）；快速热身（前
//! N 帧高频采样加速收敛）；冷启动保护（热身期预算从宽+显性标注
//! 低置信）；画像冷热分层（热数据内存/冷数据压缩持久化）。判据：
//! 默认画像、热身收敛、保护、冷热分层、判据。」
//!
//! 诊断码段 0x0A00（本模块独占：0x0A00~0x0A0F）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0A00;

/// 热身期（帧）：前 16 帧为热身期（高频采样+从宽预算）。
pub const WARMUP_FRAMES: u32 = 16;

/// 热身采样倍率：热身期每帧记 2 个样本（高频采样加速收敛）。
pub const WARMUP_SAMPLE_RATE: u32 = 2;

/// 冷启动预算放宽系数（千分比）：热身期从宽 ×1200（12 折放宽）。
pub const WARMUP_BUDGET_BOOST_PERMILLE: u32 = 1200;

/// 类别默认画像（判据「默认画像」的三类账户——无历史时的起点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccountClass {
    /// 游戏类（默认重负载画像）。
    Game,
    /// 视频类（默认稳态画像）。
    Video,
    /// 工具类（默认轻负载画像）。
    Utility,
}

impl AccountClass {
    /// 类号。
    pub fn ordinal(&self) -> u8 {
        match self {
            AccountClass::Game => 0,
            AccountClass::Video => 1,
            AccountClass::Utility => 2,
        }
    }
    /// 类别默认画像值（千分比）——无历史时不猜 0 也不猜满，给类
    /// 型先验（来源：三类典型负载中位）。
    pub fn default_profile(&self) -> u32 {
        match self {
            AccountClass::Game => 700,
            AccountClass::Video => 400,
            AccountClass::Utility => 150,
        }
    }
    /// 类别默认置信（千分比）——冷启动显性低置信（判据「显性标注
    /// 低置信」）：先验不是测量，压到 300。
    pub fn default_confidence(&self) -> u32 {
        let _ = self;
        300
    }
}

/// 冷启动账户（判据「默认画像 + 热身收敛」一等公民）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColdAccount {
    /// 账户号。
    pub account: u32,
    /// 类别（默认画像来源）。
    pub class: AccountClass,
    /// 已采样帧数。
    pub frames_sampled: u32,
    /// 累计样本（热身倍率放大后的）。
    pub samples: u32,
    /// 收敛 EWMA。
    pub ewma: u32,
}

impl ColdAccount {
    /// 建档（判据「默认画像」主判据）：EWMA 起点=类别先验。
    pub fn enroll(account: u32, class: AccountClass) -> Result<Self, u16> {
        if account == 0 {
            return Err(SEG_BASE);
        }
        Ok(ColdAccount {
            account,
            class,
            frames_sampled: 0,
            samples: 0,
            ewma: class.default_profile(),
        })
    }
    /// 热身期判定。
    pub fn warming_up(&self) -> bool {
        self.frames_sampled < WARMUP_FRAMES
    }
    /// 喂一帧（判据「热身收敛」主判据）：热身期每帧记 WARMUP_SAMPLE_RATE
    /// 个样本（高频采样），EWMA 快速收敛。
    pub fn feed(&mut self, v: u32) -> Result<(), u16> {
        let times = if self.warming_up() { WARMUP_SAMPLE_RATE } else { 1 };
        for _ in 0..times {
            self.ewma = ((375u64 * v as u64 + 625 * self.ewma as u64) / 1000) as u32;
            self.samples += 1;
        }
        self.frames_sampled += 1;
        Ok(())
    }
    /// 收敛核（判据「热身收敛」验收面）：热身期结束且累计样本 ≥
    /// WARMUP_FRAMES×RATE → 视为收敛可出正常置信。
    pub fn converged(&self) -> bool {
        !self.warming_up() && self.samples >= WARMUP_FRAMES * WARMUP_SAMPLE_RATE
    }
}

/// 冷启动保护预算（判据「保护」主判据）：热身期预算从宽——基础
/// 预算 × WARMUP_BUDGET_BOOST_PERMILLE（封顶 1000），且显性带低置
/// 信旗——放宽与低置信成对出现，缺一即判据违规。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WarmupGrant {
    /// 放宽后预算（千分比）。
    pub boosted_budget: u32,
    /// 显性低置信旗（恒 true——热身期永不假装自信）。
    pub low_confidence: bool,
}

impl WarmupGrant {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.boosted_budget == 0 || self.boosted_budget > 1000 {
            return Err(SEG_BASE + 1);
        }
        if !self.low_confidence {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
}

/// 热身保护授予（判据「保护」主判据）：base×1200 封顶 1000 + 低置
/// 信旗恒真。
pub fn warmup_grant(base_permille: u32) -> Result<WarmupGrant, u16> {
    if base_permille == 0 || base_permille > 1000 {
        return Err(SEG_BASE + 3);
    }
    let boosted = ((base_permille as u64 * WARMUP_BUDGET_BOOST_PERMILLE as u64) / 1000).min(1000) as u32;
    let g = WarmupGrant { boosted_budget: boosted, low_confidence: true };
    g.validate()?;
    Ok(g)
}

/// 画像冷热分层（判据「冷热分层」一等公民）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 热层：内存驻留（近期活跃）。
    Hot,
    /// 冷层：压缩持久化（对接 B07——语义建模，零真实 IO）。
    Cold,
}

impl Tier {
    /// 层号。
    pub fn ordinal(&self) -> u8 {
        match self {
            Tier::Hot => 0,
            Tier::Cold => 1,
        }
    }
}

/// 分层画像行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TierRow {
    /// 账户号。
    pub account: u32,
    /// 层。
    pub tier: Tier,
    /// 画像值（热层=EWMA；冷层=压缩锚——Δ量化值）。
    pub value: u32,
}

impl TierRow {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.account == 0 {
            return Err(SEG_BASE + 4);
        }
        Ok(())
    }
    /// 压缩锚（判据「冷热分层」量化面）：冷层值 = 热值 / 16 × 16
    /// （16 步长量化——压缩语义的最小实现，无损还原上界 15）。
    pub fn quantize_down(hot_value: u32) -> u32 {
        hot_value / 16 * 16
    }
}

/// 分层账（判据「冷热分层」主判据）：降冷（热→冷）与回热（冷→
/// 热）双通道；同账户同层唯一（重复降冷拒绝）；降冷必须量化。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TierLedger {
    /// 分层行。
    pub rows: Vec<TierRow>,
}

impl TierLedger {
    /// 空。
    pub fn new() -> Self {
        TierLedger { rows: Vec::new() }
    }
    /// 降冷（判据「冷热分层」主判据）：热值量化后入冷层；已在冷
    /// 层的重复降冷拒绝。
    pub fn freeze(&mut self, account: u32, hot_value: u32) -> Result<u32, u16> {
        if account == 0 {
            return Err(SEG_BASE + 5);
        }
        if self.rows.iter().any(|r| r.account == account && r.tier == Tier::Cold) {
            return Err(SEG_BASE + 6);
        }
        let v = TierRow::quantize_down(hot_value);
        self.rows.push(TierRow { account, tier: Tier::Cold, value: v });
        Ok(v)
    }
    /// 回热（判据「冷热分层」恢复通道）：冷层行升级热层；无冷行
    /// 拒绝；量化损耗显性（还原值 ≤ 冷锚 + 15）。
    pub fn thaw(&mut self, account: u32) -> Result<u32, u16> {
        let r = self
            .rows
            .iter_mut()
            .find(|r| r.account == account && r.tier == Tier::Cold)
            .ok_or(SEG_BASE + 7)?;
        r.tier = Tier::Hot;
        Ok(r.value + 15)
    }
    /// 全行合法核。
    pub fn audit(&self) -> Result<(), u16> {
        for r in &self.rows {
            r.validate()?;
        }
        Ok(())
    }
}

impl Default for TierLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 冷启动诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "账户号为 0",
        1 => "放宽预算必须 ∈ (0,1000]",
        2 => "热身期低置信旗必须为真——放宽与低置信成对",
        3 => "基础预算必须 ∈ (0,1000]",
        4 => "分层行账户为 0",
        5 => "降冷账户为 0",
        6 => "已在冷层——重复降冷",
        7 => "无冷行可回热",
        _ => "未知诊断码——查本模块 SEG_BASE 0x0A00",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_and_warmup_converge() {
        let mut a = ColdAccount::enroll(1, AccountClass::Game).unwrap();
        assert_eq!(a.ewma, AccountClass::Game.default_profile());
        assert_eq!(AccountClass::Game.default_confidence(), 300);
        assert!(a.warming_up());
        // 热身期高频采样：喂 16 帧得 32 样本。
        for _ in 0..WARMUP_FRAMES {
            a.feed(500).unwrap();
        }
        assert!(!a.warming_up());
        assert!(a.converged());
        assert_eq!(a.samples, WARMUP_FRAMES * WARMUP_SAMPLE_RATE);
        // EWMA 向 500 收敛（起点 700，α=0.375 高频翻倍）。
        assert!(a.ewma > 450 && a.ewma <= 500, "ewma={}", a.ewma);
        assert_eq!(ColdAccount::enroll(0, AccountClass::Video).unwrap_err(), SEG_BASE);
    }

    #[test]
    fn warmup_protection_pair() {
        let g = warmup_grant(600).unwrap();
        assert_eq!(g.boosted_budget, 720); // 600×1.2。
        assert!(g.low_confidence);
        assert!(g.validate().is_ok());
        // 封顶 1000。
        let g2 = warmup_grant(900).unwrap();
        assert_eq!(g2.boosted_budget, 1000);
        assert_eq!(warmup_grant(0).unwrap_err(), SEG_BASE + 3);
        // 旗缺失判据违规。
        let bad = WarmupGrant { boosted_budget: 500, low_confidence: false };
        assert_eq!(bad.validate().unwrap_err(), SEG_BASE + 2);
        assert!(diagnose(SEG_BASE + 2).contains("成对"));
    }

    #[test]
    fn tier_freeze_thaw() {
        let mut led = TierLedger::new();
        let v = led.freeze(1, 505).unwrap();
        assert_eq!(v, 496); // 505/16*16。
        // 重复降冷拒绝。
        assert_eq!(led.freeze(1, 505).unwrap_err(), SEG_BASE + 6);
        // 回热：还原上界 = 锚+15。
        let back = led.thaw(1).unwrap();
        assert_eq!(back, 511);
        // 回热后无冷行。
        assert_eq!(led.thaw(1).unwrap_err(), SEG_BASE + 7);
        assert!(led.audit().is_ok());
        assert_eq!(Tier::Hot.ordinal(), 0);
        assert_eq!(Tier::Cold.ordinal(), 1);
    }
}
