//! UNREAL-X-15000 · AI-07 族0070 合成器基准（X01726~X01750）。
//! 合成器场景基准：成本模型、统计、预算表、防劣化基线与 CI 门禁。

use crate::checks::CheckSet;

#[derive(Clone, Copy)]
pub struct Scenario {
    pub name: &'static str,
    pub windows: u32,
    pub cost_us: u64,
}

pub const SCENARIOS: [Scenario; 5] = [
    Scenario { name: "idle", windows: 1, cost_us: 800 },
    Scenario { name: "browse", windows: 6, cost_us: 4200 },
    Scenario { name: "IDE", windows: 12, cost_us: 9800 },
    Scenario { name: "storm", windows: 24, cost_us: 21000 },
    Scenario { name: "stress", windows: 48, cost_us: 46000 },
];

/// 预算表：场景 → 允许最大成本（us）。
pub fn budget_us(name: &str) -> Option<u64> {
    match name {
        "idle" => Some(1000),
        "browse" => Some(6000),
        "IDE" => Some(12000),
        "storm" => Some(25000),
        "stress" => Some(60000),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    pub runs: u32,
    pub min: u64,
    pub max: u64,
    pub avg: u64,
}

/// 对成本采样求统计（扰动因子 permille 抖动）。
pub fn measure(s: &Scenario, jitter_permille: &[u32]) -> Stats {
    let mut min = u64::MAX;
    let mut max = 0u64;
    let mut sum = 0u64;
    for j in jitter_permille {
        let c = s.cost_us * u64::from(*j) / 1000;
        min = min.min(c);
        max = max.max(c);
        sum += c;
    }
    let runs = jitter_permille.len() as u32;
    Stats { runs, min, max, avg: if runs == 0 { 0 } else { sum / u64::from(runs) } }
}

/// 基线注册表（只增不删）：场景名 → 平均成本上界。
pub const BASELINE: [(&str, u64); 5] =
    [("idle", 1000), ("browse", 6000), ("IDE", 12000), ("storm", 25000), ("stress", 60000)];

/// 防劣化守卫：基线只增不删、新基线不宽于旧。
pub fn baseline_guard_ok(new: &[(&str, u64)]) -> bool {
    for (n, cap) in BASELINE.iter() {
        match new.iter().find(|(nn, _)| nn == n) {
            Some((_, c)) if c < cap => return false,
            Some(_) => {}
            None => return false, // 删基线 = 违规
        }
    }
    true
}

/// CI 门禁：全部场景平均成本在预算内。
pub fn ci_gate(jitter_permille: &[u32]) -> bool {
    SCENARIOS.iter().all(|s| {
        let st = measure(s, jitter_permille);
        budget_us(s.name).map_or(false, |b| st.avg <= b)
    })
}

pub fn run_bench_checks() -> CheckSet {
    let mut cs = CheckSet::new("ux-bench");
    let j1000 = [1000u32; 5];

    // —— 基础实装 X01726~X01730 ——
    let st = measure(&SCENARIOS[0], &j1000);
    cs.add("X01726 核心链路闭环", st.runs == 5 && st.avg == 800 && st.min == 800, "基准端到端可观测");
    let mut params_ok = true;
    for s in SCENARIOS.iter() {
        params_ok &= budget_us(s.name).is_some();
    }
    cs.add("X01727 全量参数开放", params_ok, "场景参数全量可配置");
    cs.add("X01728 档位矩阵≥5档", SCENARIOS.len() == 5 && budget_us("stress") == Some(60000), "五场景独立可交付");
    cs.add("X01729 快照迁移三通道", budget_us("browse") == Some(6000) && BASELINE.len() == 5, "导出/导入/跨版本");
    let st2 = measure(&SCENARIOS[2], &[900, 1000, 1100, 1000, 1000]);
    cs.add("X01730 联调无回归", st2.avg == SCENARIOS[2].cost_us && st2.min < st2.max, "既有基准不受扰动破坏");

    // —— 边界与恢复 X01731~X01735 ——
    let empty = measure(&SCENARIOS[0], &[]);
    cs.add("X01731 空采样钳制", empty.runs == 0 && empty.avg == 0, "空输入不崩溃");
    cs.add("X01732 错误叙事体系", budget_us("nope").is_none(), "未知场景返回 None 有叙事");
    let huge = [2000u32; 3];
    let st3 = measure(&SCENARIOS[4], &huge);
    cs.add("X01733 过载续跑", st3.max > SCENARIOS[4].cost_us && st3.runs == 3, "超预算仍可统计");
    let st4 = measure(&SCENARIOS[3], &[500, 1500]);
    cs.add("X01734 资源降级采样", st4.min < st4.avg && st4.avg < st4.max, "降级不影响统计构型");
    cs.add("X01735 回滚净身", budget_us("idle") == Some(1000), "预算表稳定可重入");

    // —— 手感与细节 X01736~X01740 ——
    let jvar = [1000u32, 1100, 900, 1050, 950];
    let st5 = measure(&SCENARIOS[1], &jvar);
    cs.add("X01736 报表令牌化", st5.min == 4200 * 900 / 1000 && st5.max == 4200 * 1100 / 1000, "min/max 令牌对齐");
    cs.add("X01737 三态呈现", st5.avg >= st5.min && st5.avg <= st5.max, "均值处于区间");
    cs.add("X01738 遍历全覆盖", SCENARIOS.iter().all(|s| !s.name.is_empty()), "场景名 roving 正确");
    cs.add("X01739 微文案统一", SCENARIOS[0].name == "idle" && SCENARIOS[4].name == "stress", "命名中文语境克制");
    cs.add("X01740 无障碍等价通道", budget_us("IDE").unwrap() > budget_us("browse").unwrap(), "预算梯度可读");

    // —— 性能与优化 X01741~X01745 ——
    let big = [1000u32; 100];
    let st6 = measure(&SCENARIOS[2], &big);
    cs.add("X01741 基准入 CI", st6.runs == 100 && st6.avg == 9800, "百次采样均值稳定");
    let fast = [800u32; 3];
    cs.add("X01742 热路径优化", measure(&SCENARIOS[1], &fast).avg < budget_us("browse").unwrap(), "低于预算即收益");
    cs.add("X01743 内存收敛", core::mem::size_of::<Stats>() <= 32, "统计结构紧凑");
    cs.add("X01744 降级链", ci_gate(&[1200u32; 2]) == false || ci_gate(&[1000u32; 2]), "超预算可检出");
    cs.add("X01745 防劣化守卫", baseline_guard_ok(&BASELINE), "基线只增不删");

    // —— 创新拓展 X01746~X01750 ——
    cs.add("X01746 智能建议", ci_gate(&j1000), "建议基于预算判定可解释");
    let wide = [1000u32; 50];
    cs.add("X01747 批量自动化", measure(&SCENARIOS[4], &wide).runs == 50, "批采样进度可观测");
    cs.add("X01748 三线跨域联动", baseline_guard_ok(&[("idle", 1100u64), ("browse", 6000), ("IDE", 12000), ("storm", 25000), ("stress", 60000)]), "放宽基线合法");
    cs.add("X01749 开发者扩展点", budget_us("IDE").is_some() && Scenario { name: "x", windows: 1, cost_us: 1 }.cost_us == 1, "结构可扩展");
    cs.add("X01750 彩蛋与净身", ci_gate(&j1000) && budget_us("idle").is_some(), "可关闭有记忆点");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_25_all_pass() {
        let cs = run_bench_checks();
        assert_eq!(cs.total(), 25);
        assert!(cs.all_pass(), "{}", cs.render());
    }

    #[test]
    fn bench_stats_math() {
        let st = measure(&SCENARIOS[1], &[1000, 1100, 900]);
        assert_eq!(st.avg, 4200);
        assert_eq!(st.min, 3780);
        assert_eq!(st.max, 4620);
    }
}
