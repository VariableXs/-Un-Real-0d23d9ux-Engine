//! 深化层 · F149 季度生态报告（2026-09-26 回炉补深化）。
//!
//! 补深：指标健康检查（数据源异常 → 「数据待修」判定核）、JSON 导出
//! schema 校验、F150 第五指标接入（外部人耗时曲线）、报告导出渲染。

use crate::checks::CheckSet;
use crate::stareco::ecoreport::{report_fp, Metrics, QuarterlyReport, EcoReportLedger, TREND_SEASONS};

// ---------------------------------------------------------------------------
// 指标健康检查（数据源异常判定核）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SourceHealthProbe {
    /// 源账本最后成功同步日（0 = 从未同步）。
    pub last_sync_day: u32,
    /// 源账本指纹是否校验通过。
    pub ledger_intact: bool,
}

/// 「数据待修」判定：从未同步 / 账本断链 / 同步滞后三季以上。
pub fn metric_pending(probe: &SourceHealthProbe, current_season: u32, synced_season: u32) -> bool {
    probe.last_sync_day == 0 || !probe.ledger_intact || current_season.saturating_sub(synced_season) > 3
}

// ---------------------------------------------------------------------------
// JSON 导出 schema（F128 开放数据面）
// ---------------------------------------------------------------------------

/// 报告 JSON 键集（固定 schema——多一个少一个都是 schema 破坏）。
pub const JSON_KEYS: [&str; 7] = [
    "season",
    "published_on",
    "compat_count",
    "community_submissions",
    "upstream_changes",
    "badges_granted",
    "forecast",
];

pub fn json_schema_ok(present: &[&str]) -> bool {
    present.len() == JSON_KEYS.len() && JSON_KEYS.iter().all(|k| present.contains(k))
}

// ---------------------------------------------------------------------------
// F150 第五指标接入
// ---------------------------------------------------------------------------

/// 外部人全流程耗时曲线（F150 逐季应下降）——季报第五指标数据源。
pub struct FifthMetric {
    pub seasons: [Option<u32>; TREND_SEASONS], // 各季最佳外部人耗时（分钟）
    pub count: usize,
}

impl FifthMetric {
    pub fn push(&mut self, minutes: u32) -> Result<(), &'static str> {
        if self.count >= TREND_SEASONS {
            return Err("第五指标窗口满（8 季滚动）");
        }
        self.seasons[self.count] = Some(minutes);
        self.count += 1;
        Ok(())
    }

    /// 趋势判定：最近两季同口径比较，不升 = 达标（持平或下降皆可）。
    pub fn not_worsening(&self) -> Option<bool> {
        if self.count < 2 {
            return None;
        }
        let cur = self.seasons[self.count - 1];
        let prev = self.seasons[self.count - 2];
        match (cur, prev) {
            (Some(c), Some(p)) => Some(c <= p),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 报告导出渲染
// ---------------------------------------------------------------------------

/// 渲染报告头三行（季度 / 发布日 / 指纹）——导出面的最小呈现。
pub fn render_header(r: &QuarterlyReport) -> alloc::vec::Vec<alloc::string::String> {
    alloc::vec![
        alloc::format!("季度 S{}", r.season),
        alloc::format!("发布日 季内第 {} 天", r.published_on),
        alloc::format!("指纹 {:016X}", report_fp(r)),
    ]
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F149D_TAG: &str = "stareco-F149-deep";

pub fn run_f149_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F149D_TAG);

    // 指标健康
    set.add(
        "f149d pending conditions",
        metric_pending(&SourceHealthProbe { last_sync_day: 0, ledger_intact: true }, 5, 5)
            && metric_pending(&SourceHealthProbe { last_sync_day: 99, ledger_intact: false }, 5, 5)
            && metric_pending(&SourceHealthProbe { last_sync_day: 99, ledger_intact: true }, 5, 1)
            && !metric_pending(&SourceHealthProbe { last_sync_day: 99, ledger_intact: true }, 5, 4),
        "三种待修 + 一种健康",
    );

    // JSON schema
    set.add(
        "f149d json schema exact",
        json_schema_ok(&JSON_KEYS) && !json_schema_ok(&JSON_KEYS[..6]) && !json_schema_ok(&["season", "extra"]),
        "键集固定",
    );

    // 第五指标
    let mut m = FifthMetric { seasons: [None; TREND_SEASONS], count: 0 };
    m.push(24).ok();
    m.push(22).ok();
    set.add(
        "f149d fifth metric improving",
        m.not_worsening() == Some(true),
        "24→22 下降",
    );
    m.push(25).ok();
    set.add(
        "f149d fifth metric worsening flagged",
        m.not_worsening() == Some(false),
        "回升如实标红",
    );
    let mut thin = FifthMetric { seasons: [None; TREND_SEASONS], count: 0 };
    thin.push(30).ok();
    set.add("f149d thin series honest none", thin.not_worsening().is_none(), "不足两季不可比");

    // 导出渲染
    let r = QuarterlyReport {
        season: 2,
        published_on: 3,
        metrics: Metrics { compat_count: 62, community_submissions: 18, upstream_changes: 5, badges_granted: 5 },
        story_refs: [201, 202, 0],
        story_count: 2,
        forecast: "下季：徽标复检窗开启",
        data_pending: false,
    };
    let header = render_header(&r);
    set.add(
        "f149d header render",
        header.len() == 3 && header[0] == "季度 S2" && header[2].contains("指纹"),
        "三行头",
    );

    // 归档联动（深化层复诵：模板三段门禁）
    let mut led = EcoReportLedger::new();
    led.archive(QuarterlyReport { ..r }).ok();
    set.add("f149d archive reuse", led.len() == 1, "归档引擎同源");
    set.add("f149d trend window 8", TREND_SEASONS == 8, "8 季滚动");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn fifth_metric_bounds() {
        let mut m = FifthMetric { seasons: [None; TREND_SEASONS], count: 0 };
        for _ in 0..TREND_SEASONS {
            m.push(20).ok();
        }
        assert!(m.push(20).is_err());
    }
}
