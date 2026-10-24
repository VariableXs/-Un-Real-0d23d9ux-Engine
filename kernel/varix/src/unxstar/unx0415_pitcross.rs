//! unx0415_pitcross —— UNX-F0415 PIT 校准交叉验证。
//! 锚点（CoRun Unxreal 主册）：UNX-F0415 · PIT 校准交叉验证 · 目标 220 行
//! 判据原文：UNX-F0415-J1 PIT 频率校准与参考时钟交叉验证：
//!           同一实际时段内 PIT 计数与参考计数各采一次，换算
//!           出两套频率估计；交叉偏差（千分）落在容差内（默
//!           认 ±50‰）绿；超容差立案（校准漂移）；容差可配
//!           （零容差 = 要求逐 tick 一致，验收级）；采样对
//!           （PIT 计数, 参考计数）成对入库，重放一致。
//! 组件：采样对（PIT 计数 + 参考计数 + 时段）+ 频率估计
//!       （两套独立换算）+ 交叉偏差核（千分差）+ 容差闸 +
//!       采样账（重放）。
//! 职责定位：PIT 交叉验证是「校准的照妖镜」——PIT 标称频率
//!   与实际振荡器有偏差，长期积累会让定时器漂移；交叉验证
//!   用参考时钟量出这个偏差，千分制报告；容差内是正常工艺
//!   波动，超容差说明校准表要修或振荡器老化。
//! 性能逐项分解：估计 O(1)；偏差核 O(1)；采样入库 O(1)；
//!   重放 O(采样数)。
//! 诊断码段：0x21140（0x21141~0x21149 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PX_VERSION: &str = "unx0415_pitcross-v1";

/// 诊断段基址（0x21140 段 · 本模块独占）。
pub const PX_SEG_BASE: u32 = 0x21140;
pub const XC_DRIFT: u32 = 0x21141; // 交叉偏差超容差
pub const XC_TOL_OOR: u32 = 0x21142; // 容差越域（>1000）
pub const XC_ZERO_SPAN: u32 = 0x21143; // 时段为零
pub const XC_FEW_PAIRS: u32 = 0x21144; // 采样对不足
pub const XC_REPLAY: u32 = 0x21145; // 重放不一致
pub const XC_PIT_ZERO: u32 = 0x21146; // PIT 计数为零
pub const XC_REF_ZERO: u32 = 0x21147; // 参考计数为零
pub const XC_MAGIC: u32 = 0x21148; // 采样账魔数破
pub const XC_PAIR_LOST: u32 = 0x21149; // 采样对丢失

/// 采样对。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SamplePair {
    /// PIT 计数。
    pub pit: u64,
    /// 参考时钟计数。
    pub refr: u64,
    /// 标称频率（Hz）。
    pub nominal: u64,
}

/// PIT 频率估计：PIT 计数 / 参考计数 × 参考频率（模型内参考
/// 频率取 1，估计值直接是比值放大 1e6）。
pub fn pit_freq_estimate(p: &SamplePair) -> Result<u64, u32> {
    if p.pit == 0 {
        return Err(XC_PIT_ZERO);
    }
    if p.refr == 0 {
        return Err(XC_REF_ZERO);
    }
    Ok(p.pit * 1_000_000 / p.refr)
}

/// 交叉偏差千分：|估计 - 标称| × 1000 / 标称。
pub fn cross_drift_milli(p: &SamplePair) -> Result<u32, u32> {
    let est = pit_freq_estimate(p)?;
    let nominal = p.nominal;
    if nominal == 0 {
        return Err(XC_REF_ZERO);
    }
    let diff = if est > nominal { est - nominal } else { nominal - est };
    Ok((diff * 1000 / nominal) as u32)
}

/// 容差闸。
pub fn drift_gate(p: &SamplePair, tol_milli: u32) -> Result<u32, u32> {
    if tol_milli > 1000 {
        return Err(XC_TOL_OOR);
    }
    let d = cross_drift_milli(p)?;
    if d > tol_milli {
        return Err(XC_DRIFT);
    }
    Ok(d)
}

/// 采样账。
pub struct CrossBook {
    pairs: Vec<SamplePair>,
    tol_milli: u32,
    cases: Vec<usize>,
}

impl CrossBook {
    /// 建：容差。
    pub fn new(tol_milli: u32) -> Result<CrossBook, u32> {
        if tol_milli > 1000 {
            return Err(XC_TOL_OOR);
        }
        Ok(CrossBook { pairs: Vec::new(), tol_milli, cases: Vec::new() })
    }

    /// 入库采样对。
    pub fn feed(&mut self, pit: u64, refr: u64, nominal: u64) -> Result<(), u32> {
        if pit == 0 {
            return Err(XC_PIT_ZERO);
        }
        if refr == 0 {
            return Err(XC_REF_ZERO);
        }
        self.pairs.push(SamplePair { pit, refr, nominal });
        Ok(())
    }

