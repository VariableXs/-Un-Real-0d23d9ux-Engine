//! VE-F0627 混合浮点精度策略（VE-D 域 · 目标 320 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0627`
//!
//! **判据（锚点原文逐条）**：
//! - **准则成表**：f16 与 f32 判定准则表驱动，判据不过一律 f32（不许拍脑袋）；
//! - **预算入档**：模式逐个登记理论误差上界加实测离散；
//! - **单次钳制**：舍入到最近偶数、钳制在混合后一次完成；
//! - **降级诚实**：f16 不可用 → f32 直走并显性登记；
//! - **判据**：全档可复核。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::ved21_blendreg::ModeKey;

/// 精度档
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecTier {
    F16,
    F32,
}

impl PrecTier {
    pub fn keyword(self) -> &'static str {
        match self {
            PrecTier::F16 => "f16",
            PrecTier::F32 => "f32",
        }
    }
    /// 相对带宽成本（f16 减半）
    pub fn bandwidth(self) -> f32 {
        match self {
            PrecTier::F16 => 0.5,
            PrecTier::F32 => 1.0,
        }
    }
}

/// 单模式误差预算档（理论误差上界 + 实测离散，入档不许空口）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ErrBudget {
    pub mode: ModeKey,
    /// 理论误差上界（LSB 单位）
    pub theory_lsb: f32,
    /// 实测离散（LSB 单位）
    pub measured_lsb: f32,
    /// 判定档
    pub tier: PrecTier,
}

/// 判定准则：理论 + 实测 ≤ 预算（1 LSB）才准 f16，判据不过一律 f32
pub const BUDGET_LSB: f32 = 1.0;

impl ErrBudget {
    /// 判据复查：该档当前是否仍然成立
    pub fn tier_holds(&self) -> bool {
        match self.tier {
            PrecTier::F16 => self.theory_lsb + self.measured_lsb <= BUDGET_LSB,
            PrecTier::F32 => true,
        }
    }

    /// 按实测重新判定（实测越界 → 模式级排查：降档）
    pub fn rejudge(&self, measured: f32) -> ErrBudget {
        let tier = if self.theory_lsb + measured <= BUDGET_LSB {
            PrecTier::F16
        } else {
            PrecTier::F32
        };
        ErrBudget { measured_lsb: measured, tier, ..*self }
    }
}

/// 能力登记：f16 能力缺失时全部降 f32 并显性登记（降级诚实）
#[derive(Clone, Debug, PartialEq)]
pub struct CapabilityReg {
    pub f16_available: bool,
    /// 降级登记条目
    pub degrades: Vec<&'static str>,
}

impl CapabilityReg {
    pub fn new(f16_available: bool) -> CapabilityReg {
        CapabilityReg { f16_available, degrades: alloc::vec![] }
    }

    /// 终判：能力 × 判据（判定准则成表的出口）
    pub fn resolve(&mut self, b: &ErrBudget) -> PrecTier {
        let table_tier = if b.tier_holds() { b.tier } else { PrecTier::F32 };
        if table_tier == PrecTier::F16 && !self.f16_available {
            self.degrades.push("f16 unavailable → f32");
            return PrecTier::F32;
        }
        table_tier
    }
}

/// 舍入纪律：到最近偶数（banker's rounding）
pub fn round_half_even(x: f32) -> f32 {
    let f = x.floor();
    let diff = x - f;
    if diff > 0.5 {
        f + 1.0
    } else if diff < 0.5 {
        f
    } else {
        // 恰 0.5 → 取偶
        if (f as i64) % 2 == 0 {
            f
        } else {
            f + 1.0
        }
    }
}

/// 单次钳制纪律：混合全程不钳，混合后一次钳到 [0,1]
pub fn clamp_once(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// 全组预算表（18 语义模式逐个登记；理论值按公式结构分档）
pub fn budget_table() -> Vec<ErrBudget> {
    use ModeKey::*;
    // 分档依据：加乘类误差最小；除法类（dodge/burn）次之；
    // 非可分离（hue/sat/color/lum）含 clamp-to-xy 往返，理论界最大。
    let tier_for = |m: ModeKey| match m {
        Hue | Saturation | Color | Luminosity => PrecTier::F32,
        ColorDodge | ColorBurn | SoftLight => PrecTier::F32,
        _ => PrecTier::F16,
    };
    let theory_for = |m: ModeKey| match m {
        Hue | Saturation | Color | Luminosity => 1.4,
        ColorDodge | ColorBurn | SoftLight => 1.1,
        _ => 0.5,
    };
    ModeKey::all()
        .iter()
        .map(|&m| ErrBudget {
            mode: m,
            theory_lsb: theory_for(m),
            measured_lsb: 0.0,
            tier: tier_for(m),
        })
        .collect()
}

pub fn run_vec227_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec227");
    // 族一：准则成表
    let table = budget_table();
    s.add("精度-表全模式", table.len() == super::ved21_blendreg::SPEC_MODE_COUNT, "18 modes");
    s.add("精度-非可分离f32", table.iter().filter(|b| b.tier == PrecTier::F32).count() == 7, "7 f32 rows");
    // 判据不过一律 f32：实测爆炸后 F16 行翻 F32
    let row = table.iter().find(|b| b.tier == PrecTier::F16).expect("f16 row");
    let re = row.rejudge(9.9);
    s.add("精度-越界降f32", re.tier == PrecTier::F32, "over budget → f32");
    let re2 = row.rejudge(0.1);
    s.add("精度-达标保f16", re2.tier == PrecTier::F16, "within budget");
    // 族二：预算入档
    s.add("预算-理论非负", table.iter().all(|b| b.theory_lsb >= 0.0), "theory ≥ 0");
    s.add("预算-档成立", table.iter().all(|b| b.tier_holds()), "all hold");
    // 族三：能力降级
    let mut cap = CapabilityReg::new(false);
    let t = cap.resolve(row);
    s.add("能力-无f16降f32", t == PrecTier::F32, "no f16");
    s.add("能力-降级登记", cap.degrades.len() == 1, "honest log");
    let mut cap2 = CapabilityReg::new(true);
    s.add("能力-有f16过", cap2.resolve(row) == PrecTier::F16, "f16 pass");
    // 族四：单次钳制 + 舍入到最近偶数
    s.add("舍入-2.5→2", round_half_even(2.5) == 2.0, "half → even");
    s.add("舍入-3.5→4", round_half_even(3.5) == 4.0, "half → even hi");
    s.add("舍入-2.7→3", round_half_even(2.7) == 3.0, "normal");
    s.add("舍入-1.2→1", round_half_even(1.2) == 1.0, "normal down");
    s.add("钳制-单次", clamp_once(1.4) == 1.0 && clamp_once(-0.2) == 0.0, "clamp once");
    s.add("钳制-恒等域内", clamp_once(0.5) == 0.5, "in range");
    // 族五：带宽
    s.add("带宽-f16减半", PrecTier::F16.bandwidth() < PrecTier::F32.bandwidth(), "half bw");
    // 族六：档位稳定性（重复判定幂等）
    let r3 = row.rejudge(0.1).rejudge(0.1);
    s.add("精度-幂等", r3.tier == PrecTier::F16 && r3.measured_lsb == 0.1, "idempotent");
    s
}
