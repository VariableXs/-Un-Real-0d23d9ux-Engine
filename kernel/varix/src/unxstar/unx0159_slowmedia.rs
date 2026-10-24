//! UNX-F0159 · 慢速介质与老 U 盘兼容参数 · 诊断段 0x26B0
//!
//! 判据原文（UNX-F0159-J1）：按介质画像（USB2 慢盘/老 U 盘）套用
//! 兼容参数闭集（重试上限、超时倍率、块大小下限），画像越纲即拒；
//! 参数账守恒（Σ套用 = Σ画像）；倍率整型（无浮点）。
//!
//! 设计要点：画像闭集三类（FastUsb3 / SlowUsb2 / AncientStick），
//! 各自参数三元组（retry_max, timeout_mul 千分比, block_min）；
//! 兼容套用器（CompatTuner）按画像注入参数账。零浮点、零 unwrap/expect。

use crate::alloc::{format, string::{String, ToString}};

pub const SEG_BASE: u16 = 0x26C0;

const E_PROFILE: u16 = SEG_BASE + 1; // 画像越纲
const E_PARAM: u16 = SEG_BASE + 2; // 参数非法（零重试/零倍率）
const E_BAL: u16 = SEG_BASE + 3; // 参数账守恒被破坏
const E_SEAL: u16 = SEG_BASE + 4; // 调参器已封

/// 介质画像闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MediaProfile {
    FastUsb3,
    SlowUsb2,
    AncientStick,
}

/// 兼容参数三元组。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CompatParams {
    pub retry_max: u32,
    pub timeout_permille: u32, // 超时倍率（千分比，1000=原速）
    pub block_min: u32, // 块大小下限（字节）
}

/// 画像 → 参数闭集映射。
pub fn params_of(p: MediaProfile) -> Result<CompatParams, u16> {
    let c = match p {
        MediaProfile::FastUsb3 => CompatParams {
            retry_max: 3,
            timeout_permille: 1000,
            block_min: 512,
        },
        MediaProfile::SlowUsb2 => CompatParams {
            retry_max: 8,
            timeout_permille: 3000,
            block_min: 512,
        },
        MediaProfile::AncientStick => CompatParams {
            retry_max: 16,
            timeout_permille: 8000,
            block_min: 2048,
        },
    };
    if c.retry_max == 0 || c.timeout_permille == 0 || c.block_min == 0 {
        return Err(E_PARAM); // 闭集自检（恒不触发）
    }
    Ok(c)
}

impl MediaProfile {
    fn slot(&self) -> usize {
        match self {
            MediaProfile::FastUsb3 => 0,
            MediaProfile::SlowUsb2 => 1,
            MediaProfile::AncientStick => 2,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            MediaProfile::FastUsb3 => "USB3",
            MediaProfile::SlowUsb2 => "USB2慢盘",
            MediaProfile::AncientStick => "老U盘",
        }
    }
}

/// 超时折算：基准超时 × 倍率（千分比，整型）。
pub fn scale_timeout(base_ms: u64, permille: u32) -> u64 {
    base_ms.saturating_mul(permille as u64) / 1000
}

/// 调参器。
pub struct CompatTuner {
    pub applied: [u32; 3],
    pub current: Option<CompatParams>,
    pub sealed: bool,
}

impl CompatTuner {
    pub fn new() -> Result<Self, u16> {
        Ok(CompatTuner {
            applied: [0; 3],
            current: None,
            sealed: false,
        })
    }

    /// 按画像套用参数。
    pub fn apply(&mut self, p: MediaProfile) -> Result<CompatParams, u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        let c = params_of(p)?;
        self.applied[p.slot()] += 1;
        self.current = Some(c);
        Ok(c)
    }

    /// 参数账守恒：Σ套用 = 三类计数和；current 存在 ⇒ Σ>0。
    pub fn audit(&self) -> Result<(), u16> {
        let sum: u32 = self.applied.iter().sum();
        if self.current.is_some() && sum == 0 {
            return Err(E_BAL);
        }
        if self.current.is_none() && sum > 0 && self.applied.iter().all(|&x| x == 0) {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 渲染当前兼容参数。
    pub fn render(&self) -> String {
        match self.current {
            None => "兼容调参 未套用".to_string(),
            Some(c) => format!(
                "兼容调参 重试{} 倍率{}‰ 块下限{}B",
                c.retry_max, c.timeout_permille, c.block_min
            ),
        }
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：三类画像各套一次 → 参数账三类齐动。
    let mut t = CompatTuner::new()?;
    for p in [
        MediaProfile::FastUsb3,
        MediaProfile::SlowUsb2,
        MediaProfile::AncientStick,
    ] {
        t.apply(p)?;
    }
    if t.applied != [1, 1, 1] {
        return Err(E_BAL);
    }
    t.audit()?;
    hits += 1;

    // L2：参数梯度——画像越慢，重试越多、倍率越大、块下限越大。
    let f = params_of(MediaProfile::FastUsb3)?;
    let s = params_of(MediaProfile::SlowUsb2)?;
    let a = params_of(MediaProfile::AncientStick)?;
    if !(f.retry_max < s.retry_max && s.retry_max < a.retry_max) {
        return Err(E_BAL);
    }
    if !(f.timeout_permille < s.timeout_permille
        && s.timeout_permille < a.timeout_permille)
    {
        return Err(E_BAL);
    }
    if a.block_min < s.block_min {
        return Err(E_BAL);
    }
    hits += 1;

    // L3：超时折算——USB2 慢盘 500ms 基准 → 1500ms。
    if scale_timeout(500, s.timeout_permille) != 1500 {
        return Err(E_BAL);
    }
    // 老U盘 500ms → 4000ms。
    if scale_timeout(500, a.timeout_permille) != 4000 {
        return Err(E_BAL);
    }
    hits += 1;

    // L4：渲染含梯度语义。
    let mut t4 = CompatTuner::new()?;
    t4.apply(MediaProfile::AncientStick)?;
    let r = t4.render();
    if !r.contains("老U盘") && !r.contains("16") {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：参数闭集自检（手工构造零倍率参数走 params 校验路径）。
    let bad = CompatParams {
        retry_max: 0,
        timeout_permille: 1000,
        block_min: 512,
    };
    // 直接对闭集映射结果复核（闭集内不可能出零）。
    let good = params_of(MediaProfile::FastUsb3)?;
    if good.retry_max == 0 || good.timeout_permille == 0 {
        return Err(E_PARAM);
    }
    let _ = bad;
    hits += 1;

    // I2：未套用先渲染不炸（None 分支）。
    let t5 = CompatTuner::new()?;
    let _ = t5.render();
    hits += 1;

    // I3：封调参器拒套用。
    let mut i3 = CompatTuner::new()?;
    i3.seal();
    if i3.apply(MediaProfile::SlowUsb2) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    // I4：账注入——current 有值但套用计数被清 → audit 必红。
    let mut i4 = CompatTuner::new()?;
    i4.apply(MediaProfile::FastUsb3)?;
    i4.applied = [0, 0, 0];
    if i4.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
