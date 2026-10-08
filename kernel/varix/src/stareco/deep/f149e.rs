//! 深化层二 · F149 季度生态报告（2026-09-26 深化批次二）。
//!
//! 补深主册【交互设计】趋势图与 PDF 导出 +【设计细节】JSON 同步面
//! 与归因（主册 G-D-24）：四指标环比引擎、8 季趋势斜率、PDF 分页
//! 结构模型、JSON round-trip（F128 开放数据版）、下滑归因强制字段。

use crate::checks::CheckSet;
use crate::stareco::ebase::fnv1a64;
use crate::stareco::ecoreport::{Metrics, QuarterlyReport, TREND_SEASONS};

// ---------------------------------------------------------------------------
// 四指标环比引擎：本季 vs 上季（升/平/降三态 + 变化量）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delta {
    Up(u32),
    Flat,
    Down(u32),
}

/// 单指标环比（不出现负数——Down 携带下降量）。
pub fn delta_of(prev: u32, cur: u32) -> Delta {
    match cur.cmp(&prev) {
        core::cmp::Ordering::Greater => Delta::Up(cur - prev),
        core::cmp::Ordering::Equal => Delta::Flat,
        core::cmp::Ordering::Less => Delta::Down(prev - cur),
    }
}

/// 四指标环比卡（季报页的「变化」列数据源）。
pub struct DeltaCard {
    pub compat: Delta,
    pub community: Delta,
    pub upstream: Delta,
    pub badges: Delta,
}

pub fn delta_card(prev: &Metrics, cur: &Metrics) -> DeltaCard {
    DeltaCard {
        compat: delta_of(prev.compat_count, cur.compat_count),
        community: delta_of(prev.community_submissions, cur.community_submissions),
        upstream: delta_of(prev.upstream_changes, cur.upstream_changes),
        badges: delta_of(prev.badges_granted, cur.badges_granted),
    }
}

// ---------------------------------------------------------------------------
// 8 季趋势斜率：窗口序列 → 方向判定（回退红标的输入）
// ---------------------------------------------------------------------------

/// 斜率三态：尾部两点决定方向（8 季窗口的最近变化）。
pub fn trend_direction(series: &[u32]) -> &'static str {
    if series.len() < 2 {
        return "数据不足";
    }
    let (a, b) = (series[series.len() - 2], series[series.len() - 1]);
    if b > a {
        "上升"
    } else if b < a {
        "下降"
    } else {
        "持平"
    }
}

/// 趋势窗口截取：不足 8 季给全量、超出取最近 8 季（窗口纪律）。
pub fn trend_window(series: &[u32]) -> alloc::vec::Vec<u32> {
    if series.len() <= TREND_SEASONS {
        series.to_vec()
    } else {
        series[series.len() - TREND_SEASONS..].to_vec()
    }
}

// ---------------------------------------------------------------------------
// PDF 分页结构模型：封面/指标/故事/预告四节（F025 打印面输入）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PdfSection {
    Cover,
    Metrics,
    Stories,
    Forecast,
}

/// 分页规划：固定四节顺序 + 页码分配（封面 1 页 / 指标 1 / 故事按 2 条 1 页 / 预告 1）。
pub fn paginate(stories: usize) -> alloc::vec::Vec<(PdfSection, u8)> {
    let story_pages = stories.div_ceil(2).max(1) as u8;
    let mut out = alloc::vec![(PdfSection::Cover, 1), (PdfSection::Metrics, 2)];
    for p in 0..story_pages {
        out.push((PdfSection::Stories, 3 + p));
    }
    out.push((PdfSection::Forecast, 3 + story_pages));
    out
}

// ---------------------------------------------------------------------------
// JSON round-trip（F128 开放数据版）
// ---------------------------------------------------------------------------

/// 序列化为一行 JSON 形态（键序固定——指纹确定性）。
pub fn report_json(r: &QuarterlyReport) -> alloc::string::String {
    alloc::format!(
        "{{\"season\":{},\"published_on\":{},\"compat\":{},\"community\":{},\"upstream\":{},\"badges\":{},\"stories\":{},\"forecast_fp\":{},\"data_pending\":{}}}",
        r.season,
        r.published_on,
        r.metrics.compat_count,
        r.metrics.community_submissions,
        r.metrics.upstream_changes,
        r.metrics.badges_granted,
        r.story_count,
        fnv1a64(r.forecast.as_bytes()),
        r.data_pending as u8
    )
}

/// JSON 指纹：同报告同指纹（确定性——不允许哈希表序抖动）。
pub fn report_json_fp(r: &QuarterlyReport) -> u64 {
    fnv1a64(report_json(r).as_bytes())
}

// ---------------------------------------------------------------------------
// 下滑归因强制：任何下降指标必须带归因（不挑好看的说）
// ---------------------------------------------------------------------------

pub struct DeclineAttribution {
    /// 下降的指标名。
    pub metric: &'static str,
    pub reason: &'static str,
}

