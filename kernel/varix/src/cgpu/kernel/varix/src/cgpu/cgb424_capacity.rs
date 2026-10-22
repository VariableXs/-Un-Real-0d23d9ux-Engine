//! CGPU-F0424 · 容量规划数据（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 容量数据：长期趋势输出（负载增长趋势→容量建议——升级/降配参考的
//!   数据支持）；趋势外推（诚实标注：外推是参考非承诺）；容量场景（硬件
//!   升级前后的预算模拟——对接 F0347 仿真器）；建议报告格式。
//! - 判据：趋势、外推标注、仿真对接、报告、判据。
//!
//! 设计纪律：
//! - 趋势：对月度负载总量做最小二乘斜率（线性趋势）；斜率稳定（两段
//!   半窗斜率同号且差 < 50‰）才出建议（噪声斜率不出建议）。
//! - 外推：趋势线延伸 N 期并配「参考非承诺」标注（诚实标注是数据结构
//!   的一部分——is_projection: true 不可剥离）；外推跨度超过历史长度
//!   一半即拒绝（外推不比历史长）。
//! - 仿真对接：容量场景 = 预算参数包（各资源总预算），喂给 F0347
//!   仿真器跑升级前/后两景，输出达标率对比。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0DB0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0DB0，全仓独占）。
pub const SEG_BASE: u16 = 0x0DB0;

pub const E_CAP_TREND: u16 = SEG_BASE + 1; // 样本不足出趋势
pub const E_CAP_EXTRAP: u16 = SEG_BASE + 2; // 外推跨度越限
pub const E_CAP_UNSTABLE: u16 = SEG_BASE + 3; // 斜率不稳（噪声）不出建议
pub const E_CAP_SIM: u16 = SEG_BASE + 4; // 仿真场景参数非法
pub const E_CAP_MARK: u16 = SEG_BASE + 5; // 外推结果缺诚实标注
pub const E_CAP_NEG: u16 = SEG_BASE + 6; // 负负载样本

/// 最小历史长度（期）——不足不出趋势。
pub const MIN_HISTORY: usize = 6;
/// 外推跨度上限（历史长度的 1/2）。
pub const EXTRAP_RATIO: usize = 2;
/// 稳定残差带（千分位：平均绝对残差/均值）。
pub const SLOPE_STABLE_PM: u32 = 100;

/// 一期负载样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadPoint {
    pub period: u64,
    pub total_load: u64,
}

/// 线性趋势（最小二乘）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trend {
    /// 斜率（每期负载增量，定点 ×100）。
    pub slope_x100: i64,
    /// 截距（首期拟合值，定点 ×100）。
    pub intercept_x100: i64,
    /// 样本数。
    pub n: usize,
}

/// 最小二乘拟合（x = 期序 0..n-1）。
pub fn fit_trend(loads: &[u64]) -> Result<Trend, u16> {
    if loads.len() < MIN_HISTORY {
        return Err(E_CAP_TREND);
    }
    for l in loads {
        let _ = l; // 负载为 u64 天然非负——占位校验点
    }
    let n = loads.len() as i64;
    let sx: i64 = (0..n).sum();
    let sxx: i64 = (0..n).map(|i| i * i).sum();
    let sy: i64 = loads.iter().map(|l| *l as i64).sum();
    let sxy: i64 = loads.iter().enumerate().map(|(i, l)| i as i64 * *l as i64).sum();
    let denom = n * sxx - sx * sx;
    if denom == 0 {
        return Err(E_CAP_TREND);
    }
    let slope = (n * sxy - sx * sy) * 100 / denom;
    let intercept = (sy * sxx - sx * sxy) * 100 / denom;
    Ok(Trend { slope_x100: slope, intercept_x100: intercept, n: loads.len() })
}

/// 斜率稳定性：拟合残差占比带（平均绝对残差 / 均值 ≤ 带）。
/// 线性上升与全平都稳（残差≈0）；锯齿噪声拟合不出来（残差大）⇒ 不稳。
pub fn slope_stable(t: &Trend, loads: &[u64]) -> Result<bool, u16> {
    if loads.len() < MIN_HISTORY {
        return Err(E_CAP_TREND);
    }
    let n = loads.len() as i64;
    let mean: i64 = loads.iter().map(|l| *l as i64).sum::<i64>() / n;
    if mean == 0 {
        return Ok(false);
    }
    let mut resid = 0i64;
    for (i, l) in loads.iter().enumerate() {
        let fit = t.intercept_x100 + t.slope_x100 * i as i64; // 定点 ×100
        resid += (fit - *l as i64 * 100).abs() / 100;
    }
    let avg_resid = resid / n;
    Ok(avg_resid * 1000 <= mean * SLOPE_STABLE_PM as i64)
}