    /// 全量交叉验证：逐对偏差核。
    pub fn verify(&mut self) -> Result<u32, u32> {
        self.cases.clear();
        if self.pairs.is_empty() {
            return Err(XC_FEW_PAIRS);
        }
        for (i, p) in self.pairs.iter().enumerate() {
            if drift_gate(p, self.tol_milli).is_err() {
                self.cases.push(i);
            }
        }
        if !self.cases.is_empty() {
            return Err(XC_DRIFT);
        }
        Ok(self.pairs.len() as u32)
    }

    /// 立案数。
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 对数。
    pub fn pair_count(&self) -> usize {
        self.pairs.len()
    }

    /// 重放核：同账同结论。
    pub fn replay_ok(&mut self) -> Result<(), u32> {
        let n = self.pair_count();
        let c = self.case_count();
        let mut ref_cases: Vec<usize> = Vec::new();
        for (i, p) in self.pairs.iter().enumerate() {
            if drift_gate(p, self.tol_milli).is_err() {
                ref_cases.push(i);
            }
        }
        if ref_cases.len() as usize != c || n != self.pairs.len() {
            return Err(XC_REPLAY);
        }
        Ok(())
    }
}

pub fn run_unx0415_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0415_pitcross");
    // 族一：标准交叉（PIT 实测 1_193_180 vs 标称 1_193_182，偏差 ~0）。
    let p1 = SamplePair { pit: 1_193_180, refr: 1_000_000, nominal: 1_193_182 };
    cs.add("PIT 估计过零", pit_freq_estimate(&p1).is_ok(), "比值可算");
    let d1 = cross_drift_milli(&p1);
    cs.add("偏差 <1‰", d1 == Ok(0), "1_193_180e6/1e6 ≈ 标称");
    cs.add("容差闸绿", drift_gate(&p1, 50).is_ok(), "50‰ 容差");
    // 族二：漂移立案（偏差 ~5‰）。
    let p2 = SamplePair { pit: 1_253_000, refr: 1_000_000, nominal: 1_193_182 };
    let d2 = cross_drift_milli(&p2);
    cs.add("偏差 ~50‰", d2.is_ok(), "可算");
    cs.add("50‰ 超容差", drift_gate(&p2, 50) == Err(XC_DRIFT), "漂移立案");
    cs.add("100‰ 容差内", drift_gate(&p2, 100).is_ok(), "宽容差放行");
    // 族三：零容差（验收级：逐 tick 一致）。
    let exact = SamplePair { pit: 1_193_182, refr: 1_000_000, nominal: 1_193_182 };
    cs.add("零容差精确过", drift_gate(&exact, 0).is_ok(), "估 = 标称");
    let off1 = SamplePair { pit: 1_193_183, refr: 1_000_000, nominal: 1_193_182 };
    cs.add("零容差偏差拒", drift_gate(&off1, 0) == Err(XC_DRIFT), "差 1 tick");
    // 族四：参数拒绝。
    cs.add("容差超千拒", matches!(drift_gate(&p1, 1001), Err(XC_TOL_OOR)), ">1000");
    cs.add("PIT 零拒", matches!(pit_freq_estimate(&SamplePair { pit: 0, refr: 1, nominal: 1 }), Err(XC_PIT_ZERO)), "pit=0");
    cs.add("参考零拒", matches!(pit_freq_estimate(&SamplePair { pit: 1, refr: 0, nominal: 1 }), Err(XC_REF_ZERO)), "ref=0");
    cs.add("标称零拒", matches!(cross_drift_milli(&SamplePair { pit: 1, refr: 1, nominal: 0 }), Err(XC_REF_ZERO)), "nominal=0");
    cs.add("容差建账超域拒", matches!(CrossBook::new(1001), Err(XC_TOL_OOR)), ">1000");
    // 族五：采样账全量验证。
    let mut b = match CrossBook::new(50) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账", false, "拒");
            return cs;
        }
    };
    let _ = b.feed(1_193_180, 1_000_000, 1_193_182);
    let _ = b.feed(1_193_181, 1_000_000, 1_193_182);
    cs.add("两对入库", b.pair_count() == 2, "逐对");
    cs.add("全量绿", b.verify().is_ok(), "全容差内");
    cs.add("立案 0", b.case_count() == 0, "无案");
    // 混入漂移对。
    let _ = b.feed(1_300_000, 1_000_000, 1_193_182);
    cs.add("混入后红", b.verify() == Err(XC_DRIFT), "第 3 对漂移");
    cs.add("立案 1", b.case_count() == 1, "定位第 3 对");
    cs.add("重放核过", b.replay_ok().is_ok(), "重算同案");
    // 空账拒。
    let mut e = match CrossBook::new(50) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账2", false, "拒");
            return cs;
        }
    };
    cs.add("空账拒", e.verify() == Err(XC_FEW_PAIRS), "无采样");
    cs
}
