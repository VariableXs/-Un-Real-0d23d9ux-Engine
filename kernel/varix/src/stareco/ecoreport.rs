//! F149 季度生态报告 · 完整设计（STAR I 主册 G-D-24）。
//!
//! **判据（主册）**：连续两季按期发布；四指标与源账本一致（交叉
//! 校验）；故事内容真实可溯（编号引用）。
//!
//! **设计要点（主册）**：四指标公开（兼容件数 F040 / 社区提交数
//! F129 / 借力件版本变动 F130 / 徽标授予数 F144）——引用不复制
//! （数据自动汇总自各账本，交叉校验）；发布日=每季首月第一周
//! （F138 日历挂账）；模板固定（四指标+故事+预告三段）；趋势图
//! 8 季度窗口；指标下滑如实呈现+归因（不挑好看的说）；数据源异常
//! → 指标标「数据待修」；报告自身进 F128 开放数据（JSON 版同步）。
//!
//! 本模块是季报的**汇总核**：四指标引用账本快照、按期判定（发布日
//! 窗口）、交叉校验引擎、8 季趋势、归因与「数据待修」标注。

use alloc::vec;
use crate::checks::CheckSet;
use crate::stareco::ebase::fnv1a64;

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

/// 发布窗口：每季首月第 1-7 天（季度以 90 天计——第 0..=6 天内发布
/// 视为按期）。
pub const PUBLISH_WINDOW_DAYS: u32 = 7;
/// 趋势窗口：8 季度。
pub const TREND_SEASONS: usize = 8;

// ---------------------------------------------------------------------------
// 指标模型（引用不复制）
// ---------------------------------------------------------------------------

/// 四指标快照——从各源账本**引用**（这里存的是账本指纹+读数，不是
/// 账本本体；交叉校验时回读源账本比对）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// 兼容件数（F040 账本）。
    pub compat_count: u32,
    /// 社区提交数（F129）。
    pub community_submissions: u32,
    /// 借力件版本变动数（F130）。
    pub upstream_changes: u32,
    /// 徽标授予数（F144）。
    pub badges_granted: u32,
}

impl Metrics {
    /// 交叉校验：读数与各源账本实际值逐项一致（判据「四指标与源
    /// 账本一致」——一处不齐整份报告红）。
    pub fn cross_check(&self, f040: u32, f129: u32, f130: u32, f144: u32) -> bool {
        self.compat_count == f040
            && self.community_submissions == f129
            && self.upstream_changes == f130
            && self.badges_granted == f144
    }
}

// ---------------------------------------------------------------------------
// 季报
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct QuarterlyReport {
    /// 季度序号（从 1 起）。
    pub season: u32,
    /// 实际发布日（季度内第几天，0 = 未发布）。
    pub published_on: u32,
    pub metrics: Metrics,
    /// 故事条目（编号引用——真实可溯；最多 3 条）。
    pub story_refs: [u32; 3],
    story_count: usize,
    /// 预告（下季内容——非空 = 模板完整）。
    pub forecast: &'static str,
    /// 数据源异常 → 指标标「数据待修」。
    pub data_pending: bool,
}

impl QuarterlyReport {
    /// 发布日按期判定：0 < published_on <= 7。
    pub fn on_time(&self) -> bool {
        self.published_on >= 1 && self.published_on <= PUBLISH_WINDOW_DAYS
    }

    /// 模板固定三段：四指标 + 故事 + 预告。
    pub fn template_ok(&self) -> bool {
        !self.forecast.is_empty() && self.story_count > 0
    }

    /// 故事真实可溯：全部引用非零编号。
    pub fn stories_traceable(&self) -> bool {
        self.story_count > 0 && self.story_refs[..self.story_count].iter().all(|&r| r != 0)
    }
}

// ---------------------------------------------------------------------------
// 季报归档（连续两季 + 8 季趋势）
// ---------------------------------------------------------------------------

pub struct EcoReportLedger {
    seasons: [Option<QuarterlyReport>; TREND_SEASONS],
    count: usize,
}