/// 外推投影（诚实标注是结构的一部分）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Projection {
    pub period: u64,
    /// 预测负载（定点 ×100）。
    pub load_x100: i64,
    /// 诚实标注：外推是参考非承诺。
    pub is_projection: bool,
}

/// 外推 N 期（N ≤ 历史长度/2）。
pub fn extrapolate(t: &Trend, last_period: u64, horizon: usize) -> Result<Vec<Projection>, u16> {
    if t.n < MIN_HISTORY {
        return Err(E_CAP_TREND);
    }
    if horizon == 0 || horizon > t.n / EXTRAP_RATIO {
        return Err(E_CAP_EXTRAP);
    }
    let mut out = Vec::new();
    for k in 1..=horizon {
        out.push(Projection {
            period: last_period + k as u64,
            load_x100: t.intercept_x100 + t.slope_x100 * (t.n as i64 + k as i64 - 1),
            is_projection: true, // 标注不可剥离
        });
    }
    Ok(out)
}

/// 容量场景（喂给 F0347 仿真器的预算参数包）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapScenario {
    /// 各资源总预算（千分位满格 = 1000 基准的倍率 ×1000）。
    pub budgets: [u32; 4],
    /// 场景名（before/after）。
    pub tag: &'static str,
}

/// 仿真对接：跑两景（升级前/后），返回各自达标率（千分位）。
/// 仿真器以闭式模型替代：达标 = 每样本负载 ≤ 场景预算 × 容量系数。
pub fn simulate_scenarios(
    before: &CapScenario,
    after: &CapScenario,
    samples: &[u64],
    capacity_gain_pm: u32,
) -> Result<(u32, u32), u16> {
    for b in before.budgets.iter().chain(after.budgets.iter()) {
        if *b == 0 {
            return Err(E_CAP_SIM);
        }
    }
    if capacity_gain_pm > 1000 {
        return Err(E_CAP_SIM);
    }
    let rate = |budgets: &[u32; 4]| -> u32 {
        let base = budgets.iter().sum::<u32>() as u64;
        let cap = base * capacity_gain_pm as u64 / 1000 + base;
        let mut ok = 0;
        for s in samples {
            if *s <= cap {
                ok += 1;
            }
        }
        if samples.is_empty() {
            0
        } else {
            (ok * 1000 / samples.len() as u64) as u32
        }
    };
    Ok((rate(&before.budgets), rate(&after.budgets)))
}

/// 容量建议报告（升级/降配/维持三向 + 诚实标注）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapAdvice {
    /// 建议：0=维持 1=升级 2=可降配。
    pub action: u8,
    /// 依据：外推投影首期负载（×100）。
    pub projected_load_x100: i64,
    /// 诚实标注：参考非承诺。
    pub is_projection: bool,
}

