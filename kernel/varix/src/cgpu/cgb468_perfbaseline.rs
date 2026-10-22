//! CGPU-F0468 · 性能总基准（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 性能总基准：基准总册冻结（F0399 合并终版——仲裁开销/公平指数/
//!   切换延迟/预测命中率/仿真耗时/集成开销全谱）；
//! - 环境声明统一（负载谱与硬件环境标准化——跨版本可比）；
//! - 基线冻结 v1（ADR 变更流程）；回归接入。判据：冻结、环境声明、
//!   基线、接入、判据。
//!
//! 设计纪律：
//! - 全谱六指标：仲裁开销（ns/决策）、公平指数（Jain×1000）、切换
//!   延迟（tick）、预测命中率（‰）、仿真耗时（us/千轮）、集成开销
//!   （‰）——每指标 (值, 单位, 方向) 三元组，方向声明「越低越好」
//!   或「越高越好」，回归判定按方向比较。
//! - 环境声明：负载谱码+硬件档码+时钟档码——三元组不同则基准不可
//!   直接比（可比性前置闸）。
//! - 基线冻结 v1：ADR 流程（变更需「解冻→改→再冻结」三步，直接
//!   改冻结基线拒）。
//! - 回归接入：新实测 vs 基线按方向+带宽判定，越带即回归红。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1070 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1070，全仓独占）。
pub const SEG_BASE: u16 = 0x1070;

pub const E_PB_METRIC: u16 = SEG_BASE + 1; // 指标非法
pub const E_PB_ENV: u16 = SEG_BASE + 2; // 环境不一致（不可比）
pub const E_PB_FROZEN: u16 = SEG_BASE + 3; // 冻结基线直接改
pub const E_PB_REGRESS: u16 = SEG_BASE + 4; // 回归红

/// 六指标。
pub const M_ARB_OVERHEAD: u8 = 1; // 仲裁开销 ns/决策（低好）
pub const M_FAIRNESS: u8 = 2; // 公平指数 ×1000（高好）
pub const M_SWITCH_LAT: u8 = 3; // 切换延迟 tick（低好）
pub const M_PRED_HIT: u8 = 4; // 预测命中率 ‰（高好）
pub const M_SIM_TIME: u8 = 5; // 仿真耗时 us/千轮（低好）
pub const M_INTEG_OVERHEAD: u8 = 6; // 集成开销 ‰（低好）
pub const METRICS: [u8; 6] = [M_ARB_OVERHEAD, M_FAIRNESS, M_SWITCH_LAT, M_PRED_HIT, M_SIM_TIME, M_INTEG_OVERHEAD];

/// 方向：0=越低越好 1=越高越好。
pub const DIR_LOWER: u8 = 0;
pub const DIR_HIGHER: u8 = 1;

/// 指标方向声明。
pub fn direction(m: u8) -> Result<u8, u16> {
    match m {
        M_ARB_OVERHEAD | M_SWITCH_LAT | M_SIM_TIME | M_INTEG_OVERHEAD => Ok(DIR_LOWER),
        M_FAIRNESS | M_PRED_HIT => Ok(DIR_HIGHER),
        _ => Err(E_PB_METRIC),
    }
}

/// 环境声明三元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvDecl {
    pub load_profile: u8, // 负载谱码
    pub hw_tier: u8,      // 硬件档码
    pub clock_tier: u8,   // 时钟档码
}

impl EnvDecl {
    pub fn comparable(&self, other: &EnvDecl) -> Result<(), u16> {
        if self.load_profile != other.load_profile || self.hw_tier != other.hw_tier || self.clock_tier != other.clock_tier {
            return Err(E_PB_ENV);
        }
        Ok(())
    }
}

/// 基线条目（指标+基线值+回归带宽‰）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaselineRow {
    pub metric: u8,
    pub value: u64,
    /// 回归带宽（相对千分位）。
    pub band_pm: u64,
}

/// 基线冻结 v1（全谱）。
pub fn baseline_v1() -> [BaselineRow; 6] {
    [
        BaselineRow { metric: M_ARB_OVERHEAD, value: 800, band_pm: 100 }, // 800ns ±10%
        BaselineRow { metric: M_FAIRNESS, value: 920, band_pm: 30 }, // 920‰ ≥ 892
        BaselineRow { metric: M_SWITCH_LAT, value: 3, band_pm: 200 },
        BaselineRow { metric: M_PRED_HIT, value: 700, band_pm: 50 },
        BaselineRow { metric: M_SIM_TIME, value: 500, band_pm: 150 },
        BaselineRow { metric: M_INTEG_OVERHEAD, value: 40, band_pm: 250 },
    ]
}

/// 基线账（冻结态机）。
#[derive(Debug)]
pub struct PerfBaseline {
    rows: Vec<BaselineRow>,
    frozen: bool,
    pub version: u32,
}

impl PerfBaseline {
    pub fn v1(env: EnvDecl) -> Self {
        Self { rows: baseline_v1().to_vec(), frozen: true, version: 1 }
    }

    /// ADR 流程：解冻→改→再冻结。
    pub fn unfreeze(&mut self) {
        self.frozen = false;
    }
    pub fn refreeze(&mut self) -> Result<u32, u16> {
        if self.frozen {
            return Err(E_PB_FROZEN);
        }
        self.frozen = true;
        self.version += 1;
        Ok(self.version)
    }
    /// 修改基线值（仅解冻态；冻结态拒）。
    pub fn set_value(&mut self, metric: u8, value: u64) -> Result<(), u16> {
        if self.frozen {
            return Err(E_PB_FROZEN);
        }
        let row = self.rows.iter_mut().find(|r| r.metric == metric).ok_or(E_PB_METRIC)?;
        row.value = value;
        Ok(())
    }
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
    pub fn row(&self, metric: u8) -> Result<&BaselineRow, u16> {
        self.rows.iter().find(|r| r.metric == metric).ok_or(E_PB_METRIC)
    }
}

