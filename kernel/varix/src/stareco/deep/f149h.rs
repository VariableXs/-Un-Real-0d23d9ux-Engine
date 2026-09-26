//! 深化层五 · F149 季度生态报告（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：报告 → 欢迎中心/关于页的订阅行（最新值+环比
//! 人话）、缺季诚实行、报告页区块装配（指标卡/故事/预告三段）。

use super::f149f::qoq;
use super::f149g::{Goal, GridCell};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 订阅行：指标最新值 + 环比人话（增长/下滑/持平/无法比较）
// ---------------------------------------------------------------------------

pub struct MetricSubRow {
    pub metric: &'static str,
    pub value: u32,
    pub trend_label: &'static str,
}

pub fn subscription_rows(
    cells: &[GridCell],
    metric: &'static str,
) -> Option<MetricSubRow> {
    let mut series: alloc::vec::Vec<(u32, u32)> =
        cells.iter().filter(|c| c.metric == metric).map(|c| (c.quarter, c.value)).collect();
    if series.is_empty() {
        return None;
    }
    series.sort_by_key(|(q, _)| *q);
    let (last_q, last_v) = series[series.len() - 1];
    let trend_label = if series.len() < 2 {
        "首期数据"
    } else {
        let prev = series[series.len() - 2].1;
        match qoq(last_v, prev) {
            None => "无法比较",
            Some(d) if d > 200 => "明显增长",
            Some(d) if d > 0 => "略增",
            Some(0) => "持平",
            Some(d) if d > -200 => "略降",
            _ => "明显下滑",
        }
    };
    let _ = last_q;
    Some(MetricSubRow { metric, value: last_v, trend_label })
}

// ---------------------------------------------------------------------------
// 缺季诚实行：订阅指标在最新季缺数据 → 行给「数据待修」（不显示旧值
// 冒充新值——一处一事实的时间语义）
// ---------------------------------------------------------------------------

pub fn subscription_or_stale(
    cells: &[GridCell],
    metric: &'static str,
    latest_quarter: u32,
) -> MetricSubRow {
    match subscription_rows(cells, metric) {
        Some(r) => {
            let has_latest = cells.iter().any(|c| c.metric == metric && c.quarter == latest_quarter);
            if has_latest {
                r
            } else {
                MetricSubRow { metric, value: 0, trend_label: "数据待修：最新季缺失" }
            }
        }
        None => MetricSubRow { metric, value: 0, trend_label: "无数据" },
    }
}

// ---------------------------------------------------------------------------
// 报告页区块装配：指标卡/故事/预告三段齐全性门（缺段不发布——复用
// F149 判据的页面侧）
// ---------------------------------------------------------------------------

pub struct ReportPage {
    pub metric_cards: usize,
    pub stories: usize,
    pub has_preview: bool,
}

pub fn page_complete(p: &ReportPage) -> Result<(), &'static str> {
    if p.metric_cards < 4 {
        return Err("指标卡不足四张（四指标判据）");
    }
    if p.stories == 0 {
        return Err("故事段为空：报告不是表格堆");
    }
    if !p.has_preview {
        return Err("缺预告段：下季预告是承诺");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 目标核对行（g 层 Goal → 页面行）
// ---------------------------------------------------------------------------

pub fn goal_rows(goals: &[Goal]) -> alloc::vec::Vec<(&'static str, &'static str)> {
    goals
        .iter()
        .map(|g| {
            let label = if g.actual >= g.target { "达标" } else { "未达标" };
            (g.metric, label)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F149H_TAG: &str = "stareco-F149-deep5";

pub fn run_f149_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F149H_TAG);

    let cells = [
        GridCell { metric: "themes", quarter: 1, value: 100 },
        GridCell { metric: "themes", quarter: 2, value: 130 },
        GridCell { metric: "packs", quarter: 1, value: 40 },
    ];

    // 订阅行
    let sub = subscription_rows(&cells, "themes").expect("ok");
    set.add(
        "f149h sub",
        sub.value == 130 && sub.trend_label == "明显增长",
        "最新值+趋势人话",
    );
    let flat = subscription_rows(&[GridCell { metric: "m", quarter: 1, value: 50 }, GridCell { metric: "m", quarter: 2, value: 50 }], "m").expect("ok");
    set.add("f149h flat", flat.trend_label == "持平", "持平档");
    set.add("f149h no data", subscription_rows(&cells, "ghost").is_none(), "无数据 None");

    // 缺季诚实
    let stale = subscription_or_stale(&cells, "themes", 3);
    set.add(
        "f149h stale",
        stale.trend_label.contains("待修") && stale.value == 0,
        "缺最新季不冒充",
    );
    let fresh = subscription_or_stale(&cells, "themes", 2);
    set.add("f149h fresh", fresh.value == 130, "有最新季直取");

    // 页面区块门
    set.add(
        "f149h page ok",
        page_complete(&ReportPage { metric_cards: 4, stories: 2, has_preview: true }).is_ok(),
        "三段齐通过",
    );
    let no_stories = page_complete(&ReportPage { metric_cards: 4, stories: 0, has_preview: true });
    set.add("f149h page no stories", no_stories.is_err(), "空故事段拒绝");
    let few_cards = page_complete(&ReportPage { metric_cards: 3, stories: 1, has_preview: true });
    set.add("f149h page few cards", few_cards.is_err(), "指标卡不足拒绝");

    // 目标行
    let rows = goal_rows(&[
        Goal { metric: "themes", target: 100, actual: 120 },
        Goal { metric: "packs", target: 100, actual: 50 },
    ]);
    set.add(
        "f149h goal rows",
        rows == alloc::vec![("themes", "达标"), ("packs", "未达标")],
        "目标行诚实措辞",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn single_quarter_trend() {
        let sub = subscription_rows(&[GridCell { metric: "m", quarter: 1, value: 7 }], "m").unwrap();
        assert_eq!(sub.trend_label, "首期数据");
        assert_eq!(sub.value, 7);
    }

    #[test]
    fn decline_label() {
        let sub = subscription_rows(
            &[
                GridCell { metric: "m", quarter: 1, value: 100 },
                GridCell { metric: "m", quarter: 2, value: 60 },
            ],
            "m",
        )
        .unwrap();
        assert_eq!(sub.trend_label, "明显下滑");
    }
}
