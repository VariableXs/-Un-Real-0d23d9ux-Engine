//! CGPU-F2083 · 效果重建原则（CGPU-N 域 · 原生效果重建 · 批次 N03）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2083`
//!
//! 锚点原文：「重建原则：原则（重建三原则（等价优先/性能保底/降级诚实——原则
//! 声明；等价（像素级等价 vs 感知等价分级——等价分级声明（分级验收；测试
//! （原则/分级两组）。判据：三原则、分级声明、两组、判据。」
//!
//! # 一、三原则是**秩序**不是并列：等价优先排第一
//!
//! [`PRINCIPLES`] 三条按秩排列：等价优先（像素/时序不达标就不配叫重建）、
//! 性能保底（等价不豁免延迟口径）、降级诚实（降级必须自我声明）。次序即仲裁
//! 依据——原则冲突时秩位高者胜，「等价优先」因此是第 0 条不是装饰。
//!
//! # 二、分级验收是**双闸**：声明与验收必须同级
//!
//! [`EquivalenceTier`] 两级（像素级/感知级）各有量化口径：像素级 SSIM≥980‰
//! 且延迟≤2ms；感知级 SSIM≥950‰ 且延迟≤5ms。[`verify`] 按级验收，
//! [`claim_honest`] 把「降级诚实」落成闸——**声明分级与验收分级失配即拒**
//! （拿感知级冒充像素级是撒谎，不是降级）。

use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、诊断码（vcn03 续 0x9C 段，细分 0x9C2x）
// ---------------------------------------------------------------------------

/// 像素级 SSIM 不达（低于 980‰——像素级等价不成立）。
pub const E_N083_PIXEL_SSIM: u16 = 0x9C20;
/// 像素级延迟不达（超出 2ms——像素级时序不成立）。
pub const E_N083_PIXEL_LATENCY: u16 = 0x9C21;
/// 感知级验收不达（SSIM 或延迟低于该级口径）。
pub const E_N083_PERCEPTUAL_SHORT: u16 = 0x9C22;
/// 声明失配（拿感知级冒充像素级——降级不诚实）。
pub const E_N083_CLAIM_MISMATCH: u16 = 0x9C23;

/// 域批次版本。
pub const VCN03_VERSION: &str = "CN03-principles-v1";

// ---------------------------------------------------------------------------
// 二、重建三原则（封闭表，次序即秩）
// ---------------------------------------------------------------------------

/// 重建三原则（封闭——锚点「重建三原则」；次序即仲裁秩，等价优先排第一）。
pub const PRINCIPLES: [&str; 3] = ["等价优先", "性能保底", "降级诚实"];

/// 原则声明原文（与 PRINCIPLES 平行——每原则一句可 grep 的声明）。
pub const PRINCIPLE_DOCS: [&str; 3] = [
    "像素/时序不达标就不配叫重建——等价压倒一切",
    "等价不豁免延迟口径——重建不得比原生慢出预算",
    "降级必须自我声明——拿感知级冒充像素级是撒谎",
];

// ---------------------------------------------------------------------------
// 三、等价分级（两级封闭 + 量化口径）
// ---------------------------------------------------------------------------

/// 等价分级（封闭两级——锚点「像素级等价 vs 感知等价分级」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquivalenceTier {
    /// 像素级等价（与原生逐像素/逐帧时序等价）。
    PixelExact,
    /// 感知级等价（视觉不可分辨但允许有限偏差——必须自我声明）。
    Perceptual,
}

impl EquivalenceTier {
    /// 全枚举（顺序即下标）。
    pub const ALL: [EquivalenceTier; 2] =
        [EquivalenceTier::PixelExact, EquivalenceTier::Perceptual];

    /// 级下标。
    pub const fn ordinal(self) -> usize {
        match self {
            EquivalenceTier::PixelExact => 0,
            EquivalenceTier::Perceptual => 1,
        }
    }

    /// 下标 → 级（越界 None）。
    pub const fn of_ordinal(i: usize) -> Option<EquivalenceTier> {
        match i {
            0 => Some(EquivalenceTier::PixelExact),
            1 => Some(EquivalenceTier::Perceptual),
            _ => None,
        }
    }
}

/// SSIM 千分制（0..=1000，981‰ = 0.981）。
pub const SSIM_SCALE: u32 = 1000;
/// 像素级 SSIM 下限（980‰ = 0.980）。
pub const PIXEL_SSIM_FLOOR: u32 = 980;
/// 感知级 SSIM 下限（950‰ = 0.950）。
pub const PERCEPTUAL_SSIM_FLOOR: u32 = 950;
/// 像素级延迟上限（ms）。
pub const PIXEL_LATENCY_CAP_MS: u32 = 2;
/// 感知级延迟上限（ms）。
pub const PERCEPTUAL_LATENCY_CAP_MS: u32 = 5;

