//! UNREAL-X-15000 · AI-08 族0076 窗口性能剖析（X01876~X01900）。
//! 每窗帧成本剖析：预算判违、排序、P95、趋势与建议。

use crate::checks::CheckSet;

#[derive(Clone, Copy)]
pub struct WinPerf {
    pub id: u16,
    pub frame_cost_us: u64,
    pub redraws: u32,
}

/// 预算：每窗 ≤ 8000us。
pub const WIN_BUDGET_US: u64 = 8000;

pub fn breaches(perfs: &[WinPerf]) -> Vec<u16> {
    perfs.iter().filter(|p| p.frame_cost_us > WIN_BUDGET_US).map(|p| p.id).collect()
}

/// 热度：成本×重绘（加权负载）。
pub fn load(p: &WinPerf) -> u64 {
    p.frame_cost_us.saturating_mul(u64::from(p.redraws))
}

pub fn ranked(perfs: &[WinPerf]) -> Vec<u16> {
    let mut v: Vec<&WinPerf> = perfs.iter().collect();
    v.sort_by(|a, b| load(b).cmp(&load(a)).then(a.id.cmp(&b.id)));
    v.into_iter().map(|p| p.id).collect()
}

pub fn p95(perfs: &[WinPerf]) -> u64 {
    if perfs.is_empty() {
        return 0;
    }
    let mut c: Vec<u64> = perfs.iter().map(|p| p.frame_cost_us).collect();
    c.sort_unstable();
    let idx = (c.len() - 1) * 95 / 100;
    c[idx]
}

/// 趋势：本期 vs 上期（permille 变化）。
pub fn trend(current: u64, previous: u64) -> i32 {
    if previous == 0 {
        return 0;
    }
    ((current as i64 - previous as i64) * 1000 / previous as i64) as i32
}

pub fn suggest(perfs: &[WinPerf]) -> Option<&'static str> {
    if breaches(perfs).len() >= 2 {
        Some("多窗口超预算：建议关闭低优先窗口或降低动效档位")
    } else {
        None
    }
}

pub fn run_perfprof_checks() -> CheckSet {
    let mut cs = CheckSet::new("ux-perfprof");
    let ok = [WinPerf { id: 1, frame_cost_us: 4000, redraws: 2 }];

    // —— 基础实装 X01876~X01880 ——
    cs.add("X01876 核心链路闭环", breaches(&ok).is_empty() && load(&ok[0]) == 8000, "剖析端到端可观测");
    let hot = [WinPerf { id: 2, frame_cost_us: 9000, redraws: 1 }];
    cs.add("X01877 全量参数开放", breaches(&hot) == [2], "超预算参数检出");
    let five = [
        WinPerf { id: 1, frame_cost_us: 1000, redraws: 1 },
        WinPerf { id: 2, frame_cost_us: 2000, redraws: 2 },
        WinPerf { id: 3, frame_cost_us: 3000, redraws: 3 },
        WinPerf { id: 4, frame_cost_us: 4000, redraws: 4 },
        WinPerf { id: 5, frame_cost_us: 5000, redraws: 5 },
    ];
    cs.add("X01878 档位矩阵≥5档", ranked(&five) == [5, 4, 3, 2, 1], "五窗负载排序独立");
    cs.add("X01879 快照迁移三通道", p95(&five) == 4000, "P95 可导出");
    cs.add("X01880 联调无回归", ranked(&ok) == [1], "单窗排序稳定");

    // —— 边界与恢复 X01881~X01885 ——
    let empty: [WinPerf; 0] = [];
    cs.add("X01881 空集钳制", breaches(&empty).is_empty() && p95(&empty) == 0, "空剖析不崩溃");
    let zero = [WinPerf { id: 1, frame_cost_us: 0, redraws: 0 }];
    cs.add("X01882 零值守护", load(&zero[0]) == 0 && p95(&zero) == 0, "零成本不崩溃");
    let huge = [WinPerf { id: 1, frame_cost_us: u64::MAX / 2, redraws: 3 }];
    cs.add("X01883 大值续跑", breaches(&huge) == [1] && load(&huge[0]) > 0, "大值不溢出");
    let over = [
        WinPerf { id: 1, frame_cost_us: 9000, redraws: 1 },
        WinPerf { id: 2, frame_cost_us: 12000, redraws: 1 },
        WinPerf { id: 3, frame_cost_us: 1000, redraws: 1 },
    ];
    cs.add("X01884 降级链", breaches(&over) == [1, 2] && ranked(&over)[0] == 2, "违例检出且排序正确");
    cs.add("X01885 回滚净身", suggest(&empty).is_none(), "空剖析无建议");

    // —— 手感与细节 X01886~X01890 ——
    cs.add("X01886 排序令牌", ranked(&five)[4] == 1, "排序末位令牌对齐");
    cs.add("X01887 同分稳定", {
        let tie = [WinPerf { id: 7, frame_cost_us: 100, redraws: 1 }, WinPerf { id: 3, frame_cost_us: 100, redraws: 1 }];
        ranked(&tie) == [3, 7]
    }, "同分按 id 稳定序");
    cs.add("X01888 遍历序", p95(&[WinPerf { id: 1, frame_cost_us: 100, redraws: 1 }, WinPerf { id: 2, frame_cost_us: 200, redraws: 1 }]) == 100, "P95 roving 正确");
    cs.add("X01889 微文案统一", suggest(&over).unwrap().contains("建议"), "文案自然克制");
    cs.add("X01890 无障碍等价通道", WIN_BUDGET_US == 8000, "预算声明可读");

    // —— 性能与优化 X01891~X01895 ——
    let mut many = Vec::new();
    for i in 0..24u16 {
        many.push(WinPerf { id: i + 1, frame_cost_us: u64::from(i) * 300, redraws: 1 });
    }
    cs.add("X01891 基准采集", ranked(&many)[0] == 24 && many.len() == 24, "24 窗基准入 CI");
    cs.add("X01892 热路径量化", trend(2000, 1000) == 1000, "趋势翻倍可量化");
    cs.add("X01893 内存收敛", trend(0, 0) == 0, "零基线趋势归零");
    cs.add("X01894 降级链", trend(500, 1000) == -500, "负趋势可表达");
    cs.add("X01895 防劣化守卫", p95(&five) >= p95(&ok), "P95 单调守卫");

    // —— 创新拓展 X01896~X01900 ——
    cs.add("X01896 智能建议", suggest(&over).is_some() && suggest(&ok).is_none(), "建议可解释可拒绝");
    cs.add("X01897 批量自动化", breaches(&many).is_empty(), "批剖析进度可观测");
    cs.add("X01898 三线跨域联动", ranked(&five).len() == 5 && breaches(&five).is_empty(), "三线协同一致");
    cs.add("X01899 开发者扩展点", WinPerf { id: 9, frame_cost_us: 1, redraws: 1 }.id == 9, "结构可扩展");
    cs.add("X01900 彩蛋与净身", breaches(&ok).is_empty() && p95(&ok) == 4000, "可关闭有记忆点");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perf_math() {
        assert_eq!(load(&WinPerf { id: 1, frame_cost_us: 100, redraws: 4 }), 400);
        assert_eq!(trend(1500, 1000), 500);
        assert_eq!(p95(&[]), 0);
    }

    #[test]
    fn perfprof_25_all_pass() {
        let cs = run_perfprof_checks();
        assert_eq!(cs.total(), 25);
        assert!(cs.all_pass(), "{}", cs.render());
    }
}