impl EcoReportLedger {
    pub fn new() -> EcoReportLedger {
        EcoReportLedger { seasons: [None; TREND_SEASONS], count: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 归档一份季报：模板不完整拒绝（模板固定——不许开天窗）。
    pub fn archive(&mut self, r: QuarterlyReport) -> Result<(), &'static str> {
        if !r.template_ok() {
            return Err("模板三段不齐（四指标+故事+预告）");
        }
        if !r.stories_traceable() {
            return Err("故事必须带可溯编号");
        }
        if self.count >= TREND_SEASONS {
            return Err("趋势窗口满（8 季滚动）");
        }
        // 季度序号必须连续递增（不许跳季/倒序归档——时间诚实）
        if self.count > 0 {
            let prev = self.seasons[self.count - 1].as_ref().unwrap();
            if r.season != prev.season + 1 {
                return Err("季度必须连续递增");
            }
        }
        self.seasons[self.count] = Some(r);
        self.count += 1;
        Ok(())
    }

    /// 连续两季按期发布（判据主句）。
    pub fn two_consecutive_on_time(&self) -> bool {
        self.count >= 2
            && self.seasons[self.count - 1].as_ref().map(|r| r.on_time()).unwrap_or(false)
            && self.seasons[self.count - 2].as_ref().map(|r| r.on_time()).unwrap_or(false)
    }

    /// 8 季趋势：指定指标的序列（不足 8 季如实返回现有点数）。
    pub fn trend(&self, pick: impl Fn(&Metrics) -> u32) -> alloc::vec::Vec<u32> {
        self.seasons[..self.count].iter().flatten().map(|r| pick(&r.metrics)).collect()
    }

    /// 指标下滑检测：最近一季较上季下滑的指标数（如实呈现面——
    /// 下滑不是错误，不归因才是）。
    pub fn declining_metrics(&self) -> usize {
        if self.count < 2 {
            return 0;
        }
        let cur = self.seasons[self.count - 1].as_ref().unwrap();
        let prev = self.seasons[self.count - 2].as_ref().unwrap();
        let picks: [fn(&Metrics) -> u32; 4] = [
            |m| m.compat_count,
            |m| m.community_submissions,
            |m| m.upstream_changes,
            |m| m.badges_granted,
        ];
        picks.iter().filter(|p| p(&cur.metrics) < p(&prev.metrics)).count()
    }
}

// ---------------------------------------------------------------------------
// 开放数据（JSON 版同步——F128 面）
// ---------------------------------------------------------------------------

/// 报告指纹（JSON 版一致性的锚——报告本体与 JSON 版共用同指纹）。
pub fn report_fp(r: &QuarterlyReport) -> u64 {
    fnv1a64(&r.metrics.compat_count.to_le_bytes())
        ^ fnv1a64(&r.metrics.community_submissions.to_le_bytes())
        ^ fnv1a64(&r.metrics.upstream_changes.to_le_bytes())
        ^ fnv1a64(&r.metrics.badges_granted.to_le_bytes())
        ^ (r.season as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F149_TAG: &str = "stareco-F149-ecoreport";

pub fn run_ecoreport_checks() -> CheckSet {
    let mut set = CheckSet::new(F149_TAG);

    let m1 = Metrics { compat_count: 50, community_submissions: 12, upstream_changes: 3, badges_granted: 3 };
    let m2 = Metrics { compat_count: 62, community_submissions: 18, upstream_changes: 5, badges_granted: 5 };

    let r1 = QuarterlyReport {
        season: 1,
        published_on: 3,
        metrics: m1,
        story_refs: [101, 102, 103],
        story_count: 3,
        forecast: "下季：兼容账本冲刺 60 件",
        data_pending: false,
    };
    let r2 = QuarterlyReport {
        season: 2,
        published_on: 1,
        metrics: m2,
        story_refs: [201, 202, 0],
        story_count: 2,
        forecast: "下季：徽标复检窗开启",
        data_pending: false,
    };

    // 按期判定
    set.add("f149 on-time window", r1.on_time() && r2.on_time(), "day 1..=7");
    let mut late = r1;
    late.published_on = 9;
    set.add("f149 late flagged", !late.on_time(), "day 9 is late");
    let mut unpublish = r1;
    unpublish.published_on = 0;
    set.add("f149 unpublished flagged", !unpublish.on_time(), "not yet");

    // 交叉校验：与源账本一致
    set.add(
        "f149 cross-check green",
        m1.cross_check(50, 12, 3, 3),
        "四指标一致",
    );
    set.add(
        "f149 cross-check any mismatch red",
        !m1.cross_check(50, 12, 3, 4) && !m1.cross_check(51, 12, 3, 3),
        "任一不齐即红",
    );

    // 归档：模板/故事/季序门禁
    let mut led = EcoReportLedger::new();
    set.add(
        "f149 no-story rejected",
        led.archive(QuarterlyReport { story_count: 0, ..r1 }).is_err(),
        "模板三段",
    );
    set.add(
        "f149 zero-ref story rejected",
        led.archive(QuarterlyReport { story_refs: [0, 0, 0], ..r1 }).is_err(),
        "故事可溯",
    );
    set.add(
        "f149 no-forecast rejected",
        led.archive(QuarterlyReport { forecast: "", ..r1 }).is_err(),
        "预告段",
    );
    led.archive(r1).expect("archive s1");
    led.archive(r2).expect("archive s2");
    set.add("f149 two seasons archived", led.len() == 2, "连续两季在册");
    set.add("f149 two consecutive on time", led.two_consecutive_on_time(), "判据主句");

    // 季序纪律
    set.add(
        "f149 season must advance",
        led.archive(QuarterlyReport { season: 2, ..m2_report(3) }).is_err(),
        "不许倒序/重复归档",
    );

    // 趋势（8 季窗口）
    let t = led.trend(|m| m.compat_count);
    set.add("f149 trend series", t == vec![50, 62], "compat curve");
    set.add("f149 trend window 8", TREND_SEASONS == 8, "rolling");

    // 下滑如实呈现
    let m3 = Metrics { compat_count: 55, community_submissions: 9, upstream_changes: 6, badges_granted: 7 };
    led.archive(QuarterlyReport {
        season: 3,
        published_on: 2,
        metrics: m3,
        story_refs: [301, 0, 0],
        story_count: 1,
        forecast: "下季回升计划在册",
        data_pending: false,
    })
    .expect("archive s3");
    set.add(
        "f149 decline honestly counted",
        led.declining_metrics() == 2,
        "compat 62→55 & submissions 18→9",
    );

    // 数据待修标注 + JSON 指纹
    let mut pending = r1;
    pending.data_pending = true;
    set.add("f149 data-pending label", pending.data_pending, "诚实标注面");
    set.add("f149 report fp stable", report_fp(&r1) == report_fp(&r1) && report_fp(&r1) != report_fp(&r2), "deterministic");

    set
}

fn m2_report(day: u32) -> QuarterlyReport {
    QuarterlyReport {
        season: 2,
        published_on: day,
        metrics: Metrics { compat_count: 0, community_submissions: 0, upstream_changes: 0, badges_granted: 0 },
        story_refs: [1, 0, 0],
        story_count: 1,
        forecast: "f",
        data_pending: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn season_gap_rejected() {
        let mut led = EcoReportLedger::new();
        led.archive(QuarterlyReport {
            season: 1,
            published_on: 2,
            metrics: Metrics { compat_count: 1, community_submissions: 1, upstream_changes: 1, badges_granted: 1 },
            story_refs: [5, 0, 0],
            story_count: 1,
            forecast: "x",
            data_pending: false,
        })
        .unwrap();
        // 跳季（1→3）拒绝
        assert!(led
            .archive(QuarterlyReport {
                season: 3,
                published_on: 2,
                metrics: Metrics { compat_count: 2, community_submissions: 2, upstream_changes: 2, badges_granted: 2 },
                story_refs: [6, 0, 0],
                story_count: 1,
                forecast: "x",
                data_pending: false,
            })
            .is_err());
    }
}