/// 归因完整性检查：四指标逐一核对，下滑者必须有归因条目。
pub fn attributions_complete(
    prev: &Metrics,
    cur: &Metrics,
    attributions: &[DeclineAttribution],
) -> bool {
    let mut declining: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    if cur.compat_count < prev.compat_count {
        declining.push("compat");
    }
    if cur.community_submissions < prev.community_submissions {
        declining.push("community");
    }
    if cur.upstream_changes < prev.upstream_changes {
        declining.push("upstream");
    }
    if cur.badges_granted < prev.badges_granted {
        declining.push("badges");
    }
    declining.iter().all(|m| attributions.iter().any(|a| a.metric == *m && !a.reason.is_empty()))
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F149E_TAG: &str = "stareco-F149-deep2";

fn mk_report(season: u32, published_on: u32, m: Metrics) -> QuarterlyReport {
    QuarterlyReport {
        season,
        published_on,
        metrics: m,
        story_refs: [1, 2, 3],
        story_count: 2,
        forecast: "下季重点：镜像源扩容",
        data_pending: false,
    }
}

pub fn run_f149_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F149E_TAG);

    let m1 = Metrics { compat_count: 40, community_submissions: 10, upstream_changes: 6, badges_granted: 3 };
    let m2 = Metrics { compat_count: 44, community_submissions: 8, upstream_changes: 6, badges_granted: 5 };
    let card = delta_card(&m1, &m2);
    set.add(
        "f149e deltas",
        card.compat == Delta::Up(4) && card.community == Delta::Down(2) && card.upstream == Delta::Flat && card.badges == Delta::Up(2),
        "四指标环比三态",
    );
    set.add("f149e delta down zero", delta_of(0, 5) == Delta::Up(5), "从零上升");
    set.add("f149e delta eq", delta_of(3, 3) == Delta::Flat, "持平");

    // 趋势
    let series = [10u32, 12, 11, 15, 14, 16, 18, 17, 20, 21];
    set.add("f149e trend up", trend_direction(&series) == "上升", "尾部上升");
    set.add(
        "f149e trend window",
        trend_window(&series).len() == TREND_SEASONS && trend_window(&series)[0] == 13,
        "取最近 8 季",
    );
    set.add("f149e trend short", trend_direction(&[5u32]) == "数据不足", "单点不判方向");

    // 分页
    let pages = paginate(3);
    set.add("f149e pages count", pages.len() == 5, "封面+指标+2 故事页+预告");
    set.add(
        "f149e pages order",
        pages.first() == Some(&(PdfSection::Cover, 1)) && pages.last() == Some(&(PdfSection::Forecast, 5)),
        "首尾节序正确",
    );
    set.add("f149e pages ceil", paginate(5).len() == 6, "5 条故事 3 页（进位）");

    // JSON round-trip 与指纹
    let r1 = mk_report(3, 5, m1);
    let r2 = mk_report(3, 5, m1);
    let json = report_json(&r1);
    set.add(
        "f149e json keys",
        json.contains("\"season\":3") && json.contains("\"badges\":3"),
        "键值逐项",
    );
    set.add("f149e json fp stable", report_json_fp(&r1) == report_json_fp(&r2), "同报告同指纹");
    let mut r3 = mk_report(4, 5, m1);
    r3.season = 4;
    set.add("f149e json fp differs", report_json_fp(&r1) != report_json_fp(&r3), "异报告异指纹");

    // 归因
    let attribs = alloc::vec![DeclineAttribution { metric: "community", reason: "提交通道迁移期" }];
    set.add(
        "f149e attribution ok",
        attributions_complete(&m1, &m2, &attribs),
        "唯一下滑项已归因",
    );
    let none = alloc::vec::Vec::new();
    set.add(
        "f149e attribution missing",
        !attributions_complete(&m2, &m1, &none),
        "三处下滑零归因红",
    );

    // 与基础层联动：归档与双季判据
    let mut ledger = crate::stareco::ecoreport::EcoReportLedger::new();
    let _ = ledger.archive(mk_report(1, 3, m1));
    let _ = ledger.archive(mk_report(2, 4, m2));
    set.add("f149e two seasons", ledger.two_consecutive_on_time(), "连续两季按期（对账）");
    set.add("f149e trend api", ledger.trend(|m| m.compat_count).len() == 2, "趋势接口可用");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn delta_zero_base() {
        assert_eq!(delta_of(7, 0), Delta::Down(7));
        assert_eq!(delta_of(0, 0), Delta::Flat);
    }

    #[test]
    fn paginate_one_story() {
        let p = paginate(1);
        assert_eq!(p.len(), 4);
        assert_eq!(p[2], (PdfSection::Stories, 3));
    }

    #[test]
    fn trend_window_exact() {
        let s: alloc::vec::Vec<u32> = (1..=8).collect();
        let w = trend_window(&s);
        assert_eq!(w.len(), 8);
        assert_eq!(w[0], 1); // 恰好 8 季 = 全量
    }
}