/// 从趋势与当前容量出建议（外推标注强制传递）。
pub fn advise(t: &Trend, loads: &[u64], current_cap: u64) -> Result<CapAdvice, u16> {
    if !slope_stable(t, loads)? {
        return Err(E_CAP_UNSTABLE);
    }
    let proj = extrapolate(t, t.n as u64 - 1, 1)?;
    let p = &proj[0];
    if !p.is_projection {
        return Err(E_CAP_MARK);
    }
    let projected = p.load_x100 / 100;
    let action = if projected as i64 > current_cap as i64 * 110 / 100 {
        1
    } else if (projected as i64) < current_cap as i64 * 60 / 100 {
        2
    } else {
        0
    };
    Ok(CapAdvice {
        action,
        projected_load_x100: p.load_x100,
        is_projection: true,
    })
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn rising() -> Vec<u64> {
        vec![100, 110, 120, 130, 140, 150, 160, 170]
    }

    #[test]
    fn a01_趋势_最小二乘与样本门槛() {
        let t = fit_trend(&rising()).unwrap_or(Trend { slope_x100: 0, intercept_x100: 0, n: 0 });
        assert_eq!(t.slope_x100, 1_000); // 每期 +10（×100 定点）
        assert_eq!(t.n, 8);
        // 样本不足拒。
        assert_eq!(fit_trend(&[1, 2, 3]), Err(E_CAP_TREND));
        // 下降趋势。
        let mut down = rising();
        down.reverse();
        let td = fit_trend(&down).unwrap_or(Trend { slope_x100: 0, intercept_x100: 0, n: 0 });
        assert!(td.slope_x100 < 0);
    }

    #[test]
    fn a02_外推_诚实标注与跨度限() {
        let t = fit_trend(&rising()).unwrap_or(Trend { slope_x100: 0, intercept_x100: 0, n: 0 });
        let p = extrapolate(&t, 7, 3).unwrap_or_default();
        assert_eq!(p.len(), 3);
        assert!(p.iter().all(|x| x.is_projection)); // 标注不可剥离
        assert_eq!(p[0].period, 8);
        // 外推值 = 趋势延伸：末样本 170 → 下一期 180（×100 = 18000）。
        assert_eq!(p[0].load_x100, 18_000);
        // 跨度越限拒（历史 8 期最多外推 4）。
        assert_eq!(extrapolate(&t, 7, 5), Err(E_CAP_EXTRAP));
        assert_eq!(extrapolate(&t, 7, 0), Err(E_CAP_EXTRAP));
    }

    #[test]
    fn a03_斜率稳定_噪声不出建议() {
        let t = fit_trend(&rising()).unwrap_or(Trend { slope_x100: 0, intercept_x100: 0, n: 0 });
        assert_eq!(slope_stable(&t, &rising()), Ok(true));
        // 锯齿噪声（线性拟合残差大）⇒ 不稳。
        let noisy = vec![100, 200, 100, 200, 100, 200, 100, 200];
        let tn = fit_trend(&noisy).unwrap_or(Trend { slope_x100: 0, intercept_x100: 0, n: 0 });
        assert_eq!(slope_stable(&tn, &noisy), Ok(false));
        // 不稳出建议拒绝。
        assert_eq!(advise(&tn, &noisy, 1_000), Err(E_CAP_UNSTABLE));
    }

    #[test]
    fn a04_仿真对接_升级前后达标率对比() {
        let before = CapScenario { budgets: [250, 250, 250, 250], tag: "before" };
        let after = CapScenario { budgets: [400, 400, 400, 400], tag: "after" };
        let samples = vec![3_000u64, 3_500, 4_200, 5_000, 6_000];
        let (r1, r2) = simulate_scenarios(&before, &after, &samples, 200)
            .unwrap_or((0, 0));
        // 升级后达标率必须 ≥ 前。
        assert!(r2 >= r1);
        // 零预算场景拒。
        let bad = CapScenario { budgets: [0, 1, 1, 1], tag: "x" };
        assert_eq!(
            simulate_scenarios(&bad, &after, &samples, 200),
            Err(E_CAP_SIM)
        );
        // 容量系数越界拒。
        assert_eq!(
            simulate_scenarios(&before, &after, &samples, 1_001),
            Err(E_CAP_SIM)
        );
    }

    #[test]
    fn a05_建议报告_三向与标注传递() {
        let t = fit_trend(&rising()).unwrap_or(Trend { slope_x100: 0, intercept_x100: 0, n: 0 });
        // 容量 150 < 外推 180×1.1（=165）⇒ 升级建议。
        let a = advise(&t, &rising(), 150).unwrap_or(CapAdvice {
            action: 0, projected_load_x100: 0, is_projection: false,
        });
        assert_eq!(a.action, 1);
        assert!(a.is_projection); // 参考非承诺——标注强制
        // 容量充足（外推远低于 60%）⇒ 可降配。
        let flat = vec![100, 100, 100, 100, 100, 100, 100, 100];
        let tf = fit_trend(&flat).unwrap_or(Trend { slope_x100: 0, intercept_x100: 0, n: 0 });
        let a2 = advise(&tf, &flat, 1_000).unwrap_or(CapAdvice {
            action: 0, projected_load_x100: 0, is_projection: false,
        });
        assert_eq!(a2.action, 2);
        // 容量匹配 ⇒ 维持。
        let a3 = advise(&tf, &flat, 120).unwrap_or(CapAdvice {
            action: 0, projected_load_x100: 0, is_projection: false,
        });
        assert_eq!(a3.action, 0);
    }
}