/// 分级验收结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Acceptance {
    /// 像素级验收通过。
    PixelVerified,
    /// 感知级验收通过。
    PerceptualVerified,
}

/// **verify**（分级验收）：按声明级口径判定 SSIM 与延迟。
///
/// - 像素级：SSIM≥980‰ 且延迟≤2ms → [`Acceptance::PixelVerified`]；
///   SSIM 不达 → [`E_N083_PIXEL_SSIM`]；延迟不达 → [`E_N083_PIXEL_LATENCY`]。
/// - 感知级：SSIM≥950‰ 且延迟≤5ms → [`Acceptance::PerceptualVerified`]；
///   任一不达 → [`E_N083_PERCEPTUAL_SHORT`]。
pub const fn verify(
    tier: EquivalenceTier,
    ssim_permille: u32,
    latency_ms: u32,
) -> Result<Acceptance, u16> {
    match tier {
        EquivalenceTier::PixelExact => {
            if ssim_permille < PIXEL_SSIM_FLOOR {
                return Err(E_N083_PIXEL_SSIM);
            }
            if latency_ms > PIXEL_LATENCY_CAP_MS {
                return Err(E_N083_PIXEL_LATENCY);
            }
            Ok(Acceptance::PixelVerified)
        }
        EquivalenceTier::Perceptual => {
            if ssim_permille < PERCEPTUAL_SSIM_FLOOR {
                return Err(E_N083_PERCEPTUAL_SHORT);
            }
            if latency_ms > PERCEPTUAL_LATENCY_CAP_MS {
                return Err(E_N083_PERCEPTUAL_SHORT);
            }
            Ok(Acceptance::PerceptualVerified)
        }
    }
}

/// **claim_honest**（降级诚实闸）：声明分级与验收分级必须同 level——
/// 拿感知级成绩冒充像素级 = 撒谎，拒 [`E_N083_CLAIM_MISMATCH`]。
pub const fn claim_honest(
    claimed: EquivalenceTier,
    verified: Acceptance,
) -> Result<(), u16> {
    let claimed_is_pixel = matches!(claimed, EquivalenceTier::PixelExact);
    let verified_is_pixel = matches!(verified, Acceptance::PixelVerified);
    if claimed_is_pixel != verified_is_pixel {
        return Err(E_N083_CLAIM_MISMATCH);
    }
    Ok(())
}

/// 分级口径自检（等价优先的可测面——像素级阈值严于感知级）。
pub const fn tiers_ordered() -> bool {
    PIXEL_SSIM_FLOOR > PERCEPTUAL_SSIM_FLOOR
        && PIXEL_LATENCY_CAP_MS < PERCEPTUAL_LATENCY_CAP_MS
}

/// 摘要行（面板/日志共用）。
pub fn screen_line() -> String {
    let mut s = String::new();
    s.push_str(VCN03_VERSION);
    s.push_str(" principles=3 tiers=2 pixel=980/2ms perceptual=950/5ms");
    s
}

// ---------------------------------------------------------------------------
// 编译期闸
// ---------------------------------------------------------------------------

const _: () = {
    assert!(PRINCIPLES.len() == 3 && PRINCIPLE_DOCS.len() == 3);
    assert!(EquivalenceTier::ALL.len() == 2);
    assert!(SSIM_SCALE == 1000);
    assert!(PIXEL_SSIM_FLOOR == 980 && PERCEPTUAL_SSIM_FLOOR == 950);
    assert!(PIXEL_LATENCY_CAP_MS == 2 && PERCEPTUAL_LATENCY_CAP_MS == 5);
    assert!(PIXEL_SSIM_FLOOR < SSIM_SCALE && PERCEPTUAL_SSIM_FLOOR < SSIM_SCALE);
    assert!(E_N083_PIXEL_SSIM & 0xFF00 == 0x9C00);
    assert!(E_N083_PIXEL_LATENCY & 0xFF00 == 0x9C00);
    assert!(E_N083_PERCEPTUAL_SHORT & 0xFF00 == 0x9C00);
    assert!(E_N083_CLAIM_MISMATCH & 0xFF00 == 0x9C00);
    assert!(
        E_N083_PIXEL_SSIM != E_N083_PIXEL_LATENCY
            && E_N083_PIXEL_LATENCY != E_N083_PERCEPTUAL_SHORT
            && E_N083_PERCEPTUAL_SHORT != E_N083_CLAIM_MISMATCH
    );
};