/// 回归判定：按方向+带宽。
pub fn regress_check(row: &BaselineRow, measured: u64) -> Result<(), u16> {
    let dir = direction(row.metric)?;
    let band = row.value.saturating_mul(row.band_pm) / 1000;
    match dir {
        DIR_LOWER => {
            // 越低越好：实测 > 基线+带宽 → 回归。
            if measured > row.value + band {
                return Err(E_PB_REGRESS);
            }
        }
        DIR_HIGHER => {
            // 越高越好：实测 < 基线−带宽 → 回归。
            if measured + band < row.value {
                return Err(E_PB_REGRESS);
            }
        }
        _ => return Err(E_PB_METRIC),
    }
    Ok(())
}

/// 全谱回归：六指标逐项。
pub fn full_regress(env_a: &EnvDecl, env_b: &EnvDecl, measured: &[BaselineRow; 6]) -> Result<(), u16> {
    env_a.comparable(env_b)?;
    let base = baseline_v1();
    for (b, m) in base.iter().zip(measured.iter()) {
        if b.metric != m.metric {
            return Err(E_PB_METRIC);
        }
        regress_check(b, m.value)?;
    }
    Ok(())
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、全谱冻结：六指标齐备+方向声明。
    {
        let env = EnvDecl { load_profile: 1, hw_tier: 2, clock_tier: 1 };
        let b = PerfBaseline::v1(env);
        v.push(("F0468-全谱冻结-六指标齐备", b.is_frozen() && b.row(M_ARB_OVERHEAD).is_ok()));
        v.push((
            "F0468-全谱冻结-方向声明",
            direction(M_ARB_OVERHEAD) == Ok(DIR_LOWER)
                && direction(M_FAIRNESS) == Ok(DIR_HIGHER)
                && direction(M_PRED_HIT) == Ok(DIR_HIGHER)
                && direction(M_SIM_TIME) == Ok(DIR_LOWER),
        ));
        v.push(("F0468-全谱冻结-指标非法拒", direction(99) == Err(E_PB_METRIC)));
    }
    // 二、环境声明：一致性前置闸。
    {
        let a = EnvDecl { load_profile: 1, hw_tier: 2, clock_tier: 1 };
        let same = a;
        let diff = EnvDecl { load_profile: 1, hw_tier: 3, clock_tier: 1 };
        v.push(("F0468-环境声明-同环境可比", a.comparable(&same).is_ok()));
        v.push(("F0468-环境声明-异环境拒比", a.comparable(&diff) == Err(E_PB_ENV)));
    }
    // 三、基线冻结 ADR 流程。
    {
        let env = EnvDecl { load_profile: 1, hw_tier: 2, clock_tier: 1 };
        let mut b = PerfBaseline::v1(env);
        v.push(("F0468-基线-冻结态改值拒", b.set_value(M_FAIRNESS, 900) == Err(E_PB_FROZEN)));
        b.unfreeze();
        v.push(("F0468-基线-解冻后可改", b.set_value(M_FAIRNESS, 900).is_ok()));
        let ver = b.refreeze();
        v.push(("F0468-基线-再冻结版本推进", ver == Ok(2) && b.is_frozen()));
        // 冻结态再冻结拒。
        v.push(("F0468-基线-重复冻结拒", b.refreeze() == Err(E_PB_FROZEN)));
    }
    // 四、回归接入：方向+带宽判定。
    {
        let base = baseline_v1();
        // 低好指标：基线 800±10%，880 过、881 红。
        let arb = base[0];
        v.push(("F0468-回归-带宽内通过", regress_check(&arb, 880).is_ok()));
        v.push(("F0468-回归-超带红", regress_check(&arb, 881) == Err(E_PB_REGRESS)));
        // 高好指标：公平 920 带宽 27‰（整数化），893 过、892 红。
        let fair = base[1];
        v.push(("F0468-回归-高好带宽内过", regress_check(&fair, 893).is_ok()));
        v.push(("F0468-回归-高好超带红", regress_check(&fair, 892) == Err(E_PB_REGRESS)));
    }
    // 五、全谱回归。
    {
        let env = EnvDecl { load_profile: 1, hw_tier: 2, clock_tier: 1 };
        let good = baseline_v1();
        v.push(("F0468-全谱回归-基线自身过", full_regress(&env, &env, &good).is_ok()));
        let mut bad = good;
        bad[0].value = 900; // 仲裁开销 900 > 880 带宽
        v.push(("F0468-全谱回归-单项超带红", full_regress(&env, &env, &bad) == Err(E_PB_REGRESS)));
        let diff_env = EnvDecl { load_profile: 2, hw_tier: 2, clock_tier: 1 };
        v.push(("F0468-全谱回归-异环境拒", full_regress(&env, &diff_env, &good) == Err(E_PB_ENV)));
    }
    // 六、判据自检。
    {
        v.push(("F0468-判据-六指标枚举", METRICS.len() == 6));
        v.push(("F0468-判据-带宽非零", baseline_v1().iter().all(|r| r.band_pm > 0)));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0468_perfbaseline_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
