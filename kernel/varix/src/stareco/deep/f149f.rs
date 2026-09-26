//! 深化层三 · F149 季度生态报告（2026-09-26 深化批次三）。
//!
//! 补深图表工程面（主册 G-D-24）：序列聚合（同比/环比守卫）、异常
//! 检测（均值带外判红）、图表数据（归一化柱高整数口径——确定性
//! 渲染）、归因强制（下滑指标必须带故事编号）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 序列聚合：环比/同比——分母为零如实返回 None（不编 0%）
// ---------------------------------------------------------------------------

/// 环比：本期 vs 上期，(上期=0 → None)。返回万分比变化。
pub fn qoq(current: u32, previous: u32) -> Option<i64> {
    if previous == 0 {
        return None;
    }
    Some((current as i64 - previous as i64) * 10000 / previous as i64)
}

/// 同比：本期 vs 四季前。
pub fn yoy(current: u32, year_ago: u32) -> Option<i64> {
    qoq(current, year_ago)
}

// ---------------------------------------------------------------------------
// 异常检测：中位数 ± 中位数两成带宽 → 带外判红（窗口 ≥3 才判）。
// 均值会被离群点自己拉偏（尖峰把全窗带爆）——中位数是稳健口径。
// ---------------------------------------------------------------------------

/// 返回带外点序号清单。中位数取排序后 n/2 位（偶数取上中位）。
pub fn anomalies(series: &[u32]) -> alloc::vec::Vec<usize> {
    let mut out: alloc::vec::Vec<usize> = alloc::vec::Vec::new();
    if series.len() < 3 {
        return out;
    }
    let mut sorted: alloc::vec::Vec<u32> = series.to_vec();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2] as u64;
    let band = median * 20 / 100;
    for (i, v) in series.iter().enumerate() {
        if (*v as u64).abs_diff(median) > band {
            out.push(i);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 图表数据：归一化柱高（0..=100 整数口径——确定性渲染）
// ---------------------------------------------------------------------------

/// 柱高 = 值/最大值×100；全零序列全 0 高（除零守卫）。
pub fn bar_heights(series: &[u32]) -> alloc::vec::Vec<u32> {
    let max = series.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return alloc::vec![0; series.len()];
    }
    series.iter().map(|v| *v * 100 / max).collect()
}

// ---------------------------------------------------------------------------
// 归因强制：下滑指标必须带故事编号——无归因即整份红
// ---------------------------------------------------------------------------

/// (指标名, 本期, 上期, 归因故事编号)
pub struct MetricRow {
    pub name: &'static str,
    pub current: u32,
    pub previous: u32,
    pub story_ref: Option<u32>,
}

/// 全表归因裁决：任一下滑（current<previous）缺故事 → Err(指标名)。
pub fn attribution_audit(rows: &[MetricRow]) -> Result<(), &'static str> {
    for r in rows {
        if r.current < r.previous && r.story_ref.is_none() {
            return Err(r.name);
        }
    }
    Ok(())
}

/// 下滑计数（季报「如实登记」段的数据源）。
pub fn decline_count(rows: &[MetricRow]) -> usize {
    rows.iter().filter(|r| r.current < r.previous).count()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F149F_TAG: &str = "stareco-F149-deep3";

pub fn run_f149_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F149F_TAG);

    // 环比/同比
    set.add("f149f qoq up", qoq(120, 100) == Some(2000), "增长 20% = 2000‱");
    set.add("f149f qoq down", qoq(90, 100) == Some(-1000), "下滑 10%");
    set.add("f149f qoq zero prev", qoq(50, 0).is_none(), "零分母如实 None");
    set.add("f149f yoy same", yoy(100, 100) == Some(0), "同比持平");
    set.add("f149f qoq shrink", qoq(1, 10000) == Some(-9999), "万分比收缩");

    // 异常检测
    let s = [100u32, 102, 98, 300, 99];
    set.add("f149f anomaly found", anomalies(&s) == alloc::vec![3], "300 带外检出");
    set.add("f149f anomaly clean", anomalies(&[100u32, 101, 99]).is_empty(), "平稳序列零误报");
    set.add("f149f anomaly short", anomalies(&[1u32, 999]).is_empty(), "窗口不足不判");
    let spike = [50u32, 50, 50, 50, 0];
    set.add("f149f anomaly low", anomalies(&spike) == alloc::vec![4], "骤降同样带外");

    // 图表
    let bars = bar_heights(&[50u32, 100, 25]);
    set.add("f149f bars", bars == alloc::vec![50, 100, 25], "归一化柱高对拍");
    set.add("f149f bars zero", bar_heights(&[0, 0]) == alloc::vec![0, 0], "全零序列全零高");
    set.add("f149f bars empty", bar_heights(&[]).is_empty(), "空序列空柱");

    // 归因
    let rows = [
        MetricRow { name: "themes", current: 120, previous: 100, story_ref: None },
        MetricRow { name: "contributors", current: 80, previous: 90, story_ref: Some(7) },
    ];
    set.add("f149f attribution ok", attribution_audit(&rows).is_ok(), "下滑已归因");
    let bad_rows = [
        MetricRow { name: "themes", current: 120, previous: 100, story_ref: None },
        MetricRow { name: "downloads", current: 10, previous: 50, story_ref: None },
    ];
    set.add(
        "f149f attribution missing",
        attribution_audit(&bad_rows) == Err("downloads"),
        "无归因指名道姓",
    );
    set.add("f149f decline count", decline_count(&rows) == 1 && decline_count(&bad_rows) == 1, "下滑计数");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn qoq_negative_ground() {
        // 清零式下滑：-10000‱（-100%）。
        assert_eq!(qoq(0, 100), Some(-10000));
        // 增长天花板无钳制（如实放大）。
        assert_eq!(qoq(300, 100), Some(20000));
    }

    #[test]
    fn band_edges() {
        // 恰在带宽上（diff == band）：不判带外（> band 才红）。
        // 中位数 50，带 10：60 的 diff 恰 10 → 留在带内。
        let s = [40u32, 60, 50, 49];
        assert!(anomalies(&s).is_empty());
        // 61 的 diff = 11 > 10 → 带外（序号 1）。
        let s2 = [45u32, 61, 50, 49];
        assert_eq!(anomalies(&s2), alloc::vec![1]);
    }
}
